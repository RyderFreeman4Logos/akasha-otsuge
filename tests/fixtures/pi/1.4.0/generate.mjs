// Run with Node >=22.19.0, installed Pi package directory, and an empty private scratch directory.
// This invokes the native SessionManager with synthetic content; it makes no model request.
import assert from "node:assert/strict";
import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { createHash } from "node:crypto";

const [pkg, scratch] = process.argv.slice(2).map((path) => resolve(path));
assert.equal(JSON.parse(readFileSync(join(pkg, "package.json"))).version, "1.4.0");
const { SessionManager } = await import(pathToFileURL(join(pkg, "dist/core/session-manager.js")));
const workspace = join(scratch, "workspace");
const sessions = join(scratch, "sessions");
const out = join(scratch, "fixtures");
for (const path of [workspace, sessions, out]) mkdirSync(path);
const native = SessionManager.create(workspace, sessions);
const ids = {};
const user = (text) => ({ role: "user", content: text, timestamp: Date.now() });
const assistant = (content) => ({
  role: "assistant", content, api: "openai-responses", provider: "fixture", model: "fixture",
  usage: { input: 1, output: 1, cacheRead: 0, cacheWrite: 0, totalTokens: 2,
    cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } },
  stopReason: "stop", timestamp: Date.now(),
});
native.appendModelChange("fixture", "fixture");
native.appendMessage({ role: "system", content: "", sections: { preamble: "PRIVATE_SYSTEM_FIXTURE" },
  toolsAdded: [], timestamp: Date.now() });
ids.user = native.appendMessage(user("Original intent α\u2028retained"));
ids.assistant = native.appendMessage(assistant([
  { type: "thinking", thinking: "PRIVATE_THINKING_FIXTURE", thinkingSignature: "PRIVATE_SIGNATURE_FIXTURE" },
  { type: "text", text: "Original public answer" },
  { type: "toolCall", id: "call_fixture", name: "read", arguments: { path: "note.txt" } },
]));
ids.tool = native.appendMessage({ role: "toolResult", toolCallId: "call_fixture", toolName: "read",
  content: [{ type: "text", text: "Original tool result" }], details: { private: "PRIVATE_DETAILS_FIXTURE" },
  isError: false, timestamp: Date.now() });
const rootId = native.getSessionId();
const rootFile = native.getSessionFile();
copyFileSync(rootFile, join(out, "root-before.jsonl"));
native.branch(ids.user);
ids.sibling = native.appendMessage(user("SIBLING_BRANCH_ONLY"));
native.appendMessage(assistant([{ type: "text", text: "SIBLING_ANSWER_ONLY" }]));
ids.branch_summary = native.branchWithSummary(ids.tool, "A discarded branch was explored.");
ids.current_user = native.appendMessage(user("Current branch request"));
ids.edit = native.appendContextEdit(ids.user, { content: "Replacement for future model context only" });
ids.omission = native.appendContextEdit(ids.current_user, null);
assert.equal(native.getEntry(ids.user).message.content, "Original intent α\u2028retained");
ids.compaction = native.appendCompaction("A native compaction summary", ids.current_user, 50000);
ids.final = native.appendMessage(assistant([{ type: "text", text: "Current branch final" }]));
native.resetLeaf();
ids.other_root = native.appendMessage(user("OTHER_ROOT_ONLY"));
native.branch(ids.final);
assert.notEqual(native.getLeafId(), native.getEntries().at(-1).id);
const forkFile = native.createBranchedSession(ids.final);
const forkId = native.getSessionId();
ids.fork_user = native.appendMessage(user("Fork request"));
copyFileSync(forkFile, join(out, "fork.jsonl"));
const reopened = SessionManager.open(rootFile);
reopened.branch(ids.final);
ids.parent_later = reopened.appendMessage(user("LATER_PARENT_ONLY"));
copyFileSync(rootFile, join(out, "root-tree.jsonl"));
const hashes = {};
for (const name of ["root-before.jsonl", "root-tree.jsonl", "fork.jsonl"]) {
  hashes[name] = createHash("sha256").update(readFileSync(join(out, name))).digest("hex");
}
writeFileSync(join(out, "manifest.json"), JSON.stringify({
  package: "@earendil-works/pi-coding-agent", version: "1.4.0", session_version: 3,
  provenance: "Native SessionManager with synthetic content; no authenticated model or extension run",
  root_id: rootId, fork_id: forkId, root_file: rootFile, fork_file: forkFile, workspace,
  ids, hashes,
}, null, 2) + "\n");
console.log(JSON.stringify({ out, files: Object.keys(hashes), root_id: rootId, fork_id: forkId }));
