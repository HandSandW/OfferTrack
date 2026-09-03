import { describe, expect, it } from "vitest";
import type { FieldDefinition } from "../../contracts";
import { fieldDraft, fieldRequest, optionValues } from "./fieldModel";

describe("field editor DTOs", () => {
  it("preserves stable identifiers and unknown same-type config without mutating inputs", () => {
    const field: FieldDefinition = {
      id: "field",
      key: "stable",
      revision: 8,
      displayName: "等级",
      fieldType: "select",
      config: { options: ["高", "低"], future: true },
      displayOrder: 20,
      isVisible: true,
    };
    const draft = fieldDraft(field);
    const request = fieldRequest(field, {
      ...draft,
      name: " 新名 ",
      options: "高，低, 中,高",
    });
    expect(request).toEqual({
      id: "field",
      revision: 8,
      displayName: "新名",
      fieldType: "select",
      config: { options: ["高", "低", "中"], future: true },
    });
    expect(field.config).toEqual({ options: ["高", "低"], future: true });
    expect(fieldRequest(field, { ...draft, type: "number" }).config).toEqual(
      {},
    );
    expect(optionValues(" ，,, ")).toEqual([]);
  });
  it("creates explicit new metadata requests without generated client identities", () => {
    expect(
      fieldRequest(null, { ...fieldDraft(null), name: "新的字段" }),
    ).toEqual({
      id: null,
      revision: null,
      displayName: "新的字段",
      fieldType: "text",
      config: {},
    });
  });
});
