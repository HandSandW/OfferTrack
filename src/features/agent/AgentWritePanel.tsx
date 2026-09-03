import { useState } from "react";
import { desktopApi } from "../../lib/tauri";
import { useDraftGuard, useDraftState } from "../../shared/draftGuard";
import type { AgentAuditItem, AgentPermission } from "./contracts";

export function AgentWritePanel({
  writable,
  onError,
}: {
  writable: boolean;
  onError: (error: unknown) => void;
}) {
  const [permission, setPermission] = useState<AgentPermission | null>(null);
  const [audit, setAudit] = useState<AgentAuditItem[] | null>(null);
  const [detail, setDetail] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const { confirm } = useDraftGuard();
  useDraftState(false, busy, "Agent 写入设置与审计");
  async function run(action: () => Promise<void>) {
    if (busy) return;
    setBusy(true);
    setError("");
    try {
      await action();
    } catch (failure: unknown) {
      setError(
        failure instanceof Error
          ? failure.message
          : "操作未完成，请刷新后重试。",
      );
      onError(failure);
    } finally {
      setBusy(false);
    }
  }
  async function toggle() {
    if (!permission || !writable) return;
    const enabled = !permission.enabled;
    if (
      enabled &&
      !(await confirm({
        title: "长期开启 Agent 写入？",
        message:
          "将允许官方 CLI/MCP 修改元数据并创建待办、事件，直到你主动关闭。每批先备份并保存含旧值/新值的私人审计，不包含简历正文备份。只有信任的 Agent 才应接入。",
        confirmLabel: "开启写入",
      }))
    )
      return;
    setPermission(
      await desktopApi.setAgentPermission(enabled, permission.revision),
    );
  }
  return (
    <section className="panel-page">
      <h3>受控写入与审计</h3>
      <p>
        默认关闭，当前仓库长期保存，只有用户能开启。每批修改前备份数据库，失败整批回滚；不允许删除文件、清空回收站或执行
        SQL/命令。
      </p>
      <p className="muted">
        Agent
        写入前请关闭桌面的当前仓库，或改为只读打开，以释放独占写锁。写入后重新打开/刷新页面；系统另行尝试更新文件快照，快照错误不撤销已提交修改。修改公司/岗位后可在文件页重试目录规范化，Agent
        不移动附件。
      </p>
      <button
        type="button"
        disabled={busy}
        onClick={() => {
          void run(async () => {
            setPermission(null);
            setPermission(await desktopApi.getAgentPermission());
          });
        }}
      >
        读取写入权限
      </button>
      {permission && (
        <>
          <p role="status">
            当前 Agent 写入：
            {permission.enabled ? "已开启（长期有效）" : "已关闭"}
          </p>
          <button
            type="button"
            disabled={!writable || busy}
            onClick={() => {
              void run(toggle);
            }}
          >
            {permission.enabled ? "关闭 Agent 写入" : "开启 Agent 写入…"}
          </button>
        </>
      )}
      {!writable && <p>只读会话不能更改权限，可查看审计。</p>}
      <button
        type="button"
        disabled={busy}
        onClick={() => {
          void run(async () => {
            setDetail("");
            setAudit(await desktopApi.listAgentAudit());
          });
        }}
      >
        查看最近 50 条审计
      </button>
      {audit && (
        <>
          <p className="muted">
            仅按需读取。本地审计含修改前后私人内容，随数据库备份；不上传
            GitHub。失败请求不提交业务审计，成功请求 ID 用于安全重试。
          </p>
          <ul>
            {audit.map((item) => (
              <li key={item.id}>
                {item.occurred_at_utc} ·{" "}
                {item.operation === "permission" ? "权限设置" : "Agent 写入"} ·{" "}
                {item.id}{" "}
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => {
                    void run(async () => {
                      setDetail("");
                      setDetail(
                        JSON.stringify(
                          await desktopApi.getAgentAudit(item.id),
                          null,
                          2,
                        ),
                      );
                    });
                  }}
                >
                  查看变更 {item.id}
                </button>
              </li>
            ))}
          </ul>
          {audit.length === 0 && <p>尚无审计记录。</p>}
        </>
      )}
      {detail && (
        <label>
          审计详情（含私人内容）
          <textarea readOnly rows={14} value={detail} />
        </label>
      )}
      {error && <p role="alert">{error}</p>}
    </section>
  );
}
