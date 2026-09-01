use std::{collections::BTreeMap, fs, path::Path};

use chrono::{DateTime, Local, Utc};
use rust_xlsxwriter::{Color, Format, Workbook, XlsxError};
use serde::Deserialize;

use crate::{
    domain::Item,
    error::{AppError, AppResult},
};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportFilterInput {
    pub status: String,
    pub event_kind: Option<String>,
    pub category_id: Option<String>,
    pub tag_id: Option<String>,
    pub created_from: Option<String>,
    pub created_to: Option<String>,
}

pub fn filter_items(items: Vec<Item>, filter: &ExportFilterInput) -> AppResult<Vec<Item>> {
    if !matches!(filter.status.as_str(), "all" | "open" | "done") {
        return Err(AppError::Validation("导出状态筛选无效".into()));
    }
    let created_from = parse_boundary(filter.created_from.as_deref())?;
    let created_to = parse_boundary(filter.created_to.as_deref())?;
    if created_from
        .zip(created_to)
        .is_some_and(|(from, to)| from > to)
    {
        return Err(AppError::Validation("导出开始日期不能晚于结束日期".into()));
    }

    Ok(items
        .into_iter()
        .filter(|item| {
            let status_matches = match filter.status.as_str() {
                "open" => item.status == "OPEN",
                "done" => item.status == "DONE",
                _ => true,
            };
            let event_kind_matches = filter
                .event_kind
                .as_deref()
                .is_none_or(|kind| item.event_kind.as_deref() == Some(kind));
            let category_matches = filter
                .category_id
                .as_deref()
                .is_none_or(|id| item.category_id.as_deref() == Some(id));
            let tag_matches = filter
                .tag_id
                .as_deref()
                .is_none_or(|id| item.tags.iter().any(|tag| tag.id == id));
            let created = DateTime::parse_from_rfc3339(&item.created_at)
                .map(|value| value.with_timezone(&Utc))
                .ok();
            let after_start =
                created_from.is_none_or(|from| created.is_some_and(|value| value >= from));
            let before_end = created_to.is_none_or(|to| created.is_some_and(|value| value <= to));
            status_matches
                && event_kind_matches
                && category_matches
                && tag_matches
                && after_start
                && before_end
        })
        .collect())
}

fn parse_boundary(value: Option<&str>) -> AppResult<Option<DateTime<Utc>>> {
    value
        .map(|value| {
            DateTime::parse_from_rfc3339(value)
                .map(|date| date.with_timezone(&Utc))
                .map_err(|_| AppError::Validation("导出日期筛选无效".into()))
        })
        .transpose()
}

pub fn export_items(path: &Path, items: &[Item]) -> AppResult<()> {
    if path.extension().and_then(|value| value.to_str()) != Some("xlsx") {
        return Err(AppError::Validation("导出文件需要使用 .xlsx 扩展名".into()));
    }
    let parent = path
        .parent()
        .ok_or_else(|| AppError::Validation("导出位置无效".into()))?;
    if !parent.is_dir() {
        return Err(AppError::Validation(
            "导出文件夹不存在，请重新选择位置".into(),
        ));
    }

    let temporary = path.with_extension("xlsx.tmp");
    if temporary.exists() {
        fs::remove_file(&temporary)?;
    }
    let result = write_workbook(&temporary, items);
    if let Err(error) = result {
        let _ = fs::remove_file(&temporary);
        return Err(AppError::SystemIntegration(format!(
            "Excel 报表生成失败：{error}"
        )));
    }
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(temporary, path)?;
    Ok(())
}

fn write_workbook(path: &Path, items: &[Item]) -> Result<(), XlsxError> {
    let mut workbook = Workbook::new();
    let header = Format::new()
        .set_bold()
        .set_font_color(Color::White)
        .set_background_color(Color::RGB(0x243442));

    let detail = workbook.add_worksheet();
    detail.set_name("事项明细")?;
    detail.set_freeze_panes(1, 0)?;
    let headers = [
        "事项内容",
        "备注",
        "状态",
        "事件类型",
        "类型",
        "重要",
        "提醒方案",
        "标签",
        "创建时间（本地）",
        "当前到期时间（本地）",
        "开始时间（本地）",
        "结束时间（本地）",
        "预警目标（本地）",
        "完成时间（本地）",
        "顺延次数",
        "提醒状态",
    ];
    for (column, value) in headers.iter().enumerate() {
        detail.write_string_with_format(0, column as u16, *value, &header)?;
    }
    for (column, width) in [
        35.0, 30.0, 10.0, 12.0, 12.0, 9.0, 12.0, 22.0, 20.0, 20.0, 20.0, 20.0, 20.0, 20.0, 10.0,
        16.0,
    ]
    .into_iter()
    .enumerate()
    {
        detail.set_column_width(column as u16, width)?;
    }

    for (index, item) in items.iter().enumerate() {
        let row = index as u32 + 1;
        let values = [
            excel_safe_text(&item.title),
            excel_safe_text(&item.notes),
            status_label(&item.status).to_string(),
            event_kind_label(item.event_kind.as_deref()).to_string(),
            item.category_name
                .clone()
                .unwrap_or_else(|| "未分类".into()),
            if item.important { "是" } else { "否" }.into(),
            reminder_plan_label(&item.reminder_plan).to_string(),
            item.tags
                .iter()
                .map(|tag| tag.name.as_str())
                .collect::<Vec<_>>()
                .join("、"),
            local_datetime(&item.created_at),
            format!("{} {}", item.due_local_date, item.due_local_time),
            item.start_at
                .as_deref()
                .map(local_datetime)
                .unwrap_or_default(),
            item.end_at
                .as_deref()
                .map(local_datetime)
                .unwrap_or_default(),
            item.target_at
                .as_deref()
                .map(local_datetime)
                .unwrap_or_default(),
            item.completed_at
                .as_deref()
                .map(local_datetime)
                .unwrap_or_default(),
            item.rollover_count.to_string(),
            reminder_label(item).to_string(),
        ];
        for (column, value) in values.iter().enumerate() {
            detail.write_string(row, column as u16, value)?;
        }
    }
    detail.autofilter(0, 0, items.len() as u32, 15)?;

    let summary = workbook.add_worksheet();
    summary.set_name("事件类型汇总")?;
    for (column, value) in ["事件类型", "事项数", "待处理", "已完成", "今日必做"]
        .iter()
        .enumerate()
    {
        summary.write_string_with_format(0, column as u16, *value, &header)?;
    }
    let mut counts: BTreeMap<String, [u32; 4]> = BTreeMap::new();
    for item in items {
        let key = event_kind_label(item.event_kind.as_deref()).to_string();
        let entry = counts.entry(key).or_default();
        entry[0] += 1;
        if item.status == "OPEN" {
            entry[1] += 1;
        }
        if item.status == "DONE" {
            entry[2] += 1;
        }
        if item.completion_policy == "MUST_COMPLETE_TODAY" {
            entry[3] += 1;
        }
    }
    for (index, (name, values)) in counts.into_iter().enumerate() {
        let row = index as u32 + 1;
        summary.write_string(row, 0, excel_safe_text(&name))?;
        for (column, value) in values.into_iter().enumerate() {
            summary.write_number(row, column as u16 + 1, f64::from(value))?;
        }
    }
    summary.set_column_width(0, 24)?;
    for column in 1..=4 {
        summary.set_column_width(column, 12)?;
    }

    let category_summary = workbook.add_worksheet();
    category_summary.set_name("类型汇总")?;
    for (column, value) in ["类型", "事项数", "待处理", "已完成"].iter().enumerate() {
        category_summary.write_string_with_format(0, column as u16, *value, &header)?;
    }
    let mut category_counts: BTreeMap<String, [u32; 3]> = BTreeMap::new();
    for item in items {
        let key = item
            .category_name
            .clone()
            .unwrap_or_else(|| "未分类".into());
        let entry = category_counts.entry(key).or_default();
        entry[0] += 1;
        if item.status == "OPEN" {
            entry[1] += 1;
        }
        if item.status == "DONE" {
            entry[2] += 1;
        }
    }
    for (index, (name, values)) in category_counts.into_iter().enumerate() {
        let row = index as u32 + 1;
        category_summary.write_string(row, 0, excel_safe_text(&name))?;
        for (column, value) in values.into_iter().enumerate() {
            category_summary.write_number(row, column as u16 + 1, f64::from(value))?;
        }
    }
    category_summary.set_column_width(0, 24)?;
    for column in 1..=3 {
        category_summary.set_column_width(column, 12)?;
    }

    workbook.save(path)
}

fn excel_safe_text(value: &str) -> String {
    let first = value.trim_start().chars().next();
    if matches!(first, Some('=' | '+' | '-' | '@')) {
        format!("'{value}")
    } else {
        value.to_string()
    }
}

fn local_datetime(value: &str) -> String {
    DateTime::parse_from_rfc3339(value)
        .map(|date| {
            date.with_timezone(&Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|_| value.to_string())
}

fn status_label(status: &str) -> &str {
    match status {
        "OPEN" => "待处理",
        "DONE" => "已完成",
        "ARCHIVED" => "已归档",
        "DELETED" => "回收站",
        _ => "未知",
    }
}

fn event_kind_label(kind: Option<&str>) -> &str {
    match kind {
        Some("ORDINARY") => "普通",
        Some("ONE_TIME") => "一次性",
        Some("TODAY_MUST") => "今日必做",
        Some("WARNING") => "预警",
        Some("CONTINUOUS") => "持续",
        Some("MONTHLY") => "月度",
        Some("YEARLY") => "年度",
        Some(_) => "未知类型",
        None => "待选择事件类型",
    }
}

fn reminder_plan_label(plan: &str) -> &str {
    match plan {
        "REPEAT" => "重复",
        "EMPHASIS" => "强调",
        "ONCE" => "一次性",
        "FORCE" => "强制",
        "CUSTOM" => "自定义",
        _ => "未知方案",
    }
}

fn reminder_label(item: &Item) -> &str {
    if item.status != "OPEN" {
        "已停止"
    } else if item.reminder_paused {
        "已暂停"
    } else if item.next_reminder_at.is_some() {
        "等待提醒"
    } else {
        "无提醒计划"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Item, Tag};

    fn sample_item(
        id: &str,
        status: &str,
        category_id: Option<&str>,
        tag_id: Option<&str>,
        created_at: &str,
    ) -> Item {
        Item {
            id: id.into(),
            title: format!("事项 {id}"),
            notes: String::new(),
            status: status.into(),
            category_id: category_id.map(str::to_string),
            category_name: category_id.map(|_| "工作".into()),
            due_at: "2026-08-28T10:00:00+00:00".into(),
            due_local_date: "2026-08-28".into(),
            due_local_time: "18:00".into(),
            due_source: "EXPLICIT".into(),
            rollover_policy: "NONE".into(),
            rollover_count: 0,
            completion_policy: "NORMAL".into(),
            repeat_interval_minutes: None,
            next_reminder_at: None,
            reminder_paused: false,
            bypass_app_quiet_hours: false,
            created_at: created_at.into(),
            updated_at: created_at.into(),
            completed_at: None,
            deleted_at: None,
            event_kind: Some("ORDINARY".into()),
            reminder_plan: "REPEAT".into(),
            important: false,
            time_mode: "SPECIFIED".into(),
            start_at: None,
            end_at: None,
            target_at: None,
            lead_value: None,
            lead_unit: None,
            cadence_value: None,
            cadence_unit: None,
            emphasis_max_per_day: 8,
            repeat_time_mode: "SPECIFIED".into(),
            repeat_times: vec!["18:00".into()],
            tags: tag_id
                .map(|id| {
                    vec![Tag {
                        id: id.into(),
                        name: "客户".into(),
                        color: "#B06C49".into(),
                    }]
                })
                .unwrap_or_default(),
        }
    }

    #[test]
    fn user_text_that_looks_like_a_formula_is_escaped() {
        assert_eq!(
            excel_safe_text("=HYPERLINK(\"bad\")"),
            "'=HYPERLINK(\"bad\")"
        );
        assert_eq!(excel_safe_text("+1+1"), "'+1+1");
        assert_eq!(excel_safe_text("  =1+1"), "'  =1+1");
        assert_eq!(excel_safe_text("普通事项"), "普通事项");
    }

    #[test]
    fn empty_report_is_still_a_valid_workbook() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("report.xlsx");
        export_items(&path, &[]).unwrap();
        assert!(path.is_file());
        assert!(std::fs::metadata(path).unwrap().len() > 1_000);
    }

    #[test]
    fn export_filters_status_type_tag_and_created_range_together() {
        let items = vec![
            sample_item(
                "keep",
                "OPEN",
                Some("work"),
                Some("customer"),
                "2026-08-20T08:00:00+00:00",
            ),
            sample_item(
                "wrong-status",
                "DONE",
                Some("work"),
                Some("customer"),
                "2026-08-20T08:00:00+00:00",
            ),
            sample_item(
                "wrong-tag",
                "OPEN",
                Some("work"),
                Some("internal"),
                "2026-08-20T08:00:00+00:00",
            ),
            sample_item(
                "too-old",
                "OPEN",
                Some("work"),
                Some("customer"),
                "2026-07-20T08:00:00+00:00",
            ),
        ];
        let filtered = filter_items(
            items,
            &ExportFilterInput {
                status: "open".into(),
                event_kind: Some("ORDINARY".into()),
                category_id: Some("work".into()),
                tag_id: Some("customer".into()),
                created_from: Some("2026-08-01T00:00:00+00:00".into()),
                created_to: Some("2026-08-31T23:59:59+00:00".into()),
            },
        )
        .unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "keep");
    }

    #[test]
    #[ignore = "manual spreadsheet render verification"]
    fn create_visual_verification_report() {
        let directory = tempfile::tempdir().unwrap().keep();
        let path = directory.join("闪记导出验收.xlsx");
        let items = vec![Item {
            id: "verification-1".into(),
            title: "=HYPERLINK(\"https://invalid.example\",\"不应执行\")".into(),
            notes: "用于验证正文转义、列宽和本地时间。".into(),
            status: "OPEN".into(),
            category_id: Some("work".into()),
            category_name: Some("工作".into()),
            due_at: "2026-08-28T10:00:00+00:00".into(),
            due_local_date: "2026-08-28".into(),
            due_local_time: "18:00".into(),
            due_source: "EXPLICIT".into(),
            rollover_policy: "NONE".into(),
            rollover_count: 0,
            completion_policy: "MUST_COMPLETE_TODAY".into(),
            repeat_interval_minutes: Some(30),
            next_reminder_at: Some("2026-08-28T10:00:00+00:00".into()),
            reminder_paused: false,
            bypass_app_quiet_hours: false,
            created_at: "2026-08-28T09:00:00+00:00".into(),
            updated_at: "2026-08-28T09:00:00+00:00".into(),
            completed_at: None,
            deleted_at: None,
            event_kind: Some("TODAY_MUST".into()),
            reminder_plan: "EMPHASIS".into(),
            important: true,
            time_mode: "SPECIFIED".into(),
            start_at: None,
            end_at: None,
            target_at: None,
            lead_value: None,
            lead_unit: None,
            cadence_value: None,
            cadence_unit: None,
            emphasis_max_per_day: 8,
            repeat_time_mode: "SPECIFIED".into(),
            repeat_times: Vec::new(),
            tags: vec![Tag {
                id: "customer".into(),
                name: "客户".into(),
                color: "#B06C49".into(),
            }],
        }];
        export_items(&path, &items).unwrap();
        println!("{}", path.display());
    }
}
