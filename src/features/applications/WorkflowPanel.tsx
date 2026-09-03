import { useState } from "react";
import type {
  ApplicationDetail,
  InterviewRound,
  WorkflowStage,
} from "../../contracts";
import { desktopApi } from "../../lib/tauri";
import { Modal } from "../../shared/Modal";
import { useDraftGuard, useDraftState } from "../../shared/draftGuard";
import { localDateTime, utcDateTime } from "./editorModel";
import { stateName } from "../workflows/auxiliaryStateModel";
import { AuxiliaryStatesEditor } from "../workflows/AuxiliaryStatesEditor";
import { StageOrderEditor } from "../workflows/StageOrderEditor";

type Editor =
  | { kind: "stage"; stage: WorkflowStage | null }
  | { kind: "round"; round: InterviewRound | null }
  | { kind: "template" }
  | { kind: "states" }
  | { kind: "order" };
type Commit = (operation: () => Promise<ApplicationDetail>) => Promise<boolean>;

export function WorkflowPanel({
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
  const [editor, setEditor] = useState<Editor | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [failure, setFailure] = useState("");
  const { confirm, confirmLeave } = useDraftGuard();
  useDraftState(false, busy, "流程操作");
  const run = async (operation: () => Promise<unknown>) => {
    setBusy(true);
    setMessage("");
    setFailure("");
    try {
      await operation();
      return true;
    } catch (error) {
      setFailure(
        error instanceof Error
          ? error.message
          : "保存失败，输入已保留。请重试。",
      );
      onError(error);
      return false;
    } finally {
      setBusy(false);
    }
  };
  const commit: Commit = (operation) =>
    run(async () => onChange(await operation()));
  const openEditor = (next: Editor) => {
    setFailure("");
    setEditor(next);
  };
  const closeEditor = async () => {
    if (await confirmLeave()) setEditor(null);
  };
  const changeStage = (stageId: string, stageState: string) =>
    commit(() =>
      desktopApi.changeApplicationStage({
        applicationId: detail.id,
        stageId,
        stageState,
        revision: detail.revision,
        notes: "",
      }),
    );
  return (
    <>
      <p className="scope-note">
        以下修改仅对当前投递生效。保存为模板只影响以后新建的投递。
      </p>
      {failure && !editor && <p role="alert">{failure}</p>}
      {message && <p role="status">{message}</p>}
      <fieldset
        className="editor-fields"
        disabled={!writable || busy || editor !== null}
      >
        <label className="wide-field">
          招聘阶段
          <select
            value={
              detail.currentStageState === "failed"
                ? (detail.stages.find(
                    (stage) => stage.stableKey === "failed_terminal",
                  )?.id ?? "")
                : (detail.currentStageId ?? "")
            }
            onChange={(event) =>
              void changeStage(event.target.value, "pending")
            }
          >
            {detail.stages.map((stage) => (
              <option key={stage.id} value={stage.id}>
                {stage.displayName}
                {stage.isTerminal ? "（终态）" : ""}
              </option>
            ))}
          </select>
        </label>
        <label className="wide-field">
          辅助状态
          <select
            disabled={
              !detail.currentStageId ||
              detail.currentStageState === "failed" ||
              detail.stages.find((stage) => stage.id === detail.currentStageId)
                ?.isTerminal
            }
            value={detail.currentStageState}
            onChange={(event) => {
              if (detail.currentStageId)
                void changeStage(detail.currentStageId, event.target.value);
            }}
          >
            {detail.auxiliaryStates
              .filter(
                (state) =>
                  state.stableKey !== "failed" ||
                  detail.currentStageState === "failed",
              )
              .map((state) => (
                <option key={state.id} value={state.stableKey}>
                  {state.displayName}
                </option>
              ))}
          </select>
        </label>
        <div className="workflow-line">
          {detail.stages.map((stage) => (
            <span
              key={stage.id}
              style={{ borderColor: stage.color }}
              className={stage.id === detail.currentStageId ? "active" : ""}
            >
              {stage.displayName}
              <button
                type="button"
                aria-label={`编辑阶段 ${stage.displayName}`}
                onClick={() => openEditor({ kind: "stage", stage })}
              >
                编辑
              </button>
              {stage.stableKey.startsWith("custom_") && (
                <button
                  type="button"
                  aria-label={`删除阶段 ${stage.displayName}`}
                  disabled={
                    stage.id === detail.currentStageId ||
                    detail.history.some((event) => event.stageId === stage.id)
                  }
                  title="当前阶段或已有历史的阶段不能删除"
                  onClick={() => {
                    void (async () => {
                      if (
                        await confirm({
                          title: "删除自定义阶段",
                          message: `仅从当前投递移除“${stage.displayName}”，不会改动其他投递或模板。`,
                          confirmLabel: "删除阶段",
                          destructive: true,
                        })
                      )
                        await commit(() =>
                          desktopApi.deleteWorkflowStage(
                            detail.id,
                            stage.id,
                            detail.revision,
                          ),
                        );
                    })();
                  }}
                >
                  删除
                </button>
              )}
            </span>
          ))}
        </div>
        <div className="section-actions">
          <button type="button" onClick={() => openEditor({ kind: "states" })}>
            管理辅助状态
          </button>
          <button
            type="button"
            onClick={() => openEditor({ kind: "stage", stage: null })}
            disabled={detail.stages.length >= 100}
          >
            添加阶段
          </button>
          <button type="button" onClick={() => openEditor({ kind: "order" })}>
            调整阶段顺序
          </button>
          <button
            type="button"
            onClick={() => openEditor({ kind: "template" })}
          >
            保存为流程模板…
          </button>
        </div>
        <h3>面试轮次</h3>
        {detail.interviewRounds.map((round) => (
          <article className="interview-card" key={round.id}>
            <strong>
              {round.sequenceNumber}. {round.displayName}
            </strong>
            <p>{stateName(detail.auxiliaryStates, round.state)}</p>
            <p>
              计划：
              {round.scheduledAtUtc
                ? new Date(round.scheduledAtUtc).toLocaleString()
                : "未安排"}{" "}
              · 完成：
              {round.completedAtUtc
                ? new Date(round.completedAtUtc).toLocaleString()
                : "未完成"}
            </p>
            {round.result && <p>结果：{round.result}</p>}
            {round.notes && (
              <p className="preserve-lines">备注：{round.notes}</p>
            )}
            <div className="section-actions">
              <button
                type="button"
                aria-label={`编辑面试 ${round.displayName}`}
                onClick={() => openEditor({ kind: "round", round })}
              >
                编辑轮次
              </button>
              <button
                type="button"
                className="danger-text"
                aria-label={`删除面试 ${round.displayName}`}
                onClick={() => {
                  void (async () => {
                    if (
                      await confirm({
                        title: "删除面试轮次",
                        message: `将删除当前投递的“${round.displayName}”及其时间、结果和备注。此操作不删除简历文件。`,
                        confirmLabel: "删除轮次",
                        destructive: true,
                      })
                    )
                      await commit(() =>
                        desktopApi.deleteInterviewRound(
                          detail.id,
                          round.id,
                          detail.revision,
                        ),
                      );
                  })();
                }}
              >
                删除
              </button>
            </div>
          </article>
        ))}
        <button
          type="button"
          onClick={() => openEditor({ kind: "round", round: null })}
        >
          + 添加面试轮次
        </button>
      </fieldset>
      {editor?.kind === "stage" && (
        <StageEditor
          detail={detail}
          stage={editor.stage}
          busy={busy}
          failure={failure}
          commit={commit}
          onSaved={() => setEditor(null)}
          onCancel={() => void closeEditor()}
        />
      )}
      {editor?.kind === "states" && (
        <AuxiliaryStatesEditor
          states={detail.auxiliaryStates}
          scope="record"
          busy={busy}
          failure={failure}
          onCancel={() => void closeEditor()}
          onSave={async (states) => {
            const saved = await commit(() =>
              desktopApi.updateApplicationStates({
                ownerId: detail.id,
                revision: detail.revision,
                states,
              }),
            );
            if (saved) setEditor(null);
          }}
        />
      )}
      {editor?.kind === "round" && (
        <RoundEditor
          detail={detail}
          round={editor.round}
          busy={busy}
          failure={failure}
          commit={commit}
          onSaved={() => setEditor(null)}
          onCancel={() => void closeEditor()}
        />
      )}
      {editor?.kind === "template" && (
        <TemplateEditor
          companyName={detail.companyName}
          busy={busy}
          failure={failure}
          onCancel={() => void closeEditor()}
          onSave={async (name, setDefault) => {
            if (
              await run(() =>
                desktopApi.saveWorkflowAsTemplate(detail.id, name, setDefault),
              )
            ) {
              setEditor(null);
              setMessage(
                setDefault
                  ? "已保存并设为默认模板；已有投递不会改变。"
                  : "流程模板已保存；已有投递不会改变。",
              );
            }
          }}
        />
      )}
      {editor?.kind === "order" && (
        <StageOrderEditor
          stages={detail.stages}
          busy={busy}
          failure={failure}
          onCancel={() => void closeEditor()}
          onSave={async (ids) => {
            const saved = await commit(() =>
              desktopApi.reorderApplicationWorkflow(
                detail.id,
                detail.revision,
                ids,
              ),
            );
            if (saved) setEditor(null);
            return saved;
          }}
        />
      )}
    </>
  );
}

function StageEditor({
  detail,
  stage,
  busy,
  failure,
  commit,
  onSaved,
  onCancel,
}: {
  detail: ApplicationDetail;
  stage: WorkflowStage | null;
  busy: boolean;
  failure: string;
  commit: Commit;
  onSaved: () => void;
  onCancel: () => void;
}) {
  const [name, setName] = useState(stage?.displayName ?? "");
  const [color, setColor] = useState(stage?.color ?? "#2563eb");
  useDraftState(
    name !== (stage?.displayName ?? "") ||
      color !== (stage?.color ?? "#2563eb"),
    busy,
    "阶段编辑",
  );
  return (
    <Modal
      title={stage ? "编辑当前投递阶段" : "添加当前投递阶段"}
      onCancel={onCancel}
    >
      <form
        onSubmit={(event) => {
          event.preventDefault();
          void commit(() =>
            desktopApi.saveWorkflowStage({
              applicationId: detail.id,
              id: stage?.id ?? null,
              revision: detail.revision,
              displayName: name.trim(),
              color,
              isTerminal: stage?.isTerminal ?? false,
              terminalOutcome: stage?.terminalOutcome ?? null,
            }),
          ).then((saved) => {
            if (saved) onSaved();
          });
        }}
      >
        {failure && <p role="alert">{failure}</p>}
        <fieldset className="editor-fields" disabled={busy}>
          <label>
            阶段名称
            <input
              autoFocus
              required
              maxLength={200}
              value={name}
              onChange={(event) => setName(event.target.value)}
            />
          </label>
          <label>
            阶段颜色
            <input
              type="color"
              value={color}
              onChange={(event) => setColor(event.target.value)}
            />
          </label>
          <p>只影响当前投递。历史记录中的旧名称保持不变；终态属性不能修改。</p>
          <div className="modal-actions">
            <button type="button" onClick={onCancel}>
              取消
            </button>
            <button type="submit" className="primary" disabled={!name.trim()}>
              保存阶段
            </button>
          </div>
        </fieldset>
      </form>
    </Modal>
  );
}

function RoundEditor({
  detail,
  round,
  busy,
  failure,
  commit,
  onSaved,
  onCancel,
}: {
  detail: ApplicationDetail;
  round: InterviewRound | null;
  busy: boolean;
  failure: string;
  commit: Commit;
  onSaved: () => void;
  onCancel: () => void;
}) {
  const [initial] = useState(() => ({
    displayName:
      round?.displayName ??
      `第 ${Math.max(0, ...detail.interviewRounds.map((item) => item.sequenceNumber)) + 1} 轮面试`,
    state: round?.state ?? "pending",
    scheduled: localDateTime(round?.scheduledAtUtc ?? null),
    completed: localDateTime(round?.completedAtUtc ?? null),
    result: round?.result ?? "",
    notes: round?.notes ?? "",
  }));
  const [form, setForm] = useState(initial);
  const [error, setError] = useState("");
  useDraftState(
    JSON.stringify(form) !== JSON.stringify(initial),
    busy,
    "面试轮次编辑",
  );
  return (
    <Modal title={round ? "编辑面试轮次" : "添加面试轮次"} onCancel={onCancel}>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          setError("");
          try {
            const request = {
              applicationId: detail.id,
              id: round?.id ?? null,
              revision: detail.revision,
              displayName: form.displayName.trim(),
              state: form.state,
              result: form.result,
              notes: form.notes,
              scheduledAtUtc: utcDateTime(
                form.scheduled,
                round?.scheduledAtUtc ?? null,
              ),
              completedAtUtc: utcDateTime(
                form.completed,
                round?.completedAtUtc ?? null,
              ),
            };
            void commit(() => desktopApi.saveInterviewRound(request)).then(
              (saved) => {
                if (saved) onSaved();
              },
            );
          } catch (cause) {
            setError(cause instanceof Error ? cause.message : "日期时间无效。");
          }
        }}
      >
        {error && <p role="alert">{error}</p>}
        {failure && <p role="alert">{failure}</p>}
        <fieldset className="editor-fields" disabled={busy}>
          <label>
            轮次名称
            <input
              autoFocus
              required
              maxLength={200}
              value={form.displayName}
              onChange={(event) =>
                setForm({ ...form, displayName: event.target.value })
              }
            />
          </label>
          <label>
            轮次状态
            <select
              value={form.state}
              onChange={(event) =>
                setForm({ ...form, state: event.target.value })
              }
            >
              {detail.auxiliaryStates.map((state) => (
                <option key={state.id} value={state.stableKey}>
                  {state.displayName}
                </option>
              ))}
            </select>
          </label>
          <label>
            计划时间（本地时间）
            <input
              type="datetime-local"
              step="1"
              value={form.scheduled}
              onChange={(event) =>
                setForm({ ...form, scheduled: event.target.value })
              }
            />
          </label>
          <label>
            完成时间（本地时间）
            <input
              type="datetime-local"
              step="1"
              value={form.completed}
              onChange={(event) =>
                setForm({ ...form, completed: event.target.value })
              }
            />
          </label>
          <label>
            面试结果
            <input
              value={form.result}
              onChange={(event) =>
                setForm({ ...form, result: event.target.value })
              }
            />
          </label>
          <label>
            轮次备注
            <textarea
              rows={4}
              value={form.notes}
              onChange={(event) =>
                setForm({ ...form, notes: event.target.value })
              }
            />
          </label>
          <div className="modal-actions">
            <button type="button" onClick={onCancel}>
              取消
            </button>
            <button
              type="submit"
              className="primary"
              disabled={!form.displayName.trim()}
            >
              保存轮次
            </button>
          </div>
        </fieldset>
      </form>
    </Modal>
  );
}

function TemplateEditor({
  companyName,
  busy,
  failure,
  onSave,
  onCancel,
}: {
  companyName: string;
  busy: boolean;
  failure: string;
  onSave: (name: string, setDefault: boolean) => Promise<void>;
  onCancel: () => void;
}) {
  const initial = `${companyName} 招聘流程`;
  const [name, setName] = useState(initial);
  const [setDefault, setSetDefault] = useState(false);
  useDraftState(name !== initial || setDefault, busy, "流程模板编辑");
  return (
    <Modal title="保存为流程模板" onCancel={onCancel}>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          void onSave(name.trim(), setDefault);
        }}
      >
        {failure && <p role="alert">{failure}</p>}
        <fieldset className="editor-fields" disabled={busy}>
          <label>
            模板名称
            <input
              autoFocus
              required
              value={name}
              onChange={(event) => setName(event.target.value)}
            />
          </label>
          <label className="checkbox-label">
            <input
              type="checkbox"
              checked={setDefault}
              onChange={(event) => setSetDefault(event.target.checked)}
            />
            设为以后新建投递的默认模板
          </label>
          <p>
            仅复制当前流程结构，不包含这条投递的状态历史；不会修改任何已有投递。
          </p>
          <div className="modal-actions">
            <button type="button" onClick={onCancel}>
              取消
            </button>
            <button type="submit" className="primary" disabled={!name.trim()}>
              保存模板
            </button>
          </div>
        </fieldset>
      </form>
    </Modal>
  );
}
