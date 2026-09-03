import { useState } from "react";
import type { ScheduleEntry, ScheduleScope } from "./contracts";
import { dateTime, scheduleGroup, taskGroups } from "./model";
import { useOverview } from "./useOverview";

export function SchedulePage({
  onError,
  onOpen,
  initialScope,
}: {
  onError: (error: unknown) => void;
  onOpen: (entry: ScheduleEntry) => void;
  initialScope?: ScheduleScope | undefined;
}) {
  const { data, error, loading, refresh } = useOverview(onError);
  const [scope, setScope] = useState(initialScope);
  const [group, setGroup] = useState("全部");
  const [kind, setKind] = useState("");
  const now = new Date(data?.generatedAtUtc ?? 0);
  const shown = (data?.schedule ?? []).filter(
    (e) =>
      (!scope || scope.keys.includes(e.key)) &&
      (!kind || kind === e.sourceKind) &&
      (group === "全部" || scheduleGroup(e, now) === group),
  );
  return (
    <section className="panel-page">
      <h2>综合日程</h2>
      <p className="muted">
        待办按截止时间、事件优先按截止否则按计划时间归组；面试按轮次计划。关联事件替代原轮次，不重复计数。归档事项和终态投递事件不列入；未关联轮次这里只显示未完成计划，历史仍在投递流程中。
      </p>
      {scope && (
        <div className="notice info">
          来自概览：{scope.label} · {scope.keys.length} 条快照范围{" "}
          <button onClick={() => setScope(undefined)}>清除日程范围</button>
        </div>
      )}
      <div className="section-actions">
        <button disabled={loading} onClick={() => void refresh()}>
          刷新日程
        </button>
        <label>
          日程分组
          <select value={group} onChange={(e) => setGroup(e.target.value)}>
            <option>全部</option>
            {taskGroups.map((g) => (
              <option key={g}>{g}</option>
            ))}
          </select>
        </label>
        <label>
          日程来源
          <select value={kind} onChange={(e) => setKind(e.target.value)}>
            <option value="">全部来源</option>
            <option value="task">待办</option>
            <option value="event">招聘事件</option>
            <option value="interview">面试轮次</option>
          </select>
        </label>
      </div>
      {loading && <p role="status">正在更新日程…</p>}
      {error && <p role="alert">{error} 旧结果保留，请刷新。</p>}
      {!loading && !error && !shown.length && <p>当前范围暂无日程。</p>}
      {taskGroups.map((g) => {
        const items = shown
          .filter((e) => scheduleGroup(e, now) === g)
          .sort(
            (a, b) =>
              (a.atUtc ?? "~").localeCompare(b.atUtc ?? "~") ||
              a.key.localeCompare(b.key),
          );
        return (
          items.length > 0 && (
            <section key={g} className="task-group">
              <h3>
                {g} · {items.length}
              </h3>
              {items.map((e) => (
                <article className="task-item" key={e.key}>
                  <button onClick={() => onOpen(e)}>{e.label}</button>
                  <p>
                    {e.sourceKind === "task"
                      ? "待办"
                      : e.sourceKind === "event"
                        ? "招聘事件"
                        : "面试轮次"}{" "}
                    · 归组时间：{dateTime(e.atUtc)}
                    {e.highPriority ? " · 高优先级" : ""}
                  </p>
                  {e.startsAtUtc && e.startsAtUtc !== e.atUtc && (
                    <p>计划时间：{dateTime(e.startsAtUtc)}</p>
                  )}
                </article>
              ))}
            </section>
          )
        );
      })}
    </section>
  );
}
