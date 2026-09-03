import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type {
  ApplicationDetail,
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
  pinnedOffsets,
} from "./tableModel";
import { DetailPanel } from "./DetailPanel";
import { useDraftGuard, useDraftState } from "../../shared/draftGuard";
import { Modal } from "../../shared/Modal";
import { ViewControls } from "../views/ViewControls";
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
        {record.currentStageName} · {record.currentStateName}
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
}: ApplicationPageProps) {
  const { confirmLeave } = useDraftGuard();
  const [records, setRecords] = useState<ApplicationListItem[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [detail, setDetail] = useState<ApplicationDetail | null>(null);
  const [fields, setFields] = useState<FieldDefinition[]>([]);
  const [views, setViews] = useState<SavedView[]>([]);
  const [columns, setColumns] = useState(defaultColumns);
  const [sort, setSort] = useState<SortRule[]>([
    { key: "createdAtUtc", direction: "desc" },
  ]);
  const [filter, setFilter] = useState(initialFilter);
  const [group, setGroup] = useState<GroupKey | null>(null);
  const [pageSize, setPageSize] = useState(50);
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
  const selectionRequest = useRef(0);
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
    const request = ++selectionRequest.current;
    setBusy(true);
    void desktopApi
      .getApplication(drilldown.ids[0]!)
      .then((next) => {
        if (active && request === selectionRequest.current) {
          if (
            next.deletedAtUtc ||
            (scope === "archived" ? !next.archivedAtUtc : next.archivedAtUtc)
          ) {
            onError(
              new OfferTrackError({
                code: "APPLICATION_SCOPE_CHANGED",
                message: "该投递已不在当前分区，请刷新概览后重试。",
                retryable: true,
              }),
            );
            return;
          }
          selectedRef.current = next.id;
          setSelectedId(next.id);
          setDetail(next);
        }
      })
      .catch((error: unknown) => {
        if (active) onError(error);
      })
      .finally(() => {
        if (active && request === selectionRequest.current) setBusy(false);
      });
    return () => {
      active = false;
    };
  }, [drilldown, loading, loadError, onError, scope]);

  useEffect(() => {
    let timer = 0;
    let active = true;
    const rescan = async () => {
      try {
        if (writable) await desktopApi.refreshFileIndex();
        if (!active) return;
        await refresh();
        if (selectedId) {
          const next = await desktopApi.getApplication(selectedId);
          if (active)
            setDetail((current) =>
              current?.id === selectedId
                ? { ...current, documents: next.documents }
                : current,
            );
        }
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
  }, [onError, refresh, selectedId, writable]);

  const select = async (id: string) => {
    if (id === selectedRef.current || !(await confirmLeave())) return;
    const request = ++selectionRequest.current;
    setBusy(true);
    try {
      const next = await desktopApi.getApplication(id);
      if (request !== selectionRequest.current) return;
      selectedRef.current = id;
      setSelectedId(id);
      setDetail(next);
    } catch (error) {
      if (request === selectionRequest.current) onError(error);
    } finally {
      if (request === selectionRequest.current) setBusy(false);
    }
  };

  const visibleColumns = columns.filter((column) => column.visible);
  const offsets = pinnedOffsets(visibleColumns);
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
      detailChanged(created);
    } catch (error) {
      onError(error);
    } finally {
      setBusy(false);
    }
  };

  const detailChanged = (next: ApplicationDetail | null) => {
    selectionRequest.current += 1;
    selectedRef.current = next?.id ?? null;
    setDetail(next);
    if (next) setSelectedId(next.id);
    else setSelectedId(null);
    void refresh();
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
                    detailChanged(null);
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
          {detail && (
            <button
              type="button"
              disabled={busy}
              onClick={() =>
                chooseBatch(
                  records.filter((r) => r.companyName === detail.companyName),
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
                    selectionRequest.current += 1;
                    selectedRef.current = null;
                    setSelectedId(null);
                    setDetail(null);
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

        <div className="table-scroll">
          {grouped.map(([groupName, items]) => (
            <div className="record-group" key={groupName || "all"}>
              {group && (
                <h3>
                  {groupName} <small>{items.length}</small>
                </h3>
              )}
              <table className="application-table">
                <thead>
                  <tr>
                    {visibleColumns.map((column) => (
                      <th
                        key={column.key}
                        onClick={() => toggleSort(column.key)}
                        style={{
                          minWidth: column.width,
                          width: column.width,
                          ...(column.pinned
                            ? {
                                position: "sticky",
                                left: offsets.get(column.key),
                                zIndex: 2,
                              }
                            : {}),
                        }}
                      >
                        {labels[column.key]}{" "}
                        {sort[0]?.key === column.key
                          ? sort[0].direction === "asc"
                            ? "↑"
                            : "↓"
                          : ""}
                      </th>
                    ))}
                    <th>批量选择</th>
                  </tr>
                </thead>
                <tbody>
                  {items.map((record) => (
                    <tr
                      className={selectedId === record.id ? "selected" : ""}
                      key={record.id}
                      onClick={() => void select(record.id)}
                    >
                      {visibleColumns.map((column) => (
                        <td
                          key={column.key}
                          style={{
                            maxWidth: column.width,
                            minWidth: column.width,
                            ...(column.pinned
                              ? {
                                  position: "sticky",
                                  left: offsets.get(column.key),
                                }
                              : {}),
                          }}
                        >
                          <Cell
                            column={column}
                            record={record}
                            onError={onError}
                          />
                        </td>
                      ))}
                      <td onClick={(event) => event.stopPropagation()}>
                        <input
                          type="checkbox"
                          aria-label={`选择投递 ${record.companyName} ${record.positionName}`}
                          checked={checked.some((r) => r.id === record.id)}
                          disabled={busy}
                          onChange={(event) => {
                            if (!event.target.checked)
                              setChecked(
                                checked.filter((r) => r.id !== record.id),
                              );
                            else if (checked.length < 200)
                              setChecked([
                                ...checked,
                                { id: record.id, revision: record.revision },
                              ]);
                            else
                              onError(
                                new Error(
                                  "每批最多 200 条，请先清除部分选择。",
                                ),
                              );
                          }}
                        />
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
              {!loading && !loadError && !items.length && (
                <div className="table-empty">没有符合当前条件的投递记录。</div>
              )}
            </div>
          ))}
        </div>
        <div className="pagination">
          <span>
            {loading || loadError
              ? "记录数量待确认"
              : `共 ${processed.length} 条`}
          </span>
          <select
            aria-label="每页条数"
            disabled={loading || !!loadError}
            onChange={(event) => {
              const value = Number(event.target.value);
              setPageSize(value);
              setPage(1);
            }}
            value={pageSize}
          >
            {[20, 50, 100, 200].map((size) => (
              <option key={size} value={size}>
                每页 {size} 条
              </option>
            ))}
          </select>
          <button
            disabled={!writable || loading || !!loadError || busy}
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

      {detail && (
        <DetailPanel
          key={detail.id}
          detail={detail}
          fields={fields}
          onChange={detailChanged}
          onError={onError}
          scope={scope}
          writable={writable && !busy}
        />
      )}

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
