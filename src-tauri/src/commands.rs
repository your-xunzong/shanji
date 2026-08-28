use std::{path::PathBuf, process::Command, time::Duration as StdDuration};

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_autostart::ManagerExt as AutostartExt;
use tauri_plugin_global_shortcut::GlobalShortcutExt;

use crate::{
    AppState, candidate_database_paths,
    db::{DataFileSummary, DataStatus, Database},
    domain::{
        Category, CreateItemInput, Item, Settings, Tag, TaxonomyInput, UpdateItemInput,
        UpdateSettingsInput,
    },
    error::AppResult,
    export::{ExportFilterInput, export_items, filter_items},
    notification::NotificationStatus,
};

const CURRENT_ONBOARDING_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutostartStatus {
    pub enabled: bool,
    pub available: bool,
    pub portable: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingStatus {
    pub required: bool,
    pub completed_version: u32,
    pub current_version: u32,
}

#[tauri::command]
pub fn list_items(filter: String, state: State<'_, AppState>) -> Result<Vec<Item>, String> {
    state
        .database
        .list_items(&filter, state.clock.now_utc())
        .map_err(Into::into)
}

#[tauri::command]
pub fn create_item(input: CreateItemInput, state: State<'_, AppState>) -> Result<Item, String> {
    let item = state
        .database
        .create_item(&input, state.clock.now_utc())
        .map_err(String::from)?;
    state.scheduler.wake();
    Ok(item)
}

#[tauri::command]
pub fn set_item_completed(
    id: String,
    completed: bool,
    state: State<'_, AppState>,
) -> Result<Item, String> {
    let item = state
        .database
        .set_item_completed(&id, completed, state.clock.now_utc())
        .map_err(String::from)?;
    state.scheduler.wake();
    Ok(item)
}

#[tauri::command]
pub fn set_reminder_paused(
    id: String,
    paused: bool,
    state: State<'_, AppState>,
) -> Result<Item, String> {
    let item = state
        .database
        .set_reminder_paused(&id, paused, state.clock.now_utc())
        .map_err(String::from)?;
    state.scheduler.wake();
    Ok(item)
}

#[tauri::command]
pub fn reschedule_item(
    id: String,
    due_at: String,
    state: State<'_, AppState>,
) -> Result<Item, String> {
    let item = state
        .database
        .reschedule_item(&id, &due_at, state.clock.now_utc())
        .map_err(String::from)?;
    state.scheduler.wake();
    Ok(item)
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<Settings, String> {
    state.database.get_settings().map_err(Into::into)
}

#[tauri::command]
pub fn update_settings(
    input: UpdateSettingsInput,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Settings, String> {
    input.validate().map_err(String::from)?;
    let previous = state.database.get_settings().map_err(String::from)?;
    let next = input.settings();
    let shortcut_changed = previous.global_shortcut != next.global_shortcut;
    let autostart_changed = previous.autostart_enabled != next.autostart_enabled;
    let autostart_before = if autostart_changed {
        Some(read_autostart(&app)?)
    } else {
        None
    };

    if shortcut_changed {
        let shortcuts = app.global_shortcut();
        if shortcuts.is_registered(previous.global_shortcut.as_str()) {
            shortcuts
                .unregister(previous.global_shortcut.as_str())
                .map_err(|_| "无法释放原快捷键；设置没有更改，请重启闪记后再试。".to_string())?;
        }
        if shortcuts.register(next.global_shortcut.as_str()).is_err() {
            let _ = shortcuts.register(previous.global_shortcut.as_str());
            return Err(format!(
                "快捷键“{}”已被其他程序占用或被系统保留，请换一个组合；原快捷键仍然有效。",
                next.global_shortcut
            ));
        }
    }

    if let Some(before) = autostart_before
        && before != next.autostart_enabled
        && let Err(error) = set_autostart(&app, next.autostart_enabled)
    {
        let _ = set_autostart(&app, before);
        restore_shortcut(&app, &previous.global_shortcut, &next.global_shortcut);
        return Err(error);
    }

    match state
        .database
        .update_settings(&input, state.clock.now_utc())
    {
        Ok(settings) => {
            *state
                .shortcut_warning
                .lock()
                .expect("shortcut warning mutex poisoned") = None;
            state.scheduler.wake();
            Ok(settings)
        }
        Err(error) => {
            if let Some(before) = autostart_before
                && let Err(restore_error) = set_autostart(&app, before)
            {
                eprintln!("开机启动回滚失败：{restore_error}");
            }
            restore_shortcut(&app, &previous.global_shortcut, &next.global_shortcut);
            Err(error.into())
        }
    }
}

fn restore_shortcut(app: &AppHandle, previous: &str, attempted: &str) {
    if previous == attempted {
        return;
    }
    let shortcuts = app.global_shortcut();
    if shortcuts.is_registered(attempted) {
        let _ = shortcuts.unregister(attempted);
    }
    if !shortcuts.is_registered(previous)
        && let Err(error) = shortcuts.register(previous)
    {
        eprintln!("原快捷键恢复失败：{error}");
    }
}

fn read_autostart(app: &AppHandle) -> Result<bool, String> {
    app.autolaunch()
        .is_enabled()
        .map_err(|error| format!("无法读取系统开机启动状态：{error}"))
}

fn set_autostart(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let manager = app.autolaunch();
    let result = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };
    result.map_err(|error| {
        if enabled {
            format!("无法启用开机启动：{error}。原设置保持不变。")
        } else {
            format!("无法关闭开机启动：{error}。原设置保持不变。")
        }
    })?;
    let actual = read_autostart(app)?;
    if actual != enabled {
        return Err("系统没有接受开机启动变更，原设置保持不变。".into());
    }
    Ok(())
}

#[tauri::command]
pub fn get_autostart_status(app: AppHandle, state: State<'_, AppState>) -> AutostartStatus {
    match read_autostart(&app) {
        Ok(enabled) => AutostartStatus {
            enabled,
            available: true,
            portable: state.portable,
            message: if state.portable {
                "便携版启动项指向当前程序路径；移动或删除目录后将失效。".into()
            } else if enabled {
                "闪记将在登录后以后台模式启动。".into()
            } else {
                "开机启动当前未启用。".into()
            },
        },
        Err(error) => AutostartStatus {
            enabled: state
                .database
                .get_settings()
                .map(|settings| settings.autostart_enabled)
                .unwrap_or(false),
            available: false,
            portable: state.portable,
            message: error,
        },
    }
}

#[tauri::command]
pub fn get_onboarding_status(state: State<'_, AppState>) -> Result<OnboardingStatus, String> {
    let completed_version = state.database.onboarding_version().map_err(String::from)?;
    Ok(OnboardingStatus {
        required: completed_version < CURRENT_ONBOARDING_VERSION,
        completed_version,
        current_version: CURRENT_ONBOARDING_VERSION,
    })
}

#[tauri::command]
pub fn complete_onboarding(state: State<'_, AppState>) -> Result<OnboardingStatus, String> {
    state
        .database
        .complete_onboarding(CURRENT_ONBOARDING_VERSION)
        .map_err(String::from)?;
    Ok(OnboardingStatus {
        required: false,
        completed_version: CURRENT_ONBOARDING_VERSION,
        current_version: CURRENT_ONBOARDING_VERSION,
    })
}

#[tauri::command]
pub fn list_categories(state: State<'_, AppState>) -> Result<Vec<Category>, String> {
    state.database.list_categories().map_err(Into::into)
}

#[tauri::command]
pub fn create_category(
    input: TaxonomyInput,
    state: State<'_, AppState>,
) -> Result<Category, String> {
    state.database.create_category(&input).map_err(Into::into)
}

#[tauri::command]
pub fn update_category(
    id: String,
    input: TaxonomyInput,
    state: State<'_, AppState>,
) -> Result<Category, String> {
    state
        .database
        .update_category(&id, &input)
        .map_err(Into::into)
}

#[tauri::command]
pub fn delete_category(
    id: String,
    reassign_to: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .database
        .delete_category(&id, reassign_to.as_deref())
        .map_err(Into::into)
}

#[tauri::command]
pub fn move_category(
    id: String,
    direction: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .database
        .move_category(&id, &direction)
        .map_err(Into::into)
}

#[tauri::command]
pub fn list_tags(state: State<'_, AppState>) -> Result<Vec<Tag>, String> {
    state.database.list_tags().map_err(Into::into)
}

#[tauri::command]
pub fn create_tag(input: TaxonomyInput, state: State<'_, AppState>) -> Result<Tag, String> {
    state.database.create_tag(&input).map_err(Into::into)
}

#[tauri::command]
pub fn update_tag(
    id: String,
    input: TaxonomyInput,
    state: State<'_, AppState>,
) -> Result<Tag, String> {
    state.database.update_tag(&id, &input).map_err(Into::into)
}

#[tauri::command]
pub fn delete_tag(id: String, state: State<'_, AppState>) -> Result<(), String> {
    state.database.delete_tag(&id).map_err(Into::into)
}

#[tauri::command]
pub fn move_tag(id: String, direction: String, state: State<'_, AppState>) -> Result<(), String> {
    state.database.move_tag(&id, &direction).map_err(Into::into)
}

#[tauri::command]
pub fn update_item(
    id: String,
    input: UpdateItemInput,
    state: State<'_, AppState>,
) -> Result<Item, String> {
    let item = state
        .database
        .update_item(&id, &input, state.clock.now_utc())
        .map_err(String::from)?;
    state.scheduler.wake();
    Ok(item)
}

#[tauri::command]
pub fn set_item_deleted(
    id: String,
    deleted: bool,
    state: State<'_, AppState>,
) -> Result<Item, String> {
    let item = state
        .database
        .set_item_deleted(&id, deleted, state.clock.now_utc())
        .map_err(String::from)?;
    state.scheduler.wake();
    Ok(item)
}

#[tauri::command]
pub fn permanently_delete_item(id: String, state: State<'_, AppState>) -> Result<(), String> {
    state
        .database
        .permanently_delete_item(&id)
        .map_err(Into::into)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub path: String,
    pub item_count: usize,
}

#[tauri::command]
pub fn export_excel(
    path: String,
    filter: ExportFilterInput,
    state: State<'_, AppState>,
) -> Result<ExportResult, String> {
    let items = state
        .database
        .list_items("all", state.clock.now_utc())
        .map_err(String::from)?;
    let items = filter_items(items, &filter).map_err(String::from)?;
    export_items(std::path::Path::new(&path), &items).map_err(String::from)?;
    Ok(ExportResult {
        path,
        item_count: items.len(),
    })
}

#[tauri::command]
pub fn load_draft(state: State<'_, AppState>) -> Result<String, String> {
    state.database.load_draft().map_err(Into::into)
}

#[tauri::command]
pub fn save_draft(content: String, state: State<'_, AppState>) -> Result<(), String> {
    state
        .database
        .save_draft(&content, state.clock.now_utc())
        .map_err(Into::into)
}

#[tauri::command]
pub fn hide_capture(app: AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("capture")
        .ok_or_else(|| "快速录入窗口不存在".to_string())?;
    window.hide().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn show_capture(app: AppHandle) -> Result<(), String> {
    crate::show_capture_window(&app).map_err(Into::into)
}

#[tauri::command]
pub fn show_main(app: AppHandle) -> Result<(), String> {
    crate::show_main_window(&app).map_err(Into::into)
}

#[tauri::command]
pub fn get_system_warning(state: State<'_, AppState>) -> Option<String> {
    state
        .shortcut_warning
        .lock()
        .expect("shortcut warning mutex poisoned")
        .clone()
}

#[tauri::command]
pub fn get_notification_status(state: State<'_, AppState>) -> Result<NotificationStatus, String> {
    notification_status(&state)
}

#[tauri::command]
pub fn register_portable_notifications(
    state: State<'_, AppState>,
) -> Result<NotificationStatus, String> {
    state
        .notifications
        .register_portable()
        .map_err(|error| format!("无法启用便携版系统通知：{}", error.diagnostic))?;
    notification_status(&state)
}

#[tauri::command]
pub fn unregister_portable_notifications(
    state: State<'_, AppState>,
) -> Result<NotificationStatus, String> {
    state
        .notifications
        .unregister_portable()
        .map_err(|error| format!("无法撤销便携版系统通知：{}", error.diagnostic))?;
    notification_status(&state)
}

fn notification_status(state: &AppState) -> Result<NotificationStatus, String> {
    let mut status = state.notifications.status();
    if let Some(delivery) = state
        .database
        .last_notification_delivery()
        .map_err(String::from)?
    {
        if delivery.result == "FAILED" {
            status.message = match delivery.error_code.as_deref() {
                Some("identity_registration_required") => {
                    "便携版通知身份尚未注册，请先点击“启用便携版系统通知”。".into()
                }
                Some("platform_submit_failed") => {
                    "最近一次未能提交给系统；请检查 Windows“系统 > 通知”和专注模式后重试。".into()
                }
                _ => "最近一次系统通知提交失败，后台会自动重试；可再次运行测试通知核对。".into(),
            };
        }
        status.last_result = Some(delivery.result);
        status.last_error_code = delivery.error_code;
        status.last_attempt_at = Some(delivery.attempted_at);
    }
    Ok(status)
}

#[tauri::command]
pub fn create_notification_test_item(state: State<'_, AppState>) -> Result<Item, String> {
    let settings = state.database.get_settings().map_err(String::from)?;
    if !settings.notifications_enabled {
        return Err("通知总开关已关闭，请先启用系统通知".into());
    }
    let status = state.notifications.status();
    if !status.can_notify {
        return Err(status.message);
    }
    let now = state.clock.now_utc();
    let item = insert_notification_test_item(&state.database, now).map_err(String::from)?;
    state.scheduler.wake();
    state.scheduler.wake_after(StdDuration::from_secs(11));
    Ok(item)
}

#[tauri::command]
pub fn get_data_status(app: AppHandle, state: State<'_, AppState>) -> Result<DataStatus, String> {
    let candidates = candidate_database_paths(&app, state.database.path());
    state.database.data_status(&candidates).map_err(Into::into)
}

#[tauri::command]
pub fn create_data_backup(state: State<'_, AppState>) -> Result<DataFileSummary, String> {
    state.database.create_backup().map_err(Into::into)
}

#[tauri::command]
pub fn open_data_directory(state: State<'_, AppState>) -> Result<(), String> {
    let directory = state
        .database
        .path()
        .parent()
        .ok_or_else(|| "数据目录无效".to_string())?;

    #[cfg(target_os = "windows")]
    let mut command = Command::new("explorer.exe");
    #[cfg(target_os = "macos")]
    let mut command = Command::new("open");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = Command::new("xdg-open");

    command
        .arg(directory)
        .spawn()
        .map_err(|_| "无法打开数据位置，请从上方路径手动打开。".to_string())?;
    Ok(())
}

#[tauri::command]
pub fn restore_database(
    candidate_path: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let requested = PathBuf::from(&candidate_path);
    let allowed = candidate_database_paths(&app, state.database.path());
    if !allowed.iter().any(|candidate| candidate == &requested) {
        return Err("这份数据不在闪记识别的恢复位置中，请重新刷新后选择。".into());
    }
    state
        .database
        .stage_restore(&requested)
        .map_err(String::from)?;
    app.restart()
}

pub(crate) fn insert_notification_test_item(
    database: &Database,
    now: DateTime<Utc>,
) -> AppResult<Item> {
    database.create_item(
        &CreateItemInput {
            title: "通知测试：闪记提醒功能正常".into(),
            notes: "这是一条本地测试事项，可在确认通知后将其标记完成。".into(),
            category_id: None,
            due_at: Some((now + Duration::seconds(10)).to_rfc3339()),
            must_complete_today: false,
            repeat_interval_minutes: None,
            tag_ids: Vec::new(),
        },
        now,
    )
}
