use chrono::{
    DateTime, Datelike, Duration, Local, LocalResult, Months, NaiveDate, NaiveDateTime, NaiveTime,
    TimeZone, Utc,
};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use tauri_plugin_global_shortcut::{Modifiers, Shortcut};

use crate::error::{AppError, AppResult};

pub trait Clock: Send + Sync {
    fn now_utc(&self) -> DateTime<Utc>;
}

#[derive(Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_utc(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub default_due_time: String,
    pub repeat_default_times: Vec<String>,
    pub workdays: Vec<u32>,
    pub overtime_interval_minutes: u32,
    pub quiet_hours_enabled: bool,
    pub quiet_start: String,
    pub quiet_end: String,
    pub global_shortcut: String,
    pub notifications_enabled: bool,
    pub autostart_enabled: bool,
    pub persistent_notifications_enabled: bool,
    pub overlay_reminders_enabled: bool,
    pub repeat_unacknowledged_enabled: bool,
    pub unacknowledged_repeat_minutes: u32,
    pub smtp_enabled: bool,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_security: String,
    pub smtp_from: String,
    pub smtp_to: String,
    pub smtp_username: String,
    pub smtp_repeat_must_complete: bool,
    pub event_kind_defaults: Vec<EventKindDefault>,
}

pub const EVENT_KINDS: [&str; 7] = [
    "ORDINARY",
    "ONE_TIME",
    "TODAY_MUST",
    "WARNING",
    "CONTINUOUS",
    "MONTHLY",
    "YEARLY",
];

pub const REMINDER_PLANS: [&str; 5] = ["REPEAT", "EMPHASIS", "ONCE", "FORCE", "CUSTOM"];
pub const REPEAT_TIME_MODES: [&str; 2] = ["DEFAULT", "SPECIFIED"];
pub const SCHEDULE_UNITS: [&str; 4] = ["DAY", "WEEK", "MONTH", "YEAR"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EventKindDefault {
    pub event_kind: String,
    pub reminder_plan: String,
}

#[cfg(test)]
pub fn default_event_kind_defaults() -> Vec<EventKindDefault> {
    [
        ("ORDINARY", "REPEAT"),
        ("ONE_TIME", "ONCE"),
        ("TODAY_MUST", "EMPHASIS"),
        ("WARNING", "REPEAT"),
        ("CONTINUOUS", "CUSTOM"),
        ("MONTHLY", "ONCE"),
        ("YEARLY", "ONCE"),
    ]
    .into_iter()
    .map(|(event_kind, reminder_plan)| EventKindDefault {
        event_kind: event_kind.into(),
        reminder_plan: reminder_plan.into(),
    })
    .collect()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSettingsInput {
    pub default_due_time: String,
    pub repeat_default_times: Vec<String>,
    pub workdays: Vec<u32>,
    pub overtime_interval_minutes: u32,
    pub quiet_hours_enabled: bool,
    pub quiet_start: String,
    pub quiet_end: String,
    pub global_shortcut: String,
    pub notifications_enabled: bool,
    pub autostart_enabled: bool,
    pub update_existing_default_items: bool,
    pub persistent_notifications_enabled: bool,
    pub overlay_reminders_enabled: bool,
    pub unacknowledged_repeat_minutes: u32,
    pub smtp_enabled: bool,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_security: String,
    pub smtp_from: String,
    pub smtp_to: String,
    pub smtp_username: String,
    pub smtp_repeat_must_complete: bool,
    pub smtp_password: Option<String>,
    pub event_kind_defaults: Vec<EventKindDefault>,
}

impl UpdateSettingsInput {
    pub fn validate(&self) -> AppResult<()> {
        parse_time(&self.default_due_time)?;
        validate_repeat_times(&self.repeat_default_times, 2, 2)?;
        parse_time(&self.quiet_start)?;
        parse_time(&self.quiet_end)?;

        if self.workdays.is_empty() || self.workdays.iter().any(|day| !(1..=7).contains(day)) {
            return Err(AppError::Validation("至少选择一个有效工作日".into()));
        }
        if !(5..=240).contains(&self.overtime_interval_minutes) {
            return Err(AppError::Validation(
                "今日必做提醒间隔必须在 5–240 分钟之间".into(),
            ));
        }
        if !matches!(self.smtp_security.as_str(), "tls" | "starttls") {
            return Err(AppError::Validation("请选择安全的邮件加密方式".into()));
        }
        if self.smtp_enabled {
            if self.smtp_host.trim().is_empty() {
                return Err(AppError::Validation("请填写 SMTP 服务器地址".into()));
            }
            if !self.smtp_from.contains('@') || !self.smtp_to.contains('@') {
                return Err(AppError::Validation(
                    "请填写有效的发件人和收件人邮箱".into(),
                ));
            }
        }
        validate_global_shortcut(&self.global_shortcut)?;
        validate_event_kind_defaults(&self.event_kind_defaults)?;
        Ok(())
    }

    pub fn settings(&self) -> Settings {
        let mut workdays = self.workdays.clone();
        workdays.sort_unstable();
        workdays.dedup();
        let mut repeat_default_times = self.repeat_default_times.clone();
        repeat_default_times.sort();
        Settings {
            default_due_time: self.default_due_time.clone(),
            repeat_default_times,
            workdays,
            overtime_interval_minutes: self.overtime_interval_minutes,
            quiet_hours_enabled: self.quiet_hours_enabled,
            quiet_start: self.quiet_start.clone(),
            quiet_end: self.quiet_end.clone(),
            global_shortcut: self.global_shortcut.trim().to_string(),
            notifications_enabled: self.notifications_enabled,
            autostart_enabled: self.autostart_enabled,
            persistent_notifications_enabled: self.persistent_notifications_enabled,
            overlay_reminders_enabled: self.overlay_reminders_enabled,
            // 字段保留用于读取旧数据库，但新版只按事项提醒方案计算频率。
            repeat_unacknowledged_enabled: false,
            unacknowledged_repeat_minutes: self.unacknowledged_repeat_minutes,
            smtp_enabled: self.smtp_enabled,
            smtp_host: self.smtp_host.trim().to_string(),
            smtp_port: self.smtp_port,
            smtp_security: self.smtp_security.clone(),
            smtp_from: self.smtp_from.trim().to_string(),
            smtp_to: self.smtp_to.trim().to_string(),
            smtp_username: self.smtp_username.trim().to_string(),
            smtp_repeat_must_complete: self.smtp_repeat_must_complete,
            event_kind_defaults: self.event_kind_defaults.clone(),
        }
    }
}

fn validate_event_kind_defaults(defaults: &[EventKindDefault]) -> AppResult<()> {
    if defaults.len() != EVENT_KINDS.len() {
        return Err(AppError::Validation(
            "请为七种事件分别选择默认提醒方案".into(),
        ));
    }
    let mut kinds = defaults
        .iter()
        .map(|entry| entry.event_kind.as_str())
        .collect::<Vec<_>>();
    kinds.sort_unstable();
    kinds.dedup();
    if kinds.len() != EVENT_KINDS.len()
        || kinds.iter().any(|kind| !EVENT_KINDS.contains(kind))
        || defaults
            .iter()
            .any(|entry| !REMINDER_PLANS.contains(&entry.reminder_plan.as_str()))
    {
        return Err(AppError::Validation(
            "事件默认提醒方案不完整，请重新选择".into(),
        ));
    }
    Ok(())
}

pub fn validate_global_shortcut(value: &str) -> AppResult<()> {
    let value = value.trim();
    if value.is_empty() {
        return Err(AppError::Validation(
            "快捷键不能为空，请输入类似 Ctrl+Shift+Space 的组合。".into(),
        ));
    }

    let modifier_names = [
        "alt",
        "option",
        "ctrl",
        "control",
        "shift",
        "command",
        "cmd",
        "super",
        "meta",
        "commandorcontrol",
    ];
    let tokens = value.split('+').map(str::trim).collect::<Vec<_>>();
    if !tokens.is_empty()
        && tokens
            .iter()
            .all(|token| modifier_names.contains(&token.to_ascii_lowercase().as_str()))
    {
        return Err(AppError::Validation(
            "快捷键必须包含一个普通按键，例如 Space、M 或 F8。".into(),
        ));
    }

    let shortcut = Shortcut::from_str(value).map_err(|_| {
        AppError::Validation("快捷键格式无效，请输入类似 Ctrl+Shift+Space 的组合。".into())
    })?;
    if shortcut.mods == Modifiers::empty() {
        return Err(AppError::Validation(
            "快捷键必须同时包含修饰键和一个普通按键。".into(),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: String,
    pub title: String,
    pub notes: String,
    pub status: String,
    pub category_id: Option<String>,
    pub category_name: Option<String>,
    pub due_at: String,
    pub due_local_date: String,
    pub due_local_time: String,
    pub due_source: String,
    pub rollover_policy: String,
    pub rollover_count: u32,
    pub completion_policy: String,
    pub repeat_interval_minutes: Option<u32>,
    pub next_reminder_at: Option<String>,
    pub reminder_paused: bool,
    pub bypass_app_quiet_hours: bool,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
    pub deleted_at: Option<String>,
    pub event_kind: Option<String>,
    pub reminder_plan: String,
    pub important: bool,
    pub time_mode: String,
    pub start_at: Option<String>,
    pub end_at: Option<String>,
    pub target_at: Option<String>,
    pub lead_value: Option<u32>,
    pub lead_unit: Option<String>,
    pub cadence_value: Option<u32>,
    pub cadence_unit: Option<String>,
    pub emphasis_max_per_day: u32,
    pub repeat_time_mode: String,
    pub repeat_times: Vec<String>,
    pub tags: Vec<Tag>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EventConfigurationInput {
    pub kind: Option<String>,
    pub reminder_plan: Option<String>,
    #[serde(default)]
    pub important: bool,
    pub start_at: Option<String>,
    pub end_at: Option<String>,
    pub target_at: Option<String>,
    pub lead_value: Option<u32>,
    pub lead_unit: Option<String>,
    pub cadence_value: Option<u32>,
    pub cadence_unit: Option<String>,
    pub emphasis_max_per_day: Option<u32>,
    pub repeat_time_mode: Option<String>,
    #[serde(default)]
    pub repeat_times: Vec<String>,
}

impl EventConfigurationInput {
    pub fn validate(&self) -> AppResult<()> {
        if let Some(kind) = &self.kind
            && !EVENT_KINDS.contains(&kind.as_str())
        {
            return Err(AppError::Validation("请选择有效的事件类型".into()));
        }
        if let Some(plan) = &self.reminder_plan
            && !REMINDER_PLANS.contains(&plan.as_str())
        {
            return Err(AppError::Validation("请选择有效的提醒方案".into()));
        }
        if let Some(mode) = &self.repeat_time_mode
            && !REPEAT_TIME_MODES.contains(&mode.as_str())
        {
            return Err(AppError::Validation("请选择有效的重复时间方式".into()));
        }
        if !self.repeat_times.is_empty() {
            validate_repeat_times(&self.repeat_times, 1, 2)?;
        }
        if self.reminder_plan.as_deref() == Some("REPEAT") {
            match self.repeat_time_mode.as_deref() {
                Some("DEFAULT")
                    if !self.repeat_times.is_empty() && self.repeat_times.len() != 2 =>
                {
                    return Err(AppError::Validation("默认重复提醒需要两个时间".into()));
                }
                Some("SPECIFIED") if self.repeat_times.len() > 1 => {
                    return Err(AppError::Validation("指定重复提醒只能设置一个时间".into()));
                }
                _ => {}
            }
        }
        if self.lead_value == Some(0) || self.cadence_value == Some(0) {
            return Err(AppError::Validation("提醒周期必须大于 0".into()));
        }
        if self
            .emphasis_max_per_day
            .is_some_and(|value| !(1..=96).contains(&value))
        {
            return Err(AppError::Validation(
                "每日强调提醒次数必须在 1–96 次之间".into(),
            ));
        }
        if let Some(unit) = &self.lead_unit
            && !SCHEDULE_UNITS.contains(&unit.as_str())
        {
            return Err(AppError::Validation("请选择有效的预警单位".into()));
        }
        if let Some(unit) = &self.cadence_unit
            && !SCHEDULE_UNITS.contains(&unit.as_str())
        {
            return Err(AppError::Validation("请选择有效的提醒周期单位".into()));
        }

        let start = parse_optional_datetime(self.start_at.as_deref())?;
        let end = parse_optional_datetime(self.end_at.as_deref())?;
        parse_optional_datetime(self.target_at.as_deref())?;
        match self.kind.as_deref() {
            Some("WARNING")
                if self.target_at.is_none()
                    || self.lead_value.is_none()
                    || self.lead_unit.is_none() =>
            {
                return Err(AppError::Validation("预警事件需要目标时间和提前量".into()));
            }
            Some("CONTINUOUS")
                if start.is_none()
                    || end.is_none()
                    || self.cadence_value.is_none()
                    || self.cadence_unit.is_none() =>
            {
                return Err(AppError::Validation(
                    "持续事件需要开始时间、结束时间和提醒周期".into(),
                ));
            }
            _ => {}
        }
        if self.reminder_plan.as_deref() == Some("CUSTOM")
            && (self.cadence_value.is_none() || self.cadence_unit.is_none())
        {
            return Err(AppError::Validation("自定义提醒需要提醒周期".into()));
        }
        if let (Some(start), Some(end)) = (start, end)
            && start > end
        {
            return Err(AppError::Validation("结束时间不能早于开始时间".into()));
        }
        Ok(())
    }
}

fn parse_optional_datetime(value: Option<&str>) -> AppResult<Option<DateTime<Utc>>> {
    value
        .map(|value| {
            DateTime::parse_from_rfc3339(value)
                .map(|value| value.with_timezone(&Utc))
                .map_err(|_| AppError::Validation("事件时间格式无效".into()))
        })
        .transpose()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateItemInput {
    pub title: String,
    #[serde(default)]
    pub notes: String,
    pub category_id: Option<String>,
    pub due_at: Option<String>,
    pub must_complete_today: bool,
    pub repeat_interval_minutes: Option<u32>,
    #[serde(default)]
    pub tag_ids: Vec<String>,
    #[serde(default)]
    pub event: Option<EventConfigurationInput>,
}

impl CreateItemInput {
    pub fn validate(&self) -> AppResult<()> {
        let title = self.title.trim();
        if title.is_empty() {
            return Err(AppError::Validation("事项内容不能为空".into()));
        }
        if title.chars().count() > 4000 {
            return Err(AppError::Validation("事项内容不能超过 4000 个字符".into()));
        }
        if let Some(interval) = self.repeat_interval_minutes
            && !(5..=240).contains(&interval)
        {
            return Err(AppError::Validation("提醒间隔必须在 5–240 分钟之间".into()));
        }
        if let Some(due_at) = &self.due_at {
            DateTime::parse_from_rfc3339(due_at)
                .map_err(|_| AppError::Validation("指定时间格式无效".into()))?;
        }
        if let Some(event) = &self.event {
            event.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub id: String,
    pub name: String,
    pub color: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    pub id: String,
    pub name: String,
    pub color: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaxonomyInput {
    pub name: String,
    pub color: String,
}

impl TaxonomyInput {
    pub fn validate(&self, label: &str) -> AppResult<()> {
        let length = self.name.trim().chars().count();
        if !(1..=30).contains(&length) {
            return Err(AppError::Validation(format!("{label}名称需要 1–30 个字符")));
        }
        if !is_hex_color(&self.color) {
            return Err(AppError::Validation(format!("请选择有效的{label}颜色")));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateItemInput {
    pub title: String,
    #[serde(default)]
    pub notes: String,
    pub category_id: Option<String>,
    #[serde(default)]
    pub tag_ids: Vec<String>,
    pub due_at: String,
    pub must_complete_today: bool,
    pub repeat_interval_minutes: Option<u32>,
    #[serde(default)]
    pub event: Option<EventConfigurationInput>,
}

impl UpdateItemInput {
    pub fn validate(&self) -> AppResult<()> {
        CreateItemInput {
            title: self.title.clone(),
            notes: self.notes.clone(),
            category_id: self.category_id.clone(),
            due_at: Some(self.due_at.clone()),
            must_complete_today: self.must_complete_today,
            repeat_interval_minutes: self.repeat_interval_minutes,
            tag_ids: self.tag_ids.clone(),
            event: self.event.clone(),
        }
        .validate()
    }
}

fn is_hex_color(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value[1..]
            .chars()
            .all(|character| character.is_ascii_hexdigit())
}

#[derive(Debug, Clone)]
pub struct DueMoment {
    pub utc: DateTime<Utc>,
    pub local_date: NaiveDate,
    pub local_time: NaiveTime,
}

pub fn parse_time(value: &str) -> AppResult<NaiveTime> {
    NaiveTime::parse_from_str(value, "%H:%M")
        .map_err(|_| AppError::Validation(format!("时间格式无效：{value}")))
}

pub fn validate_repeat_times(values: &[String], minimum: usize, maximum: usize) -> AppResult<()> {
    if values.len() < minimum || values.len() > maximum {
        return Err(AppError::Validation(if minimum == maximum {
            "请设置两个重复提醒时间".into()
        } else {
            "重复提醒只能设置一至两个时间".into()
        }));
    }
    for value in values {
        parse_time(value)?;
    }
    let mut unique = values.to_vec();
    unique.sort();
    unique.dedup();
    if unique.len() != values.len() {
        return Err(AppError::Validation("两个重复提醒时间不能相同".into()));
    }
    Ok(())
}

pub fn next_repeat_slot_after(
    now_utc: DateTime<Utc>,
    values: &[String],
) -> AppResult<DateTime<Utc>> {
    validate_repeat_times(values, 1, 2)?;
    let local_now = now_utc.with_timezone(&Local);
    let mut times = values
        .iter()
        .map(|value| parse_time(value))
        .collect::<AppResult<Vec<_>>>()?;
    times.sort_unstable();
    for time in &times {
        let candidate =
            resolve_local_datetime(local_now.date_naive().and_time(*time))?.with_timezone(&Utc);
        if candidate > now_utc {
            return Ok(candidate);
        }
    }
    let tomorrow = local_now.date_naive() + Duration::days(1);
    Ok(resolve_local_datetime(tomorrow.and_time(times[0]))?.with_timezone(&Utc))
}

pub fn next_default_due(now_utc: DateTime<Utc>, settings: &Settings) -> AppResult<DueMoment> {
    let now = now_utc.with_timezone(&Local);
    let target_time = parse_time(&settings.default_due_time)?;
    let mut date = now.date_naive();
    let today_is_workday = settings
        .workdays
        .contains(&date.weekday().number_from_monday());

    if !today_is_workday || now.time() >= target_time {
        date = next_workday(date, &settings.workdays)?;
    }

    let local = resolve_local_datetime(date.and_time(target_time))?;
    Ok(DueMoment {
        utc: local.with_timezone(&Utc),
        local_date: date,
        local_time: target_time,
    })
}

pub fn must_complete_due(now_utc: DateTime<Utc>, settings: &Settings) -> AppResult<DueMoment> {
    let now = now_utc.with_timezone(&Local);
    let target_time = parse_time(&settings.default_due_time)?;
    let target = due_for_local_date(now.date_naive(), target_time)?;

    if target.utc > now_utc {
        return Ok(target);
    }

    Ok(DueMoment {
        utc: now_utc,
        local_date: now.date_naive(),
        local_time: now.time(),
    })
}

pub fn due_from_explicit(value: &str) -> AppResult<DueMoment> {
    let fixed = DateTime::parse_from_rfc3339(value)
        .map_err(|_| AppError::Validation("指定时间格式无效".into()))?;
    let utc = fixed.with_timezone(&Utc);
    let local = utc.with_timezone(&Local);
    Ok(DueMoment {
        utc,
        local_date: local.date_naive(),
        local_time: local.time(),
    })
}

pub fn due_for_local_date(date: NaiveDate, time: NaiveTime) -> AppResult<DueMoment> {
    let local = resolve_local_datetime(date.and_time(time))?;
    Ok(DueMoment {
        utc: local.with_timezone(&Utc),
        local_date: date,
        local_time: time,
    })
}

pub fn next_repeat_at(
    now_utc: DateTime<Utc>,
    interval_minutes: u32,
    settings: &Settings,
    bypass_quiet_hours: bool,
) -> AppResult<DateTime<Utc>> {
    let candidate = now_utc + Duration::minutes(i64::from(interval_minutes));
    if !settings.quiet_hours_enabled || bypass_quiet_hours {
        return Ok(candidate);
    }

    let local = candidate.with_timezone(&Local);
    let start = parse_time(&settings.quiet_start)?;
    let end = parse_time(&settings.quiet_end)?;
    if !time_is_quiet(local.time(), start, end) {
        return Ok(candidate);
    }

    let end_date = if start < end || local.time() < end {
        local.date_naive()
    } else {
        local.date_naive() + Duration::days(1)
    };
    Ok(resolve_local_datetime(end_date.and_time(end))?.with_timezone(&Utc))
}

pub fn next_daily_at(now_utc: DateTime<Utc>, time: NaiveTime) -> AppResult<DateTime<Utc>> {
    let local = now_utc.with_timezone(&Local);
    let mut date = local.date_naive();
    let today = resolve_local_datetime(date.and_time(time))?.with_timezone(&Utc);
    if today <= now_utc {
        date += Duration::days(1);
    }
    Ok(resolve_local_datetime(date.and_time(time))?.with_timezone(&Utc))
}

pub fn shift_calendar(
    value: DateTime<Utc>,
    amount: u32,
    unit: &str,
    forward: bool,
) -> AppResult<DateTime<Utc>> {
    if amount == 0 || !SCHEDULE_UNITS.contains(&unit) {
        return Err(AppError::Validation("提醒周期无效".into()));
    }
    let local = value.with_timezone(&Local);
    let date = local.date_naive();
    let shifted_date = match (unit, forward) {
        ("DAY", true) => date.checked_add_signed(Duration::days(i64::from(amount))),
        ("DAY", false) => date.checked_sub_signed(Duration::days(i64::from(amount))),
        ("WEEK", true) => date.checked_add_signed(Duration::weeks(i64::from(amount))),
        ("WEEK", false) => date.checked_sub_signed(Duration::weeks(i64::from(amount))),
        ("MONTH", true) => date.checked_add_months(Months::new(amount)),
        ("MONTH", false) => date.checked_sub_months(Months::new(amount)),
        ("YEAR", true) => date.checked_add_months(Months::new(amount.saturating_mul(12))),
        ("YEAR", false) => date.checked_sub_months(Months::new(amount.saturating_mul(12))),
        _ => None,
    }
    .ok_or_else(|| AppError::Validation("无法计算下一次日历时间".into()))?;
    Ok(resolve_local_datetime(shifted_date.and_time(local.time()))?.with_timezone(&Utc))
}

fn time_is_quiet(value: NaiveTime, start: NaiveTime, end: NaiveTime) -> bool {
    if start == end {
        return false;
    }
    if start < end {
        value >= start && value < end
    } else {
        value >= start || value < end
    }
}

fn next_workday(from: NaiveDate, workdays: &[u32]) -> AppResult<NaiveDate> {
    for offset in 1..=14 {
        let date = from + Duration::days(offset);
        if workdays.contains(&date.weekday().number_from_monday()) {
            return Ok(date);
        }
    }
    Err(AppError::Validation("无法计算下一工作日".into()))
}

fn resolve_local_datetime(value: NaiveDateTime) -> AppResult<DateTime<Local>> {
    match Local.from_local_datetime(&value) {
        LocalResult::Single(value) => Ok(value),
        LocalResult::Ambiguous(earlier, _) => Ok(earlier),
        LocalResult::None => {
            for minutes in 1..=180 {
                let adjusted = value + Duration::minutes(minutes);
                if let LocalResult::Single(value) = Local.from_local_datetime(&adjusted) {
                    return Ok(value);
                }
            }
            Err(AppError::Validation(
                "本地时间落在无效的时区切换区间".into(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn settings() -> Settings {
        Settings {
            default_due_time: "18:00".into(),
            repeat_default_times: vec!["10:00".into(), "17:00".into()],
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
            event_kind_defaults: default_event_kind_defaults(),
        }
    }

    #[test]
    fn before_end_of_day_uses_same_workday() {
        let local = Local.with_ymd_and_hms(2026, 8, 27, 17, 0, 0).unwrap();
        let due = next_default_due(local.with_timezone(&Utc), &settings()).unwrap();
        assert_eq!(
            due.local_date,
            NaiveDate::from_ymd_opt(2026, 8, 27).unwrap()
        );
        assert_eq!(due.local_time, NaiveTime::from_hms_opt(18, 0, 0).unwrap());
    }

    #[test]
    fn after_end_of_day_uses_next_workday() {
        let local = Local.with_ymd_and_hms(2026, 8, 28, 19, 0, 0).unwrap();
        let due = next_default_due(local.with_timezone(&Utc), &settings()).unwrap();
        assert_eq!(
            due.local_date,
            NaiveDate::from_ymd_opt(2026, 8, 31).unwrap()
        );
    }

    #[test]
    fn must_complete_after_end_of_day_is_due_immediately_today() {
        let local = Local.with_ymd_and_hms(2026, 8, 27, 22, 11, 0).unwrap();
        let due = must_complete_due(local.with_timezone(&Utc), &settings()).unwrap();
        assert_eq!(due.local_date, local.date_naive());
        assert_eq!(due.local_time, NaiveTime::from_hms_opt(22, 11, 0).unwrap());
        assert_eq!(due.utc, local.with_timezone(&Utc));
    }

    #[test]
    fn must_complete_before_end_of_day_uses_today_even_on_non_workday() {
        let local = Local.with_ymd_and_hms(2026, 8, 29, 10, 0, 0).unwrap();
        let due = must_complete_due(local.with_timezone(&Utc), &settings()).unwrap();
        assert_eq!(due.local_date, local.date_naive());
        assert_eq!(due.local_time, NaiveTime::from_hms_opt(18, 0, 0).unwrap());
    }

    #[test]
    fn repeat_waits_until_quiet_hours_end() {
        let local = Local.with_ymd_and_hms(2026, 8, 27, 22, 15, 0).unwrap();
        let next = next_repeat_at(local.with_timezone(&Utc), 30, &settings(), false).unwrap();
        let next_local = next.with_timezone(&Local);
        assert_eq!(
            next_local.date_naive(),
            NaiveDate::from_ymd_opt(2026, 8, 28).unwrap()
        );
        assert_eq!(
            next_local.time(),
            NaiveTime::from_hms_opt(7, 30, 0).unwrap()
        );
    }

    #[test]
    fn repeat_slots_choose_each_remaining_time_without_catching_up() {
        let values = vec!["10:00".into(), "17:00".into()];
        let before_first = Local.with_ymd_and_hms(2026, 9, 1, 9, 0, 0).unwrap();
        let between = Local.with_ymd_and_hms(2026, 9, 1, 12, 0, 0).unwrap();
        let after_second = Local.with_ymd_and_hms(2026, 9, 1, 22, 0, 0).unwrap();

        assert_eq!(
            next_repeat_slot_after(before_first.with_timezone(&Utc), &values)
                .unwrap()
                .with_timezone(&Local),
            Local.with_ymd_and_hms(2026, 9, 1, 10, 0, 0).unwrap()
        );
        assert_eq!(
            next_repeat_slot_after(between.with_timezone(&Utc), &values)
                .unwrap()
                .with_timezone(&Local),
            Local.with_ymd_and_hms(2026, 9, 1, 17, 0, 0).unwrap()
        );
        assert_eq!(
            next_repeat_slot_after(after_second.with_timezone(&Utc), &values)
                .unwrap()
                .with_timezone(&Local),
            Local.with_ymd_and_hms(2026, 9, 2, 10, 0, 0).unwrap()
        );
    }

    #[test]
    fn monthly_calendar_shift_uses_the_last_valid_day() {
        let january = Local
            .with_ymd_and_hms(2026, 1, 31, 9, 30, 0)
            .unwrap()
            .with_timezone(&Utc);
        let february = shift_calendar(january, 1, "MONTH", true)
            .unwrap()
            .with_timezone(&Local);
        assert_eq!(
            february.date_naive(),
            NaiveDate::from_ymd_opt(2026, 2, 28).unwrap()
        );
        assert_eq!(february.time(), NaiveTime::from_hms_opt(9, 30, 0).unwrap());
    }

    #[test]
    fn yearly_calendar_shift_clamps_leap_day_in_non_leap_year() {
        let leap_day = Local
            .with_ymd_and_hms(2028, 2, 29, 15, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        let next_year = shift_calendar(leap_day, 1, "YEAR", true)
            .unwrap()
            .with_timezone(&Local);
        assert_eq!(
            next_year.date_naive(),
            NaiveDate::from_ymd_opt(2029, 2, 28).unwrap()
        );
    }

    #[test]
    fn shortcut_validation_returns_actionable_errors() {
        assert_eq!(
            validate_global_shortcut("").unwrap_err().to_string(),
            "快捷键不能为空，请输入类似 Ctrl+Shift+Space 的组合。"
        );
        assert_eq!(
            validate_global_shortcut("Ctrl+Shift")
                .unwrap_err()
                .to_string(),
            "快捷键必须包含一个普通按键，例如 Space、M 或 F8。"
        );
        assert!(validate_global_shortcut("Ctrl+Shift+Space").is_ok());
    }
}
