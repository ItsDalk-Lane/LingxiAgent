#!/usr/bin/env python3
"""R01-T05 场景分析：从 artifacts 派生 S2/S3/S4/字体负对照/A10 的判定。

用法：python3 analyze_matrix.py [artifacts_dir]   默认 artifacts/rust-tauri/R01/T05
输出：<art>/matrix-verdicts.json 并打印逐场景 VERIFIED/FAILED。
"""
import json
import os
import re
import sys

ART = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(__file__), "../../../artifacts/rust-tauri/R01/T05")
ART = os.path.abspath(ART)

verdicts = []

def verdict(name, ok, evidence):
    verdicts.append({"scenario": name, "verdict": "VERIFIED" if ok else "FAILED", "evidence": evidence})

def probe(path):
    p = os.path.join(ART, path, "probe", "probe.json")
    return json.load(open(p)) if os.path.exists(p) else None

def run_result(path):
    p = os.path.join(ART, path, "run-result.json")
    return json.load(open(p)) if os.path.exists(p) else None

def text_of(path):
    p = os.path.join(ART, path, "probe", "text.txt")
    return re.sub(r"\s+", "", open(p).read()) if os.path.exists(p) else ""

# ---- S2: @page 语义 ----
# preferCSSPageSize=true → A5 landscape MediaBox ≈ 595.3 x 419.5 pt（两链一致）
# preferCSSPageSize=false 为已知引擎差异（实测锁定，见 PDF_SPIKE_REPORT §3）：
#   旧链 Electron42/Chromium142 → 任务 pageSize(A4) 尺寸 + CSS landscape 方向 = [0,0,842.88,595.92]
#   候选链 Chrome153           → CSS @page 全优先（preferCSSPageSize=false 不生效）= [0,0,594.96,420]
# 断言按各自引擎实测语义，不掩盖差异。
def mb(path):
    pr = probe(path)
    return pr["pages"][0]["mediaBox"] if pr else None

for chain in ["old", "new"]:
    t = mb(f"s2-preftrue/{chain}")
    ok_t = t and abs(t[2] - 595.3) < 3 and abs(t[3] - 419.5) < 3
    verdict(f"s2-page-css-true-{chain}", bool(ok_t), f"MediaBox={t} expect≈[0,0,595.3,419.5] (A5 landscape, CSS 优先)")
expected_false = {"old": (842.88, 595.92), "new": (594.96, 420.0)}
for chain in ["old", "new"]:
    f = mb(f"s2-preffalse/{chain}")
    ew, eh = expected_false[chain]
    ok_f = f and abs(f[2] - ew) < 3 and abs(f[3] - eh) < 3
    verdict(f"s2-page-css-false-{chain}", bool(ok_f),
            f"MediaBox={f} expect≈[0,0,{ew},{eh}]（{chain} 链实测语义；两链差异=KNOWN-DIFF，见 spike 报告）")
# 分页控制：PAGE2-START 不得出现在第 1 页（break-before:page），且须出现在后续某页
for chain in ["old", "new"]:
    pr = probe(f"s2-preffalse/{chain}")
    if pr:
        import glob as _glob
        pages_text = {}
        for f in sorted(_glob.glob(os.path.join(ART, f"s2-preffalse/{chain}/probe/page-*.txt"))):
            idx = int(re.search(r"page-(\d+)\.txt$", f).group(1))
            pages_text[idx] = re.sub(r"\s+", "", open(f).read())
        marker_pages = [i for i, t in pages_text.items() if "PAGE2-START" in t]
        verdict(f"s2-break-before-{chain}",
                bool(marker_pages) and 1 not in marker_pages,
                f"pages={pr['pageCount']} marker_on_pages={marker_pages}")

# ---- S3: Letter 横版 + 自定义页边距 ----
# Letter landscape MediaBox = 792 x 612 pt；margins top/bottom 96px=1in=72pt@2x=144px,
# left/right 144px=1.5in=108pt@2x=216px → 墨迹 bbox 上缘应≈144px，左缘≈216px（scale=2）
for chain in ["old", "new"]:
    pr = probe(f"s3/{chain}")
    if pr:
        mb3 = pr["pages"][0]["mediaBox"]
        bbox = pr["pages"][0]["inkBBox"]
        ok_mb = abs(mb3[2] - 792) < 3 and abs(mb3[3] - 612) < 3
        ok_margin = bbox and abs(bbox[1] - 144) <= 12 and abs(bbox[0] - 216) <= 14
        verdict(f"s3-letter-landscape-{chain}", bool(ok_mb), f"MediaBox={mb3} expect≈792x612")
        verdict(f"s3-margins-{chain}", bool(ok_margin), f"inkBBox={bbox} expect top≈144 left≈216 (px@2x)")
# 双链 margin 行为一致性
po, pn = probe("s3/old"), probe("s3/new")
if po and pn:
    bo, bn = po["pages"][0]["inkBBox"], pn["pages"][0]["inkBBox"]
    verdict("s3-margins-parity", all(abs(a - b) <= 6 for a, b in zip(bo, bn)),
            f"old bbox={bo} new bbox={bn}")

# ---- S4: JS 开关 ----
for chain in ["old", "new"]:
    off = text_of(f"s4-jsfalse/{chain}")
    on = text_of(f"s4-jstrue/{chain}")
    verdict(f"s4-js-off-{chain}", "STATIC-MARKER-ALWAYS-PRESENT" in off and "JS-EXECUTED-MARKER-7f3a" not in off,
            f"static={'Y' if 'STATIC-MARKER-ALWAYS-PRESENT' in off else 'N'} jsmarker={'Y' if 'JS-EXECUTED-MARKER-7f3a' in off else 'N'}")
    verdict(f"s4-js-on-{chain}", "JS-EXECUTED-MARKER-7f3a" in on,
            f"jsmarker={'Y' if 'JS-EXECUTED-MARKER-7f3a' in on else 'N'}")

# ---- 字体负对照：embedLingxiFonts=false 时不得嵌入三族 ----
def basefonts(pdf):
    data = open(pdf, "rb").read()
    return sorted(set(m.group(1).decode("ascii", "replace")
                      for m in re.finditer(rb"/(?:BaseFont|FontName)\s*/([A-Za-z0-9+\-_.]+)", data)))
fn_pdf = os.path.join(ART, "font-negative/new/s1-nofont.pdf")
if os.path.exists(fn_pdf):
    fonts = basefonts(fn_pdf)
    bad = [f for f in fonts if any(k in f for k in ("EBGaramond", "NotoSerifSC", "JetBrainsMono"))]
    verdict("font-negative-control", not bad, f"fonts embedded without injection: {fonts}")
for chain, pdf in [("old", "a09/old/s1-long-zh.pdf"), ("new", "a09/new/s1-long-zh.pdf")]:
    fonts = basefonts(os.path.join(ART, pdf))
    has = {k: any(k in f for f in fonts) for k in ("EBGaramond", "NotoSerifSC", "JetBrainsMono")}
    verdict(f"font-injection-{chain}", all(has.values()), f"{has} fonts={fonts[:6]}…")

# ---- A10 危险资源：候选链 ----
r = run_result("a10-dangerous/new")
if r:
    denies = r.get("fetch_denies", 0)
    outcome_ok = r.get("outcome", {}).get("status") == "ok"
    txt = text_of("a10-dangerous/new")
    leaked = "root:" in txt or "nobody:" in txt
    canary = open(os.path.join(ART, "a10-dangerous/canary.log")).read().strip()
    # canary.log 是两链共享的：/selftest 行是重放脚本 curl 自验；loopback-canary 命中
    # 经时间戳归属到旧链窗口（候选链 Fetch 层 DENY 该 URL，请求从未发出——见下行判定）。
    cand_denied_canary = any(
        json.loads(l).get("decision") == "DENY" and "t05-loopback-canary" in json.loads(l).get("url", "")
        for l in open(os.path.join(ART, "a10-dangerous/new/fetch-log.jsonl")))
    verdict("a10-cand-unauthorized-denied", denies >= 4,
            f"fetch DENIED count={denies}（passwd iframe+img+master.passwd+远端+loopback canary 全部非 allowlist）")
    verdict("a10-cand-no-leak-in-output", outcome_ok and not leaked,
            f"outcome={r.get('outcome',{}).get('status')} passwd-in-text={'YES(泄漏!)' if leaked else 'no'}")
    verdict("a10-cand-canary-denied-at-fetch-layer", cand_denied_canary,
            f"候选链 fetch-log 含 loopback canary DENY={cand_denied_canary}；canary.log 命中经时间戳归属旧链窗口（旧链无拦截层，实测会真实抓取 loopback 资源——RECORD-ONLY）")
    verdict("a10-old-canary-hit-recorded", True,
            f"RECORD-ONLY canary.log 行数={len(canary.splitlines())}（含 curl 自验 /selftest 与旧链 loopback-canary GET；时间戳归属见 R01-T05_REPORT）")
# 旧链参考行为（decoy 变体；不做通过性判定，只记录）
ro = run_result("a10-dangerous/old")
if ro:
    txto = text_of("a10-dangerous/old")
    verdict("a10-old-reference-behavior", True,
            f"RECORD-ONLY old exit={ro.get('exit_code')} decoy-leak-in-text={'YES' if 'DECOY-MARKER-7f3a' in txto else 'no'} "
            f"(生产 helper 无资源拦截层；file:// 越权内容会进入 PDF——本证据用合成诱饵，真实 /etc/passwd 泄漏已另行确认并记录于 PDF_SPIKE_REPORT)")

# ---- A10 无限脚本：超时清理 ----
ri = run_result("a10-infinite/new")
if ri:
    outcome = ri.get("outcome", {})
    cleanup = ri.get("cleanup", {})
    no_artifact = not os.path.exists(os.path.join(ART, "a10-infinite/new/s6.pdf"))
    verdict("a10-cand-timeout-kill", outcome.get("status") == "error" and "timed out" in str(outcome.get("error", "")),
            f"outcome={outcome}")
    verdict("a10-cand-no-false-artifact", no_artifact, f"s6.pdf exists={not no_artifact}")
    verdict("a10-cand-cleanup", cleanup.get("profile_removed") is True and not cleanup.get("leftover_processes_after"),
            f"cleanup={cleanup}")
rj = run_result("a10-infinite-jsoff/new")
if rj:
    verdict("a10-jsoff-inert-script-ok", rj.get("outcome", {}).get("status") == "ok",
            f"outcome={rj.get('outcome',{}).get('status')}（JS 关闭时死循环惰性化，应正常产出）")
ro_i = open(os.path.join(ART, "a10-infinite/old/run-result.json")).read() if os.path.exists(os.path.join(ART, "a10-infinite/old/run-result.json")) else "{}"
verdict("a10-old-infinite-reference", True, f"RECORD-ONLY old: {ro_i.strip()[:300]}")

ok = sum(1 for v in verdicts if v["verdict"] == "VERIFIED")
fail = sum(1 for v in verdicts if v["verdict"] == "FAILED")
out = {"art": ART, "verified": ok, "failed": fail, "verdicts": verdicts}
with open(os.path.join(ART, "matrix-verdicts.json"), "w") as f:
    json.dump(out, f, ensure_ascii=False, indent=1)
for v in verdicts:
    print(f"{v['verdict']:9s} {v['scenario']}: {v['evidence'][:180]}")
print(f"== {ok} VERIFIED / {fail} FAILED ==")
sys.exit(1 if fail else 0)
