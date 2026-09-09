use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::Serialize;

use crate::{
    db::{
        DataFileSummary, LATEST_SCHEMA_VERSION, inspect_database_file,
        summary_has_recoverable_content,
    },
    error::{AppError, AppResult},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupMode {
    Ready,
    RecoveryRequired,
    Blocked,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupStatus {
    pub mode: StartupMode,
    pub target_path: String,
    pub message: String,
    pub current: Option<DataFileSummary>,
    pub recovery_candidates: Vec<DataFileSummary>,
    pub diagnostic: String,
}

pub struct StartupState {
    pub status: StartupStatus,
    pub target_path: PathBuf,
    pub candidate_paths: Vec<PathBuf>,
}

impl StartupState {
    pub fn ready(target_path: PathBuf) -> Self {
        Self {
            status: StartupStatus {
                mode: StartupMode::Ready,
                target_path: target_path.to_string_lossy().into_owned(),
                message: String::new(),
                current: None,
                recovery_candidates: Vec::new(),
                diagnostic: format!("startup_state=ready; app_schema={LATEST_SCHEMA_VERSION}"),
            },
            target_path,
            candidate_paths: Vec::new(),
        }
    }
}

pub fn assess_database_startup(target_path: &Path, candidate_paths: &[PathBuf]) -> StartupStatus {
    let target_exists = target_path.is_file();
    let target_bytes = fs::metadata(target_path)
        .map(|value| value.len())
        .unwrap_or(0);

    if !target_exists || target_bytes == 0 {
        let recovery_candidates = valid_recovery_candidates(target_path, candidate_paths);
        if recovery_candidates.is_empty() {
            return ready_status(target_path, None, target_exists, target_bytes);
        }
        return status(
            StartupMode::RecoveryRequired,
            target_path,
            "当前数据位置没有记录，但找到了以前保存的内容。恢复前不会写入空数据。",
            None,
            recovery_candidates,
            "legacy_data_found",
            target_exists,
            target_bytes,
        );
    }

    match inspect_database_file(target_path) {
        Ok(current) if current.schema_version > LATEST_SCHEMA_VERSION => {
            let recovery_candidates = valid_recovery_candidates(target_path, candidate_paths);
            status(
                StartupMode::Blocked,
                target_path,
                "原数据来自更新版本，当前版本没有写入。请升级闪记，或恢复下方兼容的备份。",
                Some(current),
                recovery_candidates,
                "newer_schema",
                true,
                target_bytes,
            )
        }
        Ok(current) if current.item_count == 0 => {
            let recovery_candidates = valid_recovery_candidates(target_path, candidate_paths);
            if recovery_candidates.is_empty() {
                ready_status(target_path, Some(current), true, target_bytes)
            } else {
                status(
                    StartupMode::RecoveryRequired,
                    target_path,
                    "当前数据位置没有事项，但找到了以前保存的内容。当前文件会在恢复前保留，两份数据不会自动合并。",
                    Some(current),
                    recovery_candidates,
                    "empty_target_with_legacy_data",
                    true,
                    target_bytes,
                )
            }
        }
        Ok(current) => ready_status(target_path, Some(current), true, target_bytes),
        Err(error) => {
            let recovery_candidates = valid_recovery_candidates(target_path, candidate_paths);
            blocked_status(
                target_path,
                recovery_candidates,
                None,
                &error,
                true,
                target_bytes,
            )
        }
    }
}

pub fn blocked_status(
    target_path: &Path,
    recovery_candidates: Vec<DataFileSummary>,
    current: Option<DataFileSummary>,
    error: &AppError,
    target_exists: bool,
    target_bytes: u64,
) -> StartupStatus {
    let issue = error_kind(error);
    let message = match issue {
        "permission_denied" | "database_read_only" => {
            "原数据仍在，但当前位置暂时不能安全写入。闪记已停止启动，请检查文件权限后重试。"
        }
        "disk_full" => "原数据仍在，但可用空间不足，无法安全完成备份或升级。释放空间后可以重试。",
        _ => "原数据仍在，但当前版本暂时无法确认它可以安全读取。闪记已停止写入。",
    };
    status(
        StartupMode::Blocked,
        target_path,
        message,
        current,
        recovery_candidates,
        issue,
        target_exists,
        target_bytes,
    )
}

pub fn valid_recovery_candidates(
    target_path: &Path,
    candidate_paths: &[PathBuf],
) -> Vec<DataFileSummary> {
    let mut candidates = candidate_paths
        .iter()
        .filter(|path| path.as_path() != target_path)
        .filter_map(|path| inspect_database_file(path).ok())
        .filter(|summary| {
            summary.schema_version <= LATEST_SCHEMA_VERSION
                && summary_has_recoverable_content(summary)
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    candidates.dedup_by(|left, right| left.path == right.path);
    candidates
}

fn ready_status(
    target_path: &Path,
    current: Option<DataFileSummary>,
    target_exists: bool,
    target_bytes: u64,
) -> StartupStatus {
    status(
        StartupMode::Ready,
        target_path,
        "",
        current,
        Vec::new(),
        "ready",
        target_exists,
        target_bytes,
    )
}

#[allow(clippy::too_many_arguments)]
fn status(
    mode: StartupMode,
    target_path: &Path,
    message: &str,
    current: Option<DataFileSummary>,
    recovery_candidates: Vec<DataFileSummary>,
    issue: &str,
    target_exists: bool,
    target_bytes: u64,
) -> StartupStatus {
    let current_schema = current
        .as_ref()
        .map(|summary| summary.schema_version.to_string())
        .unwrap_or_else(|| "unknown".into());
    StartupStatus {
        mode,
        target_path: target_path.to_string_lossy().into_owned(),
        message: message.into(),
        current,
        diagnostic: format!(
            "startup_state={issue}; target_exists={target_exists}; target_bytes={target_bytes}; current_schema={current_schema}; candidates={}; app_schema={LATEST_SCHEMA_VERSION}",
            recovery_candidates.len()
        ),
        recovery_candidates,
    }
}

fn error_kind(error: &AppError) -> &'static str {
    match error {
        AppError::Io(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            "permission_denied"
        }
        AppError::Io(error) if error.kind() == std::io::ErrorKind::StorageFull => "disk_full",
        AppError::Io(error) if error.raw_os_error() == Some(112) => "disk_full",
        AppError::Database(error) => match error.sqlite_error_code() {
            Some(rusqlite::ErrorCode::ReadOnly) => "database_read_only",
            Some(rusqlite::ErrorCode::DiskFull) => "disk_full",
            Some(rusqlite::ErrorCode::DatabaseCorrupt) => "database_corrupt",
            Some(rusqlite::ErrorCode::CannotOpen) => "database_unavailable",
            _ => "database_error",
        },
        AppError::Validation(_) => "validation_failed",
        AppError::SettingsData(_) => "settings_invalid",
        AppError::Io(_) => "file_error",
        AppError::SystemIntegration(_) => "system_error",
        AppError::ItemNotFound => "unexpected_item_error",
    }
}

pub fn startup_state(
    status: StartupStatus,
    target_path: PathBuf,
    candidate_paths: Vec<PathBuf>,
) -> StartupState {
    StartupState {
        target_path,
        status,
        candidate_paths,
    }
}

pub fn ensure_recovery_candidate_allowed(state: &StartupState, requested: &Path) -> AppResult<()> {
    if state
        .candidate_paths
        .iter()
        .any(|candidate| candidate == requested)
        && state
            .status
            .recovery_candidates
            .iter()
            .any(|candidate| Path::new(&candidate.path) == requested)
    {
        Ok(())
    } else {
        Err(AppError::Validation(
            "这份数据不在闪记已校验的恢复位置中，请重新检查后选择".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db::Database, domain::CreateItemInput};
    use chrono::{Duration, Utc};
    use tempfile::tempdir;

    fn database_with_items(path: &Path, count: usize) {
        let database = Database::open(path).unwrap();
        let now = Utc::now();
        for index in 0..count {
            database
                .create_item(
                    &CreateItemInput {
                        title: format!("恢复测试 {index}"),
                        notes: String::new(),
                        category_id: None,
                        due_at: Some((now + Duration::hours(1)).to_rfc3339()),
                        must_complete_today: false,
                        repeat_interval_minutes: None,
                        tag_ids: Vec::new(),
                        event: None,
                    },
                    now,
                )
                .unwrap();
        }
    }

    #[test]
    fn missing_target_with_old_data_requires_recovery() {
        let directory = tempdir().unwrap();
        let target = directory.path().join("current/shanji.db");
        let candidate = directory.path().join("old/shanji.db");
        database_with_items(&candidate, 2);

        let status = assess_database_startup(&target, &[candidate]);

        assert_eq!(status.mode, StartupMode::RecoveryRequired);
        assert_eq!(status.recovery_candidates[0].item_count, 2);
        assert!(!target.exists());
    }

    #[test]
    fn empty_target_never_wins_over_non_empty_old_data() {
        let directory = tempdir().unwrap();
        let target = directory.path().join("current.db");
        Database::open(&target).unwrap();
        let candidate = directory.path().join("old.db");
        database_with_items(&candidate, 1);

        let status = assess_database_startup(&target, &[candidate]);

        assert_eq!(status.mode, StartupMode::RecoveryRequired);
        assert_eq!(status.current.unwrap().item_count, 0);
    }

    #[test]
    fn old_draft_is_recoverable_even_without_items() {
        let directory = tempdir().unwrap();
        let target = directory.path().join("current.db");
        let candidate = directory.path().join("old.db");
        let database = Database::open(&candidate).unwrap();
        database.save_draft("尚未提交的内容", Utc::now()).unwrap();
        drop(database);

        let status = assess_database_startup(&target, &[candidate]);

        assert_eq!(status.mode, StartupMode::RecoveryRequired);
        assert_eq!(status.recovery_candidates[0].item_count, 0);
        assert!(status.recovery_candidates[0].has_draft);
    }

    #[test]
    fn completed_settings_are_recoverable_without_items() {
        let directory = tempdir().unwrap();
        let target = directory.path().join("current.db");
        let candidate = directory.path().join("old.db");
        let database = Database::open(&candidate).unwrap();
        database.complete_onboarding(1).unwrap();
        drop(database);

        let status = assess_database_startup(&target, &[candidate]);

        assert_eq!(status.mode, StartupMode::RecoveryRequired);
        assert!(status.recovery_candidates[0].has_custom_settings);
    }

    #[test]
    fn current_draft_does_not_hide_an_old_database_with_items() {
        let directory = tempdir().unwrap();
        let target = directory.path().join("current.db");
        let current = Database::open(&target).unwrap();
        current.save_draft("新位置里的草稿", Utc::now()).unwrap();
        drop(current);
        let candidate = directory.path().join("old.db");
        database_with_items(&candidate, 3);

        let status = assess_database_startup(&target, &[candidate]);

        assert_eq!(status.mode, StartupMode::RecoveryRequired);
        assert!(status.current.unwrap().has_draft);
        assert_eq!(status.recovery_candidates[0].item_count, 3);
    }

    #[test]
    fn unused_empty_database_is_not_presented_as_old_user_data() {
        let directory = tempdir().unwrap();
        let target = directory.path().join("current.db");
        let candidate = directory.path().join("unused.db");
        Database::open(&candidate).unwrap();

        let status = assess_database_startup(&target, &[candidate]);

        assert_eq!(status.mode, StartupMode::Ready);
        assert!(status.recovery_candidates.is_empty());
    }

    #[test]
    fn non_empty_current_database_starts_normally() {
        let directory = tempdir().unwrap();
        let target = directory.path().join("current.db");
        database_with_items(&target, 1);
        let candidate = directory.path().join("old.db");
        database_with_items(&candidate, 2);

        let status = assess_database_startup(&target, &[candidate]);

        assert_eq!(status.mode, StartupMode::Ready);
        assert!(status.recovery_candidates.is_empty());
    }

    #[test]
    fn unreadable_target_blocks_writes() {
        let directory = tempdir().unwrap();
        let target = directory.path().join("shanji.db");
        fs::write(&target, b"not a sqlite database").unwrap();

        let status = assess_database_startup(&target, &[]);

        assert_eq!(status.mode, StartupMode::Blocked);
        assert!(status.message.contains("停止写入"));
    }

    #[test]
    fn database_from_a_newer_app_version_is_never_opened_for_writing() {
        let directory = tempdir().unwrap();
        let target = directory.path().join("shanji.db");
        database_with_items(&target, 1);
        let connection = rusqlite::Connection::open(&target).unwrap();
        connection
            .execute(
                "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                (LATEST_SCHEMA_VERSION + 1, Utc::now().to_rfc3339()),
            )
            .unwrap();
        drop(connection);

        let before = fs::metadata(&target).unwrap().len();
        let status = assess_database_startup(&target, &[]);

        assert_eq!(status.mode, StartupMode::Blocked);
        assert!(status.message.contains("更新版本"));
        assert_eq!(fs::metadata(&target).unwrap().len(), before);
    }
}
