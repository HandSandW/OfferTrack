export interface Task {
  id: string;
  revision: number;
  applicationId: string | null;
  applicationLabel: string | null;
  applicationArchived: boolean;
  title: string;
  notes: string;
  priority: "low" | "normal" | "high";
  dueAtUtc: string | null;
  remindAtUtc: string | null;
  completedAtUtc: string | null;
  createdAtUtc: string;
  updatedAtUtc: string;
}
export type SaveTask = Pick<
  Task,
  "applicationId" | "title" | "notes" | "priority" | "dueAtUtc" | "remindAtUtc"
> & { id: string | null; revision: number | null };
export interface ReminderRule {
  key: string;
  label: string;
  enabled: boolean;
  value: number;
  revision: number;
}
export interface Bucket {
  label: string;
  ids: string[];
}
export interface OverviewRecord {
  id: string;
  label: string;
  createdAtUtc: string;
  applicationDate: string | null;
  stageKey: string;
  stageName: string;
  stateKind: string;
  terminal: boolean;
  statusUpdatedAtUtc: string;
  updatedAtUtc: string;
  companyType: string;
  industry: string;
  workLocation: string;
  resumeCount: number;
}
export interface Interview {
  id: string;
  applicationId: string;
  label: string;
  scheduledAtUtc: string;
  updatedAtUtc: string;
}
export interface Reminder {
  key: string;
  fingerprint: string;
  ruleKey: string;
  sourceKind: "application" | "task" | "interview" | "event";
  sourceId: string;
  applicationId: string | null;
  label: string;
  reason: string;
  severity: "normal" | "urgent" | "overdue";
}
export interface Overview {
  generatedAtUtc: string;
  records: OverviewRecord[];
  metrics: Bucket[];
  stages: Bucket[];
  industries: Bucket[];
  locations: Bucket[];
  companyTypes: Bucket[];
  funnel: Bucket[];
  trend: { date: string; createdIds: string[]; appliedIds: string[] }[];
  tasks: Task[];
  interviews: Interview[];
  reminders: Reminder[];
  events: RecruitmentEvent[];
  schedule: ScheduleEntry[];
  dueMetrics: { label: string; keys: string[] }[];
}
export interface Drilldown {
  label: string;
  ids: string[];
}

export interface RecruitmentEvent {
  id: string;
  revision: number;
  applicationId: string | null;
  applicationLabel: string | null;
  applicationArchived: boolean;
  applicationTerminal: boolean;
  eventType: string;
  title: string;
  notes: string;
  startsAtUtc: string | null;
  deadlineAtUtc: string | null;
  completedAtUtc: string | null;
  finished: boolean;
  interviewRoundId: string | null;
  interviewRoundName: string | null;
  location: string;
  meetingUrl: string | null;
  result: string;
  createdAtUtc: string;
  updatedAtUtc: string;
  sourceVersion: string;
}
export type SaveEvent = Pick<
  RecruitmentEvent,
  | "eventType"
  | "title"
  | "notes"
  | "startsAtUtc"
  | "deadlineAtUtc"
  | "interviewRoundId"
  | "location"
  | "meetingUrl"
  | "result"
> & { id: string | null; revision: number | null; applicationId: string };
export interface ScheduleEntry {
  key: string;
  sourceKind: "task" | "event" | "interview";
  sourceId: string;
  applicationId: string | null;
  label: string;
  atUtc: string | null;
  startsAtUtc: string | null;
  finished: boolean;
  highPriority: boolean;
}
export interface ScheduleScope {
  label: string;
  keys: string[];
}
