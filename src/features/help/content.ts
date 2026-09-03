// Public user documentation is the single source; Vite embeds it in the EXE.
// A clean checkout must include the complete guide, Agent and backup documents.
import guide from "../../../docs/user-guide/README.md?raw";
import agent from "../../../docs/agent-api.md?raw";
import backup from "../../../docs/backup-format.md?raw";
import license from "../../../LICENSE?raw";
import { parseChapters } from "./model";

export const chapters = [
  ...parseChapters(guide, "guide"),
  ...parseChapters(agent, "agent"),
  ...parseChapters(backup, "backup"),
];
export const mitLicense = license;
