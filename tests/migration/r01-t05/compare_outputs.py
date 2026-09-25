#!/usr/bin/env python3
"""R01-T05 输出比较：旧链 vs 候选链 PDF 的语义断言 + 视觉差异规则。

用法：compare_outputs.py --old-probe D --new-probe D --old-pdf F --new-pdf F \
        --expect expect.json --out compare.json

expect.json:
  {"required_tokens": [...],          # 归一化（去全部空白）后两链文本都必须包含
   "required_tokens_new_only": [...], # 只要求候选链包含（记录旧链差异）
   "expected_table_rows": 140,        # ROW-NNN-END 唯一计数
   "min_ink_coverage": 0.0005,        # 逐页墨迹下限（异常空白页判据）
   "grid_diff_warn": 20,              # 16x16 亮度网格平均绝对差告警阈值（0-255）
   "expect_equal_page_count": true}

判据（任务书 R01-T05 步骤4）：不逐像素强制一致；无缺字/裁切/错页/异常空白页 +
语义断言通过即判 PASS。
"""
import argparse
import json
import re
import sys


def norm(s: str) -> str:
    return re.sub(r"\s+", "", s)


def scan_basefonts(pdf_path):
    # Chromium(Skia) PDF：部分字体字典在压缩对象流内，/BaseFont 可能不可见；
    # /FontName（FontDescriptor，通常为明文对象）一并扫描。两者皆取。
    data = open(pdf_path, "rb").read()
    names = set()
    for m in re.finditer(rb"/(?:BaseFont|FontName)\s*/([A-Za-z0-9+\-_.]+)", data):
        names.add(m.group(1).decode("ascii", "replace"))
    return sorted(names)


def count_image_xobjects(pdf_path):
    data = open(pdf_path, "rb").read()
    return len(re.findall(rb"/Subtype\s*/Image[^s]", data))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--old-probe", required=True)
    ap.add_argument("--new-probe", required=True)
    ap.add_argument("--old-pdf", required=True)
    ap.add_argument("--new-pdf", required=True)
    ap.add_argument("--expect", required=True)
    ap.add_argument("--out", required=True)
    a = ap.parse_args()

    old = json.load(open(f"{a.old_probe}/probe.json"))
    new = json.load(open(f"{a.new_probe}/probe.json"))
    expect = json.load(open(a.expect))
    old_text = norm(open(f"{a.old_probe}/text.txt").read())
    new_text = norm(open(f"{a.new_probe}/text.txt").read())

    checks = []

    def check(name, ok, detail):
        checks.append({"name": name, "pass": bool(ok), "detail": detail})

    # 1. 页数
    if expect.get("expect_equal_page_count", True):
        check("page_count_equal", old["pageCount"] == new["pageCount"],
              f"old={old['pageCount']} new={new['pageCount']}")
    else:
        check("page_count_recorded", True,
              f"old={old['pageCount']} new={new['pageCount']} (差异允许，记录)")

    # 2. 异常空白页
    min_ink = expect.get("min_ink_coverage", 0.0005)
    old_blanks = [p["index"] for p in old["pages"] if p["inkCoverage"] < min_ink]
    new_blanks = [p["index"] for p in new["pages"] if p["inkCoverage"] < min_ink]
    check("no_blank_pages_old", not old_blanks, f"blank pages: {old_blanks}")
    check("no_blank_pages_new", not new_blanks, f"blank pages: {new_blanks}")

    # 3. 语义哨兵
    for tok in expect.get("required_tokens", []):
        check(f"token:{tok}", tok in old_text and tok in new_text,
              f"old={'Y' if tok in old_text else 'N'} new={'Y' if tok in new_text else 'N'}")
    for tok in expect.get("required_tokens_new_only", []):
        check(f"token-new:{tok}", tok in new_text,
              f"new={'Y' if tok in new_text else 'N'} old={'Y' if tok in old_text else 'N(旧链差异,记录)'}")

    # 4. 表格行完整
    n_rows = expect.get("expected_table_rows")
    if n_rows:
        rows_old = set(re.findall(r"ROW-(\d{3})-END", old_text))
        rows_new = set(re.findall(r"ROW-(\d{3})-END", new_text))
        check("table_rows_old", len(rows_old) == n_rows, f"{len(rows_old)}/{n_rows}")
        check("table_rows_new", len(rows_new) == n_rows, f"{len(rows_new)}/{n_rows}")

    # 5. 文本相似度（归一化）
    import difflib
    ratio = difflib.SequenceMatcher(None, old_text, new_text, autojunk=False).ratio()
    check("text_similarity", ratio >= 0.99, f"normalized ratio={ratio:.5f} len old={len(old_text)} new={len(new_text)}")

    # 6. 视觉差异规则：16x16 亮度网格平均绝对差（页对页）
    n = min(len(old["pages"]), len(new["pages"]))
    diffs = []
    for i in range(n):
        go, gn = old["pages"][i]["lumaGrid16"], new["pages"][i]["lumaGrid16"]
        if len(go) == len(gn) == 256:
            diffs.append(sum(abs(x - y) for x, y in zip(go, gn)) / 256)
    warn = expect.get("grid_diff_warn", 20)
    max_d = max(diffs) if diffs else 0
    mean_d = sum(diffs) / len(diffs) if diffs else 0
    check("visual_grid_diff", max_d <= warn,
          f"per-page 16x16 luma grid mean|Δ|: max={max_d:.2f} mean={mean_d:.2f} warn>{warn} (0-255 scale)")

    # 7. 字体嵌入（结构证据：/BaseFont 扫描）
    fb_expect = ["EBGaramond", "NotoSerifSC", "JetBrainsMono"]
    old_fonts = scan_basefonts(a.old_pdf)
    new_fonts = scan_basefonts(a.new_pdf)
    def has(fonts, key):
        return any(key.lower() in f.lower() for f in fonts)
    for f in fb_expect:
        check(f"font_embedded_old:{f}", has(old_fonts, f), f"old fonts={old_fonts[:8]}…")
        check(f"font_embedded_new:{f}", has(new_fonts, f), f"new fonts={new_fonts[:8]}…")

    # 8. 图片 XObject
    img_old = count_image_xobjects(a.old_pdf)
    img_new = count_image_xobjects(a.new_pdf)
    check("images_present", img_old >= 1 and img_new >= 1,
          f"image XObjects old={img_old} new={img_new}")

    result = {
        "old_pdf": a.old_pdf, "new_pdf": a.new_pdf,
        "all_pass": all(c["pass"] for c in checks),
        "checks": checks,
        "basefonts_old": old_fonts, "basefonts_new": new_fonts,
    }
    with open(a.out, "w") as f:
        json.dump(result, f, ensure_ascii=False, indent=1)
    print(json.dumps({"all_pass": result["all_pass"],
                      "failed": [c["name"] for c in checks if not c["pass"]]}, ensure_ascii=False))
    sys.exit(0 if result["all_pass"] else 1)


if __name__ == "__main__":
    main()
