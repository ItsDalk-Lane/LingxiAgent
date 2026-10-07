#!/bin/bash
# F51-REVIEW-01 command runner: executes one shell command string, captures
# stdout/stderr to out-<label>.txt / err-<label>.txt and appends a true JSON
# record (label, argv, utc start/end, exit) to commands.jsonl.
# Usage: run.sh <label> <cwd> <command string executed by bash -c>
set -u
LABEL="$1"; CWD="$2"; CMD="$3"
HERE="$(cd "$(dirname "$0")" && pwd)"
OUT="$HERE/out-$LABEL.txt"; ERR="$HERE/err-$LABEL.txt"
UTC_START=$(date -u +%Y-%m-%dT%H:%M:%S.%NZ)
( cd "$CWD" && bash -c "$CMD" ) >"$OUT" 2>"$ERR"
EXIT=$?
UTC_END=$(date -u +%Y-%m-%dT%H:%M:%S.%NZ)
python3 - "$HERE/commands.jsonl" "$LABEL" "$CWD" "$CMD" "$UTC_START" "$UTC_END" "$EXIT" "$OUT" "$ERR" <<'PYEOF'
import json, os, sys
jl, label, cwd, cmd, ts, te, exit_, out, err = sys.argv[1:10]
rec = {
    "label": label,
    "cwd": cwd,
    "cmd": cmd,
    "utcStart": ts,
    "utcEnd": te,
    "exit": int(exit_),
    "stdoutFile": os.path.basename(out),
    "stdoutSha256": __import__("hashlib").sha256(open(out,'rb').read()).hexdigest(),
    "stdoutBytes": os.path.getsize(out),
    "stderrFile": os.path.basename(err),
    "stderrSha256": __import__("hashlib").sha256(open(err,'rb').read()).hexdigest(),
    "stderrBytes": os.path.getsize(err),
}
with open(jl, "a", encoding="utf-8") as f:
    f.write(json.dumps(rec, ensure_ascii=False) + "\n")
PYEOF
echo "[$LABEL] exit=$EXIT"
exit $EXIT
