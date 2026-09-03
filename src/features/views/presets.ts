import type {
  ApplicationListItem,
  BusinessState,
  FilterState,
  SortRule,
} from "../../contracts";

export const businessStates: readonly [BusinessState, string][] = [
  ["preparing", "准备投递"],
  ["inProgress", "进行中"],
  ["awaitingResult", "待结果"],
  ["ended", "已结束"],
];
export const presetViews = [
  { id: "all", name: "全部投递" },
  ...businessStates.map(([id, name]) => ({ id, name })),
  { id: "recent", name: "最近更新" },
];

export function matchesBusinessState(
  record: ApplicationListItem,
  state: BusinessState | undefined,
): boolean {
  switch (state) {
    case undefined:
      return true;
    case "ended":
      return record.currentStageTerminal;
    case "preparing":
      return (
        !record.currentStageTerminal && record.currentStageKey === "preparing"
      );
    case "inProgress":
      return (
        !record.currentStageTerminal &&
        record.currentStageKey !== null &&
        record.currentStageKey !== "preparing"
      );
    case "awaitingResult":
      return (
        !record.currentStageTerminal &&
        record.currentStateKind === "awaitingResult"
      );
    default:
      return false;
  }
}

export function presetConfiguration(
  id: string,
): { filter: FilterState; sort: SortRule[] } | null {
  if (!presetViews.some((preset) => preset.id === id)) return null;
  const state = businessStates.find(([key]) => key === id)?.[0];
  return {
    filter: {
      search: "",
      companyTypes: [],
      stages: [],
      ...(state ? { businessState: state } : {}),
    },
    sort: [
      {
        key: id === "recent" ? "updatedAtUtc" : "createdAtUtc",
        direction: "desc",
      },
    ],
  };
}

export function matchingPreset(filter: FilterState, sort: SortRule[]): string {
  if (
    filter.search ||
    filter.companyTypes.length ||
    filter.stages.length ||
    sort.length !== 1 ||
    sort[0]?.direction !== "desc"
  )
    return "";
  if (sort[0].key === "updatedAtUtc" && !filter.businessState) return "recent";
  if (sort[0].key === "createdAtUtc") return filter.businessState ?? "all";
  return "";
}
