import { useState } from "react";
import { helpApi } from "./api";

export function Diagnostics() {
  const [preview, setPreview] = useState("");
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  const read = async () => {
    setBusy(true);
    setPreview("");
    setMessage("");
    try {
      setPreview(JSON.stringify(await helpApi.diagnostics(), null, 2));
    } catch {
      setMessage("诊断读取失败，旧预览已清除。请重试。");
    } finally {
      setBusy(false);
    }
  };
  const copy = async () => {
    if (!preview || busy) return;
    setBusy(true);
    setMessage("");
    try {
      await navigator.clipboard.writeText(preview);
      setMessage("已复制预览内容，请分享前再次检查。");
    } catch {
      setMessage("复制失败，可在预览中全选并手动复制。");
    } finally {
      setBusy(false);
    }
  };
  const logs = async () => {
    setBusy(true);
    setMessage("");
    try {
      setMessage(
        (await helpApi.openLogs())
          ? "已请求打开 OfferTrack 日志目录。"
          : "当前没有日志目录。本版本没有持久应用日志，不会为此创建文件。",
      );
    } catch {
      setMessage(
        "日志目录无法安全打开，请检查目录类型或访问权限；未创建或删除文件。",
      );
    } finally {
      setBusy(false);
    }
  };
  return (
    <section aria-label="脱敏诊断预览" className="help-diagnostics">
      <div className="help-actions">
        <button type="button" disabled={busy} onClick={() => void read()}>
          读取脱敏诊断
        </button>
        <button
          type="button"
          disabled={busy || !preview}
          onClick={() => void copy()}
        >
          复制已预览诊断
        </button>
        <button type="button" disabled={busy} onClick={() => void logs()}>
          打开日志目录
        </button>
      </div>
      <label>
        诊断预览（不含求职资料和本机路径）
        <textarea readOnly rows={13} value={preview} />
      </label>
      {message && <p role="status">{message}</p>}
    </section>
  );
}
