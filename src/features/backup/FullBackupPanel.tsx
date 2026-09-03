import { useState } from "react";
import { desktopApi } from "../../lib/tauri";
import { selectDirectory, selectFullBackup } from "../../lib/dialog";
import { useDraftGuard, useDraftState } from "../../shared/draftGuard";
import type { FullBackupPreview, FullRestore } from "./contracts";

export function FullBackupPanel({
  writable,
  disabled = false,
  onError,
  onOpen,
}: {
  writable: boolean;
  disabled?: boolean;
  onError: (error: unknown) => void;
  onOpen: (path: string) => Promise<void>;
}) {
  const [includeTrash, setIncludeTrash] = useState(true);
  const [selected, setSelected] = useState<{
    path: string;
    preview: FullBackupPreview;
  } | null>(null);
  const [restored, setRestored] = useState<FullRestore | null>(null);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const { confirm } = useDraftGuard();
  useDraftState(false, busy, "完整备份与迁移");
  const blocked = disabled || busy;
  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    setError("");
    setMessage("");
    try {
      await action();
    } catch (failure) {
      setError(
        failure instanceof Error ? failure.message : "操作未完成，请重试。",
      );
      onError(failure);
    } finally {
      setBusy(false);
    }
  };
  const create = () =>
    run(async () => {
      const parent = await selectDirectory(
        "选择完整备份保存目录（不覆盖已有文件）",
      );
      if (!parent) return;
      const result = await desktopApi.createFullBackup(parent, includeTrash);
      setMessage(
        `完整备份已生成并校验：${result.path}。请妥善保管，包内包含私人资料。`,
      );
    });
  const select = () =>
    run(async () => {
      const path = await selectFullBackup();
      if (!path) return;
      setSelected(null);
      const preview = await desktopApi.previewFullBackup(path);
      setSelected({ path, preview });
    });
  const restore = () =>
    run(async () => {
      if (!selected) return;
      const parent = await selectDirectory("选择恢复位置（创建全新子目录）");
      if (
        !parent ||
        !(await confirm({
          title: "恢复完整备份",
          message: `将恢复 ${selected.preview.fileCount} 个文件到 ${parent} 下的新目录，并验证数据库。${selected.preview.includesRecycleBin ? "包含回收站。" : "不包含回收站文件，已删除记录的元信息仍保留。"}不覆盖当前仓库，完成后可确认切换。`,
          confirmLabel: "恢复到新目录",
        }))
      )
        return;
      const result = await desktopApi.restoreFullBackup(
        selected.path,
        parent,
        selected.preview.sha256,
      );
      setRestored(result);
      setMessage("完整恢复与数据库校验已完成，当前仓库尚未切换。");
    });
  const migrate = () =>
    run(async () => {
      const parent = await selectDirectory("选择迁移位置（原仓库保留）");
      if (
        !parent ||
        !(await confirm({
          title: "复制并验证迁移",
          message: `将在 ${parent} 下生成完整备份包和新的仓库副本，包含简历、子目录、历史备份及回收站。需要两份副本的空间；原仓库不删除。完成后点击切换。`,
          confirmLabel: "开始迁移复制",
        }))
      )
        return;
      const result = await desktopApi.migrateWarehouse(parent);
      setRestored(result);
      setMessage(
        "迁移副本与完整备份已校验完成，原仓库保留，当前仓库尚未切换。",
      );
    });
  return (
    <article className="panel-page">
      <h2>完整备份与迁移</h2>
      <p>
        包含已保存的数据、简历及子目录。未连接仓库时也能从完整备份恢复，不依赖原数据库可打开。
      </p>
      <p className="muted">
        单一 .offertrack-backup
        包，未压缩、未加密。选择当前仓库以外的本地目录；文件被占用或变化时会停止，失败暂存内容保留，不自动删除。
      </p>
      <label>
        <input
          type="checkbox"
          checked={includeTrash}
          disabled={blocked || !writable}
          onChange={(e) => setIncludeTrash(e.target.checked)}
        />
        包含回收站文件（完整备份）
      </label>
      <div className="section-actions">
        <button disabled={blocked || !writable} onClick={() => void create()}>
          创建完整备份
        </button>
        <button disabled={blocked || !writable} onClick={() => void migrate()}>
          迁移当前仓库…
        </button>
        <button disabled={blocked} onClick={() => void select()}>
          选择并校验完整备份
        </button>
      </div>
      {!writable && (
        <p className="muted">
          新建备份及迁移需要已打开的可写仓库；校验与恢复不需要。
        </p>
      )}
      {busy && (
        <p role="status">
          <progress aria-label="完整备份操作进度" />{" "}
          正在处理，请勿移动文件或强制退出…
        </p>
      )}
      {selected && (
        <section>
          <p>
            已校验包结构、文件大小和
            SHA-256；数据库完整性在恢复副本中进一步验证。
          </p>
          <p>{selected.path}</p>
          <p>
            {selected.preview.fileCount} 个文件 ·{" "}
            {(selected.preview.totalBytes / 1024 / 1024).toFixed(1)} MB · 数据库
            v{selected.preview.schemaVersion} ·{" "}
            {selected.preview.includesRecycleBin
              ? "含回收站"
              : "不含回收站文件"}
          </p>
          <button disabled={blocked} onClick={() => void restore()}>
            恢复已校验备份…
          </button>
        </section>
      )}
      {message && <p role="status">{message}</p>}
      {error && (
        <p role="alert">
          {error} 已完成的备份或副本不会被撤回；失败暂存文件保留。
        </p>
      )}
      {restored && (
        <section>
          <p>已验证仓库：{restored.directory}</p>
          <p>
            {restored.applicationCount} 条投递 · {restored.documentCount}{" "}
            个附件索引
          </p>
          {restored.migrationBackupPath && (
            <p>迁移完整备份：{restored.migrationBackupPath}</p>
          )}
          <button
            disabled={blocked}
            onClick={() => void onOpen(restored.directory)}
          >
            切换到已验证仓库
          </button>
        </section>
      )}
    </article>
  );
}
