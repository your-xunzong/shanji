use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use semver::Version;
use serde::Serialize;
use tauri::{AppHandle, State, ipc::Channel};
use tauri_plugin_updater::{Update, UpdaterExt};
use tokio::sync::Notify;

const GITHUB_MANIFEST: &str =
    "https://github.com/your-xunzong/shanji/releases/latest/download/latest.json";
const GITEE_MANIFEST: &str = "https://gitee.com/wu_-ji/shanji/raw/update/latest.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum UpdateSource {
    Github,
    Gitee,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MirrorStatus {
    Ready,
    SingleSource,
    OutOfSync,
}

impl UpdateSource {
    fn manifest(self) -> &'static str {
        match self {
            Self::Github => GITHUB_MANIFEST,
            Self::Gitee => GITEE_MANIFEST,
        }
    }

    fn trusts_download(self, url: &str) -> bool {
        match self {
            Self::Github => {
                url.starts_with("https://github.com/your-xunzong/shanji/releases/download/")
            }
            Self::Gitee => url.starts_with("https://gitee.com/wu_-ji/shanji/"),
        }
    }
}

#[derive(Clone)]
struct Candidate {
    source: UpdateSource,
    update: Update,
    latency_ms: u64,
}

#[derive(Clone)]
struct PendingUpdate {
    primary: Candidate,
    fallback: Option<Candidate>,
    mirror_status: MirrorStatus,
}

#[derive(Default)]
pub struct AppUpdateManager {
    pending: Mutex<Option<PendingUpdate>>,
    downloaded: Mutex<Option<Vec<u8>>>,
    cancel_requested: AtomicBool,
    cancel_signal: Arc<Notify>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateInfo {
    current_version: String,
    version: String,
    date: Option<String>,
    body: String,
    source: UpdateSource,
    latency_ms: u64,
    fallback_available: bool,
    mirror_status: MirrorStatus,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDownloadEvent {
    event: &'static str,
    chunk_length: Option<usize>,
    content_length: Option<u64>,
    source: UpdateSource,
}

struct Probe {
    source: UpdateSource,
    latency_ms: u64,
    result: Result<Option<Update>, String>,
}

async fn probe(app: &AppHandle, source: UpdateSource) -> Probe {
    let started = Instant::now();
    let result = async {
        let endpoint = source
            .manifest()
            .parse()
            .map_err(|_| "更新地址无效".to_string())?;
        let updater = app
            .updater_builder()
            .endpoints(vec![endpoint])
            .map_err(|_| "更新地址未通过安全校验".to_string())?
            .timeout(Duration::from_secs(8))
            .build()
            .map_err(|_| "更新检查器未能启动".to_string())?;
        let update = tokio::time::timeout(Duration::from_secs(10), updater.check())
            .await
            .map_err(|_| "连接更新源超时".to_string())?
            .map_err(|_| "无法读取更新信息".to_string())?;
        if let Some(update) = &update
            && !source.trusts_download(update.download_url.as_str())
        {
            return Err("更新来源无法验证".into());
        }
        Ok(update)
    }
    .await;
    Probe {
        source,
        latency_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
        result,
    }
}

fn parsed_version(value: &str) -> Result<Version, String> {
    Version::parse(value.trim_start_matches('v')).map_err(|_| "更新版本号无法识别".into())
}

fn select_candidates(probes: [Probe; 2]) -> Result<Option<PendingUpdate>, String> {
    let any_reachable = probes.iter().any(|probe| probe.result.is_ok());
    let mut candidates = probes
        .into_iter()
        .filter_map(|probe| {
            probe.result.ok().flatten().map(|update| Candidate {
                source: probe.source,
                update,
                latency_ms: probe.latency_ms,
            })
        })
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return if any_reachable {
            Ok(None)
        } else {
            Err("GitHub 与 Gitee 暂时都无法连接；当前版本和本机数据没有改变。".into())
        };
    }
    candidates.sort_by(|left, right| {
        parsed_version(&right.update.version)
            .ok()
            .cmp(&parsed_version(&left.update.version).ok())
            .then_with(|| left.latency_ms.cmp(&right.latency_ms))
    });
    let primary = candidates.remove(0);
    parsed_version(&primary.update.version)?;
    let fallback = candidates
        .into_iter()
        .find(|candidate| candidate.update.version == primary.update.version);
    if let Some(candidate) = &fallback
        && candidate.update.signature != primary.update.signature
    {
        let github = if primary.source == UpdateSource::Github {
            primary
        } else if candidate.source == UpdateSource::Github {
            candidate.clone()
        } else {
            return Err("更新镜像签名不一致，且无法确认 GitHub 规范源。".into());
        };
        return Ok(Some(PendingUpdate {
            primary: github,
            fallback: None,
            mirror_status: MirrorStatus::OutOfSync,
        }));
    }
    let mirror_status = if fallback.is_some() {
        MirrorStatus::Ready
    } else {
        MirrorStatus::SingleSource
    };
    Ok(Some(PendingUpdate {
        primary,
        fallback,
        mirror_status,
    }))
}

#[tauri::command]
pub async fn check_app_update(
    app: AppHandle,
    manager: State<'_, AppUpdateManager>,
) -> Result<Option<AppUpdateInfo>, String> {
    manager.cancel_requested.store(false, Ordering::Release);
    *manager.downloaded.lock().expect("update mutex poisoned") = None;
    let (github, gitee) = tokio::join!(
        probe(&app, UpdateSource::Github),
        probe(&app, UpdateSource::Gitee)
    );
    let selected = select_candidates([github, gitee])?;
    let Some(pending) = selected else {
        *manager.pending.lock().expect("update mutex poisoned") = None;
        return Ok(None);
    };
    let info = AppUpdateInfo {
        current_version: pending.primary.update.current_version.clone(),
        version: pending.primary.update.version.clone(),
        date: pending.primary.update.date.map(|date| date.to_string()),
        body: pending.primary.update.body.clone().unwrap_or_default(),
        source: pending.primary.source,
        latency_ms: pending.primary.latency_ms,
        fallback_available: pending.fallback.is_some(),
        mirror_status: pending.mirror_status,
    };
    *manager.pending.lock().expect("update mutex poisoned") = Some(pending);
    Ok(Some(info))
}

async fn download_candidate(
    candidate: &Candidate,
    on_event: &Channel<UpdateDownloadEvent>,
    manager: &AppUpdateManager,
) -> Result<Vec<u8>, String> {
    if manager.cancel_requested.load(Ordering::Acquire) {
        return Err("下载已取消，当前版本没有改变。".into());
    }
    let source = candidate.source;
    let channel = on_event.clone();
    let channel_finished = on_event.clone();
    let mut first_chunk = true;
    let download = candidate.update.download(
        move |chunk_length, content_length| {
            if first_chunk {
                first_chunk = false;
                let _ = channel.send(UpdateDownloadEvent {
                    event: "started",
                    chunk_length: None,
                    content_length,
                    source,
                });
            }
            let _ = channel.send(UpdateDownloadEvent {
                event: "progress",
                chunk_length: Some(chunk_length),
                content_length,
                source,
            });
        },
        move || {
            let _ = channel_finished.send(UpdateDownloadEvent {
                event: "finished",
                chunk_length: None,
                content_length: None,
                source,
            });
        },
    );
    tokio::select! {
        result = download => result.map_err(|_| "更新包下载或签名校验失败".into()),
        _ = manager.cancel_signal.notified() => Err("下载已取消，当前版本没有改变。".into()),
    }
}

#[tauri::command]
pub async fn download_app_update(
    on_event: Channel<UpdateDownloadEvent>,
    manager: State<'_, AppUpdateManager>,
) -> Result<(), String> {
    let pending = manager
        .pending
        .lock()
        .expect("update mutex poisoned")
        .clone()
        .ok_or_else(|| "请重新检查更新后再下载。".to_string())?;
    manager.cancel_requested.store(false, Ordering::Release);
    let bytes = match download_candidate(&pending.primary, &on_event, &manager).await {
        Ok(bytes) => bytes,
        Err(error) if !manager.cancel_requested.load(Ordering::Acquire) => {
            let fallback = pending.fallback.as_ref().ok_or(error)?;
            download_candidate(fallback, &on_event, &manager).await?
        }
        Err(error) => return Err(error),
    };
    *manager.downloaded.lock().expect("update mutex poisoned") = Some(bytes);
    Ok(())
}

#[tauri::command]
pub fn cancel_app_update(manager: State<'_, AppUpdateManager>) {
    manager.cancel_requested.store(true, Ordering::Release);
    *manager.downloaded.lock().expect("update mutex poisoned") = None;
    manager.cancel_signal.notify_waiters();
}

#[tauri::command]
pub fn install_app_update(manager: State<'_, AppUpdateManager>) -> Result<(), String> {
    let pending = manager
        .pending
        .lock()
        .expect("update mutex poisoned")
        .clone()
        .ok_or_else(|| "请重新检查更新后再安装。".to_string())?;
    let bytes = manager
        .downloaded
        .lock()
        .expect("update mutex poisoned")
        .take()
        .ok_or_else(|| "更新包还没有下载完成。".to_string())?;
    pending
        .primary
        .update
        .install(&bytes)
        .map_err(|_| "更新没有安装，当前版本和本机数据仍然可用。".into())
}

#[tauri::command]
pub fn clear_pending_app_update(manager: State<'_, AppUpdateManager>) {
    manager.cancel_requested.store(true, Ordering::Release);
    manager.cancel_signal.notify_waiters();
    *manager.pending.lock().expect("update mutex poisoned") = None;
    *manager.downloaded.lock().expect("update mutex poisoned") = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trusted_downloads_stay_within_the_selected_repository() {
        assert!(UpdateSource::Github.trusts_download(
            "https://github.com/your-xunzong/shanji/releases/download/v0.14.0/shanji.exe"
        ));
        assert!(UpdateSource::Gitee.trusts_download(
            "https://gitee.com/wu_-ji/shanji/releases/download/v0.14.0/shanji.exe"
        ));
        assert!(!UpdateSource::Gitee.trusts_download("https://example.com/shanji.exe"));
    }
}
