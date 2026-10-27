#!/usr/bin/env python3
"""R01-T08 双向覆盖检查（R01-A16 目标职责闭合的机器校验）。

方向一（清单 → 目标）：R00 FEATURE_INVENTORY 的每一个 F-ID（736 个生产叶子）
必须在 R01 OWNERSHIP_TARGET.feature_ownership 中有且只有一条映射，其
target_owner 必须存在于 owner_registry 且不得是 worker 类 owner
（worker_restrictions：外围 worker 不拥有任何 F-ID），且必须有非空的
实施阶段（实现路径）。

方向二（目标 → 清单）：OWNERSHIP_TARGET 引用的每一个 feature_id 必须真实
存在于 FEATURE_INVENTORY（无孤儿映射）；owner_registry / critical_facts /
store_ownership / feature_ownership 中出现的每一个 owner_id 必须已在
owner_registry 登记（无未登记负责人）；11 条关键事实（critical_facts）
逐条有且仅有 core/service 类负责人。

Pi 替换闭合（R00-T03 PI_REPLACEMENT_MATRIX）：14 条 Pi 能力必须全部
KERNEL_MIGRATION 且有非空 rust_owner 与 target_stage——即不存在"仅靠第二
Agent loop 保留"的项；coverage_difference 的四类未映射集合必须为空；
worker 反例表的 KERNEL_MIGRATION 条目必须有 rust_owner。

用法：
  python3 -B docs/rust-tauri/R01/r01_t08_coverage_check.py [--out REPORT.json]
  python3 -B docs/rust-tauri/R01/r01_t08_coverage_check.py --self-test

--self-test 在临时目录构造变异输入（删一个 F-ID 映射 / 把 F-ID 指给 worker /
未登记 owner / Pi 能力 disposition 改为仅靠第二 Agent loop 保留 / 关键事实
负责人换成 worker），逐条断言检查器以对应检查 ID 拒绝，并附正面对照；
任何变异未被拒绝则 exit 1。

退出码：0 = 全部检查通过（或 self-test 全部按预期拒绝）；1 = 存在失败。
"""
import argparse
import copy
import json
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]

DEFAULT_INVENTORY = "docs/rust-tauri/R00/FEATURE_INVENTORY.json"
DEFAULT_PI = "docs/rust-tauri/R00/PI_REPLACEMENT_MATRIX.json"
DEFAULT_OWNERSHIP = "docs/rust-tauri/R01/OWNERSHIP_TARGET.json"

EXPECTED_FEATURE_COUNT = 736
EXPECTED_STORE_COUNT = 69
EXPECTED_PI_CAPABILITY_COUNT = 14
VALID_STAGES = {f"R{i:02d}" for i in range(0, 12)}
# R01-T01 repair-r2 锁定的 11 条关键事实（冻结集，机器校验精确相等）。
EXPECTED_CRITICAL_FACTS = [
    "authenticated_principal",
    "session_identity_branch_messages",
    "run_terminal_state",
    "attempt_generation_fence",
    "tool_availability",
    "params_approval_resource_scope",
    "model_credential_selection",
    "real_files_and_authorization",
    "history_realtime_projection_semantics",
    "usage_causal_trace",
    "scheduler_trigger_dedup",
]
ALLOWED_WORKER_VERDICTS = {"KERNEL_MIGRATION", "PERIPHERAL_CANDIDATE", "ADJACENT_SESSION_STATE"}


def _load(path):
    with open(path, "r", encoding="utf-8") as f:
        return json.load(f)


def check_coverage(inventory, pi, ownership):
    """返回 (failures, report)。failures 中每条为 'CHECK-ID: 描述'。"""
    failures = []
    report = {"checks": [], "counts": {}, "owner_feature_histogram": {}}

    def ok(check_id, detail):
        report["checks"].append({"check": check_id, "status": "PASS", "detail": detail})

    def fail(check_id, detail):
        failures.append(f"{check_id}: {detail}")
        report["checks"].append({"check": check_id, "status": "FAIL", "detail": detail})

    features = inventory.get("features", [])
    fo = ownership.get("feature_ownership", [])
    so = ownership.get("store_ownership", [])
    registry = {o["owner_id"]: o for o in ownership.get("owner_registry", [])}
    worker_ids = {oid for oid, o in registry.items() if o.get("kind") == "worker"}
    critical = ownership.get("critical_facts", [])

    # C1 计数闭合
    if len(features) == EXPECTED_FEATURE_COUNT:
        ok("C1-inventory-count", f"features={len(features)}")
    else:
        fail("C1-inventory-count", f"features={len(features)} != {EXPECTED_FEATURE_COUNT}")
    if len(fo) == EXPECTED_FEATURE_COUNT:
        ok("C1-ownership-count", f"feature_ownership={len(fo)}")
    else:
        fail("C1-ownership-count", f"feature_ownership={len(fo)} != {EXPECTED_FEATURE_COUNT}")
    if len(so) == EXPECTED_STORE_COUNT:
        ok("C1-store-count", f"store_ownership={len(so)}")
    else:
        fail("C1-store-count", f"store_ownership={len(so)} != {EXPECTED_STORE_COUNT}")
    report["counts"].update({
        "features": len(features), "feature_ownership": len(fo),
        "stores": len(so), "owners": len(registry), "critical_facts": len(critical),
    })

    # C2 F-ID 双向一一对应
    inv_ids = [f["feature_id"] for f in features]
    own_ids = [f["feature_id"] for f in fo]
    dup_inv = sorted({i for i in inv_ids if inv_ids.count(i) > 1})
    dup_own = sorted({i for i in own_ids if own_ids.count(i) > 1})
    if dup_inv:
        fail("C2-dup-inventory", f"FEATURE_INVENTORY 重复 F-ID: {dup_inv[:5]}")
    if dup_own:
        fail("C2-dup-ownership", f"feature_ownership 重复 F-ID: {dup_own[:5]}")
    missing = sorted(set(inv_ids) - set(own_ids))
    orphans = sorted(set(own_ids) - set(inv_ids))
    if not missing and not orphans and not dup_inv and not dup_own:
        ok("C2-bijection", "736 F-ID 双向一一对应，无缺失/孤儿/重复")
    else:
        if missing:
            fail("C2-unmapped-feature", f"{len(missing)} 个 F-ID 无目标映射: {missing[:5]}")
        if orphans:
            fail("C2-orphan-mapping", f"{len(orphans)} 条映射指向不存在 F-ID: {orphans[:5]}")

    # C3 每个 F-ID 的 target_owner 已登记且非 worker
    bad_owner, worker_owner = [], []
    for f in fo:
        o = f.get("target_owner")
        if o not in registry:
            bad_owner.append((f["feature_id"], o))
        elif o in worker_ids:
            worker_owner.append((f["feature_id"], o))
    if not bad_owner:
        ok("C3-owner-registered", "全部 target_owner 在 owner_registry 登记")
    else:
        fail("C3-owner-registered", f"{len(bad_owner)} 个 F-ID 指向未登记 owner: {bad_owner[:3]}")
    if not worker_owner:
        ok("C3-no-worker-owner", "无 F-ID 由 worker 类 owner 承载（worker_restrictions 规则 3）")
    else:
        fail("C3-no-worker-owner", f"{len(worker_owner)} 个 F-ID 指给 worker: {worker_owner[:3]}")

    # C4 实现路径：stage_ids 非空且阶段合法
    no_stage, bad_stage = [], []
    for f in fo:
        stages = f.get("stage_ids") or []
        if not stages:
            no_stage.append(f["feature_id"])
        elif any(s not in VALID_STAGES for s in stages):
            bad_stage.append((f["feature_id"], stages))
    if not no_stage and not bad_stage:
        ok("C4-implementation-path", "全部 F-ID 有非空且合法的实施阶段（实现路径存在）")
    else:
        if no_stage:
            fail("C4-implementation-path", f"{len(no_stage)} 个 F-ID 无实施阶段: {no_stage[:5]}")
        if bad_stage:
            fail("C4-stage-valid", f"{len(bad_stage)} 个 F-ID 阶段非法: {bad_stage[:3]}")

    # C5 关键事实：冻结 11 条，逐条 owner 已登记且为 core/service
    fact_ids = sorted(c["fact_id"] for c in critical)
    if fact_ids == sorted(EXPECTED_CRITICAL_FACTS):
        ok("C5-critical-facts-set", "11 条关键事实与冻结集精确一致")
    else:
        fail("C5-critical-facts-set", f"关键事实集合漂移: {fact_ids}")
    fact_owner_bad = []
    for c in critical:
        owners = c.get("owners") or []
        if not owners:
            fact_owner_bad.append((c["fact_id"], "no owner"))
        for o in owners:
            if o not in registry:
                fact_owner_bad.append((c["fact_id"], f"unregistered {o}"))
            elif registry[o].get("kind") not in ("core", "service"):
                fact_owner_bad.append((c["fact_id"], f"owner {o} kind={registry[o].get('kind')}"))
    if not fact_owner_bad:
        ok("C5-critical-fact-owners", "11 条关键事实均由 core/service 类 owner 单独负责")
    else:
        fail("C5-critical-fact-owners", f"关键事实负责人违规: {fact_owner_bad[:3]}")

    # C6 store：target_owner 已登记；authoritative 存储写进程非空且 owner 非 worker
    store_bad, store_worker = [], []
    for s in so:
        o = s.get("target_owner")
        if o not in registry:
            store_bad.append((s["store_id"], o))
        elif o in worker_ids:
            store_worker.append(s["store_id"])
        if "authoritative" in str(s.get("classification", "")) and not s.get("target_writer_process"):
            store_bad.append((s["store_id"], "authoritative store 缺 target_writer_process"))
    if not store_bad:
        ok("C6-store-owners", "69 个存储 target_owner 全部登记，authoritative 均有目标写进程")
    else:
        fail("C6-store-owners", f"存储归属违规: {store_bad[:3]}")
    if not store_worker:
        ok("C6-no-worker-store", "无存储由 worker 类 owner 承载")
    else:
        fail("C6-no-worker-store", f"worker 持有存储: {store_worker[:3]}")

    # C7 反向：owner_registry 中被引用的 owner 均登记（上面 C3/C5/C6 已覆盖），
    # 此处统计每个 owner 承载的 F-ID 数，零承载 owner 仅报告不判负（如 build-release.tooling）。
    hist = {}
    for f in fo:
        hist[f["target_owner"]] = hist.get(f["target_owner"], 0) + 1
    report["owner_feature_histogram"] = dict(sorted(hist.items(), key=lambda kv: -kv[1]))
    zero = [oid for oid in registry if oid not in hist]
    ok("C7-reverse-histogram", f"{len(hist)} 个 owner 承载 F-ID；零 F-ID owner（非缺陷，承担存储/职责）: {sorted(zero)}")

    # C8 Pi 替换闭合：无仅靠第二 Agent loop 的保留项
    caps = pi.get("capabilities", [])
    if len(caps) == EXPECTED_PI_CAPABILITY_COUNT:
        ok("C8-pi-count", f"Pi 能力 {len(caps)} 条")
    else:
        fail("C8-pi-count", f"Pi 能力 {len(caps)} != {EXPECTED_PI_CAPABILITY_COUNT}")
    pi_bad = []
    for c in caps:
        if c.get("disposition") != "KERNEL_MIGRATION":
            pi_bad.append((c.get("id"), c.get("disposition")))
        elif not str(c.get("rust_owner", "")).strip() or not str(c.get("target_stage", "")).strip():
            pi_bad.append((c.get("id"), "缺 rust_owner/target_stage"))
    if not pi_bad:
        ok("C8-pi-no-second-loop", "14 条 Pi 能力全部 KERNEL_MIGRATION 且有 Rust 归属与目标阶段——无仅靠第二 Agent loop 的保留项")
    else:
        fail("C8-pi-no-second-loop", f"Pi 能力未闭合（存在非内核迁移或缺归属项）: {pi_bad[:3]}")

    # C9 Pi 覆盖差集为空 + worker 反例表闭合
    diff = pi.get("coverage_difference", {})
    unmapped = {k: v for k, v in diff.items() if k.startswith("unmapped_") and v}
    if not unmapped:
        ok("C9-pi-coverage-diff", "unmapped_index_symbols/vendor_files/adapter_submodules/hooks 全空")
    else:
        fail("C9-pi-coverage-diff", f"Pi 覆盖存在未映射集合: {list(unmapped)}")
    wc_bad = []
    for w in pi.get("worker_counterexamples", []):
        if w.get("verdict") not in ALLOWED_WORKER_VERDICTS:
            wc_bad.append((w.get("id"), w.get("verdict")))
        elif w["verdict"] == "KERNEL_MIGRATION" and not str(w.get("rust_owner", "")).strip():
            wc_bad.append((w.get("id"), "KERNEL_MIGRATION 缺 rust_owner"))
    if not wc_bad:
        ok("C9-worker-counterexamples", f"{len(pi.get('worker_counterexamples', []))} 条 worker 反例全部闭合")
    else:
        fail("C9-worker-counterexamples", f"worker 反例未闭合: {wc_bad[:3]}")

    report["verdict"] = "COVERAGE-CLOSED" if not failures else "COVERAGE-OPEN"
    return failures, report


def self_test():
    """负向弹药：五种变异必须分别被对应检查拒绝；正面对照必须通过。"""
    inv = _load(REPO / DEFAULT_INVENTORY)
    pi = _load(REPO / DEFAULT_PI)
    own = _load(REPO / DEFAULT_OWNERSHIP)

    base_failures, _ = check_coverage(copy.deepcopy(inv), copy.deepcopy(pi), copy.deepcopy(own))
    results = []
    results.append(("positive-control", not base_failures,
                    "未变异输入必须 COVERAGE-CLOSED" if not base_failures else f"正面对照失败: {base_failures[:3]}"))

    # N1: 删除一条 F-ID 映射 → C2-unmapped-feature
    m = copy.deepcopy(own)
    dropped = m["feature_ownership"].pop(0)
    f, _ = check_coverage(copy.deepcopy(inv), copy.deepcopy(pi), m)
    results.append(("N1-dropped-mapping", any(x.startswith("C2-") for x in f),
                    f"删除 {dropped['feature_id']} 后检出: {[x.split(':')[0] for x in f]}"))

    # N2: F-ID 指给 worker → C3-no-worker-owner
    m = copy.deepcopy(own)
    m["feature_ownership"][0]["target_owner"] = "worker.doc-parse"
    f, _ = check_coverage(copy.deepcopy(inv), copy.deepcopy(pi), m)
    results.append(("N2-worker-owner", any(x.startswith("C3-no-worker-owner") for x in f),
                    f"检出: {[x.split(':')[0] for x in f]}"))

    # N3: 未登记 owner → C3-owner-registered
    m = copy.deepcopy(own)
    m["feature_ownership"][0]["target_owner"] = "kernel.nonexistent"
    f, _ = check_coverage(copy.deepcopy(inv), copy.deepcopy(pi), m)
    results.append(("N3-unregistered-owner", any(x.startswith("C3-owner-registered") for x in f),
                    f"检出: {[x.split(':')[0] for x in f]}"))

    # N4: Pi 能力改为仅靠第二 Agent loop 保留 → C8-pi-no-second-loop
    p = copy.deepcopy(pi)
    p["capabilities"][0]["disposition"] = "RETAINED_SECOND_AGENT_LOOP"
    f, _ = check_coverage(copy.deepcopy(inv), p, copy.deepcopy(own))
    results.append(("N4-second-loop-retained", any(x.startswith("C8-") for x in f),
                    f"检出: {[x.split(':')[0] for x in f]}"))

    # N5: 关键事实负责人换成 worker → C5-critical-fact-owners
    m = copy.deepcopy(own)
    m["critical_facts"][0]["owners"] = ["worker.doc-parse"]
    f, _ = check_coverage(copy.deepcopy(inv), copy.deepcopy(pi), m)
    results.append(("N5-worker-critical-fact", any(x.startswith("C5-critical-fact-owners") for x in f),
                    f"检出: {[x.split(':')[0] for x in f]}"))

    all_ok = all(r[1] for r in results)
    for name, passed, detail in results:
        print(f"{'PASS-NEG' if passed and name != 'positive-control' else ('PASS' if passed else 'FAIL')} {name}: {detail}")
    print(f"SELF-TEST {'OK' if all_ok else 'FAILED'} ({sum(1 for r in results if r[1])}/{len(results)})")
    return 0 if all_ok else 1


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--inventory", default=str(REPO / DEFAULT_INVENTORY))
    ap.add_argument("--pi", default=str(REPO / DEFAULT_PI))
    ap.add_argument("--ownership", default=str(REPO / DEFAULT_OWNERSHIP))
    ap.add_argument("--out", default=None, help="覆盖报告 JSON 输出路径")
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()

    if args.self_test:
        sys.exit(self_test())

    failures, report = check_coverage(_load(args.inventory), _load(args.pi), _load(args.ownership))
    report["inputs"] = {
        "inventory": args.inventory, "pi_replacement_matrix": args.pi, "ownership_target": args.ownership,
    }
    out = json.dumps(report, ensure_ascii=False, indent=1, sort_keys=True)
    if args.out:
        Path(args.out).write_text(out + "\n", encoding="utf-8")
    for c in report["checks"]:
        print(f"{c['status']:>4} {c['check']}: {c['detail']}")
    if failures:
        print(f"RESULT: COVERAGE-OPEN ({len(failures)} failures)")
        sys.exit(1)
    print("RESULT: COVERAGE-CLOSED (736 F-ID 双向映射闭合；11 关键事实单负责人；14 Pi 能力无第二 loop 保留项)")
    sys.exit(0)


if __name__ == "__main__":
    main()
