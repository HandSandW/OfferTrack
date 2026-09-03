import { useState } from "react";
import type { WorkflowStage } from "../../contracts";
import { useDraftState } from "../../shared/draftGuard";
import { Modal } from "../../shared/Modal";
import { canMoveStage, moveStage } from "./workflowModel";

export function StageOrderEditor({
  stages,
  busy,
  failure,
  onSave,
  onCancel,
}: {
  stages: WorkflowStage[];
  busy: boolean;
  failure: string;
  onSave: (ids: string[]) => Promise<boolean>;
  onCancel: () => void;
}) {
  const [ordered, setOrdered] = useState(stages);
  const dirty = ordered.some((stage, index) => stages[index]?.id !== stage.id);
  useDraftState(dirty, busy, "阶段排序");
  return (
    <Modal title="调整当前投递的阶段顺序" onCancel={onCancel}>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          void onSave(ordered.map((stage) => stage.id));
        }}
      >
        <p>
          仅调整当前投递的展示顺序，不改变当前状态、日期或历史。准备投递保持第一项，终态保持末尾。
        </p>
        {failure && <p role="alert">{failure}</p>}
        <fieldset disabled={busy} className="editor-fields">
          <ol className="stage-order-list">
            {ordered.map((stage, index) => (
              <li key={stage.id}>
                <span style={{ borderColor: stage.color }}>
                  {stage.displayName}
                </span>
                <button
                  type="button"
                  aria-label={`上移 ${stage.displayName}`}
                  disabled={!canMoveStage(ordered, index, -1)}
                  onClick={() => setOrdered(moveStage(ordered, index, -1))}
                >
                  ↑
                </button>
                <button
                  type="button"
                  aria-label={`下移 ${stage.displayName}`}
                  disabled={!canMoveStage(ordered, index, 1)}
                  onClick={() => setOrdered(moveStage(ordered, index, 1))}
                >
                  ↓
                </button>
              </li>
            ))}
          </ol>
          <div className="modal-actions">
            <button type="button" onClick={onCancel}>
              取消
            </button>
            <button type="submit" className="primary" disabled={!dirty}>
              保存顺序
            </button>
          </div>
        </fieldset>
      </form>
    </Modal>
  );
}
