import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { desktopApi } from "../../lib/tauri";
import { DraftGuardProvider } from "../../shared/DraftGuardProvider";
import { AgentWritePanel } from "./AgentWritePanel";
const off = { version: 1, enabled: false, revision: 0 };
const on = { version: 1, enabled: true, revision: 1 };
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});
function show(writable = true) {
  return render(<AgentWritePanel writable={writable} onError={vi.fn()} />, {
    wrapper: DraftGuardProvider,
  });
}
it("loads permission explicitly and only enables persistently after confirmation; cancellation does not write", async () => {
  const get = vi.spyOn(desktopApi, "getAgentPermission").mockResolvedValue(off);
  const set = vi.spyOn(desktopApi, "setAgentPermission").mockResolvedValue(on);
  show();
  expect(get).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "读取写入权限" }));
  await screen.findByText("当前 Agent 写入：已关闭");
  fireEvent.click(screen.getByRole("button", { name: "开启 Agent 写入…" }));
  let dialog = await screen.findByRole("dialog");
  fireEvent.click(within(dialog).getByRole("button", { name: "取消" }));
  await waitFor(() =>
    expect(
      screen.getByRole("button", { name: "开启 Agent 写入…" }),
    ).toBeEnabled(),
  );
  expect(set).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "开启 Agent 写入…" }));
  dialog = await screen.findByRole("dialog");
  fireEvent.click(within(dialog).getByRole("button", { name: "开启写入" }));
  await screen.findByText("当前 Agent 写入：已开启（长期有效）");
  expect(set).toHaveBeenCalledWith(true, 0);
  set.mockResolvedValue({ ...off, revision: 2 });
  fireEvent.click(screen.getByRole("button", { name: "关闭 Agent 写入" }));
  await screen.findByText("当前 Agent 写入：已关闭");
  expect(set).toHaveBeenLastCalledWith(false, 1);
});
it("shows read-only permission without enabling a setter and loads sensitive audit details only on demand", async () => {
  vi.spyOn(desktopApi, "getAgentPermission").mockResolvedValue(on);
  const set = vi.spyOn(desktopApi, "setAgentPermission");
  const list = vi.spyOn(desktopApi, "listAgentAudit").mockResolvedValue([
    {
      id: "synthetic-request",
      operation: "write",
      occurred_at_utc: "2026-09-03T00:00:00Z",
      outcome: "committed",
    },
  ]);
  const detail = vi.spyOn(desktopApi, "getAgentAudit").mockResolvedValue({
    before: { notes: "原备注" },
    after: { notes: "新备注" },
  });
  show(false);
  expect(list).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "读取写入权限" }));
  await screen.findByText("当前 Agent 写入：已开启（长期有效）");
  expect(
    screen.getByRole("button", { name: "关闭 Agent 写入" }),
  ).toBeDisabled();
  expect(set).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "查看最近 50 条审计" }));
  const button = await screen.findByRole("button", {
    name: "查看变更 synthetic-request",
  });
  expect(detail).not.toHaveBeenCalled();
  fireEvent.click(button);
  expect(
    await screen.findByRole("textbox", { name: "审计详情（含私人内容）" }),
  ).toHaveValue(
    JSON.stringify(
      { before: { notes: "原备注" }, after: { notes: "新备注" } },
      null,
      2,
    ),
  );
  expect(detail).toHaveBeenCalledWith("synthetic-request");
});
it("preserves known permission on save failure, blocks duplicate actions, and clears stale permission on failed reload", async () => {
  const get = vi.spyOn(desktopApi, "getAgentPermission").mockResolvedValue(on);
  let fail!: (reason: unknown) => void;
  const set = vi.spyOn(desktopApi, "setAgentPermission").mockReturnValue(
    new Promise((_resolve, reject) => {
      fail = reject;
    }),
  );
  show();
  fireEvent.click(screen.getByRole("button", { name: "读取写入权限" }));
  await screen.findByText("当前 Agent 写入：已开启（长期有效）");
  fireEvent.click(screen.getByRole("button", { name: "关闭 Agent 写入" }));
  expect(screen.getByRole("button", { name: "读取写入权限" })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "关闭 Agent 写入" }));
  expect(set).toHaveBeenCalledOnce();
  fail(new Error("版本冲突"));
  await screen.findByText("版本冲突");
  expect(
    screen.getByText("当前 Agent 写入：已开启（长期有效）"),
  ).toBeInTheDocument();
  get.mockRejectedValue(new Error("读取失败"));
  fireEvent.click(screen.getByRole("button", { name: "读取写入权限" }));
  await screen.findByText("读取失败");
  expect(
    screen.queryByRole("button", { name: "关闭 Agent 写入" }),
  ).not.toBeInTheDocument();
});
