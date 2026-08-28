use chrono::{
    DateTime, Datelike, Duration, Local, LocalResult, NaiveDate, NaiveDateTime, NaiveTime,
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
    pub workdays: Vec<u32>,
    pub overtime_interval_minutes: u32,
    pub quiet_hours_enabled: bool,
    pub quiet_start: String,
    pub quiet_end: String,
    pub global_shortcut: String,
    pub notifications_enabled: bool,
    pub autostart_enabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSettingsInput {
    pub default_due_time: String,
    pub workdays: Vec<u32>,
    pub overtime_interval_minutes: u32,
    pub quiet_hours_enabled: bool,
    pub quiet_start: String,
    pub quiet_end: String,
    pub global_shortcut: String,
    pub notifications_enabled: bool,
    pub autostart_enabled: bool,
    pub update_existing_default_items: bool,
}

impl UpdateSettingsInput {
    pub fn validate(&self) -> AppResult<()> {
        parse_time(&self.default_due_time)?;
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
        validate_global_shortcut(&self.global_shortcut)?;
        Ok(())
    }

    pub fn settings(&self) -> Settings {
        let mut workdays = self.workdays.clone();
        workdays.sort_unstable();
        workdays.dedup();
        Settings {
            default_due_time: self.default_due_time.clone(),
            workdays,
            overtime_interval_minutes: self.overtime_interval_minutes,
            quiet_hours_enabled: self.quiet_hours_enabled,
            quiet_start: self.quiet_start.clone(),
            quiet_end: self.quiet_end.clone(),
            global_shortcut: self.global_shortcut.trim().to_string(),
            notifications_enabled: self.notifications_enabled,
            autostart_enabled: self.autostart_enabled,
        }
    }
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
    pub tags: Vec<Tag>,
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
            workdays: vec![1, 2, 3, 4, 5],
            overtime_interval_minutes: 30,
            quiet_hours_enabled: true,
            quiet_start: "22:30".into(),
            quiet_end: "07:30".into(),
            global_shortcut: "CommandOrControl+Shift+Space".into(),
            notifications_enabled: true,
            autostart_enabled: false,
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
