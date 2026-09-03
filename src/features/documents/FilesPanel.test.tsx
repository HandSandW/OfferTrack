import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApplicationDetail } from "../../contracts";
import { desktopApi } from "../../lib/tauri";
import { DraftGuardProvider } from "../../shared/DraftGuardProvider";
import { applicationFixture } from "../../test/applicationFixture";
import { FilesPanel } from "./FilesPanel";

const onChange = vi.fn();
const onError = vi.fn();
const detail = applicationFixture({
  folderNormalizationPending: true,
  documents: [
    {
      id: "doc",
      relativePath: "resume.pdf",
      displayName: "resume.pdf",
      mediaType: "application/pdf",
      sizeBytes: 1024,
      modifiedAtUtc: null,
      missing: true,
    },
  ],
});
const show = (writable = true, record: ApplicationDetail = detail) =>
  render(
    <FilesPanel
      detail={record}
      writable={writable}
      onChange={onChange}
      onError={onError}
    />,
    { wrapper: DraftGuardProvider },
  );
beforeEach(() => {
  vi.spyOn(desktopApi, "listApplicationDirectories").mockResolvedValue({
    version: 1,
    directories: [],
  });
  vi.spyOn(desktopApi, "inspectApplicationFiles").mockResolvedValue({
    state: "available",
    relativePath: detail.folderRelativePath,
  });
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  onChange.mockClear();
  onError.mockClear();
});

describe("file health feedback", () => {
  it("shows empty folders in read-only mode and keeps file indexes when directory enumeration fails", async () => {
    vi.mocked(desktopApi.listApplicationDirectories)
      .mockRejectedValueOnce(new Error("子目录占用"))
      .mockResolvedValueOnce({
        version: 1,
        directories: [{ relativePath: "空目录", empty: true }],
      });
    show(false);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "附件索引仍保留",
    );
    expect(screen.getByText("resume.pdf")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "检查目录" }));
    await screen.findByText("（空目录）");
    expect(onChange).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "重新扫描" })).toBeDisabled();
  });
  it("saves rename via IDs and refreshes the current detail", async () => {
    const record = {
      ...detail,
      documents: detail.documents.map((doc) => ({ ...doc, missing: false })),
    };
    const updated = {
      ...record,
      documents: record.documents.map((doc) => ({
        ...doc,
        relativePath: "new.pdf",
        displayName: "new.pdf",
      })),
    };
    const save = vi
      .spyOn(desktopApi, "renameDocument")
      .mockResolvedValue(updated);
    vi.spyOn(desktopApi, "availableBrowsers").mockResolvedValue([]);
    show(true, record);
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "resume.pdf 更多操作" }),
      ).toBeEnabled(),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "resume.pdf 更多操作" }),
    );
    fireEvent.click(screen.getByRole("menuitem", { name: "重命名…" }));
    fireEvent.change(screen.getByLabelText("文件名称"), {
      target: { value: "new.pdf" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存名称" }));
    await screen.findByText(/附件已重命名/);
    expect(onChange).toHaveBeenCalledWith(updated);
    expect(save).toHaveBeenCalledWith({
      applicationId: detail.id,
      documentId: "doc",
      expectedRelativePath: "resume.pdf",
      newName: "new.pdf",
    });
  });
  it("shows missing folders and old missing indexes without inventing an empty directory", async () => {
    vi.mocked(desktopApi.inspectApplicationFiles).mockResolvedValue({
      state: "missing",
      relativePath: detail.folderRelativePath,
    });
    show();
    await screen.findByText(/目录不存在/);
    expect(screen.getByText(/上次扫描未找到此文件/)).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "打开投递文件夹" }),
    ).toBeDisabled();
    expect(screen.queryByText(/索引中暂无附件/)).not.toBeInTheDocument();
    expect(onChange).not.toHaveBeenCalled();
  });
  it("keeps indexes on scan failure and lets a successful retry mark missing files", async () => {
    const scan = vi
      .spyOn(desktopApi, "scanApplicationDocuments")
      .mockRejectedValueOnce(new Error("目录被占用"));
    show();
    await screen.findByText(/目录可访问/);
    fireEvent.click(screen.getByRole("button", { name: "重新扫描" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "上次索引仍保留",
    );
    expect(onChange).not.toHaveBeenCalled();
    scan.mockResolvedValueOnce(detail.documents);
    fireEvent.click(screen.getByRole("button", { name: "重新扫描" }));
    await screen.findByText(/索引已更新/);
    expect(onChange).toHaveBeenCalledWith(detail);
    expect(scan).toHaveBeenCalledTimes(2);
  });
  it("reports pending normalization accurately instead of claiming retry succeeded", async () => {
    const normalize = vi
      .spyOn(desktopApi, "retryFolderNormalization")
      .mockResolvedValue(detail);
    show();
    await screen.findByText(/目录可访问/);
    fireEvent.click(screen.getByRole("button", { name: "重试规范化" }));
    expect(await screen.findByText(/仍未规范化/)).toBeInTheDocument();
    normalize.mockResolvedValueOnce({
      ...detail,
      folderNormalizationPending: false,
    });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "重试规范化" })).toBeEnabled(),
    );
    fireEvent.click(screen.getByRole("button", { name: "重试规范化" }));
    await screen.findByText("文件夹名称已规范化。");
  });
  it("retries failed checks and permits read-only opening without updating indexes", async () => {
    vi.mocked(desktopApi.inspectApplicationFiles).mockRejectedValueOnce(
      new Error("检查失败"),
    );
    const open = vi
      .spyOn(desktopApi, "openDocument")
      .mockRejectedValueOnce(new Error("文件仍未找到"));
    show(false);
    expect(await screen.findByRole("alert")).toHaveTextContent("检查失败");
    fireEvent.click(screen.getByRole("button", { name: "检查目录" }));
    await screen.findByText(/目录可访问/);
    expect(screen.getByRole("button", { name: "重新扫描" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "重试规范化" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "打开" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("文件仍未找到");
    expect(open).toHaveBeenCalledWith(detail.id, "doc");
    expect(onChange).not.toHaveBeenCalled();
  });
});
