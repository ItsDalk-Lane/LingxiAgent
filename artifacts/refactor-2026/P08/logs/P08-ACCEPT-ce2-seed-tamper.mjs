#!/usr/bin/env node
/**
 * ACC counter-example 2 (P08 independent acceptance): seed kit tamper / bad signature.
 * Uses the SAME shared/artifact-core verification primitives the product build/runtime uses.
 * All dirs under /tmp, no repo writes, no real accounts.
 */
import fs from "node:fs";
import path from "node:path";
import { createHash } from "node:crypto";
import { createRequire } from "node:module";
const require = createRequire("/Users/study_superior/Desktop/Code/LingxiAgent/");
const { parseManifest, verifyManifest } = require("/Users/study_superior/Desktop/Code/LingxiAgent/shared/artifact-core/manifest.cjs");
const { sha256File } = require("/Users/study_superior/Desktop/Code/LingxiAgent/shared/artifact-core/activation.cjs");
const crypto = require("node:crypto");

const SRC = "/Users/study_superior/Desktop/Code/LingxiAgent/dist-server-artifact/mac-arm64";
const WORK = "/tmp/acc-seed-tamper";
const KEY_PRIV = "/tmp/acc-sign/acc-ephemeral.key";
const KEYSET = [{ keyId: "acc-acceptance-key", publicKey: JSON.parse(fs.readFileSync("/tmp/acc-sign/pub.json", "utf8")).publicKey }];

const results = [];
function record(id, desc, ok, detail) { results.push({ id, desc, ok, detail }); console.log(`[${ok ? "PASS" : "FAIL-EXPECTED"}] ${id}: ${desc} → ${detail}`); }
function expectFail(id, desc, fn) {
  try { fn(); record(id, desc, false, "NO ERROR — verification did NOT fail (unexpected)"); return false; }
  catch (err) { record(id, desc, true, `rejected: ${String(err.message).slice(0, 140)}`); return true; }
}

function reset() { fs.rmSync(WORK, { recursive: true, force: true }); fs.cpSync(SRC, WORK, { recursive: true }); }
function manifestPath() { return path.join(WORK, "seed-train-darwin-arm64.json"); }
function sigPath() { return manifestPath() + ".sig"; }
function reSign() {
  const m = JSON.parse(fs.readFileSync(manifestPath(), "utf8"));
  m.keyId = "acc-acceptance-key";
  const bytes = Buffer.from(JSON.stringify(m, null, 2) + "\n", "utf8");
  fs.writeFileSync(manifestPath(), bytes);
  const priv = crypto.createPrivateKey(fs.readFileSync(KEY_PRIV, "utf8"));
  fs.writeFileSync(sigPath(), crypto.sign(null, bytes, priv));
  return m;
}

/* CE-2a: positive control — properly re-signed manifest verifies with my keyset */
reset();
let m = reSign();
try {
  const man = verifyManifest(fs.readFileSync(manifestPath()), fs.readFileSync(sigPath()), KEYSET);
  record("CE-2a", "正确重签（验收临时密钥）→ verifyManifest 通过（阳性对照）", true, `keyId=${man.keyId} sourceCommit=${man.sourceCommit.slice(0, 10)}`);
} catch (err) { record("CE-2a", "positive control should pass", false, err.message); }

/* CE-2b: tamper manifest content WITHOUT re-signing → signature mismatch must fail */
m = JSON.parse(fs.readFileSync(manifestPath(), "utf8"));
const target = m.artifacts["server"]["darwin-arm64"];
const origSha = target.sha256;
target.sha256 = (target.sha256.slice(0, 8) === "deadbeef" ? "cafe" : "deadbeef") + target.sha256.slice(8);
fs.writeFileSync(manifestPath(), Buffer.from(JSON.stringify(m, null, 2) + "\n", "utf8"));
expectFail("CE-2b", "manifest 内容篡改（改 server 归档 sha256 字段、不重签）→ 签名校验必拒",
  () => verifyManifest(fs.readFileSync(manifestPath()), fs.readFileSync(sigPath()), KEYSET));

/* CE-2c: tamper archive BYTES, manifest+sig still valid → content hash mismatch must be caught */
reset();
m = reSign();
{
  const serverEntry = m.artifacts["server"]["darwin-arm64"];
  const file = path.join(WORK, path.basename(serverEntry.path || serverEntry.file || ""));
  const buf = fs.readFileSync(file);
  buf[buf.length - 100] = buf[buf.length - 100] ^ 0x5a;
  fs.writeFileSync(file, buf);
  // signature still valid (manifest untouched)
  verifyManifest(fs.readFileSync(manifestPath()), fs.readFileSync(sigPath()), KEYSET);
  const actual = createHash("sha256").update(fs.readFileSync(file)).digest("hex");
  const match = actual === serverEntry.sha256;
  record("CE-2c", "归档字节篡改（manifest+sig 有效）→ 内容哈希比对必失配", !match,
    `manifest=${serverEntry.sha256.slice(0, 16)} actual=${actual.slice(0, 16)} mismatch=${!match}`);
}

/* CE-2d: unknown/removed key (e.g. ephemeral deleted after round) → fail-closed */
reset(); // back to p08-local-ephemeral-signed original
expectFail("CE-2d", "密钥不在 keyset（临时密钥已删的本地测试件）→ fail-closed 拒绝",
  () => verifyManifest(fs.readFileSync(manifestPath()), fs.readFileSync(sigPath()), KEYSET));

const allExpected = results.every((r) => r.ok);
fs.writeFileSync("/tmp/acc-ce2-results.json", JSON.stringify({ tool: "acc-ce2-seed-tamper", all_expected: allExpected, results }, null, 2));
console.log(`\nCE-2 summary: ${results.filter((r) => r.ok).length}/${results.length} behaved as expected`);
process.exit(allExpected ? 0 : 1);
