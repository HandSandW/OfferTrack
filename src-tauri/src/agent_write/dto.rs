use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u32,
    pub warehouse_id: Uuid,
    pub request_id: Uuid,
    /// Client-provided label, NOT an authenticated identity.
    pub source: String,
    pub actions: Vec<Action>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    UpdateFields {
        application_id: String,
        revision: i64,
        fields: BTreeMap<String, Value>,
    },
    AppendNotes {
        application_id: String,
        revision: i64,
        text: String,
    },
    ChangeStage {
        application_id: String,
        revision: i64,
        stage_id: String,
        state_key: String,
        #[serde(default)]
        notes: String,
    },
    CreateTask {
        application_id: Option<String>,
        application_revision: Option<i64>,
        title: String,
        #[serde(default)]
        notes: String,
        #[serde(default = "normal")]
        priority: String,
        due_at_utc: Option<String>,
        remind_at_utc: Option<String>,
    },
    CreateEvent {
        application_id: String,
        application_revision: i64,
        event_type: String,
        title: String,
        starts_at_utc: Option<String>,
        deadline_at_utc: Option<String>,
        interview_round_id: Option<String>,
        #[serde(default)]
        location: String,
        meeting_url: Option<String>,
        #[serde(default)]
        result: String,
        #[serde(default)]
        notes: String,
    },
}
fn normal() -> String {
    "normal".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Applied {
    pub version: u32,
    pub warehouse_id: Uuid,
    pub request_id: Uuid,
    pub backup_id: Uuid,
    pub committed_at_utc: String,
    pub results: Vec<Changed>,
    pub snapshot_refresh_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Changed {
    pub entity_type: String,
    pub id: String,
    pub revision: i64,
}
