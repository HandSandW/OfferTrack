import { describe, expect, it } from "vitest";
import { applicationFixture } from "../../test/applicationFixture";
import {
  columnsWithFields,
  defaultColumns,
  filterAndSort,
  initialFilter,
  pinnedOffsets,
  rawValue,
} from "./tableModel";

describe("application table model", () => {
  it("uses the documented default ordering and keeps custom columns after system columns", () => {
    expect(defaultColumns.slice(0, 5).map((column) => column.key)).toEqual([
      "createdAtUtc",
      "companyName",
      "applicationDate",
      "currentStageName",
      "statusUpdatedAtUtc",
    ]);
    const columns = columnsWithFields(
      [{ ...defaultColumns[0]!, width: -1 }],
      [
        {
          id: "salary",
          revision: 1,
          key: "salary",
          displayName: "薪酬",
          fieldType: "number",
          config: {},
          displayOrder: 1,
          isVisible: true,
        },
      ],
    );
    expect(columns[0]?.width).toBe(80);
    expect(columns.at(-1)?.key).toBe("custom:salary");
  });

  it("combines filters and sorts numeric custom values without changing source data", () => {
    const records = [
      applicationFixture({ id: "a", customFields: { salary: 20 } }),
      applicationFixture({ id: "b", customFields: { salary: 3 } }),
      applicationFixture({ id: "c", companyType: "bank" }),
    ];
    const result = filterAndSort(
      records,
      {
        ...initialFilter,
        search: "示例",
        companyTypes: ["private"],
        stages: ["准备投递"],
      },
      [{ key: "custom:salary", direction: "asc" }],
    );
    expect(result.map((record) => record.id)).toEqual(["b", "a"]);
    expect(records[0]?.id).toBe("a");
    expect(
      rawValue(
        applicationFixture({ customFields: { active: false } }),
        "custom:active",
      ),
    ).toBe(false);
  });

  it("allocates non-overlapping offsets for multiple fixed columns", () => {
    const offsets = pinnedOffsets([
      { key: "a", visible: true, pinned: true, width: 112 },
      { key: "b", visible: false, pinned: true, width: 200 },
      { key: "c", visible: true, pinned: true, width: 150 },
    ]);
    expect([...offsets.entries()]).toEqual([
      ["a", 0],
      ["c", 112],
    ]);
  });

  it("filters and sorts ten thousand long-text rows within the UI baseline", () => {
    const longText = "合成岗位介绍与备注。".repeat(40);
    const records = Array.from({ length: 10_000 }, (_, index) =>
      applicationFixture({
        id: `performance-${index.toString().padStart(5, "0")}`,
        companyName: index % 2 ? `目标公司 ${index}` : `其他公司 ${index}`,
        positionDescription: longText,
        notes: longText,
        createdAtUtc: `2026-01-${String((index % 28) + 1).padStart(2, "0")}T12:00:00Z`,
      }),
    );
    const started = performance.now();
    const result = filterAndSort(
      records,
      { ...initialFilter, search: "目标公司" },
      [{ key: "createdAtUtc", direction: "desc" }],
    );
    const elapsed = performance.now() - started;
    expect(result).toHaveLength(5_000);
    expect(elapsed).toBeLessThan(2_000);
  });
});
