import { useEffect, useState } from "react";
import type { ColumnSetting } from "../../contracts";
import type { BatchTarget } from "../batch/contracts";
import { desktopApi } from "../../lib/tauri";
import { selectDirectory } from "../../lib/dialog";
import { Modal } from "../../shared/Modal";
import { useDraftState } from "../../shared/draftGuard";
import type { ExportCatalog, ExportCreated, ExportRequest } from "./contracts";

export function ExportDialog({
  partition,
  filtered,
  selected,
  columns,
  onClose,
}: {
  partition: "active" | "archived";
  filtered: BatchTarget[];
  selected: BatchTarget[];
  columns: ColumnSetting[];
  onClose: () => void;
}) {
  const [catalog, setCatalog] = useState<ExportCatalog | null>(null);
  const [attempt, setAttempt] = useState(0);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [mode, setMode] = useState<"all" | "filtered" | "selected">("filtered");
  const [format, setFormat] = useState<"xlsx" | "csv">("xlsx");
  const [keys, setKeys] = useState(
    columns.filter((c) => c.visible).map((c) => c.key),
  );
  const [created, setCreated] = useState<ExportCreated | null>(null);
  useDraftState(false, busy, "导出投递");
  useEffect(() => {
    let active = true;
    void desktopApi
      .getExportCatalog()
      .then((value) => {
        if (active) setCatalog(value);
      })
      .catch((reason: unknown) => {
        if (active)
          setError(
            reason instanceof Error
              ? reason.message
              : "无法读取导出字段，请重试。",
          );
      });
    return () => {
      active = false;
    };
  }, [attempt]);
  const orderedKeys = [
    ...columns.map((c) => c.key),
    ...(catalog?.columns.map((c) => c.key) ?? []),
  ];
  const available = [...new Set(orderedKeys)].flatMap(
    (key) => catalog?.columns.filter((c) => c.key === key) ?? [],
  );
  const picked = available
    .filter((c) => keys.includes(c.key))
    .map((c) => c.key);
  const count =
    mode === "all"
      ? (catalog?.total ?? 0)
      : mode === "filtered"
        ? filtered.length
        : selected.length;
  const run = async () => {
    if (busy || !catalog) return;
    setBusy(true);
    setError("");
    setCreated(null);
    try {
      const parent = await selectDirectory(
        "选择导出保存位置（仓库外，将创建新文件夹）",
      );
      if (!parent) return;
      const request: ExportRequest = {
        version: 1,
        format,
        columns: picked,
        scope:
          mode === "all"
            ? { kind: "all" }
            : {
                kind: "records",
                partition,
                targets: mode === "filtered" ? filtered : selected,
              },
      };
      setCreated(await desktopApi.exportApplications(parent, request));
    } catch (reason) {
      setError(
        reason instanceof Error
          ? reason.message
          : "导出失败，原数据未修改，请重试。",
      );
    } finally {
      setBusy(false);
    }
  };
  return (
    <Modal
      title="导出投递记录"
      className="export-modal"
      focusTitle
      onCancel={() => {
        if (!busy) onClose();
      }}
    >
      <p className="muted">
        只读导出，不修改投递，不复制简历正文，也不替代完整备份。导出文件与
        fields.json 映射保存在同一个新文件夹，不覆盖已有文件。
      </p>
      {!catalog && !error && <p role="status">正在读取导出字段…</p>}
      {error && <p role="alert">{error}</p>}
      {!catalog && error && (
        <button
          type="button"
          onClick={() => {
            setError("");
            setAttempt((n) => n + 1);
          }}
        >
          重试读取导出字段
        </button>
      )}
      <fieldset className="task-form" disabled={busy || !catalog}>
        <label>
          导出范围
          <select
            value={mode}
            onChange={(e) => setMode(e.target.value as typeof mode)}
          >
            <option value="filtered">
              当前筛选（整个结果，非仅本页） · {filtered.length} 条
            </option>
            <option value="selected">
              选中记录（含筛选外选择） · {selected.length} 条
            </option>
            <option value="all">
              全部记录（包含归档，不含回收站） · {catalog?.total ?? "…"} 条
            </option>
          </select>
        </label>
        <p>
          当前分区：{partition === "active" ? "投递记录" : "已归档"}
          。筛选/选中范围在打开窗口时固定，数据版本变化时拒绝导出，请关闭窗口刷新后重试。全部范围在执行时读取，按创建时间倒序。
        </p>
        <label>
          文件格式
          <select
            value={format}
            onChange={(e) => setFormat(e.target.value as typeof format)}
          >
            <option value="xlsx">Excel 工作簿（.xlsx）</option>
            <option value="csv">CSV（UTF-8 BOM）</option>
          </select>
        </label>
        <p className="muted">
          时间戳保留 UTC 精度；单元格按文本保存。CSV
          危险前缀会加单引号以避免公式执行，Excel
          自动识别数字/日期仍可能改变显示。XLSX 超长单元格会报错，请改用 CSV。
        </p>
        <div className="section-actions">
          <button
            type="button"
            onClick={() =>
              setKeys(columns.filter((c) => c.visible).map((c) => c.key))
            }
          >
            按当前可见列
          </button>
          <button
            type="button"
            onClick={() =>
              setKeys(
                available
                  .filter((c) => c.key !== "documentPaths")
                  .map((c) => c.key),
              )
            }
          >
            全选字段（不含绝对路径）
          </button>
          <button type="button" onClick={() => setKeys([])}>
            清除字段选择
          </button>
        </div>
        <div className="export-columns">
          {available.map((c) => (
            <label key={c.key}>
              <input
                type="checkbox"
                checked={keys.includes(c.key)}
                onChange={(e) =>
                  setKeys(
                    e.target.checked
                      ? [...keys, c.key]
                      : keys.filter((k) => k !== c.key),
                  )
                }
              />
              {c.label}
            </label>
          ))}
        </div>
        <p className="muted">
          “全选字段”包含隐藏列和自定义字段，但不会自动选择绝对路径；如确有需要，请单独勾选对应字段。
        </p>
        {keys.includes("documentPaths") && (
          <p role="status">
            绝对路径会暴露本机目录、用户名等信息；只含最近索引与缺失标记，不验证文件当前可读性。分享前请检查。
          </p>
        )}
      </fieldset>
      {created && (
        <div role="status" className="export-result">
          <p>已导出 {created.rowCount} 条。</p>
          <p>表格：{created.path}</p>
          <p>字段映射：{created.mappingPath}</p>
        </div>
      )}
      <div className="section-actions">
        <button
          className="primary"
          type="button"
          disabled={
            busy ||
            !catalog ||
            !picked.length ||
            (mode === "selected" && !count)
          }
          onClick={() => void run()}
        >
          {busy ? "正在导出…" : `选择位置并导出 ${count} 条`}
        </button>
        <button type="button" disabled={busy} onClick={onClose}>
          关闭
        </button>
      </div>
    </Modal>
  );
}
