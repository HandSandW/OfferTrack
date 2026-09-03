import { useEffect, useState } from "react";
import type { ApplicationListItem } from "../../contracts";
import { desktopApi } from "../../lib/tauri";
import { useDraftState } from "../../shared/draftGuard";
import type { Task } from "./contracts";
import {
  dateTime,
  errorText,
  priorityNames,
  taskGroup,
  taskGroups,
} from "./model";
import { TaskEditor } from "./TaskEditor";

export function TasksPage({
  writable,
  onError,
  onOpenApplication,
  initialTaskId,
}: {
  writable: boolean;
  onError: (error: unknown) => void;
  onOpenApplication: (id: string, archived: boolean) => void;
  initialTaskId?: string | undefined;
}) {
  const [tasks, setTasks] = useState<Task[]>([]);
  const [applications, setApplications] = useState<ApplicationListItem[]>([]);
  const [editor, setEditor] = useState<{ task: Task | null } | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [attempt, setAttempt] = useState(0);
  const [filter, setFilter] = useState("全部");
  const [focusId, setFocusId] = useState(initialTaskId ?? "");
  const [now, setNow] = useState(() => new Date());
  useDraftState(false, busy, "待办完成状态");
  useEffect(() => {
    const timer = window.setInterval(() => setNow(new Date()), 60_000);
    return () => window.clearInterval(timer);
  }, []);
  useEffect(() => {
    let active = true;
    void Promise.all([
      desktopApi.listTasks(),
      desktopApi.listApplications("active"),
      desktopApi.listApplications("archived"),
    ])
      .then(([items, activeItems, archivedItems]) => {
        if (active) {
          setTasks(items);
          setApplications([...activeItems, ...archivedItems]);
          setError("");
        }
      })
      .catch((failure: unknown) => {
        if (active) {
          setError(errorText(failure));
          onError(failure);
        }
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [attempt, onError]);
  const complete = async (task: Task) => {
    setBusy(true);
    setError("");
    try {
      const saved = await desktopApi.completeTask(
        task.id,
        task.revision,
        !task.completedAtUtc,
      );
      setTasks((items) => items.map((t) => (t.id === saved.id ? saved : t)));
    } catch (failure) {
      setError(errorText(failure));
      onError(failure);
    } finally {
      setBusy(false);
    }
  };
  const shown = tasks.filter(
    (t) =>
      (!focusId || t.id === focusId) &&
      (filter === "全部" || taskGroup(t, now) === filter),
  );
  return (
    <section className="panel-page">
      <h2>待办列表</h2>
      <p className="muted">
        支持通用求职事项与关联投递。完成后保留历史；已归档投递的待办保留在此列表，但不产生提醒。已删除投递的待办随来源隐藏，恢复投递后重新显示。招聘事件与面试可在综合日程查看。
      </p>
      <div className="section-actions">
        <button
          disabled={!writable || busy || loading || !!error}
          onClick={() => setEditor({ task: null })}
        >
          新建待办
        </button>
        <button
          disabled={busy || loading}
          onClick={() => {
            setLoading(true);
            setAttempt((n) => n + 1);
          }}
        >
          刷新待办
        </button>
        <label>
          待办分组
          <select value={filter} onChange={(e) => setFilter(e.target.value)}>
            <option>全部</option>
            {taskGroups.map((g) => (
              <option key={g}>{g}</option>
            ))}
          </select>
        </label>
        {focusId && (
          <button onClick={() => setFocusId("")}>清除提醒定位</button>
        )}
      </div>
      {loading && <p role="status">正在读取待办…</p>}
      {error && <p role="alert">{error} 旧列表保留，刷新后再编辑。</p>}
      {!loading && !error && shown.length === 0 && <p>当前范围暂无待办。</p>}
      {taskGroups.map((group) => {
        const items = shown
          .filter((t) => taskGroup(t, now) === group)
          .sort(
            (a, b) =>
              Number(b.priority === "high") - Number(a.priority === "high") ||
              a.createdAtUtc.localeCompare(b.createdAtUtc),
          );
        return (
          items.length > 0 && (
            <section key={group} className="task-group">
              <h3>
                {group} · {items.length}
              </h3>
              {items.map((t) => (
                <article className="task-item" key={t.id}>
                  <h4>
                    {t.title}{" "}
                    <span className="muted">
                      {priorityNames[t.priority]}优先级
                    </span>
                  </h4>
                  <p>
                    截止：{dateTime(t.dueAtUtc)} · 提醒：
                    {dateTime(t.remindAtUtc)}
                    {t.completedAtUtc &&
                      ` · 完成：${dateTime(t.completedAtUtc)}`}
                  </p>
                  {t.notes && <p className="task-notes">{t.notes}</p>}
                  <div className="section-actions">
                    {t.applicationId && (
                      <button
                        disabled={busy}
                        onClick={() =>
                          onOpenApplication(
                            t.applicationId!,
                            t.applicationArchived,
                          )
                        }
                      >
                        {t.applicationLabel}
                        {t.applicationArchived && "（已归档）"}
                      </button>
                    )}
                    <button
                      disabled={!writable || busy || loading || !!error}
                      onClick={() => setEditor({ task: t })}
                    >
                      编辑待办
                    </button>
                    <button
                      disabled={!writable || busy || loading || !!error}
                      onClick={() => void complete(t)}
                    >
                      {t.completedAtUtc ? "重新打开待办" : "标记完成"}
                    </button>
                  </div>
                </article>
              ))}
            </section>
          )
        );
      })}
      {editor && (
        <TaskEditor
          task={editor.task}
          applications={applications}
          onCancel={() => setEditor(null)}
          onError={onError}
          onSaved={(saved) => {
            setTasks((items) => [
              saved,
              ...items.filter((t) => t.id !== saved.id),
            ]);
            setEditor(null);
            setFocusId("");
            setFilter("全部");
          }}
        />
      )}
    </section>
  );
}
