import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { desktopApi } from "../../lib/tauri";
import { RecoveryDiagnosticsPanel } from "./RecoveryDiagnosticsPanel";
const onError = vi.fn();
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  onError.mockClear();
});
describe("read-only recovery diagnostics", () => {
  it("only inspects on request and warns about missing identity without offering cleanup", async () => {
    const read = vi
      .spyOn(desktopApi, "getRecoveryDiagnostics")
      .mockResolvedValue({
        version: 1,
        totalPending: 2,
        items: [
          {
            id: "operation",
            kind: "creation",
            identityRecorded: false,
            source: {
              relativePath: "recycle-bin/records/.copying-fixture",
              state: "available",
            },
            target: { relativePath: null, state: "unsafe" },
          },
        ],
      });
    render(<RecoveryDiagnosticsPanel onError={onError} />);
    expect(read).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "读取只读诊断" }));
    await screen.findByText(/没有已记录的文件\/目录身份/);
    expect(screen.getByText(/仅展示 1 项/)).toBeInTheDocument();
    expect(screen.getByText(/路径已隐藏/)).toBeInTheDocument();
    expect(screen.getAllByRole("button")).toHaveLength(1);
  });
  it("does not mistake failure or an empty journal for a successful integrity check", async () => {
    vi.spyOn(desktopApi, "getRecoveryDiagnostics")
      .mockRejectedValueOnce(new Error("读取失败"))
      .mockResolvedValueOnce({ version: 1, totalPending: 0, items: [] });
    render(<RecoveryDiagnosticsPanel onError={onError} />);
    fireEvent.click(screen.getByRole("button", { name: "读取只读诊断" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("读取失败");
    expect(screen.queryByText(/没有待处理日志/)).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "读取只读诊断" }));
    await screen.findByText(/这不代表全部文件健康/);
  });
  it("renders attachment recycle recovery as file observations without repair controls", async () => {
    vi.spyOn(desktopApi, "getRecoveryDiagnostics").mockResolvedValue({
      version: 1,
      totalPending: 1,
      items: [
        {
          id: "move",
          kind: "documentTrash",
          identityRecorded: true,
          source: {
            relativePath: "applications/record/a.pdf",
            state: "available",
          },
          target: {
            relativePath: "recycle-bin/documents/id",
            state: "missing",
          },
        },
      ],
    });
    render(<RecoveryDiagnosticsPanel onError={onError} />);
    fireEvent.click(screen.getByRole("button", { name: "读取只读诊断" }));
    await screen.findByText("附件移入回收站");
    expect(screen.getByText(/文件存在/)).toBeInTheDocument();
    expect(screen.getByText(/文件不存在/)).toBeInTheDocument();
    expect(screen.getAllByRole("button")).toHaveLength(1);
  });
});
