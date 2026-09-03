import { useState } from "react";
import type { ApplicationDetail, DocumentEntry } from "../../contracts";
import { desktopApi } from "../../lib/tauri";
import { Modal } from "../../shared/Modal";
import { useDraftGuard, useDraftState } from "../../shared/draftGuard";

export function RenameDocumentDialog({
  applicationId,
  document,
  onSaved,
  onCancel,
  onError,
}: {
  applicationId: string;
  document: DocumentEntry;
  onSaved: (detail: ApplicationDetail) => void;
  onCancel: () => void;
  onError: (error: unknown) => void;
}) {
  const [name, setName] = useState(document.displayName);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState("");
  const { confirmLeave } = useDraftGuard();
  useDraftState(name !== document.displayName, busy, "附件重命名");
  const cancel = () =>
    void confirmLeave().then((accepted) => {
      if (accepted) onCancel();
    });
  const save = async () => {
    if (busy || !name.trim() || name === document.displayName) return;
    setBusy(true);
    setFailure("");
    try {
      onSaved(
        await desktopApi.renameDocument({
          applicationId,
          documentId: document.id,
          expectedRelativePath: document.relativePath,
          newName: name,
        }),
      );
    } catch (error) {
      setFailure(
        error instanceof Error ? error.message : "重命名失败，请检查后重试",
      );
      onError(error);
    } finally {
      setBusy(false);
    }
  };
  return (
    <Modal title="重命名附件" onCancel={cancel}>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          void save();
        }}
      >
        <p className="muted">
          只修改当前文件的名称，不移动目录、不覆盖同名文件。修改扩展名不会转换文件格式。
        </p>
        <p>原位置：{document.relativePath}</p>
        {failure && (
          <p role="alert">{failure} 输入已保留，请检查文件和索引后重试。</p>
        )}
        <fieldset className="editor-fields" disabled={busy}>
          <label>
            文件名称
            <input
              autoFocus
              required
              maxLength={255}
              value={name}
              onChange={(event) => setName(event.target.value)}
            />
          </label>
          <div className="modal-actions">
            <button type="button" onClick={cancel}>
              取消
            </button>
            <button
              type="submit"
              className="primary"
              disabled={!name.trim() || name === document.displayName}
            >
              保存名称
            </button>
          </div>
        </fieldset>
        {busy && <p role="status">正在重命名，请等待完成…</p>}
      </form>
    </Modal>
  );
}
