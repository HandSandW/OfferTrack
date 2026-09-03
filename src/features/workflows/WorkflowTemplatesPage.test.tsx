import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { desktopApi } from "../../lib/tauri";
import { DraftGuardProvider } from "../../shared/DraftGuardProvider";
import { workflowFixture } from "../../test/workflowFixture";
import { WorkflowTemplatesPage } from "./WorkflowTemplatesPage";

const onError = vi.fn();
beforeEach(() => {
  vi.spyOn(desktopApi, "listWorkflowTemplates").mockResolvedValue([
    workflowFixture(),
  ]);
  vi.spyOn(desktopApi, "getWorkflowTemplate").mockResolvedValue(
    workflowFixture(),
  );
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  onError.mockClear();
});
const showPage = (writable = true) =>
  render(<WorkflowTemplatesPage writable={writable} onError={onError} />, {
    wrapper: DraftGuardProvider,
  });

describe("independent workflow template management", () => {
  it("edits name, color, stage order and appends a stage in one revision-checked save", async () => {
    const update = vi
      .spyOn(desktopApi, "updateWorkflowTemplate")
      .mockResolvedValue(workflowFixture({ name: "我的模板", revision: 2 }));
    showPage();
    fireEvent.change(await screen.findByLabelText("模板名称"), {
      target: { value: "我的模板" },
    });
    fireEvent.change(screen.getByLabelText("阶段 5 名称"), {
      target: { value: "技术面" },
    });
    fireEvent.change(screen.getByLabelText("阶段 5 颜色"), {
      target: { value: "#123456" },
    });
    fireEvent.click(screen.getByRole("button", { name: "上移 技术面" }));
    expect(screen.getByLabelText("阶段 4 名称")).toHaveValue("技术面");
    fireEvent.click(screen.getByRole("button", { name: "添加模板阶段" }));
    expect(screen.getByLabelText("阶段 8 名称")).toHaveValue("自定义阶段");
    expect(screen.getByRole("button", { name: "复制模板" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "保存模板修改" }));
    await waitFor(() => expect(update).toHaveBeenCalledOnce());
    const request = update.mock.calls[0]?.[0];
    expect(request).toMatchObject({
      id: "test-template",
      revision: 1,
      name: "我的模板",
    });
    expect(request?.stages[3]).toEqual({
      id: "test-template-stage-interview",
      displayName: "技术面",
      color: "#123456",
    });
    expect(request?.stages[7]?.id).toBeNull();
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "复制模板" })).toBeEnabled(),
    );
  });
  it("retains a failed draft and reloads even when the server revision is unchanged", async () => {
    vi.spyOn(desktopApi, "updateWorkflowTemplate").mockRejectedValue(
      new Error("保存失败，请重试"),
    );
    showPage();
    fireEvent.change(await screen.findByLabelText("模板名称"), {
      target: { value: "未保存" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存模板修改" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("保存失败");
    expect(screen.getByLabelText("模板名称")).toHaveValue("未保存");
    fireEvent.click(screen.getByRole("button", { name: "重新载入" }));
    fireEvent.click(await screen.findByRole("button", { name: "继续编辑" }));
    expect(desktopApi.getWorkflowTemplate).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "重新载入" }));
    fireEvent.click(
      await screen.findByRole("button", { name: "放弃修改并继续" }),
    );
    await waitFor(() =>
      expect(screen.getByLabelText("模板名称")).toHaveValue("默认招聘流程"),
    );
    expect(screen.getByRole("button", { name: "保存模板修改" })).toBeDisabled();
  });
  it("creates a distinct named copy and requires confirmation to change the default", async () => {
    const copy = workflowFixture({
      id: "copy",
      name: "银行流程",
      isDefault: false,
    });
    const duplicate = vi
      .spyOn(desktopApi, "duplicateWorkflowTemplate")
      .mockResolvedValue(copy);
    const makeDefault = vi
      .spyOn(desktopApi, "setDefaultWorkflowTemplate")
      .mockResolvedValue({ ...copy, revision: 2, isDefault: true });
    showPage();
    await screen.findByLabelText("模板名称");
    fireEvent.click(screen.getByRole("button", { name: "复制模板" }));
    fireEvent.change(screen.getByLabelText("副本名称"), {
      target: { value: "银行流程" },
    });
    fireEvent.click(screen.getByRole("button", { name: "创建模板副本" }));
    await waitFor(() =>
      expect(screen.getByLabelText("模板名称")).toHaveValue("银行流程"),
    );
    expect(duplicate).toHaveBeenCalledWith("test-template", 1, "银行流程");
    expect(makeDefault).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "设为默认模板" }));
    const prompt = await screen.findByRole("dialog", {
      name: "切换默认流程模板",
    });
    expect(within(prompt).getByText(/已有投递不受影响/)).toBeInTheDocument();
    fireEvent.click(within(prompt).getByRole("button", { name: "取消" }));
    expect(makeDefault).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "设为默认模板" }));
    fireEvent.click(await screen.findByRole("button", { name: "设为默认" }));
    await waitFor(() => expect(makeDefault).toHaveBeenCalledWith("copy", 1));
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "设为默认模板" }),
      ).toBeDisabled(),
    );
  });
  it("guards selection changes and never removes built-in stage controls", async () => {
    const other = workflowFixture({
      id: "other",
      name: "其他流程",
      isDefault: false,
    });
    vi.mocked(desktopApi.listWorkflowTemplates).mockResolvedValue([
      workflowFixture(),
      other,
    ]);
    vi.mocked(desktopApi.getWorkflowTemplate).mockImplementation((id) =>
      Promise.resolve(id === "other" ? other : workflowFixture()),
    );
    showPage();
    fireEvent.change(await screen.findByLabelText("模板名称"), {
      target: { value: "草稿" },
    });
    fireEvent.change(screen.getByLabelText("选择流程模板"), {
      target: { value: "other" },
    });
    fireEvent.click(await screen.findByRole("button", { name: "继续编辑" }));
    expect(screen.getByLabelText("选择流程模板")).toHaveValue("test-template");
    expect(screen.getByLabelText("模板名称")).toHaveValue("草稿");
    expect(
      screen.queryByRole("button", { name: "移除 准备投递" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "上移 准备投递" }),
    ).toBeDisabled();
    expect(screen.getByRole("button", { name: "下移 待签约" })).toBeDisabled();
    fireEvent.change(screen.getByLabelText("选择流程模板"), {
      target: { value: "other" },
    });
    fireEvent.click(
      await screen.findByRole("button", { name: "放弃修改并继续" }),
    );
    await waitFor(() =>
      expect(screen.getByLabelText("模板名称")).toHaveValue("其他流程"),
    );
  });
  it("supports read-only inspection and retry after an initial load failure", async () => {
    vi.mocked(desktopApi.listWorkflowTemplates).mockRejectedValueOnce(
      new Error("读取失败"),
    );
    showPage(false);
    expect(await screen.findByRole("alert")).toHaveTextContent("读取失败");
    fireEvent.click(screen.getByRole("button", { name: "重新载入" }));
    expect(await screen.findByLabelText("模板名称")).toBeDisabled();
    expect(screen.getByRole("button", { name: "复制模板" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "添加模板阶段" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "保存模板修改" })).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "管理模板辅助状态" }),
    ).toBeDisabled();
  });

  it("edits template states independently and blocks entry while other template fields are dirty", async () => {
    const initial = workflowFixture();
    const save = vi
      .spyOn(desktopApi, "updateTemplateStates")
      .mockRejectedValueOnce(new Error("模板状态保存失败"));
    const recordSave = vi.spyOn(desktopApi, "updateApplicationStates");
    showPage();
    fireEvent.change(await screen.findByLabelText("模板名称"), {
      target: { value: "名称草稿" },
    });
    expect(
      screen.getByRole("button", { name: "管理模板辅助状态" }),
    ).toBeDisabled();
    fireEvent.change(screen.getByLabelText("模板名称"), {
      target: { value: initial.name },
    });
    fireEvent.click(screen.getByRole("button", { name: "管理模板辅助状态" }));
    fireEvent.change(screen.getByLabelText("辅助状态 4 名称"), {
      target: { value: "等待反馈" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存辅助状态" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "模板状态保存失败",
    );
    expect(screen.getByLabelText("辅助状态 4 名称")).toHaveValue("等待反馈");
    expect(save).toHaveBeenCalledWith(
      expect.objectContaining({ ownerId: initial.id, revision: 1 }),
    );
    save.mockResolvedValueOnce(
      workflowFixture({
        revision: 2,
        auxiliaryStates: initial.auxiliaryStates.map((s) =>
          s.stableKey === "awaitingResult"
            ? { ...s, displayName: "等待反馈" }
            : s,
        ),
      }),
    );
    fireEvent.click(screen.getByRole("button", { name: "保存辅助状态" }));
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(screen.getByText(/辅助状态：.*等待反馈/)).toBeInTheDocument();
    expect(recordSave).not.toHaveBeenCalled();
  });
});
