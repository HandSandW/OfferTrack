import type { ApplicationListItem, FieldDefinition } from "../../contracts";

export interface CellAddress {
  id: string;
  key: string;
}
export interface CellRequest extends CellAddress {
  version: 1;
  revision: number;
  value: unknown;
}
export interface CellApplied {
  record: ApplicationListItem;
  previousValue: unknown;
  changed: boolean;
}
const textKeys = [
  "companyName",
  "industry",
  "positionName",
  "positionCategory",
  "workLocation",
  "positionDescription",
  "notes",
  "tags",
];
const nullableKeys = [
  "applicationDate",
  "applicationUrl",
  "announcementUrl",
  "companyUrl",
  "positionUrl",
];
export function canEditCell(key: string, fields: FieldDefinition[]) {
  return (
    textKeys.includes(key) ||
    nullableKeys.includes(key) ||
    key === "companyType" ||
    fields.some((f) => `custom:${f.id}` === key)
  );
}
export function cellValue(record: ApplicationListItem, key: string): unknown {
  if (key === "tags") return record.tags.map((t) => t.name);
  if (key.startsWith("custom:"))
    return record.customFields[key.slice(7)] ?? null;
  return record[key as keyof ApplicationListItem];
}
export function editValue(record: ApplicationListItem, key: string) {
  const value = cellValue(record, key);
  if (key.startsWith("custom:")) return value ?? undefined;
  return key === "tags" ? (value as string[]).join(", ") : (value ?? "");
}
export function saveValue(key: string, value: unknown): unknown {
  if (key === "tags")
    return String(value)
      .split(/[,，]/)
      .map((s) => s.trim())
      .filter(Boolean);
  if (value === undefined || (nullableKeys.includes(key) && value === ""))
    return null;
  return value;
}

/** Navigation uses visible IDs, not indices in a differently sorted source list. */
export function moveCell(
  current: CellAddress,
  ids: string[],
  keys: string[],
  key: string,
  shift = false,
): CellAddress | null {
  let row = ids.indexOf(current.id),
    col = keys.indexOf(current.key);
  if (row < 0 || col < 0 || !ids.length || !keys.length) return null;
  if (key === "Tab") {
    const index = row * keys.length + col + (shift ? -1 : 1);
    if (index < 0 || index >= ids.length * keys.length) return null;
    row = Math.floor(index / keys.length);
    col = index % keys.length;
  } else if (key === "ArrowLeft") col = Math.max(0, col - 1);
  else if (key === "ArrowRight") col = Math.min(keys.length - 1, col + 1);
  else if (key === "ArrowUp") row = Math.max(0, row - 1);
  else if (key === "ArrowDown") row = Math.min(ids.length - 1, row + 1);
  else if (key === "Home") col = 0;
  else if (key === "End") col = keys.length - 1;
  else return null;
  return { id: ids[row]!, key: keys[col]! };
}
