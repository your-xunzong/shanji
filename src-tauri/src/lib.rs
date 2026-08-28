mod commands;
mod db;
mod domain;
mod error;
mod notification;
mod scheduler;

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use db::Database;
use domain::{Clock, SystemClock};
use error::{AppError, AppResult};
use notification::NotificationService;
use scheduler::SchedulerHandle;
use tauri::{
    AppHandle, Emitter, Manager, RunEvent,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

pub struct AppState {
    database: Arc<Database>,
    clock: Arc<dyn Clock>,
    notifications: Arc<NotificationService>,
    scheduler: SchedulerHandle,
    shortcut_warning: Mutex<Option<String>>,
}

pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            let _ = show_main_window(app);
        }))
        .plugin(tauri_plugin_notification::init())
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
            let database = Arc::new(Database::open(&database_path)?);
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
            );
            if notification_test_requested {
                scheduler.wake_after(Duration::from_secs(11));
            }
            app.manage(AppState {
                database,
                clock,
                notifications,
                scheduler,
                shortcut_warning: Mutex::new(shortcut_warning),
            });

            create_tray(app.handle())?;

            if std::env::args().any(|argument| argument == "--background") {
                if let Some(main) = app.get_webview_window("main") {
                    main.hide()?;
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_items,
            commands::create_item,
            commands::set_item_completed,
            commands::set_reminder_paused,
            commands::reschedule_item,
            commands::get_settings,
            commands::update_settings,
            commands::list_categories,
            commands::load_draft,
            commands::save_draft,
            commands::hide_capture,
            commands::show_capture,
            commands::show_main,
            commands::get_system_warning,
            commands::get_notification_status,
            commands::register_portable_notifications,
            commands::unregister_portable_notifications,
            commands::create_notification_test_item,
        ]);

    let app = builder
        .build(tauri::generate_context!())
        .expect("failed to build Shanji application");

    app.run(|app, event| {
        if let RunEvent::Exit = event
            && let Some(state) = app.try_state::<AppState>()
        {
            state.scheduler.stop();
        }
    });
}

fn resolve_database_path(app: &AppHandle) -> AppResult<PathBuf> {
    let executable = std::env::current_exe()?;
    if portable_mode()?
        && let Some(directory) = executable.parent()
    {
        return Ok(directory.join("data").join("shanji.db"));
    }
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|error| AppError::SystemIntegration(error.to_string()))?
        .join("shanji.db"))
}

fn portable_mode() -> AppResult<bool> {
    let executable = std::env::current_exe()?;
    Ok(executable
        .parent()
        .is_some_and(|directory| directory.join("portable.flag").exists()))
}

fn create_tray(app: &AppHandle) -> AppResult<()> {
    let capture = MenuItem::with_id(app, "capture", "快速记录", true, None::<&str>)
        .map_err(|error| AppError::SystemIntegration(error.to_string()))?;
    let show = MenuItem::with_id(app, "show", "打开闪记", true, None::<&str>)
        .map_err(|error| AppError::SystemIntegration(error.to_string()))?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)
        .map_err(|error| AppError::SystemIntegration(error.to_string()))?;
    let menu = Menu::with_items(app, &[&capture, &show, &quit])
        .map_err(|error| AppError::SystemIntegration(error.to_string()))?;

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
            "quit" => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder
        .build(app)
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
