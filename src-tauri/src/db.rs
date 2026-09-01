use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::SystemTime,
};

use chrono::{DateTime, Datelike, Local, NaiveDate, Utc};
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    domain::{
        Category, CreateItemInput, EventConfigurationInput, EventKindDefault, Item, Settings, Tag,
        TaxonomyInput, UpdateItemInput, UpdateSettingsInput, due_for_local_date, due_from_explicit,
        must_complete_due, next_daily_at, next_default_due, next_repeat_at, next_repeat_slot_after,
        parse_time, shift_calendar,
    },
    error::{AppError, AppResult},
};

const MIGRATION_0001: &str = include_str!("../migrations/0001_init.sql");
const MIGRATION_0002: &str = include_str!("../migrations/0002_notification_delivery.sql");
const MIGRATION_0003: &str = include_str!("../migrations/0003_autostart_onboarding.sql");
const MIGRATION_0004: &str = include_str!("../migrations/0004_types_tags_recycle.sql");
const MIGRATION_0005: &str = include_str!("../migrations/0005_reminder_channels.sql");
const MIGRATION_0006: &str = include_str!("../migrations/0006_event_kinds.sql");
const MIGRATION_0007: &str = include_str!("../migrations/0007_repeat_time_slots.sql");
const LATEST_SCHEMA_VERSION: i64 = 7;

pub struct Database {
    connection: Mutex<Connection>,
    path: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataFileSummary {
    pub path: String,
    pub item_count: u64,
    pub schema_version: i64,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataStatus {
    pub current: DataFileSummary,
    pub latest_backup: Option<DataFileSummary>,
    pub recovery_candidates: Vec<DataFileSummary>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DueNotification {
    pub event_id: String,
    pub item_id: String,
    pub title: String,
    pub due_local_date: String,
    pub due_local_time: String,
    pub completion_policy: String,
    pub event_kind: Option<String>,
    pub reminder_plan: String,
    pub tag_ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ClassificationNotification {
    pub event_ids: Vec<String>,
    pub item_id: Option<String>,
    pub count: usize,
}

#[derive(Debug, Clone)]
pub struct NotificationDelivery {
    pub result: String,
    pub error_code: Option<String>,
    pub attempted_at: String,
}

#[derive(Debug, Clone)]
pub struct EmailDeliveryJob {
    pub delivery_id: String,
    pub notification: DueNotification,
}

#[derive(Debug, Default)]
pub struct ProcessResult {
    pub notifications: Vec<DueNotification>,
    pub classification_notifications: Vec<ClassificationNotification>,
    pub changed: bool,
    pub notifications_enabled: bool,
}

struct DueRow {
    id: String,
    title: String,
    completion_policy: String,
    reminder_plan: String,
    event_kind: Option<String>,
    repeat_interval_minutes: Option<u32>,
    scheduled_for: String,
    bypass_quiet_hours: bool,
    revision: u32,
    due_local_date: String,
    due_local_time: String,
    end_at: Option<String>,
    target_at: Option<String>,
    cadence_value: Option<u32>,
    cadence_unit: Option<String>,
    emphasis_max_per_day: u32,
    emphasis_count: u32,
    emphasis_count_local_date: Option<String>,
    series_anchor_month: Option<u32>,
    series_anchor_day: Option<u32>,
    repeat_time_first: Option<String>,
    repeat_time_second: Option<String>,
}

pub fn apply_pending_restore(database_path: &Path) -> AppResult<()> {
    let staging = pending_restore_path(database_path)?;
    if !staging.exists() {
        return Ok(());
    }
    inspect_database_file(&staging)?;
    let parent = database_path
        .parent()
        .ok_or_else(|| AppError::Validation("数据目录无效".into()))?;
    fs::create_dir_all(parent)?;

    if database_path.exists() {
        let timestamp = Utc::now().format("%Y%m%d-%H%M%S-%3f");
        let previous = parent.join(format!("shanji-before-restore-{timestamp}.db"));
        fs::rename(database_path, previous)?;
    }
    for suffix in ["db-wal", "db-shm"] {
        let sidecar = database_path.with_extension(suffix);
        if sidecar.exists() {
            fs::remove_file(sidecar)?;
        }
    }
    fs::rename(staging, database_path)?;
    Ok(())
}

impl Database {
    pub fn open(path: &Path) -> AppResult<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let existed = path.exists() && fs::metadata(path)?.len() > 0;
        let mut connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;",
        )?;

        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL
            );",
        )?;
        let version = connection.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get::<_, i64>(0),
        )?;

        if existed && version < LATEST_SCHEMA_VERSION {
            let backup = create_upgrade_backup(&connection, path, version)?;
            let legacy_backup = path.with_extension("db.backup-before-v3");
            if !legacy_backup.exists() {
                fs::copy(&backup, legacy_backup)?;
            }
        }

        if version < 1 {
            let transaction = connection.transaction()?;
            transaction.execute_batch(MIGRATION_0001)?;
            transaction.commit()?;
        }
        if version < 2 {
            let transaction = connection.transaction()?;
            transaction.execute_batch(MIGRATION_0002)?;
            transaction.commit()?;
        }
        if version < 3 {
            let transaction = connection.transaction()?;
            transaction.execute_batch(MIGRATION_0003)?;
            transaction.commit()?;
        }
        if version < 4 {
            let transaction = connection.transaction()?;
            transaction.execute_batch(MIGRATION_0004)?;
            transaction.commit()?;
        }
        if version < 5 {
            let transaction = connection.transaction()?;
            transaction.execute_batch(MIGRATION_0005)?;
            transaction.commit()?;
        }
        if version < 6 {
            let transaction = connection.transaction()?;
            transaction.execute_batch(MIGRATION_0006)?;
            transaction.commit()?;
        }
        if version < 7 {
            let transaction = connection.transaction()?;
            transaction.execute_batch(MIGRATION_0007)?;
            transaction.commit()?;
        }

        Ok(Self {
            connection: Mutex::new(connection),
            path: path.to_path_buf(),
        })
    }

    #[cfg(test)]
    pub fn in_memory() -> AppResult<Self> {
        let mut connection = Connection::open_in_memory()?;
        connection.execute_batch("PRAGMA foreign_keys = ON;")?;
        let transaction = connection.transaction()?;
        transaction.execute_batch(MIGRATION_0001)?;
        transaction.execute_batch(MIGRATION_0002)?;
        transaction.execute_batch(MIGRATION_0003)?;
        transaction.execute_batch(MIGRATION_0004)?;
        transaction.execute_batch(MIGRATION_0005)?;
        transaction.execute_batch(MIGRATION_0006)?;
        transaction.execute_batch(MIGRATION_0007)?;
        transaction.commit()?;
        Ok(Self {
            connection: Mutex::new(connection),
            path: PathBuf::from(":memory:"),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn data_status(&self, candidates: &[PathBuf]) -> AppResult<DataStatus> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        let current = summarize_connection(&connection, &self.path)?;
        drop(connection);

        let backup_directory = backup_directory(&self.path)?;
        let latest_backup = latest_database_file(&backup_directory)
            .and_then(|path| inspect_database_file(&path).ok());
        let mut recovery_candidates = candidates
            .iter()
            .filter(|path| path.as_path() != self.path)
            .filter_map(|path| inspect_database_file(path).ok())
            .filter(|summary| summary.item_count > 0)
            .collect::<Vec<_>>();
        recovery_candidates.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        recovery_candidates.dedup_by(|left, right| left.path == right.path);

        Ok(DataStatus {
            current,
            latest_backup,
            recovery_candidates,
        })
    }

    pub fn create_backup(&self) -> AppResult<DataFileSummary> {
        if self.path == Path::new(":memory:") {
            return Err(AppError::Validation("内存数据库不能创建文件备份".into()));
        }
        let connection = self.connection.lock().expect("database mutex poisoned");
        let version = schema_version(&connection)?;
        let backup = create_timestamped_backup(&connection, &self.path, version, "manual")?;
        inspect_database_file(&backup)
    }

    pub fn stage_restore(&self, candidate: &Path) -> AppResult<()> {
        if candidate == self.path {
            return Err(AppError::Validation("当前数据无需恢复".into()));
        }
        let summary = inspect_database_file(candidate)?;
        if summary.item_count == 0 {
            return Err(AppError::Validation("所选数据中没有可恢复的事项".into()));
        }
        if summary.schema_version > LATEST_SCHEMA_VERSION {
            return Err(AppError::Validation(
                "这份数据来自更高版本的闪记，请先升级应用后再恢复".into(),
            ));
        }

        self.create_backup()?;
        let staging = pending_restore_path(&self.path)?;
        if staging.exists() {
            fs::remove_file(&staging)?;
        }
        let source =
            Connection::open_with_flags(candidate, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        source.execute("VACUUM INTO ?1", [staging.to_string_lossy().as_ref()])?;
        inspect_database_file(&staging)?;
        Ok(())
    }

    pub fn get_settings(&self) -> AppResult<Settings> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        read_settings(&connection)
    }

    pub fn update_settings(
        &self,
        input: &UpdateSettingsInput,
        now: DateTime<Utc>,
    ) -> AppResult<Settings> {
        input.validate()?;
        let settings = input.settings();
        let mut connection = self.connection.lock().expect("database mutex poisoned");
        let transaction = connection.transaction()?;

        transaction.execute(
            "UPDATE app_settings SET
                default_due_time = ?1,
                workdays = ?2,
                overtime_interval_minutes = ?3,
                quiet_hours_enabled = ?4,
                quiet_start = ?5,
                quiet_end = ?6,
                global_shortcut = ?7,
                notifications_enabled = ?8,
                autostart_enabled = ?9,
                persistent_notifications_enabled = ?10,
                overlay_reminders_enabled = ?11,
                repeat_unacknowledged_enabled = ?12,
                unacknowledged_repeat_minutes = ?13,
                smtp_enabled = ?14,
                smtp_host = ?15,
                smtp_port = ?16,
                smtp_security = ?17,
                smtp_from = ?18,
                smtp_to = ?19,
                smtp_username = ?20,
                smtp_repeat_must_complete = ?21,
                repeat_default_time_first = ?22,
                repeat_default_time_second = ?23
             WHERE id = 1",
            params![
                settings.default_due_time,
                serde_json::to_string(&settings.workdays)?,
                settings.overtime_interval_minutes,
                bool_to_int(settings.quiet_hours_enabled),
                settings.quiet_start,
                settings.quiet_end,
                settings.global_shortcut,
                bool_to_int(settings.notifications_enabled),
                bool_to_int(settings.autostart_enabled),
                bool_to_int(settings.persistent_notifications_enabled),
                bool_to_int(settings.overlay_reminders_enabled),
                bool_to_int(settings.repeat_unacknowledged_enabled),
                settings.unacknowledged_repeat_minutes,
                bool_to_int(settings.smtp_enabled),
                settings.smtp_host,
                settings.smtp_port,
                settings.smtp_security,
                settings.smtp_from,
                settings.smtp_to,
                settings.smtp_username,
                bool_to_int(settings.smtp_repeat_must_complete),
                settings.repeat_default_times[0],
                settings.repeat_default_times[1],
            ],
        )?;

        for entry in &settings.event_kind_defaults {
            transaction.execute(
                "INSERT INTO event_kind_defaults (event_kind, reminder_plan, updated_at)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(event_kind) DO UPDATE SET
                   reminder_plan = excluded.reminder_plan,
                   updated_at = excluded.updated_at",
                params![entry.event_kind, entry.reminder_plan, now.to_rfc3339()],
            )?;
        }

        if input.update_existing_default_items {
            update_existing_default_items(&transaction, &settings, now)?;
        }

        transaction.commit()?;
        Ok(settings)
    }

    pub fn create_item(&self, input: &CreateItemInput, now: DateTime<Utc>) -> AppResult<Item> {
        input.validate()?;
        let mut connection = self.connection.lock().expect("database mutex poisoned");
        let transaction = connection.transaction()?;
        let settings = read_settings(&transaction)?;
        validate_taxonomy_links(&transaction, input.category_id.as_deref(), &input.tag_ids)?;
        let event = resolve_event_config(
            &transaction,
            input.event.as_ref(),
            input.must_complete_today,
        )?;
        let (repeat_time_mode, repeat_times) =
            repeat_schedule_for_event(&event, input.due_at.as_deref(), &settings)?;
        let due = initial_due_for_event(&event, input.due_at.as_deref(), now, &settings)?;

        let id = Uuid::new_v4().to_string();
        let timestamp = now.to_rfc3339();
        let due_at = due.utc.to_rfc3339();
        let due_source = if input.due_at.is_some()
            || matches!(event.kind.as_deref(), Some("WARNING" | "CONTINUOUS"))
        {
            "EXPLICIT"
        } else {
            "DEFAULT_EOD"
        };
        let completion_policy = if event.kind.as_deref() == Some("TODAY_MUST") {
            "MUST_COMPLETE_TODAY"
        } else {
            "NORMAL"
        };
        let rollover_policy =
            if event.kind.is_some() || input.due_at.is_some() || event.reminder_plan != "ONCE" {
                "NONE"
            } else {
                "NEXT_WORKDAY_EOD"
            };
        let repeat_interval = matches!(event.reminder_plan.as_str(), "EMPHASIS" | "FORCE")
            .then_some(
                input
                    .repeat_interval_minutes
                    .unwrap_or(settings.overtime_interval_minutes),
            );
        let next_reminder = initial_next_reminder(&event, due.utc, now, &repeat_times)?;
        let classification_next = event.kind.is_none().then(|| due_at.clone());
        let time_mode = if input.due_at.is_some()
            || matches!(event.kind.as_deref(), Some("WARNING" | "CONTINUOUS"))
        {
            "SPECIFIED"
        } else {
            "DEFAULT"
        };
        let (series_anchor_month, series_anchor_day) =
            series_anchor(event.kind.as_deref(), due.local_date);

        transaction.execute(
            "INSERT INTO items (
                id, title, notes, status, category_id, due_at, due_local_date, due_local_time,
                due_source, rollover_policy, completion_policy, repeat_interval_minutes,
                next_reminder_at, created_at, updated_at, event_kind, reminder_plan, important,
                time_mode, start_at, end_at, target_at, lead_value, lead_unit,
                cadence_value, cadence_unit, emphasis_max_per_day, classification_next_reminder_at,
                repeat_time_mode, repeat_time_first, repeat_time_second
             ) VALUES (?1, ?2, ?3, 'OPEN', ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13,
                ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26,
                ?27, ?28, ?29)",
            params![
                id,
                input.title.trim(),
                input.notes,
                input.category_id,
                due_at,
                due.local_date.format("%Y-%m-%d").to_string(),
                due.local_time.format("%H:%M").to_string(),
                due_source,
                rollover_policy,
                completion_policy,
                repeat_interval,
                next_reminder,
                timestamp,
                event.kind,
                event.reminder_plan,
                bool_to_int(event.important),
                time_mode,
                event.start_at,
                event.end_at,
                event.target_at,
                event.lead_value,
                event.lead_unit,
                event.cadence_value,
                event.cadence_unit,
                event.emphasis_max_per_day,
                classification_next,
                repeat_time_mode,
                repeat_times.first(),
                repeat_times.get(1),
            ],
        )?;
        transaction.execute(
            "UPDATE items SET series_anchor_month = ?1, series_anchor_day = ?2 WHERE id = ?3",
            params![series_anchor_month, series_anchor_day, id],
        )?;
        replace_item_tags(&transaction, &id, &input.tag_ids)?;
        insert_item_event(&transaction, &id, "CREATED", "{}", &timestamp)?;
        transaction.execute("DELETE FROM drafts WHERE id = 1", [])?;
        transaction.commit()?;
        drop(connection);
        self.get_item(&id)
    }

    pub fn list_items(&self, filter: &str, now: DateTime<Utc>) -> AppResult<Vec<Item>> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        let local_today = now
            .with_timezone(&Local)
            .date_naive()
            .format("%Y-%m-%d")
            .to_string();
        let now_text = now.to_rfc3339();
        let (condition, first_param): (&str, Option<&dyn rusqlite::ToSql>) = match filter {
            "open" => ("i.status = 'OPEN'", None),
            "today" => (
                "i.status = 'OPEN' AND i.due_local_date = ?1",
                Some(&local_today),
            ),
            "overdue" => ("i.status = 'OPEN' AND i.due_at < ?1", Some(&now_text)),
            "done" => ("i.status = 'DONE'", None),
            "deleted" => ("i.status = 'DELETED'", None),
            "all" => ("i.status != 'DELETED'", None),
            _ => return Err(AppError::Validation("未知的事项筛选条件".into())),
        };
        let sql = format!(
            "{} WHERE {condition} ORDER BY CASE WHEN i.completion_policy = 'MUST_COMPLETE_TODAY' THEN 0 ELSE 1 END, i.due_at ASC",
            item_select()
        );
        let mut statement = connection.prepare(&sql)?;
        let mapped = if let Some(value) = first_param {
            statement.query_map([value], row_to_item)?
        } else {
            statement.query_map([], row_to_item)?
        };
        let mut items = mapped
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::from)?;
        drop(statement);
        for item in &mut items {
            item.tags = load_item_tags(&connection, &item.id)?;
        }
        Ok(items)
    }

    pub fn get_item(&self, id: &str) -> AppResult<Item> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        let mut item = get_item_from(&connection, id)?;
        item.tags = load_item_tags(&connection, id)?;
        Ok(item)
    }

    pub fn set_item_completed(
        &self,
        id: &str,
        completed: bool,
        now: DateTime<Utc>,
    ) -> AppResult<Item> {
        let mut connection = self.connection.lock().expect("database mutex poisoned");
        let transaction = connection.transaction()?;
        let existing = get_item_from(&transaction, id)?;
        let now_text = now.to_rfc3339();
        let next_reminder = if completed {
            None
        } else if existing.due_at <= now_text {
            Some(now_text.clone())
        } else {
            Some(existing.due_at.clone())
        };
        let classification_next = if completed || existing.event_kind.is_some() {
            None
        } else {
            Some(now_text.clone())
        };

        let changed = transaction.execute(
            "UPDATE items SET status = ?1, completed_at = ?2, next_reminder_at = ?3,
                reminder_paused = 0, updated_at = ?4, revision = revision + 1,
                classification_next_reminder_at = ?5,
                series_occurrence_pending = 0 WHERE id = ?6",
            params![
                if completed { "DONE" } else { "OPEN" },
                completed.then_some(now_text.clone()),
                next_reminder,
                now_text,
                classification_next,
                id,
            ],
        )?;
        if changed == 0 {
            return Err(AppError::ItemNotFound);
        }
        insert_item_event(
            &transaction,
            id,
            if completed { "COMPLETED" } else { "REOPENED" },
            "{}",
            &now.to_rfc3339(),
        )?;
        transaction.commit()?;
        drop(connection);
        self.get_item(id)
    }

    pub fn classify_as_ordinary(&self, id: &str, now: DateTime<Utc>) -> AppResult<Item> {
        let mut connection = self.connection.lock().expect("database mutex poisoned");
        let transaction = connection.transaction()?;
        let existing = get_item_from(&transaction, id)?;
        if existing.status == "DELETED" {
            return Err(AppError::Validation(
                "请先从回收站恢复记录再选择类型".into(),
            ));
        }
        let reminder_plan: String = transaction.query_row(
            "SELECT reminder_plan FROM event_kind_defaults WHERE event_kind = 'ORDINARY'",
            [],
            |row| row.get(0),
        )?;
        let now_text = now.to_rfc3339();
        let next_reminder = if existing.status == "OPEN" && !existing.reminder_paused {
            Some(if existing.due_at <= now_text {
                now_text.clone()
            } else {
                existing.due_at
            })
        } else {
            None
        };
        transaction.execute(
            "UPDATE items SET event_kind = 'ORDINARY', reminder_plan = ?1,
                completion_policy = 'NORMAL', rollover_policy = 'NONE',
                classification_next_reminder_at = NULL, next_reminder_at = ?2,
                updated_at = ?3, revision = revision + 1 WHERE id = ?4",
            params![reminder_plan, next_reminder, now_text, id],
        )?;
        insert_item_event(
            &transaction,
            id,
            "EVENT_KIND_SELECTED",
            r#"{"eventKind":"ORDINARY"}"#,
            &now_text,
        )?;
        transaction.commit()?;
        drop(connection);
        self.get_item(id)
    }

    pub fn snooze_classification(
        &self,
        item_id: Option<&str>,
        minutes: u32,
        now: DateTime<Utc>,
    ) -> AppResult<()> {
        if !(5..=1440).contains(&minutes) {
            return Err(AppError::Validation(
                "稍后处理时间必须在 5 分钟到 24 小时之间".into(),
            ));
        }
        let connection = self.connection.lock().expect("database mutex poisoned");
        let next = (now + chrono::Duration::minutes(i64::from(minutes))).to_rfc3339();
        if let Some(item_id) = item_id {
            connection.execute(
                "UPDATE items SET classification_next_reminder_at = ?1
                 WHERE id = ?2 AND event_kind IS NULL AND status = 'OPEN'",
                params![next, item_id],
            )?;
        } else {
            connection.execute(
                "UPDATE items SET classification_next_reminder_at = ?1
                 WHERE event_kind IS NULL AND status = 'OPEN'",
                [next],
            )?;
        }
        Ok(())
    }

    pub fn complete_series_occurrence(&self, id: &str, now: DateTime<Utc>) -> AppResult<Item> {
        let mut connection = self.connection.lock().expect("database mutex poisoned");
        let transaction = connection.transaction()?;
        let existing = get_item_from(&transaction, id)?;
        if !matches!(existing.event_kind.as_deref(), Some("MONTHLY" | "YEARLY")) {
            return Err(AppError::Validation(
                "只有月度或年度事件可以完成本次".into(),
            ));
        }
        let timestamp = now.to_rfc3339();
        let (pending, anchor_month, anchor_day) = transaction.query_row(
            "SELECT series_occurrence_pending, series_anchor_month, series_anchor_day
             FROM items WHERE id = ?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, bool>(0)?,
                    row.get::<_, Option<u32>>(1)?,
                    row.get::<_, Option<u32>>(2)?,
                ))
            },
        )?;
        let current = DateTime::parse_from_rfc3339(&existing.due_at)
            .map_err(|_| AppError::Validation("周期事件的当前时间数据损坏".into()))?
            .with_timezone(&Utc);
        if pending && current > now {
            transaction.execute(
                "UPDATE items SET reminder_acknowledged_at = ?1,
                    series_occurrence_pending = 0, updated_at = ?1,
                    revision = revision + 1 WHERE id = ?2 AND status = 'OPEN'",
                params![timestamp, id],
            )?;
        } else {
            let next = next_series_after(
                current,
                now,
                existing.event_kind.as_deref().unwrap_or("MONTHLY"),
                anchor_month,
                anchor_day,
            )?;
            let local = next.with_timezone(&Local);
            transaction.execute(
                "UPDATE items SET due_at = ?1, due_local_date = ?2, due_local_time = ?3,
                    next_reminder_at = ?1, reminder_acknowledged_at = ?4,
                    series_occurrence_pending = 0, updated_at = ?4,
                    revision = revision + 1 WHERE id = ?5 AND status = 'OPEN'",
                params![
                    next.to_rfc3339(),
                    local.date_naive().format("%Y-%m-%d").to_string(),
                    local.time().format("%H:%M").to_string(),
                    timestamp,
                    id,
                ],
            )?;
        }
        insert_item_event(&transaction, id, "OCCURRENCE_COMPLETED", "{}", &timestamp)?;
        transaction.commit()?;
        drop(connection);
        self.get_item(id)
    }

    pub fn set_reminder_paused(
        &self,
        id: &str,
        paused: bool,
        now: DateTime<Utc>,
    ) -> AppResult<Item> {
        let mut connection = self.connection.lock().expect("database mutex poisoned");
        let transaction = connection.transaction()?;
        let existing = get_item_from(&transaction, id)?;
        if existing.status != "OPEN" {
            return Err(AppError::Validation("只有待处理事件可以暂停提醒".into()));
        }
        let now_text = now.to_rfc3339();
        let next_reminder = if paused {
            None
        } else if existing.due_at <= now_text {
            Some(now_text.clone())
        } else {
            Some(existing.due_at.clone())
        };
        transaction.execute(
            "UPDATE items SET reminder_paused = ?1, next_reminder_at = ?2,
                updated_at = ?3, revision = revision + 1 WHERE id = ?4",
            params![bool_to_int(paused), next_reminder, now_text, id],
        )?;
        insert_item_event(
            &transaction,
            id,
            if paused {
                "REMINDER_PAUSED"
            } else {
                "REMINDER_RESUMED"
            },
            "{}",
            &now.to_rfc3339(),
        )?;
        transaction.commit()?;
        drop(connection);
        self.get_item(id)
    }

    pub fn reschedule_item(&self, id: &str, due_at: &str, now: DateTime<Utc>) -> AppResult<Item> {
        let due = due_from_explicit(due_at)?;
        let mut connection = self.connection.lock().expect("database mutex poisoned");
        let transaction = connection.transaction()?;
        let existing = get_item_from(&transaction, id)?;
        if existing.status != "OPEN" {
            return Err(AppError::Validation("只有未完成事项可以调整时间".into()));
        }
        if existing.due_at < now.to_rfc3339() {
            return Err(AppError::Validation(
                "逾期事项的原到期时间不能修改；可以转换类型并从现在开始新计划".into(),
            ));
        }

        let due_at = due.utc.to_rfc3339();
        let timestamp = now.to_rfc3339();
        transaction.execute(
            "UPDATE items SET due_at = ?1, due_local_date = ?2, due_local_time = ?3,
                due_source = 'EXPLICIT', rollover_policy = 'NONE', next_reminder_at = ?1,
                reminder_paused = 0, updated_at = ?4, revision = revision + 1,
                time_mode = 'SPECIFIED',
                repeat_time_mode = CASE WHEN reminder_plan = 'REPEAT' THEN 'SPECIFIED' ELSE repeat_time_mode END,
                repeat_time_first = CASE WHEN reminder_plan = 'REPEAT' THEN ?3 ELSE repeat_time_first END,
                repeat_time_second = CASE WHEN reminder_plan = 'REPEAT' THEN NULL ELSE repeat_time_second END
                WHERE id = ?5",
            params![
                due_at,
                due.local_date.format("%Y-%m-%d").to_string(),
                due.local_time.format("%H:%M").to_string(),
                timestamp,
                id,
            ],
        )?;
        let event_data = serde_json::json!({ "dueAt": due_at }).to_string();
        insert_item_event(&transaction, id, "RESCHEDULED", &event_data, &timestamp)?;
        transaction.commit()?;
        drop(connection);
        self.get_item(id)
    }

    pub fn list_categories(&self) -> AppResult<Vec<Category>> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        let mut statement = connection.prepare(
            "SELECT id, name, color FROM categories WHERE archived = 0 ORDER BY position, name",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(Category {
                id: row.get(0)?,
                name: row.get(1)?,
                color: row.get(2)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn create_category(&self, input: &TaxonomyInput) -> AppResult<Category> {
        input.validate("类型")?;
        let connection = self.connection.lock().expect("database mutex poisoned");
        let id = Uuid::new_v4().to_string();
        connection.execute(
            "INSERT INTO categories (id, name, color, position)
             VALUES (?1, ?2, ?3, COALESCE((SELECT MAX(position) + 10 FROM categories), 10))",
            params![id, input.name.trim(), input.color],
        )?;
        Ok(Category {
            id,
            name: input.name.trim().to_string(),
            color: input.color.clone(),
        })
    }

    pub fn update_category(&self, id: &str, input: &TaxonomyInput) -> AppResult<Category> {
        input.validate("类型")?;
        let connection = self.connection.lock().expect("database mutex poisoned");
        let changed = connection.execute(
            "UPDATE categories SET name = ?1, color = ?2 WHERE id = ?3 AND archived = 0",
            params![input.name.trim(), input.color, id],
        )?;
        if changed == 0 {
            return Err(AppError::Validation("找不到要修改的类型".into()));
        }
        Ok(Category {
            id: id.to_string(),
            name: input.name.trim().to_string(),
            color: input.color.clone(),
        })
    }

    pub fn delete_category(&self, id: &str, reassign_to: Option<&str>) -> AppResult<()> {
        if reassign_to == Some(id) {
            return Err(AppError::Validation("请选择另一个类型作为迁移目标".into()));
        }
        let mut connection = self.connection.lock().expect("database mutex poisoned");
        let transaction = connection.transaction()?;
        ensure_category_exists(&transaction, id)?;
        if let Some(target) = reassign_to {
            ensure_category_exists(&transaction, target)?;
        }
        transaction.execute(
            "UPDATE items SET category_id = ?1, revision = revision + 1 WHERE category_id = ?2",
            params![reassign_to, id],
        )?;
        transaction.execute("DELETE FROM categories WHERE id = ?1", [id])?;
        transaction.commit()?;
        Ok(())
    }

    pub fn move_category(&self, id: &str, direction: &str) -> AppResult<()> {
        let mut connection = self.connection.lock().expect("database mutex poisoned");
        move_ordered_row(&mut connection, "categories", id, direction, "类型")
    }

    pub fn list_tags(&self) -> AppResult<Vec<Tag>> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        load_tags(&connection)
    }

    pub fn create_tag(&self, input: &TaxonomyInput) -> AppResult<Tag> {
        input.validate("标签")?;
        let connection = self.connection.lock().expect("database mutex poisoned");
        let id = Uuid::new_v4().to_string();
        connection.execute(
            "INSERT INTO tags (id, name, color, position)
             VALUES (?1, ?2, ?3, COALESCE((SELECT MAX(position) + 10 FROM tags), 10))",
            params![id, input.name.trim(), input.color],
        )?;
        Ok(Tag {
            id,
            name: input.name.trim().to_string(),
            color: input.color.clone(),
        })
    }

    pub fn update_tag(&self, id: &str, input: &TaxonomyInput) -> AppResult<Tag> {
        input.validate("标签")?;
        let connection = self.connection.lock().expect("database mutex poisoned");
        let changed = connection.execute(
            "UPDATE tags SET name = ?1, color = ?2 WHERE id = ?3 AND archived = 0",
            params![input.name.trim(), input.color, id],
        )?;
        if changed == 0 {
            return Err(AppError::Validation("找不到要修改的标签".into()));
        }
        Ok(Tag {
            id: id.to_string(),
            name: input.name.trim().to_string(),
            color: input.color.clone(),
        })
    }

    pub fn delete_tag(&self, id: &str) -> AppResult<()> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        let changed = connection.execute("DELETE FROM tags WHERE id = ?1", [id])?;
        if changed == 0 {
            return Err(AppError::Validation("找不到要删除的标签".into()));
        }
        Ok(())
    }

    pub fn move_tag(&self, id: &str, direction: &str) -> AppResult<()> {
        let mut connection = self.connection.lock().expect("database mutex poisoned");
        move_ordered_row(&mut connection, "tags", id, direction, "标签")
    }

    pub fn update_item(
        &self,
        id: &str,
        input: &UpdateItemInput,
        now: DateTime<Utc>,
    ) -> AppResult<Item> {
        input.validate()?;
        let mut connection = self.connection.lock().expect("database mutex poisoned");
        let transaction = connection.transaction()?;
        let existing = get_item_from(&transaction, id)?;
        if existing.status == "DELETED" {
            return Err(AppError::Validation("请先从回收站恢复事项再编辑".into()));
        }
        validate_taxonomy_links(&transaction, input.category_id.as_deref(), &input.tag_ids)?;
        let settings = read_settings(&transaction)?;
        let event = if input.event.is_none() && !input.must_complete_today {
            ResolvedEventConfig::from_item(&existing)
        } else {
            resolve_event_config(
                &transaction,
                input.event.as_ref(),
                input.must_complete_today,
            )?
        };
        let (repeat_time_mode, repeat_times) =
            repeat_schedule_for_event(&event, Some(&input.due_at), &settings)?;
        let due = initial_due_for_event(&event, Some(&input.due_at), now, &settings)?;
        if existing.due_at < now.to_rfc3339()
            && existing.event_kind == event.kind
            && existing.due_at != due.utc.to_rfc3339()
        {
            return Err(AppError::Validation(
                "逾期事项的原到期时间不能修改；可以转换类型并从现在开始新计划".into(),
            ));
        }
        let timestamp = now.to_rfc3339();
        let due_at = due.utc.to_rfc3339();
        let completion_policy = if event.kind.as_deref() == Some("TODAY_MUST") {
            "MUST_COMPLETE_TODAY"
        } else {
            "NORMAL"
        };
        let repeat_interval = matches!(event.reminder_plan.as_str(), "EMPHASIS" | "FORCE")
            .then_some(
                input
                    .repeat_interval_minutes
                    .unwrap_or(settings.overtime_interval_minutes),
            );
        let next_reminder = if existing.status == "OPEN" && !existing.reminder_paused {
            Some(initial_next_reminder(&event, due.utc, now, &repeat_times)?)
        } else {
            None
        };
        let classification_next = if event.kind.is_none() && existing.status == "OPEN" {
            Some(due_at.clone())
        } else {
            None
        };
        let time_mode = if input.due_at == existing.due_at && event.kind == existing.event_kind {
            existing.time_mode.as_str()
        } else {
            "SPECIFIED"
        };
        transaction.execute(
            "UPDATE items SET title = ?1, notes = ?2, category_id = ?3, due_at = ?4,
                due_local_date = ?5, due_local_time = ?6, due_source = 'EXPLICIT',
                rollover_policy = 'NONE', completion_policy = ?7, repeat_interval_minutes = ?8,
                next_reminder_at = ?9, reminder_paused = 0, updated_at = ?10,
                revision = revision + 1, event_kind = ?11, reminder_plan = ?12,
                important = ?13, time_mode = ?14, start_at = ?15, end_at = ?16,
                target_at = ?17, lead_value = ?18, lead_unit = ?19,
                cadence_value = ?20, cadence_unit = ?21,
                emphasis_max_per_day = ?22, emphasis_count = 0,
                emphasis_count_local_date = NULL,
                classification_next_reminder_at = ?23,
                series_occurrence_pending = 0, repeat_time_mode = ?24,
                repeat_time_first = ?25, repeat_time_second = ?26 WHERE id = ?27",
            params![
                input.title.trim(),
                input.notes,
                input.category_id,
                due_at,
                due.local_date.format("%Y-%m-%d").to_string(),
                due.local_time.format("%H:%M").to_string(),
                completion_policy,
                repeat_interval,
                next_reminder,
                timestamp,
                event.kind,
                event.reminder_plan,
                bool_to_int(event.important),
                time_mode,
                event.start_at,
                event.end_at,
                event.target_at,
                event.lead_value,
                event.lead_unit,
                event.cadence_value,
                event.cadence_unit,
                event.emphasis_max_per_day,
                classification_next,
                repeat_time_mode,
                repeat_times.first(),
                repeat_times.get(1),
                id,
            ],
        )?;
        let (series_anchor_month, series_anchor_day) =
            series_anchor(event.kind.as_deref(), due.local_date);
        transaction.execute(
            "UPDATE items SET series_anchor_month = ?1, series_anchor_day = ?2 WHERE id = ?3",
            params![series_anchor_month, series_anchor_day, id],
        )?;
        replace_item_tags(&transaction, id, &input.tag_ids)?;
        insert_item_event(&transaction, id, "EDITED", "{}", &timestamp)?;
        transaction.commit()?;
        drop(connection);
        self.get_item(id)
    }

    pub fn set_item_deleted(&self, id: &str, deleted: bool, now: DateTime<Utc>) -> AppResult<Item> {
        let mut connection = self.connection.lock().expect("database mutex poisoned");
        let transaction = connection.transaction()?;
        let existing = get_item_from(&transaction, id)?;
        let timestamp = now.to_rfc3339();
        let deleted_from_status = transaction.query_row(
            "SELECT deleted_from_status FROM items WHERE id = ?1",
            [id],
            |row| row.get::<_, Option<String>>(0),
        )?;
        let restored_status = deleted_from_status.as_deref().unwrap_or("OPEN");
        let status = if deleted { "DELETED" } else { restored_status };
        let next_reminder = if deleted || status != "OPEN" {
            None
        } else if existing.due_at <= timestamp {
            Some(timestamp.clone())
        } else {
            Some(existing.due_at.clone())
        };
        let classification_next = if deleted || status != "OPEN" || existing.event_kind.is_some() {
            None
        } else {
            Some(timestamp.clone())
        };
        transaction.execute(
            "UPDATE items SET status = ?1, deleted_at = ?2,
                next_reminder_at = ?3, reminder_paused = 0, updated_at = ?4,
                deleted_from_status = ?5, revision = revision + 1,
                classification_next_reminder_at = ?6 WHERE id = ?7",
            params![
                status,
                deleted.then_some(timestamp.clone()),
                next_reminder,
                timestamp,
                if deleted { Some(existing.status) } else { None },
                classification_next,
                id
            ],
        )?;
        insert_item_event(
            &transaction,
            id,
            if deleted { "DELETED" } else { "RESTORED" },
            "{}",
            &now.to_rfc3339(),
        )?;
        transaction.commit()?;
        drop(connection);
        self.get_item(id)
    }

    pub fn permanently_delete_item(&self, id: &str) -> AppResult<()> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        let status = connection
            .query_row("SELECT status FROM items WHERE id = ?1", [id], |row| {
                row.get::<_, String>(0)
            })
            .optional()?
            .ok_or(AppError::ItemNotFound)?;
        if status != "DELETED" {
            return Err(AppError::Validation(
                "只有回收站中的事项可以永久删除".into(),
            ));
        }
        connection.execute("DELETE FROM items WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn list_pending_reminders(&self, now: DateTime<Utc>) -> AppResult<Vec<Item>> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        let ids = {
            let mut statement = connection.prepare(
                "SELECT i.id FROM items i
                 WHERE i.status = 'OPEN' AND i.reminder_paused = 0
                   AND i.reminder_acknowledged_at IS NULL
                   AND EXISTS (
                     SELECT 1 FROM reminder_events e
                     WHERE e.item_id = i.id AND e.scheduled_for <= ?1
                       AND e.result IN ('SUBMITTED', 'FAILED', 'DISABLED', 'CLAIMED')
                   )
                 ORDER BY i.due_at, i.created_at",
            )?;
            statement
                .query_map([now.to_rfc3339()], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        let mut items = Vec::with_capacity(ids.len());
        for id in ids {
            let mut item = get_item_from(&connection, &id)?;
            item.tags = load_item_tags(&connection, &id)?;
            items.push(item);
        }
        Ok(items)
    }

    pub fn acknowledge_reminder(&self, id: &str, now: DateTime<Utc>) -> AppResult<Item> {
        let mut connection = self.connection.lock().expect("database mutex poisoned");
        let transaction = connection.transaction()?;
        let item = get_item_from(&transaction, id)?;
        if item.status != "OPEN" {
            return Err(AppError::Validation("这个事项已经不需要确认提醒".into()));
        }
        let timestamp = now.to_rfc3339();
        transaction.execute(
            "UPDATE items SET reminder_acknowledged_at = ?1,
                next_reminder_at = CASE
                  WHEN completion_policy = 'NORMAL' THEN NULL
                  ELSE next_reminder_at
                END,
                updated_at = ?1 WHERE id = ?2",
            params![timestamp, id],
        )?;
        insert_item_event(&transaction, id, "REMINDER_ACKNOWLEDGED", "{}", &timestamp)?;
        transaction.commit()?;
        drop(connection);
        self.get_item(id)
    }

    pub fn snooze_item(&self, id: &str, minutes: u32, now: DateTime<Utc>) -> AppResult<Item> {
        if !(5..=240).contains(&minutes) {
            return Err(AppError::Validation(
                "稍后提醒时间必须在 5–240 分钟之间".into(),
            ));
        }
        let mut connection = self.connection.lock().expect("database mutex poisoned");
        let transaction = connection.transaction()?;
        let item = get_item_from(&transaction, id)?;
        if item.status != "OPEN" {
            return Err(AppError::Validation("这个事项已经不需要稍后提醒".into()));
        }
        let timestamp = now.to_rfc3339();
        let next = (now + chrono::Duration::minutes(i64::from(minutes))).to_rfc3339();
        transaction.execute(
            "UPDATE items SET next_reminder_at = ?1, reminder_acknowledged_at = ?2,
                reminder_paused = 0, updated_at = ?2, revision = revision + 1 WHERE id = ?3",
            params![next, timestamp, id],
        )?;
        let data = serde_json::json!({ "minutes": minutes }).to_string();
        insert_item_event(&transaction, id, "REMINDER_SNOOZED", &data, &timestamp)?;
        transaction.commit()?;
        drop(connection);
        self.get_item(id)
    }

    pub fn list_email_route_tag_ids(&self) -> AppResult<Vec<String>> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        let mut statement = connection.prepare(
            "SELECT tag_id FROM tag_notification_routes WHERE channel = 'email' AND enabled = 1 ORDER BY tag_id",
        )?;
        statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::from)
    }

    pub fn set_tag_email_route(&self, tag_id: &str, enabled: bool) -> AppResult<()> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        validate_taxonomy_links(&connection, None, &[tag_id.to_string()])?;
        if enabled {
            connection.execute(
                "INSERT INTO tag_notification_routes (tag_id, channel, enabled) VALUES (?1, 'email', 1)
                 ON CONFLICT(tag_id, channel) DO UPDATE SET enabled = 1",
                [tag_id],
            )?;
        } else {
            connection.execute(
                "DELETE FROM tag_notification_routes WHERE tag_id = ?1 AND channel = 'email'",
                [tag_id],
            )?;
        }
        Ok(())
    }

    pub fn record_overlay_delivery(
        &self,
        notification: &DueNotification,
        now: DateTime<Utc>,
    ) -> AppResult<()> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        connection.execute(
            "INSERT OR IGNORE INTO channel_deliveries
               (id, reminder_event_id, item_id, channel, idempotency_key, attempted_at,
                submitted_at, result, attempt_count)
             VALUES (?1, ?2, ?3, 'overlay', ?4, ?5, ?5, 'SUBMITTED', 1)",
            params![
                Uuid::new_v4().to_string(),
                notification.event_id,
                notification.item_id,
                format!("{}:overlay", notification.event_id),
                now.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn claim_email_delivery(
        &self,
        notification: &DueNotification,
        repeat_must_complete: bool,
        now: DateTime<Utc>,
    ) -> AppResult<Option<EmailDeliveryJob>> {
        let mut connection = self.connection.lock().expect("database mutex poisoned");
        let transaction = connection.transaction()?;
        let routed = transaction.query_row(
            "SELECT EXISTS(
               SELECT 1 FROM item_tags it
               JOIN tag_notification_routes r ON r.tag_id = it.tag_id
               WHERE it.item_id = ?1 AND r.channel = 'email' AND r.enabled = 1
             )",
            [&notification.item_id],
            |row| row.get::<_, bool>(0),
        )?;
        if !routed {
            transaction.commit()?;
            return Ok(None);
        }
        if notification.completion_policy != "MUST_COMPLETE_TODAY" || !repeat_must_complete {
            let already_scheduled = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM channel_deliveries WHERE item_id = ?1 AND channel = 'email')",
                [&notification.item_id],
                |row| row.get::<_, bool>(0),
            )?;
            if already_scheduled {
                transaction.commit()?;
                return Ok(None);
            }
        }
        let delivery_id = Uuid::new_v4().to_string();
        let inserted = transaction.execute(
            "INSERT OR IGNORE INTO channel_deliveries
               (id, reminder_event_id, item_id, channel, idempotency_key, attempted_at,
                result, attempt_count, next_attempt_at)
             VALUES (?1, ?2, ?3, 'email', ?4, ?5, 'CLAIMED', 0, ?5)",
            params![
                delivery_id,
                notification.event_id,
                notification.item_id,
                format!("{}:email", notification.event_id),
                now.to_rfc3339(),
            ],
        )?;
        transaction.commit()?;
        Ok((inserted > 0).then_some(EmailDeliveryJob {
            delivery_id,
            notification: notification.clone(),
        }))
    }

    pub fn claim_due_email_retries(&self, now: DateTime<Utc>) -> AppResult<Vec<EmailDeliveryJob>> {
        let mut connection = self.connection.lock().expect("database mutex poisoned");
        let transaction = connection.transaction()?;
        let rows = {
            let mut statement = transaction.prepare(
                "SELECT d.id, d.reminder_event_id, i.id, i.title, i.due_local_date,
                        i.due_local_time, i.completion_policy, i.event_kind, i.reminder_plan
                 FROM channel_deliveries d
                 JOIN items i ON i.id = d.item_id
                 WHERE d.channel = 'email' AND d.result = 'FAILED'
                   AND d.attempt_count < 3 AND d.next_attempt_at <= ?1 AND i.status = 'OPEN'
                   AND EXISTS(
                     SELECT 1 FROM item_tags it
                     JOIN tag_notification_routes r ON r.tag_id = it.tag_id
                     WHERE it.item_id = i.id AND r.channel = 'email' AND r.enabled = 1
                   )
                 ORDER BY d.next_attempt_at LIMIT 3",
            )?;
            statement
                .query_map([now.to_rfc3339()], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        DueNotification {
                            event_id: row.get(1)?,
                            item_id: row.get(2)?,
                            title: row.get(3)?,
                            due_local_date: row.get(4)?,
                            due_local_time: row.get(5)?,
                            completion_policy: row.get(6)?,
                            event_kind: row.get(7)?,
                            reminder_plan: row.get(8)?,
                            tag_ids: Vec::new(),
                        },
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        let mut jobs = Vec::with_capacity(rows.len());
        for (delivery_id, mut notification) in rows {
            notification.tag_ids = load_item_tags(&transaction, &notification.item_id)?
                .into_iter()
                .map(|tag| tag.id)
                .collect();
            transaction.execute(
                "UPDATE channel_deliveries SET result = 'CLAIMED', attempted_at = ?1 WHERE id = ?2 AND result = 'FAILED'",
                params![now.to_rfc3339(), delivery_id],
            )?;
            jobs.push(EmailDeliveryJob {
                delivery_id,
                notification,
            });
        }
        transaction.commit()?;
        Ok(jobs)
    }

    pub fn mark_email_delivery_submitted(&self, id: &str, now: DateTime<Utc>) -> AppResult<()> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        connection.execute(
            "UPDATE channel_deliveries SET result = 'SUBMITTED', submitted_at = ?1,
                error_code = NULL, attempt_count = attempt_count + 1, next_attempt_at = NULL
             WHERE id = ?2 AND result = 'CLAIMED'",
            params![now.to_rfc3339(), id],
        )?;
        Ok(())
    }

    pub fn mark_email_delivery_failed(
        &self,
        id: &str,
        error_code: &str,
        now: DateTime<Utc>,
    ) -> AppResult<()> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        let attempts = connection.query_row(
            "SELECT attempt_count FROM channel_deliveries WHERE id = ?1",
            [id],
            |row| row.get::<_, u32>(0),
        )? + 1;
        let next_attempt = match attempts {
            1 => Some(now + chrono::Duration::minutes(1)),
            2 => Some(now + chrono::Duration::minutes(5)),
            _ => None,
        }
        .map(|value| value.to_rfc3339());
        connection.execute(
            "UPDATE channel_deliveries SET result = 'FAILED', error_code = ?1,
                attempt_count = ?2, next_attempt_at = ?3 WHERE id = ?4 AND result = 'CLAIMED'",
            params![error_code, attempts, next_attempt, id],
        )?;
        Ok(())
    }

    pub fn load_draft(&self) -> AppResult<String> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        Ok(connection
            .query_row("SELECT content FROM drafts WHERE id = 1", [], |row| {
                row.get(0)
            })
            .optional()?
            .unwrap_or_default())
    }

    pub fn save_draft(&self, content: &str, now: DateTime<Utc>) -> AppResult<()> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        connection.execute(
            "INSERT INTO drafts (id, content, updated_at) VALUES (1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET content = excluded.content, updated_at = excluded.updated_at",
            params![content, now.to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn process_due(&self, now: DateTime<Utc>) -> AppResult<ProcessResult> {
        let mut connection = self.connection.lock().expect("database mutex poisoned");
        let transaction = connection.transaction()?;
        let settings = read_settings(&transaction)?;
        let mut result = ProcessResult {
            notifications_enabled: settings.notifications_enabled,
            ..ProcessResult::default()
        };

        let recycle_cutoff = (now - chrono::Duration::days(30)).to_rfc3339();
        result.changed |= transaction.execute(
            "DELETE FROM items WHERE status = 'DELETED' AND deleted_at IS NOT NULL AND deleted_at <= ?1",
            [&recycle_cutoff],
        )? > 0;

        result.changed |= repair_future_must_complete_items(&transaction, now)? > 0;
        result.changed |= rollover_default_items(&transaction, &settings, now)? > 0;

        let now_text = now.to_rfc3339();
        let stale_before = (now - chrono::Duration::minutes(2)).to_rfc3339();
        let stale_claims = {
            let mut statement = transaction.prepare(
                "SELECT e.id, e.item_id, i.title, i.due_local_date, i.due_local_time,
                        i.completion_policy, i.event_kind, i.reminder_plan
                 FROM reminder_events e
                 JOIN items i ON i.id = e.item_id
                 WHERE e.result = 'CLAIMED' AND e.sent_at <= ?1 AND i.status = 'OPEN'
                 ORDER BY e.sent_at ASC",
            )?;
            let rows = statement.query_map([&stale_before], |row| {
                Ok(DueNotification {
                    event_id: row.get(0)?,
                    item_id: row.get(1)?,
                    title: row.get(2)?,
                    due_local_date: row.get(3)?,
                    due_local_time: row.get(4)?,
                    completion_policy: row.get(5)?,
                    event_kind: row.get(6)?,
                    reminder_plan: row.get(7)?,
                    tag_ids: Vec::new(),
                })
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        for mut notification in stale_claims {
            notification.tag_ids = load_item_tags(&transaction, &notification.item_id)?
                .into_iter()
                .map(|tag| tag.id)
                .collect();
            transaction.execute(
                "UPDATE reminder_events SET sent_at = ?1 WHERE id = ?2 AND result = 'CLAIMED'",
                params![now_text, notification.event_id],
            )?;
            result.notifications.push(notification);
            result.changed = true;
        }

        let due_rows = {
            let mut statement = transaction.prepare(
                "SELECT id, title, completion_policy, reminder_plan, event_kind,
                        repeat_interval_minutes, next_reminder_at, bypass_app_quiet_hours,
                        revision, due_local_date, due_local_time, end_at, target_at,
                        cadence_value, cadence_unit, emphasis_max_per_day,
                        emphasis_count, emphasis_count_local_date,
                        series_anchor_month, series_anchor_day,
                        repeat_time_first, repeat_time_second
                 FROM items
                 WHERE status = 'OPEN' AND reminder_paused = 0
                   AND next_reminder_at IS NOT NULL AND next_reminder_at <= ?1
                 ORDER BY next_reminder_at ASC",
            )?;
            let rows = statement.query_map([&now_text], |row| {
                Ok(DueRow {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    completion_policy: row.get(2)?,
                    reminder_plan: row.get(3)?,
                    event_kind: row.get(4)?,
                    repeat_interval_minutes: row.get(5)?,
                    scheduled_for: row.get(6)?,
                    bypass_quiet_hours: row.get::<_, i64>(7)? != 0,
                    revision: row.get(8)?,
                    due_local_date: row.get(9)?,
                    due_local_time: row.get(10)?,
                    end_at: row.get(11)?,
                    target_at: row.get(12)?,
                    cadence_value: row.get(13)?,
                    cadence_unit: row.get(14)?,
                    emphasis_max_per_day: row.get(15)?,
                    emphasis_count: row.get(16)?,
                    emphasis_count_local_date: row.get(17)?,
                    series_anchor_month: row.get(18)?,
                    series_anchor_day: row.get(19)?,
                    repeat_time_first: row.get(20)?,
                    repeat_time_second: row.get(21)?,
                })
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        for row in due_rows {
            if should_stop_without_delivery(&row, now)? {
                transaction.execute(
                    "UPDATE items SET next_reminder_at = NULL, updated_at = ?1 WHERE id = ?2",
                    params![now_text, row.id],
                )?;
                result.changed = true;
                continue;
            }
            let idempotency_key = format!("{}:{}:{}", row.id, row.scheduled_for, row.revision);
            let event_id = Uuid::new_v4().to_string();
            let inserted = transaction.execute(
                "INSERT OR IGNORE INTO reminder_events
                   (id, item_id, scheduled_for, sent_at, idempotency_key, result)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'CLAIMED')",
                params![
                    event_id,
                    row.id,
                    row.scheduled_for,
                    now_text,
                    idempotency_key
                ],
            )?;

            let next_reminder = next_after_delivery(&row, now, &settings)?;
            let (emphasis_count, emphasis_date) = if row.reminder_plan == "EMPHASIS" {
                let date = now
                    .with_timezone(&Local)
                    .date_naive()
                    .format("%Y-%m-%d")
                    .to_string();
                let count = if row.emphasis_count_local_date.as_deref() == Some(date.as_str()) {
                    row.emphasis_count.saturating_add(1)
                } else {
                    1
                };
                (count, Some(date))
            } else {
                (row.emphasis_count, row.emphasis_count_local_date.clone())
            };
            let is_series = matches!(row.event_kind.as_deref(), Some("MONTHLY" | "YEARLY"));
            if is_series && row.reminder_plan == "ONCE" {
                if let Some(next) = next_reminder {
                    let local = next.with_timezone(&Local);
                    transaction.execute(
                        "UPDATE items SET due_at = ?1, due_local_date = ?2, due_local_time = ?3,
                            next_reminder_at = ?1, reminder_acknowledged_at = NULL,
                            updated_at = ?4, emphasis_count = ?5,
                            emphasis_count_local_date = ?6,
                            series_occurrence_pending = 1 WHERE id = ?7",
                        params![
                            next.to_rfc3339(),
                            local.date_naive().format("%Y-%m-%d").to_string(),
                            local.time().format("%H:%M").to_string(),
                            now_text,
                            emphasis_count,
                            emphasis_date,
                            row.id,
                        ],
                    )?;
                }
            } else if is_series {
                transaction.execute(
                    "UPDATE items SET next_reminder_at = ?1, reminder_acknowledged_at = NULL,
                        updated_at = ?2, emphasis_count = ?3,
                        emphasis_count_local_date = ?4,
                        series_occurrence_pending = 1 WHERE id = ?5",
                    params![
                        next_reminder.map(|value| value.to_rfc3339()),
                        now_text,
                        emphasis_count,
                        emphasis_date,
                        row.id
                    ],
                )?;
            } else {
                transaction.execute(
                    "UPDATE items SET next_reminder_at = ?1, reminder_acknowledged_at = NULL,
                        updated_at = ?2, emphasis_count = ?3,
                        emphasis_count_local_date = ?4 WHERE id = ?5",
                    params![
                        next_reminder.map(|value| value.to_rfc3339()),
                        now_text,
                        emphasis_count,
                        emphasis_date,
                        row.id
                    ],
                )?;
            }

            if inserted > 0 {
                let tag_ids = load_item_tags(&transaction, &row.id)?
                    .into_iter()
                    .map(|tag| tag.id)
                    .collect();
                let scheduled_local = scheduled_local_fields(&row.scheduled_for)
                    .unwrap_or_else(|| (row.due_local_date.clone(), row.due_local_time.clone()));
                result.notifications.push(DueNotification {
                    event_id,
                    item_id: row.id,
                    title: row.title,
                    due_local_date: scheduled_local.0,
                    due_local_time: scheduled_local.1,
                    completion_policy: row.completion_policy,
                    event_kind: row.event_kind,
                    reminder_plan: row.reminder_plan,
                    tag_ids,
                });
            }
            result.changed = true;
        }

        let classification_rows = {
            let mut statement = transaction.prepare(
                "SELECT id, classification_next_reminder_at, revision
                 FROM items
                 WHERE event_kind IS NULL AND status = 'OPEN' AND reminder_paused = 0
                   AND classification_next_reminder_at IS NOT NULL
                   AND classification_next_reminder_at <= ?1
                 ORDER BY classification_next_reminder_at, created_at",
            )?;
            statement
                .query_map([&now_text], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, u32>(2)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        let mut claimed_classifications = Vec::new();
        let next_classification =
            next_daily_at(now, parse_time(&settings.default_due_time)?)?.to_rfc3339();
        for (item_id, scheduled_for, revision) in classification_rows {
            let event_id = Uuid::new_v4().to_string();
            let idempotency_key = format!("classification:{item_id}:{scheduled_for}:{revision}");
            let inserted = transaction.execute(
                "INSERT OR IGNORE INTO classification_reminder_events
                   (id, item_id, scheduled_for, attempted_at, idempotency_key, result)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'CLAIMED')",
                params![event_id, item_id, scheduled_for, now_text, idempotency_key],
            )?;
            transaction.execute(
                "UPDATE items SET classification_next_reminder_at = ?1, updated_at = ?2
                 WHERE id = ?3 AND event_kind IS NULL",
                params![next_classification, now_text, item_id],
            )?;
            if inserted > 0 {
                claimed_classifications.push((event_id, item_id));
            }
            result.changed = true;
        }
        if claimed_classifications.len() == 1 {
            let (event_id, item_id) = claimed_classifications.remove(0);
            result
                .classification_notifications
                .push(ClassificationNotification {
                    event_ids: vec![event_id],
                    item_id: Some(item_id),
                    count: 1,
                });
        } else if !claimed_classifications.is_empty() {
            result
                .classification_notifications
                .push(ClassificationNotification {
                    count: claimed_classifications.len(),
                    event_ids: claimed_classifications
                        .into_iter()
                        .map(|(event_id, _)| event_id)
                        .collect(),
                    item_id: None,
                });
        }

        transaction.commit()?;
        Ok(result)
    }

    #[cfg(test)]
    pub fn mark_notification_submitted(&self, event_id: &str, now: DateTime<Utc>) -> AppResult<()> {
        self.mark_notification_submitted_with_note(event_id, now, None)
    }

    pub fn mark_notification_submitted_with_note(
        &self,
        event_id: &str,
        now: DateTime<Utc>,
        note: Option<&str>,
    ) -> AppResult<()> {
        let mut connection = self.connection.lock().expect("database mutex poisoned");
        let transaction = connection.transaction()?;
        let item_id = claimed_event_item_id(&transaction, event_id)?;
        let Some(item_id) = item_id else {
            transaction.commit()?;
            return Ok(());
        };
        transaction.execute(
            "UPDATE reminder_events
             SET result = 'SUBMITTED', submitted_at = ?1, error_code = ?2,
                 attempt_count = attempt_count + 1
             WHERE id = ?3 AND result = 'CLAIMED'",
            params![now.to_rfc3339(), note, event_id],
        )?;
        transaction.execute(
            "UPDATE items SET notification_failure_count = 0 WHERE id = ?1",
            [item_id],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn mark_notification_disabled(&self, event_id: &str, now: DateTime<Utc>) -> AppResult<()> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        connection.execute(
            "UPDATE reminder_events
             SET result = 'DISABLED', submitted_at = ?1, error_code = 'notifications_disabled'
             WHERE id = ?2 AND result = 'CLAIMED'",
            params![now.to_rfc3339(), event_id],
        )?;
        Ok(())
    }

    pub fn mark_notification_failed(
        &self,
        event_id: &str,
        error_code: &str,
        now: DateTime<Utc>,
    ) -> AppResult<()> {
        let mut connection = self.connection.lock().expect("database mutex poisoned");
        let transaction = connection.transaction()?;
        let item_id = claimed_event_item_id(&transaction, event_id)?;
        let Some(item_id) = item_id else {
            transaction.commit()?;
            return Ok(());
        };
        transaction.execute(
            "UPDATE reminder_events
             SET result = 'FAILED', submitted_at = ?1, error_code = ?2,
                 attempt_count = attempt_count + 1
             WHERE id = ?3 AND result = 'CLAIMED'",
            params![now.to_rfc3339(), error_code, event_id],
        )?;

        let failure_count = transaction.query_row(
            "SELECT notification_failure_count FROM items WHERE id = ?1",
            [&item_id],
            |row| row.get::<_, u32>(0),
        )?;
        if let Some(delay_minutes) = [1_i64, 5, 15].get(failure_count as usize) {
            let retry_at = (now + chrono::Duration::minutes(*delay_minutes)).to_rfc3339();
            transaction.execute(
                "UPDATE items
                 SET notification_failure_count = notification_failure_count + 1,
                     next_reminder_at = CASE
                         WHEN next_reminder_at IS NULL OR next_reminder_at > ?1 THEN ?1
                         ELSE next_reminder_at
                     END,
                     updated_at = ?2, revision = revision + 1
                 WHERE id = ?3 AND status = 'OPEN'",
                params![retry_at, now.to_rfc3339(), item_id],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn mark_classification_delivery(
        &self,
        event_ids: &[String],
        result: &str,
        error_code: Option<&str>,
        now: DateTime<Utc>,
    ) -> AppResult<()> {
        if !matches!(result, "SUBMITTED" | "FAILED" | "DISABLED") {
            return Err(AppError::Validation("分类提醒投递状态无效".into()));
        }
        let mut connection = self.connection.lock().expect("database mutex poisoned");
        let transaction = connection.transaction()?;
        for event_id in event_ids {
            transaction.execute(
                "UPDATE classification_reminder_events
                 SET result = ?1, submitted_at = ?2, error_code = ?3
                 WHERE id = ?4 AND result = 'CLAIMED'",
                params![result, now.to_rfc3339(), error_code, event_id],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn last_notification_delivery(&self) -> AppResult<Option<NotificationDelivery>> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        connection
            .query_row(
                "SELECT result, error_code, COALESCE(submitted_at, sent_at)
                 FROM reminder_events ORDER BY sent_at DESC, rowid DESC LIMIT 1",
                [],
                |row| {
                    Ok(NotificationDelivery {
                        result: row.get(0)?,
                        error_code: row.get(1)?,
                        attempted_at: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(AppError::from)
    }

    pub fn last_email_delivery(&self) -> AppResult<Option<NotificationDelivery>> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        connection
            .query_row(
                "SELECT result, error_code, attempted_at
                 FROM channel_deliveries
                 WHERE channel = 'email'
                 ORDER BY attempted_at DESC, rowid DESC LIMIT 1",
                [],
                |row| {
                    Ok(NotificationDelivery {
                        result: row.get(0)?,
                        error_code: row.get(1)?,
                        attempted_at: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(AppError::from)
    }

    pub fn onboarding_version(&self) -> AppResult<u32> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        connection
            .query_row(
                "SELECT onboarding_version FROM app_settings WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .map_err(AppError::from)
    }

    pub fn complete_onboarding(&self, version: u32) -> AppResult<()> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        connection.execute(
            "UPDATE app_settings
             SET onboarding_version = CASE
                 WHEN onboarding_version < ?1 THEN ?1
                 ELSE onboarding_version
             END
             WHERE id = 1",
            [version],
        )?;
        Ok(())
    }
}

fn scheduled_local_fields(value: &str) -> Option<(String, String)> {
    DateTime::parse_from_rfc3339(value).ok().map(|scheduled| {
        let local = scheduled.with_timezone(&Local);
        (
            local.date_naive().format("%Y-%m-%d").to_string(),
            local.time().format("%H:%M").to_string(),
        )
    })
}

fn claimed_event_item_id(
    transaction: &Transaction<'_>,
    event_id: &str,
) -> AppResult<Option<String>> {
    transaction
        .query_row(
            "SELECT item_id FROM reminder_events WHERE id = ?1 AND result = 'CLAIMED'",
            [event_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(AppError::from)
}

fn read_settings(connection: &Connection) -> AppResult<Settings> {
    let mut settings = connection
        .query_row(
            "SELECT default_due_time, workdays, overtime_interval_minutes,
                    quiet_hours_enabled, quiet_start, quiet_end, global_shortcut,
                    notifications_enabled, autostart_enabled,
                    persistent_notifications_enabled, overlay_reminders_enabled,
                    repeat_unacknowledged_enabled, unacknowledged_repeat_minutes,
                    smtp_enabled, smtp_host, smtp_port, smtp_security,
                    smtp_from, smtp_to, smtp_username, smtp_repeat_must_complete,
                    repeat_default_time_first, repeat_default_time_second
             FROM app_settings WHERE id = 1",
            [],
            |row| {
                let workdays_json: String = row.get(1)?;
                let workdays = serde_json::from_str(&workdays_json).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        workdays_json.len(),
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?;
                Ok(Settings {
                    default_due_time: row.get(0)?,
                    repeat_default_times: vec![row.get(21)?, row.get(22)?],
                    workdays,
                    overtime_interval_minutes: row.get(2)?,
                    quiet_hours_enabled: row.get::<_, i64>(3)? != 0,
                    quiet_start: row.get(4)?,
                    quiet_end: row.get(5)?,
                    global_shortcut: row.get(6)?,
                    notifications_enabled: row.get::<_, i64>(7)? != 0,
                    autostart_enabled: row.get::<_, i64>(8)? != 0,
                    persistent_notifications_enabled: row.get::<_, i64>(9)? != 0,
                    overlay_reminders_enabled: row.get::<_, i64>(10)? != 0,
                    repeat_unacknowledged_enabled: row.get::<_, i64>(11)? != 0,
                    unacknowledged_repeat_minutes: row.get(12)?,
                    smtp_enabled: row.get::<_, i64>(13)? != 0,
                    smtp_host: row.get(14)?,
                    smtp_port: row.get(15)?,
                    smtp_security: row.get(16)?,
                    smtp_from: row.get(17)?,
                    smtp_to: row.get(18)?,
                    smtp_username: row.get(19)?,
                    smtp_repeat_must_complete: row.get::<_, i64>(20)? != 0,
                    event_kind_defaults: Vec::new(),
                })
            },
        )
        .map_err(AppError::from)?;
    settings.event_kind_defaults = load_event_kind_defaults(connection)?;
    Ok(settings)
}

fn load_event_kind_defaults(connection: &Connection) -> AppResult<Vec<EventKindDefault>> {
    let mut statement = connection.prepare(
        "SELECT event_kind, reminder_plan FROM event_kind_defaults
         ORDER BY CASE event_kind
           WHEN 'ORDINARY' THEN 1 WHEN 'ONE_TIME' THEN 2 WHEN 'TODAY_MUST' THEN 3
           WHEN 'WARNING' THEN 4 WHEN 'CONTINUOUS' THEN 5 WHEN 'MONTHLY' THEN 6 ELSE 7 END",
    )?;
    statement
        .query_map([], |row| {
            Ok(EventKindDefault {
                event_kind: row.get(0)?,
                reminder_plan: row.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::from)
}

fn item_select() -> &'static str {
    "SELECT i.id, i.title, i.notes, i.status, i.category_id, c.name,
            i.due_at, i.due_local_date, i.due_local_time, i.due_source,
            i.rollover_policy, i.rollover_count, i.completion_policy,
            i.repeat_interval_minutes, i.next_reminder_at, i.reminder_paused,
            i.bypass_app_quiet_hours, i.created_at, i.updated_at, i.completed_at,
            i.deleted_at, i.event_kind, i.reminder_plan, i.important, i.time_mode,
            i.start_at, i.end_at, i.target_at, i.lead_value, i.lead_unit,
            i.cadence_value, i.cadence_unit, i.emphasis_max_per_day,
            i.repeat_time_mode, i.repeat_time_first, i.repeat_time_second
     FROM items i LEFT JOIN categories c ON c.id = i.category_id"
}

fn row_to_item(row: &Row<'_>) -> rusqlite::Result<Item> {
    Ok(Item {
        id: row.get(0)?,
        title: row.get(1)?,
        notes: row.get(2)?,
        status: row.get(3)?,
        category_id: row.get(4)?,
        category_name: row.get(5)?,
        due_at: row.get(6)?,
        due_local_date: row.get(7)?,
        due_local_time: row.get(8)?,
        due_source: row.get(9)?,
        rollover_policy: row.get(10)?,
        rollover_count: row.get(11)?,
        completion_policy: row.get(12)?,
        repeat_interval_minutes: row.get(13)?,
        next_reminder_at: row.get(14)?,
        reminder_paused: row.get::<_, i64>(15)? != 0,
        bypass_app_quiet_hours: row.get::<_, i64>(16)? != 0,
        created_at: row.get(17)?,
        updated_at: row.get(18)?,
        completed_at: row.get(19)?,
        deleted_at: row.get(20)?,
        event_kind: row.get(21)?,
        reminder_plan: row.get(22)?,
        important: row.get::<_, i64>(23)? != 0,
        time_mode: row.get(24)?,
        start_at: row.get(25)?,
        end_at: row.get(26)?,
        target_at: row.get(27)?,
        lead_value: row.get(28)?,
        lead_unit: row.get(29)?,
        cadence_value: row.get(30)?,
        cadence_unit: row.get(31)?,
        emphasis_max_per_day: row.get(32)?,
        repeat_time_mode: row.get(33)?,
        repeat_times: [
            row.get::<_, Option<String>>(34)?,
            row.get::<_, Option<String>>(35)?,
        ]
        .into_iter()
        .flatten()
        .collect(),
        tags: Vec::new(),
    })
}

fn get_item_from(connection: &Connection, id: &str) -> AppResult<Item> {
    let sql = format!("{} WHERE i.id = ?1", item_select());
    connection
        .query_row(&sql, [id], row_to_item)
        .optional()?
        .ok_or(AppError::ItemNotFound)
}

fn load_tags(connection: &Connection) -> AppResult<Vec<Tag>> {
    let mut statement = connection
        .prepare("SELECT id, name, color FROM tags WHERE archived = 0 ORDER BY position, name")?;
    let rows = statement.query_map([], |row| {
        Ok(Tag {
            id: row.get(0)?,
            name: row.get(1)?,
            color: row.get(2)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

fn move_ordered_row(
    connection: &mut Connection,
    table: &str,
    id: &str,
    direction: &str,
    label: &str,
) -> AppResult<()> {
    let (operator, ordering) = match direction {
        "up" => ("<", "DESC"),
        "down" => (">", "ASC"),
        _ => return Err(AppError::Validation("排序方向无效".into())),
    };
    let transaction = connection.transaction()?;
    let current = transaction
        .query_row(
            &format!("SELECT position FROM {table} WHERE id = ?1 AND archived = 0"),
            [id],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .ok_or_else(|| AppError::Validation(format!("找不到要排序的{label}")))?;
    let neighbor = transaction
        .query_row(
            &format!(
                "SELECT id, position FROM {table} WHERE archived = 0 AND position {operator} ?1 ORDER BY position {ordering}, name {ordering} LIMIT 1"
            ),
            [current],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()?;
    if let Some((neighbor_id, neighbor_position)) = neighbor {
        transaction.execute(
            &format!("UPDATE {table} SET position = ?1 WHERE id = ?2"),
            params![neighbor_position, id],
        )?;
        transaction.execute(
            &format!("UPDATE {table} SET position = ?1 WHERE id = ?2"),
            params![current, neighbor_id],
        )?;
    }
    transaction.commit()?;
    Ok(())
}

fn load_item_tags(connection: &Connection, item_id: &str) -> AppResult<Vec<Tag>> {
    let mut statement = connection.prepare(
        "SELECT t.id, t.name, t.color
         FROM tags t JOIN item_tags it ON it.tag_id = t.id
         WHERE it.item_id = ?1 AND t.archived = 0 ORDER BY t.position, t.name",
    )?;
    let rows = statement.query_map([item_id], |row| {
        Ok(Tag {
            id: row.get(0)?,
            name: row.get(1)?,
            color: row.get(2)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

fn ensure_category_exists(connection: &Connection, id: &str) -> AppResult<()> {
    let exists = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM categories WHERE id = ?1 AND archived = 0)",
        [id],
        |row| row.get::<_, bool>(0),
    )?;
    if !exists {
        return Err(AppError::Validation("所选类型已不存在，请重新选择".into()));
    }
    Ok(())
}

fn validate_taxonomy_links(
    connection: &Connection,
    category_id: Option<&str>,
    tag_ids: &[String],
) -> AppResult<()> {
    if let Some(category_id) = category_id {
        ensure_category_exists(connection, category_id)?;
    }
    for tag_id in tag_ids {
        let exists = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM tags WHERE id = ?1 AND archived = 0)",
            [tag_id],
            |row| row.get::<_, bool>(0),
        )?;
        if !exists {
            return Err(AppError::Validation("所选标签已不存在，请重新选择".into()));
        }
    }
    Ok(())
}

fn replace_item_tags(
    transaction: &Transaction<'_>,
    item_id: &str,
    tag_ids: &[String],
) -> AppResult<()> {
    transaction.execute("DELETE FROM item_tags WHERE item_id = ?1", [item_id])?;
    let mut unique = tag_ids.to_vec();
    unique.sort();
    unique.dedup();
    for tag_id in unique {
        transaction.execute(
            "INSERT INTO item_tags (item_id, tag_id) VALUES (?1, ?2)",
            params![item_id, tag_id],
        )?;
    }
    Ok(())
}

fn insert_item_event(
    transaction: &Transaction<'_>,
    item_id: &str,
    event_type: &str,
    data: &str,
    created_at: &str,
) -> AppResult<()> {
    transaction.execute(
        "INSERT INTO item_events (id, item_id, event_type, event_data, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            Uuid::new_v4().to_string(),
            item_id,
            event_type,
            data,
            created_at
        ],
    )?;
    Ok(())
}

#[derive(Debug, Clone)]
struct ResolvedEventConfig {
    kind: Option<String>,
    reminder_plan: String,
    important: bool,
    start_at: Option<String>,
    end_at: Option<String>,
    target_at: Option<String>,
    lead_value: Option<u32>,
    lead_unit: Option<String>,
    cadence_value: Option<u32>,
    cadence_unit: Option<String>,
    emphasis_max_per_day: u32,
    repeat_time_mode: Option<String>,
    repeat_times: Vec<String>,
}

impl ResolvedEventConfig {
    fn from_item(item: &Item) -> Self {
        Self {
            kind: item.event_kind.clone(),
            reminder_plan: item.reminder_plan.clone(),
            important: item.important,
            start_at: item.start_at.clone(),
            end_at: item.end_at.clone(),
            target_at: item.target_at.clone(),
            lead_value: item.lead_value,
            lead_unit: item.lead_unit.clone(),
            cadence_value: item.cadence_value,
            cadence_unit: item.cadence_unit.clone(),
            emphasis_max_per_day: item.emphasis_max_per_day,
            repeat_time_mode: Some(item.repeat_time_mode.clone()),
            repeat_times: item.repeat_times.clone(),
        }
    }
}

fn resolve_event_config(
    connection: &Connection,
    input: Option<&EventConfigurationInput>,
    legacy_must_complete: bool,
) -> AppResult<ResolvedEventConfig> {
    if let Some(input) = input {
        input.validate()?;
    }
    let kind = input
        .and_then(|event| event.kind.clone())
        .or_else(|| legacy_must_complete.then(|| "TODAY_MUST".to_string()));
    let reminder_plan = if let Some(plan) = input.and_then(|event| event.reminder_plan.clone()) {
        plan
    } else if let Some(kind) = &kind {
        connection.query_row(
            "SELECT reminder_plan FROM event_kind_defaults WHERE event_kind = ?1",
            [kind],
            |row| row.get(0),
        )?
    } else {
        "REPEAT".into()
    };
    let input = input.cloned().unwrap_or_default();
    let cadence_value = input
        .cadence_value
        .or_else(|| (reminder_plan == "CUSTOM").then_some(1));
    let cadence_unit = input
        .cadence_unit
        .or_else(|| (reminder_plan == "CUSTOM").then(|| "DAY".to_string()));
    Ok(ResolvedEventConfig {
        kind,
        reminder_plan,
        important: input.important,
        start_at: input.start_at,
        end_at: input.end_at,
        target_at: input.target_at,
        lead_value: input.lead_value,
        lead_unit: input.lead_unit,
        cadence_value,
        cadence_unit,
        emphasis_max_per_day: input.emphasis_max_per_day.unwrap_or(8),
        repeat_time_mode: input.repeat_time_mode,
        repeat_times: input.repeat_times,
    })
}

fn repeat_schedule_for_event(
    config: &ResolvedEventConfig,
    explicit_due: Option<&str>,
    settings: &Settings,
) -> AppResult<(String, Vec<String>)> {
    if config.reminder_plan != "REPEAT" {
        return Ok((
            config
                .repeat_time_mode
                .clone()
                .unwrap_or_else(|| "SPECIFIED".into()),
            config.repeat_times.clone(),
        ));
    }

    match config.repeat_time_mode.as_deref() {
        Some("DEFAULT") if config.repeat_times.len() == 2 => {
            let mut times = config.repeat_times.clone();
            times.sort();
            Ok(("DEFAULT".into(), times))
        }
        Some("DEFAULT") => Ok(("DEFAULT".into(), settings.repeat_default_times.clone())),
        Some("SPECIFIED") if !config.repeat_times.is_empty() => {
            Ok(("SPECIFIED".into(), vec![config.repeat_times[0].clone()]))
        }
        _ if !config.repeat_times.is_empty() => Ok((
            config.repeat_time_mode.clone().unwrap_or_else(|| {
                if config.repeat_times.len() == 2 {
                    "DEFAULT"
                } else {
                    "SPECIFIED"
                }
                .into()
            }),
            config.repeat_times.clone(),
        )),
        _ if explicit_due.is_some() => {
            let due = due_from_explicit(explicit_due.expect("checked above"))?;
            Ok((
                "SPECIFIED".into(),
                vec![due.local_time.format("%H:%M").to_string()],
            ))
        }
        _ => Ok(("DEFAULT".into(), settings.repeat_default_times.clone())),
    }
}

fn initial_due_for_event(
    config: &ResolvedEventConfig,
    explicit_due: Option<&str>,
    now: DateTime<Utc>,
    settings: &Settings,
) -> AppResult<crate::domain::DueMoment> {
    if config.reminder_plan == "REPEAT"
        && explicit_due.is_none()
        && !matches!(config.kind.as_deref(), Some("WARNING" | "CONTINUOUS"))
    {
        let (_, repeat_times) = repeat_schedule_for_event(config, explicit_due, settings)?;
        let next = next_repeat_slot_after(now, &repeat_times)?;
        let local = next.with_timezone(&Local);
        return due_for_local_date(local.date_naive(), local.time());
    }
    match config.kind.as_deref() {
        Some("WARNING") => due_from_explicit(
            config
                .target_at
                .as_deref()
                .ok_or_else(|| AppError::Validation("预警事件需要目标时间".into()))?,
        ),
        Some("CONTINUOUS") => due_from_explicit(
            config
                .start_at
                .as_deref()
                .ok_or_else(|| AppError::Validation("持续事件需要开始时间".into()))?,
        ),
        Some("TODAY_MUST") => match explicit_due {
            Some(value) => due_from_explicit(value),
            None => must_complete_due(now, settings),
        },
        _ => match explicit_due {
            Some(value) => due_from_explicit(value),
            None => next_default_due(now, settings),
        },
    }
}

fn initial_next_reminder(
    config: &ResolvedEventConfig,
    due_at: DateTime<Utc>,
    now: DateTime<Utc>,
    repeat_times: &[String],
) -> AppResult<String> {
    let candidate = if config.kind.as_deref() == Some("WARNING") {
        shift_calendar(
            due_at,
            config.lead_value.unwrap_or(1),
            config.lead_unit.as_deref().unwrap_or("DAY"),
            false,
        )?
    } else if config.reminder_plan == "REPEAT" && due_at <= now {
        next_repeat_slot_after(now, repeat_times)?
    } else {
        due_at
    };
    Ok(candidate.max(now).to_rfc3339())
}

fn next_after_delivery(
    row: &DueRow,
    now: DateTime<Utc>,
    settings: &Settings,
) -> AppResult<Option<DateTime<Utc>>> {
    let scheduled = DateTime::parse_from_rfc3339(&row.scheduled_for)
        .map_err(|_| AppError::Validation("提醒计划时间损坏".into()))?
        .with_timezone(&Utc);
    let local_time = parse_time(&row.due_local_time)?;
    let repeat_times = repeat_times_for_row(row);

    if row.event_kind.as_deref() == Some("WARNING")
        && let Some(target) = &row.target_at
    {
        let target = DateTime::parse_from_rfc3339(target)
            .map_err(|_| AppError::Validation("预警目标时间损坏".into()))?
            .with_timezone(&Utc);
        if scheduled < target && now < target {
            let next = if row.reminder_plan == "REPEAT" {
                next_repeat_slot_after(now, &repeat_times)?
            } else {
                crate::domain::next_daily_at(now, local_time)?
            };
            return Ok(Some(next.min(target)));
        }
        return Ok(None);
    }

    match row.event_kind.as_deref() {
        Some("CONTINUOUS") => {
            if row.reminder_plan == "REPEAT" {
                let next = next_repeat_slot_after(now, &repeat_times)?;
                let end = row
                    .end_at
                    .as_deref()
                    .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
                    .map(|value| value.with_timezone(&Utc));
                return Ok(end.filter(|end| next <= *end).map(|_| next));
            }
            let next = next_calendar_after(
                scheduled,
                now,
                row.cadence_value.unwrap_or(1),
                row.cadence_unit.as_deref().unwrap_or("DAY"),
            )?;
            let end = row
                .end_at
                .as_deref()
                .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
                .map(|value| value.with_timezone(&Utc));
            return Ok(end.filter(|end| next <= *end).map(|_| next));
        }
        Some(kind @ ("MONTHLY" | "YEARLY")) => {
            return match row.reminder_plan.as_str() {
                "ONCE" => next_series_after(
                    scheduled,
                    now,
                    kind,
                    row.series_anchor_month,
                    row.series_anchor_day,
                )
                .map(Some),
                "REPEAT" => next_repeat_slot_after(now, &repeat_times).map(Some),
                "EMPHASIS" => next_emphasis_at(row, now, settings).map(Some),
                "FORCE" => next_repeat_at(
                    now,
                    row.repeat_interval_minutes
                        .unwrap_or(settings.overtime_interval_minutes),
                    settings,
                    row.bypass_quiet_hours,
                )
                .map(Some),
                "CUSTOM" => match (row.cadence_value, row.cadence_unit.as_deref()) {
                    (Some(value), Some(unit)) => {
                        next_calendar_after(scheduled, now, value, unit).map(Some)
                    }
                    _ => Ok(None),
                },
                _ => Ok(None),
            };
        }
        _ => {}
    }

    match row.reminder_plan.as_str() {
        "REPEAT" => next_repeat_slot_after(now, &repeat_times).map(Some),
        "EMPHASIS" => next_emphasis_at(row, now, settings).map(Some),
        "FORCE" => next_repeat_at(
            now,
            row.repeat_interval_minutes
                .unwrap_or(settings.overtime_interval_minutes),
            settings,
            row.bypass_quiet_hours,
        )
        .map(Some),
        "CUSTOM" => match (row.cadence_value, row.cadence_unit.as_deref()) {
            (Some(value), Some(unit)) => next_calendar_after(scheduled, now, value, unit).map(Some),
            _ => Ok(None),
        },
        _ if settings.repeat_unacknowledged_enabled && row.event_kind.is_none() => next_repeat_at(
            now,
            settings.unacknowledged_repeat_minutes,
            settings,
            row.bypass_quiet_hours,
        )
        .map(Some),
        _ => Ok(None),
    }
}

fn repeat_times_for_row(row: &DueRow) -> Vec<String> {
    [
        row.repeat_time_first.clone(),
        row.repeat_time_second.clone(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .into_iter()
    .chain(
        (row.repeat_time_first.is_none() && row.repeat_time_second.is_none())
            .then(|| row.due_local_time.clone()),
    )
    .collect()
}

fn should_stop_without_delivery(row: &DueRow, now: DateTime<Utc>) -> AppResult<bool> {
    let expires_at_day_end = row.event_kind.as_deref() == Some("ONE_TIME")
        || (row.reminder_plan == "ONCE"
            && matches!(row.event_kind.as_deref(), Some("ORDINARY" | "TODAY_MUST")));
    if expires_at_day_end {
        let due_date = NaiveDate::parse_from_str(&row.due_local_date, "%Y-%m-%d")
            .map_err(|_| AppError::Validation("一次性事件的本地日期数据损坏".into()))?;
        if now.with_timezone(&Local).date_naive() > due_date {
            return Ok(true);
        }
    }
    if row.event_kind.as_deref() == Some("CONTINUOUS")
        && let Some(end) = row.end_at.as_deref()
    {
        let end = DateTime::parse_from_rfc3339(end)
            .map_err(|_| AppError::Validation("持续事件的结束时间数据损坏".into()))?
            .with_timezone(&Utc);
        if now > end {
            return Ok(true);
        }
    }
    Ok(false)
}

fn next_calendar_after(
    scheduled: DateTime<Utc>,
    now: DateTime<Utc>,
    amount: u32,
    unit: &str,
) -> AppResult<DateTime<Utc>> {
    let mut next = shift_calendar(scheduled, amount, unit, true)?;
    for _ in 0..512 {
        if next > now {
            return Ok(next);
        }
        next = shift_calendar(next, amount, unit, true)?;
    }
    Err(AppError::Validation(
        "无法在有限次计算内找到下一次提醒".into(),
    ))
}

fn series_anchor(kind: Option<&str>, date: NaiveDate) -> (Option<u32>, Option<u32>) {
    match kind {
        Some("MONTHLY") => (None, Some(date.day())),
        Some("YEARLY") => (Some(date.month()), Some(date.day())),
        _ => (None, None),
    }
}

fn next_series_after(
    scheduled: DateTime<Utc>,
    now: DateTime<Utc>,
    kind: &str,
    anchor_month: Option<u32>,
    anchor_day: Option<u32>,
) -> AppResult<DateTime<Utc>> {
    let local = scheduled.with_timezone(&Local);
    let mut year = local.year();
    let mut month = local.month();
    let day = anchor_day.unwrap_or(local.day());
    for _ in 0..512 {
        match kind {
            "MONTHLY" => {
                if month == 12 {
                    year = year
                        .checked_add(1)
                        .ok_or_else(|| AppError::Validation("无法计算下一年".into()))?;
                    month = 1;
                } else {
                    month += 1;
                }
            }
            "YEARLY" => {
                year = year
                    .checked_add(1)
                    .ok_or_else(|| AppError::Validation("无法计算下一年".into()))?;
                month = anchor_month.unwrap_or(local.month());
            }
            _ => return Err(AppError::Validation("周期事件类型无效".into())),
        }
        let last_day = days_in_month(year, month)?;
        let date = NaiveDate::from_ymd_opt(year, month, day.min(last_day))
            .ok_or_else(|| AppError::Validation("无法计算下一次周期日期".into()))?;
        let candidate = due_for_local_date(date, local.time())?.utc;
        if candidate > now {
            return Ok(candidate);
        }
    }
    Err(AppError::Validation(
        "无法在有限次计算内找到下一次周期".into(),
    ))
}

fn days_in_month(year: i32, month: u32) -> AppResult<u32> {
    let (next_year, next_month) = if month == 12 {
        (
            year.checked_add(1)
                .ok_or_else(|| AppError::Validation("无法计算下一年".into()))?,
            1,
        )
    } else {
        (year, month + 1)
    };
    let first_next = NaiveDate::from_ymd_opt(next_year, next_month, 1)
        .ok_or_else(|| AppError::Validation("无法计算月末日期".into()))?;
    Ok(first_next
        .pred_opt()
        .ok_or_else(|| AppError::Validation("无法计算月末日期".into()))?
        .day())
}

fn next_emphasis_at(
    row: &DueRow,
    now: DateTime<Utc>,
    settings: &Settings,
) -> AppResult<DateTime<Utc>> {
    let local_date = now
        .with_timezone(&Local)
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();
    let count = if row.emphasis_count_local_date.as_deref() == Some(local_date.as_str()) {
        row.emphasis_count.saturating_add(1)
    } else {
        1
    };
    if count >= row.emphasis_max_per_day {
        next_daily_at(now, parse_time(&row.due_local_time)?)
    } else {
        next_repeat_at(
            now,
            row.repeat_interval_minutes
                .unwrap_or(settings.overtime_interval_minutes),
            settings,
            row.bypass_quiet_hours,
        )
    }
}

fn update_existing_default_items(
    transaction: &Transaction<'_>,
    settings: &Settings,
    now: DateTime<Utc>,
) -> AppResult<()> {
    let rows = {
        let mut statement = transaction.prepare(
            "SELECT id, due_local_date FROM items
             WHERE status = 'OPEN' AND completion_policy = 'NORMAL'
               AND due_source IN ('DEFAULT_EOD', 'ROLLED_OVER')",
        )?;
        let mapped = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        mapped.collect::<Result<Vec<_>, _>>()?
    };
    let target_time = parse_time(&settings.default_due_time)?;
    let timestamp = now.to_rfc3339();

    for (id, date_text) in rows {
        let date = NaiveDate::parse_from_str(&date_text, "%Y-%m-%d")
            .map_err(|_| AppError::Validation("已有事项的本地日期数据损坏".into()))?;
        let candidate = due_for_local_date(date, target_time)?;
        let due = if candidate.utc <= now {
            next_default_due(now, settings)?
        } else {
            candidate
        };
        transaction.execute(
            "UPDATE items SET due_at = ?1, due_local_date = ?2, due_local_time = ?3,
                next_reminder_at = ?1, updated_at = ?4, revision = revision + 1 WHERE id = ?5",
            params![
                due.utc.to_rfc3339(),
                due.local_date.format("%Y-%m-%d").to_string(),
                due.local_time.format("%H:%M").to_string(),
                timestamp,
                id,
            ],
        )?;
        insert_item_event(transaction, &id, "DEFAULT_TIME_UPDATED", "{}", &timestamp)?;
    }
    Ok(())
}

fn repair_future_must_complete_items(
    transaction: &Transaction<'_>,
    now: DateTime<Utc>,
) -> AppResult<usize> {
    let local = now.with_timezone(&Local);
    let today = local.date_naive().format("%Y-%m-%d").to_string();
    let ids = {
        let mut statement = transaction.prepare(
            "SELECT id FROM items
             WHERE status = 'OPEN' AND completion_policy = 'MUST_COMPLETE_TODAY'
               AND due_source = 'DEFAULT_EOD' AND due_local_date > ?1",
        )?;
        let rows = statement.query_map([&today], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    if ids.is_empty() {
        return Ok(0);
    }

    let timestamp = now.to_rfc3339();
    let local_time = local.time().format("%H:%M").to_string();
    for id in &ids {
        transaction.execute(
            "UPDATE items SET due_at = ?1, due_local_date = ?2, due_local_time = ?3,
                next_reminder_at = ?1, updated_at = ?1, revision = revision + 1 WHERE id = ?4",
            params![timestamp, today, local_time, id],
        )?;
        insert_item_event(
            transaction,
            id,
            "MUST_COMPLETE_DUE_REPAIRED",
            "{}",
            &timestamp,
        )?;
    }
    Ok(ids.len())
}

fn rollover_default_items(
    transaction: &Transaction<'_>,
    settings: &Settings,
    now: DateTime<Utc>,
) -> AppResult<usize> {
    let today = now
        .with_timezone(&Local)
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();
    let ids = {
        let mut statement = transaction.prepare(
            "SELECT id FROM items
             WHERE status = 'OPEN' AND completion_policy = 'NORMAL'
               AND reminder_plan = 'ONCE'
               AND rollover_policy = 'NEXT_WORKDAY_EOD'
               AND due_source IN ('DEFAULT_EOD', 'ROLLED_OVER')
               AND due_local_date < ?1",
        )?;
        let rows = statement.query_map([today], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    if ids.is_empty() {
        return Ok(0);
    }

    let due = next_default_due(now, settings)?;
    let timestamp = now.to_rfc3339();
    for id in &ids {
        transaction.execute(
            "UPDATE items SET due_at = ?1, due_local_date = ?2, due_local_time = ?3,
                due_source = 'ROLLED_OVER', rollover_count = rollover_count + 1,
                next_reminder_at = ?1, updated_at = ?4, revision = revision + 1 WHERE id = ?5",
            params![
                due.utc.to_rfc3339(),
                due.local_date.format("%Y-%m-%d").to_string(),
                due.local_time.format("%H:%M").to_string(),
                timestamp,
                id,
            ],
        )?;
        insert_item_event(transaction, id, "ROLLED_OVER", "{}", &timestamp)?;
    }
    Ok(ids.len())
}

fn bool_to_int(value: bool) -> i64 {
    i64::from(value)
}

fn schema_version(connection: &Connection) -> AppResult<i64> {
    connection
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

fn backup_directory(database_path: &Path) -> AppResult<PathBuf> {
    let parent = database_path
        .parent()
        .ok_or_else(|| AppError::Validation("数据目录无效".into()))?;
    Ok(parent.join("backups"))
}

fn pending_restore_path(database_path: &Path) -> AppResult<PathBuf> {
    let parent = database_path
        .parent()
        .ok_or_else(|| AppError::Validation("数据目录无效".into()))?;
    Ok(parent.join("shanji.restore-pending.db"))
}

fn create_upgrade_backup(
    connection: &Connection,
    database_path: &Path,
    from_version: i64,
) -> AppResult<PathBuf> {
    create_timestamped_backup(
        connection,
        database_path,
        from_version,
        &format!("before-v{LATEST_SCHEMA_VERSION}"),
    )
}

fn create_timestamped_backup(
    connection: &Connection,
    database_path: &Path,
    schema_version: i64,
    reason: &str,
) -> AppResult<PathBuf> {
    let directory = backup_directory(database_path)?;
    fs::create_dir_all(&directory)?;
    let timestamp = Utc::now().format("%Y%m%d-%H%M%S-%3f");
    let path = directory.join(format!(
        "shanji-{timestamp}-schema-{schema_version}-{reason}.db"
    ));
    connection.execute("VACUUM INTO ?1", [path.to_string_lossy().as_ref()])?;
    Ok(path)
}

fn summarize_connection(connection: &Connection, path: &Path) -> AppResult<DataFileSummary> {
    let item_count = connection
        .query_row("SELECT COUNT(*) FROM items", [], |row| row.get::<_, i64>(0))?
        .max(0) as u64;
    let updated_at = file_updated_at(path);
    Ok(DataFileSummary {
        path: path.to_string_lossy().into_owned(),
        item_count,
        schema_version: schema_version(connection)?,
        updated_at,
    })
}

fn inspect_database_file(path: &Path) -> AppResult<DataFileSummary> {
    if !path.is_file() || fs::metadata(path)?.len() == 0 {
        return Err(AppError::Validation("数据文件为空或不存在".into()));
    }
    let connection = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let integrity =
        connection.query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))?;
    if integrity != "ok" {
        return Err(AppError::Validation("数据文件未通过完整性检查".into()));
    }
    summarize_connection(&connection, path)
}

fn file_updated_at(path: &Path) -> Option<String> {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.duration_since(SystemTime::UNIX_EPOCH).ok())
        .and_then(|duration| {
            DateTime::<Utc>::from_timestamp(duration.as_secs() as i64, duration.subsec_nanos())
        })
        .map(|timestamp| timestamp.to_rfc3339())
}

fn latest_database_file(directory: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(directory).ok()?;
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "db"))
        .max_by_key(|path| {
            fs::metadata(path)
                .and_then(|metadata| metadata.modified())
                .ok()
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at_local(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> DateTime<Utc> {
        Local
            .with_ymd_and_hms(year, month, day, hour, minute, 0)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn event(kind: &str, reminder_plan: &str) -> EventConfigurationInput {
        EventConfigurationInput {
            kind: Some(kind.into()),
            reminder_plan: Some(reminder_plan.into()),
            important: false,
            start_at: None,
            end_at: None,
            target_at: None,
            lead_value: None,
            lead_unit: None,
            cadence_value: None,
            cadence_unit: None,
            emphasis_max_per_day: None,
            repeat_time_mode: None,
            repeat_times: Vec::new(),
        }
    }

    fn create_configured_item(
        database: &Database,
        title: &str,
        due: DateTime<Utc>,
        event: EventConfigurationInput,
        now: DateTime<Utc>,
    ) -> Item {
        database
            .create_item(
                &CreateItemInput {
                    title: title.into(),
                    notes: String::new(),
                    category_id: Some("work".into()),
                    due_at: Some(due.to_rfc3339()),
                    must_complete_today: false,
                    repeat_interval_minutes: None,
                    tag_ids: Vec::new(),
                    event: Some(event),
                },
                now,
            )
            .unwrap()
    }

    #[test]
    fn v5_upgrade_keeps_type_and_tag_as_separate_dimensions() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("shanji.db");
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch(MIGRATION_0001).unwrap();
        connection.execute_batch(MIGRATION_0002).unwrap();
        connection.execute_batch(MIGRATION_0003).unwrap();
        connection.execute_batch(MIGRATION_0004).unwrap();
        connection.execute_batch(MIGRATION_0005).unwrap();
        connection
            .execute(
                "INSERT INTO tags (id, name, color, position) VALUES ('project-a', '项目 A', '#B06C49', 10)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO items (
                    id, title, notes, status, category_id, due_at, due_local_date,
                    due_local_time, due_source, rollover_policy, completion_policy,
                    next_reminder_at, created_at, updated_at
                 ) VALUES (
                    'legacy-item', '旧版事项', '', 'OPEN', 'work',
                    '2026-09-01T10:00:00+00:00', '2026-09-01', '18:00',
                    'EXPLICIT', 'NONE', 'NORMAL', '2026-09-01T10:00:00+00:00',
                    '2026-09-01T09:00:00+00:00', '2026-09-01T09:00:00+00:00'
                 )",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO item_tags (item_id, tag_id) VALUES ('legacy-item', 'project-a')",
                [],
            )
            .unwrap();
        drop(connection);

        let database = Database::open(&path).unwrap();
        let item = database.get_item("legacy-item").unwrap();
        assert_eq!(item.event_kind.as_deref(), Some("ORDINARY"));
        assert_eq!(item.reminder_plan, "ONCE");
        assert_eq!(item.category_id.as_deref(), Some("work"));
        assert_eq!(item.category_name.as_deref(), Some("工作"));
        assert_eq!(item.tags.len(), 1);
        assert_eq!(item.tags[0].id, "project-a");
        assert_eq!(database.list_tags().unwrap().len(), 1);
        assert!(
            database
                .list_tags()
                .unwrap()
                .iter()
                .all(|tag| !tag.id.starts_with("legacy-category-"))
        );
    }

    #[test]
    fn event_kind_conversion_preserves_type_and_tags() {
        let database = Database::in_memory().unwrap();
        let tag = database
            .create_tag(&TaxonomyInput {
                name: "项目 A".into(),
                color: "#B06C49".into(),
            })
            .unwrap();
        let due = at_local(2026, 9, 2, 18, 0);
        let item = database
            .create_item(
                &CreateItemInput {
                    title: "精细管理".into(),
                    notes: String::new(),
                    category_id: Some("work".into()),
                    due_at: Some(due.to_rfc3339()),
                    must_complete_today: false,
                    repeat_interval_minutes: None,
                    tag_ids: vec![tag.id.clone()],
                    event: Some(event("ORDINARY", "REPEAT")),
                },
                at_local(2026, 9, 2, 9, 0),
            )
            .unwrap();
        let updated = database
            .update_item(
                &item.id,
                &UpdateItemInput {
                    title: item.title.clone(),
                    notes: item.notes.clone(),
                    category_id: item.category_id.clone(),
                    tag_ids: vec![tag.id.clone()],
                    due_at: due.to_rfc3339(),
                    must_complete_today: false,
                    repeat_interval_minutes: None,
                    event: Some(event("YEARLY", "ONCE")),
                },
                at_local(2026, 9, 2, 10, 0),
            )
            .unwrap();
        assert_eq!(updated.event_kind.as_deref(), Some("YEARLY"));
        assert_eq!(updated.category_id.as_deref(), Some("work"));
        assert_eq!(updated.tags[0].id, tag.id);
    }

    #[test]
    fn ordinary_repeats_daily_but_one_time_expires_after_its_local_day() {
        let database = Database::in_memory().unwrap();
        let due = at_local(2026, 9, 1, 18, 0);
        let ordinary = create_configured_item(
            &database,
            "每日回顾",
            due,
            event("ORDINARY", "REPEAT"),
            due - chrono::Duration::hours(1),
        );
        let one_time = create_configured_item(
            &database,
            "一次性提醒",
            due,
            event("ONE_TIME", "ONCE"),
            due - chrono::Duration::hours(1),
        );

        let result = database.process_due(at_local(2026, 9, 2, 8, 0)).unwrap();
        assert!(
            result
                .notifications
                .iter()
                .any(|entry| entry.item_id == ordinary.id)
        );
        assert!(
            !result
                .notifications
                .iter()
                .any(|entry| entry.item_id == one_time.id)
        );
        assert_eq!(
            database.get_item(&one_time.id).unwrap().next_reminder_at,
            None
        );
        assert_eq!(
            database.get_item(&ordinary.id).unwrap().next_reminder_at,
            Some(at_local(2026, 9, 2, 18, 0).to_rfc3339())
        );
    }

    #[test]
    fn today_must_with_once_plan_stops_after_the_day_ends() {
        let database = Database::in_memory().unwrap();
        let due = at_local(2026, 9, 1, 22, 0);
        let item = create_configured_item(
            &database,
            "今日必做但仅提醒一次",
            due,
            event("TODAY_MUST", "ONCE"),
            due - chrono::Duration::hours(1),
        );
        let result = database.process_due(at_local(2026, 9, 2, 8, 0)).unwrap();
        assert!(result.notifications.is_empty());
        assert_eq!(database.get_item(&item.id).unwrap().next_reminder_at, None);
    }

    #[test]
    fn warning_repeats_at_daily_slots_from_lead_time_until_target() {
        let database = Database::in_memory().unwrap();
        let target = at_local(2026, 9, 10, 10, 0);
        let mut warning = event("WARNING", "REPEAT");
        warning.target_at = Some(target.to_rfc3339());
        warning.lead_value = Some(3);
        warning.lead_unit = Some("DAY".into());
        let item = database
            .create_item(
                &CreateItemInput {
                    title: "证书到期预警".into(),
                    notes: String::new(),
                    category_id: Some("work".into()),
                    due_at: None,
                    must_complete_today: false,
                    repeat_interval_minutes: None,
                    tag_ids: Vec::new(),
                    event: Some(warning),
                },
                at_local(2026, 9, 6, 9, 0),
            )
            .unwrap();
        assert_eq!(
            item.next_reminder_at,
            Some(at_local(2026, 9, 7, 10, 0).to_rfc3339())
        );
        let first = database.process_due(at_local(2026, 9, 7, 10, 0)).unwrap();
        assert_eq!(first.notifications.len(), 1);
        assert_eq!(
            database.get_item(&item.id).unwrap().next_reminder_at,
            Some(at_local(2026, 9, 7, 17, 0).to_rfc3339())
        );
        database
            .mark_notification_submitted(
                &first.notifications[0].event_id,
                at_local(2026, 9, 7, 10, 0),
            )
            .unwrap();
        database
            .connection
            .lock()
            .expect("database mutex poisoned")
            .execute(
                "UPDATE items SET next_reminder_at = ?1 WHERE id = ?2",
                params![target.to_rfc3339(), item.id],
            )
            .unwrap();
        let target_delivery = database.process_due(target).unwrap();
        assert_eq!(target_delivery.notifications.len(), 1);
        assert_eq!(database.get_item(&item.id).unwrap().next_reminder_at, None);
    }

    #[test]
    fn default_repeat_item_uses_two_snapshot_slots_and_skips_missed_history() {
        let database = Database::in_memory().unwrap();
        let created = database
            .create_item(
                &CreateItemInput {
                    title: "双时点提醒".into(),
                    notes: String::new(),
                    category_id: None,
                    due_at: None,
                    must_complete_today: false,
                    repeat_interval_minutes: None,
                    tag_ids: Vec::new(),
                    event: Some(event("ORDINARY", "REPEAT")),
                },
                at_local(2026, 9, 1, 9, 0),
            )
            .unwrap();
        assert_eq!(created.repeat_time_mode, "DEFAULT");
        assert_eq!(created.repeat_times, vec!["10:00", "17:00"]);
        assert_eq!(
            created.next_reminder_at,
            Some(at_local(2026, 9, 1, 10, 0).to_rfc3339())
        );

        let first = database.process_due(at_local(2026, 9, 1, 10, 1)).unwrap();
        assert_eq!(first.notifications.len(), 1);
        assert_eq!(first.notifications[0].due_local_time, "10:00");
        database
            .mark_notification_submitted(
                &first.notifications[0].event_id,
                at_local(2026, 9, 1, 10, 1),
            )
            .unwrap();
        assert_eq!(
            database.get_item(&created.id).unwrap().next_reminder_at,
            Some(at_local(2026, 9, 1, 17, 0).to_rfc3339())
        );

        let after_sleep = database.process_due(at_local(2026, 9, 1, 22, 0)).unwrap();
        assert_eq!(after_sleep.notifications.len(), 1);
        assert_eq!(after_sleep.notifications[0].due_local_time, "17:00");
        assert_eq!(
            database.get_item(&created.id).unwrap().next_reminder_at,
            Some(at_local(2026, 9, 2, 10, 0).to_rfc3339())
        );
    }

    #[test]
    fn changing_repeat_defaults_does_not_rewrite_existing_item_snapshot() {
        let database = Database::in_memory().unwrap();
        let now = at_local(2026, 9, 1, 9, 0);
        let existing = database
            .create_item(
                &CreateItemInput {
                    title: "保留旧时间".into(),
                    notes: String::new(),
                    category_id: None,
                    due_at: None,
                    must_complete_today: false,
                    repeat_interval_minutes: None,
                    tag_ids: Vec::new(),
                    event: Some(event("ORDINARY", "REPEAT")),
                },
                now,
            )
            .unwrap();
        let current = database.get_settings().unwrap();
        database
            .update_settings(
                &UpdateSettingsInput {
                    default_due_time: current.default_due_time,
                    repeat_default_times: vec!["09:30".into(), "16:30".into()],
                    workdays: current.workdays,
                    overtime_interval_minutes: current.overtime_interval_minutes,
                    quiet_hours_enabled: current.quiet_hours_enabled,
                    quiet_start: current.quiet_start,
                    quiet_end: current.quiet_end,
                    global_shortcut: current.global_shortcut,
                    notifications_enabled: current.notifications_enabled,
                    autostart_enabled: current.autostart_enabled,
                    update_existing_default_items: false,
                    persistent_notifications_enabled: current.persistent_notifications_enabled,
                    overlay_reminders_enabled: current.overlay_reminders_enabled,
                    repeat_unacknowledged_enabled: current.repeat_unacknowledged_enabled,
                    unacknowledged_repeat_minutes: current.unacknowledged_repeat_minutes,
                    smtp_enabled: current.smtp_enabled,
                    smtp_host: current.smtp_host,
                    smtp_port: current.smtp_port,
                    smtp_security: current.smtp_security,
                    smtp_from: current.smtp_from,
                    smtp_to: current.smtp_to,
                    smtp_username: current.smtp_username,
                    smtp_repeat_must_complete: current.smtp_repeat_must_complete,
                    smtp_password: None,
                    event_kind_defaults: current.event_kind_defaults,
                },
                now,
            )
            .unwrap();
        assert_eq!(
            database.get_item(&existing.id).unwrap().repeat_times,
            vec!["10:00", "17:00"]
        );
        let mut preserved_event = event("ORDINARY", "REPEAT");
        preserved_event.repeat_time_mode = Some("DEFAULT".into());
        preserved_event.repeat_times = vec!["10:00".into(), "17:00".into()];
        let edited = database
            .update_item(
                &existing.id,
                &UpdateItemInput {
                    title: "只修改标题".into(),
                    notes: String::new(),
                    category_id: None,
                    tag_ids: Vec::new(),
                    due_at: existing.due_at,
                    must_complete_today: false,
                    repeat_interval_minutes: None,
                    event: Some(preserved_event),
                },
                at_local(2026, 9, 1, 9, 5),
            )
            .unwrap();
        assert_eq!(edited.repeat_times, vec!["10:00", "17:00"]);
    }

    #[test]
    fn monthly_series_keeps_its_day_anchor_and_complete_current_does_not_skip() {
        let database = Database::in_memory().unwrap();
        let january = at_local(2026, 1, 31, 9, 0);
        let item = create_configured_item(
            &database,
            "月末结账",
            january,
            event("MONTHLY", "ONCE"),
            at_local(2026, 1, 10, 9, 0),
        );

        let after_early_completion = database
            .complete_series_occurrence(&item.id, at_local(2026, 1, 10, 10, 0))
            .unwrap();
        assert_eq!(
            after_early_completion.due_at,
            at_local(2026, 2, 28, 9, 0).to_rfc3339()
        );

        let february = at_local(2026, 2, 28, 9, 0);
        let notification = database
            .process_due(february)
            .unwrap()
            .notifications
            .remove(0);
        database
            .mark_notification_submitted(&notification.event_id, february)
            .unwrap();
        assert_eq!(
            database.get_item(&item.id).unwrap().due_at,
            at_local(2026, 3, 31, 9, 0).to_rfc3339()
        );

        let after_current_completion = database
            .complete_series_occurrence(&item.id, february + chrono::Duration::minutes(1))
            .unwrap();
        assert_eq!(
            after_current_completion.due_at,
            at_local(2026, 3, 31, 9, 0).to_rfc3339()
        );
        assert!(
            database
                .list_pending_reminders(february + chrono::Duration::minutes(1))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn monthly_force_plan_repeats_current_occurrence_until_it_is_completed() {
        let database = Database::in_memory().unwrap();
        let january = at_local(2026, 1, 31, 9, 0);
        let item = database
            .create_item(
                &CreateItemInput {
                    title: "月度强制盘点".into(),
                    notes: String::new(),
                    category_id: Some("work".into()),
                    due_at: Some(january.to_rfc3339()),
                    must_complete_today: false,
                    repeat_interval_minutes: Some(30),
                    tag_ids: Vec::new(),
                    event: Some(event("MONTHLY", "FORCE")),
                },
                at_local(2026, 1, 30, 9, 0),
            )
            .unwrap();

        let notification = database
            .process_due(january)
            .unwrap()
            .notifications
            .remove(0);
        database
            .mark_notification_submitted(&notification.event_id, january)
            .unwrap();
        let waiting = database.get_item(&item.id).unwrap();
        assert_eq!(waiting.due_at, january.to_rfc3339());
        assert_eq!(
            waiting.next_reminder_at,
            Some((january + chrono::Duration::minutes(30)).to_rfc3339())
        );

        let completed = database
            .complete_series_occurrence(&item.id, january + chrono::Duration::minutes(1))
            .unwrap();
        assert_eq!(completed.due_at, at_local(2026, 2, 28, 9, 0).to_rfc3339());
        assert_eq!(
            completed.next_reminder_at,
            Some(at_local(2026, 2, 28, 9, 0).to_rfc3339())
        );
    }

    #[test]
    fn continuous_event_skips_missed_cycles_and_stops_after_end() {
        let database = Database::in_memory().unwrap();
        let start = at_local(2026, 9, 1, 9, 0);
        let end = at_local(2026, 9, 10, 9, 0);
        let mut continuous = event("CONTINUOUS", "CUSTOM");
        continuous.start_at = Some(start.to_rfc3339());
        continuous.end_at = Some(end.to_rfc3339());
        continuous.cadence_value = Some(3);
        continuous.cadence_unit = Some("DAY".into());
        let item = database
            .create_item(
                &CreateItemInput {
                    title: "持续备考".into(),
                    notes: String::new(),
                    category_id: Some("personal".into()),
                    due_at: None,
                    must_complete_today: false,
                    repeat_interval_minutes: None,
                    tag_ids: Vec::new(),
                    event: Some(continuous),
                },
                start - chrono::Duration::hours(1),
            )
            .unwrap();
        let first = database.process_due(start).unwrap().notifications.remove(0);
        database
            .mark_notification_submitted(&first.event_id, start)
            .unwrap();
        let wake = at_local(2026, 9, 8, 9, 0);
        let result = database.process_due(wake).unwrap();
        assert_eq!(result.notifications.len(), 1);
        database
            .mark_notification_submitted(&result.notifications[0].event_id, wake)
            .unwrap();
        assert_eq!(
            database.get_item(&item.id).unwrap().next_reminder_at,
            Some(end.to_rfc3339())
        );
        let after_end = database.process_due(at_local(2026, 9, 11, 9, 0)).unwrap();
        assert!(after_end.notifications.is_empty());
        assert_eq!(database.get_item(&item.id).unwrap().next_reminder_at, None);
    }

    #[test]
    fn emphasis_honors_the_daily_delivery_limit() {
        let database = Database::in_memory().unwrap();
        let due = at_local(2026, 9, 1, 18, 0);
        let mut emphasis = event("TODAY_MUST", "EMPHASIS");
        emphasis.emphasis_max_per_day = Some(2);
        let item = database
            .create_item(
                &CreateItemInput {
                    title: "强调事项".into(),
                    notes: String::new(),
                    category_id: Some("work".into()),
                    due_at: Some(due.to_rfc3339()),
                    must_complete_today: false,
                    repeat_interval_minutes: Some(15),
                    tag_ids: Vec::new(),
                    event: Some(emphasis),
                },
                due - chrono::Duration::hours(1),
            )
            .unwrap();
        database.process_due(due).unwrap();
        database
            .process_due(due + chrono::Duration::minutes(15))
            .unwrap();
        assert_eq!(
            database.get_item(&item.id).unwrap().next_reminder_at,
            Some(at_local(2026, 9, 2, 18, 0).to_rfc3339())
        );
    }

    #[test]
    fn multiple_unclassified_items_share_one_classification_notification() {
        let database = Database::in_memory().unwrap();
        let due = at_local(2026, 9, 1, 18, 0);
        for title in ["待分类一", "待分类二"] {
            database
                .create_item(
                    &CreateItemInput {
                        title: title.into(),
                        notes: String::new(),
                        category_id: None,
                        due_at: Some(due.to_rfc3339()),
                        must_complete_today: false,
                        repeat_interval_minutes: None,
                        tag_ids: Vec::new(),
                        event: None,
                    },
                    due - chrono::Duration::hours(1),
                )
                .unwrap();
        }
        let result = database.process_due(due).unwrap();
        assert_eq!(result.classification_notifications.len(), 1);
        assert_eq!(result.classification_notifications[0].count, 2);
        assert!(result.classification_notifications[0].item_id.is_none());
    }

    #[test]
    fn create_default_item_and_complete_in_one_store() {
        let database = Database::in_memory().unwrap();
        let item = database
            .create_item(
                &CreateItemInput {
                    title: "提交报销".into(),
                    notes: String::new(),
                    category_id: Some("work".into()),
                    due_at: None,
                    must_complete_today: false,
                    repeat_interval_minutes: None,
                    tag_ids: Vec::new(),
                    event: Some(event("ORDINARY", "ONCE")),
                },
                at_local(2026, 8, 27, 10, 0),
            )
            .unwrap();
        assert_eq!(item.due_source, "DEFAULT_EOD");
        assert_eq!(item.due_local_time, "18:00");

        let completed = database
            .set_item_completed(&item.id, true, at_local(2026, 8, 27, 11, 0))
            .unwrap();
        assert_eq!(completed.status, "DONE");
        assert!(completed.next_reminder_at.is_none());
    }

    #[test]
    fn changing_default_time_for_new_items_keeps_existing_due_time() {
        let database = Database::in_memory().unwrap();
        let now = at_local(2026, 8, 27, 10, 0);
        let existing = database
            .create_item(
                &CreateItemInput {
                    title: "已有事项".into(),
                    notes: String::new(),
                    category_id: None,
                    due_at: None,
                    must_complete_today: false,
                    repeat_interval_minutes: None,
                    tag_ids: Vec::new(),
                    event: Some(event("ORDINARY", "ONCE")),
                },
                now,
            )
            .unwrap();
        let current = database.get_settings().unwrap();
        database
            .update_settings(
                &UpdateSettingsInput {
                    default_due_time: "17:30".into(),
                    repeat_default_times: current.repeat_default_times,
                    workdays: current.workdays,
                    overtime_interval_minutes: current.overtime_interval_minutes,
                    quiet_hours_enabled: current.quiet_hours_enabled,
                    quiet_start: current.quiet_start,
                    quiet_end: current.quiet_end,
                    global_shortcut: current.global_shortcut,
                    notifications_enabled: current.notifications_enabled,
                    autostart_enabled: current.autostart_enabled,
                    update_existing_default_items: false,
                    persistent_notifications_enabled: current.persistent_notifications_enabled,
                    overlay_reminders_enabled: current.overlay_reminders_enabled,
                    repeat_unacknowledged_enabled: current.repeat_unacknowledged_enabled,
                    unacknowledged_repeat_minutes: current.unacknowledged_repeat_minutes,
                    smtp_enabled: current.smtp_enabled,
                    smtp_host: current.smtp_host,
                    smtp_port: current.smtp_port,
                    smtp_security: current.smtp_security,
                    smtp_from: current.smtp_from,
                    smtp_to: current.smtp_to,
                    smtp_username: current.smtp_username,
                    smtp_repeat_must_complete: current.smtp_repeat_must_complete,
                    smtp_password: None,
                    event_kind_defaults: current.event_kind_defaults,
                },
                now,
            )
            .unwrap();

        assert_eq!(
            database.get_item(&existing.id).unwrap().due_local_time,
            "18:00"
        );
        let new_item = database
            .create_item(
                &CreateItemInput {
                    title: "新事项".into(),
                    notes: String::new(),
                    category_id: None,
                    due_at: None,
                    must_complete_today: false,
                    repeat_interval_minutes: None,
                    tag_ids: Vec::new(),
                    event: Some(event("ORDINARY", "ONCE")),
                },
                now,
            )
            .unwrap();
        assert_eq!(new_item.due_local_time, "17:30");
    }

    #[test]
    fn must_complete_item_repeats_instead_of_rolling_over() {
        let database = Database::in_memory().unwrap();
        let due = at_local(2026, 8, 27, 18, 0);
        let item = database
            .create_item(
                &CreateItemInput {
                    title: "加班发布".into(),
                    notes: String::new(),
                    category_id: Some("work".into()),
                    due_at: Some(due.to_rfc3339()),
                    must_complete_today: true,
                    repeat_interval_minutes: Some(30),
                    tag_ids: Vec::new(),
                    event: None,
                },
                at_local(2026, 8, 27, 17, 0),
            )
            .unwrap();

        let result = database.process_due(at_local(2026, 8, 27, 18, 1)).unwrap();
        assert_eq!(result.notifications.len(), 1);
        let updated = database.get_item(&item.id).unwrap();
        assert_eq!(updated.completion_policy, "MUST_COMPLETE_TODAY");
        assert_eq!(updated.rollover_count, 0);
        assert!(updated.next_reminder_at.is_some());
    }

    #[test]
    fn must_complete_created_after_default_time_notifies_immediately() {
        let database = Database::in_memory().unwrap();
        let now = at_local(2026, 8, 27, 22, 11);
        let item = database
            .create_item(
                &CreateItemInput {
                    title: "深夜必须完成".into(),
                    notes: String::new(),
                    category_id: None,
                    due_at: None,
                    must_complete_today: true,
                    repeat_interval_minutes: Some(15),
                    tag_ids: Vec::new(),
                    event: None,
                },
                now,
            )
            .unwrap();

        assert_eq!(item.due_local_date, "2026-08-27");
        assert_eq!(item.due_local_time, "22:11");
        let result = database.process_due(now).unwrap();
        assert_eq!(result.notifications.len(), 1);
        assert!(
            database
                .get_item(&item.id)
                .unwrap()
                .next_reminder_at
                .is_some()
        );
    }

    #[test]
    fn old_must_complete_item_misplaced_on_future_day_is_repaired() {
        let database = Database::in_memory().unwrap();
        let item = database
            .create_item(
                &CreateItemInput {
                    title: "旧版误排事项".into(),
                    notes: String::new(),
                    category_id: None,
                    due_at: None,
                    must_complete_today: true,
                    repeat_interval_minutes: Some(15),
                    tag_ids: Vec::new(),
                    event: None,
                },
                at_local(2026, 8, 27, 10, 0),
            )
            .unwrap();

        {
            let connection = database.connection.lock().expect("database mutex poisoned");
            let wrong_due = at_local(2026, 8, 28, 15, 0).to_rfc3339();
            connection
                .execute(
                    "UPDATE items SET due_at = ?1, due_local_date = '2026-08-28',
                        due_local_time = '15:00', next_reminder_at = ?1 WHERE id = ?2",
                    params![wrong_due, item.id],
                )
                .unwrap();
        }

        let now = at_local(2026, 8, 27, 22, 20);
        let result = database.process_due(now).unwrap();
        assert_eq!(result.notifications.len(), 1);
        let repaired = database.get_item(&item.id).unwrap();
        assert_eq!(repaired.due_local_date, "2026-08-27");
        assert_eq!(repaired.due_local_time, "22:20");
    }

    #[test]
    fn overdue_item_keeps_its_original_due_time() {
        let database = Database::in_memory().unwrap();
        let item = database
            .create_item(
                &CreateItemInput {
                    title: "调整逾期事项".into(),
                    notes: String::new(),
                    category_id: None,
                    due_at: Some(at_local(2026, 8, 27, 15, 0).to_rfc3339()),
                    must_complete_today: false,
                    repeat_interval_minutes: None,
                    tag_ids: Vec::new(),
                    event: None,
                },
                at_local(2026, 8, 27, 14, 0),
            )
            .unwrap();

        let next = at_local(2026, 8, 28, 9, 30);
        let error = database
            .reschedule_item(&item.id, &next.to_rfc3339(), at_local(2026, 8, 27, 22, 0))
            .unwrap_err();
        assert!(error.to_string().contains("原到期时间不能修改"));
        assert_eq!(database.get_item(&item.id).unwrap().due_local_time, "15:00");
    }

    #[test]
    fn processing_same_time_does_not_duplicate_notification() {
        let database = Database::in_memory().unwrap();
        let due = at_local(2026, 8, 27, 18, 0);
        database
            .create_item(
                &CreateItemInput {
                    title: "确认发布".into(),
                    notes: String::new(),
                    category_id: None,
                    due_at: Some(due.to_rfc3339()),
                    must_complete_today: false,
                    repeat_interval_minutes: None,
                    tag_ids: Vec::new(),
                    event: None,
                },
                at_local(2026, 8, 27, 17, 0),
            )
            .unwrap();
        let first = database.process_due(at_local(2026, 8, 27, 18, 1)).unwrap();
        let second = database.process_due(at_local(2026, 8, 27, 18, 1)).unwrap();
        assert_eq!(first.notifications.len(), 1);
        assert!(second.notifications.is_empty());
    }

    #[test]
    fn opening_v1_database_creates_backup_and_runs_all_migrations() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("shanji.db");
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch(MIGRATION_0001).unwrap();
        drop(connection);

        let database = Database::open(&path).unwrap();
        let connection = database.connection.lock().expect("database mutex poisoned");
        let version = connection
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap();
        let delivery_columns = connection
            .prepare("PRAGMA table_info(reminder_events)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let settings_columns = connection
            .prepare("PRAGMA table_info(app_settings)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(version, 7);
        assert!(delivery_columns.contains(&"submitted_at".into()));
        assert!(delivery_columns.contains(&"error_code".into()));
        assert!(settings_columns.contains(&"autostart_enabled".into()));
        assert!(settings_columns.contains(&"onboarding_version".into()));
        assert!(settings_columns.contains(&"repeat_default_time_first".into()));
        assert!(settings_columns.contains(&"repeat_default_time_second".into()));
        let tag_table_exists = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'item_tags')",
                [],
                |row| row.get::<_, bool>(0),
            )
            .unwrap();
        let item_columns = connection
            .prepare("PRAGMA table_info(items)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(tag_table_exists);
        assert!(item_columns.contains(&"deleted_at".into()));
        assert!(item_columns.contains(&"reminder_acknowledged_at".into()));
        assert!(item_columns.contains(&"event_kind".into()));
        assert!(item_columns.contains(&"series_anchor_month".into()));
        assert!(item_columns.contains(&"series_anchor_day".into()));
        assert!(item_columns.contains(&"repeat_time_mode".into()));
        assert!(item_columns.contains(&"repeat_time_first".into()));
        assert!(item_columns.contains(&"repeat_time_second".into()));
        assert!(settings_columns.contains(&"smtp_enabled".into()));
        assert!(!settings_columns.contains(&"smtp_password".into()));
        let channel_table_exists = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'channel_deliveries')",
                [],
                |row| row.get::<_, bool>(0),
            )
            .unwrap();
        assert!(channel_table_exists);
        assert!(path.with_extension("db.backup-before-v3").exists());
    }

    #[test]
    fn v6_repeat_item_migrates_to_one_specified_daily_time() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("shanji-v6.db");
        let mut connection = Connection::open(&path).unwrap();
        let transaction = connection.transaction().unwrap();
        transaction.execute_batch(MIGRATION_0001).unwrap();
        transaction.execute_batch(MIGRATION_0002).unwrap();
        transaction.execute_batch(MIGRATION_0003).unwrap();
        transaction.execute_batch(MIGRATION_0004).unwrap();
        transaction.execute_batch(MIGRATION_0005).unwrap();
        transaction.execute_batch(MIGRATION_0006).unwrap();
        transaction
            .execute(
                "INSERT INTO items (
                id, title, status, due_at, due_local_date, due_local_time,
                due_source, rollover_policy, completion_policy, next_reminder_at,
                created_at, updated_at, event_kind, reminder_plan
             ) VALUES ('old-repeat', '旧重复事项', 'OPEN', ?1, '2026-09-01', '15:00',
                'EXPLICIT', 'NONE', 'NORMAL', ?1, ?1, ?1, 'ORDINARY', 'REPEAT')",
                [at_local(2026, 9, 1, 15, 0).to_rfc3339()],
            )
            .unwrap();
        transaction.commit().unwrap();
        drop(connection);

        let database = Database::open(&path).unwrap();
        let item = database.get_item("old-repeat").unwrap();
        assert_eq!(item.repeat_time_mode, "SPECIFIED");
        assert_eq!(item.repeat_times, vec!["15:00"]);
        assert_eq!(
            database.get_settings().unwrap().repeat_default_times,
            vec!["10:00", "17:00"]
        );
    }

    #[test]
    fn types_and_tags_can_be_reordered() {
        let database = Database::in_memory().unwrap();
        database.move_category("personal", "up").unwrap();
        assert_eq!(database.list_categories().unwrap()[0].id, "personal");

        let first = database
            .create_tag(&TaxonomyInput {
                name: "客户".into(),
                color: "#B06C49".into(),
            })
            .unwrap();
        let second = database
            .create_tag(&TaxonomyInput {
                name: "内部".into(),
                color: "#627D98".into(),
            })
            .unwrap();
        database.move_tag(&second.id, "up").unwrap();
        let tags = database.list_tags().unwrap();
        let first_index = tags.iter().position(|tag| tag.id == first.id).unwrap();
        let second_index = tags.iter().position(|tag| tag.id == second.id).unwrap();
        assert!(second_index < first_index);
    }

    #[test]
    fn existing_item_can_change_type_and_tags_without_changing_status() {
        let database = Database::in_memory().unwrap();
        let now = at_local(2026, 8, 28, 17, 0);
        let tag = database
            .create_tag(&TaxonomyInput {
                name: "客户".into(),
                color: "#B06C49".into(),
            })
            .unwrap();
        let item = database
            .create_item(
                &CreateItemInput {
                    title: "确认客户反馈".into(),
                    notes: String::new(),
                    category_id: Some("work".into()),
                    due_at: Some(at_local(2026, 8, 28, 18, 0).to_rfc3339()),
                    must_complete_today: false,
                    repeat_interval_minutes: None,
                    tag_ids: Vec::new(),
                    event: None,
                },
                now,
            )
            .unwrap();

        let updated = database
            .update_item(
                &item.id,
                &UpdateItemInput {
                    title: item.title.clone(),
                    notes: "已电话确认".into(),
                    category_id: Some("personal".into()),
                    tag_ids: vec![tag.id.clone()],
                    due_at: item.due_at.clone(),
                    must_complete_today: false,
                    repeat_interval_minutes: None,
                    event: None,
                },
                now,
            )
            .unwrap();
        assert_eq!(updated.status, "OPEN");
        assert_eq!(updated.category_id.as_deref(), Some("personal"));
        assert_eq!(updated.tags.len(), 1);
        assert_eq!(updated.tags[0].name, "客户");

        database.delete_tag(&tag.id).unwrap();
        assert!(database.get_item(&item.id).unwrap().tags.is_empty());
        database.delete_category("personal", None).unwrap();
        assert!(database.get_item(&item.id).unwrap().category_id.is_none());
    }

    #[test]
    fn recycle_bin_stops_reminders_and_requires_restore_before_permanent_delete() {
        let database = Database::in_memory().unwrap();
        let now = at_local(2026, 8, 28, 17, 0);
        let item = database
            .create_item(
                &CreateItemInput {
                    title: "回收站测试".into(),
                    notes: String::new(),
                    category_id: None,
                    due_at: Some(at_local(2026, 8, 28, 18, 0).to_rfc3339()),
                    must_complete_today: false,
                    repeat_interval_minutes: None,
                    tag_ids: Vec::new(),
                    event: None,
                },
                now,
            )
            .unwrap();

        assert!(database.permanently_delete_item(&item.id).is_err());
        let deleted = database.set_item_deleted(&item.id, true, now).unwrap();
        assert_eq!(deleted.status, "DELETED");
        assert!(deleted.next_reminder_at.is_none());
        assert!(deleted.deleted_at.is_some());
        let restored = database.set_item_deleted(&item.id, false, now).unwrap();
        assert_eq!(restored.status, "OPEN");
        assert!(restored.next_reminder_at.is_some());
        database.set_item_completed(&item.id, true, now).unwrap();
        database.set_item_deleted(&item.id, true, now).unwrap();
        let restored_done = database.set_item_deleted(&item.id, false, now).unwrap();
        assert_eq!(restored_done.status, "DONE");
        assert!(restored_done.next_reminder_at.is_none());
        database.set_item_deleted(&item.id, true, now).unwrap();
        database.permanently_delete_item(&item.id).unwrap();
        assert!(matches!(
            database.get_item(&item.id),
            Err(AppError::ItemNotFound)
        ));
    }

    #[test]
    fn recycle_bin_removes_items_after_thirty_days() {
        let database = Database::in_memory().unwrap();
        let created = at_local(2026, 7, 1, 10, 0);
        let item = database
            .create_item(
                &CreateItemInput {
                    title: "过期回收项".into(),
                    notes: String::new(),
                    category_id: None,
                    due_at: Some(at_local(2026, 7, 1, 18, 0).to_rfc3339()),
                    must_complete_today: false,
                    repeat_interval_minutes: None,
                    tag_ids: Vec::new(),
                    event: None,
                },
                created,
            )
            .unwrap();
        database.set_item_deleted(&item.id, true, created).unwrap();
        database
            .process_due(created + chrono::Duration::days(31))
            .unwrap();
        assert!(matches!(
            database.get_item(&item.id),
            Err(AppError::ItemNotFound)
        ));
    }

    #[test]
    fn manual_backup_contains_latest_committed_item() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("shanji.db");
        let database = Database::open(&path).unwrap();
        database
            .create_item(
                &CreateItemInput {
                    title: "备份中的事项".into(),
                    notes: String::new(),
                    category_id: None,
                    due_at: Some(at_local(2026, 8, 28, 18, 0).to_rfc3339()),
                    must_complete_today: false,
                    repeat_interval_minutes: None,
                    tag_ids: Vec::new(),
                    event: None,
                },
                at_local(2026, 8, 28, 17, 0),
            )
            .unwrap();

        let backup = database.create_backup().unwrap();
        assert_eq!(backup.item_count, 1);
        assert!(Path::new(&backup.path).is_file());
    }

    #[test]
    fn staged_restore_replaces_data_only_after_restart_boundary() {
        let directory = tempfile::tempdir().unwrap();
        let current_path = directory.path().join("shanji.db");
        let source_path = directory.path().join("old-shanji.db");
        let now = at_local(2026, 8, 28, 17, 0);

        let current = Database::open(&current_path).unwrap();
        current
            .create_item(
                &CreateItemInput {
                    title: "当前数据".into(),
                    notes: String::new(),
                    category_id: None,
                    due_at: Some(at_local(2026, 8, 28, 18, 0).to_rfc3339()),
                    must_complete_today: false,
                    repeat_interval_minutes: None,
                    tag_ids: Vec::new(),
                    event: None,
                },
                now,
            )
            .unwrap();
        let source = Database::open(&source_path).unwrap();
        source
            .create_item(
                &CreateItemInput {
                    title: "恢复后的数据".into(),
                    notes: String::new(),
                    category_id: None,
                    due_at: Some(at_local(2026, 8, 29, 18, 0).to_rfc3339()),
                    must_complete_today: false,
                    repeat_interval_minutes: None,
                    tag_ids: Vec::new(),
                    event: None,
                },
                now,
            )
            .unwrap();
        drop(source);

        current.stage_restore(&source_path).unwrap();
        assert_eq!(current.list_items("all", now).unwrap()[0].title, "当前数据");
        drop(current);

        apply_pending_restore(&current_path).unwrap();
        let restored = Database::open(&current_path).unwrap();
        let items = restored.list_items("all", now).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "恢复后的数据");
        assert!(
            directory
                .path()
                .read_dir()
                .unwrap()
                .filter_map(Result::ok)
                .any(|entry| entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("shanji-before-restore-"))
        );
    }

    #[test]
    fn onboarding_version_only_moves_forward() {
        let database = Database::in_memory().unwrap();
        assert_eq!(database.onboarding_version().unwrap(), 0);
        database.complete_onboarding(2).unwrap();
        database.complete_onboarding(1).unwrap();
        assert_eq!(database.onboarding_version().unwrap(), 2);
    }

    #[test]
    fn submitted_notification_has_auditable_terminal_state() {
        let database = Database::in_memory().unwrap();
        let due = at_local(2026, 8, 27, 18, 0);
        let item = database
            .create_item(
                &CreateItemInput {
                    title: "系统通知测试".into(),
                    notes: String::new(),
                    category_id: None,
                    due_at: Some(due.to_rfc3339()),
                    must_complete_today: false,
                    repeat_interval_minutes: None,
                    tag_ids: Vec::new(),
                    event: None,
                },
                at_local(2026, 8, 27, 17, 0),
            )
            .unwrap();

        let now = at_local(2026, 8, 27, 18, 1);
        let notification = database.process_due(now).unwrap().notifications.remove(0);
        assert_eq!(notification.item_id, item.id);
        assert_eq!(notification.due_local_time, "18:00");
        database
            .mark_notification_submitted(&notification.event_id, now)
            .unwrap();

        let delivery = database.last_notification_delivery().unwrap().unwrap();
        assert_eq!(delivery.result, "SUBMITTED");
        assert_eq!(delivery.error_code, None);
    }

    #[test]
    fn failed_notification_retries_with_bounded_backoff() {
        let database = Database::in_memory().unwrap();
        let due = at_local(2026, 8, 27, 18, 0);
        let item = database
            .create_item(
                &CreateItemInput {
                    title: "重试系统通知".into(),
                    notes: String::new(),
                    category_id: None,
                    due_at: Some(due.to_rfc3339()),
                    must_complete_today: false,
                    repeat_interval_minutes: None,
                    tag_ids: Vec::new(),
                    event: None,
                },
                at_local(2026, 8, 27, 17, 0),
            )
            .unwrap();

        let first_at = at_local(2026, 8, 27, 18, 1);
        let first = database
            .process_due(first_at)
            .unwrap()
            .notifications
            .remove(0);
        database
            .mark_notification_failed(&first.event_id, "platform_submit_failed", first_at)
            .unwrap();
        assert_eq!(
            database.get_item(&item.id).unwrap().next_reminder_at,
            Some((first_at + chrono::Duration::minutes(1)).to_rfc3339())
        );

        let second_at = first_at + chrono::Duration::minutes(1);
        let second = database
            .process_due(second_at)
            .unwrap()
            .notifications
            .remove(0);
        database
            .mark_notification_failed(&second.event_id, "platform_submit_failed", second_at)
            .unwrap();
        assert_eq!(
            database.get_item(&item.id).unwrap().next_reminder_at,
            Some((second_at + chrono::Duration::minutes(5)).to_rfc3339())
        );

        let delivery = database.last_notification_delivery().unwrap().unwrap();
        assert_eq!(delivery.result, "FAILED");
        assert_eq!(
            delivery.error_code.as_deref(),
            Some("platform_submit_failed")
        );
    }

    #[test]
    fn stale_claim_is_recovered_once_after_interrupted_delivery() {
        let database = Database::in_memory().unwrap();
        let due = at_local(2026, 8, 27, 18, 0);
        database
            .create_item(
                &CreateItemInput {
                    title: "崩溃恢复提醒".into(),
                    notes: String::new(),
                    category_id: None,
                    due_at: Some(due.to_rfc3339()),
                    must_complete_today: false,
                    repeat_interval_minutes: None,
                    tag_ids: Vec::new(),
                    event: None,
                },
                at_local(2026, 8, 27, 17, 0),
            )
            .unwrap();

        let claimed_at = at_local(2026, 8, 27, 18, 1);
        let first = database
            .process_due(claimed_at)
            .unwrap()
            .notifications
            .remove(0);
        let recovered_at = claimed_at + chrono::Duration::minutes(2);
        let recovered = database
            .process_due(recovered_at)
            .unwrap()
            .notifications
            .remove(0);
        assert_eq!(recovered.event_id, first.event_id);
        assert!(
            database
                .process_due(recovered_at)
                .unwrap()
                .notifications
                .is_empty()
        );
    }

    #[test]
    fn reminder_center_requires_an_explicit_action_and_ack_stops_normal_repeats() {
        let database = Database::in_memory().unwrap();
        let due = at_local(2026, 8, 27, 18, 0);
        let item = database
            .create_item(
                &CreateItemInput {
                    title: "等待明确确认".into(),
                    notes: String::new(),
                    category_id: None,
                    due_at: Some(due.to_rfc3339()),
                    must_complete_today: false,
                    repeat_interval_minutes: None,
                    tag_ids: Vec::new(),
                    event: None,
                },
                due - chrono::Duration::hours(1),
            )
            .unwrap();
        {
            let connection = database.connection.lock().expect("database mutex poisoned");
            connection
                .execute(
                    "UPDATE app_settings SET repeat_unacknowledged_enabled = 1,
                        unacknowledged_repeat_minutes = 15, quiet_hours_enabled = 0 WHERE id = 1",
                    [],
                )
                .unwrap();
        }

        let first_at = due + chrono::Duration::minutes(1);
        let notification = database
            .process_due(first_at)
            .unwrap()
            .notifications
            .remove(0);
        database
            .mark_notification_submitted(&notification.event_id, first_at)
            .unwrap();
        assert_eq!(database.list_pending_reminders(first_at).unwrap().len(), 1);
        assert!(
            database
                .get_item(&item.id)
                .unwrap()
                .next_reminder_at
                .is_some()
        );

        database.acknowledge_reminder(&item.id, first_at).unwrap();
        assert!(
            database
                .list_pending_reminders(first_at)
                .unwrap()
                .is_empty()
        );
        assert!(
            database
                .get_item(&item.id)
                .unwrap()
                .next_reminder_at
                .is_none()
        );
        assert!(
            database
                .process_due(first_at + chrono::Duration::minutes(20))
                .unwrap()
                .notifications
                .is_empty()
        );
    }

    #[test]
    fn reminder_center_snooze_completion_and_pause_remove_the_current_entry() {
        let database = Database::in_memory().unwrap();
        let due = at_local(2026, 8, 27, 18, 0);
        let item = database
            .create_item(
                &CreateItemInput {
                    title: "提醒中心操作".into(),
                    notes: String::new(),
                    category_id: None,
                    due_at: Some(due.to_rfc3339()),
                    must_complete_today: true,
                    repeat_interval_minutes: Some(30),
                    tag_ids: Vec::new(),
                    event: None,
                },
                due - chrono::Duration::hours(1),
            )
            .unwrap();
        let now = due + chrono::Duration::minutes(1);
        database.process_due(now).unwrap();
        assert_eq!(database.list_pending_reminders(now).unwrap().len(), 1);

        let snoozed = database.snooze_item(&item.id, 15, now).unwrap();
        assert_eq!(
            snoozed.next_reminder_at,
            Some((now + chrono::Duration::minutes(15)).to_rfc3339())
        );
        assert!(database.list_pending_reminders(now).unwrap().is_empty());

        database
            .process_due(now + chrono::Duration::minutes(15))
            .unwrap();
        assert_eq!(
            database
                .list_pending_reminders(now + chrono::Duration::minutes(15))
                .unwrap()
                .len(),
            1
        );
        database.set_reminder_paused(&item.id, true, now).unwrap();
        assert!(database.list_pending_reminders(now).unwrap().is_empty());
        database.set_reminder_paused(&item.id, false, now).unwrap();
        database.set_item_completed(&item.id, true, now).unwrap();
        assert!(database.list_pending_reminders(now).unwrap().is_empty());
    }

    #[test]
    fn email_routes_are_tag_scoped_idempotent_and_use_bounded_retries() {
        let database = Database::in_memory().unwrap();
        let tag = database
            .create_tag(&TaxonomyInput {
                name: "外部通知".into(),
                color: "#B06C49".into(),
            })
            .unwrap();
        let due = at_local(2026, 8, 27, 18, 0);
        database
            .create_item(
                &CreateItemInput {
                    title: "邮件路由测试".into(),
                    notes: String::new(),
                    category_id: None,
                    due_at: Some(due.to_rfc3339()),
                    must_complete_today: false,
                    repeat_interval_minutes: None,
                    tag_ids: vec![tag.id.clone()],
                    event: None,
                },
                due - chrono::Duration::hours(1),
            )
            .unwrap();
        let now = due + chrono::Duration::minutes(1);
        let notification = database.process_due(now).unwrap().notifications.remove(0);

        assert!(
            database
                .claim_email_delivery(&notification, false, now)
                .unwrap()
                .is_none()
        );
        database.set_tag_email_route(&tag.id, true).unwrap();
        let job = database
            .claim_email_delivery(&notification, false, now)
            .unwrap()
            .unwrap();
        assert!(
            database
                .claim_email_delivery(&notification, false, now)
                .unwrap()
                .is_none()
        );

        database
            .mark_email_delivery_failed(&job.delivery_id, "smtp_delivery_failed", now)
            .unwrap();
        assert!(
            database
                .claim_due_email_retries(now + chrono::Duration::seconds(59))
                .unwrap()
                .is_empty()
        );
        let retry_one = database
            .claim_due_email_retries(now + chrono::Duration::minutes(1))
            .unwrap()
            .remove(0);
        database
            .mark_email_delivery_failed(
                &retry_one.delivery_id,
                "smtp_delivery_failed",
                now + chrono::Duration::minutes(1),
            )
            .unwrap();
        let retry_two = database
            .claim_due_email_retries(now + chrono::Duration::minutes(6))
            .unwrap()
            .remove(0);
        database
            .mark_email_delivery_failed(
                &retry_two.delivery_id,
                "smtp_delivery_failed",
                now + chrono::Duration::minutes(6),
            )
            .unwrap();
        assert!(
            database
                .claim_due_email_retries(now + chrono::Duration::hours(1))
                .unwrap()
                .is_empty()
        );
        let delivery = database.last_email_delivery().unwrap().unwrap();
        assert_eq!(delivery.result, "FAILED");
        assert_eq!(delivery.error_code.as_deref(), Some("smtp_delivery_failed"));
    }

    #[test]
    fn smtp_password_is_never_a_database_column() {
        let database = Database::in_memory().unwrap();
        let columns = database
            .connection
            .lock()
            .expect("database mutex poisoned")
            .prepare("PRAGMA table_info(app_settings)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(!columns.iter().any(|column| column.contains("password")));
    }

    #[test]
    fn overlay_delivery_is_recorded_once_per_reminder_event() {
        let database = Database::in_memory().unwrap();
        let due = at_local(2026, 8, 27, 18, 0);
        database
            .create_item(
                &CreateItemInput {
                    title: "置顶提醒测试".into(),
                    notes: String::new(),
                    category_id: None,
                    due_at: Some(due.to_rfc3339()),
                    must_complete_today: false,
                    repeat_interval_minutes: None,
                    tag_ids: Vec::new(),
                    event: None,
                },
                due - chrono::Duration::hours(1),
            )
            .unwrap();
        let now = due + chrono::Duration::minutes(1);
        let notification = database.process_due(now).unwrap().notifications.remove(0);
        database
            .record_overlay_delivery(&notification, now)
            .unwrap();
        database
            .record_overlay_delivery(&notification, now)
            .unwrap();
        let count = database
            .connection
            .lock()
            .expect("database mutex poisoned")
            .query_row(
                "SELECT COUNT(*) FROM channel_deliveries WHERE channel = 'overlay'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }
}
