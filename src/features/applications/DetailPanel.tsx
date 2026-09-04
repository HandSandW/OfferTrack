import { UrlLink } from "../../shared/UrlLink";
import { useState } from "react";
import type { ApplicationDetail, FieldDefinition } from "../../contracts";
import { desktopApi } from "../../lib/tauri";
import { companyTypes } from "./tableModel";
import { CustomFieldInput } from "./CustomFieldInput";
import { WorkflowPanel } from "./WorkflowPanel";
import { FilesPanel } from "../documents/FilesPanel";
import { applicationDraft } from "./editorModel";
import { useDraftGuard, useDraftState } from "../../shared/draftGuard";
import { formatLocalDateTime } from "../../shared/dateTime";

export type DetailTab = "basic" | "workflow" | "files" | "history";

export function DetailPanel({
  detail,
  fields,
  onChange,
  onError,
  scope,
  writable,
  initialTab = "basic",
  onTabChange,
  operationError = "",
}: {
  detail: ApplicationDetail;
  fields: FieldDefinition[];
  onChange: (detail: ApplicationDetail | null) => void;
  onError: (error: unknown) => void;
  scope: "active" | "archived";
  writable: boolean;
  initialTab?: DetailTab;
  onTabChange?: (tab: DetailTab) => void;
  operationError?: string;
}) {
  const [form, setForm] = useState(detail);
  const [tab, setTab] = useState<DetailTab>(initialTab);
  const [busy, setBusy] = useState(false);
  const [operationLabel, setOperationLabel] = useState("");
  const [actionError, setActionError] = useState("");
  const { confirm, confirmLeave } = useDraftGuard();
  const [tagsText, setTagsText] = useState(
    detail.tags.map((tag) => tag.name).join(", "),
  );

  useDraftState(
    JSON.stringify(applicationDraft(form, tagsText)) !==
      JSON.stringify(applicationDraft(detail)),
    busy,
    "投递资料",
  );
  const accept = (next: ApplicationDetail) => {
    setForm(next);
    setTagsText(next.tags.map((tag) => tag.name).join(", "));
    onChange(next);
  };
  const leave = async (action: () => void) => {
    if (await confirmLeave()) {
      setForm(detail);
      setTagsText(detail.tags.map((tag) => tag.name).join(", "));
      action();
    }
  };
  const guardedAct = async (operation: () => Promise<ApplicationDetail>) => {
    if (await confirmLeave()) await act(operation);
  };
  const act = async (
    operation: () => Promise<ApplicationDetail>,
    label = "正在保存…",
  ) => {
    setBusy(true);
    setOperationLabel(label);
    setActionError("");
    try {
      const next = await operation();
      accept(next);
    } catch (error) {
      setActionError(
        error instanceof Error ? error.message : "操作失败，请重试。",
      );
      onError(error);
    } finally {
      setBusy(false);
      setOperationLabel("");
    }
  };
  const save = () =>
    act(() => desktopApi.updateApplication(applicationDraft(form, tagsText)));
  const duplicateFull = async () => {
    if (!(await confirmLeave())) return;
    setBusy(true);
    setActionError("");
    setOperationLabel("正在统计附件大小…");
    try {
      const preview = await desktopApi.previewApplicationDuplicate(
        form.id,
        "fullRecord",
      );
      setOperationLabel("等待复制确认…");
      if (
        await confirm({
          title: "复制完整记录",
          message: `将复制已保存的资料、流程与 ${Math.ceil(preview.fileSizeBytes / 1024)} KB 文件，创建独立副本。新记录重置投递进度、日期、历史和面试结果。继续吗？`,
          confirmLabel: "复制",
        })
      ) {
        await act(
          () => desktopApi.duplicateApplication(form.id, "fullRecord"),
          "正在复制并校验附件、提交独立副本…请等待完成，不要移动附件或强制退出。",
        );
      }
    } catch (error) {
      setActionError(
        error instanceof Error ? error.message : "复制预览失败，请重试。",
      );
      onError(error);
    } finally {
      setBusy(false);
      setOperationLabel("");
    }
  };
  const remove = async () => {
    if (!(await confirmLeave())) return;
    if (
      !(await confirm({
        title: "删除投递",
        message: "将此投递及其文件移入 OfferTrack 回收站？可在回收站中恢复。",
        confirmLabel: "移入回收站",
        destructive: true,
      }))
    )
      return;
    setBusy(true);
    try {
      await desktopApi.moveApplicationToTrash(form.id);
      onChange(null);
    } catch (error) {
      onError(error);
    } finally {
      setBusy(false);
    }
  };

  return (
    <aside className="detail-panel">
      <header>
        <div>
          <span className="eyebrow">投递详情 · {form.shortId}</span>
          <h2>{form.companyName}</h2>
          <p>{form.positionName}</p>
        </div>
        <button
          aria-label="关闭详情"
          onClick={() => void leave(() => onChange(null))}
          type="button"
        >
          ×
        </button>
      </header>
      {busy && operationLabel && (
        <div role="status">
          <progress aria-label="当前操作进度" /> {operationLabel}
        </div>
      )}
      {actionError && <p role="alert">{actionError}</p>}
      {operationError && <p role="alert">{operationError}</p>}
      <nav className="detail-tabs">
        {(
          [
            ["basic", "资料"],
            ["workflow", "流程"],
            ["files", "文件"],
            ["history", "历史"],
          ] as const
        ).map(([key, label]) => (
          <button
            className={tab === key ? "active" : ""}
            key={key}
            onClick={() => {
              if (tab !== key)
                void leave(() => {
                  setTab(key);
                  onTabChange?.(key);
                });
            }}
            type="button"
          >
            {label}
          </button>
        ))}
      </nav>
      <div className="detail-body">
        {tab === "basic" && (
          <>
            <div className="form-grid">
              <label>
                公司名称
                <input
                  disabled={!writable || busy}
                  value={form.companyName}
                  onChange={(event) =>
                    setForm({ ...form, companyName: event.target.value })
                  }
                />
              </label>
              <label>
                岗位名称
                <input
                  disabled={!writable || busy}
                  value={form.positionName}
                  onChange={(event) =>
                    setForm({ ...form, positionName: event.target.value })
                  }
                />
              </label>
              <label>
                企业性质
                <select
                  disabled={!writable || busy}
                  value={form.companyType}
                  onChange={(event) =>
                    setForm({ ...form, companyType: event.target.value })
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
                投递日期
                <input
                  disabled={!writable || busy}
                  type="date"
                  value={form.applicationDate ?? ""}
                  onChange={(event) =>
                    setForm({
                      ...form,
                      applicationDate: event.target.value || null,
                    })
                  }
                />
              </label>
              <label>
                行业
                <input
                  disabled={!writable || busy}
                  value={form.industry}
                  onChange={(event) =>
                    setForm({ ...form, industry: event.target.value })
                  }
                />
              </label>
              <label>
                岗位类别
                <input
                  disabled={!writable || busy}
                  value={form.positionCategory}
                  onChange={(event) =>
                    setForm({ ...form, positionCategory: event.target.value })
                  }
                />
              </label>
              <label>
                工作地点
                <input
                  disabled={!writable || busy}
                  value={form.workLocation}
                  onChange={(event) =>
                    setForm({ ...form, workLocation: event.target.value })
                  }
                />
              </label>
              <label>
                标签（逗号分隔）
                <input
                  disabled={!writable || busy}
                  value={tagsText}
                  onChange={(event) => setTagsText(event.target.value)}
                />
              </label>
            </div>
            {(
              [
                ["applicationUrl", "投递链接"],
                ["announcementUrl", "公告链接"],
                ["companyUrl", "公司网址"],
                ["positionUrl", "岗位网址"],
              ] as const
            ).map(([key, label]) => (
              <label className="wide-field" key={key}>
                {label}
                <div className="input-action">
                  <input
                    disabled={!writable || busy}
                    value={form[key] ?? ""}
                    onChange={(event) =>
                      setForm({ ...form, [key]: event.target.value || null })
                    }
                  />
                  <UrlLink
                    value={form[key] ?? ""}
                    onError={onError}
                    alwaysOpen
                  />
                </div>
              </label>
            ))}
            <label className="wide-field">
              岗位介绍
              <textarea
                disabled={!writable || busy}
                rows={7}
                value={form.positionDescription}
                onChange={(event) =>
                  setForm({ ...form, positionDescription: event.target.value })
                }
              />
            </label>
            <label className="wide-field">
              备注
              <textarea
                disabled={!writable || busy}
                rows={5}
                value={form.notes}
                onChange={(event) =>
                  setForm({ ...form, notes: event.target.value })
                }
              />
            </label>
            {fields.map((field) => (
              <label className="wide-field" key={field.id}>
                {field.displayName}
                <CustomFieldInput
                  field={field}
                  disabled={!writable || busy}
                  value={form.customFields[field.id]}
                  onChange={(value) => {
                    const customFields = { ...form.customFields };
                    if (value === undefined) delete customFields[field.id];
                    else customFields[field.id] = value;
                    setForm({ ...form, customFields });
                  }}
                />
              </label>
            ))}
            <button
              className="primary full-button"
              disabled={!writable || busy}
              onClick={() => void save()}
              type="button"
            >
              保存修改
            </button>
          </>
        )}
        {tab === "workflow" && (
          <WorkflowPanel
            detail={form}
            writable={writable && !busy}
            onChange={accept}
            onError={onError}
          />
        )}
        {tab === "files" && (
          <FilesPanel
            detail={detail}
            writable={writable && !busy}
            onChange={accept}
            onError={onError}
          />
        )}
        {tab === "history" && (
          <div className="history-list">
            {form.history.map((event) => (
              <article key={event.id}>
                <time>{formatLocalDateTime(event.occurredAtUtc)}</time>
                <strong>{event.stageNameSnapshot}</strong>
                <span>
                  {event.previousStateNameSnapshot
                    ? `${event.previousStateNameSnapshot} → `
                    : ""}
                  {event.nextStateNameSnapshot}
                  {event.notes ? ` · ${event.notes}` : ""}
                </span>
              </article>
            ))}
          </div>
        )}
      </div>
      <footer className="detail-footer">
        {scope === "active" ? (
          <button
            disabled={!writable || busy}
            onClick={() =>
              void guardedAct(() =>
                desktopApi.setApplicationArchived(form.id, true),
              )
            }
            type="button"
          >
            归档
          </button>
        ) : (
          <button
            disabled={!writable || busy}
            onClick={() =>
              void guardedAct(() =>
                desktopApi.setApplicationArchived(form.id, false),
              )
            }
            type="button"
          >
            恢复到投递记录
          </button>
        )}
        <button
          disabled={!writable || busy}
          onClick={() => {
            void (async () => {
              if (!(await confirmLeave())) return;
              if (
                await confirm({
                  title: "复制公司信息",
                  message: "复制已保存的公司基本信息并新建独立投递？",
                  confirmLabel: "复制",
                })
              )
                await act(
                  () => desktopApi.duplicateApplication(form.id, "companyInfo"),
                  "正在创建公司信息副本…",
                );
            })();
          }}
          type="button"
        >
          复制公司信息
        </button>
        <button
          disabled={!writable || busy}
          onClick={() => void duplicateFull()}
          type="button"
        >
          复制完整记录
        </button>
        <button
          className="danger"
          disabled={!writable || busy}
          onClick={() => void remove()}
          type="button"
        >
          删除
        </button>
      </footer>
    </aside>
  );
}
