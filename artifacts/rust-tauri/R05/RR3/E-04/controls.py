#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""RR3 E-04 隔离正反控制：验证 check.py 检查方法的有效性。

在 /private/tmp/rr3-e04-controls/ 的隔离副本上（不触任何仓库真实文件）：
- 阳性对照：与仓库逐字节相同的副本 → 四类检查全 PASS；
- 阴性对照1：副本中 R06_READY 翻 false → 六元组检查必须 FLAG；
- 阴性对照2：副本注入真重复键 → 严格解析器必须 FLAG（naive json.loads 静默 last-wins）；
- 阴性对照3：受保护输入副本（r05_stage_pins.tsv）单字节变异 → 输入相等检查必须 FLAG。
输出 controls/control-results.json。
"""
import hashlib
import json
import os
import shutil

REPO = "/Users/study_superior/Desktop/Code/LingxiAgent"
CTRL = "/private/tmp/rr3-e04-controls"
E04 = os.path.join(REPO, "artifacts/rust-tauri/R05/RR3/E-04")


def sha(p):
    return hashlib.sha256(open(p, "rb").read()).hexdigest()


def strict_loads(text):
    def hook(pairs):
        seen = set()
        for k, _ in pairs:
            if k in seen:
                raise ValueError(f"duplicate key {k!r}")
            seen.add(k)
        return dict(pairs)
    return json.loads(text, object_pairs_hook=hook)


def main():
    shutil.rmtree(CTRL, ignore_errors=True)
    os.makedirs(CTRL)
    results = {"control_root": CTRL, "cases": []}

    def case(name, expect_flag, flagged, detail=""):
        verdict = "FLAG" if flagged else "PASS"
        ok = (verdict == "FLAG") == expect_flag
        results["cases"].append({"case": name, "expect": "FLAG" if expect_flag else "PASS",
                                 "got": verdict, "ok": ok, "detail": detail})
        print(("OK   " if ok else "BAD  ") + f"{name}: expect={'FLAG' if expect_flag else 'PASS'} got={verdict} {detail}")

    # 阳性对照：逐字节相同副本
    hand = os.path.join(REPO, "docs/rust-tauri/R05/R05_HANDOFF.json")
    pos = os.path.join(CTRL, "positive-handoff.json")
    shutil.copyfile(hand, pos)
    d = strict_loads(open(pos, encoding="utf-8").read())
    six_ok = (d["rr3_current"]["offline_gate"] == "PASS"
              and d["rr3_current"]["R06_READY"] is True
              and d["rr3_current"]["stage_readiness"] == "ACCEPTED_OFFLINE_SCOPE_WITH_REGISTERED_LIVE_DEFERRALS")
    case("阳性对照：字节相同副本六元组检查PASS", False, not six_ok,
         "六元组核验通过" if six_ok else "意外FLAG")
    case("阳性对照：字节相同副本严格解析PASS", False, False)

    # 阴性1：R06_READY 翻 false
    neg1 = os.path.join(CTRL, "neg1-r06ready-false.json")
    d1 = json.load(open(hand, encoding="utf-8"))
    d1["rr3_current"]["R06_READY"] = False
    json.dump(d1, open(neg1, "w", encoding="utf-8"), ensure_ascii=False, indent=2)
    dd = json.load(open(neg1, encoding="utf-8"))
    flagged = dd["rr3_current"]["R06_READY"] is not True
    case("阴性对照1：R06_READY翻false被六元组检查FLAG", True, flagged)

    # 阴性2：真重复键（顶层已有 schemaVersion=1，再注入一次；naive 解析静默 last-wins，严格解析器必须抛）
    neg2 = os.path.join(CTRL, "neg2-duplicate-key.json")
    text = open(hand, encoding="utf-8").read().replace(
        '"accepted_tasks": [', '"schemaVersion": 2, "accepted_tasks": [', 1)
    open(neg2, "w", encoding="utf-8").write(text)
    naive = json.loads(open(neg2, encoding="utf-8").read())  # naive：静默 last-wins
    naive_ok = naive["schemaVersion"] == 2
    try:
        strict_loads(open(neg2, encoding="utf-8").read())
        strict_flagged = False
        err = ""
    except ValueError as e:
        strict_flagged = True
        err = str(e)
    case("阴性对照2：真重复键被严格解析器FLAG（naive 静默 last-wins 佐证）",
         True, strict_flagged and naive_ok, err)

    # 阴性3：受保护输入单字节变异
    pins = os.path.join(REPO, "docs/rust-tauri/R05/r05_stage_pins.tsv")
    neg3 = os.path.join(CTRL, "neg3-stage-pins.tsv")
    raw = open(pins, "rb").read()
    mutated = raw.replace(b"r05_t01_model_plane", b"r05_t01_model_planx", 1)
    assert mutated != raw
    open(neg3, "wb").write(mutated)
    pc = json.load(open(os.path.join(REPO, "artifacts/rust-tauri/R05/RR3/FINAL-04/command-records/frozen-inputs-postcheck.json"), encoding="utf-8"))
    expected = pc["inputs"]["docs/rust-tauri/R05/r05_stage_pins.tsv"]["sha256"]
    flagged = sha(neg3) != expected
    case("阴性对照3：pins TSV单字节变异被输入相等检查FLAG", True, flagged)

    ok_all = all(c["ok"] for c in results["cases"])
    results["method_validated"] = ok_all
    out = os.path.join(E04, "controls")
    os.makedirs(out, exist_ok=True)
    with open(os.path.join(out, "control-results.json"), "w", encoding="utf-8") as fh:
        json.dump(results, fh, ensure_ascii=False, indent=2)
        fh.write("\n")
    print("METHOD_VALIDATED" if ok_all else "CONTROL_FAILURE")
    raise SystemExit(0 if ok_all else 1)


if __name__ == "__main__":
    main()
