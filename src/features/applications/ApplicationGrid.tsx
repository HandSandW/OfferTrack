import { useRef, useState, type ReactNode } from "react";
import type {
  ApplicationListItem,
  ColumnSetting,
  FieldDefinition,
  SortRule,
} from "../../contracts";
import type { BatchTarget } from "../batch/contracts";
import { desktopApi } from "../../lib/tauri";
import { useDraftState } from "../../shared/draftGuard";
import { CellEditor } from "./CellEditor";
import {
  canEditCell,
  cellValue,
  moveCell,
  type CellAddress,
  type CellRequest,
} from "./cellModel";
import { pinnedOffsets, rawValue } from "./tableModel";

interface Props {
  grouped: [string, ApplicationListItem[]][];
  grouping: boolean;
  columns: ColumnSetting[];
  labels: Record<string, string>;
  sort: SortRule[];
  fields: FieldDefinition[];
  selectedId: string | null;
  checked: BatchTarget[];
  writable: boolean;
  disabled: boolean;
  empty: boolean;
  onSort: (key: string) => void;
  onOpen: (id: string) => void;
  onBeforeEdit: () => Promise<boolean>;
  onUpdated: (record: ApplicationListItem) => void;
  onChecked: (targets: BatchTarget[]) => void;
  onError: (error: unknown) => void;
  renderCell: (record: ApplicationListItem, column: ColumnSetting) => ReactNode;
}
interface Editing {
  record: ApplicationListItem;
  key: string;
  initialText?: string;
}

export function ApplicationGrid(props: Props) {
  const {
    grouped,
    columns,
    labels,
    fields,
    writable,
    disabled,
    selectedId,
    checked,
    onSort,
    onOpen,
    onBeforeEdit,
    onUpdated,
    onChecked,
    onError,
    renderCell,
  } = props;
  const [address, setAddress] = useState<CellAddress | null>(null);
  const [editing, setEditing] = useState<Editing | null>(null);
  const [undo, setUndo] = useState<CellRequest | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const lock = useRef(false);
  const root = useRef<HTMLDivElement>(null);
  const cells = useRef(new Map<string, HTMLTableCellElement>());
  const offsets = pinnedOffsets(columns);
  const ids = grouped.flatMap(([, rows]) => rows.map((row) => row.id));
  const keys = columns.map((c) => c.key);
  const active =
    address && ids.includes(address.id) && keys.includes(address.key)
      ? address
      : ids[0] && keys[0]
        ? { id: ids[0], key: keys[0] }
        : null;
  useDraftState(false, busy, "表格写入");
  const token = (a: CellAddress) => JSON.stringify([a.id, a.key]);
  const focus = (a: CellAddress | null) => {
    if (!a) return;
    setAddress(a);
    // Modal unmount and row re-sorting happen before restoring focus by stable identity.
    requestAnimationFrame(() => {
      const target = cells.current.get(token(a));
      if (target) target.focus();
      else {
        root.current?.focus();
        setMessage("保存成功；目标已离开当前筛选/分页，请在当前表格重新定位。");
      }
    });
  };
  const begin = async (
    record: ApplicationListItem,
    key: string,
    initialText?: string,
  ) => {
    if (disabled || lock.current || !writable || !canEditCell(key, fields))
      return;
    lock.current = true;
    try {
      if (!(await onBeforeEdit())) return;
      setError("");
      setEditing(
        initialText === undefined
          ? { record, key }
          : { record, key, initialText },
      );
    } finally {
      lock.current = false;
    }
  };
  const save = async (value: unknown, step: number) => {
    if (!editing || lock.current || !writable || disabled) return;
    lock.current = true;
    setBusy(true);
    setError("");
    const origin = { id: editing.record.id, key: editing.key };
    const destination = step
      ? (moveCell(origin, ids, keys, "Tab", step < 0) ?? origin)
      : origin;
    try {
      const applied = await desktopApi.editApplicationCell({
        ...origin,
        revision: editing.record.revision,
        version: 1,
        value,
      });
      if (applied.changed)
        setUndo({
          ...origin,
          version: 1,
          revision: applied.record.revision,
          value: applied.previousValue,
        });
      onUpdated(applied.record);
      setEditing(null);
      setMessage(
        applied.changed
          ? "单元格已保存，可撤销本次修改。"
          : "内容未变化，未写入。",
      );
      focus(destination);
    } catch (err) {
      setError(err instanceof Error ? err.message : "保存失败，请重试。");
    } finally {
      lock.current = false;
      setBusy(false);
    }
  };
  const undoLast = async () => {
    if (!undo || editing || lock.current || disabled || !writable) return;
    lock.current = true;
    try {
      if (!(await onBeforeEdit())) return;
      setBusy(true);
      setError("");
      const applied = await desktopApi.editApplicationCell(undo);
      onUpdated(applied.record);
      setUndo(null);
      setMessage("已撤销最近一次单格保存；其他资料与文件未回退。");
      focus(undo);
    } catch (err) {
      setError(err instanceof Error ? err.message : "撤销失败，请重试。");
    } finally {
      lock.current = false;
      setBusy(false);
    }
  };
  return (
    <div
      ref={root}
      className="application-grid"
      tabIndex={-1}
      aria-label="投递表格区域"
    >
      <div className="grid-instructions">
        <span>
          方向键定位 · Enter/F2 编辑 · Shift+Enter 详情 · Ctrl+C 复制 · Ctrl+V
          单格粘贴 · Ctrl+Z 撤销
        </span>
        <button
          type="button"
          disabled={!undo || busy || disabled || !writable || !!editing}
          onClick={() => void undoLast()}
        >
          撤销单格修改
        </button>
      </div>
      <p role="status" className="grid-feedback">
        {message}
      </p>
      {!editing && error && (
        <p role="alert">
          {error} 撤销未完成；不会覆盖后续修改。可刷新后在详情中手动处理。
        </p>
      )}
      <div className="table-scroll">
        {grouped.map(([name, records]) => (
          <div className="record-group" key={name || "all"}>
            {props.grouping && (
              <h3>
                {name} <small>{records.length}</small>
              </h3>
            )}
            <table
              className="application-table"
              aria-label={name ? `投递记录 · ${name}` : "投递记录"}
            >
              <thead>
                <tr>
                  {columns.map((column) => (
                    <th
                      key={column.key}
                      tabIndex={0}
                      aria-sort={
                        props.sort[0]?.key === column.key
                          ? props.sort[0].direction === "asc"
                            ? "ascending"
                            : "descending"
                          : "none"
                      }
                      onClick={() => onSort(column.key)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter" || e.key === " ") {
                          e.preventDefault();
                          onSort(column.key);
                        }
                      }}
                      style={{
                        minWidth: column.width,
                        width: column.width,
                        ...(column.pinned
                          ? {
                              position: "sticky",
                              left: offsets.get(column.key),
                              zIndex: 2,
                            }
                          : {}),
                      }}
                    >
                      {labels[column.key]}{" "}
                      {props.sort[0]?.key === column.key
                        ? props.sort[0].direction === "asc"
                          ? "↑"
                          : "↓"
                        : ""}
                    </th>
                  ))}
                  <th>批量选择</th>
                </tr>
              </thead>
              <tbody>
                {records.map((record) => (
                  <tr
                    key={record.id}
                    className={selectedId === record.id ? "selected" : ""}
                    onClick={() => onOpen(record.id)}
                  >
                    {columns.map((column) => {
                      const here = { id: record.id, key: column.key };
                      return (
                        <td
                          key={column.key}
                          ref={(node) => {
                            if (node) cells.current.set(token(here), node);
                            else cells.current.delete(token(here));
                          }}
                          tabIndex={
                            active?.id === record.id &&
                            active.key === column.key
                              ? 0
                              : -1
                          }
                          aria-label={`${labels[column.key]} · ${record.companyName} · ${record.positionName}`}
                          onFocus={(e) => {
                            if (e.target === e.currentTarget) setAddress(here);
                          }}
                          onDoubleClick={(e) => {
                            if (
                              !(e.target instanceof HTMLElement) ||
                              e.target.closest("a, button, input")
                            )
                              return;
                            e.stopPropagation();
                            void begin(record, column.key);
                          }}
                          onCopy={(e) => {
                            if (
                              e.target !== e.currentTarget ||
                              window.getSelection()?.toString()
                            )
                              return;
                            e.preventDefault();
                            const value = cellValue(record, column.key);
                            e.clipboardData.setData(
                              "text/plain",
                              column.key === "tags"
                                ? (value as string[]).join(", ")
                                : String(rawValue(record, column.key) ?? ""),
                            );
                            setMessage("已复制单元格完整内容。");
                          }}
                          onPaste={(e) => {
                            if (e.target !== e.currentTarget) return;
                            e.preventDefault();
                            if (!writable || !canEditCell(column.key, fields)) {
                              setMessage("此单元格不可直接编辑，请打开详情。");
                              return;
                            }
                            const text = e.clipboardData.getData("text/plain");
                            if (
                              text.length > 100_000 ||
                              text.includes("\t") ||
                              ((text.includes("\n") || text.includes("\r")) &&
                                !["notes", "positionDescription"].includes(
                                  column.key,
                                ) &&
                                fields.find(
                                  (f) => `custom:${f.id}` === column.key,
                                )?.fieldType !== "text")
                            ) {
                              setMessage(
                                "仅支持单格粘贴（长文本允许换行），不自动拆分多行多列。",
                              );
                              return;
                            }
                            if (
                              column.key === "companyType" ||
                              fields.some(
                                (f) =>
                                  `custom:${f.id}` === column.key &&
                                  f.fieldType !== "text" &&
                                  f.fieldType !== "url",
                              )
                            ) {
                              setMessage(
                                "日期/数字/选项等类型请按 Enter，在对应控件中输入或粘贴。",
                              );
                              return;
                            }
                            void begin(record, column.key, text);
                          }}
                          onKeyDown={(e) => {
                            if (
                              e.target !== e.currentTarget ||
                              e.nativeEvent.isComposing
                            )
                              return;
                            if (
                              (e.ctrlKey || e.metaKey) &&
                              e.key.toLowerCase() === "z" &&
                              !e.shiftKey
                            ) {
                              e.preventDefault();
                              void undoLast();
                              return;
                            }
                            if (e.ctrlKey || e.metaKey || e.altKey) return;
                            if (e.key === "Enter" || e.key === "F2") {
                              e.preventDefault();
                              if (
                                e.shiftKey ||
                                !canEditCell(column.key, fields) ||
                                !writable
                              )
                                onOpen(record.id);
                              else void begin(record, column.key);
                              return;
                            }
                            const next = moveCell(
                              here,
                              ids,
                              keys,
                              e.key,
                              e.shiftKey,
                            );
                            if (next) {
                              e.preventDefault();
                              focus(next);
                            }
                          }}
                          style={{
                            maxWidth: column.width,
                            minWidth: column.width,
                            ...(column.pinned
                              ? {
                                  position: "sticky",
                                  left: offsets.get(column.key),
                                }
                              : {}),
                          }}
                        >
                          {renderCell(record, column)}
                        </td>
                      );
                    })}
                    <td onClick={(e) => e.stopPropagation()}>
                      <input
                        type="checkbox"
                        aria-label={`选择投递 ${record.companyName} ${record.positionName}`}
                        disabled={disabled || busy}
                        checked={checked.some((r) => r.id === record.id)}
                        onChange={(e) => {
                          if (!e.target.checked)
                            onChecked(
                              checked.filter((r) => r.id !== record.id),
                            );
                          else if (checked.length < 200)
                            onChecked([
                              ...checked,
                              { id: record.id, revision: record.revision },
                            ]);
                          else
                            onError(
                              new Error("每批最多 200 条，请先清除部分选择。"),
                            );
                        }}
                      />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            {props.empty && !records.length && (
              <div className="table-empty">没有符合当前条件的投递记录。</div>
            )}
          </div>
        ))}
      </div>
      {editing && (
        <CellEditor
          record={editing.record}
          columnKey={editing.key}
          label={labels[editing.key] ?? editing.key}
          field={fields.find((f) => `custom:${f.id}` === editing.key)}
          initialText={editing.initialText}
          busy={busy}
          error={error}
          onSave={(value, step) => void save(value, step)}
          onCancel={() => {
            if (lock.current) return;
            const previous = { id: editing.record.id, key: editing.key };
            setEditing(null);
            setError("");
            focus(previous);
          }}
        />
      )}
    </div>
  );
}
