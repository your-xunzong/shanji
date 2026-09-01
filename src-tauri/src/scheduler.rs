use std::{
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};

use tauri::{AppHandle, Emitter};

use crate::{
    db::{Database, EmailDeliveryJob},
    domain::{Clock, Settings},
    mail::MailService,
    notification::NotificationService,
};

#[derive(Debug, Clone, Copy)]
enum SchedulerSignal {
    Wake,
    WakeAfter(Duration),
    Stop,
}

#[derive(Clone)]
pub struct SchedulerHandle {
    sender: mpsc::Sender<SchedulerSignal>,
}

impl SchedulerHandle {
    pub fn start(
        app: AppHandle,
        database: Arc<Database>,
        clock: Arc<dyn Clock>,
        notifications: Arc<NotificationService>,
        mail: Arc<MailService>,
    ) -> Self {
        let (sender, receiver) = mpsc::channel();
        thread::Builder::new()
            .name("shanji-reminder-scheduler".into())
            .spawn(move || {
                let poll_interval = Duration::from_secs(30);
                let mut next_poll = Instant::now() + poll_interval;
                let mut delayed_wakes = Vec::<Instant>::new();

                loop {
                    let now = Instant::now();
                    let next_delayed = delayed_wakes.iter().copied().min();
                    let next_deadline = next_delayed
                        .map(|deadline| deadline.min(next_poll))
                        .unwrap_or(next_poll);
                    let wait = next_deadline.saturating_duration_since(now);

                    match receiver.recv_timeout(wait) {
                        Ok(SchedulerSignal::Stop) => break,
                        Ok(SchedulerSignal::Wake) => {
                            process_once(&app, &database, clock.as_ref(), &notifications, &mail);
                            next_poll = Instant::now() + poll_interval;
                        }
                        Ok(SchedulerSignal::WakeAfter(delay)) => {
                            delayed_wakes.push(Instant::now() + delay);
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            let now = Instant::now();
                            delayed_wakes.retain(|deadline| *deadline > now);
                            process_once(&app, &database, clock.as_ref(), &notifications, &mail);
                            if now >= next_poll {
                                next_poll = now + poll_interval;
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
            })
            .expect("failed to start reminder scheduler");

        let handle = Self { sender };
        handle.wake();
        handle
    }

    pub fn wake(&self) {
        let _ = self.sender.send(SchedulerSignal::Wake);
    }

    pub fn wake_after(&self, delay: Duration) {
        let _ = self.sender.send(SchedulerSignal::WakeAfter(delay));
    }

    pub fn stop(&self) {
        let _ = self.sender.send(SchedulerSignal::Stop);
    }
}

fn process_once(
    app: &AppHandle,
    database: &Database,
    clock: &dyn Clock,
    notifications: &NotificationService,
    mail: &MailService,
) {
    let now = clock.now_utc();
    match database.process_due(now) {
        Ok(result) => {
            let settings = match database.get_settings() {
                Ok(settings) => settings,
                Err(error) => {
                    eprintln!("提醒设置读取失败：{error}");
                    return;
                }
            };
            if settings.smtp_enabled {
                match database.claim_due_email_retries(now) {
                    Ok(jobs) => {
                        for job in jobs {
                            deliver_email(database, mail, &settings, job, now);
                        }
                    }
                    Err(error) => eprintln!("邮件重试领取失败：{error}"),
                }
            }
            for notification in result.notifications {
                if settings.overlay_reminders_enabled {
                    match crate::show_overlay_reminder(app, &notification) {
                        Ok(()) => {
                            if let Err(error) = database.record_overlay_delivery(&notification, now)
                            {
                                eprintln!("置顶提醒投递记录失败：{error}");
                            }
                        }
                        Err(error) => eprintln!("置顶提醒窗口暂时无法显示：{error}"),
                    }
                }
                if settings.smtp_enabled {
                    match database.claim_email_delivery(
                        &notification,
                        settings.smtp_repeat_must_complete,
                        now,
                    ) {
                        Ok(Some(job)) => deliver_email(database, mail, &settings, job, now),
                        Ok(None) => {}
                        Err(error) => eprintln!("邮件提醒领取失败：{error}"),
                    }
                }
                if !result.notifications_enabled {
                    if let Err(error) =
                        database.mark_notification_disabled(&notification.event_id, now)
                    {
                        eprintln!("通知关闭状态记录失败：{error}");
                    }
                    continue;
                }

                let persistent = should_use_persistent_notification(&settings, &notification);
                match notifications.send(&notification, persistent) {
                    Ok(note) => {
                        if let Err(error) = database.mark_notification_submitted_with_note(
                            &notification.event_id,
                            now,
                            note,
                        ) {
                            eprintln!("通知提交结果记录失败：{error}");
                        }
                    }
                    Err(error) => {
                        eprintln!("系统通知提交失败（{}）：{}", error.code, error.diagnostic);
                        if let Err(database_error) = database.mark_notification_failed(
                            &notification.event_id,
                            error.code,
                            now,
                        ) {
                            eprintln!("通知失败结果记录失败：{database_error}");
                        }
                    }
                }
            }
            for notification in result.classification_notifications {
                if !result.notifications_enabled {
                    if let Err(error) = database.mark_classification_delivery(
                        &notification.event_ids,
                        "DISABLED",
                        Some("notifications_disabled"),
                        now,
                    ) {
                        eprintln!("待选类型通知关闭状态记录失败：{error}");
                    }
                    continue;
                }
                match notifications.send_classification(&notification) {
                    Ok(()) => {
                        if let Err(error) = database.mark_classification_delivery(
                            &notification.event_ids,
                            "SUBMITTED",
                            None,
                            now,
                        ) {
                            eprintln!("待选类型通知结果记录失败：{error}");
                        }
                    }
                    Err(error) => {
                        eprintln!(
                            "待选类型通知提交失败（{}）：{}",
                            error.code, error.diagnostic
                        );
                        if let Err(database_error) = database.mark_classification_delivery(
                            &notification.event_ids,
                            "FAILED",
                            Some(error.code),
                            now,
                        ) {
                            eprintln!("待选类型通知失败结果记录失败：{database_error}");
                        }
                    }
                }
            }
            if result.changed {
                let _ = app.emit("items_changed", ());
                let _ = app.emit("reminder_center_changed", ());
                crate::update_tray_reminder_count(app, database, now);
            }
        }
        Err(error) => {
            eprintln!("提醒调度暂时失败：{error}");
        }
    }
}

fn should_use_persistent_notification(
    settings: &Settings,
    notification: &crate::db::DueNotification,
) -> bool {
    settings.persistent_notifications_enabled
        && (notification.completion_policy == "MUST_COMPLETE_TODAY"
            || notification.reminder_plan == "FORCE")
}

fn deliver_email(
    database: &Database,
    mail: &MailService,
    settings: &Settings,
    job: EmailDeliveryJob,
    now: chrono::DateTime<chrono::Utc>,
) {
    match mail.send_reminder(settings, &job.notification) {
        Ok(()) => {
            if let Err(error) = database.mark_email_delivery_submitted(&job.delivery_id, now) {
                eprintln!("邮件投递结果记录失败：{error}");
            }
        }
        Err(error) => {
            eprintln!("邮件投递失败（{}）", error.code);
            if let Err(database_error) =
                database.mark_email_delivery_failed(&job.delivery_id, error.code, now)
            {
                eprintln!("邮件失败结果记录失败：{database_error}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::DueNotification;

    fn settings() -> Settings {
        Settings {
            default_due_time: "18:00".into(),
            workdays: vec![1, 2, 3, 4, 5],
            overtime_interval_minutes: 30,
            quiet_hours_enabled: true,
            quiet_start: "22:30".into(),
            quiet_end: "07:30".into(),
            global_shortcut: "CommandOrControl+Shift+Space".into(),
            notifications_enabled: true,
            autostart_enabled: false,
            persistent_notifications_enabled: true,
            overlay_reminders_enabled: false,
            repeat_unacknowledged_enabled: false,
            unacknowledged_repeat_minutes: 60,
            smtp_enabled: false,
            smtp_host: String::new(),
            smtp_port: 465,
            smtp_security: "tls".into(),
            smtp_from: String::new(),
            smtp_to: String::new(),
            smtp_username: String::new(),
            smtp_repeat_must_complete: false,
            event_kind_defaults: crate::domain::default_event_kind_defaults(),
        }
    }

    fn notification(policy: &str) -> DueNotification {
        DueNotification {
            event_id: "event".into(),
            item_id: "item".into(),
            title: "测试".into(),
            due_local_date: "2026-08-29".into(),
            due_local_time: "18:00".into(),
            completion_policy: policy.into(),
            event_kind: (policy == "MUST_COMPLETE_TODAY").then(|| "TODAY_MUST".into()),
            reminder_plan: if policy == "MUST_COMPLETE_TODAY" {
                "EMPHASIS".into()
            } else {
                "ONCE".into()
            },
            tag_ids: Vec::new(),
        }
    }

    #[test]
    fn persistent_native_mode_supports_today_must_and_force_plan() {
        let mut settings = settings();
        assert!(should_use_persistent_notification(
            &settings,
            &notification("MUST_COMPLETE_TODAY")
        ));
        assert!(!should_use_persistent_notification(
            &settings,
            &notification("NORMAL")
        ));
        let mut forced = notification("NORMAL");
        forced.reminder_plan = "FORCE".into();
        assert!(should_use_persistent_notification(&settings, &forced));
        settings.persistent_notifications_enabled = false;
        assert!(!should_use_persistent_notification(
            &settings,
            &notification("MUST_COMPLETE_TODAY")
        ));
    }
}
