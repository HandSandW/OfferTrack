import { useState } from "react";
import type { WorkflowTemplate } from "../../contracts";
import { desktopApi } from "../../lib/tauri";
import { Modal } from "../../shared/Modal";
import { useDraftGuard, useDraftState } from "../../shared/draftGuard";
import { stageStates } from "../applications/tableModel";
import type {
  BatchAction,
  BatchApplied,
  BatchPreview,
  BatchRequest,
  BatchTarget,
} from "./contracts";

const stages = [
  ["preparing", "准备投递"],
  ["applied", "已投递"],
  ["assessment", "在线测评"],
  ["written_exam", "笔试考核"],
  ["interview", "面试考核"],
  ["interview_passed", "面试通过"],
  ["signing", "待签约"],
  ["offer", "offer✅️"],
  ["failed_terminal", "已挂"],
];
const states = stageStates.filter(([key]) => key !== "failed");

export function BatchDialog({
  targets,
  onClose,
  onApplied,
}: {
  targets: BatchTarget[];
  onClose: () => void;
  onApplied: () => Promise<void>;
}) {
  const [kind, setKind] = useState<BatchAction["kind"]>("archive");
  const [archived, setArchived] = useState(true);
  const [tags, setTags] = useState("");
  const [stageKey, setStageKey] = useState("applied");
  const [stateKey, setStateKey] = useState("awaitingResult");
  const [templates, setTemplates] = useState<WorkflowTemplate[]>([]);
  const [templateId, setTemplateId] = useState("");
  const [review, setReview] = useState<{
    request: BatchRequest;
    preview: BatchPreview;
  } | null>(null);
  const [result, setResult] = useState<BatchApplied | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [refreshError, setRefreshError] = useState(false);
  const { confirmLeave } = useDraftGuard();
  useDraftState(!result && (tags.length > 0 || !!review), busy, "批量修改");

  const close = async () => {
    if (!busy && (await confirmLeave())) onClose();
  };
  const fail = (failure: unknown) =>
    setError(
      failure instanceof Error
        ? failure.message
        : "操作失败，整批未保存。请重新读取投递后预览。",
    );
  const loadTemplates = async () => {
    setBusy(true);
    setError("");
    try {
      const next = await desktopApi.listWorkflowTemplates();
      setTemplates(next);
      setTemplateId(
        next.find((t) => t.id === templateId)?.id ?? next[0]?.id ?? "",
      );
    } catch (failure) {
      fail(failure);
    } finally {
      setBusy(false);
    }
  };
  const preview = async () => {
    const template = templates.find((t) => t.id === templateId);
    if (kind === "appendTemplate" && !template) {
      setError("请先读取并选择模板。");
      return;
    }
    const action: BatchAction =
      kind === "archive"
        ? { kind, archived }
        : kind === "addTags"
          ? {
              kind,
              tags: [
                ...new Set(
                  tags
                    .split(/[,，\n]/)
                    .map((s) => s.trim())
                    .filter(Boolean),
                ),
              ],
            }
          : kind === "stage"
            ? { kind, stageKey, stateKey }
            : {
                kind: "appendTemplate",
                templateId,
                revision: template!.revision,
              };
    if (action.kind === "addTags" && !action.tags.length) {
      setError("请填写至少一个标签。");
      return;
    }
    const request: BatchRequest = { version: 1, targets, action };
    setBusy(true);
    setError("");
    setReview(null);
    try {
      setReview({
        request,
        preview: await desktopApi.previewApplicationBatch(request),
      });
    } catch (failure) {
      fail(failure);
    } finally {
      setBusy(false);
    }
  };
  const apply = async () => {
    if (!review || busy || result) return;
    setBusy(true);
    setError("");
    try {
      const saved = await desktopApi.applyApplicationBatch(
        review.request,
        review.preview.fingerprint,
      );
      setResult(saved);
      setReview(null);
      // Commit success is separate from reloading the table. Never invite resubmission.
      try {
        await onApplied();
      } catch {
        setRefreshError(true);
      }
    } catch (failure) {
      fail(failure);
      setReview(null);
    } finally {
      setBusy(false);
    }
  };
  return (
    <Modal title="批量修改投递" onCancel={() => void close()}>
      <p>
        固定选中 {targets.length} 条（可能包含其他页或被筛选隐藏的记录），最多
        200 条。先逐条预览，再备份并一次性保存。无变化的记录不改版本或历史。
      </p>
      {error && <p role="alert">{error}</p>}
      {result ? (
        <>
          <p role="status">
            批量修改已完成：{result.changedCount} 条。
            {result.backupId
              ? `修改前数据库备份：${result.backupId}`
              : "没有数据变化，未新增备份。"}
          </p>
          {refreshError && (
            <p role="alert">
              修改已保存，但列表刷新失败。请关闭后刷新投递，不要重复提交。
            </p>
          )}
        </>
      ) : (
        <>
          {!review && (
            <fieldset className="editor-fields" disabled={busy}>
              <label>
                批量操作
                <select
                  value={kind}
                  onChange={(e) =>
                    setKind(e.target.value as BatchAction["kind"])
                  }
                >
                  <option value="archive">归档 / 取消归档</option>
                  <option value="addTags">添加标签（保留原标签）</option>
                  <option value="stage">修改进度</option>
                  <option value="appendTemplate">追加流程模板</option>
                </select>
              </label>
              {kind === "archive" && (
                <label>
                  归档操作
                  <select
                    value={String(archived)}
                    onChange={(e) => setArchived(e.target.value === "true")}
                  >
                    <option value="true">归档</option>
                    <option value="false">取消归档</option>
                  </select>
                </label>
              )}
              {kind === "addTags" && (
                <label>
                  待添加标签
                  <input
                    value={tags}
                    onChange={(e) => setTags(e.target.value)}
                    placeholder="逗号分隔，每个最多 40 字"
                  />
                </label>
              )}
              {kind === "stage" && (
                <>
                  <p>
                    按内置标识匹配，每条记录的实际名称会在预览中显示。首次进入已投递自动填写日期；已挂保留此前阶段；相同进度不重复写历史。自定义阶段请在单条记录中修改。
                  </p>
                  <label>
                    目标阶段
                    <select
                      value={stageKey}
                      onChange={(e) => setStageKey(e.target.value)}
                    >
                      {stages.map(([key, label]) => (
                        <option key={key} value={key}>
                          {label}
                        </option>
                      ))}
                    </select>
                  </label>
                  {!["offer", "failed_terminal"].includes(stageKey) && (
                    <label>
                      目标辅助状态
                      <select
                        value={stateKey}
                        onChange={(e) => setStateKey(e.target.value)}
                      >
                        {states.map(([key, label]) => (
                          <option key={key} value={key}>
                            {label}
                          </option>
                        ))}
                      </select>
                    </label>
                  )}
                </>
              )}
              {kind === "appendTemplate" && (
                <>
                  <p>
                    仅追加缺少的阶段和辅助状态，阶段放在终态之前；保留原名称、颜色、相对顺序、当前进度、轮次和历史。不会覆盖或同步模板，也不复制面试轮次。同名不同定义会拒绝。
                  </p>
                  <button type="button" onClick={() => void loadTemplates()}>
                    读取模板列表
                  </button>
                  <label>
                    来源模板
                    <select
                      value={templateId}
                      onChange={(e) => setTemplateId(e.target.value)}
                    >
                      <option value="">请选择</option>
                      {templates.map((t) => (
                        <option key={t.id} value={t.id}>
                          {t.name}
                        </option>
                      ))}
                    </select>
                  </label>
                </>
              )}
              <button type="button" onClick={() => void preview()}>
                预览批量修改
              </button>
            </fieldset>
          )}
          {review && (
            <section aria-label="批量修改预览">
              <p>
                将修改 {review.preview.changedCount} /{" "}
                {review.preview.items.length}{" "}
                条。任何记录或模板变化都需重新预览；不修改附件文件。
              </p>
              <ul>
                {review.preview.items.map((item) => (
                  <li key={item.id}>
                    <strong>
                      {item.companyName} · {item.positionName}
                    </strong>
                    （{item.id}）
                    <ul>
                      {(item.changes.length ? item.changes : ["无变化"]).map(
                        (change) => (
                          <li key={change}>{change}</li>
                        ),
                      )}
                    </ul>
                  </li>
                ))}
              </ul>
              <button
                type="button"
                disabled={busy}
                onClick={() => setReview(null)}
              >
                返回调整
              </button>
              <button
                type="button"
                disabled={busy}
                onClick={() => void apply()}
              >
                确认备份并保存
              </button>
            </section>
          )}
        </>
      )}
      {busy && <p role="status">正在校验或备份并提交，请勿关闭应用…</p>}
      <div className="modal-actions">
        <button type="button" disabled={busy} onClick={() => void close()}>
          {result ? "完成" : "取消"}
        </button>
      </div>
    </Modal>
  );
}
