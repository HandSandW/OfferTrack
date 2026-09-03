import { useEffect, useState } from "react";
import type {
  ApplicationDetail,
  ApplicationDirectories,
  DocumentEntry,
  PathObservation,
} from "../../contracts";
import { desktopApi } from "../../lib/tauri";
import { useDraftGuard, useDraftState } from "../../shared/draftGuard";
import { pathStateText } from "./fileStatus";
import { DocumentTree } from "./DocumentTree";
import { RenameDocumentDialog } from "./RenameDocumentDialog";

export function FilesPanel({
  detail,
  writable,
  onChange,
  onError,
}: {
  detail: ApplicationDetail;
  writable: boolean;
  onChange: (detail: ApplicationDetail) => void;
  onError: (error: unknown) => void;
}) {
  const [health, setHealth] = useState<PathObservation | null>(null);
  const [loading, setLoading] = useState(true);
  const [attempt, setAttempt] = useState(0);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState("");
  const [message, setMessage] = useState("");
  const [directories, setDirectories] = useState<
    ApplicationDirectories["directories"]
  >([]);
  const [directoryFailure, setDirectoryFailure] = useState("");
  const [renaming, setRenaming] = useState<DocumentEntry | null>(null);
  useDraftState(false, busy, "文件操作");
  const { confirm } = useDraftGuard();
  useEffect(() => {
    let active = true;
    void desktopApi
      .inspectApplicationFiles(detail.id)
      .then((value) => {
        if (active) setHealth(value);
        if (active && value.state === "available") {
          return desktopApi
            .listApplicationDirectories(detail.id)
            .then((result) => {
              if (active) {
                setDirectories(result.directories);
                setDirectoryFailure("");
              }
            })
            .catch((error: unknown) => {
              if (active) {
                setDirectoryFailure(
                  error instanceof Error ? error.message : "子目录读取失败",
                );
                onError(error);
              }
            });
        }
        if (active) setDirectories([]);
      })
      .catch((error: unknown) => {
        if (active) {
          setFailure(error instanceof Error ? error.message : "目录检查失败");
          onError(error);
        }
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [
    detail.id,
    detail.folderRelativePath,
    detail.documents,
    onError,
    attempt,
  ]);
  const check = () => {
    setLoading(true);
    setHealth(null);
    setFailure("");
    setDirectories([]);
    setDirectoryFailure("");
    setAttempt((n) => n + 1);
  };
  const run = async (operation: () => Promise<void>) => {
    setBusy(true);
    setFailure("");
    setMessage("");
    try {
      await operation();
    } catch (error) {
      setFailure(
        error instanceof Error ? error.message : "文件操作失败，请重试",
      );
      onError(error);
    } finally {
      setBusy(false);
    }
  };
  const unavailable = loading || !health || health.state !== "available";
  return (
    <>
      {failure && (
        <p role="alert">{failure} 上次索引仍保留，可检查目录或重新扫描。</p>
      )}
      {message && <p role="status">{message}</p>}
      {directoryFailure && (
        <p role="alert">
          {directoryFailure}{" "}
          子目录列表可能已过期；附件索引仍保留，可点击“检查目录”重试。
        </p>
      )}
      {loading ? (
        <p role="status">正在检查目录…</p>
      ) : (
        health && (
          <p
            role="status"
            className={
              health.state === "available" ? "muted" : "inline-warning"
            }
          >
            {pathStateText[health.state]}。记录和原索引不会因此被删除。
          </p>
        )
      )}
      {health?.relativePath && <p className="muted">{health.relativePath}</p>}
      {!writable && (
        <p className="muted">
          只读模式显示最近已提交索引；检查目录不回写附件状态。
        </p>
      )}
      <div className="section-actions">
        <button
          disabled={busy || unavailable}
          onClick={() =>
            void run(() => desktopApi.openApplicationFolder(detail.id))
          }
        >
          打开投递文件夹
        </button>
        <button disabled={busy || loading} onClick={check}>
          检查目录
        </button>
        <button
          disabled={!writable || busy || loading}
          onClick={() =>
            void run(async () => {
              const documents = await desktopApi.scanApplicationDocuments(
                detail.id,
              );
              onChange({ ...detail, documents });
              check();
              setMessage(
                "索引已更新；缺失文件仅标记，不删除记录或重新创建文件。",
              );
            })
          }
        >
          重新扫描
        </button>
      </div>
      {detail.folderNormalizationPending && (
        <div className="inline-warning">
          文件夹名称尚未规范化，可能被占用或目标重名；原目录关联仍保留。
          <button
            disabled={!writable || busy || unavailable}
            onClick={() =>
              void run(async () => {
                const next = await desktopApi.retryFolderNormalization(
                  detail.id,
                );
                onChange(next);
                check();
                setMessage(
                  next.folderNormalizationPending
                    ? "仍未规范化。请关闭占用程序并检查目标是否重名，再重试；未强制改名。"
                    : "文件夹名称已规范化。",
                );
              })
            }
          >
            重试规范化
          </button>
        </div>
      )}
      {busy && <p role="status">正在执行文件操作，请等待完成…</p>}
      <DocumentTree
        documents={detail.documents}
        applicationId={detail.id}
        disabled={busy || unavailable}
        run={run}
        onCopied={() => setMessage("文件路径已复制。")}
        directories={directories}
        onRename={writable && !renaming ? setRenaming : undefined}
        onTrash={
          writable
            ? (document) =>
                void run(async () => {
                  if (
                    !(await confirm({
                      title: "删除附件",
                      message: `将“${document.displayName}”移入附件回收站？之后可恢复。`,
                      confirmLabel: "是",
                      destructive: true,
                    }))
                  )
                    return;
                  const next = await desktopApi.trashDocument({
                    applicationId: detail.id,
                    documentId: document.id,
                    expectedRelativePath: document.relativePath,
                  });
                  onChange(next);
                  check();
                  setMessage("附件已移入回收站；文件和元数据可在回收站恢复。");
                })
            : undefined
        }
      />
      {renaming && (
        <RenameDocumentDialog
          applicationId={detail.id}
          document={renaming}
          onCancel={() => setRenaming(null)}
          onSaved={(next) => {
            setRenaming(null);
            onChange(next);
            check();
            setMessage("附件已重命名，文件内容和附件 ID 保持不变。");
          }}
          onError={onError}
        />
      )}
      {!loading &&
        !failure &&
        health?.state === "available" &&
        !detail.documents.length && (
          <p className="muted">
            索引中暂无附件。可在资源管理器放入材料，再重新扫描；目录检查只读取目录结构，不回写附件索引。
          </p>
        )}
    </>
  );
}
