import type {
  ApplicationDetail,
  UpdateApplicationRequest,
} from "../../contracts";

export function applicationDraft(
  detail: ApplicationDetail,
  tagsText = detail.tags.map((tag) => tag.name).join(", "),
): UpdateApplicationRequest {
  return {
    id: detail.id,
    revision: detail.revision,
    companyName: detail.companyName,
    companyType: detail.companyType,
    industry: detail.industry,
    positionName: detail.positionName,
    positionCategory: detail.positionCategory,
    workLocation: detail.workLocation,
    applicationDate: detail.applicationDate,
    applicationUrl: detail.applicationUrl,
    announcementUrl: detail.announcementUrl,
    companyUrl: detail.companyUrl,
    positionUrl: detail.positionUrl,
    positionDescription: detail.positionDescription,
    notes: detail.notes,
    tags: tagsText
      .split(/[,，]/)
      .map((name) => name.trim())
      .filter(Boolean),
    customFields: detail.customFields,
  };
}

export function localDateTime(utc: string | null): string {
  if (!utc) return "";
  const date = new Date(utc);
  if (!Number.isFinite(date.getTime())) return "";
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}

export function utcDateTime(
  input: string,
  original: string | null,
): string | null {
  if (!input) return null;
  // Keep the original offset/subsecond precision when the field was not edited.
  if (input === localDateTime(original)) return original;
  if (!/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}(:\d{2})?$/.test(input))
    throw new Error("请输入有效的本地日期和时间。");
  const date = new Date(input);
  const normalized = input.length === 16 ? `${input}:00` : input;
  if (
    !Number.isFinite(date.getTime()) ||
    localDateTime(date.toISOString()) !== normalized
  )
    throw new Error("该本地时间无效，请检查日期或夏令时变化。");
  return date.toISOString();
}
