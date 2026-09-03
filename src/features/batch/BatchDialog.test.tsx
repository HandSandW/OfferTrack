import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { desktopApi } from "../../lib/tauri";
import { DraftGuardProvider } from "../../shared/DraftGuardProvider";
import { useDraftGuard } from "../../shared/draftGuard";
import { workflowFixture } from "../../test/workflowFixture";
import { BatchDialog } from "./BatchDialog";
import type { BatchApplied, BatchPreview } from "./contracts";

const targets = [
  { id: "a", revision: 7 },
  { id: "b", revision: 2 },
];
const preview: BatchPreview = {
  version: 1,
  fingerprint: "confirmed-hash",
  changedCount: 1,
  items: [
    {
      id: "a",
      companyName: "虚构甲",
      positionName: "开发",
      changes: ["活跃 → 已归档"],
    },
    { id: "b", companyName: "虚构乙", positionName: "测试", changes: [] },
  ],
};
const onClose = vi.fn();
const onApplied = vi.fn();
beforeEach(() => {
  vi.spyOn(desktopApi, "previewApplicationBatch").mockResolvedValue(preview);
  vi.spyOn(desktopApi, "applyApplicationBatch").mockResolvedValue({
    changedCount: 1,
    backupId: "backup-id",
  });
  onApplied.mockResolvedValue(undefined);
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  onClose.mockReset();
  onApplied.mockReset();
});
function mount() {
  render(
    <BatchDialog targets={targets} onClose={onClose} onApplied={onApplied} />,
    { wrapper: DraftGuardProvider },
  );
}
async function review() {
  fireEvent.click(screen.getByText("预览批量修改"));
  return screen.findByRole("region", { name: "批量修改预览" });
}

it("requires exact target preview and keeps commit success when table refresh fails", async () => {
  onApplied.mockRejectedValueOnce(new Error("reload failed"));
  mount();
  expect(screen.queryByText("确认备份并保存")).not.toBeInTheDocument();
  const region = await review();
  expect(within(region).getByText("虚构甲 · 开发")).toBeInTheDocument();
  expect(within(region).getByText("无变化")).toBeInTheDocument();
  expect(desktopApi.applyApplicationBatch).not.toHaveBeenCalled();
  fireEvent.click(screen.getByText("确认备份并保存"));
  await screen.findByText(/批量修改已完成：1 条/);
  await screen.findByText(/修改已保存，但列表刷新失败/);
  expect(desktopApi.applyApplicationBatch).toHaveBeenCalledWith(
    { version: 1, targets, action: { kind: "archive", archived: true } },
    "confirmed-hash",
  );
  expect(screen.queryByText("确认备份并保存")).not.toBeInTheDocument();
  fireEvent.click(screen.getByText("完成"));
  await waitFor(() => expect(onClose).toHaveBeenCalledOnce());
});

it("cancels without committing and invalidates the preview when changing action", async () => {
  mount();
  await review();
  fireEvent.click(screen.getByText("返回调整"));
  fireEvent.change(screen.getByLabelText("批量操作"), {
    target: { value: "addTags" },
  });
  fireEvent.change(screen.getByLabelText("待添加标签"), {
    target: { value: "校招，重点,校招" },
  });
  await review();
  expect(desktopApi.previewApplicationBatch).toHaveBeenLastCalledWith({
    version: 1,
    targets,
    action: { kind: "addTags", tags: ["校招", "重点"] },
  });
  fireEvent.click(screen.getByText("取消"));
  fireEvent.click(await screen.findByText("放弃修改并继续"));
  await waitFor(() => expect(onClose).toHaveBeenCalledOnce());
  expect(desktopApi.applyApplicationBatch).not.toHaveBeenCalled();
});

it("uses built-in stage/state keys and template revisions, retaining local workflow policy", async () => {
  vi.spyOn(desktopApi, "listWorkflowTemplates").mockResolvedValue([
    workflowFixture({ revision: 9 }),
  ]);
  mount();
  fireEvent.change(screen.getByLabelText("批量操作"), {
    target: { value: "stage" },
  });
  fireEvent.change(screen.getByLabelText("目标阶段"), {
    target: { value: "written_exam" },
  });
  await review();
  expect(desktopApi.previewApplicationBatch).toHaveBeenLastCalledWith({
    version: 1,
    targets,
    action: {
      kind: "stage",
      stageKey: "written_exam",
      stateKey: "awaitingResult",
    },
  });
  fireEvent.click(screen.getByText("返回调整"));
  fireEvent.change(screen.getByLabelText("批量操作"), {
    target: { value: "appendTemplate" },
  });
  fireEvent.click(screen.getByText("读取模板列表"));
  await waitFor(() =>
    expect(screen.getByLabelText("来源模板")).toHaveValue("test-template"),
  );
  await review();
  expect(desktopApi.previewApplicationBatch).toHaveBeenLastCalledWith({
    version: 1,
    targets,
    action: {
      kind: "appendTemplate",
      templateId: "test-template",
      revision: 9,
    },
  });
});

it("retains inputs but requires another preview after an execution conflict", async () => {
  vi.mocked(desktopApi.applyApplicationBatch).mockRejectedValue(
    new Error("版本冲突，整批未保存"),
  );
  mount();
  fireEvent.change(screen.getByLabelText("批量操作"), {
    target: { value: "addTags" },
  });
  fireEvent.change(screen.getByLabelText("待添加标签"), {
    target: { value: "保留输入" },
  });
  await review();
  fireEvent.click(screen.getByText("确认备份并保存"));
  await screen.findByText("版本冲突，整批未保存");
  expect(screen.getByLabelText("待添加标签")).toHaveValue("保留输入");
  expect(screen.queryByText("确认备份并保存")).not.toBeInTheDocument();
  expect(onApplied).not.toHaveBeenCalled();
});

it("blocks duplicate submission, cancellation and navigation during backup/commit", async () => {
  let finish!: (result: BatchApplied) => void;
  vi.mocked(desktopApi.applyApplicationBatch).mockReturnValue(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  const leave = vi.fn();
  function Harness() {
    const { confirmLeave } = useDraftGuard();
    return (
      <>
        <button onClick={() => void confirmLeave().then(leave)}>
          外部导航
        </button>
        <BatchDialog
          targets={targets}
          onClose={onClose}
          onApplied={onApplied}
        />
      </>
    );
  }
  render(<Harness />, { wrapper: DraftGuardProvider });
  await review();
  fireEvent.click(screen.getByText("确认备份并保存"));
  expect(screen.getByText("确认备份并保存")).toBeDisabled();
  expect(screen.getByText("取消")).toBeDisabled();
  fireEvent.click(screen.getByText("确认备份并保存"));
  fireEvent.click(screen.getByText("外部导航"));
  await screen.findByRole("dialog", { name: "正在保存" });
  fireEvent.click(screen.getByText("知道了"));
  await waitFor(() => expect(leave).toHaveBeenCalledWith(false));
  finish({ changedCount: 1, backupId: "backup" });
  await screen.findByText(/批量修改已完成/);
  expect(desktopApi.applyApplicationBatch).toHaveBeenCalledOnce();
});
