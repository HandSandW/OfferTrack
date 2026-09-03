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
import { useDraftGuard } from "../../shared/draftGuard";
import type { BackupItem, DatabaseRestore } from "./contracts";
import { DatabaseBackupPanel } from "./DatabaseBackupPanel";

const item: BackupItem = {
  version: 1,
  kind: "database",
  id: "backup-id",
  warehouseId: "warehouse-id",
  schemaVersion: 7,
  createdAtUtc: "2026-09-03T01:00:00Z",
  localDate: "2026-09-03",
  reason: "manual",
  sizeBytes: 4096,
  sha256: "a".repeat(64),
  recycled: true,
};
const onError = vi.fn();
const onLeave = vi.fn();
function Harness({ writable }: { writable: boolean }) {
  const { confirmLeave } = useDraftGuard();
  return (
    <>
      <DatabaseBackupPanel writable={writable} onError={onError} />
      <button onClick={() => void confirmLeave().then(onLeave)}>
        离开备份页
      </button>
    </>
  );
}
const show = (writable = true) =>
  render(<Harness writable={writable} />, { wrapper: DraftGuardProvider });
async function load() {
  fireEvent.click(screen.getByRole("button", { name: "读取备份列表" }));
  await screen.findByRole("button", { name: "恢复到新目录…" });
}
beforeEach(() => {
  vi.spyOn(desktopApi, "listDatabaseBackups").mockResolvedValue({
    items: [item],
    incompleteCount: 1,
    invalidCount: 2,
  });
  vi.spyOn(desktopApi, "previewDatabaseBackup").mockResolvedValue({
    backup: item,
    applicationCount: 3,
    documentCount: 4,
  });
  vi.spyOn(desktopApi, "restoreDatabaseBackup").mockResolvedValue({
    directory: "synthetic-restored-directory",
    applicationCount: 3,
    documentCount: 4,
  });
  vi.spyOn(dialog, "selectDirectory").mockResolvedValue("synthetic-parent");
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  onError.mockClear();
  onLeave.mockClear();
});

describe("database backup and safe restore", () => {
  it("requires another confirmation for backup purge and distinguishes partial deletion from refresh errors", async () => {
    vi.spyOn(desktopApi, "prepareBackupRecycleBin").mockResolvedValue({
      confirmationToken: "token",
      itemIds: ["one", "two"],
      skippedCount: 1,
    });
    const purge = vi
      .spyOn(desktopApi, "emptyBackupRecycleBin")
      .mockResolvedValue({
        deletedIds: ["one"],
        failed: [
          {
            id: "two",
            error: { code: "FILE_BUSY", message: "文件占用", retryable: true },
          },
        ],
        skippedCount: 1,
      });
    show();
    fireEvent.click(screen.getByText("清空备份回收站…"));
    fireEvent.click(await screen.findByText("取消"));
    await waitFor(() =>
      expect(screen.getByText("清空备份回收站…")).toBeEnabled(),
    );
    expect(purge).not.toHaveBeenCalled();
    vi.mocked(desktopApi.listDatabaseBackups).mockRejectedValueOnce(
      new Error("刷新失败"),
    );
    fireEvent.click(screen.getByText("清空备份回收站…"));
    fireEvent.click(await screen.findByText("是，永久清空备份"));
    await screen.findByText(/备份回收站清理完成：删除 1 个目录/);
    expect(purge).toHaveBeenCalledWith("token");
    expect(screen.getByLabelText("备份清理失败项目")).toHaveTextContent(
      "two：文件占用",
    );
    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent("刷新失败"),
    );
  });

  it("does not allow backup purge in read-only mode", () => {
    show(false);
    expect(screen.getByText("清空备份回收站…")).toBeDisabled();
  });

  it("loads only on request, preserves failed state and reports retained incomplete snapshots", async () => {
    vi.mocked(desktopApi.listDatabaseBackups).mockRejectedValueOnce(
      new Error("列表读取失败"),
    );
    show();
    expect(desktopApi.listDatabaseBackups).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "读取备份列表" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("列表读取失败");
    expect(
      screen.queryByText("暂无可识别的数据库备份。"),
    ).not.toBeInTheDocument();
    await load();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(
      screen.getByText(/发现 1 个未完成备份、2 个无法识别的项目/),
    ).toBeInTheDocument();
    expect(screen.getByText(/已移入回收站/)).toBeInTheDocument();
  });

  it("does not misreport a committed manual backup as failed when list refresh fails", async () => {
    const create = vi
      .spyOn(desktopApi, "createDatabaseBackup")
      .mockResolvedValue({
        backup: item,
        retentionWarning: true,
      });
    vi.mocked(desktopApi.listDatabaseBackups).mockRejectedValueOnce(
      new Error("刷新失败"),
    );
    show();
    fireEvent.click(screen.getByRole("button", { name: "立即备份数据库" }));
    expect(await screen.findByText(/数据库备份已完成/)).toHaveTextContent(
      "数据库备份已完成；旧备份轮换未完成，原文件仍保留",
    );
    expect(await screen.findByRole("alert")).toHaveTextContent("刷新失败");
    expect(create).toHaveBeenCalledTimes(1);
    expect(onError).toHaveBeenCalledOnce();
  });

  it("allows read-only restore with preview/hash confirmation, supports cancellation, and never switches warehouses", async () => {
    const open = vi.spyOn(desktopApi, "openWarehouse");
    show(false);
    expect(
      screen.getByRole("button", { name: "立即备份数据库" }),
    ).toBeDisabled();
    await load();
    fireEvent.click(screen.getByRole("button", { name: "恢复到新目录…" }));
    expect(
      await screen.findByRole("dialog", { name: "恢复数据库到新目录" }),
    ).toHaveTextContent("不包含简历正文");
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "恢复到新目录…" }),
      ).toBeEnabled(),
    );
    expect(desktopApi.restoreDatabaseBackup).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "恢复到新目录…" }));
    fireEvent.click(
      await screen.findByRole("button", { name: "创建恢复副本" }),
    );
    expect(
      await screen.findByText(/恢复完成：synthetic-restored-directory/),
    ).toHaveTextContent("当前仓库未切换");
    expect(desktopApi.restoreDatabaseBackup).toHaveBeenCalledWith(
      item.id,
      true,
      item.sha256,
      "synthetic-parent",
    );
    expect(open).not.toHaveBeenCalled();
  });

  it("does not request a destination or restore when verification fails", async () => {
    vi.mocked(desktopApi.previewDatabaseBackup).mockRejectedValueOnce(
      new Error("备份损坏"),
    );
    show();
    await load();
    fireEvent.click(screen.getByRole("button", { name: "恢复到新目录…" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("备份损坏");
    expect(dialog.selectDirectory).not.toHaveBeenCalled();
    expect(desktopApi.restoreDatabaseBackup).not.toHaveBeenCalled();
  });

  it("blocks navigation and duplicate actions during restore and preserves failure feedback", async () => {
    let reject!: (error: Error) => void;
    vi.mocked(desktopApi.restoreDatabaseBackup).mockImplementationOnce(
      () =>
        new Promise<DatabaseRestore>((_resolve, fail) => {
          reject = fail;
        }),
    );
    show();
    await load();
    fireEvent.click(screen.getByRole("button", { name: "恢复到新目录…" }));
    fireEvent.click(
      await screen.findByRole("button", { name: "创建恢复副本" }),
    );
    await waitFor(() =>
      expect(desktopApi.restoreDatabaseBackup).toHaveBeenCalledOnce(),
    );
    expect(
      screen.getByRole("button", { name: "立即备份数据库" }),
    ).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "恢复到新目录…" }),
    ).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "离开备份页" }));
    expect(
      await screen.findByRole("dialog", { name: "正在保存" }),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "知道了" }));
    reject(new Error("恢复位置暂不可写"));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "恢复位置暂不可写",
    );
    expect(onLeave).not.toHaveBeenCalledWith(true);
    expect(screen.getByRole("button", { name: "恢复到新目录…" })).toBeEnabled();
  });
});
