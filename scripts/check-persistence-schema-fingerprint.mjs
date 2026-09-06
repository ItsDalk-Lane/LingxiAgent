// Fast persistence-schema-fingerprint repin guard.
//
// 权威检查位于 tests/persistence-schema-tripwire.test.ts，在 npm test 中运行
// 完整的 assertCommittedPersistenceSchemaFingerprint：解析指纹列出的受护源文件，
// 并检查 SQLite 实际结构；受护文件数量随指纹变化，不在此维护副本。
// 本脚本只比较 diff，不解析 TypeScript 或打开数据库，可在本地独立运行。
// CI 将它与测试矩阵并行执行；它不是测试矩阵的前置依赖，也不代替完整检查。
//
// It derives the guarded file set straight from the committed fingerprint JSON
// (siteMappings[].sourceFile + schemas[].module + extensions[].module +
// protocolModules[].module), diffs the change set against the base, and fails
// fast when a guarded source moved but the fingerprint file did not move in
// the same change — pointing at the repin command. A schema edit that is
// comment-only or whitespace-only still hashes identically (the digest walks
// the parse tree), so the tripwire test itself would not fire; this guard is
// intentionally a superset: it flags ANY guarded-file touch without a repin,
// because the review discipline applies at edit time regardless of whether the
// hash ultimately changed.
//
// Environments:
//   - CI push:        $GITHUB_EVENT_BEFORE (previous SHA) vs HEAD
//   - CI pull_request: $GITHUB_BASE_REF (origin/<base>) merge base vs HEAD
//   - local:          HEAD vs working tree (staged + unstaged), so a developer
//                     can run this pre-push and see the same failure.
//
// Skips (exit 0): a brand-new branch (before is all-zero / base missing), a
// missing fingerprint (nothing to guard yet), or a change that already
// repins the fingerprint in the same diff.

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPOSITORY_ROOT = path.resolve(SCRIPT_DIR, "..");
const FINGERPRINT_PATH = "build/persistence-schema-fingerprint.json";
const ALL_ZERO_SHA = "0000000000000000000000000000000000000000";

function git(args, { rejectOnError = true } = {}) {
  try {
    return execFileSync("git", args, {
      cwd: REPOSITORY_ROOT,
      encoding: "utf-8",
      maxBuffer: 16 * 1024 * 1024,
      stdio: ["ignore", "pipe", "pipe"],
    }).trim();
  } catch (error) {
    if (!rejectOnError) return null;
    throw error;
  }
}

function guardedFiles(fingerprint) {
  const files = new Set();
  for (const site of fingerprint.siteMappings ?? []) {
    if (site.sourceFile) files.add(site.sourceFile);
  }
  for (const schema of fingerprint.schemas ?? []) {
    if (schema.module) files.add(schema.module);
    for (const ext of schema.extensions ?? []) {
      if (ext.module) files.add(ext.module);
    }
    for (const proto of schema.protocolModules ?? []) {
      if (proto.module) files.add(proto.module);
    }
  }
  return files;
}

// Resolve the changed-file set for this change. Priority mirrors how a
// contributor experiences the diff: CI-provided SHAs first, then the local
// working tree as a fallback so the same script is useful pre-push.
function changedFiles() {
  const baseRef = process.env.GITHUB_BASE_REF;
  const eventBefore = process.env.GITHUB_EVENT_BEFORE;

  if (baseRef) {
    // pull_request: diff the merge base of origin/<base>...HEAD against HEAD.
    const mergeBase = git(["merge-base", `origin/${baseRef}`, "HEAD"], { rejectOnError: false });
    if (mergeBase) {
      return git(["diff", "--name-only", `${mergeBase}..HEAD`]).split("\n").filter(Boolean);
    }
  }

  if (eventBefore && eventBefore !== ALL_ZERO_SHA) {
    // push: diff the previous commit against HEAD.
    return git(["diff", "--name-only", `${eventBefore}..HEAD`]).split("\n").filter(Boolean);
  }

  if (eventBefore === ALL_ZERO_SHA) {
    // First commit on a new branch — no base to compare against.
    return { skip: true };
  }

  // Local fallback: staged + unstaged vs HEAD. Gives the same signal pre-push.
  return git(["diff", "--name-only", "HEAD"]).split("\n").filter(Boolean);
}

function main() {
  const fingerprintFile = path.join(REPOSITORY_ROOT, ...FINGERPRINT_PATH.split("/"));
  if (!fs.existsSync(fingerprintFile)) {
    console.log(`persistence-schema-guard: ${FINGERPRINT_PATH} not found; nothing to guard yet.`);
    return;
  }

  const fingerprint = JSON.parse(fs.readFileSync(fingerprintFile, "utf-8"));
  const guarded = guardedFiles(fingerprint);
  if (guarded.size === 0) {
    console.log("persistence-schema-guard: fingerprint lists no guarded sources; skipping.");
    return;
  }

  const result = changedFiles();
  if (result && result.skip) {
    console.log("persistence-schema-guard: first commit on a new branch; skipping (no base to diff).");
    return;
  }
  const changed = result;
  if (changed.length === 0) {
    console.log("persistence-schema-guard: no changed files; OK.");
    return;
  }

  const touchedGuarded = changed.filter((file) => guarded.has(file)).sort();
  const repinned = changed.some(
    (file) => file === FINGERPRINT_PATH
      // normalize both posix-style separators
      || file === FINGERPRINT_PATH.replaceAll("/", path.sep),
  );

  if (touchedGuarded.length === 0) {
    console.log(`persistence-schema-guard: no guarded persistence sources touched (${guarded.size} watched); OK.`);
    return;
  }

  if (repinned) {
    console.log(`persistence-schema-guard: guarded sources touched AND fingerprint repinned in the same change (${touchedGuarded.length} source(s)); OK.`);
    console.log("The full assertion in tests/persistence-schema-tripwire.test.ts still runs under `npm test`.");
    return;
  }

  // Fail: a guarded source moved without a fingerprint repin in the same diff.
  console.error("::error::persistence schema sources changed but the fingerprint was not repinned in the same commit.");
  console.error("");
  console.error("Guarded sources touched without a repin:");
  for (const file of touchedGuarded) console.error(`  - ${file}`);
  console.error("");
  console.error(`The committed fingerprint (${FINGERPRINT_PATH}) pins the persisted shape of ${guarded.size} guarded sources.`);
  console.error("When any of them changes, regenerate the fingerprint in the same commit so the tripwire stays accurate.");
  console.error("");
  console.error("If the change is a COMPATIBLE addition (no persisted shape breaks):");
  console.error("  node scripts/generate-persistence-schema-fingerprint.mjs \\");
  console.error('    --classification compatible \\');
  console.error('    --compatibility-reason "<explain why this source change is schema-compatible>"');
  console.error("");
  console.error("If the change is BREAKING (a persisted shape that existing data cannot satisfy),");
  console.error("bump DATA_EPOCH and declare the full breaking transition:");
  console.error("  node scripts/generate-persistence-schema-fingerprint.mjs \\");
  console.error("    --classification breaking \\");
  console.error("    --source-data-epoch <current> --target-data-epoch <current+1> \\");
  console.error('    --affected-store <storeId> --checkpoint-policy "<...>" --restore-policy "<...>"');
  console.error("");
  console.error("Then commit build/persistence-schema-fingerprint.json together with the source change.");
  console.error("(A comment-only or whitespace-only edit hashes identically and will not move the");
  console.error("fingerprint; this guard is deliberately stricter — repin regardless, then let the");
  console.error("tripwire test confirm the hash is unchanged.)");
  process.exit(1);
}

main();
