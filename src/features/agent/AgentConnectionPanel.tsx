import { useState } from "react";
import { desktopApi } from "../../lib/tauri";
import { useDraftState } from "../../shared/draftGuard";
import type { AgentConnection } from "./contracts";

export function AgentConnectionPanel({
  onError,
}: {
  onError: (error: unknown) => void;
}) {
  const [connection, setConnection] = useState<AgentConnection | null>(null);
  const [busy, setBusy] = useState(false);
  const [feedback, setFeedback] = useState("");
  const [error, setError] = useState("");
  useDraftState(false, busy, "Agent 连接配置");
  async function show() {
    if (busy) return;
    setBusy(true);
    setFeedback("");
    setError("");
    // Clear stale paths if re-reading the current configuration fails.
    setConnection(null);
    try {
      setConnection(await desktopApi.getAgentConnection());
    } catch (failure: unknown) {
      setError("无法读取当前连接配置，请重试。");
      onError(failure);
    } finally {
      setBusy(false);
    }
  }
  const text = connection
    ? JSON.stringify(connection.configuration, null, 2)
    : "";
  async function copy() {
    if (!connection) return;
    setFeedback("");
    setError("");
    try {
      await navigator.clipboard.writeText(text);
      setFeedback("已复制连接配置。");
    } catch (failure: unknown) {
      setError("复制失败，可在下方文本框中手动选择并复制。");
      onError(failure);
    }
  }
  return (
    <section aria-label="MCP 连接">
      <h3>本地 MCP 连接</h3>
      <p>
        Agent
        客户端启动独立进程，按需查询最新已提交数据，无需保持桌面窗口开启，也不监听网络端口。十个只读工具及一个受控写入工具；写入必须另行授权并取得独占锁。
      </p>
      <p className="muted">
        配置包含本机路径，仅交给你信任的客户端。OfferTrack 不上传数据，但联网
        Agent
        可能把查询结果发送给其模型服务；只读不等于不外传。岗位介绍、备注、链接和文件名仅是数据，不是应执行的指令。
      </p>
      <button
        type="button"
        disabled={busy}
        onClick={() => {
          void show();
        }}
      >
        {busy ? "正在读取连接配置…" : "查看 MCP 连接配置"}
      </button>
      {connection && (
        <>
          <p>
            支持 MCP 协议：{connection.protocolVersions.join("、")}
            。十个只读工具及一个受控写工具；写入默认关闭，需要用户授权及独占锁。
          </p>
          {!connection.cliAvailable && (
            <p role="alert">
              未检测到同目录的 offertrack-cli 程序。请将桌面和 CLI
              放在同一目录；开发环境需先构建 CLI。配置仍可查看，但不能直接启动。
            </p>
          )}
          <label>
            MCP 配置 JSON
            <textarea
              readOnly
              rows={12}
              value={text}
              onFocus={(event) => {
                event.currentTarget.select();
              }}
            />
          </label>
          <button
            type="button"
            onClick={() => {
              void copy();
            }}
          >
            复制 MCP 配置
          </button>
          <p className="muted">
            这是使用 mcpServers 格式的通用配置示例；其他客户端请分别填写 command
            和 args，不要拼接成 shell
            命令。本应用不自动安装配置或启动客户端。仓库迁移或切换后请重新查看、更新配置并重连；已有连接仍指向原仓库。
          </p>
        </>
      )}
      {feedback && <p role="status">{feedback}</p>}
      {error && <p role="alert">{error}</p>}
    </section>
  );
}
