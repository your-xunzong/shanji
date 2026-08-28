use std::{
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};

use tauri::{AppHandle, Emitter};

use crate::{db::Database, domain::Clock, notification::NotificationService};

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
                            process_once(&app, &database, clock.as_ref(), &notifications);
                            next_poll = Instant::now() + poll_interval;
                        }
                        Ok(SchedulerSignal::WakeAfter(delay)) => {
                            delayed_wakes.push(Instant::now() + delay);
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            let now = Instant::now();
                            delayed_wakes.retain(|deadline| *deadline > now);
                            process_once(&app, &database, clock.as_ref(), &notifications);
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
) {
    let now = clock.now_utc();
    match database.process_due(now) {
        Ok(result) => {
            for notification in result.notifications {
                if !result.notifications_enabled {
                    if let Err(error) =
                        database.mark_notification_disabled(&notification.event_id, now)
                    {
                        eprintln!("通知关闭状态记录失败：{error}");
                    }
                    continue;
                }

                match notifications.send(&notification) {
                    Ok(()) => {
                        if let Err(error) =
                            database.mark_notification_submitted(&notification.event_id, now)
                        {
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
            if result.changed {
                let _ = app.emit("items_changed", ());
            }
        }
        Err(error) => {
            eprintln!("提醒调度暂时失败：{error}");
        }
    }
}
