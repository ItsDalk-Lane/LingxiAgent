#!/usr/bin/env python3
"""R01-T01 contract validator.

Verifies, mechanically and with a non-zero exit on any violation:

  Ownership contract (OWNERSHIP_TARGET.json):
    O1  every R00 F-ID (FEATURE_INVENTORY.json) is assigned exactly one
        target owner; no unknown F-IDs, no duplicates, no missing;
    O2  every R00 store (STORES.json) is assigned exactly one target owner;
    O3  every referenced owner exists in the owner_registry;
    O4  every critical fact (incl. run_terminal_state) has EXACTLY ONE
        owner. A second owner, an auxiliary owner field
        (secondary_owner/co_owners/backup_owner/sync_owner/reconciliation)
        or any unknown key on the fact entry is a hard rejection -
        "二者同步" (dual-write sync) is not an accepted escape hatch;
    O5  critical facts are never owned by worker/ui/host-kind owners;
    O6  no feature or store is owned by a worker/ui-kind owner;
    O7  the on-disk OWNERSHIP_TARGET.json matches regeneration by
        r01_t01_build_ownership.py (drift check);
    O8  every authoritative-classification store has an authority-kind
        (core/service/adapters) owner AND its target_writer_process is
        exactly the locked canonical rust-service writer string -- the
        single writer of authoritative business data (host/build/ui/worker
        owners, any other writer, or a prefix look-alike such as
        "rust-service-fork" are rejected).

  The critical-fact table is a LOCKED exact set (contract 02 §3, 11 facts):
  a fact entry outside the locked fact_id vocabulary (e.g. a renamed
  "shadow" of a locked fact) is rejected under O4; adding or renaming a
  critical fact is a governance change requiring an ADR amendment.

  Dependency contract (DEPENDENCY_RULES.json vs the real rust/ workspace):
    D1  every rule with forbidden_dep_patterns holds transitively over the
        `cargo metadata` resolve graph for each existing applies_to module
        (the graph is keyed on package IDs, and pattern matching is
        hyphen/underscore-normalized, because cargo rewrites hyphenated
        package names to the underscore crate-identifier form in
        resolve.nodes[].deps[].name). Additionally, a manifest-level
        declaration scan over packages[].dependencies rejects forbidden
        deps that are DECLARED but never activated (inactive optional or
        unselected-target deps never enter the resolve graph; the
        dependencies[].name field carries the real package name, so a
        `rename` cannot disguise the declaration);
    D2  allowed_module_deps rules hold for workspace-internal deps;
    D3  forbidden_source_tokens do not appear in crate source code
        (comments stripped; mentioning a prohibition in a doc comment is
        not a violation, importing the symbol is);
    D4  RunContext source scan: no host-handle field types (DEP-06);
    D5  modules declared status=exists exist on disk and in cargo metadata;
        dually, a module declared status=planned whose crate already exists
        on disk or in the cargo workspace is a violation (a materialized
        "planned" module must not silently bypass the not-yet-effective
        rules). Reverse closure: every cargo workspace member must be a
        REGISTERED module -- cargo build --workspace compiles every member,
        so an unregistered member would be built into the headless
        workspace while no applies_to-keyed rule (DEP-07's workspace-wide
        desktop ban included) ever inspects it.

  Negative battery (--self-test):
    N1  dual owner on run_terminal_state                -> must be rejected
    N2  "sync" bypass (secondary_owner field)           -> must be rejected
    N3  missing feature coverage                        -> must be rejected
    N4  feature owned by a worker                       -> must be rejected
    N5  critical fact owned by a worker                 -> must be rejected
    N6  kernel depending on tauri (synthetic metadata)  -> must be rejected
    N7  RunContext carrying an AppHandle (synthetic)    -> must be rejected
    N8  kernel -> hyphenated internal crate lingxi-adapters, dep recorded
        in cargo's real underscore form (synthetic metadata) -> rejected
    N9  kernel -> lingxi-adapters -> tauri (desktop stack reaching kernel
        transitively through a hyphenated internal crate)    -> rejected
    N10 forged workspace member slipping past an
        allowed_module_deps whitelist                        -> rejected
    N11 authoritative store mapped to a host owner/writer    -> rejected
    N12 shadow critical fact outside the locked 02 §3 set    -> rejected
    N13 unregistered workspace member carrying a desktop dep
        (invisible to every applies_to-keyed rule)           -> rejected
    N14 forbidden dep declared optional but never activated
        (absent from the resolve graph)                      -> rejected
    N15 authoritative store written by a "rust-service-fork"
        prefix look-alike shadow writer                      -> rejected
    Each negative case is accepted ONLY if the checker rejects it with a
    specific, matching violation id; a silent pass or a wrong-reason
    rejection fails the self-test.

Usage:
  python3 -B r01_t01_check_ownership.py                # full check, exit 0/1
  python3 -B r01_t01_check_ownership.py --self-test    # positive + negative battery
  python3 -B r01_t01_check_ownership.py --ownership X --skip-deps --skip-drift
  python3 -B r01_t01_check_ownership.py --emit-negative-fixtures DIR
"""

from __future__ import annotations

import argparse
import copy
import json
import re
import subprocess
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
R01_DIR = Path(__file__).resolve().parent
DEFAULT_OWNERSHIP = R01_DIR / "OWNERSHIP_TARGET.json"
DEFAULT_RULES = R01_DIR / "DEPENDENCY_RULES.json"
FEATURE_INVENTORY = REPO_ROOT / "docs/rust-tauri/R00/FEATURE_INVENTORY.json"
STORES = REPO_ROOT / "docs/rust-tauri/R00/STORES.json"
GENERATOR = R01_DIR / "r01_t01_build_ownership.py"

ALLOWED_FACT_KEYS = {"fact_id", "description", "owners", "contract_ref"}
# Any of these on a critical-fact entry is an attempted dual-authority bypass.
FORBIDDEN_FACT_KEYS = {
    "secondary_owner", "co_owners", "backup_owner", "sync_owner",
    "shadow_owner", "reconciliation", "sync_note", "mirror_owner",
}
# Owner kinds that may never own critical facts / business data.
NON_AUTHORITY_KINDS = {"worker", "ui", "host", "build", "xtask", "cli"}
WORKER_UI_KINDS = {"worker", "ui"}
# Owner kinds allowed to own an authoritative-classification store (O8).
AUTHORITY_STORE_KINDS = {"core", "service", "adapters"}
# The authoritative-store writer contract: rust-service is the single writer.
# Locked CANONICAL writer string, exact match (this is the one string the
# generator r01_t01_build_ownership.py emits as RUST_SERVICE). A prefix
# match would let a look-alike such as "rust-service-fork" pass as a
# shadow writer; changing the canonical string is a generator + data change.
RUST_SERVICE_WRITER = "rust-service (lingxi-service 组合根进程，唯一业务数据写者)"
# Contract 02 §3 critical facts: a LOCKED exact set. Adding or renaming a
# fact (e.g. a "shadow" duplicate under a fresh fact_id) is a governance
# change requiring an ADR amendment, and is rejected here under O4.
LOCKED_CRITICAL_FACTS = {
    "authenticated_principal", "session_identity_branch_messages",
    "run_terminal_state", "attempt_generation_fence", "tool_availability",
    "params_approval_resource_scope", "model_credential_selection",
    "real_files_and_authorization", "history_realtime_projection_semantics",
    "usage_causal_trace", "scheduler_trigger_dedup",
}


def norm_dep_name(name: str) -> str:
    """Normalize a package/dep name for comparison: cargo metadata reports
    dep names in crate-identifier form (hyphens rewritten to underscores,
    e.g. `lingxi-adapters` -> `lingxi_adapters`) while package names and
    rule patterns use the hyphenated form. Comparing either side without
    normalization silently blinds hyphenated forbidden patterns."""
    return name.lower().replace("_", "-")


class Violation(Exception):
    def __init__(self, check_id: str, message: str):
        super().__init__(f"[{check_id}] {message}")
        self.check_id = check_id


# ---------------------------------------------------------------------------
# Ownership checks (pure functions over the loaded document)
# ---------------------------------------------------------------------------

def check_ownership(doc: dict, features: list[dict], stores: list[dict]) -> list[str]:
    findings: list[str] = []

    registry = {o["owner_id"]: o for o in doc["owner_registry"]}
    if len(registry) != len(doc["owner_registry"]):
        raise Violation("O3", "duplicate owner_id in owner_registry")

    # O1: feature coverage
    expected_fids = [f["feature_id"] for f in features]
    rows = doc["feature_ownership"]
    seen: set[str] = set()
    for row in rows:
        fid = row["feature_id"]
        if fid in seen:
            raise Violation("O1", f"duplicate assignment for feature {fid}")
        seen.add(fid)
    missing = [f for f in expected_fids if f not in seen]
    unknown = [f for f in seen if f not in set(expected_fids)]
    if missing:
        raise Violation("O1", f"{len(missing)} features lack a target owner, e.g. {missing[:3]}")
    if unknown:
        raise Violation("O1", f"assignments for unknown F-IDs, e.g. {unknown[:3]}")
    findings.append(f"O1 OK: {len(seen)} features, exactly one assignment each")

    # O3 + O6 for features
    for row in rows:
        owner = row["target_owner"]
        if owner not in registry:
            raise Violation("O3", f"feature {row['feature_id']} references unknown owner {owner}")
        kind = registry[owner]["kind"]
        if kind in WORKER_UI_KINDS:
            raise Violation(
                "O6", f"feature {row['feature_id']} owned by {kind}-kind owner {owner}; "
                      "workers/UI never own features")
    findings.append("O3/O6 OK: every feature owner exists and is an authority kind")

    # O2: store coverage
    expected_stores = [s["id"] for s in stores]
    srows = doc["store_ownership"]
    sseen: set[str] = set()
    for row in srows:
        sid = row["store_id"]
        if sid in sseen:
            raise Violation("O2", f"duplicate assignment for store {sid}")
        sseen.add(sid)
    smissing = [s for s in expected_stores if s not in sseen]
    sunknown = [s for s in sseen if s not in set(expected_stores)]
    if smissing:
        raise Violation("O2", f"{len(smissing)} stores lack a target owner, e.g. {smissing[:3]}")
    if sunknown:
        raise Violation("O2", f"assignments for unknown stores, e.g. {sunknown[:3]}")
    for row in srows:
        owner = row["target_owner"]
        if owner not in registry:
            raise Violation("O3", f"store {row['store_id']} references unknown owner {owner}")
        kind = registry[owner]["kind"]
        if kind in WORKER_UI_KINDS:
            raise Violation(
                "O6", f"store {row['store_id']} owned by {kind}-kind owner {owner}; "
                      "workers/UI never own stores")
    findings.append(f"O2 OK: {len(sseen)} stores, exactly one owner each")

    # O8: authoritative stores are owned by an authority kind and written by
    # rust-service only (TB-01/TB-02: tauri-host writes shell state only,
    # build tooling only applies signed artifacts).
    for row in srows:
        classification = row.get("classification") or {}
        if classification.get("kind") != "authoritative":
            continue
        owner = row["target_owner"]
        kind = registry[owner]["kind"]  # owner existence proven above (O3)
        if kind not in AUTHORITY_STORE_KINDS:
            raise Violation(
                "O8", f"authoritative store {row['store_id']} owned by "
                      f"{kind}-kind owner {owner}; authoritative stores require a "
                      "core/service/adapters owner (never host/build/ui/worker)")
        writer = row.get("target_writer_process", "")
        if not isinstance(writer, str) or writer != RUST_SERVICE_WRITER:
            raise Violation(
                "O8", f"authoritative store {row['store_id']} has writer "
                      f"{writer!r}; the single writer of authoritative business "
                      "data is rust-service, matched against the locked "
                      f"canonical string {RUST_SERVICE_WRITER!r} (exact match, "
                      "not a prefix -- a 'rust-service-fork' look-alike is a "
                      "shadow writer and is rejected)")
    findings.append("O8 OK: authoritative stores owned by authority kinds, "
                    "written by rust-service only")

    # O4/O5: critical facts
    seen_facts: set[str] = set()
    for fact in doc["critical_facts"]:
        fid = fact.get("fact_id", "<missing>")
        if fid in seen_facts:
            raise Violation("O4", f"duplicate critical fact entry {fid}")
        seen_facts.add(fid)
        extra = set(fact.keys()) - ALLOWED_FACT_KEYS
        if extra:
            bypass = extra & FORBIDDEN_FACT_KEYS
            detail = (f"bypass fields {sorted(bypass)}" if bypass
                      else f"unknown fields {sorted(extra)}")
            raise Violation(
                "O4", f"critical fact {fid} carries {detail}; a fact has exactly one owner, "
                      "dual-write/sync is not an accepted escape hatch")
        owners = fact.get("owners")
        if not isinstance(owners, list) or len(owners) != 1:
            raise Violation(
                "O4", f"critical fact {fid} has "
                      f"{0 if not isinstance(owners, list) else len(owners)} owners; "
                      "exactly one owner is required")
        owner = owners[0]
        if owner not in registry:
            raise Violation("O3", f"critical fact {fid} references unknown owner {owner}")
        kind = registry[owner]["kind"]
        if kind in NON_AUTHORITY_KINDS:
            raise Violation(
                "O5", f"critical fact {fid} owned by {kind}-kind owner {owner}; "
                      "only core/service/adapters kinds may own critical facts")
    findings.append(f"O4/O5 OK: {len(seen_facts)} critical facts, single authority-kind owner each")

    # The critical-fact table is the LOCKED exact 02 §3 set: a missing entry
    # breaks coverage, and any entry outside the locked vocabulary (e.g. a
    # renamed "shadow" fact carrying a second semantic owner) is rejected.
    # Adding or renaming a critical fact is a governance change requiring an
    # ADR-001 amendment, not a quiet edit of this table.
    extra_facts = seen_facts - LOCKED_CRITICAL_FACTS
    if extra_facts:
        raise Violation(
            "O4", f"critical fact entries outside the locked 02 §3 set: "
                  f"{sorted(extra_facts)}; adding/renaming a critical fact is a "
                  "governance change (ADR-001 amendment required)")
    missing_facts = LOCKED_CRITICAL_FACTS - seen_facts
    if missing_facts:
        raise Violation("O4", f"missing critical facts: {sorted(missing_facts)}")

    # O7 counts coherence
    counts = doc.get("counts", {})
    if counts.get("features") != len(rows) or counts.get("stores") != len(srows):
        raise Violation("O7", "counts block inconsistent with actual rows")
    findings.append("O7 OK: counts coherent")
    return findings


# ---------------------------------------------------------------------------
# Dependency checks
# ---------------------------------------------------------------------------

def run_cargo_metadata() -> dict:
    cmd = [
        "cargo", "metadata", "--manifest-path", str(REPO_ROOT / "rust/Cargo.toml"),
        "--format-version", "1", "--offline",
    ]
    proc = subprocess.run(
        ["env", "-u", "all_proxy", "-u", "ALL_PROXY", "-u", "http_proxy", "-u",
         "HTTP_PROXY", "-u", "https_proxy", "-u", "HTTPS_PROXY"] + cmd,
        capture_output=True, text=True, cwd=REPO_ROOT)
    if proc.returncode != 0:
        raise Violation("D1", f"cargo metadata failed: {proc.stderr.strip()[:500]}")
    return json.loads(proc.stdout)


def transitive_deps(metadata: dict) -> dict[str, set[str]]:
    """workspace package name -> set of transitively depended-on package names.

    The graph is keyed on PACKAGE IDs (resolve.nodes[].deps[].pkg), never on
    the dep `name` field: cargo reports deps[].name in crate-identifier form
    with hyphens rewritten to underscores (e.g. `lingxi-adapters` ->
    `lingxi_adapters`), while packages[].name keeps the hyphenated form. A
    name-keyed graph therefore (a) never matches hyphenated forbidden
    patterns and (b) breaks the transitive closure at the first hyphenated
    internal package. Package IDs are unambiguous (they also resolve any
    same-name multi-version case); names are only attached after the closure
    for pattern matching and reporting.
    """
    name_of = {p["id"]: p["name"] for p in metadata["packages"]}
    adjacency: dict[str, set[str]] = {}
    for node in metadata["resolve"]["nodes"]:
        adjacency[node["id"]] = {d["pkg"] for d in node["deps"]}

    def closure(pkg_id: str) -> set[str]:
        out: set[str] = set()
        seen = {pkg_id}
        stack = [pkg_id]
        while stack:
            current = stack.pop()
            for dep_id in adjacency.get(current, ()):
                if dep_id in seen:
                    continue
                seen.add(dep_id)
                out.add(name_of.get(dep_id, dep_id))
                stack.append(dep_id)
        return out

    result: dict[str, set[str]] = {}
    for member in metadata["workspace_members"]:
        result[name_of[member]] = closure(member)
    return result


def strip_comments(source: str) -> str:
    """Remove // line comments and /* */ block comments (sufficient for the
    crate sources in scope; string literals containing such markers are not
    used for forbidden-token identifiers)."""
    source = re.sub(r"/\*.*?\*/", "", source, flags=re.S)
    return "\n".join(line.split("//", 1)[0] for line in source.splitlines())


def crate_dir_of(module: dict) -> Path:
    p = Path(module["crate_path"])
    return p if p.is_absolute() else REPO_ROOT / p


def display_path(path: Path) -> str:
    try:
        return str(path.relative_to(REPO_ROOT))
    except ValueError:
        return str(path)


def check_dependencies(rules_doc: dict, metadata: dict) -> list[str]:
    findings: list[str] = []
    closure = transitive_deps(metadata)
    module_by_name = {m["module"]: m for m in rules_doc["module_registry"]}
    workspace_names = set(closure.keys())
    # Manifest-level view of each workspace member: packages[].dependencies
    # lists ALL declared dependencies (incl. optional/target-specific ones
    # that never enter the resolve graph), and dependencies[].name is the
    # real package name (a `rename` only affects the separate `rename`
    # field), so a declared forbidden dep cannot hide behind a rename or
    # behind not being activated.
    member_ids = set(metadata["workspace_members"])
    pkg_of = {p["name"]: p for p in metadata["packages"] if p["id"] in member_ids}

    # D5: module registry consistency, both directions
    for module in rules_doc["module_registry"]:
        if module["status"] == "exists":
            crate_dir = crate_dir_of(module)
            if not (crate_dir / "Cargo.toml").is_file():
                raise Violation("D5", f"module {module['module']} declared exists but "
                                      f"{module['crate_path']}/Cargo.toml missing")
            if module["module"] not in workspace_names:
                raise Violation("D5", f"module {module['module']} declared exists but "
                                      "absent from cargo metadata workspace")
        else:
            # Dual check: a module declared planned must NOT already exist.
            # A materialized "planned" module would silently bypass every
            # rule that binds only once the module is established.
            crate_dir = crate_dir_of(module)
            if (crate_dir / "Cargo.toml").is_file():
                raise Violation(
                    "D5", f"module {module['module']} declared {module['status']} but "
                          f"{module['crate_path']}/Cargo.toml already exists; a "
                          "materialized module must be re-registered as exists with "
                          "its dependency rules made effective (deliberate registry "
                          "update), not slipped in under planned status")
            if module["module"] in workspace_names:
                raise Violation(
                    "D5", f"module {module['module']} declared {module['status']} but "
                          "already present in the cargo metadata workspace")
    findings.append("D5 OK: module registry matches disk and cargo metadata "
                    "(exists modules present, planned modules absent)")

    for rule in rules_doc["dependency_rules"]:
        rid = rule["id"]
        for module_name in rule["applies_to_modules"]:
            module = module_by_name[module_name]
            if module["status"] != "exists":
                continue  # rule binds once the module is established
            deps = closure.get(module_name, set())

            # D1: forbidden dependency patterns (transitive). Both sides are
            # hyphen/underscore-normalized so a pattern written in package
            # form (lingxi-adapters) also matches the crate-identifier form
            # (lingxi_adapters) and vice versa.
            for pattern in rule.get("forbidden_dep_patterns", []):
                norm_pattern = norm_dep_name(pattern)
                hits = sorted(d for d in deps if norm_pattern in norm_dep_name(d))
                if hits:
                    raise Violation(
                        "D1", f"{rid}: module {module_name} transitively depends on "
                              f"forbidden {hits} (pattern {pattern!r})")

            # D1 manifest-level declaration scan: the resolve graph only
            # contains ACTIVATED edges, so a forbidden dep declared optional
            # (or under an unselected target) is invisible to the transitive
            # check above until some feature activates it. The contract is
            # "must not depend", and a manifest declaration alone already
            # violates it, so declared dependencies are scanned directly.
            pkg = pkg_of.get(module_name)
            if pkg is not None:
                for pattern in rule.get("forbidden_dep_patterns", []):
                    norm_pattern = norm_dep_name(pattern)
                    declared = sorted({
                        d["name"] for d in pkg.get("dependencies", [])
                        if norm_pattern in norm_dep_name(d["name"])})
                    if declared:
                        raise Violation(
                            "D1", f"{rid}: module {module_name} declares forbidden "
                                  f"dependencies {declared} (pattern {pattern!r}) in its "
                                  "manifest; declaration alone violates the contract "
                                  "even if the dep is optional/inactive and absent "
                                  "from the resolve graph")

            # D2: allowed workspace-internal deps
            if "allowed_module_deps" in rule:
                internal = deps & workspace_names
                extra = internal - set(rule["allowed_module_deps"]) - {module_name}
                if extra:
                    raise Violation(
                        "D2", f"{rid}: module {module_name} depends on disallowed "
                              f"workspace modules {sorted(extra)}")

            # D3: forbidden source tokens (comments stripped)
            tokens = rule.get("forbidden_source_tokens", [])
            if tokens:
                src_dir = crate_dir_of(module) / "src"
                for path in sorted(src_dir.rglob("*.rs")):
                    code = strip_comments(path.read_text(encoding="utf-8"))
                    for token in tokens:
                        if token in code:
                            raise Violation(
                                "D3", f"{rid}: forbidden token {token!r} in "
                                      f"{display_path(path)}")

            # D4: RunContext host-handle scan (DEP-06 style)
            scan = rule.get("source_scan")
            if scan and scan.get("symbol") == "RunContext":
                forbidden = scan["forbidden_field_types"]
                src_dir = crate_dir_of(module) / "src"
                for path in sorted(src_dir.rglob("*.rs")):
                    code = strip_comments(path.read_text(encoding="utf-8"))
                    m = re.search(r"pub struct RunContext\s*\{(.*?)\}", code, re.S)
                    if m:
                        body = m.group(1)
                        for token in forbidden:
                            if token in body:
                                raise Violation(
                                    "D4", f"{rid}: RunContext in "
                                          f"{display_path(path)} carries "
                                          f"host-handle type {token!r}")
            findings.append(f"rule {rid} OK for module {module_name}")

    # D5 reverse closure: every cargo workspace member must be a REGISTERED
    # module. cargo build --workspace compiles every member, so an
    # unregistered member would be built into the headless workspace while
    # no dependency rule (all keyed on registered applies_to modules) ever
    # inspects it -- DEP-07's declared workspace-wide desktop ban would go
    # unenforced. This closes the registry in both directions
    # (registry == workspace reality). It runs after the rule loop so a
    # member that also trips a specific rule (e.g. a D2 whitelist) is still
    # reported under that rule's id.
    unregistered = sorted(workspace_names - set(module_by_name))
    if unregistered:
        raise Violation(
            "D5", f"cargo workspace members missing from module_registry: "
                  f"{unregistered}; an unregistered member escapes every "
                  "applies_to-keyed dependency rule (DEP-07's workspace-wide "
                  "desktop ban included) while cargo build --workspace still "
                  "compiles it; register it deliberately with its dependency "
                  "rules made effective")
    findings.append("D5-reverse OK: every cargo workspace member is a "
                    "registered module")
    return findings


# ---------------------------------------------------------------------------
# Negative battery
# ---------------------------------------------------------------------------

def expect_rejection(name: str, expect_check: "str | tuple[str, ...]", fn) -> str:
    expected = (expect_check,) if isinstance(expect_check, str) else expect_check
    try:
        fn()
    except Violation as v:
        if v.check_id in expected:
            return f"{name}: REJECTED as required ({v})"
        raise AssertionError(f"{name}: rejected with wrong check id "
                             f"{v.check_id!r}, expected one of {expected!r}: {v}")
    raise AssertionError(f"{name}: NOT rejected (silent pass) - validator is broken")


def negative_battery(base_doc: dict, features: list[dict], stores: list[dict],
                     metadata: dict, rules_doc: dict) -> list[str]:
    results: list[str] = []

    # N1: dual owner on run_terminal_state
    def n1():
        doc = copy.deepcopy(base_doc)
        for fact in doc["critical_facts"]:
            if fact["fact_id"] == "run_terminal_state":
                fact["owners"] = ["kernel.run-supervisor", "adapters.storage"]
        check_ownership(doc, features, stores)
    results.append(expect_rejection("N1 dual-owner run_terminal_state", "O4", n1))

    # N2: sync bypass via auxiliary field
    def n2():
        doc = copy.deepcopy(base_doc)
        for fact in doc["critical_facts"]:
            if fact["fact_id"] == "run_terminal_state":
                fact["secondary_owner"] = "adapters.storage"
                fact["reconciliation"] = "二者同步"
        check_ownership(doc, features, stores)
    results.append(expect_rejection("N2 sync-bypass secondary_owner", "O4", n2))

    # N3: missing feature coverage
    def n3():
        doc = copy.deepcopy(base_doc)
        doc["feature_ownership"] = doc["feature_ownership"][1:]
        doc["counts"]["features"] -= 1
        check_ownership(doc, features, stores)
    results.append(expect_rejection("N3 missing-feature", "O1", n3))

    # N4: feature owned by a worker
    def n4():
        doc = copy.deepcopy(base_doc)
        doc["feature_ownership"][0]["target_owner"] = "worker.doc-parse"
        check_ownership(doc, features, stores)
    results.append(expect_rejection("N4 worker-owned feature", "O6", n4))

    # N5: critical fact owned by a worker
    def n5():
        doc = copy.deepcopy(base_doc)
        for fact in doc["critical_facts"]:
            if fact["fact_id"] == "run_terminal_state":
                fact["owners"] = ["worker.browser-engine"]
        check_ownership(doc, features, stores)
    results.append(expect_rejection("N5 worker-owned critical fact", "O5", n5))

    # N6: kernel depending on tauri (synthetic metadata)
    def n6():
        fake = copy.deepcopy(metadata)
        kernel_id = next(m for m in fake["workspace_members"] if "lingxi-kernel" in m)
        fake["packages"].append({
            "name": "tauri", "id": "registry+fake#tauri@2.0.0",
            "version": "2.0.0", "dependencies": [], "targets": [],
            "features": {}, "manifest_path": "/fake", "edition": "2021",
        })
        for node in fake["resolve"]["nodes"]:
            if node["id"] == kernel_id:
                node["deps"] = list(node["deps"]) + [{
                    "name": "tauri", "pkg": "registry+fake#tauri@2.0.0",
                    "dep_kinds": []}]
        fake["resolve"]["nodes"].append(
            {"id": "registry+fake#tauri@2.0.0", "deps": [], "features": []})
        check_dependencies(rules_doc, fake)
    results.append(expect_rejection("N6 kernel->tauri transitive dep", "D1", n6))

    # N7: RunContext carrying an AppHandle (synthetic crate copy in a temp dir)
    def n7():
        fake_rules = copy.deepcopy(rules_doc)
        with tempfile.TemporaryDirectory(prefix="r01-t01-neg7-") as td:
            crate = Path(td) / "lingxi-kernel"
            (crate / "src").mkdir(parents=True)
            (crate / "Cargo.toml").write_text(
                "[package]\nname = \"lingxi-kernel\"\nversion = \"0.0.0\"\n",
                encoding="utf-8")
            (crate / "src" / "lib.rs").write_text(
                "// RunContext must never carry a host handle\n"
                "pub struct RunContext {\n"
                "    pub app: AppHandle,\n"
                "}\n", encoding="utf-8")
            for module in fake_rules["module_registry"]:
                if module["module"] == "lingxi-kernel":
                    module["crate_path"] = str(crate)  # absolute, outside the repo
            check_dependencies(fake_rules, metadata)
    # D3 (forbidden source token) fires before D4 here; both are valid
    # rejections of a host handle entering kernel sources.
    results.append(expect_rejection("N7 RunContext carries AppHandle", ("D3", "D4"), n7))

    # N8: kernel directly depends on the hyphenated internal crate
    # lingxi-adapters. The dep is recorded in cargo's REAL output form:
    # deps[].name uses the underscore crate identifier ("lingxi_adapters")
    # while packages[].name keeps the hyphen ("lingxi-adapters"). The crate
    # is an external path dependency (own [workspace]), so it is not a
    # workspace member and the D5 dual check does not pre-empt D1.
    def n8():
        fake = copy.deepcopy(metadata)
        kernel_id = next(m for m in fake["workspace_members"] if "lingxi-kernel" in m)
        adapters_id = "path+file:///fake/lingxi-adapters#0.1.0"
        fake["packages"].append({"name": "lingxi-adapters", "id": adapters_id})
        fake["resolve"]["nodes"].append({"id": adapters_id, "deps": [], "features": []})
        for node in fake["resolve"]["nodes"]:
            if node["id"] == kernel_id:
                node["deps"] = list(node["deps"]) + [{
                    "name": "lingxi_adapters", "pkg": adapters_id, "dep_kinds": []}]
        check_dependencies(rules_doc, fake)
    results.append(expect_rejection(
        "N8 kernel->lingxi-adapters (hyphenated internal crate)", "D1", n8))

    # N9: the desktop stack reaches kernel TRANSITIVELY through the
    # hyphenated internal crate: kernel -> lingxi-adapters -> tauri.
    def n9():
        fake = copy.deepcopy(metadata)
        kernel_id = next(m for m in fake["workspace_members"] if "lingxi-kernel" in m)
        adapters_id = "path+file:///fake/lingxi-adapters#0.1.0"
        tauri_id = "path+file:///fake/tauri#2.0.0"
        fake["packages"].append({"name": "lingxi-adapters", "id": adapters_id})
        fake["packages"].append({"name": "tauri", "id": tauri_id})
        fake["resolve"]["nodes"].append({"id": adapters_id, "deps": [{
            "name": "tauri", "pkg": tauri_id, "dep_kinds": []}], "features": []})
        fake["resolve"]["nodes"].append({"id": tauri_id, "deps": [], "features": []})
        for node in fake["resolve"]["nodes"]:
            if node["id"] == kernel_id:
                node["deps"] = list(node["deps"]) + [{
                    "name": "lingxi_adapters", "pkg": adapters_id, "dep_kinds": []}]
        check_dependencies(rules_doc, fake)
    results.append(expect_rejection(
        "N9 kernel->lingxi-adapters->tauri (desktop via hyphenated crate)", "D1", n9))

    # N10: a forged workspace member must not slip past an
    # allowed_module_deps whitelist. The forged crate is a workspace member
    # but not a registered module, kernel depends on it, and a whitelist
    # rule for kernel allows only lingxi-protocol.
    def n10():
        fake = copy.deepcopy(metadata)
        fake_rules = copy.deepcopy(rules_doc)
        kernel_id = next(m for m in fake["workspace_members"] if "lingxi-kernel" in m)
        forged_id = "path+file:///fake/forged-internal#0.1.0"
        fake["packages"].append({"name": "forged-internal", "id": forged_id})
        fake["resolve"]["nodes"].append({"id": forged_id, "deps": [], "features": []})
        fake["workspace_members"] = list(fake["workspace_members"]) + [forged_id]
        for node in fake["resolve"]["nodes"]:
            if node["id"] == kernel_id:
                node["deps"] = list(node["deps"]) + [{
                    "name": "forged_internal", "pkg": forged_id, "dep_kinds": []}]
        fake_rules["dependency_rules"].append({
            "id": "DEP-T10",
            "name": "kernel-whitelist-negative-test",
            "applies_to_modules": ["lingxi-kernel"],
            "allowed_module_deps": ["lingxi-protocol"],
        })
        check_dependencies(fake_rules, fake)
    results.append(expect_rejection(
        "N10 forged workspace member vs allowed_module_deps whitelist", "D2", n10))

    # N11: an authoritative store mapped to a host owner/writer must be
    # rejected (rust-service is the single writer of authoritative data).
    def n11():
        doc = copy.deepcopy(base_doc)
        for row in doc["store_ownership"]:
            if row["store_id"] == "session-jsonl":
                assert row["classification"]["kind"] == "authoritative"
                row["target_owner"] = "tauri-host.desktop-host"
                row["target_writer_process"] = "tauri-host (仅写自身壳自态)"
        check_ownership(doc, features, stores)
    results.append(expect_rejection(
        "N11 authoritative store owned/written by tauri-host", "O8", n11))

    # N12: a "shadow" critical fact under a fresh fact_id (semantic duplicate
    # of run_terminal_state with a second owner) is outside the locked 02 §3
    # vocabulary and must be rejected.
    def n12():
        doc = copy.deepcopy(base_doc)
        doc["critical_facts"].append({
            "fact_id": "run_terminal_state_shadow",
            "description": "影子终态记录（换名第二写者）",
            "owners": ["adapters.storage"],
            "contract_ref": "02 §3",
        })
        check_ownership(doc, features, stores)
    results.append(expect_rejection(
        "N12 shadow critical fact outside locked 02 §3 set", "O4", n12))

    # N13: an UNREGISTERED workspace member (stealth-crate) carrying a
    # desktop dependency. No registered module depends on it, so every
    # applies_to-keyed rule is blind to it; only the D5 reverse closure
    # (workspace_members ⊆ module_registry) rejects it.
    def n13():
        fake = copy.deepcopy(metadata)
        stealth_id = "path+file:///fake/stealth-crate#0.1.0"
        tauri_id = "path+file:///fake/tauri#2.0.0"
        fake["packages"].append({"name": "stealth-crate", "id": stealth_id})
        fake["packages"].append({"name": "tauri", "id": tauri_id})
        fake["resolve"]["nodes"].append({"id": stealth_id, "deps": [{
            "name": "tauri", "pkg": tauri_id, "dep_kinds": []}],
            "features": []})
        fake["resolve"]["nodes"].append(
            {"id": tauri_id, "deps": [], "features": []})
        fake["workspace_members"] = list(fake["workspace_members"]) + [stealth_id]
        check_dependencies(rules_doc, fake)
    results.append(expect_rejection(
        "N13 unregistered workspace member (stealth-crate) with desktop dep",
        "D5", n13))

    # N14: a forbidden dependency DECLARED in the manifest but never
    # activated (optional, absent from the resolve graph). The transitive
    # closure cannot see it; the manifest-level declaration scan must.
    def n14():
        fake = copy.deepcopy(metadata)
        member_ids = set(fake["workspace_members"])
        for p in fake["packages"]:
            if p["name"] == "lingxi-kernel" and p["id"] in member_ids:
                p["dependencies"] = list(p.get("dependencies", [])) + [{
                    "name": "tauri", "source": None, "req": "*", "kind": None,
                    "optional": True, "uses_default_features": True,
                    "features": [], "target": None, "rename": None}]
        check_dependencies(rules_doc, fake)
    results.append(expect_rejection(
        "N14 kernel declares optional (inactive) tauri dep", "D1", n14))

    # N15: an authoritative store keeps an authority-kind owner (the O8
    # owner-kind half passes) but is written by a "rust-service-fork"
    # prefix look-alike. The writer must match the locked canonical string
    # exactly; a prefix match would let the shadow writer through.
    def n15():
        doc = copy.deepcopy(base_doc)
        for row in doc["store_ownership"]:
            if row["store_id"] == "session-jsonl":
                assert row["classification"]["kind"] == "authoritative"
                row["target_writer_process"] = "rust-service-fork (旁路写进程)"
        check_ownership(doc, features, stores)
    results.append(expect_rejection(
        "N15 authoritative store written by rust-service-fork shadow writer",
        "O8", n15))

    return results


# ---------------------------------------------------------------------------
# main
# ---------------------------------------------------------------------------

def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--ownership", default=str(DEFAULT_OWNERSHIP))
    ap.add_argument("--rules", default=str(DEFAULT_RULES))
    ap.add_argument("--skip-deps", action="store_true")
    ap.add_argument("--skip-drift", action="store_true")
    ap.add_argument("--self-test", action="store_true")
    ap.add_argument("--emit-negative-fixtures", metavar="DIR",
                    help="write the N1/N2 negative fixture files and exit")
    args = ap.parse_args()

    ownership_path = Path(args.ownership)
    doc = json.loads(ownership_path.read_text(encoding="utf-8"))
    rules_doc = json.loads(Path(args.rules).read_text(encoding="utf-8"))
    features = json.loads(FEATURE_INVENTORY.read_text(encoding="utf-8"))["features"]
    stores = json.loads(STORES.read_text(encoding="utf-8"))["stores"]

    if args.emit_negative_fixtures:
        out = Path(args.emit_negative_fixtures)
        out.mkdir(parents=True, exist_ok=True)
        n1 = copy.deepcopy(doc)
        for fact in n1["critical_facts"]:
            if fact["fact_id"] == "run_terminal_state":
                fact["owners"] = ["kernel.run-supervisor", "adapters.storage"]
        (out / "OWNERSHIP_TARGET.n1-dual-owner.json").write_text(
            json.dumps(n1, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
        n2 = copy.deepcopy(doc)
        for fact in n2["critical_facts"]:
            if fact["fact_id"] == "run_terminal_state":
                fact["secondary_owner"] = "adapters.storage"
                fact["reconciliation"] = "二者同步"
        (out / "OWNERSHIP_TARGET.n2-sync-bypass.json").write_text(
            json.dumps(n2, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
        print(f"emitted negative fixtures under {out}")
        return 0

    failures: list[str] = []
    try:
        for line in check_ownership(doc, features, stores):
            print(f"PASS {line}")
    except Violation as v:
        failures.append(str(v))
        print(f"FAIL {v}")

    if not args.skip_drift and ownership_path.resolve() == DEFAULT_OWNERSHIP.resolve():
        proc = subprocess.run(
            [sys.executable, "-B", str(GENERATOR), "--check"],
            capture_output=True, text=True, cwd=REPO_ROOT)
        if proc.returncode != 0:
            failures.append(f"[O7] generator drift: {proc.stdout.strip()}")
            print(f"FAIL [O7] generator drift: {proc.stdout.strip()}")
        else:
            print(f"PASS O7-drift OK: {proc.stdout.strip()}")

    metadata = None
    if not args.skip_deps:
        try:
            metadata = run_cargo_metadata()
            for line in check_dependencies(rules_doc, metadata):
                print(f"PASS {line}")
        except Violation as v:
            failures.append(str(v))
            print(f"FAIL {v}")

    if args.self_test:
        if metadata is None:
            metadata = run_cargo_metadata()
        try:
            for line in negative_battery(doc, features, stores, metadata, rules_doc):
                print(f"PASS-NEG {line}")
        except AssertionError as e:
            failures.append(str(e))
            print(f"FAIL-NEG {e}")

    if failures:
        print(f"RESULT: FAIL ({len(failures)} violation(s))")
        return 1
    print("RESULT: OK (ownership contract + dependency rules + negative battery)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
