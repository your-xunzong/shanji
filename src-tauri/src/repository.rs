use std::{
    collections::{BTreeMap, HashMap},
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, Transaction, params, params_from_iter, types::ValueRef};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    db::{Database, LATEST_SCHEMA_VERSION},
    error::{AppError, AppResult},
};

const PACKAGE_FORMAT: &str = "shanji-local-repository";
const PACKAGE_VERSION: u32 = 1;
const PACKAGE_EXTENSION: &str = "sjpack";
const REPOSITORY_DIRECTORY: &str = "shanji-repository";
const MAX_PACKAGE_BYTES: u64 = 256 * 1024 * 1024;
const SNAPSHOT_TABLES: [&str; 13] = [
    "app_settings",
    "categories",
    "tags",
    "items",
    "item_tags",
    "reminder_events",
    "classification_reminder_events",
    "item_events",
    "drafts",
    "event_kind_defaults",
    "item_tombstones",
    "email_delivery_rules",
    "timeline_view_preferences",
];

type PackageRow = BTreeMap<String, JsonValue>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepositoryPayload {
    format: String,
    package_version: u32,
    schema_version: i64,
    source_instance_id: String,
    exported_at: String,
    tables: BTreeMap<String, Vec<PackageRow>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepositoryEnvelope {
    checksum_sha256: String,
    payload: RepositoryPayload,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositorySourcePreview {
    pub source_instance_id: String,
    pub exported_at: String,
    pub item_count: u64,
    pub new_items: u64,
    pub unchanged_items: u64,
    pub conflicts: u64,
    pub tombstones: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryPreview {
    pub sources: Vec<RepositorySourcePreview>,
    pub new_items: u64,
    pub unchanged_items: u64,
    pub conflicts: u64,
    pub tombstones: u64,
    pub settings_need_review: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryStatus {
    pub configured: bool,
    pub path: Option<String>,
    pub available: bool,
    pub local_item_count: u64,
    pub package_count: u64,
    pub pending_source_count: u64,
    pub conflict_count: u64,
    pub last_synced_at: Option<String>,
    pub last_package_at: Option<String>,
    pub snapshot_ready: bool,
    pub last_error_code: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositorySyncResult {
    pub imported_items: u64,
    pub unchanged_items: u64,
    pub conflicts: u64,
    pub tombstones_applied: u64,
    pub package_path: Option<String>,
    pub package_written: bool,
    pub synced_at: String,
}

#[derive(Default)]
struct MergeCounts {
    imported: u64,
    unchanged: u64,
    conflicts: u64,
    tombstones: u64,
}

pub fn configure_repository(
    database: &Database,
    selected_directory: &Path,
    now: DateTime<Utc>,
) -> AppResult<RepositoryStatus> {
    if !selected_directory.is_dir() {
        return Err(AppError::Validation(
            "请选择一个当前可用的文件夹作为个人仓库".into(),
        ));
    }
    let root = selected_directory.join(REPOSITORY_DIRECTORY);
    fs::create_dir_all(&root)?;
    verify_directory_writable(&root)?;
    let normalized = fs::canonicalize(&root)?;
    let connection = database.connection.lock().expect("database mutex poisoned");
    connection.execute(
        "INSERT INTO data_repository_config
           (id, path, enabled, last_synced_at, last_package_at, last_error_code)
         VALUES (1, ?1, 1, NULL, NULL, NULL)
         ON CONFLICT(id) DO UPDATE SET path = excluded.path, enabled = 1,
           last_synced_at = NULL, last_package_at = NULL, last_error_code = NULL",
        [normalized.to_string_lossy().as_ref()],
    )?;
    drop(connection);
    if write_snapshot_and_record(database, &normalized, now).is_err() {
        record_repository_error(database, "snapshot_write_failed");
    }
    repository_status(database, now)
}

pub fn disable_repository(database: &Database, now: DateTime<Utc>) -> AppResult<RepositoryStatus> {
    let connection = database.connection.lock().expect("database mutex poisoned");
    connection.execute(
        "UPDATE data_repository_config SET enabled = 0, last_error_code = NULL WHERE id = 1",
        [],
    )?;
    drop(connection);
    repository_status(database, now)
}

pub fn repository_status(database: &Database, _now: DateTime<Utc>) -> AppResult<RepositoryStatus> {
    let connection = database.connection.lock().expect("database mutex poisoned");
    let local_item_count =
        connection.query_row("SELECT COUNT(*) FROM items", [], |row| row.get::<_, i64>(0))? as u64;
    let conflict_count = connection.query_row(
        "SELECT COUNT(*) FROM merge_conflicts WHERE resolved_at IS NULL",
        [],
        |row| row.get::<_, i64>(0),
    )? as u64;
    let config = connection
        .query_row(
            "SELECT path, enabled, last_synced_at, last_package_at, last_error_code
             FROM data_repository_config WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)? != 0,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        )
        .optional()?;
    let instance_id = instance_id(&connection)?;
    drop(connection);

    let Some((path, enabled, last_synced_at, last_package_at, last_error_code)) = config else {
        return Ok(RepositoryStatus {
            configured: false,
            path: None,
            available: false,
            local_item_count,
            package_count: 0,
            pending_source_count: 0,
            conflict_count,
            last_synced_at: None,
            last_package_at: None,
            snapshot_ready: false,
            last_error_code: None,
            message: "尚未选择个人仓库，本机记录与提醒不受影响。".into(),
        });
    };
    if !enabled {
        return Ok(RepositoryStatus {
            configured: false,
            path: Some(path),
            available: false,
            local_item_count,
            package_count: 0,
            pending_source_count: 0,
            conflict_count,
            last_synced_at,
            last_package_at,
            snapshot_ready: false,
            last_error_code,
            message: "个人仓库已停用，本机仍可正常记录和提醒。".into(),
        });
    }
    let root = PathBuf::from(&path);
    let available = root.is_dir() && verify_directory_readable(&root).is_ok();
    let packages = if available {
        latest_packages(&root).unwrap_or_default()
    } else {
        Vec::new()
    };
    let pending_source_count = packages
        .iter()
        .filter(|package| package.payload.source_instance_id != instance_id)
        .count() as u64;
    let snapshot_ready = packages
        .iter()
        .any(|package| package.payload.source_instance_id == instance_id);
    Ok(RepositoryStatus {
        configured: true,
        path: Some(path),
        available,
        local_item_count,
        package_count: packages.len() as u64,
        pending_source_count,
        conflict_count,
        last_synced_at,
        last_package_at,
        snapshot_ready,
        last_error_code,
        message: if available && snapshot_ready {
            "个人仓库可用；归并期间本机仍是运行数据来源。".into()
        } else if available {
            "个人仓库位置可用，但本机快照尚未写入。".into()
        } else {
            "个人仓库暂时不可用，本机记录仍然安全并可继续使用。".into()
        },
    })
}

pub fn preview_repository(database: &Database) -> AppResult<RepositoryPreview> {
    let root = configured_root(database)?;
    let packages = latest_packages(&root)?;
    let connection = database.connection.lock().expect("database mutex poisoned");
    let own_instance = instance_id(&connection)?;
    let mut sources = Vec::new();
    let mut preview = RepositoryPreview {
        sources: Vec::new(),
        new_items: 0,
        unchanged_items: 0,
        conflicts: 0,
        tombstones: 0,
        settings_need_review: false,
    };
    for package in packages
        .into_iter()
        .filter(|package| package.payload.source_instance_id != own_instance)
    {
        let source = preview_package(&connection, &package.payload)?;
        preview.new_items += source.new_items;
        preview.unchanged_items += source.unchanged_items;
        preview.conflicts += source.conflicts;
        preview.tombstones += source.tombstones;
        preview.settings_need_review |= package
            .payload
            .tables
            .get("app_settings")
            .is_some_and(|rows| !rows.is_empty());
        sources.push(source);
    }
    preview.sources = sources;
    Ok(preview)
}

pub fn sync_repository(database: &Database, now: DateTime<Utc>) -> AppResult<RepositorySyncResult> {
    let root = configured_root(database)?;
    verify_directory_writable(&root)?;
    let packages = latest_packages(&root)?;
    database.create_backup()?;

    let mut counts = MergeCounts::default();
    let own_instance;
    {
        let mut connection = database.connection.lock().expect("database mutex poisoned");
        own_instance = instance_id(&connection)?;
        let transaction = connection.transaction()?;
        for package in packages
            .iter()
            .filter(|package| package.payload.source_instance_id != own_instance)
        {
            merge_package(&transaction, &package.payload, now, &mut counts)?;
        }
        transaction.execute(
            "UPDATE data_repository_config
             SET last_synced_at = ?1, last_error_code = NULL WHERE id = 1",
            [now.to_rfc3339()],
        )?;
        transaction.commit()?;
    }

    let package_path = match write_snapshot_and_record(database, &root, now) {
        Ok(path) => Some(path.to_string_lossy().into_owned()),
        Err(_) => {
            record_repository_error(database, "snapshot_write_failed");
            None
        }
    };
    Ok(RepositorySyncResult {
        imported_items: counts.imported,
        unchanged_items: counts.unchanged,
        conflicts: counts.conflicts,
        tombstones_applied: counts.tombstones,
        package_written: package_path.is_some(),
        package_path,
        synced_at: now.to_rfc3339(),
    })
}

pub fn publish_repository_snapshot(
    database: &Database,
    now: DateTime<Utc>,
) -> AppResult<RepositoryStatus> {
    let root = configured_root(database)?;
    match write_snapshot_and_record(database, &root, now) {
        Ok(_) => repository_status(database, now),
        Err(error) => {
            record_repository_error(database, "snapshot_write_failed");
            Err(error)
        }
    }
}

fn write_snapshot_and_record(
    database: &Database,
    root: &Path,
    now: DateTime<Utc>,
) -> AppResult<PathBuf> {
    verify_directory_writable(root)?;
    let path = write_current_package(database, root, now)?;
    let connection = database.connection.lock().expect("database mutex poisoned");
    connection.execute(
        "UPDATE data_repository_config
         SET last_package_at = ?1, last_error_code = NULL WHERE id = 1",
        [now.to_rfc3339()],
    )?;
    Ok(path)
}

fn record_repository_error(database: &Database, code: &str) {
    if let Ok(connection) = database.connection.lock() {
        let _ = connection.execute(
            "UPDATE data_repository_config SET last_error_code = ?1 WHERE id = 1",
            [code],
        );
    }
}

fn configured_root(database: &Database) -> AppResult<PathBuf> {
    let connection = database.connection.lock().expect("database mutex poisoned");
    let config = connection
        .query_row(
            "SELECT path FROM data_repository_config WHERE id = 1 AND enabled = 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let path = config.ok_or_else(|| AppError::Validation("请先选择个人仓库位置".into()))?;
    let root = PathBuf::from(path);
    if !root.is_dir() {
        return Err(AppError::Validation(
            "个人仓库暂时不可用，本机记录未受影响；请重新连接后重试".into(),
        ));
    }
    Ok(root)
}

fn verify_directory_readable(path: &Path) -> AppResult<()> {
    fs::read_dir(path)?;
    Ok(())
}

fn verify_directory_writable(path: &Path) -> AppResult<()> {
    verify_directory_readable(path)?;
    let probe = path.join(format!(".write-check-{}", Uuid::new_v4()));
    let mut file = File::create(&probe)?;
    file.write_all(b"shanji")?;
    file.sync_all()?;
    fs::remove_file(probe)?;
    Ok(())
}

fn write_current_package(
    database: &Database,
    root: &Path,
    now: DateTime<Utc>,
) -> AppResult<PathBuf> {
    let connection = database.connection.lock().expect("database mutex poisoned");
    let source_instance_id = instance_id(&connection)?;
    let mut tables = BTreeMap::new();
    for table in SNAPSHOT_TABLES {
        let excluded = match table {
            "app_settings" => &["autostart_enabled", "smtp_verified_at"][..],
            _ => &[][..],
        };
        tables.insert(
            table.to_string(),
            export_table(&connection, table, excluded)?,
        );
    }
    drop(connection);
    let payload = RepositoryPayload {
        format: PACKAGE_FORMAT.into(),
        package_version: PACKAGE_VERSION,
        schema_version: LATEST_SCHEMA_VERSION,
        source_instance_id: source_instance_id.clone(),
        exported_at: now.to_rfc3339(),
        tables,
    };
    let checksum_sha256 = payload_checksum(&payload)?;
    let envelope = RepositoryEnvelope {
        checksum_sha256,
        payload,
    };
    let bytes = serde_json::to_vec(&envelope)?;
    let stamp = now.format("%Y%m%dT%H%M%S%3fZ");
    let destination = root.join(format!(
        "shanji-{source_instance_id}-{stamp}.{PACKAGE_EXTENSION}"
    ));
    let temporary = root.join(format!(".{}.tmp", Uuid::new_v4()));
    let mut file = File::create(&temporary)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    load_package(&temporary)?;
    fs::rename(&temporary, &destination)?;
    Ok(destination)
}

fn latest_packages(root: &Path) -> AppResult<Vec<RepositoryEnvelope>> {
    let mut latest = HashMap::<String, RepositoryEnvelope>::new();
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) != Some(PACKAGE_EXTENSION) {
            continue;
        }
        let package = load_package(&path)?;
        let source = package.payload.source_instance_id.clone();
        let replace = latest
            .get(&source)
            .is_none_or(|current| package.payload.exported_at > current.payload.exported_at);
        if replace {
            latest.insert(source, package);
        }
    }
    let mut values = latest.into_values().collect::<Vec<_>>();
    values.sort_by(|left, right| right.payload.exported_at.cmp(&left.payload.exported_at));
    Ok(values)
}

fn load_package(path: &Path) -> AppResult<RepositoryEnvelope> {
    let metadata = fs::metadata(path)?;
    if metadata.len() == 0 || metadata.len() > MAX_PACKAGE_BYTES {
        return Err(AppError::Validation(
            "个人仓库中有无法读取的数据包，归并已停止，原文件保持不变".into(),
        ));
    }
    let mut file = File::open(path)?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes)?;
    let envelope: RepositoryEnvelope = serde_json::from_slice(&bytes)
        .map_err(|_| AppError::Validation("个人仓库中的数据包格式无效，归并已停止".into()))?;
    if envelope.payload.format != PACKAGE_FORMAT
        || envelope.payload.package_version != PACKAGE_VERSION
        || envelope.payload.schema_version > LATEST_SCHEMA_VERSION
        || envelope.payload.source_instance_id.trim().is_empty()
        || envelope
            .payload
            .tables
            .keys()
            .any(|table| !SNAPSHOT_TABLES.contains(&table.as_str()))
    {
        return Err(AppError::Validation(
            "个人仓库中的数据包版本暂不兼容，归并已停止".into(),
        ));
    }
    if payload_checksum(&envelope.payload)? != envelope.checksum_sha256 {
        return Err(AppError::Validation(
            "个人仓库中的数据包未通过完整性检查，归并已停止".into(),
        ));
    }
    Ok(envelope)
}

fn payload_checksum(payload: &RepositoryPayload) -> AppResult<String> {
    let bytes = serde_json::to_vec(payload)?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

fn export_table(
    connection: &rusqlite::Connection,
    table: &str,
    excluded: &[&str],
) -> AppResult<Vec<PackageRow>> {
    let columns = table_columns(connection, table)?
        .into_iter()
        .filter(|column| !excluded.contains(&column.as_str()))
        .collect::<Vec<_>>();
    let sql = format!("SELECT {} FROM {table} ORDER BY rowid", columns.join(", "));
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map([], |row| {
        let mut result = BTreeMap::new();
        for (index, column) in columns.iter().enumerate() {
            result.insert(column.clone(), sqlite_value(row.get_ref(index)?));
        }
        Ok(result)
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

fn table_columns(connection: &rusqlite::Connection, table: &str) -> AppResult<Vec<String>> {
    if !SNAPSHOT_TABLES.contains(&table) {
        return Err(AppError::Validation("数据包包含未支持的数据范围".into()));
    }
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    if columns.is_empty() {
        return Err(AppError::Validation(
            "本机数据结构不完整，无法创建数据包".into(),
        ));
    }
    Ok(columns)
}

fn sqlite_value(value: ValueRef<'_>) -> JsonValue {
    match value {
        ValueRef::Null => JsonValue::Null,
        ValueRef::Integer(value) => JsonValue::from(value),
        ValueRef::Real(value) => JsonValue::from(value),
        ValueRef::Text(value) => JsonValue::String(String::from_utf8_lossy(value).into_owned()),
        ValueRef::Blob(_) => JsonValue::Null,
    }
}

fn preview_package(
    connection: &rusqlite::Connection,
    payload: &RepositoryPayload,
) -> AppResult<RepositorySourcePreview> {
    let mut result = RepositorySourcePreview {
        source_instance_id: payload.source_instance_id.clone(),
        exported_at: payload.exported_at.clone(),
        item_count: 0,
        new_items: 0,
        unchanged_items: 0,
        conflicts: 0,
        tombstones: payload
            .tables
            .get("item_tombstones")
            .map_or(0, |rows| rows.len() as u64),
    };
    for row in payload.tables.get("items").into_iter().flatten() {
        result.item_count += 1;
        let id = row_text(row, "id")?;
        let incoming_hash = row_checksum(row)?;
        let already_seen = connection
            .query_row(
                "SELECT content_hash FROM repository_seen_revisions
                 WHERE source_instance_id = ?1 AND item_id = ?2",
                params![payload.source_instance_id, id],
                |record| record.get::<_, String>(0),
            )
            .optional()?;
        if already_seen.as_deref() == Some(incoming_hash.as_str()) {
            result.unchanged_items += 1;
            continue;
        }
        let local = local_item_state(connection, id)?;
        match local {
            None => result.new_items += 1,
            Some((_, local_hash)) if local_hash == incoming_hash => result.unchanged_items += 1,
            Some(_) => result.conflicts += 1,
        }
    }
    Ok(result)
}

fn merge_package(
    transaction: &Transaction<'_>,
    payload: &RepositoryPayload,
    now: DateTime<Utc>,
    counts: &mut MergeCounts,
) -> AppResult<()> {
    validate_package_columns(transaction, payload)?;
    let category_map = merge_taxonomy(
        transaction,
        "categories",
        payload.tables.get("categories").map_or(&[], Vec::as_slice),
    )?;
    let tag_map = merge_taxonomy(
        transaction,
        "tags",
        payload.tables.get("tags").map_or(&[], Vec::as_slice),
    )?;
    let mut item_map = HashMap::<String, String>::new();
    let local_item_count =
        transaction.query_row("SELECT COUNT(*) FROM items", [], |row| row.get::<_, i64>(0))?;

    for row in payload.tables.get("items").into_iter().flatten() {
        let source_id = row_text(row, "id")?.to_string();
        let revision = row_i64(row, "revision")?;
        let incoming_hash = row_checksum(row)?;
        let tombstone_revision = transaction
            .query_row(
                "SELECT revision FROM item_tombstones WHERE item_id = ?1",
                [&source_id],
                |record| record.get::<_, i64>(0),
            )
            .optional()?;
        if tombstone_revision.is_some_and(|value| value >= revision) {
            counts.unchanged += 1;
            continue;
        }
        let local = local_item_state(transaction, &source_id)?;
        let seen = transaction
            .query_row(
                "SELECT revision, content_hash, destination_item_id FROM repository_seen_revisions
                 WHERE source_instance_id = ?1 AND item_id = ?2",
                params![payload.source_instance_id, source_id],
                |record| {
                    Ok((
                        record.get::<_, i64>(0)?,
                        record.get::<_, String>(1)?,
                        record.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .optional()?;
        let already_merged = seen.as_ref().is_some_and(|(seen_revision, seen_hash, _)| {
            *seen_revision == revision && seen_hash == &incoming_hash
        });
        let destination_id = if already_merged {
            counts.unchanged += 1;
            seen.as_ref()
                .and_then(|(_, _, destination)| destination.clone())
        } else {
            Some(match local {
                None => {
                    let mut imported = row.clone();
                    remap_category(&mut imported, &category_map)?;
                    insert_row(transaction, "items", &imported, false)?;
                    counts.imported += 1;
                    source_id.clone()
                }
                Some((_, local_hash)) if local_hash == incoming_hash => {
                    counts.unchanged += 1;
                    source_id.clone()
                }
                Some((local_revision, _))
                    if seen
                        .as_ref()
                        .is_some_and(|(seen_revision, _, _)| *seen_revision == local_revision)
                        && revision > local_revision =>
                {
                    let mut imported = row.clone();
                    remap_category(&mut imported, &category_map)?;
                    update_row(transaction, "items", "id", &source_id, &imported)?;
                    counts.imported += 1;
                    source_id.clone()
                }
                Some(_) => {
                    let conflict_id = Uuid::new_v4().to_string();
                    let mut imported = row.clone();
                    imported.insert("id".into(), JsonValue::String(conflict_id.clone()));
                    let title = row_text(row, "title")?;
                    imported.insert(
                        "title".into(),
                        JsonValue::String(format!("{title}（冲突副本）")),
                    );
                    imported.insert("revision".into(), JsonValue::from(1));
                    imported.insert("updated_at".into(), JsonValue::String(now.to_rfc3339()));
                    remap_category(&mut imported, &category_map)?;
                    insert_row(transaction, "items", &imported, false)?;
                    transaction.execute(
                        "INSERT INTO merge_conflicts
                       (id, original_item_id, conflict_item_id, source_instance_id, detected_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                        params![
                            Uuid::new_v4().to_string(),
                            source_id,
                            conflict_id,
                            payload.source_instance_id,
                            now.to_rfc3339()
                        ],
                    )?;
                    counts.conflicts += 1;
                    conflict_id
                }
            })
        };
        if let Some(destination_id) = &destination_id {
            let destination_exists = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM items WHERE id = ?1)",
                [destination_id],
                |record| record.get::<_, bool>(0),
            )?;
            if destination_exists {
                item_map.insert(source_id.clone(), destination_id.clone());
            }
        }
        transaction.execute(
            "INSERT INTO repository_seen_revisions
               (source_instance_id, item_id, revision, content_hash, destination_item_id, seen_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(source_instance_id, item_id) DO UPDATE SET
               revision = excluded.revision, content_hash = excluded.content_hash,
               destination_item_id = excluded.destination_item_id,
               seen_at = excluded.seen_at",
            params![
                payload.source_instance_id,
                source_id,
                revision,
                incoming_hash,
                destination_id,
                now.to_rfc3339()
            ],
        )?;
    }

    merge_item_tags(
        transaction,
        payload.tables.get("item_tags").map_or(&[], Vec::as_slice),
        &item_map,
        &tag_map,
    )?;
    for table in [
        "reminder_events",
        "classification_reminder_events",
        "item_events",
    ] {
        merge_item_history(
            transaction,
            table,
            payload.tables.get(table).map_or(&[], Vec::as_slice),
            &item_map,
        )?;
    }
    merge_tombstones(
        transaction,
        payload
            .tables
            .get("item_tombstones")
            .map_or(&[], Vec::as_slice),
        &payload.source_instance_id,
        now,
        counts,
    )?;
    if local_item_count == 0 {
        merge_singleton_settings(transaction, payload)?;
    }
    merge_email_rules(transaction, payload, now)?;
    Ok(())
}

fn validate_package_columns(
    transaction: &Transaction<'_>,
    payload: &RepositoryPayload,
) -> AppResult<()> {
    for (table, rows) in &payload.tables {
        let local_columns = table_columns(transaction, table)?;
        if rows
            .iter()
            .flat_map(|row| row.keys())
            .any(|column| !local_columns.contains(column))
        {
            return Err(AppError::Validation(
                "个人仓库中的数据字段暂不兼容，归并已停止".into(),
            ));
        }
    }
    Ok(())
}

fn merge_taxonomy(
    transaction: &Transaction<'_>,
    table: &str,
    rows: &[PackageRow],
) -> AppResult<HashMap<String, String>> {
    let mut mapping = HashMap::new();
    for row in rows {
        let source_id = row_text(row, "id")?.to_string();
        let name = row_text(row, "name")?;
        let existing = transaction
            .query_row(
                &format!("SELECT id FROM {table} WHERE id = ?1 OR name = ?2 LIMIT 1"),
                params![source_id, name],
                |record| record.get::<_, String>(0),
            )
            .optional()?;
        let destination = if let Some(existing) = existing {
            existing
        } else {
            insert_row(transaction, table, row, true)?;
            source_id.clone()
        };
        mapping.insert(source_id, destination);
    }
    Ok(mapping)
}

fn remap_category(row: &mut PackageRow, mapping: &HashMap<String, String>) -> AppResult<()> {
    if let Some(JsonValue::String(source)) = row.get("category_id") {
        let destination = mapping.get(source).ok_or_else(|| {
            AppError::Validation("数据包中的事项类型关系不完整，归并已停止".into())
        })?;
        row.insert("category_id".into(), JsonValue::String(destination.clone()));
    }
    Ok(())
}

fn merge_item_tags(
    transaction: &Transaction<'_>,
    rows: &[PackageRow],
    item_map: &HashMap<String, String>,
    tag_map: &HashMap<String, String>,
) -> AppResult<()> {
    for row in rows {
        let source_item = row_text(row, "item_id")?;
        let source_tag = row_text(row, "tag_id")?;
        let (Some(item_id), Some(tag_id)) = (item_map.get(source_item), tag_map.get(source_tag))
        else {
            continue;
        };
        transaction.execute(
            "INSERT OR IGNORE INTO item_tags (item_id, tag_id) VALUES (?1, ?2)",
            params![item_id, tag_id],
        )?;
    }
    Ok(())
}

fn merge_item_history(
    transaction: &Transaction<'_>,
    table: &str,
    rows: &[PackageRow],
    item_map: &HashMap<String, String>,
) -> AppResult<()> {
    for row in rows {
        let source_item = row_text(row, "item_id")?;
        let Some(destination_item) = item_map.get(source_item) else {
            continue;
        };
        let mut imported = row.clone();
        imported.insert(
            "item_id".into(),
            JsonValue::String(destination_item.clone()),
        );
        if destination_item != source_item {
            imported.insert("id".into(), JsonValue::String(Uuid::new_v4().to_string()));
            if imported.contains_key("idempotency_key") {
                imported.insert(
                    "idempotency_key".into(),
                    JsonValue::String(format!("conflict:{destination_item}:{}", Uuid::new_v4())),
                );
            }
        }
        insert_row(transaction, table, &imported, true)?;
    }
    Ok(())
}

fn merge_tombstones(
    transaction: &Transaction<'_>,
    rows: &[PackageRow],
    source_instance: &str,
    now: DateTime<Utc>,
    counts: &mut MergeCounts,
) -> AppResult<()> {
    for row in rows {
        let item_id = row_text(row, "item_id")?;
        let revision = row_i64(row, "revision")?;
        let seen = transaction
            .query_row(
                "SELECT revision FROM repository_seen_revisions
                 WHERE source_instance_id = ?1 AND item_id = ?2",
                params![source_instance, item_id],
                |record| record.get::<_, i64>(0),
            )
            .optional()?;
        let local_revision = transaction
            .query_row(
                "SELECT revision FROM items WHERE id = ?1",
                [item_id],
                |record| record.get::<_, i64>(0),
            )
            .optional()?;
        if local_revision.is_some_and(|local| seen.is_some_and(|known| local <= known)) {
            transaction.execute("DELETE FROM items WHERE id = ?1", [item_id])?;
            counts.tombstones += 1;
        }
        transaction.execute(
            "INSERT INTO item_tombstones (item_id, revision, deleted_at, source_instance_id)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(item_id) DO UPDATE SET
               revision = MAX(revision, excluded.revision),
               deleted_at = CASE WHEN excluded.revision >= revision THEN excluded.deleted_at ELSE deleted_at END,
               source_instance_id = CASE WHEN excluded.revision >= revision THEN excluded.source_instance_id ELSE source_instance_id END",
            params![item_id, revision, now.to_rfc3339(), source_instance],
        )?;
    }
    Ok(())
}

fn merge_singleton_settings(
    transaction: &Transaction<'_>,
    payload: &RepositoryPayload,
) -> AppResult<()> {
    if let Some(row) = payload
        .tables
        .get("app_settings")
        .and_then(|rows| rows.first())
    {
        update_row(transaction, "app_settings", "id", "1", row)?;
    }
    if let Some(row) = payload.tables.get("drafts").and_then(|rows| rows.first()) {
        let has_draft = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM drafts WHERE id = 1)",
            [],
            |record| record.get::<_, bool>(0),
        )?;
        if !has_draft {
            insert_row(transaction, "drafts", row, true)?;
        }
    }
    for row in payload
        .tables
        .get("event_kind_defaults")
        .into_iter()
        .flatten()
    {
        let kind = row_text(row, "event_kind")?;
        update_row(transaction, "event_kind_defaults", "event_kind", kind, row)?;
    }
    Ok(())
}

fn merge_email_rules(
    transaction: &Transaction<'_>,
    payload: &RepositoryPayload,
    now: DateTime<Utc>,
) -> AppResult<()> {
    for row in payload
        .tables
        .get("email_delivery_rules")
        .into_iter()
        .flatten()
    {
        let mut imported = row.clone();
        imported.insert("enabled".into(), JsonValue::from(0));
        imported.insert("updated_at".into(), JsonValue::String(now.to_rfc3339()));
        insert_row(transaction, "email_delivery_rules", &imported, true)?;
    }
    Ok(())
}

fn local_item_state(
    connection: &rusqlite::Connection,
    item_id: &str,
) -> AppResult<Option<(i64, String)>> {
    let revision = connection
        .query_row(
            "SELECT revision FROM items WHERE id = ?1",
            [item_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    let Some(revision) = revision else {
        return Ok(None);
    };
    let row =
        export_single_row(connection, "items", "id", item_id)?.ok_or(AppError::ItemNotFound)?;
    Ok(Some((revision, row_checksum(&row)?)))
}

fn export_single_row(
    connection: &rusqlite::Connection,
    table: &str,
    key: &str,
    value: &str,
) -> AppResult<Option<PackageRow>> {
    let columns = table_columns(connection, table)?;
    if !columns.contains(&key.to_string()) {
        return Err(AppError::Validation("数据包主键无效".into()));
    }
    let sql = format!(
        "SELECT {} FROM {table} WHERE {key} = ?1",
        columns.join(", ")
    );
    connection
        .query_row(&sql, [value], |row| {
            let mut result = BTreeMap::new();
            for (index, column) in columns.iter().enumerate() {
                result.insert(column.clone(), sqlite_value(row.get_ref(index)?));
            }
            Ok(result)
        })
        .optional()
        .map_err(AppError::from)
}

fn insert_row(
    transaction: &Transaction<'_>,
    table: &str,
    row: &PackageRow,
    ignore_conflict: bool,
) -> AppResult<()> {
    let columns = row.keys().cloned().collect::<Vec<_>>();
    let values = columns
        .iter()
        .map(|column| json_to_sql(row.get(column).expect("row column exists")))
        .collect::<AppResult<Vec<_>>>()?;
    let placeholders = (1..=columns.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let prefix = if ignore_conflict {
        "INSERT OR IGNORE"
    } else {
        "INSERT"
    };
    transaction.execute(
        &format!(
            "{prefix} INTO {table} ({}) VALUES ({placeholders})",
            columns.join(", ")
        ),
        params_from_iter(values),
    )?;
    Ok(())
}

fn update_row(
    transaction: &Transaction<'_>,
    table: &str,
    key: &str,
    key_value: &str,
    row: &PackageRow,
) -> AppResult<()> {
    let columns = row
        .keys()
        .filter(|column| column.as_str() != key)
        .cloned()
        .collect::<Vec<_>>();
    let mut values = columns
        .iter()
        .map(|column| json_to_sql(row.get(column).expect("row column exists")))
        .collect::<AppResult<Vec<_>>>()?;
    values.push(rusqlite::types::Value::Text(key_value.to_string()));
    let assignments = columns
        .iter()
        .enumerate()
        .map(|(index, column)| format!("{column} = ?{}", index + 1))
        .collect::<Vec<_>>()
        .join(", ");
    transaction.execute(
        &format!(
            "UPDATE {table} SET {assignments} WHERE {key} = ?{}",
            columns.len() + 1
        ),
        params_from_iter(values),
    )?;
    Ok(())
}

fn json_to_sql(value: &JsonValue) -> AppResult<rusqlite::types::Value> {
    match value {
        JsonValue::Null => Ok(rusqlite::types::Value::Null),
        JsonValue::Bool(value) => Ok(rusqlite::types::Value::Integer(i64::from(*value))),
        JsonValue::Number(value) => value
            .as_i64()
            .map(rusqlite::types::Value::Integer)
            .or_else(|| value.as_f64().map(rusqlite::types::Value::Real))
            .ok_or_else(|| AppError::Validation("数据包数值无效".into())),
        JsonValue::String(value) => Ok(rusqlite::types::Value::Text(value.clone())),
        JsonValue::Array(_) | JsonValue::Object(_) => {
            Err(AppError::Validation("数据包字段类型无效".into()))
        }
    }
}

fn row_text<'a>(row: &'a PackageRow, key: &str) -> AppResult<&'a str> {
    row.get(key)
        .and_then(JsonValue::as_str)
        .ok_or_else(|| AppError::Validation("数据包缺少必要文字字段".into()))
}

fn row_i64(row: &PackageRow, key: &str) -> AppResult<i64> {
    row.get(key)
        .and_then(JsonValue::as_i64)
        .ok_or_else(|| AppError::Validation("数据包缺少必要数值字段".into()))
}

fn row_checksum(row: &PackageRow) -> AppResult<String> {
    let bytes = serde_json::to_vec(row)?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

fn instance_id(connection: &rusqlite::Connection) -> AppResult<String> {
    connection
        .query_row(
            "SELECT instance_id FROM app_instance WHERE id = 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .map_err(AppError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::CreateItemInput;
    use chrono::Duration;

    fn add_item(database: &Database, title: &str, now: DateTime<Utc>) -> String {
        database
            .create_item(
                &CreateItemInput {
                    title: title.into(),
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
            .unwrap()
            .id
    }

    #[test]
    fn repository_package_round_trip_imports_without_opening_remote_sqlite() {
        let directory = tempfile::tempdir().unwrap();
        let source = Database::open(&directory.path().join("source.db")).unwrap();
        let target = Database::open(&directory.path().join("target.db")).unwrap();
        let now = Utc::now();
        add_item(&source, "跨平台数据包", now);
        configure_repository(&source, directory.path(), now).unwrap();
        sync_repository(&source, now).unwrap();
        configure_repository(&target, directory.path(), now).unwrap();

        let preview = preview_repository(&target).unwrap();
        assert_eq!(preview.new_items, 1);
        let result = sync_repository(&target, now + Duration::seconds(1)).unwrap();
        assert_eq!(result.imported_items, 1);
        assert_eq!(target.list_items("all", now).unwrap().len(), 1);
    }

    #[test]
    fn configuring_repository_immediately_writes_this_devices_snapshot() {
        let directory = tempfile::tempdir().unwrap();
        let database = Database::open(&directory.path().join("device.db")).unwrap();
        let now = Utc::now();
        add_item(&database, "立即进入仓库", now);

        let status = configure_repository(&database, directory.path(), now).unwrap();

        assert!(status.snapshot_ready);
        assert_eq!(status.package_count, 1);
        assert_eq!(status.local_item_count, 1);
        assert!(status.last_package_at.is_some());
    }

    #[test]
    fn divergent_same_id_creates_a_conflict_copy_instead_of_overwriting() {
        let directory = tempfile::tempdir().unwrap();
        let source = Database::open(&directory.path().join("source.db")).unwrap();
        let target = Database::open(&directory.path().join("target.db")).unwrap();
        let now = Utc::now();
        let item_id = add_item(&source, "来源版本", now);
        let source_item = source.get_item(&item_id).unwrap();
        let target_connection = target.connection.lock().unwrap();
        let source_connection = source.connection.lock().unwrap();
        let mut row = export_single_row(&source_connection, "items", "id", &item_id)
            .unwrap()
            .unwrap();
        row.insert("title".into(), JsonValue::String("本机版本".into()));
        let transaction = target_connection.unchecked_transaction().unwrap();
        insert_row(&transaction, "items", &row, false).unwrap();
        transaction.commit().unwrap();
        drop(source_connection);
        drop(target_connection);
        assert_eq!(source_item.title, "来源版本");

        configure_repository(&source, directory.path(), now).unwrap();
        sync_repository(&source, now).unwrap();
        configure_repository(&target, directory.path(), now).unwrap();
        let result = sync_repository(&target, now + Duration::seconds(1)).unwrap();
        assert_eq!(result.conflicts, 1);
        let items = target.list_items("all", now).unwrap();
        assert_eq!(items.len(), 2);
        assert!(items.iter().any(|item| item.title.contains("冲突副本")));

        let repeated_preview = preview_repository(&target).unwrap();
        assert_eq!(repeated_preview.conflicts, 0);
        assert_eq!(repeated_preview.unchanged_items, 1);
        let repeated = sync_repository(&target, now + Duration::seconds(2)).unwrap();
        assert_eq!(repeated.conflicts, 0);
        assert_eq!(target.list_items("all", now).unwrap().len(), 2);
    }

    #[test]
    fn tampered_package_is_rejected_before_merge() {
        let directory = tempfile::tempdir().unwrap();
        let database = Database::open(&directory.path().join("data.db")).unwrap();
        let now = Utc::now();
        configure_repository(&database, directory.path(), now).unwrap();
        let result = sync_repository(&database, now).unwrap();
        let package_path = result.package_path.unwrap();
        let mut bytes = fs::read(&package_path).unwrap();
        let index = bytes.len() / 2;
        bytes[index] ^= 1;
        fs::write(&package_path, bytes).unwrap();
        let error = preview_repository(&database).unwrap_err();
        assert!(error.to_string().contains("数据包"));
    }
}
