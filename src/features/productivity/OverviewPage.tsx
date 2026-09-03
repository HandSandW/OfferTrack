import { useState } from "react";
import { desktopApi } from "../../lib/tauri";
import { useDraftState } from "../../shared/draftGuard";
import { companyTypeName } from "../applications/tableModel";
import type { Bucket, Drilldown, Reminder, ScheduleScope } from "./contracts";
import { dateTime, errorText } from "./model";
import { useOverview } from "./useOverview";

export function OverviewPage({
  writable,
  onError,
  onDrilldown,
  onTask,
  onSettings,
  onNew,
  onEvent,
  onSchedule,
}: {
  writable: boolean;
  onError: (error: unknown) => void;
  onDrilldown: (value: Drilldown) => void;
  onTask: (id?: string) => void;
  onSettings: () => void;
  onNew: () => void;
  onEvent?: (id: string) => void;
  onSchedule?: (scope: ScheduleScope) => void;
}) {
  const { data, error, loading, refresh } = useOverview(onError);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  useDraftState(false, busy, "处理提醒");
  const respond = async (reminder: Reminder, snooze: boolean) => {
    setBusy(true);
    setMessage("");
    try {
      await desktopApi.respondToReminder(
        reminder.key,
        reminder.fingerprint,
        snooze,
      );
      setMessage(
        snooze
          ? "已推迟 24 小时；来源变化或紧急程度升级会重新提醒。"
          : "本次提醒已处理，不会修改投递状态或完成待办。",
      );
      await refresh();
    } catch (failure) {
      setMessage(errorText(failure));
      onError(failure);
    } finally {
      setBusy(false);
    }
  };
  const buckets = (title: string, values: Bucket[]) => (
    <section className="panel-page">
      <h3>{title}</h3>
      {values.length ? (
        <div className="bucket-list">
          {values.map((b, i) => (
            <button
              disabled={busy}
              key={`${b.label}:${i}`}
              onClick={() => onDrilldown(b)}
            >
              {b.label} <strong>{b.ids.length}</strong>
            </button>
          ))}
        </div>
      ) : (
        <p>暂无足够数据。</p>
      )}
    </section>
  );
  return (
    <section className="overview-page">
      <div className="section-actions">
        <h2>求职概览</h2>
        <button disabled={!writable || busy} onClick={onNew}>
          新建投递
        </button>
        <button disabled={loading || busy} onClick={() => void refresh()}>
          刷新概览
        </button>
        <button disabled={busy} onClick={onSettings}>
          调整提醒规则
        </button>
      </div>
      <p className="muted">
        统计范围：未归档、未删除的投递（含终态）。近 7/30
        天含今天，按本机日期；实际投递以投递日期为准。提醒在各页每分钟及回到窗口时更新，不后台常驻。简历提醒依据最近文件索引。
      </p>
      {loading && <p role="status">正在更新概览…</p>}
      {error && (
        <p role="alert">{error} 上次结果保留，不能将读取失败视为空数据。</p>
      )}
      {message && <p role="status">{message}</p>}
      {data && (
        <>
          <p className="muted">统计于 {dateTime(data.generatedAtUtc)}</p>
          <div className="metric-grid">
            {data.metrics.map((metric) => (
              <button
                disabled={busy}
                className="metric-card"
                aria-label={`${metric.label} ${metric.ids.length}`}
                key={metric.label}
                onClick={() => onDrilldown(metric)}
              >
                <span>{metric.label}</span>
                <strong>{metric.ids.length}</strong>
              </button>
            ))}
          </div>
          <div className="metric-grid">
            {data.dueMetrics.map((metric) => (
              <button
                className="metric-card"
                key={metric.label}
                disabled={busy}
                aria-label={`${metric.label} ${metric.keys.length}`}
                onClick={() => onSchedule?.(metric)}
              >
                <span>{metric.label}</span>
                <strong>{metric.keys.length}</strong>
              </button>
            ))}
          </div>
          <p className="muted">
            到期数按日程事项去重，提醒已处理/关闭不改变此统计。事件有截止则按截止，否则按计划时间；关联轮次不重复计数。
          </p>
          <section className="panel-page">
            <h3>重要提醒 · {data.reminders.length}</h3>
            <p className="muted">
              “已处理”仅隐藏当前提醒，不自动改变阶段或完成待办；来源修改或紧急程度升级时重新显示。
            </p>
            {!data.reminders.length && <p>当前没有触发的提醒。</p>}
            {data.reminders.map((r) => (
              <article className={`reminder-item ${r.severity}`} key={r.key}>
                <h4>{r.label}</h4>
                <p>
                  {r.severity === "overdue"
                    ? "逾期"
                    : r.severity === "urgent"
                      ? "紧急"
                      : "提醒"}{" "}
                  · {r.reason}
                </p>
                <div className="section-actions">
                  <button
                    disabled={busy}
                    onClick={() =>
                      r.sourceKind === "task"
                        ? onTask(r.sourceId)
                        : r.sourceKind === "event"
                          ? onEvent?.(r.sourceId)
                          : onDrilldown({
                              label: r.label,
                              ids: [r.applicationId!],
                            })
                    }
                  >
                    {r.sourceKind === "task"
                      ? "打开待办"
                      : r.sourceKind === "event"
                        ? "打开事件"
                        : "打开投递"}
                  </button>
                  <button
                    disabled={!writable || busy || loading || !!error}
                    onClick={() => void respond(r, false)}
                  >
                    本次已处理
                  </button>
                  <button
                    disabled={!writable || busy || loading || !!error}
                    onClick={() => void respond(r, true)}
                  >
                    24 小时后提醒
                  </button>
                </div>
              </article>
            ))}
          </section>
          <div className="overview-columns">
            <section className="panel-page">
              <h3>近期待办</h3>
              <button disabled={busy} onClick={() => onTask()}>
                管理全部待办
              </button>
              {data.tasks
                .filter((t) => !t.completedAtUtc && !t.applicationArchived)
                .sort(
                  (a, b) =>
                    Number(b.priority === "high") -
                      Number(a.priority === "high") ||
                    (a.dueAtUtc ?? "~").localeCompare(b.dueAtUtc ?? "~"),
                )
                .slice(0, 8)
                .map((t) => (
                  <p key={t.id}>
                    <button disabled={busy} onClick={() => onTask(t.id)}>
                      {t.title}
                    </button>{" "}
                    · {dateTime(t.dueAtUtc)}
                  </p>
                ))}
            </section>
            <section className="panel-page">
              <h3>待参加的面试日程</h3>
              {data.interviews.length ? (
                data.interviews.slice(0, 12).map((i) => (
                  <p key={i.id}>
                    <button
                      disabled={busy}
                      onClick={() =>
                        onDrilldown({ label: i.label, ids: [i.applicationId] })
                      }
                    >
                      {i.label}
                    </button>{" "}
                    · {dateTime(i.scheduledAtUtc)}
                  </p>
                ))
              ) : (
                <p>暂无未完成的面试日程。</p>
              )}
            </section>
          </div>
          <div className="overview-columns">
            {buckets("当前阶段分布", data.stages)}
            {buckets("历史阶段到达情况", data.funnel)}
          </div>
          <p className="muted">
            阶段到达只计有历史证据的记录，不推断跳过的阶段；流程不同、样本不足时不计算转化率或录用概率。
          </p>
          <section className="panel-page">
            <h3>最近 30 天趋势</h3>
            <div className="trend-table">
              <table>
                <thead>
                  <tr>
                    <th>日期</th>
                    <th>创建</th>
                    <th>实际投递</th>
                  </tr>
                </thead>
                <tbody>
                  {data.trend.map((d) => (
                    <tr key={d.date}>
                      <td>{d.date}</td>
                      <td>
                        <button
                          disabled={busy}
                          onClick={() =>
                            onDrilldown({
                              label: `${d.date} 创建`,
                              ids: d.createdIds,
                            })
                          }
                        >
                          {d.createdIds.length}
                        </button>
                      </td>
                      <td>
                        <button
                          disabled={busy}
                          onClick={() =>
                            onDrilldown({
                              label: `${d.date} 投递`,
                              ids: d.appliedIds,
                            })
                          }
                        >
                          {d.appliedIds.length}
                        </button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </section>
          <div className="overview-columns">
            {buckets("行业", data.industries)}
            {buckets("工作地点", data.locations)}
            {buckets(
              "公司性质",
              data.companyTypes.map((b) => ({
                ...b,
                label: companyTypeName(b.label),
              })),
            )}
          </div>
          <section className="panel-page">
            <h3>最近更新</h3>
            {data.records.slice(0, 10).map((r) => (
              <p key={r.id}>
                <button
                  disabled={busy}
                  onClick={() => onDrilldown({ label: r.label, ids: [r.id] })}
                >
                  {r.label}
                </button>{" "}
                · {dateTime(r.updatedAtUtc)}
              </p>
            ))}
          </section>
        </>
      )}
    </section>
  );
}
