use std::{collections::BTreeMap, ffi::OsStr, path::Path};

use chrono::{Local, NaiveDate, SecondsFormat, Utc};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    auxiliary_states::{self, Owner},
    copying::PendingCreation,
    domain::{
        ApplicationDetail, ApplicationListItem, ApplicationScope, ChangeStageRequest,
        ClaimFolderRequest, CreateApplicationRequest, DocumentEntry, DuplicateMode,
        DuplicatePreview, FieldDefinition, FieldDefinitionRequest, InterviewRound,
        InterviewRoundRequest, Tag, UpdateApplicationRequest, WorkflowEvent, WorkflowStage,
        WorkflowStageRequest, WorkflowTemplate,
    },
    error::CoreError,
    filesystem, recycle_bin,
    warehouse::WarehouseSession,
    workflows,
};

const VALID_COMPANY_TYPES: &[&str] = &["stateOwned", "private", "foreign", "bank", "uncategorized"];
const VALID_FIELD_TYPES: &[&str] = &["text", "number", "date", "boolean", "url", "select"];

pub fn list(
    session: &WarehouseSession,
    scope: ApplicationScope,
) -> Result<Vec<ApplicationListItem>, CoreError> {
    let condition = match scope {
        ApplicationScope::Active => "a.deleted_at_utc IS NULL AND a.archived_at_utc IS NULL",
        ApplicationScope::Archived => "a.deleted_at_utc IS NULL AND a.archived_at_utc IS NOT NULL",
        ApplicationScope::Trash => "a.deleted_at_utc IS NOT NULL",
    };
    let sql = format!(
        "SELECT a.id, a.short_id, a.created_at_utc, a.application_date,
                a.company_name, a.company_type, a.industry, a.position_name,
                a.position_category, a.work_location, a.application_url,
                a.announcement_url, a.company_url, a.position_url,
                a.position_description, a.notes, a.folder_relative_path,
                a.folder_normalization_pending, a.current_stage_id,
                COALESCE(s.display_name, '准备投递'), a.current_stage_state,
                COALESCE(s.display_order, 0), COALESCE(s.color, '#64748b'),
                a.status_updated_at_utc, a.updated_at_utc, a.archived_at_utc,
                a.deleted_at_utc, a.revision,
                (SELECT COUNT(*) FROM documents d
                 WHERE d.application_id = a.id AND d.missing_at_utc IS NULL)
         FROM applications a
         LEFT JOIN workflow_stages s ON s.id = a.current_stage_id
         WHERE {condition}
         ORDER BY a.created_at_utc DESC"
    );
    let connection = session.connection();
    let mut statement = connection
        .prepare(&sql)
        .map_err(|_| CoreError::DatabaseInvalid)?;
    let rows = statement
        .query_map([], map_application_row)
        .map_err(|_| CoreError::DatabaseInvalid)?;
    let mut records = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    drop(statement);
    for record in &mut records {
        (record.current_state_name, record.current_state_kind) =
            auxiliary_states::describe(connection, &record.id, &record.current_stage_state)?;
        record.current_stage_progress = workflows::progress(
            &load_stages(connection, &record.id)?,
            record.current_stage_id.as_deref(),
        );
        record.tags = load_tags(connection, &record.id)?;
        record.custom_fields = load_custom_fields(connection, &record.id)?;
        record.document_names = load_documents(connection, &record.id)?
            .into_iter()
            .filter(|document| !document.missing)
            .map(|document| document.display_name)
            .collect();
    }
    Ok(records)
}

pub fn get(session: &WarehouseSession, id: &str) -> Result<ApplicationDetail, CoreError> {
    let connection = session.connection();
    let record = load_record(connection, id)?;
    Ok(ApplicationDetail {
        auxiliary_states: auxiliary_states::load(connection, Owner::Application(id))?,
        stages: load_stages(connection, id)?,
        history: load_history(connection, id)?,
        interview_rounds: load_interview_rounds(connection, id)?,
        documents: load_documents(connection, id)?,
        record,
    })
}

pub(crate) fn load_record(
    connection: &Connection,
    id: &str,
) -> Result<ApplicationListItem, CoreError> {
    let mut record = connection
        .query_row(
            "SELECT a.id, a.short_id, a.created_at_utc, a.application_date,
                    a.company_name, a.company_type, a.industry, a.position_name,
                    a.position_category, a.work_location, a.application_url,
                    a.announcement_url, a.company_url, a.position_url,
                    a.position_description, a.notes, a.folder_relative_path,
                    a.folder_normalization_pending, a.current_stage_id,
                    COALESCE(s.display_name, '准备投递'), a.current_stage_state,
                    COALESCE(s.display_order, 0), COALESCE(s.color, '#64748b'),
                    a.status_updated_at_utc, a.updated_at_utc, a.archived_at_utc,
                    a.deleted_at_utc, a.revision,
                    (SELECT COUNT(*) FROM documents d
                     WHERE d.application_id = a.id AND d.missing_at_utc IS NULL)
             FROM applications a
             LEFT JOIN workflow_stages s ON s.id = a.current_stage_id
             WHERE a.id = ?1",
            [id],
            map_application_row,
        )
        .optional()
        .map_err(|_| CoreError::DatabaseInvalid)?
        .ok_or(CoreError::NotFound)?;
    record.tags = load_tags(connection, id)?;
    (record.current_state_name, record.current_state_kind) =
        auxiliary_states::describe(connection, id, &record.current_stage_state)?;
    record.current_stage_progress = workflows::progress(
        &load_stages(connection, id)?,
        record.current_stage_id.as_deref(),
    );
    record.custom_fields = load_custom_fields(connection, id)?;
    record.document_names = load_documents(connection, id)?
        .into_iter()
        .filter(|document| !document.missing)
        .map(|document| document.display_name)
        .collect();
    Ok(record)
}

fn map_application_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ApplicationListItem> {
    Ok(ApplicationListItem {
        id: row.get(0)?,
        short_id: row.get(1)?,
        created_at_utc: row.get(2)?,
        application_date: row.get(3)?,
        company_name: row.get(4)?,
        company_type: row.get(5)?,
        industry: row.get(6)?,
        position_name: row.get(7)?,
        position_category: row.get(8)?,
        work_location: row.get(9)?,
        application_url: row.get(10)?,
        announcement_url: row.get(11)?,
        company_url: row.get(12)?,
        position_url: row.get(13)?,
        position_description: row.get(14)?,
        notes: row.get(15)?,
        folder_relative_path: row.get(16)?,
        folder_normalization_pending: row.get::<_, i64>(17)? != 0,
        current_stage_id: row.get(18)?,
        current_stage_name: row.get(19)?,
        current_stage_state: row.get(20)?,
        current_state_name: String::new(),
        current_state_kind: None,
        current_stage_order: row.get(21)?,
        current_stage_progress: 0,
        current_stage_color: row.get(22)?,
        status_updated_at_utc: row.get(23)?,
        updated_at_utc: row.get(24)?,
        archived_at_utc: row.get(25)?,
        deleted_at_utc: row.get(26)?,
        revision: row.get(27)?,
        tags: Vec::new(),
        document_count: row.get(28)?,
        document_names: Vec::new(),
        custom_fields: BTreeMap::new(),
    })
}

fn load_tags(connection: &Connection, application_id: &str) -> Result<Vec<Tag>, CoreError> {
    let mut statement = connection
        .prepare(
            "SELECT t.id, t.name, t.color, t.scope
             FROM tags t
             JOIN application_tags at ON at.tag_id = t.id
             WHERE at.application_id = ?1
             ORDER BY at.display_order, t.name",
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    statement
        .query_map([application_id], |row| {
            Ok(Tag {
                id: row.get(0)?,
                name: row.get(1)?,
                color: row.get(2)?,
                scope: row.get(3)?,
            })
        })
        .map_err(|_| CoreError::DatabaseInvalid)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| CoreError::DatabaseInvalid)
}

fn load_custom_fields(
    connection: &Connection,
    application_id: &str,
) -> Result<BTreeMap<String, Value>, CoreError> {
    let mut statement = connection
        .prepare(
            "SELECT field_definition_id, value_json
             FROM field_values WHERE application_id = ?1",
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    let rows = statement
        .query_map([application_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|_| CoreError::DatabaseInvalid)?;
    let mut values = BTreeMap::new();
    for row in rows {
        let (id, json) = row.map_err(|_| CoreError::DatabaseInvalid)?;
        values.insert(
            id,
            serde_json::from_str(&json).map_err(|_| CoreError::DatabaseInvalid)?,
        );
    }
    Ok(values)
}

pub(crate) fn load_stages(
    connection: &Connection,
    application_id: &str,
) -> Result<Vec<WorkflowStage>, CoreError> {
    let mut statement = connection
        .prepare(
            "SELECT id, stable_key, display_name, stage_kind, display_order,
                    color, is_terminal, terminal_outcome
             FROM workflow_stages WHERE application_id = ?1 ORDER BY display_order",
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    statement
        .query_map([application_id], |row| {
            Ok(WorkflowStage {
                id: row.get(0)?,
                stable_key: row.get(1)?,
                display_name: row.get(2)?,
                stage_kind: row.get(3)?,
                display_order: row.get(4)?,
                color: row.get(5)?,
                is_terminal: row.get::<_, i64>(6)? != 0,
                terminal_outcome: row.get(7)?,
            })
        })
        .map_err(|_| CoreError::DatabaseInvalid)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| CoreError::DatabaseInvalid)
}

fn load_history(
    connection: &Connection,
    application_id: &str,
) -> Result<Vec<WorkflowEvent>, CoreError> {
    let mut statement = connection
        .prepare(
            "SELECT id, stage_id, stage_name_snapshot, previous_state, next_state,
                    notes, occurred_at_utc, actor_type, previous_state_name_snapshot,
                    COALESCE(next_state_name_snapshot, next_state), previous_state_kind_snapshot, next_state_kind_snapshot
             FROM workflow_events WHERE application_id = ?1
             ORDER BY occurred_at_utc DESC, rowid DESC",
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    statement
        .query_map([application_id], |row| {
            Ok(WorkflowEvent {
                id: row.get(0)?,
                stage_id: row.get(1)?,
                stage_name_snapshot: row.get(2)?,
                previous_state: row.get(3)?,
                next_state: row.get(4)?,
                notes: row.get(5)?,
                occurred_at_utc: row.get(6)?,
                actor_type: row.get(7)?,
                previous_state_name_snapshot: row.get(8)?,
                next_state_name_snapshot: row.get(9)?,
                previous_state_kind_snapshot: row.get(10)?,
                next_state_kind_snapshot: row.get(11)?,
            })
        })
        .map_err(|_| CoreError::DatabaseInvalid)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| CoreError::DatabaseInvalid)
}

fn load_interview_rounds(
    connection: &Connection,
    application_id: &str,
) -> Result<Vec<InterviewRound>, CoreError> {
    let mut statement = connection
        .prepare(
            "SELECT id, sequence_number, display_name, state, scheduled_at_utc,
                    completed_at_utc, result, notes
             FROM interview_rounds WHERE application_id = ?1 ORDER BY sequence_number",
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    statement
        .query_map([application_id], |row| {
            Ok(InterviewRound {
                id: row.get(0)?,
                sequence_number: row.get(1)?,
                display_name: row.get(2)?,
                state: row.get(3)?,
                scheduled_at_utc: row.get(4)?,
                completed_at_utc: row.get(5)?,
                result: row.get(6)?,
                notes: row.get(7)?,
            })
        })
        .map_err(|_| CoreError::DatabaseInvalid)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| CoreError::DatabaseInvalid)
}

fn load_documents(
    connection: &Connection,
    application_id: &str,
) -> Result<Vec<DocumentEntry>, CoreError> {
    let mut statement = connection
        .prepare(
            "SELECT id, relative_path, display_name, media_type, size_bytes,
                    modified_at_utc, missing_at_utc
             FROM documents WHERE application_id = ?1
             ORDER BY missing_at_utc IS NOT NULL, relative_path",
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    statement
        .query_map([application_id], |row| {
            Ok(DocumentEntry {
                id: row.get(0)?,
                relative_path: row.get(1)?,
                display_name: row.get(2)?,
                media_type: row.get(3)?,
                size_bytes: row.get(4)?,
                modified_at_utc: row.get(5)?,
                missing: row.get::<_, Option<String>>(6)?.is_some(),
            })
        })
        .map_err(|_| CoreError::DatabaseInvalid)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| CoreError::DatabaseInvalid)
}

pub fn create(
    session: &mut WarehouseSession,
    request: CreateApplicationRequest,
) -> Result<ApplicationDetail, CoreError> {
    session.connection_mut()?;
    validate_required_text(&request.company_name)?;
    validate_required_text(&request.position_name)?;
    let company_type = if request.company_type.is_empty() {
        "uncategorized".to_owned()
    } else {
        validate_company_type(&request.company_type)?;
        request.company_type.clone()
    };
    let id = Uuid::new_v4().to_string();
    let short_id = id.replace('-', "")[..6].to_ascii_uppercase();
    let created_local = Local::now();
    let now = created_local
        .with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::Millis, true);
    let offset_minutes = created_local.offset().local_minus_utc() / 60;
    let folder_name = filesystem::normalized_application_folder_name(
        &created_local.format("%Y-%m-%d_%H-%M-%S").to_string(),
        &request.company_name,
        &request.position_name,
        &short_id,
    );
    let folder_relative_path = format!("applications/{folder_name}");
    let operation = PendingCreation::begin(session, &id, &folder_relative_path, &now)?;
    if let Err(error) = operation.copy_and_verify(session, None) {
        operation
            .cancel(session)
            .map_err(|_| CoreError::CopyRecovery)?;
        return Err(error);
    }

    let result = (|| {
        let connection = session.connection_mut()?;
        let transaction = connection
            .transaction()
            .map_err(|_| CoreError::DatabaseInvalid)?;
        transaction
            .execute(
                "INSERT INTO applications (
                    id, short_id, created_at_utc, created_timezone_offset_minutes,
                    company_name, company_type, industry, position_name,
                    position_category, work_location, folder_relative_path,
                    current_stage_state, status_updated_at_utc, updated_at_utc, revision
                 ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11,
                    'pending', ?3, ?3, 1
                 )",
                params![
                    id,
                    short_id,
                    now,
                    offset_minutes,
                    request.company_name.trim(),
                    company_type,
                    request.industry.trim(),
                    request.position_name.trim(),
                    request.position_category.trim(),
                    request.work_location.trim(),
                    folder_relative_path,
                ],
            )
            .map_err(|_| CoreError::DatabaseInvalid)?;
        let initial = clone_default_workflow(&transaction, &id, &now)?;
        transaction
            .execute(
                "UPDATE applications SET current_stage_id = ?1 WHERE id = ?2",
                params![initial.id, id],
            )
            .map_err(|_| CoreError::DatabaseInvalid)?;
        transaction
            .execute(
                "INSERT INTO workflow_events (
                    id, application_id, stage_id, stage_name_snapshot,
                    previous_state, next_state, notes, occurred_at_utc, actor_type
                 ) VALUES (?1, ?2, ?3, ?4, NULL, 'pending', '', ?5, 'user')",
                params![
                    Uuid::new_v4().to_string(),
                    id,
                    initial.id,
                    initial.display_name,
                    now
                ],
            )
            .map_err(|_| CoreError::DatabaseInvalid)?;
        operation.publish(&transaction)?;
        transaction.commit().map_err(|_| CoreError::DatabaseInvalid)
    })();

    if let Err(error) = result {
        operation
            .cancel(session)
            .map_err(|_| CoreError::CopyRecovery)?;
        return Err(error);
    }
    get(session, &id)
}

fn clone_default_workflow(
    transaction: &Transaction<'_>,
    application_id: &str,
    now: &str,
) -> Result<WorkflowStage, CoreError> {
    let template_id = transaction
        .query_row(
            "SELECT id FROM workflow_templates WHERE is_default = 1 ORDER BY created_at_utc LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|_| CoreError::DatabaseInvalid)?
        .ok_or(CoreError::DatabaseInvalid)?;
    let template_stages = {
        let mut statement = transaction
            .prepare(
                "SELECT stable_key, display_name, stage_kind, display_order, color,
                        is_terminal, terminal_outcome
                 FROM workflow_stages WHERE template_id = ?1 ORDER BY display_order",
            )
            .map_err(|_| CoreError::DatabaseInvalid)?;
        statement
            .query_map([&template_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)? != 0,
                    row.get::<_, Option<String>>(6)?,
                ))
            })
            .map_err(|_| CoreError::DatabaseInvalid)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| CoreError::DatabaseInvalid)?
    };

    auxiliary_states::clone_into_new_owner(
        transaction,
        Owner::Template(&template_id),
        Owner::Application(application_id),
    )?;
    let mut initial = None;
    for (stable_key, display_name, stage_kind, display_order, color, is_terminal, outcome) in
        template_stages
    {
        let id = Uuid::new_v4().to_string();
        transaction
            .execute(
                "INSERT INTO workflow_stages (
                    id, application_id, template_id, stable_key, display_name,
                    stage_kind, display_order, color, is_terminal, terminal_outcome,
                    created_at_utc, updated_at_utc
                 ) VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
                params![
                    id,
                    application_id,
                    stable_key,
                    display_name,
                    stage_kind,
                    display_order,
                    color,
                    is_terminal,
                    outcome,
                    now,
                ],
            )
            .map_err(|_| CoreError::DatabaseInvalid)?;
        let stage = WorkflowStage {
            id,
            stable_key,
            display_name,
            stage_kind,
            display_order,
            color,
            is_terminal,
            terminal_outcome: outcome,
        };
        if stage.stable_key == "preparing" {
            initial = Some(stage);
        }
    }
    initial.ok_or(CoreError::DatabaseInvalid)
}

pub fn update(
    session: &mut WarehouseSession,
    request: UpdateApplicationRequest,
) -> Result<ApplicationDetail, CoreError> {
    let connection = session.connection_mut()?;
    let transaction = connection
        .transaction()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    update_in_transaction(&transaction, &request)?;
    transaction
        .commit()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    try_normalize_folder(session, &request.id)?;
    get(session, &request.id)
}

/// Metadata only; the caller owns commit/backup/audit and any later folder normalization.
pub(crate) fn update_in_transaction(
    transaction: &Transaction<'_>,
    request: &UpdateApplicationRequest,
) -> Result<(), CoreError> {
    validate_update_request(request)?;
    let now = now_utc();
    let changed = transaction
        .execute(
            "UPDATE applications SET
                company_name = ?1, company_type = ?2, industry = ?3,
                position_name = ?4, position_category = ?5, work_location = ?6,
                application_date = ?7, application_url = ?8, announcement_url = ?9,
                company_url = ?10, position_url = ?11, position_description = ?12,
                notes = ?13, updated_at_utc = ?14, revision = revision + 1
             WHERE id = ?15 AND revision = ?16 AND deleted_at_utc IS NULL",
            params![
                request.company_name.trim(),
                request.company_type,
                request.industry.trim(),
                request.position_name.trim(),
                request.position_category.trim(),
                request.work_location.trim(),
                request.application_date,
                request.application_url,
                request.announcement_url,
                request.company_url,
                request.position_url,
                request.position_description,
                request.notes,
                now,
                request.id,
                request.revision,
            ],
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    if changed == 0 {
        return Err(if application_exists(transaction, &request.id)? {
            CoreError::RevisionConflict
        } else {
            CoreError::NotFound
        });
    }
    replace_tags(transaction, &request.id, &request.tags, &now)?;
    replace_custom_fields(transaction, &request.id, &request.custom_fields, &now)?;
    Ok(())
}

fn validate_update_request(request: &UpdateApplicationRequest) -> Result<(), CoreError> {
    validate_required_text(&request.company_name)?;
    validate_required_text(&request.position_name)?;
    validate_company_type(&request.company_type)?;
    if let Some(date) = &request.application_date {
        NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|_| CoreError::Validation)?;
    }
    for url in [
        &request.application_url,
        &request.announcement_url,
        &request.company_url,
        &request.position_url,
    ]
    .into_iter()
    .flatten()
    {
        validate_web_url(url)?;
    }
    Ok(())
}

pub(crate) fn validate_required_text(value: &str) -> Result<(), CoreError> {
    if value.trim().is_empty() || value.chars().count() > 200 {
        Err(CoreError::Validation)
    } else {
        Ok(())
    }
}

fn validate_company_type(value: &str) -> Result<(), CoreError> {
    if VALID_COMPANY_TYPES.contains(&value) {
        Ok(())
    } else {
        Err(CoreError::Validation)
    }
}

fn validate_web_url(value: &str) -> Result<(), CoreError> {
    let parsed = url::Url::parse(value).map_err(|_| CoreError::Validation)?;
    if matches!(parsed.scheme(), "http" | "https") {
        Ok(())
    } else {
        Err(CoreError::Validation)
    }
}

fn application_exists(transaction: &Transaction<'_>, id: &str) -> Result<bool, CoreError> {
    transaction
        .query_row("SELECT 1 FROM applications WHERE id = ?1", [id], |_| Ok(()))
        .optional()
        .map(|value| value.is_some())
        .map_err(|_| CoreError::DatabaseInvalid)
}

fn replace_tags(
    transaction: &Transaction<'_>,
    application_id: &str,
    tags: &[String],
    now: &str,
) -> Result<(), CoreError> {
    transaction
        .execute(
            "DELETE FROM application_tags WHERE application_id = ?1",
            [application_id],
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    let mut normalized = Vec::new();
    for value in tags {
        let name = value.trim();
        if name.is_empty() || name.chars().count() > 40 {
            continue;
        }
        let folded = name.to_lowercase();
        if normalized.iter().any(|seen: &String| seen == &folded) {
            continue;
        }
        normalized.push(folded);
        let existing = transaction
            .query_row("SELECT id FROM tags WHERE name = ?1", [name], |row| {
                row.get::<_, String>(0)
            })
            .optional()
            .map_err(|_| CoreError::DatabaseInvalid)?;
        let tag_id = existing.unwrap_or_else(|| Uuid::new_v4().to_string());
        transaction
            .execute(
                "INSERT OR IGNORE INTO tags (id, name, color, created_at_utc, updated_at_utc, scope)
                 VALUES (?1, ?2, ?3, ?4, ?4, 'record')",
                params![tag_id, name, tag_color(name), now],
            )
            .map_err(|_| CoreError::DatabaseInvalid)?;
        transaction
            .execute(
                "INSERT INTO application_tags (application_id, tag_id, display_order)
                 VALUES (?1, ?2, ?3)",
                params![application_id, tag_id, normalized.len() as i64],
            )
            .map_err(|_| CoreError::DatabaseInvalid)?;
    }
    Ok(())
}

fn tag_color(name: &str) -> &'static str {
    const COLORS: &[&str] = &[
        "#2563eb", "#7c3aed", "#db2777", "#ea580c", "#0891b2", "#16a34a",
    ];
    let checksum = name
        .bytes()
        .fold(0_usize, |value, byte| value + usize::from(byte));
    COLORS[checksum % COLORS.len()]
}

fn replace_custom_fields(
    transaction: &Transaction<'_>,
    application_id: &str,
    values: &BTreeMap<String, Value>,
    now: &str,
) -> Result<(), CoreError> {
    let definitions = load_field_definition_types(transaction)?;
    transaction
        .execute(
            "DELETE FROM field_values WHERE application_id = ?1",
            [application_id],
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    for (definition_id, value) in values {
        let (field_type, config) = definitions
            .get(definition_id)
            .ok_or(CoreError::Validation)?;
        validate_custom_value(field_type, config, value)?;
        transaction
            .execute(
                "INSERT INTO field_values (
                    application_id, field_definition_id, value_json, updated_at_utc
                 ) VALUES (?1, ?2, ?3, ?4)",
                params![application_id, definition_id, value.to_string(), now],
            )
            .map_err(|_| CoreError::DatabaseInvalid)?;
    }
    Ok(())
}

fn load_field_definition_types(
    transaction: &Transaction<'_>,
) -> Result<BTreeMap<String, (String, Value)>, CoreError> {
    let mut statement = transaction
        .prepare("SELECT id, field_type, config_json FROM field_definitions")
        .map_err(|_| CoreError::DatabaseInvalid)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|_| CoreError::DatabaseInvalid)?;
    let mut result = BTreeMap::new();
    for row in rows {
        let (id, field_type, config) = row.map_err(|_| CoreError::DatabaseInvalid)?;
        result.insert(
            id,
            (
                field_type,
                serde_json::from_str(&config).map_err(|_| CoreError::DatabaseInvalid)?,
            ),
        );
    }
    Ok(result)
}

fn validate_custom_value(field_type: &str, config: &Value, value: &Value) -> Result<(), CoreError> {
    let valid = match field_type {
        "text" => value.is_string(),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        "date" => value
            .as_str()
            .is_some_and(|date| NaiveDate::parse_from_str(date, "%Y-%m-%d").is_ok()),
        "url" => value
            .as_str()
            .is_some_and(|url| validate_web_url(url).is_ok()),
        "select" => value.as_str().is_some_and(|selected| {
            config
                .get("options")
                .and_then(Value::as_array)
                .is_some_and(|options| {
                    options
                        .iter()
                        .any(|option| option.as_str() == Some(selected))
                })
        }),
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(CoreError::Validation)
    }
}

pub(crate) fn now_utc() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn try_normalize_folder(
    session: &mut WarehouseSession,
    application_id: &str,
) -> Result<(), CoreError> {
    let warehouse_root = session.root().to_path_buf();
    let (relative, company, position, short_id, created, offset) = session
        .connection()
        .query_row(
            "SELECT folder_relative_path, company_name, position_name, short_id, created_at_utc, created_timezone_offset_minutes
             FROM applications WHERE id = ?1",
            [application_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            },
        )
        .optional()
        .map_err(|_| CoreError::DatabaseInvalid)?
        .ok_or(CoreError::NotFound)?;
    let current = filesystem::application_folder(&warehouse_root, &relative)?;
    let Some(current_name) = current.file_name().and_then(OsStr::to_str) else {
        return Err(CoreError::UnsafePath);
    };
    let created_local = chrono::DateTime::parse_from_rfc3339(&created)
        .map_err(|_| CoreError::DatabaseInvalid)?
        .with_timezone(&Utc)
        + chrono::Duration::minutes(offset);
    let prefix = created_local.format("%Y-%m-%d_%H-%M-%S").to_string();
    let normalized =
        filesystem::normalized_application_folder_name(&prefix, &company, &position, &short_id);
    if normalized == current_name {
        session
            .connection_mut()?
            .execute(
                "UPDATE applications SET folder_normalization_pending = 0 WHERE id = ?1",
                [application_id],
            )
            .map_err(|_| CoreError::DatabaseInvalid)?;
        return Ok(());
    }
    session
        .connection_mut()?
        .execute(
            "UPDATE applications SET folder_normalization_pending = 1 WHERE id = ?1",
            [application_id],
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    match recycle_bin::normalize_record_folder(
        session,
        application_id,
        &relative,
        &format!("applications/{normalized}"),
    ) {
        Ok(())
        | Err(
            CoreError::FileOperation
            | CoreError::FileBusy
            | CoreError::FileAccessDenied
            | CoreError::FileMissing
            | CoreError::FileTypeMismatch,
        ) => {}
        Err(error) => return Err(error),
    }
    Ok(())
}

pub fn retry_folder_normalization(
    session: &mut WarehouseSession,
    application_id: &str,
) -> Result<ApplicationDetail, CoreError> {
    session.connection_mut()?;
    try_normalize_folder(session, application_id)?;
    get(session, application_id)
}

pub fn scan_documents(
    session: &mut WarehouseSession,
    application_id: &str,
) -> Result<Vec<DocumentEntry>, CoreError> {
    session.connection_mut()?;
    let warehouse_root = session.root().to_path_buf();
    let folder_relative_path = session
        .connection()
        .query_row(
            "SELECT folder_relative_path FROM applications WHERE id = ?1",
            [application_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|_| CoreError::DatabaseInvalid)?
        .ok_or(CoreError::NotFound)?;
    let scanned = filesystem::scan_application_files(&warehouse_root, &folder_relative_path)?;
    let now = now_utc();
    let transaction = session
        .connection_mut()?
        .transaction()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    sync_document_index(&transaction, application_id, scanned, &now)?;
    transaction
        .commit()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    load_documents(session.connection(), application_id)
}

fn sync_document_index(
    transaction: &Transaction<'_>,
    application_id: &str,
    scanned: Vec<filesystem::ScannedFile>,
    now: &str,
) -> Result<(), CoreError> {
    transaction
        .execute(
            "UPDATE documents SET missing_at_utc = ?1 WHERE application_id = ?2",
            params![now, application_id],
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    for file in scanned {
        transaction
            .execute(
                "INSERT INTO documents (
                    id, application_id, relative_path, display_name, media_type,
                    size_bytes, content_hash, discovered_at_utc, last_observed_at_utc,
                    missing_at_utc, modified_at_utc
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, ?7, NULL, ?8)
                 ON CONFLICT(application_id, relative_path) DO UPDATE SET
                    display_name = excluded.display_name,
                    media_type = excluded.media_type,
                    size_bytes = excluded.size_bytes,
                    last_observed_at_utc = excluded.last_observed_at_utc,
                    missing_at_utc = NULL,
                    modified_at_utc = excluded.modified_at_utc",
                params![
                    Uuid::new_v4().to_string(),
                    application_id,
                    file.relative_path,
                    file.display_name,
                    file.media_type,
                    file.size_bytes,
                    now,
                    file.modified_at_utc,
                ],
            )
            .map_err(|_| CoreError::DatabaseInvalid)?;
    }
    Ok(())
}

pub fn scan_all_documents(session: &mut WarehouseSession) -> Result<(), CoreError> {
    if !session.is_writable() {
        return Ok(());
    }
    let ids = {
        let mut statement = session
            .connection()
            .prepare("SELECT id FROM applications WHERE deleted_at_utc IS NULL")
            .map_err(|_| CoreError::DatabaseInvalid)?;
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|_| CoreError::DatabaseInvalid)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| CoreError::DatabaseInvalid)?
    };
    for id in ids {
        scan_documents(session, &id)?;
    }
    Ok(())
}

pub fn list_unlinked_folders(
    session: &WarehouseSession,
    include_hidden: bool,
) -> Result<Vec<crate::domain::UnlinkedFolder>, CoreError> {
    let linked = {
        let mut statement = session
            .connection()
            .prepare("SELECT folder_relative_path FROM applications WHERE deleted_at_utc IS NULL")
            .map_err(|_| CoreError::DatabaseInvalid)?;
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|_| CoreError::DatabaseInvalid)?
            .filter_map(Result::ok)
            .filter_map(|relative| {
                Path::new(&relative)
                    .file_name()?
                    .to_str()
                    .map(str::to_owned)
            })
            .collect::<std::collections::HashSet<_>>()
    };
    Ok(
        filesystem::list_candidate_directories(session.root(), include_hidden)?
            .into_iter()
            .filter(|(name, _)| !linked.contains(name))
            .map(|(name, hidden)| crate::domain::UnlinkedFolder { name, hidden })
            .collect(),
    )
}

pub fn claim_folder(
    session: &mut WarehouseSession,
    request: ClaimFolderRequest,
) -> Result<ApplicationDetail, CoreError> {
    session.connection_mut()?;
    validate_required_text(&request.application.company_name)?;
    validate_required_text(&request.application.position_name)?;
    if !request.application.company_type.is_empty() {
        validate_company_type(&request.application.company_type)?;
    }
    let candidate = list_unlinked_folders(session, request.include_hidden)?
        .into_iter()
        .find(|candidate| candidate.name == request.folder_name)
        .ok_or(CoreError::NotFound)?;
    if candidate.hidden && !request.include_hidden {
        return Err(CoreError::NotFound);
    }
    let id = Uuid::new_v4().to_string();
    let short_id = id.replace('-', "")[..6].to_ascii_uppercase();
    // Associate the original folder first. If interrupted or occupied during
    // normalization, the committed record still points to a complete folder.
    let folder_relative_path = format!("applications/{}", request.folder_name);
    let normalization_pending = true;
    let company_type = if request.application.company_type.is_empty() {
        "uncategorized".to_owned()
    } else {
        validate_company_type(&request.application.company_type)?;
        request.application.company_type
    };
    let now = now_utc();
    let offset_minutes = Local::now().offset().local_minus_utc() / 60;
    let transaction = session
        .connection_mut()?
        .transaction()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    transaction
        .execute(
            "INSERT INTO applications (
                id, short_id, created_at_utc, created_timezone_offset_minutes,
                company_name, company_type, industry, position_name, position_category,
                work_location, folder_relative_path, folder_normalization_pending,
                current_stage_state, status_updated_at_utc, updated_at_utc, revision
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                       'pending', ?3, ?3, 1)",
            params![
                id,
                short_id,
                now,
                offset_minutes,
                request.application.company_name.trim(),
                company_type,
                request.application.industry.trim(),
                request.application.position_name.trim(),
                request.application.position_category.trim(),
                request.application.work_location.trim(),
                folder_relative_path,
                normalization_pending,
            ],
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    let initial = clone_default_workflow(&transaction, &id, &now)?;
    transaction
        .execute(
            "UPDATE applications SET current_stage_id = ?1 WHERE id = ?2",
            params![initial.id, id],
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    transaction
        .execute(
            "INSERT INTO workflow_events (
                id, application_id, stage_id, stage_name_snapshot, previous_state,
                next_state, notes, occurred_at_utc, actor_type
             ) VALUES (?1, ?2, ?3, ?4, NULL, 'pending', '', ?5, 'user')",
            params![
                Uuid::new_v4().to_string(),
                id,
                initial.id,
                initial.display_name,
                now
            ],
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    transaction
        .commit()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    try_normalize_folder(session, &id)?;
    scan_documents(session, &id)?;
    get(session, &id)
}

pub fn change_stage(
    session: &mut WarehouseSession,
    request: ChangeStageRequest,
) -> Result<ApplicationDetail, CoreError> {
    let now = now_utc();
    let transaction = session
        .connection_mut()?
        .transaction()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    change_stage_in_transaction(&transaction, &request, &now)?;
    transaction
        .commit()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    get(session, &request.application_id)
}

/// Shared by single-record editing and atomic batch operations.
pub(crate) fn change_stage_in_transaction(
    transaction: &Transaction<'_>,
    request: &ChangeStageRequest,
    now: &str,
) -> Result<(), CoreError> {
    change_stage_with_actor(transaction, request, now, "user")
}

pub(crate) fn change_stage_with_actor(
    transaction: &Transaction<'_>,
    request: &ChangeStageRequest,
    now: &str,
    actor: &str,
) -> Result<(), CoreError> {
    let (previous_state, application_date, current_revision, previous_stage_id) = transaction
        .query_row(
            "SELECT current_stage_state, application_date, revision, current_stage_id
             FROM applications WHERE id = ?1 AND deleted_at_utc IS NULL",
            [&request.application_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()
        .map_err(|_| CoreError::DatabaseInvalid)?
        .ok_or(CoreError::NotFound)?;
    if current_revision != request.revision {
        return Err(CoreError::RevisionConflict);
    }
    auxiliary_states::require_state(transaction, &request.application_id, &request.stage_state)?;
    let (stage_name, stable_key) = transaction
        .query_row(
            "SELECT display_name, stable_key FROM workflow_stages
             WHERE id = ?1 AND application_id = ?2",
            params![request.stage_id, request.application_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(|_| CoreError::DatabaseInvalid)?
        .ok_or(CoreError::Validation)?;
    let previously_applied: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM workflow_events e JOIN workflow_stages s ON e.stage_id = s.id
            WHERE e.application_id = ?1 AND s.stable_key = 'applied')",
        [&request.application_id], |row| row.get(0),
    ).map_err(|_| CoreError::DatabaseInvalid)?;
    let next_application_date =
        if application_date.is_none() && stable_key == "applied" && !previously_applied {
            Some(Local::now().format("%Y-%m-%d").to_string())
        } else {
            application_date
        };
    let next_state = match stable_key.as_str() {
        "failed_terminal" => "failed",
        "offer" => "completed",
        _ if request.stage_state == "failed" => return Err(CoreError::Validation),
        _ => &request.stage_state,
    };
    let next_stage_id = if stable_key == "failed_terminal" {
        previous_stage_id.unwrap_or_else(|| request.stage_id.clone())
    } else {
        request.stage_id.clone()
    };
    transaction
        .execute(
            "UPDATE applications SET current_stage_id = ?1, current_stage_state = ?2,
                    application_date = ?3, status_updated_at_utc = ?4,
                    updated_at_utc = ?4, revision = revision + 1
             WHERE id = ?5 AND revision = ?6",
            params![
                next_stage_id,
                next_state,
                next_application_date,
                now,
                request.application_id,
                request.revision,
            ],
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    transaction
        .execute(
            "INSERT INTO workflow_events (
                id, application_id, stage_id, stage_name_snapshot, previous_state,
                next_state, notes, occurred_at_utc, actor_type
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                Uuid::new_v4().to_string(),
                request.application_id,
                request.stage_id,
                stage_name,
                Some(previous_state),
                next_state,
                request.notes,
                now,
                actor,
            ],
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    Ok(())
}

pub fn save_workflow_stage(
    session: &mut WarehouseSession,
    request: WorkflowStageRequest,
) -> Result<ApplicationDetail, CoreError> {
    validate_required_text(&request.display_name)?;
    if !is_hex_color(&request.color) {
        return Err(CoreError::Validation);
    }
    let now = now_utc();
    let connection = session
        .connection_mut()?
        .transaction()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    require_record_revision(&connection, &request.application_id, request.revision)?;
    let application_is_active: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM applications WHERE id = ?1 AND deleted_at_utc IS NULL)",
            [&request.application_id],
            |row| row.get(0),
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    if !application_is_active {
        return Err(CoreError::NotFound);
    }
    let changed = if let Some(id) = request.id {
        let (terminal, outcome) = connection.query_row(
            "SELECT is_terminal, terminal_outcome FROM workflow_stages WHERE id = ?1 AND application_id = ?2",
            params![id, request.application_id], |row| Ok((row.get::<_, bool>(0)?, row.get::<_, Option<String>>(1)?)),
        ).optional().map_err(|_| CoreError::DatabaseInvalid)?.ok_or(CoreError::NotFound)?;
        if terminal != request.is_terminal || outcome != request.terminal_outcome {
            return Err(CoreError::Validation);
        }
        connection
            .execute(
                "UPDATE workflow_stages SET display_name = ?1, color = ?2,
                        is_terminal = ?3, terminal_outcome = ?4, updated_at_utc = ?5
                 WHERE id = ?6 AND application_id = ?7",
                params![
                    request.display_name,
                    request.color,
                    request.is_terminal,
                    request.terminal_outcome,
                    now,
                    id,
                    request.application_id,
                ],
            )
            .map_err(|_| CoreError::DatabaseInvalid)?
    } else {
        if request.is_terminal || request.terminal_outcome.is_some() {
            return Err(CoreError::Validation);
        }
        if load_stages(&connection, &request.application_id)?.len() >= 100 {
            return Err(CoreError::Validation);
        }
        let order = connection
            .query_row(
                "SELECT COALESCE(MAX(display_order), 0) + 1 FROM workflow_stages
                 WHERE application_id = ?1 AND is_terminal = 0",
                [&request.application_id],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|_| CoreError::DatabaseInvalid)?;
        let inserted = connection
            .execute(
                "INSERT INTO workflow_stages (
                    id, application_id, template_id, stable_key, display_name,
                    stage_kind, display_order, color, is_terminal, terminal_outcome,
                    created_at_utc, updated_at_utc
                 ) VALUES (?1, ?2, NULL, ?3, ?4, 'custom', ?5, ?6, ?7, ?8, ?9, ?9)",
                params![
                    Uuid::new_v4().to_string(),
                    request.application_id,
                    format!("custom_{}", Uuid::new_v4().simple()),
                    request.display_name,
                    order,
                    request.color,
                    request.is_terminal,
                    request.terminal_outcome,
                    now,
                ],
            )
            .map_err(|_| CoreError::DatabaseInvalid)?;
        // Sort numbers are not percentages. Keep both terminal markers after
        // all intermediate stages even when many custom stages were appended.
        connection
            .execute(
                "UPDATE workflow_stages SET display_order = CASE stable_key
                 WHEN 'offer' THEN ?1 + 10 ELSE ?1 + 20 END, updated_at_utc = ?2
             WHERE application_id = ?3 AND stable_key IN ('offer', 'failed_terminal')",
                params![order, now, request.application_id],
            )
            .map_err(|_| CoreError::DatabaseInvalid)?;
        inserted
    };
    if changed != 1 {
        return Err(CoreError::NotFound);
    }
    connection
        .execute(
            "UPDATE applications SET updated_at_utc = ?1, revision = revision + 1 WHERE id = ?2",
            params![now, request.application_id],
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    connection
        .commit()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    get(session, &request.application_id)
}

pub fn delete_workflow_stage(
    session: &mut WarehouseSession,
    application_id: &str,
    stage_id: &str,
    revision: i64,
) -> Result<ApplicationDetail, CoreError> {
    let now = now_utc();
    let connection = session
        .connection_mut()?
        .transaction()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    require_record_revision(&connection, application_id, revision)?;
    let changed = connection
        .execute(
            "DELETE FROM workflow_stages
             WHERE id = ?1 AND application_id = ?2 AND substr(stable_key, 1, 7) = 'custom_'
               AND id != COALESCE((SELECT current_stage_id FROM applications WHERE id = ?2), '')
               AND NOT EXISTS (SELECT 1 FROM workflow_events WHERE stage_id = ?1)",
            params![stage_id, application_id],
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    if changed != 1 {
        return Err(CoreError::Validation);
    }
    connection
        .execute(
            "UPDATE applications SET updated_at_utc = ?1, revision = revision + 1 WHERE id = ?2",
            params![now, application_id],
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    connection
        .commit()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    get(session, application_id)
}

pub fn list_workflow_templates(
    session: &WarehouseSession,
) -> Result<Vec<WorkflowTemplate>, CoreError> {
    let mut statement = session
        .connection()
        .prepare(
            "SELECT t.id, t.name, t.description, t.is_default, COUNT(s.id), t.revision
             FROM workflow_templates t LEFT JOIN workflow_stages s ON s.template_id = t.id
             GROUP BY t.id ORDER BY t.is_default DESC, t.name",
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    statement
        .query_map([], |row| {
            Ok(WorkflowTemplate {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                is_default: row.get::<_, i64>(3)? != 0,
                stage_count: row.get(4)?,
                revision: row.get(5)?,
            })
        })
        .map_err(|_| CoreError::DatabaseInvalid)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| CoreError::DatabaseInvalid)
}

pub fn save_workflow_as_template(
    session: &mut WarehouseSession,
    application_id: &str,
    name: &str,
    set_default: bool,
) -> Result<Vec<WorkflowTemplate>, CoreError> {
    validate_required_text(name)?;
    let now = now_utc();
    let template_id = Uuid::new_v4().to_string();
    let transaction = session
        .connection_mut()?
        .transaction()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    let exists: bool = transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM applications WHERE id = ?1 AND deleted_at_utc IS NULL)",
            [application_id],
            |row| row.get(0),
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    if !exists {
        return Err(CoreError::NotFound);
    }
    if set_default {
        transaction
            .execute("UPDATE workflow_templates SET is_default = 0, revision = revision + 1, updated_at_utc = ?1 WHERE is_default = 1", [&now])
            .map_err(|_| CoreError::DatabaseInvalid)?;
    }
    transaction
        .execute(
            "INSERT INTO workflow_templates (id, name, description, is_default, created_at_utc, updated_at_utc)
             VALUES (?1, ?2, '由单条投递流程保存；用于以后创建的投递。', ?3, ?4, ?4)",
            params![template_id, name.trim(), set_default, now],
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    let source_stages = load_stages(&transaction, application_id)?;
    workflows::validate_order(&source_stages)?;
    auxiliary_states::clone_into_new_owner(
        &transaction,
        Owner::Application(application_id),
        Owner::Template(&template_id),
    )?;
    for stage in source_stages {
        transaction
            .execute(
                "INSERT INTO workflow_stages (
                    id, application_id, template_id, stable_key, display_name, stage_kind,
                    display_order, color, is_terminal, terminal_outcome, created_at_utc, updated_at_utc
                 ) VALUES (?1, NULL, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
                params![
                    Uuid::new_v4().to_string(), template_id, stage.stable_key,
                    stage.display_name, stage.stage_kind, stage.display_order, stage.color,
                    stage.is_terminal, stage.terminal_outcome, now,
                ],
            )
            .map_err(|_| CoreError::DatabaseInvalid)?;
    }
    transaction
        .commit()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    list_workflow_templates(session)
}

pub(crate) fn is_hex_color(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(crate) fn require_record_revision(
    connection: &Connection,
    application_id: &str,
    expected: i64,
) -> Result<(), CoreError> {
    let actual: i64 = connection
        .query_row(
            "SELECT revision FROM applications WHERE id = ?1 AND deleted_at_utc IS NULL",
            [application_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| CoreError::DatabaseInvalid)?
        .ok_or(CoreError::NotFound)?;
    if actual != expected {
        Err(CoreError::RevisionConflict)
    } else {
        Ok(())
    }
}

pub fn save_interview_round(
    session: &mut WarehouseSession,
    request: InterviewRoundRequest,
) -> Result<ApplicationDetail, CoreError> {
    validate_required_text(&request.display_name)?;
    for timestamp in [&request.scheduled_at_utc, &request.completed_at_utc]
        .into_iter()
        .flatten()
    {
        chrono::DateTime::parse_from_rfc3339(timestamp).map_err(|_| CoreError::Validation)?;
    }
    let now = now_utc();
    let connection = session
        .connection_mut()?
        .transaction()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    require_record_revision(&connection, &request.application_id, request.revision)?;
    auxiliary_states::require_state(&connection, &request.application_id, &request.state)?;
    let exists = connection
        .query_row(
            "SELECT 1 FROM applications WHERE id = ?1 AND deleted_at_utc IS NULL",
            [&request.application_id],
            |_| Ok(()),
        )
        .optional()
        .map_err(|_| CoreError::DatabaseInvalid)?
        .is_some();
    if !exists {
        return Err(CoreError::NotFound);
    }
    if let Some(id) = request.id {
        let changed = connection
            .execute(
                "UPDATE interview_rounds SET display_name = ?1, state = ?2,
                        scheduled_at_utc = ?3, completed_at_utc = ?4, result = ?5,
                        notes = ?6, updated_at_utc = ?7
                 WHERE id = ?8 AND application_id = ?9",
                params![
                    request.display_name.trim(),
                    request.state,
                    request.scheduled_at_utc,
                    request.completed_at_utc,
                    request.result,
                    request.notes,
                    now,
                    id,
                    request.application_id,
                ],
            )
            .map_err(|_| CoreError::DatabaseInvalid)?;
        if changed == 0 {
            return Err(CoreError::NotFound);
        }
    } else {
        let sequence: i64 = connection
            .query_row(
                "SELECT COALESCE(MAX(sequence_number), 0) + 1
                 FROM interview_rounds WHERE application_id = ?1",
                [&request.application_id],
                |row| row.get(0),
            )
            .map_err(|_| CoreError::DatabaseInvalid)?;
        connection
            .execute(
                "INSERT INTO interview_rounds (
                    id, application_id, sequence_number, display_name, state,
                    scheduled_at_utc, completed_at_utc, result, notes,
                    created_at_utc, updated_at_utc
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
                params![
                    Uuid::new_v4().to_string(),
                    request.application_id,
                    sequence,
                    request.display_name.trim(),
                    request.state,
                    request.scheduled_at_utc,
                    request.completed_at_utc,
                    request.result,
                    request.notes,
                    now,
                ],
            )
            .map_err(|_| CoreError::DatabaseInvalid)?;
    }
    connection
        .execute(
            "UPDATE applications SET updated_at_utc = ?1, revision = revision + 1 WHERE id = ?2",
            params![now, request.application_id],
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    connection
        .commit()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    get(session, &request.application_id)
}

pub fn delete_interview_round(
    session: &mut WarehouseSession,
    application_id: &str,
    round_id: &str,
    revision: i64,
) -> Result<ApplicationDetail, CoreError> {
    let transaction = session
        .connection_mut()?
        .transaction()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    require_record_revision(&transaction, application_id, revision)?;
    let linked: bool = transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM recruitment_events WHERE interview_round_id=?1)",
            [round_id],
            |r| r.get(0),
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    if linked {
        return Err(CoreError::EventRoundInUse);
    }
    let changed = transaction
        .execute(
            "DELETE FROM interview_rounds WHERE id = ?1 AND application_id = ?2",
            params![round_id, application_id],
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    if changed == 0 {
        return Err(CoreError::NotFound);
    }
    transaction
        .execute(
            "UPDATE applications SET updated_at_utc = ?1, revision = revision + 1 WHERE id = ?2",
            params![now_utc(), application_id],
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    transaction
        .commit()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    get(session, application_id)
}

pub fn list_field_definitions(
    session: &WarehouseSession,
) -> Result<Vec<FieldDefinition>, CoreError> {
    field_definitions(session.connection())
}
fn field_definitions(connection: &Connection) -> Result<Vec<FieldDefinition>, CoreError> {
    let mut statement = connection
        .prepare(
            "SELECT id, key, display_name, field_type, config_json,
                    display_order, is_visible, revision
             FROM field_definitions ORDER BY display_order",
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)? != 0,
                row.get::<_, i64>(7)?,
            ))
        })
        .map_err(|_| CoreError::DatabaseInvalid)?;
    rows.map(|row| {
        let (id, key, display_name, field_type, config, display_order, is_visible, revision) =
            row.map_err(|_| CoreError::DatabaseInvalid)?;
        Ok(FieldDefinition {
            id,
            revision,
            key,
            display_name,
            field_type,
            config: serde_json::from_str(&config).map_err(|_| CoreError::DatabaseInvalid)?,
            display_order,
            is_visible,
        })
    })
    .collect()
}

pub fn save_field_definition(
    session: &mut WarehouseSession,
    request: FieldDefinitionRequest,
) -> Result<Vec<FieldDefinition>, CoreError> {
    validate_required_text(&request.display_name)?;
    if !VALID_FIELD_TYPES.contains(&request.field_type.as_str()) {
        return Err(CoreError::Validation);
    }
    if !request.config.is_object() {
        return Err(CoreError::Validation);
    }
    if request.field_type == "select" {
        let options = request
            .config
            .get("options")
            .and_then(Value::as_array)
            .filter(|values| !values.is_empty())
            .ok_or(CoreError::Validation)?;
        let mut seen = std::collections::HashSet::new();
        for option in options {
            let text = option.as_str().ok_or(CoreError::Validation)?;
            validate_required_text(text)?;
            if text != text.trim() || !seen.insert(text) {
                return Err(CoreError::Validation);
            }
        }
    }
    let now = now_utc();
    let connection = session
        .connection_mut()?
        .transaction()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    if let Some(id) = request.id {
        let revision: i64 = connection
            .query_row(
                "SELECT revision FROM field_definitions WHERE id = ?1",
                [&id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|_| CoreError::DatabaseInvalid)?
            .ok_or(CoreError::NotFound)?;
        if request.revision != Some(revision) {
            return Err(CoreError::RevisionConflict);
        }
        let mut statement = connection
            .prepare("SELECT value_json FROM field_values WHERE field_definition_id = ?1")
            .map_err(|_| CoreError::DatabaseInvalid)?;
        let values = statement
            .query_map([&id], |row| row.get::<_, String>(0))
            .map_err(|_| CoreError::DatabaseInvalid)?;
        for value in values {
            let value: Value =
                serde_json::from_str(&value.map_err(|_| CoreError::DatabaseInvalid)?)
                    .map_err(|_| CoreError::DatabaseInvalid)?;
            validate_custom_value(&request.field_type, &request.config, &value)?;
        }
        let changed = connection
            .execute(
                "UPDATE field_definitions SET display_name = ?1, field_type = ?2,
                        config_json = ?3, updated_at_utc = ?4, revision = revision + 1 WHERE id = ?5",
                params![
                    request.display_name.trim(),
                    request.field_type,
                    request.config.to_string(),
                    now,
                    id,
                ],
            )
            .map_err(|_| CoreError::DatabaseInvalid)?;
        if changed == 0 {
            return Err(CoreError::NotFound);
        }
    } else {
        if request.revision.is_some() {
            return Err(CoreError::Validation);
        }
        let id = Uuid::new_v4().to_string();
        let order: i64 = connection
            .query_row(
                "SELECT COALESCE(MAX(display_order), 0) + 10 FROM field_definitions",
                [],
                |row| row.get(0),
            )
            .map_err(|_| CoreError::DatabaseInvalid)?;
        connection
            .execute(
                "INSERT INTO field_definitions (
                    id, key, display_name, field_type, config_json, display_order,
                    is_visible, created_at_utc, updated_at_utc
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?7)",
                params![
                    id,
                    format!("custom_{}", id.replace('-', "")[..8].to_ascii_lowercase()),
                    request.display_name.trim(),
                    request.field_type,
                    request.config.to_string(),
                    order,
                    now,
                ],
            )
            .map_err(|_| CoreError::DatabaseInvalid)?;
    }
    let result = field_definitions(&connection)?;
    connection
        .commit()
        .map_err(|_| CoreError::DatabaseInvalid)?;
    Ok(result)
}

pub use crate::views::{list as list_views, save as save_view};

pub fn page_size(session: &WarehouseSession) -> Result<i64, CoreError> {
    let json = session
        .connection()
        .query_row(
            "SELECT value_json FROM settings WHERE key = 'applications.page_size'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|_| CoreError::DatabaseInvalid)?
        .unwrap_or_else(|| "50".to_owned());
    serde_json::from_str(&json).map_err(|_| CoreError::DatabaseInvalid)
}

pub fn set_page_size(session: &mut WarehouseSession, value: i64) -> Result<i64, CoreError> {
    if ![20_i64, 50, 100, 200].contains(&value) {
        return Err(CoreError::Validation);
    }
    session
        .connection_mut()?
        .execute(
            "INSERT INTO settings (key, value_json, updated_at_utc)
             VALUES ('applications.page_size', ?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json,
                 updated_at_utc = excluded.updated_at_utc",
            params![value.to_string(), now_utc()],
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    Ok(value)
}

pub fn set_archived(
    session: &mut WarehouseSession,
    application_id: &str,
    archived: bool,
) -> Result<ApplicationDetail, CoreError> {
    let now = now_utc();
    let changed = session
        .connection_mut()?
        .execute(
            "UPDATE applications SET archived_at_utc = ?1, updated_at_utc = ?2,
                    revision = revision + 1
             WHERE id = ?3 AND deleted_at_utc IS NULL",
            params![
                if archived { Some(now.clone()) } else { None },
                now,
                application_id
            ],
        )
        .map_err(|_| CoreError::DatabaseInvalid)?;
    if changed == 0 {
        return Err(CoreError::NotFound);
    }
    get(session, application_id)
}

pub fn duplicate_preview(
    session: &WarehouseSession,
    application_id: &str,
    mode: DuplicateMode,
) -> Result<DuplicatePreview, CoreError> {
    let detail = get(session, application_id)?;
    let file_size_bytes = if matches!(mode, DuplicateMode::FullRecord) {
        let folder =
            filesystem::application_folder(session.root(), &detail.record.folder_relative_path)?;
        filesystem::directory_size(&folder)?
    } else {
        0
    };
    Ok(DuplicatePreview {
        mode,
        file_size_bytes,
        editable_field_count: if matches!(mode, DuplicateMode::FullRecord) {
            13 + detail.record.custom_fields.len()
        } else {
            3
        },
    })
}

pub fn duplicate(
    session: &mut WarehouseSession,
    application_id: &str,
    mode: DuplicateMode,
) -> Result<ApplicationDetail, CoreError> {
    session.connection_mut()?;
    let mut source = get(session, application_id)?;
    if source.record.deleted_at_utc.is_some() {
        return Err(CoreError::NotFound);
    }
    let full_record = matches!(mode, DuplicateMode::FullRecord);
    if !full_record {
        source.record.position_name.clear();
        source.record.position_category.clear();
        source.record.work_location.clear();
        source.record.application_url = None;
        source.record.announcement_url = None;
        source.record.company_url = None;
        source.record.position_url = None;
        source.record.position_description.clear();
        source.record.notes.clear();
        source.record.custom_fields.clear();
        source.record.tags.retain(|tag| tag.scope == "company");
    }
    let warehouse_root = session.root().to_path_buf();
    let source_folder =
        filesystem::application_folder(&warehouse_root, &source.record.folder_relative_path)?;
    let id = Uuid::new_v4().to_string();
    let short_id = id.replace('-', "")[..6].to_ascii_uppercase();
    let created_local = Local::now();
    let now = created_local
        .with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::Millis, true);
    let offset_minutes = created_local.offset().local_minus_utc() / 60;
    let final_name = filesystem::normalized_application_folder_name(
        &created_local.format("%Y-%m-%d_%H-%M-%S").to_string(),
        &source.record.company_name,
        &source.record.position_name,
        &short_id,
    );
    let operation =
        PendingCreation::begin(session, &id, &format!("applications/{final_name}"), &now)?;
    if let Err(error) =
        operation.copy_and_verify(session, full_record.then_some(source_folder.as_path()))
    {
        operation
            .cancel(session)
            .map_err(|_| CoreError::CopyRecovery)?;
        return Err(error);
    }
    let database_result = (|| {
        let transaction = session
            .connection_mut()?
            .transaction()
            .map_err(|_| CoreError::DatabaseInvalid)?;
        transaction
            .execute(
                "INSERT INTO applications (
                    id, short_id, created_at_utc, created_timezone_offset_minutes,
                    application_date, company_name, company_type, industry,
                    position_name, position_category, work_location, application_url,
                    announcement_url, company_url, position_url, position_description,
                    notes, folder_relative_path, current_stage_state,
                    status_updated_at_utc, updated_at_utc, revision
                 ) VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6, ?7, ?8, ?9, ?10,
                           ?11, ?12, ?13, ?14, ?15, ?16, ?17, 'pending', ?3, ?3, 1)",
                params![
                    id,
                    short_id,
                    now,
                    offset_minutes,
                    source.record.company_name,
                    source.record.company_type,
                    source.record.industry,
                    source.record.position_name,
                    source.record.position_category,
                    source.record.work_location,
                    source.record.application_url,
                    source.record.announcement_url,
                    source.record.company_url,
                    source.record.position_url,
                    source.record.position_description,
                    source.record.notes,
                    format!("applications/{final_name}"),
                ],
            )
            .map_err(|_| CoreError::DatabaseInvalid)?;
        let initial = if full_record {
            clone_workflow_from_application(&transaction, application_id, &id, &now)?
        } else {
            clone_default_workflow(&transaction, &id, &now)?
        };
        transaction
            .execute(
                "UPDATE applications SET current_stage_id = ?1 WHERE id = ?2",
                params![initial.id, id],
            )
            .map_err(|_| CoreError::DatabaseInvalid)?;
        transaction
            .execute(
                "INSERT INTO workflow_events (
                    id, application_id, stage_id, stage_name_snapshot, previous_state,
                    next_state, notes, occurred_at_utc, actor_type
                 ) VALUES (?1, ?2, ?3, ?4, NULL, 'pending', '', ?5, 'user')",
                params![
                    Uuid::new_v4().to_string(),
                    id,
                    initial.id,
                    initial.display_name,
                    now
                ],
            )
            .map_err(|_| CoreError::DatabaseInvalid)?;
        let tag_names = source
            .record
            .tags
            .iter()
            .map(|tag| tag.name.clone())
            .collect::<Vec<_>>();
        replace_tags(&transaction, &id, &tag_names, &now)?;
        replace_custom_fields(&transaction, &id, &source.record.custom_fields, &now)?;
        if full_record {
            // Copy round structure and user notes, not past dates or outcomes.
            for round in &source.interview_rounds {
                transaction
                    .execute(
                        "INSERT INTO interview_rounds (id, application_id, sequence_number,
                     display_name, state, notes, created_at_utc, updated_at_utc, result)
                     VALUES (?1, ?2, ?3, ?4, 'pending', ?5, ?6, ?6, '')",
                        params![
                            Uuid::new_v4().to_string(),
                            id,
                            round.sequence_number,
                            round.display_name,
                            round.notes,
                            now
                        ],
                    )
                    .map_err(|_| CoreError::DatabaseInvalid)?;
            }
        }
        operation.publish(&transaction)?;
        let scanned =
            filesystem::scan_application_files(&warehouse_root, &operation.target_relative)?;
        sync_document_index(&transaction, &id, scanned, &now)?;
        transaction.commit().map_err(|_| CoreError::DatabaseInvalid)
    })();
    if let Err(error) = database_result {
        operation
            .cancel(session)
            .map_err(|_| CoreError::CopyRecovery)?;
        return Err(error);
    }
    get(session, &id)
}

fn clone_workflow_from_application(
    transaction: &Transaction<'_>,
    source_application_id: &str,
    target_application_id: &str,
    now: &str,
) -> Result<WorkflowStage, CoreError> {
    auxiliary_states::clone_into_new_owner(
        transaction,
        Owner::Application(source_application_id),
        Owner::Application(target_application_id),
    )?;
    let stages = {
        let mut statement = transaction
            .prepare(
                "SELECT stable_key, display_name, stage_kind, display_order, color,
                        is_terminal, terminal_outcome
                 FROM workflow_stages WHERE application_id = ?1 ORDER BY display_order",
            )
            .map_err(|_| CoreError::DatabaseInvalid)?;
        statement
            .query_map([source_application_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)? != 0,
                    row.get::<_, Option<String>>(6)?,
                ))
            })
            .map_err(|_| CoreError::DatabaseInvalid)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| CoreError::DatabaseInvalid)?
    };
    let mut initial = None;
    for (stable_key, display_name, stage_kind, order, color, terminal, outcome) in stages {
        let stage = WorkflowStage {
            id: Uuid::new_v4().to_string(),
            stable_key,
            display_name,
            stage_kind,
            display_order: order,
            color,
            is_terminal: terminal,
            terminal_outcome: outcome,
        };
        transaction
            .execute(
                "INSERT INTO workflow_stages (
                    id, application_id, template_id, stable_key, display_name,
                    stage_kind, display_order, color, is_terminal, terminal_outcome,
                    created_at_utc, updated_at_utc
                 ) VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
                params![
                    stage.id,
                    target_application_id,
                    stage.stable_key,
                    stage.display_name,
                    stage.stage_kind,
                    stage.display_order,
                    stage.color,
                    stage.is_terminal,
                    stage.terminal_outcome,
                    now,
                ],
            )
            .map_err(|_| CoreError::DatabaseInvalid)?;
        if stage.stable_key == "preparing" || initial.is_none() {
            initial = Some(stage);
        }
    }
    initial.ok_or(CoreError::DatabaseInvalid)
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use tempfile::tempdir;

    use super::*;
    use crate::{domain::ApplicationScope, warehouse};

    fn request(company: &str, position: &str) -> CreateApplicationRequest {
        CreateApplicationRequest {
            company_name: company.to_owned(),
            position_name: position.to_owned(),
            company_type: "private".to_owned(),
            industry: "软件".to_owned(),
            position_category: "研发".to_owned(),
            work_location: "上海".to_owned(),
        }
    }

    #[test]
    fn creates_an_independent_folder_and_starts_preparing() {
        let directory = tempdir().unwrap();
        let mut session = warehouse::create(directory.path()).unwrap();

        let detail = create(&mut session, request("示例公司", "开发工程师")).unwrap();

        assert_eq!(detail.record.current_stage_name, "准备投递");
        assert_eq!(detail.record.current_stage_state, "pending");
        assert!(detail.record.application_date.is_none());
        assert!(
            directory
                .path()
                .join(&detail.record.folder_relative_path)
                .is_dir()
        );
        assert_eq!(list(&session, ApplicationScope::Active).unwrap().len(), 1);
    }

    #[test]
    fn first_applied_transition_sets_date_and_preserves_previous_state() {
        let directory = tempdir().unwrap();
        let mut session = warehouse::create(directory.path()).unwrap();
        let detail = create(&mut session, request("示例公司", "开发工程师")).unwrap();
        let applied = detail
            .stages
            .iter()
            .find(|stage| stage.stable_key == "applied")
            .unwrap();

        let changed = change_stage(
            &mut session,
            ChangeStageRequest {
                application_id: detail.record.id.clone(),
                stage_id: applied.id.clone(),
                stage_state: "awaitingResult".to_owned(),
                revision: detail.record.revision,
                notes: "已提交".to_owned(),
            },
        )
        .unwrap();

        assert!(changed.record.application_date.is_some());
        assert_eq!(changed.record.current_stage_name, "已投递");
        assert_eq!(
            changed.history[0].previous_state.as_deref(),
            Some("pending")
        );
        assert_eq!(changed.history[0].next_state, "awaitingResult");
    }

    #[test]
    fn full_duplicate_copies_files_but_uses_a_distinct_record_and_folder() {
        let directory = tempdir().unwrap();
        let mut session = warehouse::create(directory.path()).unwrap();
        let original = create(&mut session, request("示例公司", "开发工程师")).unwrap();
        let original_folder = directory.path().join(&original.record.folder_relative_path);
        fs::create_dir(original_folder.join("简历版本")).unwrap();
        fs::write(original_folder.join("简历版本").join("resume.pdf"), b"pdf").unwrap();
        scan_documents(&mut session, &original.record.id).unwrap();
        save_interview_round(
            &mut session,
            InterviewRoundRequest {
                id: None,
                application_id: original.record.id.clone(),
                revision: original.record.revision,
                display_name: "主管面".into(),
                state: "completed".into(),
                scheduled_at_utc: Some("2026-09-01T00:00:00Z".into()),
                completed_at_utc: Some("2026-09-01T01:00:00Z".into()),
                result: "旧结果".into(),
                notes: "轮次说明".into(),
            },
        )
        .unwrap();
        let source_round = get(&session, &original.record.id).unwrap().interview_rounds[0]
            .id
            .clone();
        session.connection_mut().unwrap().execute(
            "UPDATE applications SET application_date = '2026-09-01', archived_at_utc = ?1 WHERE id = ?2",
            params![now_utc(), original.record.id]).unwrap();

        let copied =
            duplicate(&mut session, &original.record.id, DuplicateMode::FullRecord).unwrap();

        assert_ne!(copied.record.id, original.record.id);
        assert_ne!(
            copied.record.folder_relative_path,
            original.record.folder_relative_path
        );
        assert_eq!(copied.record.current_stage_name, "准备投递");
        assert!(copied.record.application_date.is_none());
        assert!(copied.record.archived_at_utc.is_none());
        assert_eq!(copied.history.len(), 1);
        assert_eq!(copied.interview_rounds.len(), 1);
        let round = &copied.interview_rounds[0];
        assert_ne!(round.id, source_round);
        assert_eq!(round.display_name, "主管面");
        assert_eq!(round.notes, "轮次说明");
        assert_eq!(round.state, "pending");
        assert!(round.scheduled_at_utc.is_none());
        assert!(round.completed_at_utc.is_none());
        assert!(round.result.is_empty());
        assert!(
            copied
                .stages
                .iter()
                .all(|stage| original.stages.iter().all(|source| source.id != stage.id))
        );
        assert!(
            directory
                .path()
                .join(&copied.record.folder_relative_path)
                .join("简历版本")
                .join("resume.pdf")
                .is_file()
        );
        let copied_folder = directory.path().join(&copied.record.folder_relative_path);
        fs::write(copied_folder.join("简历版本/resume.pdf"), b"edited copy").unwrap();
        assert_eq!(
            fs::read(original_folder.join("简历版本/resume.pdf")).unwrap(),
            b"pdf"
        );
        let copied_id = copied.record.id.clone();
        drop(session);
        let reopened =
            warehouse::open(directory.path(), warehouse::WarehouseAccessMode::Write).unwrap();
        assert!(copied_folder.exists());
        assert_eq!(get(&reopened, &copied_id).unwrap().record.id, copied_id);
    }

    #[test]
    fn unlinked_scan_ignores_dot_directories_and_claim_uses_confirmation_time() {
        let directory = tempdir().unwrap();
        let mut session = warehouse::create(directory.path()).unwrap();
        fs::create_dir(directory.path().join("applications").join("手动文件夹")).unwrap();
        fs::create_dir(directory.path().join("applications").join(".hidden")).unwrap();

        let visible = list_unlinked_folders(&session, false).unwrap();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].name, "手动文件夹");
        let before = now_utc();
        let claimed = claim_folder(
            &mut session,
            ClaimFolderRequest {
                folder_name: "手动文件夹".to_owned(),
                include_hidden: false,
                application: request("认领公司", "分析师"),
            },
        )
        .unwrap();

        assert!(claimed.record.created_at_utc >= before);
        assert!(
            Path::new(&claimed.record.folder_relative_path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains("认领公司")
        );
        assert!(list_unlinked_folders(&session, false).unwrap().is_empty());
    }

    #[test]
    fn company_copy_only_keeps_company_fields_and_company_scoped_tags() {
        let directory = tempdir().unwrap();
        let mut session = warehouse::create(directory.path()).unwrap();
        let original = create(&mut session, request("同公司", "原始岗位")).unwrap();
        let transaction = session.connection_mut().unwrap().transaction().unwrap();
        replace_tags(
            &transaction,
            &original.record.id,
            &["公司标签".into(), "岗位标签".into()],
            &now_utc(),
        )
        .unwrap();
        transaction
            .execute(
                "UPDATE tags SET scope = 'company' WHERE name = '公司标签'",
                [],
            )
            .unwrap();
        transaction.execute("UPDATE applications SET notes = '不复制', application_url = 'https://example.com' WHERE id = ?1", [&original.record.id]).unwrap();
        transaction.commit().unwrap();
        fs::write(
            directory
                .path()
                .join(&original.record.folder_relative_path)
                .join("resume.pdf"),
            b"original",
        )
        .unwrap();
        let copied = duplicate(
            &mut session,
            &original.record.id,
            DuplicateMode::CompanyInfo,
        )
        .unwrap();
        assert_eq!(copied.record.company_name, original.record.company_name);
        assert_eq!(copied.record.company_type, original.record.company_type);
        assert_eq!(copied.record.industry, original.record.industry);
        assert!(copied.record.position_name.is_empty());
        assert!(copied.record.position_category.is_empty());
        assert!(copied.record.work_location.is_empty());
        assert!(copied.record.notes.is_empty());
        assert!(copied.record.application_url.is_none());
        assert!(copied.record.custom_fields.is_empty());
        assert!(copied.documents.is_empty());
        assert_eq!(copied.record.tags.len(), 1);
        assert_eq!(copied.record.tags[0].name, "公司标签");
        assert_eq!(copied.record.tags[0].scope, "company");
    }

    #[test]
    fn failed_database_insert_cancels_staged_creation_without_active_orphans() {
        let directory = tempdir().unwrap();
        let mut session = warehouse::create(directory.path()).unwrap();
        let original = create(&mut session, request("原记录", "岗位")).unwrap();
        let source_folder = directory.path().join(&original.record.folder_relative_path);
        fs::write(source_folder.join("resume.pdf"), b"source").unwrap();
        session
            .connection_mut()
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER fail_new_application BEFORE INSERT ON applications
             BEGIN SELECT RAISE(ABORT, 'injected database failure'); END;",
            )
            .unwrap();
        assert!(matches!(
            duplicate(&mut session, &original.record.id, DuplicateMode::FullRecord),
            Err(CoreError::DatabaseInvalid)
        ));
        assert!(matches!(
            create(&mut session, request("新记录", "岗位")),
            Err(CoreError::DatabaseInvalid)
        ));
        session
            .connection_mut()
            .unwrap()
            .execute_batch(
                "DROP TRIGGER fail_new_application;
             CREATE TRIGGER fail_file_index BEFORE INSERT ON documents
             BEGIN SELECT RAISE(ABORT, 'injected index failure after publish'); END;",
            )
            .unwrap();
        assert!(matches!(
            duplicate(&mut session, &original.record.id, DuplicateMode::FullRecord),
            Err(CoreError::DatabaseInvalid)
        ));
        assert_eq!(list(&session, ApplicationScope::Active).unwrap().len(), 1);
        assert_eq!(
            fs::read_dir(directory.path().join("applications"))
                .unwrap()
                .count(),
            1
        );
        assert_eq!(
            fs::read_dir(directory.path().join("recycle-bin/records"))
                .unwrap()
                .count(),
            0
        );
        assert_eq!(
            fs::read(source_folder.join("resume.pdf")).unwrap(),
            b"source"
        );
        let pending: i64 = session
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM record_creations WHERE state IN ('copying', 'verified')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(pending, 0);
    }

    #[test]
    fn read_only_cannot_start_creation_or_either_copy_mode() {
        let directory = tempdir().unwrap();
        let mut session = warehouse::create(directory.path()).unwrap();
        let original = create(&mut session, request("只读公司", "岗位")).unwrap();
        let mut read_only =
            warehouse::open(directory.path(), warehouse::WarehouseAccessMode::ReadOnly).unwrap();
        for mode in [DuplicateMode::CompanyInfo, DuplicateMode::FullRecord] {
            assert!(matches!(
                duplicate(&mut read_only, &original.record.id, mode),
                Err(CoreError::ReadOnlyWarehouse)
            ));
        }
        assert!(matches!(
            create(&mut read_only, request("禁止创建", "岗位")),
            Err(CoreError::ReadOnlyWarehouse)
        ));
        assert_eq!(
            fs::read_dir(directory.path().join("applications"))
                .unwrap()
                .count(),
            1
        );
        assert_eq!(
            fs::read_dir(directory.path().join("recycle-bin/records"))
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn failed_terminal_preserves_the_last_stage_and_offer_completes() {
        let directory = tempdir().unwrap();
        let mut session = warehouse::create(directory.path()).unwrap();
        let record = create(&mut session, request("终态测试", "开发")).unwrap();
        let interview = record
            .stages
            .iter()
            .find(|stage| stage.stable_key == "interview")
            .unwrap()
            .id
            .clone();
        let failure = record
            .stages
            .iter()
            .find(|stage| stage.stable_key == "failed_terminal")
            .unwrap()
            .id
            .clone();
        let offer = record
            .stages
            .iter()
            .find(|stage| stage.stable_key == "offer")
            .unwrap()
            .id
            .clone();
        let progressed = change_stage(
            &mut session,
            ChangeStageRequest {
                application_id: record.record.id.clone(),
                stage_id: interview.clone(),
                stage_state: "awaitingResult".into(),
                revision: 1,
                notes: "".into(),
            },
        )
        .unwrap();
        let failed = change_stage(
            &mut session,
            ChangeStageRequest {
                application_id: record.record.id.clone(),
                stage_id: failure,
                stage_state: "pending".into(),
                revision: progressed.record.revision,
                notes: "未通过".into(),
            },
        )
        .unwrap();
        assert_eq!(
            failed.record.current_stage_id.as_deref(),
            Some(interview.as_str())
        );
        assert_eq!(failed.record.current_stage_state, "failed");
        assert_eq!(
            failed.record.current_stage_order,
            progressed.record.current_stage_order
        );
        let accepted = change_stage(
            &mut session,
            ChangeStageRequest {
                application_id: record.record.id,
                stage_id: offer,
                stage_state: "pending".into(),
                revision: failed.record.revision,
                notes: "".into(),
            },
        )
        .unwrap();
        assert_eq!(accepted.record.current_stage_state, "completed");
    }

    #[test]
    fn clearing_application_date_is_respected_on_subsequent_applied_transitions() {
        let directory = tempdir().unwrap();
        let mut session = warehouse::create(directory.path()).unwrap();
        let record = create(&mut session, request("日期测试", "开发")).unwrap();
        let stage = record
            .stages
            .iter()
            .find(|stage| stage.stable_key == "applied")
            .unwrap()
            .id
            .clone();
        let changed = change_stage(
            &mut session,
            ChangeStageRequest {
                application_id: record.record.id.clone(),
                stage_id: stage.clone(),
                stage_state: "awaitingResult".into(),
                revision: 1,
                notes: "".into(),
            },
        )
        .unwrap();
        session
            .connection_mut()
            .unwrap()
            .execute(
                "UPDATE applications SET application_date = NULL WHERE id = ?1",
                [&record.record.id],
            )
            .unwrap();
        let changed = change_stage(
            &mut session,
            ChangeStageRequest {
                application_id: record.record.id,
                stage_id: stage,
                stage_state: "completed".into(),
                revision: changed.record.revision,
                notes: "".into(),
            },
        )
        .unwrap();
        assert!(changed.record.application_date.is_none());
    }

    #[test]
    fn workflow_templates_are_independent_and_reject_new_terminal_types() {
        let directory = tempdir().unwrap();
        let mut session = warehouse::create(directory.path()).unwrap();
        let first = create(&mut session, request("流程测试", "开发")).unwrap();
        let second = create(&mut session, request("流程测试", "测试")).unwrap();
        assert!(
            save_workflow_stage(
                &mut session,
                WorkflowStageRequest {
                    application_id: first.record.id.clone(),
                    revision: first.record.revision,
                    id: None,
                    display_name: "非允许终态".into(),
                    color: "#2563eb".into(),
                    is_terminal: true,
                    terminal_outcome: Some("custom".into())
                }
            )
            .is_err()
        );
        save_workflow_stage(
            &mut session,
            WorkflowStageRequest {
                application_id: first.record.id.clone(),
                revision: first.record.revision,
                id: None,
                display_name: "主管沟通".into(),
                color: "#2563eb".into(),
                is_terminal: false,
                terminal_outcome: None,
            },
        )
        .unwrap();
        save_workflow_as_template(&mut session, &first.record.id, "新流程", true).unwrap();
        let third = create(&mut session, request("新公司", "开发")).unwrap();
        assert!(
            third
                .stages
                .iter()
                .any(|stage| stage.display_name == "主管沟通")
        );
        assert!(
            !get(&session, &second.record.id)
                .unwrap()
                .stages
                .iter()
                .any(|stage| stage.display_name == "主管沟通")
        );
        assert_eq!(third.record.current_stage_name, "准备投递");
    }

    #[test]
    fn interview_edits_are_revision_checked_and_persist_all_fields() {
        let directory = tempdir().unwrap();
        let mut session = warehouse::create(directory.path()).unwrap();
        let record = create(&mut session, request("轮次测试", "开发")).unwrap();
        let initial = InterviewRoundRequest {
            application_id: record.record.id.clone(),
            revision: record.record.revision,
            id: None,
            display_name: "主管面".into(),
            state: "awaitingResult".into(),
            scheduled_at_utc: Some("2026-09-04T11:30:00.123456+08:00".into()),
            completed_at_utc: Some("2026-09-04T12:00:00+08:00".into()),
            result: "待通知".into(),
            notes: "项目经历交流\n补充材料".into(),
        };
        let saved = save_interview_round(&mut session, initial.clone()).unwrap();
        assert_eq!(saved.record.revision, 2);
        assert_eq!(
            saved.record.status_updated_at_utc,
            record.record.status_updated_at_utc
        );
        assert!(matches!(
            save_interview_round(&mut session, initial.clone()),
            Err(CoreError::RevisionConflict)
        ));
        let round_id = saved.interview_rounds[0].id.clone();
        let mut edit = initial;
        edit.id = Some(round_id.clone());
        edit.revision = saved.record.revision;
        edit.result = "通过".into();
        edit.state = "completed".into();
        save_interview_round(&mut session, edit.clone()).unwrap();
        assert!(matches!(
            delete_interview_round(&mut session, &record.record.id, &round_id, 2),
            Err(CoreError::RevisionConflict)
        ));
        drop(session);
        let mut read_only =
            warehouse::open(directory.path(), warehouse::WarehouseAccessMode::ReadOnly).unwrap();
        let reopened = get(&read_only, &record.record.id).unwrap();
        let round = &reopened.interview_rounds[0];
        assert_eq!(round.scheduled_at_utc, edit.scheduled_at_utc);
        assert_eq!(round.completed_at_utc, edit.completed_at_utc);
        assert_eq!(round.result, "通过");
        assert_eq!(round.notes, edit.notes);
        assert_eq!(round.state, "completed");
        edit.revision = reopened.record.revision;
        assert!(save_interview_round(&mut read_only, edit.clone()).is_err());
        assert!(
            delete_interview_round(&mut read_only, &record.record.id, &round_id, edit.revision)
                .is_err()
        );
        drop(read_only);
        let mut session =
            warehouse::open(directory.path(), warehouse::WarehouseAccessMode::Write).unwrap();
        edit.scheduled_at_utc = None;
        edit.completed_at_utc = None;
        let cleared = save_interview_round(&mut session, edit).unwrap();
        assert!(cleared.interview_rounds[0].scheduled_at_utc.is_none());
        assert!(cleared.interview_rounds[0].completed_at_utc.is_none());
        let deleted = delete_interview_round(
            &mut session,
            &record.record.id,
            &round_id,
            cleared.record.revision,
        )
        .unwrap();
        assert!(deleted.interview_rounds.is_empty());
    }

    #[test]
    fn workflow_deletion_is_atomic_revision_checked_and_preserves_history() {
        let directory = tempdir().unwrap();
        let mut session = warehouse::create(directory.path()).unwrap();
        let record = create(&mut session, request("流程保护测试", "开发")).unwrap();
        let stage_request = WorkflowStageRequest {
            application_id: record.record.id.clone(),
            revision: 1,
            id: None,
            display_name: "主管沟通".into(),
            color: "#123456".into(),
            is_terminal: false,
            terminal_outcome: None,
        };
        let saved = save_workflow_stage(&mut session, stage_request.clone()).unwrap();
        let custom_id = saved
            .stages
            .iter()
            .find(|stage| stage.display_name == "主管沟通")
            .unwrap()
            .id
            .clone();
        assert!(matches!(
            save_workflow_stage(&mut session, stage_request),
            Err(CoreError::RevisionConflict)
        ));
        assert!(matches!(
            delete_workflow_stage(&mut session, &record.record.id, &custom_id, 1),
            Err(CoreError::RevisionConflict)
        ));
        session.connection_mut().unwrap().execute_batch(
            "CREATE TRIGGER fail_revision_update BEFORE UPDATE ON applications BEGIN SELECT RAISE(ABORT, 'injected'); END;"
        ).unwrap();
        assert!(delete_workflow_stage(&mut session, &record.record.id, &custom_id, 2).is_err());
        assert!(
            get(&session, &record.record.id)
                .unwrap()
                .stages
                .iter()
                .any(|stage| stage.id == custom_id)
        );
        assert_eq!(get(&session, &record.record.id).unwrap().record.revision, 2);
        session
            .connection_mut()
            .unwrap()
            .execute_batch("DROP TRIGGER fail_revision_update")
            .unwrap();
        let visited = change_stage(
            &mut session,
            ChangeStageRequest {
                application_id: record.record.id.clone(),
                stage_id: custom_id.clone(),
                stage_state: "awaitingResult".into(),
                revision: 2,
                notes: "等待主管反馈".into(),
            },
        )
        .unwrap();
        let returned = change_stage(
            &mut session,
            ChangeStageRequest {
                application_id: record.record.id.clone(),
                stage_id: record.record.current_stage_id.clone().unwrap(),
                stage_state: "pending".into(),
                revision: visited.record.revision,
                notes: "".into(),
            },
        )
        .unwrap();
        assert!(matches!(
            delete_workflow_stage(
                &mut session,
                &record.record.id,
                &custom_id,
                returned.record.revision
            ),
            Err(CoreError::Validation)
        ));
        let renamed = save_workflow_stage(
            &mut session,
            WorkflowStageRequest {
                application_id: record.record.id.clone(),
                revision: returned.record.revision,
                id: Some(custom_id.clone()),
                display_name: "主管面".into(),
                color: "#654321".into(),
                is_terminal: false,
                terminal_outcome: None,
            },
        )
        .unwrap();
        assert!(
            renamed
                .history
                .iter()
                .any(|event| event.stage_id.as_deref() == Some(&custom_id)
                    && event.stage_name_snapshot == "主管沟通")
        );
        assert_eq!(
            renamed.record.status_updated_at_utc,
            returned.record.status_updated_at_utc
        );
        let unused = save_workflow_stage(
            &mut session,
            WorkflowStageRequest {
                application_id: record.record.id.clone(),
                revision: renamed.record.revision,
                id: None,
                display_name: "可移除阶段".into(),
                color: "#123456".into(),
                is_terminal: false,
                terminal_outcome: None,
            },
        )
        .unwrap();
        let unused_id = &unused
            .stages
            .iter()
            .find(|stage| stage.display_name == "可移除阶段")
            .unwrap()
            .id;
        let deleted = delete_workflow_stage(
            &mut session,
            &record.record.id,
            unused_id,
            unused.record.revision,
        )
        .unwrap();
        assert!(!deleted.stages.iter().any(|stage| &stage.id == unused_id));
        assert_eq!(deleted.record.revision, unused.record.revision + 1);
    }

    #[test]
    fn missing_template_source_cannot_replace_the_default() {
        let directory = tempdir().unwrap();
        let mut session = warehouse::create(directory.path()).unwrap();
        let before = list_workflow_templates(&session).unwrap();
        assert!(matches!(
            save_workflow_as_template(&mut session, "missing", "空模板", true),
            Err(CoreError::NotFound)
        ));
        let after = list_workflow_templates(&session).unwrap();
        assert_eq!(after.len(), before.len());
        assert_eq!(
            after
                .iter()
                .find(|template| template.is_default)
                .unwrap()
                .id,
            before
                .iter()
                .find(|template| template.is_default)
                .unwrap()
                .id
        );
    }

    #[test]
    fn default_views_and_pagination_survive_reopening() {
        let directory = tempdir().unwrap();
        let mut session = warehouse::create(directory.path()).unwrap();
        set_page_size(&mut session, 100).unwrap();
        save_view(&mut session, crate::domain::SavedViewRequest { id: None, revision: None, name: "开发岗位".into(),
            layout: serde_json::json!({"columns":[{"key":"companyName","visible":true,"pinned":true,"width":180}]}),
            sort: serde_json::json!([{"key":"createdAtUtc","direction":"desc"}]),
            filter: serde_json::json!({"search":"开发","companyTypes":[],"stages":[]}),
            group: None, is_default: true }).unwrap();
        drop(session);
        let reopened =
            warehouse::open(directory.path(), warehouse::WarehouseAccessMode::Write).unwrap();
        assert_eq!(page_size(&reopened).unwrap(), 100);
        let views = list_views(&reopened).unwrap();
        assert_eq!(views[0].name, "开发岗位");
        assert!(views[0].is_default);
        assert_eq!(views[0].layout["columns"][0]["width"], 180);
    }
}
