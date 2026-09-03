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
import { DocumentTrashPanel } from "./DocumentTrashPanel";
const item = {
  id: "trash",
  documentId: "doc",
  applicationId: "record",
  companyName: "示例",
  positionName: "研发",
  displayName: "简历.pdf",
  originalRelativePath: "材料/简历.pdf",
  deletedAtUtc: "2026-09-04T00:00:00Z",
  parentDeleted: false,
  fileState: "available" as const,
};
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});
it("restores without overwriting and reports the actual path", async () => {
  vi.spyOn(desktopApi, "listDocumentTrash")
    .mockResolvedValueOnce([item])
    .mockResolvedValueOnce([]);
  const restore = vi.spyOn(desktopApi, "restoreDocument").mockResolvedValue({
    id: "trash",
    applicationId: "record",
    documentId: "doc",
    relativePath: "材料/restored-id.pdf",
    relocated: true,
  });
  render(<DocumentTrashPanel writable onError={vi.fn()} />, {
    wrapper: DraftGuardProvider,
  });
  fireEvent.click(await screen.findByRole("button", { name: "恢复" }));
  await screen.findByText(/未覆盖原位置内容/);
  expect(restore).toHaveBeenCalledExactlyOnceWith("trash");
});
it("requires confirmation and binds the cleanup token", async () => {
  vi.spyOn(desktopApi, "listDocumentTrash").mockResolvedValue([item]);
  vi.spyOn(desktopApi, "prepareDocumentTrashCleanup").mockResolvedValue({
    confirmationToken: "token",
    itemIds: ["trash"],
    missingCount: 0,
  });
  const empty = vi
    .spyOn(desktopApi, "emptyDocumentTrash")
    .mockResolvedValue({ deletedIds: ["trash"], failed: [] });
  render(<DocumentTrashPanel writable onError={vi.fn()} />, {
    wrapper: DraftGuardProvider,
  });
  fireEvent.click(
    await screen.findByRole("button", { name: "清空附件回收站" }),
  );
  expect(empty).not.toHaveBeenCalled();
  fireEvent.click(await screen.findByRole("button", { name: "取消" }));
  await waitFor(() =>
    expect(
      screen.getByRole("button", { name: "清空附件回收站" }),
    ).toBeEnabled(),
  );
  fireEvent.click(screen.getByRole("button", { name: "清空附件回收站" }));
  fireEvent.click(await screen.findByRole("button", { name: "是" }));
  await waitFor(() => expect(empty).toHaveBeenCalledExactlyOnceWith("token"));
});
it("disables restore for deleted parents and all mutations in read-only mode", async () => {
  vi.spyOn(desktopApi, "listDocumentTrash").mockResolvedValue([
    { ...item, parentDeleted: true },
  ]);
  render(<DocumentTrashPanel writable={false} onError={vi.fn()} />, {
    wrapper: DraftGuardProvider,
  });
  expect(await screen.findByText(/请先恢复投递/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "恢复" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "清空附件回收站" })).toBeDisabled();
});
