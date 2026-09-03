import type {
  ApplicationListItem,
  ColumnSetting,
  CreateApplicationRequest,
  FieldDefinition,
  FilterState,
  SortRule,
} from "../../contracts";

export const companyTypes = [
  ["stateOwned", "央国企"],
  ["private", "民企"],
  ["foreign", "外企"],
  ["bank", "银行"],
  ["uncategorized", "未分类"],
] as const;

export const stageStates = [
  ["pending", "尚未开始"],
  ["awaitingParticipation", "待参加"],
  ["awaitingCompletion", "待完成"],
  ["awaitingResult", "待结果"],
  ["completed", "已完成"],
  ["failed", "未通过"],
] as const;

export const defaultColumns: ColumnSetting[] = [
  { key: "createdAtUtc", visible: true, width: 112, pinned: true },
  { key: "companyName", visible: true, width: 170, pinned: false },
  { key: "applicationDate", visible: true, width: 112, pinned: false },
  { key: "currentStageName", visible: true, width: 170, pinned: false },
  { key: "statusUpdatedAtUtc", visible: true, width: 130, pinned: false },
  { key: "companyType", visible: true, width: 100, pinned: false },
  { key: "industry", visible: true, width: 120, pinned: false },
  { key: "positionName", visible: true, width: 170, pinned: false },
  { key: "positionCategory", visible: true, width: 120, pinned: false },
  { key: "workLocation", visible: true, width: 110, pinned: false },
  { key: "documentNames", visible: true, width: 190, pinned: false },
  { key: "applicationUrl", visible: true, width: 150, pinned: false },
  { key: "positionDescription", visible: true, width: 190, pinned: false },
  { key: "notes", visible: true, width: 190, pinned: false },
  { key: "tags", visible: true, width: 150, pinned: false },
  { key: "announcementUrl", visible: false, width: 150, pinned: false },
  { key: "companyUrl", visible: false, width: 150, pinned: false },
  { key: "positionUrl", visible: false, width: 150, pinned: false },
];

export const columnLabels: Record<string, string> = {
  createdAtUtc: "创建日期",
  applicationDate: "投递日期",
  companyName: "公司名称",
  companyType: "企业性质",
  industry: "行业",
  positionName: "岗位名称",
  positionCategory: "岗位类别",
  workLocation: "工作地点",
  applicationUrl: "投递链接",
  announcementUrl: "公告链接",
  companyUrl: "公司网址",
  positionUrl: "岗位网址",
  documentNames: "简历文件",
  positionDescription: "岗位介绍",
  currentStageName: "投递进度",
  statusUpdatedAtUtc: "上次更新时间",
  tags: "标签",
  notes: "备注",
};

export const initialFilter: FilterState = {
  search: "",
  companyTypes: [],
  stages: [],
};
export const initialCreate: CreateApplicationRequest = {
  companyName: "",
  positionName: "",
  companyType: "uncategorized",
  industry: "",
  positionCategory: "",
  workLocation: "",
};

export function dateOnly(value: string | null) {
  if (!value) return "—";
  if (value.length === 10) return value;
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "—";
  return [
    date.getFullYear(),
    String(date.getMonth() + 1).padStart(2, "0"),
    String(date.getDate()).padStart(2, "0"),
  ].join("-");
}

export function companyTypeName(value: string) {
  return companyTypes.find(([key]) => key === value)?.[1] ?? "未分类";
}

export function rawValue(
  record: ApplicationListItem,
  key: string,
): string | number | boolean | null {
  if (key === "tags") return record.tags.map((tag) => tag.name).join("、");
  if (key === "documentNames") return record.documentNames.join("、");
  if (key.startsWith("custom:")) {
    const value = record.customFields[key.slice(7)];
    return typeof value === "string" ||
      typeof value === "number" ||
      typeof value === "boolean"
      ? value
      : null;
  }
  const value = record[key as keyof ApplicationListItem];
  if (
    typeof value === "string" ||
    typeof value === "number" ||
    typeof value === "boolean" ||
    value === null
  )
    return value;
  return "";
}

export function editableValue(value: unknown): string | number {
  return typeof value === "string" || typeof value === "number" ? value : "";
}

export function columnsWithFields(
  saved: ColumnSetting[],
  fields: FieldDefinition[],
): ColumnSetting[] {
  const available = [
    ...defaultColumns,
    ...fields.map((field) => ({
      key: `custom:${field.id}`,
      width: 150,
      visible: field.isVisible,
      pinned: false,
    })),
  ];
  const known = new Map(available.map((column) => [column.key, column]));
  const seen = new Set<string>();
  const result: ColumnSetting[] = [];
  for (const column of [...saved, ...available]) {
    if (!known.has(column.key) || seen.has(column.key)) continue;
    seen.add(column.key);
    result.push({
      ...column,
      width: Math.min(600, Math.max(80, column.width || 150)),
    });
  }
  return result;
}

export function filterAndSort(
  records: ApplicationListItem[],
  filter: FilterState,
  sort: SortRule[],
) {
  const search = filter.search.trim().toLocaleLowerCase();
  return records
    .filter((record) => {
      const text = [
        record.companyName,
        record.positionName,
        record.notes,
        record.positionDescription,
        ...record.tags.map((tag) => tag.name),
      ]
        .join(" ")
        .toLocaleLowerCase();
      return (
        (!search || text.includes(search)) &&
        (!filter.companyTypes.length ||
          filter.companyTypes.includes(record.companyType)) &&
        (!filter.stages.length ||
          filter.stages.includes(record.currentStageName))
      );
    })
    .sort((left, right) => {
      for (const rule of sort) {
        const a = rawValue(left, rule.key);
        const b = rawValue(right, rule.key);
        const comparison =
          typeof a === "number" && typeof b === "number"
            ? a - b
            : String(a ?? "").localeCompare(String(b ?? ""), "zh-CN", {
                numeric: true,
              });
        if (comparison)
          return rule.direction === "asc" ? comparison : -comparison;
      }
      return left.id.localeCompare(right.id);
    });
}

export function pinnedOffsets(columns: ColumnSetting[]): Map<string, number> {
  const offsets = new Map<string, number>();
  let left = 0;
  for (const column of columns) {
    if (column.visible && column.pinned) {
      offsets.set(column.key, left);
      left += column.width;
    }
  }
  return offsets;
}
