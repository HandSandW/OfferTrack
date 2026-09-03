import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { HelpWindow } from "./HelpWindow";
import { Diagnostics } from "./Diagnostics";
import { Markdown } from "./Markdown";
import { chapters } from "./content";
import {
  parseChapters,
  readingHistory,
  resolveHelpLink,
  searchChapters,
  topicChapter,
  topicTitles,
} from "./model";
import type { HelpLocation } from "./api";

const mocks = vi.hoisted(() => ({
  location: vi.fn(),
  diagnostics: vi.fn(),
  openLogs: vi.fn(),
  isTauri: vi.fn(),
  listen: vi.fn(),
  copy: vi.fn(),
}));
vi.mock("./api", () => ({ helpApi: mocks }));
vi.mock("@tauri-apps/api/core", () => ({ isTauri: mocks.isTauri }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
beforeEach(() => {
  vi.resetAllMocks();
  mocks.isTauri.mockReturnValue(false);
  mocks.listen.mockResolvedValue(() => undefined);
  mocks.location.mockResolvedValue({ topic: "manual", revision: 0 });
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: mocks.copy },
  });
  mocks.copy.mockResolvedValue(undefined);
});
afterEach(cleanup);

it("bundles all help entry topics and both appendices, retaining examples inside fenced code", () => {
  for (const topic of Object.keys(topicTitles))
    expect(chapters.some((chapter) => chapter.id === topicChapter(topic))).toBe(
      true,
    );
  for (const title of [
    "打开已有仓库",
    "自定义辅助状态",
    "安全批量修改",
    "独立数据库快照恢复",
    "导出投递表格",
    "允许 Agent 修改元数据",
    "浅色与深色主题",
  ])
    expect(chapters.some((chapter) => chapter.title === title)).toBe(true);
  expect(
    chapters.filter((chapter) => chapter.source === "guide").length,
  ).toBeGreaterThan(25);
  expect(chapters.some((chapter) => chapter.source === "agent")).toBe(true);
  expect(chapters.some((chapter) => chapter.source === "backup")).toBe(true);
  expect(new Set(chapters.map((chapter) => chapter.id)).size).toBe(
    chapters.length,
  );
  const parsed = parseChapters(
    "# Title\nintro\n## A\n```text\n## example, not heading\n```\n## B\nend",
    "test",
  );
  expect(parsed).toHaveLength(3);
  expect(parsed[1]?.body).toContain("## example, not heading");
  expect(() => parseChapters("## A\n1\n## A\n2", "x")).toThrow("Duplicate");
  expect(
    searchChapters(chapters, "恢复 简历").every(
      (chapter) =>
        chapter.body.includes("恢复") && chapter.body.includes("简历"),
    ),
  ).toBe(true);
  expect(searchChapters(chapters, "does-not-exist")).toEqual([]);
});

it("uses local-only links and never interprets markup, images or executable URLs", () => {
  const current = chapters.find(
    (chapter) => chapter.title === "让本地 Agent 分析投递",
  )!;
  expect(resolveHelpLink("../agent-api.md", current, chapters)).toBe(
    "agent:OfferTrack Agent 接口 v1",
  );
  expect(resolveHelpLink("../backup-format.md", current, chapters)).toBe(
    "backup:OfferTrack 完整备份格式 v1",
  );
  for (const href of [
    "javascript:alert(1)",
    "https://example.invalid",
    "file:///private",
    "../../../private",
    "#%zz",
  ])
    expect(resolveHelpLink(href, current, chapters)).toBeUndefined();
  const navigate = vi.fn();
  const { container } = render(
    <Markdown
      body={
        '<script>bad()</script>\n\n![remote](https://example.invalid/a.png) [run](javascript:alert)\n\n[appendix](../agent-api.md)\n\n```html\n<img src="https://example.invalid">\n```'
      }
      navigate={navigate}
      resolve={(href) => !!resolveHelpLink(href, current, chapters)}
    />,
  );
  expect(container.querySelector("script,img,a,iframe")).toBeNull();
  expect(screen.getByText("<script>bad()</script>")).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "appendix" }));
  expect(navigate).toHaveBeenCalledWith("../agent-api.md");
});

it("searches full chapters and supports local history, keyboard shortcuts and about license", () => {
  render(<HelpWindow />);
  const nav = within(screen.getByRole("navigation", { name: "手册章节" }));
  expect(screen.getByRole("heading", { name: "快速开始" })).toHaveFocus();
  fireEvent.click(nav.getByRole("button", { name: "快捷键" }));
  expect(screen.getByRole("heading", { name: "快捷键" })).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "后退" }));
  expect(screen.getByRole("heading", { name: "快速开始" })).toBeVisible();
  fireEvent.keyDown(window, { key: "ArrowRight", altKey: true });
  expect(screen.getByRole("heading", { name: "快捷键" })).toBeVisible();
  fireEvent.keyDown(window, { key: "f", ctrlKey: true });
  expect(screen.getByRole("searchbox")).toHaveFocus();
  fireEvent.change(screen.getByRole("searchbox"), {
    target: { value: "no-such-help" },
  });
  expect(screen.getByText(/没有匹配章节/)).toBeVisible();
  expect(screen.getByRole("heading", { name: "快捷键" })).toBeVisible();
  fireEvent.change(screen.getByRole("searchbox"), { target: { value: "" } });
  fireEvent.click(nav.getByRole("button", { name: "关于 OfferTrack" }));
  expect(screen.getByText(/Permission is hereby granted/)).toBeVisible();
  fireEvent.keyDown(window, { key: "F1" });
  expect(screen.getByRole("heading", { name: "快速开始" })).toBeVisible();
  expect(mocks.location).not.toHaveBeenCalled();
  const previous = { entries: ["a", "b", "c"], index: 1 };
  expect(readingHistory(previous, { chapter: "d" })).toEqual({
    entries: ["a", "b", "d"],
    index: 2,
  });
});

it("subscribes before cold-start target and ignores an older retained response", async () => {
  mocks.isTauri.mockReturnValue(true);
  let receive: ((event: { payload: HelpLocation }) => void) | undefined;
  const stop = vi.fn();
  mocks.listen.mockImplementation((_name, callback: typeof receive) => {
    receive = callback;
    return Promise.resolve(stop);
  });
  let complete: ((location: HelpLocation) => void) | undefined;
  mocks.location.mockImplementation(
    () =>
      new Promise<HelpLocation>((resolve) => {
        complete = resolve;
      }),
  );
  const ui = render(<HelpWindow />);
  await act(async () => {
    await Promise.resolve();
  });
  act(() => receive!({ payload: { topic: "backup", revision: 2 } }));
  await act(async () => {
    complete!({ topic: "about", revision: 1 });
    await Promise.resolve();
  });
  expect(
    screen.getByRole("heading", { name: "完整备份、独立恢复与迁移" }),
  ).toBeVisible();
  ui.unmount();
  expect(stop).toHaveBeenCalledTimes(1);
});

it("requires explicit diagnostic preview before copying; failed refresh discards it", async () => {
  mocks.diagnostics.mockResolvedValue({
    version: 1,
    application: "OfferTrack",
    warehouseAccess: "closed",
  });
  render(<Diagnostics />);
  expect(mocks.diagnostics).not.toHaveBeenCalled();
  expect(screen.getByRole("button", { name: "复制已预览诊断" })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "读取脱敏诊断" }));
  const preview = await screen.findByDisplayValue(
    /"warehouseAccess": "closed"/,
  );
  expect(mocks.copy).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "复制已预览诊断" }));
  await screen.findByText("已复制预览内容，请分享前再次检查。");
  expect(mocks.copy).toHaveBeenCalledWith(
    (preview as HTMLTextAreaElement).value,
  );
  mocks.diagnostics.mockRejectedValue(
    new Error("private path must not be rendered"),
  );
  fireEvent.click(screen.getByRole("button", { name: "读取脱敏诊断" }));
  await screen.findByText(/旧预览已清除/);
  expect(preview).toHaveValue("");
  expect(screen.queryByText(/private path/)).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "复制已预览诊断" })).toBeDisabled();
});

it("keeps preview on clipboard failure and honestly handles missing log directory", async () => {
  mocks.diagnostics.mockResolvedValue({ version: 1 });
  mocks.copy.mockRejectedValue(new Error("clipboard denied"));
  mocks.openLogs.mockResolvedValue(false);
  render(<Diagnostics />);
  fireEvent.click(screen.getByRole("button", { name: "读取脱敏诊断" }));
  const preview = await screen.findByDisplayValue(/"version": 1/);
  fireEvent.click(screen.getByRole("button", { name: "复制已预览诊断" }));
  await screen.findByText(/复制失败，可在预览中全选/);
  expect(preview).not.toHaveValue("");
  fireEvent.click(screen.getByRole("button", { name: "打开日志目录" }));
  await screen.findByText(/当前没有日志目录/);
  expect(mocks.openLogs).toHaveBeenCalledTimes(1);
});
