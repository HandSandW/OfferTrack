import { useEffect, useState, type FormEvent } from "react";
import type { ApplicationListItem, InterviewRound } from "../../contracts";
import { desktopApi } from "../../lib/tauri";
import { Modal } from "../../shared/Modal";
import { useDraftGuard, useDraftState } from "../../shared/draftGuard";
import { localDateTime, utcDateTime } from "../applications/editorModel";
import type { RecruitmentEvent } from "./contracts";
import { dateTime, errorText, eventTypes } from "./model";

export function EventEditor({
  event,
  applications,
  onSaved,
  onCancel,
  onError,
}: {
  event: RecruitmentEvent | null;
  applications: ApplicationListItem[];
  onSaved: (event: RecruitmentEvent) => void;
  onCancel: () => void;
  onError: (error: unknown) => void;
}) {
  const [initial] = useState(() => ({
    applicationId: event?.applicationId ?? "",
    eventType: event?.eventType ?? "assessment",
    title: event?.title ?? "",
    notes: event?.notes ?? "",
    start: localDateTime(event?.startsAtUtc ?? null),
    deadline: localDateTime(event?.deadlineAtUtc ?? null),
    roundId: event?.interviewRoundId ?? "",
    location: event?.location ?? "",
    meetingUrl: event?.meetingUrl ?? "",
    result: event?.result ?? "",
  }));
  const [form, setForm] = useState(initial);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [rounds, setRounds] = useState<InterviewRound[]>([]);
  const [roundLoading, setRoundLoading] = useState(false);
  const [roundError, setRoundError] = useState("");
  const [attempt, setAttempt] = useState(0);
  const { confirmLeave } = useDraftGuard();
  useDraftState(
    JSON.stringify(initial) !== JSON.stringify(form),
    busy,
    "招聘事件编辑",
  );
  useEffect(() => {
    let active = true;
    setRounds([]);
    setRoundError("");
    if (!form.applicationId || form.eventType !== "interview") {
      setRoundLoading(false);
      return;
    }
    setRoundLoading(true);
    void desktopApi
      .getApplication(form.applicationId)
      .then((detail) => {
        if (!active) return;
        if (detail.deletedAtUtc)
          throw new Error("关联投递已删除，请刷新列表。");
        setRounds(detail.interviewRounds);
      })
      .catch((failure: unknown) => {
        if (active) setRoundError(errorText(failure));
      })
      .finally(() => {
        if (active) setRoundLoading(false);
      });
    return () => {
      active = false;
    };
  }, [form.applicationId, form.eventType, attempt]);
  const cancel = async () => {
    if (await confirmLeave()) onCancel();
  };
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError("");
    try {
      const saved = await desktopApi.saveRecruitmentEvent({
        id: event?.id ?? null,
        revision: event?.revision ?? null,
        applicationId: form.applicationId,
        eventType: form.eventType,
        title: form.title,
        notes: form.notes,
        interviewRoundId: form.roundId || null,
        startsAtUtc: form.roundId
          ? null
          : utcDateTime(form.start, event?.startsAtUtc ?? null),
        deadlineAtUtc: utcDateTime(form.deadline, event?.deadlineAtUtc ?? null),
        location: form.location,
        meetingUrl: form.meetingUrl.trim() || null,
        result: form.roundId ? "" : form.result,
      });
      onSaved(saved);
    } catch (failure) {
      setError(errorText(failure));
      onError(failure);
    } finally {
      setBusy(false);
    }
  };
  const round = rounds.find((r) => r.id === form.roundId);
  return (
    <Modal
      title={event ? "编辑招聘事件" : "新建招聘事件"}
      onCancel={() => void cancel()}
    >
      <form onSubmit={(e) => void submit(e)}>
        <fieldset className="task-form" disabled={busy}>
          <label>
            事件标题
            <input
              required
              maxLength={200}
              value={form.title}
              onChange={(e) => setForm({ ...form, title: e.target.value })}
            />
          </label>
          <label>
            事件关联投递
            <select
              required
              value={form.applicationId}
              onChange={(e) =>
                setForm({ ...form, applicationId: e.target.value, roundId: "" })
              }
            >
              <option value="">选择投递</option>
              {applications.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.companyName} · {a.positionName}
                  {a.archivedAtUtc ? "（已归档）" : ""}
                </option>
              ))}
            </select>
          </label>
          <label>
            事件类型
            <select
              value={form.eventType}
              onChange={(e) =>
                setForm({ ...form, eventType: e.target.value, roundId: "" })
              }
            >
              {Object.entries(eventTypes).map(([key, label]) => (
                <option key={key} value={key}>
                  {label}
                </option>
              ))}
            </select>
          </label>
          {form.eventType === "interview" && (
            <label>
              关联面试轮次
              <select
                disabled={roundLoading || !!roundError}
                value={form.roundId}
                onChange={(e) => setForm({ ...form, roundId: e.target.value })}
              >
                <option value="">独立事件（不关联轮次）</option>
                {rounds.map((r) => (
                  <option key={r.id} value={r.id}>
                    {r.displayName}
                  </option>
                ))}
              </select>
            </label>
          )}
          {roundLoading && <p role="status">正在读取面试轮次…</p>}
          {roundError && (
            <p role="alert">
              {roundError}{" "}
              <button type="button" onClick={() => setAttempt((n) => n + 1)}>
                重读轮次
              </button>
            </p>
          )}
          {form.roundId ? (
            <p>
              轮次计划：{dateTime(round?.scheduledAtUtc ?? null)}
              。时间、结果及完成状态以轮次为准，请在投递流程中编辑。关联后不会再单独列出该轮次日程。
            </p>
          ) : (
            <>
              <label>
                计划时间
                <input
                  required
                  type="datetime-local"
                  step="1"
                  value={form.start}
                  onChange={(e) => setForm({ ...form, start: e.target.value })}
                />
              </label>
              <label>
                事件结果
                <textarea
                  maxLength={100000}
                  value={form.result}
                  onChange={(e) => setForm({ ...form, result: e.target.value })}
                />
              </label>
            </>
          )}
          <label>
            事件截止时间
            <input
              type="datetime-local"
              step="1"
              value={form.deadline}
              onChange={(e) => setForm({ ...form, deadline: e.target.value })}
            />
          </label>
          <label>
            地点
            <input
              maxLength={1000}
              value={form.location}
              onChange={(e) => setForm({ ...form, location: e.target.value })}
            />
          </label>
          <label>
            会议链接
            <input
              type="url"
              maxLength={4096}
              placeholder="https://"
              value={form.meetingUrl}
              onChange={(e) => setForm({ ...form, meetingUrl: e.target.value })}
            />
          </label>
          <label>
            事件备注
            <textarea
              maxLength={100000}
              value={form.notes}
              onChange={(e) => setForm({ ...form, notes: e.target.value })}
            />
          </label>
        </fieldset>
        <p className="muted">
          时间按本机时区输入；截止不可早于计划时间。一轮最多关联一个事件；失败保留输入。
        </p>
        {error && <p role="alert">{error}</p>}
        <div className="section-actions">
          <button type="button" disabled={busy} onClick={() => void cancel()}>
            取消
          </button>
          <button type="submit" disabled={busy || roundLoading || !!roundError}>
            {busy ? "正在保存…" : "保存事件"}
          </button>
        </div>
      </form>
    </Modal>
  );
}
