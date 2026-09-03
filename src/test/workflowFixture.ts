import type { WorkflowTemplateDetail } from "../contracts";
import { auxiliaryStateFixture } from "./auxiliaryStateFixture";

export function workflowFixture(
  overrides: Partial<WorkflowTemplateDetail> = {},
): WorkflowTemplateDetail {
  const id = overrides.id ?? "test-template";
  return {
    id,
    name: "默认招聘流程",
    description: "虚构模板",
    isDefault: true,
    revision: 1,
    stageCount: 9,
    auxiliaryStates: auxiliaryStateFixture(id).map((state) => ({
      ...state,
      inUse: false,
    })),
    stages: [
      ["preparing", "准备投递"],
      ["applied", "已投递"],
      ["assessment", "在线测评"],
      ["written_exam", "笔试"],
      ["interview", "面试考核"],
      ["interview_passed", "面试通过"],
      ["signing", "待签约"],
      ["offer", "offer✅️"],
      ["failed_terminal", "已挂"],
    ].map(([key, name], index) => ({
      id: `${id}-stage-${key}`,
      stableKey: key!,
      displayName: name!,
      stageKind: index < 7 ? "application" : "terminal",
      displayOrder: (index + 1) * 10,
      color: "#2563eb",
      isTerminal: index >= 7,
      terminalOutcome: index === 7 ? "offer" : index === 8 ? "failed" : null,
    })),
    ...overrides,
  };
}
