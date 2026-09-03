import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { desktopApi } from "../../lib/tauri";
import { DraftGuardProvider } from "../../shared/DraftGuardProvider";
import { AgentConnectionPanel } from "./AgentConnectionPanel";
import type { AgentConnection } from "./contracts";

const connection: AgentConnection = {
  version: 1,
  cliAvailable: true,
  configuration: {
    mcpServers: {
      offertrack: {
        command: "synthetic/offertrack-cli.exe",
        args: ["--warehouse", "synthetic/求职 & 仓库", "mcp"],
      },
    },
  },
  protocolVersions: ["2025-11-25", "2025-06-18"],
};
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});
function show(onError = vi.fn()) {
  return render(<AgentConnectionPanel onError={onError} />, {
    wrapper: DraftGuardProvider,
  });
}
it("loads only on request and copies exact structured config without changing client settings", async () => {
  const load = vi
    .spyOn(desktopApi, "getAgentConnection")
    .mockResolvedValue(connection);
  const copy = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: copy },
  });
  show();
  expect(load).not.toHaveBeenCalled();
  expect(screen.getByText(/联网 Agent 可能把查询结果/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "查看 MCP 连接配置" }));
  const textarea = await screen.findByRole("textbox", {
    name: "MCP 配置 JSON",
  });
  expect(textarea).toHaveValue(
    JSON.stringify(connection.configuration, null, 2),
  );
  expect(textarea).toHaveAttribute("readonly");
  fireEvent.click(screen.getByRole("button", { name: "复制 MCP 配置" }));
  expect(await screen.findByRole("status")).toHaveTextContent(
    "已复制连接配置。",
  );
  expect(copy).toHaveBeenCalledWith(
    JSON.stringify(connection.configuration, null, 2),
  );
  expect(load).toHaveBeenCalledOnce();
});
it("reports missing CLI and preserves selectable config when clipboard is denied", async () => {
  vi.spyOn(desktopApi, "getAgentConnection").mockResolvedValue({
    ...connection,
    cliAvailable: false,
  });
  const denied = new Error("clipboard denied");
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: vi.fn().mockRejectedValue(denied) },
  });
  const onError = vi.fn();
  show(onError);
  fireEvent.click(screen.getByRole("button", { name: "查看 MCP 连接配置" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("未检测到同目录");
  fireEvent.click(screen.getByRole("button", { name: "复制 MCP 配置" }));
  expect(
    await screen.findByText(/复制失败，可在下方文本框/),
  ).toBeInTheDocument();
  expect(screen.getByRole("textbox")).toHaveValue(
    JSON.stringify(connection.configuration, null, 2),
  );
  expect(onError).toHaveBeenCalledWith(denied);
});
it("prevents duplicate loading and clears stale configuration after a failed refresh", async () => {
  let resolve!: (value: AgentConnection) => void;
  const load = vi
    .spyOn(desktopApi, "getAgentConnection")
    .mockReturnValueOnce(
      new Promise((done) => {
        resolve = done;
      }),
    )
    .mockRejectedValueOnce(new Error("warehouse unavailable"));
  const onError = vi.fn();
  show(onError);
  fireEvent.click(screen.getByRole("button", { name: "查看 MCP 连接配置" }));
  expect(
    screen.getByRole("button", { name: "正在读取连接配置…" }),
  ).toBeDisabled();
  expect(load).toHaveBeenCalledOnce();
  resolve(connection);
  await screen.findByRole("textbox");
  fireEvent.click(screen.getByRole("button", { name: "查看 MCP 连接配置" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "无法读取当前连接配置",
  );
  expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
  await waitFor(() =>
    expect(
      screen.getByRole("button", { name: "查看 MCP 连接配置" }),
    ).toBeEnabled(),
  );
  expect(onError).toHaveBeenCalledOnce();
});
