//! R01-T05 HTML→PDF 渲染管线：以 CDP `Page.printToPDF` 复刻生产
//! `desktop/src/office-pdf-helper.cjs` 的 job 契约与语义（字体注入、资产等待、
//! JS 开关、打印参数、超时），并在候选链上新增生产没有的资源拦截边界
//! （Fetch 域 allowlist：仅 html 所在目录 + 注入字体目录 + data:，其余全拒）。
//!
//! 非生产组件；不接入任何生产入口。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use base64::Engine;
use serde::Deserialize;
use serde_json::json;

use crate::cdp::CdpClient;
use crate::ops::{self, Tab};

pub const DEFAULT_TIMEOUT_MS: u64 = 60_000;

/// 与 office-pdf-helper.cjs normalizeJob 完全同构的 job 契约。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawJob {
    pub html_path: String,
    pub output_path: String,
    pub viewport: Option<RawViewport>,
    pub print_background: Option<bool>,
    pub prefer_css_page_size: Option<bool>,
    pub page_size: Option<String>,
    pub landscape: Option<bool>,
    pub margins: Option<RawMargins>,
    pub allow_java_script: Option<bool>,
    pub embed_lingxi_fonts: Option<bool>,
    pub settle_ms: Option<f64>,
    pub timeout_ms: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawViewport {
    pub width: Option<f64>,
    pub height: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawMargins {
    pub top: Option<f64>,
    pub bottom: Option<f64>,
    pub left: Option<f64>,
    pub right: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct Job {
    pub html_path: PathBuf,
    pub output_path: PathBuf,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub print_background: bool,
    pub prefer_css_page_size: bool,
    pub page_size: String,
    pub landscape: bool,
    /// 生产语义实测（R01-T05 bisect，Electron 42.8.1）：printToPDF 的 margins
    /// 数字被**按 inch 直传 Chromium**（margins=4 → 实测墨迹左缘恰为 4in；
    /// 且校验 `margins ≤ pageSize` 按 inch 数值比较，故 top:96 在 A4 上被拒）。
    /// electron.d.ts 注释称 "in pixels" 与实际行为不符——候选链按**实测生产行为**
    /// 对齐：数值直传 CDP margin*（inch），不做 px→in 换算。
    pub margins_in: Option<(f64, f64, f64, f64)>, // top,bottom,left,right（inch，实测语义）
    pub allow_javascript: bool,
    pub embed_lingxi_fonts: bool,
    pub settle_ms: u64,
    pub timeout_ms: u64,
}

fn norm_num(v: Option<f64>, fallback: u64, min: u64, max: u64) -> u64 {
    match v {
        Some(n) if n.is_finite() => (n.floor() as i64).clamp(min as i64, max as i64) as u64,
        _ => fallback,
    }
}

impl Job {
    pub fn normalize(raw: RawJob) -> Result<Self, String> {
        let html_path = PathBuf::from(&raw.html_path);
        if raw.html_path.is_empty() || !html_path.exists() {
            return Err(format!("htmlPath does not exist: {}", raw.html_path));
        }
        if raw.output_path.is_empty() {
            return Err("outputPath is required".to_string());
        }
        let (vw, vh) = match &raw.viewport {
            Some(v) => (
                norm_num(v.width, 1280, 320, 4096),
                norm_num(v.height, 900, 320, 4096),
            ),
            None => (1280, 900),
        };
        let margins_in = raw.margins.map(|m| {
            (
                m.top.unwrap_or(0.0),
                m.bottom.unwrap_or(0.0),
                m.left.unwrap_or(0.0),
                m.right.unwrap_or(0.0),
            )
        });
        Ok(Job {
            html_path: html_path.clone(),
            output_path: PathBuf::from(&raw.output_path),
            viewport_width: vw as u32,
            viewport_height: vh as u32,
            print_background: raw.print_background != Some(false),
            prefer_css_page_size: raw.prefer_css_page_size != Some(false),
            page_size: match raw.page_size.as_deref().map(str::trim) {
                Some(s) if !s.is_empty() => s.to_string(),
                _ => "A4".to_string(),
            },
            landscape: raw.landscape == Some(true),
            margins_in,
            allow_javascript: raw.allow_java_script == Some(true),
            embed_lingxi_fonts: raw.embed_lingxi_fonts != Some(false),
            settle_ms: norm_num(raw.settle_ms, 250, 0, 30000),
            timeout_ms: norm_num(raw.timeout_ms, DEFAULT_TIMEOUT_MS, 1000, 300_000),
        })
    }
}

/// Electron pageSize 具名值 → (width_in, height_in)。与 Electron 文档一致
/// （A0..A6 / Legal / Letter / Tabloid / Ledger，单位 inch）。
pub fn page_size_inches(name: &str) -> Result<(f64, f64), String> {
    let wh = match name {
        "A0" => (33.1, 46.8),
        "A1" => (23.4, 33.1),
        "A2" => (16.54, 23.4),
        "A3" => (11.7, 16.54),
        "A4" => (8.27, 11.69),
        "A5" => (5.83, 8.27),
        "A6" => (4.13, 5.83),
        "Legal" => (8.5, 14.0),
        "Letter" => (8.5, 11.0),
        "Tabloid" => (11.0, 17.0),
        "Ledger" => (17.0, 11.0),
        other => return Err(format!("unsupported pageSize: {other}")),
    };
    Ok(wh)
}

// ---------------------------------------------------------------- 字体注入
// 端口自 desktop/src/office-pdf-fonts.cjs：提取白名单族的 @font-face，
// 把 ./fonts/ 相对 URL 重写为绝对 file://，全部 URL 校验存在且非空；
// 白名单任一族缺失即失败（不静默回退）。

pub const LINGXI_PDF_FONT_FAMILIES: [&str; 3] = ["EB Garamond", "Noto Serif SC", "JetBrains Mono"];
pub const FONTS_CSS_FILENAME: &str = "new-warm-paper-fonts.css";

fn extract_font_face_blocks(css: &str) -> Vec<&str> {
    let mut blocks = Vec::new();
    let mut rest = css;
    while let Some(start) = rest.find("@font-face") {
        let after = &rest[start..];
        let Some(open) = after.find('{') else { break };
        let Some(close) = after[open..].find('}') else {
            break;
        };
        blocks.push(&after[..open + close + 1]);
        rest = &after[open + close + 1..];
    }
    blocks
}

fn family_of(block: &str) -> Option<String> {
    let key = "font-family:";
    let idx = block.find(key)?;
    let after = block[idx + key.len()..].trim_start();
    let end = after.find(';')?;
    let fam = after[..end].trim().trim_matches(|c| c == '\'' || c == '"');
    Some(fam.to_string())
}

fn path_is_inside(parent: &Path, candidate: &Path) -> bool {
    candidate.starts_with(parent)
}

fn file_url(p: &Path) -> String {
    // 最小 file:// URL 构造（macOS 绝对路径；空格等按 RFC3986 百分号编码）
    let mut s = String::from("file://");
    for seg in p.to_string_lossy().split('/') {
        if seg.is_empty() {
            continue;
        }
        s.push('/');
        for b in seg.bytes() {
            let c = b as char;
            if c.is_ascii_alphanumeric() || "-._~".contains(c) {
                s.push(c);
            } else {
                s.push_str(&format!("%{b:02X}"));
            }
        }
    }
    s
}

/// 构建可注入 @font-face CSS；任一白名单族缺失、URL 越界或字体文件缺失即 Err。
pub fn build_font_injection_css(themes_dir: &Path) -> Result<String, String> {
    let css_path = themes_dir.join(FONTS_CSS_FILENAME);
    let css =
        fs::read_to_string(&css_path).map_err(|e| format!("read {}: {e}", css_path.display()))?;
    let blocks = extract_font_face_blocks(&css);
    let mut selected: Vec<(&str, String)> = Vec::new();
    for b in &blocks {
        if let Some(fam) = family_of(b) {
            if LINGXI_PDF_FONT_FAMILIES.contains(&fam.as_str()) {
                selected.push((b, fam));
            }
        }
    }
    let mut missing: Vec<&str> = Vec::new();
    for f in LINGXI_PDF_FONT_FAMILIES {
        if !selected.iter().any(|(_, fam)| fam == f) {
            missing.push(f);
        }
    }
    if !missing.is_empty() {
        return Err(format!(
            "Hana font css at {} is missing families: {}",
            css_path.display(),
            missing.join(", ")
        ));
    }
    let fonts_dir = themes_dir.join("fonts");
    let mut out = String::new();
    for (block, fam) in &selected {
        let mut rewritten = String::new();
        let mut rest = *block;
        let mut url_count = 0usize;
        while let Some(u) = rest.find("url(") {
            rewritten.push_str(&rest[..u + 4]);
            let after = &rest[u + 4..];
            let Some(close) = after.find(')') else {
                return Err(format!("unterminated url() in @font-face for {fam}"));
            };
            let raw = after[..close]
                .trim()
                .trim_matches(|c| c == '\'' || c == '"');
            url_count += 1;
            let Some(rel) = raw.strip_prefix("./fonts/") else {
                return Err(format!(
                    "unsupported font URL for {fam}: {raw} (must start with ./fonts/)"
                ));
            };
            let font_path = fonts_dir.join(rel);
            if rel.is_empty() || rel.contains("..") || !path_is_inside(&fonts_dir, &font_path) {
                return Err(format!("font URL outside themes/fonts for {fam}: {raw}"));
            }
            let meta = fs::metadata(&font_path)
                .map_err(|_| format!("missing font file for {fam}: {}", font_path.display()))?;
            if !meta.is_file() || meta.len() == 0 {
                return Err(format!(
                    "font is not a non-empty file for {fam}: {}",
                    font_path.display()
                ));
            }
            rewritten.push_str(&format!("'{}'", file_url(&font_path)));
            rest = &after[close..];
        }
        rewritten.push_str(rest);
        if url_count == 0 {
            return Err(format!("@font-face for {fam} declares no url()"));
        }
        out.push_str(&rewritten);
        out.push('\n');
    }
    Ok(out)
}

// ---------------------------------------------------------------- 资源拦截

/// 决策结果（证据日志用）。
#[derive(Debug, Clone)]
pub enum FetchDecision {
    Allow,
    Deny(String),
}

/// file:// URL → 本地路径（百分号解码 + 去 host）。非 file URL 返回 None。
pub fn file_url_to_path(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let mut bytes = Vec::with_capacity(rest.len());
    let b = rest.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&rest[i + 1..i + 3], 16) {
                bytes.push(v);
                i += 3;
                continue;
            }
        }
        bytes.push(b[i]);
        i += 1;
    }
    Some(PathBuf::from(String::from_utf8_lossy(&bytes).to_string()))
}

/// 资源策略：data:/blob: 放行；file:// 仅在 allowlist 根内放行；其余全拒。
pub fn decide_fetch(url: &str, allowed_roots: &[PathBuf]) -> FetchDecision {
    if url.starts_with("data:") || url.starts_with("blob:") || url == "about:blank" {
        return FetchDecision::Allow;
    }
    if let Some(p) = file_url_to_path(url) {
        let canon = fs::canonicalize(&p).unwrap_or(p);
        for root in allowed_roots {
            if canon.starts_with(root) {
                return FetchDecision::Allow;
            }
        }
        return FetchDecision::Deny(format!("file outside allowlist: {url}"));
    }
    FetchDecision::Deny(format!("scheme not allowed: {url}"))
}

// ---------------------------------------------------------------- 渲染管线

pub struct RenderTimings {
    pub launch_ms: u128,
    pub attach_ms: u128,
    pub navigate_ms: u128,
    pub font_inject_ms: u128,
    pub asset_wait_ms: u128,
    pub print_ms: u128,
    pub total_ms: u128,
}

pub struct RenderOutcome {
    /// 已通过结构校验（%PDF- 头 + %%EOF 尾）的完整 PDF 字节流；
    /// 由调用方在成功路径一次性原子写出，失败路径不得落盘。
    pub pdf: Vec<u8>,
    pub pdf_sha256: String,
    pub timings: RenderTimings,
    pub asset_wait_note: String,
}

/// 在每个新文档任何页面脚本之前注入 CSP（R1 验收 F1 修复：WebSocket 握手不经
/// Fetch 域 requestPaused，且 Network.setBlockedURLs 对 WS 实测无效——repair-r1
/// 实测 ws://127.0.0.1 握手仍到达 loopback canary）。connect-src 白名单刻意
/// 只排除 ws:/wss:：http(s)/file/data/blob 连接**不放行**，而是照常穿透到
/// Fetch 域 allowlist 审计层（DENY 日志语义与修复前逐条一致，S5 证据不变）；
/// ws/wss 由浏览器 CSP 策略层直接阻断（onerror，零网络命中）。
/// 脚本本身不吞错：注入调用失败在 CDP 层即报错（fail-closed）。
const CSP_CONNECT_SRC_INJECT: &str = r#"(function () {
  var m = document.createElement('meta');
  m.setAttribute('http-equiv', 'Content-Security-Policy');
  m.setAttribute('content', "connect-src file: data: blob: http: https:");
  var root = document.head || document.documentElement;
  if (root) { root.appendChild(m); return; }
  // 文档根尚未创建（早于解析）：挂观察器，根一出现即注入。
  new MutationObserver(function (_, obs) {
    var r = document.head || document.documentElement;
    if (r) { r.appendChild(m); obs.disconnect(); }
  }).observe(document, { childList: true, subtree: true });
})()"#;

/// 与 office-pdf-helper waitForPageAssets 相同的等待表达式。
const ASSET_WAIT_EXPR: &str = r#"Promise.all([
  document.fonts && document.fonts.ready ? document.fonts.ready : Promise.resolve(),
  Promise.all(Array.from(document.images || []).map((img) => {
    if (img.complete) return Promise.resolve();
    return new Promise((resolve) => {
      img.addEventListener('load', resolve, { once: true });
      img.addEventListener('error', resolve, { once: true });
    });
  })),
]).then(() => true)"#;

fn budget(job: &Job) -> Duration {
    Duration::from_millis(job.timeout_ms)
}

/// 在已启动的浏览器上执行一次渲染 job。调用方负责浏览器生命周期与清理。
pub fn render_job(
    cdp: &CdpClient,
    job: &Job,
    themes_dir: Option<&Path>,
    started: Instant,
) -> Result<RenderOutcome, String> {
    // 0. 注入素材在开页面前构建（与 helper 同序：字体缺失先失败）
    let font_css = if job.embed_lingxi_fonts {
        let dir = themes_dir.ok_or("embedLingxiFonts=true but no themes dir provided")?;
        Some(build_font_injection_css(dir)?)
    } else {
        None
    };

    // 1. 开 tab 并设置视口（对应 helper 的 BrowserWindow 尺寸）
    let tab: Tab = ops::open_tab(cdp, None, "about:blank").map_err(|e| e.to_string())?;
    cdp.call(
        Some(&tab.session_id),
        "Emulation.setDeviceMetricsOverride",
        json!({
            "width": job.viewport_width, "height": job.viewport_height,
            "deviceScaleFactor": 0, "mobile": false,
        }),
    )
    .map_err(|e| e.to_string())?;
    let attach_ms = started.elapsed().as_millis();

    // 2. JS 开关（对应 webPreferences.javascript）
    if !job.allow_javascript {
        cdp.call(
            Some(&tab.session_id),
            "Emulation.setScriptExecutionDisabled",
            json!({ "value": true }),
        )
        .map_err(|e| e.to_string())?;
    }

    // 3. 资源拦截（候选链新增边界；生产 helper 无此层）
    cdp.call(
        Some(&tab.session_id),
        "Fetch.enable",
        json!({ "patterns": [{ "urlPattern": "*" }] }),
    )
    .map_err(|e| e.to_string())?;
    // WebSocket 边界（R1 验收 F1）：WS 握手不经 Fetch 域 requestPaused，且实测
    // Network.setBlockedURLs(["ws://*","wss://*"]) 在 Chrome 153 对 WS 不生效
    // （repair-r1 实测：ws://127.0.0.1 握手仍到达 loopback canary）。改用 CSP：
    // 在每个新文档任何页面脚本执行前注入 connect-src（白名单仅排除 ws:/wss:），
    // 由浏览器策略层阻断 WebSocket；http(s)/file 连接照常穿透到上面的 Fetch 域
    // allowlist 审计层，DENY 日志语义与修复前逐条一致（详见 CSP_CONNECT_SRC_INJECT）。
    // fail-closed：注入调用失败即整个 job 失败，不静默降级。
    cdp.call(
        Some(&tab.session_id),
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": CSP_CONNECT_SRC_INJECT }),
    )
    .map_err(|e| format!("Page.addScriptToEvaluateOnNewDocument: {e}"))?;

    // 4. 导航 + 等待 load（预算 = timeoutMs）
    let nav_t = Instant::now();
    let url = file_url(&job.html_path);
    let nav = cdp.call_timeout(
        Some(&tab.session_id),
        "Page.navigate",
        json!({ "url": url }),
        budget(job),
    );
    let nav = nav.map_err(|e| format!("navigate: {e}"))?;
    if let Some(err) = nav.get("errorText").and_then(|v| v.as_str()) {
        return Err(format!("navigate errorText: {err}"));
    }
    cdp.wait_event("Page.loadEventFired", |_| true, budget(job))
        .map_err(|_| format!("loadURL timed out after {}ms", job.timeout_ms))?;
    let navigate_ms = nav_t.elapsed().as_millis();

    // 5. 字体 CSS 注入（CSS 域，不依赖页面 JS，与 insertCSS 同为 inspector 层）
    let inj_t = Instant::now();
    if let Some(css_text) = &font_css {
        cdp.call_timeout(Some(&tab.session_id), "CSS.enable", json!({}), budget(job))
            .map_err(|e| format!("CSS.enable: {e}"))?;
        let sheet = cdp
            .call_timeout(
                Some(&tab.session_id),
                "CSS.createStyleSheet",
                json!({ "frameId": nav["frameId"] }),
                budget(job),
            )
            .map_err(|e| format!("CSS.createStyleSheet: {e}"))?;
        let sheet_id = sheet["styleSheetId"]
            .as_str()
            .ok_or("no styleSheetId")?
            .to_string();
        cdp.call_timeout(
            Some(&tab.session_id),
            "CSS.setStyleSheetText",
            json!({ "styleSheetId": sheet_id, "text": css_text }),
            budget(job),
        )
        .map_err(|e| format!("CSS.setStyleSheetText: {e}"))?;
    }
    let font_inject_ms = inj_t.elapsed().as_millis();

    // 6. 资产等待：settleMs + (JS 开时) fonts.ready/images；JS 关时与 helper 一样
    //    evaluate 会失败 → catch，仅靠 settleMs（注释语义逐字对齐生产）。
    let aw_t = Instant::now();
    let mut asset_note = String::from("js-enabled fonts.ready+images awaited");
    if job.settle_ms > 0 {
        std::thread::sleep(Duration::from_millis(job.settle_ms));
    }
    if job.allow_javascript {
        let r = cdp.call_timeout(
            Some(&tab.session_id),
            "Runtime.evaluate",
            json!({ "expression": ASSET_WAIT_EXPR, "awaitPromise": true, "returnByValue": true }),
            budget(job),
        );
        if let Err(e) = r {
            asset_note = format!("asset wait evaluate failed (continuing): {e}");
        }
    } else {
        asset_note = String::from(
            "javascript=false: evaluate skipped (same as helper catch path); settleMs only",
        );
    }
    let asset_wait_ms = aw_t.elapsed().as_millis();

    // 7. printToPDF（参数映射：Electron 具名 pageSize → inch；margins 数值按实测
    //    生产语义直传 inch——见 Job::margins_in 注释的 bisect 证据）
    let pr_t = Instant::now();
    let (w_in, h_in) = page_size_inches(&job.page_size)?;
    let mut params = json!({
        "printBackground": job.print_background,
        "preferCSSPageSize": job.prefer_css_page_size,
        "landscape": job.landscape,
        "paperWidth": w_in,
        "paperHeight": h_in,
    });
    if let Some((t, b, l, r)) = job.margins_in {
        params["marginTop"] = json!(t);
        params["marginBottom"] = json!(b);
        params["marginLeft"] = json!(l);
        params["marginRight"] = json!(r);
    }
    let printed = cdp
        .call_timeout(
            Some(&tab.session_id),
            "Page.printToPDF",
            params,
            budget(job),
        )
        .map_err(|e| format!("printToPDF: {e}"))?;
    let b64 = printed["data"].as_str().ok_or("printToPDF: no data")?;
    let pdf = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| format!("printToPDF base64: {e}"))?;
    let print_ms = pr_t.elapsed().as_millis();

    // 8. 输出校验（结构：%PDF- 头 + %%EOF 尾 + 非空）
    if pdf.len() < 16 || !pdf.starts_with(b"%PDF-") {
        return Err("output validation failed: missing %PDF- header".into());
    }
    let tail = &pdf[pdf.len().saturating_sub(2048)..];
    if !tail.windows(5).any(|w| w == b"%%EOF") {
        return Err("output validation failed: missing %%EOF trailer".into());
    }

    use sha2::Digest;
    let digest = sha2::Sha256::digest(&pdf);
    let pdf_sha256 = digest.iter().map(|b| format!("{b:02x}")).collect();

    Ok(RenderOutcome {
        pdf,
        pdf_sha256,
        timings: RenderTimings {
            launch_ms: 0,
            attach_ms,
            navigate_ms,
            font_inject_ms,
            asset_wait_ms,
            print_ms,
            total_ms: started.elapsed().as_millis(),
        },
        asset_wait_note: asset_note,
    })
}

pub fn err_is_timeout(e: &crate::cdp::CdpError) -> bool {
    matches!(e, crate::cdp::CdpError::Timeout(_))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_sizes() {
        assert_eq!(page_size_inches("A4").unwrap(), (8.27, 11.69));
        assert_eq!(page_size_inches("Letter").unwrap(), (8.5, 11.0));
        assert!(page_size_inches("B5").is_err());
    }

    #[test]
    fn fetch_policy() {
        let roots = vec![PathBuf::from("/tmp/allowed")];
        assert!(matches!(
            decide_fetch("file:///tmp/allowed/a.png", &roots),
            FetchDecision::Allow
        ));
        assert!(matches!(
            decide_fetch("file:///etc/passwd", &roots),
            FetchDecision::Deny(_)
        ));
        assert!(matches!(
            decide_fetch("https://example.com/x", &roots),
            FetchDecision::Deny(_)
        ));
        assert!(matches!(
            decide_fetch("data:image/png;base64,AA==", &roots),
            FetchDecision::Allow
        ));
    }

    #[test]
    fn percent_decode() {
        assert_eq!(
            file_url_to_path("file:///tmp/a%20b/c.png"),
            Some(PathBuf::from("/tmp/a b/c.png"))
        );
        assert_eq!(file_url_to_path("https://x/y"), None);
    }

    #[test]
    fn font_css_extract_and_rewrite() {
        let css = "/* latin */\n@font-face {\n  font-family: 'Noto Serif SC';\n  font-style: normal;\n  font-weight: 400;\n  src: url('./fonts/noto.woff2') format('woff2');\n}\n@font-face {\n  font-family: 'Inter';\n  src: url('./fonts/inter.woff2') format('woff2');\n}\n";
        let blocks = extract_font_face_blocks(css);
        assert_eq!(blocks.len(), 2);
        assert_eq!(family_of(blocks[0]).as_deref(), Some("Noto Serif SC"));
        assert_eq!(family_of(blocks[1]).as_deref(), Some("Inter"));
    }
}
