import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { desktopApi } from "../lib/tauri";
import { UrlLink } from "./UrlLink";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});
it("preserves normal clicks and supports detected browsers, copy and keyboard dismissal", async () => {
  const open = vi.spyOn(desktopApi, "openWebUrl").mockResolvedValue();
  vi.spyOn(desktopApi, "availableBrowsers").mockResolvedValue(["chrome"]);
  const copy = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: copy },
  });
  const select = vi.fn();
  render(
    <div onClick={select}>
      <UrlLink value="https://example.com/jobs" onError={vi.fn()} />
    </div>,
  );
  const link = screen.getByRole("button");
  fireEvent.click(link);
  expect(select).toHaveBeenCalledTimes(1);
  expect(open).not.toHaveBeenCalled();
  fireEvent.click(link, { ctrlKey: true });
  expect(open).toHaveBeenCalledWith("https://example.com/jobs");
  fireEvent.contextMenu(link, { clientX: 20, clientY: 30 });
  fireEvent.click(
    await screen.findByRole("menuitem", { name: "使用 Chrome 打开" }),
  );
  expect(open).toHaveBeenLastCalledWith("https://example.com/jobs", "chrome");
  expect(select).toHaveBeenCalledTimes(1);
  fireEvent.contextMenu(link);
  fireEvent.click(screen.getByRole("menuitem", { name: "复制链接" }));
  expect(copy).toHaveBeenCalledWith("https://example.com/jobs");
  fireEvent.contextMenu(link);
  await screen.findByRole("menuitem", { name: "使用 Chrome 打开" });
  expect(
    screen.queryByRole("menuitem", { name: "使用 Edge 打开" }),
  ).not.toBeInTheDocument();
  fireEvent.keyDown(screen.getByRole("menu"), { key: "End" });
  expect(
    screen.getByRole("menuitem", { name: "使用 Chrome 打开" }),
  ).toHaveFocus();
  fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
  expect(screen.queryByRole("menu")).not.toBeInTheDocument();
});
it("keeps fallback actions when detection fails and reports launch errors", async () => {
  vi.spyOn(desktopApi, "availableBrowsers").mockRejectedValue(
    new Error("detection"),
  );
  vi.spyOn(desktopApi, "openWebUrl").mockRejectedValue(new Error("打开失败"));
  const error = vi.fn();
  render(<UrlLink value="https://example.com" onError={error} />);
  fireEvent.contextMenu(screen.getByRole("button"));
  await screen.findByText(/浏览器检测失败/);
  fireEvent.click(screen.getByRole("menuitem", { name: "使用默认浏览器打开" }));
  await waitFor(() =>
    expect(error).toHaveBeenCalledWith(
      expect.objectContaining({ message: "打开失败" }),
    ),
  );
});
