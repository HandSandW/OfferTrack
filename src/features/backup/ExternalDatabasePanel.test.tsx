import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { desktopApi } from "../../lib/tauri";
import * as dialog from "../../lib/dialog";
import { DraftGuardProvider } from "../../shared/DraftGuardProvider";
import { ExternalDatabasePanel } from "./ExternalDatabasePanel";
import type { DatabaseRestore, ExternalDatabasePreview } from "./contracts";

const preview: ExternalDatabasePreview = {
  fingerprint: "manifest-and-data-hash",
  applicationCount: 3,
  documentCount: 4,
  backup: {
    version: 1,
    kind: "database",
    id: "backup",
    warehouseId: "old-warehouse",
    schemaVersion: 7,
    createdAtUtc: "2026-09-03T02:00:00Z",
    localDate: "2026-09-03",
    reason: "manual",
    sizeBytes: 1024,
    sha256: "a".repeat(64),
  },
};
const onError = vi.fn();
const onOpen = vi.fn();
beforeEach(() => {
  vi.spyOn(dialog, "selectDirectory")
    .mockResolvedValueOnce("synthetic-snapshot")
    .mockResolvedValue("synthetic-parent");
  vi.spyOn(desktopApi, "previewExternalDatabaseBackup").mockResolvedValue(
    preview,
  );
  vi.spyOn(desktopApi, "restoreExternalDatabaseBackup").mockResolvedValue({
    directory: "synthetic-restored",
    applicationCount: 3,
    documentCount: 4,
  });
  onOpen.mockResolvedValue(undefined);
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  onError.mockReset();
  onOpen.mockReset();
});
const show = () =>
  render(<ExternalDatabasePanel onError={onError} onOpen={onOpen} />, {
    wrapper: DraftGuardProvider,
  });
async function select() {
  fireEvent.click(screen.getByText("选择并校验数据库快照目录"));
  await screen.findByText("恢复独立数据库快照…");
}

it("restores without an active warehouse only after confirmation and opens explicitly", async () => {
  show();
  await select();
  expect(desktopApi.restoreExternalDatabaseBackup).not.toHaveBeenCalled();
  fireEvent.click(screen.getByText("恢复独立数据库快照…"));
  fireEvent.click(await screen.findByText("取消"));
  await waitFor(() =>
    expect(screen.getByText("恢复独立数据库快照…")).toBeEnabled(),
  );
  expect(desktopApi.restoreExternalDatabaseBackup).not.toHaveBeenCalled();
  fireEvent.click(screen.getByText("恢复独立数据库快照…"));
  fireEvent.click(await screen.findByText("创建数据库恢复副本"));
  await screen.findByText(/数据库恢复完成：synthetic-restored/);
  expect(desktopApi.restoreExternalDatabaseBackup).toHaveBeenCalledWith(
    "synthetic-snapshot",
    "synthetic-parent",
    preview.fingerprint,
  );
  expect(onOpen).not.toHaveBeenCalled();
  fireEvent.click(screen.getByText("打开数据库恢复副本"));
  expect(onOpen).toHaveBeenCalledWith("synthetic-restored");
});

it("invalidates an old selection if a new snapshot fails verification", async () => {
  show();
  await select();
  vi.mocked(desktopApi.previewExternalDatabaseBackup).mockRejectedValueOnce(
    new Error("快照已损坏"),
  );
  fireEvent.click(screen.getByText("选择并校验数据库快照目录"));
  await screen.findByText("快照已损坏");
  expect(screen.queryByText("恢复独立数据库快照…")).not.toBeInTheDocument();
  expect(desktopApi.restoreExternalDatabaseBackup).not.toHaveBeenCalled();
});

it("blocks duplicate restoration and preserves preview after a recoverable failure", async () => {
  let reject!: (error: Error) => void;
  vi.mocked(desktopApi.restoreExternalDatabaseBackup).mockReturnValueOnce(
    new Promise<DatabaseRestore>((_resolve, fail) => {
      reject = fail;
    }),
  );
  show();
  await select();
  fireEvent.click(screen.getByText("恢复独立数据库快照…"));
  fireEvent.click(await screen.findByText("创建数据库恢复副本"));
  await waitFor(() =>
    expect(desktopApi.restoreExternalDatabaseBackup).toHaveBeenCalledOnce(),
  );
  expect(screen.getByText("选择并校验数据库快照目录")).toBeDisabled();
  expect(screen.getByText("恢复独立数据库快照…")).toBeDisabled();
  reject(new Error("目标暂不可写"));
  await screen.findByText("目标暂不可写");
  expect(screen.getByText("synthetic-snapshot")).toBeInTheDocument();
});
