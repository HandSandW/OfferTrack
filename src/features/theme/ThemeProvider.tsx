import { useEffect, useLayoutEffect, useState, type ReactNode } from "react";
import {
  loadTheme,
  saveTheme,
  themeKey,
  ThemeContext,
  type Theme,
} from "./theme";

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState(loadTheme);
  const [saved, setSaved] = useState(false);
  useEffect(() => {
    const changed = (event: StorageEvent) => {
      if (event.key === themeKey || event.key === null) {
        setState(loadTheme());
        setSaved(false);
      }
    };
    window.addEventListener("storage", changed);
    return () => window.removeEventListener("storage", changed);
  }, []);
  useLayoutEffect(() => {
    document.documentElement.dataset.theme = state.theme;
    return () => {
      delete document.documentElement.dataset.theme;
    };
  }, [state.theme]);
  const change = (theme: Theme) => {
    try {
      saveTheme(theme);
      setState({ theme, error: "" });
      setSaved(true);
    } catch {
      setState((previous) => ({
        ...previous,
        error: "主题未保存：本机偏好存储不可用，已保留原主题，请重试。",
      }));
      setSaved(false);
    }
  };
  return (
    <ThemeContext.Provider value={{ ...state, saved, change }}>
      {children}
    </ThemeContext.Provider>
  );
}
