import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { desktopApi } from "../../lib/tauri";
import { DraftGuardProvider } from "../../shared/DraftGuardProvider";
import { applicationFixture } from "../../test/applicationFixture";
import { ApplicationGrid } from "./ApplicationGrid";
import { columnLabels, defaultColumns } from "./tableModel";

const onError = vi.fn();
const onUpdated = vi.fn();
const onOpen = vi.fn();
const editable = defaultColumns.filter((c) =>
  ["companyName", "industry", "notes"].includes(c.key),
);
function show(
  overrides: Partial<React.ComponentProps<typeof ApplicationGrid>> = {},
) {
  const record = applicationFixture();
  return {
    record,
    ...render(
      <DraftGuardProvider>
        <ApplicationGrid
          grouped={[["", [record]]]}
          grouping={false}
          columns={editable}
          labels={columnLabels}
          sort={[]}
          fields={[]}
          selectedId={null}
          checked={[]}
          writable
          disabled={false}
          empty
          onSort={vi.fn()}
          onOpen={onOpen}
          onBeforeEdit={() => Promise.resolve(true)}
          onUpdated={onUpdated}
          onChecked={vi.fn()}
          onError={onError}
          renderCell={(r, c) =>
            c.key === "companyName"
              ? r.companyName
              : c.key === "industry"
                ? r.industry
                : r.notes
          }
          {...overrides}
        />
      </DraftGuardProvider>,
    ),
  };
}
beforeEach(() => {
  vi.restoreAllMocks();
  onError.mockReset();
  onUpdated.mockReset();
  onOpen.mockReset();
});
afterEach(cleanup);

describe("keyboard application grid", () => {
  it("navigates by direction and opens detail only when requested", async () => {
    show();
    const company = screen.getByLabelText("公司名称 · 示例公司 · 开发工程师");
    company.focus();
    fireEvent.keyDown(company, { key: "ArrowRight" });
    await waitFor(() =>
      expect(
        screen.getByLabelText("行业 · 示例公司 · 开发工程师"),
      ).toHaveFocus(),
    );
    fireEvent.keyDown(screen.getByLabelText("行业 · 示例公司 · 开发工程师"), {
      key: "Home",
    });
    await waitFor(() => expect(company).toHaveFocus());
    fireEvent.keyDown(company, { key: "Enter", shiftKey: true });
    expect(onOpen).toHaveBeenCalledWith("fixture-application");
  });

  it("saves one cell, restores focus and undoes only with the returned revision", async () => {
    const edit = vi
      .spyOn(desktopApi, "editApplicationCell")
      .mockImplementationOnce((request) =>
        Promise.resolve({
          changed: true,
          previousValue: "示例公司",
          record: applicationFixture({
            companyName: String(request.value),
            revision: 2,
          }),
        }),
      )
      .mockImplementationOnce((request) =>
        Promise.resolve({
          changed: true,
          previousValue: "新公司",
          record: applicationFixture({
            companyName: String(request.value),
            revision: 3,
          }),
        }),
      );
    show();
    const cell = screen.getByLabelText("公司名称 · 示例公司 · 开发工程师");
    cell.focus();
    fireEvent.keyDown(cell, { key: "Enter" });
    const dialog = await screen.findByRole("dialog");
    fireEvent.change(within(dialog).getByLabelText("公司名称"), {
      target: { value: "新公司" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "保存单元格" }));
    await waitFor(() =>
      expect(edit).toHaveBeenNthCalledWith(1, {
        version: 1,
        id: "fixture-application",
        revision: 1,
        key: "companyName",
        value: "新公司",
      }),
    );
    await waitFor(() => expect(cell).toHaveFocus());
    fireEvent.keyDown(cell, { key: "z", ctrlKey: true });
    await waitFor(() =>
      expect(edit).toHaveBeenNthCalledWith(2, {
        version: 1,
        id: "fixture-application",
        revision: 2,
        key: "companyName",
        value: "示例公司",
      }),
    );
    expect(await screen.findByText(/已撤销最近一次/)).toBeVisible();
    expect(screen.getByRole("button", { name: "撤销单格修改" })).toBeDisabled();
  });

  it("keeps failed input, supports safe single-cell paste and rejects grid-shaped paste", async () => {
    vi.spyOn(desktopApi, "editApplicationCell").mockRejectedValueOnce(
      new Error("记录已被其他操作修改"),
    );
    show();
    const notes = screen.getByLabelText("备注 · 示例公司 · 开发工程师");
    fireEvent.paste(notes, {
      clipboardData: { getData: () => "第一行\n第二行" },
    });
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByLabelText("备注")).toHaveValue("第一行\n第二行");
    fireEvent.keyDown(within(dialog).getByLabelText("备注"), {
      key: "Enter",
      ctrlKey: true,
    });
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      "记录已被其他操作修改",
    );
    expect(within(dialog).getByLabelText("备注")).toHaveValue("第一行\n第二行");
    fireEvent.click(
      within(dialog).getByRole("button", { name: "放弃本格修改" }),
    );
    fireEvent.paste(notes, { clipboardData: { getData: () => "a\tb" } });
    expect(await screen.findByText(/仅支持单格粘贴/)).toBeVisible();
  });

  it("keeps read-only cells navigable but never opens a writer", () => {
    const edit = vi.spyOn(desktopApi, "editApplicationCell");
    show({ writable: false });
    const cell = screen.getByLabelText("公司名称 · 示例公司 · 开发工程师");
    cell.focus();
    fireEvent.keyDown(cell, { key: "Enter" });
    expect(onOpen).toHaveBeenCalledWith("fixture-application");
    fireEvent.paste(cell, { clipboardData: { getData: () => "不可写" } });
    expect(screen.getByText(/此单元格不可直接编辑/)).toBeVisible();
    expect(edit).not.toHaveBeenCalled();
  });

  it("copies the full cell value without opening a writer", () => {
    const setData = vi.fn();
    show();
    const cell = screen.getByLabelText("公司名称 · 示例公司 · 开发工程师");
    cell.focus();
    fireEvent.copy(cell, { clipboardData: { setData } });
    expect(setData).toHaveBeenCalledWith("text/plain", "示例公司");
    expect(screen.getByText("已复制单元格完整内容。")).toBeVisible();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("saves select editors with Enter and preserves multiline custom text", async () => {
    const record = applicationFixture({
      companyType: "private",
      customFields: { memo: "原文" },
    });
    const edit = vi
      .spyOn(desktopApi, "editApplicationCell")
      .mockImplementation((request) =>
        Promise.resolve({
          changed: true,
          previousValue: "private",
          record: {
            ...record,
            revision: 2,
            companyType: String(request.value),
          },
        }),
      );
    show({
      grouped: [["", [record]]],
      columns: [
        { key: "companyType", width: 100, visible: true, pinned: false },
        { key: "custom:memo", width: 150, visible: true, pinned: false },
      ],
      labels: { ...columnLabels, "custom:memo": "补充说明" },
      fields: [
        {
          id: "memo",
          key: "memo",
          displayName: "补充说明",
          fieldType: "text",
          displayOrder: 1,
          isVisible: true,
          config: {},
          revision: 1,
        },
      ],
    });
    const type = screen.getByLabelText("企业性质 · 示例公司 · 开发工程师");
    type.focus();
    fireEvent.keyDown(type, { key: "Enter" });
    const select = await screen.findByLabelText("企业性质");
    fireEvent.change(select, { target: { value: "bank" } });
    fireEvent.keyDown(select, { key: "Enter" });
    await waitFor(() =>
      expect(edit).toHaveBeenCalledWith(
        expect.objectContaining({ key: "companyType", value: "bank" }),
      ),
    );
    const memo = screen.getByLabelText("补充说明 · 示例公司 · 开发工程师");
    memo.focus();
    fireEvent.keyDown(memo, { key: "Enter" });
    const area = await screen.findByLabelText("补充说明");
    expect(area.tagName).toBe("TEXTAREA");
    fireEvent.change(area, { target: { value: "第一行\n第二行" } });
    expect(area).toHaveValue("第一行\n第二行");
  });
});
