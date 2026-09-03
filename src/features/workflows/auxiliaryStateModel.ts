import type { AuxiliaryState, AuxiliaryStateEdit } from "../../contracts";

export interface AuxiliaryStateDraft extends AuxiliaryStateEdit {
  clientKey: string;
  stableKey: string;
  inUse: boolean;
}
export function stateDraft(states: AuxiliaryState[]): AuxiliaryStateDraft[] {
  return states.map(({ id, stableKey, displayName, semanticKind, inUse }) => ({
    id,
    clientKey: id,
    stableKey,
    displayName,
    semanticKind,
    inUse,
  }));
}
export function stateEdits(draft: AuxiliaryStateDraft[]): AuxiliaryStateEdit[] {
  return draft.map(({ id, displayName, semanticKind }) => ({
    id,
    displayName: displayName.trim(),
    semanticKind,
  }));
}
export function stateDraftError(draft: AuxiliaryStateDraft[]): string {
  if (draft.some((s) => !s.displayName.trim())) return "辅助状态名称不能为空。";
  const names = draft.map((s) => s.displayName.trim().toLowerCase());
  return new Set(names).size !== names.length ? "辅助状态名称不能重复。" : "";
}
export function moveState(
  draft: AuxiliaryStateDraft[],
  index: number,
  delta: -1 | 1,
): AuxiliaryStateDraft[] {
  const next = [...draft];
  if (next[index] && next[index + delta])
    [next[index], next[index + delta]] = [next[index + delta]!, next[index]];
  return next;
}
export function stateName(states: AuxiliaryState[], key: string): string {
  return states.find((state) => state.stableKey === key)?.displayName ?? key;
}
