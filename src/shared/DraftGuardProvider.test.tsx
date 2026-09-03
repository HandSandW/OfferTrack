import { useLayoutEffect, useState } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DraftGuardProvider } from "./DraftGuardProvider";
import { useDraftGuard, useDraftState } from "./draftGuard";

const native = vi.hoisted(() => ({
  isTauri: vi.fn(),
  onCloseRequested: vi.fn(),
  destroy: vi.fn(),
  stop: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ isTauri: native.isTauri }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => native }));

function Draft({ busy = false }: { busy?: boolean }) {
  const [value, setValue] = useState("");
  const [left, setLeft] = useState(false);
  const { confirmLeave } = useDraftGuard();
  useDraftState(value !== "", busy, "测试表单");
  return (
    <>
      <input
        aria-label="草稿"
        value={value}
        onChange={(event) => setValue(event.target.value)}
      />
      <button
        onClick={() => {
          void confirmLeave().then(setLeft);
        }}
      >
        离开
      </button>
      {left && <span>已离开</span>}
    </>
  );
}
type CloseHandler = (event: { preventDefault: () => void }) => Promise<void>;
function CommitObserver({
  busy,
  onResult,
}: {
  busy: boolean;
  onResult: (allowed: boolean) => void;
}) {
  useDraftState(false, busy, "刚完成的操作");
  const { confirmLeave } = useDraftGuard();
  useLayoutEffect(() => {
    if (!busy) void confirmLeave().then(onResult);
  }, [busy, confirmLeave, onResult]);
  return null;
}
describe("draft navigation and native close protection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    native.isTauri.mockReturnValue(false);
    native.onCloseRequested.mockResolvedValue(native.stop);
    native.destroy.mockResolvedValue(undefined);
  });
  afterEach(cleanup);
  it("commits the latest busy state before controls in the same frame can navigate", async () => {
    const result = vi.fn();
    const view = render(<CommitObserver busy onResult={result} />, {
      wrapper: DraftGuardProvider,
    });
    view.rerender(<CommitObserver busy={false} onResult={result} />);
    await waitFor(() => expect(result).toHaveBeenCalledWith(true));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
  it("allows clean navigation and asks before discarding an edited draft", async () => {
    render(<Draft />, { wrapper: DraftGuardProvider });
    fireEvent.change(screen.getByLabelText("草稿"), {
      target: { value: "未保存" },
    });
    fireEvent.click(screen.getByText("离开"));
    fireEvent.click(await screen.findByText("继续编辑"));
    expect(screen.queryByText("已离开")).not.toBeInTheDocument();
    expect(screen.getByLabelText("草稿")).toHaveValue("未保存");
    fireEvent.click(screen.getByText("离开"));
    fireEvent.click(await screen.findByText("放弃修改并继续"));
    expect(await screen.findByText("已离开")).toBeInTheDocument();
  });
  it("never offers discard while a write is in progress", async () => {
    render(<Draft busy />, { wrapper: DraftGuardProvider });
    fireEvent.click(screen.getByText("离开"));
    expect(
      await screen.findByRole("dialog", { name: "正在保存" }),
    ).toBeInTheDocument();
    expect(screen.queryByText("放弃修改并继续")).not.toBeInTheDocument();
    fireEvent.click(screen.getByText("知道了"));
    expect(screen.queryByText("已离开")).not.toBeInTheDocument();
  });
  it("guards browser unload only for dirty drafts", () => {
    render(<Draft />, { wrapper: DraftGuardProvider });
    expect(
      window.dispatchEvent(new Event("beforeunload", { cancelable: true })),
    ).toBe(true);
    fireEvent.change(screen.getByLabelText("草稿"), {
      target: { value: "draft" },
    });
    expect(
      window.dispatchEvent(new Event("beforeunload", { cancelable: true })),
    ).toBe(false);
  });
  it("intercepts native close, cancels safely and destroys only after confirmation", async () => {
    native.isTauri.mockReturnValue(true);
    render(<Draft />, { wrapper: DraftGuardProvider });
    fireEvent.change(screen.getByLabelText("草稿"), {
      target: { value: "draft" },
    });
    const handler = native.onCloseRequested.mock.calls[0]?.[0] as CloseHandler;
    const event = { preventDefault: vi.fn() };
    let pending: Promise<void>;
    act(() => {
      pending = handler(event);
    });
    expect(event.preventDefault).toHaveBeenCalledOnce();
    fireEvent.click(await screen.findByText("继续编辑"));
    await act(async () => {
      await pending;
    });
    expect(native.destroy).not.toHaveBeenCalled();
    act(() => {
      pending = handler(event);
    });
    fireEvent.click(await screen.findByText("放弃修改并继续"));
    await act(async () => {
      await pending;
    });
    expect(native.destroy).toHaveBeenCalledOnce();
    expect(
      window.dispatchEvent(new Event("beforeunload", { cancelable: true })),
    ).toBe(true);
  });
  it("closes a clean native window and reports a failed destroy", async () => {
    native.isTauri.mockReturnValue(true);
    native.destroy.mockRejectedValueOnce(new Error("denied"));
    render(<Draft />, { wrapper: DraftGuardProvider });
    const handler = native.onCloseRequested.mock.calls[0]?.[0] as CloseHandler;
    let pending: Promise<void>;
    act(() => {
      pending = handler({ preventDefault: vi.fn() });
    });
    expect(await screen.findByText("窗口未关闭")).toBeInTheDocument();
    fireEvent.click(screen.getByText("知道了"));
    await act(async () => {
      await pending;
    });
    expect(screen.getByLabelText("草稿")).toBeInTheDocument();
  });
});
