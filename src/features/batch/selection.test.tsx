import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { desktopApi } from "../../lib/tauri";
import * as dialogs from "../../lib/dialog";
import { applicationFixture } from "../../test/applicationFixture";
import { DraftGuardProvider } from "../../shared/DraftGuardProvider";
import { ApplicationPage } from "../applications/ApplicationPage";

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => undefined),
}));
const records = Array.from({ length: 22 }, (_, i) =>
  applicationFixture({
    id: `r-${i}`,
    companyName: `示例${String(i).padStart(2, "0")}`,
    positionName: "开发",
    revision: i + 1,
    createdAtUtc: new Date(Date.UTC(2026, 8, 3, 0, 0, 22 - i)).toISOString(),
  }),
);
beforeEach(() => {
  vi.spyOn(desktopApi, "listApplications").mockResolvedValue(records);
  vi.spyOn(desktopApi, "listFieldDefinitions").mockResolvedValue([]);
  vi.spyOn(desktopApi, "listApplicationViews").mockResolvedValue([]);
  vi.spyOn(desktopApi, "getApplicationPageSize").mockResolvedValue(20);
  vi.spyOn(desktopApi, "previewApplicationBatch").mockResolvedValue({
    version: 1,
    fingerprint: "hash",
    changedCount: 2,
    items: [],
  });
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

it("keeps explicit selection across pages and filters without expanding it implicitly", async () => {
  render(<ApplicationPage scope="active" writable onError={vi.fn()} />, {
    wrapper: DraftGuardProvider,
  });
  fireEvent.click(await screen.findByLabelText("选择投递 示例00 开发"));
  expect(screen.queryByLabelText("关闭详情")).not.toBeInTheDocument();
  fireEvent.click(screen.getByText("下一页"));
  fireEvent.click(screen.getByLabelText("选择投递 示例20 开发"));
  fireEvent.change(screen.getByLabelText("搜索投递"), {
    target: { value: "示例21" },
  });
  expect(screen.getByText("已选 2 条（跨页保留）")).toBeInTheDocument();
  fireEvent.click(screen.getByText("批量修改"));
  fireEvent.click(await screen.findByText("预览批量修改"));
  await waitFor(() =>
    expect(desktopApi.previewApplicationBatch).toHaveBeenCalledWith({
      version: 1,
      targets: [
        { id: "r-0", revision: 1 },
        { id: "r-20", revision: 21 },
      ],
      action: { kind: "archive", archived: true },
    }),
  );
});

it("allows read-only export selection while keeping batch mutation disabled", async () => {
  render(
    <ApplicationPage scope="active" writable={false} onError={vi.fn()} />,
    { wrapper: DraftGuardProvider },
  );
  expect(await screen.findByLabelText("选择投递 示例00 开发")).toBeEnabled();
  expect(screen.getByText("选择本页")).toBeEnabled();
  expect(screen.queryByText("批量修改")).not.toBeInTheDocument();
  fireEvent.click(screen.getByText("选择本页"));
  expect(screen.getByText("批量修改")).toBeDisabled();
  expect(screen.getByText("导出")).toBeEnabled();
  expect(desktopApi.previewApplicationBatch).not.toHaveBeenCalled();
});

it("exports the entire filtered table across pagination from a read-only warehouse", async () => {
  vi.spyOn(desktopApi, "getExportCatalog").mockResolvedValue({
    version: 1,
    total: 22,
    columns: [{ key: "companyName", label: "公司名称", fieldType: "text" }],
  });
  vi.spyOn(desktopApi, "exportApplications").mockResolvedValue({
    path: "export/table.xlsx",
    mappingPath: "export/fields.json",
    rowCount: 22,
  });
  vi.spyOn(dialogs, "selectDirectory").mockResolvedValue("chosen-parent");
  render(
    <ApplicationPage scope="active" writable={false} onError={vi.fn()} />,
    { wrapper: DraftGuardProvider },
  );
  await screen.findByLabelText("选择投递 示例00 开发");
  expect(
    screen.queryByLabelText("选择投递 示例20 开发"),
  ).not.toBeInTheDocument();
  fireEvent.click(screen.getByText("导出"));
  const button = await screen.findByText("选择位置并导出 22 条");
  await waitFor(() => expect(button).toBeEnabled());
  fireEvent.click(button);
  await screen.findByText("已导出 22 条。");
  expect(desktopApi.exportApplications).toHaveBeenCalledWith(
    "chosen-parent",
    expect.objectContaining({
      scope: {
        kind: "records",
        partition: "active",
        targets: records.map((r) => ({ id: r.id, revision: r.revision })),
      },
    }),
  );
  expect(desktopApi.previewApplicationBatch).not.toHaveBeenCalled();
});
