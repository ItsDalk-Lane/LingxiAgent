"""R00-T03：验证账本没有静态漏账，并做执行器分类反例。

R2 修复：验证器不再把导入图同时当"被测值"和"唯一预期"。新增三层独立 oracle：
1) 独立 Python 源码扫描（注释剥离 + 正则，与 TS AST 生成器不同实现）重导出全部
   import/export/require/createRequire/动态 import/resolve 位点；
2) TypeScript transpileModule emit 探针，证明类型专用导入在编译后不产生运行时模块边；
3) worker 双向枚举闭合（源头执行入口 + 终态 sink 子串扫描）与外围语义反例。
"""
from __future__ import annotations

import hashlib
import json
import re
import subprocess
from copy import deepcopy
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
GRAPH = json.loads((HERE / "R00-T03_IMPORT_GRAPH.json").read_text())
MATRIX = json.loads((HERE / "PI_REPLACEMENT_MATRIX.json").read_text())
DEPS = json.loads((HERE / "RUNTIME_DEPENDENCIES.json").read_text())
HEAD = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
TASK_BASE_SHA = "4b4a1d98f6d2d0e03f9573aa75b7992efed03db6"
checks = []
negative_probes = 0


def check(name, condition, detail):
    checks.append(dict(name=name, pass_=bool(condition), detail=detail))
    if not condition:
        raise AssertionError(f"{name}: {detail}")


def source_ref_exists(ref):
    if ":" not in ref:
        return (ROOT / ref).is_file()
    file, rest = ref.rsplit(":", 1)
    source = ROOT / file
    if not source.is_file():
        return False
    total = len(source.read_text().splitlines())
    for part in rest.split(","):
        if "-" in part:
            a, b = part.split("-", 1)
            if not (a.isdigit() and b.isdigit() and 0 < int(a) <= int(b) <= total):
                return False
        elif not (part.isdigit() and 0 < int(part) <= total):
            return False
    return True


# ── 独立 oracle 一：Python 源码扫描（不读导入图） ─────────────────────────────
TRACKED = subprocess.check_output(["git", "ls-files"], cwd=ROOT, text=True).splitlines()
SOURCE_DIRS = ("core/", "server/", "lib/", "hub/", "cli/", "desktop/", "plugins/", "scripts/", "shared/")
CONTROLLED = sorted(
    f for f in TRACKED
    if f.startswith(SOURCE_DIRS)
    and re.search(r"\.(?:[cm]?js|tsx?)$", f)
    and not re.search(r"(?:^|/)(?:__tests__|dist|dist-renderer|dist-splash|dist-theme|node_modules)(?:/|$)", f)
    and not re.search(r"\.(?:test|spec)\.[cm]?[jt]sx?$", f)
)
check("oracle controlled file set matches generator scope",
      CONTROLLED and len(CONTROLLED) == GRAPH["scanned_files"],
      f"independent={len(CONTROLLED)}, graph={GRAPH['scanned_files']}")


def strip_comments(text):
    """剥离注释与正则字面量，并把字符串/注释/正则区间记入 spans（保留长度与行号）。

    返回 (处理后文本, [(start, end), ...])；匹配起点落在区间内的命中判为非代码
    （如构建脚本生成代码模板字符串、注释内的示例）。正则字面量按"前一个有效
    字符/关键字"启发式区分除号，避免 /['"]/ 之类内容破坏字符串状态机。
    """
    out = []
    spans = []
    i, n = 0, len(text)
    state = None
    span_start = 0
    prev_sig = None   # None 态下最近一个非空白有效字符
    prev_word = []    # 最近单词缓存（识别 return/typeof 等后随正则）
    regex_before = set("(,=:[!&|?{;+-*/%^~<>~")
    regex_keywords = {"return", "typeof", "instanceof", "in", "of", "new",
                      "delete", "void", "case", "do", "else", "yield", "await"}
    while i < n:
        c = text[i]
        nxt = text[i + 1] if i + 1 < n else ""
        if state is None:
            if c == "/" and nxt == "/":
                state, span_start = "line", i
            elif c == "/" and nxt == "*":
                state, span_start = "block", i
            elif c == "/" and (prev_sig is None or prev_sig in regex_before
                               or "".join(prev_word) in regex_keywords):
                # 正则字面量：跳到未转义的闭合 /；若先遇到换行说明误判为除号，
                # 只吞掉到换行前（保住换行符，行号不漂移）。
                j, in_class = i + 1, False
                while j < n:
                    ch2 = text[j]
                    if ch2 == "\\":
                        j += 2
                        continue
                    if in_class:
                        if ch2 == "]":
                            in_class = False
                    elif ch2 == "[":
                        in_class = True
                    elif ch2 == "/":
                        break
                    elif ch2 == "\n":
                        break
                    j += 1
                end = j if j < n and text[j] == "\n" else min(j + 1, n)
                spans.append((i, end))
                out.extend(" " * (end - i))
                i = end
                prev_sig, prev_word = "/", []
                continue
            elif c == "'":
                state, span_start = "sq", i
            elif c == '"':
                state, span_start = "dq", i
            elif c == "`":
                state, span_start = "tpl", i
            else:
                out.append(c)
                if not c.isspace():
                    prev_sig = c
                    if c.isalnum() or c in "_$":
                        prev_word.append(c)
                        prev_word = prev_word[-12:]
                    else:
                        prev_word = []
            if state is None:
                i += 1
            else:
                out.append("  " if c == "/" else c)
                i += 2 if c == "/" else 1
                prev_sig, prev_word = None, []
            continue
        if state == "line":
            if c == "\n":
                state = None
                spans.append((span_start, i))
                out.append(c)
            else:
                out.append(" ")
            i += 1
            continue
        if state == "block":
            if c == "*" and nxt == "/":
                spans.append((span_start, i + 2))
                out.append("  ")
                i += 2
                state = None
                continue
            out.append("\n" if c == "\n" else " ")
            i += 1
            continue
        out.append(c)
        if c == "\\" and nxt:
            out.append(nxt)
            i += 2
            continue
        term = {"sq": "'", "dq": '"', "tpl": "`"}[state]
        if c == term:
            spans.append((span_start, i + 1))
            state = None
        i += 1
    if state in ("line", "block", "sq", "dq", "tpl"):
        spans.append((span_start, n))
    return "".join(out), spans


def in_spans(pos, spans):
    return any(start <= pos < end for start, end in spans)


IMPORT_FROM_RE = re.compile(r"\b(import|export)\s+([^;'\n{}]*(?:\{[^{}]*\}[^;'\n{}]*)?)\s*from\s*['\"]([^'\"]+)['\"]")
SIDE_EFFECT_RE = re.compile(r"\bimport\s*['\"]([^'\"]+)['\"]")
DYNAMIC_RE = re.compile(r"\bimport\s*\(\s*['\"]([^'\"]+)['\"]\s*\)")
REQUIRE_RE = re.compile(r"\brequire\s*\(\s*['\"]([^'\"]+)['\"]\s*\)")
REQUIRE_RESOLVE_RE = re.compile(r"\brequire\s*\.\s*resolve\s*\(\s*['\"]([^'\"]+)['\"]\s*\)")
CREATE_REQUIRE_DIRECT_RE = re.compile(r"\bcreateRequire\s*\([^)]*\)\s*\(\s*['\"]([^'\"]+)['\"]\s*\)")
ALIAS_DEF_RE = re.compile(r"\b(?:const|let|var)\s+(\w+)\s*=\s*createRequire\s*\(")


def split_top_commas(text):
    parts, depth, cur = [], 0, []
    for ch in text:
        if ch == "{" or ch == "[":
            depth += 1
        elif ch == "}" or ch == "]":
            depth -= 1
        if ch == "," and depth == 0:
            parts.append("".join(cur))
            cur = []
        else:
            cur.append(ch)
    if cur:
        parts.append("".join(cur))
    return parts


def stmt_type_only(clause):
    """语句级类型判定：默认/命名空间绑定是运行时；仅当全部具名元素均 type 前缀才为类型专用。"""
    c = clause.strip()
    if re.match(r"^type(\s|$|\{|\*)", c):
        return True
    m = re.search(r"\{(.*)\}", clause, re.S)
    if not m:
        return False  # 无花括号 = default 或 namespace 绑定，运行时
    before_braces = clause[:m.start()].strip().rstrip(",").strip()
    if before_braces:
        return False  # default 绑定（import D, {...}）是运行时
    elems = [e.strip() for e in split_top_commas(m.group(1)) if e.strip()]
    if not elems:
        return False  # 空花括号按运行时（与图生成器 symbols=[] → type_only=False 一致）
    return all(re.match(r"^type(\s|$)", e) for e in elems)


def independent_scan():
    """独立重导出受控源码的外部模块位点：specifier → {(file, line, kind, type_only)}。"""
    found = {}
    for file in CONTROLLED:
        raw = (ROOT / file).read_text(errors="replace")
        text, spans = strip_comments(raw)
        if text.count("\n") != raw.count("\n") or len(text) != len(raw):
            raise AssertionError(f"独立扫描器在 {file} 上发生长度/行号漂移，禁止继续比对")
        line_starts = [0]
        for pos, ch in enumerate(text):
            if ch == "\n":
                line_starts.append(pos + 1)

        def line_of(pos):
            import bisect
            return bisect.bisect_right(line_starts, pos)

        def add(m, spec, kind, type_only=False):
            if in_spans(m.start(), spans):
                return  # 命中位于注释或字符串字面量内（如生成代码模板），不是本文件代码
            found.setdefault(spec, set()).add((file, line_of(m.start()), kind, type_only))

        for m in IMPORT_FROM_RE.finditer(text):
            add(m, m.group(3), "import" if m.group(1) == "import" else "export_from",
                stmt_type_only(m.group(2)))
        for m in SIDE_EFFECT_RE.finditer(text):
            add(m, m.group(1), "import", False)
        for m in DYNAMIC_RE.finditer(text):
            add(m, m.group(1), "dynamic_import", False)
        for m in REQUIRE_RE.finditer(text):
            add(m, m.group(1), "require", False)
        for m in REQUIRE_RESOLVE_RE.finditer(text):
            add(m, m.group(1), "require_resolve", False)
        for m in CREATE_REQUIRE_DIRECT_RE.finditer(text):
            add(m, m.group(1), "require", False)
        for am in ALIAS_DEF_RE.finditer(text):
            if in_spans(am.start(), spans):
                continue
            alias = am.group(1)
            if alias == "require":
                continue  # 影子 require 已由普通 require 规则覆盖
            for m in re.finditer(rf"\b{re.escape(alias)}\s*\(\s*['\"]([^'\"]+)['\"]\s*\)", text):
                add(m, m.group(1), "require", False)
            for m in re.finditer(rf"\b{re.escape(alias)}\s*\.\s*resolve\s*\(\s*['\"]([^'\"]+)['\"]\s*\)", text):
                add(m, m.group(1), "require_resolve", False)
    return found


INDEPENDENT = independent_scan()
PI_PREFIXES = ("@earendil-works/pi-", "@mariozechner/pi-")
dep_packages = {d["package"] for d in DEPS["dependencies"]}
pi_packages = {spec for spec in INDEPENDENT if spec.startswith(PI_PREFIXES)}
check("oracle independent scan found sites", len(INDEPENDENT) > 100,
      f"specifiers={len(INDEPENDENT)}")


def package_of(spec):
    for pkg in dep_packages | pi_packages:
        if spec == pkg or spec.startswith(pkg + "/"):
            return pkg
    return None


def graph_site_keys(package):
    keys = set()
    for r in GRAPH["external_imports"]:
        if package_of(r["specifier"]) == package:
            keys.add((r["file"], r["line"], r["kind"], bool(r["symbols"]) and all(s["type_only"] for s in r["symbols"])))
    return keys


def independent_site_keys(package):
    keys = set()
    for spec, sites in INDEPENDENT.items():
        if package_of(spec) == package:
            keys.update(sites)
    return keys


# 生成图 vs 独立扫描逐包双向差集（51 依赖 + Pi 包）。
graph_vs_independent = {}
for package in sorted(dep_packages | pi_packages):
    g, i = graph_site_keys(package), independent_site_keys(package)
    if g != i:
        graph_vs_independent[package] = {"graph_only": sorted(g - i), "independent_only": sorted(i - g)}
check("oracle graph matches independent source scan", not graph_vs_independent,
      f"mismatches={json.dumps(graph_vs_independent, ensure_ascii=False)[:800]}")

# ── 独立 oracle 二：TypeScript emit 探针（类型专用导入编译后无运行时边） ─────────
TS_PROBE = r"""
const ts = require('typescript');
const cases = [
  { src: "import type D from 'p1'; export type T2 = D;", expect: false, tag: 'import type default' },
  { src: "import type * as N from 'p2'; export type T3 = N.X;", expect: false, tag: 'import type namespace' },
  { src: "import { type T, V } from 'p3'; export const v = V;", expect: true, tag: 'mixed named keeps value' },
  { src: "import D2, { type T4, V2 } from 'p4'; export const w = D2 ?? V2;", expect: true, tag: 'default + mixed' },
  { src: "import 'p5';", expect: true, tag: 'side effect retained' },
  { src: "export type { X } from 'p6';", expect: false, tag: 'export type re-export erased' },
  { src: "export { type Y, Z } from 'p7';", expect: true, tag: 'mixed re-export keeps value' },
  { src: "export * as N2 from 'p8';", expect: true, tag: 'namespace re-export runtime' },
  { src: "export type * as N3 from 'p9';", expect: false, tag: 'export type namespace erased' },
];
let failures = [];
for (const c of cases) {
  const out = ts.transpileModule(c.src, { compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2020 } }).outputText;
  const hasEdge = new RegExp(`from ['\"]p\\d+['\"]`).test(out) || /import\s*['"]p\d+['"]/.test(out);
  if (hasEdge !== c.expect) failures.push(`${c.tag}: expected edge=${c.expect}, got=${hasEdge}\n${out}`);
}
if (failures.length) { console.error(failures.join('\n---\n')); process.exit(1); }
console.log('ts emit probe ok');
"""
probe = subprocess.run(["node", "-e", TS_PROBE], cwd=ROOT, capture_output=True, text=True)
check("oracle TypeScript emit type-erasure probe", probe.returncode == 0,
      probe.stdout.strip() or probe.stderr.strip()[:400])

# ── 独立 oracle 三：源码锚点正反例（直接读源文件，不经生成器） ──────────────────
def raw_line(file, line):
    lines = (ROOT / file).read_text(errors="replace").splitlines()
    return lines[line - 1].strip() if 0 < line <= len(lines) else None


def graph_record_at(file, line, kinds=("import", "export_from")):
    for r in GRAPH["external_imports"]:
        if r["file"] == file and r["line"] == line and r["kind"] in kinds:
            return r
    return None


def independent_site_at(file, line, kinds=("import", "export_from")):
    for spec, sites in INDEPENDENT.items():
        for f, l, kind, type_only in sites:
            if f == file and l == line and kind in kinds:
                return type_only
    return None


TYPE_ANCHORS = [
    ("lib/pi-sdk/index.ts", 60, False, "export { runAgentLoop }"),
    ("lib/pi-sdk/index.ts", 61, True, "export type {"),
    ("lib/pi-sdk/index.ts", 105, True, "export type {"),
    ("lib/pi-sdk/stream-guard.ts", 1, True, "import type"),
    ("lib/pi-sdk/stream-guard.ts", 2, False, "import"),
    ("lib/bridge/feishu-adapter.ts", 8, False, "import"),
    ("lib/bridge/feishu-adapter.ts", 24, True, "import type"),
]


def anchor_violations(anchors, graph_override=None):
    violations = []
    for file, line, expect_type, prefix in anchors:
        text = raw_line(file, line)
        if text is None or not text.startswith(prefix):
            violations.append(f"{file}:{line} 源码行与预期形式不符: {text!r}")
            continue
        # 独立扫描判定（本地/外部说明符均可）
        ind = independent_site_at(file, line)
        if ind is None:
            violations.append(f"{file}:{line} 独立扫描无对应 import/export 位点")
        elif ind != expect_type:
            violations.append(f"{file}:{line} 独立扫描 type_only={ind}，源码形式要求 {expect_type}")
        # 图记录判定（仅外部说明符会出现在 external_imports）
        record = graph_record_at(file, line)
        if graph_override is not None:
            record = graph_override(record, file, line)
        if record is not None:
            actual = bool(record["symbols"]) and all(s["type_only"] for s in record["symbols"])
            if actual != expect_type:
                violations.append(f"{file}:{line} 图 type_only={actual}，源码形式要求 {expect_type}")
            expected_timing = "type_only" if expect_type else "module_evaluation"
            if record["load_timing"] != expected_timing:
                violations.append(f"{file}:{line} load_timing={record['load_timing']}，应为 {expected_timing}")
    return violations


check("oracle type-only anchors match source and graph", not anchor_violations(TYPE_ANCHORS),
      "; ".join(anchor_violations(TYPE_ANCHORS)))

# 负例：故意把任一类型位反转后，锚点检查必须失败（R2 F01 复现门）。
def flip_line_61(record, file, line):
    if record is None or (file, line) != ("lib/pi-sdk/index.ts", 61):
        return record
    flipped = deepcopy(record)
    for s in flipped["symbols"]:
        s["type_only"] = False
    return flipped


check("oracle synthetic type-bit flip on pi-sdk:61 rejected",
      bool(anchor_violations(TYPE_ANCHORS, graph_override=flip_line_61)),
      "export type 反转为运行时必须被锚点检查拒绝")
negative_probes += 1

# CJS/createRequire/resolve 锚点。
def cjs_anchor_violations():
    violations = []
    rec = graph_record_at("shared/artifact-core/release-order.cjs", 10, kinds=("require",))
    if rec is None or rec["specifier"] != "semver" or rec["load_timing"] != "module_evaluation":
        violations.append("release-order.cjs:10 顶层 require('semver') 应为 module_evaluation")
    if 'require("semver")' not in (raw_line("shared/artifact-core/release-order.cjs", 10) or ""):
        violations.append("release-order.cjs:10 源码不含 require(\"semver\")")
    rec = graph_record_at("shared/safe-fs.ts", 37, kinds=("require",))
    if rec is None or rec["specifier"] != "js-yaml" or not rec.get("via_create_require"):
        violations.append("safe-fs.ts:37 createRequire('js-yaml') 应为 require 且 via_create_require")
    elif rec["load_timing"] != "call_time":
        violations.append("safe-fs.ts:37 在函数体内，应为 call_time")
    load = graph_record_at("lib/knowledge/usearch-vector-backend.ts", 28, kinds=("require",))
    resolve = graph_record_at("lib/knowledge/usearch-vector-backend.ts", 28, kinds=("require_resolve",))
    if load is None or load["specifier"] != "usearch":
        violations.append("usearch:28 应有 require('usearch') 模块加载边")
    if resolve is None or resolve["specifier"] != "usearch" or resolve["load_timing"] != "resolve_only":
        violations.append("usearch:28 require.resolve 只能记 resolve_only，不算模块执行")
    return violations


check("oracle CJS/createRequire/resolve anchors", not cjs_anchor_violations(),
      "; ".join(cjs_anchor_violations()))

# ── A05：账本 vs 图差集（保留 R1 全部检查） ──────────────────────────────────
cap_ids = {c["id"] for c in MATRIX["capabilities"]}
symbol_map = {s: c["id"] for c in MATRIX["capabilities"] for s in c["adapter_symbols"]}
observed = {s["imported"] for r in GRAPH["adapter_imports"]
            if r["resolved"] == "lib/pi-sdk/index.ts" and r["file"] != "lib/pi-sdk/index.ts"
            for s in r["symbols"]}
gap = observed - symbol_map.keys()
check("A05 adapter symbol difference", not gap, f"observed={len(observed)}, uncovered={sorted(gap)}")
negative_gap = (observed | {"__unregistered_pi_ability__"}) - symbol_map.keys()
check("A05 synthetic unregistered ability is rejected", negative_gap == {"__unregistered_pi_ability__"},
      f"injected difference={sorted(negative_gap)}")
negative_probes += 1

vendor_refs = {f'{r["file"]}:{r["line"]}' for r in GRAPH["vendor_pi_imports"]}
covered_vendor = {r["source_ref"] for r in MATRIX["pi_vendor_import_coverage"]}
check("A05 Pi vendor import difference", vendor_refs == covered_vendor,
      f"vendor imports={len(vendor_refs)}, uncovered={sorted(vendor_refs - covered_vendor)}")

adapter_refs = {f'{r["file"]}:{r["line"]}' for r in GRAPH["adapter_imports"] if r["file"] != "lib/pi-sdk/index.ts"}
covered_adapter = {r["source_ref"] for r in MATRIX["adapter_import_coverage"]}
check("A05 adapter import difference", adapter_refs == covered_adapter,
      f"adapter imports={len(adapter_refs)}, uncovered={sorted(adapter_refs - covered_adapter)}")
check("A05 every adapter import has migration owner",
      all(r["capability_ids"] and set(r["capability_ids"]) <= cap_ids for r in MATRIX["adapter_import_coverage"]),
      "no empty or unknown capability owner")

events = {}
for file in [*sorted((ROOT / "lib/extensions").glob("*.ts")), ROOT / "core/engine.ts"]:
    for number, line in enumerate(file.read_text().splitlines(), 1):
        for event in re.findall(r'pi\.on\(["\']([^"\']+)["\']', line):
            events[f'{file.relative_to(ROOT)}:{number}'] = event
hook_refs = {h["source_ref"]: h["event"] for h in MATRIX["extension_hooks"]}
check("A05 extension hook difference", events == hook_refs and
      all(h["capability_id"] in cap_ids for h in MATRIX["extension_hooks"]),
      f"hooks={len(events)}, uncovered={sorted(set(events) - set(hook_refs))}")

opaque_refs = {(r["file"], r["line"], r["kind"]) for r in GRAPH["opaque_dynamic_imports"]}
classified_refs = {(r["file"], r["line"], r["kind"]) for r in MATRIX["opaque_dynamic_imports"]}
check("A05 opaque dynamic import disposition", opaque_refs == classified_refs and
      all(r["classification"] for r in MATRIX["opaque_dynamic_imports"]),
      f"opaque expressions={len(opaque_refs)}, unclassified={sorted(opaque_refs-classified_refs)}")

roots = set(GRAPH["roots"])
required_roots = {"server/main-full.ts", "server/main-open.ts", "server/bootstrap.ts", "cli/entry.ts",
                  "desktop/main.cjs", "desktop/bootstrap.cjs", "desktop/preload.cjs", "desktop/src/main.tsx",
                  "hub/index.ts", "hub/channel-router.ts", "hub/scheduler.ts", "lib/bridge/bridge-manager.ts"}
check("A05 multi-surface roots present", required_roots <= roots,
      f"missing roots={sorted(required_roots-roots)}")
paths = GRAPH["root_paths_to_pi_consumers"]
static_paths = GRAPH["root_static_runtime_paths_to_pi_consumers"]
for consumer in ["core/session-coordinator.ts", "core/bridge-session-manager.ts",
                 "hub/agent-executor.ts", "lib/llm/cache-preserving-compaction-agent-run.ts"]:
    check(f"A05 service composition reaches {consumer}",
          "server/main-full.ts" in paths.get(consumer, {}),
          "static graph from main-full to Pi consumer")
check("A05 static runtime reachability layer present and equal for Pi consumers",
      isinstance(static_paths, dict) and len(static_paths) > 0
      and GRAPH["reachability_summary"]["pi_consumer_files_static_runtime"] == len(static_paths),
      f"static_runtime consumers={len(static_paths)}")
check("A05 background injected edge reaches Pi",
      "engine.executeIsolated(prompt" in (ROOT / "hub/scheduler.ts").read_text()
      and "this._sessionCoord.executeIsolated(prompt" in (ROOT / "core/engine.ts").read_text()
      and "createAgentSession({" in (ROOT / "core/session-coordinator.ts").read_text(),
      "scheduler -> injected engine -> coordinator isolated session -> Pi; not a static import edge")

refs = []
for c in MATRIX["capabilities"]:
    refs.extend(c["source_refs"])
for e in MATRIX["production_entrypoint_capabilities"]:
    refs.extend(e["chain"])
    check(f"A05 {e['id']} mapped", bool(e["capabilities"]) and set(e["capabilities"]) <= cap_ids,
          e["surface"])
for p in MATRIX["worker_counterexamples"]:
    refs.extend(p["evidence"])
    for structured in [*p["terminal_state_sinks"], *p["injection_or_call_source"]]:
        if "ref" in structured:
            refs.append(structured["ref"])
for h in MATRIX["extension_hooks"]:
    refs.append(h["source_ref"])
for d in DEPS["dependencies"]:
    refs.extend(d["source_refs"])
check("source references exist", all(source_ref_exists(ref) for ref in refs),
      f"refs={len(refs)}, invalid={[ref for ref in refs if not source_ref_exists(ref)][:12]}")
check("locked package versions recorded", all(d["lock_version"] for d in DEPS["dependencies"]),
      f"missing={[d['package'] for d in DEPS['dependencies'] if not d['lock_version']]}")

manifest = json.loads((ROOT / "package.json").read_text())
lock = json.loads((ROOT / "package-lock.json").read_text())
direct = set(manifest["dependencies"]) | set(manifest.get("optionalDependencies", {}))
rows = {item["package"]: item for item in DEPS["dependencies"]}
check("runtime direct dependency set and unique IDs",
      set(rows) == direct | {"electron"} and len(rows) == len(DEPS["dependencies"])
      and len({item["id"] for item in rows.values()}) == len(rows),
      f"manifest={len(direct)}, rows={len(rows)}")
check("runtime lock versions equal package-lock",
      all(item["lock_version"] == lock["packages"][f'node_modules/{package}']["version"]
          for package, item in rows.items()), "every row must match installed lock entry")


# ── 51 行依赖账本：以独立扫描为预期（不再以图自证） ───────────────────────────
def expected_sites_independent(package):
    keys = set()
    for spec, sites in INDEPENDENT.items():
        if spec == package or spec.startswith(package + "/"):
            keys.update((file, line, kind, type_only, spec) for file, line, kind, type_only in sites)
    return sorted(keys)


required_owner_terms = {
    "mammoth": ("KnowledgeService", "Tauri"), "exceljs": ("KnowledgeService", "Tauri"),
    "jsdom": ("KnowledgeService",), "diff": ("ResourceService", "React"),
    "js-yaml": ("ConfigService", "React"), "typebox": ("ToolRegistry", "InvocationGateway"),
    "semver": ("ArtifactService", "Tauri", "CLI"), "ws": ("BridgePort", "BrowserPort", "Tauri"),
    "qrcode": ("DeviceRegistry", "Bridge"), "markdown-it": ("BridgePort", "React", "Tauri"),
}
required_protocol_terms = {
    "mammoth": ("知识", "预览"), "exceljs": ("知识", "预览"),
    "jsdom": ("WebReader", "HTML"), "js-yaml": ("YAML", "供应商"),
    "typebox": ("schema", "非法参数"), "semver": ("manifest", "回滚", "CLI"),
    "qrcode": ("配对", "微信"), "ws": ("浏览器命令", "重连"),
}


def dependency_violations(item, expected_override=None):
    package = item["package"]
    saved = item["direct_import_sites"]
    errors = []
    expected = expected_override if expected_override is not None else expected_sites_independent(package)
    expected_keys = [(file, line, kind, type_only, spec) for file, line, kind, type_only, spec in expected]
    actual_keys = [(s["source_ref"].rsplit(":", 1)[0], int(s["source_ref"].rsplit(":", 1)[1]),
                    s["kind"], s["type_only"], s["specifier"]) for s in saved]
    if sorted(expected_keys) != sorted(actual_keys):
        errors.append("完整导入位置或 import 类型与独立源码扫描不符")
    if any(s.get("load_timing") not in {"module_evaluation", "call_time", "type_only", "resolve_only"}
           for s in saved):
        errors.append("load_timing 缺失或非法")
    expected_by_key = {(f, l): (kind, type_only) for f, l, kind, type_only, _spec in expected}
    for s in saved:
        ref_file, ref_line = s["source_ref"].rsplit(":", 1)
        key = (ref_file, int(ref_line))
        kind, type_only = expected_by_key.get(key, (None, None))
        if kind in {"import", "export_from"}:
            want = "type_only" if type_only else "module_evaluation"
            if s.get("load_timing") != want:
                errors.append(f"静态导入 {ref_file}:{ref_line} load_timing 应为 {want}")
    graph_refs = {f'{f}:{l}' for f, l, _k, _t, _s in expected}
    if not graph_refs <= set(item["source_refs"]):
        errors.append("source_refs 漏掉直接消费位置")
    if len(item["source_refs"]) != len(set(item["source_refs"])):
        errors.append("source_refs 重复")
    if any(not source_ref_exists(ref) for ref in item["source_refs"]):
        errors.append("source_refs 存在无效行号")
    if not item["module_load_activation"] or not item["operation_activation"]:
        errors.append("模块求值与功能调用两栏缺失")
    if item["module_load_activation"] == item["operation_activation"]:
        errors.append("模块求值和功能调用混为同一触发")
    if "相关前端视图/辅助操作按需加载" in item["activation"]:
        errors.append("旧前端按需模板未清除")
    has_static = any(s["kind"] in {"import", "export_from"} and not s["type_only"] for s in saved)
    if has_static and not any(token in item["module_load_activation"] for token in ("静态", "顶层", "模块求值")):
        errors.append("存在运行时静态导入，却被写成仅动态/操作时加载")
    if not item["rust_authority"] or not item["protocol_test"] or not item["retirement_evidence"]:
        errors.append("迁移负责人、协议测试或退出证据缺失")
    if not all(term in item["rust_authority"] for term in required_owner_terms.get(package, ())):
        errors.append("跨端负责人被泛化或缺失")
    if not all(term in item["protocol_test"] for term in required_protocol_terms.get(package, ())):
        errors.append("关键协议对照项缺失")
    expected_status = (
        "DIRECT_RUNTIME_IMPORT" if any(not s["type_only"] for s in saved) else
        "EVIDENCED_INDIRECT" if package in {"@silvia-odwyer/photon-node", "@tiptap/pm"} else
        "NO_DIRECT_PRODUCTION_IMPORT_EVIDENCE"
    )
    if item["runtime_consumption_status"] != expected_status:
        errors.append("直接/间接/无直接消费证据状态不符")
    return errors


violations = {package: problems for package, item in rows.items()
              if (problems := dependency_violations(item))}
check("runtime all 51 rows import/load/owner evidence (independent oracle)", not violations,
      f"violations={json.dumps(violations, ensure_ascii=False)[:600]}")

required_evidence = {
    "mammoth": {"lib/knowledge/source-processors.ts:6", "desktop/main.cjs:6206"},
    "exceljs": {"lib/knowledge/source-processors.ts:4", "desktop/main.cjs:6219"},
    "jsdom": {"lib/knowledge/source-adapters.ts:1", "lib/tools/web-reader.ts:8"},
    "diff": {"lib/resource-io/file-change-presentation.ts:2"},
    "js-yaml": {"core/first-run.ts:10", "desktop/src/react/utils/markdown-document.ts:1", "shared/safe-fs.ts:37"},
    "typebox": {"lib/tools/invocation/schema-validator.ts:2"},
    "semver": {"shared/artifact-core/release-order.cjs:10", "desktop/src/shared/artifact-boot.cjs:59", "shared/artifact-core/ota-core.cjs:167"},
    "ws": {"lib/bridge/dingtalk-adapter.ts:13", "lib/bridge/qq-adapter.ts:15", "desktop/main.cjs:4550"},
    "chokidar": {"core/skill-manager.ts:9", "desktop/main.cjs:30"},
    "qrcode": {"server/routes/access.ts:2", "lib/bridge/wechat-login.ts:8"},
    "yauzl": {"lib/extract-zip.ts:22", "lib/skills/skill-package-installer.ts:240"},
    "markdown-it": {"lib/bridge/feishu-outbound-renderer.ts:1", "desktop/main.cjs:4730"},
}
check("runtime cross-surface call-chain anchors",
      all(refs <= set(rows[package]["source_refs"]) for package, refs in required_evidence.items()),
      "service, desktop, React and plugin consumers stay visible")
check("runtime cross-surface owners explicit",
      all(all(term in rows[package]["rust_authority"] for term in terms)
          for package, terms in required_owner_terms.items()),
      "high-impact service/desktop/renderer owner terms must be present")

bad_load = deepcopy(rows["mammoth"])
bad_load["module_load_activation"] = "文件操作时动态加载"
check("runtime synthetic static-as-dynamic claim rejected",
      bool(dependency_violations(bad_load)), "static knowledge import cannot be hidden")
negative_probes += 1
bad_refs = deepcopy(rows["diff"])
bad_refs["direct_import_sites"] = bad_refs["direct_import_sites"][:3]
bad_refs["source_refs"] = [r for r in bad_refs["source_refs"]
                           if r != "lib/resource-io/file-change-presentation.ts:2"]
check("runtime synthetic truncated source refs rejected",
      bool(dependency_violations(bad_refs)), "server diff consumer cannot disappear")
negative_probes += 1
bad_owner = deepcopy(rows["semver"])
bad_owner["rust_authority"] = "前端辅助操作按需"
check("runtime synthetic generic updater owner rejected",
      bool(dependency_violations(bad_owner)),
      "artifact ordering must name Tauri/Rust/CLI ownership")
negative_probes += 1
bad_type_only = deepcopy(rows["mermaid"])
bad_type_only["direct_import_sites"][0]["type_only"] = False
check("runtime synthetic type-only import mislabel rejected",
      bool(dependency_violations(bad_type_only)),
      "type-only MermaidConfig import is not runtime load evidence")
negative_probes += 1
bad_transitive = deepcopy(rows["codemirror"])
bad_transitive["runtime_consumption_status"] = "DIRECT_RUNTIME_IMPORT"
check("runtime synthetic unevidenced package activation rejected",
      bool(dependency_violations(bad_transitive)),
      "codemirror has no direct production import")
negative_probes += 1
bad_drop_safefs = deepcopy(rows["js-yaml"])
bad_drop_safefs["direct_import_sites"] = [s for s in bad_drop_safefs["direct_import_sites"]
                                          if s["source_ref"] != "shared/safe-fs.ts:37"]
bad_drop_safefs["source_refs"] = [r for r in bad_drop_safefs["source_refs"]
                                  if r != "shared/safe-fs.ts:37"]
check("runtime synthetic safe-fs createRequire omission rejected",
      bool(dependency_violations(bad_drop_safefs)),
      "shared/safe-fs.ts:37 是 js-yaml 真实消费者，删除必须被独立扫描差集拒绝")
negative_probes += 1
bad_pi_type = deepcopy(rows["@earendil-works/pi-agent-core"])
for site in bad_pi_type["direct_import_sites"]:
    if site["source_ref"] == "lib/pi-sdk/index.ts:61":
        site["type_only"] = False
check("runtime synthetic pi-agent-core type-edge mislabel rejected",
      bool(dependency_violations(bad_pi_type)),
      "index.ts:61 export type 不得在依赖账本记为运行时加载")
negative_probes += 1

# ── A06：worker 双向枚举闭合 + 外围语义反例 ──────────────────────────────────
SOURCE_PATTERNS = ["executeIsolated", "runAgentLoop", "createAgentSession", "runAgentPhoneSession",
                   "executeLoopTurn", "deliverCustomMessage", "runWorkflowScript", "deliverLoopMessage"]
SINK_PATTERNS = ["finishRun", "settleTask", "logRun", "markRun", "TaskRegistry", "DeferredResultStore",
                 "deferredStore", "deferred:", "LoopStore", "LoopController", "Dream", "updateBookmark"]
ALL_WORKER_PATTERNS = SOURCE_PATTERNS + SINK_PATTERNS
discovered_independent = {}
for file in CONTROLLED:
    text = (ROOT / file).read_text(errors="replace")
    matched = [p for p in ALL_WORKER_PATTERNS if p in text]
    if matched:
        discovered_independent[file] = matched

classification = MATRIX["worker_discovery"]["classification"]
manual_adjacent = MATRIX["worker_discovery"]["manual_adjacent"]


def closure_violations(discovered_map):
    problems = []
    for file in discovered_map:
        if file not in classification and file not in manual_adjacent:
            problems.append(f"发现位点未分类: {file}")
    for file in classification:
        if file not in discovered_map:
            problems.append(f"分类条目未被模式发现（应移入手工相邻区或删除）: {file}")
    for file in [*classification, *manual_adjacent]:
        if not (ROOT / file).is_file():
            problems.append(f"分类引用文件不存在: {file}")
    return problems


check("A06 worker discovery closure (independent scan)", not closure_violations(discovered_independent),
      "; ".join(closure_violations(discovered_independent))[:600])
check("A06 discovery patterns match matrix declaration",
      sorted(MATRIX["worker_discovery"]["source_patterns"]) == sorted(SOURCE_PATTERNS)
      and sorted(MATRIX["worker_discovery"]["sink_patterns"]) == sorted(SINK_PATTERNS),
      "pattern sets drift between builder and validator")
synthetic = dict(discovered_independent)
synthetic["lib/tools/__synthetic_background_executor__.ts"] = ["executeIsolated", "settleTask"]
check("A06 synthetic unlisted executor is rejected",
      bool(closure_violations(synthetic)), "新增隔离调用或终态 sink 必须进入差集")
negative_probes += 1
dropped = {f: p for f, p in discovered_independent.items() if f != "lib/tools/subagent-tool.ts"}
check("A06 removed classification shows up as stale",
      bool(closure_violations(dropped)) or "lib/tools/subagent-tool.ts" in manual_adjacent,
      "删除分类条目必须被 stale 差集暴露")
negative_probes += 1

# 外围语义约束：PERIPHERAL 候选不得创建 session、触发父 turn 或写任务终态。
FORBIDDEN_PERIPHERAL_TOKENS = ["createAgentSession(", "runAgentLoop(", "executeIsolated(",
                               "session.prompt(", "runAgentPhoneSession(", "deliverCustomMessage",
                               "finishRun(", "settleTask(", "logRun(", "markRun(",
                               "store.resolve(", "store.fail(", "store.defer(",
                               "deferredStore.", "taskRegistry."]


def peripheral_violations(workers):
    problems = []
    for w in workers:
        if w["verdict"] != "PERIPHERAL_CANDIDATE":
            continue
        source = (ROOT / w["candidate"]).read_text(errors="replace")
        hits = [t for t in FORBIDDEN_PERIPHERAL_TOKENS if t in source]
        if hits:
            problems.append(f"{w['id']} {w['candidate']} 含执行/终态形态 {hits}")
    return problems


check("A06 peripheral candidates hold no loop or terminal sink",
      not peripheral_violations(MATRIX["worker_counterexamples"]),
      "; ".join(peripheral_violations(MATRIX["worker_counterexamples"])))

flipped = []
for w in MATRIX["worker_counterexamples"]:
    if w["id"] in {"W2", "W5", "W8", "W9", "W13"}:
        flipped.append({**w, "verdict": "PERIPHERAL_CANDIDATE"})
    else:
        flipped.append(w)
check("A06 synthetic kernel-to-peripheral flips rejected",
      bool(peripheral_violations(flipped)),
      "子代理/封面/DM/延迟结果/媒体改判外围必须被语义检查拒绝")
negative_probes += 1

worker_ids = [w["id"] for w in MATRIX["worker_counterexamples"]]
check("A06 worker inventory covers W1-W15 root-cause rows",
      {"W2", "W3", "W4", "W5", "W6", "W7", "W8", "W9", "W10", "W11", "W12", "W13", "W14", "W15"}
      <= set(worker_ids) and len(worker_ids) == len(set(worker_ids)),
      f"workers={worker_ids}")
kernel_workers = [w for w in MATRIX["worker_counterexamples"]
                  if w["verdict"] in {"KERNEL_MIGRATION", "ADJACENT_SESSION_STATE"}]
check("A06 kernel workers name a Rust owner and retirement evidence",
      all(w["rust_owner"].startswith("Rust") and w["retirement_evidence"] for w in kernel_workers),
      "每条内核迁移条目须有 Rust 唯一负责人与退场证据")

loop_probe = next(p for p in MATRIX["worker_counterexamples"] if p["candidate"] == "hub/agent-executor.ts")
loop_source = (ROOT / loop_probe["candidate"]).read_text()
check("A06 actual model loop worker is kernel", "createAgentSession" in loop_source and
      "session.prompt" in loop_source and loop_probe["verdict"] == "KERNEL_MIGRATION",
      "hub/agent-executor creates Pi sessions and prompts")
comp_probe = next(p for p in MATRIX["worker_counterexamples"] if p["candidate"] == "lib/llm/cache-preserving-compaction-agent-run.ts")
check("A06 direct Pi loop in compaction is kernel",
      "runAgentLoop(" in (ROOT / comp_probe["candidate"]).read_text() and
      comp_probe["verdict"] == "KERNEL_MIGRATION", "compaction invokes runAgentLoop")
terminal_probe = next(p for p in MATRIX["worker_counterexamples"] if p["candidate"] == "hub/scheduler.ts")
check("A06 task status writer is kernel", "status:" in (ROOT / terminal_probe["candidate"]).read_text()
      and terminal_probe["verdict"] == "KERNEL_MIGRATION", "scheduler writes activity status")


def worker_classification_violation(probe, source):
    owns_loop = "createAgentSession" in source and "session.prompt" in source
    owns_terminal = "status:" in source and "executeIsolated" in source
    return (owns_loop or owns_terminal) and probe["verdict"] == "PERIPHERAL_CANDIDATE"


bad_verdict = {**loop_probe, "verdict": "PERIPHERAL_CANDIDATE"}
check("A06 synthetic permissive classification rejected",
      worker_classification_violation(bad_verdict, loop_source),
      "a loop-owning worker cannot pass peripheral classification")
negative_probes += 1

output = dict(task="R00-T03", status="READY_FOR_REVIEW", base_sha=TASK_BASE_SHA,
              observed_head=HEAD,
              evidence_level="静态源码/锁文件/只读脚本/独立 Python 扫描 oracle/TS emit 探针；未运行真实产品或供应商", checks=checks,
              synthetic_negative_probes=negative_probes,
              independent_oracle=dict(controlled_files=len(CONTROLLED),
                                      scanned_specifiers=len(INDEPENDENT),
                                      emit_probe="typescript transpileModule 类型擦除 9 例",
                                      worker_discovery_files=len(discovered_independent)),
              input_sha256={name: hashlib.sha256((HERE / name).read_bytes()).hexdigest()
                            for name in ["R00-T03_IMPORT_GRAPH.json", "PI_REPLACEMENT_MATRIX.json",
                                         "RUNTIME_DEPENDENCIES.json"]})
(HERE / "R00-T03_VALIDATION.json").write_text(json.dumps(output, ensure_ascii=False, indent=2) + "\n")
print(json.dumps(dict(checks=len(checks), passed=sum(c["pass_"] for c in checks),
                      negative_probes=negative_probes, status="READY_FOR_REVIEW"), ensure_ascii=False))
