#!/usr/bin/env node
/**
 * R01-T07 repair-r1 sample builder (ZCode:R01-T07-repair-r1, 2026-09-25).
 *
 * Independently constructs the synthetic LINGXI_HOME directories for the
 * review-R1 variants R7/R9/R10/R11 plus a blocked control C1. Valid stamps
 * and journals are produced with the production schema constructors in
 * shared/data-epoch.cjs (createDataEpochStamp / createDataEpochJournal), so
 * "valid" means "passes the production reader's full validation", not my
 * guess at the shape. Corrupt samples are torn-write truncations of those
 * same valid payloads. Every planted file is also copied verbatim into
 * artifacts/rust-tauri/R01/T07/repair-r1/samples/<variant>/ for audit.
 *
 * Homes live under /tmp/lingxi-r01t07-repair-r1/homes/<variant> — synthetic
 * only, zero real user data. Each home also gets a variant-marker.txt so the
 * before/after filesystem diff proves pre-existing content survives (or not).
 */
"use strict";

const fs = require("fs");
const path = require("path");

const REPO = "/Users/study_superior/Desktop/Code/LingxiAgent";
const { createDataEpochStamp, createDataEpochJournal } = require(
  path.join(REPO, "shared/data-epoch.cjs"),
);

const HOMES = "/tmp/lingxi-r01t07-repair-r1/homes";
const SAMPLES = path.join(REPO, "artifacts/rust-tauri/R01/T07/repair-r1/samples");

const STAMP_FILE = "data-epoch.json";
const JOURNAL_FILE = "data-epoch-transition.json";

function plant(variant, files) {
  const home = path.join(HOMES, variant);
  const sampleDir = path.join(SAMPLES, variant);
  fs.rmSync(home, { recursive: true, force: true });
  fs.rmSync(sampleDir, { recursive: true, force: true });
  fs.mkdirSync(home, { recursive: true });
  fs.mkdirSync(sampleDir, { recursive: true });
  const marker = `repair-r1 variant ${variant} pre-existing content marker\n`;
  files[ "variant-marker.txt" ] = marker;
  for (const [name, content] of Object.entries(files)) {
    fs.writeFileSync(path.join(home, name), content);
    fs.writeFileSync(path.join(sampleDir, name), content);
  }
  console.log(`planted ${variant}: ${Object.keys(files).sort().join(", ")}`);
}

const json = (v) => `${JSON.stringify(v, null, 2)}\n`;

// Valid epoch-2 v2 stamp (blocked control + building block).
const stamp2 = createDataEpochStamp({
  minimumReaderEpoch: 2,
  committedDataEpoch: 2,
  lastVersion: "0.0.0-repair-r1-newworld",
  updatedAt: "2026-09-25T00:00:00.000Z",
});

// Valid barrier_raised journal 1 -> 2 (phase past checkpoint, so a checkpoint
// receipt is required by the production schema).
const journalBarrier = createDataEpochJournal({
  transitionId: "repair-r1-transition-0002",
  fromEpoch: 1,
  toEpoch: 2,
  migrationIds: ["m-repair-r1-0002"],
  affectedStoreIds: ["session-jsonl"],
  recoveryModes: { "m-repair-r1-0002": "resume-idempotent" },
  phase: "barrier_raised",
  checkpointId: "ckpt-repair-r1-0002",
  checkpointReceipt: {
    id: "ckpt-repair-r1-0002",
    createdAt: "2026-09-25T00:00:00.000Z",
    storeDigests: { "session-jsonl": "sha256:repair-r1-synthetic" },
  },
  createdAt: "2026-09-25T00:00:00.000Z",
  lastVersion: "0.0.0-repair-r1-newworld",
});

// Valid prepared journal 1 -> 2 (pre-checkpoint: checkpoint fields must be null).
const journalPrepared = createDataEpochJournal({
  transitionId: "repair-r1-transition-0003",
  fromEpoch: 1,
  toEpoch: 2,
  migrationIds: ["m-repair-r1-0003"],
  affectedStoreIds: ["session-jsonl"],
  recoveryModes: { "m-repair-r1-0003": "resume-idempotent" },
  phase: "prepared",
  checkpointId: null,
  checkpointReceipt: null,
  createdAt: "2026-09-25T00:00:00.000Z",
  lastVersion: "0.0.0-repair-r1-newworld",
});

// Torn-write corruptions: truncate a valid payload mid-stream.
function torn(payload) {
  return payload.slice(0, Math.floor(payload.length / 2));
}
const stamp2Json = json(stamp2);
const journalBarrierJson = json(journalBarrier);

// C1: blocked control — intact epoch-2 v2 stamp only. Expect exit 1, zero fs diff.
plant("c1-blocked-control", { [STAMP_FILE]: stamp2Json });

// R7: corrupt (torn) stamp + VALID high-epoch barrier_raised journal.
// Review R1 finding: fails open with reason=corrupt-stamp even though a
// readable journal targeting epoch 2 is present.
plant("r7-corrupt-stamp-valid-barrier-journal", {
  [STAMP_FILE]: torn(stamp2Json),
  [JOURNAL_FILE]: journalBarrierJson,
});

// R9: corrupt (torn) journal + NO stamp. Expect fail-open reason=corrupt-journal.
plant("r9-corrupt-journal-no-stamp", {
  [JOURNAL_FILE]: torn(journalBarrierJson),
});

// R10: VALID barrier_raised journal (toEpoch=2) + NO stamp. The journal
// contradicts the missing stamp -> corrupt-transition -> fail-open.
plant("r10-valid-barrier-journal-no-stamp", {
  [JOURNAL_FILE]: journalBarrierJson,
});

// R11: VALID prepared journal (toEpoch=2) + NO stamp. Expect REFUSAL with
// reason=incomplete-transition (phase-dependent contrast to R10).
plant("r11-valid-prepared-journal-no-stamp", {
  [JOURNAL_FILE]: json(journalPrepared),
});

console.log("all repair-r1 samples planted under", HOMES);
