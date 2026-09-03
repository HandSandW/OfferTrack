import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFile, realpath } from "node:fs/promises";
import { join, resolve } from "node:path";
import {
  auditRepository,
  releaseRustFlags,
  scanBinaryForLocalPaths,
} from "./release-lib.mjs";
import { assertThirdPartyNoticesCurrent } from "./third-party-licenses.mjs";

assert.equal(
  process.platform,
  "win32",
  "OfferTrack release builds currently support Windows only",
);
assert(
  !process.env.RUSTFLAGS,
  "Use CARGO_ENCODED_RUSTFLAGS for inherited release flags; plain RUSTFLAGS is ambiguous",
);
const repositoryRoot = await realpath(resolve(import.meta.dirname, "../.."));
await assertThirdPartyNoticesCurrent(repositoryRoot);
await auditRepository(repositoryRoot);
const userProfile = process.env.USERPROFILE;
const environment = {
  ...process.env,
  CARGO_ENCODED_RUSTFLAGS: releaseRustFlags(
    repositoryRoot,
    userProfile,
    process.env.CARGO_ENCODED_RUSTFLAGS,
  ),
};
const pnpmEntrypoint = process.env.npm_execpath;
assert(
  pnpmEntrypoint,
  "Run this script through `pnpm release:build` so pnpm can be invoked without a command shell",
);
const result = spawnSync(
  process.execPath,
  [pnpmEntrypoint, "tauri", "build", "--no-bundle"],
  {
    cwd: repositoryRoot,
    env: environment,
    stdio: "inherit",
    windowsHide: true,
    shell: false,
  },
);
assert.equal(result.error, undefined, "Unable to start the release build");
assert.equal(result.status, 0, "Release build failed");
for (const name of ["offertrack.exe", "offertrack-cli.exe"]) {
  const bytes = await readFile(
    join(repositoryRoot, "src-tauri/target/release", name),
  );
  scanBinaryForLocalPaths(name, bytes, [repositoryRoot, userProfile]);
}
console.log("Path-clean Windows release binaries built successfully.");
