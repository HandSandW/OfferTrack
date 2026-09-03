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
import { applicationFixture } from "../../test/applicationFixture";
import { RenameDocumentDialog } from "./RenameDocumentDialog";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});
const doc = {
  id: "doc",
  relativePath: "材料/resume.pdf",
  displayName: "resume.pdf",
  mediaType: "application/pdf",
  sizeBytes: 10,
  modifiedAtUtc: null,
  missing: false,
};
it("retains failed names, confirms dirty cancellation and blocks Escape during saving", async () => {
  const save = vi
    .spyOn(desktopApi, "renameDocument")
    .mockRejectedValueOnce(new Error("目标已存在"));
  const onCancel = vi.fn();
  const onSaved = vi.fn();
  render(
    <RenameDocumentDialog
      applicationId="record"
      document={doc}
      onCancel={onCancel}
      onSaved={onSaved}
      onError={vi.fn()}
    />,
    { wrapper: DraftGuardProvider },
  );
  fireEvent.change(screen.getByLabelText("文件名称"), {
    target: { value: "offer.pdf" },
  });
  fireEvent.click(screen.getByRole("button", { name: "取消" }));
  fireEvent.click(await screen.findByRole("button", { name: "继续编辑" }));
  expect(onCancel).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "保存名称" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("输入已保留");
  expect(screen.getByLabelText("文件名称")).toHaveValue("offer.pdf");
  let resolve!: (value: ReturnType<typeof applicationFixture>) => void;
  save.mockImplementationOnce(
    () =>
      new Promise((done) => {
        resolve = done;
      }),
  );
  fireEvent.click(screen.getByRole("button", { name: "保存名称" }));
  expect(screen.getByLabelText("文件名称")).toBeDisabled();
  fireEvent(
    screen.getByRole("dialog"),
    new Event("cancel", { cancelable: true }),
  );
  await screen.findByRole("dialog", { name: "正在保存" });
  expect(onCancel).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "知道了" }));
  const result = applicationFixture();
  resolve(result);
  await waitFor(() => expect(onSaved).toHaveBeenCalledWith(result));
  expect(save).toHaveBeenLastCalledWith({
    applicationId: "record",
    documentId: "doc",
    expectedRelativePath: "材料/resume.pdf",
    newName: "offer.pdf",
  });
});
