import type { FieldDefinition, FieldDefinitionRequest } from "../../contracts";

export const fieldTypes = [
  ["text", "文本"],
  ["number", "数字"],
  ["date", "日期"],
  ["boolean", "布尔"],
  ["url", "网址"],
  ["select", "选项"],
] as const;
export interface FieldDraft {
  name: string;
  type: string;
  options: string;
}
export function fieldDraft(field: FieldDefinition | null): FieldDraft {
  const config = field?.config;
  const options =
    typeof config === "object" &&
    config !== null &&
    "options" in config &&
    Array.isArray(config.options)
      ? config.options.filter((v): v is string => typeof v === "string")
      : [];
  return {
    name: field?.displayName ?? "",
    type: field?.fieldType ?? "text",
    options: options.join(", "),
  };
}
export function optionValues(raw: string): string[] {
  return [
    ...new Set(
      raw
        .split(/[,，]/)
        .map((s) => s.trim())
        .filter(Boolean),
    ),
  ];
}
export function fieldRequest(
  field: FieldDefinition | null,
  draft: FieldDraft,
): FieldDefinitionRequest {
  const existing =
    field?.fieldType === draft.type &&
    typeof field.config === "object" &&
    field.config !== null
      ? field.config
      : {};
  return {
    id: field?.id ?? null,
    revision: field?.revision ?? null,
    displayName: draft.name.trim(),
    fieldType: draft.type,
    config:
      draft.type === "select"
        ? { ...existing, options: optionValues(draft.options) }
        : existing,
  };
}
