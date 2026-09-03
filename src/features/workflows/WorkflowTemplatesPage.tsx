import { useEffect, useState } from "react";
import type { WorkflowTemplate, WorkflowTemplateDetail } from "../../contracts";
import { desktopApi } from "../../lib/tauri";
import { useDraftGuard, useDraftState } from "../../shared/draftGuard";
import { Modal } from "../../shared/Modal";
import { AuxiliaryStatesEditor } from "./AuxiliaryStatesEditor";
import {
  addTemplateStage,
  canMoveStage,
  moveStage,
  templateDraft,
  templateRequest,
} from "./workflowModel";

export function WorkflowTemplatesPage({
  writable,
  onError,
}: {
  writable: boolean;
  onError: (error: unknown) => void;
}) {
  const [templates, setTemplates] = useState<WorkflowTemplate[]>([]);
  const [detail, setDetail] = useState<WorkflowTemplateDetail | null>(null);
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState("");
  const [reloadVersion, setReloadVersion] = useState(0);
  const { confirmLeave } = useDraftGuard();
  useDraftState(false, busy, "加载流程模板");
  const report = (cause: unknown) => {
    setError(cause instanceof Error ? cause.message : "模板读取失败，请重试。");
    onError(cause);
  };
  useEffect(() => {
    let active = true;
    void desktopApi
      .listWorkflowTemplates()
      .then(async (items) => {
        if (!active) return;
        setTemplates(items);
        const initial = items.find((item) => item.isDefault) ?? items[0];
        if (initial) {
          const next = await desktopApi.getWorkflowTemplate(initial.id);
          if (active) setDetail(next);
        }
      })
      .catch((cause: unknown) => {
        if (active) {
          setError(
            cause instanceof Error ? cause.message : "模板读取失败，请重试。",
          );
          onError(cause);
        }
      })
      .finally(() => {
        if (active) setBusy(false);
      });
    return () => {
      active = false;
    };
  }, [onError]);

  const reload = async (id?: string) => {
    if (!(await confirmLeave())) return;
    setBusy(true);
    setError("");
    try {
      const items = await desktopApi.listWorkflowTemplates();
      const target =
        id ??
        detail?.id ??
        items.find((item) => item.isDefault)?.id ??
        items[0]?.id;
      const next = target ? await desktopApi.getWorkflowTemplate(target) : null;
      setTemplates(items);
      setDetail(next);
      setReloadVersion((value) => value + 1);
    } catch (cause) {
      report(cause);
    } finally {
      setBusy(false);
    }
  };
  const saved = (next: WorkflowTemplateDetail) => {
    setDetail(next);
    setTemplates((items) => {
      const updated = items.some((item) => item.id === next.id)
        ? items.map((item) => (item.id === next.id ? next : item))
        : [...items, next];
      // The backend increments the old default's version; detail is fetched
      // again before editing another template, never reconstructed from this list.
      return updated.map((item) =>
        next.isDefault && item.id !== next.id
          ? { ...item, isDefault: false }
          : item,
      );
    });
  };
  return (
    <section className="templates-page">
      <p className="scope-note">
        模板只用于以后新建的投递。编辑、复制或切换默认模板不会修改任何已有投递、文件或状态历史。
      </p>
      <div className="template-toolbar">
        <label>
          选择流程模板
          <select
            aria-label="选择流程模板"
            value={detail?.id ?? ""}
            disabled={busy}
            onChange={(event) => void reload(event.target.value)}
          >
            {!detail && <option value="">请选择模板</option>}
            {templates.map((item) => (
              <option key={item.id} value={item.id}>
                {item.name}
                {item.isDefault ? "（默认）" : ""}
              </option>
            ))}
          </select>
        </label>
        <button type="button" disabled={busy} onClick={() => void reload()}>
          重新载入
        </button>
      </div>
      {busy && <p role="status">正在读取模板…</p>}
      {error && <p role="alert">{error}</p>}
      {!busy && !templates.length && !error && <p>当前仓库没有流程模板。</p>}
      {detail && (
        <TemplateEditor
          key={`${detail.id}:${detail.revision}:${reloadVersion}`}
          detail={detail}
          writable={writable && !busy}
          onSaved={saved}
          onError={onError}
        />
      )}
    </section>
  );
}

function TemplateEditor({
  detail,
  writable,
  onSaved,
  onError,
}: {
  detail: WorkflowTemplateDetail;
  writable: boolean;
  onSaved: (detail: WorkflowTemplateDetail) => void;
  onError: (error: unknown) => void;
}) {
  const [draft, setDraft] = useState(() => templateDraft(detail));
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState("");
  const [copyName, setCopyName] = useState<string | null>(null);
  const [editingStates, setEditingStates] = useState(false);
  const initialCopyName = `${detail.name}（副本）`;
  const dirty = JSON.stringify(draft) !== JSON.stringify(templateDraft(detail));
  const { confirm, confirmLeave } = useDraftGuard();
  useDraftState(
    dirty || (copyName !== null && copyName !== initialCopyName),
    busy,
    "流程模板编辑",
  );
  const run = async (operation: () => Promise<WorkflowTemplateDetail>) => {
    setBusy(true);
    setFailure("");
    try {
      const next = await operation();
      setCopyName(null);
      setEditingStates(false);
      setDraft(templateDraft(next));
      onSaved(next);
    } catch (cause) {
      setFailure(
        cause instanceof Error ? cause.message : "保存失败，修改已保留。",
      );
      onError(cause);
    } finally {
      setBusy(false);
    }
  };
  const cancelCopy = async () => {
    if (await confirmLeave()) {
      setCopyName(null);
      setFailure("");
    }
  };
  const makeDefault = async () => {
    if (
      await confirm({
        title: "切换默认流程模板",
        message: `以后新建投递将使用“${detail.name}”。已有投递不受影响。`,
        confirmLabel: "设为默认",
      })
    )
      await run(() =>
        desktopApi.setDefaultWorkflowTemplate(detail.id, detail.revision),
      );
  };
  return (
    <>
      <div className="template-toolbar">
        <h2>
          {detail.name}
          {detail.isDefault ? " · 默认模板" : ""}
        </h2>
        <button
          type="button"
          disabled={!writable || busy || dirty}
          onClick={() => {
            setFailure("");
            setEditingStates(true);
          }}
        >
          管理模板辅助状态
        </button>
        <button
          type="button"
          disabled={!writable || busy || dirty}
          onClick={() => {
            setFailure("");
            setCopyName(initialCopyName);
          }}
        >
          复制模板
        </button>
        <button
          type="button"
          disabled={!writable || busy || dirty || detail.isDefault}
          onClick={() => void makeDefault()}
        >
          设为默认模板
        </button>
      </div>
      {dirty && (
        <p role="status">
          有未保存的模板修改。请先保存或重新载入，再复制或设为默认。
        </p>
      )}
      {failure && copyName === null && !editingStates && (
        <p role="alert">{failure}</p>
      )}
      <p>
        辅助状态：
        {detail.auxiliaryStates.map((state) => state.displayName).join("、")}
      </p>
      {editingStates && (
        <AuxiliaryStatesEditor
          states={detail.auxiliaryStates}
          scope="template"
          busy={busy}
          failure={failure}
          onCancel={() => {
            void confirmLeave().then((accepted) => {
              if (accepted) {
                setEditingStates(false);
                setFailure("");
              }
            });
          }}
          onSave={(states) =>
            run(() =>
              desktopApi.updateTemplateStates({
                ownerId: detail.id,
                revision: detail.revision,
                states,
              }),
            )
          }
        />
      )}
      <form
        onSubmit={(event) => {
          event.preventDefault();
          void run(() =>
            desktopApi.updateWorkflowTemplate(templateRequest(detail, draft)),
          );
        }}
      >
        <fieldset
          className="editor-fields"
          disabled={!writable || busy || copyName !== null || editingStates}
        >
          <label>
            模板名称
            <input
              required
              maxLength={200}
              value={draft.name}
              onChange={(event) =>
                setDraft({ ...draft, name: event.target.value })
              }
            />
          </label>
          <label>
            模板说明
            <textarea
              rows={2}
              maxLength={10000}
              value={draft.description}
              onChange={(event) =>
                setDraft({ ...draft, description: event.target.value })
              }
            />
          </label>
          <p>
            阶段名和颜色可编辑；中间阶段可上下移动。准备投递保持第一项，Offer
            和已挂保持末尾。内置阶段不能移除。
          </p>
          <ol className="template-stage-list">
            {draft.stages.map((stage, index) => (
              <li key={stage.clientKey}>
                <label>
                  阶段名称
                  <input
                    aria-label={`阶段 ${index + 1} 名称`}
                    required
                    maxLength={200}
                    value={stage.displayName}
                    onChange={(event) =>
                      setDraft({
                        ...draft,
                        stages: draft.stages.map((item) =>
                          item.clientKey === stage.clientKey
                            ? { ...item, displayName: event.target.value }
                            : item,
                        ),
                      })
                    }
                  />
                </label>
                <label>
                  颜色
                  <input
                    aria-label={`阶段 ${index + 1} 颜色`}
                    type="color"
                    value={stage.color}
                    onChange={(event) =>
                      setDraft({
                        ...draft,
                        stages: draft.stages.map((item) =>
                          item.clientKey === stage.clientKey
                            ? { ...item, color: event.target.value }
                            : item,
                        ),
                      })
                    }
                  />
                </label>
                <div className="section-actions">
                  <button
                    type="button"
                    aria-label={`上移 ${stage.displayName}`}
                    disabled={!canMoveStage(draft.stages, index, -1)}
                    onClick={() =>
                      setDraft({
                        ...draft,
                        stages: moveStage(draft.stages, index, -1),
                      })
                    }
                  >
                    ↑
                  </button>
                  <button
                    type="button"
                    aria-label={`下移 ${stage.displayName}`}
                    disabled={!canMoveStage(draft.stages, index, 1)}
                    onClick={() =>
                      setDraft({
                        ...draft,
                        stages: moveStage(draft.stages, index, 1),
                      })
                    }
                  >
                    ↓
                  </button>
                  {stage.stableKey.startsWith("custom_") && (
                    <button
                      type="button"
                      aria-label={`移除 ${stage.displayName}`}
                      onClick={() => {
                        void confirm({
                          title: "从模板移除阶段",
                          message:
                            "保存模板后生效；不会移除任何已有投递中的阶段或历史。",
                          confirmLabel: "从模板移除",
                          destructive: true,
                        }).then((accepted) => {
                          if (accepted)
                            setDraft((current) => ({
                              ...current,
                              stages: current.stages.filter(
                                (item) => item.clientKey !== stage.clientKey,
                              ),
                            }));
                        });
                      }}
                    >
                      移除
                    </button>
                  )}
                </div>
              </li>
            ))}
          </ol>
          <div className="section-actions">
            <button
              type="button"
              disabled={draft.stages.length >= 100}
              onClick={() =>
                setDraft(addTemplateStage(draft, crypto.randomUUID()))
              }
            >
              添加模板阶段
            </button>
            <button
              type="submit"
              className="primary"
              disabled={
                !dirty ||
                !draft.name.trim() ||
                draft.stages.some((stage) => !stage.displayName.trim())
              }
            >
              保存模板修改
            </button>
          </div>
        </fieldset>
      </form>
      {copyName !== null && (
        <Modal title="复制流程模板" onCancel={() => void cancelCopy()}>
          <form
            onSubmit={(event) => {
              event.preventDefault();
              void run(() =>
                desktopApi.duplicateWorkflowTemplate(
                  detail.id,
                  detail.revision,
                  copyName.trim(),
                ),
              );
            }}
          >
            {failure && <p role="alert">{failure}</p>}
            <fieldset className="editor-fields" disabled={busy || !writable}>
              <label>
                副本名称
                <input
                  autoFocus
                  required
                  maxLength={200}
                  value={copyName}
                  onChange={(event) => setCopyName(event.target.value)}
                />
              </label>
              <p>
                副本拥有独立模板和阶段
                ID，不自动设为默认，也不创建投递或复制简历。
              </p>
              <div className="modal-actions">
                <button type="button" onClick={() => void cancelCopy()}>
                  取消
                </button>
                <button
                  type="submit"
                  className="primary"
                  disabled={!copyName.trim()}
                >
                  创建模板副本
                </button>
              </div>
            </fieldset>
          </form>
        </Modal>
      )}
    </>
  );
}
