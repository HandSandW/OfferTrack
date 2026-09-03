import type { BatchTarget } from "../batch/contracts";

export interface ExportColumn {
  key: string;
  label: string;
  fieldType: string;
}
export interface ExportCatalog {
  version: 1;
  total: number;
  columns: ExportColumn[];
}
export type ExportScope =
  | { kind: "all" }
  | {
      kind: "records";
      partition: "active" | "archived";
      targets: BatchTarget[];
    };
export interface ExportRequest {
  version: 1;
  scope: ExportScope;
  columns: string[];
  format: "xlsx" | "csv";
}
export interface ExportCreated {
  path: string;
  mappingPath: string;
  rowCount: number;
}
