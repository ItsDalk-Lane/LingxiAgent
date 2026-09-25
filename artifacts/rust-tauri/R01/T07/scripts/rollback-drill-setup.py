#!/usr/bin/env python3
"""R01-T07 / R01-A14 drill world setup (synthetic data only, /tmp sandbox)."""
import json
import shutil
import sqlite3
import subprocess
import sys
from pathlib import Path

SB = Path(sys.argv[1])
rb = SB / "rollback"
shutil.rmtree(rb, ignore_errors=True)
root = rb / "root"
ep1 = root / "epoch-1"
ep2 = root / "epoch-2"

# t0: epoch-1 old world
(ep1 / "agents/agent-main/sessions").mkdir(parents=True)
(ep1 / "session-files/hash-old").mkdir(parents=True)
(ep1 / "agents/agent-main/sessions/s-001.jsonl").write_text(
    '{"type":"session","id":"s-001"}\n'
    '{"type":"message","role":"user","content":"pre-cutover question"}\n'
    '{"type":"message","role":"assistant","content":"pre-cutover answer"}\n')
(ep1 / "session-files/hash-old/att-old.bin").write_bytes(b"old-attachment-bytes")
(ep1 / "data-epoch.json").write_text(json.dumps({
    "schemaVersion": 2, "epoch": 1, "minimumReaderEpoch": 1,
    "committedDataEpoch": 1, "lastVersion": "0.0.0-t07-old",
    "updatedAt": "2026-09-25T00:00:00.000Z"}, indent=2))

# t1: pre-cutover backup B0 (writers stopped, consistent copy)
shutil.copytree(ep1, rb / "backup-epoch1", symlinks=True)

# t2: epoch-2 new world — imported history + receipts
ep2.mkdir(parents=True)
(ep2 / "attachments").mkdir()
(ep2 / "runs").mkdir()
(ep2 / "data-epoch.json").write_text(json.dumps({
    "schemaVersion": 2, "epoch": 2, "minimumReaderEpoch": 2,
    "committedDataEpoch": 2, "lastVersion": "9.9.9-rust-prototype",
    "updatedAt": "2026-09-25T01:00:00.000Z"}, indent=2))
db = sqlite3.connect(ep2 / "messages.sqlite")
db.execute("PRAGMA journal_mode=WAL")  # W07: WAL-mode live store
db.execute("CREATE TABLE messages (msg_id TEXT PRIMARY KEY, run_id TEXT,"
           " role TEXT, content TEXT, created_at TEXT, post_cutover INTEGER)")
db.execute("INSERT INTO messages VALUES ('m-001','run-imported','user',"
           "'pre-cutover question','2026-09-25T00:30:00Z',0)")
db.execute("INSERT INTO messages VALUES ('m-002','run-imported','assistant',"
           "'pre-cutover answer','2026-09-25T00:30:05Z',0)")
db.commit()
(ep2 / "cutover-receipt.json").write_text(json.dumps({
    "receipt": "epoch1->2", "imported_sessions": ["s-001"],
    "source_archive": "epoch-1 retained read-only",
    "completed_at": "2026-09-25T01:00:00.000Z"}, indent=2))

# t3: post-cutover NEW traffic (sessions + attachment created after cutover)
db.execute("INSERT INTO messages VALUES ('m-101','run-post-1','user',"
           "'post-cutover question','2026-09-25T02:00:00Z',1)")
db.execute("INSERT INTO messages VALUES ('m-102','run-post-1','assistant',"
           "'post-cutover answer','2026-09-25T02:00:05Z',1)")
db.commit()
(ep2 / "attachments/blob-new.bin").write_bytes(b"new-post-cutover-attachment")
(ep2 / "runs/run-post-1.json").write_text('{"run_id":"run-post-1","status":"done"}')
# leave WAL hot on purpose: drill must prove the backup is still consistent
db.close()

# t2 pointer: active -> epoch-2 (post-cutover live state)
subprocess.run(["zsh", str(Path(__file__).parent / "atomic-switch.zsh"),
                str(root), "epoch-2"], check=True)
print("DRILL-WORLD-READY", rb)
