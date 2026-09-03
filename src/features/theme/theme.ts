import { createContext, useContext } from "react";

export type Theme = "light" | "dark";
export const themeKey = "offertrack.appearance.v1";
export function loadTheme(): { theme: Theme; error: string } {
  try {
    const saved = localStorage.getItem(themeKey);
    if (saved === null) return { theme: "light", error: "" };
    const value: unknown = JSON.parse(saved);
    if (
      typeof value === "object" &&
      value !== null &&
      "version" in value &&
      value.version === 1 &&
      "theme" in value &&
      (value.theme === "light" || value.theme === "dark")
    )
      return { theme: value.theme, error: "" };
    throw new Error("Invalid appearance preference");
  } catch {
    return {
      theme: "light",
      error: "外观偏好无法读取，暂用浅色；请选择主题重新保存。",
    };
  }
}
export function saveTheme(theme: Theme) {
  localStorage.setItem(themeKey, JSON.stringify({ version: 1, theme }));
}
export const ThemeContext = createContext<{
  theme: Theme;
  error: string;
  saved: boolean;
  change: (theme: Theme) => void;
} | null>(null);
export function useTheme() {
  const context = useContext(ThemeContext);
  if (!context) throw new Error("ThemeProvider is required");
  return context;
}
