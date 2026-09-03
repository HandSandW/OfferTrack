import { resolve } from "node:path";
import {
  assertThirdPartyNoticesCurrent,
  writeThirdPartyNotices,
} from "./third-party-licenses.mjs";

const repositoryRoot = resolve(import.meta.dirname, "../..");
if (process.argv.includes("--check")) {
  const output = await assertThirdPartyNoticesCurrent(repositoryRoot);
  console.log(
    `Third-party notices are current (${output.length} UTF-8 characters).`,
  );
} else {
  const output = await writeThirdPartyNotices(repositoryRoot);
  console.log(
    `Generated THIRD_PARTY_NOTICES.md (${output.length} UTF-8 characters).`,
  );
}
