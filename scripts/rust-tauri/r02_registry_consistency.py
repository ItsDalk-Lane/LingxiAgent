#!/usr/bin/env python3
"""R02-T04 registry consistency gate (R05 RR2 F41).

Low-cost EQUALITY self-check between the compiled-in migration set and the
R02 fingerprint registry:

    rust/crates/lingxi-adapters/src/storage/migrations.rs  MIGRATIONS
      == docs/rust-tauri/R02/R02-T04_STORAGE_REGISTRY.json migrations

The check is exact — count, versions, names and sha256(SQL) fingerprints
must all agree. It is deliberately NOT a subset/intersection comparison:
a migration shipped in code but missing from the registry (F41: v6/v7 were
added without registering them) AND a registry entry with no backing
migration both fail, each named by version. Fingerprints are recomputed
from the SQL text parsed out of migrations.rs with the registry's own
sha256-of-SQL method, so a drifted SQL body fails even if nobody touched
the JSON.

Pure literal-pattern string parsing (no dynamic patterns, no shell-out);
the registry is read as JSON with the stdlib parser.

Exit codes: 0 = identical, 1 = any named disagreement / parse failure.
Usage: python3 scripts/rust-tauri/r02_registry_consistency.py [--root DIR]
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path

MIGRATIONS_RS = Path("rust/crates/lingxi-adapters/src/storage/migrations.rs")
REGISTRY_JSON = Path("docs/rust-tauri/R02/R02-T04_STORAGE_REGISTRY.json")

NAME_CONST_RE = re.compile(
    r'pub const (V\d+)_NAME: &str = "([^"]*)";', re.DOTALL
)
SQL_CONST_RE = re.compile(r'pub const (V\d+)_SQL: &str = r#"(.*?)"#;', re.DOTALL)
MIGRATIONS_ENTRY_RE = re.compile(
    r"Migration\s*\{\s*version:\s*(\d+),\s*name:\s*([A-Za-z0-9_]+),"
    r"\s*sql:\s*([A-Za-z0-9_]+),\s*\}",
    re.DOTALL,
)


def parse_migrations_rs(path: Path) -> list[dict]:
    """Extract the MIGRATIONS list with resolved names and SQL bodies."""
    text = path.read_text(encoding="utf-8")
    names = {}
    for key, value in NAME_CONST_RE.findall(text):
        const = f"{key}_NAME"
        if const in names:
            raise SystemExit(f"FAIL: duplicate {const} const in {path}")
        names[const] = value
    sqls = {}
    for key, value in SQL_CONST_RE.findall(text):
        const = f"{key}_SQL"
        if const in sqls:
            raise SystemExit(f"FAIL: duplicate {const} const in {path}")
        sqls[const] = value
    # Only the entries of the MIGRATIONS slice count (a stray const that the
    # slice does not reference must not silently pass).
    slice_match = re.search(
        r"pub const MIGRATIONS: &\[Migration\] = &\[(.*?)\];", text, re.DOTALL
    )
    if slice_match is None:
        raise SystemExit(f"FAIL: cannot locate the MIGRATIONS slice in {path}")
    entries = []
    for version, name_const, sql_const in MIGRATIONS_ENTRY_RE.findall(
        slice_match.group(1)
    ):
        if name_const not in names:
            raise SystemExit(
                f"FAIL: MIGRATIONS references {name_const} which has no const"
            )
        if sql_const not in sqls:
            raise SystemExit(
                f"FAIL: MIGRATIONS references {sql_const} which has no const"
            )
        sql_body = sqls[sql_const]
        entries.append(
            {
                "version": int(version),
                "name": names[name_const],
                "sql": sql_body,
                "fingerprint": hashlib.sha256(
                    sql_body.encode("utf-8")
                ).hexdigest(),
            }
        )
    if not entries:
        raise SystemExit(f"FAIL: MIGRATIONS slice in {path} parsed empty")
    return entries


def parse_registry(path: Path) -> list[dict]:
    doc = json.loads(path.read_text(encoding="utf-8"))
    points = doc.get("new_persistence_points") or []
    if len(points) != 1:
        raise SystemExit(
            f"FAIL: expected exactly 1 new_persistence_point in {path}, "
            f"got {len(points)}"
        )
    out = []
    for m in points[0].get("migrations") or []:
        out.append(
            {
                "version": int(m["version"]),
                "name": m["name"],
                "fingerprint": m["fingerprint_sha256"],
            }
        )
    return out


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--root",
        default=None,
        help="repository root (default: two levels above this script)",
    )
    args = parser.parse_args()
    root = Path(args.root).resolve() if args.root else Path(__file__).resolve().parents[2]

    rs_path = root / MIGRATIONS_RS
    reg_path = root / REGISTRY_JSON
    for path in (rs_path, reg_path):
        if not path.is_file():
            print(f"FAIL: required input missing: {path}", file=sys.stderr)
            return 1

    compiled = parse_migrations_rs(rs_path)
    registered = parse_registry(reg_path)
    compiled.sort(key=lambda m: m["version"])
    registered.sort(key=lambda m: m["version"])

    problems: list[str] = []
    if len(compiled) != len(registered):
        problems.append(
            f"count mismatch: migrations.rs MIGRATIONS has {len(compiled)} "
            f"entries, the registry has {len(registered)}"
        )
    by_version_c = {m["version"]: m for m in compiled}
    by_version_r = {m["version"]: m for m in registered}
    for version in sorted(set(by_version_c) | set(by_version_r)):
        c = by_version_c.get(version)
        r = by_version_r.get(version)
        if c is not None and r is None:
            problems.append(
                f"v{version} ({c['name']}) exists in migrations.rs MIGRATIONS "
                f"but is MISSING from the registry"
            )
            continue
        if c is None and r is not None:
            problems.append(
                f"v{version} ({r['name']}) is registered but has no backing "
                f"migration in migrations.rs MIGRATIONS"
            )
            continue
        if c["name"] != r["name"]:
            problems.append(
                f"v{version} name mismatch: migrations.rs {c['name']!r} != "
                f"registry {r['name']!r}"
            )
        if c["fingerprint"] != r["fingerprint"]:
            problems.append(
                f"v{version} fingerprint mismatch: sha256(SQL) from "
                f"migrations.rs is {c['fingerprint']}, registry has "
                f"{r['fingerprint']}"
            )

    if problems:
        for line in problems:
            print(f"FAIL: {line}", file=sys.stderr)
        print(
            "FAIL: registry consistency — migrations.rs MIGRATIONS and "
            "R02-T04_STORAGE_REGISTRY.json disagree "
            f"(root={root})",
            file=sys.stderr,
        )
        return 1

    for m in compiled:
        print(
            f"OK v{m['version']} {m['name']} {m['fingerprint']}"
        )
    print(
        f"PASS: migrations.rs MIGRATIONS == registry "
        f"({len(compiled)} entries; versions, names and sha256(SQL) "
        f"fingerprints exact)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
