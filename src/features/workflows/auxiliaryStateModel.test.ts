import { describe, expect, it } from "vitest";
import { auxiliaryStateFixture } from "../../test/auxiliaryStateFixture";
import {
  moveState,
  stateDraft,
  stateDraftError,
  stateEdits,
  stateName,
} from "./auxiliaryStateModel";

describe("auxiliary state draft model", () => {
  it("reorders immutably and only submits permitted definition fields", () => {
    const states = auxiliaryStateFixture();
    const draft = stateDraft(states);
    const moved = moveState(draft, 0, 1);
    expect(draft[0]?.stableKey).toBe("pending");
    expect(moved[1]?.stableKey).toBe("pending");
    expect(stateEdits(moved)[1]).toEqual({
      id: states[0]!.id,
      displayName: "尚未开始",
      semanticKind: "pending",
    });
    expect(stateDraftError(draft)).toBe("");
    draft[0]!.displayName = " 待结果 ";
    expect(stateDraftError(draft)).toMatch("不能重复");
    draft[0]!.displayName = " ";
    expect(stateDraftError(draft)).toMatch("不能为空");
    expect(stateName(states, "awaitingResult")).toBe("待结果");
    expect(stateName(states, "external-code")).toBe("external-code");
  });
});
