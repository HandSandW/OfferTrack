import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
  act,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { desktopApi } from "../../lib/tauri";
import { DraftGuardProvider } from "../../shared/DraftGuardProvider";
import {
  taskFixture,
  overviewFixture,
  rulesFixture,
} from "../../test/productivityFixture";
import { applicationFixture } from "../../test/applicationFixture";
import { TasksPage } from "./TasksPage";
import { OverviewPage } from "./OverviewPage";
import { ReminderSettings } from "./ReminderSettings";
import { taskGroup } from "./model";
import type { Task } from "./contracts";

const onError = vi.fn();
const open = vi.fn();
beforeEach(() => {
  vi.spyOn(desktopApi, "listTasks").mockResolvedValue([taskFixture()]);
  vi.spyOn(desktopApi, "listApplications").mockImplementation((scope) =>
    Promise.resolve(scope === "active" ? [applicationFixture()] : []),
  );
  vi.spyOn(desktopApi, "getOverview").mockResolvedValue(overviewFixture());
  vi.spyOn(desktopApi, "listReminderRules").mockResolvedValue(rulesFixture());
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.useRealTimers();
  onError.mockReset();
  open.mockReset();
});
const showTasks = (writable = true) =>
  render(
    <TasksPage
      writable={writable}
      onError={onError}
      onOpenApplication={open}
    />,
    { wrapper: DraftGuardProvider },
  );
const showOverview = (writable = true) =>
  render(
    <OverviewPage
      writable={writable}
      onError={onError}
      onDrilldown={open}
      onTask={open}
      onSettings={open}
      onNew={open}
    />,
    { wrapper: DraftGuardProvider },
  );

it("groups exact local deadlines without turning undated or completed tasks overdue", () => {
  const now = new Date("2026-09-03T12:00:00+08:00");
  expect(taskGroup(taskFixture(), now)).toBe("无日期");
  expect(taskGroup(taskFixture({ dueAtUtc: now.toISOString() }), now)).toBe(
    "今天",
  );
  expect(
    taskGroup(
      taskFixture({ dueAtUtc: new Date(now.getTime() - 1).toISOString() }),
      now,
    ),
  ).toBe("已逾期");
  expect(
    taskGroup(
      taskFixture({
        dueAtUtc: "2020-01-01T00:00:00Z",
        completedAtUtc: now.toISOString(),
      }),
      now,
    ),
  ).toBe("已完成");
});

it("creates a general task then edits its link and preserves completion history", async () => {
  const save = vi
    .spyOn(desktopApi, "saveTask")
    .mockResolvedValue(taskFixture({ id: "new", title: "更新简历" }));
  const complete = vi.spyOn(desktopApi, "completeTask").mockResolvedValue(
    taskFixture({
      id: "new",
      title: "更新简历",
      revision: 2,
      completedAtUtc: "2026-09-03T03:00:00Z",
    }),
  );
  showTasks();
  await screen.findByText("准备作品集");
  fireEvent.click(screen.getByText("新建待办"));
  fireEvent.change(screen.getByLabelText("待办标题"), {
    target: { value: "更新简历" },
  });
  fireEvent.click(screen.getByText("保存待办"));
  await screen.findByText("更新简历");
  expect(save).toHaveBeenCalledWith(
    expect.objectContaining({
      id: null,
      revision: null,
      applicationId: null,
      title: "更新简历",
      dueAtUtc: null,
    }),
  );
  const item = screen.getByText("更新简历").closest("article")!;
  fireEvent.click(within(item).getByText("标记完成"));
  await screen.findByText("重新打开待办");
  expect(complete).toHaveBeenCalledWith("new", 1, true);
  fireEvent.click(
    within(screen.getByText("更新简历").closest("article")!).getByText(
      "编辑待办",
    ),
  );
  fireEvent.change(screen.getByLabelText("关联投递"), {
    target: { value: "fixture-application" },
  });
  fireEvent.click(screen.getByText("保存待办"));
  await waitFor(() => expect(save).toHaveBeenCalledTimes(2));
  expect(save).toHaveBeenLastCalledWith(
    expect.objectContaining({
      id: "new",
      revision: 2,
      applicationId: "fixture-application",
    }),
  );
});

it("retains failed task drafts, confirms discard, and blocks repeated pending saves", async () => {
  let reject!: (error: Error) => void;
  const save = vi.spyOn(desktopApi, "saveTask").mockImplementation(
    () =>
      new Promise<Task>((_, fail) => {
        reject = fail;
      }),
  );
  showTasks();
  await screen.findByText("准备作品集");
  fireEvent.click(screen.getByText("编辑待办"));
  fireEvent.change(screen.getByLabelText("待办备注"), {
    target: { value: "保留我的修改" },
  });
  fireEvent.click(screen.getByText("保存待办"));
  expect(screen.getByText("正在保存…")).toBeDisabled();
  await act(async () => {
    reject(new Error("版本冲突"));
    await Promise.resolve();
  });
  expect(await screen.findByRole("alert")).toHaveTextContent("版本冲突");
  expect(screen.getByLabelText("待办备注")).toHaveValue("保留我的修改");
  fireEvent.click(screen.getByText("取消"));
  await screen.findByText("有未保存的修改");
  fireEvent.click(screen.getByText("放弃修改并继续"));
  await waitFor(() =>
    expect(screen.queryByLabelText("待办备注")).not.toBeInTheDocument(),
  );
  expect(save).toHaveBeenCalledTimes(1);
});

it("does not invent empty data on load failure and disables readonly mutations", async () => {
  vi.mocked(desktopApi.listTasks).mockRejectedValueOnce(new Error("读取失败"));
  showTasks(false);
  await screen.findByRole("alert");
  expect(screen.queryByText("当前范围暂无待办。")).not.toBeInTheDocument();
  fireEvent.click(screen.getByText("刷新待办"));
  await screen.findByText("准备作品集");
  expect(screen.getByText("新建待办")).toBeDisabled();
  expect(screen.getByText("标记完成")).toBeDisabled();
});

it("saves independent reminder thresholds with revisions and retains rejected inputs", async () => {
  const save = vi
    .spyOn(desktopApi, "saveReminderRules")
    .mockRejectedValueOnce(new Error("设置冲突"))
    .mockResolvedValue(
      rulesFixture().map((r) => ({
        ...r,
        revision: 2,
        value: r.key === "missing_resume" ? 5 : r.value,
      })),
    );
  render(<ReminderSettings writable onError={onError} />, {
    wrapper: DraftGuardProvider,
  });
  fireEvent.click(screen.getByText("读取提醒规则"));
  await screen.findByLabelText("创建后尚无简历阈值");
  fireEvent.change(screen.getByLabelText("创建后尚无简历阈值"), {
    target: { value: "5" },
  });
  fireEvent.click(screen.getByText("保存提醒规则"));
  await screen.findByText("设置冲突");
  expect(screen.getByLabelText("创建后尚无简历阈值")).toHaveValue(5);
  fireEvent.click(screen.getByText("保存提醒规则"));
  await screen.findByText(/提醒规则已保存/);
  expect(save).toHaveBeenCalledWith(
    expect.arrayContaining([
      expect.objectContaining({ key: "missing_resume", value: 5, revision: 1 }),
    ]),
  );
  expect(screen.getByText("保存提醒规则")).toBeDisabled();
});

it("drills into explicit source IDs and handles reminders without changing a task", async () => {
  const reminder = {
    key: "task:one:priority",
    fingerprint: "verified",
    ruleKey: "priority",
    sourceKind: "task" as const,
    sourceId: "one",
    applicationId: null,
    label: "准备作品集",
    reason: "高优先级待办",
    severity: "normal" as const,
  };
  vi.mocked(desktopApi.getOverview)
    .mockResolvedValueOnce(
      overviewFixture({
        metrics: [{ label: "活跃投递", ids: ["a", "b"] }],
        reminders: [reminder],
      }),
    )
    .mockResolvedValue(overviewFixture());
  const respond = vi
    .spyOn(desktopApi, "respondToReminder")
    .mockResolvedValue(undefined);
  showOverview();
  fireEvent.click(await screen.findByRole("button", { name: "活跃投递 2" }));
  expect(open).toHaveBeenCalledWith({ label: "活跃投递", ids: ["a", "b"] });
  fireEvent.click(screen.getByText("打开待办"));
  expect(open).toHaveBeenLastCalledWith("one");
  fireEvent.click(screen.getByText("24 小时后提醒"));
  await screen.findByText(/已推迟 24 小时/);
  expect(respond).toHaveBeenCalledWith(reminder.key, "verified", true);
  await screen.findByText("当前没有触发的提醒。");
});

it("keeps previous overview on refresh failure and refreshes again on focus", async () => {
  showOverview();
  await screen.findByRole("button", { name: "活跃投递 0" });
  vi.mocked(desktopApi.getOverview).mockRejectedValueOnce(
    new Error("暂时不可读取"),
  );
  fireEvent.click(screen.getByText("刷新概览"));
  await screen.findByRole("alert");
  expect(
    screen.getByRole("button", { name: "活跃投递 0" }),
  ).toBeInTheDocument();
  fireEvent(window, new Event("focus"));
  await waitFor(() =>
    expect(screen.queryByRole("alert")).not.toBeInTheDocument(),
  );
  expect(desktopApi.getOverview).toHaveBeenCalledTimes(3);
});
