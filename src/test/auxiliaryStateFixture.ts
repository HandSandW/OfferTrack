import type { AuxiliaryState } from "../contracts";
import { stageStates } from "../features/applications/tableModel";

export function auxiliaryStateFixture(owner = "fixture"): AuxiliaryState[] {
  return stageStates.map(([key, label], index) => ({
    id: `${owner}-${key}`,
    stableKey: key,
    displayName: label,
    semanticKind: key,
    displayOrder: (index + 1) * 10,
    inUse: key === "pending",
  }));
}
