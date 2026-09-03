import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  DraftGuardContext,
  type Confirmation,
  type DraftState,
} from "./draftGuard";
import { Modal } from "./Modal";

export function DraftGuardProvider({ children }: { children: ReactNode }) {
  const drafts = useRef(new Map<string, DraftState>());
  const resolver = useRef<((confirmed: boolean) => void) | null>(null);
  const [prompt, setPrompt] = useState<Confirmation | null>(null);
  const register = useCallback((id: string, state: DraftState) => {
    drafts.current.set(id, state);
    return () => {
      drafts.current.delete(id);
    };
  }, []);
  const confirm = useCallback((options: Confirmation): Promise<boolean> => {
    if (resolver.current) return Promise.resolve(false);
    return new Promise((resolve) => {
      resolver.current = resolve;
      setPrompt(options);
    });
  }, []);
  const settle = (value: boolean) => {
    const resolve = resolver.current;
    resolver.current = null;
    setPrompt(null);
    resolve?.(value);
  };
  const confirmLeave = useCallback(async () => {
    const states = [...drafts.current.values()];
    if (states.some((item) => item.busy)) {
      await confirm({
        title: "正在保存",
        message: "请等待当前操作完成后再离开，避免重复操作。",
        cancelLabel: "知道了",
      });
      return false;
    }
    const dirty = states.filter((item) => item.dirty);
    if (!dirty.length) return true;
    return confirm({
      title: "有未保存的修改",
      message: `${[...new Set(dirty.map((item) => item.label))].join("、")}尚未保存。放弃修改并继续吗？`,
      confirmLabel: "放弃修改并继续",
      cancelLabel: "继续编辑",
      destructive: true,
    });
  }, [confirm]);
  useEffect(() => {
    let bypass = false;
    const beforeUnload = (event: BeforeUnloadEvent) => {
      if (
        !bypass &&
        [...drafts.current.values()].some((item) => item.dirty || item.busy)
      ) {
        event.preventDefault();
        event.returnValue = "";
      }
    };
    window.addEventListener("beforeunload", beforeUnload);
    let disposed = false;
    const unlisten = isTauri()
      ? getCurrentWindow().onCloseRequested(async (event) => {
          // Always own the final close so permission/runtime failures can be
          // reported; the Tauri helper otherwise destroys outside our handler.
          event.preventDefault();
          if ((await confirmLeave()) && !disposed) {
            bypass = true;
            try {
              await getCurrentWindow().destroy();
            } catch {
              bypass = false;
              await confirm({
                title: "窗口未关闭",
                message: "关闭窗口失败，内容仍保留在当前页面。请重试。",
                cancelLabel: "知道了",
              });
            }
          }
        })
      : Promise.resolve(() => undefined);
    // Report registration failure without silently claiming close protection.
    void unlisten.catch(() => {
      if (!disposed)
        void confirm({
          title: "关闭保护未启用",
          message: "无法监听窗口关闭。请先保存修改，再关闭应用。",
          cancelLabel: "知道了",
        });
    });
    return () => {
      disposed = true;
      window.removeEventListener("beforeunload", beforeUnload);
      void unlisten.then(
        (stop) => stop(),
        () => undefined,
      );
      resolver.current?.(false);
      resolver.current = null;
    };
  }, [confirm, confirmLeave]);
  const value = useMemo(
    () => ({ register, confirm, confirmLeave }),
    [register, confirm, confirmLeave],
  );
  return (
    <DraftGuardContext.Provider value={value}>
      {children}
      {prompt && (
        <Modal title={prompt.title} onCancel={() => settle(false)}>
          <p>{prompt.message}</p>
          <div className="modal-actions">
            <button autoFocus type="button" onClick={() => settle(false)}>
              {prompt.cancelLabel ?? "取消"}
            </button>
            {prompt.confirmLabel && (
              <button
                type="button"
                className={prompt.destructive ? "danger" : "primary"}
                onClick={() => settle(true)}
              >
                {prompt.confirmLabel}
              </button>
            )}
          </div>
        </Modal>
      )}
    </DraftGuardContext.Provider>
  );
}
