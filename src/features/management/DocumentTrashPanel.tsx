import { useCallback, useEffect, useState } from "react";
import type { DocumentTrashEntry } from "../../contracts";
import { desktopApi } from "../../lib/tauri";
import { useDraftGuard, useDraftState } from "../../shared/draftGuard";
export function DocumentTrashPanel({
  writable,
  onError,
}: {
  writable: boolean;
  onError: (error: unknown) => void;
}) {
  const [items, setItems] = useState<DocumentTrashEntry[]>([]),
    [busy, setBusy] = useState(false),
    [failure, setFailure] = useState(""),
    [message, setMessage] = useState("");
  useDraftState(false, busy, "附件回收站操作");
  const { confirm } = useDraftGuard();
  const refresh = useCallback(async () => {
    try {
      setItems(await desktopApi.listDocumentTrash());
      setFailure("");
    } catch (e) {
      setFailure("附件回收站读取失败，请重试。");
      onError(e);
    }
  }, [onError]);
  useEffect(() => {
    void refresh();
  }, [refresh]);
  const run = async (op: () => Promise<void>) => {
    setBusy(true);
    setFailure("");
    setMessage("");
    try {
      await op();
    } catch (e) {
      setFailure(e instanceof Error ? e.message : "操作失败，请重试。");
      onError(e);
    } finally {
      setBusy(false);
    }
  };
  return (
    <section aria-labelledby="document-trash-title">
      <div className="panel-heading">
        <div>
          <h3 id="document-trash-title">附件回收站</h3>
          <p>单个附件与投递记录分开保留；恢复不覆盖同名文件。</p>
        </div>
        <button
          className="danger"
          disabled={!writable || busy || !items.length}
          onClick={() =>
            void run(async () => {
              const c = await desktopApi.prepareDocumentTrashCleanup();
              if (
                !(await confirm({
                  title: "清空附件回收站",
                  message: `将永久删除 ${c.itemIds.length} 个附件${c.missingCount ? `（其中 ${c.missingCount} 个文件已缺失）` : ""}，且保留审计元数据。此操作无法撤销，是否继续？`,
                  confirmLabel: "是",
                  destructive: true,
                }))
              )
                return;
              const r = await desktopApi.emptyDocumentTrash(
                c.confirmationToken,
              );
              setMessage(
                `已永久删除 ${r.deletedIds.length} 个附件；${r.failed.length} 个未删除。`,
              );
              await refresh();
            })
          }
        >
          清空附件回收站
        </button>
      </div>
      {failure && <p role="alert">{failure}</p>}
      {message && <p role="status">{message}</p>}
      <button disabled={busy} onClick={() => void refresh()}>
        刷新附件回收站
      </button>
      <div className="simple-list">
        {items.map((i) => (
          <article key={i.id}>
            <div>
              <strong>{i.displayName}</strong>
              <span>
                {i.companyName} · {i.positionName}
              </span>
              <span>
                原位置：{i.originalRelativePath} ·{" "}
                {i.fileState === "available" ? "文件可恢复" : "文件当前不可用"}
              </span>
              {i.parentDeleted && (
                <span className="inline-warning">
                  所属投递已删除，请先恢复投递。
                </span>
              )}
            </div>
            <button
              disabled={
                !writable ||
                busy ||
                i.parentDeleted ||
                i.fileState !== "available"
              }
              onClick={() =>
                void run(async () => {
                  const r = await desktopApi.restoreDocument(i.id);
                  setMessage(
                    r.relocated
                      ? `附件已恢复为 ${r.relativePath}，未覆盖原位置内容。`
                      : `附件已恢复到 ${r.relativePath}。`,
                  );
                  await refresh();
                })
              }
            >
              恢复
            </button>
          </article>
        ))}
      </div>
      {!items.length && <div className="table-empty">附件回收站为空。</div>}
    </section>
  );
}
