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
import { applicationFixture } from "../../test/applicationFixture";
import { ApplicationPage } from "./ApplicationPage";
import { DraftGuardProvider } from "../../shared/DraftGuardProvider";
import { CustomFieldInput } from "./CustomFieldInput";
import { defaultColumns, initialFilter } from "./tableModel";
import { workflowFixture } from "../../test/workflowFixture";

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => undefined),
}));
const onError = vi.fn();

describe("application management", () => {
  it("completes the UI journey from creation through editing, application, offer and archive", async () => {
    const stages = workflowFixture().stages;
    let record = applicationFixture({ stages, currentStageId: stages[0]!.id });
    let created = false;
    vi.mocked(desktopApi.listApplications).mockImplementation(() =>
      Promise.resolve(created && !record.archivedAtUtc ? [record] : []),
    );
    vi.mocked(desktopApi.getApplication).mockImplementation(() =>
      Promise.resolve(record),
    );
    const create = vi
      .spyOn(desktopApi, "createApplication")
      .mockImplementation((request) => {
        created = true;
        record = { ...record, ...request };
        return Promise.resolve(record);
      });
    const update = vi
      .spyOn(desktopApi, "updateApplication")
      .mockImplementation((request) => {
        record = {
          ...record,
          notes: request.notes,
          revision: record.revision + 1,
        };
        return Promise.resolve(record);
      });
    const transition = vi
      .spyOn(desktopApi, "changeApplicationStage")
      .mockImplementation((request) => {
        const stage = stages.find((entry) => entry.id === request.stageId)!;
        record = {
          ...record,
          currentStageId: stage.id,
          currentStageName: stage.displayName,
          currentStageState: stage.isTerminal
            ? "completed"
            : request.stageState,
          applicationDate: record.applicationDate ?? "2026-09-03",
          revision: record.revision + 1,
        };
        return Promise.resolve(record);
      });
    const archive = vi
      .spyOn(desktopApi, "setApplicationArchived")
      .mockImplementation(() => {
        record = { ...record, archivedAtUtc: "2026-09-03T03:00:00Z" };
        return Promise.resolve(record);
      });
    render(<ApplicationPage scope="active" writable onError={onError} />, {
      wrapper: DraftGuardProvider,
    });
    await screen.findByText("没有符合当前条件的投递记录。");
    fireEvent.click(screen.getByRole("button", { name: "新建投递" }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.change(within(dialog).getByLabelText("公司名称"), {
      target: { value: "验收公司" },
    });
    fireEvent.change(within(dialog).getByLabelText("岗位名称"), {
      target: { value: "开发" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "创建投递" }));
    await screen.findByLabelText("关闭详情");
    expect(create).toHaveBeenCalledTimes(1);
    fireEvent.change(screen.getByLabelText("备注"), {
      target: { value: "准备材料" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存修改" }));
    await waitFor(() =>
      expect(update).toHaveBeenCalledWith(
        expect.objectContaining({ notes: "准备材料" }),
      ),
    );
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "保存修改" })).toBeEnabled(),
    );
    fireEvent.click(screen.getByRole("button", { name: "流程" }));
    await screen.findByLabelText("招聘阶段");
    for (const key of ["applied", "interview", "offer"]) {
      const stage = stages.find((entry) => entry.stableKey === key)!;
      await waitFor(() =>
        expect(screen.getByLabelText("招聘阶段")).toBeEnabled(),
      );
      fireEvent.change(screen.getByLabelText("招聘阶段"), {
        target: { value: stage.id },
      });
      await waitFor(() =>
        expect(screen.getByLabelText("招聘阶段")).toHaveValue(stage.id),
      );
    }
    expect(transition).toHaveBeenCalledTimes(3);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "归档" })).toBeEnabled(),
    );
    fireEvent.click(screen.getByRole("button", { name: "归档" }));
    await waitFor(() => expect(archive).toHaveBeenCalledWith(record.id, true));
    await screen.findByText("没有符合当前条件的投递记录。");
    expect(onError).not.toHaveBeenCalled();
  }, 15000);
  beforeEach(() => {
    vi.spyOn(desktopApi, "listApplications").mockResolvedValue([
      applicationFixture(),
    ]);
    vi.spyOn(desktopApi, "getApplication").mockResolvedValue(
      applicationFixture(),
    );
    vi.spyOn(desktopApi, "listFieldDefinitions").mockResolvedValue([]);
    vi.spyOn(desktopApi, "listApplicationViews").mockResolvedValue([]);
    vi.spyOn(desktopApi, "getApplicationPageSize").mockResolvedValue(50);
    vi.spyOn(desktopApi, "setApplicationPageSize").mockResolvedValue(20);
    vi.spyOn(desktopApi, "openWebUrl").mockResolvedValue();
  });
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
    onError.mockClear();
  });

  it("restores the default view and page size on mount and does not reset them on selection", async () => {
    vi.mocked(desktopApi.listApplicationViews).mockResolvedValue([
      {
        id: "saved",
        revision: 1,
        name: "准备阶段",
        layout: {
          columns: defaultColumns.map((column) => ({
            ...column,
            visible: column.key === "companyName",
          })),
        },
        sort: [{ key: "companyName", direction: "asc" }],
        filter: { ...initialFilter, search: "示例" },
        group: "companyType",
        isDefault: true,
      },
    ]);
    vi.mocked(desktopApi.getApplicationPageSize).mockResolvedValue(100);
    render(<ApplicationPage scope="active" writable onError={onError} />, {
      wrapper: DraftGuardProvider,
    });
    await screen.findByText("示例公司");
    expect(screen.getByLabelText("每页条数")).toHaveValue("100");
    expect(screen.getByLabelText("搜索投递")).toHaveValue("示例");
    expect(screen.getByLabelText("已保存视图")).toHaveValue("saved");
    fireEvent.change(screen.getByLabelText("每页条数"), {
      target: { value: "20" },
    });
    fireEvent.click(screen.getByText("示例公司"));
    await screen.findByLabelText("关闭详情");
    expect(screen.getByLabelText("每页条数")).toHaveValue("20");
    expect(desktopApi.getApplicationPageSize).toHaveBeenCalledTimes(1);
    expect(desktopApi.setApplicationPageSize).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "设为默认" }));
    await waitFor(() =>
      expect(desktopApi.setApplicationPageSize).toHaveBeenCalledWith(20),
    );
  });

  it("uses presets in read-only archives and saves a semantic filter for the next session", async () => {
    const ended = applicationFixture({
      id: "ended",
      companyName: "已结束公司",
      currentStageKey: "interview",
      currentStageTerminal: true,
      currentStageState: "failed",
    });
    vi.mocked(desktopApi.listApplications).mockResolvedValue([
      applicationFixture(),
      ended,
    ]);
    const saved = {
      id: "view",
      revision: 1,
      name: "结果跟踪",
      isDefault: true,
      layout: { columns: defaultColumns },
      filter: { ...initialFilter, businessState: "ended" as const },
      sort: [{ key: "createdAtUtc", direction: "desc" as const }],
      group: null,
    };
    const save = vi
      .spyOn(desktopApi, "saveApplicationView")
      .mockResolvedValue({ view: saved, views: [saved] });
    const mounted = render(
      <ApplicationPage scope="active" writable onError={onError} />,
      { wrapper: DraftGuardProvider },
    );
    await screen.findByText("已结束公司");
    fireEvent.change(screen.getByLabelText("快捷视图"), {
      target: { value: "ended" },
    });
    expect(screen.queryByText("示例公司")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "保存视图" }));
    fireEvent.change(await screen.findByLabelText("视图名称"), {
      target: { value: "结果跟踪" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() =>
      expect(save).toHaveBeenCalledWith(
        expect.objectContaining({ filter: saved.filter }),
      ),
    );
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    mounted.unmount();
    vi.mocked(desktopApi.listApplicationViews).mockResolvedValue([saved]);
    render(
      <ApplicationPage scope="archived" writable={false} onError={onError} />,
      { wrapper: DraftGuardProvider },
    );
    await screen.findByText("已结束公司");
    expect(desktopApi.listApplications).toHaveBeenLastCalledWith("archived");
    expect(screen.getByLabelText("业务状态筛选")).toHaveValue("ended");
    expect(screen.queryByText("示例公司")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "保存视图" })).toBeDisabled();
    fireEvent.change(screen.getByLabelText("快捷视图"), {
      target: { value: "all" },
    });
    expect(screen.getByText("示例公司")).toBeInTheDocument();
    expect(save).toHaveBeenCalledTimes(1);
  });

  it("preserves draft, columns and selection when a preset exits an overview scope", async () => {
    const ended = applicationFixture({
      id: "ended",
      companyName: "已结束公司",
      currentStageKey: "offer",
      currentStageTerminal: true,
    });
    vi.mocked(desktopApi.listApplications).mockResolvedValue([
      applicationFixture(),
      ended,
    ]);
    vi.mocked(desktopApi.listApplicationViews).mockResolvedValue([
      {
        id: "layout",
        revision: 1,
        name: "公司列",
        isDefault: true,
        layout: {
          columns: defaultColumns.map((column) => ({
            ...column,
            visible: column.key === "companyName",
            width: 300,
          })),
        },
        sort: [],
        filter: initialFilter,
        group: null,
      },
    ]);
    const onRecycle = vi.fn();
    render(
      <ApplicationPage
        scope="active"
        writable
        onError={onError}
        onRecycle={onRecycle}
        drilldown={{
          label: "测试范围",
          ids: ["fixture-application", "missing"],
        }}
      />,
      { wrapper: DraftGuardProvider },
    );
    fireEvent.click(await screen.findByText("示例公司"));
    fireEvent.change(await screen.findByLabelText("岗位名称"), {
      target: { value: "保留此草稿" },
    });
    fireEvent.click(screen.getByRole("button", { name: "选择本页" }));
    fireEvent.change(screen.getByLabelText("快捷视图"), {
      target: { value: "ended" },
    });
    expect(
      screen.queryByRole("button", { name: "清除概览范围" }),
    ).not.toBeInTheDocument();
    expect(screen.getByText("已结束公司")).toBeInTheDocument();
    expect(screen.getByLabelText("岗位名称")).toHaveValue("保留此草稿");
    expect(screen.getByText("已选 1 条（跨页保留）")).toBeInTheDocument();
    expect(screen.getByRole("columnheader", { name: "公司名称" })).toHaveStyle({
      width: "300px",
    });
    expect(
      screen.queryByRole("columnheader", { name: /创建日期/ }),
    ).not.toBeInTheDocument();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("快捷视图"), {
      target: { value: "recycle" },
    });
    expect(onRecycle).toHaveBeenCalledOnce();
    expect(screen.getByLabelText("快捷视图")).toHaveValue("ended");
  });

  it("intersects the business filter with search without dropping an empty overview scope", async () => {
    render(
      <ApplicationPage
        scope="active"
        writable={false}
        onError={onError}
        drilldown={{ label: "空范围", ids: [] }}
      />,
      { wrapper: DraftGuardProvider },
    );
    await screen.findByText("没有符合当前条件的投递记录。");
    fireEvent.change(screen.getByLabelText("业务状态筛选"), {
      target: { value: "preparing" },
    });
    expect(screen.queryByText("示例公司")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "清除概览范围" }));
    expect(screen.getByText("示例公司")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("搜索投递"), {
      target: { value: "无此公司" },
    });
    expect(screen.queryByText("示例公司")).not.toBeInTheDocument();
    expect(screen.getByLabelText("业务状态筛选")).toHaveValue("preparing");
    expect(screen.getByLabelText("快捷视图")).toHaveValue("");
    fireEvent.change(screen.getByLabelText("快捷视图"), {
      target: { value: "all" },
    });
    expect(screen.getByLabelText("搜索投递")).toHaveValue("");
    expect(screen.getByLabelText("业务状态筛选")).toHaveValue("");
    expect(screen.getByText("示例公司")).toBeInTheDocument();
  });

  it("opens URLs only with Ctrl+click and keeps a read-only view from saving settings", async () => {
    render(
      <ApplicationPage scope="active" writable={false} onError={onError} />,
      { wrapper: DraftGuardProvider },
    );
    const link = await screen.findByRole("button", {
      name: "https://example.com/apply",
    });
    fireEvent.click(link);
    await screen.findByLabelText("关闭详情");
    expect(desktopApi.openWebUrl).not.toHaveBeenCalled();
    fireEvent.click(link, { ctrlKey: true });
    expect(desktopApi.openWebUrl).toHaveBeenCalledWith(
      "https://example.com/apply",
    );
    expect(screen.getByRole("button", { name: "保存视图" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "设为默认" })).toBeDisabled();
  });

  it("retries initial metadata failure without claiming there are no records", async () => {
    vi.mocked(desktopApi.listApplicationViews).mockRejectedValueOnce(
      new Error("视图读取失败"),
    );
    render(<ApplicationPage scope="active" writable onError={onError} />, {
      wrapper: DraftGuardProvider,
    });
    expect(await screen.findByRole("alert")).toHaveTextContent("视图读取失败");
    expect(
      screen.queryByText("没有符合当前条件的投递记录。"),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("共 0 条")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "保存视图" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "重新读取投递与视图" }));
    await screen.findByText("示例公司");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "保存视图" })).toBeEnabled();
  });

  it("deleting a default view retains records and temporary layout, and an empty sort stays empty", async () => {
    vi.mocked(desktopApi.listApplicationViews).mockResolvedValue([
      {
        id: "view",
        revision: 4,
        name: "默认视图",
        isDefault: true,
        layout: { columns: defaultColumns },
        sort: [],
        filter: initialFilter,
        group: null,
      },
    ]);
    const remove = vi
      .spyOn(desktopApi, "deleteApplicationView")
      .mockResolvedValue([]);
    render(<ApplicationPage scope="active" writable onError={onError} />, {
      wrapper: DraftGuardProvider,
    });
    await screen.findByText("示例公司");
    expect(
      screen.getByRole("columnheader", { name: "创建日期" }),
    ).toHaveTextContent(/^创建日期\s*$/);
    fireEvent.change(screen.getByLabelText("搜索投递"), {
      target: { value: "示例" },
    });
    fireEvent.click(screen.getByRole("button", { name: "删除视图" }));
    fireEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "删除视图",
      }),
    );
    await waitFor(() =>
      expect(screen.getByLabelText("已保存视图")).toHaveValue(""),
    );
    expect(remove).toHaveBeenCalledWith("view", 4);
    expect(screen.getByLabelText("搜索投递")).toHaveValue("示例");
    expect(screen.getByText("示例公司")).toBeInTheDocument();
    expect(
      screen.getByRole("columnheader", { name: "创建日期" }),
    ).toBeInTheDocument();
  });

  it("submits real booleans and clears optional fields instead of inventing values", () => {
    const onChange = vi.fn();
    render(
      <CustomFieldInput
        disabled={false}
        field={{
          id: "flag",
          revision: 1,
          key: "flag",
          displayName: "是否内推",
          fieldType: "boolean",
          config: {},
          displayOrder: 0,
          isVisible: true,
        }}
        value={undefined}
        onChange={onChange}
      />,
    );
    fireEvent.change(screen.getByLabelText("是否内推"), {
      target: { value: "false" },
    });
    expect(onChange).toHaveBeenLastCalledWith(false);
    fireEvent.change(screen.getByLabelText("是否内推"), {
      target: { value: "" },
    });
    expect(onChange).toHaveBeenLastCalledWith(undefined);
  });

  it("shows the current record label in the table but immutable names in history", async () => {
    const detail = applicationFixture({
      currentStateName: "现在的状态名",
      currentStageState: "custom_wait",
      history: [
        {
          id: "state-event",
          stageId: "stage-preparing",
          stageNameSnapshot: "原始阶段名",
          previousState: "pending",
          nextState: "custom_wait",
          previousStateNameSnapshot: "过去的准备名称",
          nextStateNameSnapshot: "过去的等待名称",
          previousStateKindSnapshot: "pending",
          nextStateKindSnapshot: "awaitingResult",
          notes: "",
          occurredAtUtc: "2026-09-03T01:00:00Z",
          actorType: "user",
        },
      ],
    });
    vi.mocked(desktopApi.listApplications).mockResolvedValue([detail]);
    vi.mocked(desktopApi.getApplication).mockResolvedValue(detail);
    render(<ApplicationPage scope="active" writable onError={onError} />, {
      wrapper: DraftGuardProvider,
    });
    expect(
      await screen.findByText(/准备投递 · 现在的状态名/),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByText("示例公司"));
    fireEvent.click(await screen.findByRole("button", { name: "历史" }));
    expect(
      await screen.findByText("过去的准备名称 → 过去的等待名称"),
    ).toBeInTheDocument();
    expect(screen.getByText("原始阶段名")).toBeInTheDocument();
  });

  it("keeps edited details when switching is cancelled and discards only after confirmation", async () => {
    const second = applicationFixture({
      id: "second",
      companyName: "第二家公司",
    });
    vi.mocked(desktopApi.listApplications).mockResolvedValue([
      applicationFixture(),
      second,
    ]);
    vi.mocked(desktopApi.getApplication).mockImplementation((id) =>
      Promise.resolve(id === "second" ? second : applicationFixture()),
    );
    render(<ApplicationPage scope="active" writable onError={onError} />, {
      wrapper: DraftGuardProvider,
    });
    fireEvent.click(await screen.findByText("示例公司"));
    fireEvent.change(await screen.findByLabelText("岗位名称"), {
      target: { value: "未保存岗位" },
    });
    fireEvent.click(screen.getByText("第二家公司"));
    fireEvent.click(await screen.findByText("继续编辑"));
    expect(screen.getByLabelText("岗位名称")).toHaveValue("未保存岗位");
    expect(desktopApi.getApplication).not.toHaveBeenCalledWith("second");
    fireEvent.click(screen.getByRole("button", { name: "流程" }));
    fireEvent.click(await screen.findByText("继续编辑"));
    expect(screen.getByLabelText("岗位名称")).toHaveValue("未保存岗位");
    fireEvent.click(screen.getByText("第二家公司"));
    fireEvent.click(await screen.findByText("放弃修改并继续"));
    await waitFor(() =>
      expect(screen.getByLabelText("公司名称")).toHaveValue("第二家公司"),
    );
  });

  it("keeps inputs on save failure and prevents closing during an in-flight save", async () => {
    let fail: (error: Error) => void = () => undefined;
    vi.spyOn(desktopApi, "updateApplication").mockImplementation(
      () =>
        new Promise((_, reject) => {
          fail = reject;
        }),
    );
    render(<ApplicationPage scope="active" writable onError={onError} />, {
      wrapper: DraftGuardProvider,
    });
    fireEvent.click(await screen.findByText("示例公司"));
    fireEvent.change(await screen.findByLabelText("岗位名称"), {
      target: { value: "未保存岗位" },
    });
    fireEvent.click(screen.getByText("保存修改"));
    expect(screen.getByLabelText("岗位名称")).toBeDisabled();
    fireEvent.click(screen.getByLabelText("关闭详情"));
    expect(
      await screen.findByRole("dialog", { name: "正在保存" }),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByText("知道了"));
    fail(new Error("保存失败"));
    await waitFor(() =>
      expect(screen.getByLabelText("岗位名称")).toBeEnabled(),
    );
    expect(screen.getByLabelText("岗位名称")).toHaveValue("未保存岗位");
    expect(onError).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByLabelText("关闭详情"));
    fireEvent.click(await screen.findByText("放弃修改并继续"));
    await waitFor(() =>
      expect(screen.queryByLabelText("关闭详情")).not.toBeInTheDocument(),
    );
  });

  it("protects the new-record form and submits without a spurious discard prompt", async () => {
    const create = vi
      .spyOn(desktopApi, "createApplication")
      .mockResolvedValue(applicationFixture());
    render(<ApplicationPage scope="active" writable onError={onError} />, {
      wrapper: DraftGuardProvider,
    });
    fireEvent.click(await screen.findByRole("button", { name: "新建投递" }));
    fireEvent.change(await screen.findByLabelText("公司名称"), {
      target: { value: "示例公司" },
    });
    fireEvent.change(screen.getByLabelText("岗位名称"), {
      target: { value: "开发工程师" },
    });
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    fireEvent.click(await screen.findByText("继续编辑"));
    expect(screen.getByLabelText("公司名称")).toHaveValue("示例公司");
    fireEvent.click(screen.getByRole("button", { name: "创建投递" }));
    await waitFor(() => expect(create).toHaveBeenCalledOnce());
    await screen.findByLabelText("关闭详情");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});
