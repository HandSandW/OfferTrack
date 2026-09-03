import { useState } from "react";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ApplicationDetail } from "../../contracts";
import { desktopApi } from "../../lib/tauri";
import { DraftGuardProvider } from "../../shared/DraftGuardProvider";
import { applicationFixture } from "../../test/applicationFixture";
import { workflowFixture } from "../../test/workflowFixture";
import { WorkflowPanel } from "./WorkflowPanel";

const onError = vi.fn();
function Harness({
  initial = applicationFixture(),
  writable = true,
}: {
  initial?: ApplicationDetail;
  writable?: boolean;
}) {
  const [detail, setDetail] = useState(initial);
  return (
    <WorkflowPanel
      detail={detail}
      writable={writable}
      onChange={setDetail}
      onError={onError}
    />
  );
}
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  onError.mockClear();
});

describe("record-scoped workflow forms", () => {
  it("saves an independent stage name and color with optimistic revision", async () => {
    const save = vi
      .spyOn(desktopApi, "saveWorkflowStage")
      .mockResolvedValue(applicationFixture({ revision: 2 }));
    render(<Harness />, { wrapper: DraftGuardProvider });
    fireEvent.click(screen.getByRole("button", { name: "编辑阶段 准备投递" }));
    fireEvent.change(screen.getByLabelText("阶段名称"), {
      target: { value: "准备材料" },
    });
    fireEvent.change(screen.getByLabelText("阶段颜色"), {
      target: { value: "#123456" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存阶段" }));
    await waitFor(() =>
      expect(save).toHaveBeenCalledWith(
        expect.objectContaining({
          applicationId: "fixture-application",
          id: "stage-preparing",
          revision: 1,
          displayName: "准备材料",
          color: "#123456",
          isTerminal: false,
        }),
      ),
    );
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
  });
  it("retains a failed interview draft and converts local timestamps on explicit save", async () => {
    const save = vi
      .spyOn(desktopApi, "saveInterviewRound")
      .mockRejectedValueOnce(new Error("版本冲突，请重新载入"));
    render(<Harness />, { wrapper: DraftGuardProvider });
    fireEvent.click(screen.getByRole("button", { name: "+ 添加面试轮次" }));
    expect(save).not.toHaveBeenCalled();
    fireEvent.change(screen.getByLabelText("轮次名称"), {
      target: { value: "主管面" },
    });
    fireEvent.change(screen.getByLabelText("计划时间（本地时间）"), {
      target: { value: "2026-09-04T11:30" },
    });
    fireEvent.change(screen.getByLabelText("完成时间（本地时间）"), {
      target: { value: "2026-09-04T12:00" },
    });
    fireEvent.change(screen.getByLabelText("轮次状态"), {
      target: { value: "completed" },
    });
    fireEvent.change(screen.getByLabelText("面试结果"), {
      target: { value: "通过" },
    });
    fireEvent.change(screen.getByLabelText("轮次备注"), {
      target: { value: "项目经验讨论" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存轮次" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("版本冲突");
    expect(screen.getByLabelText("轮次名称")).toHaveValue("主管面");
    expect(save).toHaveBeenCalledWith(
      expect.objectContaining({
        revision: 1,
        id: null,
        displayName: "主管面",
        state: "completed",
        result: "通过",
        notes: "项目经验讨论",
        scheduledAtUtc: new Date("2026-09-04T11:30").toISOString(),
        completedAtUtc: new Date("2026-09-04T12:00").toISOString(),
      }),
    );
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    fireEvent.click(await screen.findByText("继续编辑"));
    expect(screen.getByLabelText("轮次备注")).toHaveValue("项目经验讨论");
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    fireEvent.click(await screen.findByText("放弃修改并继续"));
    await waitFor(() =>
      expect(screen.queryByLabelText("轮次名称")).not.toBeInTheDocument(),
    );
  });
  it("preserves unedited timestamp precision, and allows clearing it", async () => {
    const timestamp = "2026-09-04T11:30:00.123456+08:00";
    const detail = applicationFixture({
      interviewRounds: [
        {
          id: "round-1",
          sequenceNumber: 1,
          displayName: "HR 面",
          state: "pending",
          scheduledAtUtc: timestamp,
          completedAtUtc: null,
          result: "",
          notes: "",
        },
      ],
    });
    const save = vi
      .spyOn(desktopApi, "saveInterviewRound")
      .mockResolvedValue({ ...detail, revision: 2 });
    render(<Harness initial={detail} />, { wrapper: DraftGuardProvider });
    fireEvent.click(screen.getByRole("button", { name: "编辑面试 HR 面" }));
    fireEvent.change(screen.getByLabelText("轮次备注"), {
      target: { value: "带简历" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存轮次" }));
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(save).toHaveBeenLastCalledWith(
      expect.objectContaining({ scheduledAtUtc: timestamp }),
    );
    fireEvent.click(screen.getByRole("button", { name: "编辑面试 HR 面" }));
    fireEvent.change(screen.getByLabelText("计划时间（本地时间）"), {
      target: { value: "" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存轮次" }));
    await waitFor(() =>
      expect(save).toHaveBeenLastCalledWith(
        expect.objectContaining({ scheduledAtUtc: null, revision: 2 }),
      ),
    );
  });
  it("shows template failures inside the modal and only sets default explicitly", async () => {
    const save = vi
      .spyOn(desktopApi, "saveWorkflowAsTemplate")
      .mockRejectedValue(new Error("模板保存失败"));
    render(<Harness />, { wrapper: DraftGuardProvider });
    fireEvent.click(screen.getByRole("button", { name: "保存为流程模板…" }));
    const modal = screen.getByRole("dialog");
    fireEvent.change(within(modal).getByLabelText("模板名称"), {
      target: { value: "校园招聘" },
    });
    fireEvent.click(within(modal).getByRole("button", { name: "保存模板" }));
    expect(await within(modal).findByRole("alert")).toHaveTextContent(
      "模板保存失败",
    );
    expect(save).toHaveBeenLastCalledWith(
      "fixture-application",
      "校园招聘",
      false,
    );
    fireEvent.click(within(modal).getByLabelText("设为以后新建投递的默认模板"));
    fireEvent.click(within(modal).getByRole("button", { name: "保存模板" }));
    await waitFor(() =>
      expect(save).toHaveBeenLastCalledWith(
        "fixture-application",
        "校园招聘",
        true,
      ),
    );
  });
  it("keeps read-only workflow actions disabled", () => {
    render(<Harness writable={false} />, { wrapper: DraftGuardProvider });
    expect(screen.getByLabelText("招聘阶段")).toBeDisabled();
    expect(screen.getByRole("button", { name: "添加阶段" })).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "+ 添加面试轮次" }),
    ).toBeDisabled();
    expect(screen.getByRole("button", { name: "调整阶段顺序" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "管理辅助状态" })).toBeDisabled();
  });

  it("saves a complete stage permutation only after explicit confirmation and keeps failures editable", async () => {
    const stages = workflowFixture().stages;
    const initial = applicationFixture({
      stages,
      currentStageId: stages[0]!.id,
    });
    const save = vi
      .spyOn(desktopApi, "reorderApplicationWorkflow")
      .mockRejectedValueOnce(new Error("顺序保存失败"));
    render(<Harness initial={initial} />, { wrapper: DraftGuardProvider });
    fireEvent.click(screen.getByRole("button", { name: "调整阶段顺序" }));
    expect(
      screen.getByRole("button", { name: "上移 准备投递" }),
    ).toBeDisabled();
    expect(screen.getByRole("button", { name: "下移 待签约" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "上移 面试考核" }));
    expect(save).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    fireEvent.click(await screen.findByRole("button", { name: "继续编辑" }));
    fireEvent.click(screen.getByRole("button", { name: "保存顺序" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("顺序保存失败");
    const expected = stages.map((stage) => stage.id);
    [expected[3], expected[4]] = [expected[4]!, expected[3]!];
    expect(save).toHaveBeenCalledWith(initial.id, 1, expected);
    save.mockResolvedValueOnce({
      ...initial,
      revision: 2,
      stages: expected.map((id) => stages.find((s) => s.id === id)!),
    });
    fireEvent.click(screen.getByRole("button", { name: "保存顺序" }));
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
  });

  it("edits record-only state labels, order and classification with draft protection and retry", async () => {
    const initial = applicationFixture();
    const save = vi
      .spyOn(desktopApi, "updateApplicationStates")
      .mockRejectedValueOnce(new Error("辅助状态版本冲突"));
    render(<Harness initial={initial} />, { wrapper: DraftGuardProvider });
    fireEvent.click(screen.getByRole("button", { name: "管理辅助状态" }));
    expect(screen.getByLabelText("辅助状态 1 分类")).toBeDisabled();
    expect(screen.getByLabelText("辅助状态 6 名称")).toBeDisabled();
    expect(
      screen.queryByRole("button", { name: "移除辅助状态 尚未开始" }),
    ).not.toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("辅助状态 4 名称"), {
      target: { value: "等待通知" },
    });
    fireEvent.click(screen.getByRole("button", { name: "添加辅助状态" }));
    fireEvent.change(screen.getByLabelText("辅助状态 7 名称"), {
      target: { value: "等主管反馈" },
    });
    fireEvent.change(screen.getByLabelText("辅助状态 7 分类"), {
      target: { value: "awaitingResult" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "上移辅助状态 等主管反馈" }),
    );
    expect(save).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    fireEvent.click(await screen.findByRole("button", { name: "继续编辑" }));
    fireEvent.click(screen.getByRole("button", { name: "保存辅助状态" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "辅助状态版本冲突",
    );
    expect(screen.getByLabelText("辅助状态 6 名称")).toHaveValue("等主管反馈");
    const request = save.mock.calls[0]?.[0];
    expect(request).toMatchObject({ ownerId: initial.id, revision: 1 });
    expect(request?.states).toHaveLength(7);
    expect(request?.states[3]).toEqual({
      id: initial.auxiliaryStates[3]!.id,
      displayName: "等待通知",
      semanticKind: "awaitingResult",
    });
    expect(request?.states[5]).toEqual({
      id: null,
      displayName: "等主管反馈",
      semanticKind: "awaitingResult",
    });
    const renamed = initial.auxiliaryStates.map((state) =>
      state.stableKey === "awaitingResult"
        ? { ...state, displayName: "等待通知" }
        : state,
    );
    save.mockResolvedValueOnce(
      applicationFixture({ revision: 2, auxiliaryStates: renamed }),
    );
    fireEvent.click(screen.getByRole("button", { name: "保存辅助状态" }));
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(
      within(screen.getByLabelText("辅助状态")).getByRole("option", {
        name: "等待通知",
      }),
    ).toBeInTheDocument();
  });

  it("uses scoped state keys for progress and interview selections without offering a custom terminal", async () => {
    const base = applicationFixture();
    const initial = applicationFixture({
      auxiliaryStates: [
        ...base.auxiliaryStates,
        {
          id: "custom-id",
          stableKey: "custom_wait",
          displayName: "待主管回复",
          semanticKind: "awaitingResult",
          displayOrder: 70,
          inUse: true,
        },
      ],
    });
    const transition = vi
      .spyOn(desktopApi, "changeApplicationStage")
      .mockResolvedValue(initial);
    const round = vi
      .spyOn(desktopApi, "saveInterviewRound")
      .mockResolvedValue(initial);
    render(<Harness initial={initial} />, { wrapper: DraftGuardProvider });
    expect(
      within(screen.getByLabelText("辅助状态")).queryByRole("option", {
        name: "未通过",
      }),
    ).not.toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("辅助状态"), {
      target: { value: "custom_wait" },
    });
    await waitFor(() =>
      expect(transition).toHaveBeenCalledWith(
        expect.objectContaining({
          applicationId: initial.id,
          stageState: "custom_wait",
          revision: 1,
        }),
      ),
    );
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "+ 添加面试轮次" }),
      ).toBeEnabled(),
    );
    fireEvent.click(screen.getByRole("button", { name: "+ 添加面试轮次" }));
    fireEvent.change(screen.getByLabelText("轮次状态"), {
      target: { value: "custom_wait" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存轮次" }));
    await waitFor(() =>
      expect(round).toHaveBeenCalledWith(
        expect.objectContaining({ state: "custom_wait", revision: 1 }),
      ),
    );
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    fireEvent.click(screen.getByRole("button", { name: "管理辅助状态" }));
    expect(
      screen.getByRole("button", { name: "移除辅助状态 待主管回复" }),
    ).toBeDisabled();
  });

  it("validates duplicate labels and confirms removal without writing until save", async () => {
    const save = vi.spyOn(desktopApi, "updateApplicationStates");
    render(<Harness />, { wrapper: DraftGuardProvider });
    fireEvent.click(screen.getByRole("button", { name: "管理辅助状态" }));
    fireEvent.click(screen.getByRole("button", { name: "添加辅助状态" }));
    fireEvent.change(screen.getByLabelText("辅助状态 7 名称"), {
      target: { value: "待结果" },
    });
    expect(screen.getByRole("alert")).toHaveTextContent("名称不能重复");
    expect(screen.getByRole("button", { name: "保存辅助状态" })).toBeDisabled();
    fireEvent.click(
      screen.getByRole("button", { name: "移除辅助状态 待结果" }),
    );
    fireEvent.click(await screen.findByRole("button", { name: "移除状态" }));
    await waitFor(() =>
      expect(
        screen.queryByLabelText("辅助状态 7 名称"),
      ).not.toBeInTheDocument(),
    );
    expect(screen.getByRole("button", { name: "保存辅助状态" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(save).not.toHaveBeenCalled();
  });
});
