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
  python3 "/Users/study_superior/Desktop/Code/LingxiAgent/artifacts/rust-tauri/R05/RR3/A-REVIEW-01/discover-old.py" "$MAIN_REPO" "$EVIDENCE_DIR"
}

validate_run_output_unit() {
  local repo="$1" unit="$2"
  case "$unit" in
    ""|.|..|../*|/*)
      printf 'not a repo-relative path (repo root / parent / absolute paths can never be exclusion roots)\n' >&2
      return 1
      ;;
  esac
  python3 - "$repo" "$unit" <<'PYLINK' || return 1
import os
import sys
current = sys.argv[1]
for component in sys.argv[2].split("/"):
    if component in ("", ".", ".."):
        raise SystemExit("output root is not a plain relative path")
    current = os.path.join(current, component)
    if os.path.islink(current):
        raise SystemExit("output root crosses a symlink: " + current)
PYLINK
  if [ -n "$(git --literal-pathspecs -C "$repo" ls-files -- "$unit")" ]; then
    printf 'covers INDEX-TRACKED content (candidate source / committed evidence must stay bound)\n' >&2
    return 1
  fi
  return 0
}

validate_declared_run_root() {
  local repo="$1" unit="$2"
  case "$unit" in
    ""|.|..|../*|/*|*/)
      printf 'not a plain repo-relative directory path\n' >&2
      return 1
      ;;
  esac
  case "$unit" in
    artifacts/*/*/*) : ;;                        # >= artifacts/<area>/<stage>/<run>: `*` in a case
                                                 # pattern matches across `/`, so this is a depth
                                                 # floor of 4 components — artifacts itself, a bare
                                                 # area, or a bare stage dir can never pass
    *)
      printf 'not a dedicated run-output root strictly inside artifacts/ at run-root depth (>= artifacts/<area>/<stage>/…): excluding the whole evidence tree or foreign directories is forbidden\n' >&2
      return 1
      ;;
  esac
  [ -d "$repo/$unit" ] || { printf 'not an existing directory\n' >&2; return 1; }
  python3 - "$repo" "$unit" <<'PYLINK' || return 1
import os
import sys
current = sys.argv[1]
for component in sys.argv[2].split("/"):
    if component in ("", ".", ".."):
        raise SystemExit("output root is not a plain relative path")
    current = os.path.join(current, component)
    if os.path.islink(current):
        raise SystemExit("output root crosses a symlink: " + current)
PYLINK
  if [ -n "$(git --literal-pathspecs -C "$repo" ls-files -- "$unit")" ]; then
    printf 'covers INDEX-TRACKED content (candidate source / committed evidence must stay bound)\n' >&2
    return 1
  fi
  return 0
}
