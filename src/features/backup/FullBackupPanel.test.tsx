import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { desktopApi } from "../../lib/tauri";
import * as dialog from "../../lib/dialog";
import { DraftGuardProvider } from "../../shared/DraftGuardProvider";
import type { FullBackupPreview, FullRestore } from "./contracts";
import { FullBackupPanel } from "./FullBackupPanel";

const preview: FullBackupPreview = {
  version: 1,
  warehouseId: "warehouse-id",
  schemaVersion: 7,
  createdAtUtc: "2026-09-03T00:00:00Z",
  includesRecycleBin: true,
  fileCount: 5,
  totalBytes: 2_097_152,
  sha256: "a".repeat(64),
};
const restored: FullRestore = {
  directory: "restored-directory",
  warehouseId: "warehouse-id",
  applicationCount: 3,
  documentCount: 4,
  includesRecycleBin: true,
  migrationBackupPath: null,
};
const onError = vi.fn();
const onOpen = vi.fn().mockResolvedValue(undefined);
const show = (writable = true) =>
  render(
    <FullBackupPanel writable={writable} onError={onError} onOpen={onOpen} />,
    { wrapper: DraftGuardProvider },
  );

beforeEach(() => {
  vi.spyOn(dialog, "selectDirectory").mockResolvedValue("destination");
  vi.spyOn(dialog, "selectFullBackup").mockResolvedValue(
    "archive.offertrack-backup",
  );
  vi.spyOn(desktopApi, "createFullBackup").mockResolvedValue({
    path: "created.offertrack-backup",
    preview,
  });
  vi.spyOn(desktopApi, "previewFullBackup").mockResolvedValue(preview);
  vi.spyOn(desktopApi, "restoreFullBackup").mockResolvedValue(restored);
  vi.spyOn(desktopApi, "migrateWarehouse").mockResolvedValue({
    ...restored,
    migrationBackupPath: "migration.offertrack-backup",
  });
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  onError.mockClear();
  onOpen.mockClear();
});

describe("full backup, independent recovery and migration", () => {
  it("creates with trash by default and allows explicitly excluding it", async () => {
    show();
    fireEvent.click(screen.getByRole("button", { name: "创建完整备份" }));
    expect(
      await screen.findByText(/created.offertrack-backup/),
    ).toHaveTextContent("私人资料");
    expect(desktopApi.createFullBackup).toHaveBeenLastCalledWith(
      "destination",
      true,
    );
    fireEvent.click(screen.getByRole("checkbox", { name: /包含回收站文件/ }));
    fireEvent.click(screen.getByRole("button", { name: "创建完整备份" }));
    await waitFor(() =>
      expect(desktopApi.createFullBackup).toHaveBeenCalledTimes(2),
    );
    expect(desktopApi.createFullBackup).toHaveBeenLastCalledWith(
      "destination",
      false,
    );
  });

  it("restores only after package preview, destination selection and confirmation", async () => {
    show(false);
    expect(screen.getByRole("button", { name: "创建完整备份" })).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "迁移当前仓库…" }),
    ).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "选择并校验完整备份" }));
    expect(await screen.findByText(/5 个文件 · 2.0 MB/)).toHaveTextContent(
      "含回收站",
    );
    fireEvent.click(screen.getByRole("button", { name: "恢复已校验备份…" }));
    expect(
      await screen.findByRole("dialog", { name: "恢复完整备份" }),
    ).toHaveTextContent("不覆盖当前仓库");
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    expect(desktopApi.restoreFullBackup).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "恢复已校验备份…" }),
      ).toBeEnabled(),
    );
    fireEvent.click(screen.getByRole("button", { name: "恢复已校验备份…" }));
    fireEvent.click(
      await screen.findByRole("button", { name: "恢复到新目录" }),
    );
    expect(
      await screen.findByText(/完整恢复与数据库校验已完成/),
    ).toHaveTextContent("尚未切换");
    expect(desktopApi.restoreFullBackup).toHaveBeenCalledWith(
      "archive.offertrack-backup",
      "destination",
      preview.sha256,
    );
  });

  it("keeps a verified result until the user explicitly switches", async () => {
    show();
    fireEvent.click(screen.getByRole("button", { name: "迁移当前仓库…" }));
    fireEvent.click(
      await screen.findByRole("button", { name: "开始迁移复制" }),
    );
    expect(
      await screen.findByText(/迁移副本与完整备份已校验完成/),
    ).toHaveTextContent("原仓库保留");
    expect(screen.getByText(/migration.offertrack-backup/)).toBeInTheDocument();
    expect(onOpen).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "切换到已验证仓库" }));
    await waitFor(() =>
      expect(onOpen).toHaveBeenCalledWith("restored-directory"),
    );
  });

  it("does not select a destination or restore when package verification fails", async () => {
    vi.mocked(desktopApi.previewFullBackup).mockRejectedValueOnce(
      new Error("完整包损坏"),
    );
    show(false);
    fireEvent.click(screen.getByRole("button", { name: "选择并校验完整备份" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("完整包损坏");
    expect(desktopApi.restoreFullBackup).not.toHaveBeenCalled();
    expect(dialog.selectDirectory).not.toHaveBeenCalled();
    expect(onError).toHaveBeenCalledOnce();
  });
});
