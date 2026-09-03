use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateApplicationRequest {
    pub company_name: String,
    pub position_name: String,
    #[serde(default)]
    pub company_type: String,
    #[serde(default)]
    pub industry: String,
    #[serde(default)]
    pub position_category: String,
    #[serde(default)]
    pub work_location: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateApplicationRequest {
    pub id: String,
    pub revision: i64,
    pub company_name: String,
    pub company_type: String,
    pub industry: String,
    pub position_name: String,
    pub position_category: String,
    pub work_location: String,
    pub application_date: Option<String>,
    pub application_url: Option<String>,
    pub announcement_url: Option<String>,
    pub company_url: Option<String>,
    pub position_url: Option<String>,
    pub position_description: String,
    pub notes: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub custom_fields: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeStageRequest {
    pub application_id: String,
    pub stage_id: String,
    pub stage_state: String,
    pub revision: i64,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterviewRoundRequest {
    pub application_id: String,
    pub revision: i64,
    pub id: Option<String>,
    pub display_name: String,
    pub state: String,
    pub scheduled_at_utc: Option<String>,
    pub completed_at_utc: Option<String>,
    #[serde(default)]
    pub result: String,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowStageRequest {
    pub application_id: String,
    pub revision: i64,
    pub id: Option<String>,
    pub display_name: String,
    pub color: String,
    #[serde(default)]
    pub is_terminal: bool,
    pub terminal_outcome: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FieldDefinitionRequest {
    pub id: Option<String>,
    pub revision: Option<i64>,
    pub display_name: String,
    pub field_type: String,
    #[serde(default)]
    pub config: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimFolderRequest {
    pub folder_name: String,
    pub include_hidden: bool,
    pub application: CreateApplicationRequest,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SavedViewRequest {
    pub id: Option<String>,
    pub revision: Option<i64>,
    pub name: String,
    pub layout: serde_json::Value,
    pub sort: serde_json::Value,
    pub filter: serde_json::Value,
    pub group: Option<serde_json::Value>,
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ApplicationScope {
    Active,
    Archived,
    Trash,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DuplicateMode {
    CompanyInfo,
    FullRecord,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicatePreview {
    pub mode: DuplicateMode,
    pub file_size_bytes: u64,
    pub editable_field_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationListItem {
    pub id: String,
    pub short_id: String,
    pub created_at_utc: String,
    pub application_date: Option<String>,
    pub company_name: String,
    pub company_type: String,
    pub industry: String,
    pub position_name: String,
    pub position_category: String,
    pub work_location: String,
    pub application_url: Option<String>,
    pub announcement_url: Option<String>,
    pub company_url: Option<String>,
    pub position_url: Option<String>,
    pub position_description: String,
    pub notes: String,
    pub folder_relative_path: String,
    pub folder_normalization_pending: bool,
    pub current_stage_id: Option<String>,
    pub current_stage_name: String,
    pub current_stage_state: String,
    pub current_state_name: String,
    pub current_state_kind: Option<String>,
    pub current_stage_order: i64,
    pub current_stage_progress: i64,
    pub current_stage_color: String,
    pub status_updated_at_utc: String,
    pub updated_at_utc: String,
    pub archived_at_utc: Option<String>,
    pub deleted_at_utc: Option<String>,
    pub revision: i64,
    pub tags: Vec<Tag>,
    pub document_count: i64,
    pub document_names: Vec<String>,
    pub custom_fields: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationDetail {
    #[serde(flatten)]
    pub record: ApplicationListItem,
    pub stages: Vec<WorkflowStage>,
    pub auxiliary_states: Vec<AuxiliaryState>,
    pub history: Vec<WorkflowEvent>,
    pub interview_rounds: Vec<InterviewRound>,
    pub documents: Vec<DocumentEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    pub id: String,
    pub name: String,
    pub color: String,
    pub scope: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowStage {
    pub id: String,
    pub stable_key: String,
    pub display_name: String,
    pub stage_kind: String,
    pub display_order: i64,
    pub color: String,
    pub is_terminal: bool,
    pub terminal_outcome: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowTemplate {
    pub id: String,
    pub name: String,
    pub description: String,
    pub is_default: bool,
    pub stage_count: i64,
    pub revision: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowTemplateDetail {
    #[serde(flatten)]
    pub template: WorkflowTemplate,
    pub stages: Vec<WorkflowStage>,
    pub auxiliary_states: Vec<AuxiliaryState>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuxiliaryState {
    pub id: String,
    pub stable_key: String,
    pub display_name: String,
    pub semantic_kind: String,
    pub display_order: i64,
    pub in_use: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuxiliaryStateEdit {
    pub id: Option<String>,
    pub display_name: String,
    pub semantic_kind: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateAuxiliaryStatesRequest {
    pub owner_id: String,
    pub revision: i64,
    pub states: Vec<AuxiliaryStateEdit>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TemplateStageEdit {
    pub id: Option<String>,
    pub display_name: String,
    pub color: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateWorkflowTemplateRequest {
    pub id: String,
    pub revision: i64,
    pub name: String,
    pub description: String,
    pub stages: Vec<TemplateStageEdit>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReorderWorkflowRequest {
    pub application_id: String,
    pub revision: i64,
    pub stage_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowEvent {
    pub id: String,
    pub stage_id: Option<String>,
    pub stage_name_snapshot: String,
    pub previous_state: Option<String>,
    pub next_state: String,
    pub previous_state_name_snapshot: Option<String>,
    pub next_state_name_snapshot: String,
    pub previous_state_kind_snapshot: Option<String>,
    pub next_state_kind_snapshot: Option<String>,
    pub notes: String,
    pub occurred_at_utc: String,
    pub actor_type: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InterviewRound {
    pub id: String,
    pub sequence_number: i64,
    pub display_name: String,
    pub state: String,
    pub scheduled_at_utc: Option<String>,
    pub completed_at_utc: Option<String>,
    pub result: String,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentEntry {
    pub id: String,
    pub relative_path: String,
    pub display_name: String,
    pub media_type: Option<String>,
    pub size_bytes: Option<i64>,
    pub modified_at_utc: Option<String>,
    pub missing: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldDefinition {
    pub id: String,
    pub revision: i64,
    pub key: String,
    pub display_name: String,
    pub field_type: String,
    pub config: serde_json::Value,
    pub display_order: i64,
    pub is_visible: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedView {
    pub id: String,
    pub revision: i64,
    pub name: String,
    pub layout: serde_json::Value,
    pub sort: serde_json::Value,
    pub filter: serde_json::Value,
    pub group: Option<serde_json::Value>,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedViewChange {
    pub view: SavedView,
    pub views: Vec<SavedView>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ViewMetadataRequest {
    pub id: String,
    pub revision: i64,
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnlinkedFolder {
    pub name: String,
    pub hidden: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashEntry {
    pub application_id: String,
    pub company_name: String,
    pub position_name: String,
    pub deleted_at_utc: String,
    pub original_relative_path: String,
    pub trash_relative_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmptyTrashChallenge {
    pub warehouse_id: String,
    pub confirmation_token: String,
    pub item_count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmptyTrashResult {
    pub deleted_count: usize,
    pub failed_application_ids: Vec<String>,
}
