export type AgentSnapshot = {
  version: number;
  path: string;
  generatedAtUtc: string;
  applicationCount: number;
  rootInstructionsCreated: boolean;
  warnings: string[];
};

export type SnapshotReport = {
  version: number;
  warehouse_id: string;
  checked_at_utc: string;
  state: "current" | "stale" | "missing" | "error";
  snapshot: {
    relative_path: string;
    generated_at_utc: string;
    application_count: number;
    content_sha256: string;
  } | null;
  published: boolean;
  error: { code: string; message: string; retryable: boolean } | null;
  warnings: string[];
};

export type AgentPermission = {
  version: number;
  enabled: boolean;
  revision: number;
};
export type AgentAuditItem = {
  id: string;
  operation: string;
  occurred_at_utc: string;
  outcome: string;
};

export type AgentConnection = {
  version: number;
  cliAvailable: boolean;
  configuration: {
    mcpServers: { offertrack: { command: string; args: string[] } };
  };
  protocolVersions: string[];
};
