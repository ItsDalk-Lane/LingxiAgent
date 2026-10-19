// R01-A03 round-trip driver — the TS consumer leg.
//
// For every golden sample produced by the Rust authority
// (contracts/generated/golden/, written by lingxi-protocol-gen):
//   1. read the exact bytes (Rust canonical encoding);
//   2. JSON.parse (TS decode) and validate against the generated JSON
//      Schema for the declared type;
//   3. re-encode with the TS canonical JSON implementation and assert the
//      bytes are identical to the golden (Rust encode → TS decode →
//      TS encode closure);
//   4. write the re-encoded bytes to --out for the Rust verifier
//      (lingxi-protocol-verify) to read back and re-validate;
//   5. run the sample-specific checks recorded in golden/index.json
//      (big-seq precision proof, cross-language args digest parity,
//      unknown-event preservation, third-party schema verbatim).
//
// Exit 0 only if every sample and every check passes.

import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { canonicalBytes } from "./canonical-json.mjs";
import { validateAgainstSchema } from "./validator.mjs";

const args = process.argv.slice(2);
function opt(name, fallback) {
  const i = args.indexOf(name);
  return i >= 0 ? args[i + 1] : fallback;
}
const generatedDir = path.resolve(opt("--generated", "contracts/generated"));
const goldenDir = path.join(generatedDir, "golden");
const schemaDir = path.join(generatedDir, "jsonschema");
const outDir = opt("--out", null);
if (outDir) fs.mkdirSync(outDir, { recursive: true });

const failures = [];
let sampleCount = 0;

const index = JSON.parse(fs.readFileSync(path.join(goldenDir, "index.json"), "utf-8"));

for (const sample of index.samples) {
  sampleCount += 1;
  const file = sample.file;
  const typeName = sample.type;
  const goldenPath = path.join(goldenDir, file);
  const goldenBytes = fs.readFileSync(goldenPath);
  const label = `${file} (${typeName})`;

  // sha256 pinned by the generator
  const sha = createHash("sha256").update(goldenBytes).digest("hex");
  if (sha !== sample.sha256) {
    failures.push(`${label}: sha256 ${sha} != index ${sample.sha256}`);
  }

  // TS decode
  let decoded;
  try {
    decoded = JSON.parse(goldenBytes.toString("utf-8"));
  } catch (e) {
    failures.push(`${label}: JSON.parse failed: ${e.message}`);
    continue;
  }

  // Schema validation against the generated schema of the declared type
  try {
    const schema = JSON.parse(
      fs.readFileSync(path.join(schemaDir, `${typeName}.schema.json`), "utf-8"),
    );
    validateAgainstSchema(decoded, schema);
  } catch (e) {
    failures.push(`${label}: schema validation failed: ${e.message}`);
  }

  // TS re-encode → byte equality with the Rust canonical golden
  let reEncoded;
  try {
    reEncoded = canonicalBytes(decoded);
  } catch (e) {
    failures.push(`${label}: TS canonical re-encode failed: ${e.message}`);
    continue;
  }
  if (!reEncoded.equals(goldenBytes)) {
    failures.push(
      `${label}: TS re-encoded bytes differ from golden\n    golden: ${goldenBytes.toString("utf-8")}\n    ts:     ${reEncoded.toString("utf-8")}`,
    );
  }
  if (outDir) fs.writeFileSync(path.join(outDir, file), reEncoded);

  // Sample-specific checks
  for (const check of sample.checks ?? []) {
    try {
      runCheck(check, decoded, label);
    } catch (e) {
      failures.push(`${label}: check ${check.kind} failed: ${e.message}`);
    }
  }
}

function getPath(obj, pathParts) {
  let cur = obj;
  for (const p of pathParts) {
    if (cur === null || typeof cur !== "object") throw new Error(`path broke at ${p}`);
    cur = cur[p];
  }
  return cur;
}

function runCheck(check, decoded, label) {
  switch (check.kind) {
    case "big_seq_string": {
      const seq = getPath(decoded, check.field.split("."));
      if (typeof seq !== "string") {
        throw new Error(`seq is ${typeof seq}, expected decimal string`);
      }
      const asBig = BigInt(seq);
      if (asBig <= BigInt(Number.MAX_SAFE_INTEGER)) {
        throw new Error(`sample seq ${seq} does not exceed 2^53-1`);
      }
      // Prove the precision point: as a JS Number this value is lossy.
      if (BigInt(Number(seq)) === asBig) {
        throw new Error(`expected Number(${seq}) to be lossy, but it was exact`);
      }
      console.log(
        `  [check] ${label}: seq=${seq} kept as string; Number() would lose precision ` +
          `(Number→${Number(seq)}, BigInt roundtrip mismatch proven)`,
      );
      break;
    }
    case "args_digest": {
      const digestBytes = createHash("sha256")
        .update(canonicalBytes(check.args))
        .digest("hex");
      const embedded = getPath(decoded, check.digestField);
      if (embedded !== digestBytes) {
        throw new Error(
          `cross-language digest mismatch: TS computed ${digestBytes}, golden carries ${embedded}`,
        );
      }
      console.log(`  [check] ${label}: args digest parity sha256=${digestBytes}`);
      break;
    }
    case "unknown_event_preserved": {
      const payload = decoded.payload;
      if (!payload || typeof payload.type !== "string") {
        throw new Error("payload has no type tag");
      }
      const KNOWN = new Set([
        "run_state_changed", "model_call_started", "model_call_delta",
        "model_call_completed", "tool_call_started", "tool_call_completed",
        "approval_requested", "approval_decided", "assistant_segment_start",
        "assistant_segment_delta", "assistant_segment_end", "final_message_committed",
      ]);
      if (KNOWN.has(payload.type)) {
        throw new Error(`sample type ${payload.type} is unexpectedly a known event`);
      }
      for (const key of ["permissionHint", "payload"]) {
        if (!(key in payload)) {
          throw new Error(`unknown-event field ${key} was dropped`);
        }
      }
      if (decoded.eventType !== payload.type) {
        throw new Error("envelope eventType does not mirror unknown payload type");
      }
      console.log(
        `  [check] ${label}: unknown event "${payload.type}" preserved with ` +
          `${Object.keys(payload).length} top-level fields (incl. permissionHint)`,
      );
      break;
    }
    case "schema_verbatim": {
      const schema = decoded.schema;
      if (!schema || typeof schema !== "object") throw new Error("schema missing");
      const vendor = schema["x-vendor-extension"];
      if (!vendor || vendor.honorific !== "供應商自訂欄位") {
        throw new Error("x-vendor-extension was not preserved verbatim");
      }
      console.log(`  [check] ${label}: third-party schema verbatim, vendor extension intact`);
      break;
    }
    case "nullable_default_thinking_level": {
      // R07-T09: null defaultThinkingLevel must survive the wire as an
      // explicit null — a consumer that drops or invents a level breaks the
      // model-context contract.
      const parts = check.field.split(".");
      if (!(parts[parts.length - 1] in decoded)) {
        throw new Error(`${check.field} was dropped instead of kept as null`);
      }
      if (getPath(decoded, parts) !== null) {
        throw new Error(`${check.field} must be null in this sample`);
      }
      console.log(`  [check] ${label}: ${check.field} kept as explicit null`);
      break;
    }
    case "decimal_string_generation": {
      // R07-T09: modelGeneration rides the wire as a decimal string; this
      // sample exceeds 2^53-1 so a Number conversion is provably lossy.
      const value = getPath(decoded, check.field.split("."));
      if (typeof value !== "string") {
        throw new Error(`${check.field} is ${typeof value}, expected decimal string`);
      }
      const asBig = BigInt(value);
      if (asBig <= BigInt(Number.MAX_SAFE_INTEGER)) {
        throw new Error(`sample ${check.field} ${value} does not exceed 2^53-1`);
      }
      if (BigInt(Number(value)) === asBig) {
        throw new Error(`expected Number(${value}) to be lossy, but it was exact`);
      }
      console.log(
        `  [check] ${label}: ${check.field}=${value} kept as string; Number() would lose precision`,
      );
      break;
    }
    default:
      throw new Error(`unknown check kind ${check.kind}`);
  }
}

if (failures.length > 0) {
  console.error(`ROUND-TRIP FAILURES (${failures.length}):`);
  for (const f of failures) console.error(`  - ${f}`);
  process.exit(1);
}
console.log(
  `OK: ${sampleCount} golden samples decoded, schema-validated and re-encoded byte-identically by the TS consumer` +
    (outDir ? `; re-encoded copies written to ${outDir}` : ""),
);
