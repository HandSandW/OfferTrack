import { useCallback, useEffect, useState } from "react";
import type { TrashEntry } from "../../contracts";
import { desktopApi } from "../../lib/tauri";
import { useDraftGuard, useDraftState } from "../../shared/draftGuard";

export function RecycleBinPage({
  writable,
  onError,
}: {
  writable: boolean;
  onError: (error: unknown) => void;
}) {
  const [items, setItems] = useState<TrashEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState("");
  const [message, setMessage] = useState("");
  useDraftState(false, busy, "回收站操作");
  const { confirm } = useDraftGuard();
  const refresh = useCallback(async () => {
    setLoading(true);
    setFailure("");
    try {
      setItems(await desktopApi.listTrash());
    } catch (error) {
      setFailure("回收站列表刷新失败，请重试。已完成的恢复不会撤回。");
      onError(error);
    } finally {
      setLoading(false);
    }
  }, [onError]);
  useEffect(() => {
    let active = true;
    desktopApi
      .listTrash()
      .then((next) => {
        if (active) setItems(next);
      })
      .catch((error: unknown) => {
        if (active) {
          setFailure("回收站读取失败，请重试。");
          onError(error);
        }
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [onError]);
  const empty = async () => {
    setBusy(true);
    setMessage("");
    setFailure("");
    try {
      const challenge = await desktopApi.prepareEmptyRecycleBin();
      if (
        !(await confirm({
          title: "清空回收站",
          message: `将永久删除回收站中的 ${challenge.itemCount} 条投递及其文件。此操作无法撤销，是否继续？`,
          confirmLabel: "是",
          destructive: true,
        }))
      )
        return;
      const result = await desktopApi.emptyRecycleBin(
        challenge.warehouseId,
        challenge.confirmationToken,
      );
      setMessage(
        `已删除 ${result.deletedCount} 条；${result.failedApplicationIds.length} 条未清空。失败条目剩余文件仍保留，可释放占用后重试。`,
      );
      await refresh();
    } catch (error) {
      setFailure(error instanceof Error ? error.message : "清空失败，请重试。");
      onError(error);
    } finally {
      setBusy(false);
    }
  };
  const restore = async (item: TrashEntry) => {
    setBusy(true);
    setFailure("");
    setMessage("");
    try {
      const result = await desktopApi.restoreApplication(item.applicationId);
      setItems((current) =>
        current.filter((row) => row.applicationId !== result.applicationId),
      );
      setMessage(
        result.renamed
          ? `“${item.companyName}”已恢复。原位置被占用，已使用新目录：${result.folderRelativePath}。原位置内容未覆盖。`
          : `“${item.companyName}”已恢复到：${result.folderRelativePath}。`,
      );
      await refresh();
    } catch (error) {
      setFailure(
        error instanceof Error
          ? error.message
          : "恢复未确认完成，请检查后重试。",
      );
      onError(error);
    } finally {
      setBusy(false);
    }
  };
  return (
    <section className="panel-page">
      <div className="panel-heading">
        <div>
          <p className="eyebrow">安全删除</p>
          <h2>OfferTrack 回收站</h2>
          <p>“删除”只会把记录文件夹移到当前仓库的回收站；清空前仍可恢复。</p>
        </div>
        <button
          className="danger"
          disabled={!writable || busy || loading || !!failure || !items.length}
          onClick={() => void empty()}
          type="button"
        >
          清空回收站
        </button>
      </div>
      {message && <p role="status">{message}</p>}
      {failure && <p role="alert">{failure}</p>}
      {loading && <p role="status">正在读取回收站…</p>}
      {busy && <p role="status">正在处理，请等待完成…</p>}
      <button disabled={busy || loading} onClick={() => void refresh()}>
        刷新回收站
      </button>
      <div className="simple-list">
        {items.map((item) => (
          <article key={item.applicationId}>
            <div>
              <strong>{item.companyName}</strong>
              <span>
                {item.positionName} · 删除于{" "}
                {new Date(item.deletedAtUtc).toLocaleString()}
              </span>
              <span>原位置：{item.originalRelativePath}</span>
            </div>
            <button
              disabled={!writable || busy || loading}
              onClick={() => void restore(item)}
              type="button"
            >
              恢复
            </button>
          </article>
        ))}
      </div>
      {!items.length && !loading && !failure && (
        <div className="table-empty">回收站为空。</div>
      )}
    </section>
  );
}

export { SettingsPage } from "./SettingsPage";
