import { resolve } from "node:path";
import { buildPortablePackage } from "./release-lib.mjs";
import { assertThirdPartyNoticesCurrent } from "./third-party-licenses.mjs";

const outputParent = process.argv[2];
if (!outputParent) {
  throw new Error(
    "Usage: node scripts/release/package-portable.mjs <existing-output-directory-outside-repository>",
  );
}
const repositoryRoot = resolve(import.meta.dirname, "../..");
await assertThirdPartyNoticesCurrent(repositoryRoot);
const result = await buildPortablePackage(
  repositoryRoot,
  resolve(outputParent),
);
console.log(
  JSON.stringify(
    {
      version: result.metadata.packageVersion,
      directory: result.candidate,
      archive: result.archive,
      checksumFile: result.externalChecksum,
      sha256: result.archiveHash,
    },
    null,
    2,
  ),
);
