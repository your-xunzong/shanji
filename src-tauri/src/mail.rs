use std::{sync::Mutex, time::Duration};

use lettre::{
    Message, SmtpTransport, Transport,
    message::{Mailbox, MultiPart, SinglePart, header::ContentType},
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
        recipient: &str,
        notification: &DueNotification,
    ) -> Result<(), MailError> {
        let subject = format!("闪记提醒：{}", notification.title);
        let kind = event_kind_label(notification.event_kind.as_deref());
        let text = format!(
            "{}\n\n类型：{}\n到期时间：{} {}\n\n请在闪记中完成、稍后提醒或确认已看到。",
            notification.title, kind, notification.due_local_date, notification.due_local_time
        );
        let html = reminder_html(
            &notification.title,
            kind,
            &notification.due_local_date,
            &notification.due_local_time,
        );
        self.send_message(settings, recipient, &subject, &text, &html)
    }

    pub fn send_digest(
        &self,
        settings: &Settings,
        recipient: &str,
        local_date: &str,
        notifications: &[DueNotification],
    ) -> Result<(), MailError> {
        let subject = format!("闪记每日摘要：{} 项待处理", notifications.len());
        let mut text = format!("{local_date} 的闪记提醒摘要\n\n");
        for notification in notifications {
            text.push_str(&format!(
                "- {}（{} {}）\n",
                notification.title, notification.due_local_date, notification.due_local_time
            ));
        }
        text.push_str("\n请回到闪记完成、调整或暂停事项。");
        let html = digest_html(local_date, notifications);
        self.send_message(settings, recipient, &subject, &text, &html)
    }

    pub fn send_test(&self, settings: &Settings) -> Result<(), MailError> {
        self.send_message(
            settings,
            &settings.smtp_to,
            "闪记邮件通知测试",
            "这是一封由你主动发送的测试邮件。收到它说明当前 SMTP 设置可以正常投递。",
            &test_html(),
        )
    }

    fn send_message(
        &self,
        settings: &Settings,
        recipient: &str,
        subject: &str,
        text: &str,
        html: &str,
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
        let to: Mailbox = recipient.parse().map_err(MailError::configuration)?;
        let message = Message::builder()
            .from(from)
            .to(to)
            .subject(subject)
            .multipart(
                MultiPart::alternative()
                    .singlepart(
                        SinglePart::builder()
                            .header(ContentType::TEXT_PLAIN)
                            .body(text.to_string()),
                    )
                    .singlepart(
                        SinglePart::builder()
                            .header(ContentType::TEXT_HTML)
                            .body(html.to_string()),
                    ),
            )
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

fn event_kind_label(kind: Option<&str>) -> &'static str {
    match kind {
        Some("ORDINARY") => "普通",
        Some("ONE_TIME") => "一次性",
        Some("TODAY_MUST") => "今日必做",
        Some("WARNING") => "预警",
        Some("CONTINUOUS") => "持续",
        Some("MONTHLY") => "月度",
        Some("YEARLY") => "年度",
        _ => "待选择事件类型",
    }
}

fn reminder_html(title: &str, kind: &str, date: &str, time: &str) -> String {
    format!(
        "<!doctype html><html><body style=\"margin:0;background:#e9edf2;color:#17202a;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI','Microsoft YaHei',sans-serif\"><table role=\"presentation\" width=\"100%\" cellspacing=\"0\" cellpadding=\"0\"><tr><td align=\"center\" style=\"padding:28px 12px\"><table role=\"presentation\" width=\"560\" cellspacing=\"0\" cellpadding=\"0\" style=\"max-width:100%;background:#f9fafb;border:1px solid #cbd5df\"><tr><td style=\"border-left:5px solid #7d91a6;padding:18px 22px 14px\"><div style=\"font-size:12px;letter-spacing:.12em;color:#607487\">闪记 · {kind}</div><h1 style=\"margin:10px 0 4px;font-size:22px;line-height:1.45;font-weight:650\">{title}</h1></td></tr><tr><td style=\"padding:18px 22px;border-top:1px solid #d8e0e8\"><div style=\"font-family:Consolas,monospace;font-size:14px;color:#45586a\">计划时间&nbsp; {date} {time}</div><p style=\"margin:20px 0 0;font-size:14px;line-height:1.7;color:#45586a\">请回到闪记完成、稍后提醒或确认已看到。</p></td></tr></table></td></tr></table></body></html>",
        kind = escape_html(kind),
        title = escape_html(title),
        date = escape_html(date),
        time = escape_html(time)
    )
}

fn digest_html(local_date: &str, notifications: &[DueNotification]) -> String {
    let mut rows = String::new();
    for notification in notifications {
        rows.push_str(&format!(
            "<tr><td style=\"padding:13px 0;border-top:1px solid #d8e0e8\"><strong style=\"font-size:15px\">{}</strong><div style=\"margin-top:4px;font-family:Consolas,monospace;font-size:12px;color:#607487\">{} {}</div></td></tr>",
            escape_html(&notification.title),
            escape_html(&notification.due_local_date),
            escape_html(&notification.due_local_time)
        ));
    }
    format!(
        "<!doctype html><html><body style=\"margin:0;background:#e9edf2;color:#17202a;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI','Microsoft YaHei',sans-serif\"><table role=\"presentation\" width=\"100%\" cellspacing=\"0\" cellpadding=\"0\"><tr><td align=\"center\" style=\"padding:28px 12px\"><table role=\"presentation\" width=\"560\" cellspacing=\"0\" cellpadding=\"0\" style=\"max-width:100%;background:#f9fafb;border:1px solid #cbd5df\"><tr><td style=\"border-left:5px solid #2f6f62;padding:18px 22px\"><div style=\"font-size:12px;letter-spacing:.12em;color:#607487\">闪记 · 每日摘要</div><h1 style=\"margin:10px 0 0;font-size:22px\">{} 项待处理</h1><p style=\"margin:6px 0 0;font-family:Consolas,monospace;color:#607487\">{}</p></td></tr><tr><td style=\"padding:5px 22px 18px\"><table role=\"presentation\" width=\"100%\" cellspacing=\"0\" cellpadding=\"0\">{rows}</table><p style=\"margin:18px 0 0;font-size:14px;color:#45586a\">请回到闪记完成、调整或暂停事项。</p></td></tr></table></td></tr></table></body></html>",
        notifications.len(),
        escape_html(local_date)
    )
}

fn test_html() -> String {
    "<!doctype html><html><body style=\"margin:0;background:#e9edf2;color:#17202a;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI','Microsoft YaHei',sans-serif\"><table role=\"presentation\" width=\"100%\"><tr><td align=\"center\" style=\"padding:28px 12px\"><table role=\"presentation\" width=\"560\" style=\"max-width:100%;background:#f9fafb;border:1px solid #cbd5df\"><tr><td style=\"border-left:5px solid #2f6f62;padding:22px\"><div style=\"font-size:12px;letter-spacing:.12em;color:#607487\">闪记 · 连接测试</div><h1 style=\"font-size:22px;margin:10px 0\">邮件设置可以投递</h1><p style=\"font-size:14px;line-height:1.7;color:#45586a\">这封邮件由你主动发送。收到它说明当前邮件服务器已经接受测试消息。</p></td></tr></table></td></tr></table></body></html>".into()
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
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

    #[test]
    fn reminder_html_escapes_user_text_and_has_no_remote_content() {
        let html = reminder_html(
            "季度复盘 <script>alert('x')</script> & 跟进",
            "普通",
            "2026-09-03",
            "18:00",
        );
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("&amp; 跟进"));
        assert!(!html.contains("<script>"));
        assert!(!html.contains("http://"));
        assert!(!html.contains("https://"));
        assert!(!html.contains("<img"));
    }

    #[test]
    fn digest_html_uses_a_table_layout_and_escapes_every_title() {
        let notifications = vec![DueNotification {
            event_id: "event-1".into(),
            item_id: "item-1".into(),
            title: "处理 <合同> & 回函".into(),
            due_local_date: "2026-09-03".into(),
            due_local_time: "17:30".into(),
            completion_policy: "NORMAL".into(),
            event_kind: Some("ORDINARY".into()),
            reminder_plan: "ONCE".into(),
            tag_ids: Vec::new(),
        }];
        let html = digest_html("2026-09-03", &notifications);
        assert!(html.contains("role=\"presentation\""));
        assert!(html.contains("处理 &lt;合同&gt; &amp; 回函"));
        assert!(!html.contains("处理 <合同>"));
    }
}
