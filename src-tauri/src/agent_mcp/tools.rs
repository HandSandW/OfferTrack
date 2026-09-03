//! Fixed tool catalogue. Business arguments deserialize through the shared v1 DTO.
use crate::{agent_access::Operation, error::CoreError};
use serde_json::{Value, json};

pub(super) const NAMES: [&str; 10] = [
    "describe",
    "summary",
    "list_applications",
    "get_application",
    "list_tasks",
    "list_events",
    "list_documents",
    "resolve_document",
    "write_status",
    "snapshot_status",
];

fn schema(name: &str) -> Value {
    let mut properties = json!({});
    let mut required = Vec::new();
    if ["list_applications", "list_tasks", "list_events"].contains(&name) {
        properties["offset"] = json!({"type":"integer","minimum":0,"maximum":10000,"default":0});
        properties["limit"] = json!({"type":"integer","minimum":1,"maximum":200,"default":50});
    }
    if name == "list_applications" {
        properties["scope"] =
            json!({"type":"string","enum":["all","active","archived"],"default":"all"});
        properties["search"] = json!({"type":"string","maxLength":500,"default":""});
    }
    for (field, applicable) in [
        ("id", name == "get_application"),
        (
            "application_id",
            ["list_documents", "resolve_document"].contains(&name),
        ),
        ("document_id", name == "resolve_document"),
    ] {
        if applicable {
            properties[field] = json!({"type":"string"});
            required.push(field);
        }
    }
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}

pub(super) fn list() -> Value {
    let descriptions = [
        "OfferTrack Agent v1 capabilities, limits and privacy notes. Read-only.",
        "Current counts across active AND archived applications, tasks, events and indexed documents; excludes deleted records.",
        "Page full applications, newest first. Includes full long texts, custom fields, workflow history, interview rounds and resume paths, not resume contents. Search matches text values. Default scope all includes archived; deleted records excluded.",
        "Read a full application by ID, including long texts, workflow/history, interview rounds, fields and indexed documents. Deleted records excluded.",
        "Page tasks, including general job-search tasks, completed tasks and archived applications; excludes deleted applications.",
        "Page recruitment events, including completed events and archived applications; excludes deleted applications.",
        "Read an application's last document index. Paths relative to warehouse root. Does not scan or read resume contents; indexed_missing is the last scan observation.",
        "Validate an indexed document by application/document ID and resolve its current absolute path. Does not read file contents. Path validity is a point-in-time observation, not durable permission.",
        "Read the current persistent Agent write permission and custom field definitions. Cannot enable writes. Desktop writable sessions must close before Agent writes.",
        "Read snapshot freshness against current indexed data and validate generation file hashes. Returns the generation's warehouse-relative path and check timestamp; never generates files. Not a live attachment scan.",
    ];
    let mut tools: Vec<Value> = NAMES.iter().zip(descriptions).map(|(name, description)| json!({
        "name":format!("offertrack_{name}"),"description":description,
        "inputSchema":schema(name),
        "annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}
    })).collect();
    tools.push(json!({"name":"offertrack_write","description":"Atomically modify allowed fields, append notes, change stages or create tasks/events (1-50 actions). Requires user-enabled permission and exclusive lock. Backed up and audited; no resume changes/deletion. Query current IDs/revisions first. After an uncertain result retry IDENTICAL request_id and content, never a new ID. Separately attempts derived snapshot refresh; snapshot_status errors do not undo committed writes.",
        "inputSchema":crate::agent_write::schema::schema(),"annotations":{"readOnlyHint":false,"destructiveHint":true,"idempotentHint":true,"openWorldHint":false}}));
    json!({"tools":tools})
}

pub(super) fn operation(name: &str, arguments: Value) -> Result<Operation, CoreError> {
    let mut object = arguments
        .as_object()
        .cloned()
        .ok_or(CoreError::Validation)?;
    // Do not let caller arguments override the fixed tool selected by the host.
    if object.contains_key("operation") {
        return Err(CoreError::Validation);
    }
    object.insert("operation".into(), name.into());
    let operation: Operation =
        serde_json::from_value(Value::Object(object)).map_err(|_| CoreError::Validation)?;
    match &operation {
        Operation::ListApplications {
            offset,
            limit,
            search,
            ..
        } => {
            if *offset > 10000 || !(1..=200).contains(limit) || search.chars().count() > 500 {
                return Err(CoreError::Validation);
            }
        }
        Operation::ListTasks { offset, limit } | Operation::ListEvents { offset, limit }
            if *offset > 10000 || !(1..=200).contains(limit) =>
        {
            return Err(CoreError::Validation);
        }
        _ => (),
    }
    Ok(operation)
}
