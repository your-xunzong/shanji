use std::{fs, path::Path, sync::Mutex};

use chrono::{DateTime, Local, NaiveDate, Utc};
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};
use uuid::Uuid;

use crate::{
    domain::{
        Category, CreateItemInput, Item, Settings, UpdateSettingsInput, due_for_local_date,
        due_from_explicit, must_complete_due, next_default_due, next_repeat_at, parse_time,
    },
    error::{AppError, AppResult},
};

const MIGRATION_0001: &str = include_str!("../migrations/0001_init.sql");

pub struct Database {
    connection: Mutex<Connection>,
}

#[derive(Debug, Clone)]
pub struct DueNotification {
    pub title: String,
}

#[derive(Debug, Default)]
pub struct ProcessResult {
    pub notifications: Vec<DueNotification>,
    pub changed: bool,
    pub notifications_enabled: bool,
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

        let version: Option<i64> = connection
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .optional()
            .unwrap_or(None)
            .flatten();

        if existed && version.unwrap_or(0) < 1 {
            let backup = path.with_extension("db.backup-before-migration");
            fs::copy(path, backup)?;
        }

        let transaction = connection.transaction()?;
        transaction.execute_batch(MIGRATION_0001)?;
        transaction.commit()?;

        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    #[cfg(test)]
    pub fn in_memory() -> AppResult<Self> {
        let mut connection = Connection::open_in_memory()?;
        connection.execute_batch("PRAGMA foreign_keys = ON;")?;
        let transaction = connection.transaction()?;
        transaction.execute_batch(MIGRATION_0001)?;
        transaction.commit()?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
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
                notifications_enabled = ?8
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
        mapped
            .collect::<Result<Vec<_>, _>>()
            .map_err(AppError::from)
    }

    pub fn get_item(&self, id: &str) -> AppResult<Item> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        get_item_from(&connection, id)
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

        result.changed |= repair_future_must_complete_items(&transaction, now)? > 0;
        result.changed |= rollover_default_items(&transaction, &settings, now)? > 0;

        let now_text = now.to_rfc3339();
        let due_rows = {
            let mut statement = transaction.prepare(
                "SELECT id, title, completion_policy, repeat_interval_minutes,
                        next_reminder_at, bypass_app_quiet_hours, revision
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
                ))
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        for (id, title, policy, interval, scheduled_for, bypass_quiet, revision) in due_rows {
            let idempotency_key = format!("{id}:{scheduled_for}:{revision}");
            let inserted = transaction.execute(
                "INSERT OR IGNORE INTO reminder_events
                   (id, item_id, scheduled_for, sent_at, idempotency_key, result)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'QUEUED')",
                params![
                    Uuid::new_v4().to_string(),
                    id,
                    scheduled_for,
                    now_text,
                    idempotency_key
                ],
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
                result.notifications.push(DueNotification { title });
            }
            result.changed = true;
        }

        transaction.commit()?;
        Ok(result)
    }
}

fn read_settings(connection: &Connection) -> AppResult<Settings> {
    connection
        .query_row(
            "SELECT default_due_time, workdays, overtime_interval_minutes,
                    quiet_hours_enabled, quiet_start, quiet_end, global_shortcut, notifications_enabled
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
            i.bypass_app_quiet_hours, i.created_at, i.updated_at, i.completed_at
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
    })
}

fn get_item_from(connection: &Connection, id: &str) -> AppResult<Item> {
    let sql = format!("{} WHERE i.id = ?1", item_select());
    connection
        .query_row(&sql, [id], row_to_item)
        .optional()?
        .ok_or(AppError::ItemNotFound)
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
                },
                at_local(2026, 8, 27, 17, 0),
            )
            .unwrap();
        let first = database.process_due(at_local(2026, 8, 27, 18, 1)).unwrap();
        let second = database.process_due(at_local(2026, 8, 27, 18, 1)).unwrap();
        assert_eq!(first.notifications.len(), 1);
        assert!(second.notifications.is_empty());
    }
}
