//! Public v1 DTOs are deliberately separate from the desktop IPC/domain types.
//! Explicit field lists prevent new internal fields from silently becoming public.
use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

use crate::{domain, recruitment, tasks};

macro_rules! dto {
    ($name:ident, $source:ty, { $($field:ident: $ty:ty),* $(,)? }) => {
        #[derive(Debug, Clone, Serialize)]
        pub struct $name { $(pub $field: $ty),* }
        impl From<$source> for $name {
            fn from(value: $source) -> Self { Self { $($field: value.$field),* } }
        }
        impl Shape for $name {
            fn shape() -> Value {
                let properties = serde_json::Map::from_iter([
                    $((stringify!($field).to_owned(), <$ty as Shape>::shape())),*
                ]);
                object(Value::Object(properties))
            }
        }
    };
}

dto!(Stage, domain::WorkflowStage, {
    id: String, stable_key: String, display_name: String, stage_kind: String,
    display_order: i64, color: String, is_terminal: bool, terminal_outcome: Option<String>,
});
dto!(AuxiliaryState, domain::AuxiliaryState, {
    id: String, stable_key: String, display_name: String, semantic_kind: String,
    display_order: i64, in_use: bool,
});
dto!(History, domain::WorkflowEvent, {
    id: String, stage_id: Option<String>, stage_name_snapshot: String,
    previous_state: Option<String>, next_state: String,
    previous_state_name_snapshot: Option<String>, next_state_name_snapshot: String,
    previous_state_kind_snapshot: Option<String>, next_state_kind_snapshot: Option<String>,
    notes: String, occurred_at_utc: String, actor_type: String,
});
dto!(Interview, domain::InterviewRound, {
    id: String, sequence_number: i64, display_name: String, state: String,
    scheduled_at_utc: Option<String>, completed_at_utc: Option<String>, result: String, notes: String,
});
dto!(Tag, domain::Tag, { id: String, name: String, color: String, scope: String });
dto!(Field, domain::FieldDefinition, {
    id: String, revision: i64, key: String, display_name: String, field_type: String,
    config: Value, display_order: i64, is_visible: bool,
});
dto!(Task, tasks::Task, {
    id: String, revision: i64, application_id: Option<String>, application_label: Option<String>,
    application_archived: bool, title: String, notes: String, priority: String,
    due_at_utc: Option<String>, remind_at_utc: Option<String>, completed_at_utc: Option<String>,
    created_at_utc: String, updated_at_utc: String,
});
dto!(Event, recruitment::Event, {
    id: String, revision: i64, application_id: Option<String>, application_label: Option<String>,
    application_archived: bool, application_terminal: bool, event_type: String, title: String,
    notes: String, starts_at_utc: Option<String>, deadline_at_utc: Option<String>,
    completed_at_utc: Option<String>, finished: bool, interview_round_id: Option<String>,
    interview_round_name: Option<String>, location: String, meeting_url: Option<String>,
    result: String, created_at_utc: String, updated_at_utc: String, source_version: String,
});

dto!(Record, domain::ApplicationListItem, {
    id: String, short_id: String, revision: i64, created_at_utc: String,
    application_date: Option<String>, company_name: String, company_type: String, industry: String,
    position_name: String, position_category: String, work_location: String,
    application_url: Option<String>, announcement_url: Option<String>, company_url: Option<String>,
    position_url: Option<String>, position_description: String, notes: String,
    folder_relative_path: String, folder_normalization_pending: bool,
    current_stage_id: Option<String>, current_stage_name: String, current_stage_state: String,
    current_state_name: String, current_state_kind: Option<String>, current_stage_progress: i64,
    status_updated_at_utc: String, updated_at_utc: String, archived_at_utc: Option<String>,
    custom_fields: BTreeMap<String, Value>,
});

#[derive(Debug, Clone, Serialize)]
pub struct Document {
    pub id: String,
    /// Warehouse-root relative, NEVER relative to the application's directory.
    pub relative_path: String,
    pub display_name: String,
    pub media_type: Option<String>,
    pub size_bytes: Option<i64>,
    pub modified_at_utc: Option<String>,
    /// Last indexed observation, not a live filesystem assertion.
    pub indexed_missing: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Application {
    #[serde(flatten)]
    pub record: Record,
    pub tags: Vec<Tag>,
    pub stages: Vec<Stage>,
    pub auxiliary_states: Vec<AuxiliaryState>,
    pub history: Vec<History>,
    pub interview_rounds: Vec<Interview>,
    pub documents: Vec<Document>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub active_applications: usize,
    pub archived_applications: usize,
    pub offers: usize,
    pub failed_applications: usize,
    pub open_tasks: usize,
    pub open_events: usize,
    pub indexed_documents: usize,
    pub indexed_missing_documents: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Dataset {
    pub version: u32,
    pub warehouse_id: String,
    pub warehouse_format_version: u32,
    pub generated_at_utc: String,
    pub summary: Summary,
    pub fields: Vec<Field>,
    pub applications: Vec<Application>,
    pub tasks: Vec<Task>,
    pub events: Vec<Event>,
}

trait Shape {
    fn shape() -> Value;
}
impl Shape for String {
    fn shape() -> Value {
        serde_json::json!({"type":"string"})
    }
}
impl Shape for i64 {
    fn shape() -> Value {
        serde_json::json!({"type":"integer"})
    }
}
impl Shape for bool {
    fn shape() -> Value {
        serde_json::json!({"type":"boolean"})
    }
}
impl Shape for Value {
    fn shape() -> Value {
        serde_json::json!({})
    }
}
impl<T: Shape> Shape for Option<T> {
    fn shape() -> Value {
        serde_json::json!({"anyOf":[T::shape(),{"type":"null"}]})
    }
}
impl<T: Shape> Shape for Vec<T> {
    fn shape() -> Value {
        serde_json::json!({"type":"array","items":T::shape()})
    }
}
impl<T: Shape> Shape for BTreeMap<String, T> {
    fn shape() -> Value {
        serde_json::json!({"type":"object","additionalProperties":T::shape()})
    }
}
fn object(properties: Value) -> Value {
    let required: Vec<_> = properties
        .as_object()
        .expect("schema properties")
        .keys()
        .collect();
    serde_json::json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
impl Shape for Document {
    fn shape() -> Value {
        object(
            serde_json::json!({"id":String::shape(),"relative_path":String::shape(),
            "display_name":String::shape(),"media_type":Option::<String>::shape(),
            "size_bytes":Option::<i64>::shape(),"modified_at_utc":Option::<String>::shape(),"indexed_missing":bool::shape()}),
        )
    }
}
impl Shape for Application {
    fn shape() -> Value {
        let mut properties = Record::shape()["properties"].clone();
        properties.as_object_mut().expect("record schema").extend(serde_json::json!({
            "tags":Vec::<Tag>::shape(),"stages":Vec::<Stage>::shape(),"auxiliary_states":Vec::<AuxiliaryState>::shape(),
            "history":Vec::<History>::shape(),"interview_rounds":Vec::<Interview>::shape(),"documents":Vec::<Document>::shape()
        }).as_object().expect("detail schema").clone());
        object(properties)
    }
}

/// Generated from the same explicit field lists as the DTOs; no internal schema introspection.
pub fn schema() -> Value {
    serde_json::json!({"$schema":"https://json-schema.org/draft/2020-12/schema",
        "title":"OfferTrack Agent v1 JSONL entities", "$comment":"Version and warehouse identity are in manifest.json. Validate each JSONL line against the corresponding $defs entry; nullable fields are present, not omitted.",
        "$defs":{"Application":Application::shape(),"Task":Task::shape(),"Event":Event::shape(),"Field":Field::shape()}})
}
