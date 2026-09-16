use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use chrono::{DateTime, Local, NaiveDate, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::{
    db::Database,
    domain::Item,
    error::{AppError, AppResult},
};

const COLLECTIONS: [&str; 5] = [
    "event_kind",
    "category",
    "completed_item",
    "open_item",
    "important_item",
];

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryRequest {
    pub period: String,
    pub year: i32,
    pub month: Option<u32>,
    pub quarter: Option<u32>,
    pub template_path: String,
    pub output_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryTemplatePreview {
    pub template_path: String,
    pub placeholders: Vec<String>,
    pub unsupported_placeholders: Vec<String>,
    pub valid: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryResult {
    pub path: String,
    pub item_count: usize,
    pub completed_count: usize,
    pub open_count: usize,
    pub overdue_count: usize,
    pub important_count: usize,
    pub period_label: String,
}

pub fn validate_template(path: &Path) -> AppResult<SummaryTemplatePreview> {
    let xml = read_document_xml(path)?;
    let placeholders = collect_placeholders(&xml);
    let supported = supported_placeholder_names();
    let unsupported = placeholders
        .iter()
        .filter(|name| !supported.contains(name.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let structure_error = collection_structure_error(&xml);
    let valid = !placeholders.is_empty() && unsupported.is_empty() && structure_error.is_none();
    let message = if placeholders.is_empty() {
        "模板中没有找到受支持的占位符。".into()
    } else if unsupported.is_empty() {
        structure_error
            .unwrap_or_else(|| format!("已识别 {} 个占位符，可以生成小结。", placeholders.len()))
    } else {
        format!(
            "有 {} 个占位符暂不支持，请修改模板后重试。",
            unsupported.len()
        )
    };
    Ok(SummaryTemplatePreview {
        template_path: path.to_string_lossy().into_owned(),
        placeholders,
        unsupported_placeholders: unsupported,
        valid,
        message,
    })
}

pub fn generate_summary(
    database: &Database,
    request: &SummaryRequest,
    now: DateTime<Utc>,
) -> AppResult<SummaryResult> {
    let template = PathBuf::from(&request.template_path);
    let output = PathBuf::from(&request.output_path);
    validate_summary_request(request, &template, &output)?;
    let preview = validate_template(&template)?;
    if !preview.valid {
        return Err(AppError::Validation(preview.message));
    }
    let (start, end, period_label) = period_bounds(request)?;
    let mut items = database.list_items("all", now)?;
    items.retain(|item| item_in_period(item, start, end));
    items.sort_by(|left, right| {
        left.due_at
            .cmp(&right.due_at)
            .then(left.title.cmp(&right.title))
    });

    let completed = items
        .iter()
        .filter(|item| item.status == "DONE")
        .cloned()
        .collect::<Vec<_>>();
    let open = items
        .iter()
        .filter(|item| item.status == "OPEN")
        .cloned()
        .collect::<Vec<_>>();
    let overdue = open
        .iter()
        .filter(|item| item.due_at < now.to_rfc3339())
        .count();
    let important = items
        .iter()
        .filter(|item| item.important)
        .cloned()
        .collect::<Vec<_>>();
    let completion_rate = if items.is_empty() {
        "0%".into()
    } else {
        format!(
            "{:.0}%",
            completed.len() as f64 * 100.0 / items.len() as f64
        )
    };

    let mut scalar = BTreeMap::new();
    scalar.insert("report_title".into(), format!("{period_label}小结"));
    scalar.insert("period_label".into(), period_label.clone());
    scalar.insert(
        "generated_at".into(),
        now.with_timezone(&Local)
            .format("%Y-%m-%d %H:%M")
            .to_string(),
    );
    scalar.insert(
        "period_summary".into(),
        format!(
            "本期共纳入 {} 项，完成 {} 项，期末未完成 {} 项，其中逾期 {} 项、重要 {} 项。",
            items.len(),
            completed.len(),
            open.len(),
            overdue,
            important.len()
        ),
    );
    scalar.insert("total_count".into(), items.len().to_string());
    scalar.insert("completed_count".into(), completed.len().to_string());
    scalar.insert("open_count".into(), open.len().to_string());
    scalar.insert("overdue_count".into(), overdue.to_string());
    scalar.insert("important_count".into(), important.len().to_string());
    scalar.insert("completion_rate".into(), completion_rate);

    let collections = summary_collections(&items, &completed, &open, &important, now);
    let xml = read_document_xml(&template)?;
    let rendered = render_document_xml(&xml, &scalar, &collections)?;
    write_docx(&template, &output, &rendered)?;

    Ok(SummaryResult {
        path: output.to_string_lossy().into_owned(),
        item_count: items.len(),
        completed_count: completed.len(),
        open_count: open.len(),
        overdue_count: overdue,
        important_count: important.len(),
        period_label,
    })
}

fn validate_summary_request(
    request: &SummaryRequest,
    template: &Path,
    output: &Path,
) -> AppResult<()> {
    if !matches!(request.period.as_str(), "MONTH" | "QUARTER") {
        return Err(AppError::Validation("请选择月度或季度小结".into()));
    }
    if template
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
        != Some("docx")
    {
        return Err(AppError::Validation(
            "请选择有效的 Word 模板（.docx）".into(),
        ));
    }
    if output
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
        != Some("docx")
    {
        return Err(AppError::Validation(
            "小结保存位置必须使用 .docx 扩展名".into(),
        ));
    }
    if template == output {
        return Err(AppError::Validation(
            "请为小结选择不同于模板的保存位置".into(),
        ));
    }
    Ok(())
}

fn period_bounds(request: &SummaryRequest) -> AppResult<(DateTime<Utc>, DateTime<Utc>, String)> {
    let (start_month, label) = match request.period.as_str() {
        "MONTH" => {
            let month = request
                .month
                .filter(|value| (1..=12).contains(value))
                .ok_or_else(|| AppError::Validation("请选择有效月份".into()))?;
            (month, format!("{} 年 {} 月", request.year, month))
        }
        "QUARTER" => {
            let quarter = request
                .quarter
                .filter(|value| (1..=4).contains(value))
                .ok_or_else(|| AppError::Validation("请选择有效季度".into()))?;
            (
                (quarter - 1) * 3 + 1,
                format!("{} 年第 {} 季度", request.year, quarter),
            )
        }
        _ => return Err(AppError::Validation("请选择月度或季度小结".into())),
    };
    let months = if request.period == "MONTH" { 1 } else { 3 };
    let start_date = NaiveDate::from_ymd_opt(request.year, start_month, 1)
        .ok_or_else(|| AppError::Validation("小结周期无效".into()))?;
    let (end_year, end_month) = if start_month + months > 12 {
        (request.year + 1, start_month + months - 12)
    } else {
        (request.year, start_month + months)
    };
    let end_date = NaiveDate::from_ymd_opt(end_year, end_month, 1)
        .ok_or_else(|| AppError::Validation("小结周期无效".into()))?;
    let start_local = Local
        .from_local_datetime(&start_date.and_hms_opt(0, 0, 0).unwrap())
        .earliest()
        .ok_or_else(|| AppError::Validation("小结开始时间无效".into()))?;
    let end_local = Local
        .from_local_datetime(&end_date.and_hms_opt(0, 0, 0).unwrap())
        .earliest()
        .ok_or_else(|| AppError::Validation("小结结束时间无效".into()))?;
    Ok((
        start_local.with_timezone(&Utc),
        end_local.with_timezone(&Utc),
        label,
    ))
}

fn item_in_period(item: &Item, start: DateTime<Utc>, end: DateTime<Utc>) -> bool {
    let dated_in_period = [&item.created_at, &item.due_at]
        .into_iter()
        .chain(item.completed_at.as_ref())
        .filter_map(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc))
        .any(|value| value >= start && value < end);
    let still_overdue = item.status == "OPEN"
        && DateTime::parse_from_rfc3339(&item.due_at)
            .is_ok_and(|value| value.with_timezone(&Utc) < end);
    dated_in_period || still_overdue
}

fn summary_collections(
    items: &[Item],
    completed: &[Item],
    open: &[Item],
    important: &[Item],
    now: DateTime<Utc>,
) -> BTreeMap<String, Vec<BTreeMap<String, String>>> {
    let mut result = BTreeMap::new();
    let kind_order = [
        "ORDINARY",
        "ONE_TIME",
        "TODAY_MUST",
        "WARNING",
        "CONTINUOUS",
        "MONTHLY",
        "YEARLY",
    ];
    result.insert(
        "event_kind".into(),
        kind_order
            .iter()
            .map(|kind| {
                let values = items
                    .iter()
                    .filter(|item| item.event_kind.as_deref() == Some(*kind))
                    .collect::<Vec<_>>();
                BTreeMap::from([
                    ("name".into(), event_kind_label(Some(kind)).into()),
                    ("total".into(), values.len().to_string()),
                    (
                        "completed".into(),
                        values
                            .iter()
                            .filter(|item| item.status == "DONE")
                            .count()
                            .to_string(),
                    ),
                ])
            })
            .collect(),
    );
    let mut categories = BTreeMap::<String, (usize, usize)>::new();
    for item in items {
        let entry = categories
            .entry(
                item.category_name
                    .clone()
                    .unwrap_or_else(|| "未分类".into()),
            )
            .or_default();
        entry.0 += 1;
        entry.1 += usize::from(item.status == "DONE");
    }
    result.insert(
        "category".into(),
        categories
            .into_iter()
            .map(|(name, (total, done))| {
                BTreeMap::from([
                    ("name".into(), name),
                    ("total".into(), total.to_string()),
                    ("completed".into(), done.to_string()),
                ])
            })
            .collect(),
    );
    result.insert(
        "completed_item".into(),
        completed.iter().map(|item| item_row(item, now)).collect(),
    );
    result.insert(
        "open_item".into(),
        open.iter().map(|item| item_row(item, now)).collect(),
    );
    result.insert(
        "important_item".into(),
        important.iter().map(|item| item_row(item, now)).collect(),
    );
    result
}

fn item_row(item: &Item, now: DateTime<Utc>) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("title".into(), item.title.clone()),
        (
            "event_kind".into(),
            event_kind_label(item.event_kind.as_deref()).into(),
        ),
        (
            "completed_at".into(),
            local_datetime(item.completed_at.as_deref()),
        ),
        (
            "due_at".into(),
            format!("{} {}", item.due_local_date, item.due_local_time),
        ),
        (
            "status".into(),
            if item.status == "DONE" {
                "已完成".into()
            } else if item.due_at < now.to_rfc3339() {
                "已逾期".into()
            } else {
                "待处理".into()
            },
        ),
    ])
}

fn local_datetime(value: Option<&str>) -> String {
    value
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| {
            value
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

fn event_kind_label(value: Option<&str>) -> &'static str {
    match value {
        Some("ORDINARY") => "普通",
        Some("ONE_TIME") => "一次性",
        Some("TODAY_MUST") => "今日必做",
        Some("WARNING") => "预警",
        Some("CONTINUOUS") => "持续",
        Some("MONTHLY") => "月度",
        Some("YEARLY") => "年度",
        _ => "待选择",
    }
}

fn read_document_xml(path: &Path) -> AppResult<String> {
    let file = File::open(path)
        .map_err(|_| AppError::Validation("Word 模板无法读取，请重新选择".into()))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|_| AppError::Validation("Word 模板格式无效，原文件没有改变".into()))?;
    archive
        .by_name("[Content_Types].xml")
        .map_err(|_| AppError::Validation("Word 模板结构不完整，原文件没有改变".into()))?;
    let mut document = archive
        .by_name("word/document.xml")
        .map_err(|_| AppError::Validation("Word 模板缺少正文，原文件没有改变".into()))?;
    let mut xml = String::new();
    document.read_to_string(&mut xml)?;
    Ok(xml)
}

fn collect_placeholders(xml: &str) -> Vec<String> {
    let mut result = BTreeSet::new();
    let mut rest = xml;
    while let Some(start) = rest.find("{{") {
        rest = &rest[start + 2..];
        let Some(end) = rest.find("}}") else { break };
        result.insert(rest[..end].trim().to_string());
        rest = &rest[end + 2..];
    }
    result.into_iter().collect()
}

fn supported_placeholder_names() -> BTreeSet<&'static str> {
    let scalar = [
        "report_title",
        "period_label",
        "generated_at",
        "period_summary",
        "total_count",
        "completed_count",
        "open_count",
        "overdue_count",
        "important_count",
        "completion_rate",
    ];
    let mut values = scalar.into_iter().collect::<BTreeSet<_>>();
    for name in [
        "event_kind.name",
        "event_kind.total",
        "event_kind.completed",
        "category.name",
        "category.total",
        "category.completed",
        "completed_item.title",
        "completed_item.event_kind",
        "completed_item.completed_at",
        "open_item.title",
        "open_item.due_at",
        "open_item.status",
        "important_item.title",
        "important_item.status",
        "important_item.due_at",
    ] {
        values.insert(name);
    }
    values
}

fn collection_structure_error(xml: &str) -> Option<String> {
    for collection in COLLECTIONS {
        let marker = format!("{{{{{collection}.");
        let mut offset = 0;
        while let Some(relative) = xml[offset..].find(&marker) {
            let position = offset + relative;
            let row_start = xml[..position].rfind("<w:tr");
            let previous_row_end = xml[..position].rfind("</w:tr>");
            let row_end = xml[position..].find("</w:tr>");
            if row_start.is_none()
                || row_end.is_none()
                || previous_row_end.is_some_and(|end| end > row_start.unwrap_or_default())
            {
                return Some(format!(
                    "{collection} 列表占位符必须放在 Word 表格模板行中。"
                ));
            }
            offset = position + marker.len();
        }
    }

    let mut offset = 0;
    while let Some(relative_start) = xml[offset..].find("<w:tr") {
        let row_start = offset + relative_start;
        let Some(relative_end) = xml[row_start..].find("</w:tr>") else {
            return Some("Word 表格模板行结构不完整。".into());
        };
        let row_end = row_start + relative_end + "</w:tr>".len();
        let row = &xml[row_start..row_end];
        let present = COLLECTIONS
            .iter()
            .filter(|collection| row.contains(&format!("{{{{{collection}.")))
            .collect::<Vec<_>>();
        if present.len() > 1 {
            return Some("同一表格模板行不能混用两种事项列表占位符。".into());
        }
        offset = row_end;
    }
    None
}

fn render_document_xml(
    xml: &str,
    scalar: &BTreeMap<String, String>,
    collections: &BTreeMap<String, Vec<BTreeMap<String, String>>>,
) -> AppResult<String> {
    let mut rendered = xml.to_string();
    for collection in COLLECTIONS {
        rendered = expand_collection_rows(
            &rendered,
            collection,
            collections.get(collection).map_or(&[], Vec::as_slice),
        )?;
    }
    for (name, value) in scalar {
        rendered = rendered.replace(&format!("{{{{{name}}}}}"), &escape_xml(value));
    }
    let remaining = collect_placeholders(&rendered);
    if !remaining.is_empty() {
        return Err(AppError::Validation(format!(
            "模板占位符没有完整替换：{}",
            remaining.join("、")
        )));
    }
    Ok(rendered)
}

fn expand_collection_rows(
    xml: &str,
    collection: &str,
    rows: &[BTreeMap<String, String>],
) -> AppResult<String> {
    let marker = format!("{{{{{collection}.");
    let Some(marker_at) = xml.find(&marker) else {
        return Ok(xml.to_string());
    };
    let row_start = xml[..marker_at]
        .rfind("<w:tr")
        .ok_or_else(|| AppError::Validation(format!("{collection} 占位符必须放在表格模板行中")))?;
    let relative_end = xml[marker_at..]
        .find("</w:tr>")
        .ok_or_else(|| AppError::Validation(format!("{collection} 模板行结构不完整")))?;
    let row_end = marker_at + relative_end + "</w:tr>".len();
    let template = &xml[row_start..row_end];
    let mut replacements = String::new();
    for values in rows {
        let mut row = template.to_string();
        for (field, value) in values {
            row = row.replace(&format!("{{{{{collection}.{field}}}}}"), &escape_xml(value));
        }
        replacements.push_str(&row);
    }
    let mut result = String::with_capacity(xml.len() + replacements.len());
    result.push_str(&xml[..row_start]);
    result.push_str(&replacements);
    result.push_str(&xml[row_end..]);
    if result.contains(&marker) {
        return expand_collection_rows(&result, collection, rows);
    }
    Ok(result)
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn write_docx(template: &Path, output: &Path, document_xml: &str) -> AppResult<()> {
    let parent = output
        .parent()
        .ok_or_else(|| AppError::Validation("小结保存位置无效".into()))?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".shanji-summary-{}.tmp", Uuid::new_v4()));
    if let Err(error) = write_temporary_docx(template, &temporary, document_xml)
        .and_then(|_| read_document_xml(&temporary).map(|_| ()))
    {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    if output.exists() {
        let backup = parent.join(format!(".shanji-summary-{}.bak", Uuid::new_v4()));
        if let Err(error) = fs::rename(output, &backup) {
            let _ = fs::remove_file(&temporary);
            return Err(error.into());
        }
        if let Err(error) = fs::rename(&temporary, output) {
            let _ = fs::rename(&backup, output);
            let _ = fs::remove_file(&temporary);
            return Err(error.into());
        }
        let _ = fs::remove_file(backup);
    } else if let Err(error) = fs::rename(&temporary, output) {
        let _ = fs::remove_file(&temporary);
        return Err(error.into());
    }
    Ok(())
}

fn write_temporary_docx(template: &Path, temporary: &Path, document_xml: &str) -> AppResult<()> {
    let input = File::open(template)?;
    let mut archive =
        ZipArchive::new(input).map_err(|_| AppError::Validation("Word 模板格式无效".into()))?;
    let temp_file = File::create(temporary)?;
    let mut writer = ZipWriter::new(temp_file);
    for index in 0..archive.len() {
        let mut source = archive
            .by_index(index)
            .map_err(|_| AppError::Validation("Word 模板内容无法读取，原文件没有改变".into()))?;
        let name = source.name().to_string();
        let options = SimpleFileOptions::default().compression_method(source.compression());
        if source.is_dir() {
            writer.add_directory(name, options).map_err(|_| {
                AppError::Validation("Word 小结生成失败，模板和已有文件没有改变".into())
            })?;
            continue;
        }
        writer.start_file(name.clone(), options).map_err(|_| {
            AppError::Validation("Word 小结生成失败，模板和已有文件没有改变".into())
        })?;
        if name == "word/document.xml" {
            writer.write_all(document_xml.as_bytes())?;
        } else {
            std::io::copy(&mut source, &mut writer)?;
        }
    }
    writer
        .finish()
        .map_err(|_| AppError::Validation("Word 小结生成失败，模板和已有文件没有改变".into()))?
        .sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{CreateItemInput, EventConfigurationInput};

    #[test]
    fn scalar_and_collection_rows_are_replaced_without_touching_formatting() {
        let xml = r#"<w:document><w:body><w:p><w:r><w:t>{{period_label}}</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:p><w:r><w:t>{{open_item.title}}</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>{{open_item.status}}</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:body></w:document>"#;
        let scalar = BTreeMap::from([("period_label".into(), "2026 年 9 月".into())]);
        let rows = BTreeMap::from([(
            "open_item".into(),
            vec![
                BTreeMap::from([
                    ("title".into(), "核对 <合同>".into()),
                    ("status".into(), "待处理".into()),
                ]),
                BTreeMap::from([
                    ("title".into(), "提交总结".into()),
                    ("status".into(), "已逾期".into()),
                ]),
            ],
        )]);
        let rendered = render_document_xml(xml, &scalar, &rows).unwrap();
        assert!(rendered.contains("2026 年 9 月"));
        assert!(rendered.contains("核对 &lt;合同&gt;"));
        assert_eq!(rendered.matches("<w:tr>").count(), 2);
        assert!(collect_placeholders(&rendered).is_empty());
    }

    #[test]
    fn mixed_collection_placeholders_in_one_table_row_are_rejected() {
        let xml = r#"<w:document><w:body><w:tbl><w:tr><w:tc>{{open_item.title}}</w:tc><w:tc>{{completed_item.title}}</w:tc></w:tr></w:tbl></w:body></w:document>"#;
        assert_eq!(
            collection_structure_error(xml).as_deref(),
            Some("同一表格模板行不能混用两种事项列表占位符。")
        );
    }

    #[test]
    #[ignore = "manual Word render verification"]
    fn create_visual_verification_summary() {
        let template_path = std::env::var("SHANJI_SUMMARY_TEMPLATE").unwrap();
        let output_path = std::env::var("SHANJI_SUMMARY_OUTPUT").unwrap();
        let database = Database::in_memory().unwrap();
        let now = Local
            .with_ymd_and_hms(2026, 9, 15, 10, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        for (index, (title, due_day, important)) in [
            ("核对九月项目预算", 12, true),
            ("整理客户回访清单", 20, false),
            ("准备季度复盘材料", 28, true),
        ]
        .into_iter()
        .enumerate()
        {
            let due = Local
                .with_ymd_and_hms(2026, 9, due_day, 17, 0, 0)
                .unwrap()
                .with_timezone(&Utc);
            let item = database
                .create_item(
                    &CreateItemInput {
                        title: title.into(),
                        notes: String::new(),
                        category_id: None,
                        due_at: Some(due.to_rfc3339()),
                        must_complete_today: false,
                        repeat_interval_minutes: None,
                        tag_ids: Vec::new(),
                        event: Some(EventConfigurationInput {
                            kind: Some("ORDINARY".into()),
                            reminder_plan: None,
                            reminder_plan_source: None,
                            important,
                            ..EventConfigurationInput::default()
                        }),
                    },
                    now - chrono::Duration::days(10 - index as i64),
                )
                .unwrap();
            if index == 0 {
                database
                    .set_item_completed(&item.id, true, now - chrono::Duration::days(1))
                    .unwrap();
            }
        }
        let result = generate_summary(
            &database,
            &SummaryRequest {
                period: "MONTH".into(),
                year: 2026,
                month: Some(9),
                quarter: None,
                template_path,
                output_path: output_path.clone(),
            },
            now,
        )
        .unwrap();
        assert_eq!(result.path, output_path);
        assert_eq!(result.item_count, 3);
    }
}
