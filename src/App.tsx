import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { WarehouseSummary } from "./contracts";
import { selectDirectory } from "./lib/dialog";
import { desktopApi, OfferTrackError } from "./lib/tauri";

const pages = [
  { id: "overview", label: "概览", phase: 1 },
  { id: "applications", label: "投递记录", phase: 2 },
  { id: "tasks", label: "待办与提醒", phase: 3 },
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
  const [page, setPage] = useState<PageId>("overview");
  const [warehouse, setWarehouse] = useState<WarehouseSummary | null>(null);
  const [rememberedPath, setRememberedPath] = useState<string | null>(null);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [busy, setBusy] = useState(true);

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
      ...(known.code === "WAREHOUSE_LOCKED" && lockedPath
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
    [reportError],
  );

  const openRemembered = async () => {
    if (!rememberedPath) return;
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

  const openReadOnly = async (path: string) => {
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
  }, [reportError]);

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
        case "help":
          setPage(event.payload);
          break;
        case "about":
          setNotice({
            kind: "info",
            text: "OfferTrack 0.1.0 · 本地优先的求职投递管理工具 · MIT License",
          });
          break;
      }
    });

    return () => {
      void unlisten.then((dispose) => dispose());
    };
  }, [chooseAndOpen, closeWarehouse]);

  const selectedPage = pages.find((item) => item.id === page) ?? pages[0];

  return (
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
              key={item.id}
              onClick={() => setPage(item.id)}
              type="button"
            >
              <span>{item.label}</span>
              {item.phase > 1 && <small>阶段 {item.phase}</small>}
            </button>
          ))}
        </nav>
        <div className="sidebar-footer">
          <span className={warehouse ? "status-dot online" : "status-dot"} />
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

        <div className="content">
          {notice && (
            <div className={`notice ${notice.kind}`} role="alert">
              <span>{notice.text}</span>
              {notice.lockedPath && (
                <button
                  onClick={() => void openReadOnly(notice.lockedPath!)}
                  type="button"
                >
                  只读打开
                </button>
              )}
            </div>
          )}

          {page === "overview" ? (
            <>
              {warehouse ? (
                <WarehouseCard warehouse={warehouse} />
              ) : (
                <section className="empty-state">
                  <div className="empty-icon" aria-hidden="true">
                    OT
                  </div>
                  <p className="eyebrow">开始使用</p>
                  <h2>选择一个本地文件夹作为数据仓库</h2>
                  <p>
                    投递数据、简历和备份将保存在你指定的位置。OfferTrack
                    不依赖云服务，也不会把真实数据写入应用安装目录。
                  </p>
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
                        onClick={() => void openRemembered()}
                        type="button"
                      >
                        打开上次仓库
                      </button>
                    )}
                  </div>
                </section>
              )}

              <section className="stage-grid" aria-label="阶段一能力">
                <article>
                  <span>01</span>
                  <h3>本地优先</h3>
                  <p>数据库与附件目录位于用户选择的数据仓库中。</p>
                </article>
                <article>
                  <span>02</span>
                  <h3>单写者保护</h3>
                  <p>同一仓库只允许一个写入实例，其他实例可只读查看。</p>
                </article>
                <article>
                  <span>03</span>
                  <h3>版本化迁移</h3>
                  <p>仓库格式和数据库结构都带版本，为后续升级留出边界。</p>
                </article>
              </section>
            </>
          ) : page === "help" ? (
            <section className="placeholder help-content">
              <p className="eyebrow">阶段 1 使用说明</p>
              <h2>帮助</h2>
              <ol>
                <li>
                  点击“新建仓库”，选择一个空文件夹；OfferTrack
                  会建立版本化数据库和附件目录。
                </li>
                <li>
                  已有仓库使用“打开仓库”。应用会校验仓库格式和数据库迁移版本。
                </li>
                <li>同一仓库只能有一个写入实例；发生占用时可选择只读打开。</li>
                <li>
                  看到网络盘、云同步或移动磁盘提示时，请避免多设备同时写入并及时备份。
                </li>
              </ol>
              <p>
                投递记录、回收站、备份与 Agent
                接口将在对应开发阶段补充到本帮助中。
              </p>
            </section>
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
  );
}
