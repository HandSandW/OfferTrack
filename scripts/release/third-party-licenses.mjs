import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { readFile, readdir, writeFile } from "node:fs/promises";
import { basename, dirname, join, resolve } from "node:path";
import { scanPublicText } from "./release-lib.mjs";

const POLICY_FILE = "scripts/release/third-party-license-policy.json";
const NOTICE_FILE = "THIRD_PARTY_NOTICES.md";
const LICENSE_FILE_PATTERN =
  /^(?:(?:license|copying|notice)(?:$|[._-])|copyright$)/i;
const MAX_LICENSE_BYTES = 1024 * 1024;

function runJson(command, args, options, label) {
  const result = spawnSync(command, args, {
    ...options,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    shell: false,
    windowsHide: true,
  });
  assert.equal(result.error, undefined, `Unable to start ${label}`);
  assert.equal(result.status, 0, result.stderr || `${label} failed`);
  try {
    return JSON.parse(result.stdout);
  } catch {
    throw new Error(`${label} did not return valid JSON`);
  }
}

function packageKey(pkg) {
  return `${pkg.name}@${pkg.version}`;
}

function comparePackages(left, right) {
  return (
    left.ecosystem.localeCompare(right.ecosystem) ||
    left.name.localeCompare(right.name) ||
    left.version.localeCompare(right.version)
  );
}

function cleanUrl(value, fallback) {
  if (typeof value !== "string" || !/^https?:\/\//i.test(value)) {
    return fallback;
  }
  return value.replace(/\.git$/i, "");
}

export function resolvedCargoPackageIds(metadata) {
  assert(metadata?.resolve?.root, "Cargo metadata has no root package");
  const nodes = new Map(metadata.resolve.nodes.map((node) => [node.id, node]));
  const seen = new Set();
  const pending = [metadata.resolve.root];
  while (pending.length > 0) {
    const id = pending.shift();
    if (seen.has(id)) continue;
    seen.add(id);
    const node = nodes.get(id);
    assert(node, `Cargo resolve node is missing for ${id}`);
    for (const dependency of node.deps) {
      if (dependency.dep_kinds.some((item) => item.kind !== "dev")) {
        pending.push(dependency.pkg);
      }
    }
  }
  seen.delete(metadata.resolve.root);
  return seen;
}

export function flattenPnpmLicenseReport(report) {
  assert(report && typeof report === "object", "Invalid pnpm license report");
  const packages = [];
  for (const entries of Object.values(report)) {
    assert(Array.isArray(entries), "Invalid pnpm license group");
    for (const entry of entries) {
      assert.equal(
        entry.versions?.length,
        1,
        `Ambiguous version for ${entry.name}`,
      );
      assert.equal(entry.paths?.length, 1, `Ambiguous path for ${entry.name}`);
      packages.push({
        ecosystem: "pnpm",
        name: entry.name,
        version: entry.versions[0],
        license: entry.license,
        sourceUrl: cleanUrl(
          entry.homepage,
          `https://www.npmjs.com/package/${encodeURIComponent(entry.name)}`,
        ),
        packageDirectory: entry.paths[0],
      });
    }
  }
  return packages.sort(comparePackages);
}

async function collectPackages(repositoryRoot) {
  const cargoMetadata = runJson(
    "cargo",
    [
      "metadata",
      "--manifest-path",
      "src-tauri/Cargo.toml",
      "--format-version",
      "1",
      "--locked",
      "--filter-platform",
      "x86_64-pc-windows-msvc",
    ],
    { cwd: repositoryRoot },
    "cargo metadata",
  );
  const cargoIds = resolvedCargoPackageIds(cargoMetadata);
  const cargoPackages = cargoMetadata.packages
    .filter((pkg) => cargoIds.has(pkg.id))
    .map((pkg) => ({
      ecosystem: "Cargo",
      name: pkg.name,
      version: pkg.version,
      license: pkg.license,
      sourceUrl: cleanUrl(
        pkg.repository,
        `https://crates.io/crates/${encodeURIComponent(pkg.name)}`,
      ),
      packageDirectory: dirname(pkg.manifest_path),
      explicitLicenseFile: pkg.license_file,
    }));

  const pnpmEntrypoint = process.env.npm_execpath;
  assert(
    pnpmEntrypoint,
    "Run through pnpm so production JavaScript licenses can be enumerated safely",
  );
  const pnpmReport = runJson(
    process.execPath,
    [pnpmEntrypoint, "licenses", "list", "--prod", "--json"],
    { cwd: repositoryRoot },
    "pnpm licenses list",
  );
  const packages = [...cargoPackages, ...flattenPnpmLicenseReport(pnpmReport)];
  for (const pkg of packages) {
    assert(
      pkg.name && pkg.version,
      "Dependency has incomplete identity metadata",
    );
    assert(pkg.license, `${packageKey(pkg)} has no declared license`);
  }
  return packages.sort(comparePackages);
}

async function readLicenseFiles(pkg) {
  const entries = await readdir(pkg.packageDirectory, { withFileTypes: true });
  const names = new Set(
    entries
      .filter(
        (entry) => entry.isFile() && LICENSE_FILE_PATTERN.test(entry.name),
      )
      .map((entry) => entry.name),
  );
  if (pkg.explicitLicenseFile) {
    const explicit = resolve(pkg.packageDirectory, pkg.explicitLicenseFile);
    assert.equal(
      dirname(explicit),
      resolve(pkg.packageDirectory),
      `${packageKey(pkg)} license-file must stay at package root`,
    );
    names.add(basename(explicit));
  }
  const documents = [];
  for (const name of [...names].sort()) {
    const path = join(pkg.packageDirectory, name);
    const bytes = await readFile(path);
    assert(bytes.length > 0, `${packageKey(pkg)} ${name} is empty`);
    assert(
      bytes.length <= MAX_LICENSE_BYTES,
      `${packageKey(pkg)} ${name} is unexpectedly large`,
    );
    assert(!bytes.includes(0), `${packageKey(pkg)} ${name} is not plain text`);
    const text = bytes
      .toString("utf8")
      .replaceAll("\r\n", "\n")
      .split("\n")
      .map((line) => line.trimEnd())
      .join("\n")
      .trim();
    documents.push({ name, text });
  }
  return documents;
}

function escapeTable(value) {
  return String(value).replaceAll("|", "\\|").replaceAll("\n", " ");
}

function documentHash(text) {
  return createHash("sha256").update(text, "utf8").digest("hex");
}

export function validateMissingLicensePolicy(missingPackages, policy) {
  assert.equal(
    policy.formatVersion,
    1,
    "Review third-party license policy version",
  );
  const expected = [...missingPackages].sort();
  const configured = Object.keys(policy.cargoMissingLicenseFiles ?? {}).sort();
  assert.deepEqual(
    configured,
    expected,
    "Missing-license override set changed; review every added or removed package",
  );
}

async function sqliteComponent(packages) {
  const pkg = packages.find(
    (item) => item.ecosystem === "Cargo" && item.name === "libsqlite3-sys",
  );
  assert(pkg, "Bundled SQLite carrier libsqlite3-sys is missing");
  const bindings = await readFile(
    join(pkg.packageDirectory, "sqlite3/bindgen_bundled_version.rs"),
    "utf8",
  );
  const match = bindings.match(/SQLITE_VERSION[^=]*=\s*c"([^"]+)"/);
  assert(match?.[1], "Unable to determine bundled SQLite version");
  return {
    ecosystem: "embedded",
    name: "SQLite",
    version: match[1],
    license: "Public Domain",
    sourceUrl: "https://www.sqlite.org/copyright.html",
    documentRefs: [],
    policyNote:
      "Bundled by libsqlite3-sys; SQLite is dedicated to the public domain.",
  };
}

export async function generateThirdPartyNotices(repositoryRoot) {
  const root = resolve(repositoryRoot);
  const [packages, policy] = await Promise.all([
    collectPackages(root),
    readFile(join(root, POLICY_FILE), "utf8").then(JSON.parse),
  ]);
  const documentsByPackage = new Map();
  for (const pkg of packages) {
    documentsByPackage.set(packageKey(pkg), await readLicenseFiles(pkg));
  }

  const missing = packages
    .filter(
      (pkg) =>
        pkg.ecosystem === "Cargo" &&
        documentsByPackage.get(packageKey(pkg)).length === 0,
    )
    .map(packageKey);
  validateMissingLicensePolicy(missing, policy);

  for (const key of missing) {
    const pkg = packages.find((item) => packageKey(item) === key);
    const override = policy.cargoMissingLicenseFiles[key];
    assert(
      pkg.license.includes(override.selectedLicense),
      `${key} does not declare selected license ${override.selectedLicense}`,
    );
    const sourceDocuments = documentsByPackage.get(override.sourcePackage);
    assert(
      sourceDocuments,
      `${key} fallback package is not a production dependency`,
    );
    const selected = override.sourceFiles.map((name) => {
      const document = sourceDocuments.find((item) => item.name === name);
      assert(document, `${key} fallback license file ${name} is missing`);
      return {
        ...document,
        name: `fallback:${override.sourcePackage}/${name}`,
      };
    });
    documentsByPackage.set(key, selected);
    pkg.policyNote = override.reason;
  }

  const textPool = new Map();
  for (const pkg of packages) {
    const refs = [];
    for (const document of documentsByPackage.get(packageKey(pkg))) {
      const hash = documentHash(document.text);
      const ref = `L-${hash.slice(0, 12)}`;
      if (!textPool.has(hash)) {
        textPool.set(hash, { ref, text: document.text, sources: [] });
      }
      textPool
        .get(hash)
        .sources.push(`${pkg.ecosystem}:${packageKey(pkg)}/${document.name}`);
      refs.push(ref);
    }
    pkg.documentRefs = [...new Set(refs)].sort();
    assert(
      pkg.documentRefs.length > 0,
      `${packageKey(pkg)} has no license text`,
    );
  }
  packages.push(await sqliteComponent(packages));
  packages.sort(comparePackages);
  const reviewedFallbacks = packages.filter(
    (pkg) => policy.cargoMissingLicenseFiles[packageKey(pkg)],
  );

  const lines = [
    "# Third-party notices",
    "",
    "<!-- Generated by `pnpm license:generate`; do not edit by hand. -->",
    "",
    "OfferTrack itself is licensed under the MIT License. The following inventory covers the locked Windows x64 production and build dependency graph used by the portable release. Development-only dependencies are excluded.",
    "",
    `Inventory: ${packages.filter((item) => item.ecosystem === "Cargo").length} Cargo packages, ${packages.filter((item) => item.ecosystem === "pnpm").length} pnpm packages, and 1 embedded component. Multiple identical license texts are stored once and referenced by content hash.`,
    "",
    "A declared `OR` expression is satisfied using any included alternative; a declared `AND` expression requires all applicable texts. Exact overrides for crate archives that omit license files are reviewed in `scripts/release/third-party-license-policy.json`.",
    "",
    "## Dependency inventory",
    "",
    "| Ecosystem | Package | Version | Declared license | License text | Source |",
    "| --- | --- | --- | --- | --- | --- |",
  ];
  for (const pkg of packages) {
    const references = pkg.documentRefs.length
      ? pkg.documentRefs
          .map((ref) => `[${ref}](#${ref.toLowerCase()})`)
          .join(", ")
      : "Public domain";
    lines.push(
      `| ${escapeTable(pkg.ecosystem)} | ${escapeTable(pkg.name)} | ${escapeTable(pkg.version)} | ${escapeTable(pkg.license)} | ${references} | [upstream](${pkg.sourceUrl}) |`,
    );
  }
  lines.push(
    "",
    "## Reviewed missing-file fallbacks",
    "",
    "The following crate archives declare a license but contain no top-level license, copying, notice, or copyright file. Each exact locked package is mapped to a reviewed text from another locked dependency. Any change to this exact set fails generation until the policy is reviewed.",
    "",
    "| Package | Selected license | Text source | Review note |",
    "| --- | --- | --- | --- |",
  );
  for (const pkg of reviewedFallbacks) {
    const override = policy.cargoMissingLicenseFiles[packageKey(pkg)];
    lines.push(
      `| ${escapeTable(packageKey(pkg))} | ${escapeTable(override.selectedLicense)} | ${escapeTable(`${override.sourcePackage}/${override.sourceFiles.join(", ")}`)} | ${escapeTable(pkg.policyNote)} |`,
    );
  }
  lines.push("", "## License and notice texts", "");
  for (const [hash, document] of [...textPool.entries()].sort(([a], [b]) =>
    a.localeCompare(b),
  )) {
    const sources = [...new Set(document.sources)].sort();
    lines.push(
      `<details id="${document.ref.toLowerCase()}">`,
      `<summary>${document.ref} · SHA-256 ${hash} · ${sources.length} package reference(s)</summary>`,
      "",
      `Sources: ${sources.map((source) => `\`${source}\``).join(", ")}`,
      "",
      "````text",
      document.text,
      "````",
      "",
      "</details>",
      "",
    );
  }
  const output = `${lines.join("\n").trimEnd()}\n`;
  scanPublicText(NOTICE_FILE, output);
  return output;
}

export async function assertThirdPartyNoticesCurrent(repositoryRoot) {
  const root = resolve(repositoryRoot);
  const expected = await generateThirdPartyNotices(root);
  const actual = await readFile(join(root, NOTICE_FILE), "utf8");
  assert.equal(
    actual.replaceAll("\r\n", "\n"),
    expected,
    `${NOTICE_FILE} is stale; run pnpm license:generate`,
  );
  return expected;
}

export async function writeThirdPartyNotices(repositoryRoot) {
  const root = resolve(repositoryRoot);
  const output = await generateThirdPartyNotices(root);
  await writeFile(join(root, NOTICE_FILE), output, "utf8");
  return output;
}
