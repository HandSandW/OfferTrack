import { useState } from "react";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { FieldDefinition } from "../../contracts";
import { desktopApi } from "../../lib/tauri";
import { DraftGuardProvider } from "../../shared/DraftGuardProvider";
import { useDraftGuard } from "../../shared/draftGuard";
import { applicationFixture } from "../../test/applicationFixture";
import { SettingsPage } from "./SettingsPage";
const onError = vi.fn();
const field: FieldDefinition = {
  id: "field",
  revision: 7,
  key: "stable-key",
  displayName: "优先级",
  fieldType: "select",
  config: { options: ["高", "低"], future: true },
  displayOrder: 10,
  isVisible: true,
};
function Harness({ writable = true }: { writable?: boolean }) {
  const [left, setLeft] = useState(false);
  const { confirmLeave } = useDraftGuard();
  return (
    <>
      <SettingsPage writable={writable} onError={onError} />
      <button onClick={() => void confirmLeave().then(setLeft)}>
        离开设置
      </button>
      {left && <p>已离开设置</p>}
    </>
  );
}
const show = (writable = true) =>
  render(<Harness writable={writable} />, { wrapper: DraftGuardProvider });
beforeEach(() => {
  vi.spyOn(desktopApi, "listFieldDefinitions").mockResolvedValue([field]);
  vi.spyOn(desktopApi, "listUnlinkedFolders").mockResolvedValue([
    { name: "我的目录", hidden: false },
  ]);
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  onError.mockClear();
});

describe("metadata settings forms", () => {
  it("loads fields and folder candidates independently and retries without showing a false empty state", async () => {
    vi.mocked(desktopApi.listFieldDefinitions).mockRejectedValueOnce(
      new Error("字段读取失败"),
    );
    show();
    expect(await screen.findByRole("alert")).toHaveTextContent("字段读取失败");
    expect(await screen.findByText("我的目录")).toBeInTheDocument();
    expect(screen.queryByText("尚未创建自定义字段。")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "添加字段" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "刷新字段列表" }));
    await screen.findByRole("button", { name: "编辑字段 优先级" });
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(desktopApi.listUnlinkedFolders).toHaveBeenCalledTimes(1);
    expect(desktopApi.listUnlinkedFolders).toHaveBeenCalledWith(false);
  });

  it("edits with a stable identifier/revision, preserves options on failure, and protects dirty navigation", async () => {
    const save = vi
      .spyOn(desktopApi, "saveFieldDefinition")
      .mockRejectedValueOnce(new Error("选项仍被使用"));
    show();
    fireEvent.click(
      await screen.findByRole("button", { name: "编辑字段 优先级" }),
    );
    fireEvent.change(screen.getByLabelText("字段名称"), {
      target: { value: "新的优先级" },
    });
    fireEvent.change(screen.getByLabelText("选项（逗号分隔）"), {
      target: { value: "高，低, 中,高, " },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存字段" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("输入已保留");
    expect(save).toHaveBeenCalledWith({
      id: "field",
      revision: 7,
      displayName: "新的优先级",
      fieldType: "select",
      config: { options: ["高", "低", "中"], future: true },
    });
    fireEvent.click(screen.getByRole("button", { name: "离开设置" }));
    fireEvent.click(await screen.findByRole("button", { name: "继续编辑" }));
    expect(screen.queryByText("已离开设置")).not.toBeInTheDocument();
    expect(screen.getByLabelText("字段名称")).toHaveValue("新的优先级");
    save.mockResolvedValueOnce([
      { ...field, displayName: "新的优先级", revision: 8 },
    ]);
    fireEvent.click(screen.getByRole("button", { name: "保存字段" }));
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(
      screen.getByRole("button", { name: "编辑字段 新的优先级" }),
    ).toBeInTheDocument();
  });

  it("does not create a field until saved and blocks navigation during the write", async () => {
    let resolve!: (fields: FieldDefinition[]) => void;
    const save = vi.spyOn(desktopApi, "saveFieldDefinition").mockImplementation(
      () =>
        new Promise((done) => {
          resolve = done;
        }),
    );
    show();
    await screen.findByText("优先级");
    fireEvent.click(screen.getByRole("button", { name: "添加字段" }));
    fireEvent.change(screen.getByLabelText("字段名称"), {
      target: { value: "备注字段" },
    });
    fireEvent(
      screen.getByRole("dialog"),
      new Event("cancel", { cancelable: true }),
    );
    fireEvent.click(await screen.findByRole("button", { name: "继续编辑" }));
    expect(save).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "保存字段" }));
    fireEvent.click(screen.getByRole("button", { name: "离开设置" }));
    expect(
      await screen.findByRole("dialog", { name: "正在保存" }),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("字段名称")).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "知道了" }));
    resolve([field]);
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(save).toHaveBeenCalledWith({
      id: null,
      revision: null,
      displayName: "备注字段",
      fieldType: "text",
      config: {},
    });
    expect(screen.queryByText("已离开设置")).not.toBeInTheDocument();
  });

  it("guards folder-claim drafts, retains failed input, and distinguishes successful creation from rescan failure", async () => {
    const claim = vi
      .spyOn(desktopApi, "claimApplicationFolder")
      .mockRejectedValueOnce(new Error("目录暂不可访问"))
      .mockResolvedValueOnce(applicationFixture());
    show();
    fireEvent.click(
      await screen.findByRole("button", { name: "用此文件夹新建投递" }),
    );
    fireEvent.change(screen.getByLabelText("岗位名称"), {
      target: { value: "开发工程师" },
    });
    fireEvent(
      screen.getByRole("dialog"),
      new Event("cancel", { cancelable: true }),
    );
    fireEvent.click(await screen.findByRole("button", { name: "继续编辑" }));
    expect(claim).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "创建并规范化名称" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("输入仍保留");
    expect(screen.getByLabelText("岗位名称")).toHaveValue("开发工程师");
    vi.mocked(desktopApi.listUnlinkedFolders).mockRejectedValueOnce(
      new Error("扫描暂时失败"),
    );
    fireEvent.click(screen.getByRole("button", { name: "创建并规范化名称" }));
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent("扫描暂时失败");
    expect(screen.getByText(/投递记录已创建/)).toBeInTheDocument();
    expect(claim).toHaveBeenLastCalledWith(
      "我的目录",
      expect.objectContaining({
        companyName: "我的目录",
        positionName: "开发工程师",
      }),
    );
    expect(
      screen.getByRole("button", { name: "用此文件夹新建投递" }),
    ).toBeDisabled();
    vi.mocked(desktopApi.listUnlinkedFolders).mockResolvedValueOnce([]);
    fireEvent.click(screen.getByRole("button", { name: "重新扫描" }));
    await screen.findByText("没有发现未关联文件夹。");
    expect(claim).toHaveBeenCalledTimes(2);
  });

  it("allows read-only metadata inspection but disables both editing and folder claims", async () => {
    show(false);
    expect(
      await screen.findByRole("button", { name: "编辑字段 优先级" }),
    ).toBeDisabled();
    expect(
      await screen.findByRole("button", { name: "用此文件夹新建投递" }),
    ).toBeDisabled();
    expect(screen.getByRole("button", { name: "添加字段" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "刷新字段列表" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "重新扫描" })).toBeEnabled();
  });
});
