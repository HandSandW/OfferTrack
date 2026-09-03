import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ApplicationDetail, DuplicatePreview } from "../../contracts";
import { desktopApi } from "../../lib/tauri";
import { DraftGuardProvider } from "../../shared/DraftGuardProvider";
import { applicationFixture } from "../../test/applicationFixture";
import { DetailPanel } from "./DetailPanel";
const onChange = vi.fn();
const onError = vi.fn();
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  onChange.mockClear();
  onError.mockClear();
});
const show = () =>
  render(
    <DetailPanel
      detail={applicationFixture()}
      fields={[]}
      writable
      scope="active"
      onChange={onChange}
      onError={onError}
    />,
    { wrapper: DraftGuardProvider },
  );
describe("background copy feedback", () => {
  it("shows preview and copy phases, prevents repeated writes/leaving, and waits for the committed record", async () => {
    let preview!: (value: DuplicatePreview) => void;
    let copied!: (value: ApplicationDetail) => void;
    vi.spyOn(desktopApi, "previewApplicationDuplicate").mockImplementation(
      () =>
        new Promise((done) => {
          preview = done;
        }),
    );
    const duplicate = vi
      .spyOn(desktopApi, "duplicateApplication")
      .mockImplementation(
        () =>
          new Promise((done) => {
            copied = done;
          }),
      );
    show();
    fireEvent.click(screen.getByRole("button", { name: "复制完整记录" }));
    await screen.findByText(/正在统计附件大小/);
    expect(screen.getByRole("progressbar")).not.toHaveAttribute("value");
    preview({
      mode: "fullRecord",
      fileSizeBytes: 1048576,
      editableFieldCount: 13,
    });
    fireEvent.click(await screen.findByRole("button", { name: "复制" }));
    await screen.findByText(/正在复制并校验附件/);
    expect(screen.getByRole("button", { name: "复制完整记录" })).toBeDisabled();
    fireEvent.click(screen.getByLabelText("关闭详情"));
    const prompt = await screen.findByRole("dialog", { name: "正在保存" });
    fireEvent.click(within(prompt).getByRole("button", { name: "知道了" }));
    expect(onChange).not.toHaveBeenCalled();
    expect(duplicate).toHaveBeenCalledOnce();
    const result = applicationFixture({ id: "copy" });
    copied(result);
    await waitFor(() => expect(onChange).toHaveBeenCalledWith(result));
    expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();
  });
  it("cancels without copying and preserves the original when copying fails", async () => {
    vi.spyOn(desktopApi, "previewApplicationDuplicate").mockResolvedValue({
      mode: "fullRecord",
      fileSizeBytes: 10,
      editableFieldCount: 13,
    });
    const duplicate = vi
      .spyOn(desktopApi, "duplicateApplication")
      .mockRejectedValue(new Error("附件被占用，请关闭编辑程序后重试"));
    show();
    fireEvent.click(screen.getByRole("button", { name: "复制完整记录" }));
    fireEvent.click(await screen.findByRole("button", { name: "取消" }));
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "复制完整记录" }),
      ).toBeEnabled(),
    );
    expect(duplicate).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "复制完整记录" }));
    fireEvent.click(await screen.findByRole("button", { name: "复制" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("附件被占用");
    expect(onChange).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "复制完整记录" })).toBeEnabled();
  });
});
