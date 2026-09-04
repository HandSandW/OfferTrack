import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { ApplicationDetail, FieldDefinition } from "../../contracts";
import { desktopApi } from "../../lib/tauri";
import { useDraftGuard } from "../../shared/draftGuard";
import { DetailPanel, type DetailTab } from "./DetailPanel";

interface Target {
  applicationId: string;
  revision: number;
}

export function ApplicationDetailWindow() {
  const { confirmLeave } = useDraftGuard();
  const [detail, setDetail] = useState<ApplicationDetail | null>(null);
  const [fields, setFields] = useState<FieldDefinition[]>([]);
  const [writable, setWritable] = useState(false);
  const [error, setError] = useState("");
  const [operationError, setOperationError] = useState("");
  const [tab, setTab] = useState<DetailTab>("basic");
  const request = useRef(0);

  const load = useCallback(
    async (target: Target) => {
      if (!(await confirmLeave())) return;
      const token = ++request.current;
      setError("");
      setOperationError("");
      try {
        const [next, definitions, startup] = await Promise.all([
          desktopApi.getApplication(target.applicationId),
          desktopApi.listFieldDefinitions(),
          desktopApi.getStartupState(),
        ]);
        if (token !== request.current) return;
        setDetail(next);
        setFields(definitions);
        setWritable(startup.activeWarehouse?.accessMode === "write");
      } catch (reason) {
        if (token === request.current) {
          setError(
            reason instanceof Error ? reason.message : "投递详情读取失败。",
          );
        }
      }
    },
    [confirmLeave],
  );

  useEffect(() => {
    let active = true;
    void desktopApi
      .getApplicationDetailTarget()
      .then((target) => {
        if (active && target) void load(target);
      })
      .catch((reason: unknown) => {
        if (active) {
          setError(
            reason instanceof Error ? reason.message : "详情目标读取失败。",
          );
        }
      });
    const unlisten = listen<Target>("application-detail-target", (event) => {
      if (active) void load(event.payload);
    });
    return () => {
      active = false;
      request.current += 1;
      void unlisten.then((dispose) => dispose());
    };
  }, [load]);

  if (error)
    return (
      <main className="detail-window-message" role="alert">
        <h1>无法打开投递详情</h1>
        <p>{error}</p>
        <button type="button" onClick={() => void getCurrentWindow().destroy()}>
          关闭详情窗口
        </button>
      </main>
    );
  if (!detail)
    return (
      <main className="detail-window-message" role="status">
        正在读取投递详情…
      </main>
    );
  return (
    <DetailPanel
      key={detail.id}
      detail={detail}
      fields={fields}
      scope={detail.archivedAtUtc ? "archived" : "active"}
      writable={writable}
      initialTab={tab}
      onTabChange={setTab}
      operationError={operationError}
      onError={(reason) =>
        setOperationError(
          reason instanceof Error ? reason.message : "详情操作失败。",
        )
      }
      onChange={(next) => {
        if (!next) {
          void desktopApi
            .notifyApplicationDetailChanged()
            .finally(() => getCurrentWindow().destroy());
          return;
        }
        setOperationError("");
        setDetail(next);
        void desktopApi.notifyApplicationDetailChanged();
      }}
    />
  );
}
