use std::collections::HashMap;

use chrono::{
    DateTime, Datelike, Duration, Local, Months, NaiveDate, NaiveDateTime, TimeZone, Timelike, Utc,
};
use rusqlite::{params_from_iter, types::Value};
use serde::{Deserialize, Serialize};

use crate::{
    db::Database,
    domain::EVENT_KINDS,
    error::{AppError, AppResult},
};

const SCALES: [&str; 4] = ["DAY", "WEEK", "MONTH", "YEAR"];
const MAX_VISIBLE_ENTRIES: usize = 2_000;
const MAX_UNSCHEDULED_ITEMS: usize = 200;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineQuery {
    pub scale: String,
    pub anchor_date: String,
    #[serde(default)]
    pub include_done: bool,
    #[serde(default)]
    pub event_kinds: Vec<String>,
    pub category_id: Option<String>,
    #[serde(default)]
    pub tag_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineEntry {
    pub id: String,
    pub item_id: String,
    pub title: String,
    pub status: String,
    pub event_kind: Option<String>,
    pub category_name: Option<String>,
    pub tag_names: Vec<String>,
    pub shape: String,
    pub start_date: String,
    pub end_date: String,
    pub target_date: Option<String>,
    pub important: bool,
    pub occurrence_label: Option<String>,
    pub position: TimelinePosition,
    pub highlight: Option<TimelinePosition>,
    pub state_segments: Vec<TimelineStateSegment>,
    pub deadline_marker: Option<TimelineMarker>,
    pub completion_marker: Option<TimelineMarker>,
    pub overdue: bool,
    pub open_ended: bool,
    pub history_incomplete: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineStateSegment {
    pub kind: String,
    pub label: String,
    pub position: TimelinePosition,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineMarker {
    pub position: f64,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelinePosition {
    pub left: f64,
    pub width: f64,
    pub clipped_start: bool,
    pub clipped_end: bool,
    pub start_label: String,
    pub end_label: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TimelineTick {
    pub label: String,
    pub position: f64,
    pub weekend: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineUnscheduledItem {
    pub item_id: String,
    pub title: String,
    pub status: String,
    pub event_kind: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineData {
    pub scale: String,
    pub range_start: String,
    pub range_end: String,
    pub today: String,
    pub entries: Vec<TimelineEntry>,
    pub unscheduled: Vec<TimelineUnscheduledItem>,
    pub total_visible_count: u64,
    pub total_unscheduled_count: u64,
    pub truncated: bool,
    pub skipped_invalid_count: u64,
    pub ticks: Vec<TimelineTick>,
    pub today_position: Option<f64>,
}

struct TimelineSourceItem {
    id: String,
    title: String,
    status: String,
    category_name: Option<String>,
    created_at: String,
    completed_at: Option<String>,
    due_at: Option<String>,
    event_kind: Option<String>,
    start_at: Option<String>,
    end_at: Option<String>,
    target_at: Option<String>,
    lead_value: Option<u32>,
    lead_unit: Option<String>,
    important: bool,
    tags: Vec<TimelineSourceTag>,
    anchor_day: Option<u32>,
    anchor_month: Option<u32>,
}

struct TimelineSourceTag {
    name: String,
}

pub fn project_timeline(
    database: &Database,
    query: &TimelineQuery,
    now: DateTime<Utc>,
) -> AppResult<TimelineData> {
    project_in_zone(database, query, now, &Local)
}

fn project_in_zone<T: TimeZone>(
    database: &Database,
    query: &TimelineQuery,
    now: DateTime<Utc>,
    zone: &T,
) -> AppResult<TimelineData> {
    validate_query(query)?;
    let anchor = NaiveDate::parse_from_str(&query.anchor_date, "%Y-%m-%d")
        .map_err(|_| AppError::Validation("时间画布的日期无效".into()))?;
    let (range_start, range_end) = visible_range(&query.scale, anchor)?;
    let context = Projection::new(zone, &query.scale, range_start, range_end, now)?;
    let items = load_timeline_items(database, query)?;
    let mut entries = Vec::new();
    let mut unscheduled = Vec::new();
    let mut total_visible_count = 0_u64;
    let mut total_unscheduled_count = 0_u64;
    let mut skipped_invalid_count = 0_u64;

    for item in items {
        match project_item_limited(
            &item,
            &context,
            MAX_VISIBLE_ENTRIES.saturating_sub(entries.len()),
        ) {
            Ok(projected) => {
                total_visible_count += projected.total_count;
                entries.extend(projected.entries);
            }
            Err(_) => {
                total_unscheduled_count += 1;
                skipped_invalid_count += 1;
                if unscheduled.len() < MAX_UNSCHEDULED_ITEMS {
                    unscheduled.push(TimelineUnscheduledItem {
                        item_id: item.id,
                        title: item.title,
                        status: item.status,
                        event_kind: item.event_kind,
                        reason: "时间信息需要核对".into(),
                    });
                }
            }
        }
    }
    entries.sort_by(|left, right| {
        left.start_date
            .cmp(&right.start_date)
            .then_with(|| left.title.cmp(&right.title))
    });
    unscheduled.sort_by(|left, right| left.title.cmp(&right.title));
    Ok(TimelineData {
        scale: query.scale.clone(),
        range_start: range_start.to_string(),
        range_end: range_end.to_string(),
        today: now.with_timezone(zone).date_naive().to_string(),
        truncated: total_visible_count > entries.len() as u64
            || total_unscheduled_count > unscheduled.len() as u64,
        entries,
        unscheduled,
        total_visible_count,
        total_unscheduled_count,
        skipped_invalid_count,
        ticks: context.ticks()?,
        today_position: context
            .point(now)
            .filter(|point| *point >= 0.0 && *point < 100.0),
    })
}

pub fn save_timeline_preference(
    database: &Database,
    scale: &str,
    include_done: bool,
    now: DateTime<Utc>,
) -> AppResult<()> {
    if !SCALES.contains(&scale) {
        return Err(AppError::Validation("请选择有效的时间画布范围".into()));
    }
    let connection = database.connection.lock().expect("database mutex poisoned");
    connection.execute(
        "UPDATE timeline_view_preferences
         SET scale = ?1, include_done = ?2, updated_at = ?3 WHERE id = 1",
        rusqlite::params![scale, i64::from(include_done), now.to_rfc3339()],
    )?;
    Ok(())
}

fn validate_query(query: &TimelineQuery) -> AppResult<()> {
    if !SCALES.contains(&query.scale.as_str()) {
        return Err(AppError::Validation("请选择日、周、月或年视图".into()));
    }
    if query
        .event_kinds
        .iter()
        .any(|kind| !EVENT_KINDS.contains(&kind.as_str()))
    {
        return Err(AppError::Validation("时间画布包含无效的事件筛选".into()));
    }
    Ok(())
}

fn load_timeline_items(
    database: &Database,
    query: &TimelineQuery,
) -> AppResult<Vec<TimelineSourceItem>> {
    let connection = database.connection.lock().expect("database mutex poisoned");
    let mut conditions = vec!["i.status != 'DELETED'".to_string()];
    let mut parameters = Vec::new();
    if !query.include_done {
        conditions.push("i.status = 'OPEN'".into());
    }
    if !query.event_kinds.is_empty() {
        conditions.push(format!(
            "i.event_kind IN ({})",
            std::iter::repeat_n("?", query.event_kinds.len())
                .collect::<Vec<_>>()
                .join(", ")
        ));
        parameters.extend(query.event_kinds.iter().cloned().map(Value::Text));
    }
    if let Some(category_id) = &query.category_id {
        conditions.push("i.category_id = ?".into());
        parameters.push(Value::Text(category_id.clone()));
    }
    for tag_id in &query.tag_ids {
        conditions.push(
            "EXISTS (
               SELECT 1 FROM item_tags required_item_tag
               JOIN tags required_tag ON required_tag.id = required_item_tag.tag_id
               WHERE required_item_tag.item_id = i.id
                 AND required_item_tag.tag_id = ? AND required_tag.archived = 0
             )"
            .into(),
        );
        parameters.push(Value::Text(tag_id.clone()));
    }
    let condition = conditions.join(" AND ");
    let sql = format!(
        "SELECT i.id, i.title, i.status, c.name, i.created_at, i.completed_at, i.event_kind,
                i.start_at, i.end_at, i.target_at, i.lead_value, i.lead_unit, i.important,
                i.due_at, i.series_anchor_day, i.series_anchor_month
         FROM items i LEFT JOIN categories c ON c.id = i.category_id
         WHERE {condition}"
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(parameters.iter()), |row| {
        Ok(TimelineSourceItem {
            id: row.get(0)?,
            title: row.get(1)?,
            status: row.get(2)?,
            category_name: row.get(3)?,
            created_at: row.get(4)?,
            completed_at: row.get(5)?,
            event_kind: row.get(6)?,
            start_at: row.get(7)?,
            end_at: row.get(8)?,
            target_at: row.get(9)?,
            lead_value: row.get(10)?,
            lead_unit: row.get(11)?,
            important: row.get::<_, i64>(12)? != 0,
            tags: Vec::new(),
            due_at: row.get(13)?,
            anchor_day: row.get(14)?,
            anchor_month: row.get(15)?,
        })
    })?;
    let mut items = rows.collect::<Result<Vec<_>, _>>()?;
    drop(statement);

    let tags_sql = format!(
        "SELECT it.item_id, t.name
         FROM item_tags it
         JOIN tags t ON t.id = it.tag_id
         JOIN items i ON i.id = it.item_id
         WHERE {condition} AND t.archived = 0
         ORDER BY it.item_id, t.position, t.name"
    );
    let mut tags_statement = connection.prepare(&tags_sql)?;
    let tag_rows = tags_statement.query_map(params_from_iter(parameters.iter()), |row| {
        Ok((
            row.get::<_, String>(0)?,
            TimelineSourceTag { name: row.get(1)? },
        ))
    })?;
    let mut tags_by_item: HashMap<String, Vec<TimelineSourceTag>> = HashMap::new();
    for row in tag_rows {
        let (item_id, tag) = row?;
        tags_by_item.entry(item_id).or_default().push(tag);
    }
    for item in &mut items {
        item.tags = tags_by_item.remove(&item.id).unwrap_or_default();
    }
    Ok(items)
}

fn visible_range(scale: &str, anchor: NaiveDate) -> AppResult<(NaiveDate, NaiveDate)> {
    match scale {
        "DAY" => Ok((anchor, anchor)),
        "WEEK" => {
            let offset = i64::from(anchor.weekday().num_days_from_monday());
            let start = anchor - Duration::days(offset);
            Ok((start, start + Duration::days(6)))
        }
        "MONTH" => {
            let start = NaiveDate::from_ymd_opt(anchor.year(), anchor.month(), 1)
                .ok_or_else(|| AppError::Validation("无法计算月份范围".into()))?;
            Ok((start, last_day_of_month(anchor.year(), anchor.month())?))
        }
        "YEAR" => Ok((
            NaiveDate::from_ymd_opt(anchor.year(), 1, 1)
                .ok_or_else(|| AppError::Validation("无法计算年份范围".into()))?,
            NaiveDate::from_ymd_opt(anchor.year(), 12, 31)
                .ok_or_else(|| AppError::Validation("无法计算年份范围".into()))?,
        )),
        _ => Err(AppError::Validation("时间画布范围无效".into())),
    }
}

struct Projection<'a, T: TimeZone> {
    zone: &'a T,
    scale: &'a str,
    start_date: NaiveDate,
    end_date: NaiveDate,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    now: DateTime<Utc>,
}

impl<'a, T: TimeZone> Projection<'a, T> {
    fn new(
        zone: &'a T,
        scale: &'a str,
        start_date: NaiveDate,
        end_date: NaiveDate,
        now: DateTime<Utc>,
    ) -> AppResult<Self> {
        let next = end_date.succ_opt().ok_or_else(invalid_time)?;
        Ok(Self {
            zone,
            scale,
            start_date,
            end_date,
            start: resolve(zone, start_date.and_hms_opt(0, 0, 0).unwrap())?,
            end: resolve(zone, next.and_hms_opt(0, 0, 0).unwrap())?,
            now,
        })
    }

    fn point(&self, value: DateTime<Utc>) -> Option<f64> {
        if self.scale == "DAY" {
            let total = (self.end - self.start).num_milliseconds() as f64;
            return (total > 0.0)
                .then(|| (value - self.start).num_milliseconds() as f64 / total * 100.0);
        }
        let local = value.with_timezone(self.zone);
        let days = (self.end_date - self.start_date).num_days() + 1;
        let offset = (local.date_naive() - self.start_date).num_days() as f64
            + f64::from(local.time().num_seconds_from_midnight()) / 86_400.0;
        Some(offset / days as f64 * 100.0)
    }

    fn span(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Option<TimelinePosition> {
        if start > end || start >= self.end || end < self.start {
            return None;
        }
        let left = self.point(start)?.clamp(0.0, 100.0);
        let right = self.point(end)?.clamp(0.0, 100.0);
        Some(TimelinePosition {
            left,
            width: (right - left).max(0.0),
            clipped_start: start < self.start,
            clipped_end: end >= self.end,
            start_label: self.time_label(start),
            end_label: self.time_label(end),
        })
    }

    fn time_label(&self, value: DateTime<Utc>) -> String {
        let local = value.with_timezone(self.zone);
        if matches!(
            self.zone.from_local_datetime(&local.naive_local()),
            chrono::LocalResult::Ambiguous(_, _)
        ) {
            local
                .fixed_offset()
                .format("%Y/%m/%d %H:%M (%:z)")
                .to_string()
        } else {
            local.naive_local().format("%Y/%m/%d %H:%M").to_string()
        }
    }

    fn ticks(&self) -> AppResult<Vec<TimelineTick>> {
        let mut ticks = Vec::new();
        if self.scale == "DAY" {
            let mut hour = self.start;
            while hour < self.end {
                let local = hour.with_timezone(self.zone);
                // 回拨日重复小时带 UTC 偏移，保留真实时间顺序。
                let label = if (self.end - self.start).num_hours() != 24 {
                    local.fixed_offset().format("%H:%M (%:z)").to_string()
                } else {
                    local.naive_local().format("%H:%M").to_string()
                };
                ticks.push(TimelineTick {
                    label,
                    position: self.point(hour).unwrap_or(0.0),
                    weekend: false,
                });
                hour += Duration::hours(1);
            }
        } else {
            let mut day = self.start_date;
            while day <= self.end_date {
                if self.scale != "YEAR" || day.day() == 1 {
                    let label = if self.scale == "YEAR" {
                        format!("{}月", day.month())
                    } else if self.scale == "WEEK" {
                        format!("{}/{}", day.month(), day.day())
                    } else {
                        format!("{:02}", day.day())
                    };
                    ticks.push(TimelineTick {
                        label,
                        position: self
                            .point(resolve(self.zone, day.and_hms_opt(0, 0, 0).unwrap())?)
                            .unwrap_or(0.0),
                        weekend: day.weekday().number_from_monday() > 5,
                    });
                }
                day = day.succ_opt().ok_or_else(invalid_time)?;
            }
        }
        Ok(ticks)
    }
}

fn invalid_time() -> AppError {
    AppError::Validation("时间信息需要核对".into())
}

fn timestamp(value: &str) -> AppResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|date| date.with_timezone(&Utc))
        .map_err(|_| invalid_time())
}

fn resolve<T: TimeZone>(zone: &T, local: NaiveDateTime) -> AppResult<DateTime<Utc>> {
    // 重复时刻取较早一次；夏令时跳跃顺移到第一个有效分钟，规则不依赖系统当前时间。
    for minute in 0..=180 {
        if let Some(value) = zone
            .from_local_datetime(&(local + Duration::minutes(minute)))
            .earliest()
        {
            return Ok(value.with_timezone(&Utc));
        }
    }
    Err(invalid_time())
}

#[cfg(test)]
fn project_item<T: TimeZone>(
    item: &TimelineSourceItem,
    context: &Projection<'_, T>,
) -> AppResult<Vec<TimelineEntry>> {
    Ok(project_item_limited(item, context, usize::MAX)?.entries)
}

#[derive(Default)]
struct ProjectedEntries {
    entries: Vec<TimelineEntry>,
    total_count: u64,
}

impl ProjectedEntries {
    fn add<'a, T: TimeZone>(
        &'a mut self,
        item: &TimelineSourceItem,
        segment: Segment<'_>,
        context: &Projection<'_, T>,
        limit: usize,
    ) -> Option<&'a mut TimelineEntry> {
        if segment.start > segment.end
            || segment.start >= context.end
            || segment.end < context.start
        {
            return None;
        }
        self.total_count += 1;
        // Keep exact counts and validation, but do not format discarded payloads.
        if self.entries.len() >= limit {
            return None;
        }
        let entry = make_entry(item, segment, context)?;
        self.entries.push(entry);
        self.entries.last_mut()
    }
}

fn project_item_limited<T: TimeZone>(
    item: &TimelineSourceItem,
    context: &Projection<'_, T>,
    limit: usize,
) -> AppResult<ProjectedEntries> {
    let created = timestamp(&item.created_at)?;
    let completed = item.completed_at.as_deref().map(timestamp).transpose()?;
    if completed.is_some_and(|value| value < created) {
        return Err(invalid_time());
    }
    let kind = item.event_kind.as_deref();
    let end_value = match kind {
        Some("CONTINUOUS") => item.end_at.as_deref(),
        Some("WARNING") => item.target_at.as_deref(),
        _ => item.due_at.as_deref(),
    };
    let planned_end = end_value
        .map(timestamp)
        .transpose()?
        .or(completed)
        .unwrap_or(context.now);
    if planned_end < created {
        return Err(invalid_time());
    }
    if matches!(kind, Some("MONTHLY" | "YEARLY")) && end_value.is_some() {
        return periodic_entries(item, created, planned_end, context, limit);
    }
    let highlight = match kind {
        Some("CONTINUOUS") => item.start_at.as_deref().map(timestamp).transpose()?,
        Some("WARNING") => match (item.lead_value, item.lead_unit.as_deref()) {
            (Some(amount), Some(unit)) if amount > 0 => {
                let local = planned_end.with_timezone(context.zone).naive_local();
                let shifted = match unit {
                    "DAY" => local.checked_sub_signed(Duration::days(i64::from(amount))),
                    "WEEK" => local.checked_sub_signed(Duration::weeks(i64::from(amount))),
                    "MONTH" => local.checked_sub_months(Months::new(amount)),
                    "YEAR" => local.checked_sub_months(Months::new(
                        amount.checked_mul(12).ok_or_else(invalid_time)?,
                    )),
                    _ => None,
                }
                .ok_or_else(invalid_time)?;
                Some(resolve(context.zone, shifted)?)
            }
            _ => None,
        },
        _ => None,
    };
    if highlight.is_some_and(|start| start > planned_end) {
        return Err(invalid_time());
    }
    let display_end = if item.status == "DONE" {
        completed.map_or(planned_end, |value| value.max(planned_end))
    } else if end_value.is_some() && planned_end < context.now {
        context.now
    } else {
        planned_end
    };
    let shape = if kind == Some("WARNING") {
        "WARNING"
    } else {
        "RANGE"
    };
    let mut result = ProjectedEntries::default();
    result.add(
        item,
        Segment {
            start: created,
            end: display_end,
            shape,
            highlight: highlight.map(|start| start.max(created)),
            deadline: end_value.map(|_| planned_end),
            business_start: highlight,
            open_ended: end_value.is_none() && item.status == "OPEN",
            history_incomplete: false,
            occurrence_label: None,
            future_preview: false,
            allow_overdue: true,
        },
        context,
        limit,
    );
    Ok(result)
}

struct Segment<'a> {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    shape: &'a str,
    highlight: Option<DateTime<Utc>>,
    deadline: Option<DateTime<Utc>>,
    business_start: Option<DateTime<Utc>>,
    open_ended: bool,
    history_incomplete: bool,
    occurrence_label: Option<String>,
    future_preview: bool,
    allow_overdue: bool,
}

fn make_entry<T: TimeZone>(
    item: &TimelineSourceItem,
    segment: Segment<'_>,
    context: &Projection<'_, T>,
) -> Option<TimelineEntry> {
    let Segment {
        start,
        end,
        shape,
        highlight,
        deadline,
        business_start,
        open_ended,
        history_incomplete,
        occurrence_label,
        future_preview,
        allow_overdue,
    } = segment;
    let position = context.span(start, end)?;
    let completion = item
        .completed_at
        .as_deref()
        .and_then(|value| timestamp(value).ok());
    let state_segments = build_state_segments(
        item,
        context,
        start,
        end,
        deadline,
        business_start,
        completion,
        future_preview,
        allow_overdue,
    );
    let overdue = state_segments
        .iter()
        .any(|segment| segment.kind == "OVERDUE");
    Some(TimelineEntry {
        id: format!("{}:{}", item.id, end.to_rfc3339()),
        item_id: item.id.clone(),
        title: item.title.clone(),
        status: item.status.clone(),
        event_kind: item.event_kind.clone(),
        category_name: item.category_name.clone(),
        tag_names: item.tags.iter().map(|tag| tag.name.clone()).collect(),
        shape: shape.into(),
        start_date: start.with_timezone(context.zone).date_naive().to_string(),
        end_date: end.with_timezone(context.zone).date_naive().to_string(),
        target_date: deadline
            .map(|value| value.with_timezone(context.zone).date_naive().to_string()),
        important: item.important,
        occurrence_label,
        position,
        highlight: highlight.and_then(|start| context.span(start, end)),
        state_segments,
        deadline_marker: deadline.and_then(|value| marker(context, value, "截止")),
        completion_marker: completion.and_then(|value| marker(context, value, "完成")),
        overdue,
        open_ended,
        history_incomplete,
    })
}

#[allow(clippy::too_many_arguments)]
fn build_state_segments<T: TimeZone>(
    item: &TimelineSourceItem,
    context: &Projection<'_, T>,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    deadline: Option<DateTime<Utc>>,
    business_start: Option<DateTime<Utc>>,
    completion: Option<DateTime<Utc>>,
    future_preview: bool,
    allow_overdue: bool,
) -> Vec<TimelineStateSegment> {
    let mut states = Vec::new();
    let mut push = |kind: &str, label: &str, from: DateTime<Utc>, to: DateTime<Utc>| {
        if to <= from {
            return;
        }
        if let Some(position) = context.span(from, to) {
            states.push(TimelineStateSegment {
                kind: kind.into(),
                label: format!(
                    "{label}：{} 至 {}",
                    position.start_label, position.end_label
                ),
                position,
            });
        }
    };

    if future_preview {
        push("FUTURE", "未来计划", start, end);
        return states;
    }
    if item.status == "DONE" && completion.is_none() {
        push("COMPLETED", "已完成（完成时间未记录）", start, end);
        return states;
    }

    let active_start = business_start.unwrap_or(start).clamp(start, end);
    let completion_value = completion.filter(|_| item.status == "DONE");
    let recorded_end = completion_value
        .map(|value| value.min(active_start))
        .unwrap_or(active_start)
        .clamp(start, end);
    push("RECORDED", "已记录", start, recorded_end);

    let active_ceiling = deadline.unwrap_or(end).min(end);
    let active_end = completion_value
        .map(|value| value.min(active_ceiling))
        .unwrap_or(active_ceiling)
        .clamp(start, end);
    push("ACTIVE", "进行中", active_start.min(active_end), active_end);

    if allow_overdue {
        if let Some(due) = deadline {
            let overdue_end = if item.status == "DONE" {
                completion_value.unwrap_or(due).min(end)
            } else {
                end
            };
            push("OVERDUE", "已逾期", due.max(start), overdue_end);
        }
    }

    if let Some(value) = completion_value {
        if value < end {
            push("COMPLETED", "已完成", value.max(start), end);
        }
    }
    states
}

fn marker<T: TimeZone>(
    context: &Projection<'_, T>,
    value: DateTime<Utc>,
    label: &str,
) -> Option<TimelineMarker> {
    let position = context.point(value)?;
    (0.0..=100.0).contains(&position).then(|| TimelineMarker {
        position,
        label: format!("{label}：{}", context.time_label(value)),
    })
}

fn periodic_entries<T: TimeZone>(
    item: &TimelineSourceItem,
    created: DateTime<Utc>,
    anchor: DateTime<Utc>,
    context: &Projection<'_, T>,
    limit: usize,
) -> AppResult<ProjectedEntries> {
    let mut result = ProjectedEntries::default();
    let local = anchor.with_timezone(context.zone).naive_local();
    let step = if item.event_kind.as_deref() == Some("YEARLY") {
        12
    } else {
        1
    };
    let anchor_day = item.anchor_day.unwrap_or(local.day());
    let anchor_month = item.anchor_month.unwrap_or(local.month());
    if !(1..=31).contains(&anchor_day) || !(1..=12).contains(&anchor_month) {
        return Err(invalid_time());
    }
    // 当前截止之前的旧周期没有持久化快照，不能反推出历史。
    result.add(
        item,
        Segment {
            start: created,
            end: anchor,
            shape: "OCCURRENCE",
            highlight: None,
            deadline: Some(anchor),
            business_start: None,
            open_ended: false,
            history_incomplete: true,
            occurrence_label: Some("当前已知周期".into()),
            future_preview: false,
            allow_overdue: false,
        },
        context,
        limit,
    );
    if item.status == "DONE" {
        return Ok(result);
    }
    let mut previous = anchor;
    // 跳过视窗前的确定性未来周期，避免远期视窗与旧创建日期产生无界循环。
    let range_month = context.start_date.year() * 12 + context.start_date.month0() as i32;
    let anchor_month_index = local.year() * 12 + local.month0() as i32;
    let first_index = ((range_month - anchor_month_index) / step as i32 - 1).max(1) as u32;
    let occurrence = |index: u32| -> AppResult<DateTime<Utc>> {
        let base = local
            .date()
            .with_day(1)
            .unwrap()
            .checked_add_months(Months::new(
                index.checked_mul(step).ok_or_else(invalid_time)?,
            ))
            .ok_or_else(invalid_time)?;
        let month = if step == 12 {
            anchor_month
        } else {
            base.month()
        };
        let date = date_with_clamped_day(base.year(), month, anchor_day)?;
        resolve(context.zone, date.and_time(local.time()))
    };
    if first_index > 1 {
        previous = occurrence(first_index - 1)?;
    }
    for index in first_index..first_index + 15 {
        let end = occurrence(index)?;
        if end <= previous {
            return Err(invalid_time());
        }
        if let Some(entry) = result.add(
            item,
            Segment {
                start: previous,
                end,
                shape: "OCCURRENCE",
                highlight: None,
                deadline: Some(end),
                business_start: None,
                open_ended: false,
                history_incomplete: true,
                occurrence_label: Some(format!(
                    "{} 计划",
                    end.with_timezone(context.zone)
                        .naive_local()
                        .format("%Y/%m")
                )),
                future_preview: previous >= context.now,
                allow_overdue: false,
            },
            context,
            limit,
        ) {
            entry.overdue = false;
        }
        if end >= context.end {
            break;
        }
        previous = end;
    }
    Ok(result)
}

fn last_day_of_month(year: i32, month: u32) -> AppResult<NaiveDate> {
    let first = NaiveDate::from_ymd_opt(year, month, 1)
        .ok_or_else(|| AppError::Validation("月份无效".into()))?;
    let next = first
        .checked_add_months(Months::new(1))
        .ok_or_else(|| AppError::Validation("月份超出可用范围".into()))?;
    Ok(next - Duration::days(1))
}

fn date_with_clamped_day(year: i32, month: u32, day: u32) -> AppResult<NaiveDate> {
    Ok(NaiveDate::from_ymd_opt(year, month, day).unwrap_or(last_day_of_month(year, month)?))
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;
    use crate::domain::{CreateItemInput, EventConfigurationInput, TaxonomyInput};

    fn query(scale: &str, anchor: &str) -> TimelineQuery {
        TimelineQuery {
            scale: scale.into(),
            anchor_date: anchor.into(),
            include_done: false,
            event_kinds: Vec::new(),
            category_id: None,
            tag_ids: Vec::new(),
        }
    }

    #[test]
    fn month_range_uses_calendar_boundaries() {
        assert_eq!(
            visible_range("MONTH", NaiveDate::from_ymd_opt(2028, 2, 15).unwrap()).unwrap(),
            (
                NaiveDate::from_ymd_opt(2028, 2, 1).unwrap(),
                NaiveDate::from_ymd_opt(2028, 2, 29).unwrap()
            )
        );
    }

    #[test]
    fn payload_limit_preserves_cycle_counts_and_invalid_time_validation() {
        let day = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 9, 30).unwrap();
        let context = Projection::new(
            &Utc,
            "MONTH",
            day,
            end,
            timestamp("2026-09-07T12:00:00Z").unwrap(),
        )
        .unwrap();
        let mut item = sample();
        item.event_kind = Some("MONTHLY".into());
        item.due_at = Some("2026-09-10T18:00:00Z".into());
        let full = project_item(&item, &context).unwrap();
        assert!(full.len() > 1);
        let limited = project_item_limited(&item, &context, 1).unwrap();
        assert_eq!(limited.total_count, full.len() as u64);
        assert_eq!(limited.entries.len(), 1);
        assert_eq!(limited.entries[0].end_date, full[0].end_date);
        let count_only = project_item_limited(&item, &context, 0).unwrap();
        assert_eq!(count_only.total_count, full.len() as u64);
        assert!(count_only.entries.is_empty());
        item.created_at = "damaged-time".into();
        assert!(project_item_limited(&item, &context, 0).is_err());
    }

    // Fixed 2026 US-style transitions; no host timezone or extra runtime dependency.
    #[derive(Clone, Copy)]
    struct TransitionZone;

    impl TimeZone for TransitionZone {
        type Offset = chrono::FixedOffset;
        fn from_offset(_: &Self::Offset) -> Self {
            Self
        }
        fn offset_from_local_date(&self, date: &NaiveDate) -> chrono::LocalResult<Self::Offset> {
            self.offset_from_local_datetime(&date.and_hms_opt(12, 0, 0).unwrap())
        }
        fn offset_from_local_datetime(
            &self,
            local: &NaiveDateTime,
        ) -> chrono::LocalResult<Self::Offset> {
            let standard = chrono::FixedOffset::west_opt(5 * 3600).unwrap();
            let daylight = chrono::FixedOffset::west_opt(4 * 3600).unwrap();
            let spring = NaiveDate::from_ymd_opt(2026, 3, 8)
                .unwrap()
                .and_hms_opt(2, 0, 0)
                .unwrap();
            let fall = NaiveDate::from_ymd_opt(2026, 11, 1)
                .unwrap()
                .and_hms_opt(1, 0, 0)
                .unwrap();
            if *local >= spring && *local < spring + Duration::hours(1) {
                chrono::LocalResult::None
            } else if *local >= fall && *local < fall + Duration::hours(1) {
                chrono::LocalResult::Ambiguous(daylight, standard)
            } else {
                chrono::LocalResult::Single(if *local >= spring && *local < fall {
                    daylight
                } else {
                    standard
                })
            }
        }
        fn offset_from_utc_date(&self, date: &NaiveDate) -> Self::Offset {
            self.offset_from_utc_datetime(&date.and_hms_opt(12, 0, 0).unwrap())
        }
        fn offset_from_utc_datetime(&self, utc: &NaiveDateTime) -> Self::Offset {
            let spring = NaiveDate::from_ymd_opt(2026, 3, 8)
                .unwrap()
                .and_hms_opt(7, 0, 0)
                .unwrap();
            let fall = NaiveDate::from_ymd_opt(2026, 11, 1)
                .unwrap()
                .and_hms_opt(6, 0, 0)
                .unwrap();
            chrono::FixedOffset::west_opt(if *utc >= spring && *utc < fall {
                4 * 3600
            } else {
                5 * 3600
            })
            .unwrap()
        }
    }

    #[test]
    fn dst_days_keep_23_and_25_hours_and_distinguish_repeated_hour() {
        let spring = NaiveDate::from_ymd_opt(2026, 3, 8).unwrap();
        let spring_view = Projection::new(
            &TransitionZone,
            "DAY",
            spring,
            spring,
            timestamp("2026-03-08T12:00:00Z").unwrap(),
        )
        .unwrap();
        let ticks = spring_view.ticks().unwrap();
        assert_eq!(ticks.len(), 23);
        assert!(!ticks.iter().any(|tick| tick.label.starts_with("02:00")));
        assert!(
            (spring_view
                .point(timestamp("2026-03-08T07:00:00Z").unwrap())
                .unwrap()
                - 200.0 / 23.0)
                .abs()
                < 0.001
        );
        assert_eq!(
            resolve(&TransitionZone, spring.and_hms_opt(2, 30, 0).unwrap()).unwrap(),
            timestamp("2026-03-08T07:00:00Z").unwrap()
        );

        let fall = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
        let fall_view = Projection::new(
            &TransitionZone,
            "DAY",
            fall,
            fall,
            timestamp("2026-11-01T12:00:00Z").unwrap(),
        )
        .unwrap();
        let ticks = fall_view.ticks().unwrap();
        assert_eq!(ticks.len(), 25);
        let repeated: Vec<_> = ticks
            .iter()
            .filter(|tick| tick.label.starts_with("01:00"))
            .collect();
        assert_eq!(repeated.len(), 2);
        assert_ne!(repeated[0].label, repeated[1].label);
        assert!(repeated[0].position < repeated[1].position);
        let interval = fall_view
            .span(
                timestamp("2026-11-01T05:30:00Z").unwrap(),
                timestamp("2026-11-01T06:30:00Z").unwrap(),
            )
            .unwrap();
        assert!((interval.width - 4.0).abs() < 0.001);
        assert_ne!(interval.start_label, interval.end_label);
    }

    #[test]
    fn continuous_and_warning_events_keep_distinct_shapes() {
        let database = Database::in_memory().unwrap();
        let now = DateTime::parse_from_rfc3339("2026-09-03T08:00:00+08:00")
            .unwrap()
            .with_timezone(&Utc);
        for input in [
            CreateItemInput {
                title: "持续准备".into(),
                notes: String::new(),
                category_id: None,
                due_at: Some("2026-09-05T09:00:00+08:00".into()),
                must_complete_today: false,
                repeat_interval_minutes: None,
                tag_ids: Vec::new(),
                event: Some(EventConfigurationInput {
                    kind: Some("CONTINUOUS".into()),
                    reminder_plan: Some("CUSTOM".into()),
                    start_at: Some("2026-09-02T09:00:00+08:00".into()),
                    end_at: Some("2026-09-08T18:00:00+08:00".into()),
                    cadence_value: Some(1),
                    cadence_unit: Some("DAY".into()),
                    ..EventConfigurationInput::default()
                }),
            },
            CreateItemInput {
                title: "发布预警".into(),
                notes: String::new(),
                category_id: None,
                due_at: Some("2026-09-10T09:00:00+08:00".into()),
                must_complete_today: false,
                repeat_interval_minutes: None,
                tag_ids: Vec::new(),
                event: Some(EventConfigurationInput {
                    kind: Some("WARNING".into()),
                    reminder_plan: Some("REPEAT".into()),
                    target_at: Some("2026-09-10T09:00:00+08:00".into()),
                    lead_value: Some(1),
                    lead_unit: Some("WEEK".into()),
                    repeat_time_mode: Some("SPECIFIED".into()),
                    repeat_times: vec!["09:00".into()],
                    ..EventConfigurationInput::default()
                }),
            },
        ] {
            database.create_item(&input, now).unwrap();
        }
        let timeline = project_timeline(&database, &query("MONTH", "2026-09-03"), now).unwrap();
        assert!(timeline.entries.iter().any(|entry| entry.shape == "RANGE"));
        assert!(
            timeline
                .entries
                .iter()
                .any(|entry| entry.shape == "WARNING")
        );
    }

    #[test]
    fn event_category_and_tag_filters_are_combined() {
        let database = Database::in_memory().unwrap();
        let now = DateTime::parse_from_rfc3339("2026-09-03T08:00:00+08:00")
            .unwrap()
            .with_timezone(&Utc);
        let tag = database
            .create_tag(&TaxonomyInput {
                name: "发布".into(),
                color: "#D94B35".into(),
            })
            .unwrap();
        database
            .create_item(
                &CreateItemInput {
                    title: "带标签的发布事项".into(),
                    notes: String::new(),
                    category_id: Some("work".into()),
                    due_at: Some("2026-09-15T09:00:00+08:00".into()),
                    must_complete_today: false,
                    repeat_interval_minutes: None,
                    tag_ids: vec![tag.id.clone()],
                    event: Some(EventConfigurationInput {
                        kind: Some("ORDINARY".into()),
                        reminder_plan: Some("ONCE".into()),
                        ..EventConfigurationInput::default()
                    }),
                },
                now,
            )
            .unwrap();
        database
            .create_item(
                &CreateItemInput {
                    title: "没有标签的发布事项".into(),
                    notes: String::new(),
                    category_id: Some("work".into()),
                    due_at: Some("2026-09-15T09:00:00+08:00".into()),
                    must_complete_today: false,
                    repeat_interval_minutes: None,
                    tag_ids: Vec::new(),
                    event: Some(EventConfigurationInput {
                        kind: Some("ORDINARY".into()),
                        reminder_plan: Some("ONCE".into()),
                        ..EventConfigurationInput::default()
                    }),
                },
                now,
            )
            .unwrap();
        let timeline = project_timeline(
            &database,
            &TimelineQuery {
                event_kinds: vec!["ORDINARY".into()],
                category_id: Some("work".into()),
                tag_ids: vec![tag.id],
                ..query("MONTH", "2026-09-03")
            },
            now,
        )
        .unwrap();

        assert_eq!(timeline.entries.len(), 1);
        assert_eq!(timeline.entries[0].title, "带标签的发布事项");
        assert_eq!(timeline.entries[0].tag_names, vec!["发布"]);
    }

    #[test]
    fn invalid_time_payload_is_bounded_and_reports_the_full_count() {
        let database = Database::in_memory().unwrap();
        {
            let connection = database.connection.lock().expect("database mutex poisoned");
            connection
                .execute_batch(
                    "WITH RECURSIVE sequence(value) AS (
                       VALUES(1)
                       UNION ALL
                       SELECT value + 1 FROM sequence WHERE value < 250
                     )
                     INSERT INTO items (
                       id, title, status, due_at, due_local_date, due_local_time,
                       due_source, rollover_policy, completion_policy, created_at, updated_at
                     )
                     SELECT
                       printf('unscheduled-%03d', value), printf('待排期 %03d', value), 'OPEN',
                       '2026-09-15T01:00:00Z', '2026-09-15', '09:00',
                       'EXPLICIT', 'NONE', 'NORMAL',
                       'invalid-time', '2026-09-01T00:00:00Z'
                     FROM sequence;",
                )
                .unwrap();
        }
        let now = DateTime::parse_from_rfc3339("2026-09-03T08:00:00+08:00")
            .unwrap()
            .with_timezone(&Utc);
        let timeline = project_timeline(&database, &query("MONTH", "2026-09-03"), now).unwrap();

        assert_eq!(timeline.total_unscheduled_count, 250);
        assert_eq!(timeline.unscheduled.len(), MAX_UNSCHEDULED_ITEMS);
        assert!(timeline.truncated);
    }

    fn sample() -> TimelineSourceItem {
        TimelineSourceItem {
            id: "sample".into(),
            title: "核对预算".into(),
            status: "OPEN".into(),
            category_name: None,
            created_at: "2026-09-07T09:00:00+08:00".into(),
            completed_at: None,
            due_at: Some("2026-09-12T18:00:00+08:00".into()),
            event_kind: Some("ORDINARY".into()),
            start_at: None,
            end_at: None,
            target_at: None,
            lead_value: None,
            lead_unit: None,
            important: false,
            tags: vec![],
            anchor_day: None,
            anchor_month: None,
        }
    }

    #[test]
    fn ordinary_kinds_keep_the_deadline_marker_and_extend_overdue_to_now() {
        let zone = chrono::FixedOffset::east_opt(8 * 3600).unwrap();
        let context = Projection::new(
            &zone,
            "MONTH",
            NaiveDate::from_ymd_opt(2026, 9, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 9, 30).unwrap(),
            timestamp("2026-09-15T00:00:00Z").unwrap(),
        )
        .unwrap();
        for kind in [None, Some("ORDINARY"), Some("ONE_TIME"), Some("TODAY_MUST")] {
            let mut item = sample();
            item.event_kind = kind.map(String::from);
            let entries = project_item(&item, &context).unwrap();
            assert_eq!(entries[0].start_date, "2026-09-07");
            assert_eq!(entries[0].end_date, "2026-09-15");
            assert_eq!(entries[0].target_date.as_deref(), Some("2026-09-12"));
            assert_eq!(entries[0].shape, "RANGE");
            assert!(entries[0].overdue);
            assert!(entries[0].deadline_marker.is_some());
            assert_eq!(entries[0].state_segments.last().unwrap().kind, "OVERDUE");
            item.status = "DONE".into();
            let done = project_item(&item, &context).unwrap();
            assert!(!done[0].overdue);
            assert_eq!(done[0].end_date, "2026-09-12");
            assert_eq!(done[0].state_segments[0].kind, "COMPLETED");
        }
    }

    #[test]
    fn completion_marker_splits_early_and_late_completion_without_guessing() {
        let zone = chrono::FixedOffset::east_opt(8 * 3600).unwrap();
        let context = Projection::new(
            &zone,
            "MONTH",
            NaiveDate::from_ymd_opt(2026, 9, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 9, 30).unwrap(),
            timestamp("2026-09-15T00:00:00Z").unwrap(),
        )
        .unwrap();
        let mut item = sample();
        item.status = "DONE".into();
        item.completed_at = Some("2026-09-10T10:00:00+08:00".into());
        let early = project_item(&item, &context).unwrap();
        assert_eq!(early[0].end_date, "2026-09-12");
        assert_eq!(early[0].state_segments.last().unwrap().kind, "COMPLETED");
        assert!(early[0].completion_marker.is_some());

        item.completed_at = Some("2026-09-14T10:00:00+08:00".into());
        let late = project_item(&item, &context).unwrap();
        assert_eq!(late[0].end_date, "2026-09-14");
        assert!(late[0].overdue);
        assert_eq!(late[0].state_segments.last().unwrap().kind, "OVERDUE");
    }

    #[test]
    fn day_view_has_hour_positions_and_clipped_continuations() {
        let zone = chrono::FixedOffset::east_opt(8 * 3600).unwrap();
        let date = NaiveDate::from_ymd_opt(2026, 9, 7).unwrap();
        let context = Projection::new(
            &zone,
            "DAY",
            date,
            date,
            timestamp("2026-09-07T04:00:00Z").unwrap(),
        )
        .unwrap();
        let entry = &project_item(&sample(), &context).unwrap()[0];
        assert!((entry.position.left - 37.5).abs() < 0.001);
        assert!(entry.position.clipped_end);
        assert_eq!(context.ticks().unwrap().len(), 24);
        assert_eq!(context.point(context.now), Some(50.0));
        let tomorrow = Projection::new(
            &zone,
            "DAY",
            date.succ_opt().unwrap(),
            date.succ_opt().unwrap(),
            context.now,
        )
        .unwrap();
        assert!(
            project_item(&sample(), &tomorrow).unwrap()[0]
                .position
                .clipped_start
        );
    }

    #[test]
    fn no_deadline_grows_to_now_and_invalid_boundaries_are_not_hidden() {
        let zone = Utc;
        let mut item = sample();
        item.due_at = None;
        let date = NaiveDate::from_ymd_opt(2026, 9, 7).unwrap();
        for day in [7, 8] {
            let now = timestamp(&format!("2026-09-{day:02}T12:00:00Z")).unwrap();
            let context = Projection::new(
                &zone,
                "MONTH",
                date,
                NaiveDate::from_ymd_opt(2026, 9, 30).unwrap(),
                now,
            )
            .unwrap();
            let projected = project_item(&item, &context).unwrap();
            assert!(projected[0].open_ended);
            assert_eq!(projected[0].end_date, format!("2026-09-{day:02}"));
            assert!(!projected[0].overdue);
        }
        let context = Projection::new(
            &zone,
            "DAY",
            date,
            date,
            timestamp("2026-09-06T12:00:00Z").unwrap(),
        )
        .unwrap();
        assert!(project_item(&item, &context).is_err());
        item.created_at = "broken".into();
        assert!(project_item(&item, &context).is_err());
    }

    #[test]
    fn activity_and_warning_are_highlights_inside_the_lifecycle() {
        let zone = Utc;
        let context = Projection::new(
            &zone,
            "MONTH",
            NaiveDate::from_ymd_opt(2026, 9, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 9, 30).unwrap(),
            timestamp("2026-09-09T12:00:00Z").unwrap(),
        )
        .unwrap();
        let mut item = sample();
        item.event_kind = Some("CONTINUOUS".into());
        item.start_at = Some("2026-09-10T09:00:00Z".into());
        item.end_at = Some("2026-09-15T18:00:00Z".into());
        let projected = project_item(&item, &context).unwrap();
        assert!(projected[0].highlight.as_ref().unwrap().left > projected[0].position.left);
        item.event_kind = Some("WARNING".into());
        item.target_at = item.end_at.clone();
        item.lead_value = Some(2);
        item.lead_unit = Some("DAY".into());
        let projected = project_item(&item, &context).unwrap();
        assert_eq!(
            projected[0].highlight.as_ref().unwrap().start_label,
            "2026/09/13 18:00"
        );
        assert_eq!(projected[0].start_date, "2026-09-07");
    }

    #[test]
    fn monthly_segments_keep_original_day_after_february_and_dont_invent_history() {
        let mut item = sample();
        item.event_kind = Some("MONTHLY".into());
        item.created_at = "2028-01-05T09:00:00Z".into();
        item.due_at = Some("2028-02-29T18:00:00Z".into());
        item.anchor_day = Some(31);
        let context = Projection::new(
            &Utc,
            "YEAR",
            NaiveDate::from_ymd_opt(2028, 1, 1).unwrap(),
            NaiveDate::from_ymd_opt(2028, 12, 31).unwrap(),
            timestamp("2028-02-15T12:00:00Z").unwrap(),
        )
        .unwrap();
        let entries = project_item(&item, &context).unwrap();
        assert_eq!(entries[0].start_date, "2028-01-05");
        assert_eq!(entries[0].end_date, "2028-02-29");
        assert!(entries.iter().all(|entry| entry.history_incomplete));
        assert_eq!(entries[1].start_date, "2028-02-29");
        assert_eq!(entries[1].end_date, "2028-03-31");
        assert!(!entries.iter().any(|entry| entry.end_date == "2028-01-31"));
    }

    #[test]
    fn yearly_leap_anchor_recovers_and_timezone_is_injected() {
        let mut item = sample();
        item.event_kind = Some("YEARLY".into());
        item.created_at = "2027-01-01T09:00:00Z".into();
        item.due_at = Some("2027-02-28T18:00:00Z".into());
        item.anchor_day = Some(29);
        item.anchor_month = Some(2);
        let context = Projection::new(
            &Utc,
            "YEAR",
            NaiveDate::from_ymd_opt(2028, 1, 1).unwrap(),
            NaiveDate::from_ymd_opt(2028, 12, 31).unwrap(),
            timestamp("2028-01-01T00:00:00Z").unwrap(),
        )
        .unwrap();
        let entries = project_item(&item, &context).unwrap();
        assert_eq!(entries[0].end_date, "2028-02-29");
        let west = chrono::FixedOffset::west_opt(7 * 3600).unwrap();
        let db = Database::in_memory().unwrap();
        let data = project_in_zone(&db, &query("DAY", "2028-01-01"), context.now, &west).unwrap();
        assert_eq!(data.today, "2027-12-31");
        assert!(data.today_position.is_none());
    }

    #[test]
    fn viewing_is_read_only() {
        let db = Database::in_memory().unwrap();
        let before = db.connection.lock().unwrap().total_changes();
        project_in_zone(
            &db,
            &query("MONTH", "2026-09-07"),
            timestamp("2026-09-07T01:00:00Z").unwrap(),
            &Utc,
        )
        .unwrap();
        assert_eq!(db.connection.lock().unwrap().total_changes(), before);
    }

    #[test]
    fn hundred_thousand_items_are_projected_with_a_bounded_payload() {
        let database = Database::in_memory().unwrap();
        {
            let connection = database.connection.lock().expect("database mutex poisoned");
            connection
                .execute_batch(
                    "WITH RECURSIVE sequence(value) AS (
                       VALUES(1)
                       UNION ALL
                       SELECT value + 1 FROM sequence WHERE value < 100000
                     )
                     INSERT INTO items (
                       id, title, status, due_at, due_local_date, due_local_time,
                       due_source, rollover_policy, completion_policy,
                       created_at, updated_at, event_kind, reminder_plan
                     )
                     SELECT
                       printf('load-%06d', value), printf('事项 %06d', value), 'OPEN',
                       '2026-09-15T01:00:00Z', '2026-09-15', '09:00',
                       'EXPLICIT', 'NONE', 'NORMAL',
                       '2026-09-01T00:00:00Z', '2026-09-01T00:00:00Z', 'ORDINARY', 'ONCE'
                     FROM sequence;",
                )
                .unwrap();
        }
        let now = DateTime::parse_from_rfc3339("2026-09-03T08:00:00+08:00")
            .unwrap()
            .with_timezone(&Utc);
        let started = Instant::now();
        let timeline = project_timeline(&database, &query("MONTH", "2026-09-03"), now).unwrap();
        let elapsed = started.elapsed();

        assert_eq!(timeline.total_visible_count, 100_000);
        assert_eq!(timeline.entries.len(), MAX_VISIBLE_ENTRIES);
        assert!(timeline.truncated);
        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "10 万条时间画布投影耗时过长：{:?}",
            elapsed
        );
        eprintln!("10 万条时间画布投影耗时：{elapsed:?}");
    }
}
