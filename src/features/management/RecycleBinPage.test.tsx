import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { desktopApi } from "../../lib/tauri";
import { DraftGuardProvider } from "../../shared/DraftGuardProvider";
import { RecycleBinPage } from "./PhaseTwoPages";

const item = {
  applicationId: "record",
  companyName: "示例",
  positionName: "研发",
  deletedAtUtc: "2026-09-01T00:00:00Z",
  originalRelativePath: "applications/original",
  trashRelativePath: "recycle-bin/records/item",
};
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});
it("requires an explicit second confirmation before sending the bound purge token", async () => {
  vi.spyOn(desktopApi, "listTrash").mockResolvedValue([item]);
  vi.spyOn(desktopApi, "prepareEmptyRecycleBin").mockResolvedValue({
    warehouseId: "warehouse",
    confirmationToken: "token",
    itemCount: 1,
  });
  const empty = vi.spyOn(desktopApi, "emptyRecycleBin").mockResolvedValue({
    deletedCount: 0,
    failedApplicationIds: [item.applicationId],
  });
  render(<RecycleBinPage writable onError={vi.fn()} />, {
    wrapper: DraftGuardProvider,
  });
  await screen.findByRole("button", { name: "恢复" });
  fireEvent.click(screen.getByRole("button", { name: "清空回收站" }));
  await screen.findByRole("dialog");
  expect(empty).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "取消" }));
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "清空回收站" })).toBeEnabled(),
  );
  expect(empty).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "清空回收站" }));
  fireEvent.click(await screen.findByRole("button", { name: "是" }));
  await screen.findByText(/0 条；1 条未清空/);
  expect(empty).toHaveBeenCalledExactlyOnceWith("warehouse", "token");
});
it("reports collision restore's committed path even if subsequent refresh fails", async () => {
  vi.spyOn(desktopApi, "listTrash")
    .mockResolvedValueOnce([item])
    .mockRejectedValueOnce(new Error("读取失败"));
  const restore = vi.spyOn(desktopApi, "restoreApplication").mockResolvedValue({
    applicationId: "record",
    folderRelativePath: "applications/restored",
    renamed: true,
  });
  render(<RecycleBinPage writable onError={vi.fn()} />, {
    wrapper: DraftGuardProvider,
  });
  fireEvent.click(await screen.findByRole("button", { name: "恢复" }));
  await screen.findByText(/原位置被占用.*applications\/restored/);
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "已完成的恢复不会撤回",
  );
  expect(
    screen.queryByRole("button", { name: "恢复" }),
  ).not.toBeInTheDocument();
  expect(restore).toHaveBeenCalledTimes(1);
});
it("retains the entry after failure and allows retry but prohibits read-only mutations", async () => {
  vi.spyOn(desktopApi, "listTrash").mockResolvedValue([item]);
  const restore = vi
    .spyOn(desktopApi, "restoreApplication")
    .mockRejectedValue(new Error("文件被占用"));
  const { rerender } = render(<RecycleBinPage writable onError={vi.fn()} />, {
    wrapper: DraftGuardProvider,
  });
  fireEvent.click(await screen.findByRole("button", { name: "恢复" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("文件被占用");
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "恢复" })).toBeEnabled(),
  );
  fireEvent.click(screen.getByRole("button", { name: "恢复" }));
  await waitFor(() => expect(restore).toHaveBeenCalledTimes(2));
  rerender(<RecycleBinPage writable={false} onError={vi.fn()} />);
  expect(screen.getByRole("button", { name: "恢复" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "清空回收站" })).toBeDisabled();
});
