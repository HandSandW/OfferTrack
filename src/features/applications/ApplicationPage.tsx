import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type {
  ApplicationListItem,
  ApplicationScope,
  ColumnSetting,
  FieldDefinition,
  GroupKey,
  SavedView,
  SortRule,
} from "../../contracts";
import { desktopApi, OfferTrackError } from "../../lib/tauri";

import {
  companyTypes,
  defaultColumns,
  columnLabels,
  initialFilter,
  initialCreate,
  dateOnly,
  companyTypeName,
  rawValue,
  columnsWithFields,
  filterAndSort,
} from "./tableModel";
import { ApplicationGrid } from "./ApplicationGrid";
import { useDraftGuard, useDraftState } from "../../shared/draftGuard";
import { Modal } from "../../shared/Modal";
import { ViewControls } from "../views/ViewControls";
import { PresetViewControls } from "../views/PresetViewControls";
import { presetConfiguration } from "../views/presets";
import { UrlLink as LinkValue } from "../../shared/UrlLink";
import { BatchDialog } from "../batch/BatchDialog";
import { ExportDialog } from "../export/ExportDialog";
import type { BatchTarget } from "../batch/contracts";
import type { Drilldown } from "../productivity/contracts";

// Shared file/URL menu preserves ordinary-click row selection.

function Cell({
  record,
  column,
  onError,
}: {
  record: ApplicationListItem;
  column: ColumnSetting;
  onError: (error: unknown) => void;
}) {
  if (column.key === "companyType") {
    return (
      <span className={`company-badge ${record.companyType}`}>
        {companyTypeName(record.companyType)}
      </span>
    );
  }
  if (column.key === "currentStageName") {
    const progress = Math.min(100, Math.max(0, record.currentStageProgress));
    return (
      <span
        className="progress-cell"
        style={{
          background: `linear-gradient(90deg, ${record.currentStageState === "failed" ? "#dc2626" : record.currentStageColor}33 ${progress}%, var(--surface-subtle) ${progress}%)`,
        }}
      >
        {record.currentStageState === "failed" ? "已挂 · " : ""}
        {record.currentStageName}
        {record.currentStateName ? ` · ${record.currentStateName}` : ""}
      </span>
    );
  }
  if (column.key === "createdAtUtc")
    return <>{dateOnly(record.createdAtUtc)}</>;
  if (column.key === "statusUpdatedAtUtc")
    return <>{dateOnly(record.statusUpdatedAtUtc)}</>;
  if (column.key === "applicationDate")
    return <>{dateOnly(record.applicationDate)}</>;
  if (column.key === "applicationUrl" && record.applicationUrl)
    return <LinkValue value={record.applicationUrl} onError={onError} />;
  if (column.key === "announcementUrl" && record.announcementUrl)
    return <LinkValue value={record.announcementUrl} onError={onError} />;
  if (
    (column.key === "companyUrl" || column.key === "positionUrl") &&
    record[column.key]
  )
    return <LinkValue value={record[column.key] ?? ""} onError={onError} />;
  const text = String(rawValue(record, column.key) ?? "—");
  return (
    <span className="truncate" title={text}>
      {text}
    </span>
  );
}

interface ApplicationPageProps {
  onRecycle?: (() => void) | undefined;
  drilldown?: Drilldown | undefined;
  initialCreateOpen?: boolean | undefined;
  scope: Extract<ApplicationScope, "active" | "archived">;
  writable: boolean;
  onError: (error: unknown) => void;
}

export function ApplicationPage({
  drilldown,
  initialCreateOpen = false,
  scope,
  writable,
  onError,
  onRecycle,
}: ApplicationPageProps) {
  const { confirmLeave } = useDraftGuard();
  const [records, setRecords] = useState<ApplicationListItem[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [fields, setFields] = useState<FieldDefinition[]>([]);
  const [views, setViews] = useState<SavedView[]>([]);
  const [columns, setColumns] = useState(defaultColumns);
  const [sort, setSort] = useState<SortRule[]>([
    { key: "createdAtUtc", direction: "desc" },
  ]);
  const [filter, setFilter] = useState(initialFilter);
  const [group, setGroup] = useState<GroupKey | null>(null);
  const [pageSize, setPageSize] = useState(50);
  const [pageSizeInput, setPageSizeInput] = useState("50");
  const [pageSizeError, setPageSizeError] = useState("");
  const [page, setPage] = useState(1);
  const [showColumns, setShowColumns] = useState(false);
  const [showCreate, setShowCreate] = useState(initialCreateOpen);
  const [sourceScope, setSourceScope] = useState(drilldown);
  const [createForm, setCreateForm] = useState(initialCreate);
  const [busy, setBusy] = useState(false);
  const [activeViewId, setActiveViewId] = useState("");
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState("");
  const [loadAttempt, setLoadAttempt] = useState(0);
  const [checked, setChecked] = useState<BatchTarget[]>([]);
  const [batchTargets, setBatchTargets] = useState<BatchTarget[] | null>(null);
  const [exportSelection, setExportSelection] = useState<{
    filtered: BatchTarget[];
    selected: BatchTarget[];
    columns: ColumnSetting[];
  } | null>(null);
  const selectedRef = useRef<string | null>(null);
  useDraftState(
    showCreate && JSON.stringify(createForm) !== JSON.stringify(initialCreate),
    busy,
    "新建投递",
  );

  const refresh = useCallback(async () => {
    try {
      const next = await desktopApi.listApplications(scope);
      setRecords(next);
      // A file-index refresh must not silently unmount an edited record. If it
      // disappears externally, keep the draft; explicit saving reports conflict.
    } catch (error) {
      onError(error);
    }
  }, [onError, scope]);

  useEffect(() => {
    let active = true;
    void Promise.all([
      desktopApi.listApplications(scope),
      desktopApi.listFieldDefinitions(),
      desktopApi.listApplicationViews(),
      desktopApi.getApplicationPageSize(),
    ])
      .then(([nextRecords, nextFields, nextViews, size]) => {
        if (!active) return;
        setRecords(nextRecords);
        setFields(nextFields);
        setViews(nextViews);
        setPageSize(size);
        setPageSizeInput(String(size));
        const view = nextViews.find((item) => item.isDefault);
        setColumns(
          columnsWithFields(view?.layout.columns ?? defaultColumns, nextFields),
        );
        if (view && !drilldown) {
          setActiveViewId(view.id);
          setSort(view.sort);
          setFilter(view.filter);
          setGroup(view.group);
        }
      })
      .catch((error: unknown) => {
        if (active) {
          setLoadError(
            error instanceof Error ? error.message : "投递或视图读取失败",
          );
          onError(error);
        }
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [onError, scope, loadAttempt, drilldown]);

  useEffect(() => {
    if (
      loading ||
      loadError ||
      drilldown?.ids.length !== 1 ||
      selectedRef.current
    )
      return;
    let active = true;
    const id = drilldown.ids[0]!;
    if (!records.some((record) => record.id === id)) {
      onError(
        new OfferTrackError({
          code: "APPLICATION_SCOPE_CHANGED",
          message: "该投递已不在当前分区，请刷新概览后重试。",
          retryable: true,
        }),
      );
      return;
    }
    setBusy(true);
    void desktopApi
      .setApplicationDetailTarget(id, true)
      .then(() => {
        if (active) {
          selectedRef.current = id;
          setSelectedId(id);
        }
      })
      .catch((error: unknown) => {
        if (active) onError(error);
      })
      .finally(() => {
        if (active) setBusy(false);
      });
    return () => {
      active = false;
    };
  }, [drilldown, loading, loadError, onError, records]);

  useEffect(() => {
    let timer = 0;
    let active = true;
    const rescan = async () => {
      try {
        if (writable) await desktopApi.refreshFileIndex();
        if (!active) return;
        await refresh();
      } catch (error) {
        if (
          active &&
          !(
            error instanceof OfferTrackError &&
            error.code === "WAREHOUSE_OPERATION_BUSY"
          )
        )
          onError(error);
      }
    };
    const schedule = () => {
      window.clearTimeout(timer);
      timer = window.setTimeout(() => {
        void rescan();
      }, 450);
    };
    const unlisten = listen("filesystem-changed", schedule);
    window.addEventListener("focus", schedule);
    return () => {
      active = false;
      window.clearTimeout(timer);
      window.removeEventListener("focus", schedule);
      void unlisten.then((dispose) => dispose());
    };
  }, [onError, refresh, writable]);

  useEffect(() => {
    const unlisten = listen("application-detail-changed", () => void refresh());
    return () => void unlisten.then((dispose) => dispose());
  }, [refresh]);

  const select = (id: string) => {
    if (id === selectedRef.current) return;
    selectedRef.current = id;
    setSelectedId(id);
    void desktopApi.setApplicationDetailTarget(id, false).catch(onError);
  };

  const openDetail = (id: string) => {
    selectedRef.current = id;
    setSelectedId(id);
    void desktopApi.setApplicationDetailTarget(id, true).catch(onError);
  };

  const visibleColumns = columns.filter((column) => column.visible);
  const labels = {
    ...columnLabels,
    ...Object.fromEntries(
      fields.map((field) => [`custom:${field.id}`, field.displayName]),
    ),
  };
  const processed = useMemo(
    () =>
      filterAndSort(
        sourceScope
          ? records.filter((r) => sourceScope.ids.includes(r.id))
          : records,
        filter,
        sort,
      ),
    [records, filter, sort, sourceScope],
  );
  const pageCount = Math.max(1, Math.ceil(processed.length / pageSize));
  const currentPage = Math.min(page, pageCount);
  const pageRecords = processed.slice(
    (currentPage - 1) * pageSize,
    currentPage * pageSize,
  );
  const grouped = useMemo(() => {
    if (!group) return [["", pageRecords]] as [string, ApplicationListItem[]][];
    const buckets = new Map<string, ApplicationListItem[]>();
    for (const record of pageRecords) {
      const key = String(rawValue(record, group) || "未填写");
      buckets.set(key, [...(buckets.get(key) ?? []), record]);
    }
    return [...buckets.entries()];
  }, [group, pageRecords]);

  const toggleSort = (key: string) => {
    setSort((current) => {
      const existing = current.find((item) => item.key === key);
      return [
        { key, direction: existing?.direction === "asc" ? "desc" : "asc" },
      ];
    });
  };

  const loadView = (id: string) => {
    if (!id) {
      setActiveViewId("");
      return;
    }
    const view = views.find((item) => item.id === id);
    if (!view) return;
    applyView(view);
  };

  const applyView = (view: SavedView) => {
    setActiveViewId(view.id);
    setColumns(columnsWithFields(view.layout.columns, fields));
    setSort(view.sort);
    setFilter(view.filter ?? initialFilter);
    setGroup(view.group);
    setPage(1);
  };

  const createRecord = async () => {
    setBusy(true);
    try {
      const created = await desktopApi.createApplication(createForm);
      setShowCreate(false);
      setCreateForm(initialCreate);
      await refresh();
      select(created.id);
    } catch (error) {
      onError(error);
    } finally {
      setBusy(false);
    }
  };

  const cancelCreate = async () => {
    if (await confirmLeave()) {
      setShowCreate(false);
      setCreateForm(initialCreate);
    }
  };

  const chooseBatch = (next: ApplicationListItem[]) => {
    if (next.length > 200) {
      onError(new Error("每批最多 200 条，请缩小选择范围。"));
      return;
    }
    setChecked(next.map(({ id, revision }) => ({ id, revision })));
  };
  const reloadAfterBatch = async () => {
    setChecked([]);
    const next = await desktopApi.listApplications(scope);
    setRecords(next);
  };

  return (
    <section className="applications-workspace">
      <div className="table-pane">
        {sourceScope && (
          <div className="notice info">
            <span>
              来自概览：{sourceScope.label} · {sourceScope.ids.length}{" "}
              条快照范围；不写入保存视图。当前结果可能因归档或删除减少。
            </span>
            <button onClick={() => setSourceScope(undefined)}>
              清除概览范围
            </button>
          </div>
        )}
        {loading && <p role="status">正在读取投递、字段和视图…</p>}
        {loadError && (
          <div role="alert">
            {loadError}
            <button
              type="button"
              disabled={loading}
              onClick={() => {
                setLoading(true);
                setLoadError("");
                setLoadAttempt((n) => n + 1);
              }}
            >
              重新读取投递与视图
            </button>
          </div>
        )}
        <div className="record-toolbar">
          <PresetViewControls
            filter={filter}
            sort={sort}
            scoped={!!sourceScope}
            disabled={loading || !!loadError || busy}
            onRecycle={onRecycle}
            onPreset={(id) => {
              const preset = presetConfiguration(id);
              if (!preset) return;
              setFilter(preset.filter);
              setSort(preset.sort);
              setSourceScope(undefined);
              setActiveViewId("");
              setPage(1);
            }}
            onState={(businessState) => {
              const next = { ...filter };
              if (businessState) next.businessState = businessState;
              else delete next.businessState;
              setFilter(next);
              setPage(1);
            }}
          />
          <input
            aria-label="搜索投递"
            onChange={(event) => {
              setFilter({ ...filter, search: event.target.value });
              setPage(1);
            }}
            placeholder="搜索公司、岗位、标签或备注"
            value={filter.search}
          />
          <select
            aria-label="企业性质筛选"
            onChange={(event) =>
              setFilter({
                ...filter,
                companyTypes: event.target.value ? [event.target.value] : [],
              })
            }
            value={filter.companyTypes[0] ?? ""}
          >
            <option value="">全部企业性质</option>
            {companyTypes.map(([key, label]) => (
              <option key={key} value={key}>
                {label}
              </option>
            ))}
          </select>
          <select
            aria-label="阶段筛选"
            value={filter.stages[0] ?? ""}
            onChange={(event) => {
              setFilter({
                ...filter,
                stages: event.target.value ? [event.target.value] : [],
              });
              setPage(1);
            }}
          >
            <option value="">全部阶段</option>
            {[...new Set(records.map((record) => record.currentStageName))].map(
              (name) => (
                <option key={name} value={name}>
                  {name}
                </option>
              ),
            )}
          </select>
          <select
            aria-label="分组"
            onChange={(event) =>
              setGroup((event.target.value || null) as GroupKey | null)
            }
            value={group ?? ""}
          >
            <option value="">不分组</option>
            <option value="companyType">按企业性质</option>
            <option value="currentStageName">按投递进度</option>
            <option value="workLocation">按工作地点</option>
          </select>
          <ViewControls
            views={views}
            activeId={activeViewId}
            current={{ layout: { columns }, sort, filter, group }}
            writable={writable}
            disabled={loading || !!loadError || busy}
            onSelect={loadView}
            onViews={(next) => {
              setViews(next);
              if (!next.some((v) => v.id === activeViewId)) setActiveViewId("");
            }}
            onApply={applyView}
            onError={onError}
          />
          <button onClick={() => setShowColumns(!showColumns)} type="button">
            列设置
          </button>
          <button
            type="button"
            disabled={loading || !!loadError || busy}
            onClick={() =>
              void (async () => {
                if (!(await confirmLeave())) return;
                setExportSelection({
                  filtered: processed.map((r) => ({
                    id: r.id,
                    revision: r.revision,
                  })),
                  selected: [...checked],
                  columns: [...columns],
                });
              })()
            }
          >
            导出
          </button>
          <button
            onClick={() => {
              setColumns(columnsWithFields(defaultColumns, fields));
              setSort([{ key: "createdAtUtc", direction: "desc" }]);
              setFilter(initialFilter);
              setGroup(null);
              setActiveViewId("");
            }}
            type="button"
          >
            恢复默认布局
          </button>
          {scope === "active" && (
            <button
              className="primary"
              disabled={!writable || loading || !!loadError || busy}
              onClick={() => {
                void (async () => {
                  if (await confirmLeave()) {
                    setShowCreate(true);
                  }
                })();
              }}
              type="button"
            >
              新建投递
            </button>
          )}
        </div>

        <div className="record-toolbar" aria-label="批量选择工具">
          <button
            type="button"
            disabled={loading || !!loadError || busy}
            onClick={() => chooseBatch(pageRecords)}
          >
            选择本页
          </button>
          {selectedId && (
            <button
              type="button"
              disabled={busy}
              onClick={() =>
                chooseBatch(
                  records.filter(
                    (r) =>
                      r.companyName ===
                      records.find((item) => item.id === selectedId)
                        ?.companyName,
                  ),
                )
              }
            >
              选择当前列表中同公司投递
            </button>
          )}
          <span>已选 {checked.length} 条（跨页保留）</span>
          {!!checked.length && (
            <>
              <button type="button" onClick={() => setChecked([])}>
                清除选择
              </button>
              <button
                type="button"
                disabled={!writable || busy || loading || !!loadError}
                onClick={() =>
                  void (async () => {
                    if (!(await confirmLeave())) return;
                    selectedRef.current = null;
                    setSelectedId(null);
                    setBatchTargets([...checked]);
                  })()
                }
              >
                批量修改
              </button>
            </>
          )}
          <button
            type="button"
            disabled={busy || loading}
            onClick={() =>
              void (async () => {
                try {
                  const next = await desktopApi.listApplications(scope);
                  setRecords(next);
                  setChecked([]);
                } catch (error) {
                  onError(error);
                }
              })()
            }
          >
            刷新投递并清除选择
          </button>
        </div>

        {showColumns && (
          <div className="column-settings">
            {columns.map((column, index) => (
              <div
                draggable
                key={column.key}
                onDragStart={(event) =>
                  event.dataTransfer.setData("text/plain", String(index))
                }
                onDragOver={(event) => event.preventDefault()}
                onDrop={(event) => {
                  const from = Number(event.dataTransfer.getData("text/plain"));
                  const next = [...columns];
                  const [moved] = next.splice(from, 1);
                  if (!moved) return;
                  next.splice(index, 0, moved);
                  setColumns(next);
                }}
              >
                <span className="drag-handle">⋮⋮</span>
                <label>
                  <input
                    checked={column.visible}
                    onChange={(event) =>
                      setColumns(
                        columns.map((item) =>
                          item.key === column.key
                            ? { ...item, visible: event.target.checked }
                            : item,
                        ),
                      )
                    }
                    type="checkbox"
                  />
                  {labels[column.key]}
                </label>
                <label>
                  宽{" "}
                  <input
                    min="80"
                    onChange={(event) =>
                      setColumns(
                        columns.map((item) =>
                          item.key === column.key
                            ? {
                                ...item,
                                width: Math.min(
                                  600,
                                  Math.max(80, Number(event.target.value)),
                                ),
                              }
                            : item,
                        ),
                      )
                    }
                    type="number"
                    value={column.width}
                  />
                </label>
                <label>
                  <input
                    checked={column.pinned}
                    onChange={(event) =>
                      setColumns(
                        columns.map((item) =>
                          item.key === column.key
                            ? { ...item, pinned: event.target.checked }
                            : item,
                        ),
                      )
                    }
                    type="checkbox"
                  />
                  固定
                </label>
              </div>
            ))}
          </div>
        )}

        <ApplicationGrid
          grouped={grouped}
          grouping={!!group}
          columns={visibleColumns}
          labels={labels}
          sort={sort}
          fields={fields}
          selectedId={selectedId}
          checked={checked}
          writable={writable}
          disabled={busy || loading || !!loadError}
          empty={!loading && !loadError}
          onSort={toggleSort}
          onFocus={select}
          onOpen={openDetail}
          onColumnsChange={(nextVisible) => {
            const visible = [...nextVisible];
            setColumns((current) => {
              let visibleIndex = 0;
              return current.map((item) =>
                item.visible ? (visible[visibleIndex++] ?? item) : item,
              );
            });
          }}
          onBeforeEdit={async () => {
            if (!(await confirmLeave())) return false;
            return true;
          }}
          onUpdated={(record) => {
            setRecords((current) =>
              current.map((item) =>
                item.id === record.id && item.revision <= record.revision
                  ? record
                  : item,
              ),
            );
          }}
          onChecked={setChecked}
          onError={onError}
          renderCell={(record, column) => (
            <Cell record={record} column={column} onError={onError} />
          )}
        />
        <div className="pagination">
          <span>
            {loading || loadError
              ? "记录数量待确认"
              : `共 ${processed.length} 条`}
          </span>
          <select
            aria-label="每页条数快捷选择"
            disabled={loading || !!loadError}
            onChange={(event) => {
              if (event.target.value === "custom") return;
              const value = Number(event.target.value);
              setPageSize(value);
              setPageSizeInput(String(value));
              setPageSizeError("");
              setPage(1);
            }}
            value={[20, 50, 100, 200].includes(pageSize) ? pageSize : "custom"}
          >
            {[20, 50, 100, 200].map((size) => (
              <option key={size} value={size}>
                每页 {size} 条
              </option>
            ))}
            <option value="custom">自定义</option>
          </select>
          <label className="page-size-input">
            每页
            <input
              aria-label="自定义每页条数"
              disabled={loading || !!loadError}
              inputMode="numeric"
              min={1}
              max={500}
              type="number"
              value={pageSizeInput}
              onChange={(event) => {
                const text = event.target.value;
                setPageSizeInput(text);
                const value = Number(text);
                if (!/^\d+$/.test(text) || value < 1 || value > 500) {
                  setPageSizeError("请输入 1–500 的整数。");
                  return;
                }
                setPageSizeError("");
                setPageSize(value);
                setPage(1);
              }}
            />
            条
          </label>
          {pageSizeError && <span role="alert">{pageSizeError}</span>}
          <button
            disabled={
              !writable || loading || !!loadError || busy || !!pageSizeError
            }
            onClick={() =>
              void desktopApi.setApplicationPageSize(pageSize).catch(onError)
            }
            type="button"
          >
            设为默认
          </button>
          <button
            disabled={currentPage <= 1}
            onClick={() => setPage(currentPage - 1)}
            type="button"
          >
            上一页
          </button>
          <span>
            {currentPage} / {pageCount}
          </span>
          <button
            disabled={currentPage >= pageCount}
            onClick={() => setPage(currentPage + 1)}
            type="button"
          >
            下一页
          </button>
        </div>
      </div>

      {batchTargets && (
        <BatchDialog
          targets={batchTargets}
          onClose={() => setBatchTargets(null)}
          onApplied={reloadAfterBatch}
        />
      )}
      {exportSelection && (
        <ExportDialog
          partition={scope}
          {...exportSelection}
          onClose={() => setExportSelection(null)}
        />
      )}

      {showCreate && (
        <Modal title="新建投递" onCancel={() => void cancelCreate()}>
          <form
            onSubmit={(event) => {
              event.preventDefault();
              void createRecord();
            }}
          >
            <fieldset className="editor-fields" disabled={busy}>
              <label>
                公司名称
                <input
                  autoFocus
                  required
                  value={createForm.companyName}
                  onChange={(event) =>
                    setCreateForm({
                      ...createForm,
                      companyName: event.target.value,
                    })
                  }
                />
              </label>
              <label>
                岗位名称
                <input
                  required
                  value={createForm.positionName}
                  onChange={(event) =>
                    setCreateForm({
                      ...createForm,
                      positionName: event.target.value,
                    })
                  }
                />
              </label>
              <label>
                企业性质
                <select
                  value={createForm.companyType}
                  onChange={(event) =>
                    setCreateForm({
                      ...createForm,
                      companyType: event.target.value,
                    })
                  }
                >
                  {companyTypes.map(([key, label]) => (
                    <option key={key} value={key}>
                      {label}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                行业
                <input
                  value={createForm.industry}
                  onChange={(event) =>
                    setCreateForm({
                      ...createForm,
                      industry: event.target.value,
                    })
                  }
                />
              </label>
              <label>
                岗位类别
                <input
                  value={createForm.positionCategory}
                  onChange={(event) =>
                    setCreateForm({
                      ...createForm,
                      positionCategory: event.target.value,
                    })
                  }
                />
              </label>
              <label>
                工作地点
                <input
                  value={createForm.workLocation}
                  onChange={(event) =>
                    setCreateForm({
                      ...createForm,
                      workLocation: event.target.value,
                    })
                  }
                />
              </label>
              <div className="modal-actions">
                <button onClick={() => void cancelCreate()} type="button">
                  取消
                </button>
                <button className="primary" disabled={busy} type="submit">
                  创建投递
                </button>
              </div>
            </fieldset>
          </form>
        </Modal>
      )}
    </section>
  );
}
