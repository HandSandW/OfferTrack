import { useContext, type ReactNode } from "react";
import { OverviewContext, useOverviewState } from "./useOverview";

export function OverviewProvider({
  enabled,
  page,
  onError,
  children,
}: {
  enabled: boolean;
  page: string;
  onError: (error: unknown) => void;
  children: ReactNode;
}) {
  const value = useOverviewState(onError, enabled, page);
  return (
    <OverviewContext.Provider value={value}>
      {children}
    </OverviewContext.Provider>
  );
}

export function ReminderBanner({ onOpen }: { onOpen: () => void }) {
  const value = useContext(OverviewContext);
  if (!value) return null;
  const { data, error, loading } = value;
  return (
    <div className="notice info" aria-label="应用内提醒">
      <button onClick={onOpen}>
        查看重要提醒{data ? `（${data.reminders.length}）` : ""}
      </button>
      <span>
        {error
          ? "提醒暂未更新，保留上次结果。"
          : loading
            ? "正在更新提醒…"
            : "仅在应用运行时检查，不发系统通知。"}
      </span>
    </div>
  );
}
