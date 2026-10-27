//! 页面操作仅在所属target上运行；引用与文档世代绑定，不复用旧快照编号。
use super::{
    error,
    host::Core,
    runtime::{allowed_url, Live, MAX_TABS},
    BrowserError,
};
use base64::Engine;
use lingxi_kernel::browser::BrowserScope;
use lingxi_protocol::{BrowserAction as A, BrowserCommand, BrowserOpenBehavior, BrowserSettings};
use serde_json::{json, Value};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

const SNAPSHOT: &str = include_str!("snapshot.js");
fn navigation_error_text(raw: &str) -> &str {
    // Chrome的标准网络错误编号可保留；地址、页面文字和其他任意详情一律隐藏。
    raw.split_whitespace()
        .find(|part| {
            part.len() <= 128
                && part.strip_prefix("net::ERR_").is_some_and(|suffix| {
                    !suffix.is_empty()
                        && suffix
                            .bytes()
                            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_')
                })
        })
        .unwrap_or("非标准导航错误详情已隐藏")
}
pub(super) fn evaluate(
    live: &Live,
    sid: &str,
    expression: &str,
    deadline: Instant,
    cancel: &AtomicBool,
    effect: bool,
) -> Result<Value, BrowserError> {
    evaluate_context(live, sid, None, expression, deadline, cancel, effect)
}
fn evaluate_context(
    live: &Live,
    sid: &str,
    context: Option<u64>,
    expression: &str,
    deadline: Instant,
    cancel: &AtomicBool,
    effect: bool,
) -> Result<Value, BrowserError> {
    let mut params = json!({"expression":expression,"returnByValue":true,"awaitPromise":true});
    if let Some(context) = context {
        params["contextId"] = json!(context);
    }
    let r = live.call(
        Some(sid),
        "Runtime.evaluate",
        params,
        deadline,
        cancel,
        effect,
    )?;
    if r.get("exceptionDetails").is_some() {
        return Err(error("browser_javascript", "页面脚本执行失败", effect));
    }
    if r["result"]["subtype"] == "error" {
        return Err(error("browser_javascript", "页面返回了脚本错误", effect));
    }
    Ok(r["result"].get("value").cloned().unwrap_or(Value::Null))
}
pub(super) fn new_tab(
    live: &Live,
    url: &str,
    visible: bool,
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<String, BrowserError> {
    if !allowed_url(url) {
        return Err(error(
            "browser_url_forbidden",
            "浏览器仅允许http/https或空白页",
            false,
        ));
    }
    live.refresh(deadline, cancel)?;
    if live.tabs.lock().unwrap_or_else(|p| p.into_inner()).len() >= MAX_TABS {
        return Err(error("browser_capacity", "本会话标签页已达上限", false));
    }
    let first = live
        .tabs
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .is_empty();
    let created = live.call(
        None,
        "Target.createTarget",
        // no-startup-window下首个target必须同时创建窗口；headless窗口仍不可见。
        json!({"url":"about:blank","newWindow":first}),
        deadline,
        cancel,
        true,
    )?;
    let id = created["targetId"]
        .as_str()
        .ok_or_else(|| error("browser_protocol", "新标签没有真实编号", true))?
        .to_string();
    let result = (|| {
        live.refresh(deadline, cancel)?;
        let sid = live.attach(&id, deadline, cancel)?;
        if first && visible {
            let window = live.call(
                None,
                "Browser.getWindowForTarget",
                json!({"targetId":id}),
                deadline,
                cancel,
                false,
            )?;
            let state = window["bounds"]["windowState"]
                .as_str()
                .ok_or_else(|| error("browser_protocol", "浏览器可见窗口状态无效", false))?;
            live.visible.store(state != "minimized", Ordering::Release);
        }
        *live.active.lock().unwrap_or_else(|p| p.into_inner()) = Some(id.clone());
        if url != "about:blank" {
            navigate(live, &sid, url, deadline, cancel)?;
        }
        Ok(id.clone())
    })();
    if result.is_err() {
        let _ = live.call(
            None,
            "Target.closeTarget",
            json!({"targetId":id}),
            Instant::now() + Duration::from_secs(1),
            &AtomicBool::new(false),
            true,
        );
    }
    result
}
pub(super) fn navigate(
    live: &Live,
    sid: &str,
    url: &str,
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<Value, BrowserError> {
    if !allowed_url(url) {
        return Err(error(
            "browser_url_forbidden",
            "浏览器地址不在允许的协议范围",
            false,
        ));
    }
    let cursor = live.cdp().cursor();
    let nav = live.call(
        Some(sid),
        "Page.navigate",
        json!({"url":url}),
        deadline,
        cancel,
        true,
    )?;
    if nav["isDownload"] == true || nav["errorText"].as_str() == Some("net::ERR_ABORTED") {
        let frame = nav["frameId"].as_str().map(String::from);
        let event = live.cdp().wait_event(
            cursor,
            |e| {
                e["method"] == "Browser.downloadWillBegin"
                    && frame.as_ref().map_or_else(
                        || e["params"]["url"] == url,
                        |f| e["params"]["frameId"] == *f,
                    )
            },
            deadline,
            cancel,
        )?;
        let guid = event["params"]["guid"]
            .as_str()
            .ok_or_else(|| error("browser_download_unknown", "下载缺少真实编号", true))?;
        loop {
            if let Some(e) = live
                .failure
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone()
            {
                return Err(e);
            }
            if live
                .downloads
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .iter()
                .any(|d| d.id == guid && d.completed && d.bytes.is_some())
            {
                return Ok(json!({"downloadId":guid,"completed":true}));
            }
            if cancel.load(Ordering::Acquire) || live.cdp().is_closed() {
                return Err(error("browser_download_unknown", "下载完成尚未确认", true));
            }
            if Instant::now() >= deadline {
                return Err(error(
                    "browser_download_unknown",
                    "下载未在预算内完成",
                    true,
                ));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    if let Some(cause) = nav["errorText"].as_str() {
        return Err(error(
            "browser_navigation",
            &format!(
                "浏览器导航失败，未回退其他网络路径 [{}]",
                navigation_error_text(cause)
            ),
            false,
        ));
    }
    if let Some(loader) = nav["loaderId"].as_str() {
        let frame = nav["frameId"]
            .as_str()
            .ok_or_else(|| error("browser_protocol", "导航缺少frame身份", true))?;
        live.cdp().wait_event(
            cursor,
            |e| {
                e["sessionId"] == sid
                    && e["method"] == "Page.lifecycleEvent"
                    && e["params"]["frameId"] == frame
                    && e["params"]["loaderId"] == loader
                    && e["params"]["name"] == "load"
            },
            deadline,
            cancel,
        )?;
    }
    Ok(nav)
}

fn loader(
    live: &Live,
    sid: &str,
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<String, BrowserError> {
    let tree = live.call(
        Some(sid),
        "Page.getFrameTree",
        json!({}),
        deadline,
        cancel,
        false,
    )?;
    tree["frameTree"]["frame"]["loaderId"]
        .as_str()
        .map(String::from)
        .ok_or_else(|| error("browser_protocol", "浏览器文档身份无效", false))
}
fn history_navigation(
    live: &Live,
    sid: &str,
    entry: Option<&Value>,
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<(), BrowserError> {
    let tree = live.call(
        Some(sid),
        "Page.getFrameTree",
        json!({}),
        deadline,
        cancel,
        false,
    )?;
    let frame = tree["frameTree"]["frame"]["id"]
        .as_str()
        .ok_or_else(|| error("browser_protocol", "导航缺少真实frame", false))?;
    let previous = tree["frameTree"]["frame"]["loaderId"]
        .as_str()
        .ok_or_else(|| error("browser_protocol", "导航缺少真实文档", false))?;
    let expected_url = entry
        .and_then(|entry| entry["url"].as_str())
        .or_else(|| tree["frameTree"]["frame"]["url"].as_str())
        .ok_or_else(|| error("browser_protocol", "导航缺少真实地址", false))?;
    let cursor = live.cdp().cursor();
    if let Some(entry) = entry {
        live.call(
            Some(sid),
            "Page.navigateToHistoryEntry",
            json!({"entryId":entry["id"]}),
            deadline,
            cancel,
            true,
        )?;
    } else {
        live.call(Some(sid), "Page.reload", json!({}), deadline, cancel, true)?;
    }
    // 重载必须观察到新loader；历史跳转也允许真实同文档/缓存恢复事件。
    let event = live.cdp().wait_event(
        cursor,
        |event| {
            if event["sessionId"] != sid {
                return false;
            }
            (event["method"] == "Page.frameNavigated"
                && event["params"]["frame"]["id"] == frame
                && (entry.is_some()
                    || event["params"]["frame"]["loaderId"]
                        .as_str()
                        .is_some_and(|id| id != previous)))
                || (entry.is_some()
                    && event["method"] == "Page.navigatedWithinDocument"
                    && event["params"]["frameId"] == frame)
        },
        deadline,
        cancel,
    )?;
    let observed_loader = event["params"]["frame"]["loaderId"].as_str();
    loop {
        let current = live.call(
            Some(sid),
            "Page.getFrameTree",
            json!({}),
            deadline,
            cancel,
            false,
        )?;
        let frame_now = &current["frameTree"]["frame"];
        let document = evaluate(
            live,
            sid,
            "({ready:document.readyState,url:document.URL})",
            deadline,
            cancel,
            false,
        )?;
        if frame_now["id"] == frame
            && observed_loader.is_none_or(|id| frame_now["loaderId"] == id)
            && document["ready"] == "complete"
            && document["url"] == frame_now["url"]
            && document["url"] == expected_url
        {
            if let Some(expected) = entry {
                let history = live.call(
                    Some(sid),
                    "Page.getNavigationHistory",
                    json!({}),
                    deadline,
                    cancel,
                    false,
                )?;
                let matching = history["currentIndex"]
                    .as_u64()
                    .and_then(|i| {
                        history["entries"]
                            .as_array()
                            .and_then(|entries| entries.get(i as usize))
                    })
                    .is_some_and(|actual| actual["id"] == expected["id"]);
                if matching {
                    return Ok(());
                }
            } else {
                return Ok(());
            }
        }
        if cancel.load(Ordering::Acquire) {
            return Err(error("cancelled", "导航等待已取消", true));
        }
        if Instant::now() >= deadline {
            return Err(error("browser_timeout", "导航未在预算内完成", true));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn snapshot(
    live: &Live,
    tab: &str,
    sid: &str,
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<Value, BrowserError> {
    let document = loader(live, sid, deadline, cancel)?;
    let base = live
        .refs
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
            n.checked_add(65536)
        })
        .map_err(|_| {
            error(
                "browser_reference_exhausted",
                "浏览器引用编号已耗尽，请重新建立页面",
                false,
            )
        })?;
    let frame = live.call(
        Some(sid),
        "Page.getFrameTree",
        json!({}),
        deadline,
        cancel,
        false,
    )?;
    let world = live.call(
        Some(sid),
        "Page.createIsolatedWorld",
        json!({"frameId":frame["frameTree"]["frame"]["id"],"worldName":"LingxiSnapshot"}),
        deadline,
        cancel,
        false,
    )?;
    let context = world["executionContextId"]
        .as_u64()
        .ok_or_else(|| error("browser_protocol", "快照隔离上下文无效", false))?;
    // 页面能改DOM属性，但不能改隔离world保存的真实Element引用。
    let script=format!("(function(){{const result=({SNAPSHOT});const base={base};const refs=new Map();const frozen=new Map();document.querySelectorAll('[data-hana-ref]').forEach(el=>{{const n=Number(el.getAttribute('data-hana-ref'));if(!Number.isInteger(n)||n<1||n>=65536)throw new Error('snapshot reference limit');refs.set(n,n+base);frozen.set(n+base,el);el.setAttribute('data-hana-ref',String(n+base));}});globalThis.__lingxiFrozenReferences=frozen;result.text=result.text.replace(/\\[(\\d+)\\]/g,(m,n)=>refs.has(Number(n))?'['+refs.get(Number(n))+']':m);return result;}})()" );
    let mut value = evaluate_context(live, sid, Some(context), &script, deadline, cancel, false)?;
    if !value.is_object()
        || !value["text"].is_string()
        || !value["title"].is_string()
        || !value["currentUrl"].is_string()
    {
        return Err(error("browser_protocol", "浏览器快照缺少实际正文", false));
    }
    if !allowed_url(value["currentUrl"].as_str().unwrap_or("")) {
        return Err(error(
            "browser_url_forbidden",
            "当前页面协议不允许读取",
            false,
        ));
    }
    if value["text"].as_str().unwrap().len() > 256 * 1024 {
        return Err(error("browser_budget", "浏览器快照超过正文预算", false));
    }
    if document != loader(live, sid, deadline, cancel)? {
        return Err(error(
            "browser_stale_snapshot",
            "生成快照时页面已更换，请重新读取",
            false,
        ));
    }
    if let Some(a) = live
        .attached
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get_mut(tab)
    {
        a.snapshot_loader = Some(document);
        a.reference_range = Some((base + 1, base + 65535));
        a.reference_context = Some(context);
    }
    value["tabId"] = json!(tab);
    value["url"] = value["currentUrl"].clone();
    value["snapshot"] = value["text"].clone();
    Ok(value)
}
struct Reference {
    context: u64,
    expression: String,
}
fn require_reference(
    live: &Live,
    tab: &str,
    sid: &str,
    reference: u32,
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<Reference, BrowserError> {
    let a = live
        .attached
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get(tab)
        .cloned()
        .ok_or_else(|| error("browser_stale_snapshot", "请先读取当前标签快照", false))?;
    if a.snapshot_loader.as_deref() != Some(loader(live, sid, deadline, cancel)?.as_str())
        || !a
            .reference_range
            .is_some_and(|(lo, hi)| (lo..=hi).contains(&reference))
    {
        return Err(error(
            "browser_stale_snapshot",
            "页面或快照已更新，旧引用不可使用",
            false,
        ));
    }
    let context = a
        .reference_context
        .ok_or_else(|| error("browser_stale_snapshot", "快照上下文已失效", false))?;
    let expression = format!("globalThis.__lingxiFrozenReferences?.get({reference})");
    let connected=evaluate_context(live,sid,Some(context),&format!("(()=>{{const el={expression};return !!el&&el.isConnected&&el.ownerDocument===document;}})()"),deadline,cancel,false)?;
    if connected != true {
        return Err(error(
            "browser_stale_snapshot",
            "快照中的元素已被替换，不能改用伪造同编号元素",
            false,
        ));
    }
    Ok(Reference {
        context,
        expression,
    })
}
fn visible(
    live: &Live,
    tab: &str,
    state: &str,
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<(), BrowserError> {
    live.call(
        None,
        "Target.activateTarget",
        json!({"targetId":tab}),
        deadline,
        cancel,
        false,
    )?;
    let window = live.call(
        None,
        "Browser.getWindowForTarget",
        json!({"targetId":tab}),
        deadline,
        cancel,
        false,
    )?;
    live.call(
        None,
        "Browser.setWindowBounds",
        json!({"windowId":window["windowId"],"bounds":{"windowState":state}}),
        deadline,
        cancel,
        false,
    )?;
    live.visible.store(state != "minimized", Ordering::Release);
    Ok(())
}
fn key(
    live: &Live,
    sid: &str,
    key: &str,
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<(), BrowserError> {
    let mut parts = key.split('+').collect::<Vec<_>>();
    let key = parts.pop().unwrap_or("");
    let mut modifiers = 0;
    for modifier in parts {
        modifiers |= match modifier.to_ascii_lowercase().as_str() {
            "alt" => 1,
            "control" | "ctrl" => 2,
            "meta" | "command" | "cmd" => 4,
            "shift" => 8,
            _ => return Err(error("browser_key", "浏览器按键修饰符无效", false)),
        };
    }
    let (key, code, n) = match key {
        "Enter" | "Return" => ("Enter", "Enter", 13),
        "Tab" => ("Tab", "Tab", 9),
        "Escape" => ("Escape", "Escape", 27),
        "Backspace" => ("Backspace", "Backspace", 8),
        "Delete" => ("Delete", "Delete", 46),
        "Space" => (" ", "Space", 32),
        "ArrowDown" => ("ArrowDown", "ArrowDown", 40),
        "ArrowUp" => ("ArrowUp", "ArrowUp", 38),
        "ArrowLeft" => ("ArrowLeft", "ArrowLeft", 37),
        "ArrowRight" => ("ArrowRight", "ArrowRight", 39),
        _ if key.chars().count() == 1 => (
            key,
            key,
            key.chars().next().unwrap().to_ascii_uppercase() as u32,
        ),
        _ => return Err(error("browser_key", "当前浏览器不支持此按键", false)),
    };
    for event in ["rawKeyDown", "keyUp"] {
        live.call(Some(sid),"Input.dispatchKeyEvent",json!({"type":event,"key":key,"code":code,"windowsVirtualKeyCode":n,"modifiers":modifiers}),deadline,cancel,true)?;
    }
    Ok(())
}
pub(super) fn execute(
    core: &Core,
    scope: &BrowserScope,
    live: &Live,
    command: &BrowserCommand,
    settings: &BrowserSettings,
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<(Value, Option<Vec<u8>>), BrowserError> {
    core.current(scope, cancel)?;
    if !scope.human
        && live.takeover.load(Ordering::Acquire)
        && (command.action.modifies_page()
            || matches!(
                command.action,
                A::Scroll | A::SwitchTab | A::Show | A::Suspend
            ))
    {
        return Err(error(
            "browser_user_takeover",
            "用户正在操作同一页面，请等待归还控制",
            false,
        ));
    }
    live.refresh(deadline, cancel)?;
    if matches!(command.action, A::NewTab)
        || (command.action == A::Navigate
            && command.tab_id.is_none()
            && settings.agent_open_behavior == BrowserOpenBehavior::NewTab)
    {
        let tab = new_tab(
            live,
            command.url.as_deref().unwrap_or("about:blank"),
            !scope.execution_node.is_empty() && live.visible.load(Ordering::Acquire),
            deadline,
            cancel,
        )?;
        let sid = live.attach(&tab, deadline, cancel)?;
        return Ok((snapshot(live, &tab, &sid, deadline, cancel)?, None));
    }
    let tab = command.tab_id.clone().or_else(|| {
        live.active
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    });
    if matches!(command.action, A::Start | A::Resume | A::Show) && tab.is_none() {
        return Ok((json!({"running":true,"empty":true}), None));
    }
    let tab = tab.ok_or_else(|| error("browser_empty_workspace", "当前会话没有标签页", false))?;
    let sid = live.attach(&tab, deadline, cancel)?;
    core.current(scope, cancel)?;
    match command.action {
        A::Start | A::Resume => {}
        A::Navigate => {
            let r = navigate(
                live,
                &sid,
                command.url.as_deref().unwrap(),
                deadline,
                cancel,
            )?;
            if r["completed"] == true && r["downloadId"].is_string() {
                return Ok((r, None));
            }
        }
        A::Snapshot => {}
        A::Screenshot => {
            let r = live.call(
                Some(&sid),
                "Page.captureScreenshot",
                json!({"format":"png"}),
                deadline,
                cancel,
                false,
            )?;
            let encoded = r["data"]
                .as_str()
                .ok_or_else(|| error("browser_protocol", "截图缺少真实图片", false))?;
            if encoded.len() > 24 * 1024 * 1024 {
                return Err(error("browser_budget", "截图超过图片预算", false));
            }
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(|_| error("browser_protocol", "截图编码无效", false))?;
            if bytes.len() < 24
                || !bytes.starts_with(b"\x89PNG\r\n\x1a\n")
                || &bytes[12..16] != b"IHDR"
            {
                return Err(error("browser_protocol", "截图不是有效PNG", false));
            }
            return Ok((
                json!({"tabId":tab,"mimeType":"image/png","bytes":bytes.len()}),
                Some(bytes),
            ));
        }
        A::Click => {
            let reference = require_reference(
                live,
                &tab,
                &sid,
                command.element_ref.unwrap(),
                deadline,
                cancel,
            )?;
            let point=evaluate_context(live,&sid,Some(reference.context),&format!("(()=>{{const el={};if(!el||!el.isConnected)throw new Error('stale ref');el.scrollIntoView({{block:'center'}});const r=el.getBoundingClientRect();return {{x:r.left+r.width/2,y:r.top+r.height/2}};}})()",reference.expression),deadline,cancel,false)?;
            let x = point["x"]
                .as_f64()
                .filter(|x| x.is_finite())
                .ok_or_else(|| error("browser_element", "元素位置无效", false))?;
            let y = point["y"]
                .as_f64()
                .filter(|x| x.is_finite())
                .ok_or_else(|| error("browser_element", "元素位置无效", false))?;
            for event in ["mousePressed", "mouseReleased"] {
                live.call(
                    Some(&sid),
                    "Input.dispatchMouseEvent",
                    json!({"type":event,"x":x,"y":y,"button":"left","clickCount":1}),
                    deadline,
                    cancel,
                    true,
                )?;
            }
        }
        A::Type => {
            if let Some(reference) = command.element_ref {
                let reference = require_reference(live, &tab, &sid, reference, deadline, cancel)?;
                let focused=evaluate_context(live,&sid,Some(reference.context),&format!("(()=>{{const el={};if(!el||!el.isConnected)return false;el.scrollIntoView({{block:'center'}});el.focus();if(el.select)el.select();return document.activeElement===el;}})()",reference.expression),deadline,cancel,true)?;
                if focused != true {
                    return Err(error(
                        "browser_stale_snapshot",
                        "原始输入元素已经失效或无法聚焦",
                        false,
                    ));
                }
            }
            live.call(
                Some(&sid),
                "Input.insertText",
                json!({"text":command.text}),
                deadline,
                cancel,
                true,
            )?;
            if command.press_enter == Some(true) {
                key(live, &sid, "Enter", deadline, cancel)?;
            }
        }
        A::Select => {
            let reference = require_reference(
                live,
                &tab,
                &sid,
                command.element_ref.unwrap(),
                deadline,
                cancel,
            )?;
            evaluate_context(live,&sid,Some(reference.context),&format!("(()=>{{const el={};if(!el||!el.isConnected)throw new Error('stale ref');el.value={};el.dispatchEvent(new Event('change',{{bubbles:true}}));}})()",reference.expression,json!(command.value)),deadline,cancel,true)?;
        }
        A::Key => key(
            live,
            &sid,
            command.key.as_deref().unwrap(),
            deadline,
            cancel,
        )?,
        A::Scroll => {
            let delta = command.amount.unwrap_or(3.0)
                * 300.0
                * if command.direction.as_deref() == Some("up") {
                    -1.0
                } else {
                    1.0
                };
            evaluate(
                live,
                &sid,
                &format!("window.scrollBy({{top:{delta}}});true"),
                deadline,
                cancel,
                true,
            )?;
        }
        A::Evaluate => {
            let value = evaluate(
                live,
                &sid,
                command.expression.as_deref().unwrap(),
                deadline,
                cancel,
                true,
            )?;
            if serde_json::to_vec(&value)
                .map_err(|_| error("browser_protocol", "脚本结果无效", false))?
                .len()
                > 1024 * 1024
            {
                return Err(error("browser_budget", "脚本结果超过预算", true));
            }
            return Ok((json!({"tabId":tab,"value":value}), None));
        }
        A::Wait => {
            let state = command.state.as_deref().unwrap_or("stable");
            if !matches!(state, "stable" | "load" | "domcontentloaded") {
                return Err(error("browser_wait", "浏览器等待状态无效", false));
            }
            let mut previous = None;
            let mut stable = 0;
            loop {
                core.current(scope, cancel)?;
                let now=evaluate(live,&sid,"({ready:document.readyState,signature:document.body?document.body.innerText.slice(0,30000):''})",deadline,cancel,false)?;
                if now["ready"] == "complete"
                    || (state == "domcontentloaded" && now["ready"] == "interactive")
                {
                    if state != "stable" || previous.as_ref() == Some(&now) {
                        stable += 1;
                    } else {
                        stable = 0;
                    }
                    if state != "stable" || stable >= 2 {
                        break;
                    }
                }
                previous = Some(now);
                if Instant::now() >= deadline {
                    return Err(error("browser_timeout", "页面没有在预算内稳定", false));
                }
                std::thread::sleep(
                    Duration::from_millis(100)
                        .min(deadline.saturating_duration_since(Instant::now())),
                );
            }
        }
        A::Show | A::Takeover => {
            visible(live, &tab, "normal", deadline, cancel)?;
            if command.action == A::Takeover {
                live.takeover.store(true, Ordering::Release);
                for a in live
                    .attached
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .values_mut()
                {
                    a.snapshot_loader = None;
                    a.reference_range = None;
                    a.reference_context = None;
                }
            }
        }
        A::Release => {
            live.takeover.store(false, Ordering::Release);
            for a in live
                .attached
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .values_mut()
            {
                a.snapshot_loader = None;
                a.reference_range = None;
                a.reference_context = None;
            }
        }
        A::Suspend => {
            if live.visible.load(Ordering::Acquire) {
                visible(live, &tab, "minimized", deadline, cancel)?;
            }
        }
        A::SwitchTab => {
            live.call(
                None,
                "Target.activateTarget",
                json!({"targetId":tab}),
                deadline,
                cancel,
                false,
            )?;
            *live.active.lock().unwrap_or_else(|p| p.into_inner()) = Some(tab.clone());
        }
        A::CloseTab => {
            let cursor = live.cdp().cursor();
            live.call(
                None,
                "Target.closeTarget",
                json!({"targetId":tab}),
                deadline,
                cancel,
                true,
            )?;
            live.cdp().wait_event(
                cursor,
                |event| {
                    event["method"] == "Target.targetDestroyed"
                        && event["params"]["targetId"] == tab
                },
                deadline,
                cancel,
            )?;
            live.refresh(deadline, cancel)?;
            return Ok((json!({"closedTabId":tab}), None));
        }
        A::Back | A::Forward => {
            let h = live.call(
                Some(&sid),
                "Page.getNavigationHistory",
                json!({}),
                deadline,
                cancel,
                false,
            )?;
            let i = h["currentIndex"]
                .as_i64()
                .ok_or_else(|| error("browser_protocol", "导航历史无效", false))?
                + if command.action == A::Back { -1 } else { 1 };
            let entry = h["entries"]
                .as_array()
                .and_then(|v| usize::try_from(i).ok().and_then(|i| v.get(i)))
                .ok_or_else(|| error("browser_history", "此方向没有可用的导航历史", false))?;
            if !entry["url"].as_str().is_some_and(allowed_url) {
                return Err(error("browser_url_forbidden", "历史地址协议不允许", false));
            }
            history_navigation(live, &sid, Some(entry), deadline, cancel)?;
        }
        A::Reload => {
            history_navigation(live, &sid, None, deadline, cancel)?;
        }
        A::NewTab | A::Stop | A::Destroy => unreachable!("宿主已处理此操作"),
    }
    live.refresh(deadline, cancel)?;
    snapshot(live, &tab, &sid, deadline, cancel).map(|r| (r, None))
}

#[cfg(test)]
mod navigation_diagnostics_tests {
    use super::navigation_error_text;

    #[test]
    fn navigation_diagnostic_keeps_network_code_without_urls_credentials_or_arbitrary_text() {
        assert_eq!(
            navigation_error_text(
                "net::ERR_FAILED https://fixture.invalid/path?password=synthetic-private-marker"
            ),
            "net::ERR_FAILED"
        );
        assert_eq!(
            navigation_error_text("net::ERR_CONNECTION_CLOSED"),
            "net::ERR_CONNECTION_CLOSED"
        );
        for raw in [
            "https://user:synthetic-private-marker@fixture.invalid/body",
            "arbitrary page text synthetic-private-marker",
            "net::ERR_FAILED/synthetic-private-marker",
            "net::ERR_",
            "",
        ] {
            assert_eq!(navigation_error_text(raw), "非标准导航错误详情已隐藏");
        }
    }
}
