import { invoke } from "@tauri-apps/api/core";
import type {
  AppErrorPayload,
  StartupState,
  WarehouseAccessMode,
  WarehouseSummary,
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
  getStartupState: () => call<StartupState>("get_startup_state"),
  createWarehouse: (path: string) =>
    call<WarehouseSummary>("create_warehouse", { path }),
  openWarehouse: (path: string, accessMode: WarehouseAccessMode = "write") =>
    call<WarehouseSummary>("open_warehouse", { path, accessMode }),
  closeWarehouse: () => call<void>("close_warehouse"),
};
