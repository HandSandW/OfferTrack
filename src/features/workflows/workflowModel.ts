import type {
  TemplateStageEdit,
  WorkflowStage,
  WorkflowTemplateDetail,
  UpdateWorkflowTemplateRequest,
} from "../../contracts";

export interface StageDraft extends TemplateStageEdit {
  clientKey: string;
  stableKey: string;
  isTerminal: boolean;
}
export interface TemplateDraft {
  name: string;
  description: string;
  stages: StageDraft[];
}

export function templateDraft(detail: WorkflowTemplateDetail): TemplateDraft {
  return {
    name: detail.name,
    description: detail.description,
    stages: detail.stages.map((stage) => ({
      clientKey: stage.id,
      id: stage.id,
      stableKey: stage.stableKey,
      displayName: stage.displayName,
      color: stage.color,
      isTerminal: stage.isTerminal,
    })),
  };
}

export function templateRequest(
  detail: WorkflowTemplateDetail,
  draft: TemplateDraft,
): UpdateWorkflowTemplateRequest {
  return {
    id: detail.id,
    revision: detail.revision,
    name: draft.name.trim(),
    description: draft.description,
    stages: draft.stages.map(({ id, displayName, color }) => ({
      id,
      displayName: displayName.trim(),
      color,
    })),
  };
}

type OrderedStage = Pick<WorkflowStage, "stableKey" | "isTerminal">;
export function canMoveStage(
  stages: readonly OrderedStage[],
  index: number,
  delta: -1 | 1,
): boolean {
  const stage = stages[index];
  const adjacent = stages[index + delta];
  return (
    !!stage &&
    !!adjacent &&
    !stage.isTerminal &&
    !adjacent.isTerminal &&
    stage.stableKey !== "preparing" &&
    adjacent.stableKey !== "preparing"
  );
}
export function moveStage<T extends OrderedStage>(
  stages: readonly T[],
  index: number,
  delta: -1 | 1,
): T[] {
  const next = [...stages];
  if (canMoveStage(stages, index, delta)) {
    [next[index], next[index + delta]] = [next[index + delta]!, next[index]!];
  }
  return next;
}
export function addTemplateStage(
  draft: TemplateDraft,
  clientKey: string,
): TemplateDraft {
  const stages = [...draft.stages];
  const index = stages.findIndex((stage) => stage.isTerminal);
  stages.splice(index < 0 ? stages.length : index, 0, {
    id: null,
    clientKey,
    stableKey: "custom_draft",
    isTerminal: false,
    displayName: "自定义阶段",
    color: "#2563eb",
  });
  return { ...draft, stages };
}
