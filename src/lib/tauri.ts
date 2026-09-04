import { invoke } from "@tauri-apps/api/core";
import type {
  CellApplied,
  CellRequest,
} from "../features/applications/cellModel";
import type {
  AgentConnection,
  AgentSnapshot,
  AgentPermission,
  AgentAuditItem,
  SnapshotReport,
} from "../features/agent/contracts";
import type {
  ExportCatalog,
  ExportCreated,
  ExportRequest,
} from "../features/export/contracts";
import type {
  Overview,
  Task,
  SaveTask,
  ReminderRule,
  RecruitmentEvent,
  SaveEvent,
} from "../features/productivity/contracts";
import type {
  BatchRequest,
  BatchPreview,
  BatchApplied,
} from "../features/batch/contracts";
import type {
  BackupCatalog,
  BackupCreated,
  BackupPreview,
  DatabaseRestore,
  FullBackupPreview,
  FullBackupCreated,
  FullRestore,
  ExternalDatabasePreview,
  BackupTrashChallenge,
  BackupTrashResult,
} from "../features/backup/contracts";
import type {
  AppErrorPayload,
  ApplicationDetail,
  ApplicationDirectories,
  RenameDocumentRequest,
  TrashDocumentRequest,
  DocumentTrashEntry,
  RestoredDocument,
  DocumentTrashChallenge,
  DocumentTrashPurged,
  ApplicationListItem,
  ApplicationScope,
  CreateApplicationRequest,
  DuplicateMode,
  DuplicatePreview,
  EmptyTrashChallenge,
  EmptyTrashResult,
  FieldDefinition,
  FieldDefinitionRequest,
  SavedView,
  SavedViewChange,
  SaveViewRequest,
  TrashEntry,
  UnlinkedFolder,
  UpdateApplicationRequest,
  UpdateAuxiliaryStatesRequest,
  WorkflowTemplate,
  WorkflowTemplateDetail,
  UpdateWorkflowTemplateRequest,
  StartupState,
  WarehouseAccessMode,
  WarehouseSummary,
  PathObservation,
  RecoveryDiagnostics,
  InstalledBrowser,
  RestoreResult,
} from "../contracts";

export class OfferTrackError extends Error {
  readonly code: string;
  readonly retryable: boolean;

  constructor(payload: AppErrorPayload) {
    super(payload.message);
    this.name = "OfferTrackError";
    this.code = payload.code;
    this.retryable = payload.retryable;
  }
}

function normalizeError(error: unknown): OfferTrackError {
  if (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    "message" in error &&
    "retryable" in error
  ) {
    return new OfferTrackError(error as AppErrorPayload);
  }

  return new OfferTrackError({
    code: "UNEXPECTED_ERROR",
    message: "发生了未预期的错误，请重试。",
    retryable: true,
  });
}

async function call<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  try {
    const result = await invoke<T>(command, args);
    if (
      [
        "edit_application_cell",
        "rename_document",
        "trash_document",
        "restore_document",
        "save_task",
        "complete_task",
        "save_reminder_rules",
        "respond_to_reminder",
        "save_recruitment_event",
        "complete_recruitment_event",
      ].includes(command)
    ) {
      window.dispatchEvent(new Event("offertrack-data-changed"));
    }
    return result;
  } catch (error: unknown) {
    throw normalizeError(error);
  } finally {
    // Only changes to the Agent projection (and manual publication), not reads or auto checks.
    // File-operation failures may have committed a recoverable partial operation: check those too.
    if (SNAPSHOT_CHANGES.has(command)) {
      window.dispatchEvent(new Event("offertrack-snapshot-dirty"));
    }
  }
}

const SNAPSHOT_CHANGES = new Set([
  "create_application",
  "update_application",
  "edit_application_cell",
  "change_application_stage",
  "set_application_archived",
  "move_application_to_trash",
  "restore_application",
  "duplicate_application",
  "claim_application_folder",
  "apply_application_batch",
  "save_workflow_stage",
  "delete_workflow_stage",
  "reorder_application_workflow",
  "save_interview_round",
  "delete_interview_round",
  "update_application_states",
  "save_field_definition",
  "save_task",
  "complete_task",
  "save_recruitment_event",
  "complete_recruitment_event",
  "refresh_file_index",
  "scan_application_documents",
  "rename_document",
  "trash_document",
  "restore_document",
  "retry_folder_normalization",
  "create_agent_snapshot",
]);

export const desktopApi = {
  getApplicationDetailTarget: () =>
    call<{ applicationId: string; revision: number } | null>(
      "get_application_detail_target",
    ),
  setApplicationDetailTarget: (applicationId: string, show: boolean) =>
    call<void>("set_application_detail_target", { applicationId, show }),
  notifyApplicationDetailChanged: () =>
    call<void>("notify_application_detail_changed"),
  checkAgentSnapshot: (warehouseId: string, warehousePath: string) =>
    call<SnapshotReport>("check_agent_snapshot", {
      warehouseId,
      warehousePath,
    }),
  getAgentConnection: () => call<AgentConnection>("get_agent_connection"),
  createAgentSnapshot: () => call<AgentSnapshot>("create_agent_snapshot"),
  getAgentPermission: () => call<AgentPermission>("get_agent_permission"),
  setAgentPermission: (enabled: boolean, revision: number) =>
    call<AgentPermission>("set_agent_permission", { enabled, revision }),
  listAgentAudit: () => call<AgentAuditItem[]>("list_agent_audit"),
  getAgentAudit: (id: string) => call<unknown>("get_agent_audit", { id }),
  getExportCatalog: () => call<ExportCatalog>("get_export_catalog"),
  exportApplications: (parentDirectory: string, request: ExportRequest) =>
    call<ExportCreated>("export_applications", { parentDirectory, request }),
  listRecruitmentEvents: () =>
    call<RecruitmentEvent[]>("list_recruitment_events"),
  saveRecruitmentEvent: (request: SaveEvent) =>
    call<RecruitmentEvent>("save_recruitment_event", { request }),
  completeRecruitmentEvent: (
    id: string,
    revision: number,
    completed: boolean,
  ) =>
    call<RecruitmentEvent>("complete_recruitment_event", {
      id,
      revision,
      completed,
    }),
  getOverview: () => call<Overview>("get_overview"),
  listTasks: () => call<Task[]>("list_tasks"),
  saveTask: (request: SaveTask) => call<Task>("save_task", { request }),
  completeTask: (id: string, revision: number, completed: boolean) =>
    call<Task>("complete_task", { id, revision, completed }),
  listReminderRules: () => call<ReminderRule[]>("list_reminder_rules"),
  saveReminderRules: (rules: ReminderRule[]) =>
    call<ReminderRule[]>("save_reminder_rules", { rules }),
  respondToReminder: (key: string, fingerprint: string, snooze: boolean) =>
    call<void>("respond_to_reminder", { key, fingerprint, snooze }),
  previewExternalDatabaseBackup: (directory: string) =>
    call<ExternalDatabasePreview>("preview_external_database_backup", {
      directory,
    }),
  restoreExternalDatabaseBackup: (
    directory: string,
    parentDirectory: string,
    expectedFingerprint: string,
  ) =>
    call<DatabaseRestore>("restore_external_database_backup", {
      directory,
      parentDirectory,
      expectedFingerprint,
    }),
  prepareBackupRecycleBin: () =>
    call<BackupTrashChallenge>("prepare_backup_recycle_bin"),
  emptyBackupRecycleBin: (confirmationToken: string) =>
    call<BackupTrashResult>("empty_backup_recycle_bin", { confirmationToken }),
  prepareAgentSnapshotRecycleBin: () =>
    call<BackupTrashChallenge>("prepare_agent_snapshot_recycle_bin"),
  emptyAgentSnapshotRecycleBin: (confirmationToken: string) =>
    call<BackupTrashResult>("empty_agent_snapshot_recycle_bin", {
      confirmationToken,
    }),
  previewApplicationBatch: (request: BatchRequest) =>
    call<BatchPreview>("preview_application_batch", { request }),
  applyApplicationBatch: (request: BatchRequest, expectedFingerprint: string) =>
    call<BatchApplied>("apply_application_batch", {
      request,
      expectedFingerprint,
    }),
  createFullBackup: (parentDirectory: string, includeRecycleBin: boolean) =>
    call<FullBackupCreated>("create_full_backup", {
      parentDirectory,
      includeRecycleBin,
    }),
  previewFullBackup: (archivePath: string) =>
    call<FullBackupPreview>("preview_full_backup", { archivePath }),
  restoreFullBackup: (
    archivePath: string,
    parentDirectory: string,
    expectedSha256: string,
  ) =>
    call<FullRestore>("restore_full_backup", {
      archivePath,
      parentDirectory,
      expectedSha256,
    }),
  migrateWarehouse: (parentDirectory: string) =>
    call<FullRestore>("migrate_warehouse", { parentDirectory }),
  listDatabaseBackups: () => call<BackupCatalog>("list_database_backups"),
  createDatabaseBackup: () => call<BackupCreated>("create_database_backup"),
  previewDatabaseBackup: (backupId: string, recycled: boolean) =>
    call<BackupPreview>("preview_database_backup", { backupId, recycled }),
  restoreDatabaseBackup: (
    backupId: string,
    recycled: boolean,
    expectedSha256: string,
    parentDirectory: string,
  ) =>
    call<DatabaseRestore>("restore_database_backup", {
      backupId,
      recycled,
      expectedSha256,
      parentDirectory,
    }),
  inspectApplicationFiles: (applicationId: string) =>
    call<PathObservation>("inspect_application_files", { applicationId }),
  getRecoveryDiagnostics: () =>
    call<RecoveryDiagnostics>("get_recovery_diagnostics"),
  getStartupState: () => call<StartupState>("get_startup_state"),
  createWarehouse: (path: string) =>
    call<WarehouseSummary>("create_warehouse", { path }),
  openWarehouse: (path: string, accessMode: WarehouseAccessMode = "write") =>
    call<WarehouseSummary>("open_warehouse", { path, accessMode }),
  closeWarehouse: () => call<void>("close_warehouse"),
  listApplications: (scope: ApplicationScope) =>
    call<ApplicationListItem[]>("list_applications", { scope }),
  getApplication: (id: string) =>
    call<ApplicationDetail>("get_application", { id }),
  createApplication: (request: CreateApplicationRequest) =>
    call<ApplicationDetail>("create_application", { request }),
  updateApplication: (request: UpdateApplicationRequest) =>
    call<ApplicationDetail>("update_application", { request }),
  editApplicationCell: (request: CellRequest) =>
    call<CellApplied>("edit_application_cell", { request }),
  changeApplicationStage: (request: {
    applicationId: string;
    stageId: string;
    stageState: string;
    revision: number;
    notes: string;
  }) => call<ApplicationDetail>("change_application_stage", { request }),
  saveWorkflowStage: (request: {
    applicationId: string;
    revision: number;
    id: string | null;
    displayName: string;
    color: string;
    isTerminal: boolean;
    terminalOutcome: string | null;
  }) => call<ApplicationDetail>("save_workflow_stage", { request }),
  deleteWorkflowStage: (
    applicationId: string,
    stageId: string,
    revision: number,
  ) =>
    call<ApplicationDetail>("delete_workflow_stage", {
      applicationId,
      stageId,
      revision,
    }),
  listWorkflowTemplates: () =>
    call<WorkflowTemplate[]>("list_workflow_templates"),
  getWorkflowTemplate: (id: string) =>
    call<WorkflowTemplateDetail>("get_workflow_template", { id }),
  updateApplicationStates: (request: UpdateAuxiliaryStatesRequest) =>
    call<ApplicationDetail>("update_application_states", { request }),
  updateTemplateStates: (request: UpdateAuxiliaryStatesRequest) =>
    call<WorkflowTemplateDetail>("update_template_states", { request }),
  updateWorkflowTemplate: (request: UpdateWorkflowTemplateRequest) =>
    call<WorkflowTemplateDetail>("update_workflow_template", { request }),
  duplicateWorkflowTemplate: (id: string, revision: number, name: string) =>
    call<WorkflowTemplateDetail>("duplicate_workflow_template", {
      id,
      revision,
      name,
    }),
  setDefaultWorkflowTemplate: (id: string, revision: number) =>
    call<WorkflowTemplateDetail>("set_default_workflow_template", {
      id,
      revision,
    }),
  reorderApplicationWorkflow: (
    applicationId: string,
    revision: number,
    stageIds: string[],
  ) =>
    call<ApplicationDetail>("reorder_application_workflow", {
      request: { applicationId, revision, stageIds },
    }),
  saveWorkflowAsTemplate: (
    applicationId: string,
    name: string,
    setDefault: boolean,
  ) =>
    call<WorkflowTemplate[]>("save_workflow_as_template", {
      applicationId,
      name,
      setDefault,
    }),
  saveInterviewRound: (request: {
    applicationId: string;
    revision: number;
    id: string | null;
    displayName: string;
    state: string;
    scheduledAtUtc: string | null;
    completedAtUtc: string | null;
    result: string;
    notes: string;
  }) => call<ApplicationDetail>("save_interview_round", { request }),
  deleteInterviewRound: (
    applicationId: string,
    roundId: string,
    revision: number,
  ) =>
    call<ApplicationDetail>("delete_interview_round", {
      applicationId,
      roundId,
      revision,
    }),
  listFieldDefinitions: () => call<FieldDefinition[]>("list_field_definitions"),
  saveFieldDefinition: (request: FieldDefinitionRequest) =>
    call<FieldDefinition[]>("save_field_definition", { request }),
  listApplicationViews: () => call<SavedView[]>("list_application_views"),
  saveApplicationView: (request: SaveViewRequest) =>
    call<SavedViewChange>("save_application_view", { request }),
  updateViewMetadata: (request: {
    id: string;
    revision: number;
    name: string;
    isDefault: boolean;
  }) => call<SavedViewChange>("update_view_metadata", { request }),
  duplicateApplicationView: (id: string, revision: number, name: string) =>
    call<SavedViewChange>("duplicate_application_view", { id, revision, name }),
  deleteApplicationView: (id: string, revision: number) =>
    call<SavedView[]>("delete_application_view", { id, revision }),
  getApplicationPageSize: () => call<number>("get_application_page_size"),
  setApplicationPageSize: (value: number) =>
    call<number>("set_application_page_size", { value }),
  setApplicationArchived: (applicationId: string, archived: boolean) =>
    call<ApplicationDetail>("set_application_archived", {
      applicationId,
      archived,
    }),
  scanApplicationDocuments: (applicationId: string) =>
    call<ApplicationDetail["documents"]>("scan_application_documents", {
      applicationId,
    }),
  refreshFileIndex: () => call<void>("refresh_file_index"),
  listApplicationDirectories: (applicationId: string) =>
    call<ApplicationDirectories>("list_application_directories", {
      applicationId,
    }),
  renameDocument: (request: RenameDocumentRequest) =>
    call<ApplicationDetail>("rename_document", { request }),
  trashDocument: (request: TrashDocumentRequest) =>
    call<ApplicationDetail>("trash_document", { request }),
  listDocumentTrash: () => call<DocumentTrashEntry[]>("list_document_trash"),
  restoreDocument: (id: string) =>
    call<RestoredDocument>("restore_document", { id }),
  prepareDocumentTrashCleanup: () =>
    call<DocumentTrashChallenge>("prepare_document_trash_cleanup"),
  emptyDocumentTrash: (confirmationToken: string) =>
    call<DocumentTrashPurged>("empty_document_trash", { confirmationToken }),
  listUnlinkedFolders: (includeHidden = false) =>
    call<UnlinkedFolder[]>("list_unlinked_folders", { includeHidden }),
  claimApplicationFolder: (
    folderName: string,
    application: CreateApplicationRequest,
    includeHidden = false,
  ) =>
    call<ApplicationDetail>("claim_application_folder", {
      request: { folderName, includeHidden, application },
    }),
  retryFolderNormalization: (applicationId: string) =>
    call<ApplicationDetail>("retry_folder_normalization", { applicationId }),
  previewApplicationDuplicate: (applicationId: string, mode: DuplicateMode) =>
    call<DuplicatePreview>("preview_application_duplicate", {
      applicationId,
      mode,
    }),
  duplicateApplication: (applicationId: string, mode: DuplicateMode) =>
    call<ApplicationDetail>("duplicate_application", { applicationId, mode }),
  moveApplicationToTrash: (applicationId: string) =>
    call<void>("move_application_to_trash", { applicationId }),
  listTrash: () => call<TrashEntry[]>("list_trash"),
  restoreApplication: (applicationId: string) =>
    call<RestoreResult>("restore_application", { applicationId }),
  prepareEmptyRecycleBin: () =>
    call<EmptyTrashChallenge>("prepare_empty_recycle_bin"),
  emptyRecycleBin: (warehouseId: string, confirmationToken: string) =>
    call<EmptyTrashResult>("empty_recycle_bin", {
      warehouseId,
      confirmationToken,
    }),
  openApplicationFolder: (applicationId: string) =>
    call<void>("open_application_folder", { applicationId }),
  openDocument: (
    applicationId: string,
    documentId: string,
    mode: "default" | "chooseOther" | InstalledBrowser = "default",
  ) => call<void>("open_document", { applicationId, documentId, mode }),
  availableBrowsers: () => call<InstalledBrowser[]>("available_browsers"),
  revealDocument: (applicationId: string, documentId: string) =>
    call<void>("reveal_document", { applicationId, documentId }),
  getDocumentPath: (applicationId: string, documentId: string) =>
    call<string>("get_document_path", { applicationId, documentId }),
  openWebUrl: (
    url: string,
    browser: "default" | "edge" | "chrome" | "firefox" = "default",
  ) => call<void>("open_web_url", { url, browser }),
};
