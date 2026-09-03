export interface DatabaseBackup {
  version: number;
  kind: "database";
  id: string;
  warehouseId: string;
  schemaVersion: number;
  createdAtUtc: string;
  localDate: string;
  reason:
    | "manual"
    | "daily"
    | "beforeUpgrade"
    | "beforeMigration"
    | "beforeBatch"
    | "beforeAgentWrite";
  sizeBytes: number;
  sha256: string;
}
export interface BackupItem extends DatabaseBackup {
  recycled: boolean;
}
export interface BackupCatalog {
  items: BackupItem[];
  incompleteCount: number;
  invalidCount: number;
}
export interface BackupPreview {
  backup: DatabaseBackup;
  applicationCount: number;
  documentCount: number;
}
export interface BackupCreated {
  backup: DatabaseBackup;
  retentionWarning: boolean;
}
export interface DatabaseRestore {
  directory: string;
  applicationCount: number;
  documentCount: number;
}
export interface ExternalDatabasePreview extends BackupPreview {
  fingerprint: string;
}
export interface BackupTrashChallenge {
  confirmationToken: string;
  itemIds: string[];
  skippedCount: number;
}
export interface BackupTrashResult {
  deletedIds: string[];
  failed: {
    id: string;
    error: { code: string; message: string; retryable: boolean };
  }[];
  skippedCount: number;
}

export interface FullBackupPreview {
  version: number;
  warehouseId: string;
  schemaVersion: number;
  createdAtUtc: string;
  includesRecycleBin: boolean;
  fileCount: number;
  totalBytes: number;
  sha256: string;
}
export interface FullBackupCreated {
  path: string;
  preview: FullBackupPreview;
}
export interface FullRestore extends DatabaseRestore {
  warehouseId: string;
  includesRecycleBin: boolean;
  migrationBackupPath: string | null;
}
