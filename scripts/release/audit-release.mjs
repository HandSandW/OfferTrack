import { resolve } from "node:path";
import {
  auditPortableDirectory,
  auditRepository,
  readReleaseMetadata,
} from "./release-lib.mjs";
import { assertThirdPartyNoticesCurrent } from "./third-party-licenses.mjs";

const repositoryRoot = resolve(import.meta.dirname, "../..");
await assertThirdPartyNoticesCurrent(repositoryRoot);
const metadata = await auditRepository(repositoryRoot);
const candidateIndex = process.argv.indexOf("--candidate");
if (candidateIndex >= 0) {
  const candidate = process.argv[candidateIndex + 1];
  if (!candidate) throw new Error("--candidate requires a directory");
  await auditPortableDirectory(
    resolve(candidate),
    await readReleaseMetadata(repositoryRoot),
  );
}
console.log(
  `Release safety audit passed: OfferTrack ${metadata.packageVersion}, schema ${metadata.schemaVersion}, fixed recycle-bin boundaries, restricted help window, HTTP(S)-only links and ${metadata.mcpTools.length} read-only Agent tools + controlled write.`,
);
