#!/usr/bin/env python3
"""R01-T05 sample-set generator. Regenerates tests/migration/r01-t05/samples/ deterministically.

All assets are constructed locally (PNG built pixel-by-pixel with zlib; no
network, no external files). Samples reference only relative/data: resources so
both render chains can run with zero network access.

Usage: python3 generate_samples.py           # regenerate into ./samples
       python3 generate_samples.py --check   # verify samples match generator output
"""
import base64
import hashlib
import os
import struct
import sys
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
SAMPLES = os.path.join(HERE, "samples")

# ---------------------------------------------------------------- PNG builder

def png_rgba(width, height, pixel_fn):
    """Minimal truecolor+alpha PNG. pixel_fn(x,y)->(r,g,b,a)."""
    raw = bytearray()
    for y in range(height):
        raw.append(0)  # filter type 0
        for x in range(width):
            raw.extend(pixel_fn(x, y))
    def chunk(tag, data):
        c = tag + data
        return struct.pack(">I", len(data)) + c + struct.pack(">I", zlib.crc32(c) & 0xFFFFFFFF)
    ihdr = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr)
            + chunk(b"IDAT", zlib.compress(bytes(raw), 9)) + chunk(b"IEND", b""))


def chart_png():
    """Deterministic synthetic 'chart' 480x240: axis + 8 colored bars + grid."""
    W, H = 480, 240
    bars = [(60 + i * 48, 40 + ((i * 37) % 140), (37 * i) % 255) for i in range(8)]
    def px(x, y):
        # grid lines every 40px
        if y % 40 == 0 or x % 40 == 0:
            return (220, 220, 220, 255)
        # axis
        if y == H - 24 or x == 40:
            return (60, 60, 60, 255)
        for i, (bx, bh, hue) in enumerate(bars):
            if bx <= x < bx + 28 and H - 24 - bh <= y < H - 24:
                return ((hue + 40) % 255, (hue * 2 + 90) % 255, (hue * 3 + 140) % 255, 255)
        return (255, 255, 255, 255)
    return png_rgba(W, H, px)

# ---------------------------------------------------------------- corpora

RARE_SUBSET_OK = "龘靐麤爨驫鬱"        # covered by bundled Noto Serif SC subsets
RARE_FALLBACK = "齉爩龗灪"             # BMP rare, NOT covered by subsets -> system fallback
RARE_EXTB = "𠮷𪚥"                     # Extension B (SIP) — coverage recorded, not assumed

ZH_PARA = (
    "灵犀是一个拥有记忆与人格的个人智能体。它把每一次对话、每一份文档、每一个决定都"
    "放进同一条可追溯的时间线里，让「记得」成为默认能力而非偶然。渲染管线必须保证长文档"
    "在跨页时不丢行、不裁切、不产生异常空白页；中文字体必须真实嵌入而不是静默回退。"
)
CODE_SNIPPET = """\
async fn render(job: &Job) -> Result<Vec<u8>, PdfError> {
    let page = browser.attach("about:blank").await?;
    page.navigate(&job.url).await?;
    page.wait_fonts_ready(Duration::from_millis(job.settle_ms)).await?;
    let pdf = page.print_to_pdf(job.options()).await?;
    validate_pdf(&pdf)?; // %PDF- header, %%EOF trailer, page count >= 1
    Ok(pdf)
}"""

def css_common():
    return """
  body { font-family: 'EB Garamond', 'Noto Serif SC', serif; margin: 0; color: #1a1a1a; }
  main { padding: 24px 32px; }
  h1, h2 { font-family: 'EB Garamond', 'Noto Serif SC', serif; }
  code, pre { font-family: 'JetBrains Mono', monospace; }
  table { border-collapse: collapse; width: 100%; font-size: 12px; }
  thead { display: table-header-group; }
  tr { break-inside: avoid; }
  th, td { border: 1px solid #888; padding: 3px 8px; }
  .rare { font-size: 28px; letter-spacing: 6px; }
  .marker { white-space: nowrap; font-family: 'JetBrains Mono', monospace; font-size: 10px; }
  .codeblock { background: #f5f2ec; border: 1px solid #d8d2c4; padding: 12px;
               white-space: pre; font-size: 12px; break-inside: avoid; }
  .math { font-size: 20px; }
"""

def html_doc(title, body, extra_css="", extra_head=""):
    return ("<!DOCTYPE html>\n<html lang=\"zh-CN\">\n<head>\n<meta charset=\"utf-8\">\n"
            f"<title>{title}</title>\n<style>{css_common()}{extra_css}</style>\n{extra_head}\n"
            f"</head>\n<body>\n<main>\n{body}\n</main>\n</body>\n</html>\n")

# ---------------------------------------------------------------- samples

def sample_long_zh(png_b64):
    rows = []
    for i in range(1, 141):
        rows.append(
            f"<tr><td>{i:03d}</td><td>第{i}行·灵犀记忆条目</td>"
            f"<td>{'甲乙丙丁戊己庚辛'[i % 8]}{(i * 7) % 100:02d}</td>"
            f"<td>{(i * 123457) % 99991}</td><td>{'是' if i % 3 else '否'}</td>"
            f"<td class=\"marker\">ROW-{i:03d}-END</td></tr>")
    table = ("<table><thead><tr><th>#</th><th>条目</th><th>分类</th>"
             "<th>数值</th><th>归档</th><th>校验标记</th></tr></thead><tbody>"
             + "".join(rows) + "</tbody></table>")
    # 段落数取 650，使样本总量 ~240KB，与 R00-T06 office 基线样本（239,073B）口径相当。
    paras = "".join(f"<p>{ZH_PARA}（段落 {i}）</p>" for i in range(1, 651))
    body = f"""
<h1>灵犀长文档渲染样本 R01-T05-S1</h1>
<p>本样本覆盖：中文正文、生僻字、跨页长表格、图片、代码块、数学内容。</p>
<h2>生僻字（BMP·子集内）<span class="rare">{RARE_SUBSET_OK}</span></h2>
<h2>生僻字（BMP·子集外·系统回退）<span class="rare">{RARE_FALLBACK}</span></h2>
<h2>生僻字（扩展B区·SIP）<span class="rare">{RARE_EXTB}</span></h2>
<h2>图表图片（本地生成 PNG，data: URL）</h2>
<p><img alt="chart" width="480" height="240" src="data:image/png;base64,{png_b64}"></p>
<p><img alt="chart-file" width="480" height="240" src="assets/chart.png"></p>
<h2>代码块</h2>
<div class="codeblock">{CODE_SNIPPET}</div>
<h2>数学内容（内联 SVG + CSS 分数）</h2>
<p class="math">求根公式：
<svg width="150" height="52" viewBox="0 0 150 52" xmlns="http://www.w3.org/2000/svg" role="img" aria-label="x=(-b±√(b²-4ac))/2a">
<text x="0" y="34" font-size="20">x =</text>
<line x1="32" y1="26" x2="146" y2="26" stroke="black"/>
<text x="38" y="18" font-size="16">−b ± √(b²−4ac)</text>
<text x="82" y="46" font-size="16">2a</text>
</svg>
；欧拉恒等式 e<sup>iπ</sup> + 1 = 0；∑<sub>k=1</sub><sup>n</sup> k = n(n+1)/2。</p>
<h2>跨页长表格（140 行）</h2>
{table}
<h2>长正文（650 段）</h2>
{paras}
<p>文档结束标记 DOC-END-OMEGA。</p>
"""
    return html_doc("R01-T05-S1 中文长文档", body)

def sample_page_semantics():
    """@page size A5 landscape + break controls. With preferCSSPageSize=true the
    MediaBox must be A5 landscape; with false + pageSize=A4 it must be A4 portrait."""
    blocks = "".join(
        f'<div style="break-inside:avoid;border:1px solid #999;margin:6px 0;padding:8px">'
        f'块 {i} BREAK-KEEP-{i:02d}<br>第二行<br>第三行</div>' for i in range(1, 13))
    body = f"""
<h1>@page 语义样本 S2</h1>
<p>本页声明 @page size:A5 landscape; margin:15mm。preferCSSPageSize=true 时 MediaBox 应为 A5 横版。</p>
<h2 style="break-before:page">强制分页标题 PAGE2-START</h2>
{blocks}
<p>结束 S2-END。</p>
"""
    return html_doc("R01-T05-S2 page 语义", body,
                    extra_css="\n  @page { size: A5 landscape; margin: 15mm; }\n")

def sample_js_gated():
    body = """
<h1>JS 开关样本 S4</h1>
<p id="static-marker">STATIC-MARKER-ALWAYS-PRESENT</p>
<p id="js-marker">JS-MARKER-ABSENT</p>
<script>
  document.getElementById('js-marker').textContent = 'JS-EXECUTED-MARKER-7f3a';
</script>
"""
    return html_doc("R01-T05-S4 JS 开关", body)

def sample_dangerous():
    body = """
<h1>危险资源样本 S5</h1>
<p>DANGER-DOC-START</p>
<iframe src="file:///etc/passwd" width="600" height="200"></iframe>
<p><img src="file:///etc/passwd" alt="passwd-as-img" width="100" height="40"></p>
<p><img src="file:///etc/master.passwd" alt="master-passwd" width="100" height="40"></p>
<p><img src="https://canary.invalid/t05-beacon.png" alt="remote-beacon" width="100" height="40"></p>
<p><img src="http://127.0.0.1:18291/t05-loopback-canary.png" alt="loopback-canary" width="100" height="40"></p>
<script>
  try { fetch('file:///etc/hosts').then(r => r.text()).then(t => {
    const d = document.createElement('div'); d.id = 'exfil-hosts'; d.textContent = t; document.body.appendChild(d);
  }); } catch (e) {}
  try {
    const x = new XMLHttpRequest(); x.open('GET', 'file:///etc/passwd'); x.onload = () => {
      const d = document.createElement('div'); d.id = 'exfil-passwd'; d.textContent = x.responseText; document.body.appendChild(d);
    }; x.send();
  } catch (e) {}
</script>
<p>DANGER-DOC-END</p>
"""
    return html_doc("R01-T05-S5 危险资源", body)

def sample_dangerous_decoy():
    """S5 的合成诱饵变体：越权目标换成 tests/migration/r01-t05/decoy/passwd
    （位于样本目录 allowlist 之外）。旧链对照跑此变体，使泄漏证据不含真实系统文件内容。
    语义与 S5 同构：file:// 越权 + 远端 + loopback canary。"""
    body = """
<h1>危险资源样本 S5-DECOY（旧链对照专用）</h1>
<p>DANGER-DECOY-DOC-START</p>
<iframe src="../decoy/passwd" width="600" height="200"></iframe>
<p><img src="../decoy/passwd" alt="decoy-as-img" width="100" height="40"></p>
<p><img src="https://canary.invalid/t05-beacon.png" alt="remote-beacon" width="100" height="40"></p>
<p><img src="http://127.0.0.1:18291/t05-loopback-canary.png" alt="loopback-canary" width="100" height="40"></p>
<script>
  try {
    const x = new XMLHttpRequest(); x.open('GET', '../decoy/passwd'); x.onload = () => {
      const d = document.createElement('div'); d.id = 'exfil-decoy'; d.textContent = x.responseText; document.body.appendChild(d);
    }; x.send();
  } catch (e) {}
</script>
<p>DANGER-DECOY-DOC-END</p>
"""
    return html_doc("R01-T05-S5 危险资源(decoy)", body)

DECOY_PASSWD = """##
# Decoy User Database — R01-T05 合成诱饵，非真实系统文件
# DECOY-MARKER-7f3a
##
nobody:DECOY:-2:-2:Decoy Unprivileged User:/var/decoy:/usr/bin/false
root:DECOY:0:0:Decoy System Administrator:/var/decoy-root:/bin/decoy-sh
daemon:DECOY:1:1:Decoy Services:/var/decoy:/usr/bin/false
"""

def sample_infinite_script():
    body = """
<h1>无限脚本样本 S6</h1>
<p>INFINITE-DOC-START（load 事件被死循环阻断）</p>
<script>
  const t0 = Date.now();
  while (true) { if (Date.now() - t0 > 1e12) break; }
</script>
<p>这行永远不会被解析到。</p>
"""
    return html_doc("R01-T05-S6 无限脚本", body)

def sample_margins_paper():
    body = """
<h1>纸张/页边距样本 S3</h1>
<p>MARGIN-MARKER-TOP（内容应随 margins 参数整体内缩）</p>
<div style="position:absolute;top:0;left:0">CORNER-ORIGIN-MARKER</div>
<p>S3-END</p>
"""
    return html_doc("R01-T05-S3 纸张页边距", body)

SAMPLE_BUILDERS = {
    "s1-long-zh.html": sample_long_zh,
    "s2-page-semantics.html": sample_page_semantics,
    "s3-margins-paper.html": sample_margins_paper,
    "s4-js-gated.html": sample_js_gated,
    "s5-dangerous.html": sample_dangerous,
    "s5-dangerous-decoy.html": sample_dangerous_decoy,
    "s6-infinite-script.html": sample_infinite_script,
}

def build_all():
    png = chart_png()
    png_b64 = base64.b64encode(png).decode()
    outputs = {"assets/chart.png": png, "../decoy/passwd": DECOY_PASSWD.encode("utf-8")}
    for name, fn in SAMPLE_BUILDERS.items():
        if name == "s1-long-zh.html":
            outputs[name] = fn(png_b64).encode("utf-8")
        else:
            outputs[name] = fn().encode("utf-8")
    return outputs

def main():
    check = "--check" in sys.argv
    outputs = build_all()
    ok = True
    for rel, data in sorted(outputs.items()):
        p = os.path.join(SAMPLES, rel)
        if check:
            with open(p, "rb") as f:
                existing = f.read()
            match = existing == data
            print(f"{'MATCH' if match else 'DRIFT'} {rel} sha256={hashlib.sha256(data).hexdigest()[:16]}")
            ok = ok and match
        else:
            os.makedirs(os.path.dirname(p), exist_ok=True)
            with open(p, "wb") as f:
                f.write(data)
            print(f"wrote {rel} {len(data)}B sha256={hashlib.sha256(data).hexdigest()[:16]}")
    sys.exit(0 if ok else 1)

if __name__ == "__main__":
    main()
