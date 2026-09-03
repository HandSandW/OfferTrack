// Local, ignored documentation is the single source; Vite embeds it in the EXE.
// Do not replace missing documentation with a tracked copy or a truncated guide.
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
