import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { WarehouseSummary } from "./contracts";
import { selectDirectory } from "./lib/dialog";
import { desktopApi, OfferTrackError } from "./lib/tauri";
import { ApplicationPage } from "./features/applications/ApplicationPage";
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
  { id: "tasks", label: "待办与提醒", phase: 3 },
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
    <DraftGuardProvider>
      <AppContent />
    </DraftGuardProvider>
  );
}

function AppContent() {
  const { confirmLeave } = useDraftGuard();
  const [page, setPage] = useState<PageId>("overview");
  const [warehouse, setWarehouse] = useState<WarehouseSummary | null>(null);
  const [rememberedPath, setRememberedPath] = useState<string | null>(null);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [busy, setBusy] = useState(true);
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
      if (target === page || !(await confirmLeave())) return;
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
        case "help":
          void navigate(event.payload);
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
  }, [chooseAndOpen, closeWarehouse, navigate]);

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
              disabled={busy}
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

        <div
          className={
            page === "applications" || page === "archive"
              ? "content content-wide"
              : "content"
          }
        >
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

          {!warehouse && page !== "help" ? (
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
              </div>
            </section>
          ) : page === "overview" ? (
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

              <section className="stage-grid" aria-label="当前能力">
                <article>
                  <span>01</span>
                  <h3>投递记录</h3>
                  <p>
                    阶段 2 已支持记录、流程、面试轮次、筛选分组和持久化视图。
                  </p>
                </article>
                <article>
                  <span>02</span>
                  <h3>独立文件夹</h3>
                  <p>
                    每条投递拥有独立目录，可直接通过文件管理器维护简历材料。
                  </p>
                </article>
                <article>
                  <span>03</span>
                  <h3>安全回收站</h3>
                  <p>删除先移入仓库回收站，确认清空时才永久删除。</p>
                </article>
              </section>
            </>
          ) : page === "applications" ? (
            <ApplicationPage
              key={`${warehouse!.warehouseId}:active`}
              onError={reportError}
              scope="active"
              writable={!busy && warehouse!.accessMode === "write"}
            />
          ) : page === "archive" ? (
            <ApplicationPage
              key={`${warehouse!.warehouseId}:archived`}
              onError={reportError}
              scope="archived"
              writable={!busy && warehouse!.accessMode === "write"}
            />
          ) : page === "templates" ? (
            <WorkflowTemplatesPage
              key={warehouse!.warehouseId}
              onError={reportError}
              writable={!busy && warehouse!.accessMode === "write"}
            />
          ) : page === "recycle" ? (
            <RecycleBinPage
              key={warehouse!.warehouseId}
              onError={reportError}
              writable={!busy && warehouse!.accessMode === "write"}
            />
          ) : page === "settings" ? (
            <SettingsPage
              key={warehouse!.warehouseId}
              onError={reportError}
              writable={!busy && warehouse!.accessMode === "write"}
            />
          ) : page === "help" ? (
            <section className="placeholder help-content">
              <p className="eyebrow">阶段 2 使用说明</p>
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
                  在“投递记录”中新建记录。首次切换到“已投递”时，系统自动填写当天投递日期。
                </li>
                <li>
                  单击一行打开右侧详情；资料、招聘阶段、面试轮次、文件和变更历史分别管理。
                </li>
                <li>
                  资料修改后点击“保存修改”。新建表单、资料或流程表单未保存时，离开页面或关闭窗口会提示；保存进行中请等待完成。
                </li>
                <li>
                  “流程”中可编辑当前投递的阶段名称与颜色；面试轮次可记录本地计划/完成时间、状态、结果和备注。保存为模板不会改变已有投递。
                </li>
                <li>
                  “调整阶段顺序”仅改变当前投递的展示顺序；“流程模板”页可编辑、复制模板并设为默认，只影响以后新建的投递。进度条表示流程位置，不代表成功概率。
                </li>
                <li>
                  “管理辅助状态”可改名、添加或排序当前投递及面试轮次的状态。模板辅助状态单独管理，历史保留当时名称；被使用的自定义状态不能移除。
                </li>
                <li>
                  可以直接在投递文件夹中增删文件，应用会监听变化，也可点击“重新扫描”。
                </li>
                <li>
                  文件按子目录折叠显示；双击文件名用默认应用打开。右键或“更多”可选择其他应用、打开所在文件夹或复制路径，PDF
                  还可选择已检测到的浏览器。网址
                  Ctrl+点击使用默认浏览器，右键可更换浏览器或复制链接。
                </li>
                <li>
                  “复制公司信息”建立新岗位；“复制完整记录”会复制数据和文件，但副本始终使用独立目录。
                </li>
                <li>
                  归档不会删除内容；删除会移入 OfferTrack
                  回收站，清空回收站需再次确认。
                </li>
                <li>
                  恢复遇到重名会使用新目录并显示实际位置，不覆盖原位置。清空不可撤销；失败项剩余内容保留，可释放占用后重试。
                </li>
                <li>
                  看到网络盘、云同步或移动磁盘提示时，请避免多设备同时写入并及时备份。
                </li>
              </ol>
              <p>
                概览统计、待办提醒、备份恢复、导出与 Agent
                接口属于后续阶段，本阶段未提前实现。
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
