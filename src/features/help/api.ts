import { invoke } from "@tauri-apps/api/core";

export interface HelpLocation {
  topic: string;
  revision: number;
}
export interface HelpDiagnostics {
  version: number;
  application: string;
  applicationVersion: string;
  platform: string;
  architecture: string;
  build: string;
  sqliteVersion: string;
  supportedSchema: number;
  warehouseAccess: "closed" | "write" | "readOnly" | "busy";
  persistentApplicationLog: boolean;
}

export const helpApi = {
  open: (topic = "manual") => invoke<void>("open_help", { topic }),
  location: () => invoke<HelpLocation>("get_help_location"),
  diagnostics: () => invoke<HelpDiagnostics>("get_help_diagnostics"),
  openLogs: () => invoke<boolean>("open_help_logs"),
};
