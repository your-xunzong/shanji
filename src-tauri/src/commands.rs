use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, Utc};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_global_shortcut::GlobalShortcutExt;

use crate::{
    AppState,
    db::Database,
    domain::{Category, CreateItemInput, Item, Settings, UpdateSettingsInput},
    error::AppResult,
    notification::NotificationStatus,
};

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

    if previous.global_shortcut != next.global_shortcut {
        let shortcuts = app.global_shortcut();
        if shortcuts.is_registered(previous.global_shortcut.as_str()) {
            shortcuts
                .unregister(previous.global_shortcut.as_str())
                .map_err(|error| format!("无法释放旧快捷键：{error}"))?;
        }
        if let Err(error) = shortcuts.register(next.global_shortcut.as_str()) {
            let _ = shortcuts.register(previous.global_shortcut.as_str());
            return Err(format!("新快捷键不可用，已恢复原设置：{error}"));
        }
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
            if previous.global_shortcut != next.global_shortcut {
                let shortcuts = app.global_shortcut();
                let _ = shortcuts.unregister(next.global_shortcut.as_str());
                let _ = shortcuts.register(previous.global_shortcut.as_str());
            }
            Err(error.into())
        }
    }
}

#[tauri::command]
pub fn list_categories(state: State<'_, AppState>) -> Result<Vec<Category>, String> {
    state.database.list_categories().map_err(Into::into)
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
        },
        now,
    )
}
