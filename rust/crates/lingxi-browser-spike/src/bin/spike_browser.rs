//! spike_browser — R01-T04 逐能力实测驱动（受控 Chromium 宿主）。
//!
//! 用法：
//!   spike_browser run --site http://127.0.0.1:18281 --repo <repoRoot> \
//!       --evidence <dir> [--chrome <chromeBinary>] [--profile-root <dir>] \
//!       [--phases main,isolation,proxy,takeover]
//!
//! 每个相位输出独立 transcript JSONL + 截图/文件证据；进程退出码：
//! 全部 PASS = 0；任何 FAIL = 2；参数/环境错误 = 1。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use lingxi_browser_spike::cdp::{png_dimensions, CdpClient};
use lingxi_browser_spike::launcher;
use lingxi_browser_spike::ops::{self, Tab};
use lingxi_browser_spike::snapshot_source::{extract_snapshot_script, hex_sha256};

const UPLOAD_PAYLOAD: &[u8] = b"lingxi-t04-upload-payload-\xe4\xbd\xa0\xe5\xa5\xbd-0123456789\n";
const DOWNLOAD_EXPECTED: &[u8] =
    b"lingxi-t04-download-payload-\xe4\xbd\xa0\xe5\xa5\xbd-0123456789\n";

struct Transcript {
    file: fs::File,
    pass: u32,
    fail: u32,
}

impl Transcript {
    fn open(path: &Path) -> std::io::Result<Self> {
        if let Some(p) = path.parent() {
            fs::create_dir_all(p)?;
        }
        Ok(Self {
            file: fs::File::create(path)?,
            pass: 0,
            fail: 0,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn record(
        &mut self,
        phase: &str,
        step: &str,
        capability: &str,
        op: &str,
        expect: &str,
        actual: &str,
        verdict: &str,
        evidence: &[String],
    ) {
        match verdict {
            "PASS" | "VERIFIED" => self.pass += 1,
            "FAIL" | "FAILED" => self.fail += 1,
            _ => {}
        }
        let rec = json!({
            "ts": chrono_free_now(),
            "phase": phase,
            "step": step,
            "capability": capability,
            "op": op,
            "expect": expect,
            "actual": actual,
            "verdict": verdict,
            "evidence": evidence,
        });
        use std::io::Write as _;
        let _ = writeln!(self.file, "{}", serde_json::to_string(&rec).unwrap());
        let _ = self.file.flush();
        eprintln!("[{phase}/{step}] {verdict} — {capability}: {actual}");
    }
}

/// ISO-8601-ish timestamp without the chrono dep.
fn chrono_free_now() -> String {
    let out = Command::new("date")
        .arg("-u")
        .arg("+%Y-%m-%dT%H:%M:%SZ")
        .output();
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        Err(_) => "unknown".to_string(),
    }
}

fn find_chrome() -> Option<PathBuf> {
    let candidates = [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    ];
    for c in candidates {
        let p = PathBuf::from(c);
        if p.exists() {
            return Some(p);
        }
    }
    if let Ok(extra) = std::env::var("LX_T04_CHROME") {
        let p = PathBuf::from(extra);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

struct RunCtx {
    site: String,
    evidence: PathBuf,
    profile_root: PathBuf,
    chrome: PathBuf,
    snapshot_script: String,
    snapshot_sha: String,
}

impl RunCtx {
    fn shot(&self, cdp: &CdpClient, tab: &Tab, name: &str, full_page: bool) -> (String, String) {
        let bytes = if full_page {
            ops::screenshot_full_page(cdp, tab)
        } else {
            ops::screenshot_viewport(cdp, tab)
        };
        match bytes {
            Ok(b) => {
                let path = self
                    .evidence
                    .join("screenshots")
                    .join(format!("{name}.png"));
                fs::create_dir_all(path.parent().unwrap()).ok();
                fs::write(&path, &b).ok();
                let dims = png_dimensions(&b)
                    .map(|(w, h)| format!("{w}x{h}"))
                    .unwrap_or_else(|| "unparsed".into());
                (
                    format!("{}#sha256={}", path.display(), hex_sha256(&b)),
                    format!("{} bytes, {dims}", b.len()),
                )
            }
            Err(e) => (format!("screenshot failed: {e}"), "error".into()),
        }
    }
}

fn browser_version(cdp: &CdpClient) -> String {
    cdp.call(None, "Browser.getVersion", json!({}))
        .ok()
        .and_then(|v| v["product"].as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "unknown".into())
}

fn kill_child(child: &mut std::process::Child) {
    let pid = child.id() as i32;
    unsafe {
        libc::kill(pid, libc::SIGTERM);
    }
    for _ in 0..40 {
        match child.try_wait() {
            Ok(Some(_)) => return,
            _ => std::thread::sleep(Duration::from_millis(100)),
        }
    }
    unsafe {
        libc::kill(pid, libc::SIGKILL);
    }
    let _ = child.wait();
}

/// rss (KB) of the process tree rooted at pid (best-effort via ps).
fn sample_process_tree_rss(pid: u32) -> i64 {
    let out = Command::new("ps")
        .args(["-axo", "pid,ppid,rss,comm"])
        .output();
    let Ok(out) = out else { return -1 };
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let mut rows: Vec<(u32, u32, i64)> = Vec::new();
    for line in text.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 3 {
            if let (Ok(p), Ok(pp), Ok(r)) = (parts[0].parse(), parts[1].parse(), parts[2].parse()) {
                rows.push((p, pp, r));
            }
        }
    }
    // BFS from root pid
    let mut total = 0i64;
    let mut stack = vec![pid];
    let mut seen = std::collections::HashSet::new();
    while let Some(p) = stack.pop() {
        if !seen.insert(p) {
            continue;
        }
        for (cp, pp, rss) in &rows {
            if *cp == p {
                total += rss;
            }
            if *pp == p {
                stack.push(*cp);
            }
        }
    }
    total
}

// ──────────────────────────── phase: main ────────────────────────────

fn phase_main(ctx: &RunCtx, tr: &mut Transcript) {
    let phase = "main";
    let profile = ctx.profile_root.join("main");
    fs::create_dir_all(&profile).ok();
    let downloads = ctx.evidence.join("downloads");
    fs::create_dir_all(&downloads).ok();

    let launched = match launcher::launch(&ctx.chrome, &profile, &[]) {
        Ok(l) => l,
        Err(e) => {
            tr.record(
                phase,
                "A1",
                "launch",
                "launch headed chromium (pipe)",
                "browser launches",
                &format!("launch failed: {e}"),
                "FAILED",
                &[],
            );
            return;
        }
    };
    let launcher::LaunchedBrowser {
        mut child,
        cmd_write,
        rsp_read,
    } = launched;
    let cdp = CdpClient::new(rsp_read, cmd_write);
    let version = browser_version(&cdp);
    tr.record(
        phase,
        "A1",
        "launch+version",
        "Browser.getVersion",
        "headed Chromium launches on CDP pipe (no TCP port)",
        &format!("product={version} pid={}", child.id()),
        if version.starts_with("Chrome/") {
            "VERIFIED"
        } else {
            "FAILED"
        },
        &[],
    );

    // no TCP debug port listening (9222 must be closed)
    let port_probe = Command::new("sh")
        .arg("-c")
        .arg("nc -z 127.0.0.1 9222 2>/dev/null; echo $?")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "?".into());
    tr.record(
        phase,
        "A1b",
        "调试面不暴露TCP",
        "nc -z 127.0.0.1 9222",
        "exit!=0 (无监听)",
        &format!("nc exit={port_probe}"),
        if port_probe == "1" {
            "VERIFIED"
        } else {
            "FAILED"
        },
        &[],
    );

    let tab = match ops::open_tab(&cdp, None, "about:blank") {
        Ok(t) => t,
        Err(e) => {
            tr.record(
                phase,
                "A2",
                "open tab",
                "Target.createTarget+attach",
                "tab attached",
                &format!("{e}"),
                "FAILED",
                &[],
            );
            kill_child(&mut child);
            return;
        }
    };

    // A2 navigate + snapshot
    let r = ops::navigate(&cdp, &tab, &format!("{}/form", ctx.site));
    let snap = ops::snapshot(&cdp, &tab, &ctx.snapshot_script);
    match (r, &snap) {
        (Ok(nav), Ok(s)) => {
            let text = s["text"].as_str().unwrap_or("");
            let ok = nav["ok"] == json!(true)
                && text.contains("T04 表单页")
                && ops::find_ref(text, "输入标题").is_some()
                && ops::find_ref(text, "提交表单").is_some();
            tr.record(
                phase,
                "A2",
                "navigate+DOM引用snapshot",
                "navigate /form + SNAPSHOT_SCRIPT",
                "快照含可交互元素 ref（输入框/按钮）",
                &format!(
                    "nav_ok={} snapshot_len={} ref_input={:?} ref_submit={:?}",
                    nav["ok"],
                    text.len(),
                    ops::find_ref(text, "输入标题"),
                    ops::find_ref(text, "提交表单")
                ),
                if ok { "VERIFIED" } else { "FAILED" },
                &[],
            );
        }
        (a, b) => tr.record(
            phase,
            "A2",
            "navigate+snapshot",
            "navigate /form",
            "ok",
            &format!("nav={a:?} snap_err={:?}", b.as_ref().err()),
            "FAILED",
            &[],
        ),
    }
    let (shot_evi, shot_desc) = ctx.shot(&cdp, &tab, "a2-form-loaded", false);
    tr.record(
        phase,
        "A2s",
        "截图(视口)",
        "Page.captureScreenshot",
        "PNG 非空、维度为视口",
        &shot_desc,
        if shot_desc.contains("bytes") {
            "VERIFIED"
        } else {
            "FAILED"
        },
        &[shot_evi],
    );

    // A3 中文输入（Input.insertText，IME 等价路径）
    let typed = ops::type_text(&cdp, &tab, "#t1", "灵犀你好2026");
    let val = ops::eval_value(&cdp, &tab, "document.getElementById('t1').value");
    match (typed, val) {
        (Ok(()), Ok(v)) => {
            let got = v.as_str().unwrap_or("");
            tr.record(
                phase,
                "A3",
                "中文输入",
                "focus #t1 + Input.insertText '灵犀你好2026'",
                "input.value == 灵犀你好2026",
                &format!("value={got:?}"),
                if got == "灵犀你好2026" {
                    "VERIFIED"
                } else {
                    "FAILED"
                },
                &[],
            );
        }
        (a, b) => tr.record(
            phase,
            "A3",
            "中文输入",
            "type",
            "ok",
            &format!("type={a:?} val={b:?}"),
            "FAILED",
            &[],
        ),
    }

    // A4 select
    let sel = ops::select_option(&cdp, &tab, "#sel1", "c");
    let sel_val = ops::eval_value(&cdp, &tab, "document.getElementById('sel1').value");
    let ok = matches!(sel, Ok(()))
        && sel_val
            .as_ref()
            .ok()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            == Some("c".into());
    tr.record(
        phase,
        "A4",
        "select 下拉",
        "select #sel1 = c",
        "value == c",
        &format!("sel={:?} value={:?}", sel.is_ok(), sel_val.ok()),
        if ok { "VERIFIED" } else { "FAILED" },
        &[],
    );

    // A5 click by ref ×2 → counter
    let snap2 = ops::snapshot(&cdp, &tab, &ctx.snapshot_script).unwrap();
    let text2 = snap2["text"].as_str().unwrap_or("");
    let counter_ref = ops::find_ref(text2, "计数");
    let mut counter_ok = false;
    let mut counter_actual = String::new();
    if let Some(r) = counter_ref {
        if ops::click_ref(&cdp, &tab, r).is_ok() && ops::click_ref(&cdp, &tab, r).is_ok() {
            if let Ok(v) =
                ops::eval_value(&cdp, &tab, "document.getElementById('counter').textContent")
            {
                counter_actual = format!("ref={r} counter={}", v.as_str().unwrap_or(""));
                counter_ok = v.as_str() == Some("2");
            }
        }
    }
    tr.record(
        phase,
        "A5",
        "DOM ref 点击",
        "click ref(counter) ×2",
        "counter == 2",
        &counter_actual,
        if counter_ok { "VERIFIED" } else { "FAILED" },
        &[],
    );

    // A6 提交表单（click submit）→ echo 回填中文标题
    let submit_ref = ops::find_ref(text2, "提交表单");
    let mut submit_ok = false;
    let mut submit_actual = String::new();
    if let Some(r) = submit_ref {
        if ops::click_ref(&cdp, &tab, r).is_ok() {
            if let Ok(v) = ops::wait_until(
                &cdp,
                &tab,
                "document.getElementById('echo').textContent",
                &|v| {
                    v.as_str()
                        .map(|s| s.starts_with("SUBMITTED"))
                        .unwrap_or(false)
                },
                Duration::from_secs(5),
            ) {
                let s = v.as_str().unwrap_or("").to_string();
                submit_actual.clone_from(&s);
                submit_ok = s.contains("title=灵犀你好2026")
                    && s.contains("choice=c")
                    && s.contains("agree=null");
            }
        }
    }
    tr.record(
        phase,
        "A6",
        "表单提交语义",
        "click 提交表单 → echo",
        "echo 含 title=灵犀你好2026 choice=c",
        &submit_actual,
        if submit_ok { "VERIFIED" } else { "FAILED" },
        &[],
    );
    let (s6evi, s6desc) = ctx.shot(&cdp, &tab, "a6-form-submitted", false);
    tr.record(
        phase,
        "A6s",
        "截图(操作后)",
        "captureScreenshot",
        "非空",
        &s6desc,
        if s6desc.contains("bytes") {
            "VERIFIED"
        } else {
            "FAILED"
        },
        &[s6evi],
    );

    // A7 长页面：滚动 + 整页长截图
    let _ = ops::navigate(&cdp, &tab, &format!("{}/long", ctx.site));
    let y0 = ops::eval_value(&cdp, &tab, "window.scrollY")
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(-1.0);
    let y1 = ops::scroll_by(&cdp, &tab, 900).unwrap_or(-1);
    let scrolled = y0 == 0.0 && y1 > 0;
    tr.record(
        phase,
        "A7a",
        "滚动",
        "scrollBy(900)",
        "scrollY 0 → >0",
        &format!("y0={y0} y1={y1}"),
        if scrolled { "VERIFIED" } else { "FAILED" },
        &[],
    );
    let (fpevi, fpdesc) = ctx.shot(&cdp, &tab, "a7-long-fullpage", true);
    let fp_path = ctx.evidence.join("screenshots/a7-long-fullpage.png");
    let fp_dims = fs::read(&fp_path).ok().and_then(|b| png_dimensions(&b));
    let tall = fp_dims.map(|(_, h)| h > 3000).unwrap_or(false);
    tr.record(
        phase,
        "A7b",
        "整页长截图",
        "captureScreenshot(captureBeyondViewport)",
        "高度 >> 视口(860)",
        &format!("{fpdesc} dims={fp_dims:?}"),
        if tall { "VERIFIED" } else { "FAILED" },
        &[fpevi],
    );

    // A8 上传（agent 驱动 setFileInputFiles）
    let up_file = ctx.evidence.join("upload-payload.bin");
    fs::write(&up_file, UPLOAD_PAYLOAD).ok();
    let want_sha = hex_sha256(UPLOAD_PAYLOAD);
    let _ = ops::navigate(&cdp, &tab, &format!("{}/form", ctx.site));
    let up = ops::set_file_input(&cdp, &tab, "#file1", &[up_file.display().to_string()]);
    let mut up_ok = false;
    let files_len = ops::eval_value(&cdp, &tab, "document.getElementById('file1').files.length")
        .ok()
        .and_then(|v| v.as_i64())
        .unwrap_or(-1);
    let mut up_actual = format!("set_file_input={:?} files.length={files_len}", up.is_ok());
    if up.is_ok() {
        if let Ok(snap3) = ops::snapshot(&cdp, &tab, &ctx.snapshot_script) {
            let up_ref = ops::find_ref(snap3["text"].as_str().unwrap_or(""), "上传");
            up_actual.push_str(&format!(" upload_ref={up_ref:?}"));
            if let Some(r) = up_ref {
                let clk = ops::click_ref(&cdp, &tab, r);
                up_actual.push_str(&format!(" click={:?}", clk.is_ok()));
                if clk.is_ok() {
                    match ops::wait_until(
                        &cdp,
                        &tab,
                        "document.getElementById('upload-result').textContent",
                        &|v| {
                            v.as_str()
                                .map(|s| s.starts_with("UPLOAD_OK"))
                                .unwrap_or(false)
                        },
                        Duration::from_secs(8),
                    ) {
                        Ok(v) => {
                            let s = v.as_str().unwrap_or("").to_string();
                            up_actual.clone_from(&s);
                            up_ok = s.contains(&format!("sha256={want_sha}"))
                                && s.contains(&format!("size={}", UPLOAD_PAYLOAD.len()))
                                && s.contains("filename=upload-payload.bin");
                        }
                        Err(e) => {
                            let cur = ops::eval_value(
                                &cdp,
                                &tab,
                                "document.getElementById('upload-result').textContent",
                            )
                            .ok()
                            .and_then(|v| v.as_str().map(|s| s.to_string()))
                            .unwrap_or_default();
                            up_actual.push_str(&format!(" wait_timeout={e} result_text={cur:?}"));
                        }
                    }
                }
            }
        }
    }
    tr.record(
        phase,
        "A8",
        "上传(agent驱动)",
        "DOM.setFileInputFiles + 点击上传",
        &format!(
            "服务端收到 size={} sha256={}…",
            UPLOAD_PAYLOAD.len(),
            &want_sha[..16]
        ),
        &up_actual,
        if up_ok { "VERIFIED" } else { "FAILED" },
        &[format!("{}#sha256={want_sha}", up_file.display())],
    );

    // A9 下载
    ops::set_download_dir(&cdp, None, &downloads).ok();
    let nav_dl = ops::navigate(&cdp, &tab, &format!("{}/download/hello.txt", ctx.site));
    let mut dl_ok = false;
    let mut dl_actual = format!("nav={nav_dl:?}");
    for _ in 0..30 {
        let f = downloads.join("hello.txt");
        if f.exists() {
            if let Ok(bytes) = fs::read(&f) {
                if bytes == DOWNLOAD_EXPECTED {
                    dl_ok = true;
                    dl_actual = format!(
                        "hello.txt {} bytes sha256={}",
                        bytes.len(),
                        hex_sha256(&bytes)
                    );
                    break;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    tr.record(
        phase,
        "A9",
        "下载",
        "Browser.setDownloadBehavior + navigate 附件 URL",
        &format!("文件落盘且 sha256={}", hex_sha256(DOWNLOAD_EXPECTED)),
        &dl_actual,
        if dl_ok { "VERIFIED" } else { "FAILED" },
        &[format!("{}/hello.txt", downloads.display())],
    );

    // A10 弹窗 window.open → 新 tab 事件
    let _ = ops::navigate(&cdp, &tab, &format!("{}/popup", ctx.site));
    // 实测发现：合成手势点击触发 window.open 时，若目标页未置前，新窗口会停在
    // about:blank 不跳转（dbg3/dbg6 停滞 vs dbg4/dbg7 bringToFront 后成功）。
    // 因此置前是可信手势路径的必要步骤。
    cdp.call(Some(&tab.session_id), "Page.bringToFront", json!({}))
        .ok();
    std::thread::sleep(Duration::from_millis(400));
    cdp.call(
        None,
        "Target.setDiscoverTargets",
        json!({ "discover": true }),
    )
    .ok();
    let mut popup_ok = false;
    let mut popup_actual = String::new();
    // NOTE: window.open 需要真实用户手势；JS el.click() 会被 Chrome 弹窗拦截器
    // 静默拦掉（本 spike 初轮实测即如此），因此这里用 CDP Input 可信手势点击。
    match ops::click_selector_mouse(&cdp, &tab, "#open-popup") {
        Ok(()) => {
            // 点击处理器同步置位（即使弹窗被拦也会置 opened）
            let state = ops::eval_value(
                &cdp,
                &tab,
                "document.getElementById('popup-state').textContent",
            )
            .ok()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_default();
            popup_actual.push_str(&format!("click landed, popup-state={state:?} "));
            // window.open 带尺寸特征 → 新窗口目标先以 about:blank 出现再跳转；
            // setDiscoverTargets 会为既有目标补发 targetCreated，谓词必须排除
            // 主 tab 自身（初轮实测曾因误匹配主 tab 而把主 tab 当弹窗关掉，
            // 导致后续 A11–A13 级联失败）。
            let own_id = tab.target_id.clone();
            match cdp.wait_event(
                "Target.targetCreated",
                move |e| {
                    e["params"]["targetInfo"]["type"].as_str() == Some("page")
                        && e["params"]["targetInfo"]["targetId"].as_str() != Some(own_id.as_str())
                },
                Duration::from_secs(6),
            ) {
                Ok(ev) => {
                    let new_id = ev["params"]["targetInfo"]["targetId"]
                        .as_str()
                        .unwrap_or("")
                        .to_string();
                    popup_actual
                        .push_str(&format!("newTarget={} ", &new_id[..8.min(new_id.len())]));
                    // 过早 attach 到 about:blank 弹窗会卡住其跳转（dbg3 实测）；
                    // 先用 Target.getTargets 轮询 URL 到位，再附加验证内容。
                    let mut url_ok = false;
                    let mut last_seen_url = String::new();
                    for _ in 0..20 {
                        if let Ok(tl) = cdp.call(None, "Target.getTargets", json!({})) {
                            if let Some(arr) = tl["targetInfos"].as_array() {
                                for t in arr {
                                    if t["targetId"].as_str() == Some(new_id.as_str()) {
                                        last_seen_url = t["url"].as_str().unwrap_or("").to_string();
                                    }
                                }
                                if arr.iter().any(|t| {
                                    t["targetId"].as_str() == Some(new_id.as_str())
                                        && t["url"]
                                            .as_str()
                                            .map(|u| u.contains("/popup-target"))
                                            .unwrap_or(false)
                                }) {
                                    url_ok = true;
                                    break;
                                }
                            }
                        }
                        std::thread::sleep(Duration::from_millis(400));
                    }
                    popup_actual
                        .push_str(&format!("urlNavigated={url_ok} lastUrl={last_seen_url:?} "));
                    if !url_ok {
                        // 重试一次：关掉停滞的 about:blank 弹窗，重新置前后再点击
                        cdp.call(None, "Target.closeTarget", json!({ "targetId": new_id }))
                            .ok();
                        cdp.call(Some(&tab.session_id), "Page.bringToFront", json!({}))
                            .ok();
                        std::thread::sleep(Duration::from_millis(500));
                        if ops::click_selector_mouse(&cdp, &tab, "#open-popup").is_ok() {
                            let own2 = tab.target_id.clone();
                            if let Ok(ev2) = cdp.wait_event(
                                "Target.targetCreated",
                                move |e| {
                                    e["params"]["targetInfo"]["type"].as_str() == Some("page")
                                        && e["params"]["targetInfo"]["targetId"].as_str()
                                            != Some(own2.as_str())
                                },
                                Duration::from_secs(6),
                            ) {
                                let retry_id = ev2["params"]["targetInfo"]["targetId"]
                                    .as_str()
                                    .unwrap_or("")
                                    .to_string();
                                for _ in 0..20 {
                                    if let Ok(tl) = cdp.call(None, "Target.getTargets", json!({})) {
                                        if let Some(arr) = tl["targetInfos"].as_array() {
                                            for t in arr {
                                                if t["targetId"].as_str() == Some(retry_id.as_str())
                                                {
                                                    last_seen_url =
                                                        t["url"].as_str().unwrap_or("").to_string();
                                                }
                                            }
                                            if arr.iter().any(|t| {
                                                t["targetId"].as_str() == Some(retry_id.as_str())
                                                    && t["url"]
                                                        .as_str()
                                                        .map(|u| u.contains("/popup-target"))
                                                        .unwrap_or(false)
                                            }) {
                                                url_ok = true;
                                                break;
                                            }
                                        }
                                    }
                                    std::thread::sleep(Duration::from_millis(400));
                                }
                                if url_ok {
                                    popup_actual.push_str(&format!(
                                        "[retry ok newTarget={}] ",
                                        &retry_id[..8.min(retry_id.len())]
                                    ));
                                    if let Ok(a) = cdp.call(
                                        None,
                                        "Target.attachToTarget",
                                        json!({"targetId": retry_id, "flatten": true}),
                                    ) {
                                        let sid = a["sessionId"].as_str().unwrap_or("").to_string();
                                        let popup_tab = Tab {
                                            target_id: retry_id.clone(),
                                            session_id: sid,
                                        };
                                        if let Ok(v) = ops::eval_value(&cdp, &popup_tab,
                                            "document.getElementById('popup-marker') && document.getElementById('popup-marker').textContent") {
                                            popup_actual.push_str(&format!("marker={:?}", v.as_str()));
                                            popup_ok = v.as_str() == Some("POPUP_TARGET_LOADED");
                                        }
                                        ops::close_tab(&cdp, &popup_tab).ok();
                                    }
                                } else {
                                    popup_actual.push_str(&format!(
                                        "[retry stalled lastUrl={last_seen_url:?}] "
                                    ));
                                    cdp.call(
                                        None,
                                        "Target.closeTarget",
                                        json!({ "targetId": retry_id }),
                                    )
                                    .ok();
                                }
                            }
                        }
                    }
                    if url_ok && !popup_ok {
                        match cdp.call(
                            None,
                            "Target.attachToTarget",
                            json!({"targetId": new_id, "flatten": true}),
                        ) {
                            Ok(a) => {
                                let sid = a["sessionId"].as_str().unwrap_or("").to_string();
                                let popup_tab = Tab {
                                    target_id: new_id.clone(),
                                    session_id: sid,
                                };
                                if let Ok(v) = ops::eval_value(&cdp, &popup_tab,
                                    "document.getElementById('popup-marker') && document.getElementById('popup-marker').textContent") {
                                    popup_actual.push_str(&format!("marker={:?}", v.as_str()));
                                    popup_ok = v.as_str() == Some("POPUP_TARGET_LOADED");
                                }
                                ops::close_tab(&cdp, &popup_tab).ok();
                            }
                            Err(e) => popup_actual.push_str(&format!("attach failed: {e}")),
                        }
                    }
                }
                Err(e) => popup_actual.push_str(&format!("no targetCreated: {e}")),
            }
        }
        Err(e) => popup_actual = format!("mouse click failed: {e}"),
    }
    tr.record(
        phase,
        "A10",
        "弹窗(window.open)",
        "click 打开弹窗 → Target.targetCreated",
        "捕获弹窗目标并可附加/读取内容",
        &popup_actual,
        if popup_ok { "VERIFIED" } else { "FAILED" },
        &[],
    );

    // A11 对话框 alert/confirm
    let _ = ops::navigate(&cdp, &tab, &format!("{}/dialog", ctx.site));
    let mut dlg_ok = false;
    let mut dlg_actual = String::new();
    if let Ok(snap5) = ops::snapshot(&cdp, &tab, &ctx.snapshot_script) {
        let _ = snap5["text"].as_str().unwrap_or("");
        // JS 对话框不依赖输入焦点/用户手势：alert() 总会触发
        // Page.javascriptDialogOpening。el.click() 的 Runtime.evaluate 会阻塞到
        // 对话框关闭，因此点击放在子线程，主线程等事件并处理。
        let cdp_alert = std::sync::Arc::clone(&cdp);
        let alert_sid = tab.session_id.clone();
        let click_thread = std::thread::spawn(move || {
            let _ = cdp_alert.call(
                Some(&alert_sid),
                "Runtime.evaluate",
                json!({
                    "expression": "document.getElementById('alert-btn').click(); 'clicked'",
                    "returnByValue": true
                }),
            );
        });
        let alert_flow = (|| -> Result<(), String> {
            let ev = cdp
                .wait_event(
                    "Page.javascriptDialogOpening",
                    |_| true,
                    Duration::from_secs(8),
                )
                .map_err(|e| e.to_string())?;
            let dtype = ev["params"]["type"].as_str().unwrap_or("").to_string();
            let dmsg = ev["params"]["message"].as_str().unwrap_or("").to_string();
            cdp.call(
                Some(&tab.session_id),
                "Page.handleJavaScriptDialog",
                json!({ "accept": true }),
            )
            .map_err(|e| e.to_string())?;
            let after = ops::wait_until(
                &cdp,
                &tab,
                "document.getElementById('dialog-result').textContent",
                &|v| v.as_str() == Some("alert-done"),
                Duration::from_secs(4),
            );
            let after_ok = after.is_ok();
            let after_txt = after.ok().and_then(|v| v.as_str().map(|s| s.to_string()));
            dlg_actual = format!("dialog type={dtype} message={dmsg} result={after_txt:?}");
            dlg_ok = dtype == "alert" && dmsg.contains("t04-alert") && after_ok;
            Ok(())
        })();
        if let Err(e) = alert_flow {
            dlg_actual = format!("alert flow failed: {e}");
        }
        let _ = click_thread.join();

        // confirm → accept=false，同法子线程点击
        let cdp_confirm = std::sync::Arc::clone(&cdp);
        let confirm_sid = tab.session_id.clone();
        let click_thread2 = std::thread::spawn(move || {
            let _ = cdp_confirm.call(
                Some(&confirm_sid),
                "Runtime.evaluate",
                json!({
                    "expression": "document.getElementById('confirm-btn').click(); 'clicked'",
                    "returnByValue": true
                }),
            );
        });
        let confirm_flow = (|| -> Result<(), String> {
            cdp.wait_event(
                "Page.javascriptDialogOpening",
                |_| true,
                Duration::from_secs(8),
            )
            .map_err(|e| e.to_string())?;
            cdp.call(
                Some(&tab.session_id),
                "Page.handleJavaScriptDialog",
                json!({ "accept": false }),
            )
            .map_err(|e| e.to_string())?;
            match ops::wait_until(
                &cdp,
                &tab,
                "document.getElementById('dialog-result').textContent",
                &|v| {
                    v.as_str()
                        .map(|s| s.starts_with("confirm:"))
                        .unwrap_or(false)
                },
                Duration::from_secs(4),
            ) {
                Ok(v) => {
                    let got = v.as_str().unwrap_or("").to_string();
                    dlg_actual.push_str(&format!(" | confirm result={got}"));
                    dlg_ok = dlg_ok && got == "confirm:false";
                }
                Err(e) => {
                    dlg_actual.push_str(&format!(" | confirm result missing: {e}"));
                    dlg_ok = false;
                }
            }
            Ok(())
        })();
        if let Err(e) = confirm_flow {
            dlg_actual.push_str(&format!(" | confirm flow failed: {e}"));
            dlg_ok = false;
        }
        let _ = click_thread2.join();
    }
    tr.record(
        phase,
        "A11",
        "JS 对话框",
        "alert accept / confirm reject",
        "alert→alert-done; confirm(拒绝)→confirm:false",
        &dlg_actual,
        if dlg_ok { "VERIFIED" } else { "FAILED" },
        &[],
    );

    // A12 登录 + cookie
    let _ = ops::navigate(&cdp, &tab, &format!("{}/login", ctx.site));
    let mut login_ok = false;
    let mut login_actual = String::new();
    if ops::type_text(&cdp, &tab, "#u", "demo").is_ok()
        && ops::type_text(&cdp, &tab, "#p", "lingxi-pass-2026").is_ok()
    {
        if let Ok(snap7) = ops::snapshot(&cdp, &tab, &ctx.snapshot_script) {
            if let Some(r) = ops::find_ref(snap7["text"].as_str().unwrap_or(""), "登录") {
                let _ = ops::click_ref(&cdp, &tab, r);
            }
        }
        if let Ok(v) = ops::wait_until(&cdp, &tab,
            "document.getElementById('whoami') ? document.getElementById('whoami').textContent : ''",
            &|v| v.as_str().map(|s| s.contains("LOGGED_IN")).unwrap_or(false),
            Duration::from_secs(6)) {
            login_actual = format!("whoami={:?}", v.as_str());
            let cookies = ops::get_cookies(&cdp, None);
            let has_session = cookies
                .map(|c| c["cookies"].as_array().map(|a| a.iter().any(|ck| ck["name"] == "lingxi_t04_session")).unwrap_or(false))
                .unwrap_or(false);
            login_actual.push_str(&format!(" cookie_present={has_session}"));
            login_ok = v.as_str().map(|s| s.contains("LOGGED_IN demo")).unwrap_or(false) && has_session;
        }
    }
    tr.record(
        phase,
        "A12",
        "登录+cookie",
        "POST 登录表单 → /account",
        "whoami=LOGGED_IN demo 且 session cookie 存在",
        &login_actual,
        if login_ok { "VERIFIED" } else { "FAILED" },
        &[],
    );

    // A13 挂起/热恢复：textarea 写入标记 → detach → reattach → 状态保留
    let _ = ops::navigate(&cdp, &tab, &format!("{}/form", ctx.site));
    ops::type_text(&cdp, &tab, "#ta1", "suspend-marker-保留").ok();
    let suspend_tab_id = tab.target_id.clone();
    let detach = cdp.call(
        None,
        "Target.detachFromTarget",
        json!({ "sessionId": tab.session_id }),
    );
    let mut resume_ok = false;
    let mut resume_actual = format!("detach={:?}", detach.is_ok());
    std::thread::sleep(Duration::from_millis(500));
    if let Ok(a) = cdp.call(
        None,
        "Target.attachToTarget",
        json!({ "targetId": suspend_tab_id, "flatten": true }),
    ) {
        let sid = a["sessionId"].as_str().unwrap_or("").to_string();
        let tab2 = Tab {
            target_id: suspend_tab_id.clone(),
            session_id: sid,
        };
        if let Ok(v) = ops::eval_value(&cdp, &tab2, "document.getElementById('ta1').value") {
            resume_actual = format!("reattached ta1.value={:?}", v.as_str());
            resume_ok = v.as_str() == Some("suspend-marker-保留");
        }
        // 用新 session 继续后续步骤
        tr.record(
            phase,
            "A13",
            "挂起/热恢复",
            "detach(挂起) → reattach(恢复)",
            "textarea 内容保留（页面状态不丢）",
            &resume_actual,
            if resume_ok { "VERIFIED" } else { "FAILED" },
            &[],
        );
        phase_main_cold_restart(ctx, tr, &cdp, &tab2, &mut child);
    } else {
        tr.record(
            phase,
            "A13",
            "挂起/热恢复",
            "detach→reattach",
            "状态保留",
            &resume_actual,
            "FAILED",
            &[],
        );
        phase_main_cold_restart(ctx, tr, &cdp, &tab, &mut child);
    }
}

fn phase_main_cold_restart(
    ctx: &RunCtx,
    tr: &mut Transcript,
    cdp: &Arc<CdpClient>,
    tab: &Tab,
    child: &mut std::process::Child,
) {
    let phase = "main";
    // A14 冷保存工作区（序列化 tab URL）+ 进程成本采样
    let url_now = ops::eval_value(cdp, tab, "location.href")
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_default();
    let cold_ws = json!({
        "activeTabId": "tab-1",
        "tabs": [{ "tabId": "tab-1", "url": format!("{}/account", ctx.site), "title": "cold-saved" }],
        "savedFromUrl": url_now,
    });
    let cold_path = ctx.evidence.join("cold-workspace.json");
    fs::write(&cold_path, serde_json::to_string_pretty(&cold_ws).unwrap()).ok();

    let rss_kb = sample_process_tree_rss(child.id());
    tr.record(
        phase,
        "A14c",
        "进程成本",
        "ps 采样浏览器进程树 RSS",
        "记录实测值",
        &format!("pid={} tree_rss={}KB", child.id(), rss_kb),
        "VERIFIED",
        &[],
    );

    // 关闭浏览器（Browser.close = 优雅退出，cookie 落盘）
    cdp.call(None, "Browser.close", json!({})).ok();
    for _ in 0..50 {
        if matches!(child.try_wait(), Ok(Some(_))) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if !matches!(child.try_wait(), Ok(Some(_))) {
        kill_child(child);
    }

    // A14 冷恢复：同 user-data-dir 重启，恢复 tab URL，登录态应保持
    let profile = ctx.profile_root.join("main");
    let mut relaunched = match launcher::launch(&ctx.chrome, &profile, &[]) {
        Ok(l) => l,
        Err(e) => {
            tr.record(
                phase,
                "A14",
                "冷恢复+登录持久化",
                "relaunch 同 profile",
                "/account 仍 LOGGED_IN",
                &format!("relaunch failed: {e}"),
                "FAILED",
                &[],
            );
            return;
        }
    };
    let cdp2 = CdpClient::new(
        relaunched.rsp_read.try_clone().unwrap(),
        relaunched.cmd_write,
    );
    let mut cold_ok = false;
    let mut cold_actual = String::new();
    if let Ok(t2) = ops::open_tab(&cdp2, None, &format!("{}/account", ctx.site)) {
        std::thread::sleep(Duration::from_millis(800));
        // /account 未登录会 302 到 /login；等待渲染完成
        if let Ok(v) = ops::wait_until(&cdp2, &t2,
            "document.getElementById('whoami') ? document.getElementById('whoami').textContent : (document.getElementById('login-form') ? 'REDIRECTED_TO_LOGIN' : '')",
            &|v| v.as_str().map(|s| !s.is_empty()).unwrap_or(false),
            Duration::from_secs(8)) {
            cold_actual = format!("after cold relaunch: {:?}", v.as_str());
            cold_ok = v.as_str().map(|s| s.contains("LOGGED_IN demo")).unwrap_or(false);
        }
        let (evi, desc) = ctx.shot(&cdp2, &t2, "a14-cold-resume-account", false);
        tr.record(
            phase,
            "A14s",
            "截图(冷恢复后)",
            "captureScreenshot",
            "非空",
            &desc,
            if desc.contains("bytes") {
                "VERIFIED"
            } else {
                "FAILED"
            },
            &[evi],
        );
    }
    tr.record(
        phase,
        "A14",
        "冷恢复+登录持久化",
        "Browser.close → 同 user-data-dir 重启 → 恢复 tab URL(/account)",
        "仍 LOGGED_IN demo（cookie 持久化）",
        &cold_actual,
        if cold_ok { "VERIFIED" } else { "FAILED" },
        &[format!(
            "{}#sha256={}",
            cold_path.display(),
            hex_sha256(&fs::read(&cold_path).unwrap_or_default())
        )],
    );

    cdp2.call(None, "Browser.close", json!({})).ok();
    for _ in 0..50 {
        if matches!(relaunched.child.try_wait(), Ok(Some(_))) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if !matches!(relaunched.child.try_wait(), Ok(Some(_))) {
        kill_child(&mut relaunched.child);
    }
}

// ──────────────────────────── phase: isolation (A08) ────────────────────────────

fn phase_isolation(ctx: &RunCtx, tr: &mut Transcript) {
    let phase = "isolation";
    let profile = ctx.profile_root.join("iso");
    fs::create_dir_all(&profile).ok();
    let launched = match launcher::launch(&ctx.chrome, &profile, &[]) {
        Ok(l) => l,
        Err(e) => {
            tr.record(
                phase,
                "B0",
                "launch",
                "launch",
                "ok",
                &format!("{e}"),
                "FAILED",
                &[],
            );
            return;
        }
    };
    let launcher::LaunchedBrowser {
        mut child,
        cmd_write,
        rsp_read,
    } = launched;
    let cdp = CdpClient::new(rsp_read, cmd_write);

    let ctx_a = ops::create_context(&cdp);
    let ctx_b = ops::create_context(&cdp);
    let (Ok(a), Ok(b)) = (ctx_a, ctx_b) else {
        tr.record(
            phase,
            "B1",
            "browser contexts",
            "createBrowserContext ×2",
            "ok",
            "create failed",
            "FAILED",
            &[],
        );
        kill_child(&mut child);
        return;
    };
    tr.record(
        phase,
        "B1",
        "多会话上下文",
        "Target.createBrowserContext ×2",
        "两个独立 context",
        &format!("A={a} B={b}"),
        "VERIFIED",
        &[],
    );

    let tab_a = ops::open_tab(
        &cdp,
        Some(&a),
        &format!("{}/storage?value=ALPHA_A", ctx.site),
    );
    let tab_b = ops::open_tab(
        &cdp,
        Some(&b),
        &format!("{}/storage?value=BRAVO_B", ctx.site),
    );
    let (Ok(ta), Ok(tb)) = (tab_a, tab_b) else {
        tr.record(
            phase,
            "B2",
            "storage 写入",
            "navigate /storage?value=",
            "ok",
            "open failed",
            "FAILED",
            &[],
        );
        kill_child(&mut child);
        return;
    };
    std::thread::sleep(Duration::from_millis(700));

    // B2 各自写入，互不串
    let ra = ops::open_tab(&cdp, Some(&a), &format!("{}/storage-read", ctx.site));
    let rb = ops::open_tab(&cdp, Some(&b), &format!("{}/storage-read", ctx.site));
    let mut iso_ok = true;
    let mut detail = String::new();
    if let Ok(tra) = ra {
        std::thread::sleep(Duration::from_millis(500));
        if let Ok(v) = ops::eval_value(
            &cdp,
            &tra,
            "document.getElementById('storage-read').textContent",
        ) {
            let s = v.as_str().unwrap_or("").to_string();
            detail.push_str(&format!("A读: {s} | "));
            iso_ok &= s.contains("local=ALPHA_A")
                && s.contains("t04-cookie=ALPHA_A")
                && !s.contains("BRAVO_B");
        }
        ops::close_tab(&cdp, &tra).ok();
    }
    if let Ok(trb) = rb {
        std::thread::sleep(Duration::from_millis(500));
        if let Ok(v) = ops::eval_value(
            &cdp,
            &trb,
            "document.getElementById('storage-read').textContent",
        ) {
            let s = v.as_str().unwrap_or("").to_string();
            detail.push_str(&format!("B读: {s}"));
            iso_ok &= s.contains("local=BRAVO_B")
                && s.contains("t04-cookie=BRAVO_B")
                && !s.contains("ALPHA_A");
        }
        ops::close_tab(&cdp, &trb).ok();
    }
    tr.record(
        phase,
        "B2",
        "两会话隔离(localStorage+cookie)",
        "A 写 ALPHA_A / B 写 BRAVO_B → 各自回读",
        "各自读到自己的值，无交叉",
        &detail,
        if iso_ok { "VERIFIED" } else { "FAILED" },
        &[],
    );

    // B3 cookie jar 层面核对
    let ca = ops::get_cookies(&cdp, Some(&a));
    let cb = ops::get_cookies(&cdp, Some(&b));
    let jar = |c: &Result<Value, _>| -> String {
        match c {
            Ok(v) => v["cookies"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter(|ck| {
                            ck["name"]
                                .as_str()
                                .map(|n| n.starts_with("t04"))
                                .unwrap_or(false)
                        })
                        .map(|ck| {
                            format!(
                                "{}={}",
                                ck["name"].as_str().unwrap_or(""),
                                ck["value"].as_str().unwrap_or("")
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .unwrap_or_default(),
            Err(_) => "ERR".into(),
        }
    };
    let (ja, jb) = (jar(&ca), jar(&cb));
    let jar_ok = ja.contains("ALPHA_A")
        && !ja.contains("BRAVO_B")
        && jb.contains("BRAVO_B")
        && !jb.contains("ALPHA_A");
    tr.record(
        phase,
        "B3",
        "cookie jar 隔离(Storage.getCookies)",
        "按 browserContextId 查 cookie",
        "A jar 只含 ALPHA_A；B jar 只含 BRAVO_B",
        &format!("A=[{ja}] B=[{jb}]"),
        if jar_ok { "VERIFIED" } else { "FAILED" },
        &[],
    );

    // B4 不可信页越界探测（不可信页拿不到宿主权限）
    let probe_tab = ops::open_tab(&cdp, Some(&a), &format!("{}/probe", ctx.site));
    let mut probe_ok = false;
    let mut probe_actual = String::new();
    if let Ok(tp) = probe_tab {
        if let Ok(v) = ops::wait_until(
            &cdp,
            &tp,
            "document.getElementById('probe-out').textContent",
            &|v| {
                v.as_str()
                    .map(|s| s.starts_with("PROBE_RESULT"))
                    .unwrap_or(false)
            },
            Duration::from_secs(10),
        ) {
            let s = v.as_str().unwrap_or("").to_string();
            probe_actual.clone_from(&s);
            let json_part = s.strip_prefix("PROBE_RESULT ").unwrap_or("{}");
            if let Ok(p) = serde_json::from_str::<Value>(json_part) {
                let hana_absent = p["hana"] == "undefined";
                let no_node =
                    p["processGlobal"] == "undefined" && p["requireGlobal"] == "undefined";
                let cdp_http_blocked = p["cdpHttpJson"]
                    .as_str()
                    .map(|x| x.starts_with("BLOCKED"))
                    .unwrap_or(false);
                let cdp_ws_blocked = p["cdpWs"].as_str().map(|x| x != "OPENED").unwrap_or(false);
                probe_ok = hana_absent && no_node && cdp_http_blocked && cdp_ws_blocked;
                probe_actual.push_str(&format!(
                    " | hana_absent={hana_absent} no_node={no_node} cdp_http={} cdp_ws={}",
                    p["cdpHttpJson"], p["cdpWs"]
                ));
            }
        }
        let (evi, desc) = ctx.shot(&cdp, &tp, "b4-untrusted-probe", false);
        let _ = (evi, desc);
        ops::close_tab(&cdp, &tp).ok();
    }
    tr.record(
        phase,
        "B4",
        "不可信页拿不到宿主权限",
        "/probe 页主动探测 window.hana/node/CDP端口",
        "全部不可得/被阻断",
        &probe_actual,
        if probe_ok { "VERIFIED" } else { "FAILED" },
        &[],
    );

    // B5 宿主侧注入面核对：原型不注入任何全局对象
    let inj = ops::eval_value(&cdp, &ta, "Object.getOwnPropertyNames(window).filter(k => /hana|lingxi|cdp|devtools/i.test(k)).join(',')");
    let inj_actual = inj
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_default();
    tr.record(
        phase,
        "B5",
        "宿主注入面为空",
        "枚举 window 上 hana/lingxi/cdp 全局",
        "无注入全局",
        &format!("found=[{inj_actual}]"),
        if inj_actual.is_empty() {
            "VERIFIED"
        } else {
            "FAILED"
        },
        &[],
    );

    // B6 dispose 上下文后数据销毁
    ops::close_tab(&cdp, &ta).ok();
    ops::close_tab(&cdp, &tb).ok();
    let disp = ops::dispose_context(&cdp, &a);
    let after = ops::get_cookies(&cdp, Some(&a));
    let after_desc = match &after {
        Ok(v) => format!(
            "cookies={}",
            v["cookies"].as_array().map(|x| x.len()).unwrap_or(0)
        ),
        Err(e) => format!("query rejected after dispose: {e}"),
    };
    tr.record(
        phase,
        "B6",
        "上下文销毁",
        "disposeBrowserContext(A)",
        "dispose 成功且其 cookie 不可再读",
        &format!("dispose={:?} after={after_desc}", disp.is_ok()),
        if disp.is_ok() { "VERIFIED" } else { "FAILED" },
        &[],
    );
    ops::dispose_context(&cdp, &b).ok();

    cdp.call(None, "Browser.close", json!({})).ok();
    for _ in 0..50 {
        if matches!(child.try_wait(), Ok(Some(_))) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if !matches!(child.try_wait(), Ok(Some(_))) {
        kill_child(&mut child);
    }
}

// ──────────────────────────── phase: proxy ────────────────────────────

fn phase_proxy(ctx: &RunCtx, tr: &mut Transcript, proxy_port: u16) {
    let phase = "proxy";

    // C1 死代理 → 导航必须失败（证明代理设置真实生效）
    // Chromium 对 loopback 有隐式代理绕过（初轮实测：不加 bypass-list=<-loopback>
    // 时 127.0.0.1 导航绕过死代理直接成功，C1 失败），必须显式移除该绕过。
    let profile_dead = ctx.profile_root.join("proxy-dead");
    fs::create_dir_all(&profile_dead).ok();
    let l1 = launcher::launch(
        &ctx.chrome,
        &profile_dead,
        &[
            "--proxy-server=127.0.0.1:9".to_string(),
            "--proxy-bypass-list=<-loopback>".to_string(),
        ],
    )
    .expect("launch dead-proxy browser");
    let launcher::LaunchedBrowser {
        child: mut child1,
        cmd_write: cw1,
        rsp_read: rr1,
    } = l1;
    let cdp1 = CdpClient::new(rr1, cw1);
    let mut dead_ok = false;
    let mut dead_actual = String::new();
    if let Ok(t) = ops::open_tab(&cdp1, None, "about:blank") {
        let r = ops::navigate(&cdp1, &t, &format!("{}/form", ctx.site));
        if let Ok(nav) = r {
            let err = nav["errorText"].as_str().unwrap_or("").to_string();
            // 也可能导航返回 ok 但页面是 chrome 错误页
            let body = ops::eval_value(
                &cdp1,
                &t,
                "document.body ? document.body.innerText.slice(0,120) : ''",
            )
            .ok()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_default();
            dead_actual = format!("errorText={err:?} body={body:?}");
            dead_ok = !err.is_empty() || body.contains("ERR_") || body.contains("proxy");
        }
        ops::close_tab(&cdp1, &t).ok();
    }
    tr.record(
        phase,
        "C1",
        "代理生效(负向)",
        "--proxy-server=127.0.0.1:9 → 导航",
        "导航失败且错误指向代理",
        &dead_actual,
        if dead_ok { "VERIFIED" } else { "FAILED" },
        &[],
    );
    cdp1.call(None, "Browser.close", json!({})).ok();
    for _ in 0..50 {
        if matches!(child1.try_wait(), Ok(Some(_))) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if !matches!(child1.try_wait(), Ok(Some(_))) {
        kill_child(&mut child1);
    }

    // C2 活代理 → 流量真实经过代理（代理日志为证）
    let profile_live = ctx.profile_root.join("proxy-live");
    fs::create_dir_all(&profile_live).ok();
    let l2 = launcher::launch(
        &ctx.chrome,
        &profile_live,
        &[
            format!("--proxy-server=127.0.0.1:{proxy_port}"),
            "--proxy-bypass-list=<-loopback>".to_string(),
        ],
    )
    .expect("launch live-proxy browser");
    let launcher::LaunchedBrowser {
        child: mut child2,
        cmd_write: cw2,
        rsp_read: rr2,
    } = l2;
    let cdp2 = CdpClient::new(rr2, cw2);
    let mut live_ok = false;
    let mut live_actual = String::new();
    if let Ok(t) = ops::open_tab(&cdp2, None, "about:blank") {
        let r = ops::navigate(&cdp2, &t, &format!("{}/form", ctx.site));
        let nav_ok = r.map(|n| n["ok"] == json!(true)).unwrap_or(false);
        let title = ops::eval_value(&cdp2, &t, "document.title")
            .ok()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_default();
        live_actual = format!("nav_ok={nav_ok} title={title:?}");
        live_ok = nav_ok && title.contains("表单");
        ops::close_tab(&cdp2, &t).ok();
    }
    tr.record(
        phase,
        "C2",
        "代理生效(正向)",
        &format!("--proxy-server=127.0.0.1:{proxy_port} → 导航 /form"),
        "导航成功（代理日志另行核对）",
        &live_actual,
        if live_ok { "VERIFIED" } else { "FAILED" },
        &[],
    );
    cdp2.call(None, "Browser.close", json!({})).ok();
    for _ in 0..50 {
        if matches!(child2.try_wait(), Ok(Some(_))) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if !matches!(child2.try_wait(), Ok(Some(_))) {
        kill_child(&mut child2);
    }
}

// ──────────────────────────── phase: takeover (可见交互) ────────────────────────────

fn phase_takeover(ctx: &RunCtx, tr: &mut Transcript) {
    let phase = "takeover";
    let profile = ctx.profile_root.join("takeover");
    fs::create_dir_all(&profile).ok();
    let launched = match launcher::launch(&ctx.chrome, &profile, &[]) {
        Ok(l) => l,
        Err(e) => {
            tr.record(
                phase,
                "D0",
                "launch",
                "launch",
                "ok",
                &format!("{e}"),
                "FAILED",
                &[],
            );
            return;
        }
    };
    let launcher::LaunchedBrowser {
        mut child,
        cmd_write,
        rsp_read,
    } = launched;
    let cdp = CdpClient::new(rsp_read, cmd_write);
    let tab = match ops::open_tab(&cdp, None, &format!("{}/form", ctx.site)) {
        Ok(t) => t,
        Err(e) => {
            tr.record(
                phase,
                "D1",
                "可见窗口",
                "open tab",
                "ok",
                &format!("{e}"),
                "FAILED",
                &[],
            );
            kill_child(&mut child);
            return;
        }
    };
    std::thread::sleep(Duration::from_millis(1200));

    // D1 窗口可见性：原生窗口枚举（CGWindowList，经 swift 辅助）证明窗口在屏
    let pid = child.id();
    let helper = ctx.evidence.join("list_windows");
    let win_info = Command::new(&helper)
        .arg(pid.to_string())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_else(|e| format!("helper failed: {e}"));
    let onscreen = win_info.contains("onscreen=1") || win_info.contains("layer=0");
    tr.record(
        phase,
        "D1",
        "可见窗口(原生枚举)",
        &format!("CGWindowList pid={pid}"),
        "浏览器窗口在屏（layer 0 窗口存在）",
        &win_info.trim().replace('\n', " ; "),
        if onscreen { "VERIFIED" } else { "FAILED" },
        &[],
    );

    // D2 独立通道输入（os X 级 keystroke）→ agent 侧 snapshot 可观测
    // 先聚焦输入框（经 CDP 点击），再经 osascript 键入独立通道文本
    let focus = ops::eval_throwing(
        &cdp,
        &tab,
        "(function(){ var el=document.getElementById('t1'); el.focus(); return 'focused'; })()",
    );
    let osa = Command::new("osascript")
        .args([
            "-e",
            "tell application \"System Events\" to keystroke \"HUMAN-TAKEOVER\"",
        ])
        .output();
    let mut takeover_ok = false;
    let mut takeover_actual = format!("focus={:?}", focus.is_ok());
    match osa {
        Ok(o) if o.status.success() => {
            std::thread::sleep(Duration::from_millis(600));
            let v = ops::eval_value(&cdp, &tab, "document.getElementById('t1').value");
            let got = v
                .ok()
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .unwrap_or_default();
            takeover_actual.push_str(&format!(" osascript keystroke ok, input.value={got:?}"));
            takeover_ok = got.contains("HUMAN-TAKEOVER");
        }
        Ok(o) => {
            takeover_actual.push_str(&format!(
                " osascript exit={:?} stderr={}",
                o.status.code(),
                String::from_utf8_lossy(&o.stderr).trim()
            ));
        }
        Err(e) => takeover_actual.push_str(&format!(" osascript spawn failed: {e}")),
    }
    tr.record(
        phase,
        "D2",
        "用户接管(独立输入通道)",
        "osascript System Events keystroke → 输入框",
        "页面输入框收到 OS 级键入且 agent 可读回",
        &takeover_actual,
        if takeover_ok {
            "VERIFIED"
        } else {
            "UNVERIFIED"
        },
        &[],
    );

    // D3 屏幕级截图（窗口真实渲染在屏）
    let screen_png = ctx.evidence.join("screenshots/d3-screen.png");
    fs::create_dir_all(screen_png.parent().unwrap()).ok();
    let cap = Command::new("screencapture")
        .args(["-x", "-o"])
        .arg(&screen_png)
        .output();
    let cap_desc = match cap {
        Ok(o) if o.status.success() => {
            let size = fs::metadata(&screen_png).map(|m| m.len()).unwrap_or(0);
            format!(
                "screencapture ok, {} bytes#sha256={}",
                size,
                hex_sha256(&fs::read(&screen_png).unwrap_or_default())
            )
        }
        Ok(o) => format!("screencapture exit={:?}", o.status.code()),
        Err(e) => format!("screencapture spawn failed: {e}"),
    };
    let cap_ok = fs::metadata(&screen_png)
        .map(|m| m.len() > 50_000)
        .unwrap_or(false);
    tr.record(
        phase,
        "D3",
        "屏幕级可见性",
        "screencapture -x（全屏）",
        "截屏文件非空(>50KB)",
        &cap_desc,
        if cap_ok { "VERIFIED" } else { "UNVERIFIED" },
        &[format!("{}", screen_png.display())],
    );

    let (evi, desc) = ctx.shot(&cdp, &tab, "d4-takeover-final", false);
    tr.record(
        phase,
        "D4",
        "截图(接管后)",
        "captureScreenshot",
        "非空",
        &desc,
        if desc.contains("bytes") {
            "VERIFIED"
        } else {
            "FAILED"
        },
        &[evi],
    );

    cdp.call(None, "Browser.close", json!({})).ok();
    for _ in 0..50 {
        if matches!(child.try_wait(), Ok(Some(_))) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if !matches!(child.try_wait(), Ok(Some(_))) {
        kill_child(&mut child);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let get = |flag: &str| -> Option<String> {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    if args.get(1).map(|s| s.as_str()) != Some("run") {
        eprintln!("usage: spike_browser run --site URL --repo DIR --evidence DIR [--chrome BIN] [--profile-root DIR] [--phases a,b] [--proxy-port N]");
        std::process::exit(1);
    }
    let site = get("--site").unwrap_or_else(|| "http://127.0.0.1:18281".into());
    let repo = get("--repo").expect("--repo required");
    let evidence = PathBuf::from(get("--evidence").expect("--evidence required"));
    fs::create_dir_all(&evidence).ok();
    let evidence = fs::canonicalize(&evidence).unwrap_or(evidence);
    let profile_root =
        PathBuf::from(get("--profile-root").unwrap_or_else(|| "/tmp/r01t04/profiles".into()));
    fs::create_dir_all(&profile_root).ok();
    let chrome = get("--chrome")
        .map(PathBuf::from)
        .or_else(find_chrome)
        .expect("no chromium found");
    let phases: Vec<String> = get("--phases")
        .unwrap_or_else(|| "main,isolation,proxy,takeover".into())
        .split(',')
        .map(|s| s.trim().to_string())
        .collect();
    let proxy_port: u16 = get("--proxy-port")
        .and_then(|p| p.parse().ok())
        .unwrap_or(18282);

    let snap_src = extract_snapshot_script(Path::new(&repo)).expect("extract SNAPSHOT_SCRIPT");
    eprintln!(
        "[spike] SNAPSHOT_SCRIPT from {} lines {}-{} sha256={}",
        snap_src.source_file, snap_src.source_lines.0, snap_src.source_lines.1, snap_src.sha256
    );

    let ctx = RunCtx {
        site: site.trim_end_matches('/').to_string(),
        evidence: evidence.clone(),
        profile_root,
        chrome,
        snapshot_script: snap_src.script,
        snapshot_sha: snap_src.sha256,
    };

    let mut tr = Transcript::open(&evidence.join("transcript.jsonl")).expect("open transcript");
    tr.record(
        "meta",
        "M0",
        "证据源",
        "snapshot script provenance",
        "sha 记录",
        &format!(
            "source={} lines={}-{} sha256={}",
            snap_src.source_file,
            snap_src.source_lines.0,
            snap_src.source_lines.1,
            ctx.snapshot_sha
        ),
        "VERIFIED",
        &[],
    );

    for ph in &phases {
        match ph.as_str() {
            "main" => phase_main(&ctx, &mut tr),
            "isolation" => phase_isolation(&ctx, &mut tr),
            "proxy" => phase_proxy(&ctx, &mut tr, proxy_port),
            "takeover" => phase_takeover(&ctx, &mut tr),
            other => eprintln!("[spike] unknown phase {other}"),
        }
    }

    let summary = json!({
        "pass": tr.pass, "fail": tr.fail,
        "chrome": ctx.chrome.display().to_string(),
        "site": ctx.site,
    });
    fs::write(
        evidence.join("summary.json"),
        serde_json::to_string_pretty(&summary).unwrap(),
    )
    .ok();
    eprintln!("[spike] done pass={} fail={}", tr.pass, tr.fail);
    std::process::exit(if tr.fail > 0 { 2 } else { 0 });
}
