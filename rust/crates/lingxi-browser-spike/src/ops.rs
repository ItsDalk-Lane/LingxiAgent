//! Browser capability operations over CDP, mirroring the semantics of the
//! production browser contract (lib/browser/browser-manager.ts +
//! lib/tools/browser-tool.ts + desktop/main.cjs).

use std::path::Path;
use std::time::Duration;

use base64::Engine;
use serde_json::{json, Value};

use crate::cdp::{CdpClient, CdpResult};

pub struct Tab {
    pub target_id: String,
    pub session_id: String,
}

/// Create a page target inside an optional browser context and attach flat.
pub fn open_tab(cdp: &CdpClient, context: Option<&str>, url: &str) -> CdpResult<Tab> {
    let mut params = json!({ "url": url });
    if let Some(c) = context {
        params["browserContextId"] = json!(c);
    }
    let created = cdp.call(None, "Target.createTarget", params)?;
    let target_id = created["targetId"]
        .as_str()
        .ok_or_else(|| crate::cdp::CdpError::Protocol("no targetId".into()))?
        .to_string();
    let attached = cdp.call(
        None,
        "Target.attachToTarget",
        json!({ "targetId": target_id, "flatten": true }),
    )?;
    let session_id = attached["sessionId"]
        .as_str()
        .ok_or_else(|| crate::cdp::CdpError::Protocol("no sessionId".into()))?
        .to_string();
    cdp.call(Some(&session_id), "Page.enable", json!({}))?;
    cdp.call(Some(&session_id), "Runtime.enable", json!({}))?;
    cdp.call(Some(&session_id), "DOM.enable", json!({}))?;
    Ok(Tab {
        target_id,
        session_id,
    })
}

pub fn close_tab(cdp: &CdpClient, tab: &Tab) -> CdpResult<()> {
    cdp.call(
        None,
        "Target.closeTarget",
        json!({ "targetId": tab.target_id }),
    )?;
    Ok(())
}

pub fn navigate(cdp: &CdpClient, tab: &Tab, url: &str) -> CdpResult<Value> {
    let r = cdp.call(
        Some(&tab.session_id),
        "Page.navigate",
        json!({ "url": url }),
    )?;
    // errorText is set when the load fails at the network layer (e.g. proxy dead)
    if let Some(err) = r.get("errorText").and_then(|v| v.as_str()) {
        return Ok(json!({ "ok": false, "errorText": err }));
    }
    // wait for the load event of THIS frame
    let _ = cdp.wait_event("Page.loadEventFired", |_| true, Duration::from_secs(15));
    Ok(json!({ "ok": true }))
}

/// Run the production snapshot script; returns {title, currentUrl, text}.
pub fn snapshot(cdp: &CdpClient, tab: &Tab, script: &str) -> CdpResult<Value> {
    let r = cdp.call(
        Some(&tab.session_id),
        "Runtime.evaluate",
        json!({ "expression": script, "returnByValue": true }),
    )?;
    Ok(r["result"]["value"].clone())
}

/// Click the element carrying data-hana-ref=N (same semantics as production:
/// scrollIntoView + el.click()).
pub fn click_ref(cdp: &CdpClient, tab: &Tab, refr: u64) -> CdpResult<()> {
    let expr = format!(
        "(function(){{ var el = document.querySelector('[data-hana-ref=\"{refr}\"]'); \
         if (!el) throw new Error('Element [{refr}] not found'); \
         el.scrollIntoView({{block:'center'}}); el.click(); }})()"
    );
    eval_throwing(cdp, tab, &expr)?;
    Ok(())
}

/// Find a ref number in snapshot text by a needle appearing in the segment
/// belonging to that ref (the snapshot compactor may place several [n]
/// entries on one line; the needle must fall between this ref and the next).
pub fn find_ref(snapshot_text: &str, needle: &str) -> Option<u64> {
    for line in snapshot_text.lines() {
        if !line.contains(needle) {
            continue;
        }
        // collect all [n] marker positions on this line
        let bytes = line.as_bytes();
        let mut marks: Vec<(usize, usize, u64)> = Vec::new(); // (open, close, n)
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'[' {
                if let Some(rel) = line[i + 1..].find(']') {
                    let close = i + 1 + rel;
                    if let Ok(n) = line[i + 1..close].parse::<u64>() {
                        marks.push((i, close, n));
                    }
                    i = close + 1;
                    continue;
                }
            }
            i += 1;
        }
        if marks.is_empty() {
            continue;
        }
        // find which segment (mark.close .. next mark.open) contains the needle
        for (idx, (_, close, n)) in marks.iter().enumerate() {
            let seg_end = if idx + 1 < marks.len() {
                marks[idx + 1].0
            } else {
                line.len()
            };
            if *close <= seg_end && line[*close..seg_end].contains(needle) {
                return Some(*n);
            }
        }
    }
    None
}

/// Element center in viewport coordinates (CSS px).
pub fn element_center(cdp: &CdpClient, tab: &Tab, selector: &str) -> CdpResult<(f64, f64)> {
    let expr = format!(
        "(function(){{ var el = document.querySelector({});          if (!el) throw new Error('selector not found: {selector}');          el.scrollIntoView({{block:'center'}});          var r = el.getBoundingClientRect();          return JSON.stringify({{x: r.left + r.width / 2, y: r.top + r.height / 2}}); }})()",
        json!(selector)
    );
    let v = eval_value(cdp, tab, &expr)?;
    let pt: Value = serde_json::from_str(v.as_str().unwrap_or("{}"))?;
    let x = pt["x"].as_f64().unwrap_or(0.0);
    let y = pt["y"].as_f64().unwrap_or(0.0);
    if x <= 0.0 || y <= 0.0 {
        return Err(crate::cdp::CdpError::Protocol(format!(
            "bad click point for {selector}: {x},{y}"
        )));
    }
    Ok((x, y))
}

/// One Input.dispatchMouseEvent at (x, y).
pub fn dispatch_mouse(cdp: &CdpClient, tab: &Tab, kind: &str, x: f64, y: f64) -> CdpResult<()> {
    cdp.call(
        Some(&tab.session_id),
        "Input.dispatchMouseEvent",
        json!({ "type": kind, "x": x, "y": y, "button": "left", "clickCount": 1 }),
    )?;
    Ok(())
}

/// Trusted-gesture click at an element's visual center via CDP
/// Input.dispatchMouseEvent (needed where Chrome requires a user gesture,
/// e.g. window.open popup; JS el.click() is blocked by the popup blocker).
pub fn click_selector_mouse(cdp: &CdpClient, tab: &Tab, selector: &str) -> CdpResult<()> {
    let (x, y) = element_center(cdp, tab, selector)?;
    for t in ["mousePressed", "mouseReleased"] {
        dispatch_mouse(cdp, tab, t, x, y)?;
    }
    Ok(())
}

/// Focus element by CSS selector via JS (as production `type` does), then
/// insert text through CDP Input.insertText — the same IME-compatible path
/// class as Electron's webContents.insertText (handles CJK without key codes).
pub fn type_text(cdp: &CdpClient, tab: &Tab, selector: &str, text: &str) -> CdpResult<()> {
    let focus = format!(
        "(function(){{ var el = document.querySelector({}); \
         if (!el) throw new Error('selector not found: {selector}'); \
         el.scrollIntoView({{block:'center'}}); el.focus(); if (el.select) el.select(); }})()",
        json!(selector)
    );
    eval_throwing(cdp, tab, &focus)?;
    cdp.call(
        Some(&tab.session_id),
        "Input.insertText",
        json!({ "text": text }),
    )?;
    Ok(())
}

pub fn press_enter(cdp: &CdpClient, tab: &Tab) -> CdpResult<()> {
    for t in ["rawKeyDown", "keyUp"] {
        cdp.call(
            Some(&tab.session_id),
            "Input.dispatchKeyEvent",
            json!({
                "type": t, "key": "Enter", "code": "Enter",
                "windowsVirtualKeyCode": 13, "nativeVirtualKeyCode": 13
            }),
        )?;
    }
    Ok(())
}

/// Select an option by value (same JS semantics as production `select`).
pub fn select_option(cdp: &CdpClient, tab: &Tab, selector: &str, value: &str) -> CdpResult<()> {
    let expr = format!(
        "(function(){{ var el = document.querySelector({}); \
         if (!el) throw new Error('selector not found: {selector}'); \
         el.value = {}; el.dispatchEvent(new Event('change',{{bubbles:true}})); }})()",
        json!(selector),
        json!(value)
    );
    eval_throwing(cdp, tab, &expr)?;
    Ok(())
}

pub fn scroll_by(cdp: &CdpClient, tab: &Tab, dy: i64) -> CdpResult<i64> {
    let v = eval_value(
        cdp,
        tab,
        &format!(
            "(function(){{ window.scrollBy({{top:{dy}}}); return Math.round(window.scrollY); }})()"
        ),
    )?;
    Ok(v.as_i64().unwrap_or(-1))
}

/// Evaluate an expression; exceptions become errors (production propagates too).
pub fn eval_throwing(cdp: &CdpClient, tab: &Tab, expression: &str) -> CdpResult<Value> {
    let r = cdp.call(
        Some(&tab.session_id),
        "Runtime.evaluate",
        json!({ "expression": expression, "returnByValue": true, "awaitPromise": true }),
    )?;
    if let Some(exc) = r.get("exceptionDetails") {
        let msg = exc
            .get("exception")
            .and_then(|e| e.get("description"))
            .and_then(|d| d.as_str())
            .or_else(|| exc.get("text").and_then(|t| t.as_str()))
            .unwrap_or("js exception")
            .to_string();
        return Err(crate::cdp::CdpError::Protocol(format!(
            "js exception: {msg}"
        )));
    }
    Ok(r["result"].clone())
}

pub fn eval_value(cdp: &CdpClient, tab: &Tab, expression: &str) -> CdpResult<Value> {
    Ok(eval_throwing(cdp, tab, expression)?["value"].clone())
}

/// Viewport screenshot (production: capturePage). Returns PNG bytes.
pub fn screenshot_viewport(cdp: &CdpClient, tab: &Tab) -> CdpResult<Vec<u8>> {
    let r = cdp.call(
        Some(&tab.session_id),
        "Page.captureScreenshot",
        json!({ "format": "png" }),
    )?;
    let b64 = r["data"].as_str().unwrap_or("");
    base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| crate::cdp::CdpError::Protocol(format!("screenshot b64: {e}")))
}

/// Full-page screenshot (captureBeyondViewport) — the "滚动长截图" capability.
pub fn screenshot_full_page(cdp: &CdpClient, tab: &Tab) -> CdpResult<Vec<u8>> {
    let r = cdp.call(
        Some(&tab.session_id),
        "Page.captureScreenshot",
        json!({ "format": "png", "captureBeyondViewport": true }),
    )?;
    let b64 = r["data"].as_str().unwrap_or("");
    base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| crate::cdp::CdpError::Protocol(format!("screenshot b64: {e}")))
}

/// Point the browser's download stream at `dir` (whole browser process).
pub fn set_download_dir(cdp: &CdpClient, context: Option<&str>, dir: &Path) -> CdpResult<()> {
    let mut params = json!({
        "behavior": "allow",
        "downloadPath": dir.display().to_string(),
        "eventsEnabled": true
    });
    if let Some(c) = context {
        params["browserContextId"] = json!(c);
    }
    cdp.call(None, "Browser.setDownloadBehavior", params)?;
    Ok(())
}

/// Programmatic file-input upload (agent-driven upload path).
pub fn set_file_input(
    cdp: &CdpClient,
    tab: &Tab,
    selector: &str,
    files: &[String],
) -> CdpResult<()> {
    let doc = cdp.call(
        Some(&tab.session_id),
        "DOM.getDocument",
        json!({ "depth": -1 }),
    )?;
    let root_id = doc["root"]["nodeId"]
        .as_i64()
        .ok_or_else(|| crate::cdp::CdpError::Protocol("no root nodeId".into()))?;
    let node = cdp.call(
        Some(&tab.session_id),
        "DOM.querySelector",
        json!({ "nodeId": root_id, "selector": selector }),
    )?;
    let node_id = node["nodeId"].as_i64().unwrap_or(0);
    if node_id == 0 {
        return Err(crate::cdp::CdpError::Protocol(format!(
            "selector not found: {selector}"
        )));
    }
    cdp.call(
        Some(&tab.session_id),
        "DOM.setFileInputFiles",
        json!({ "nodeId": node_id, "files": files }),
    )?;
    Ok(())
}

pub fn create_context(cdp: &CdpClient) -> CdpResult<String> {
    let r = cdp.call(
        None,
        "Target.createBrowserContext",
        json!({ "disposeOnDetach": false }),
    )?;
    Ok(r["browserContextId"].as_str().unwrap_or("").to_string())
}

pub fn dispose_context(cdp: &CdpClient, context: &str) -> CdpResult<()> {
    cdp.call(
        None,
        "Target.disposeBrowserContext",
        json!({ "browserContextId": context }),
    )?;
    Ok(())
}

/// Cookies visible to a given browser context (CDP Storage domain).
pub fn get_cookies(cdp: &CdpClient, context: Option<&str>) -> CdpResult<Value> {
    let params = match context {
        Some(c) => json!({ "browserContextId": c }),
        None => json!({}),
    };
    cdp.call(None, "Storage.getCookies", params)
}

/// Wait until `pred(expr result)` holds, polling `expression`.
pub fn wait_until(
    cdp: &CdpClient,
    tab: &Tab,
    expression: &str,
    pred: &dyn Fn(&Value) -> bool,
    timeout: Duration,
) -> CdpResult<Value> {
    let start = std::time::Instant::now();
    loop {
        if let Ok(v) = eval_value(cdp, tab, expression) {
            if pred(&v) {
                return Ok(v);
            }
        }
        if start.elapsed() > timeout {
            return Err(crate::cdp::CdpError::Timeout(format!(
                "wait_until: {expression}"
            )));
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_ref_parses_snapshot_lines() {
        let snap = "Page: T\n\n  [3] textbox \"输入标题\"\n  [7] button \"提交表单\"\n";
        assert_eq!(find_ref(snap, "输入标题"), Some(3));
        assert_eq!(find_ref(snap, "提交表单"), Some(7));
        assert_eq!(find_ref(snap, "不存在"), None);
        // compacted multi-ref single line (生产 SNAPSHOT_SCRIPT 的同构兄弟压缩)
        let compact = "  [1] link \"首页\" | [2] button \"计数:0\" | 说明文本\n";
        assert_eq!(find_ref(compact, "计数"), Some(2));
        assert_eq!(find_ref(compact, "首页"), Some(1));
    }
}
