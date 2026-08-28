use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::SystemTime,
};

use chrono::{DateTime, Local, NaiveDate, Utc};
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    domain::{
        Category, CreateItemInput, Item, Settings, Tag, TaxonomyInput, UpdateItemInput,
        UpdateSettingsInput, due_for_local_date, due_from_explicit, must_complete_due,
        next_default_due, next_repeat_at, parse_time,
    },
    error::{AppError, AppResult},
};

const MIGRATION_0001: &str = include_str!("../migrations/0001_init.sql");
const MIGRATION_0002: &str = include_str!("../migrations/0002_notification_delivery.sql");
const MIGRATION_0003: &str = include_str!("../migrations/0003_autostart_onboarding.sql");
const MIGRATION_0004: &str = include_str!("../migrations/0004_types_tags_recycle.sql");
const LATEST_SCHEMA_VERSION: i64 = 4;

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

#[derive(Debug, Clone)]
pub struct DueNotification {
    pub event_id: String,
    pub item_id: String,
    pub title: String,
    pub due_local_date: String,
    pub due_local_time: String,
}

#[derive(Debug, Clone)]
pub struct NotificationDelivery {
    pub result: String,
    pub error_code: Option<String>,
    pub attempted_at: String,
}

#[derive(Debug, Default)]
pub struct ProcessResult {
    pub notifications: Vec<DueNotification>,
    pub changed: bool,
    pub notifications_enabled: bool,
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
                autostart_enabled = ?9
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
            ],
        )?;

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
        let due = match &input.due_at {
            Some(value) => due_from_explicit(value)?,
            None if input.must_complete_today => must_complete_due(now, &settings)?,
            None => next_default_due(now, &settings)?,
        };

        let id = Uuid::new_v4().to_string();
        let timestamp = now.to_rfc3339();
        let due_at = due.utc.to_rfc3339();
        let due_source = if input.due_at.is_some() {
            "EXPLICIT"
        } else {
            "DEFAULT_EOD"
        };
        let completion_policy = if input.must_complete_today {
            "MUST_COMPLETE_TODAY"
        } else {
            "NORMAL"
        };
        let rollover_policy = if input.must_complete_today || input.due_at.is_some() {
            "NONE"
        } else {
            "NEXT_WORKDAY_EOD"
        };
        let repeat_interval = input.must_complete_today.then_some(
            input
                .repeat_interval_minutes
                .unwrap_or(settings.overtime_interval_minutes),
        );

        transaction.execute(
            "INSERT INTO items (
                id, title, notes, status, category_id, due_at, due_local_date, due_local_time,
                due_source, rollover_policy, completion_policy, repeat_interval_minutes,
                next_reminder_at, created_at, updated_at
             ) VALUES (?1, ?2, ?3, 'OPEN', ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?5, ?12, ?12)",
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
                timestamp,
            ],
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

        let changed = transaction.execute(
            "UPDATE items SET status = ?1, completed_at = ?2, next_reminder_at = ?3,
                reminder_paused = 0, updated_at = ?4, revision = revision + 1 WHERE id = ?5",
            params![
                if completed { "DONE" } else { "OPEN" },
                completed.then_some(now_text.clone()),
                next_reminder,
                now_text,
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

    pub fn set_reminder_paused(
        &self,
        id: &str,
        paused: bool,
        now: DateTime<Utc>,
    ) -> AppResult<Item> {
        let mut connection = self.connection.lock().expect("database mutex poisoned");
        let transaction = connection.transaction()?;
        let existing = get_item_from(&transaction, id)?;
        if existing.completion_policy != "MUST_COMPLETE_TODAY" {
            return Err(AppError::Validation(
                "只有今日必做事项可以暂停持续提醒".into(),
            ));
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

        let due_at = due.utc.to_rfc3339();
        let timestamp = now.to_rfc3339();
        transaction.execute(
            "UPDATE items SET due_at = ?1, due_local_date = ?2, due_local_time = ?3,
                due_source = 'EXPLICIT', rollover_policy = 'NONE', next_reminder_at = ?1,
                reminder_paused = 0, updated_at = ?4, revision = revision + 1 WHERE id = ?5",
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
        let due = due_from_explicit(&input.due_at)?;
        let mut connection = self.connection.lock().expect("database mutex poisoned");
        let transaction = connection.transaction()?;
        let existing = get_item_from(&transaction, id)?;
        if existing.status == "DELETED" {
            return Err(AppError::Validation("请先从回收站恢复事项再编辑".into()));
        }
        validate_taxonomy_links(&transaction, input.category_id.as_deref(), &input.tag_ids)?;
        let settings = read_settings(&transaction)?;
        let timestamp = now.to_rfc3339();
        let due_at = due.utc.to_rfc3339();
        let completion_policy = if input.must_complete_today {
            "MUST_COMPLETE_TODAY"
        } else {
            "NORMAL"
        };
        let repeat_interval = input.must_complete_today.then_some(
            input
                .repeat_interval_minutes
                .unwrap_or(settings.overtime_interval_minutes),
        );
        let next_reminder = if existing.status == "OPEN" && !existing.reminder_paused {
            Some(if due.utc <= now {
                timestamp.clone()
            } else {
                due_at.clone()
            })
        } else {
            None
        };
        transaction.execute(
            "UPDATE items SET title = ?1, notes = ?2, category_id = ?3, due_at = ?4,
                due_local_date = ?5, due_local_time = ?6, due_source = 'EXPLICIT',
                rollover_policy = 'NONE', completion_policy = ?7, repeat_interval_minutes = ?8,
                next_reminder_at = ?9, reminder_paused = 0, updated_at = ?10,
                revision = revision + 1 WHERE id = ?11",
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
                id,
            ],
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
            Some(existing.due_at)
        };
        transaction.execute(
            "UPDATE items SET status = ?1, deleted_at = ?2,
                next_reminder_at = ?3, reminder_paused = 0, updated_at = ?4,
                deleted_from_status = ?5, revision = revision + 1 WHERE id = ?6",
            params![
                status,
                deleted.then_some(timestamp.clone()),
                next_reminder,
                timestamp,
                if deleted { Some(existing.status) } else { None },
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
                "SELECT e.id, e.item_id, i.title, i.due_local_date, i.due_local_time
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
                })
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        for notification in stale_claims {
            transaction.execute(
                "UPDATE reminder_events SET sent_at = ?1 WHERE id = ?2 AND result = 'CLAIMED'",
                params![now_text, notification.event_id],
            )?;
            result.notifications.push(notification);
            result.changed = true;
        }

        let due_rows = {
            let mut statement = transaction.prepare(
                "SELECT id, title, completion_policy, repeat_interval_minutes,
                        next_reminder_at, bypass_app_quiet_hours, revision,
                        due_local_date, due_local_time
                 FROM items
                 WHERE status = 'OPEN' AND reminder_paused = 0
                   AND next_reminder_at IS NOT NULL AND next_reminder_at <= ?1
                 ORDER BY next_reminder_at ASC",
            )?;
            let rows = statement.query_map([&now_text], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<u32>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)? != 0,
                    row.get::<_, u32>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                ))
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        for (
            id,
            title,
            policy,
            interval,
            scheduled_for,
            bypass_quiet,
            revision,
            due_local_date,
            due_local_time,
        ) in due_rows
        {
            let idempotency_key = format!("{id}:{scheduled_for}:{revision}");
            let event_id = Uuid::new_v4().to_string();
            let inserted = transaction.execute(
                "INSERT OR IGNORE INTO reminder_events
                   (id, item_id, scheduled_for, sent_at, idempotency_key, result)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'CLAIMED')",
                params![event_id, id, scheduled_for, now_text, idempotency_key],
            )?;

            let next_reminder = if policy == "MUST_COMPLETE_TODAY" {
                Some(
                    next_repeat_at(
                        now,
                        interval.unwrap_or(settings.overtime_interval_minutes),
                        &settings,
                        bypass_quiet,
                    )?
                    .to_rfc3339(),
                )
            } else {
                None
            };
            transaction.execute(
                "UPDATE items SET next_reminder_at = ?1, updated_at = ?2 WHERE id = ?3",
                params![next_reminder, now_text, id],
            )?;

            if inserted > 0 {
                result.notifications.push(DueNotification {
                    event_id,
                    item_id: id,
                    title,
                    due_local_date,
                    due_local_time,
                });
            }
            result.changed = true;
        }

        transaction.commit()?;
        Ok(result)
    }

    pub fn mark_notification_submitted(&self, event_id: &str, now: DateTime<Utc>) -> AppResult<()> {
        let mut connection = self.connection.lock().expect("database mutex poisoned");
        let transaction = connection.transaction()?;
        let item_id = claimed_event_item_id(&transaction, event_id)?;
        let Some(item_id) = item_id else {
            transaction.commit()?;
            return Ok(());
        };
        transaction.execute(
            "UPDATE reminder_events
             SET result = 'SUBMITTED', submitted_at = ?1, error_code = NULL,
                 attempt_count = attempt_count + 1
             WHERE id = ?2 AND result = 'CLAIMED'",
            params![now.to_rfc3339(), event_id],
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
    connection
        .query_row(
            "SELECT default_due_time, workdays, overtime_interval_minutes,
                    quiet_hours_enabled, quiet_start, quiet_end, global_shortcut,
                    notifications_enabled, autostart_enabled
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
                    workdays,
                    overtime_interval_minutes: row.get(2)?,
                    quiet_hours_enabled: row.get::<_, i64>(3)? != 0,
                    quiet_start: row.get(4)?,
                    quiet_end: row.get(5)?,
                    global_shortcut: row.get(6)?,
                    notifications_enabled: row.get::<_, i64>(7)? != 0,
                    autostart_enabled: row.get::<_, i64>(8)? != 0,
                })
            },
        )
        .map_err(AppError::from)
}

fn item_select() -> &'static str {
    "SELECT i.id, i.title, i.notes, i.status, i.category_id, c.name,
            i.due_at, i.due_local_date, i.due_local_time, i.due_source,
            i.rollover_policy, i.rollover_count, i.completion_policy,
            i.repeat_interval_minutes, i.next_reminder_at, i.reminder_paused,
            i.bypass_app_quiet_hours, i.created_at, i.updated_at, i.completed_at,
            i.deleted_at
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
                },
                now,
            )
            .unwrap();
        let current = database.get_settings().unwrap();
        database
            .update_settings(
                &UpdateSettingsInput {
                    default_due_time: "17:30".into(),
                    workdays: current.workdays,
                    overtime_interval_minutes: current.overtime_interval_minutes,
                    quiet_hours_enabled: current.quiet_hours_enabled,
                    quiet_start: current.quiet_start,
                    quiet_end: current.quiet_end,
                    global_shortcut: current.global_shortcut,
                    notifications_enabled: current.notifications_enabled,
                    autostart_enabled: current.autostart_enabled,
                    update_existing_default_items: false,
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
    fn overdue_item_can_be_rescheduled() {
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
                },
                at_local(2026, 8, 27, 14, 0),
            )
            .unwrap();

        let next = at_local(2026, 8, 28, 9, 30);
        let updated = database
            .reschedule_item(&item.id, &next.to_rfc3339(), at_local(2026, 8, 27, 22, 0))
            .unwrap();
        assert_eq!(updated.due_local_date, "2026-08-28");
        assert_eq!(updated.due_local_time, "09:30");
        assert_eq!(updated.due_source, "EXPLICIT");
        assert_eq!(updated.next_reminder_at, Some(next.to_rfc3339()));
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

        assert_eq!(version, 4);
        assert!(delivery_columns.contains(&"submitted_at".into()));
        assert!(delivery_columns.contains(&"error_code".into()));
        assert!(settings_columns.contains(&"autostart_enabled".into()));
        assert!(settings_columns.contains(&"onboarding_version".into()));
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
        assert!(path.with_extension("db.backup-before-v3").exists());
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
        assert_eq!(tags[0].id, second.id);
        assert_eq!(tags[1].id, first.id);
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
}
