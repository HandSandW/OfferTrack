import { useState } from "react";
import { desktopApi } from "../../lib/tauri";
import { selectDirectory } from "../../lib/dialog";
import { useDraftGuard, useDraftState } from "../../shared/draftGuard";
import type { DatabaseRestore, ExternalDatabasePreview } from "./contracts";

export function ExternalDatabasePanel({
  disabled = false,
  onError,
  onOpen,
}: {
  disabled?: boolean;
  onError: (error: unknown) => void;
  onOpen: (path: string) => Promise<void>;
}) {
  const [selected, setSelected] = useState<{
    directory: string;
    preview: ExternalDatabasePreview;
  } | null>(null);
  const [restored, setRestored] = useState<DatabaseRestore | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const { confirm } = useDraftGuard();
  useDraftState(false, busy, "独立数据库恢复");
  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    setError("");
    try {
      await action();
    } catch (failure) {
      setError(
        failure instanceof Error
          ? failure.message
          : "未完成，请重试。原数据及成功恢复的副本保留。",
      );
      onError(failure);
    } finally {
      setBusy(false);
    }
  };
  const select = () =>
    run(async () => {
      const directory = await selectDirectory(
        "选择数据库快照目录（内含 manifest.json 和 database.sqlite）",
      );
      if (!directory) return;
      setSelected(null);
      const preview = await desktopApi.previewExternalDatabaseBackup(directory);
      setSelected({ directory, preview });
    });
  const restore = () =>
    run(async () => {
      if (!selected) return;
      const parent =
        await selectDirectory("选择独立数据库恢复位置（新建子目录）");
      if (
        !parent ||
        !(await confirm({
          title: "从独立数据库快照恢复",
          message: `已校验 ${selected.preview.applicationCount} 条投递、${selected.preview.documentCount} 个附件索引。将在 ${parent} 下创建新仓库，不覆盖或切换当前仓库。此快照没有简历正文，文件需要另行找回。继续吗？`,
          confirmLabel: "创建数据库恢复副本",
        }))
      )
        return;
      setRestored(
        await desktopApi.restoreExternalDatabaseBackup(
          selected.directory,
          parent,
          selected.preview.fingerprint,
        ),
      );
    });
  return (
    <article className="panel-page">
      <h2>独立数据库快照恢复</h2>
      <p>
        原仓库打不开时也可使用。选择 backups/database
        下某个快照目录，或手动复制到其他位置的同格式目录：必须同时包含
        manifest.json 和 database.sqlite。不是导入到当前仓库，也不接受孤立的
        SQLite 文件。
      </p>
      <p className="muted">
        只恢复元信息与附件索引，不含 PDF/Word
        正文；源快照保持不动。校验版本、SHA-256、SQLite
        完整性及未完成操作，恢复到全新目录并生成新仓库 ID。
      </p>
      <button disabled={busy || disabled} onClick={() => void select()}>
        选择并校验数据库快照目录
      </button>
      {selected && (
        <section aria-label="独立快照预览">
          <p>{selected.directory}</p>
          <p>
            源仓库 {selected.preview.backup.warehouseId} · 数据库 v
            {selected.preview.backup.schemaVersion} ·{" "}
            {selected.preview.applicationCount} 条投递 ·{" "}
            {selected.preview.documentCount} 个附件索引
          </p>
          <button disabled={busy || disabled} onClick={() => void restore()}>
            恢复独立数据库快照…
          </button>
        </section>
      )}
      {restored && (
        <section aria-label="独立快照恢复结果">
          <p role="status">
            数据库恢复完成：{restored.directory}
            。当前仓库未切换；没有恢复简历文件。
          </p>
          <button
            disabled={busy || disabled}
            onClick={() => void onOpen(restored.directory).catch(onError)}
          >
            打开数据库恢复副本
          </button>
        </section>
      )}
      {busy && <p role="status">正在校验或恢复，请勿移动文件或关闭应用…</p>}
      {error && <p role="alert">{error}</p>}
    </article>
  );
}
