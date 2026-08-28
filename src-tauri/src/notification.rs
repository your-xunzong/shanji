use std::{fs, path::PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::{db::DueNotification, error::AppResult};

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

    pub fn send(&self, notification: &DueNotification) -> Result<(), DeliveryError> {
        #[cfg(windows)]
        {
            self.send_windows(notification)
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
                .map_err(|error| DeliveryError {
                    code: "platform_submit_failed",
                    diagnostic: error.to_string(),
                })
        }
    }

    #[cfg(windows)]
    fn send_windows(&self, notification: &DueNotification) -> Result<(), DeliveryError> {
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
        let item_id = notification.item_id.clone();
        Toast::new(self.windows_app_id())
            .title("闪记提醒")
            .text1(&notification.title)
            .text2(&format!(
                "到期：{} {}",
                notification.due_local_date, notification.due_local_time
            ))
            .on_activated(move |_| {
                let _ = crate::show_main_window(&app);
                let _ = app.emit("notification_opened", item_id.clone());
                Ok(())
            })
            .show()
            .map_err(|error| DeliveryError {
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
