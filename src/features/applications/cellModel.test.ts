import { describe, expect, it } from "vitest";
import { applicationFixture } from "../../test/applicationFixture";
import {
  canEditCell,
  cellValue,
  editValue,
  moveCell,
  saveValue,
} from "./cellModel";

describe("table cell model", () => {
  it("moves by displayed stable IDs and wraps only with Tab", () => {
    expect(
      moveCell(
        { id: "b", key: "two" },
        ["a", "b", "c"],
        ["one", "two"],
        "ArrowUp",
      ),
    ).toEqual({ id: "a", key: "two" });
    expect(
      moveCell({ id: "a", key: "two" }, ["a", "b"], ["one", "two"], "Tab"),
    ).toEqual({ id: "b", key: "one" });
    expect(
      moveCell({ id: "a", key: "one" }, ["a"], ["one", "two"], "Tab", true),
    ).toBeNull();
    expect(
      moveCell({ id: "missing", key: "one" }, ["a"], ["one"], "ArrowDown"),
    ).toBeNull();
  });
  it("maps editable, tags, nulls and custom values without touching hidden fields", () => {
    const record = applicationFixture({
      tags: [{ id: "t", name: "重点", color: "#000000", scope: "record" }],
      customFields: { f: 12 },
    });
    expect(canEditCell("notes", [])).toBe(true);
    expect(canEditCell("currentStageName", [])).toBe(false);
    expect(cellValue(record, "tags")).toEqual(["重点"]);
    expect(editValue(record, "custom:missing")).toBeUndefined();
    expect(saveValue("tags", "重点， 远程,重点")).toEqual([
      "重点",
      "远程",
      "重点",
    ]);
    expect(saveValue("applicationUrl", "")).toBeNull();
    expect(saveValue("notes", "")).toBe("");
  });
});
