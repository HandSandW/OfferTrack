import { useEffect, useState } from "react";
import type { ApplicationListItem } from "../../contracts";
import { desktopApi } from "../../lib/tauri";
import { useDraftState } from "../../shared/draftGuard";
import { UrlLink } from "../../shared/UrlLink";
import type { RecruitmentEvent } from "./contracts";
import { dateTime, errorText, eventTypes } from "./model";
import { EventEditor } from "./EventEditor";

export function EventsPage({
  writable,
  onError,
  onOpenApplication,
  initialEventId,
}: {
  writable: boolean;
  onError: (error: unknown) => void;
  onOpenApplication: (id: string, archived: boolean) => void;
  initialEventId?: string | undefined;
}) {
  const [events, setEvents] = useState<RecruitmentEvent[]>([]);
  const [applications, setApplications] = useState<ApplicationListItem[]>([]);
  const [editor, setEditor] = useState<{
    event: RecruitmentEvent | null;
  } | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [attempt, setAttempt] = useState(0);
  const [focus, setFocus] = useState(initialEventId ?? "");
  const [filter, setFilter] = useState("all");
  useDraftState(false, busy, "招聘事件完成状态");
  useEffect(() => {
    let active = true;
    void Promise.all([
      desktopApi.listRecruitmentEvents(),
      desktopApi.listApplications("active"),
      desktopApi.listApplications("archived"),
    ])
      .then(([items, a, b]) => {
        if (active) {
          setEvents(items);
          setApplications([...a, ...b]);
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
  const complete = async (event: RecruitmentEvent) => {
    setBusy(true);
    setError("");
    try {
      const saved = await desktopApi.completeRecruitmentEvent(
        event.id,
        event.revision,
        !event.finished,
      );
      setEvents((items) => items.map((e) => (e.id === saved.id ? saved : e)));
    } catch (failure) {
      setError(errorText(failure));
      onError(failure);
    } finally {
      setBusy(false);
    }
  };
  const shown = events.filter(
    (e) =>
      (!focus || focus === e.id) &&
      (filter === "all" || (filter === "done" ? e.finished : !e.finished)),
  );
  return (
    <section className="panel-page">
      <h2>招聘事件</h2>
      <p className="muted">
        归档和终态事件仍可查看，但不进入综合日程或提醒。删除投递后随来源隐藏；完成不删除资料。关联轮次以其时间和完成情况为准。
      </p>
      <div className="section-actions">
        <button
          disabled={!writable || busy || loading || !!error}
          onClick={() => setEditor({ event: null })}
        >
          新建招聘事件
        </button>
        <button
          disabled={busy || loading}
          onClick={() => {
            setLoading(true);
            setAttempt((n) => n + 1);
          }}
        >
          刷新事件
        </button>
        <label>
          事件状态
          <select value={filter} onChange={(e) => setFilter(e.target.value)}>
            <option value="all">全部</option>
            <option value="open">未完成</option>
            <option value="done">已完成</option>
          </select>
        </label>
        {focus && <button onClick={() => setFocus("")}>清除事件定位</button>}
      </div>
      {loading && <p role="status">正在读取招聘事件…</p>}
      {error && <p role="alert">{error} 旧列表保留，刷新后再编辑。</p>}
      {!loading && !error && !shown.length && <p>当前范围暂无事件。</p>}
      {shown.map((e) => (
        <article key={e.id} className="task-item">
          <h3>
            {e.title} · {eventTypes[e.eventType] ?? e.eventType} ·{" "}
            {e.finished ? "已完成" : "未完成"}
          </h3>
          <p>
            计划：{dateTime(e.startsAtUtc)} · 截止：{dateTime(e.deadlineAtUtc)}
          </p>
          {e.interviewRoundId && (
            <p>
              关联轮次：{e.interviewRoundName}
              ；在投递流程中修改时间、结果与完成状态。
            </p>
          )}
          {e.completedAtUtc && <p>完成：{dateTime(e.completedAtUtc)}</p>}
          {e.location && <p>地点：{e.location}</p>}
          {e.meetingUrl && (
            <p>
              会议：
              <UrlLink value={e.meetingUrl} onError={onError} />
            </p>
          )}
          {e.result && <p className="task-notes">结果：{e.result}</p>}
          {e.notes && <p className="task-notes">{e.notes}</p>}
          <div className="section-actions">
            {e.applicationId && (
              <button
                disabled={busy}
                onClick={() =>
                  onOpenApplication(e.applicationId!, e.applicationArchived)
                }
              >
                {e.applicationLabel}
                {e.applicationArchived
                  ? "（已归档）"
                  : e.applicationTerminal
                    ? "（已结束）"
                    : ""}
              </button>
            )}
            <button
              disabled={!writable || busy || loading || !!error}
              onClick={() => setEditor({ event: e })}
            >
              编辑事件
            </button>
            {!e.interviewRoundId && (
              <button
                disabled={!writable || busy || loading || !!error}
                onClick={() => void complete(e)}
              >
                {e.finished ? "重新打开事件" : "完成事件"}
              </button>
            )}
          </div>
        </article>
      ))}
      {editor && (
        <EventEditor
          event={editor.event}
          applications={applications}
          onCancel={() => setEditor(null)}
          onError={onError}
          onSaved={(saved) => {
            setEvents((items) => [
              saved,
              ...items.filter((e) => e.id !== saved.id),
            ]);
            setEditor(null);
            setFocus("");
            setFilter("all");
          }}
        />
      )}
    </section>
  );
}
