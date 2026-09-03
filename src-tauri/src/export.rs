//! Read-only, versioned tabular projection. Encoders never receive database or path access.
//! A future importer must parse this mapping into validated application commands, not SQL.
use crate::{
    applications, copying, database_backup,
    domain::ApplicationListItem,
    error::{CoreError, file_error},
    full_backup,
    warehouse::WarehouseSession,
};
use chrono::{SecondsFormat, Utc};
use rust_xlsxwriter::{Format, Workbook, XlsxError};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs, io::Write, path::Path};
use uuid::Uuid;

const MAX_ROWS: usize = 10_000;
const MAX_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Target {
    pub id: String,
    pub revision: i64,
}
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Partition {
    Active,
    Archived,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Scope {
    All {},
    Records {
        partition: Partition,
        targets: Vec<Target>,
    },
}
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FileFormat {
    Csv,
    Xlsx,
}
impl FileFormat {
    fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Xlsx => "xlsx",
        }
    }
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub version: u32,
    pub scope: Scope,
    pub columns: Vec<String>,
    pub format: FileFormat,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Column {
    pub key: String,
    pub label: String,
    pub field_type: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub version: u32,
    pub total: i64,
    pub columns: Vec<Column>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub version: u32,
    pub generated_at_utc: String,
    pub format: FileFormat,
    pub row_count: usize,
    pub columns: Vec<Column>,
    pub cell_encoding: &'static str,
    pub csv_formula_protection: &'static str,
    pub document_source: &'static str,
}
pub struct Table {
    pub manifest: Manifest,
    pub rows: Vec<Vec<String>>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Created {
    pub path: String,
    pub mapping_path: String,
    pub row_count: usize,
}

pub fn catalog(session: &WarehouseSession) -> Result<Catalog, CoreError> {
    let mut columns: Vec<Column> = [
        ("createdAtUtc", "创建时间（UTC）", "timestamp"),
        ("companyName", "公司名称", "text"),
        ("applicationDate", "投递日期", "date"),
        ("currentStageName", "投递进度", "text"),
        ("statusUpdatedAtUtc", "状态更新时间（UTC）", "timestamp"),
        ("companyType", "企业性质", "select"),
        ("industry", "行业", "text"),
        ("positionName", "岗位名称", "text"),
        ("positionCategory", "岗位类别", "text"),
        ("workLocation", "工作地点", "text"),
        ("documentNames", "简历文件（含缺失标记）", "text"),
        ("applicationUrl", "投递链接", "url"),
        ("positionDescription", "岗位介绍", "text"),
        ("notes", "备注", "text"),
        ("tags", "标签", "text"),
        ("announcementUrl", "公告链接", "url"),
        ("companyUrl", "公司网址", "url"),
        ("positionUrl", "岗位网址", "url"),
        ("id", "记录 ID", "text"),
        ("currentStateName", "辅助状态", "text"),
        ("updatedAtUtc", "内容更新时间（UTC）", "timestamp"),
        ("archivedAtUtc", "归档时间（UTC）", "timestamp"),
        ("folderRelativePath", "投递文件夹（相对路径）", "text"),
        ("documentPaths", "简历绝对路径（含缺失标记）", "text"),
    ]
    .into_iter()
    .map(|(key, label, kind)| Column {
        key: key.into(),
        label: label.into(),
        field_type: kind.into(),
    })
    .collect();
    for field in applications::list_field_definitions(session)? {
        columns.push(Column {
            key: format!("custom:{}", field.id),
            label: field.display_name,
            field_type: field.field_type,
        });
    }
    let total = session
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM applications WHERE deleted_at_utc IS NULL",
            [],
            |r| r.get(0),
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    Ok(Catalog {
        version: 1,
        total,
        columns,
    })
}

pub fn project(session: &WarehouseSession, request: &Request) -> Result<Table, CoreError> {
    if request.version != 1 || request.columns.is_empty() {
        return Err(CoreError::Validation);
    }
    if request.columns.len() > 256 {
        return Err(CoreError::ExportLimit);
    }
    let tx = session
        .connection()
        .unchecked_transaction()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    let catalog = catalog(session)?;
    let mut seen = HashSet::new();
    let columns = request
        .columns
        .iter()
        .map(|key| {
            if !seen.insert(key) {
                return Err(CoreError::Validation);
            }
            catalog
                .columns
                .iter()
                .find(|c| c.key == *key)
                .cloned()
                .ok_or(CoreError::Validation)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let targets = match &request.scope {
        Scope::All {} => {
            if catalog.total > MAX_ROWS as i64 {
                return Err(CoreError::ExportLimit);
            }
            let mut statement = tx.prepare("SELECT id,revision FROM applications WHERE deleted_at_utc IS NULL ORDER BY created_at_utc DESC,id").map_err(|_|CoreError::DatabaseInvalid)?;
            statement
                .query_map([], |r| {
                    Ok(Target {
                        id: r.get(0)?,
                        revision: r.get(1)?,
                    })
                })
                .map_err(|_| CoreError::DatabaseInvalid)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| CoreError::DatabaseInvalid)?
        }
        Scope::Records { targets, .. } => {
            if targets.len() > MAX_ROWS {
                return Err(CoreError::ExportLimit);
            }
            targets.clone()
        }
    };
    let mut ids = HashSet::new();
    let mut rows = Vec::new();
    let mut size = 0;
    for target in targets {
        if !ids.insert(target.id.clone()) {
            return Err(CoreError::Validation);
        }
        let record = applications::load_record(&tx, &target.id)?;
        let wrong_partition = match &request.scope {
            Scope::All {} => false,
            Scope::Records { partition, .. } => {
                record.archived_at_utc.is_some() != matches!(partition, Partition::Archived)
            }
        };
        if record.deleted_at_utc.is_some() || record.revision != target.revision || wrong_partition
        {
            return Err(CoreError::RevisionConflict);
        }
        let values = serde_json::to_value(&record).map_err(|_| CoreError::DatabaseInvalid)?;
        let mut row = Vec::new();
        for column in &columns {
            let value = cell(session, &record, &values, &column.key)?;
            size += value.len();
            if size > MAX_BYTES
                || (matches!(request.format, FileFormat::Xlsx)
                    && value.encode_utf16().count() > 32767)
            {
                return Err(CoreError::ExportLimit);
            }
            row.push(value);
        }
        rows.push(row);
    }
    tx.rollback().map_err(|_| CoreError::DatabaseInvalid)?;
    Ok(Table {
        manifest: Manifest {
            version: 1,
            generated_at_utc: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            format: request.format,
            row_count: rows.len(),
            columns,
            cell_encoding: "text; timestamps=UTC RFC3339; empty=null-or-empty; multi-value=newline",
            csv_formula_protection: "prefix apostrophe for leading control/whitespace or =,+,-,@; XLSX uses literal strings",
            document_source: "last committed index, including missing markers; no file content or live rescan",
        },
        rows,
    })
}

fn cell(
    session: &WarehouseSession,
    r: &ApplicationListItem,
    values: &serde_json::Value,
    key: &str,
) -> Result<String, CoreError> {
    if let Some(id) = key.strip_prefix("custom:") {
        return Ok(value_text(
            r.custom_fields.get(id).unwrap_or(&serde_json::Value::Null),
        ));
    }
    Ok(match key {
        "companyType" => match r.company_type.as_str() {
            "stateOwned" => "央国企",
            "private" => "民企",
            "foreign" => "外企",
            "bank" => "银行",
            _ => "未分类",
        }
        .into(),
        "currentStageName" => format!(
            "{}{} · {}",
            if r.current_stage_state == "failed" {
                "已挂 · "
            } else {
                ""
            },
            r.current_stage_name,
            r.current_state_name
        ),
        "tags" => r
            .tags
            .iter()
            .map(|t| t.name.clone())
            .collect::<Vec<_>>()
            .join("\n"),
        "documentNames" | "documentPaths" => {
            let mut statement = session.connection().prepare("SELECT display_name,relative_path,missing_at_utc IS NOT NULL FROM documents WHERE application_id=?1 ORDER BY relative_path,id").map_err(|_|CoreError::DatabaseInvalid)?;
            let documents = statement
                .query_map([&r.id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, bool>(2)?,
                    ))
                })
                .map_err(|_| CoreError::DatabaseInvalid)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| CoreError::DatabaseInvalid)?;
            documents
                .into_iter()
                .map(|(name, path, missing)| {
                    let text = if key == "documentPaths" {
                        if !crate::backup_archive::valid_path(&path) {
                            return Err(CoreError::UnsafePath);
                        }
                        let root = r.folder_relative_path.replace('\\', "/");
                        if !crate::backup_archive::valid_path(&root)
                            || !root.starts_with("applications/")
                        {
                            return Err(CoreError::UnsafePath);
                        }
                        session
                            .root()
                            .join(root)
                            .join(path)
                            .to_string_lossy()
                            .into_owned()
                    } else {
                        name
                    };
                    Ok(format!("{text}{}", if missing { " [缺失]" } else { "" }))
                })
                .collect::<Result<Vec<_>, CoreError>>()?
                .join("\n")
        }
        _ => value_text(&values[key]),
    })
}
fn value_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(s) => s.clone(),
        _ => value.to_string(),
    }
}

fn csv_cell(value: &str) -> String {
    let protect = value.starts_with(|c: char| {
        c.is_control()
            || c.is_whitespace()
            || matches!(c, '=' | '+' | '-' | '@' | '\u{feff}' | '\u{200b}')
    });
    format!(
        "\"{}{}\"",
        if protect { "'" } else { "" },
        value.replace('"', "\"\"")
    )
}
pub fn csv(table: &Table) -> Vec<u8> {
    let mut text = String::from("\u{feff}");
    let headers = table
        .manifest
        .columns
        .iter()
        .map(|c| c.label.clone())
        .collect::<Vec<_>>();
    for row in std::iter::once(&headers).chain(&table.rows) {
        text.push_str(
            &row.iter()
                .map(|v| csv_cell(v))
                .collect::<Vec<_>>()
                .join(","),
        );
        text.push_str("\r\n");
    }
    text.into_bytes()
}
pub fn xlsx(table: &Table) -> Result<Vec<u8>, CoreError> {
    xlsx_inner(table).map_err(|_| CoreError::ExportEncoding)
}
fn xlsx_inner(table: &Table) -> Result<Vec<u8>, XlsxError> {
    let mut workbook = Workbook::new();
    let header = Format::new().set_bold().set_background_color("#DCE8FA");
    let body = Format::new().set_text_wrap().set_num_format("@");
    let sheet = workbook.add_worksheet();
    sheet.set_name("投递记录")?;
    sheet.set_freeze_panes(1, 0)?;
    for (col, column) in table.manifest.columns.iter().enumerate() {
        sheet.set_column_width(col as u16, 24)?;
        sheet.write_string_with_format(0, col as u16, &column.label, &header)?;
    }
    for (row, values) in table.rows.iter().enumerate() {
        for (col, value) in values.iter().enumerate() {
            sheet.write_string_with_format(row as u32 + 1, col as u16, value, &body)?;
        }
    }
    if !table.rows.is_empty() {
        sheet.autofilter(
            0,
            0,
            table.rows.len() as u32,
            table.manifest.columns.len() as u16 - 1,
        )?;
    }
    let mapping = workbook.add_worksheet();
    mapping.set_name("字段映射")?;
    mapping.write_string(
        0,
        0,
        "OfferTrack export version 1; cells are literal text; not a full backup",
    )?;
    for (col, label) in ["列序（从 1 开始）", "稳定键", "显示名称", "业务类型"]
        .iter()
        .enumerate()
    {
        mapping.write_string_with_format(1, col as u16, *label, &header)?;
        mapping.set_column_width(col as u16, 28)?;
    }
    for (index, c) in table.manifest.columns.iter().enumerate() {
        let row = index as u32 + 2;
        mapping.write_number(row, 0, (index + 1) as f64)?;
        mapping.write_string(row, 1, &c.key)?;
        mapping.write_string(row, 2, &c.label)?;
        mapping.write_string(row, 3, &c.field_type)?;
    }
    workbook.save_to_buffer()
}

pub fn create(
    session: &WarehouseSession,
    parent: &Path,
    request: &Request,
) -> Result<Created, CoreError> {
    let (parent, _ancestors) = full_backup::outside_parent(parent, Some(session.root()))?;
    let table = project(session, request)?;
    let bytes = match request.format {
        FileFormat::Csv => csv(&table),
        FileFormat::Xlsx => xlsx(&table)?,
    };
    let manifest =
        serde_json::to_vec_pretty(&table.manifest).map_err(|_| CoreError::ExportEncoding)?;
    let id = Uuid::new_v4();
    let staging = parent.join(format!(".offertrack-export-{id}"));
    let target = parent.join(format!("OfferTrack-export-{id}"));
    fs::create_dir(&staging).map_err(file_error)?;
    let identity = copying::directory_identity(&staging)?;
    let guard = database_backup::open_guard(&staging, true)?;
    let name = format!("applications.{}", request.format.extension());
    for (name, bytes) in [
        (name.as_str(), bytes.as_slice()),
        ("fields.json", manifest.as_slice()),
    ] {
        let mut file = full_backup::new_output(&staging.join(name), false)?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(file_error)?;
    }
    drop(guard);
    copying::rename_no_replace(&staging, &target, &identity)?;
    Ok(Created {
        path: target.join(name).to_string_lossy().into_owned(),
        mapping_path: target.join("fields.json").to_string_lossy().into_owned(),
        row_count: table.rows.len(),
    })
}

#[cfg(test)]
#[path = "export_tests.rs"]
mod tests;
