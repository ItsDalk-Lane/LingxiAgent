#!/usr/bin/env python3
"""R00-T04 reproducible scan & validator.

Modes (default: all validations):
  --generate-stores   Regenerate STORES.json from shared/persistence/store-registry.ts
                      (registry is the single source; STORES.json adds T04 classification
                      overlay + cross-process writer facts only).
  --validate          Run A/B/C/D checks + negative self-checks, write
                      artifacts/rust-tauri/R00/T04/scan-output.json, exit non-zero on failure.

Checks:
  A. ENTRYPOINTS.json anchors: every {file,line,expect} must resolve (file exists,
     line in range, expect substring on that line).
  A2. ENTRYPOINTS.json counts: entrypoint_status_counts (active/dormant/residual/
     total) and entrypoint_category_counts ({active,total} per category) must
     equal mechanical recomputation from entries; status vocabulary is
     active/dormant/residual (as documented in anchor_semantics).
  B. HTTP route surface: extract literal route paths from server routes; every
     route factory referenced by composition/open-root.ts + full-root.ts must
     yield >=1 literal path; report total surface size.
  C. STORES.json <-> registry: 1:1 by id; ownerModule file exists; epochPolicy
     values constrained; classification consistent with epochPolicy.
  D. Independent persistence oracle (file level): production-root files containing
     persistence tokens (after comment stripping) must be covered by the committed
     inventory build/persistence-store-inventory.json (site sourceFile) — i.e. the
     registry census owns every writing file. Diff must be empty (or explicitly
     classified false-positive, which fails the run by default).
  E. Deterministic site sampling (A07 manual sample): N sites' excerpts must exist
     verbatim in their sourceFile.
Negative self-checks: tampered anchor / tampered counts block / injected fake
store id / uncovered fake write file must each make the corresponding check fail.
"""

import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
DOCS_R00 = ROOT / "docs" / "rust-tauri" / "R00"
ART_T04 = ROOT / "artifacts" / "rust-tauri" / "R00" / "T04"
INVENTORY_PATH = ROOT / "build" / "persistence-store-inventory.json"
ENTRYPOINTS_PATH = DOCS_R00 / "ENTRYPOINTS.json"
STORES_PATH = DOCS_R00 / "STORES.json"

PRODUCTION_ROOTS = ["server", "core", "hub", "lib", "shared", "plugins", "desktop", "cli"]
SOURCE_EXTENSIONS = {".ts", ".tsx", ".js", ".mjs", ".cjs"}
SOURCE_EXCLUSION_PATTERNS = [
    r"^desktop/(?:main|preload)[.]bundle[.]cjs$",
    r"^desktop/dist-(?:renderer|splash)(/|$)",
    r"^desktop/native(/|$)",
    r"^desktop/src/react(/|$)",
    r"^desktop/src/(?:lib|modules)(/|$)",
    r"^desktop/src/(?:browser-viewer-main|main|mobile-main|onboarding-main|quick-chat-main|settings-main|splash-main|viewer-window-entry)[.]tsx$",
    r"^desktop/src/(?:mobile-sw[.]js|viewer-resource-events[.]ts)$",
    r"/(?:tests|__tests__)(/|$)",
    r"[.](?:test|spec)[.](?:[cm]?[jt]sx?)$",
]
SOURCE_EXCLUSIONS = [re.compile(p) for p in SOURCE_EXCLUSION_PATTERNS]

# File-level oracle hit classifications (2026-09-24, HEAD ffcb85830). Every file
# that carries a persistence token but is absent from the committed census must
# appear here with a verified classification; a NEW uncovered file fails the run.
# subclassing:
#   substring-fp      — token matched inside an unrelated identifier/string
#   forwarding-facade — no direct fs call; the write lands in a registered module
#   di-receiver       — real write on an injected fs-like receiver (AST census
#                       blind spot, non-authoritative target)
ORACLE_UNCOVERED_CLASSIFICATION = {
    "core/session-inline-media-prune.ts": {
        "class": "substring-fp",
        "token_line": 47,
        "reason": "'sessionManager._rewriteFile()' 命中 'writeFile('；本文件无直接 fs 调用，实际写经 Pi SessionManager / core/session-jsonl-file.ts（后者已由 session-jsonl store 注册，inventory:4472）",
    },
    "desktop/mac-self-install.cjs": {
        "class": "di-receiver",
        "token_line": 290,
        "reason": "真实 writeFileSync，但接收器是注入的 fsImpl（AST 常谱只追踪直接 fs 绑定，故未收录）；目标为 os.tmpdir 一次性自装脚本（mkdtemp lingxi-self-install-*），不在 LINGXI_HOME、非权威、安装后清理。属常谱盲区类发现，见 R00-T04_REPORT.md 风险 R-T04-03",
    },
    "desktop/src/shared/server-readiness.cjs": {
        "class": "substring-fp",
        "token_line": 22,
        "reason": "'better-sqlite3' 是待存在性检查的模块名字符串，非 Database 打开",
    },
    "lib/sandbox/index.ts": {
        "class": "forwarding-facade",
        "token_line": 211,
        "reason": "lspTool 的 writeFile 转发 shim → createResourceIoToolOperations → LocalFsProvider；真实落盘在 lib/resource-io/providers/local-fs-provider.ts（已注册，store-registry.ts:1767）",
    },
    "lib/search/session-search-tokenizer.ts": {
        "class": "substring-fp",
        "token_line": 22,
        "reason": "'better-sqlite3 1000 eng' 为词元样例字符串，非 Database 打开",
    },
}


# File-level persistence token oracle. Mirrors the kinds the official scanner
# recognizes (scripts/scan-persistent-stores.mjs FS_METHOD_KINDS + sqlite opens),
# reduced to "this file performs some persistence-shaped call" granularity.
PERSISTENCE_TOKENS = [
    "writeFileSync", "writeFile(", "writeFile <", "promises.writeFile",
    "appendFileSync", "appendFile(", "promises.appendFile",
    "createWriteStream", "renameSync", "promises.rename",
    "copyFileSync", "promises.copyFile", "truncateSync",
    "rmSync", "unlinkSync", "mkdirSync", "promises.mkdir",
    "new Database(", "better-sqlite3",
]

# T04 classification overlay: subcategory + which processes touch the store.
# classification.kind is derived from the registry's own epochPolicy; only these
# curated facts are added here.
STORE_OVERLAY = {
    "data-epoch-stamp": {"subcategory": "epoch_meta", "processes": {"writers": ["server", "cli(data)"], "readers": ["server", "cli"]}},
    "data-epoch-transition-journal": {"subcategory": "epoch_meta", "processes": {"writers": ["server"], "readers": ["server", "cli"]}},
    "data-epoch-checkpoints": {"subcategory": "epoch_meta", "processes": {"writers": ["server"], "readers": ["cli(data)"]}},
    "data-epoch-restore-quarantine": {"subcategory": "logs_diagnostics", "processes": {"writers": ["cli(data)"], "readers": ["cli(data)"]}},
    "server-runtime-info": {"subcategory": "runtime_state", "processes": {
        "writers": ["server", "desktop(unlink-only)"],
        "readers": ["desktop", "cli"],
        "note": "desktop 仅做删除性 unlink，不创建/写入内容（4 个 remove-path 位点：stale/死内核探测清理 desktop/main.cjs:1364/1368、spawn 前清旧文件 :1915、shutdownServer 关停去留 :6635）；server-info.json 内容唯一写者为 server",
    }},
    "server-node-identity": {"subcategory": "user_data", "processes": {"writers": ["server"], "readers": ["server"]}},
    "user-studio-registries": {"subcategory": "user_data", "processes": {"writers": ["server"], "readers": ["server"]}},
    "local-user-auth": {"subcategory": "credentials", "processes": {"writers": ["server"], "readers": ["server"]}},
    "device-access-registries": {"subcategory": "credentials", "processes": {"writers": ["server"], "readers": ["server"]}},
    "server-network-config": {"subcategory": "user_data", "processes": {"writers": ["server"], "readers": ["server", "desktop"]}},
    "studio-mount-registry": {"subcategory": "user_data", "processes": {"writers": ["server"], "readers": ["server"]}},
    "web-session-registry": {"subcategory": "credentials", "processes": {"writers": ["server"], "readers": ["server"]}},
    "security-grants": {"subcategory": "credentials", "processes": {"writers": ["server"], "readers": ["server"]}},
    "execution-leases": {"subcategory": "runtime_state", "processes": {"writers": ["server"], "readers": ["server"]}},
    "security-key-material": {"subcategory": "credentials", "processes": {"writers": ["server"], "readers": ["server"]}},
    "security-audit-log": {"subcategory": "logs_diagnostics", "processes": {"writers": ["server"], "readers": ["server"]}},
    "user-preferences": {"subcategory": "user_data", "processes": {"writers": ["server"], "readers": ["server", "desktop"]}},
    "provider-state": {"subcategory": "config_credentials", "processes": {"writers": ["server"], "readers": ["server"]}},
    "agent-profile": {"subcategory": "user_data", "processes": {"writers": ["server"], "readers": ["server"]}},
    "agent-facts-sqlite": {"subcategory": "user_data", "processes": {"writers": ["server"], "readers": ["server"]}},
    "agent-memory": {"subcategory": "user_data", "processes": {"writers": ["server"], "readers": ["server"]}},
    "file-history-sqlite": {"subcategory": "user_data", "processes": {"writers": ["server"], "readers": ["server"]}},
    "session-manifest-sqlite": {"subcategory": "user_data", "processes": {"writers": ["server"], "readers": ["server"]}},
    "session-jsonl": {"subcategory": "user_data", "processes": {"writers": ["server"], "readers": ["server"]}},
    "session-sidecars": {"subcategory": "user_data", "processes": {"writers": ["server"], "readers": ["server"]}},
    "workspace-snapshots": {"subcategory": "cache", "processes": {"writers": ["server"], "readers": ["server"]}},
    "session-checkpoints": {"subcategory": "cache", "processes": {"writers": ["server"], "readers": ["server"]}},
    "ephemeral-scanner-scratch": {"subcategory": "cache", "processes": {"writers": ["server"], "readers": ["server"]}},
    "session-goal": {"subcategory": "cache", "processes": {"writers": ["server"], "readers": ["server"]}},
    "session-context-notes": {"subcategory": "cache", "processes": {"writers": ["server"], "readers": ["server"]}},
    "session-files": {"subcategory": "user_data", "processes": {"writers": ["server"], "readers": ["server"]}},
    "session-drafts-and-projects": {"subcategory": "user_data", "processes": {"writers": ["server"], "readers": ["server"]}},
    "conversation-map-layout": {"subcategory": "user_data", "processes": {"writers": ["server"], "readers": ["server"]}},
    "channels": {"subcategory": "user_data", "processes": {"writers": ["server"], "readers": ["server"]}},
    "desk-activity": {"subcategory": "user_data", "processes": {"writers": ["server"], "readers": ["server"]}},
    "cron-automation": {"subcategory": "user_data", "processes": {"writers": ["server"], "readers": ["server"]}},
    "agent-authored-records": {"subcategory": "user_data", "processes": {"writers": ["server"], "readers": ["server"]}},
    "agent-phone": {"subcategory": "user_data", "processes": {"writers": ["server"], "readers": ["server"]}},
    "workflow-state": {"subcategory": "runtime_state", "processes": {"writers": ["server"], "readers": ["server"]}},
    "subagent-state": {"subcategory": "runtime_state", "processes": {"writers": ["server"], "readers": ["server"]}},
    "plugin-task-registry": {"subcategory": "runtime_state", "processes": {"writers": ["server"], "readers": ["server"]}},
    "deferred-result-state": {"subcategory": "runtime_state", "processes": {"writers": ["server"], "readers": ["server"]}},
    "loop-state": {"subcategory": "runtime_state", "processes": {"writers": ["server"], "readers": ["server"]}},
    "terminal-session-state": {"subcategory": "runtime_state", "processes": {"writers": ["server"], "readers": ["server"]}},
    "skill-translation-cache": {"subcategory": "cache", "processes": {"writers": ["server"], "readers": ["server"]}},
    "knowledge-database": {"subcategory": "user_data", "processes": {"writers": ["server"], "readers": ["server"]}},
    "knowledge-source-snapshots": {"subcategory": "user_data", "processes": {"writers": ["server"], "readers": ["server"]}},
    "knowledge-parse-artifacts": {"subcategory": "derived_data", "processes": {"writers": ["server"], "readers": ["server"]}},
    "knowledge-processing-artifacts": {"subcategory": "derived_data", "processes": {"writers": ["server"], "readers": ["server"]}},
    "knowledge-indexes": {"subcategory": "cache", "processes": {"writers": ["server"], "readers": ["server"]}},
    "usage-ledger": {"subcategory": "user_data", "processes": {"writers": ["server"], "readers": ["server"]}},
    "model-observability-db": {"subcategory": "user_data", "processes": {"writers": ["server"], "readers": ["server"]}},
    "model-observability-blobs": {"subcategory": "derived_data", "processes": {"writers": ["server"], "readers": ["server"]}},
    "mcp-config": {"subcategory": "user_data", "processes": {"writers": ["server"], "readers": ["server"]}},
    "plugin-runtime-data": {"subcategory": "user_data", "processes": {"writers": ["server"], "readers": ["server"]}},
    "skill-state": {"subcategory": "user_data", "processes": {"writers": ["server"], "readers": ["server"]}},
    "operational-checkpoints": {"subcategory": "runtime_state", "processes": {"writers": ["server"], "readers": ["server"]}},
    "runtime-diagnostics": {"subcategory": "logs_diagnostics", "processes": {"writers": ["server"], "readers": ["server", "desktop"]}},
    "desktop-diagnostics": {"subcategory": "logs_diagnostics", "processes": {"writers": ["desktop"], "readers": ["desktop"]}},
    "desktop-gpu-startup-state": {"subcategory": "shell_state", "processes": {"writers": ["desktop"], "readers": ["desktop"]}},
    "desktop-win32-install-acl-heal-state": {"subcategory": "shell_state", "processes": {"writers": ["desktop"], "readers": ["desktop"]}},
    "desktop-window-version-state": {"subcategory": "shell_state", "processes": {"writers": ["desktop"], "readers": ["desktop"]}},
    "desktop-update-channel": {"subcategory": "shell_state", "processes": {"writers": ["desktop"], "readers": ["desktop"]}},
    "managed-runtime-caches": {"subcategory": "cache", "processes": {"writers": ["server"], "readers": ["server"]}},
    "signed-artifacts": {"subcategory": "shell_state", "processes": {"writers": ["desktop", "cli(bundle)"], "readers": ["desktop", "server", "cli"]}},
    "legacy-upload-cache": {"subcategory": "cache", "processes": {"writers": ["server"], "readers": ["server"]}},
    "character-card-staging": {"subcategory": "cache", "processes": {"writers": ["server"], "readers": ["server"]}},
    "desk-cover-upload-staging": {"subcategory": "cache", "processes": {"writers": ["server"], "readers": ["server"]}},
    "office-render-jobs": {"subcategory": "cache", "processes": {"writers": ["server"], "readers": ["server"]}},
}

CLASSIFICATION_BY_EPOCH_POLICY = {
    "epoch-managed": "authoritative",
    "regenerable": "rebuildable_cache",
    "compatible": "adjacent_compatible",
}


def fail(msg):
    print(f"FAIL: {msg}")
    sys.exit(1)


def dump_registry():
    code = (
        "import { PERSISTENT_STORES, PERSISTENCE_EXEMPTIONS } from "
        "'./shared/persistence/store-registry.ts';"
        " console.log(JSON.stringify({ stores: PERSISTENT_STORES, exemptions: PERSISTENCE_EXEMPTIONS }));"
    )
    proc = subprocess.run(
        ["node", "--input-type=module", "-e", code],
        cwd=ROOT, capture_output=True, text=True, timeout=120,
    )
    if proc.returncode != 0:
        fail(f"registry dump via node failed: {proc.stderr[:500]}")
    return json.loads(proc.stdout)


def generate_stores(registry):
    stores = []
    site_counts = {}
    inventory = json.loads(INVENTORY_PATH.read_text())
    for site in inventory.get("discoveredSites", []):
        if site.get("storeId"):
            site_counts[site["storeId"]] = site_counts.get(site["storeId"], 0) + 1
    for s in registry["stores"]:
        overlay = STORE_OVERLAY.get(s["id"], {})
        if not overlay:
            fail(f"STORE_OVERLAY missing store id: {s['id']}")
        kind = CLASSIFICATION_BY_EPOCH_POLICY.get(s.get("epochPolicy"))
        if kind is None:
            fail(f"unknown epochPolicy for {s['id']}: {s.get('epochPolicy')}")
        stores.append({
            "id": s["id"],
            "owner_module": s["ownerModule"],
            "path_patterns": s["pathPatterns"],
            "path_kind": s["pathKind"],
            "format": s["format"],
            "epoch_policy": s["epochPolicy"],
            "classification": {
                "kind": kind,
                "subcategory": overlay["subcategory"],
            },
            "schema": {
                "source_kind": s["schemaSource"]["kind"],
                "source": s["schemaSource"].get("module") or s["schemaSource"].get("packageName"),
                "contract": s["schemaSource"].get("contract"),
                "schema_contract_kind": s["schemaContract"]["kind"],
                "schema_contract_compatibility": s["schemaContract"].get("compatibility"),
            },
            "readers_writers": {
                "open_entry": s["openEntry"],
                "migration_entry": s["migrationEntry"],
                "protocol_modules": s["protocolModules"],
                "site_rule_files": sorted({r["sourceFile"] for r in s.get("siteRules", [])}),
            },
            "first_possible_open_phase": s["firstPossibleOpenPhase"],
            "first_possible_write_phase": s["firstPossibleWritePhase"],
            "processes": overlay["processes"],
            "epoch_checkpoint_policy": s["checkpointPolicy"],
            "epoch_restore_policy": s["restorePolicy"],
            "affected_by_epoch_migration": s["affectedByEpochMigration"],
            "identity_contract": s["identityContract"],
            "rebuild_or_loss_semantics": s["checkpointPolicy"] if kind == "rebuildable_cache" else None,
            "evidence": {
                "inventory_sites_owned": site_counts.get(s["id"], 0),
                "registry_enforced_by": "tests/persistence-store-registry.test.ts (owns every production persistence site exactly once)",
            },
        })
    doc = {
        "schema_version": 1,
        "task_id": "R00-T04",
        "generated_by": "docs/rust-tauri/R00/r00_t04_scan.py --generate-stores",
        "registry_source": "shared/persistence/store-registry.ts (PERSISTENT_STORES)",
        "registry_enforcement": [
            "tests/persistence-store-registry.test.ts — AST census: every production persistence site owned exactly once (store or exemption)",
            "tests/persistence-schema-tripwire.test.ts + scripts/check-persistence-schema-fingerprint.mjs — guarded persistence source fingerprints",
        ],
        "classification_semantics": {
            "authoritative": "epochPolicy=epoch-managed — 权威数据，受 data-epoch 契约管理，旧内核低于 stamp 最低读取代次时拒绝启动",
            "rebuildable_cache": "epochPolicy=regenerable — 可重建缓存；rebuild_or_loss_semantics 引用注册表声明的丢失/重建语义（可核实）",
            "adjacent_compatible": "epochPolicy=compatible — 与 epoch 契约并存的壳/观测/诊断状态，不由 epoch 迁移改写",
        },
        "counts": {
            "stores": len(stores),
            "by_classification": {},
            "by_subcategory": {},
            "registry_exemptions": len(registry["exemptions"]),
        },
        "stores": stores,
    }
    for st in stores:
        doc["counts"]["by_classification"][st["classification"]["kind"]] = \
            doc["counts"]["by_classification"].get(st["classification"]["kind"], 0) + 1
        doc["counts"]["by_subcategory"][st["classification"]["subcategory"]] = \
            doc["counts"]["by_subcategory"].get(st["classification"]["subcategory"], 0) + 1
    STORES_PATH.write_text(json.dumps(doc, ensure_ascii=False, indent=1) + "\n")
    print(f"STORES.json written: {len(stores)} stores, "
          f"classification={doc['counts']['by_classification']}")
    return doc


# ── A. anchor validation ──────────────────────────────────────────────────────

ENTRYPOINT_STATUSES = ("active", "dormant", "residual")


def validate_anchors(entrypoints, tamper=None):
    errors = []
    checked = 0
    for entry in entrypoints["entries"]:
        for anchor in entry.get("anchors", []):
            if tamper and anchor is tamper["anchor"]:
                line_no, expect = tamper["line"], tamper["expect"]
            else:
                line_no, expect = anchor["line"], anchor["expect"]
            rel = anchor["file"]
            path = ROOT / rel
            if not path.exists():
                errors.append(f"{entry['id']}: missing file {rel}")
                continue
            lines = path.read_text(errors="replace").splitlines()
            if not (1 <= line_no <= len(lines)):
                errors.append(f"{entry['id']}: {rel}:{line_no} out of range (file has {len(lines)} lines)")
                continue
            if expect not in lines[line_no - 1]:
                errors.append(f"{entry['id']}: {rel}:{line_no} expected {expect!r}, got: {lines[line_no - 1].strip()[:120]!r}")
            checked += 1
    return checked, errors


# ── A2. entrypoint counts consistency (R00-T04 R1-F02) ────────────────────────

def validate_entrypoint_counts(entrypoints):
    errors = []
    entries = entrypoints["entries"]
    for e in entries:
        if e.get("status") not in ENTRYPOINT_STATUSES:
            errors.append(f"{e.get('id')}: unknown status {e.get('status')!r} "
                          f"(vocabulary: {ENTRYPOINT_STATUSES})")
    status_counts = {"active": 0, "dormant": 0, "residual": 0}
    for e in entries:
        if e.get("status") in status_counts:
            status_counts[e["status"]] += 1
    status_counts["total"] = len(entries)
    declared_status = entrypoints.get("entrypoint_status_counts")
    if declared_status != status_counts:
        errors.append(f"entrypoint_status_counts mismatch: declared={declared_status} recomputed={status_counts}")
    per_cat = {}
    for e in entries:
        cat = per_cat.setdefault(e["category"], {"active": 0, "total": 0})
        cat["total"] += 1
        if e.get("status") == "active":
            cat["active"] += 1
    declared_cat = entrypoints.get("entrypoint_category_counts")
    if not isinstance(declared_cat, dict):
        errors.append("entrypoint_category_counts missing or not an object")
    else:
        if set(declared_cat) != set(per_cat):
            errors.append(f"entrypoint_category_counts category set mismatch: "
                          f"declared={sorted(declared_cat)} recomputed={sorted(per_cat)}")
        for cat in sorted(set(declared_cat) & set(per_cat)):
            if declared_cat[cat] != per_cat[cat]:
                errors.append(f"entrypoint_category_counts[{cat}] mismatch: "
                              f"declared={declared_cat[cat]} recomputed={per_cat[cat]}")
    return errors


# ── B. route surface ──────────────────────────────────────────────────────────

ROUTE_RE = re.compile(
    r"(?:app|sub|route|router|restRoute|wsRoute)\.(get|post|put|delete|patch|on|all)\(\s*[\"'`]([^\"'`]+)"
)


def extract_route_surface():
    routes = []
    route_files = sorted((ROOT / "server" / "routes").glob("*.ts"))
    targets = route_files + [ROOT / "server" / "index.ts"] + \
        sorted((ROOT / "server" / "composition").glob("*.ts"))
    for f in targets:
        for i, line in enumerate(f.read_text(errors="replace").splitlines(), 1):
            for m in ROUTE_RE.finditer(line):
                routes.append({"file": str(f.relative_to(ROOT)), "line": i,
                               "method": m.group(1).upper(), "path": m.group(2)})
    return routes


def validate_route_surface(routes, entrypoints):
    errors = []
    per_file = {}
    for r in routes:
        per_file.setdefault(r["file"], 0)
        per_file[r["file"]] += 1
    for f, count in sorted(per_file.items()):
        if count == 0:
            errors.append(f"route file with zero literal routes: {f}")
    # every composition-referenced factory file must contribute
    for comp in ["server/composition/open-root.ts", "server/composition/full-root.ts"]:
        text = (ROOT / comp).read_text()
        for m in re.finditer(r'from "\.\./routes/([a-z0-9-]+)\.ts"', text):
            rel = f"server/routes/{m.group(1)}.ts"
            if per_file.get(rel, 0) == 0:
                errors.append(f"{comp} references {rel} which yields no literal route")
    return per_file, errors


# ── C. stores vs registry ─────────────────────────────────────────────────────

def validate_stores(stores_doc, registry):
    errors = []
    doc_ids = [s["id"] for s in stores_doc["stores"]]
    reg_ids = [s["id"] for s in registry["stores"]]
    if sorted(doc_ids) != sorted(reg_ids):
        errors.append(f"STORES.json ids != registry ids: only-in-docs={sorted(set(doc_ids) - set(reg_ids))} "
                      f"only-in-registry={sorted(set(reg_ids) - set(doc_ids))}")
    if len(set(doc_ids)) != len(doc_ids):
        errors.append("STORES.json has duplicate ids")
    for s in stores_doc["stores"]:
        owner = ROOT / s["owner_module"]
        if not owner.exists():
            errors.append(f"{s['id']}: ownerModule missing: {s['owner_module']}")
        if s["classification"]["kind"] != CLASSIFICATION_BY_EPOCH_POLICY.get(s["epoch_policy"]):
            errors.append(f"{s['id']}: classification inconsistent with epochPolicy")
        if s["classification"]["kind"] == "rebuildable_cache" and not s.get("rebuild_or_loss_semantics"):
            errors.append(f"{s['id']}: regenerable store without rebuild semantics")
        for rf in s["readers_writers"]["site_rule_files"]:
            if not (ROOT / rf).exists():
                errors.append(f"{s['id']}: site rule file missing: {rf}")
    return errors


# ── D. independent persistence oracle ─────────────────────────────────────────

def strip_comments(text):
    out = []
    in_block = False
    for line in text.splitlines():
        stripped = line
        if in_block:
            end = stripped.find("*/")
            if end >= 0:
                stripped = stripped[end + 2:]
                in_block = False
            else:
                continue
        if stripped.lstrip().startswith("//") or stripped.lstrip().startswith("*"):
            continue
        if "/*" in stripped:
            in_block = True
            stripped = stripped.split("/*")[0]
        out.append(stripped)
    return "\n".join(out)


def oracle_persistence_files():
    hits = {}
    for rel_root in PRODUCTION_ROOTS:
        base = ROOT / rel_root
        if not base.exists():
            continue
        for dirpath, dirnames, filenames in os.walk(base):
            rel_dir = Path(dirpath).relative_to(ROOT).as_posix()
            dirnames[:] = [d for d in dirnames
                           if not any(p.search(f"{rel_dir}/{d}/") for p in SOURCE_EXCLUSIONS)
                           and d not in ("node_modules", "dist", ".cache")]
            for name in filenames:
                rel = f"{rel_dir}/{name}"
                if Path(name).suffix not in SOURCE_EXTENSIONS:
                    continue
                if any(p.search(rel) for p in SOURCE_EXCLUSIONS):
                    continue
                text = strip_comments((ROOT / rel).read_text(errors="replace"))
                matched = [t for t in PERSISTENCE_TOKENS if t in text]
                if matched:
                    hits[rel] = matched
    return hits


def validate_oracle_coverage(hits, extra_uncovered=None):
    inventory = json.loads(INVENTORY_PATH.read_text())
    covered = {site["sourceFile"] for site in inventory.get("discoveredSites", [])}
    uncovered = sorted(set(hits) - covered)
    # classified entries must remain: (a) actually token-hitting (else stale) and
    # (b) still absent from the census (else classification is stale).
    stale = [f for f in ORACLE_UNCOVERED_CLASSIFICATION if f not in hits]
    stale += [f for f in ORACLE_UNCOVERED_CLASSIFICATION if f in covered]
    unclassified = [f for f in uncovered if f not in ORACLE_UNCOVERED_CLASSIFICATION]
    if extra_uncovered:
        unclassified = unclassified + [extra_uncovered]
    return covered, sorted(unclassified), sorted(stale)


# ── E. deterministic sampling ─────────────────────────────────────────────────

def sample_sites(n=8):
    inventory = json.loads(INVENTORY_PATH.read_text())
    sites = inventory["discoveredSites"]
    step = max(1, len(sites) // n)
    picked = [sites[i] for i in range(0, len(sites), step)][:n]
    errors = []
    for site in picked:
        path = ROOT / site["sourceFile"]
        if not path.exists():
            errors.append(f"sample site file missing: {site['sourceFile']}")
            continue
        text = path.read_text(errors="replace")
        if site["excerpt"] not in text:
            errors.append(f"sample excerpt not found in {site['sourceFile']}: {site['excerpt'][:80]}")
        if not site["storeId"] and not site["exemptionId"]:
            errors.append(f"sample site has neither storeId nor exemptionId: {site}")
    return picked, errors


# ── main ──────────────────────────────────────────────────────────────────────

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--generate-stores", action="store_true")
    ap.add_argument("--validate", action="store_true")
    args = ap.parse_args()

    if not (args.generate_stores or args.validate):
        args.generate_stores = True
        args.validate = True

    registry = dump_registry()

    if args.generate_stores:
        generate_stores(registry)

    if not args.validate:
        return

    ART_T04.mkdir(parents=True, exist_ok=True)
    report = {"mode": "validate", "checks": {}}
    fatal = []

    entrypoints = json.loads(ENTRYPOINTS_PATH.read_text())
    stores_doc = json.loads(STORES_PATH.read_text())

    # A. anchors
    checked, errors = validate_anchors(entrypoints)
    report["checks"]["A_anchors"] = {"anchors_checked": checked, "errors": errors}
    fatal += [f"A: {e}" for e in errors]

    # A2. entrypoint counts vs entries (R1-F02 guardrail)
    errors = validate_entrypoint_counts(entrypoints)
    report["checks"]["A2_entrypoint_counts"] = {"errors": errors}
    fatal += [f"A2: {e}" for e in errors]

    # B. routes
    routes = extract_route_surface()
    per_file, errors = validate_route_surface(routes, entrypoints)
    literal_paths = sorted({r["path"] for r in routes})
    report["checks"]["B_routes"] = {
        "route_sites": len(routes),
        "unique_literal_paths": len(literal_paths),
        "files_with_routes": len(per_file),
        "errors": errors,
    }
    fatal += [f"B: {e}" for e in errors]

    # C. stores vs registry
    errors = validate_stores(stores_doc, registry)
    report["checks"]["C_stores"] = {
        "doc_store_count": len(stores_doc["stores"]),
        "registry_store_count": len(registry["stores"]),
        "errors": errors,
    }
    fatal += [f"C: {e}" for e in errors]

    # D. oracle coverage
    hits = oracle_persistence_files()
    covered, unclassified, stale = validate_oracle_coverage(hits)
    report["checks"]["D_oracle"] = {
        "files_with_persistence_tokens": len(hits),
        "inventory_covered_files": len(covered),
        "classified_uncovered": {f: ORACLE_UNCOVERED_CLASSIFICATION[f] for f in sorted(ORACLE_UNCOVERED_CLASSIFICATION) if f in hits},
        "unclassified_uncovered_files": unclassified,
        "stale_classifications": stale,
    }
    if unclassified:
        fatal.append(f"D: {len(unclassified)} production files with persistence tokens not owned by census or classification: {unclassified[:10]}")
    if stale:
        fatal.append(f"D: stale oracle classifications (file no longer hits / became census-covered): {stale}")

    # E. sampling
    picked, errors = sample_sites()
    report["checks"]["E_sampling"] = {
        "samples": [{"file": s["sourceFile"], "storeId": s["storeId"], "exemptionId": s["exemptionId"], "kind": s["kind"]} for s in picked],
        "errors": errors,
    }
    fatal += [f"E: {e}" for e in errors]

    # Negative self-checks
    neg = {}
    first_anchor = entrypoints["entries"][0]["anchors"][0]
    _, tamper_errors = validate_anchors(
        entrypoints, tamper={"anchor": first_anchor, "line": first_anchor["line"], "expect": "ZZZ_NOT_PRESENT_ZZZ"})
    neg["tampered_anchor_detected"] = bool(tamper_errors)
    tampered_counts_doc = dict(entrypoints)
    tampered_counts_doc["entrypoint_category_counts"] = {
        cat: ({"active": vals["active"] + 1, "total": vals["total"]} if cat == "plugins_mcp" else dict(vals))
        for cat, vals in entrypoints["entrypoint_category_counts"].items()
    }
    neg["tampered_counts_detected"] = bool(validate_entrypoint_counts(tampered_counts_doc))
    fake_doc = {"stores": stores_doc["stores"] + [{"id": "fake-store-xyz", "owner_module": "server/does-not-exist.ts",
                                                    "path_patterns": ["fake.json"], "path_kind": "file", "format": "json",
                                                    "epoch_policy": "epoch-managed",
                                                    "classification": {"kind": "authoritative", "subcategory": "user_data"},
                                                    "schema": {}, "readers_writers": {"site_rule_files": []},
                                                    "rebuild_or_loss_semantics": None}]}
    neg["fake_store_detected"] = bool(validate_stores(fake_doc, registry))
    _, unc, _ = validate_oracle_coverage(hits, extra_uncovered="server/routes/__fake_write__.ts")
    neg["fake_uncovered_detected"] = len(unc) > 0
    report["negative_self_checks"] = neg
    if not all(neg.values()):
        fatal.append(f"N: negative self-checks did not all trigger: {neg}")

    report["fatal"] = fatal
    out_path = ART_T04 / "scan-output.json"
    out_path.write_text(json.dumps(report, ensure_ascii=False, indent=1) + "\n")
    print(json.dumps({
        "A_anchors": report["checks"]["A_anchors"]["anchors_checked"],
        "A2_counts_errors": len(report["checks"]["A2_entrypoint_counts"]["errors"]),
        "B_routes": report["checks"]["B_routes"]["unique_literal_paths"],
        "C_stores": report["checks"]["C_stores"]["doc_store_count"],
        "D_unclassified": len(report["checks"]["D_oracle"]["unclassified_uncovered_files"]),
        "D_stale": len(report["checks"]["D_oracle"]["stale_classifications"]),
        "D_token_files": report["checks"]["D_oracle"]["files_with_persistence_tokens"],
        "E_samples": len(report["checks"]["E_sampling"]["samples"]),
        "negative": neg,
        "fatal_count": len(fatal),
    }, ensure_ascii=False, indent=1))
    if fatal:
        for f in fatal[:20]:
            print("FATAL:", f)
        sys.exit(1)
    print("R00_T04_SCAN_OK")


if __name__ == "__main__":
    main()
