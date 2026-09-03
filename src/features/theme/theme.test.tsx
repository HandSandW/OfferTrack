import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { ThemeProvider } from "./ThemeProvider";
import { ThemeSettings } from "./ThemeSettings";
import { loadTheme, themeKey } from "./theme";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  localStorage.clear();
});
function mount() {
  return render(<ThemeSettings />, { wrapper: ThemeProvider });
}
it("defaults to light, persists dark locally and restores on remount without a warehouse", () => {
  const ui = mount();
  expect(document.documentElement.dataset.theme).toBe("light");
  fireEvent.change(screen.getByLabelText("主题"), {
    target: { value: "dark" },
  });
  expect(document.documentElement.dataset.theme).toBe("dark");
  expect(JSON.parse(localStorage.getItem(themeKey)!)).toEqual({
    version: 1,
    theme: "dark",
  });
  expect(screen.getByRole("status")).toHaveTextContent("主题已保存");
  ui.unmount();
  mount();
  expect(document.documentElement.dataset.theme).toBe("dark");
  expect(screen.getByLabelText("主题")).toHaveValue("dark");
});
it("preserves the previous theme and reports storage failure instead of false success", () => {
  mount();
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new Error("denied");
  });
  fireEvent.change(screen.getByLabelText("主题"), {
    target: { value: "dark" },
  });
  expect(screen.getByRole("alert")).toHaveTextContent("主题未保存");
  expect(document.documentElement.dataset.theme).toBe("light");
  expect(screen.getByLabelText("主题")).toHaveValue("light");
  expect(screen.queryByRole("status")).not.toBeInTheDocument();
});
it("handles invalid or unavailable preferences without overwriting them during reads", () => {
  localStorage.setItem(themeKey, JSON.stringify({ version: 2, theme: "dark" }));
  expect(loadTheme()).toMatchObject({ theme: "light" });
  expect(localStorage.getItem(themeKey)).toContain('"version":2');
  mount();
  expect(screen.getByRole("alert")).toHaveTextContent("无法读取");
  vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
    throw new Error("denied");
  });
  expect(loadTheme().error).not.toBe("");
});

it("updates an open help window when another webview changes the stored theme", () => {
  mount();
  localStorage.setItem(themeKey, JSON.stringify({ version: 1, theme: "dark" }));
  fireEvent(window, new StorageEvent("storage", { key: themeKey }));
  expect(document.documentElement.dataset.theme).toBe("dark");
  localStorage.clear();
  fireEvent(window, new StorageEvent("storage", { key: null }));
  expect(document.documentElement.dataset.theme).toBe("light");
});
