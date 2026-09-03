import { useState } from "react";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { SavedView, SavedViewChange, ViewSnapshot } from "../../contracts";
import { desktopApi } from "../../lib/tauri";
import { DraftGuardProvider } from "../../shared/DraftGuardProvider";
import { useDraftGuard } from "../../shared/draftGuard";
import { defaultColumns, initialFilter } from "../applications/tableModel";
import { ViewControls } from "./ViewControls";

const stored: SavedView = {
  id: "saved",
  revision: 3,
  name: "原视图",
  isDefault: true,
  layout: { columns: defaultColumns },
  sort: [],
  filter: initialFilter,
  group: null,
};
const temporary: ViewSnapshot = {
  layout: { columns: defaultColumns.slice(0, 2) },
  sort: [{ key: "companyName", direction: "asc" }],
  filter: { ...initialFilter, search: "临时搜索" },
  group: "companyType",
};
const onError = vi.fn();
const onApply = vi.fn();
function Harness({ writable = true }: { writable?: boolean }) {
  const [views, setViews] = useState([stored]);
  const [id, setId] = useState(stored.id);
  const [snapshot, setSnapshot] = useState<ViewSnapshot>(temporary);
  const [left, setLeft] = useState(false);
  const { confirmLeave } = useDraftGuard();
  return (
    <>
      <ViewControls
        views={views}
        activeId={id}
        current={snapshot}
        writable={writable}
        disabled={false}
        onSelect={setId}
        onError={onError}
        onViews={(next) => {
          setViews(next);
          if (!next.some((view) => view.id === id)) setId("");
        }}
        onApply={(view) => {
          setId(view.id);
          setSnapshot(view);
          onApply(view);
        }}
      />
      <output aria-label="当前筛选">{snapshot.filter.search}</output>
      <button onClick={() => void confirmLeave().then(setLeft)}>
        离开页面
      </button>
      {left && <p>已经离开</p>}
    </>
  );
}
const show = (writable = true) =>
  render(<Harness writable={writable} />, { wrapper: DraftGuardProvider });
const save = () =>
  fireEvent.click(
    within(screen.getByRole("dialog")).getByRole("button", {
      name: "保存",
    }),
  );
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  onError.mockClear();
  onApply.mockClear();
});

describe("saved view management", () => {
  it("renames and copies saved metadata without overwriting temporary layout, then changes the default", async () => {
    const renamed = { ...stored, name: "新名字", revision: 4 };
    const copy = {
      ...renamed,
      id: "copy",
      name: "副本",
      revision: 1,
      isDefault: false,
    };
    const metadata = vi
      .spyOn(desktopApi, "updateViewMetadata")
      .mockResolvedValue({ view: renamed, views: [renamed] });
    const duplicate = vi
      .spyOn(desktopApi, "duplicateApplicationView")
      .mockResolvedValue({ view: copy, views: [renamed, copy] });
    const saveSnapshot = vi.spyOn(desktopApi, "saveApplicationView");
    show();
    fireEvent.click(screen.getByRole("button", { name: "重命名视图" }));
    fireEvent.change(screen.getByLabelText("视图名称"), {
      target: { value: "新名字" },
    });
    save();
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(metadata).toHaveBeenCalledWith({
      id: "saved",
      revision: 3,
      name: "新名字",
      isDefault: true,
    });
    expect(screen.getByLabelText("当前筛选")).toHaveTextContent("临时搜索");
    fireEvent.click(screen.getByRole("button", { name: "复制视图" }));
    expect(screen.getByText(/复制已保存的布局/)).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("视图名称"), {
      target: { value: "副本" },
    });
    save();
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(duplicate).toHaveBeenCalledWith("saved", 4, "副本");
    expect(screen.getByLabelText("已保存视图")).toHaveValue("saved");
    expect(screen.getByLabelText("当前筛选")).toHaveTextContent("临时搜索");
    fireEvent.change(screen.getByLabelText("已保存视图"), {
      target: { value: "copy" },
    });
    metadata.mockResolvedValue({
      view: { ...copy, revision: 2, isDefault: true },
      views: [
        { ...renamed, isDefault: false, revision: 5 },
        { ...copy, revision: 2, isDefault: true },
      ],
    });
    fireEvent.click(screen.getByRole("button", { name: "设为默认视图" }));
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "设为默认视图" }),
      ).toBeDisabled(),
    );
    expect(metadata).toHaveBeenLastCalledWith({
      id: "copy",
      revision: 1,
      name: "副本",
      isDefault: true,
    });
    expect(saveSnapshot).not.toHaveBeenCalled();
    expect(onApply).not.toHaveBeenCalled();
  });

  it("only replaces an existing snapshot on explicit save with its expected revision", async () => {
    const updated = { ...stored, ...temporary, revision: 4 };
    const persist = vi
      .spyOn(desktopApi, "saveApplicationView")
      .mockResolvedValue({ view: updated, views: [updated] });
    show();
    fireEvent.click(screen.getByRole("button", { name: "更新当前视图" }));
    expect(persist).not.toHaveBeenCalled();
    expect(screen.getByText(/覆盖此视图的配置/)).toBeInTheDocument();
    save();
    await waitFor(() => expect(onApply).toHaveBeenCalledWith(updated));
    expect(persist).toHaveBeenCalledWith({
      ...temporary,
      id: "saved",
      revision: 3,
      name: "原视图",
      isDefault: true,
    });
  });

  it("protects new drafts on Escape/navigation, retains failures, and blocks leaving while saving", async () => {
    const persist = vi
      .spyOn(desktopApi, "saveApplicationView")
      .mockRejectedValueOnce(new Error("保存暂时失败"));
    show();
    fireEvent.click(screen.getByRole("button", { name: "保存视图" }));
    fireEvent.change(screen.getByLabelText("视图名称"), {
      target: { value: "草稿" },
    });
    fireEvent(
      screen.getByRole("dialog"),
      new Event("cancel", { cancelable: true }),
    );
    fireEvent.click(await screen.findByRole("button", { name: "继续编辑" }));
    expect(screen.getByLabelText("视图名称")).toHaveValue("草稿");
    save();
    expect(await screen.findByRole("alert")).toHaveTextContent("输入仍保留");
    expect(screen.getByLabelText("视图名称")).toHaveValue("草稿");
    let resolve!: (value: SavedViewChange) => void;
    persist.mockImplementationOnce(
      () =>
        new Promise((done) => {
          resolve = done;
        }),
    );
    save();
    fireEvent.click(screen.getByRole("button", { name: "离开页面" }));
    expect(
      await screen.findByRole("dialog", { name: "正在保存" }),
    ).toBeInTheDocument();
    expect(screen.queryByText("已经离开")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "知道了" }));
    expect(screen.getByLabelText("视图名称")).toBeDisabled();
    const created = {
      ...stored,
      ...temporary,
      id: "new",
      name: "草稿",
      revision: 1,
    };
    resolve({ view: created, views: [created] });
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(persist).toHaveBeenLastCalledWith({
      ...temporary,
      id: null,
      revision: null,
      name: "草稿",
      isDefault: true,
    });
    expect(screen.getByLabelText("已保存视图")).toHaveValue("new");
  });

  it("cancels dirty editors without a write when discard is confirmed", async () => {
    const persist = vi.spyOn(desktopApi, "saveApplicationView");
    show();
    fireEvent.click(screen.getByRole("button", { name: "保存视图" }));
    fireEvent.change(screen.getByLabelText("视图名称"), {
      target: { value: "放弃" },
    });
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    fireEvent.click(
      await screen.findByRole("button", { name: "放弃修改并继续" }),
    );
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(persist).not.toHaveBeenCalled();
    expect(screen.getByLabelText("当前筛选")).toHaveTextContent("临时搜索");
  });

  it("confirms deletion, retains a failed selection, and leaves the current display as temporary", async () => {
    const remove = vi
      .spyOn(desktopApi, "deleteApplicationView")
      .mockRejectedValueOnce(new Error("版本已变化"))
      .mockResolvedValueOnce([]);
    show();
    fireEvent.click(screen.getByRole("button", { name: "删除视图" }));
    fireEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", { name: "取消" }),
    );
    expect(remove).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "删除视图" }));
    fireEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "删除视图",
      }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent("版本已变化");
    expect(screen.getByLabelText("已保存视图")).toHaveValue("saved");
    fireEvent.click(screen.getByRole("button", { name: "删除视图" }));
    fireEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "删除视图",
      }),
    );
    await waitFor(() =>
      expect(screen.getByLabelText("已保存视图")).toHaveValue(""),
    );
    expect(remove).toHaveBeenLastCalledWith("saved", 3);
    expect(screen.getByLabelText("当前筛选")).toHaveTextContent("临时搜索");
    expect(onApply).not.toHaveBeenCalled();
  });

  it("allows read-only list refresh and retry but no metadata writes", async () => {
    const list = vi
      .spyOn(desktopApi, "listApplicationViews")
      .mockRejectedValueOnce(new Error("读取失败"))
      .mockResolvedValueOnce([]);
    show(false);
    for (const name of [
      "保存视图",
      "更新当前视图",
      "重命名视图",
      "复制视图",
      "删除视图",
      "设为默认视图",
    ])
      expect(screen.getByRole("button", { name })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "刷新视图列表" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("读取失败");
    expect(screen.getByLabelText("已保存视图")).toHaveValue("saved");
    fireEvent.click(screen.getByRole("button", { name: "刷新视图列表" }));
    await waitFor(() =>
      expect(screen.getByLabelText("已保存视图")).toHaveValue(""),
    );
    expect(list).toHaveBeenCalledTimes(2);
    expect(screen.getByLabelText("当前筛选")).toHaveTextContent("临时搜索");
  });
});
