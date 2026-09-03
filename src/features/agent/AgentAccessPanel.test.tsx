import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { desktopApi } from "../../lib/tauri";
import { DraftGuardProvider } from "../../shared/DraftGuardProvider";
import { useDraftGuard } from "../../shared/draftGuard";
import { AgentAccessPanel } from "./AgentAccessPanel";
import type { AgentSnapshot } from "./contracts";

const result: AgentSnapshot = {
  version: 1,
  path: "synthetic-snapshot-path",
  generatedAtUtc: "2026-09-03T00:00:00Z",
  applicationCount: 2,
  rootInstructionsCreated: false,
  warnings: ["已保留用户说明"],
};
const onError = vi.fn();
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  onError.mockClear();
});
function show(writable = true) {
  return render(<AgentAccessPanel writable={writable} onError={onError} />, {
    wrapper: DraftGuardProvider,
  });
}
describe("Agent read-only snapshots", () => {
  it("requires explicit generation and exposes timestamp, complete path and preservation warnings", async () => {
    const create = vi
      .spyOn(desktopApi, "createAgentSnapshot")
      .mockResolvedValue(result);
    show();
    expect(create).not.toHaveBeenCalled();
    fireEvent.click(
      screen.getByRole("button", { name: "生成 Agent 只读快照" }),
    );
    expect(await screen.findByText(result.path)).toBeInTheDocument();
    expect(screen.getByText(/已生成 2 条投递/)).toHaveTextContent(
      result.generatedAtUtc,
    );
    expect(screen.getByText("已保留用户说明")).toBeInTheDocument();
    expect(create).toHaveBeenCalledOnce();
  });
  it("does not enable snapshot writes in read-only warehouse mode", () => {
    const create = vi.spyOn(desktopApi, "createAgentSnapshot");
    show(false);
    expect(
      screen.getByRole("button", { name: "生成 Agent 只读快照" }),
    ).toBeDisabled();
    expect(create).not.toHaveBeenCalled();
    expect(screen.getByText(/只读仓库可通过 CLI 查询/)).toBeInTheDocument();
  });
  it("preserves prior successful result after a later generation fails and permits retry", async () => {
    const create = vi
      .spyOn(desktopApi, "createAgentSnapshot")
      .mockResolvedValueOnce(result)
      .mockRejectedValueOnce(new Error("路径被占用"));
    show();
    fireEvent.click(
      screen.getByRole("button", { name: "生成 Agent 只读快照" }),
    );
    await screen.findByText(result.path);
    fireEvent.click(
      screen.getByRole("button", { name: "生成 Agent 只读快照" }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent("路径被占用");
    expect(screen.getByText(result.path)).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "生成 Agent 只读快照" }),
    ).toBeEnabled();
    expect(create).toHaveBeenCalledTimes(2);
    expect(onError).toHaveBeenCalledOnce();
  });
  it("blocks duplicate generation and leaving during publication", async () => {
    let finish!: (value: AgentSnapshot) => void;
    const create = vi.spyOn(desktopApi, "createAgentSnapshot").mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    const leave = vi.fn();
    function Harness() {
      const { confirmLeave } = useDraftGuard();
      return (
        <>
          <AgentAccessPanel writable onError={onError} />
          <button
            onClick={() => {
              void confirmLeave().then(leave);
            }}
          >
            离开
          </button>
        </>
      );
    }
    render(<Harness />, { wrapper: DraftGuardProvider });
    fireEvent.click(
      screen.getByRole("button", { name: "生成 Agent 只读快照" }),
    );
    expect(
      screen.getByRole("button", { name: "正在生成 Agent 快照…" }),
    ).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "离开" }));
    fireEvent.click(await screen.findByRole("button", { name: "知道了" }));
    await waitFor(() => expect(leave).toHaveBeenCalledWith(false));
    expect(create).toHaveBeenCalledOnce();
    finish(result);
    await screen.findByText(result.path);
  });
});
