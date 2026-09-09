mod commands;
mod db;
mod domain;
mod email_rules;
mod error;
mod export;
mod mail;
mod notification;
mod repository;
mod scheduler;
mod startup;
mod timeline;

use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use db::{Database, apply_pending_restore};
use domain::{Clock, SystemClock};
use error::{AppError, AppResult};
use mail::MailService;
use notification::NotificationService;
use scheduler::SchedulerHandle;
use startup::{
    StartupMode, StartupState, assess_database_startup, blocked_status, startup_state,
    valid_recovery_candidates,
};
use tauri::{
    AppHandle, Emitter, Manager, RunEvent, WebviewWindowBuilder,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

pub struct AppState {
    database: Arc<Database>,
    clock: Arc<dyn Clock>,
    notifications: Arc<NotificationService>,
    mail: Arc<MailService>,
    portable: bool,
    scheduler: SchedulerHandle,
    shortcut_warning: Mutex<Option<String>>,
    pending_reminder_menu: Mutex<Option<MenuItem<tauri::Wry>>>,
    update_menu: Mutex<Option<MenuItem<tauri::Wry>>>,
}

pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            let _ = show_main_window(app);
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .arg("--background")
                .build(),
        )
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        let _ = show_capture_window(app);
                    }
                })
                .build(),
        )
        .setup(|app| {
            let notification_test_requested =
                std::env::args().any(|argument| argument == "--test-notification");
            let portable = portable_mode()?;
            let database_path = resolve_database_path(app.handle())?;
            let candidate_paths = candidate_database_paths(app.handle(), &database_path);

            if let Err(error) = apply_pending_restore(&database_path) {
                let recovery_candidates =
                    valid_recovery_candidates(&database_path, &candidate_paths);
                let metadata = fs::metadata(&database_path).ok();
                let status = blocked_status(
                    &database_path,
                    recovery_candidates,
                    None,
                    &error,
                    metadata.is_some(),
                    metadata.map(|value| value.len()).unwrap_or(0),
                );
                app.manage(startup_state(
                    status,
                    database_path.clone(),
                    candidate_paths,
                ));
                build_configured_windows(app, true)?;
                return Ok(());
            }

            let startup_status = assess_database_startup(&database_path, &candidate_paths);
            if startup_status.mode != StartupMode::Ready {
                app.manage(startup_state(
                    startup_status,
                    database_path.clone(),
                    candidate_paths,
                ));
                build_configured_windows(app, true)?;
                return Ok(());
            }

            let preflight_schema = startup_status
                .current
                .as_ref()
                .map(|summary| summary.schema_version);
            let database = match Database::open_preflighted(&database_path, preflight_schema) {
                Ok(database) => Arc::new(database),
                Err(error) => {
                    let candidate_paths = candidate_database_paths(app.handle(), &database_path);
                    let recovery_candidates =
                        valid_recovery_candidates(&database_path, &candidate_paths);
                    let current = startup_status.current;
                    let metadata = fs::metadata(&database_path).ok();
                    let status = blocked_status(
                        &database_path,
                        recovery_candidates,
                        current,
                        &error,
                        metadata.is_some(),
                        metadata.map(|value| value.len()).unwrap_or(0),
                    );
                    app.manage(startup_state(
                        status,
                        database_path.clone(),
                        candidate_paths,
                    ));
                    build_configured_windows(app, true)?;
                    return Ok(());
                }
            };
            app.manage(StartupState::ready(database_path.clone()));
            let settings = database.get_settings()?;
            let shortcut_warning = app
                .global_shortcut()
                .register(settings.global_shortcut.as_str())
                .err()
                .map(|error| format!("全局快捷键未生效，请在后台设置中换一个组合键：{error}"));

            let clock: Arc<dyn Clock> = Arc::new(SystemClock);
            let notifications = Arc::new(NotificationService::new(
                app.handle().clone(),
                portable,
                database_path
                    .parent()
                    .ok_or_else(|| AppError::SystemIntegration("数据目录无效".into()))?
                    .to_path_buf(),
            )?);
            let mail = Arc::new(MailService::new());
            if notification_test_requested {
                if !settings.notifications_enabled {
                    return Err(AppError::SystemIntegration(
                        "通知总开关已关闭，请先在后台设置中启用".into(),
                    )
                    .into());
                }
                let status = notifications.status();
                if !status.can_notify {
                    return Err(AppError::SystemIntegration(status.message).into());
                }
                commands::insert_notification_test_item(&database, clock.now_utc())?;
            }
            let scheduler = SchedulerHandle::start(
                app.handle().clone(),
                Arc::clone(&database),
                Arc::clone(&clock),
                Arc::clone(&notifications),
                Arc::clone(&mail),
            );
            if notification_test_requested {
                scheduler.wake_after(Duration::from_secs(11));
            }
            app.manage(AppState {
                database,
                clock,
                notifications,
                mail,
                portable,
                scheduler,
                shortcut_warning: Mutex::new(shortcut_warning),
                pending_reminder_menu: Mutex::new(None),
                update_menu: Mutex::new(None),
            });

            build_configured_windows(app, false)?;

            let (pending_menu, update_menu) = create_tray(app.handle())?;
            if let Some(state) = app.try_state::<AppState>() {
                *state
                    .pending_reminder_menu
                    .lock()
                    .expect("tray menu mutex poisoned") = Some(pending_menu);
                *state.update_menu.lock().expect("tray menu mutex poisoned") = Some(update_menu);
                update_tray_reminder_count(app.handle(), &state.database, state.clock.now_utc());
            }

            if std::env::args().any(|argument| argument == "--background") {
                if let Some(main) = app.get_webview_window("main") {
                    main.hide()?;
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.app_handle().try_state::<AppState>().is_some() {
                    api.prevent_close();
                    if window.hide().is_ok() && window.label() == "reminder" {
                        let _ = window
                            .app_handle()
                            .emit_to("reminder", "overlay_hidden", ());
                    }
                } else {
                    window.app_handle().exit(0);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_items,
            commands::get_app_info,
            commands::create_item,
            commands::set_item_completed,
            commands::classify_item_as_ordinary,
            commands::complete_series_occurrence,
            commands::set_reminder_paused,
            commands::reschedule_item,
            commands::get_settings,
            commands::update_settings,
            commands::get_update_state,
            commands::set_auto_update_enabled,
            commands::dismiss_update_permission,
            commands::record_update_check,
            commands::snooze_update,
            commands::mark_update_notified,
            commands::mark_release_notes_seen,
            commands::notify_update_available,
            commands::set_tray_update,
            commands::list_categories,
            commands::create_category,
            commands::update_category,
            commands::delete_category,
            commands::move_category,
            commands::list_tags,
            commands::create_tag,
            commands::update_tag,
            commands::delete_tag,
            commands::move_tag,
            commands::update_item,
            commands::set_item_deleted,
            commands::permanently_delete_item,
            commands::list_pending_reminders,
            commands::acknowledge_reminder,
            commands::snooze_item,
            commands::list_email_route_tag_ids,
            commands::set_tag_email_route,
            commands::export_excel,
            commands::load_draft,
            commands::save_draft,
            commands::hide_capture,
            commands::show_capture,
            commands::show_main,
            commands::hide_reminder,
            commands::get_system_warning,
            commands::get_autostart_status,
            commands::get_onboarding_status,
            commands::complete_onboarding,
            commands::get_notification_status,
            commands::get_smtp_status,
            commands::test_smtp,
            commands::list_email_delivery_rules,
            commands::save_email_delivery_rule,
            commands::delete_email_delivery_rule,
            commands::register_portable_notifications,
            commands::unregister_portable_notifications,
            commands::create_notification_test_item,
            commands::test_reminder_mode,
            commands::get_data_status,
            commands::create_data_backup,
            commands::open_data_directory,
            commands::restore_database,
            commands::get_startup_status,
            commands::restore_startup_database,
            commands::open_startup_data_directory,
            commands::retry_startup,
            commands::get_repository_status,
            commands::configure_data_repository,
            commands::disable_data_repository,
            commands::preview_repository_merge,
            commands::publish_data_repository_snapshot,
            commands::sync_data_repository,
            commands::open_repository_directory,
            commands::open_release_page,
            commands::get_timeline,
            commands::save_timeline_view,
        ]);

    let app = builder
        .build(tauri::generate_context!())
        .expect("failed to build Shanji application");

    app.run(|app, event| {
        if let RunEvent::Exit = event {
            if let Some(state) = app.try_state::<AppState>() {
                state.scheduler.stop();
            }
        }
    });
}

fn resolve_database_path(app: &AppHandle) -> AppResult<PathBuf> {
    let executable = std::env::current_exe()?;
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| AppError::SystemIntegration(error.to_string()))?;
    Ok(resolve_database_path_from(
        &executable,
        portable_mode()?,
        &app_data,
    ))
}

fn resolve_database_path_from(
    executable: &std::path::Path,
    portable: bool,
    app_data: &std::path::Path,
) -> PathBuf {
    if portable {
        if let Some(directory) = executable.parent() {
            return directory.join("data").join("shanji.db");
        }
    }
    app_data.join("shanji.db")
}

fn portable_mode() -> AppResult<bool> {
    let executable = std::env::current_exe()?;
    Ok(executable
        .parent()
        .is_some_and(|directory| directory.join("portable.flag").exists()))
}

pub(crate) fn candidate_database_paths(app: &AppHandle, current: &std::path::Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(app_data) = app.path().app_data_dir() {
        add_database_candidates(&mut candidates, &app_data);
    }
    if let Ok(app_local_data) = app.path().app_local_data_dir() {
        add_database_candidates(&mut candidates, &app_local_data);
    }
    if let Ok(app_config) = app.path().app_config_dir() {
        add_database_candidates(&mut candidates, &app_config);
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(directory) = executable.parent() {
            add_database_candidates(&mut candidates, directory);
            add_database_candidates(&mut candidates, &directory.join("data"));
        }
    }
    if let Some(directory) = current.parent() {
        add_database_candidates(&mut candidates, directory);
    }
    candidates.retain(|path| {
        path != current
            && path.is_file()
            && !path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.contains("restore-pending") || name.contains("migration-pending")
                })
    });
    candidates.sort();
    candidates.dedup();
    candidates
}

fn add_database_candidates(candidates: &mut Vec<PathBuf>, directory: &std::path::Path) {
    candidates.push(directory.join("shanji.db"));
    candidates.push(directory.join("shanji.db.backup-before-v3"));
    if let Ok(entries) = fs::read_dir(directory.join("backups")) {
        candidates.extend(entries.filter_map(Result::ok).map(|entry| entry.path()));
    }
}

fn build_configured_windows(app: &tauri::App, recovery_only: bool) -> AppResult<()> {
    // Windows are intentionally created only after startup state is managed, preventing IPC from
    // racing initialization and keeping capture/reminder paths unavailable during data recovery.
    for window_config in app.config().app.windows.clone() {
        if recovery_only && window_config.label != "main" {
            continue;
        }
        WebviewWindowBuilder::from_config(app.handle(), &window_config)
            .map_err(|error| AppError::SystemIntegration(error.to_string()))?
            .build()
            .map_err(|error| AppError::SystemIntegration(error.to_string()))?;
    }
    Ok(())
}

fn create_tray(app: &AppHandle) -> AppResult<(MenuItem<tauri::Wry>, MenuItem<tauri::Wry>)> {
    let capture = MenuItem::with_id(app, "capture", "快速记录", true, None::<&str>)
        .map_err(|error| AppError::SystemIntegration(error.to_string()))?;
    let show = MenuItem::with_id(app, "show", "打开闪记", true, None::<&str>)
        .map_err(|error| AppError::SystemIntegration(error.to_string()))?;
    let reminders = MenuItem::with_id(app, "reminders", "待确认提醒（0）", true, None::<&str>)
        .map_err(|error| AppError::SystemIntegration(error.to_string()))?;
    let updates = MenuItem::with_id(app, "updates", "检查更新", true, None::<&str>)
        .map_err(|error| AppError::SystemIntegration(error.to_string()))?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)
        .map_err(|error| AppError::SystemIntegration(error.to_string()))?;
    let menu = Menu::with_items(app, &[&capture, &show, &reminders, &updates, &quit])
        .map_err(|error| AppError::SystemIntegration(error.to_string()))?;

    let last_click = Arc::new(Mutex::new(None::<Instant>));
    let click_tracker = Arc::clone(&last_click);
    let mut builder = TrayIconBuilder::with_id("main-tray")
        .menu(&menu)
        .tooltip("闪记")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "capture" => {
                let _ = show_capture_window(app);
            }
            "show" => {
                let _ = show_main_window(app);
            }
            "reminders" => {
                let _ = show_main_window(app);
                let _ = app.emit("open_reminder_center", ());
            }
            "updates" => {
                let _ = show_main_window(app);
                let _ = app.emit("open_update", ());
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(move |tray, event| match event {
            TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } => {
                let _ = show_main_window(tray.app_handle());
            }
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } => {
                let now = Instant::now();
                let mut previous = click_tracker.lock().expect("tray click mutex poisoned");
                if previous
                    .is_some_and(|value| now.duration_since(value) <= Duration::from_millis(450))
                {
                    *previous = None;
                    let _ = show_main_window(tray.app_handle());
                } else {
                    *previous = Some(now);
                }
            }
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder
        .build(app)
        .map_err(|error| AppError::SystemIntegration(error.to_string()))?;
    Ok((reminders, updates))
}

pub(crate) fn update_tray_reminder_count(
    app: &AppHandle,
    database: &Database,
    now: chrono::DateTime<chrono::Utc>,
) {
    let Ok(count) = database
        .list_pending_reminders(now)
        .map(|items| items.len())
    else {
        return;
    };
    if let Some(state) = app.try_state::<AppState>() {
        if let Some(item) = state
            .pending_reminder_menu
            .lock()
            .expect("tray menu mutex poisoned")
            .as_ref()
        {
            let _ = item.set_text(format!("待确认提醒（{count}）"));
        }
    }
}

pub(crate) fn show_overlay_reminder(
    app: &AppHandle,
    notification: &db::DueNotification,
) -> AppResult<()> {
    let window = app
        .get_webview_window("reminder")
        .ok_or_else(|| AppError::SystemIntegration("置顶提醒窗口不存在".into()))?;
    if let Some(monitor) = window
        .current_monitor()
        .map_err(|error| AppError::SystemIntegration(error.to_string()))?
    {
        let monitor_size = monitor.size();
        let monitor_position = monitor.position();
        let window_size = window
            .outer_size()
            .map_err(|error| AppError::SystemIntegration(error.to_string()))?;
        let x = monitor_position.x
            + i32::try_from(monitor_size.width.saturating_sub(window_size.width + 24)).unwrap_or(0);
        let y = monitor_position.y
            + i32::try_from(monitor_size.height.saturating_sub(window_size.height + 56))
                .unwrap_or(0);
        let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
    }
    window
        .show()
        .map_err(|error| AppError::SystemIntegration(error.to_string()))?;
    app.emit_to("reminder", "overlay_reminder", notification.clone())
        .map_err(|error| AppError::SystemIntegration(error.to_string()))?;
    Ok(())
}

pub(crate) fn show_capture_window(app: &AppHandle) -> AppResult<()> {
    let window = app
        .get_webview_window("capture")
        .ok_or_else(|| AppError::SystemIntegration("快速录入窗口不存在".into()))?;
    window
        .center()
        .map_err(|error| AppError::SystemIntegration(error.to_string()))?;
    window
        .show()
        .map_err(|error| AppError::SystemIntegration(error.to_string()))?;
    window
        .set_focus()
        .map_err(|error| AppError::SystemIntegration(error.to_string()))?;
    let _ = app.emit_to("capture", "capture_opened", ());
    Ok(())
}

pub(crate) fn show_main_window(app: &AppHandle) -> AppResult<()> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| AppError::SystemIntegration("主窗口不存在".into()))?;
    window
        .show()
        .map_err(|error| AppError::SystemIntegration(error.to_string()))?;
    window
        .set_focus()
        .map_err(|error| AppError::SystemIntegration(error.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::resolve_database_path_from;

    #[test]
    fn configured_windows_wait_until_managed_state_is_ready() {
        let context: tauri::Context<tauri::Wry> = tauri::generate_context!();
        let windows = &context.config().app.windows;

        assert_eq!(windows.len(), 3);
        assert!(windows.iter().all(|window| !window.create));
    }

    #[test]
    fn installed_database_path_does_not_follow_the_executable() {
        let sandbox = tempfile::tempdir().expect("create test directory");
        let first_executable = sandbox.path().join("installed/Shanji/shanji.exe");
        let moved_executable = sandbox.path().join("moved/Shanji/shanji.exe");
        let app_data = sandbox.path().join("app-data/com.shanji.desktop");

        assert_eq!(
            resolve_database_path_from(&first_executable, false, &app_data),
            app_data.join("shanji.db")
        );
        assert_eq!(
            resolve_database_path_from(&moved_executable, false, &app_data),
            app_data.join("shanji.db")
        );
        assert_eq!(
            resolve_database_path_from(&moved_executable, true, &app_data),
            moved_executable
                .parent()
                .expect("executable has parent directory")
                .join("data/shanji.db")
        );
    }
}
