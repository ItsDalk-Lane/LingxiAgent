//! spike_pdf — R01-T05 候选链原型：受控 Chromium（CDP over pipe）执行
//! HTML→PDF，复刻生产 office-pdf-helper 的 job 契约并加上资源拦截边界。
//!
//! 用法：
//!   spike_pdf run --job <job.json> --repo <repoRoot> --evidence <dir>
//!                 --profile-root <dir> [--chrome <binary>] [--proxy 127.0.0.1:port]
//!                 [--themes-dir <dir>] [--allow-file-root <dir>]... [--headful]
//!
//! 行为契约：
//!  - 成功：写出 PDF（先 tmp 再原子 rename），stdout 一行结果 JSON，exit 0；
//!  - 失败：不产生输出文件（伪成功产物禁止），stderr 记错误，exit 1；超时 exit 2；
//!  - 无论成败：关闭/杀死浏览器、删除 profile 目录、ps 扫描残留并入证据。

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use lingxi_browser_spike::cdp::CdpClient;
use lingxi_browser_spike::launcher;
use lingxi_browser_spike::pdf::{self, Job, RawJob};

fn get(flag: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1).cloned())
}

fn get_all(flag: &str) -> Vec<String> {
    let args: Vec<String> = std::env::args().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == flag {
            if let Some(v) = args.get(i + 1) {
                out.push(v.clone());
            }
        }
        i += 1;
    }
    out
}

fn find_chrome() -> Option<PathBuf> {
    for c in [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
    ] {
        let p = PathBuf::from(c);
        if p.exists() {
            return Some(p);
        }
    }
    std::env::var("LX_T05_CHROME")
        .ok()
        .map(PathBuf::from)
        .filter(|p| p.exists())
}

fn kill_child(child: &mut std::process::Child) {
    let pid = child.id() as i32;
    unsafe {
        libc::kill(pid, libc::SIGTERM);
    }
    for _ in 0..30 {
        if let Ok(Some(_)) = child.try_wait() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    unsafe {
        libc::kill(pid, libc::SIGKILL);
    }
    let _ = child.wait();
}

/// ps 扫描命令行里含 `needle` 的残留进程（浏览器树核实）。
fn ps_scan(needle: &str) -> Vec<String> {
    let out = Command::new("ps").args(["-axo", "pid,command"]).output();
    let Ok(out) = out else {
        return vec!["ps failed".into()];
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| l.contains(needle) && !l.contains("ps -axo"))
        .map(|l| l.trim().to_string())
        .collect()
}

/// 超时判定（exit 2 契约）：渲染管线各阶段的超时错误有两类文案——
/// 导航/load 阶段为 "loadURL timed out after Nms"（render_job 自造），
/// CDP 调用超时为 "…: timeout: <method> id=N"（CdpError::Timeout 的 Display，
/// 如 printToPDF 阶段 "printToPDF: timeout: Page.printToPDF id=15"）。
/// R1 验收 F2：原实现只匹配 "timed out"，printToPDF 阶段超时实际 exit=1，
/// 与文档承诺的 exit=2 不一致；此处统一按两类文案判定。
fn error_is_timeout(e: &str) -> bool {
    e.contains("timed out") || e.contains("timeout:")
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 || args[1] != "run" {
        eprintln!("usage: spike_pdf run --job J --repo R --evidence E --profile-root P [--chrome C] [--proxy H:P] [--themes-dir D] [--allow-file-root D]... [--headful]");
        std::process::exit(64);
    }
    let job_path = get("--job").expect("--job required");
    let repo = PathBuf::from(get("--repo").expect("--repo required"));
    let evidence = PathBuf::from(get("--evidence").expect("--evidence required"));
    let profile_root = PathBuf::from(get("--profile-root").expect("--profile-root required"));
    let headful = args.iter().any(|a| a == "--headful");
    let chrome = get("--chrome").map(PathBuf::from).or_else(find_chrome);
    let Some(chrome) = chrome else {
        eprintln!("no chrome binary found");
        std::process::exit(65);
    };
    let themes_dir = get("--themes-dir")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo.join("desktop/src/themes"));
    let proxy = get("--proxy");

    fs::create_dir_all(&evidence).ok();
    fs::create_dir_all(&profile_root).ok();

    // --- job 解析（契约同 office-pdf-helper normalizeJob） ---
    let raw_text = fs::read_to_string(&job_path).expect("read job json");
    let raw: RawJob = serde_json::from_str(&raw_text).expect("parse job json");
    let job = match Job::normalize(raw) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("job invalid: {e}");
            std::process::exit(66);
        }
    };

    // --- 资源 allowlist：html 所在目录 + 字体目录 + 显式追加根 ---
    let mut allowed_roots: Vec<PathBuf> = Vec::new();
    if let Some(parent) = job.html_path.parent() {
        allowed_roots.push(fs::canonicalize(parent).unwrap_or(parent.to_path_buf()));
    }
    let fonts_dir = themes_dir.join("fonts");
    allowed_roots.push(fs::canonicalize(&fonts_dir).unwrap_or(fonts_dir));
    for extra in get_all("--allow-file-root") {
        let p = PathBuf::from(extra);
        allowed_roots.push(fs::canonicalize(&p).unwrap_or(p));
    }

    let profile = profile_root.join(format!("job-{}", std::process::id()));
    if profile.exists() {
        fs::remove_dir_all(&profile).ok();
    }
    fs::create_dir_all(&profile).ok();

    // --- 启动受控 Chromium（headless 默认；pipe 无 TCP 调试口） ---
    let mut extra_args: Vec<String> = Vec::new();
    if !headful {
        extra_args.push("--headless=new".into());
    }
    if let Some(p) = &proxy {
        extra_args.push(format!("--proxy-server=http://{p}"));
        // T04 教训：必须关闭 Chromium 对 loopback 的隐式代理绕过
        extra_args.push("--proxy-bypass-list=<-loopback>".into());
    }
    let t_launch = Instant::now();
    let launched = match launcher::launch(&chrome, &profile, &extra_args) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("launch failed: {e}");
            std::process::exit(70);
        }
    };
    let launch_ms = t_launch.elapsed().as_millis();
    let launcher::LaunchedBrowser {
        mut child,
        cmd_write,
        rsp_read,
    } = launched;
    let cdp = CdpClient::new(rsp_read, cmd_write);

    let version = cdp
        .call(None, "Browser.getVersion", json!({}))
        .ok()
        .and_then(|v| v["product"].as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "unknown".into());

    // --- Fetch 拦截线程（事件自带 sessionId，扁平模式） ---
    let stop = Arc::new(AtomicBool::new(false));
    let fetch_log: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    {
        let cdp = Arc::clone(&cdp);
        let stop = Arc::clone(&stop);
        let fetch_log = Arc::clone(&fetch_log);
        let roots = allowed_roots.clone();
        std::thread::spawn(move || loop {
            if stop.load(Ordering::SeqCst) {
                break;
            }
            match cdp.wait_event("Fetch.requestPaused", |_| true, Duration::from_millis(100)) {
                Ok(ev) => {
                    let sid = ev["sessionId"].as_str().unwrap_or("").to_string();
                    let params = &ev["params"];
                    let req_id = params["requestId"].as_str().unwrap_or("").to_string();
                    let url = params["request"]["url"].as_str().unwrap_or("").to_string();
                    let resource = params["resourceType"].as_str().unwrap_or("?").to_string();
                    match pdf::decide_fetch(&url, &roots) {
                        pdf::FetchDecision::Allow => {
                            fetch_log.lock().unwrap().push(
                                json!({"decision":"ALLOW","url":url,"resourceType":resource}),
                            );
                            let _ = cdp.call(
                                Some(&sid),
                                "Fetch.continueRequest",
                                json!({ "requestId": req_id }),
                            );
                        }
                        pdf::FetchDecision::Deny(reason) => {
                            fetch_log.lock().unwrap().push(
                                json!({"decision":"DENY","url":url,"resourceType":resource,"reason":reason}),
                            );
                            let _ = cdp.call(
                                Some(&sid),
                                "Fetch.failRequest",
                                json!({ "requestId": req_id, "errorReason": "AccessDenied" }),
                            );
                        }
                    }
                }
                Err(lingxi_browser_spike::cdp::CdpError::Timeout(_)) => continue, // 轮询节拍
                Err(_) => break,
            }
        });
    }

    // --- 渲染 ---
    let started = Instant::now();
    let result = pdf::render_job(&cdp, &job, Some(&themes_dir), started);
    stop.store(true, Ordering::SeqCst);

    let (exit_code, outcome_json) = match &result {
        Ok(outcome) => {
            // 原子写出：先 tmp 再 rename；失败路径绝不留产物
            if let Some(parent) = job.output_path.parent() {
                fs::create_dir_all(parent).ok();
            }
            let tmp = job
                .output_path
                .with_extension(format!("tmp-{}", std::process::id()));
            match fs::write(&tmp, &outcome.pdf).and_then(|_| fs::rename(&tmp, &job.output_path)) {
                Ok(()) => (
                    0,
                    json!({
                        "status": "ok",
                        "output_path": job.output_path.display().to_string(),
                        "pdf_bytes": outcome.pdf.len(),
                        "pdf_sha256": outcome.pdf_sha256,
                    }),
                ),
                Err(e) => {
                    fs::remove_file(&tmp).ok();
                    (
                        1,
                        json!({ "status": "error", "error": format!("write output: {e}") }),
                    )
                }
            }
        }
        Err(e) => {
            let timeout = error_is_timeout(e);
            (
                if timeout { 2 } else { 1 },
                json!({ "status": "error", "error": e }),
            )
        }
    };

    // --- 清理：浏览器 + profile + 残留核实 ---
    let _ = cdp.call(None, "Browser.close", json!({}));
    std::thread::sleep(Duration::from_millis(500));
    kill_child(&mut child);
    let leftovers_before_rm = ps_scan(&profile.display().to_string());
    fs::remove_dir_all(&profile).ok();
    let profile_removed = !profile.exists();
    let leftovers_after = ps_scan(&profile.display().to_string());

    let fetch_entries = fetch_log.lock().unwrap().clone();
    let fetch_log_path = evidence.join("fetch-log.jsonl");
    let mut fetch_text = String::new();
    for e in &fetch_entries {
        fetch_text.push_str(&serde_json::to_string(e).unwrap_or_default());
        fetch_text.push('\n');
    }
    fs::write(&fetch_log_path, fetch_text).ok();

    let result_json = json!({
        "job": job_path,
        "chrome": chrome.display().to_string(),
        "browser_version": version,
        "headless": !headful,
        "proxy": proxy,
        "launch_ms": launch_ms,
        "outcome": outcome_json,
        "timings": result.as_ref().ok().map(|o| json!({
            "attach_ms": o.timings.attach_ms,
            "navigate_ms": o.timings.navigate_ms,
            "font_inject_ms": o.timings.font_inject_ms,
            "asset_wait_ms": o.timings.asset_wait_ms,
            "print_ms": o.timings.print_ms,
            "total_ms": o.timings.total_ms,
        })),
        "asset_wait_note": result.as_ref().ok().map(|o| o.asset_wait_note.clone()),
        "fetch_decisions": fetch_entries.len(),
        "fetch_denies": fetch_entries.iter().filter(|e| e["decision"] == "DENY").count(),
        // WebSocket 不经 Fetch 域拦截、Network.setBlockedURLs 对 WS 实测无效
        // （R1 验收 F1）；边界由 render_job 注入的 CSP connect-src（排除 ws:/wss:）承担。
        "connect_boundary": "CSP connect-src (ws:/wss: excluded) via Page.addScriptToEvaluateOnNewDocument",
        "cleanup": {
            "leftover_processes_before_profile_rm": leftovers_before_rm,
            "profile_removed": profile_removed,
            "leftover_processes_after": leftovers_after,
        },
        "wall_ms": started.elapsed().as_millis() + launch_ms,
    });
    fs::write(
        evidence.join("spike-result.json"),
        serde_json::to_string_pretty(&result_json).unwrap_or_default(),
    )
    .ok();
    println!(
        "{}",
        serde_json::to_string(&result_json).unwrap_or_default()
    );
    std::process::exit(exit_code);
}

#[cfg(test)]
mod tests {
    use super::error_is_timeout;

    /// F2 回归：两类超时文案都必须判定为超时（exit 2），非超时错误不得误判。
    #[test]
    fn timeout_exit_code_classification() {
        // load/导航阶段（render_job 自造文案）
        assert!(error_is_timeout("loadURL timed out after 5000ms"));
        // printToPDF 阶段（CDP CdpError::Timeout 的 Display 文案，R1 验收 ATK5 实测）
        assert!(error_is_timeout(
            "printToPDF: timeout: Page.printToPDF id=15"
        ));
        assert!(error_is_timeout("navigate: timeout: Page.navigate id=7"));
        // 非超时错误一律 exit 1
        assert!(!error_is_timeout("write output: Permission denied"));
        assert!(!error_is_timeout("navigate: io: broken pipe"));
        assert!(!error_is_timeout(
            "output validation failed: missing %PDF- header"
        ));
    }
}
