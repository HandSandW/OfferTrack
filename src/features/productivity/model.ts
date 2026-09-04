import type { Task } from "./contracts";
import { formatOptionalLocalDateTime } from "../../shared/dateTime";
export function taskGroup(task: Task, now: Date): string {
  if (task.completedAtUtc) return "已完成";
  if (!task.dueAtUtc) return "无日期";
  const due = new Date(task.dueAtUtc);
  if (due.getTime() < now.getTime()) return "已逾期";
  if (due.toDateString() === now.toDateString()) return "今天";
  if (due.getTime() <= now.getTime() + 7 * 86400000) return "未来 7 天";
  return "以后";
}
export const taskGroups = [
  "已逾期",
  "今天",
  "未来 7 天",
  "以后",
  "无日期",
  "已完成",
];
export const priorityNames = { low: "低", normal: "普通", high: "高" };
export const eventTypes: Record<string, string> = {
  assessment: "在线测评",
  writtenExam: "笔试",
  interview: "面试",
  signing: "签约",
  other: "其他",
};
export function scheduleGroup(
  item: { finished: boolean; atUtc: string | null },
  now: Date,
): string {
  if (item.finished) return "已完成";
  if (!item.atUtc) return "无日期";
  const date = new Date(item.atUtc);
  if (date.getTime() < now.getTime()) return "已逾期";
  if (date.toDateString() === now.toDateString()) return "今天";
  if (date.getTime() <= now.getTime() + 7 * 86400000) return "未来 7 天";
  return "以后";
}
export const errorText = (error: unknown) =>
  error instanceof Error ? error.message : "读取或保存失败，请重试。";
export const dateTime = (value: string | null) =>
  formatOptionalLocalDateTime(value);
