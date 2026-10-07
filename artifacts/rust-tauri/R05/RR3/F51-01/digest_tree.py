#!/usr/bin/env python3
"""F51 tree digest calculator (fd-based, PATH_MAX-safe).

Computes a deterministic, byte-level manifest digest of a directory tree:
- recursion is performed on directory file descriptors (scandir(fd) + stat/open/
  readlink with dir_fd), so trees whose absolute child paths exceed macOS
  PATH_MAX (1024) after relocation remain fully traversable and hashable;
- regular files: SHA256 of content; symlinks: SHA256 of the link-target byte
  string (never follows the link, stable across a pure rename relocation);
- manifest rows "TYPE SIZE SHA256 RELPATH" sorted by RELPATH; tree_digest is
  SHA256 of the manifest text. The manifest format is identical to the
  run-1 scanner, so digests from both runs are directly comparable;
- summary also reports max_relpath_len (deepest relative path length), used by
  the driver to keep destination prefixes within PATH_MAX.

Usage: digest_tree.py <dir> [--json]
"""
import hashlib
import json
import os
import stat as statmod
import subprocess
import sys


def _sha256_stream(fd):
    h = hashlib.sha256()
    while True:
        chunk = os.read(fd, 1 << 20)
        if not chunk:
            break
        h.update(chunk)
    return h.hexdigest()


def scan_tree(base):
    rows = []
    stats = {"max_relpath_len": 0, "scan_errors": []}

    def rec(prefix, dir_fd):
        with os.scandir(dir_fd) as it:
            children = sorted(it, key=lambda e: e.name)
        for entry in children:
            name = entry.name
            rel = prefix + "/" + name if prefix else name
            if len(rel.encode("utf-8", "surrogateescape")) > stats["max_relpath_len"]:
                stats["max_relpath_len"] = len(rel.encode("utf-8", "surrogateescape"))
            try:
                st = os.stat(name, dir_fd=dir_fd, follow_symlinks=False)
            except OSError as e:
                raise SystemExit("FATAL stat %s: %s" % (rel, e))
            mode = st.st_mode
            if statmod.S_ISDIR(mode):
                dfd = os.open(name, os.O_RDONLY | os.O_DIRECTORY, dir_fd=dir_fd)
                try:
                    rec(rel, dfd)
                finally:
                    os.close(dfd)
            elif statmod.S_ISLNK(mode):
                target = os.readlink(name, dir_fd=dir_fd)
                tb = target.encode("utf-8", "surrogateescape")
                rows.append((rel, "l", len(tb), hashlib.sha256(tb).hexdigest()))
            elif statmod.S_ISREG(mode):
                fd = os.open(name, os.O_RDONLY, dir_fd=dir_fd)
                try:
                    rows.append((rel, "f", st.st_size, _sha256_stream(fd)))
                finally:
                    os.close(fd)
            else:
                rows.append((rel, "x%o" % (mode & 0o170000), st.st_size, None))

    base_fd = os.open(base, os.O_RDONLY | os.O_DIRECTORY)
    try:
        rec("", base_fd)
    finally:
        os.close(base_fd)
    rows.sort(key=lambda r: r[0])
    manifest_lines = ["%s %d %s %s\n" % (t, size, sha if sha else "-", rel)
                      for rel, t, size, sha in rows]
    manifest = "".join(manifest_lines).encode("utf-8", "surrogateescape")
    summary = {
        "file_count": sum(1 for r in rows if r[1] == "f"),
        "symlink_count": sum(1 for r in rows if r[1] == "l"),
        "other_count": sum(1 for r in rows if r[1].startswith("x")),
        "total_bytes": sum(r[2] for r in rows),
        "tree_digest": hashlib.sha256(manifest).hexdigest(),
        "max_relpath_len": stats["max_relpath_len"],
    }
    return summary, rows


def du_kb(path):
    """Best-effort; returns None on failure (recorded as UNKNOWN, never fabricated)."""
    try:
        out = subprocess.run(["du", "-sk", path], capture_output=True, text=True, check=True, timeout=300)
        return int(out.stdout.split()[0])
    except Exception:
        return None


def nested_git_head(path):
    try:
        out = subprocess.run(["git", "-C", path, "rev-parse", "HEAD"],
                             capture_output=True, text=True, timeout=30)
        if out.returncode == 0:
            return out.stdout.strip()
    except Exception:
        pass
    return None


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    base = sys.argv[1].rstrip("/")
    summary, rows = scan_tree(base)
    summary["du_kilobytes"] = du_kb(base)
    summary["nested_git_head"] = nested_git_head(base)
    if "--json" in sys.argv:
        print(json.dumps(summary, sort_keys=True))
    else:
        for k in sorted(summary):
            print("%s=%s" % (k, summary[k]))
    return 0


if __name__ == "__main__":
    sys.exit(main())
