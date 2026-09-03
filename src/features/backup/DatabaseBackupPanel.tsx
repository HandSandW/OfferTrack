import { useState } from "react";
import { desktopApi } from "../../lib/tauri";
import { selectDirectory } from "../../lib/dialog";
import { useDraftGuard, useDraftState } from "../../shared/draftGuard";
import type { BackupCatalog, BackupItem, BackupTrashResult } from "./contracts";

const reasons = {
  manual: "手动",
  daily: "每日",
  beforeUpgrade: "升级前",
  beforeMigration: "迁移前",
  beforeBatch: "批量修改前",
  beforeAgentWrite: "Agent 写入前",
};
export function DatabaseBackupPanel({
  writable,
  onError,
}: {
  writable: boolean;
  onError: (error: unknown) => void;
}) {
  const [catalog, setCatalog] = useState<BackupCatalog | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");
  const [purged, setPurged] = useState<BackupTrashResult | null>(null);
  const { confirm } = useDraftGuard();
  useDraftState(false, busy, "备份与恢复");
  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    setError("");
    setMessage("");
    try {
      await action();
    } catch (failure) {
      setError(
        failure instanceof Error ? failure.message : "备份操作未完成，请重试。",
      );
      onError(failure);
    } finally {
      setBusy(false);
    }
  };
  const load = async () => {
    setCatalog(await desktopApi.listDatabaseBackups());
  };
  const restore = (item: BackupItem) =>
    run(async () => {
      const preview = await desktopApi.previewDatabaseBackup(
        item.id,
        item.recycled,
      );
      const parent = await selectDirectory(
        "选择恢复位置（将创建全新子目录，不覆盖已有文件）",
      );
      if (!parent) return;
      if (
        !(await confirm({
          title: "恢复数据库到新目录",
          message: `已校验 ${preview.applicationCount} 条投递、${preview.documentCount} 个附件索引。此备份不包含简历正文；恢复后文件仍需另行找回。将在 ${parent} 中创建全新子目录，不覆盖或切换当前仓库。继续吗？`,
          confirmLabel: "创建恢复副本",
        }))
      )
        return;
      const result = await desktopApi.restoreDatabaseBackup(
        item.id,
        item.recycled,
        preview.backup.sha256,
        parent,
      );
      setMessage(
        `恢复完成：${result.directory}。当前仓库未切换；请通过“打开仓库”自行查看。只恢复元信息，没有恢复简历文件。`,
      );
    });
  const purge = () =>
    run(async () => {
      const challenge = await desktopApi.prepareBackupRecycleBin();
      if (!challenge.itemIds.length) {
        setMessage(
          `没有可清理的备份目录；保留 ${challenge.skippedCount} 个不明或不可访问项目。`,
        );
        return;
      }
      if (
        !(await confirm({
          title: "永久清空备份回收站",
          message: `将永久删除当前仓库 recycle-bin/backups 下 ${challenge.itemIds.length} 个已识别目录及全部内容（包含可能损坏的备份），无法撤销。保留 ${challenge.skippedCount} 个不明或不可访问项目。不删除活动备份或投递附件。确认有效期 60 秒。继续吗？`,
          confirmLabel: "是，永久清空备份",
          destructive: true,
        }))
      )
        return;
      const result = await desktopApi.emptyBackupRecycleBin(
        challenge.confirmationToken,
      );
      setPurged(result);
      setMessage(
        `备份回收站清理完成：删除 ${result.deletedIds.length} 个目录，${result.failed.length} 个失败，另保留 ${result.skippedCount} 个不明或不可访问项目。永久删除的内容无法恢复。`,
      );
      setCatalog(null);
      await load();
    });
  return (
    <article className="panel-page">
      <h2>数据库备份与恢复</h2>
      <p>
        保存投递、流程、视图等元信息，不包含 PDF/Word
        正文。需要简历正文请使用“完整备份与迁移”。
      </p>
      <p className="muted">
        写入打开仓库及跨日首次写入时每日备份一次，升级前另备份；保留最近 30 日和
        12
        个月的每日快照。过期快照移入仓库回收站，不永久删除。手动与升级前备份暂不自动轮换。
      </p>
      {!writable && (
        <p className="muted">
          只读仓库可以校验备份、恢复到新的外部目录，不能向当前仓库新增备份。
        </p>
      )}
      <div className="section-actions">
        <button disabled={busy || !writable} onClick={() => void purge()}>
          清空备份回收站…
        </button>
        <button disabled={busy} onClick={() => void run(load)}>
          读取备份列表
        </button>
        <button
          disabled={busy || !writable}
          onClick={() =>
            void run(async () => {
              const result = await desktopApi.createDatabaseBackup();
              setMessage(
                `数据库备份已完成${result.retentionWarning ? "；旧备份轮换未完成，原文件仍保留" : ""}。`,
              );
              await load();
            })
          }
        >
          立即备份数据库
        </button>
      </div>
      {busy && (
        <p role="status">
          <progress aria-label="备份操作进度" />{" "}
          正在处理，请勿移动文件或强制退出…
        </p>
      )}
      {message && <p role="status">{message}</p>}
      {purged && purged.failed.length > 0 && (
        <ul aria-label="备份清理失败项目">
          {purged.failed.map((failure) => (
            <li key={failure.id}>
              {failure.id}：{failure.error.message}{" "}
              可能已删除部分文件；剩余内容保留，释放占用后重新确认重试。
            </li>
          ))}
        </ul>
      )}
      {error && (
        <p role="alert">
          {error} 列表可能不是最新状态；已完成的备份或恢复不会因此撤回。
        </p>
      )}
      {catalog && (
        <>
          {(catalog.incompleteCount > 0 || catalog.invalidCount > 0) && (
            <p className="inline-warning">
              发现 {catalog.incompleteCount} 个未完成备份、
              {catalog.invalidCount}{" "}
              个无法识别的项目。原内容保留，没有自动清理。
            </p>
          )}
          <p className="muted">
            列表只读取清单；“校验”会检查文件大小、SHA-256、SQLite
            完整性和版本。回收站中的数据库备份在永久清空前仍可恢复；损坏的剩余目录也可通过清空入口重试清理。
          </p>
          <div className="simple-list">
            {catalog.items.map((item) => (
              <section key={`${item.recycled}-${item.id}`}>
                <strong>
                  {new Date(item.createdAtUtc).toLocaleString()} ·{" "}
                  {reasons[item.reason]}
                  {item.recycled ? " · 已移入回收站" : ""}
                </strong>
                <p>
                  数据库 v{item.schemaVersion} ·{" "}
                  {Math.ceil(item.sizeBytes / 1024)} KB · {item.id}
                </p>
                <div className="section-actions">
                  <button
                    disabled={busy}
                    onClick={() =>
                      void run(async () => {
                        const preview = await desktopApi.previewDatabaseBackup(
                          item.id,
                          item.recycled,
                        );
                        setMessage(
                          `校验通过：${preview.applicationCount} 条投递，${preview.documentCount} 个附件索引；不包含附件正文。`,
                        );
                      })
                    }
                  >
                    校验
                  </button>
                  <button disabled={busy} onClick={() => void restore(item)}>
                    恢复到新目录…
                  </button>
                </div>
              </section>
            ))}
          </div>
          {!catalog.items.length && <p>暂无可识别的数据库备份。</p>}
        </>
      )}
    </article>
  );
}
