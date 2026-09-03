import { useState, type FormEvent } from "react";
import type { ApplicationListItem } from "../../contracts";
import { desktopApi } from "../../lib/tauri";
import { Modal } from "../../shared/Modal";
import { useDraftGuard, useDraftState } from "../../shared/draftGuard";
import { localDateTime, utcDateTime } from "../applications/editorModel";
import type { Task } from "./contracts";
import { errorText, priorityNames } from "./model";

export function TaskEditor({
  task,
  applications,
  onSaved,
  onCancel,
  onError,
}: {
  task: Task | null;
  applications: ApplicationListItem[];
  onSaved: (task: Task) => void;
  onCancel: () => void;
  onError: (error: unknown) => void;
}) {
  const [initial] = useState(() => ({
    title: task?.title ?? "",
    notes: task?.notes ?? "",
    applicationId: task?.applicationId ?? "",
    priority: task?.priority ?? "normal",
    due: localDateTime(task?.dueAtUtc ?? null),
    remind: localDateTime(task?.remindAtUtc ?? null),
  }));
  const [form, setForm] = useState(initial);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const { confirmLeave } = useDraftGuard();
  useDraftState(
    JSON.stringify(initial) !== JSON.stringify(form),
    busy,
    "待办编辑",
  );
  const cancel = async () => {
    if (await confirmLeave()) onCancel();
  };
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setError("");
    try {
      const saved = await desktopApi.saveTask({
        id: task?.id ?? null,
        revision: task?.revision ?? null,
        title: form.title,
        notes: form.notes,
        applicationId: form.applicationId || null,
        priority: form.priority,
        dueAtUtc: utcDateTime(form.due, task?.dueAtUtc ?? null),
        remindAtUtc: utcDateTime(form.remind, task?.remindAtUtc ?? null),
      });
      onSaved(saved);
    } catch (failure) {
      setError(errorText(failure));
      onError(failure);
    } finally {
      setBusy(false);
    }
  };
  return (
    <Modal
      title={task ? "编辑待办" : "新建待办"}
      onCancel={() => void cancel()}
    >
      <form onSubmit={(event) => void submit(event)}>
        <fieldset disabled={busy} className="task-form">
          <label>
            待办标题
            <input
              required
              maxLength={200}
              value={form.title}
              onChange={(e) => setForm({ ...form, title: e.target.value })}
            />
          </label>
          <label>
            关联投递
            <select
              value={form.applicationId}
              onChange={(e) =>
                setForm({ ...form, applicationId: e.target.value })
              }
            >
              <option value="">不关联（通用求职事项）</option>
              {applications.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.companyName} · {a.positionName}
                  {a.archivedAtUtc ? "（已归档）" : ""}
                </option>
              ))}
            </select>
          </label>
          <label>
            优先级
            <select
              value={form.priority}
              onChange={(e) =>
                setForm({
                  ...form,
                  priority: e.target.value as Task["priority"],
                })
              }
            >
              {Object.entries(priorityNames).map(([key, label]) => (
                <option key={key} value={key}>
                  {label}
                </option>
              ))}
            </select>
          </label>
          <label>
            截止时间
            <input
              type="datetime-local"
              step="1"
              value={form.due}
              onChange={(e) => setForm({ ...form, due: e.target.value })}
            />
          </label>
          <label>
            提醒时间
            <input
              type="datetime-local"
              step="1"
              value={form.remind}
              onChange={(e) => setForm({ ...form, remind: e.target.value })}
            />
          </label>
          <label>
            待办备注
            <textarea
              maxLength={100000}
              value={form.notes}
              onChange={(e) => setForm({ ...form, notes: e.target.value })}
            />
          </label>
        </fieldset>
        <p className="muted">
          时间按本机时区显示。失败保留输入；版本冲突请先保留文字，再取消、刷新后重新编辑。
        </p>
        {error && <p role="alert">{error}</p>}
        <div className="section-actions">
          <button type="button" disabled={busy} onClick={() => void cancel()}>
            取消
          </button>
          <button className="primary" disabled={busy} type="submit">
            {busy ? "正在保存…" : "保存待办"}
          </button>
        </div>
      </form>
    </Modal>
  );
}
