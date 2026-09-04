import assert from "node:assert/strict";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import test from "node:test";
import {
  PORTABLE_SOURCE_FILES,
  assemblePortableDirectory,
  auditPortableDirectory,
  readReleaseMetadata,
  releaseRustFlags,
  scanBinaryForLocalPaths,
  scanPublicText,
  validateReleaseMetadata,
} from "./release-lib.mjs";

const repositoryRoot = resolve(import.meta.dirname, "../..");

test("current release metadata preserves the security and format contract", async () => {
  const metadata = await readReleaseMetadata(repositoryRoot);
  assert.equal(metadata.packageVersion, "0.1.0");
  assert.equal(metadata.schemaVersion, 12);
  assert.equal(metadata.mcpTools.length, 10);
});

test("metadata validation rejects a widened help capability", async () => {
  const metadata = await readReleaseMetadata(repositoryRoot);
  assert.throws(
    () =>
      validateReleaseMetadata({
        ...metadata,
        helpPermissions: [...metadata.helpPermissions, "dialog:allow-open"],
      }),
    /Expected values to be strictly deep-equal/,
  );
});

test("public text scanner rejects secrets and machine-specific user paths", () => {
  scanPublicText("safe", "OfferTrack stores relative paths only.");
  assert.throws(
    () =>
      scanPublicText("secret", "token ghp_abcdefghijklmnopqrstuvwxyz123456"),
    /GitHub token/,
  );
  assert.throws(
    () => scanPublicText("path", String.raw`C:\Users\developer\resume.pdf`),
    /Windows user path/,
  );
});

test("binary scanner detects UTF-8 and UTF-16 local build paths", () => {
  const local = String.raw`C:\Users\developer\OfferTrack`;
  scanBinaryForLocalPaths("safe", Buffer.from("MZ production"), [local]);
  assert.throws(
    () => scanBinaryForLocalPaths("utf8", Buffer.from(`MZ ${local}`), [local]),
    /local build path/,
  );
  assert.throws(
    () =>
      scanBinaryForLocalPaths(
        "utf16",
        Buffer.concat([Buffer.from("MZ"), Buffer.from(local, "utf16le")]),
        [local],
      ),
    /local build path/,
  );
});

test("release Rust flags preserve encoded flags and remap local roots", () => {
  const flags = releaseRustFlags(
    String.raw`D:\work tree\OfferTrack`,
    String.raw`C:\Users\developer`,
    "-Copt-level=3",
  ).split("\u001f");
  assert.deepEqual(flags, [
    "-Copt-level=3",
    String.raw`--remap-path-prefix=D:\work tree\OfferTrack=/offertrack`,
    String.raw`--remap-path-prefix=C:\Users\developer=/build-user`,
  ]);
});

test("portable assembly is allowlisted, self-verifying and never overwrites", async () => {
  const temporary = await mkdtemp(join(tmpdir(), "offertrack-release-test-"));
  const fakeRepository = join(temporary, "repository");
  const output = join(temporary, "output");
  await mkdir(fakeRepository);
  await mkdir(output);
  try {
    for (const [source, name, kind] of PORTABLE_SOURCE_FILES) {
      const target = join(fakeRepository, source);
      await mkdir(dirname(target), { recursive: true });
      let value;
      if (kind === "binary") value = Buffer.from(`MZsynthetic-${name}`);
      else if (kind === "text") value = `${name}\n${"safe ".repeat(30)}`;
      else {
        value = Buffer.alloc(24);
        Buffer.from("89504e470d0a1a0a", "hex").copy(value);
        value.writeUInt32BE(1, 16);
        value.writeUInt32BE(1, 20);
      }
      await writeFile(target, value, { flag: "wx" });
    }
    await writeFile(join(fakeRepository, "private.sqlite"), "must not ship", {
      flag: "wx",
    });
    const metadata = {
      packageVersion: "0.1.0",
      schemaVersion: 12,
      warehouseFormatVersion: 1,
    };
    const result = await assemblePortableDirectory({
      repositoryRoot: fakeRepository,
      outputParent: output,
      metadata,
    });
    await auditPortableDirectory(result.candidate, metadata);
    const manifest = JSON.parse(
      await readFile(join(result.candidate, "RELEASE-MANIFEST.json"), "utf8"),
    );
    assert.equal(
      manifest.files.some((file) => file.path.endsWith(".sqlite")),
      false,
    );
    assert(
      manifest.files.some((file) => file.path === "docs/user-guide/README.md"),
    );
    assert(
      manifest.files.some(
        (file) => file.path === "docs/assets/offertrack-slogan.png",
      ),
    );
    await assert.rejects(
      assemblePortableDirectory({
        repositoryRoot: fakeRepository,
        outputParent: output,
        metadata,
      }),
      /EEXIST/,
    );
  } finally {
    await rm(temporary, { recursive: true, force: true });
  }
});

test("portable output is rejected inside the repository", async () => {
  const metadata = {
    packageVersion: "0.1.0",
    schemaVersion: 12,
    warehouseFormatVersion: 1,
  };
  await assert.rejects(
    assemblePortableDirectory({
      repositoryRoot,
      outputParent: repositoryRoot,
      metadata,
    }),
    /outside the repository/,
  );
});

test("portable ZIP helper preserves nested allowlisted files and rejects links", async () => {
  const source = await readFile(
    join(repositoryRoot, "scripts/release/create-portable-zip.ps1"),
    "utf8",
  );
  assert(source.includes("-Recurse"));
  assert(source.includes("Substring($sourcePath.Length)"));
  assert(!source.includes("GetRelativePath"));
  assert(source.includes("ReparsePoint"));
  assert(!source.includes("only regular root files"));
});
