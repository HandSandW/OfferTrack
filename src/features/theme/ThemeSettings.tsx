import { useTheme } from "./theme";

export function ThemeSettings() {
  const { theme, error, saved, change } = useTheme();
  return (
    <section className="panel-page">
      <h2>外观</h2>
      <p className="muted">
        默认浅色；保存在本机 WebView2
        应用偏好中，重启后保留。切换仓库不改变主题，只读或未打开仓库时也可设置。不随数据备份迁移。
      </p>
      <label>
        主题
        <select
          aria-label="主题"
          value={theme}
          onChange={(e) => change(e.target.value === "dark" ? "dark" : "light")}
        >
          <option value="light">浅色</option>
          <option value="dark">深色</option>
        </select>
      </label>
      {error && <p role="alert">{error}</p>}
      {saved && <p role="status">主题已保存，下次启动继续使用。</p>}
    </section>
  );
}
