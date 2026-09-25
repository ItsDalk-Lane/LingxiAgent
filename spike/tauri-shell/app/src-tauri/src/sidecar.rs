//! Sidecar 生命周期管理（宿主 Rust 侧，前端只可经窄命令 status/ping/restart 接触）：
//! 随宿主启动、就绪握手（hello 行 + 版本/协议校验）、版本不匹配拒绝、
//! 有序退出（shutdown 握手）、异常崩溃检测与有限恢复（最多重启 1 次，再死则标 dead）。
//! 握手校验是纯逻辑 `validate_hello`，集成测试直接驱动真实 sidecar 二进制。

use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;

use crate::report::Reporter;

pub const EXPECTED_SIDECAR_VERSION: &str = "1.0.0";
pub const EXPECTED_PROTOCOL: u64 = 1;
const MAX_RESTARTS: u32 = 1;

type PendingMap = Arc<Mutex<HashMap<u64, tokio::sync::oneshot::Sender<Value>>>>;

pub struct SidecarInner {
    child: Option<CommandChild>,
    pid: Option<u32>,
    dead: bool,
    restarts: u32,
    hello: Option<Value>,
    seq: u64,
}

pub struct SharedSidecar {
    inner: Mutex<SidecarInner>,
    pending: PendingMap,
}

impl SharedSidecar {
    pub fn new() -> Self {
        SharedSidecar {
            inner: Mutex::new(SidecarInner {
                child: None,
                pid: None,
                dead: true,
                restarts: 0,
                hello: None,
                seq: 0,
            }),
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

/// 纯逻辑：校验 hello 行。Ok(hello) 或 Err(拒绝原因)。
pub fn validate_hello(line: &str) -> Result<Value, String> {
    let v: Value = serde_json::from_str(line).map_err(|e| format!("hello not json: {e}"))?;
    if v.get("lingxi_sidecar_hello") != Some(&Value::Bool(true)) {
        return Err("missing lingxi_sidecar_hello marker".into());
    }
    if v.get("name").and_then(Value::as_str) != Some("lingxi-t06-sidecar") {
        return Err("unexpected sidecar name".into());
    }
    let version = v
        .get("version")
        .and_then(Value::as_str)
        .ok_or("missing version")?;
    if version != EXPECTED_SIDECAR_VERSION {
        return Err(format!(
            "sidecar version mismatch: expected {EXPECTED_SIDECAR_VERSION}, got {version}"
        ));
    }
    let protocol = v
        .get("protocol")
        .and_then(Value::as_u64)
        .ok_or("missing protocol")?;
    if protocol != EXPECTED_PROTOCOL {
        return Err(format!(
            "sidecar protocol mismatch: expected {EXPECTED_PROTOCOL}, got {protocol}"
        ));
    }
    Ok(v)
}

/// 启动 sidecar 并完成握手；失败即杀掉子进程并返回 Err（不静默降级）。
pub async fn spawn_and_handshake(
    app: &tauri::AppHandle,
    shared: Arc<SharedSidecar>,
) -> Result<Value, String> {
    let reporter = Reporter::global();
    let cmd = app
        .shell()
        .sidecar("lingxi-t06-sidecar")
        .map_err(|e| format!("sidecar() failed: {e}"))?;
    let (mut rx, child) = cmd.spawn().map_err(|e| format!("spawn failed: {e}"))?;
    let pid = child.pid();

    let (hello_tx, hello_rx) = tokio::sync::oneshot::channel::<Result<Value, String>>();
    let mut hello_tx = Some(hello_tx);

    // 事件泵：stdout 行 -> 握手或按 seq 路由到 pending；Terminated -> 标记 dead 并唤醒等待者。
    let pending_for_pump = shared.pending.clone();
    let shared_for_pump = shared.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            match event {
                CommandEvent::Stdout(line) => {
                    let line = String::from_utf8_lossy(&line).to_string();
                    if let Some(tx) = hello_tx.take() {
                        let _ = tx.send(validate_hello(line.trim()));
                        continue;
                    }
                    if let Ok(v) = serde_json::from_str::<Value>(line.trim()) {
                        if let Some(seq) = v.get("seq").and_then(Value::as_u64) {
                            if let Some(tx) = pending_for_pump.lock().unwrap().remove(&seq) {
                                let _ = tx.send(v);
                            }
                        }
                    }
                }
                CommandEvent::Stderr(line) => {
                    Reporter::global().event(
                        "sidecar.stderr",
                        serde_json::json!({"line": String::from_utf8_lossy(&line)}),
                    );
                }
                CommandEvent::Error(e) => {
                    Reporter::global().event("sidecar.error", serde_json::json!({"error": e}));
                }
                CommandEvent::Terminated(status) => {
                    Reporter::global().event(
                        "sidecar.terminated",
                        serde_json::json!({"code": status.code, "signal": status.signal}),
                    );
                    {
                        let mut g = shared_for_pump.inner.lock().unwrap();
                        g.dead = true;
                        g.child = None;
                        g.pid = None;
                    }
                    let mut pend = pending_for_pump.lock().unwrap();
                    for (_, tx) in pend.drain() {
                        let _ =
                            tx.send(serde_json::json!({"ok": false, "error": "sidecar_terminated"}));
                    }
                }
                _ => {}
            }
        }
    });

    let hello = match tokio::time::timeout(std::time::Duration::from_secs(5), hello_rx).await {
        Ok(Ok(Ok(v))) => v,
        Ok(Ok(Err(e))) => {
            reporter.event("sidecar.handshake_rejected", serde_json::json!({"reason": e}));
            let _ = child.kill();
            return Err(e);
        }
        Ok(Err(_)) => {
            let _ = child.kill();
            return Err("sidecar hello channel closed".into());
        }
        Err(_) => {
            reporter.event("sidecar.handshake_timeout", serde_json::json!({}));
            let _ = child.kill();
            return Err("sidecar handshake timeout".into());
        }
    };

    {
        let mut g = shared.inner.lock().unwrap();
        g.child = Some(child);
        g.pid = Some(pid);
        g.dead = false;
        g.hello = Some(hello.clone());
    }
    reporter.event(
        "sidecar.spawned",
        serde_json::json!({"pid": pid, "hello": hello}),
    );
    Ok(hello)
}

pub fn status(shared: &SharedSidecar) -> Value {
    let g = shared.inner.lock().unwrap();
    serde_json::json!({
        "alive": !g.dead && g.child.is_some(),
        "pid": g.pid,
        "restarts": g.restarts,
        "hello": g.hello,
    })
}

async fn send_command(shared: &Arc<SharedSidecar>, cmd: &str) -> Result<Value, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    {
        let mut g = shared.inner.lock().unwrap();
        if g.dead || g.child.is_none() {
            return Err("sidecar not running".into());
        }
        g.seq += 1;
        let seq = g.seq;
        shared.pending.lock().unwrap().insert(seq, tx);
        let line = format!("{{\"cmd\":\"{cmd}\",\"seq\":{seq}}}\n");
        g.child
            .as_mut()
            .unwrap()
            .write(line.as_bytes())
            .map_err(|e| format!("write failed: {e}"))?;
    };
    match tokio::time::timeout(std::time::Duration::from_secs(3), rx).await {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(_)) => Err("response channel closed".into()),
        Err(_) => Err(format!("{cmd} timeout")),
    }
}

pub async fn ping(shared: &Arc<SharedSidecar>) -> Result<Value, String> {
    send_command(shared, "ping").await
}

pub async fn restart(app: &tauri::AppHandle, shared: &Arc<SharedSidecar>) -> Result<Value, String> {
    {
        let mut g = shared.inner.lock().unwrap();
        if g.restarts >= MAX_RESTARTS {
            return Err(format!(
                "restart budget exhausted (max {MAX_RESTARTS}); sidecar stays dead"
            ));
        }
        g.restarts += 1;
        if let Some(c) = g.child.take() {
            let _ = c.kill();
        }
        g.dead = true;
        g.pid = None;
    }
    let hello = spawn_and_handshake(app, shared.clone()).await?;
    Ok(serde_json::json!({"restarted": true, "hello": hello}))
}

/// 有序退出：发送 shutdown 命令，等待 bye；超时/失败强杀。同步包装供 finish_e2e 用。
pub fn shutdown_blocking(shared: &Arc<SharedSidecar>) -> Value {
    let shared = shared.clone();
    tauri::async_runtime::block_on(async move {
        if status(&shared)
            .get("alive")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            != true
        {
            return serde_json::json!({"orderly": false, "reason": "sidecar not running"});
        }
        match send_command(&shared, "shutdown").await {
            Ok(v) if v.get("bye") == Some(&Value::Bool(true)) => {
                serde_json::json!({"orderly": true, "bye": true})
            }
            other => {
                let mut g = shared.inner.lock().unwrap();
                if let Some(c) = g.child.take() {
                    let _ = c.kill();
                }
                serde_json::json!({"orderly": false, "killed": true, "detail": format!("{other:?}")})
            }
        }
    })
}
