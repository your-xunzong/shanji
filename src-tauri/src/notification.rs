use std::{fs, path::PathBuf};

use serde::Serialize;
use tauri::AppHandle;
#[cfg(windows)]
use tauri::{Emitter, Manager};

use crate::{
    db::{ClassificationNotification, DueNotification},
    error::AppResult,
};

#[cfg(windows)]
const WINDOWS_INSTALLED_APP_ID: &str = "com.shanji.desktop";
#[cfg(windows)]
const WINDOWS_PORTABLE_APP_ID: &str = "com.shanji.desktop.portable";
#[cfg(windows)]
const WINDOWS_INSTALLED_REGISTRY_PATH: &str = r"Software\Classes\AppUserModelId\com.shanji.desktop";
#[cfg(windows)]
const WINDOWS_PORTABLE_REGISTRY_PATH: &str =
    r"Software\Classes\AppUserModelId\com.shanji.desktop.portable";

const NOTIFICATION_ICON: &[u8] = include_bytes!("../icons/128x128.png");

#[derive(Debug, Clone)]
pub struct DeliveryError {
    pub code: &'static str,
    pub diagnostic: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationStatus {
    pub platform: String,
    pub portable: bool,
    pub identity_status: String,
    pub can_notify: bool,
    pub message: String,
    pub last_result: Option<String>,
    pub last_error_code: Option<String>,
    pub last_attempt_at: Option<String>,
}

pub struct NotificationService {
    app: AppHandle,
    portable: bool,
    #[cfg(windows)]
    icon_path: PathBuf,
}

impl NotificationService {
    pub fn new(app: AppHandle, portable: bool, data_directory: PathBuf) -> AppResult<Self> {
        fs::create_dir_all(&data_directory)?;
        let icon_path = data_directory.join("notification-icon.png");
        if !icon_path.exists() || fs::metadata(&icon_path)?.len() != NOTIFICATION_ICON.len() as u64
        {
            fs::write(&icon_path, NOTIFICATION_ICON)?;
        }

        let service = Self {
            app,
            portable,
            #[cfg(windows)]
            icon_path,
        };

        #[cfg(windows)]
        if !portable {
            if let Err(error) = service.register_windows_identity() {
                eprintln!("Windows 通知身份初始化失败：{}", error.diagnostic);
            }
        }

        Ok(service)
    }

    pub fn status(&self) -> NotificationStatus {
        #[cfg(windows)]
        {
            let registered = self.windows_identity_registered();
            let can_notify = registered;
            let (identity_status, message) = if self.portable && !registered {
                (
                    "registration_required",
                    "便携版尚未注册系统通知身份，启用后才能显示 Windows 原生横幅。",
                )
            } else if registered {
                (
                    "ready",
                    "系统通知身份已就绪；横幅仍受 Windows 通知和专注模式控制。",
                )
            } else {
                (
                    "unavailable",
                    "系统通知身份初始化失败，请重新安装或检查系统通知设置。",
                )
            };
            NotificationStatus {
                platform: "windows".into(),
                portable: self.portable,
                identity_status: identity_status.into(),
                can_notify,
                message: message.into(),
                last_result: None,
                last_error_code: None,
                last_attempt_at: None,
            }
        }

        #[cfg(not(windows))]
        NotificationStatus {
            platform: std::env::consts::OS.into(),
            portable: self.portable,
            identity_status: "ready".into(),
            can_notify: true,
            message: "系统通知适配器已就绪；显示行为受操作系统权限与勿扰设置控制。".into(),
            last_result: None,
            last_error_code: None,
            last_attempt_at: None,
        }
    }

    pub fn register_portable(&self) -> Result<NotificationStatus, DeliveryError> {
        if !self.portable {
            return Ok(self.status());
        }

        #[cfg(windows)]
        self.register_windows_identity()?;

        #[cfg(not(windows))]
        return Err(DeliveryError {
            code: "portable_registration_unsupported",
            diagnostic: "当前平台不需要 Windows 便携通知注册".into(),
        });

        #[cfg(windows)]
        Ok(self.status())
    }

    pub fn unregister_portable(&self) -> Result<NotificationStatus, DeliveryError> {
        if !self.portable {
            return Err(DeliveryError {
                code: "installed_identity_required",
                diagnostic: "安装版通知身份由安装器管理".into(),
            });
        }

        #[cfg(windows)]
        self.unregister_windows_identity()?;

        #[cfg(not(windows))]
        return Err(DeliveryError {
            code: "portable_registration_unsupported",
            diagnostic: "当前平台不需要 Windows 便携通知注册".into(),
        });

        #[cfg(windows)]
        Ok(self.status())
    }

    pub fn send(
        &self,
        notification: &DueNotification,
        persistent: bool,
    ) -> Result<Option<&'static str>, DeliveryError> {
        #[cfg(windows)]
        {
            if persistent {
                match self.send_windows(notification, true) {
                    Ok(()) => Ok(None),
                    Err(_) => self
                        .send_windows(notification, false)
                        .map(|()| Some("persistent_fell_back_to_standard")),
                }
            } else {
                self.send_windows(notification, false).map(|()| None)
            }
        }

        #[cfg(not(windows))]
        {
            use tauri_plugin_notification::NotificationExt;

            self.app
                .notification()
                .builder()
                .title("闪记提醒")
                .body(format!(
                    "{}\n到期：{} {}",
                    notification.title, notification.due_local_date, notification.due_local_time
                ))
                .show()
                .map(|()| persistent.then_some("persistent_not_supported"))
                .map_err(|error| DeliveryError {
                    code: "platform_submit_failed",
                    diagnostic: error.to_string(),
                })
        }
    }

    pub fn send_classification(
        &self,
        notification: &ClassificationNotification,
    ) -> Result<(), DeliveryError> {
        #[cfg(windows)]
        {
            use tauri_winrt_notification::{Scenario, Toast};

            if self.portable && !self.windows_identity_registered() {
                return Err(DeliveryError {
                    code: "identity_registration_required",
                    diagnostic: "便携版通知身份尚未注册".into(),
                });
            }
            if !self.windows_identity_registered() {
                self.register_windows_identity()?;
            }

            let app = self.app.clone();
            let item_id = notification.item_id.clone();
            let count = notification.count;
            let body = if count == 1 {
                "这条记录还没有选择事件类型".to_string()
            } else {
                format!("有 {count} 条记录还没有选择事件类型")
            };
            let mut toast = Toast::new(self.windows_app_id())
                .title("闪记 · 待选择事件类型")
                .text1(&body)
                .text2("记录已经安全保存，可以现在处理或稍后再选。")
                .scenario(Scenario::Reminder)
                .on_activated(move |action| {
                    handle_classification_action(&app, item_id.as_deref(), action.as_deref());
                    Ok(())
                });
            if count == 1 {
                toast = toast.add_button("设为普通", "set-ordinary");
            }
            toast = toast
                .add_button(
                    if count == 1 {
                        "选择其他事件类型"
                    } else {
                        "处理待选类型"
                    },
                    "choose-type",
                )
                .add_button("稍后处理", "snooze-classification");
            toast.show().map_err(|error| DeliveryError {
                code: "platform_submit_failed",
                diagnostic: error.to_string(),
            })
        }

        #[cfg(not(windows))]
        {
            use tauri_plugin_notification::NotificationExt;

            let body = if notification.count == 1 {
                "这条记录还没有选择事件类型。打开闪记即可处理。".to_string()
            } else {
                format!(
                    "有 {} 条记录还没有选择事件类型。打开闪记即可处理。",
                    notification.count
                )
            };
            self.app
                .notification()
                .builder()
                .title("闪记 · 待选择事件类型")
                .body(body)
                .show()
                .map_err(|error| DeliveryError {
                    code: "platform_submit_failed",
                    diagnostic: error.to_string(),
                })
        }
    }

    pub fn send_update_available(&self, version: &str) -> Result<(), DeliveryError> {
        #[cfg(windows)]
        {
            use tauri_winrt_notification::Toast;

            if self.portable && !self.windows_identity_registered() {
                return Err(DeliveryError {
                    code: "identity_registration_required",
                    diagnostic: "便携版通知身份尚未注册".into(),
                });
            }
            if !self.windows_identity_registered() {
                self.register_windows_identity()?;
            }

            let app = self.app.clone();
            Toast::new(self.windows_app_id())
                .title("闪记 · 新版本可用")
                .text1(&format!("可以更新到 v{version}"))
                .text2("打开闪记查看本次变化，再决定何时安装。")
                .on_activated(move |_| {
                    let _ = crate::show_main_window(&app);
                    Ok(())
                })
                .show()
                .map_err(|error| DeliveryError {
                    code: "platform_submit_failed",
                    diagnostic: error.to_string(),
                })
        }

        #[cfg(not(windows))]
        {
            use tauri_plugin_notification::NotificationExt;

            self.app
                .notification()
                .builder()
                .title("闪记 · 新版本可用")
                .body(format!("可以更新到 v{version}。打开闪记查看本次变化。"))
                .show()
                .map_err(|error| DeliveryError {
                    code: "platform_submit_failed",
                    diagnostic: error.to_string(),
                })
        }
    }

    #[cfg(windows)]
    fn send_windows(
        &self,
        notification: &DueNotification,
        persistent: bool,
    ) -> Result<(), DeliveryError> {
        use tauri_winrt_notification::{Scenario, Toast};

        if self.portable && !self.windows_identity_registered() {
            return Err(DeliveryError {
                code: "identity_registration_required",
                diagnostic: "便携版通知身份尚未注册".into(),
            });
        }
        if !self.windows_identity_registered() {
            self.register_windows_identity()?;
        }

        let app = self.app.clone();
        let item_id = notification.item_id.clone();
        let mut toast = Toast::new(self.windows_app_id())
            .title("闪记提醒")
            .text1(&notification.title)
            .text2(&format!(
                "到期：{} {}",
                notification.due_local_date, notification.due_local_time
            ))
            .on_activated(move |action| {
                handle_notification_action(&app, &item_id, action.as_deref());
                Ok(())
            });
        if persistent {
            toast = toast
                .scenario(Scenario::Reminder)
                .add_button("完成", "complete")
                .add_button("15 分钟后提醒", "snooze")
                .add_button("打开闪记", "open");
        }
        toast.show().map_err(|error| DeliveryError {
            code: "platform_submit_failed",
            diagnostic: error.to_string(),
        })
    }

    #[cfg(windows)]
    fn windows_identity_registered(&self) -> bool {
        use winreg::{RegKey, enums::HKEY_CURRENT_USER};

        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(self.windows_registry_path())
            .and_then(|key| key.get_value::<String, _>("DisplayName"))
            .is_ok_and(|name| name == "闪记")
    }

    #[cfg(windows)]
    fn register_windows_identity(&self) -> Result<(), DeliveryError> {
        use winreg::{RegKey, enums::HKEY_CURRENT_USER};

        let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey(self.windows_registry_path())
            .map_err(|error| DeliveryError {
                code: "identity_registration_failed",
                diagnostic: error.to_string(),
            })?;
        key.set_value("DisplayName", &"闪记")
            .and_then(|_| key.set_value("IconBackgroundColor", &"0"))
            .and_then(|_| key.set_value("IconUri", &self.icon_path.to_string_lossy().to_string()))
            .map_err(|error| DeliveryError {
                code: "identity_registration_failed",
                diagnostic: error.to_string(),
            })
    }

    #[cfg(windows)]
    fn unregister_windows_identity(&self) -> Result<(), DeliveryError> {
        use std::io::ErrorKind;
        use winreg::{RegKey, enums::HKEY_CURRENT_USER};

        match RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(WINDOWS_PORTABLE_REGISTRY_PATH) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(DeliveryError {
                code: "identity_unregistration_failed",
                diagnostic: error.to_string(),
            }),
        }
    }

    #[cfg(windows)]
    fn windows_app_id(&self) -> &'static str {
        if self.portable {
            WINDOWS_PORTABLE_APP_ID
        } else {
            WINDOWS_INSTALLED_APP_ID
        }
    }

    #[cfg(windows)]
    fn windows_registry_path(&self) -> &'static str {
        if self.portable {
            WINDOWS_PORTABLE_REGISTRY_PATH
        } else {
            WINDOWS_INSTALLED_REGISTRY_PATH
        }
    }
}

#[cfg(windows)]
fn handle_notification_action(app: &AppHandle, item_id: &str, action: Option<&str>) {
    match action {
        Some("complete") => {
            if let Some(state) = app.try_state::<crate::AppState>() {
                let now = state.clock.now_utc();
                if state
                    .database
                    .set_item_completed(item_id, true, now)
                    .is_ok()
                {
                    state.scheduler.wake();
                    crate::update_tray_reminder_count(app, &state.database, now);
                    let _ = app.emit("items_changed", ());
                    let _ = app.emit("reminder_center_changed", ());
                }
            }
        }
        Some("snooze") => {
            if let Some(state) = app.try_state::<crate::AppState>() {
                let now = state.clock.now_utc();
                if state.database.snooze_item(item_id, 15, now).is_ok() {
                    state.scheduler.wake();
                    crate::update_tray_reminder_count(app, &state.database, now);
                    let _ = app.emit("items_changed", ());
                    let _ = app.emit("reminder_center_changed", ());
                }
            }
        }
        _ => {
            let _ = crate::show_main_window(app);
            let _ = app.emit("notification_opened", item_id.to_string());
        }
    }
}

#[cfg(windows)]
fn handle_classification_action(app: &AppHandle, item_id: Option<&str>, action: Option<&str>) {
    match action {
        Some("set-ordinary") => {
            if let (Some(state), Some(item_id)) = (app.try_state::<crate::AppState>(), item_id) {
                let now = state.clock.now_utc();
                if state.database.classify_as_ordinary(item_id, now).is_ok() {
                    state.scheduler.wake();
                    let _ = app.emit("items_changed", ());
                }
            }
        }
        Some("snooze-classification") => {
            if let Some(state) = app.try_state::<crate::AppState>() {
                let now = state.clock.now_utc();
                let _ = state.database.snooze_classification(item_id, 60, now);
                state.scheduler.wake();
                let _ = app.emit("items_changed", ());
            }
        }
        _ => {
            let _ = crate::show_main_window(app);
            let _ = app.emit(
                "classification_opened",
                item_id.unwrap_or_default().to_string(),
            );
        }
    }
}
