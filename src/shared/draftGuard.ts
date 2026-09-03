import { createContext, useContext, useLayoutEffect, useId } from "react";

export interface Confirmation {
  title: string;
  message: string;
  confirmLabel?: string;
  cancelLabel?: string;
  destructive?: boolean;
}
export interface DraftState {
  dirty: boolean;
  busy: boolean;
  label: string;
}
export interface DraftGuard {
  register: (id: string, state: DraftState) => () => void;
  confirmLeave: () => Promise<boolean>;
  confirm: (options: Confirmation) => Promise<boolean>;
}
export const DraftGuardContext = createContext<DraftGuard | null>(null);
export function useDraftGuard() {
  const guard = useContext(DraftGuardContext);
  if (!guard) throw new Error("DraftGuardProvider is required");
  return guard;
}
export function useDraftState(dirty: boolean, busy: boolean, label: string) {
  const { register } = useDraftGuard();
  const id = useId();
  // Commit the guard before newly enabled controls can receive input. A passive
  // effect leaves a frame where navigation sees the previous busy/draft state.
  useLayoutEffect(
    () => register(id, { dirty, busy, label }),
    [id, dirty, busy, label, register],
  );
}
