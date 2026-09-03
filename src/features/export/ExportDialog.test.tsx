import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { desktopApi } from "../../lib/tauri";
import * as dialog from "../../lib/dialog";
import { DraftGuardProvider } from "../../shared/DraftGuardProvider";
import { useDraftGuard } from "../../shared/draftGuard";
import { ExportDialog } from "./ExportDialog";
import type { ExportCreated } from "./contracts";

const onClose = vi.fn();
const catalog = {
  version: 1 as const,
  total: 8,
  columns: [
    { key: "companyName", label: "公司名称", fieldType: "text" },
    { key: "notes", label: "备注", fieldType: "text" },
    { key: "custom:field", label: "薪资", fieldType: "number" },
    { key: "documentPaths", label: "简历绝对路径", fieldType: "text" },
  ],
};
beforeEach(() => {
  vi.spyOn(desktopApi, "getExportCatalog").mockResolvedValue(catalog);
  vi.spyOn(desktopApi, "exportApplications").mockResolvedValue({
    path: "export/applications.xlsx",
    mappingPath: "export/fields.json",
    rowCount: 2,
  });
  vi.spyOn(dialog, "selectDirectory").mockResolvedValue("chosen-parent");
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  onClose.mockReset();
});
function Navigation() {
  const { confirmLeave } = useDraftGuard();
  return <button onClick={() => void confirmLeave()}>尝试离开</button>;
}
function mount(
  filtered = [
    { id: "a", revision: 1 },
    { id: "b", revision: 3 },
  ],
) {
  render(
    <>
      <ExportDialog
        partition="active"
        filtered={filtered}
        selected={[{ id: "b", revision: 3 }]}
        columns={[
          { key: "companyName", visible: true, width: 100, pinned: false },
          { key: "notes", visible: false, width: 100, pinned: false },
          { key: "custom:field", visible: true, width: 100, pinned: false },
        ]}
        onClose={onClose}
      />
      <Navigation />
    </>,
    { wrapper: DraftGuardProvider },
  );
}
async function ready() {
  await waitFor(() => expect(screen.getByLabelText("导出范围")).toBeEnabled());
}
async function run() {
  fireEvent.click(screen.getByRole("button", { name: /选择位置并导出/ }));
  await screen.findByText("已导出 2 条。");
}
it("exports all filtered rows with revisions and visible custom columns but not private paths", async () => {
  mount();
  await ready();
  expect(screen.getByRole("heading", { name: "导出投递记录" })).toHaveFocus();
  await run();
  expect(desktopApi.exportApplications).toHaveBeenCalledWith("chosen-parent", {
    version: 1,
    format: "xlsx",
    columns: ["companyName", "custom:field"],
    scope: {
      kind: "records",
      partition: "active",
      targets: [
        { id: "a", revision: 1 },
        { id: "b", revision: 3 },
      ],
    },
  });
  expect(screen.getByText(/export\/fields.json/)).toBeInTheDocument();
  expect(onClose).not.toHaveBeenCalled();
});
it("selects explicit whole-warehouse or selected scopes and keeps paths opt-in", async () => {
  mount();
  await ready();
  fireEvent.change(screen.getByLabelText("导出范围"), {
    target: { value: "all" },
  });
  fireEvent.change(screen.getByLabelText("文件格式"), {
    target: { value: "csv" },
  });
  fireEvent.click(screen.getByText("包含隐藏列和自定义字段（不含绝对路径）"));
  expect(screen.getByLabelText("简历绝对路径")).not.toBeChecked();
  await run();
  expect(desktopApi.exportApplications).toHaveBeenLastCalledWith(
    "chosen-parent",
    expect.objectContaining({
      format: "csv",
      scope: { kind: "all" },
      columns: ["companyName", "notes", "custom:field"],
    }),
  );
  fireEvent.change(screen.getByLabelText("导出范围"), {
    target: { value: "selected" },
  });
  fireEvent.click(screen.getByLabelText("简历绝对路径"));
  expect(screen.getByText(/绝对路径会暴露本机目录/)).toBeInTheDocument();
  await run();
  expect(desktopApi.exportApplications).toHaveBeenLastCalledWith(
    "chosen-parent",
    expect.objectContaining({
      scope: {
        kind: "records",
        partition: "active",
        targets: [{ id: "b", revision: 3 }],
      },
      columns: ["companyName", "notes", "custom:field", "documentPaths"],
    }),
  );
});
it("does not turn an empty filtered set into all records, and canceling the picker does not export", async () => {
  vi.mocked(dialog.selectDirectory).mockResolvedValueOnce(null);
  mount([]);
  await ready();
  fireEvent.click(screen.getByText("选择位置并导出 0 条"));
  await waitFor(() =>
    expect(screen.getByText("选择位置并导出 0 条")).toBeEnabled(),
  );
  expect(desktopApi.exportApplications).not.toHaveBeenCalled();
  await run();
  expect(desktopApi.exportApplications).toHaveBeenCalledWith(
    "chosen-parent",
    expect.objectContaining({
      scope: { kind: "records", partition: "active", targets: [] },
    }),
  );
});
it("retains choices after failure, prevents duplicate export and blocks close/navigation while running", async () => {
  let complete!: (result: ExportCreated) => void;
  vi.mocked(desktopApi.exportApplications).mockRejectedValueOnce(
    new Error("版本冲突"),
  );
  mount();
  await ready();
  fireEvent.click(screen.getByRole("button", { name: /选择位置并导出/ }));
  await screen.findByText("版本冲突");
  expect(screen.getByLabelText("薪资")).toBeChecked();
  vi.mocked(desktopApi.exportApplications).mockReturnValueOnce(
    new Promise((resolve) => {
      complete = resolve;
    }),
  );
  fireEvent.click(screen.getByRole("button", { name: /选择位置并导出/ }));
  await waitFor(() =>
    expect(desktopApi.exportApplications).toHaveBeenCalledTimes(2),
  );
  expect(screen.getByText("正在导出…")).toBeDisabled();
  expect(screen.getByText("关闭")).toBeDisabled();
  fireEvent.click(screen.getByText("尝试离开"));
  await screen.findByText("请等待当前操作完成后再离开，避免重复操作。");
  fireEvent.click(screen.getByText("知道了"));
  expect(onClose).not.toHaveBeenCalled();
  await act(async () => {
    complete({ path: "done", mappingPath: "map", rowCount: 2 });
    await Promise.resolve();
  });
  expect(screen.getByText("已导出 2 条。")).toBeInTheDocument();
});
it("retries catalog failure and disables an empty field selection", async () => {
  vi.mocked(desktopApi.getExportCatalog).mockRejectedValueOnce(
    new Error("读取失败"),
  );
  mount();
  await screen.findByText("读取失败");
  expect(screen.getByRole("button", { name: /选择位置并导出/ })).toBeDisabled();
  fireEvent.click(screen.getByText("重试读取导出字段"));
  await ready();
  fireEvent.click(screen.getByText("清除字段选择"));
  expect(screen.getByRole("button", { name: /选择位置并导出/ })).toBeDisabled();
  fireEvent.click(screen.getByText("关闭"));
  expect(onClose).toHaveBeenCalledOnce();
});
