import { useContext, type ReactNode } from "react";
import type { WarehouseSummary } from "../../contracts";
import { formatLocalDateTime } from "../../shared/dateTime";
import { SnapshotContext, useSnapshot } from "./useSnapshot";

export function AgentSnapshotProvider({
  warehouse,
  enabled,
  children,
}: {
  warehouse: WarehouseSummary | null;
  enabled: boolean;
  children: ReactNode;
}) {
  const value = useSnapshot(warehouse, enabled);
  return (
    <SnapshotContext.Provider value={value}>
      {children}
    </SnapshotContext.Provider>
  );
}

export function SnapshotNotice() {
  const value = useContext(SnapshotContext);
  if (
    !value ||
    (!value.error && !value.report?.error && value.report?.state !== "error")
  )
    return null;
  return (
    <div className="notice warning" role="status">
      Agent 快照暂未同步；已保存的业务修改不受影响。请在设置检查快照，或用
      CLI/MCP 读取当前数据。
    </div>
  );
}

export function SnapshotStatus() {
  const value = useContext(SnapshotContext);
  if (!value) return null;
  const { report, error, pending, refresh } = value;
  return (
    <section aria-label="快照新鲜度">
      <h3>快照新鲜度</h3>
      <p role="status">
        {pending
          ? "等待检查 / 正在检查，尚不能确认最新。"
          : error
            ? "检查失败，保留上次结果。"
            : report?.state === "current"
              ? "检查时与当前索引数据一致，文件校验通过。"
              : report?.state === "stale"
                ? "快照已过期或不可用；只读模式不自动生成。"
                : report?.state === "missing"
                  ? "尚无可追踪快照；只读模式不自动生成。"
                  : "快照同步失败，业务数据不受影响。"}
      </p>
      {(error || report?.error) && (
        <p role="alert">{error || report?.error?.message}</p>
      )}
      {report?.published && report.state === "error" && (
        <p>
          文件已发布但检查点未保存，本次监测已暂停自动重试。请排除错误后主动检查，或重新连接仓库。
        </p>
      )}
      {report && <p>上次检查：{formatLocalDateTime(report.checked_at_utc)}</p>}
      {report?.snapshot && (
        <>
          <p>提供给 Agent 的固定快照目录：</p>
          <p className="path-label">agent-access/snapshot</p>
          <p>
            已记录的快照生成时间：
            {formatLocalDateTime(report.snapshot.generated_at_utc)}
          </p>
          <p>
            共 {report.snapshot.application_count}{" "}
            条投递。路径相对于当前仓库根；请结合上述新鲜度使用，不能仅凭目录时间判断。
          </p>
        </>
      )}
      {report?.warnings.map((warning) => (
        <p key={warning}>{warning}</p>
      ))}
      <button disabled={pending} onClick={refresh}>
        检查并按需刷新快照
      </button>
    </section>
  );
}
