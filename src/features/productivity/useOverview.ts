import {
  createContext,
  useContext,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import { desktopApi } from "../../lib/tauri";
import type { Overview } from "./contracts";
import { errorText } from "./model";

export function useOverview(onError: (error: unknown) => void) {
  const shared = useContext(OverviewContext);
  const local = useOverviewState(onError, !shared);
  return shared ?? local;
}

export const OverviewContext = createContext<ReturnType<
  typeof useOverviewState
> | null>(null);

export function useOverviewState(
  onError: (error: unknown) => void,
  enabled = true,
  page = "",
) {
  const [data, setData] = useState<Overview | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(true);
  const sequence = useRef(0);
  const invalidate = useCallback(() => {
    sequence.current++;
  }, []);
  const refresh = useCallback(async () => {
    if (!enabled) return;
    const request = ++sequence.current;
    setLoading(true);
    try {
      const next = await desktopApi.getOverview();
      if (request === sequence.current) {
        setData(next);
        setError("");
      }
    } catch (failure) {
      if (request === sequence.current) {
        setError(errorText(failure));
        onError(failure);
      }
    } finally {
      if (request === sequence.current) setLoading(false);
    }
  }, [onError, enabled]);
  useEffect(() => {
    if (!enabled) return;
    void refresh();
    const timer = window.setInterval(() => void refresh(), 60_000);
    const focus = () => void refresh();
    window.addEventListener("focus", focus);
    window.addEventListener("offertrack-data-changed", focus);
    return () => {
      invalidate();
      window.clearInterval(timer);
      window.removeEventListener("focus", focus);
      window.removeEventListener("offertrack-data-changed", focus);
    };
  }, [refresh, invalidate, enabled, page]);
  return { data, error, loading, refresh };
}
