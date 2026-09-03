import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { desktopApi, OfferTrackError } from "../../lib/tauri";
import type { WarehouseSummary } from "../../contracts";
import type { SnapshotReport } from "./contracts";
import {
  AgentSnapshotProvider,
  SnapshotNotice,
  SnapshotStatus,
} from "./AgentSnapshotProvider";

const warehouse: WarehouseSummary = {
  warehouseId: "test-warehouse",
  displayPath: "synthetic-directory",
  formatVersion: 1,
  accessMode: "write",
  warnings: [],
};
function report(overrides: Partial<SnapshotReport> = {}): SnapshotReport {
  return {
    version: 1,
    warehouse_id: warehouse.warehouseId,
    checked_at_utc: "2026-09-03T01:00:00Z",
    state: "current",
    snapshot: {
      relative_path: "agent-access/snapshot-synthetic",
      generated_at_utc: "2026-09-03T00:00:00Z",
      application_count: 2,
      content_sha256: "a".repeat(64),
    },
    published: false,
    error: null,
    warnings: [],
    ...overrides,
  };
}
function Harness({
  value = warehouse,
  enabled = true,
}: {
  value?: WarehouseSummary;
  enabled?: boolean;
}) {
  return (
    <AgentSnapshotProvider
      key={`${value.warehouseId}:${value.displayPath}`}
      warehouse={value}
      enabled={enabled}
    >
      <SnapshotNotice />
      <SnapshotStatus />
    </AgentSnapshotProvider>
  );
}
async function advance(ms = 1000) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });
}
beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("automatic Agent snapshots", () => {
  it("coalesces edits, checks on minute/focus and stops when disconnected", async () => {
    const check = vi
      .spyOn(desktopApi, "checkAgentSnapshot")
      .mockResolvedValue(report());
    const view = render(<Harness />);
    expect(check).not.toHaveBeenCalled();
    await advance();
    expect(check).toHaveBeenCalledExactlyOnceWith(
      warehouse.warehouseId,
      warehouse.displayPath,
    );
    expect(screen.getByText(/文件校验通过/)).toBeInTheDocument();
    expect(screen.getByText(/上次检查/)).toHaveTextContent(
      "2026-09-03T01:00:00Z",
    );
    expect(screen.getByText(/已记录的快照生成时间/)).toHaveTextContent(
      "2026-09-03T00:00:00Z",
    );
    fireEvent(window, new Event("offertrack-snapshot-dirty"));
    fireEvent(window, new Event("offertrack-snapshot-dirty"));
    expect(screen.getByText(/尚不能确认最新/)).toBeInTheDocument();
    await advance();
    expect(check).toHaveBeenCalledTimes(2);
    await advance(59000);
    expect(check).toHaveBeenCalledTimes(3);
    fireEvent.focus(window);
    await advance();
    expect(check).toHaveBeenCalledTimes(4);
    view.rerender(<Harness enabled={false} />);
    fireEvent(window, new Event("offertrack-snapshot-dirty"));
    await advance(120000);
    expect(check).toHaveBeenCalledTimes(4);
  });

  it("keeps one in-flight check and queues edits arriving during it", async () => {
    let finish!: (value: SnapshotReport) => void;
    const check = vi
      .spyOn(desktopApi, "checkAgentSnapshot")
      .mockReturnValueOnce(
        new Promise((resolve) => {
          finish = resolve;
        }),
      )
      .mockResolvedValue(report());
    render(<Harness />);
    await advance();
    fireEvent(window, new Event("offertrack-snapshot-dirty"));
    await advance(5000);
    expect(check).toHaveBeenCalledOnce();
    await act(async () => {
      finish(report());
      await Promise.resolve();
    });
    expect(
      screen.getByRole("button", { name: "检查并按需刷新快照" }),
    ).toBeDisabled();
    await advance();
    expect(check).toHaveBeenCalledTimes(2);
    expect(screen.getByText(/文件校验通过/)).toBeInTheDocument();
  });

  it("discards old warehouse responses even when migration preserves logical ID", async () => {
    let finish!: (value: SnapshotReport) => void;
    const check = vi
      .spyOn(desktopApi, "checkAgentSnapshot")
      .mockReturnValueOnce(
        new Promise((resolve) => {
          finish = resolve;
        }),
      )
      .mockResolvedValue(report({ snapshot: null, state: "missing" }));
    const view = render(<Harness />);
    await advance();
    view.rerender(
      <Harness
        value={{
          ...warehouse,
          displayPath: "different-synthetic-directory",
          accessMode: "readOnly",
        }}
      />,
    );
    await advance();
    expect(check).toHaveBeenLastCalledWith(
      warehouse.warehouseId,
      "different-synthetic-directory",
    );
    await act(async () => {
      finish(report());
      await Promise.resolve();
    });
    expect(screen.getByText(/尚无可追踪快照/)).toBeInTheDocument();
    expect(
      screen.queryByText("agent-access/snapshot-synthetic"),
    ).not.toBeInTheDocument();
  });

  it("retains previous details on failed check, separates business success, and permits retry", async () => {
    const check = vi
      .spyOn(desktopApi, "checkAgentSnapshot")
      .mockResolvedValueOnce(report())
      .mockRejectedValueOnce(new Error("临时检查错误"))
      .mockResolvedValueOnce(
        report({
          state: "error",
          error: {
            code: "STORAGE_ERROR",
            message: "快照文件已发布但检查点失败",
            retryable: true,
          },
          published: true,
        }),
      )
      .mockResolvedValue(report());
    render(<Harness />);
    await advance();
    fireEvent.focus(window);
    await advance();
    expect(screen.getByText(/业务修改不受影响/)).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("临时检查错误");
    expect(
      screen.getByText("agent-access/snapshot-synthetic"),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "检查并按需刷新快照" }));
    await advance(0);
    expect(screen.getByRole("alert")).toHaveTextContent(
      "快照文件已发布但检查点失败",
    );
    await advance(120000);
    expect(check).toHaveBeenCalledTimes(3); // avoid accumulating unpublished checkpoints every minute
    fireEvent.click(screen.getByRole("button", { name: "检查并按需刷新快照" }));
    await advance(0);
    expect(check).toHaveBeenCalledTimes(4);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("retries temporary warehouse contention and never treats it as a successful check", async () => {
    const check = vi
      .spyOn(desktopApi, "checkAgentSnapshot")
      .mockRejectedValueOnce(
        new OfferTrackError({
          code: "WAREHOUSE_OPERATION_BUSY",
          message: "正在操作",
          retryable: true,
        }),
      )
      .mockResolvedValue(report({ state: "stale" }));
    render(<Harness value={{ ...warehouse, accessMode: "readOnly" }} />);
    await advance();
    expect(screen.getByText(/尚不能确认最新/)).toBeInTheDocument();
    await advance(5000);
    expect(check).toHaveBeenCalledTimes(2);
    expect(screen.getByText(/只读模式不自动生成/)).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
});
