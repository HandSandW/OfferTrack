//! Public write schema shared by CLI capability discovery and MCP tools/list.
use serde_json::{Value, json};

fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
pub fn schema() -> Value {
    let text = json!({"type":"string"});
    let optional = json!({"type":["string","null"]});
    let revision = json!({"type":"integer","minimum":1});
    let mut fields = json!({});
    for name in [
        "company_name",
        "company_type",
        "industry",
        "position_name",
        "position_category",
        "work_location",
        "position_description",
        "notes",
    ] {
        fields[name] = text.clone();
    }
    for name in [
        "application_date",
        "application_url",
        "announcement_url",
        "company_url",
        "position_url",
    ] {
        fields[name] = optional.clone();
    }
    fields["tags"] = json!({"type":"array","items":{"type":"string","minLength":1,"maxLength":40},"maxItems":100});
    fields["custom_fields"] = json!({"type":"object","description":"Field ID to typed JSON value; unspecified values preserved, null clears. Definitions from write_status."});
    let mut patch = object(fields, &[]);
    patch["minProperties"] = 1.into();
    let mut actions = Vec::new();
    for name in ["update_fields", "append_notes", "change_stage"] {
        let mut p = json!({"operation":{"const":name},"application_id":text,"revision":revision});
        let mut required = vec!["operation", "application_id", "revision"];
        match name {
            "update_fields" => {
                p["fields"] = patch.clone();
                required.push("fields");
            }
            "append_notes" => {
                p["text"] = text.clone();
                required.push("text");
            }
            _ => {
                p["stage_id"] = text.clone();
                p["state_key"] = text.clone();
                p["notes"] = text.clone();
                required.extend(["stage_id", "state_key"]);
            }
        }
        actions.push(object(p, &required));
    }
    actions.push(object(
        json!({"operation":{"const":"create_task"},"application_id":optional,
        "application_revision":{"type":["integer","null"],"minimum":1},"title":text,"notes":text,
        "priority":{"type":"string","enum":["low","normal","high"],"default":"normal"},
        "due_at_utc":optional,"remind_at_utc":optional}),
        &["operation", "title"],
    ));
    actions.push(object(json!({"operation":{"const":"create_event"},"application_id":text,"application_revision":revision,
        "event_type":{"type":"string","enum":["assessment","writtenExam","interview","signing","other"]},"title":text,
        "starts_at_utc":optional,"deadline_at_utc":optional,"interview_round_id":optional,"location":text,"meeting_url":optional,
        "result":text,"notes":text}), &["operation","application_id","application_revision","event_type","title"]));
    object(
        json!({"version":{"const":1},"warehouse_id":{"type":"string","format":"uuid"},"request_id":{"type":"string","format":"uuid"},
        "source":{"type":"string","minLength":1,"maxLength":200,"description":"Client-provided label, not authenticated identity"},
        "actions":{"type":"array","minItems":1,"maxItems":50,"items":{"oneOf":actions}}}),
        &["version", "warehouse_id", "request_id", "source", "actions"],
    )
}
