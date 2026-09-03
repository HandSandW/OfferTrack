import { createContext, useEffect, useRef, useState } from "react";
import { desktopApi, OfferTrackError } from "../../lib/tauri";
import type { WarehouseSummary } from "../../contracts";
import type { SnapshotReport } from "./contracts";

export const SnapshotContext = createContext<ReturnType<
  typeof useSnapshot
> | null>(null);

export function useSnapshot(
  warehouse: WarehouseSummary | null,
  enabled: boolean,
) {
  const [report, setReport] = useState<SnapshotReport | null>(null);
  const [error, setError] = useState("");
  const [pending, setPending] = useState(true);
  const run = useRef<() => void>(() => undefined);
  const id = warehouse?.warehouseId;
  const path = warehouse?.displayPath;
  useEffect(() => {
    if (!enabled || !id || !path) return;
    let disposed = false;
    let inFlight = false;
    let again = false;
    let timer: number | undefined;
    let contentionRetries = 0;
    let publicationNeedsRetry = false;
    function schedule(delay = 1000) {
      if (disposed || publicationNeedsRetry) return;
      setPending(true);
      if (inFlight) {
        again = true;
        return;
      }
      window.clearTimeout(timer);
      timer = window.setTimeout(() => {
        void check();
      }, delay);
    }
    async function check() {
      if (disposed || inFlight) return;
      inFlight = true;
      let retry = false;
      try {
        const next = await desktopApi.checkAgentSnapshot(id!, path!);
        if (disposed) return;
        if (next.warehouse_id !== id)
          throw new Error("仓库已切换，请重新检查快照。");
        setReport(next);
        // Do not accumulate a new generation every minute if files published but the
        // checkpoint cannot be recorded. Keep the error visible until explicit retry/reconnect.
        publicationNeedsRetry = next.published && next.state === "error";
        setError("");
        contentionRetries = 0;
      } catch (failure) {
        if (disposed) return;
        if (
          failure instanceof OfferTrackError &&
          failure.code === "WAREHOUSE_OPERATION_BUSY" &&
          contentionRetries++ < 3
        ) {
          retry = true;
        } else {
          setError(
            failure instanceof Error
              ? failure.message
              : "快照检查失败，请重试。",
          );
        }
      } finally {
        inFlight = false;
        if (!disposed) {
          if (!publicationNeedsRetry && (again || retry)) {
            again = false;
            schedule(retry ? 5000 : 1000);
          } else {
            setPending(false);
          }
        }
      }
    }
    run.current = () => {
      contentionRetries = 0;
      publicationNeedsRetry = false;
      schedule(0);
    };
    schedule();
    const interval = window.setInterval(() => schedule(), 60_000);
    const dirty = () => schedule();
    window.addEventListener("offertrack-snapshot-dirty", dirty);
    window.addEventListener("focus", dirty);
    return () => {
      disposed = true;
      window.clearInterval(interval);
      window.clearTimeout(timer);
      window.removeEventListener("offertrack-snapshot-dirty", dirty);
      window.removeEventListener("focus", dirty);
      run.current = () => undefined;
    };
  }, [id, path, enabled]);
  return { report, error, pending, refresh: () => run.current() };
}
