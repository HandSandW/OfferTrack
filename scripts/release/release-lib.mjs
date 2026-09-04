import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  constants,
  copyFile,
  lstat,
  mkdir,
  open,
  readFile,
  readdir,
  realpath,
  stat,
  writeFile,
} from "node:fs/promises";
import { spawnSync } from "node:child_process";
import {
  basename,
  dirname,
  isAbsolute,
  join,
  relative,
  resolve,
} from "node:path";

export const PORTABLE_SOURCE_FILES = Object.freeze([
  ["src-tauri/target/release/offertrack.exe", "offertrack.exe", "binary"],
  [
    "src-tauri/target/release/offertrack-cli.exe",
    "offertrack-cli.exe",
    "binary",
  ],
  ["README.md", "README.md", "text"],
  ["LICENSE", "LICENSE", "text"],
  ["THIRD_PARTY_NOTICES.md", "THIRD_PARTY_NOTICES.md", "text"],
  ["CHANGELOG.md", "CHANGELOG.md", "text"],
  ["SECURITY.md", "SECURITY.md", "text"],
  ["docs/user-guide/README.md", "docs/user-guide/README.md", "text"],
  ["docs/agent-api.md", "docs/agent-api.md", "text"],
  ["docs/backup-format.md", "docs/backup-format.md", "text"],
  [
    "docs/assets/offertrack-slogan.png",
    "docs/assets/offertrack-slogan.png",
    "asset",
  ],
]);

const GENERATED_FILES = ["RELEASE-MANIFEST.json", "SHA256SUMS.txt"];
const REQUIRED_MCP_TOOLS = [
  "describe",
  "summary",
  "list_applications",
  "get_application",
  "list_tasks",
  "list_events",
  "list_documents",
  "resolve_document",
  "write_status",
  "snapshot_status",
];

function normalizedPath(value) {
  const result = resolve(value).replaceAll("\\", "/");
  return process.platform === "win32" ? result.toLowerCase() : result;
}

function isWithin(parent, child) {
  const value = relative(parent, child);
  return value === "" || (!value.startsWith("..") && !isAbsolute(value));
}

function capture(source, expression, label) {
  const match = source.match(expression);
  assert(match?.[1], `Unable to read ${label}`);
  return match[1];
}

function parseJson(text, label) {
  try {
    return JSON.parse(text);
  } catch {
    throw new Error(`${label} is not valid JSON`);
  }
}

export async function sha256(path) {
  const input = await open(path, "r");
  const digest = createHash("sha256");
  try {
    for await (const chunk of input.createReadStream()) digest.update(chunk);
  } finally {
    await input.close();
  }
  return digest.digest("hex");
}

export function scanPublicText(label, text) {
  const forbidden = [
    [/-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----/i, "private key"],
    [/(?:ghp|github_pat)_[A-Za-z0-9_]{20,}/i, "GitHub token"],
    [/AKIA[0-9A-Z]{16}/, "AWS access key"],
    [/xox[baprs]-[A-Za-z0-9-]{10,}/i, "Slack token"],
    [/[A-Za-z]:[\\/]Users[\\/][^\\/\s]+/i, "Windows user path"],
    [/(?:^|[\s("'`])\/(?:Users|home)\/[^/\s]+/m, "Unix user path"],
  ];
  for (const [pattern, name] of forbidden) {
    assert(!pattern.test(text), `${label} contains a ${name}`);
  }
}

export function scanBinaryForLocalPaths(label, bytes, localPaths) {
  for (const value of localPaths.filter(Boolean)) {
    for (const spelling of new Set([
      resolve(value),
      resolve(value).replaceAll("\\", "/"),
    ])) {
      for (const encoding of ["utf8", "utf16le"]) {
        assert(
          !bytes.includes(Buffer.from(spelling, encoding)),
          `${label} embeds a local build path`,
        );
      }
    }
  }
}

export function releaseRustFlags(repositoryRoot, userProfile, inherited = "") {
  assert(repositoryRoot, "Repository root is required");
  assert(userProfile, "USERPROFILE is required for a path-clean Windows build");
  const flags = inherited ? inherited.split("\u001f").filter(Boolean) : [];
  flags.push(
    `--remap-path-prefix=${resolve(repositoryRoot)}=/offertrack`,
    `--remap-path-prefix=${resolve(userProfile)}=/build-user`,
  );
  return flags.join("\u001f");
}

export function validateReleaseMetadata(metadata) {
  const versions = [
    metadata.packageVersion,
    metadata.tauriVersion,
    metadata.cargoVersion,
  ];
  assert(
    versions.every((value) => value === versions[0]),
    "Version mismatch",
  );
  assert.equal(metadata.packageLicense, "MIT", "package.json must use MIT");
  assert.equal(metadata.cargoLicense, "MIT", "Cargo.toml must use MIT");
  assert.equal(
    metadata.bundleActive,
    false,
    "Installer bundling must stay disabled",
  );
  assert.equal(metadata.schemaVersion, 12, "Review schema version metadata");
  assert.equal(metadata.warehouseFormatVersion, 1);
  assert.deepEqual(metadata.mainPermissions, [
    "core:default",
    "core:window:allow-destroy",
    "dialog:allow-open",
  ]);
  assert.deepEqual(metadata.helpPermissions, [
    "core:event:allow-listen",
    "core:event:allow-unlisten",
  ]);
  assert.deepEqual(metadata.helpWindows, ["help"]);
  assert.deepEqual(metadata.detailPermissions, [
    "core:event:allow-listen",
    "core:event:allow-unlisten",
    "core:window:allow-destroy",
  ]);
  assert.deepEqual(metadata.detailWindows, ["application-detail"]);
  assert(
    metadata.csp.includes("default-src 'self'"),
    "CSP must default to self",
  );
  assert(
    !metadata.csp.includes("default-src *"),
    "CSP cannot allow every origin",
  );
  assert.deepEqual(metadata.mcpTools, REQUIRED_MCP_TOOLS);
}

export async function readReleaseMetadata(repositoryRoot) {
  const root = resolve(repositoryRoot);
  const [
    packageText,
    tauriText,
    cargoText,
    migrations,
    warehouse,
    mainCapabilityText,
    helpCapabilityText,
    detailCapabilityText,
    platform,
    help,
    recycle,
    archive,
    tools,
    agentSnapshot,
    detailWindow,
    frontendEntry,
    appRuntime,
  ] = await Promise.all([
    readFile(join(root, "package.json"), "utf8"),
    readFile(join(root, "src-tauri/tauri.conf.json"), "utf8"),
    readFile(join(root, "src-tauri/Cargo.toml"), "utf8"),
    readFile(join(root, "src-tauri/src/migrations.rs"), "utf8"),
    readFile(join(root, "src-tauri/src/warehouse.rs"), "utf8"),
    readFile(join(root, "src-tauri/capabilities/default.json"), "utf8"),
    readFile(join(root, "src-tauri/capabilities/help.json"), "utf8"),
    readFile(join(root, "src-tauri/capabilities/detail.json"), "utf8"),
    readFile(join(root, "src-tauri/src/platform.rs"), "utf8"),
    readFile(join(root, "src-tauri/src/help.rs"), "utf8"),
    readFile(join(root, "src-tauri/src/recycle_bin.rs"), "utf8"),
    readFile(join(root, "src-tauri/src/backup_archive.rs"), "utf8"),
    readFile(join(root, "src-tauri/src/agent_mcp/tools.rs"), "utf8"),
    readFile(join(root, "src-tauri/src/agent_access/snapshot.rs"), "utf8"),
    readFile(join(root, "src-tauri/src/application_detail.rs"), "utf8"),
    readFile(join(root, "src/main.tsx"), "utf8"),
    readFile(join(root, "src-tauri/src/lib.rs"), "utf8"),
  ]);
  const packageJson = parseJson(packageText, "package.json");
  const tauriJson = parseJson(tauriText, "tauri.conf.json");
  const mainCapability = parseJson(mainCapabilityText, "default capability");
  const helpCapability = parseJson(helpCapabilityText, "help capability");
  const detailCapability = parseJson(
    detailCapabilityText,
    "application detail capability",
  );
  const toolBlock = capture(
    tools,
    /pub\(super\) const NAMES:[^=]+=[\s\S]*?\[([\s\S]*?)\];/,
    "MCP tool catalogue",
  );
  const mcpTools = [...toolBlock.matchAll(/"([a-z_]+)"/g)].map(
    (match) => match[1],
  );

  assert(
    !/(?:cmd|powershell)\.exe/i.test(platform),
    "Production launcher uses a shell",
  );
  assert(platform.includes('matches!(parsed.scheme(), "http" | "https")'));
  assert(platform.includes("Command::new(executable)"));
  assert(platform.includes(".arg(value)"));
  assert(
    help.includes('label == "main"'),
    "Missing central webview command gate",
  );
  assert(help.includes('label == "help"'), "Missing help webview command gate");
  assert(
    help.includes('label == "application-detail"'),
    "Missing detail webview command gate",
  );
  assert(
    recycle.includes("enum TrashArea"),
    "Missing closed trash-area allowlist",
  );
  assert(
    recycle.includes(
      "Records,\n    Backups,\n    Documents,\n    AgentSnapshots,",
    ),
  );
  assert(!recycle.includes("pub fn remove_tree_in_area"));
  assert(archive.includes("deny_unknown_fields"));
  assert(archive.includes("valid_path"));
  assert(archive.includes("payload_offset.checked_add(total_bytes)"));
  assert(!/offertrack_(?:delete|remove|trash|purge|clear)/.test(tools));
  assert(
    agentSnapshot.includes('relative_path: "agent-access/snapshot".into()'),
    "Agent snapshot path is not fixed",
  );
  assert(
    !agentSnapshot.includes("fn publish_current_pointer"),
    "Legacy Agent current pointer publication returned",
  );
  assert(
    detailWindow.includes(
      'WebviewUrl::App("index.html?window=application-detail".into())',
    ) &&
      !detailWindow.includes(".visible(false)") &&
      frontendEntry.includes('"application-detail"'),
    "Detail window is not using the shared visible frontend entry",
  );
  assert(
    appRuntime.includes("async fn set_application_detail_target(") &&
      /async fn set_application_detail_target\([\s\S]*?tauri::async_runtime::spawn_blocking/.test(
        appRuntime,
      ) &&
      !appRuntime.includes("fn application_detail_ready("),
    "Detail window native operations must stay outside synchronous IPC handlers",
  );

  const metadata = {
    packageVersion: packageJson.version,
    tauriVersion: tauriJson.version,
    cargoVersion: capture(
      cargoText,
      /^version\s*=\s*"([^"]+)"/m,
      "Cargo version",
    ),
    packageLicense: packageJson.license,
    cargoLicense: capture(
      cargoText,
      /^license\s*=\s*"([^"]+)"/m,
      "Cargo license",
    ),
    bundleActive: tauriJson.bundle?.active,
    csp: tauriJson.app?.security?.csp ?? "",
    schemaVersion: Number(
      capture(
        migrations,
        /CURRENT_SCHEMA_VERSION:\s*i64\s*=\s*(\d+)/,
        "schema version",
      ),
    ),
    warehouseFormatVersion: Number(
      capture(
        warehouse,
        /WAREHOUSE_FORMAT_VERSION:\s*u32\s*=\s*(\d+)/,
        "warehouse format version",
      ),
    ),
    mainPermissions: mainCapability.permissions,
    helpPermissions: helpCapability.permissions,
    helpWindows: helpCapability.windows,
    detailPermissions: detailCapability.permissions,
    detailWindows: detailCapability.windows,
    mcpTools,
  };
  validateReleaseMetadata(metadata);
  return metadata;
}

export async function auditRepository(repositoryRoot) {
  const root = await realpath(resolve(repositoryRoot));
  const metadata = await readReleaseMetadata(root);
  for (const [relativeSource, publicName, kind] of PORTABLE_SOURCE_FILES) {
    const source = join(root, relativeSource);
    if (kind === "text") {
      const text = await readFile(source, "utf8");
      assert(text.trim().length > 100, `${publicName} is unexpectedly short`);
      scanPublicText(publicName, text);
    }
  }
  const license = await readFile(join(root, "LICENSE"), "utf8");
  assert(license.includes("MIT License"));
  assert(license.includes("Permission is hereby granted"));
  const [
    appIcon,
    slogan,
    nativeIcon,
    readme,
    mainHtml,
    helpHtml,
    app,
    help,
    buildScript,
  ] = await Promise.all([
    readFile(join(root, "public/app-icon.png")),
    readFile(join(root, "docs/assets/offertrack-slogan.png")),
    readFile(join(root, "src-tauri/icons/icon.ico")),
    readFile(join(root, "README.md"), "utf8"),
    readFile(join(root, "index.html"), "utf8"),
    readFile(join(root, "help.html"), "utf8"),
    readFile(join(root, "src/App.tsx"), "utf8"),
    readFile(join(root, "src/features/help/HelpWindow.tsx"), "utf8"),
    readFile(join(root, "src-tauri/build.rs"), "utf8"),
  ]);
  assert.equal(appIcon.subarray(0, 8).toString("hex"), "89504e470d0a1a0a");
  assert.equal(appIcon.readUInt32BE(16), 1024, "App icon width changed");
  assert.equal(appIcon.readUInt32BE(20), 1024, "App icon height changed");
  assert.equal(slogan.subarray(0, 8).toString("hex"), "89504e470d0a1a0a");
  assert(slogan.readUInt32BE(16) >= 720, "README slogan is too narrow");
  assert(slogan.readUInt32BE(20) >= 720, "README slogan is too short");
  assert.equal(nativeIcon.subarray(0, 4).toString("hex"), "00000100");
  assert(nativeIcon.readUInt16LE(4) > 0, "Windows icon has no images");
  assert(readme.includes("docs/assets/offertrack-slogan.png"));
  for (const source of [mainHtml, helpHtml, app, help]) {
    assert(source.includes("/app-icon.png"), "A UI entry lost the app icon");
  }
  assert(
    buildScript.includes("cargo:rerun-if-changed=icons/icon.ico"),
    "Cargo does not rebuild Windows resources after an icon change",
  );
  return metadata;
}

async function verifyPe(path, label, localPaths = []) {
  const file = await open(path, "r");
  try {
    const magic = Buffer.alloc(2);
    const { bytesRead } = await file.read(magic, 0, 2, 0);
    assert.equal(bytesRead, 2, `${label} is empty`);
    assert.equal(
      magic.toString("ascii"),
      "MZ",
      `${label} is not a Windows PE file`,
    );
  } finally {
    await file.close();
  }
  scanBinaryForLocalPaths(label, await readFile(path), localPaths);
}

async function verifyPng(path, label) {
  const value = await readFile(path);
  assert(value.length >= 24, `${label} is too short to be a PNG`);
  assert.equal(
    value.subarray(0, 8).toString("hex"),
    "89504e470d0a1a0a",
    `${label} is not a PNG file`,
  );
  assert(value.readUInt32BE(16) > 0, `${label} has an invalid width`);
  assert(value.readUInt32BE(20) > 0, `${label} has an invalid height`);
}

async function listPortableEntries(root) {
  const files = [];
  const directories = [];
  async function visit(current) {
    const entries = await readdir(current, { withFileTypes: true });
    for (const entry of entries) {
      const path = join(current, entry.name);
      const info = await lstat(path);
      assert(!info.isSymbolicLink(), "Portable package contains a link");
      const name = relative(root, path).replaceAll("\\", "/");
      if (info.isDirectory()) {
        directories.push(name);
        await visit(path);
      } else {
        assert(info.isFile(), "Portable package contains a special file");
        files.push(name);
      }
    }
  }
  await visit(root);
  return { files: files.sort(), directories: directories.sort() };
}

async function assertSafeOutputParent(repositoryRoot, outputParent) {
  const repo = await realpath(resolve(repositoryRoot));
  const requested = resolve(outputParent);
  const parent = await realpath(requested);
  assert.equal(
    normalizedPath(requested),
    normalizedPath(parent),
    "Output parent uses a link or junction",
  );
  assert(
    !isWithin(repo, parent),
    "Portable output must be outside the repository",
  );
  assert(
    !isWithin(parent, repo),
    "Portable output cannot contain the repository",
  );
  const info = await lstat(parent);
  assert(
    info.isDirectory() && !info.isSymbolicLink(),
    "Output parent must be a real directory",
  );
  return parent;
}

async function copyExclusive(source, target) {
  await copyFile(source, target, constants.COPYFILE_EXCL);
  // FlushFileBuffers on Windows requires a handle opened for writing.
  const handle = await open(target, "r+");
  try {
    await handle.sync();
  } finally {
    await handle.close();
  }
}

export async function assemblePortableDirectory({
  repositoryRoot,
  outputParent,
  metadata,
}) {
  const root = await realpath(resolve(repositoryRoot));
  const parent = await assertSafeOutputParent(root, outputParent);
  const packageName = `OfferTrack-v${metadata.packageVersion}-windows-x64-portable`;
  const candidate = join(parent, packageName);
  await mkdir(candidate);

  const files = [];
  for (const [relativeSource, publicName, kind] of PORTABLE_SOURCE_FILES) {
    const source = join(root, relativeSource);
    const sourceInfo = await stat(source);
    assert(sourceInfo.isFile(), `${relativeSource} is not a file`);
    if (kind === "binary") {
      await verifyPe(source, publicName, [root, process.env.USERPROFILE]);
    } else if (kind === "text") {
      scanPublicText(publicName, await readFile(source, "utf8"));
    } else {
      await verifyPng(source, publicName);
    }
    const target = join(candidate, publicName);
    await mkdir(dirname(target), { recursive: true });
    await copyExclusive(source, target);
    const copiedInfo = await stat(target);
    files.push({
      path: publicName,
      sizeBytes: copiedInfo.size,
      sha256: await sha256(target),
    });
  }
  assert.notEqual(
    files[0].sha256,
    files[1].sha256,
    "Desktop and CLI binaries are identical",
  );

  const manifest = {
    formatVersion: 1,
    product: "OfferTrack",
    version: metadata.packageVersion,
    platform: "windows-x64",
    databaseSchemaVersion: metadata.schemaVersion,
    warehouseFormatVersion: metadata.warehouseFormatVersion,
    databaseBackupFormatVersion: 1,
    fullBackupFormatVersion: 1,
    agentContractVersion: 1,
    files,
  };
  const manifestPath = join(candidate, GENERATED_FILES[0]);
  await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`, {
    encoding: "utf8",
    flag: "wx",
  });
  const checksummed = [
    ...files,
    {
      path: GENERATED_FILES[0],
      sizeBytes: (await stat(manifestPath)).size,
      sha256: await sha256(manifestPath),
    },
  ];
  const checksumPath = join(candidate, GENERATED_FILES[1]);
  await writeFile(
    checksumPath,
    `${checksummed.map((file) => `${file.sha256}  ${file.path}`).join("\n")}\n`,
    { encoding: "utf8", flag: "wx" },
  );
  await auditPortableDirectory(candidate, metadata);
  return { candidate, packageName };
}

export async function auditPortableDirectory(candidateDirectory, metadata) {
  const candidate = await realpath(resolve(candidateDirectory));
  const expected = [
    ...PORTABLE_SOURCE_FILES.map(([, name]) => name),
    ...GENERATED_FILES,
  ].sort();
  const expectedDirectories = ["docs", "docs/assets", "docs/user-guide"];
  const entries = await listPortableEntries(candidate);
  assert.deepEqual(entries.files, expected);
  assert.deepEqual(entries.directories, expectedDirectories);

  const manifest = parseJson(
    await readFile(join(candidate, GENERATED_FILES[0]), "utf8"),
    GENERATED_FILES[0],
  );
  assert.equal(manifest.formatVersion, 1);
  assert.equal(manifest.version, metadata.packageVersion);
  assert.equal(manifest.databaseSchemaVersion, metadata.schemaVersion);
  assert.equal(
    manifest.warehouseFormatVersion,
    metadata.warehouseFormatVersion,
  );
  assert.deepEqual(
    manifest.files.map((file) => file.path),
    PORTABLE_SOURCE_FILES.map(([, name]) => name),
  );
  for (const file of manifest.files) {
    const path = join(candidate, file.path);
    assert.equal(
      (await stat(path)).size,
      file.sizeBytes,
      `${file.path} size changed`,
    );
    assert.equal(await sha256(path), file.sha256, `${file.path} hash changed`);
  }
  const expectedChecksums = [
    ...manifest.files,
    {
      path: GENERATED_FILES[0],
      sha256: await sha256(join(candidate, GENERATED_FILES[0])),
    },
  ]
    .map((file) => `${file.sha256}  ${file.path}`)
    .join("\n");
  assert.equal(
    (await readFile(join(candidate, GENERATED_FILES[1]), "utf8")).trimEnd(),
    expectedChecksums,
  );
  for (const [, name, kind] of PORTABLE_SOURCE_FILES) {
    if (kind === "text")
      scanPublicText(name, await readFile(join(candidate, name), "utf8"));
    else if (kind === "binary") await verifyPe(join(candidate, name), name);
    else await verifyPng(join(candidate, name), name);
  }
}

export function createPortableArchive(
  repositoryRoot,
  candidate,
  outputParent,
  packageName,
) {
  assert.equal(
    process.platform,
    "win32",
    "Portable ZIP creation currently supports Windows only",
  );
  const archive = join(outputParent, `${packageName}.zip`);
  const script = join(
    repositoryRoot,
    "scripts/release/create-portable-zip.ps1",
  );
  const result = spawnSync(
    "powershell.exe",
    [
      "-NoProfile",
      "-NonInteractive",
      "-ExecutionPolicy",
      "Bypass",
      "-File",
      script,
      "-Source",
      candidate,
      "-Destination",
      archive,
    ],
    { encoding: "utf8", windowsHide: true, shell: false },
  );
  assert.equal(
    result.error,
    undefined,
    "Unable to start PowerShell archive helper",
  );
  assert.equal(result.status, 0, result.stderr || result.stdout);
  return archive;
}

export async function buildPortablePackage(repositoryRoot, outputParent) {
  const root = await realpath(resolve(repositoryRoot));
  const metadata = await auditRepository(root);
  const { candidate, packageName } = await assemblePortableDirectory({
    repositoryRoot: root,
    outputParent,
    metadata,
  });
  const archive = createPortableArchive(
    root,
    candidate,
    dirname(candidate),
    packageName,
  );
  const archiveHash = await sha256(archive);
  const externalChecksum = `${archive}.sha256`;
  await writeFile(externalChecksum, `${archiveHash}  ${basename(archive)}\n`, {
    encoding: "utf8",
    flag: "wx",
  });
  return { candidate, archive, externalChecksum, archiveHash, metadata };
}
