//! One-cell metadata writes. No file operations and no post-commit fallible reads.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub version: u32,
    pub id: String,
    pub revision: i64,
    pub key: String,
    pub value: Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Applied {
    pub record: ApplicationListItem,
    pub previous_value: Value,
    pub changed: bool,
}

// SQL identifiers come only from this allowlist, never from a caller's key.
fn column(key: &str) -> Option<&'static str> {
    Some(match key {
        "companyName" => "company_name",
        "companyType" => "company_type",
        "industry" => "industry",
        "positionName" => "position_name",
        "positionCategory" => "position_category",
        "workLocation" => "work_location",
        "applicationDate" => "application_date",
        "applicationUrl" => "application_url",
        "announcementUrl" => "announcement_url",
        "companyUrl" => "company_url",
        "positionUrl" => "position_url",
        "positionDescription" => "position_description",
        "notes" => "notes",
        _ => return None,
    })
}

pub fn apply(session: &mut WarehouseSession, request: Request) -> Result<Applied, CoreError> {
    if request.version != 1 || request.revision < 1 || request.value.to_string().len() > 400_000 {
        return Err(CoreError::Validation);
    }
    let tx = session
        .connection_mut()?
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|_| CoreError::DatabaseInvalid)?;
    let original = load_record(&tx, &request.id)?;
    if original.deleted_at_utc.is_some() {
        return Err(CoreError::NotFound);
    }
    if original.revision != request.revision {
        return Err(CoreError::RevisionConflict);
    }
    let key = request.key.as_str();
    let custom_id = key.strip_prefix("custom:");
    let mut value = request.value;
    let previous_value = if let Some(id) = custom_id {
        let definitions = load_field_definition_types(&tx)?;
        let (kind, config) = definitions.get(id).ok_or(CoreError::Validation)?;
        if !value.is_null() {
            validate_custom_value(kind, config, &value)?;
        }
        original
            .custom_fields
            .get(id)
            .cloned()
            .unwrap_or(Value::Null)
    } else if key == "tags" {
        let input = value.as_array().ok_or(CoreError::Validation)?;
        if input.len() > 100 {
            return Err(CoreError::Validation);
        }
        let mut names = Vec::<String>::new();
        for item in input {
            let name = item.as_str().ok_or(CoreError::Validation)?.trim();
            if name.is_empty() || name.chars().count() > 40 {
                return Err(CoreError::Validation);
            }
            if !names
                .iter()
                .any(|s| s.to_lowercase() == name.to_lowercase())
            {
                names.push(name.to_owned());
            }
        }
        value = serde_json::json!(names);
        serde_json::json!(original.tags.iter().map(|t| &t.name).collect::<Vec<_>>())
    } else {
        column(key).ok_or(CoreError::Validation)?;
        let nullable = key == "applicationDate" || key.ends_with("Url");
        if !(nullable && value.is_null()) {
            let text = value.as_str().ok_or(CoreError::Validation)?;
            if text.chars().count() > 100_000 {
                return Err(CoreError::Validation);
            }
            match key {
                "companyName" | "positionName" => validate_required_text(text)?,
                "companyType" => validate_company_type(text)?,
                "applicationDate" => {
                    NaiveDate::parse_from_str(text, "%Y-%m-%d")
                        .map_err(|_| CoreError::Validation)?;
                }
                _ if key.ends_with("Url") => validate_web_url(text)?,
                _ => (),
            }
            if matches!(
                key,
                "companyName" | "positionName" | "industry" | "positionCategory" | "workLocation"
            ) {
                value = Value::String(text.trim().into());
            }
        }
        serde_json::to_value(&original)
            .map_err(|_| CoreError::DatabaseInvalid)?
            .get(key)
            .cloned()
            .ok_or(CoreError::Validation)?
    };
    if previous_value == value {
        return Ok(Applied {
            record: original,
            previous_value,
            changed: false,
        });
    }
    let now = now_utc();
    if let Some(id) = custom_id {
        if value.is_null() {
            tx.execute(
                "DELETE FROM field_values WHERE application_id=?1 AND field_definition_id=?2",
                params![request.id, id],
            )
            .map_err(|_| CoreError::DatabaseInvalid)?;
        } else {
            tx.execute("INSERT INTO field_values (application_id, field_definition_id, value_json, updated_at_utc) VALUES (?1,?2,?3,?4) ON CONFLICT(application_id,field_definition_id) DO UPDATE SET value_json=excluded.value_json, updated_at_utc=excluded.updated_at_utc", params![request.id, id, value.to_string(), now])
                .map_err(|_| CoreError::DatabaseInvalid)?;
        }
    } else if key == "tags" {
        let names: Vec<String> =
            serde_json::from_value(value).map_err(|_| CoreError::Validation)?;
        replace_tags(&tx, &request.id, &names, &now)?;
    } else {
        let sql = format!(
            "UPDATE applications SET {}=?1 WHERE id=?2",
            column(key).ok_or(CoreError::Validation)?
        );
        tx.execute(&sql, params![value.as_str(), request.id])
            .map_err(|_| CoreError::DatabaseInvalid)?;
    }
    // Naming changes are durable before a separate, journalled folder-normalization action.
    tx.execute("UPDATE applications SET revision=revision+1, updated_at_utc=?1, folder_normalization_pending=CASE WHEN ?2 THEN 1 ELSE folder_normalization_pending END WHERE id=?3", params![now, matches!(key, "companyName" | "positionName"), request.id])
        .map_err(|_| CoreError::DatabaseInvalid)?;
    let record = load_record(&tx, &request.id)?;
    tx.commit().map_err(|_| CoreError::DatabaseInvalid)?;
    Ok(Applied {
        record,
        previous_value,
        changed: true,
    })
}

#[cfg(test)]
mod tests;
