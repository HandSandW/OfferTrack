export interface BatchTarget {
  id: string;
  revision: number;
}
export type BatchAction =
  | { kind: "archive"; archived: boolean }
  | { kind: "addTags"; tags: string[] }
  | { kind: "stage"; stageKey: string; stateKey: string }
  | { kind: "appendTemplate"; templateId: string; revision: number };
export interface BatchRequest {
  version: 1;
  targets: BatchTarget[];
  action: BatchAction;
}
export interface BatchPreview {
  version: 1;
  fingerprint: string;
  changedCount: number;
  items: {
    id: string;
    companyName: string;
    positionName: string;
    changes: string[];
  }[];
}
export interface BatchApplied {
  changedCount: number;
  backupId: string | null;
}
