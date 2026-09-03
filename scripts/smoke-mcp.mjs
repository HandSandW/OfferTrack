// Native subprocess protocol smoke. No network, real warehouse or external SDK.
// Run after building both binaries: node scripts/smoke-mcp.mjs [path-to-cli]
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdtemp, rmdir } from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve, join } from "node:path";
import { createInterface } from "node:readline";

const executable = resolve(
  process.argv[2] ?? "src-tauri/target/release/offertrack-cli.exe",
);
// Empty directory only: the read-only adapter must not initialize a warehouse.
const temporary = await mkdtemp(join(tmpdir(), "offertrack-mcp-smoke-"));
const child = spawn(executable, ["--warehouse", temporary, "mcp"], {
  stdio: ["pipe", "pipe", "pipe"],
  windowsHide: true,
  shell: false,
});
const pending = new Map();
let stderr = "";
const exit = once(child, "exit");
child.stderr.setEncoding("utf8");
child.stderr.on("data", (text) => {
  stderr += text;
});
const lines = createInterface({ input: child.stdout });
let messages = 0;
let protocolError;
lines.on("line", (line) => {
  try {
    const value = JSON.parse(line);
    assert.equal(value.jsonrpc, "2.0");
    assert(pending.has(value.id), "Unexpected response or notification");
    messages += 1;
    pending.get(value.id)(value);
    pending.delete(value.id);
  } catch (error) {
    protocolError = error;
    child.kill();
  }
});
function send(value) {
  child.stdin.write(`${JSON.stringify(value)}\n`, "utf8");
}
async function request(id, method, params) {
  const response = new Promise((done) => {
    pending.set(id, done);
  });
  send({
    jsonrpc: "2.0",
    id,
    method,
    ...(params === undefined ? {} : { params }),
  });
  return Promise.race([
    response,
    exit.then(() => {
      throw protocolError ?? new Error("Server exited before response");
    }),
  ]);
}
const timeout = setTimeout(() => {
  child.kill();
}, 15000);
try {
  const hello = await request(1, "initialize", {
    protocolVersion: "2025-11-25",
    capabilities: {},
    clientInfo: { name: "OfferTrack smoke", version: "1" },
  });
  assert.equal(hello.result.protocolVersion, "2025-11-25");
  assert.deepEqual(hello.result.capabilities, {
    tools: { listChanged: false },
  });
  send({ jsonrpc: "2.0", method: "notifications/initialized" });
  const catalogue = await request("中文 ID", "tools/list");
  assert.equal(catalogue.result.tools.length, 11);
  assert.equal(
    catalogue.result.tools.find(
      (tool) => tool.name === "offertrack_snapshot_status",
    )?.annotations.readOnlyHint,
    true,
  );
  assert(
    catalogue.result.tools.every(
      (t) =>
        t.annotations.readOnlyHint === (t.name !== "offertrack_write") &&
        t.annotations.destructiveHint === (t.name === "offertrack_write"),
    ),
  );
  const capability = await request(3, "tools/call", {
    name: "offertrack_describe",
    arguments: {},
  });
  assert.equal(capability.result.structuredContent.data.write_enabled, false);
  assert.deepEqual(
    JSON.parse(capability.result.content[0].text),
    capability.result.structuredContent,
  );
  const denied = await request(4, "tools/call", {
    name: "offertrack_clear_trash",
    arguments: {},
  });
  assert.equal(denied.error.code, -32602);
  const missing = await request(5, "tools/call", {
    name: "offertrack_summary",
    arguments: {},
  });
  assert.equal(missing.result.isError, true);
  assert.equal(missing.result.structuredContent.ok, false);
  assert(!JSON.stringify(missing).includes(temporary));
  const snapshot = await request(7, "tools/call", {
    name: "offertrack_snapshot_status",
    arguments: {},
  });
  assert.equal(snapshot.result.isError, true);
  assert.equal(snapshot.result.structuredContent.ok, false);
  assert(!JSON.stringify(snapshot).includes(temporary));
  send({
    jsonrpc: "2.0",
    method: "notifications/cancelled",
    params: { requestId: 5 },
  });
  assert.deepEqual((await request(6, "ping")).result, {});
  child.stdin.end();
  const [code] = await exit;
  assert.equal(code, 0);
  assert.equal(stderr, "");
  assert.equal(messages, 7);
  assert.equal(protocolError, undefined);
  console.log(
    "MCP native smoke passed: handshake, 10 read-only tools + 1 controlled write, snapshot status, UTF-8, denied deletion, tool error, ping, clean EOF.",
  );
} finally {
  clearTimeout(timeout);
  if (child.exitCode === null) {
    child.kill();
    await exit;
  }
  lines.close();
  // Only the explicitly created, still-empty temporary directory may be removed.
  await rmdir(temporary);
}
