import { describe, expect, it } from "vitest";
import { applicationFixture } from "../../test/applicationFixture";
import { filterAndSort, initialFilter } from "../applications/tableModel";
import {
  matchingPreset,
  matchesBusinessState,
  presetConfiguration,
  presetViews,
} from "./presets";

describe("business view semantics", () => {
  const records = [
    applicationFixture({ id: "preparing", currentStageName: "收集资料" }),
    applicationFixture({
      id: "interview",
      currentStageKey: "interview",
      currentStageName: "主管面",
      currentStateKind: "awaitingResult",
      currentStateName: "等主管回信",
      currentStageState: "custom-wait",
    }),
    applicationFixture({
      id: "custom",
      currentStageKey: "custom-exam",
      currentStageName: "企业加试",
      currentStateKind: "completed",
    }),
    applicationFixture({
      id: "failed",
      currentStageKey: "interview",
      currentStageTerminal: true,
      currentStageState: "failed",
    }),
    applicationFixture({
      id: "offer",
      currentStageKey: "offer",
      currentStageTerminal: true,
      currentStateKind: "completed",
    }),
    applicationFixture({ id: "unknown", currentStageKey: null }),
  ];
  it("uses stable stage semantics, not names, progress, or the completed auxiliary state", () => {
    for (const [state, ids] of [
      ["preparing", ["preparing"]],
      ["inProgress", ["interview", "custom"]],
      ["awaitingResult", ["interview"]],
      ["ended", ["failed", "offer"]],
    ] as const) {
      expect(
        records.filter((r) => matchesBusinessState(r, state)).map((r) => r.id),
      ).toEqual(ids);
    }
    expect(
      matchesBusinessState(
        applicationFixture({
          currentStageTerminal: true,
          currentStateKind: "awaitingResult",
        }),
        "awaitingResult",
      ),
    ).toBe(false);
    expect(
      records.filter((r) => matchesBusinessState(r, undefined)),
    ).toHaveLength(6);
  });

  it("intersects semantic filters with existing fields and keeps old saved filters compatible", () => {
    const filtered = filterAndSort(
      records,
      {
        ...initialFilter,
        businessState: "awaitingResult",
        search: "示例",
        companyTypes: ["private"],
        stages: ["主管面"],
      },
      [],
    );
    expect(filtered.map((r) => r.id)).toEqual(["interview"]);
    expect(
      filterAndSort(
        records,
        { ...initialFilter, businessState: "ended", search: "missing" },
        [],
      ),
    ).toEqual([]);
    expect(filterAndSort(records, initialFilter, [])).toHaveLength(6);
    expect(records[0]?.id).toBe("preparing");
  });

  it("creates fresh reusable configurations and recent means actual edit time, not status time", () => {
    for (const preset of presetViews) {
      const config = presetConfiguration(preset.id)!;
      expect(matchingPreset(config.filter, config.sort)).toBe(preset.id);
    }
    expect(presetConfiguration("recycle")).toBeNull();
    const first = presetConfiguration("preparing")!;
    first.filter.search = "changed";
    expect(presetConfiguration("preparing")!.filter.search).toBe("");
    expect(matchingPreset(first.filter, first.sort)).toBe("");
    const recent = presetConfiguration("recent")!;
    const changed = [
      applicationFixture({
        id: "old",
        updatedAtUtc: "2026-09-03T08:00:00+08:00",
        statusUpdatedAtUtc: "2026-09-04T00:00:00Z",
      }),
      applicationFixture({ id: "new", updatedAtUtc: "2026-09-03T01:00:00Z" }),
    ];
    expect(
      filterAndSort(changed, recent.filter, recent.sort).map((r) => r.id),
    ).toEqual(["new", "old"]);
  });
});
