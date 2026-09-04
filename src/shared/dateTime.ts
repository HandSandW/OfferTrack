export interface DateTimeFormatOptions {
  timeZone?: string;
}

const partsValue = (
  parts: Intl.DateTimeFormatPart[],
  type: Intl.DateTimeFormatPartTypes,
) => parts.find((part) => part.type === type)?.value ?? "";

export function formatLocalDateTime(
  value: string | Date,
  options: DateTimeFormatOptions = {},
): string {
  const date = value instanceof Date ? value : new Date(value);
  if (Number.isNaN(date.getTime())) return "时间无效";
  const formatter = new Intl.DateTimeFormat("zh-CN", {
    ...(options.timeZone ? { timeZone: options.timeZone } : {}),
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hourCycle: "h23",
    timeZoneName: "longOffset",
  });
  const parts = formatter.formatToParts(date);
  const zone = partsValue(parts, "timeZoneName")
    .replace(/^GMT$/, "UTC+00:00")
    .replace(/^GMT/, "UTC");
  return `${partsValue(parts, "year")}-${partsValue(parts, "month")}-${partsValue(parts, "day")} ${partsValue(parts, "hour")}:${partsValue(parts, "minute")}:${partsValue(parts, "second")} ${zone}`;
}

export function formatOptionalLocalDateTime(value: string | null): string {
  return value ? formatLocalDateTime(value) : "未设置";
}
