import { useState } from "react";
import type {
  AuxiliaryState,
  AuxiliaryStateEdit,
  StageState,
} from "../../contracts";
import { useDraftGuard, useDraftState } from "../../shared/draftGuard";
import { Modal } from "../../shared/Modal";
import { stageStates } from "../applications/tableModel";
import {
  moveState,
  stateDraft,
  stateDraftError,
  stateEdits,
} from "./auxiliaryStateModel";

export function AuxiliaryStatesEditor({
  states,
  scope,
  busy,
  failure,
  onSave,
  onCancel,
}: {
  states: AuxiliaryState[];
  scope: "record" | "template";
  busy: boolean;
  failure: string;
  onSave: (states: AuxiliaryStateEdit[]) => Promise<unknown>;
  onCancel: () => void;
}) {
  const [initial] = useState(() => stateDraft(states));
  const [draft, setDraft] = useState(initial);
  const dirty = JSON.stringify(initial) !== JSON.stringify(draft);
  const error = stateDraftError(draft);
  const { confirm } = useDraftGuard();
  useDraftState(dirty, busy, "辅助状态编辑");
  return (
    <Modal
      title={scope === "record" ? "当前投递的辅助状态" : "模板的辅助状态"}
      onCancel={onCancel}
    >
      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (!error && dirty && !busy) void onSave(stateEdits(draft));
        }}
      >
        <p>
          {scope === "record"
            ? "仅影响当前投递及其面试轮次；其他投递和模板不变。"
            : "只影响以后由此模板新建的投递；已有记录不变。"}
          改名不会改写历史快照。
        </p>
        <p>
          基础分类用于后续提醒与统计，创建后固定；“已完成”只表示该阶段完成，不代表获得
          Offer。内置状态不能移除，失败状态名称固定。最多 100 项。
        </p>
        {failure && <p role="alert">{failure}</p>}
        {error && <p role="alert">{error}</p>}
        <fieldset disabled={busy} className="editor-fields">
          <ol className="auxiliary-state-list">
            {draft.map((state, index) => (
              <li key={state.clientKey}>
                <label>
                  状态名称
                  <input
                    aria-label={`辅助状态 ${index + 1} 名称`}
                    required
                    maxLength={200}
                    disabled={state.stableKey === "failed"}
                    value={state.displayName}
                    onChange={(event) =>
                      setDraft(
                        draft.map((item) =>
                          item.clientKey === state.clientKey
                            ? { ...item, displayName: event.target.value }
                            : item,
                        ),
                      )
                    }
                  />
                </label>
                <label>
                  基础分类
                  <select
                    aria-label={`辅助状态 ${index + 1} 分类`}
                    value={state.semanticKind}
                    disabled={state.id !== null}
                    onChange={(event) =>
                      setDraft(
                        draft.map((item) =>
                          item.clientKey === state.clientKey
                            ? {
                                ...item,
                                semanticKind: event.target.value as StageState,
                              }
                            : item,
                        ),
                      )
                    }
                  >
                    {stageStates
                      .filter(
                        ([key]) =>
                          key !== "failed" || state.stableKey === "failed",
                      )
                      .map(([key, name]) => (
                        <option key={key} value={key}>
                          {name}
                        </option>
                      ))}
                  </select>
                </label>
                <div className="section-actions">
                  <button
                    type="button"
                    aria-label={`上移辅助状态 ${state.displayName}`}
                    disabled={index === 0}
                    onClick={() => setDraft(moveState(draft, index, -1))}
                  >
                    ↑
                  </button>
                  <button
                    type="button"
                    aria-label={`下移辅助状态 ${state.displayName}`}
                    disabled={index === draft.length - 1}
                    onClick={() => setDraft(moveState(draft, index, 1))}
                  >
                    ↓
                  </button>
                  {state.stableKey.startsWith("custom_") && (
                    <button
                      type="button"
                      aria-label={`移除辅助状态 ${state.displayName}`}
                      disabled={state.inUse}
                      title={
                        state.inUse
                          ? "当前进度、轮次或历史引用的状态不能移除"
                          : "保存后生效"
                      }
                      onClick={() => {
                        void confirm({
                          title: "移除辅助状态",
                          message:
                            "保存后仅从当前定义中移除，不影响其他投递或模板。",
                          confirmLabel: "移除状态",
                          destructive: true,
                        }).then((accepted) => {
                          if (accepted)
                            setDraft((current) =>
                              current.filter(
                                (item) => item.clientKey !== state.clientKey,
                              ),
                            );
                        });
                      }}
                    >
                      移除
                    </button>
                  )}
                </div>
                {state.inUse && <small>已被当前进度、轮次或历史引用</small>}
              </li>
            ))}
          </ol>
          <button
            type="button"
            disabled={draft.length >= 100}
            onClick={() => {
              let number = 1;
              while (
                draft.some(
                  (item) => item.displayName.trim() === `自定义状态 ${number}`,
                )
              )
                number++;
              setDraft([
                ...draft,
                {
                  id: null,
                  clientKey: crypto.randomUUID(),
                  stableKey: "custom_draft",
                  displayName: `自定义状态 ${number}`,
                  semanticKind: "awaitingResult",
                  inUse: false,
                },
              ]);
            }}
          >
            添加辅助状态
          </button>
          <div className="modal-actions">
            <button type="button" onClick={onCancel}>
              取消
            </button>
            <button
              type="submit"
              className="primary"
              disabled={!dirty || !!error}
            >
              保存辅助状态
            </button>
          </div>
        </fieldset>
      </form>
    </Modal>
  );
}
