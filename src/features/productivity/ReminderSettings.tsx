import { useState } from "react";
import { desktopApi } from "../../lib/tauri";
import { useDraftGuard, useDraftState } from "../../shared/draftGuard";
import type { ReminderRule } from "./contracts";
import { errorText } from "./model";

export function ReminderSettings({
  writable,
  onError,
}: {
  writable: boolean;
  onError: (error: unknown) => void;
}) {
  const [rules, setRules] = useState<ReminderRule[] | null>(null);
  const [saved, setSaved] = useState<ReminderRule[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const { confirmLeave } = useDraftGuard();
  useDraftState(
    JSON.stringify(rules) !== JSON.stringify(saved),
    busy,
    "提醒规则设置",
  );
  const run = async (save: boolean) => {
    if (!save && !(await confirmLeave())) return;
    setBusy(true);
    setMessage("");
    try {
      const next = save
        ? await desktopApi.saveReminderRules(rules!)
        : await desktopApi.listReminderRules();
      setRules(next);
      setSaved(next);
      setMessage(save ? "提醒规则已保存，概览将按新阈值计算。" : "");
    } catch (error) {
      setMessage(errorText(error));
      onError(error);
    } finally {
      setBusy(false);
    }
  };
  return (
    <article className="panel-page" id="reminder-settings">
      <h2>提醒规则</h2>
      <p className="muted">
        每条规则可关闭；修改只影响当前仓库后续计算，不改历史。天数按连续 24
        小时计算，逾期规则为超过时间立即提示。用户设定的提醒时间与高优先级待办不受这些规则开关影响。
      </p>
      <button disabled={busy} onClick={() => void run(false)}>
        读取提醒规则
      </button>
      {rules && (
        <form
          onSubmit={(e) => {
            e.preventDefault();
            void run(true);
          }}
        >
          <fieldset disabled={busy || !writable} className="task-form">
            {rules.map((rule, index) => (
              <div key={rule.key} className="rule-row">
                <label>
                  <input
                    type="checkbox"
                    checked={rule.enabled}
                    onChange={(e) =>
                      setRules(
                        rules.map((r, i) =>
                          i === index ? { ...r, enabled: e.target.checked } : r,
                        ),
                      )
                    }
                  />
                  {rule.label}
                </label>
                {rule.key !== "overdue" && (
                  <label>
                    {rule.key === "due_urgent" ? "小时" : "天"}
                    <input
                      aria-label={`${rule.label}阈值`}
                      type="number"
                      required
                      min={1}
                      max={8760}
                      step={1}
                      value={rule.value}
                      onChange={(e) =>
                        setRules(
                          rules.map((r, i) =>
                            i === index
                              ? { ...r, value: Number(e.target.value) }
                              : r,
                          ),
                        )
                      }
                    />
                  </label>
                )}
              </div>
            ))}
          </fieldset>
          <button
            disabled={
              busy ||
              !writable ||
              JSON.stringify(rules) === JSON.stringify(saved)
            }
          >
            保存提醒规则
          </button>
        </form>
      )}
      {message && <p role="status">{message}</p>}
    </article>
  );
}
