//! PDF 只接收宿主收集的资产字节；文档不能给渲染器授予文件、网络或桌面权限。
use crate::{office_tools::OfficeApi, resourceio::OpContext};
use lingxi_kernel::browser::{PdfAsset, PdfPrintSettings, PdfRenderJob};
use regex::Regex;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

const MAX_ASSETS: usize = 512;
const MAX_TOTAL: usize = 64 * 1024 * 1024;
fn number(args: &Value, key: &str, default: f64, min: f64, max: f64) -> Result<f64, String> {
    let value = match args.get(key) {
        None => default,
        Some(Value::String(text))
            if text.len() <= 32
                && Regex::new(r"^(?:0|[1-9][0-9]*)(?:\.[0-9]+)?$")
                    .expect("固定十进制表达式")
                    .is_match(text) =>
        {
            text.parse::<f64>()
                .map_err(|_| format!("invalid decimal {key}"))?
        }
        Some(v) => v.as_f64().ok_or_else(|| {
            format!("{key} must be an integer or a decimal string; margins use inches")
        })?,
    };
    if !value.is_finite() || !(min..=max).contains(&value) {
        return Err(format!("{key} is outside the supported range"));
    }
    Ok(value)
}
fn boolean(args: &Value, key: &str, default: bool) -> Result<bool, String> {
    match args.get(key) {
        None => Ok(default),
        Some(v) => v.as_bool().ok_or_else(|| format!("{key} must be boolean")),
    }
}
fn paper(name: &str) -> Result<(f64, f64), String> {
    match name {
        "A0" => Ok((33.1, 46.8)),
        "A1" => Ok((23.4, 33.1)),
        "A2" => Ok((16.54, 23.4)),
        "A3" => Ok((11.7, 16.54)),
        "A4" => Ok((8.27, 11.69)),
        "A5" => Ok((5.83, 8.27)),
        "A6" => Ok((4.13, 5.83)),
        "Letter" => Ok((8.5, 11.0)),
        "Legal" => Ok((8.5, 14.0)),
        "Tabloid" => Ok((11.0, 17.0)),
        "Ledger" => Ok((17.0, 11.0)),
        _ => Err("unsupported PDF pageSize".into()),
    }
}
pub(crate) fn settings(args: &Value) -> Result<PdfPrintSettings, String> {
    let (width, height) = paper(args["pageSize"].as_str().unwrap_or("A4"))?;
    let landscape = boolean(args, "landscape", false)?;
    let (print_width, print_height) = if landscape {
        (height, width)
    } else {
        (width, height)
    };
    let margins = args
        .get("margins")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    if !margins.is_object()
        || margins
            .as_object()
            .unwrap()
            .keys()
            .any(|k| !["top", "bottom", "left", "right", "marginType"].contains(&k.as_str()))
    {
        return Err("invalid PDF margins".into());
    }
    if margins.get("marginType").is_some_and(|v| v != "custom") {
        return Err(
            "only explicit custom PDF margins are supported; each edge is in inches".into(),
        );
    }
    let top = number(&margins, "top", 0.4, 0.0, print_height)?;
    let bottom = number(&margins, "bottom", 0.4, 0.0, print_height)?;
    let left = number(&margins, "left", 0.4, 0.0, print_width)?;
    let right = number(&margins, "right", 0.4, 0.0, print_width)?;
    if top + bottom >= print_height || left + right >= print_width {
        return Err("PDF margins leave no printable page".into());
    }
    Ok(PdfPrintSettings {
        landscape,
        print_background: boolean(args, "printBackground", true)?,
        prefer_css_page_size: boolean(args, "preferCSSPageSize", true)?,
        paper_width: width,
        paper_height: height,
        margin_top: top,
        margin_bottom: bottom,
        margin_left: left,
        margin_right: right,
        page_ranges: String::new(),
        scale: number(args, "scale", 1.0, 0.1, 2.0)?,
    })
}
struct Assets {
    values: BTreeMap<String, PdfAsset>,
    paths: BTreeMap<PathBuf, String>,
    total: usize,
    base: String,
}
impl Assets {
    fn add(&mut self, url: String, mime: &str, bytes: Vec<u8>) -> Result<(), String> {
        if self.values.contains_key(&url) {
            return Ok(());
        }
        self.total = self
            .total
            .checked_add(bytes.len())
            .ok_or("PDF asset size overflow")?;
        if self.values.len() >= MAX_ASSETS || self.total > MAX_TOTAL {
            return Err("PDF assets exceed the bounded job budget".into());
        }
        self.values.insert(
            url.clone(),
            PdfAsset {
                url,
                mime_type: mime.into(),
                bytes,
            },
        );
        Ok(())
    }
}
fn mime(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|x| x.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "css" => "text/css",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "js" => "application/javascript",
        _ => "application/octet-stream",
    }
}
fn asset_re() -> Regex {
    // 普通超链接只保留为 PDF 链接；只有真实会加载的文档资源才读取。
    Regex::new(r#"(?i)(?:\b(?:src|poster)\s*=\s*["']([^"']+)["']|<link\b[^>]*\bhref\s*=\s*["']([^"']+)["']|url\(\s*["']?([^\s)'";]+)["']?\s*\)|@import\s+["']([^"']+)["'])"#).expect("固定资产表达式")
}
fn local_asset(raw: &str, dir: Option<&Path>) -> Result<Option<PathBuf>, String> {
    let raw = raw.trim();
    if raw.starts_with('#') || raw.starts_with("data:") || raw.starts_with("blob:") {
        return Ok(None);
    }
    // 协议相对引用必须先于路径判定拒绝：POSIX 下 "//" 前缀被 is_absolute
    // 当作绝对路径（双斜杠语义由实现定义），会绕过外链拒绝。
    if raw.starts_with("//") {
        return Err("external PDF assets must be saved as authorized local resources first".into());
    }
    if Path::new(raw).is_absolute() {
        return Ok(Some(PathBuf::from(raw)));
    }
    if let Ok(url) = reqwest::Url::parse(raw) {
        return if url.scheme() == "file" {
            url.to_file_path()
                .map(Some)
                .map_err(|_| "invalid local PDF asset URL".into())
        } else {
            Err("external PDF assets must be saved as authorized local resources first".into())
        };
    }
    let Some(dir) = dir else {
        return Err("relative PDF assets require htmlPath".into());
    };
    let name = raw.split(['?', '#']).next().unwrap_or(raw);
    let decoded = percent_encoding::percent_decode_str(name)
        .decode_utf8()
        .map_err(|_| "invalid PDF asset encoding")?;
    Ok(Some(dir.join(decoded.as_ref())))
}
fn rewrite<'a>(
    api: &'a OfficeApi,
    text: String,
    dir: Option<PathBuf>,
    op: &'a OpContext,
    assets: &'a mut Assets,
    depth: usize,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, String>> + Send + 'a>> {
    Box::pin(async move {
        if depth > 8 {
            return Err("PDF stylesheet nesting exceeds limit".into());
        }
        let refs = asset_re()
            .captures_iter(&text)
            .filter_map(|c| {
                (1..=4)
                    .find_map(|i| c.get(i))
                    .map(|m| m.as_str().to_owned())
            })
            .collect::<Vec<_>>();
        let mut output = text;
        for raw in refs {
            let Some(path) = local_asset(&raw, dir.as_deref())? else {
                continue;
            };
            // 每个资产重新经过正式资源授权，不把文档所在目录当作无限权限。
            let (actual, bytes, _) = api
                .read_resource(&serde_json::json!({"kind":"local-file","path":path}), op)
                .await?;
            let actual = actual.ok_or("PDF asset did not resolve to a local snapshot")?;
            let url = if let Some(url) = assets.paths.get(&actual) {
                url.clone()
            } else {
                let url = format!(
                    "{}asset/{}",
                    assets.base,
                    lingxi_protocol::canon::sha256_hex(actual.to_string_lossy().as_bytes())
                );
                assets.paths.insert(actual.clone(), url.clone());
                let content = if mime(&actual) == "text/css" {
                    let css = String::from_utf8(bytes).map_err(|_| "PDF CSS is not UTF-8")?;
                    rewrite(
                        api,
                        css,
                        actual.parent().map(Path::to_path_buf),
                        op,
                        assets,
                        depth + 1,
                    )
                    .await?
                    .into_bytes()
                } else {
                    bytes
                };
                assets.add(url.clone(), mime(&actual), content)?;
                url
            };
            output = output.replace(&raw, &url);
        }
        Ok(output)
    })
}
fn trusted_fonts(root: &Path, assets: &mut Assets) -> Result<String, String> {
    let root = root
        .canonicalize()
        .map_err(|_| "bundled PDF fonts are unavailable")?;
    let css_path = root.join("new-warm-paper-fonts.css");
    let css = crate::file_safety::Snapshot::read(&css_path)
        .map_err(|_| "bundled PDF font stylesheet is unavailable")?;
    let css = String::from_utf8(css.bytes).map_err(|_| "bundled font stylesheet is not UTF-8")?;
    let blocks = Regex::new(r"(?is)@font-face\s*\{[^}]*\}").unwrap();
    let family = Regex::new(r#"(?i)font-family\s*:\s*["']?([^;'"}]+)"#).unwrap();
    let urls = Regex::new(r#"(?i)url\(\s*["']?([^)'"\s]+)["']?\s*\)"#).unwrap();
    let mut found = std::collections::BTreeSet::new();
    let mut output = String::new();
    for block in blocks.find_iter(&css) {
        let Some(name) = family
            .captures(block.as_str())
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().trim())
        else {
            continue;
        };
        if !["EB Garamond", "Noto Serif SC", "JetBrains Mono"].contains(&name) {
            continue;
        }
        let mut next = block.as_str().to_owned();
        for captures in urls.captures_iter(block.as_str()) {
            let raw = captures.get(1).unwrap().as_str();
            let path = root
                .join(raw)
                .canonicalize()
                .map_err(|_| "a bundled PDF font is missing")?;
            if !path.starts_with(&root) {
                return Err("bundled font leaves the trusted asset root".into());
            }
            let bytes = crate::file_safety::Snapshot::read(&path)
                .map_err(|_| "cannot snapshot bundled PDF font")?
                .bytes;
            let url = format!(
                "{}font/{}",
                assets.base,
                lingxi_protocol::canon::sha256_hex(&bytes)
            );
            assets.add(url.clone(), mime(&path), bytes)?;
            next = next.replace(raw, &url);
        }
        found.insert(name.to_owned());
        output.push_str(&next);
        output.push('\n');
    }
    if found.len() != 3 {
        return Err("all three bundled PDF font families are required".into());
    }
    Ok(output)
}
pub(crate) async fn collect(
    api: &OfficeApi,
    args: &Value,
    op: &OpContext,
    operation: &str,
) -> Result<PdfRenderJob, String> {
    let has_html = args.get("html").is_some();
    let has_path = args.get("htmlPath").is_some();
    if has_html == has_path {
        return Err("provide exactly one of html or htmlPath".into());
    }
    let (html, dir) = if has_html {
        (
            args["html"].as_str().ok_or("html must be text")?.to_owned(),
            None,
        )
    } else {
        let path = args["htmlPath"]
            .as_str()
            .ok_or("htmlPath must be an absolute path")?;
        let (actual, bytes, _) = api
            .read_resource(&serde_json::json!({"kind":"local-file","path":path}), op)
            .await?;
        (
            String::from_utf8(bytes).map_err(|_| "PDF HTML is not UTF-8")?,
            actual.and_then(|p| p.parent().map(Path::to_path_buf)),
        )
    };
    if html.len() > 8 * 1024 * 1024 {
        return Err("PDF HTML exceeds 8MiB".into());
    }
    let base = format!(
        "https://lingxi-pdf.invalid/{}/",
        lingxi_protocol::canon::sha256_hex(operation.as_bytes())
    );
    let mut assets = Assets {
        values: BTreeMap::new(),
        paths: BTreeMap::new(),
        total: 0,
        base: base.clone(),
    };
    let mut html = rewrite(api, html, dir, op, &mut assets, 0).await?;
    if boolean(args, "embedLingxiFonts", true)? {
        let root = &api
            .assets
            .as_ref()
            .ok_or("bundled Office assets are unavailable")?
            .themes_dir;
        let css = trusted_fonts(root, &mut assets)?;
        let head = Regex::new(r"(?i)<head\b[^>]*>").expect("固定head表达式");
        if let Some(found) = head.find(&html) {
            html.insert_str(found.end(), &format!("<style>{css}</style>"));
        } else {
            html = format!(
                "<!doctype html><html><head><style>{css}</style></head><body>{html}</body></html>"
            );
        }
    }
    Ok(PdfRenderJob {
        html,
        base_url: format!("{base}index.html"),
        assets: assets.values.into_values().collect(),
        settings: settings(args)?,
        allow_javascript: boolean(args, "allowJavaScript", false)?,
        timeout_ms: number(args, "timeoutMs", 60000.0, 1000.0, 300000.0)? as u64,
        viewport_width: number(args, "viewportWidth", 1280.0, 320.0, 4096.0)? as u32,
        viewport_height: number(args, "viewportHeight", 900.0, 320.0, 4096.0)? as u32,
        settle_ms: number(args, "settleMs", 250.0, 0.0, 30000.0)? as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn paper_and_margins_keep_existing_inches_and_landscape_printable_area() {
        let a5 = settings(
            &json!({"pageSize":"A5","margins":{"marginType":"custom","top":"0.5","left":"0.25"},"scale":"0.8","preferCSSPageSize":false}),
        )
        .unwrap();
        assert_eq!((a5.paper_width, a5.paper_height), (5.83, 8.27));
        assert_eq!((a5.margin_top, a5.margin_left), (0.5, 0.25));
        assert!(!a5.prefer_css_page_size);
        assert_eq!(a5.margin_bottom, 0.4);
        assert_eq!(a5.scale, 0.8);
        let default = settings(&json!({})).unwrap();
        assert_eq!(
            (
                default.margin_top,
                default.margin_bottom,
                default.margin_left,
                default.margin_right
            ),
            (0.4, 0.4, 0.4, 0.4)
        );
        assert!(settings(&json!({"landscape":true,"margins":{"top":5,"bottom":4}})).is_err());
        assert!(settings(&json!({"margins":{"top":"10mm"}})).is_err());
    }

    #[test]
    fn loaded_assets_are_distinct_from_links_and_external_assets_fail_explicitly() {
        let html = "<a href='https://example.org/'>link</a><link rel='stylesheet' href='style.css'><img src='figure.png'><style>@import 'nested.css';p{background:url(bg.png)}</style>";
        let captures = asset_re()
            .captures_iter(html)
            .filter_map(|c| {
                (1..=4)
                    .find_map(|i| c.get(i))
                    .map(|m| m.as_str().to_owned())
            })
            .collect::<Vec<_>>();
        assert_eq!(
            captures,
            vec!["style.css", "figure.png", "nested.css", "bg.png"]
        );
        assert!(local_asset("https://example.org/secret.png", None).is_err());
        assert!(local_asset("//example.org/secret.png", None).is_err());
        assert!(local_asset("relative.png", None).is_err());
        assert!(local_asset("data:image/png;base64,AA==", None)
            .unwrap()
            .is_none());
    }
}
