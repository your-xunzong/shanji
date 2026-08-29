use std::{sync::Mutex, time::Duration};

use lettre::{
    Message, SmtpTransport, Transport,
    message::{Mailbox, header::ContentType},
    transport::smtp::authentication::Credentials,
};

use crate::{db::DueNotification, domain::Settings};

const CREDENTIAL_SERVICE: &str = "com.shanji.desktop.smtp";
const CREDENTIAL_ACCOUNT: &str = "default";

#[derive(Debug)]
pub struct MailError {
    pub code: &'static str,
    pub user_message: String,
    pub diagnostic: String,
}

impl MailError {
    fn credential(action: &str, error: impl std::fmt::Display) -> Self {
        Self {
            code: "credential_store_unavailable",
            user_message: format!("无法{action}邮件密码，请检查系统凭据存储后重试。"),
            diagnostic: error.to_string(),
        }
    }

    fn configuration(error: impl std::fmt::Display) -> Self {
        Self {
            code: "smtp_configuration_invalid",
            user_message: "邮件设置无法使用，请检查服务器地址和邮箱格式。".into(),
            diagnostic: error.to_string(),
        }
    }

    fn delivery(error: impl std::fmt::Display) -> Self {
        Self {
            code: "smtp_delivery_failed",
            user_message: "邮件服务器没有接受测试邮件，请检查网络、端口、加密方式和登录信息。"
                .into(),
            diagnostic: error.to_string(),
        }
    }
}

trait CredentialStore: Send + Sync {
    fn get(&self) -> Result<Option<String>, MailError>;
    fn set(&self, password: &str) -> Result<(), MailError>;
    fn delete(&self) -> Result<(), MailError>;
}

struct SystemCredentialStore {
    access: Mutex<()>,
}

impl SystemCredentialStore {
    fn new() -> Self {
        Self {
            access: Mutex::new(()),
        }
    }

    fn entry() -> Result<keyring::Entry, MailError> {
        keyring::Entry::new(CREDENTIAL_SERVICE, CREDENTIAL_ACCOUNT)
            .map_err(|error| MailError::credential("打开", error))
    }
}

impl CredentialStore for SystemCredentialStore {
    fn get(&self) -> Result<Option<String>, MailError> {
        let _guard = self.access.lock().expect("credential store mutex poisoned");
        match Self::entry()?.get_password() {
            Ok(password) => Ok(Some(password)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(MailError::credential("读取", error)),
        }
    }

    fn set(&self, password: &str) -> Result<(), MailError> {
        let _guard = self.access.lock().expect("credential store mutex poisoned");
        Self::entry()?
            .set_password(password)
            .map_err(|error| MailError::credential("保存", error))
    }

    fn delete(&self) -> Result<(), MailError> {
        let _guard = self.access.lock().expect("credential store mutex poisoned");
        match Self::entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(MailError::credential("删除", error)),
        }
    }
}

pub struct MailService {
    credentials: Box<dyn CredentialStore>,
}

impl MailService {
    pub fn new() -> Self {
        Self {
            credentials: Box::new(SystemCredentialStore::new()),
        }
    }

    #[cfg(test)]
    fn with_credentials(credentials: Box<dyn CredentialStore>) -> Self {
        Self { credentials }
    }

    pub fn password(&self) -> Result<Option<String>, MailError> {
        self.credentials.get()
    }

    pub fn password_configured(&self) -> Result<bool, MailError> {
        self.password().map(|value| value.is_some())
    }

    pub fn store_password(&self, password: &str) -> Result<(), MailError> {
        if password.is_empty() {
            return Err(MailError::configuration("password is empty"));
        }
        self.credentials.set(password)
    }

    pub fn restore_password(&self, previous: Option<&str>) -> Result<(), MailError> {
        match previous {
            Some(password) => self.credentials.set(password),
            None => self.credentials.delete(),
        }
    }

    pub fn send_reminder(
        &self,
        settings: &Settings,
        notification: &DueNotification,
    ) -> Result<(), MailError> {
        let subject = format!("闪记提醒：{}", notification.title);
        let kind = if notification.completion_policy == "MUST_COMPLETE_TODAY" {
            "今日必做"
        } else {
            "待办事项"
        };
        let body = format!(
            "{}\n\n类型：{}\n到期时间：{} {}\n\n请在闪记中完成、稍后提醒或确认已看到。",
            notification.title, kind, notification.due_local_date, notification.due_local_time
        );
        self.send_message(settings, &subject, &body)
    }

    pub fn send_test(&self, settings: &Settings) -> Result<(), MailError> {
        self.send_message(
            settings,
            "闪记邮件通知测试",
            "这是一封由你主动发送的测试邮件。收到它说明当前 SMTP 设置可以正常投递。",
        )
    }

    fn send_message(
        &self,
        settings: &Settings,
        subject: &str,
        body: &str,
    ) -> Result<(), MailError> {
        let password = self.password()?.ok_or_else(|| MailError {
            code: "smtp_password_missing",
            user_message: "尚未保存 SMTP 密码，请填写后先保存设置。".into(),
            diagnostic: "credential entry is missing".into(),
        })?;
        let from: Mailbox = settings
            .smtp_from
            .parse()
            .map_err(MailError::configuration)?;
        let to: Mailbox = settings.smtp_to.parse().map_err(MailError::configuration)?;
        let message = Message::builder()
            .from(from)
            .to(to)
            .subject(subject)
            .header(ContentType::TEXT_PLAIN)
            .body(body.to_string())
            .map_err(MailError::configuration)?;

        let builder = match settings.smtp_security.as_str() {
            "tls" => SmtpTransport::relay(&settings.smtp_host),
            "starttls" => SmtpTransport::starttls_relay(&settings.smtp_host),
            _ => return Err(MailError::configuration("unsupported SMTP security mode")),
        }
        .map_err(MailError::configuration)?;
        let username = if settings.smtp_username.is_empty() {
            settings.smtp_from.clone()
        } else {
            settings.smtp_username.clone()
        };
        let transport = builder
            .port(settings.smtp_port)
            .credentials(Credentials::new(username, password))
            .timeout(Some(Duration::from_secs(15)))
            .build();
        transport.send(&message).map_err(MailError::delivery)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    struct MemoryCredentials(Mutex<Option<String>>);

    impl CredentialStore for MemoryCredentials {
        fn get(&self) -> Result<Option<String>, MailError> {
            Ok(self.0.lock().unwrap().clone())
        }

        fn set(&self, password: &str) -> Result<(), MailError> {
            *self.0.lock().unwrap() = Some(password.to_string());
            Ok(())
        }

        fn delete(&self) -> Result<(), MailError> {
            *self.0.lock().unwrap() = None;
            Ok(())
        }
    }

    #[test]
    fn credential_replacement_can_be_rolled_back_without_database_storage() {
        let service = MailService::with_credentials(Box::new(MemoryCredentials(Mutex::new(Some(
            "old-secret".into(),
        )))));
        let previous = service.password().unwrap();
        service.store_password("new-secret").unwrap();
        assert_eq!(service.password().unwrap().as_deref(), Some("new-secret"));

        service.restore_password(previous.as_deref()).unwrap();
        assert_eq!(service.password().unwrap().as_deref(), Some("old-secret"));
    }

    #[test]
    fn empty_password_is_rejected() {
        let service = MailService::with_credentials(Box::new(MemoryCredentials(Mutex::new(None))));
        assert_eq!(
            service.store_password("").unwrap_err().code,
            "smtp_configuration_invalid"
        );
    }
}
