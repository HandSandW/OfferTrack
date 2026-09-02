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
