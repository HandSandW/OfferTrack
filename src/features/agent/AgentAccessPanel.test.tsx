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
import { AgentAccessPanel } from "./AgentAccessPanel";

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
  it("publishes one fixed snapshot directory and does not expose force-generation", () => {
    const create = vi.spyOn(desktopApi, "createAgentSnapshot");
    show();
    expect(screen.getByText(/agent-access\/snapshot/)).toBeInTheDocument();
    expect(screen.getByText(/不保留新的历史代际/)).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "生成 Agent 只读快照" }),
    ).not.toBeInTheDocument();
    expect(create).not.toHaveBeenCalled();
  });

  it("explains that read-only warehouses can inspect but not refresh", () => {
    show(false);
    expect(
      screen.getByText(/只读仓库可检查已有快照或通过 CLI 查询/),
    ).toBeInTheDocument();
  });

  it("requires ordinary confirmation before clearing only recycled generations", async () => {
    vi.spyOn(desktopApi, "prepareAgentSnapshotRecycleBin").mockResolvedValue({
      confirmationToken: "bound-token",
      itemIds: ["one", "two"],
      skippedCount: 1,
    });
    const purge = vi
      .spyOn(desktopApi, "emptyAgentSnapshotRecycleBin")
      .mockResolvedValue({
        deletedIds: ["one", "two"],
        failed: [],
        skippedCount: 1,
      });
    show();
    fireEvent.click(
      screen.getByRole("button", { name: "清理旧预览版 Agent 快照…" }),
    );
    expect(await screen.findByText(/将永久删除.*2/)).toHaveTextContent(
      "私人投递信息；此操作无法恢复",
    );
    expect(purge).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(screen.getByRole("dialog", { hidden: true })).toHaveAttribute(
        "open",
      ),
    );
    fireEvent.click(screen.getByRole("button", { name: "是，永久清理" }));
    expect(
      await screen.findByText(/已永久清理 2 个旧快照/),
    ).toBeInTheDocument();
    expect(purge).toHaveBeenCalledWith("bound-token");
  });
});
