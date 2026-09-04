import { useState } from "react";
import type {
  BackupTrashChallenge,
  BackupTrashResult,
} from "../backup/contracts";
import { desktopApi } from "../../lib/tauri";
import { useDraftState } from "../../shared/draftGuard";
import { Modal } from "../../shared/Modal";
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
  const [challenge, setChallenge] = useState<BackupTrashChallenge | null>(null);
  const [cleanupResult, setCleanupResult] = useState<BackupTrashResult | null>(
    null,
  );
  const [cleanupMessage, setCleanupMessage] = useState("");
  useDraftState(false, busy, "Agent 快照回收站清理");

  async function prepareCleanup() {
    if (busy) return;
    setBusy(true);
    setCleanupMessage("");
    setCleanupResult(null);
    try {
      const next = await desktopApi.prepareAgentSnapshotRecycleBin();
      if (!next.itemIds.length) {
        setCleanupMessage(
          next.skippedCount
            ? `没有可安全清理的旧快照；跳过 ${next.skippedCount} 个未知或不安全项目。`
            : "Agent 快照回收区为空。",
        );
      } else {
        setChallenge(next);
      }
    } catch (error) {
      onError(error);
    } finally {
      setBusy(false);
    }
  }

  async function confirmCleanup() {
    if (!challenge || busy) return;
    setBusy(true);
    try {
      const result = await desktopApi.emptyAgentSnapshotRecycleBin(
        challenge.confirmationToken,
      );
      setCleanupResult(result);
      setChallenge(null);
    } catch (error) {
      onError(error);
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
        连接仓库后每分钟、窗口回到前台和编辑后自动检查；可写模式只在内容变化时更新固定快照，只读模式仅检查。分析前可主动检查，或使用
        offertrack-cli.exe 查询当前数据，也可通过下方 MCP
        连接查询。只读是接口约束，不限制其他程序的系统文件权限。受控写入需在下方明确开启。
      </p>
      <p className="muted">
        Agent 直接读取固定目录
        agent-access/snapshot。内容变化时覆写这一份派生快照，不保留新的历史代际；manifest.json
        最后提交并列出全部文件哈希。旧预览版代际迁移到固定回收区后可在此清理，仍应按私人数据保管，不上传
        GitHub。快照不是备份，不能代替数据库或完整备份。
      </p>
      <SnapshotStatus />
      <button
        type="button"
        disabled={!writable || busy}
        onClick={() => void prepareCleanup()}
      >
        清理旧预览版 Agent 快照…
      </button>
      {!writable && (
        <p className="muted">
          只读仓库可检查已有快照或通过 CLI 查询；刷新快照需要以写入方式打开。
        </p>
      )}
      {cleanupMessage && <p role="status">{cleanupMessage}</p>}
      {cleanupResult && (
        <div role="status">
          已永久清理 {cleanupResult.deletedIds.length} 个旧快照；失败{" "}
          {cleanupResult.failed.length}
          个，跳过 {cleanupResult.skippedCount} 个。
          {cleanupResult.failed.map((item) => (
            <p key={item.id} role="alert">
              {item.id}：{item.error.message}
            </p>
          ))}
        </div>
      )}
      <AgentConnectionPanel onError={onError} />
      <AgentWritePanel writable={writable} onError={onError} />
      {challenge && (
        <Modal
          title="永久清理旧预览版 Agent 快照"
          onCancel={() => {
            if (!busy) setChallenge(null);
          }}
        >
          <p>
            将永久删除 Agent 快照回收区中的 {challenge.itemIds.length}
            个旧预览版代际。它们可能包含后来已修改或删除的私人投递信息；此操作无法恢复。
          </p>
          {challenge.skippedCount > 0 && (
            <p>另有 {challenge.skippedCount} 个未知或不安全项目会保留。</p>
          )}
          <div className="modal-actions">
            <button disabled={busy} onClick={() => setChallenge(null)}>
              取消
            </button>
            <button
              className="danger"
              disabled={busy}
              onClick={() => void confirmCleanup()}
            >
              {busy ? "正在清理…" : "是，永久清理"}
            </button>
          </div>
        </Modal>
      )}
    </article>
  );
}
