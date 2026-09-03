import { useState } from "react";
import type { SavedView, SavedViewChange, ViewSnapshot } from "../../contracts";
import { desktopApi } from "../../lib/tauri";
import { useDraftGuard, useDraftState } from "../../shared/draftGuard";
import { Modal } from "../../shared/Modal";

type Editor = {
  mode: "create" | "update" | "rename" | "copy";
  view: SavedView | null;
  snapshot: ViewSnapshot;
};
const titles = {
  create: "保存当前视图",
  update: "更新当前视图",
  rename: "重命名视图",
  copy: "复制已保存视图",
};

export function ViewControls({
  views,
  activeId,
  current,
  writable,
  disabled,
  onSelect,
  onViews,
  onApply,
  onError,
}: {
  views: SavedView[];
  activeId: string;
  current: ViewSnapshot;
  writable: boolean;
  disabled: boolean;
  onSelect: (id: string) => void;
  onViews: (views: SavedView[]) => void;
  onApply: (view: SavedView) => void;
  onError: (error: unknown) => void;
}) {
  const [editor, setEditor] = useState<Editor | null>(null);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState("");
  const [message, setMessage] = useState("");
  const { confirm, confirmLeave } = useDraftGuard();
  useDraftState(false, busy, "视图操作");
  const selected = views.find((view) => view.id === activeId);
  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    setFailure("");
    setMessage("");
    try {
      await action();
    } catch (error) {
      setFailure(
        error instanceof Error ? error.message : "视图操作失败，请重试。",
      );
      onError(error);
    } finally {
      setBusy(false);
    }
  };
  const open = (mode: Editor["mode"]) => {
    setFailure("");
    setEditor({ mode, view: selected ?? null, snapshot: current });
  };
  const cancel = async () => {
    if (await confirmLeave()) {
      setEditor(null);
      setFailure("");
    }
  };
  const changed = (result: SavedViewChange, apply: boolean) => {
    onViews(result.views);
    if (apply) onApply(result.view);
    setEditor(null);
    setMessage("视图配置已保存，投递数据和文件未改变。");
  };
  const remove = async () => {
    if (
      !selected ||
      !(await confirm({
        title: "删除视图",
        message: `仅删除“${selected.name}”的显示配置，不删除任何投递或简历。当前表格保留为临时视图；若删除默认视图，下次打开使用基础布局。`,
        confirmLabel: "删除视图",
        destructive: true,
      }))
    )
      return;
    await run(async () => {
      onViews(
        await desktopApi.deleteApplicationView(selected.id, selected.revision),
      );
      setMessage("已删除视图配置；投递数据和文件未改变。");
    });
  };
  return (
    <>
      <select
        aria-label="已保存视图"
        value={activeId}
        disabled={disabled || busy}
        onChange={(event) => onSelect(event.target.value)}
      >
        <option value="">临时视图（未保存）</option>
        {views.map((view) => (
          <option key={view.id} value={view.id}>
            {view.name}
            {view.isDefault ? "（默认）" : ""}
          </option>
        ))}
      </select>
      <button
        type="button"
        disabled={!writable || disabled || busy}
        onClick={() => open("create")}
      >
        保存视图
      </button>
      {selected && (
        <>
          <button
            type="button"
            disabled={!writable || disabled || busy}
            onClick={() => open("update")}
          >
            更新当前视图
          </button>
          <button
            type="button"
            disabled={!writable || disabled || busy}
            onClick={() => open("rename")}
          >
            重命名视图
          </button>
          <button
            type="button"
            disabled={!writable || disabled || busy}
            onClick={() => open("copy")}
          >
            复制视图
          </button>
          <button
            type="button"
            disabled={!writable || disabled || busy}
            onClick={() => void remove()}
          >
            删除视图
          </button>
          <button
            type="button"
            disabled={!writable || disabled || busy || selected.isDefault}
            onClick={() =>
              void run(async () =>
                changed(
                  await desktopApi.updateViewMetadata({
                    id: selected.id,
                    revision: selected.revision,
                    name: selected.name,
                    isDefault: true,
                  }),
                  false,
                ),
              )
            }
          >
            设为默认视图
          </button>
        </>
      )}
      <button
        type="button"
        disabled={disabled || busy}
        onClick={() =>
          void run(async () => {
            onViews(await desktopApi.listApplicationViews());
            setMessage("视图列表已刷新，当前表格布局未改变。");
          })
        }
      >
        刷新视图列表
      </button>
      {failure && !editor && <p role="alert">{failure}</p>}
      {message && <p role="status">{message}</p>}
      {editor && (
        <ViewEditor
          editor={editor}
          busy={busy}
          failure={failure}
          onCancel={() => void cancel()}
          onSave={(name, isDefault) =>
            run(async () => {
              const view = editor.view;
              if (editor.mode === "copy" && view)
                changed(
                  await desktopApi.duplicateApplicationView(
                    view.id,
                    view.revision,
                    name,
                  ),
                  false,
                );
              else if (editor.mode === "rename" && view)
                changed(
                  await desktopApi.updateViewMetadata({
                    id: view.id,
                    revision: view.revision,
                    name,
                    isDefault,
                  }),
                  false,
                );
              else
                changed(
                  await desktopApi.saveApplicationView({
                    ...editor.snapshot,
                    name,
                    isDefault,
                    id: editor.mode === "update" ? view!.id : null,
                    revision: editor.mode === "update" ? view!.revision : null,
                  }),
                  true,
                );
            })
          }
        />
      )}
    </>
  );
}

function ViewEditor({
  editor,
  busy,
  failure,
  onSave,
  onCancel,
}: {
  editor: Editor;
  busy: boolean;
  failure: string;
  onSave: (name: string, isDefault: boolean) => Promise<void>;
  onCancel: () => void;
}) {
  const initialName =
    editor.mode === "create"
      ? ""
      : editor.mode === "copy"
        ? `${editor.view!.name}（副本）`
        : editor.view!.name;
  const initialDefault =
    editor.mode === "create" ||
    (editor.mode !== "copy" && !!editor.view?.isDefault);
  const [name, setName] = useState(initialName);
  const [isDefault, setDefault] = useState(initialDefault);
  useDraftState(
    name !== initialName || isDefault !== initialDefault,
    busy,
    "视图编辑",
  );
  return (
    <Modal title={titles[editor.mode]} onCancel={onCancel}>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (name.trim() && !busy) void onSave(name.trim(), isDefault);
        }}
      >
        {failure && (
          <p role="alert">
            {failure}{" "}
            未保存输入仍保留；遇到版本冲突可先复制输入，再取消并刷新视图列表。
          </p>
        )}
        <fieldset className="editor-fields" disabled={busy}>
          <label>
            视图名称
            <input
              autoFocus
              required
              maxLength={200}
              value={name}
              onChange={(event) => setName(event.target.value)}
            />
          </label>
          {editor.mode !== "copy" && (
            <label className="checkbox-label">
              <input
                type="checkbox"
                checked={isDefault}
                onChange={(event) => setDefault(event.target.checked)}
              />
              重启后默认使用此视图
            </label>
          )}
          <p>
            {editor.mode === "copy"
              ? "复制已保存的布局、排序、筛选和分组，不包含当前临时调整，不自动设为默认或切换当前表格。"
              : editor.mode === "rename"
                ? "只改视图名称和默认标记，不覆盖已保存的布局。"
                : editor.mode === "update"
                  ? "保存后以当前布局、排序、筛选和分组覆盖此视图的配置；不更改投递记录和文件。"
                  : "保存当前布局、排序、筛选和分组，不复制投递记录或文件。"}
          </p>
          <div className="modal-actions">
            <button type="button" onClick={onCancel}>
              取消
            </button>
            <button type="submit" className="primary" disabled={!name.trim()}>
              保存
            </button>
          </div>
        </fieldset>
      </form>
    </Modal>
  );
}
