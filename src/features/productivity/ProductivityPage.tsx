import { useState } from "react";
import { useDraftGuard } from "../../shared/draftGuard";
import type { ScheduleScope } from "./contracts";
import { TasksPage } from "./TasksPage";
import { EventsPage } from "./EventsPage";
import { SchedulePage } from "./SchedulePage";

export function ProductivityPage({
  writable,
  onError,
  onOpenApplication,
  initialTaskId,
  initialEventId,
  initialSchedule,
}: {
  writable: boolean;
  onError: (error: unknown) => void;
  onOpenApplication: (id: string, archived: boolean) => void;
  initialTaskId?: string | undefined;
  initialEventId?: string | undefined;
  initialSchedule?: ScheduleScope | undefined;
}) {
  const [tab, setTab] = useState(
    initialEventId ? "events" : initialSchedule ? "schedule" : "tasks",
  );
  const [taskId, setTaskId] = useState(initialTaskId);
  const [eventId, setEventId] = useState(initialEventId);
  const { confirmLeave } = useDraftGuard();
  const switchTab = async (next: string) => {
    if (tab !== next && (await confirmLeave())) {
      setTab(next);
      setTaskId(undefined);
      setEventId(undefined);
    }
  };
  return (
    <div>
      <nav className="section-actions" aria-label="待办日程视图">
        {[
          ["tasks", "待办列表"],
          ["events", "招聘事件"],
          ["schedule", "综合日程"],
        ].map(([key, label]) => (
          <button
            aria-pressed={tab === key}
            key={key}
            onClick={() => void switchTab(key!)}
          >
            {label}
          </button>
        ))}
      </nav>
      {tab === "tasks" ? (
        <TasksPage
          key={taskId ?? "all"}
          writable={writable}
          onError={onError}
          onOpenApplication={onOpenApplication}
          initialTaskId={taskId}
        />
      ) : tab === "events" ? (
        <EventsPage
          key={eventId ?? "all"}
          writable={writable}
          onError={onError}
          onOpenApplication={onOpenApplication}
          initialEventId={eventId}
        />
      ) : (
        <SchedulePage
          onError={onError}
          initialScope={initialSchedule}
          onOpen={(entry) => {
            if (entry.sourceKind === "task") {
              setTaskId(entry.sourceId);
              setTab("tasks");
            } else if (entry.sourceKind === "event") {
              setEventId(entry.sourceId);
              setTab("events");
            } else if (entry.applicationId)
              onOpenApplication(entry.applicationId, false);
          }}
        />
      )}
    </div>
  );
}
