import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { desktopApi } from "../../lib/tauri";
import { DraftGuardProvider } from "../../shared/DraftGuardProvider";
import { applicationFixture } from "../../test/applicationFixture";
import { overviewFixture } from "../../test/productivityFixture";
import type { RecruitmentEvent, ScheduleEntry } from "./contracts";
import { EventEditor } from "./EventEditor";
import { EventsPage } from "./EventsPage";
import { SchedulePage } from "./SchedulePage";
import { OverviewProvider, ReminderBanner } from "./OverviewProvider";
import { scheduleGroup } from "./model";

const event = (
  overrides: Partial<RecruitmentEvent> = {},
): RecruitmentEvent => ({
  id: "event-one",
  revision: 1,
  applicationId: "fixture-application",
  applicationLabel: "示例公司 · 开发工程师",
  applicationArchived: false,
  applicationTerminal: false,
  eventType: "assessment",
  title: "测评安排",
  notes: "事件长说明",
  startsAtUtc: "2026-09-03T05:00:00Z",
  deadlineAtUtc: "2026-09-04T04:00:00Z",
  completedAtUtc: null,
  finished: false,
  interviewRoundId: null,
  interviewRoundName: null,
  location: "线上",
  meetingUrl: "https://example.com/meeting",
  result: "待结果",
  createdAtUtc: "2026-09-03T00:00:00Z",
  updatedAtUtc: "2026-09-03T00:00:00Z",
  sourceVersion: "1",
  ...overrides,
});
const entry = (overrides: Partial<ScheduleEntry> = {}): ScheduleEntry => ({
  key: "event:event-one",
  sourceKind: "event",
  sourceId: "event-one",
  applicationId: "fixture-application",
  label: "测评安排",
  atUtc: "2026-09-04T04:00:00Z",
  startsAtUtc: "2026-09-03T05:00:00Z",
  finished: false,
  highPriority: false,
  ...overrides,
});
const onError = vi.fn(),
  open = vi.fn(),
  cancel = vi.fn(),
  saved = vi.fn();
beforeEach(() => {
  vi.spyOn(desktopApi, "listRecruitmentEvents").mockResolvedValue([event()]);
  vi.spyOn(desktopApi, "listApplications").mockImplementation((scope) =>
    Promise.resolve(scope === "active" ? [applicationFixture()] : []),
  );
  vi.spyOn(desktopApi, "getApplication").mockResolvedValue(
    applicationFixture({
      interviewRounds: [
        {
          id: "round-one",
          sequenceNumber: 1,
          displayName: "HR 面",
          state: "awaitingParticipation",
          scheduledAtUtc: "2026-09-03T05:00:00Z",
          completedAtUtc: null,
          result: "",
          notes: "",
        },
      ],
    }),
  );
  vi.spyOn(desktopApi, "getOverview").mockResolvedValue(
    overviewFixture({ schedule: [entry()] }),
  );
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.useRealTimers();
  onError.mockReset();
  open.mockReset();
  cancel.mockReset();
  saved.mockReset();
});
const showEvents = (writable = true) =>
  render(
    <EventsPage
      writable={writable}
      onError={onError}
      onOpenApplication={open}
    />,
    { wrapper: DraftGuardProvider },
  );

it("creates an event with metadata then completes and reopens without deleting content", async () => {
  const save = vi
    .spyOn(desktopApi, "saveRecruitmentEvent")
    .mockResolvedValue(event({ id: "new", title: "新测评" }));
  const complete = vi
    .spyOn(desktopApi, "completeRecruitmentEvent")
    .mockResolvedValue(
      event({
        id: "new",
        title: "新测评",
        revision: 2,
        finished: true,
        completedAtUtc: "2026-09-04T04:00:00Z",
      }),
    );
  showEvents();
  await screen.findByText("测评安排 · 在线测评 · 未完成");
  fireEvent.click(screen.getByText("新建招聘事件"));
  const dialog = screen.getByRole("dialog");
  fireEvent.change(within(dialog).getByLabelText("事件标题"), {
    target: { value: "新测评" },
  });
  fireEvent.change(within(dialog).getByLabelText("事件关联投递"), {
    target: { value: "fixture-application" },
  });
  fireEvent.change(within(dialog).getByLabelText("计划时间"), {
    target: { value: "2026-09-03T13:00" },
  });
  fireEvent.change(within(dialog).getByLabelText("事件备注"), {
    target: { value: "保留备注" },
  });
  fireEvent.click(within(dialog).getByText("保存事件"));
  await screen.findByText("新测评 · 在线测评 · 未完成");
  expect(save).toHaveBeenCalledWith(
    expect.objectContaining({
      id: null,
      revision: null,
      applicationId: "fixture-application",
      title: "新测评",
      notes: "保留备注",
      interviewRoundId: null,
    }),
  );
  const row = screen
    .getByText("新测评 · 在线测评 · 未完成")
    .closest("article")!;
  fireEvent.click(within(row).getByText("完成事件"));
  await screen.findByText("新测评 · 在线测评 · 已完成");
  expect(complete).toHaveBeenCalledWith("new", 1, true);
  expect(within(row).getByText("事件长说明")).toBeInTheDocument();
  complete.mockResolvedValue(
    event({ id: "new", title: "新测评", revision: 3 }),
  );
  fireEvent.click(within(row).getByText("重新打开事件"));
  await screen.findByText("新测评 · 在线测评 · 未完成");
  expect(complete).toHaveBeenLastCalledWith("new", 2, false);
});

it("links only an owned round and sends no duplicate schedule or result fields", async () => {
  const save = vi
    .spyOn(desktopApi, "saveRecruitmentEvent")
    .mockResolvedValue(
      event({ interviewRoundId: "round-one", eventType: "interview" }),
    );
  render(
    <EventEditor
      event={event()}
      applications={[applicationFixture()]}
      onSaved={saved}
      onCancel={cancel}
      onError={onError}
    />,
    { wrapper: DraftGuardProvider },
  );
  fireEvent.change(screen.getByLabelText("事件类型"), {
    target: { value: "interview" },
  });
  await screen.findByRole("option", { name: "HR 面" });
  fireEvent.change(screen.getByLabelText("关联面试轮次"), {
    target: { value: "round-one" },
  });
  expect(screen.queryByLabelText("计划时间")).not.toBeInTheDocument();
  expect(screen.queryByLabelText("事件结果")).not.toBeInTheDocument();
  fireEvent.click(screen.getByText("保存事件"));
  await waitFor(() => expect(saved).toHaveBeenCalled());
  expect(save).toHaveBeenCalledWith(
    expect.objectContaining({
      id: "event-one",
      revision: 1,
      eventType: "interview",
      interviewRoundId: "round-one",
      startsAtUtc: null,
      result: "",
    }),
  );
});

it("keeps failed edits, prevents repeat submission, and confirms abandoning a draft", async () => {
  let reject!: (error: Error) => void;
  vi.spyOn(desktopApi, "saveRecruitmentEvent").mockImplementation(
    () =>
      new Promise((_, no) => {
        reject = no;
      }),
  );
  render(
    <EventEditor
      event={event()}
      applications={[applicationFixture()]}
      onSaved={saved}
      onCancel={cancel}
      onError={onError}
    />,
    { wrapper: DraftGuardProvider },
  );
  fireEvent.change(screen.getByLabelText("事件备注"), {
    target: { value: "尚未保存的说明" },
  });
  fireEvent.click(screen.getByText("保存事件"));
  expect(screen.getByText("正在保存…")).toBeDisabled();
  expect(screen.getByText("取消")).toBeDisabled();
  await act(async () => {
    reject(new Error("事件版本冲突"));
    await Promise.resolve();
  });
  expect(await screen.findByText("事件版本冲突")).toBeInTheDocument();
  expect(screen.getByLabelText("事件备注")).toHaveValue("尚未保存的说明");
  fireEvent.click(screen.getByText("取消"));
  await screen.findByText("放弃修改并继续");
  expect(cancel).not.toHaveBeenCalled();
  fireEvent.click(screen.getByText("放弃修改并继续"));
  await waitFor(() => expect(cancel).toHaveBeenCalledOnce());
});

it("shows read failures distinctly, protects readonly events, and leaves linked completion in the workflow", async () => {
  vi.mocked(desktopApi.listRecruitmentEvents).mockRejectedValueOnce(
    new Error("事件读取失败"),
  );
  const view = showEvents();
  await screen.findByRole("alert");
  expect(screen.queryByText("当前范围暂无事件。")).not.toBeInTheDocument();
  fireEvent.click(screen.getByText("刷新事件"));
  await screen.findByText("测评安排 · 在线测评 · 未完成");
  view.unmount();
  vi.mocked(desktopApi.listRecruitmentEvents).mockResolvedValue([
    event({ interviewRoundId: "round-one", interviewRoundName: "HR 面" }),
  ]);
  showEvents(false);
  await screen.findByText("测评安排 · 在线测评 · 未完成");
  expect(screen.getByText("编辑事件")).toBeDisabled();
  expect(screen.getByText("新建招聘事件")).toBeDisabled();
  expect(screen.queryByText("完成事件")).not.toBeInTheDocument();
  fireEvent.click(screen.getByText("示例公司 · 开发工程师"));
  expect(open).toHaveBeenCalledWith("fixture-application", false);
});

it("drills down exact schedule keys, supports empty scope, and opens original source IDs", async () => {
  vi.mocked(desktopApi.getOverview).mockResolvedValue(
    overviewFixture({
      schedule: [
        entry(),
        entry({
          key: "task:t",
          sourceId: "t",
          sourceKind: "task",
          label: "通用待办",
          startsAtUtc: null,
        }),
      ],
    }),
  );
  const view = render(
    <SchedulePage
      onError={onError}
      onOpen={open}
      initialScope={{ label: "空指标", keys: [] }}
    />,
  );
  await screen.findByText("当前范围暂无日程。");
  expect(screen.queryByText("通用待办")).not.toBeInTheDocument();
  fireEvent.click(screen.getByText("清除日程范围"));
  await screen.findByText("通用待办");
  fireEvent.click(screen.getByText("测评安排"));
  expect(open).toHaveBeenCalledWith(
    expect.objectContaining({ key: "event:event-one", sourceId: "event-one" }),
  );
  fireEvent.change(screen.getByLabelText("日程来源"), {
    target: { value: "task" },
  });
  expect(screen.queryByText("测评安排")).not.toBeInTheDocument();
  view.unmount();
  expect(
    scheduleGroup(
      entry({ finished: true, atUtc: "2020-01-01T00:00:00Z" }),
      new Date(),
    ),
  ).toBe("已完成");
  expect(scheduleGroup(entry({ atUtc: null }), new Date())).toBe("无日期");
});

it("shares one overview subscription across pages, refreshes on focus and writes, and stops when disabled", async () => {
  const tree = (page: string, enabled = true) => (
    <OverviewProvider enabled={enabled} page={page} onError={onError}>
      <ReminderBanner onOpen={open} />
      <SchedulePage onError={onError} onOpen={open} />
    </OverviewProvider>
  );
  const view = render(tree("tasks"));
  await screen.findByText("测评安排");
  expect(desktopApi.getOverview).toHaveBeenCalledTimes(1);
  view.rerender(tree("settings"));
  await waitFor(() => expect(desktopApi.getOverview).toHaveBeenCalledTimes(2));
  fireEvent(window, new Event("focus"));
  await waitFor(() => expect(desktopApi.getOverview).toHaveBeenCalledTimes(3));
  fireEvent(window, new Event("offertrack-data-changed"));
  await waitFor(() => expect(desktopApi.getOverview).toHaveBeenCalledTimes(4));
  view.rerender(tree("settings", false));
  fireEvent(window, new Event("focus"));
  fireEvent(window, new Event("offertrack-data-changed"));
  expect(desktopApi.getOverview).toHaveBeenCalledTimes(4);
});

it("ignores late results after warehouse context replacement", async () => {
  let resolve!: (data: ReturnType<typeof overviewFixture>) => void;
  vi.mocked(desktopApi.getOverview).mockImplementationOnce(
    () =>
      new Promise((yes) => {
        resolve = yes;
      }),
  );
  const tree = (key: string) => (
    <OverviewProvider key={key} enabled page="tasks" onError={onError}>
      <SchedulePage onError={onError} onOpen={open} />
    </OverviewProvider>
  );
  const view = render(tree("warehouse-a"));
  view.rerender(tree("warehouse-b"));
  await screen.findByText("测评安排");
  await act(async () => {
    resolve(overviewFixture({ schedule: [entry({ label: "旧仓库条目" })] }));
    await Promise.resolve();
  });
  expect(screen.queryByText("旧仓库条目")).not.toBeInTheDocument();
  expect(screen.getByText("测评安排")).toBeInTheDocument();
});

it("polls once per minute across the app and removes its timer on unmount", async () => {
  vi.useFakeTimers();
  const view = render(
    <OverviewProvider enabled page="settings" onError={onError}>
      <ReminderBanner onOpen={open} />
    </OverviewProvider>,
  );
  await act(async () => {
    await Promise.resolve();
  });
  expect(desktopApi.getOverview).toHaveBeenCalledTimes(1);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60_000);
  });
  expect(desktopApi.getOverview).toHaveBeenCalledTimes(2);
  view.unmount();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(120_000);
  });
  expect(desktopApi.getOverview).toHaveBeenCalledTimes(2);
});
