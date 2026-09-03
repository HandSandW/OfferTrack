import { describe, expect, it } from "vitest";
import { workflowFixture } from "../../test/workflowFixture";
import {
  addTemplateStage,
  canMoveStage,
  moveStage,
  templateDraft,
  templateRequest,
} from "./workflowModel";

describe("workflow template draft and order model", () => {
  it("preserves anchors and moves only intermediate stages without mutation", () => {
    const stages = workflowFixture().stages;
    expect(canMoveStage(stages, 0, 1)).toBe(false);
    expect(canMoveStage(stages, 1, -1)).toBe(false);
    expect(canMoveStage(stages, 6, 1)).toBe(false);
    expect(canMoveStage(stages, 7, -1)).toBe(false);
    expect(canMoveStage(stages, 8, 1)).toBe(false);
    expect(canMoveStage(stages, 4, -1)).toBe(true);
    expect(moveStage(stages, 4, -1)[3]?.stableKey).toBe("interview");
    expect(stages[3]?.stableKey).toBe("written_exam");
  });
  it("adds before the terminal stages and sends only the server's editable fields", () => {
    const detail = workflowFixture();
    const draft = addTemplateStage(templateDraft(detail), "draft-only-key");
    const request = templateRequest(detail, draft);
    expect(request.revision).toBe(1);
    expect(request.stages[7]).toEqual({
      id: null,
      displayName: "自定义阶段",
      color: "#2563eb",
    });
    expect(request.stages[8]?.id).toBe(detail.stages[7]?.id);
    expect(request.stages[0]).not.toHaveProperty("stableKey");
    expect(request.stages[0]).not.toHaveProperty("isTerminal");
    expect(detail.stages).toHaveLength(9);
  });
});
