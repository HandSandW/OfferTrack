import { useState } from "react";
import type { RecoveryDiagnostics } from "../../contracts";
import { desktopApi } from "../../lib/tauri";
import { pathStateText } from "./fileStatus";

const labels: Record<string, string> = {
  creation: "未完成的新建/复制",
  normalize: "目录规范化",
  trash: "移入回收站",
  restore: "从回收站恢复",
};
export function RecoveryDiagnosticsPanel({
  onError,
}: {
  onError: (error: unknown) => void;
}) {
  const [report, setReport] = useState<RecoveryDiagnostics | null>(null);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState("");
  const inspect = async () => {
    setBusy(true);
    setFailure("");
    try {
      setReport(await desktopApi.getRecoveryDiagnostics());
    } catch (error) {
      setFailure(
        error instanceof Error ? error.message : "诊断读取失败，请重试",
      );
      onError(error);
    } finally {
      setBusy(false);
    }
  };
  return (
    <article className="panel-page">
      <h2>未完成文件操作诊断</h2>
      <p className="muted">
        只读取当前仓库日志和目录状态，不修复、不移动、不清空。目录身份和附件完整性未在此核验。请先关闭相关程序；持续异常时保留仓库副本，不要手动删除
        .copying- 临时目录。
      </p>
      <button disabled={busy} onClick={() => void inspect()}>
        读取只读诊断
      </button>
      {busy && <p role="status">正在读取诊断…</p>}
      {failure && <p role="alert">{failure} 上次诊断可能已过期，请重试。</p>}
      {report && (
        <>
          <p role="status">
            待处理日志 {report.totalPending} 项。
            {report.items.length < report.totalPending &&
              `仅展示 ${report.items.length} 项。`}
          </p>
          {!report.totalPending && (
            <p>没有待处理日志；这不代表全部文件健康，也不是备份验证。</p>
          )}
          <div className="simple-list">
            {report.items.map((item) => (
              <article key={`${item.kind}:${item.id}`}>
                <div>
                  <strong>{labels[item.kind] ?? "未知操作"}</strong>
                  <span>操作 ID：{item.id}</span>
                  <span>
                    来源：{item.source.relativePath ?? "路径已隐藏"} ·{" "}
                    {pathStateText[item.source.state]}
                  </span>
                  <span>
                    目标：{item.target.relativePath ?? "路径已隐藏"} ·{" "}
                    {pathStateText[item.target.state]}
                  </span>
                  {item.identityRecorded !== null && (
                    <span>
                      {item.identityRecorded
                        ? "目录身份已记录，尚未核验是否匹配。"
                        : "没有已记录的目录身份，不能据此安全清理。"}
                    </span>
                  )}
                </div>
              </article>
            ))}
          </div>
        </>
      )}
    </article>
  );
}
