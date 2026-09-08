use std::collections::BTreeMap;

use chrono::{DateTime, Local, Utc};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    db::{Database, DueNotification, EmailDeliveryJob},
    domain::{EVENT_KINDS, parse_time},
    error::{AppError, AppResult},
};

const DIMENSIONS: [&str; 3] = ["EVENT_KIND", "CATEGORY", "TAG"];
const STRATEGIES: [&str; 4] = ["FIRST_DUE", "DAILY_FIRST", "EACH_PLAN", "DAILY_DIGEST"];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailDeliveryRule {
    pub id: String,
    pub match_dimension: String,
    pub match_value: String,
    pub strategy: String,
    pub recipient: String,
    pub digest_local_time: String,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailDeliveryRuleInput {
    pub id: Option<String>,
    pub match_dimension: String,
    pub match_value: String,
    pub strategy: String,
    pub recipient: String,
    pub digest_local_time: String,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct EmailDigestJob {
    pub delivery_id: String,
    pub recipient: String,
    pub local_date: String,
    pub notifications: Vec<DueNotification>,
}

pub fn list_rules(database: &Database) -> AppResult<Vec<EmailDeliveryRule>> {
    let connection = database.connection.lock().expect("database mutex poisoned");
    let mut statement = connection.prepare(
        "SELECT id, match_dimension, match_value, strategy, recipient,
                digest_local_time, enabled, created_at, updated_at
         FROM email_delivery_rules
         ORDER BY enabled DESC,
           CASE match_dimension WHEN 'EVENT_KIND' THEN 1 WHEN 'CATEGORY' THEN 2 ELSE 3 END,
           updated_at DESC",
    )?;
    statement
        .query_map([], rule_from_row)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::from)
}

pub fn save_rule(
    database: &Database,
    input: &EmailDeliveryRuleInput,
    now: DateTime<Utc>,
) -> AppResult<EmailDeliveryRule> {
    validate_rule(input)?;
    let connection = database.connection.lock().expect("database mutex poisoned");
    validate_match_value(&connection, input)?;
    if input.enabled {
        let verified = connection.query_row(
            "SELECT smtp_enabled = 1 AND smtp_verified_at IS NOT NULL
             FROM app_settings WHERE id = 1",
            [],
            |row| row.get::<_, bool>(0),
        )?;
        if !verified {
            return Err(AppError::Validation(
                "请先发送测试邮件并确认服务器接受，再启用邮件规则".into(),
            ));
        }
    }
    let id = input
        .id
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let existing_created_at = connection
        .query_row(
            "SELECT created_at FROM email_delivery_rules WHERE id = ?1",
            [&id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if input.id.is_some() && existing_created_at.is_none() {
        return Err(AppError::Validation("找不到要修改的邮件规则".into()));
    }
    let timestamp = now.to_rfc3339();
    connection.execute(
        "INSERT INTO email_delivery_rules
           (id, match_dimension, match_value, strategy, recipient,
            digest_local_time, enabled, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
         ON CONFLICT(id) DO UPDATE SET
           match_dimension = excluded.match_dimension,
           match_value = excluded.match_value,
           strategy = excluded.strategy,
           recipient = excluded.recipient,
           digest_local_time = excluded.digest_local_time,
           enabled = excluded.enabled,
           updated_at = excluded.updated_at",
        params![
            id,
            input.match_dimension,
            input.match_value,
            input.strategy,
            input.recipient.trim(),
            input.digest_local_time,
            i64::from(input.enabled),
            existing_created_at.unwrap_or(timestamp)
        ],
    )?;
    get_rule(&connection, &id)
}

pub fn delete_rule(database: &Database, id: &str, now: DateTime<Utc>) -> AppResult<()> {
    let mut connection = database.connection.lock().expect("database mutex poisoned");
    let transaction = connection.transaction()?;
    let exists = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM email_delivery_rules WHERE id = ?1)",
        [id],
        |row| row.get::<_, bool>(0),
    )?;
    if !exists {
        return Err(AppError::Validation("找不到要删除的邮件规则".into()));
    }
    transaction.execute(
        "UPDATE channel_deliveries SET result = 'DISABLED', submitted_at = ?1,
           error_code = 'rule_removed', next_attempt_at = NULL
         WHERE rule_id = ?2 AND result IN ('CLAIMED', 'FAILED')",
        params![now.to_rfc3339(), id],
    )?;
    transaction.execute("DELETE FROM email_delivery_rules WHERE id = ?1", [id])?;
    transaction.execute(
        "UPDATE email_digest_deliveries
         SET result = 'DISABLED', submitted_at = ?1,
             error_code = 'rule_removed', next_attempt_at = NULL
         WHERE result IN ('CLAIMED', 'FAILED')
           AND NOT EXISTS (
             SELECT 1 FROM email_digest_queue q
             WHERE q.recipient = email_digest_deliveries.recipient
               AND q.local_date = email_digest_deliveries.local_date
           )",
        [now.to_rfc3339()],
    )?;
    transaction.commit()?;
    Ok(())
}

pub fn mark_smtp_verified(database: &Database, now: DateTime<Utc>) -> AppResult<()> {
    let connection = database.connection.lock().expect("database mutex poisoned");
    connection.execute(
        "UPDATE app_settings SET smtp_verified_at = ?1 WHERE id = 1",
        [now.to_rfc3339()],
    )?;
    Ok(())
}

pub fn invalidate_smtp_verification(database: &Database) -> AppResult<()> {
    let connection = database.connection.lock().expect("database mutex poisoned");
    connection.execute(
        "UPDATE app_settings SET smtp_verified_at = NULL WHERE id = 1",
        [],
    )?;
    Ok(())
}

pub fn smtp_verified_at(database: &Database) -> AppResult<Option<String>> {
    let connection = database.connection.lock().expect("database mutex poisoned");
    connection
        .query_row(
            "SELECT smtp_verified_at FROM app_settings WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .map_err(AppError::from)
}

pub fn claim_deliveries(
    database: &Database,
    notification: &DueNotification,
    now: DateTime<Utc>,
) -> AppResult<Vec<EmailDeliveryJob>> {
    let mut connection = database.connection.lock().expect("database mutex poisoned");
    let transaction = connection.transaction()?;
    let category_id = transaction.query_row(
        "SELECT category_id FROM items WHERE id = ?1",
        [&notification.item_id],
        |row| row.get::<_, Option<String>>(0),
    )?;
    let rules = matching_rules(
        &transaction,
        notification.event_kind.as_deref(),
        category_id.as_deref(),
        &notification.tag_ids,
    )?;
    let mut by_recipient = BTreeMap::<String, EmailDeliveryRule>::new();
    for rule in rules {
        by_recipient.entry(rule.recipient.clone()).or_insert(rule);
    }
    let local_date = now.with_timezone(&Local).date_naive().to_string();
    let mut jobs = Vec::new();
    for (_, rule) in by_recipient {
        if rule.strategy == "DAILY_DIGEST" {
            let already_sent = transaction.query_row(
                "SELECT EXISTS(
                   SELECT 1 FROM email_digest_deliveries
                   WHERE recipient = ?1 AND local_date = ?2 AND result = 'SUBMITTED'
                 )",
                params![rule.recipient, local_date],
                |record| record.get::<_, bool>(0),
            )?;
            let digest_date = if already_sent {
                (now.with_timezone(&Local).date_naive() + chrono::Duration::days(1)).to_string()
            } else {
                local_date.clone()
            };
            transaction.execute(
                "INSERT OR IGNORE INTO email_digest_queue
                   (id, rule_id, item_id, reminder_event_id, recipient,
                    local_date, due_at, queued_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    Uuid::new_v4().to_string(),
                    rule.id,
                    notification.item_id,
                    notification.event_id,
                    rule.recipient,
                    digest_date,
                    format!(
                        "{}T{}",
                        notification.due_local_date, notification.due_local_time
                    ),
                    now.to_rfc3339()
                ],
            )?;
            continue;
        }
        let recipient_key = recipient_key(&rule.recipient);
        let idempotency_key = match rule.strategy.as_str() {
            "FIRST_DUE" => format!("{}:email:{recipient_key}:first", notification.item_id),
            "DAILY_FIRST" => format!(
                "{}:email:{recipient_key}:day:{local_date}",
                notification.item_id
            ),
            "EACH_PLAN" => format!("{}:email:{recipient_key}", notification.event_id),
            _ => continue,
        };
        let delivery_id = Uuid::new_v4().to_string();
        let inserted = transaction.execute(
            "INSERT OR IGNORE INTO channel_deliveries
               (id, reminder_event_id, item_id, channel, idempotency_key,
                attempted_at, result, attempt_count, next_attempt_at,
                recipient, rule_id, strategy, local_date)
             VALUES (?1, ?2, ?3, 'email', ?4, ?5, 'CLAIMED', 0, ?5,
                ?6, ?7, ?8, ?9)",
            params![
                delivery_id,
                notification.event_id,
                notification.item_id,
                idempotency_key,
                now.to_rfc3339(),
                rule.recipient,
                rule.id,
                rule.strategy,
                local_date
            ],
        )?;
        if inserted > 0 {
            jobs.push(EmailDeliveryJob {
                delivery_id,
                recipient: rule.recipient,
                notification: notification.clone(),
            });
        }
    }
    transaction.commit()?;
    Ok(jobs)
}

pub fn claim_due_digests(
    database: &Database,
    now: DateTime<Utc>,
) -> AppResult<Vec<EmailDigestJob>> {
    let local = now.with_timezone(&Local);
    let local_date = local.date_naive().to_string();
    let local_time = local.time().format("%H:%M").to_string();
    let stale_before = (now - chrono::Duration::minutes(2)).to_rfc3339();
    let mut connection = database.connection.lock().expect("database mutex poisoned");
    let transaction = connection.transaction()?;
    transaction.execute(
        "UPDATE email_digest_deliveries
         SET result = 'FAILED', error_code = 'stale_claim', next_attempt_at = ?1,
             attempt_count = attempt_count + 1
         WHERE result = 'CLAIMED' AND attempted_at <= ?2 AND attempt_count < 3",
        params![now.to_rfc3339(), stale_before],
    )?;
    let retry_groups = {
        let mut statement = transaction.prepare(
            "SELECT id, recipient, local_date FROM email_digest_deliveries
             WHERE result = 'FAILED' AND attempt_count < 3 AND next_attempt_at <= ?1
               AND EXISTS (
                 SELECT 1 FROM email_digest_queue q
                 WHERE q.recipient = email_digest_deliveries.recipient
                   AND q.local_date = email_digest_deliveries.local_date
               )
             ORDER BY next_attempt_at LIMIT 3",
        )?;
        statement
            .query_map([now.to_rfc3339()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?
    };
    let mut jobs = Vec::new();
    for (delivery_id, recipient, date) in retry_groups {
        transaction.execute(
            "UPDATE email_digest_deliveries SET result = 'CLAIMED', attempted_at = ?1
             WHERE id = ?2 AND result = 'FAILED'",
            params![now.to_rfc3339(), delivery_id],
        )?;
        jobs.push(EmailDigestJob {
            notifications: digest_notifications(&transaction, &recipient, &date)?,
            delivery_id,
            recipient,
            local_date: date,
        });
    }
    let groups = {
        let mut statement = transaction.prepare(
            "SELECT q.recipient, q.local_date
             FROM email_digest_queue q
             JOIN email_delivery_rules r ON r.id = q.rule_id AND r.enabled = 1
             WHERE q.local_date <= ?1
             GROUP BY q.recipient, q.local_date
             HAVING q.local_date < ?1 OR MAX(r.digest_local_time) <= ?2
             ORDER BY q.local_date, q.recipient LIMIT 3",
        )?;
        statement
            .query_map(params![local_date, local_time], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?
    };
    for (recipient, date) in groups {
        let key = format!("digest:{}:{date}", recipient_key(&recipient));
        let delivery_id = Uuid::new_v4().to_string();
        let item_count = transaction.query_row(
            "SELECT COUNT(DISTINCT item_id) FROM email_digest_queue
             WHERE recipient = ?1 AND local_date = ?2",
            params![recipient, date],
            |row| row.get::<_, i64>(0),
        )?;
        let inserted = transaction.execute(
            "INSERT OR IGNORE INTO email_digest_deliveries
               (id, recipient, local_date, idempotency_key, item_count,
                attempted_at, result, attempt_count, next_attempt_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'CLAIMED', 0, ?6)",
            params![
                delivery_id,
                recipient,
                date,
                key,
                item_count,
                now.to_rfc3339()
            ],
        )?;
        if inserted == 0 {
            continue;
        }
        let notifications = digest_notifications(&transaction, &recipient, &date)?;
        jobs.push(EmailDigestJob {
            delivery_id,
            recipient,
            local_date: date,
            notifications,
        });
    }
    transaction.commit()?;
    Ok(jobs)
}

pub fn mark_digest_submitted(
    database: &Database,
    job: &EmailDigestJob,
    now: DateTime<Utc>,
) -> AppResult<()> {
    let mut connection = database.connection.lock().expect("database mutex poisoned");
    let transaction = connection.transaction()?;
    transaction.execute(
        "UPDATE email_digest_deliveries SET result = 'SUBMITTED', submitted_at = ?1,
           error_code = NULL, attempt_count = attempt_count + 1, next_attempt_at = NULL
         WHERE id = ?2 AND result = 'CLAIMED'",
        params![now.to_rfc3339(), job.delivery_id],
    )?;
    for notification in &job.notifications {
        transaction.execute(
            "DELETE FROM email_digest_queue
             WHERE recipient = ?1 AND local_date = ?2 AND item_id = ?3",
            params![job.recipient, job.local_date, notification.item_id],
        )?;
    }
    transaction.commit()?;
    Ok(())
}

pub fn mark_digest_failed(
    database: &Database,
    id: &str,
    error_code: &str,
    now: DateTime<Utc>,
) -> AppResult<()> {
    let connection = database.connection.lock().expect("database mutex poisoned");
    let attempts = connection.query_row(
        "SELECT attempt_count FROM email_digest_deliveries WHERE id = ?1",
        [id],
        |row| row.get::<_, u32>(0),
    )? + 1;
    let next = match attempts {
        1 => Some(now + chrono::Duration::minutes(1)),
        2 => Some(now + chrono::Duration::minutes(5)),
        _ => None,
    }
    .map(|value| value.to_rfc3339());
    connection.execute(
        "UPDATE email_digest_deliveries SET result = 'FAILED', error_code = ?1,
           attempt_count = ?2, next_attempt_at = ?3 WHERE id = ?4 AND result = 'CLAIMED'",
        params![error_code, attempts, next, id],
    )?;
    Ok(())
}

fn validate_rule(input: &EmailDeliveryRuleInput) -> AppResult<()> {
    if !DIMENSIONS.contains(&input.match_dimension.as_str()) {
        return Err(AppError::Validation("请选择邮件规则的匹配范围".into()));
    }
    if input.match_value.trim().is_empty() {
        return Err(AppError::Validation("请选择邮件规则要匹配的内容".into()));
    }
    if !STRATEGIES.contains(&input.strategy.as_str()) {
        return Err(AppError::Validation("请选择邮件投递时机".into()));
    }
    if !valid_email(input.recipient.trim()) {
        return Err(AppError::Validation("请填写有效的邮件收件地址".into()));
    }
    parse_time(&input.digest_local_time)?;
    Ok(())
}

fn validate_match_value(
    connection: &rusqlite::Connection,
    input: &EmailDeliveryRuleInput,
) -> AppResult<()> {
    let valid = match input.match_dimension.as_str() {
        "EVENT_KIND" => EVENT_KINDS.contains(&input.match_value.as_str()),
        "CATEGORY" => connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM categories WHERE id = ?1 AND archived = 0)",
            [&input.match_value],
            |row| row.get::<_, bool>(0),
        )?,
        "TAG" => connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM tags WHERE id = ?1 AND archived = 0)",
            [&input.match_value],
            |row| row.get::<_, bool>(0),
        )?,
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(AppError::Validation(
            "邮件规则引用的事件类型、类型或标签已经不存在".into(),
        ))
    }
}

fn valid_email(value: &str) -> bool {
    let Some((local, domain)) = value.rsplit_once('@') else {
        return false;
    };
    !local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.')
}

fn matching_rules(
    connection: &rusqlite::Connection,
    event_kind: Option<&str>,
    category_id: Option<&str>,
    tag_ids: &[String],
) -> AppResult<Vec<EmailDeliveryRule>> {
    let mut statement = connection.prepare(
        "SELECT id, match_dimension, match_value, strategy, recipient,
                digest_local_time, enabled, created_at, updated_at
         FROM email_delivery_rules WHERE enabled = 1
         ORDER BY CASE match_dimension WHEN 'TAG' THEN 1 WHEN 'CATEGORY' THEN 2 ELSE 3 END,
           updated_at DESC",
    )?;
    let rules = statement
        .query_map([], rule_from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rules
        .into_iter()
        .filter(|rule| match rule.match_dimension.as_str() {
            "EVENT_KIND" => event_kind == Some(rule.match_value.as_str()),
            "CATEGORY" => category_id == Some(rule.match_value.as_str()),
            "TAG" => tag_ids.iter().any(|tag| tag == &rule.match_value),
            _ => false,
        })
        .collect())
}

fn get_rule(connection: &rusqlite::Connection, id: &str) -> AppResult<EmailDeliveryRule> {
    connection
        .query_row(
            "SELECT id, match_dimension, match_value, strategy, recipient,
                    digest_local_time, enabled, created_at, updated_at
             FROM email_delivery_rules WHERE id = ?1",
            [id],
            rule_from_row,
        )
        .map_err(AppError::from)
}

fn rule_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<EmailDeliveryRule> {
    Ok(EmailDeliveryRule {
        id: row.get(0)?,
        match_dimension: row.get(1)?,
        match_value: row.get(2)?,
        strategy: row.get(3)?,
        recipient: row.get(4)?,
        digest_local_time: row.get(5)?,
        enabled: row.get::<_, i64>(6)? != 0,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

fn digest_notifications(
    connection: &rusqlite::Connection,
    recipient: &str,
    local_date: &str,
) -> AppResult<Vec<DueNotification>> {
    let mut statement = connection.prepare(
        "SELECT MIN(q.reminder_event_id), i.id, i.title, i.due_local_date,
                i.due_local_time, i.completion_policy, i.event_kind, i.reminder_plan
         FROM email_digest_queue q JOIN items i ON i.id = q.item_id
         WHERE q.recipient = ?1 AND q.local_date = ?2
         GROUP BY i.id, i.title, i.due_local_date, i.due_local_time,
                  i.completion_policy, i.event_kind, i.reminder_plan
         ORDER BY i.due_at, i.created_at",
    )?;
    let mut notifications = statement
        .query_map(params![recipient, local_date], |row| {
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
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for notification in &mut notifications {
        let mut tags = connection
            .prepare("SELECT tag_id FROM item_tags WHERE item_id = ?1 ORDER BY tag_id")?;
        notification.tag_ids = tags
            .query_map([&notification.item_id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
    }
    Ok(notifications)
}

fn recipient_key(recipient: &str) -> String {
    hex::encode(Sha256::digest(recipient.as_bytes()))[..16].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{CreateItemInput, EventConfigurationInput};
    use chrono::TimeZone;

    fn notification(item_id: String, event_id: &str) -> DueNotification {
        DueNotification {
            event_id: event_id.into(),
            item_id,
            title: "规则测试".into(),
            due_local_date: "2026-09-03".into(),
            due_local_time: "09:00".into(),
            completion_policy: "NORMAL".into(),
            event_kind: Some("ORDINARY".into()),
            reminder_plan: "REPEAT".into(),
            tag_ids: Vec::new(),
        }
    }

    fn enable_verified_smtp(database: &Database, now: DateTime<Utc>) {
        let connection = database.connection.lock().unwrap();
        connection
            .execute("UPDATE app_settings SET smtp_enabled = 1 WHERE id = 1", [])
            .unwrap();
        drop(connection);
        mark_smtp_verified(database, now).unwrap();
    }

    fn record_reminder_event(
        database: &Database,
        item_id: &str,
        event_id: &str,
        now: DateTime<Utc>,
    ) {
        let connection = database.connection.lock().unwrap();
        connection
            .execute(
                "INSERT INTO reminder_events
                   (id, item_id, scheduled_for, sent_at, idempotency_key, result)
                 VALUES (?1, ?2, ?3, ?3, ?4, 'SUBMITTED')",
                params![
                    event_id,
                    item_id,
                    now.to_rfc3339(),
                    format!("test:{event_id}")
                ],
            )
            .unwrap();
    }

    fn ordinary_item(database: &Database, now: DateTime<Utc>) -> String {
        database
            .create_item(
                &CreateItemInput {
                    title: "摘要测试".into(),
                    notes: String::new(),
                    category_id: None,
                    due_at: Some(now.to_rfc3339()),
                    must_complete_today: false,
                    repeat_interval_minutes: None,
                    tag_ids: Vec::new(),
                    event: Some(EventConfigurationInput {
                        kind: Some("ORDINARY".into()),
                        reminder_plan: Some("REPEAT".into()),
                        repeat_time_mode: Some("SPECIFIED".into()),
                        repeat_times: vec!["09:00".into()],
                        ..EventConfigurationInput::default()
                    }),
                },
                now,
            )
            .unwrap()
            .id
    }

    fn digest_rule(database: &Database, now: DateTime<Utc>) {
        save_rule(
            database,
            &EmailDeliveryRuleInput {
                id: None,
                match_dimension: "EVENT_KIND".into(),
                match_value: "ORDINARY".into(),
                strategy: "DAILY_DIGEST".into(),
                recipient: "me@example.com".into(),
                digest_local_time: "18:00".into(),
                enabled: true,
            },
            now,
        )
        .unwrap();
    }

    #[test]
    fn overlapping_rules_for_same_address_create_one_delivery() {
        let database = Database::in_memory().unwrap();
        let now = Utc::now();
        enable_verified_smtp(&database, now);
        let item = database
            .create_item(
                &CreateItemInput {
                    title: "规则测试".into(),
                    notes: String::new(),
                    category_id: Some("work".into()),
                    due_at: Some(now.to_rfc3339()),
                    must_complete_today: false,
                    repeat_interval_minutes: None,
                    tag_ids: Vec::new(),
                    event: Some(EventConfigurationInput {
                        kind: Some("ORDINARY".into()),
                        reminder_plan: Some("REPEAT".into()),
                        repeat_time_mode: Some("SPECIFIED".into()),
                        repeat_times: vec!["09:00".into()],
                        ..EventConfigurationInput::default()
                    }),
                },
                now,
            )
            .unwrap();
        for (dimension, value) in [("EVENT_KIND", "ORDINARY"), ("CATEGORY", "work")] {
            save_rule(
                &database,
                &EmailDeliveryRuleInput {
                    id: None,
                    match_dimension: dimension.into(),
                    match_value: value.into(),
                    strategy: "EACH_PLAN".into(),
                    recipient: "me@example.com".into(),
                    digest_local_time: "18:00".into(),
                    enabled: true,
                },
                now,
            )
            .unwrap();
        }
        record_reminder_event(&database, &item.id, "event-1", now);
        let jobs = claim_deliveries(&database, &notification(item.id, "event-1"), now).unwrap();
        assert_eq!(jobs.len(), 1);
    }

    #[test]
    fn daily_first_is_idempotent_per_local_day() {
        let database = Database::in_memory().unwrap();
        let now = Utc::now();
        enable_verified_smtp(&database, now);
        let item = database
            .create_item(
                &CreateItemInput {
                    title: "每日首次".into(),
                    notes: String::new(),
                    category_id: None,
                    due_at: Some(now.to_rfc3339()),
                    must_complete_today: false,
                    repeat_interval_minutes: None,
                    tag_ids: Vec::new(),
                    event: Some(EventConfigurationInput {
                        kind: Some("ORDINARY".into()),
                        reminder_plan: Some("REPEAT".into()),
                        repeat_time_mode: Some("SPECIFIED".into()),
                        repeat_times: vec!["09:00".into()],
                        ..EventConfigurationInput::default()
                    }),
                },
                now,
            )
            .unwrap();
        save_rule(
            &database,
            &EmailDeliveryRuleInput {
                id: None,
                match_dimension: "EVENT_KIND".into(),
                match_value: "ORDINARY".into(),
                strategy: "DAILY_FIRST".into(),
                recipient: "me@example.com".into(),
                digest_local_time: "18:00".into(),
                enabled: true,
            },
            now,
        )
        .unwrap();
        record_reminder_event(&database, &item.id, "event-1", now);
        record_reminder_event(&database, &item.id, "event-2", now);
        assert_eq!(
            claim_deliveries(&database, &notification(item.id.clone(), "event-1"), now)
                .unwrap()
                .len(),
            1
        );
        assert!(
            claim_deliveries(&database, &notification(item.id, "event-2"), now)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn missed_digest_is_claimed_after_restart_before_the_next_digest_time() {
        let database = Database::in_memory().unwrap();
        let queued_at = Local
            .with_ymd_and_hms(2026, 9, 3, 17, 30, 0)
            .single()
            .unwrap()
            .with_timezone(&Utc);
        enable_verified_smtp(&database, queued_at);
        let item_id = ordinary_item(&database, queued_at);
        digest_rule(&database, queued_at);
        record_reminder_event(&database, &item_id, "digest-event", queued_at);
        assert!(
            claim_deliveries(&database, &notification(item_id, "digest-event"), queued_at)
                .unwrap()
                .is_empty()
        );

        let restarted_at = Local
            .with_ymd_and_hms(2026, 9, 4, 8, 0, 0)
            .single()
            .unwrap()
            .with_timezone(&Utc);
        let jobs = claim_due_digests(&database, restarted_at).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].notifications.len(), 1);
    }

    #[test]
    fn interrupted_digest_claim_stops_after_three_recoveries() {
        let database = Database::in_memory().unwrap();
        let now = Local
            .with_ymd_and_hms(2026, 9, 3, 18, 1, 0)
            .single()
            .unwrap()
            .with_timezone(&Utc);
        enable_verified_smtp(&database, now);
        let item_id = ordinary_item(&database, now);
        digest_rule(&database, now);
        record_reminder_event(&database, &item_id, "digest-stale", now);
        claim_deliveries(&database, &notification(item_id, "digest-stale"), now).unwrap();
        assert_eq!(claim_due_digests(&database, now).unwrap().len(), 1);
        assert_eq!(
            claim_due_digests(&database, now + chrono::Duration::minutes(3))
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            claim_due_digests(&database, now + chrono::Duration::minutes(6))
                .unwrap()
                .len(),
            1
        );
        assert!(
            claim_due_digests(&database, now + chrono::Duration::minutes(9))
                .unwrap()
                .is_empty()
        );
    }
}
