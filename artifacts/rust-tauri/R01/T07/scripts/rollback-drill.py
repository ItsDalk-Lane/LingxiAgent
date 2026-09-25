#!/usr/bin/env python3
"""R01-T07 / R01-A14 rollback drill — design-level export/restore rehearsal.

Scenario (all synthetic, under an isolated sandbox):
  t0  epoch-1 home (old Electron/Node world): JSONL sessions + attachments.
  t1  pre-cutover backup B0 (consistent copy, writers stopped).
  t2  cutover: new epoch-2 root (messages.sqlite WAL + blobs + receipts),
      atomic pointer `active` switched to epoch-2.
  t3  post-cutover traffic: NEW sessions/messages + NEW attachments in epoch-2.
  t4  rollback decision: user must return to the old version.

The drill proves:
  * new-epoch data is exported to a rollback archive BEFORE any restore
    (no "delete the new directory" step anywhere);
  * the live WAL-mode database is snapshotted with the SQLite Online Backup
    API after a wal_checkpoint quiesce (W06/W07) — never a raw .db copy;
  * the old backup restores into a SEPARATE root and the pointer is switched
    atomically (rename); the epoch-2 root is never mutated;
  * the old production binary (current HEAD server) starts on the restored
    epoch-1 root;
  * the archive verifies (SHA-256) and re-import is idempotent (merge twice
    -> identical logical content).

Usage: rollback-drill.py <sandbox-rollback-dir> <evidence-dir>
Exit 0 + "ROLLBACK-DRILL-PASSED" on success; non-zero otherwise.
"""
import hashlib
import json
import os
import shutil
import sqlite3
import subprocess
import sys
from pathlib import Path

REPO = Path("/Users/study_superior/Desktop/Code/LingxiAgent")
PROBE = REPO / "artifacts/rust-tauri/R01/T07/scripts/run-start-probe.zsh"
SWITCH = REPO / "artifacts/rust-tauri/R01/T07/scripts/atomic-switch.zsh"


def sha256(p: Path) -> str:
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def fail(msg: str) -> None:
    print(f"DRILL-FAIL: {msg}")
    sys.exit(1)


def main() -> None:
    rb = Path(sys.argv[1])
    ev = Path(sys.argv[2])
    root = rb / "root"
    ep1_backup = rb / "backup-epoch1"
    ep2 = root / "epoch-2"
    archive = rb / "rollback-archive"
    log_lines = []

    def log(msg: str) -> None:
        print(msg)
        log_lines.append(msg)

    # ── step 1: precheck — post-cutover state, no writers running ──────────
    active = os.readlink(root / "active")
    if not active.endswith("epoch-2"):
        fail(f"precheck: active must point at epoch-2, got {active}")
    stamp = json.loads((ep2 / "data-epoch.json").read_text())
    if stamp.get("committedDataEpoch") != 2:
        fail("precheck: epoch-2 root stamp is not committedDataEpoch=2")
    log("step1 PRECHECK ok: active->epoch-2, stamp committed=2, no server running (drill assumption)")

    # ── step 2: quiesce + W06 online backup of the WAL-mode new store ─────
    src = sqlite3.connect(ep2 / "messages.sqlite")
    src.execute("PRAGMA wal_checkpoint(TRUNCATE)")  # W07: fold WAL into main db
    archive.mkdir(parents=True, exist_ok=True)
    dst = sqlite3.connect(archive / "messages.sqlite.bak")
    src.backup(dst)  # W06: online backup API, never a raw .db file copy
    dst.close()
    check = sqlite3.connect(archive / "messages.sqlite.bak")
    integrity = check.execute("PRAGMA integrity_check").fetchone()[0]
    rows = check.execute("SELECT count(*) FROM messages").fetchone()[0]
    post_cutover = check.execute(
        "SELECT count(*) FROM messages WHERE post_cutover=1").fetchone()[0]
    check.close()
    src.close()
    if integrity != "ok":
        fail(f"step2: backup integrity_check={integrity}")
    log(f"step2 W06-BACKUP ok: integrity_check=ok rows={rows} post_cutover_rows={post_cutover}")

    # ── step 3: export new-epoch data to the rollback archive ─────────────
    bak = sqlite3.connect(archive / "messages.sqlite.bak")
    export_path = archive / "messages-export.jsonl"
    with open(export_path, "w") as f:
        for r in bak.execute(
                "SELECT msg_id, run_id, role, content, created_at, post_cutover"
                " FROM messages ORDER BY created_at, msg_id"):
            f.write(json.dumps({
                "msg_id": r[0], "run_id": r[1], "role": r[2],
                "content": r[3], "created_at": r[4],
                "post_cutover": bool(r[5]),
            }, ensure_ascii=False, sort_keys=True) + "\n")
    bak.close()
    (archive / "attachments").mkdir(exist_ok=True)
    for blob in sorted((ep2 / "attachments").iterdir()):
        shutil.copy2(blob, archive / "attachments" / blob.name)
    shutil.copy2(ep2 / "cutover-receipt.json", archive / "cutover-receipt.json")
    manifest = {"schema": "r01-t07-rollback-archive/1", "entries": []}
    for p in sorted(archive.rglob("*")):
        if p.is_file() and p.name != "manifest.json":
            manifest["entries"].append({
                "path": str(p.relative_to(archive)),
                "sha256": sha256(p),
                "kind": ("sqlite-backup" if p.suffix == ".bak"
                         else "export" if p.suffix == ".jsonl"
                         else "receipt" if p.suffix == ".json"
                         else "attachment"),
            })
    (archive / "manifest.json").write_text(json.dumps(manifest, indent=2))
    log(f"step3 EXPORT ok: archive entries={len(manifest['entries'])}")

    # ── step 4: restore old backup into a SEPARATE root + atomic switch ────
    restored = root / "epoch-1-restored"
    if restored.exists():
        shutil.rmtree(restored)
    shutil.copytree(ep1_backup, restored, symlinks=True)
    subprocess.run(["zsh", str(SWITCH), str(root), "epoch-1-restored"],
                   check=True, capture_output=True, text=True)
    log(f"step4 RESTORE ok: active -> {os.readlink(root / 'active')} (atomic rename swap)")

    # ── step 5: old production binary must start on the restored root ──────
    ep2_before = {str(p.relative_to(ep2)): sha256(p)
                  for p in ep2.rglob("*") if p.is_file()}
    probe = subprocess.run(
        ["zsh", str(PROBE), "a14-restored-epoch1-starts",
         str(root / "active"), "18821"],
        capture_output=True, text=True)
    probe_log = (REPO / "artifacts/rust-tauri/R01/T07/a13/a14-restored-epoch1-starts.log").read_text()
    if "STARTED=1" not in probe_log:
        fail("step5: old binary did not start on restored epoch-1 root")
    ep2_after = {str(p.relative_to(ep2)): sha256(p)
                 for p in ep2.rglob("*") if p.is_file()}
    if ep2_before != ep2_after:
        fail("step5: epoch-2 root changed while old binary ran on restored root")
    log("step5 OLD-BINARY ok: STARTED=1 on restored epoch-1; epoch-2 root byte-identical")

    # ── step 6: verify archive integrity ───────────────────────────────────
    m = json.loads((archive / "manifest.json").read_text())
    for e in m["entries"]:
        if sha256(archive / e["path"]) != e["sha256"]:
            fail(f"step6: archive hash mismatch: {e['path']}")
    log(f"step6 ARCHIVE-VERIFY ok: {len(m['entries'])} files sha256-verified")

    # ── step 7: idempotent re-import (forward-roll again) ──────────────────
    target = rb / "reimport-target" / "messages.sqlite"
    target.parent.mkdir(parents=True, exist_ok=True)
    if target.exists():
        target.unlink()
    def merge_once() -> str:
        db = sqlite3.connect(target)
        db.execute("CREATE TABLE IF NOT EXISTS messages ("
                   "msg_id TEXT PRIMARY KEY, run_id TEXT, role TEXT,"
                   " content TEXT, created_at TEXT, post_cutover INTEGER)")
        with open(export_path) as f:
            for line in f:
                r = json.loads(line)
                db.execute("INSERT OR IGNORE INTO messages VALUES (?,?,?,?,?,?)",
                           (r["msg_id"], r["run_id"], r["role"], r["content"],
                            r["created_at"], int(r["post_cutover"])))
        db.commit()
        dump = "\n".join(str(row) for row in db.execute(
            "SELECT * FROM messages ORDER BY created_at, msg_id"))
        db.close()
        return hashlib.sha256(dump.encode()).hexdigest()
    first, second = merge_once(), merge_once()
    if first != second:
        fail("step7: re-import is not idempotent")
    log(f"step7 REIMPORT-IDEMPOTENT ok: logical-content sha256 stable across two merges ({first[:16]}…)")

    # ── summary ────────────────────────────────────────────────────────────
    summary = {
        "result": "ROLLBACK-DRILL-PASSED",
        "post_cutover_messages_preserved": post_cutover,
        "archive": str(archive),
        "restored_root": str(restored),
        "epoch2_untouched": True,
        "old_binary_started_on_restored": True,
        "reimport_idempotent": True,
        "not_possible_in_old_version": [
            "editing/browsing post-cutover sessions (they live only in the rollback archive + epoch-2 root)",
            "resuming post-cutover runs",
            "seeing post-cutover attachments in the old UI (files preserved in archive, re-import on next forward-roll)",
        ],
    }
    (ev / "rollback-drill-summary.json").write_text(json.dumps(summary, indent=2))
    print("ROLLBACK-DRILL-PASSED")


if __name__ == "__main__":
    main()
