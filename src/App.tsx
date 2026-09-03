import { useCallback, useEffect, useState } from "react";
import { ThemeProvider } from "./features/theme/ThemeProvider";
import { ThemeSettings } from "./features/theme/ThemeSettings";
import { helpApi } from "./features/help/api";
import {
  AgentSnapshotProvider,
  SnapshotNotice,
} from "./features/agent/AgentSnapshotProvider";
import { listen } from "@tauri-apps/api/event";
import type { WarehouseSummary } from "./contracts";
import { selectDirectory } from "./lib/dialog";
import { desktopApi, OfferTrackError } from "./lib/tauri";
import { ApplicationPage } from "./features/applications/ApplicationPage";
import { OverviewPage } from "./features/productivity/OverviewPage";
import { ProductivityPage } from "./features/productivity/ProductivityPage";
import {
  OverviewProvider,
  ReminderBanner,
} from "./features/productivity/OverviewProvider";
import type {
  Drilldown,
  ScheduleScope,
} from "./features/productivity/contracts";
import { FullBackupPanel } from "./features/backup/FullBackupPanel";
import { ExternalDatabasePanel } from "./features/backup/ExternalDatabasePanel";
import { WorkflowTemplatesPage } from "./features/workflows/WorkflowTemplatesPage";
import { DraftGuardProvider } from "./shared/DraftGuardProvider";
import { useDraftGuard, useDraftState } from "./shared/draftGuard";
import {
  RecycleBinPage,
  SettingsPage,
} from "./features/management/PhaseTwoPages";

const pages = [
  { id: "overview", label: "概览", phase: 1 },
  { id: "applications", label: "投递记录", phase: 2 },
  { id: "tasks", label: "待办与日程", phase: 3 },
  { id: "templates", label: "流程模板", phase: 2 },
  { id: "archive", label: "已归档", phase: 2 },
  { id: "recycle", label: "回收站", phase: 2 },
  { id: "settings", label: "设置", phase: 1 },
  { id: "help", label: "帮助", phase: 1 },
] as const;

type PageId = (typeof pages)[number]["id"];

interface Notice {
  kind: "error" | "info";
  text: string;
  lockedPath?: string;
}

function WarehouseCard({ warehouse }: { warehouse: WarehouseSummary }) {
  return (
    <section className="warehouse-card" aria-labelledby="warehouse-title">
      <div>
        <p className="eyebrow">当前数据仓库</p>
        <h2 id="warehouse-title">已安全连接</h2>
        <p className="path" title={warehouse.displayPath}>
          {warehouse.displayPath}
        </p>
      </div>
      <dl className="warehouse-meta">
        <div>
          <dt>访问模式</dt>
          <dd>{warehouse.accessMode === "write" ? "独占写入" : "只读"}</dd>
        </div>
        <div>
          <dt>仓库格式</dt>
          <dd>v{warehouse.formatVersion}</dd>
        </div>
      </dl>
      {warehouse.warnings.length > 0 && (
        <div className="warning-list" role="status">
          {warehouse.warnings.map((warning) => (
            <p key={warning.code}>{warning.message}</p>
          ))}
        </div>
      )}
    </section>
  );
}

export function App() {
  return (
    <ThemeProvider>
      <DraftGuardProvider>
        <AppContent />
      </DraftGuardProvider>
    </ThemeProvider>
  );
}

function AppContent() {
  const { confirmLeave } = useDraftGuard();
  const [page, setPage] = useState<PageId>("overview");
  const [warehouse, setWarehouse] = useState<WarehouseSummary | null>(null);
  const [rememberedPath, setRememberedPath] = useState<string | null>(null);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [busy, setBusy] = useState(true);
  const [destination, setDestination] = useState<{
    context: string;
    drilldown?: Drilldown;
    taskId?: string | undefined;
    eventId?: string;
    schedule?: ScheduleScope;
    create?: boolean;
  } | null>(null);
  const context = `${warehouse?.warehouseId}:${warehouse?.displayPath}`;
  const currentDestination =
    destination?.context === context ? destination : null;
  useDraftState(false, busy, "数据仓库操作");

  const reportError = useCallback((error: unknown, lockedPath?: string) => {
    const known =
      error instanceof OfferTrackError
        ? error
        : new OfferTrackError({
            code: "UNEXPECTED_ERROR",
            message: "操作失败，请重试。",
            retryable: true,
          });
    setNotice({
      kind: "error",
      text: known.message,
      ...([
        "WAREHOUSE_LOCKED",
        "COPY_RECOVERY_REQUIRED",
        "DOCUMENT_RENAME_RECOVERY",
        "DOCUMENT_TRASH_RECOVERY",
        "FILE_OPERATION_FAILED",
        "FILE_MISSING",
        "FILE_BUSY",
        "FILE_ACCESS_DENIED",
        "FILE_TYPE_MISMATCH",
        "UNSAFE_PATH_REJECTED",
      ].includes(known.code) && lockedPath
        ? { lockedPath }
        : {}),
    });
  }, []);

  useEffect(() => {
    let active = true;
    desktopApi
      .getStartupState()
      .then((state) => {
        if (!active) return;
        setWarehouse(state.activeWarehouse);
        setRememberedPath(state.rememberedWarehousePath);
      })
      .catch((error: unknown) => {
        if (active) reportError(error);
      })
      .finally(() => {
        if (active) setBusy(false);
      });

    return () => {
      active = false;
    };
  }, [reportError]);

  const chooseAndOpen = useCallback(
    async (create: boolean) => {
      if (!(await confirmLeave())) return;
      setNotice(null);
      const path = await selectDirectory(
        create
          ? "选择用于新建 OfferTrack 数据仓库的文件夹"
          : "选择已有 OfferTrack 数据仓库",
      );
      if (!path) return;

      setBusy(true);
      try {
        const next = create
          ? await desktopApi.createWarehouse(path)
          : await desktopApi.openWarehouse(path);
        setWarehouse(next);
        setRememberedPath(path);
      } catch (error: unknown) {
        reportError(error, path);
      } finally {
        setBusy(false);
      }
    },
    [reportError, confirmLeave],
  );

  const openRemembered = async () => {
    if (!rememberedPath) return;
    if (!(await confirmLeave())) return;
    setBusy(true);
    setNotice(null);
    try {
      setWarehouse(await desktopApi.openWarehouse(rememberedPath));
    } catch (error: unknown) {
      reportError(error, rememberedPath);
    } finally {
      setBusy(false);
    }
  };

  const openTransferred = async (path: string) => {
    if (!(await confirmLeave())) return;
    setBusy(true);
    setNotice(null);
    try {
      const next = await desktopApi.openWarehouse(path);
      setWarehouse(next);
      setRememberedPath(path);
      setNotice({
        kind: "info",
        text: "已切换到验证后的仓库；原仓库和完整备份保持不动。",
      });
    } catch (error: unknown) {
      reportError(error, path);
    } finally {
      setBusy(false);
    }
  };

  const openReadOnly = async (path: string) => {
    if (!(await confirmLeave())) return;
    setBusy(true);
    setNotice(null);
    try {
      setWarehouse(await desktopApi.openWarehouse(path, "readOnly"));
    } catch (error: unknown) {
      reportError(error);
    } finally {
      setBusy(false);
    }
  };

  const closeWarehouse = useCallback(async () => {
    if (!(await confirmLeave())) return;
    setBusy(true);
    try {
      await desktopApi.closeWarehouse();
      setWarehouse(null);
      setNotice({ kind: "info", text: "数据仓库已关闭，写入锁已释放。" });
    } catch (error: unknown) {
      reportError(error);
    } finally {
      setBusy(false);
    }
  }, [reportError, confirmLeave]);

  const navigate = useCallback(
    async (target: PageId) => {
      if (target === "help") {
        try {
          await helpApi.open();
        } catch {
          setNotice({
            kind: "error",
            text: "帮助窗口未打开，请重试；当前编辑内容保持不变。",
          });
        }
        return;
      }
      if (target === page || !(await confirmLeave())) return;
      setDestination(null);
      setPage(target);
    },
    [page, confirmLeave],
  );

  useEffect(() => {
    const unlisten = listen<string>("menu-action", (event) => {
      switch (event.payload) {
        case "new-warehouse":
          void chooseAndOpen(true);
          break;
        case "open-warehouse":
          void chooseAndOpen(false);
          break;
        case "close-warehouse":
          void closeWarehouse();
          break;
        case "overview":
        case "settings":
          void navigate(event.payload);
          break;
        case "help":
          void navigate("help");
          break;
        case "about":
          void helpApi.open("about").catch(reportError);
          break;
      }
    });

    return () => {
      void unlisten.then((dispose) => dispose());
    };
  }, [chooseAndOpen, closeWarehouse, navigate, reportError]);

  useEffect(() => {
    const keydown = (event: KeyboardEvent) => {
      if (event.key === "F1") {
        event.preventDefault();
        void navigate("help");
      }
    };
    window.addEventListener("keydown", keydown);
    const unlisten = listen("help-open-failed", () =>
      setNotice({
        kind: "error",
        text: "帮助窗口未打开，请重试；当前编辑内容保持不变。",
      }),
    );
    const stopLogs = listen("help-logs-failed", () =>
      setNotice({
        kind: "error",
        text: "日志目录无法安全打开。可在帮助的诊断信息章节查看说明；未创建或删除文件。",
      }),
    );
    return () => {
      window.removeEventListener("keydown", keydown);
      void unlisten.then(
        (stop) => stop(),
        () => undefined,
      );
      void stopLogs.then(
        (stop) => stop(),
        () => undefined,
      );
    };
  }, [navigate]);

  const selectedPage = pages.find((item) => item.id === page) ?? pages[0];
  const openDestination = async (
    target: PageId,
    details: {
      drilldown?: Drilldown;
      taskId?: string | undefined;
      eventId?: string;
      schedule?: ScheduleScope;
      create?: boolean;
    },
  ) => {
    if (!(await confirmLeave())) return;
    setDestination({ context, ...details });
    setPage(target);
  };

  return (
    <AgentSnapshotProvider
      key={context}
      warehouse={warehouse}
      enabled={!!warehouse && !busy}
    >
      <OverviewProvider
        key={context}
        enabled={!!warehouse && !busy}
        page={page}
        onError={reportError}
      >
        <div className="app-shell">
          <aside className="sidebar">
            <div className="brand">
              <span className="brand-mark" aria-hidden="true">
                O
              </span>
              <div>
                <strong>OfferTrack</strong>
                <span>离线求职管理</span>
              </div>
            </div>
            <nav aria-label="主导航">
              {pages.map((item) => (
                <button
                  className={page === item.id ? "nav-item active" : "nav-item"}
                  disabled={busy && item.id !== "help"}
                  key={item.id}
                  onClick={() => void navigate(item.id)}
                  type="button"
                >
                  <span>{item.label}</span>
                  {item.phase > 1 && <small>阶段 {item.phase}</small>}
                </button>
              ))}
            </nav>
            <div className="sidebar-footer">
              <span
                className={warehouse ? "status-dot online" : "status-dot"}
              />
              {warehouse ? "仓库已连接" : "未选择仓库"}
            </div>
          </aside>

          <main>
            <header className="topbar">
              <div>
                <p className="eyebrow">OfferTrack / {selectedPage.label}</p>
                <h1>{selectedPage.label}</h1>
              </div>
              <div className="top-actions">
                <button
                  type="button"
                  onClick={() =>
                    void helpApi.open(page).catch(() =>
                      setNotice({
                        kind: "error",
                        text: "本页帮助未打开，请重试；当前编辑内容保持不变。",
                      }),
                    )
                  }
                >
                  本页帮助
                </button>
                <button
                  disabled={busy}
                  onClick={() => void navigate("settings")}
                  type="button"
                >
                  备份与迁移
                </button>
                <button
                  disabled={busy}
                  onClick={() => void chooseAndOpen(false)}
                  type="button"
                >
                  打开仓库
                </button>
                <button
                  className="primary"
                  disabled={busy}
                  onClick={() => void chooseAndOpen(true)}
                  type="button"
                >
                  新建仓库
                </button>
              </div>
            </header>

            <div
              className={
                page === "applications" || page === "archive"
                  ? "content content-wide"
                  : "content"
              }
            >
              {warehouse && <SnapshotNotice />}
              {warehouse && page !== "overview" && (
                <ReminderBanner onOpen={() => void navigate("overview")} />
              )}
              {notice && (
                <div className={`notice ${notice.kind}`} role="alert">
                  <span>{notice.text}</span>
                  {notice.lockedPath && (
                    <button
                      disabled={busy}
                      onClick={() => void openReadOnly(notice.lockedPath!)}
                      type="button"
                    >
                      只读打开
                    </button>
                  )}
                </div>
              )}

              {!warehouse && page !== "settings" ? (
                <section className="empty-state">
                  <div className="empty-icon" aria-hidden="true">
                    OT
                  </div>
                  <p className="eyebrow">尚未连接数据仓库</p>
                  <h2>先打开或新建本地数据仓库</h2>
                  <p>连接后即可管理投递、简历文件、归档和回收站。</p>
                  <div className="empty-actions">
                    <button
                      className="primary"
                      disabled={busy}
                      onClick={() => void chooseAndOpen(true)}
                      type="button"
                    >
                      新建数据仓库
                    </button>
                    <button
                      disabled={busy}
                      onClick={() => void chooseAndOpen(false)}
                      type="button"
                    >
                      打开已有仓库
                    </button>
                    {rememberedPath && (
                      <button
                        disabled={busy}
                        type="button"
                        onClick={() => void openRemembered()}
                      >
                        打开上次仓库
                      </button>
                    )}
                  </div>
                </section>
              ) : page === "settings" ? (
                <div
                  className="settings-stack"
                  key={`${warehouse?.warehouseId}:${warehouse?.displayPath}`}
                >
                  <ThemeSettings />
                  <FullBackupPanel
                    writable={warehouse?.accessMode === "write"}
                    disabled={busy}
                    onError={reportError}
                    onOpen={openTransferred}
                  />
                  <ExternalDatabasePanel
                    disabled={busy}
                    onError={reportError}
                    onOpen={openTransferred}
                  />
                  {warehouse && (
                    <SettingsPage
                      onError={reportError}
                      writable={!busy && warehouse.accessMode === "write"}
                    />
                  )}
                </div>
              ) : page === "overview" ? (
                <>
                  <OverviewPage
                    key={context}
                    writable={!busy && warehouse!.accessMode === "write"}
                    onError={reportError}
                    onDrilldown={(drilldown) =>
                      void openDestination("applications", { drilldown })
                    }
                    onTask={(taskId) =>
                      void openDestination("tasks", { taskId })
                    }
                    onEvent={(eventId) =>
                      void openDestination("tasks", { eventId })
                    }
                    onSchedule={(schedule) =>
                      void openDestination("tasks", { schedule })
                    }
                    onSettings={() => void navigate("settings")}
                    onNew={() =>
                      void openDestination("applications", { create: true })
                    }
                  />
                  <WarehouseCard warehouse={warehouse!} />
                </>
              ) : page === "tasks" ? (
                <ProductivityPage
                  key={`${context}:${JSON.stringify(currentDestination)}`}
                  writable={!busy && warehouse!.accessMode === "write"}
                  onError={reportError}
                  initialTaskId={currentDestination?.taskId}
                  initialEventId={currentDestination?.eventId}
                  initialSchedule={currentDestination?.schedule}
                  onOpenApplication={(id, archived) =>
                    void openDestination(
                      archived ? "archive" : "applications",
                      {
                        drilldown: { label: "日程关联投递", ids: [id] },
                      },
                    )
                  }
                />
              ) : page === "applications" ? (
                <ApplicationPage
                  key={`${context}:active:${currentDestination?.drilldown?.label ?? ""}:${currentDestination?.create ?? false}`}
                  drilldown={currentDestination?.drilldown}
                  initialCreateOpen={currentDestination?.create}
                  onError={reportError}
                  scope="active"
                  onRecycle={() => void navigate("recycle")}
                  writable={!busy && warehouse!.accessMode === "write"}
                />
              ) : page === "archive" ? (
                <ApplicationPage
                  key={`${warehouse!.warehouseId}:${warehouse!.displayPath}:archived`}
                  drilldown={currentDestination?.drilldown}
                  onError={reportError}
                  scope="archived"
                  onRecycle={() => void navigate("recycle")}
                  writable={!busy && warehouse!.accessMode === "write"}
                />
              ) : page === "templates" ? (
                <WorkflowTemplatesPage
                  key={`${warehouse!.warehouseId}:${warehouse!.displayPath}`}
                  onError={reportError}
                  writable={!busy && warehouse!.accessMode === "write"}
                />
              ) : page === "recycle" ? (
                <RecycleBinPage
                  key={`${warehouse!.warehouseId}:${warehouse!.displayPath}`}
                  onError={reportError}
                  writable={!busy && warehouse!.accessMode === "write"}
                />
              ) : (
                <section className="placeholder">
                  <p className="eyebrow">功能边界</p>
                  <h2>{selectedPage.label}</h2>
                  <p>
                    {selectedPage.phase === 1
                      ? "阶段 1 已建立此页面入口；具体设置项会随对应功能分阶段接入。"
                      : `该业务功能计划在阶段 ${selectedPage.phase} 实现，本阶段不提前加入占位数据。`}
                  </p>
                </section>
              )}
            </div>

            {warehouse && (
              <footer className="workspace-footer">
                <span>仓库 ID：{warehouse.warehouseId.slice(0, 8)}…</span>
                <button
                  disabled={busy}
                  onClick={() => void closeWarehouse()}
                  type="button"
                >
                  关闭仓库
                </button>
              </footer>
            )}
          </main>
        </div>
      </OverviewProvider>
    </AgentSnapshotProvider>
  );
}
