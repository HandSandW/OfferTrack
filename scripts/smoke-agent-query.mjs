// Read-only acceptance of a synthetic warehouse exported by the named Rust test.
// node scripts/smoke-agent-query.mjs <fixture-root> [path-to-cli]
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { resolve, join } from "node:path";

assert(process.argv[2], "Pass the exported synthetic fixture directory");
const root = resolve(process.argv[2]);
const executable = resolve(
  process.argv[3] ?? "src-tauri/target/release/offertrack-cli.exe",
);
function run(mode, input) {
  const child = spawnSync(executable, ["--warehouse", root, mode], {
    input,
    encoding: "utf8",
    timeout: 15000,
    maxBuffer: 4 * 1024 * 1024,
    windowsHide: true,
    shell: false,
  });
  assert.equal(child.error, undefined);
  assert.equal(child.status, 0, child.stderr);
  assert.equal(child.stderr, "");
  return child.stdout
    .trim()
    .split("\n")
    .map((line) => JSON.parse(line));
}
function query(request) {
  const replies = run("query", JSON.stringify({ version: 1, request }));
  assert.equal(replies.length, 1);
  assert.equal(replies[0].ok, true);
  return replies[0].data;
}
const applications = query({ operation: "list_applications" }).result;
assert.equal(applications.total, 1, "Expected isolated acceptance fixture");
const application = applications.items[0];
assert.equal(application.company_name, "只读 CLI 示例");
const documents = query({
  operation: "list_documents",
  application_id: application.id,
}).result;
assert.equal(documents.length, 1);
const resolution = query({
  operation: "resolve_document",
  application_id: application.id,
  document_id: documents[0].id,
}).result;
assert(resolution.relative_path.endsWith("/acceptance-resume.pdf"));
const status = query({ operation: "snapshot_status" });
assert.equal(status.state, "current");
assert.equal(status.published, false);
assert(
  /^agent-access\/snapshot-[A-Za-z0-9-]+$/.test(status.snapshot.relative_path),
);
const snapshot = join(root, status.snapshot.relative_path);
const manifest = JSON.parse(
  readFileSync(join(snapshot, "manifest.json"), "utf8"),
);
const names = [
  "applications.jsonl",
  "tasks.jsonl",
  "events.jsonl",
  "fields.json",
  "summary.json",
  "schema.json",
  "README.md",
];
assert.deepEqual(Object.keys(manifest.files).sort(), [...names].sort());
for (const name of names) {
  const bytes = readFileSync(join(snapshot, name));
  assert.equal(bytes.length, manifest.files[name].size_bytes);
  assert.equal(
    createHash("sha256").update(bytes).digest("hex"),
    manifest.files[name].sha256,
  );
}
const snapshotRecord = JSON.parse(
  readFileSync(join(snapshot, "applications.jsonl"), "utf8").trim(),
);
assert.equal(snapshotRecord.id, application.id);
assert.equal(
  snapshotRecord.documents[0].relative_path,
  resolution.relative_path,
);

const frames = [
  {
    jsonrpc: "2.0",
    id: 1,
    method: "initialize",
    params: {
      protocolVersion: "2025-11-25",
      capabilities: {},
      clientInfo: { name: "OfferTrack acceptance", version: "1" },
    },
  },
  { jsonrpc: "2.0", method: "notifications/initialized" },
  {
    jsonrpc: "2.0",
    id: 2,
    method: "tools/call",
    params: { name: "offertrack_list_applications", arguments: {} },
  },
  {
    jsonrpc: "2.0",
    id: 3,
    method: "tools/call",
    params: {
      name: "offertrack_resolve_document",
      arguments: {
        application_id: application.id,
        document_id: documents[0].id,
      },
    },
  },
  {
    jsonrpc: "2.0",
    id: 4,
    method: "tools/call",
    params: { name: "offertrack_write_status", arguments: {} },
  },
];
const replies = run(
  "mcp",
  frames.map((frame) => JSON.stringify(frame)).join("\n") + "\n",
);
assert.equal(replies.length, 4);
for (const reply of replies.slice(1)) {
  assert.equal(reply.result.structuredContent.ok, true);
  assert.deepEqual(
    JSON.parse(reply.result.content[0].text),
    reply.result.structuredContent,
  );
}
assert.equal(
  replies[1].result.structuredContent.data.result.items[0].id,
  application.id,
);
assert.equal(
  replies[2].result.structuredContent.data.result.resolved_path,
  resolution.resolved_path,
);
assert.equal(
  replies[3].result.structuredContent.data.permission.enabled,
  false,
);
console.log(
  JSON.stringify({
    accepted: true,
    sources: ["hashed snapshot", "native JSON CLI", "native stdio MCP"],
    company: application.company_name,
    applicationId: application.id,
    documentId: documents[0].id,
    relativePath: resolution.relative_path,
    resolvedPath: resolution.resolved_path,
    snapshot: status.snapshot.relative_path,
    writesEnabled: false,
  }),
);
// Keep the fixture for inspection; this script has no write or deletion calls.
