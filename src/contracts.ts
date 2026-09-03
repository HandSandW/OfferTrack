export type WarehouseAccessMode = "write" | "readOnly";

export type StorageRiskCode =
  "networkLocation" | "cloudSyncLocation" | "removableDrive";

export interface StorageWarning {
  code: StorageRiskCode;
  message: string;
}

export interface WarehouseSummary {
  warehouseId: string;
  formatVersion: number;
  displayPath: string;
  accessMode: WarehouseAccessMode;
  warnings: StorageWarning[];
}

export interface StartupState {
  rememberedWarehousePath: string | null;
  activeWarehouse: WarehouseSummary | null;
}

export interface AppErrorPayload {
  code: string;
  message: string;
  retryable: boolean;
}

export type ApplicationScope = "active" | "archived" | "trash";
export type DuplicateMode = "companyInfo" | "fullRecord";
export type StageState =
  | "pending"
  | "awaitingParticipation"
  | "awaitingCompletion"
  | "awaitingResult"
  | "completed"
  | "failed";

export interface Tag {
  id: string;
  name: string;
  color: string;
  scope: string;
}

export interface WorkflowStage {
  id: string;
  stableKey: string;
  displayName: string;
  stageKind: string;
  displayOrder: number;
  color: string;
  isTerminal: boolean;
  terminalOutcome: string | null;
}

export interface WorkflowTemplate {
  id: string;
  name: string;
  description: string;
  isDefault: boolean;
  stageCount: number;
  revision: number;
}

export interface WorkflowTemplateDetail extends WorkflowTemplate {
  stages: WorkflowStage[];
  auxiliaryStates: AuxiliaryState[];
}

export interface AuxiliaryState {
  id: string;
  stableKey: string;
  displayName: string;
  semanticKind: StageState;
  displayOrder: number;
  inUse: boolean;
}
export interface AuxiliaryStateEdit {
  id: string | null;
  displayName: string;
  semanticKind: StageState;
}
export interface UpdateAuxiliaryStatesRequest {
  ownerId: string;
  revision: number;
  states: AuxiliaryStateEdit[];
}

export interface TemplateStageEdit {
  id: string | null;
  displayName: string;
  color: string;
}

export interface UpdateWorkflowTemplateRequest {
  id: string;
  revision: number;
  name: string;
  description: string;
  stages: TemplateStageEdit[];
}

export interface WorkflowEvent {
  id: string;
  stageId: string | null;
  stageNameSnapshot: string;
  previousState: string | null;
  nextState: string;
  previousStateNameSnapshot: string | null;
  nextStateNameSnapshot: string;
  previousStateKindSnapshot: StageState | null;
  nextStateKindSnapshot: StageState | null;
  notes: string;
  occurredAtUtc: string;
  actorType: string;
}

export interface InterviewRound {
  id: string;
  sequenceNumber: number;
  displayName: string;
  state: string;
  scheduledAtUtc: string | null;
  completedAtUtc: string | null;
  result: string;
  notes: string;
}

export interface DocumentEntry {
  id: string;
  relativePath: string;
  displayName: string;
  mediaType: string | null;
  sizeBytes: number | null;
  modifiedAtUtc: string | null;
  missing: boolean;
}
export interface TrashDocumentRequest {
  applicationId: string;
  documentId: string;
  expectedRelativePath: string;
}
export interface DocumentTrashEntry {
  id: string;
  documentId: string;
  applicationId: string;
  companyName: string;
  positionName: string;
  displayName: string;
  originalRelativePath: string;
  deletedAtUtc: string;
  parentDeleted: boolean;
  fileState:
    | "available"
    | "missing"
    | "wrongType"
    | "busy"
    | "accessDenied"
    | "unsafe"
    | "unavailable";
}
export interface RestoredDocument {
  id: string;
  applicationId: string;
  documentId: string;
  relativePath: string;
  relocated: boolean;
}
export interface DocumentTrashChallenge {
  confirmationToken: string;
  itemIds: string[];
  missingCount: number;
}
export interface DocumentTrashPurged {
  deletedIds: string[];
  failed: { id: string; error: AppErrorPayload }[];
}

export interface ApplicationListItem {
  id: string;
  shortId: string;
  createdAtUtc: string;
  applicationDate: string | null;
  companyName: string;
  companyType: string;
  industry: string;
  positionName: string;
  positionCategory: string;
  workLocation: string;
  applicationUrl: string | null;
  announcementUrl: string | null;
  companyUrl: string | null;
  positionUrl: string | null;
  positionDescription: string;
  notes: string;
  folderRelativePath: string;
  folderNormalizationPending: boolean;
  currentStageId: string | null;
  currentStageName: string;
  currentStageKey: string | null;
  currentStageTerminal: boolean;
  currentStageState: string;
  currentStateName: string;
  currentStateKind: StageState | null;
  currentStageOrder: number;
  currentStageProgress: number;
  currentStageColor: string;
  statusUpdatedAtUtc: string;
  updatedAtUtc: string;
  archivedAtUtc: string | null;
  deletedAtUtc: string | null;
  revision: number;
  tags: Tag[];
  documentCount: number;
  documentNames: string[];
  customFields: Record<string, unknown>;
}

export interface ApplicationDetail extends ApplicationListItem {
  customFields: Record<string, unknown>;
  stages: WorkflowStage[];
  auxiliaryStates: AuxiliaryState[];
  history: WorkflowEvent[];
  interviewRounds: InterviewRound[];
  documents: DocumentEntry[];
}

export interface CreateApplicationRequest {
  companyName: string;
  positionName: string;
  companyType: string;
  industry: string;
  positionCategory: string;
  workLocation: string;
}

export interface UpdateApplicationRequest {
  id: string;
  revision: number;
  companyName: string;
  companyType: string;
  industry: string;
  positionName: string;
  positionCategory: string;
  workLocation: string;
  applicationDate: string | null;
  applicationUrl: string | null;
  announcementUrl: string | null;
  companyUrl: string | null;
  positionUrl: string | null;
  positionDescription: string;
  notes: string;
  tags: string[];
  customFields: Record<string, unknown>;
}

export interface FieldDefinition {
  id: string;
  revision: number;
  key: string;
  displayName: string;
  fieldType: string;
  config: unknown;
  displayOrder: number;
  isVisible: boolean;
}

export interface SavedView {
  id: string;
  revision: number;
  name: string;
  layout: ViewLayout;
  sort: SortRule[];
  filter: FilterState;
  group: GroupKey | null;
  isDefault: boolean;
}

export interface SavedViewChange {
  view: SavedView;
  views: SavedView[];
}
export interface ViewSnapshot {
  layout: ViewLayout;
  sort: SortRule[];
  filter: FilterState;
  group: GroupKey | null;
}
export interface SaveViewRequest extends ViewSnapshot {
  id: string | null;
  revision: number | null;
  name: string;
  isDefault: boolean;
}
export interface FieldDefinitionRequest {
  id: string | null;
  revision: number | null;
  displayName: string;
  fieldType: string;
  config: unknown;
}

export interface ColumnSetting {
  key: string;
  visible: boolean;
  width: number;
  pinned: boolean;
}

export interface ViewLayout {
  columns: ColumnSetting[];
}
export interface SortRule {
  key: string;
  direction: "asc" | "desc";
}
export type GroupKey = "companyType" | "currentStageName" | "workLocation";
export interface FilterState {
  search: string;
  companyTypes: string[];
  stages: string[];
  businessState?: BusinessState;
}

export type BusinessState =
  "preparing" | "inProgress" | "awaitingResult" | "ended";

export interface UnlinkedFolder {
  name: string;
  hidden: boolean;
}

export interface ApplicationDirectories {
  version: number;
  directories: { relativePath: string; empty: boolean }[];
}
export interface RenameDocumentRequest {
  applicationId: string;
  documentId: string;
  expectedRelativePath: string;
  newName: string;
}

export type PathState =
  | "available"
  | "missing"
  | "wrongType"
  | "busy"
  | "accessDenied"
  | "unsafe"
  | "unavailable";
export interface PathObservation {
  relativePath: string | null;
  state: PathState;
}
export interface RecoveryDiagnostics {
  version: number;
  totalPending: number;
  items: {
    id: string;
    kind: string;
    source: PathObservation;
    target: PathObservation;
    identityRecorded: boolean | null;
  }[];
}
export interface TrashEntry {
  applicationId: string;
  companyName: string;
  positionName: string;
  deletedAtUtc: string;
  originalRelativePath: string;
  trashRelativePath: string;
}
export type InstalledBrowser = "edge" | "chrome" | "firefox";
export interface RestoreResult {
  applicationId: string;
  folderRelativePath: string;
  renamed: boolean;
}
export interface EmptyTrashChallenge {
  warehouseId: string;
  confirmationToken: string;
  itemCount: number;
}
export interface EmptyTrashResult {
  deletedCount: number;
  failedApplicationIds: string[];
}
export interface DuplicatePreview {
  mode: DuplicateMode;
  fileSizeBytes: number;
  editableFieldCount: number;
}
