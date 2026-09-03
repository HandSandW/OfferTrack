import { invoke } from "@tauri-apps/api/core";
import type {
  AppErrorPayload,
  ApplicationDetail,
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
    return await invoke<T>(command, args);
  } catch (error: unknown) {
    throw normalizeError(error);
  }
}

export const desktopApi = {
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
