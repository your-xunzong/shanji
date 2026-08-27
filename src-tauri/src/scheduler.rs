use std::{
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};

use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;

use crate::{db::Database, domain::Clock};

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
    pub fn start(app: AppHandle, database: Arc<Database>, clock: Arc<dyn Clock>) -> Self {
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
                            process_once(&app, &database, clock.as_ref());
                            next_poll = Instant::now() + poll_interval;
                        }
                        Ok(SchedulerSignal::WakeAfter(delay)) => {
                            delayed_wakes.push(Instant::now() + delay);
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            let now = Instant::now();
                            delayed_wakes.retain(|deadline| *deadline > now);
                            process_once(&app, &database, clock.as_ref());
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

fn process_once(app: &AppHandle, database: &Database, clock: &dyn Clock) {
    match database.process_due(clock.now_utc()) {
        Ok(result) => {
            if result.notifications_enabled {
                for notification in result.notifications {
                    if let Err(error) = app
                        .notification()
                        .builder()
                        .title("闪记提醒")
                        .body(notification.title)
                        .show()
                    {
                        eprintln!("系统通知发送失败：{error}");
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
