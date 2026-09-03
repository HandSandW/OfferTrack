import { useEffect, useState } from "react";
import type { FieldDefinition, UnlinkedFolder } from "../../contracts";
import { desktopApi } from "../../lib/tauri";
import { Modal } from "../../shared/Modal";
import { useDraftGuard, useDraftState } from "../../shared/draftGuard";
import { companyTypes, initialCreate } from "../applications/tableModel";
import { RecoveryDiagnosticsPanel } from "../documents/RecoveryDiagnosticsPanel";
import {
  fieldDraft,
  fieldRequest,
  fieldTypes,
  optionValues,
} from "./fieldModel";

type Props = { writable: boolean; onError: (error: unknown) => void };
function errorText(error: unknown) {
  return error instanceof Error ? error.message : "读取或保存失败，请重试。";
}
export function SettingsPage(props: Props) {
  return (
    <section className="settings-grid">
      <FieldsPanel {...props} />
      <FoldersPanel {...props} />
      <RecoveryDiagnosticsPanel onError={props.onError} />
    </section>
  );
}

function FieldsPanel({ writable, onError }: Props) {
  const [fields, setFields] = useState<FieldDefinition[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState("");
  const [attempt, setAttempt] = useState(0);
  const [editor, setEditor] = useState<{
    field: FieldDefinition | null;
  } | null>(null);
  const { confirmLeave } = useDraftGuard();
  useEffect(() => {
    let active = true;
    void desktopApi
      .listFieldDefinitions()
      .then((next) => {
        if (active) setFields(next);
      })
      .catch((error: unknown) => {
        if (active) {
          setLoadError(errorText(error));
          onError(error);
        }
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [onError, attempt]);
  const reload = () => {
    setLoading(true);
    setLoadError("");
    setAttempt((n) => n + 1);
  };
  return (
    <article className="panel-page">
      <h2>自定义字段</h2>
      <p className="muted">
        字段定义对当前仓库生效；字段值仍分别保存在每条投递中。编辑不转换或清除已有字段值。
      </p>
      <div className="section-actions">
        <button
          type="button"
          disabled={!writable || loading || !!loadError}
          onClick={() => setEditor({ field: null })}
        >
          添加字段
        </button>
        <button type="button" disabled={loading} onClick={reload}>
          刷新字段列表
        </button>
      </div>
      {loading && <p role="status">正在读取字段…</p>}
      {loadError && <p role="alert">{loadError} 请刷新字段列表重试。</p>}
      <div className="simple-list">
        {fields.map((field) => (
          <article key={field.id}>
            <div>
              <strong>{field.displayName}</strong>
              <span>
                {fieldTypes.find(([key]) => key === field.fieldType)?.[1] ??
                  field.fieldType}
              </span>
            </div>
            <button
              type="button"
              disabled={!writable || loading || !!loadError}
              aria-label={`编辑字段 ${field.displayName}`}
              onClick={() => setEditor({ field })}
            >
              编辑
            </button>
          </article>
        ))}
      </div>
      {!loading && !loadError && !fields.length && (
        <p className="muted">尚未创建自定义字段。</p>
      )}
      {editor && (
        <FieldEditor
          field={editor.field}
          onError={onError}
          onSaved={(next) => {
            setFields(next);
            setEditor(null);
          }}
          onCancel={() => {
            void confirmLeave().then((accepted) => {
              if (accepted) setEditor(null);
            });
          }}
        />
      )}
    </article>
  );
}

function FieldEditor({
  field,
  onSaved,
  onError,
  onCancel,
}: {
  field: FieldDefinition | null;
  onSaved: (fields: FieldDefinition[]) => void;
  onError: Props["onError"];
  onCancel: () => void;
}) {
  const [initial] = useState(() => fieldDraft(field));
  const [draft, setDraft] = useState(initial);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState("");
  useDraftState(
    JSON.stringify(draft) !== JSON.stringify(initial),
    busy,
    "自定义字段编辑",
  );
  const save = async () => {
    setBusy(true);
    setFailure("");
    try {
      onSaved(await desktopApi.saveFieldDefinition(fieldRequest(field, draft)));
    } catch (error) {
      setFailure(errorText(error));
      onError(error);
    } finally {
      setBusy(false);
    }
  };
  const invalid =
    !draft.name.trim() ||
    (draft.type === "select" && !optionValues(draft.options).length);
  return (
    <Modal
      title={field ? "编辑自定义字段" : "添加自定义字段"}
      onCancel={onCancel}
    >
      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (!invalid && !busy) void save();
        }}
      >
        {failure && (
          <p role="alert">
            {failure} 输入已保留；版本冲突时请先复制输入，再取消并刷新字段列表。
          </p>
        )}
        <fieldset className="editor-fields" disabled={busy}>
          <label>
            字段名称
            <input
              autoFocus
              required
              maxLength={200}
              value={draft.name}
              onChange={(event) =>
                setDraft({ ...draft, name: event.target.value })
              }
            />
          </label>
          <label>
            字段类型
            <select
              value={draft.type}
              onChange={(event) =>
                setDraft({ ...draft, type: event.target.value })
              }
            >
              {fieldTypes.map(([key, name]) => (
                <option key={key} value={key}>
                  {name}
                </option>
              ))}
            </select>
          </label>
          {draft.type === "select" && (
            <label>
              选项（逗号分隔）
              <input
                value={draft.options}
                onChange={(event) =>
                  setDraft({ ...draft, options: event.target.value })
                }
                placeholder="例如：高优先级, 中优先级, 低优先级"
              />
            </label>
          )}
          <p>
            修改类型或选项时会校验所有已有值；不兼容的修改会被拒绝。此处不提供删除字段或批量转换数据。
          </p>
          <div className="modal-actions">
            <button type="button" onClick={onCancel}>
              取消
            </button>
            <button type="submit" className="primary" disabled={invalid}>
              保存字段
            </button>
          </div>
        </fieldset>
      </form>
    </Modal>
  );
}

function FoldersPanel({ writable, onError }: Props) {
  const [folders, setFolders] = useState<UnlinkedFolder[]>([]);
  const [loading, setLoading] = useState(true);
  const [failure, setFailure] = useState("");
  const [attempt, setAttempt] = useState(0);
  const [claiming, setClaiming] = useState<string | null>(null);
  const [message, setMessage] = useState("");
  const { confirmLeave } = useDraftGuard();
  useEffect(() => {
    let active = true;
    void desktopApi
      .listUnlinkedFolders(false)
      .then((next) => {
        if (active) setFolders(next);
      })
      .catch((error: unknown) => {
        if (active) {
          setFailure(errorText(error));
          onError(error);
        }
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [onError, attempt]);
  const reload = () => {
    setLoading(true);
    setFailure("");
    setAttempt((n) => n + 1);
  };
  return (
    <article className="panel-page">
      <div className="panel-heading">
        <div>
          <h2>未关联文件夹</h2>
          <p className="muted">默认忽略隐藏目录和点开头目录。</p>
        </div>
        <button type="button" disabled={loading} onClick={reload}>
          重新扫描
        </button>
      </div>
      {loading && <p role="status">正在扫描未关联文件夹…</p>}
      {failure && <p role="alert">{failure} 请重新扫描后重试。</p>}
      {message && <p role="status">{message}</p>}
      <div className="simple-list">
        {folders.map((folder) => (
          <article key={folder.name}>
            <strong>{folder.name}</strong>
            <button
              type="button"
              disabled={!writable || loading || !!failure}
              onClick={() => setClaiming(folder.name)}
            >
              用此文件夹新建投递
            </button>
          </article>
        ))}
      </div>
      {!loading && !failure && !folders.length && (
        <div className="table-empty">没有发现未关联文件夹。</div>
      )}
      {claiming && (
        <ClaimEditor
          folder={claiming}
          onError={onError}
          onCancel={() => {
            void confirmLeave().then((accepted) => {
              if (accepted) setClaiming(null);
            });
          }}
          onSaved={() => {
            setClaiming(null);
            setMessage("投递记录已创建，文件夹规范化结果可在记录详情查看。");
            reload();
          }}
        />
      )}
    </article>
  );
}

function ClaimEditor({
  folder,
  onError,
  onCancel,
  onSaved,
}: {
  folder: string;
  onError: Props["onError"];
  onCancel: () => void;
  onSaved: () => void;
}) {
  const [initial] = useState(() => ({ ...initialCreate, companyName: folder }));
  const [form, setForm] = useState(initial);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState("");
  useDraftState(
    JSON.stringify(form) !== JSON.stringify(initial),
    busy,
    "文件夹认领",
  );
  const save = async () => {
    setBusy(true);
    setFailure("");
    try {
      await desktopApi.claimApplicationFolder(folder, form);
      onSaved();
    } catch (error) {
      setFailure(errorText(error));
      onError(error);
    } finally {
      setBusy(false);
    }
  };
  return (
    <Modal title="认领文件夹" onCancel={onCancel}>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (!busy && form.companyName.trim() && form.positionName.trim())
            void save();
        }}
      >
        <p>{folder}</p>
        {failure && <p role="alert">{failure} 输入仍保留，可重试。</p>}
        <fieldset className="editor-fields" disabled={busy}>
          <label>
            公司名称
            <input
              autoFocus
              required
              maxLength={200}
              value={form.companyName}
              onChange={(event) =>
                setForm({ ...form, companyName: event.target.value })
              }
            />
          </label>
          <label>
            岗位名称
            <input
              required
              maxLength={200}
              value={form.positionName}
              onChange={(event) =>
                setForm({ ...form, positionName: event.target.value })
              }
            />
          </label>
          <label>
            企业性质
            <select
              value={form.companyType}
              onChange={(event) =>
                setForm({ ...form, companyType: event.target.value })
              }
            >
              {companyTypes.map(([key, name]) => (
                <option key={key} value={key}>
                  {name}
                </option>
              ))}
            </select>
          </label>
          <div className="modal-actions">
            <button type="button" onClick={onCancel}>
              取消
            </button>
            <button
              type="submit"
              className="primary"
              disabled={!form.companyName.trim() || !form.positionName.trim()}
            >
              创建并规范化名称
            </button>
          </div>
        </fieldset>
      </form>
    </Modal>
  );
}
