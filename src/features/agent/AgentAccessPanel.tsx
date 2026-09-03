import { useState } from "react";
import { desktopApi } from "../../lib/tauri";
import { useDraftState } from "../../shared/draftGuard";
import type { AgentSnapshot } from "./contracts";
import { AgentConnectionPanel } from "./AgentConnectionPanel";
import { AgentWritePanel } from "./AgentWritePanel";
import { SnapshotStatus } from "./AgentSnapshotProvider";

export function AgentAccessPanel({
  writable,
  onError,
}: {
  writable: boolean;
  onError: (error: unknown) => void;
}) {
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<AgentSnapshot | null>(null);
  const [error, setError] = useState("");
  useDraftState(false, busy, "Agent 快照生成");
  async function generate() {
    if (busy) return;
    setBusy(true);
    setError("");
    try {
      setResult(await desktopApi.createAgentSnapshot());
    } catch (failure: unknown) {
      setError(
        failure instanceof Error
          ? failure.message
          : "快照生成失败，请重试。失败暂存保留。",
      );
      onError(failure);
    } finally {
      setBusy(false);
    }
  }
  return (
    <article className="panel-page">
      <h2>本地 Agent 访问</h2>
      <p>
        默认只读。快照包含活跃及已归档投递的完整长文本、流程历史、面试轮次、待办、事件、自定义字段和简历相对路径，不读取简历正文。
      </p>
      <p className="muted">
        连接仓库后每分钟、窗口回到前台和编辑后自动检查；可写模式按内容变化生成新代，只读模式仅检查。分析前可主动检查，或使用
        offertrack-cli.exe 查询当前数据，也可通过下方 MCP
        连接查询。只读是接口约束，不限制其他程序的系统文件权限。受控写入需在下方明确开启。
      </p>
      <p className="muted">
        每次生成一个完整目录，旧快照及失败暂存均保留。旧快照可能仍含后来删除的资料；请按私人数据保管，不上传
        GitHub。快照不是备份，不能代替数据库或完整备份。
      </p>
      <SnapshotStatus />
      <button
        type="button"
        disabled={!writable || busy}
        onClick={() => {
          void generate();
        }}
      >
        {busy ? "正在生成 Agent 快照…" : "生成 Agent 只读快照"}
      </button>
      {!writable && (
        <p className="muted">
          只读仓库可通过 CLI 查询；请以写入方式打开后再生成仓库内快照。
        </p>
      )}
      {error && <p role="alert">{error}</p>}
      {result && (
        <div role="status">
          <p>
            已生成 {result.applicationCount} 条投递的快照（版本 {result.version}
            ），时间：{result.generatedAtUtc}。
          </p>
          <p className="path-label">{result.path}</p>
          <p>
            请让 Agent 阅读此目录中的 README.md 和 manifest.json。根目录已有的
            AGENTS.md 不会被覆盖。
          </p>
          {result.warnings.map((warning) => (
            <p key={warning}>{warning}</p>
          ))}
        </div>
      )}
      <AgentConnectionPanel onError={onError} />
      <AgentWritePanel writable={writable} onError={onError} />
    </article>
  );
}
