bind_worktree() {
  python3 - "$1" "${3:-}" > "$2" <<'PYBIND'
import hashlib
import os
import subprocess
import sys

repo = sys.argv[1]
exclude_units = [u for u in (sys.argv[2] if len(sys.argv) > 2 else "").split("\n") if u]
raw = subprocess.run(
    ["git", "-C", repo, "status", "--porcelain=v1", "-z", "--untracked-files=all"],
    check=True, capture_output=True,
).stdout
records = raw.split(b"\0")
rows = []
i = 0
while i < len(records):
    rec = records[i]
    i += 1
    if not rec:
        continue
    xy = rec[:2].decode("ascii", "replace")
    path = rec[3:].decode("utf-8", "surrogateescape")
    if "R" in xy or "C" in xy:
        # rename/copy records carry the original path in the next record
        i += 1
    if any(path == unit or path.startswith(unit + "/") for unit in exclude_units):
        # this run's own run-time output root (runner output, never
        # candidate source) — excluded SYMMETRICALLY from every binding
        # that passes the same unit set (repair-group5 / F42; see the
        # bind_worktree header)
        continue
    digest = "-"
    full = os.path.join(repo, path)
    if os.path.isfile(full) and not os.path.islink(full):
        with open(full, "rb") as fh:
            digest = hashlib.sha256(fh.read()).hexdigest()
    rows.append((path, xy, digest))
rows.sort()
for unit in exclude_units:
    print(f"# binding-exclusion: {unit}/** — this run's run-time output root (runner output, never candidate source); applied symmetrically to BOTH the main-repo binding and the candidate-copy binding (E0 mirror compare)")
for path, xy, digest in rows:
    print(f"{digest}  {xy}  {path}")
PYBIND
}

discover_run_output_sinks() {
  python3 "/Users/study_superior/Desktop/Code/LingxiAgent/artifacts/rust-tauri/R05/RR3/A-01/discover-before.py" "$MAIN_REPO" "$EVIDENCE_DIR"
}
