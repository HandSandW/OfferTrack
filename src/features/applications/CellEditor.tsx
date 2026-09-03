import { useEffect, useRef, useState } from "react";
import type { ApplicationListItem, FieldDefinition } from "../../contracts";
import { Modal } from "../../shared/Modal";
import { useDraftState } from "../../shared/draftGuard";
import { CustomFieldInput } from "./CustomFieldInput";
import { companyTypes } from "./tableModel";
import { editValue, saveValue } from "./cellModel";

export function CellEditor({
  record,
  columnKey,
  label,
  field,
  initialText,
  busy,
  error,
  onSave,
  onCancel,
}: {
  record: ApplicationListItem;
  columnKey: string;
  label: string;
  field: FieldDefinition | undefined;
  initialText: string | undefined;
  busy: boolean;
  error: string;
  onSave: (value: unknown, step: number) => void;
  onCancel: () => void;
}) {
  const original = editValue(record, columnKey);
  const [value, setValue] = useState<unknown>(initialText ?? original);
  const container = useRef<HTMLDivElement>(null);
  useEffect(() => {
    container.current
      ?.querySelector<HTMLElement>("input, textarea, select")
      ?.focus();
  }, []);
  useDraftState(
    JSON.stringify(value) !== JSON.stringify(original),
    busy,
    "表格单格编辑",
  );
  const cancel = () => {
    if (!busy) onCancel();
  };
  return (
    <Modal title={`编辑单元格 · ${label}`} onCancel={cancel}>
      <p>
        {record.companyName} · {record.positionName}
      </p>
      <p className="muted">
        Enter 保存，长文本 Ctrl+Enter 保存；Tab / Shift+Tab
        保存后移到相邻单元格。Escape
        放弃本格草稿。支持输入框原生复制、粘贴和撤销。
      </p>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          if (!busy) onSave(saveValue(columnKey, value), 0);
        }}
      >
        <div
          ref={container}
          onKeyDown={(e) => {
            if (e.nativeEvent.isComposing || e.keyCode === 229) return;
            if (
              e.key === "Tab" &&
              e.target instanceof HTMLElement &&
              e.target.matches("input, textarea, select") &&
              !e.altKey &&
              !e.ctrlKey &&
              !e.metaKey
            ) {
              e.preventDefault();
              if (!busy)
                onSave(saveValue(columnKey, value), e.shiftKey ? -1 : 1);
            }
            if (
              e.key === "Enter" &&
              e.target instanceof HTMLTextAreaElement &&
              (e.ctrlKey || e.metaKey)
            ) {
              e.preventDefault();
              if (!busy) onSave(saveValue(columnKey, value), 0);
            }
            if (
              e.key === "Enter" &&
              e.target instanceof HTMLSelectElement &&
              !e.altKey &&
              !e.ctrlKey &&
              !e.metaKey
            ) {
              e.preventDefault();
              if (!busy) onSave(saveValue(columnKey, value), 0);
            }
          }}
        >
          {field?.fieldType === "text" ? (
            <label>
              {label}
              <textarea
                rows={4}
                value={typeof value === "string" ? value : ""}
                disabled={busy}
                onChange={(e) => setValue(e.target.value)}
              />
            </label>
          ) : field ? (
            <CustomFieldInput
              field={field}
              value={value}
              disabled={busy}
              onChange={setValue}
            />
          ) : (
            <label>
              {label}
              {columnKey === "companyType" ? (
                <select
                  value={String(value)}
                  disabled={busy}
                  onChange={(e) => setValue(e.target.value)}
                >
                  {companyTypes.map(([id, name]) => (
                    <option key={id} value={id}>
                      {name}
                    </option>
                  ))}
                </select>
              ) : columnKey === "notes" ||
                columnKey === "positionDescription" ? (
                <textarea
                  rows={6}
                  value={String(value)}
                  disabled={busy}
                  onChange={(e) => setValue(e.target.value)}
                />
              ) : (
                <input
                  type={
                    columnKey === "applicationDate"
                      ? "date"
                      : columnKey.endsWith("Url")
                        ? "url"
                        : "text"
                  }
                  value={String(value)}
                  disabled={busy}
                  onChange={(e) => setValue(e.target.value)}
                />
              )}
            </label>
          )}
        </div>
        {columnKey === "tags" && (
          <p className="muted">
            使用中文或英文逗号分隔；清空将解除本记录的标签关联。
          </p>
        )}
        {(columnKey === "companyName" || columnKey === "positionName") && (
          <p className="muted">
            此处只保存资料；文件夹暂不移动，可在详情“文件”中重试规范化。
          </p>
        )}
        {error && (
          <p role="alert">
            {error} 输入仍保留；版本冲突请先复制草稿，再取消并刷新记录。
          </p>
        )}
        <div className="actions">
          <button type="submit" className="primary" disabled={busy}>
            {busy ? "正在保存…" : "保存单元格"}
          </button>
          <button type="button" disabled={busy} onClick={cancel}>
            放弃本格修改
          </button>
        </div>
      </form>
    </Modal>
  );
}
