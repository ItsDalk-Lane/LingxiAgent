//! 诊断与扫码只替换平台等待响应；真实子进程、退出登记和管道均不替换。
use lingxi_protocol::BridgeDiagnosticCommand;
use lingxi_service::{bridge::BridgeHost, bridge_worker::BridgeWorkerLaunch};
use lingxi_test_guard as _;
use serde_json::json;
use std::{path::PathBuf, sync::Arc, time::Duration};
fn fixture(delay: Option<u64>) -> (Arc<BridgeHost>, PathBuf) {
    let home = std::env::temp_dir().join(format!(
        "r07-bridge-diagnostic-{}",
        lingxi_adapters::storage::agent_memory::random_token_suffix()
    ));
    std::fs::create_dir_all(&home).unwrap();
    let worker = home.join("bridge-sdk-worker.mjs");
    std::fs::write(&worker, "").unwrap();
    let script = format!(
        r#"import fs from 'node:fs';import readline from 'node:readline';
const rl=readline.createInterface({{input:process.stdin}});
rl.once('line',()=>{{fs.appendFileSync(new URL('./pids',import.meta.url),String(process.pid)+'\n');if({delay}!==null)setTimeout(()=>{{process.stdout.write(JSON.stringify({{kind:'diagnostic_result',ok:true,status:'waiting'}})+'\n');}}, {delay});}});
"#,
        delay = delay
            .map(|n| n.to_string())
            .unwrap_or_else(|| "null".into())
    );
    std::fs::write(home.join("bridge-sdk-diagnostic.mjs"), script).unwrap();
    let node = std::process::Command::new("which")
        .arg("node")
        .output()
        .unwrap();
    assert!(node.status.success());
    let host = Arc::new(BridgeHost::new(Some(BridgeWorkerLaunch {
        program: PathBuf::from(String::from_utf8(node.stdout).unwrap().trim()),
        args: vec![worker.to_string_lossy().into_owned()],
        cwd: home.clone(),
    })));
    (host, home)
}
#[tokio::test]
async fn normal_qrcode_long_poll_can_exceed_twenty_seconds_and_is_reaped() {
    let (host, home) = fixture(Some(21_000));
    let result = tokio::time::timeout(
        Duration::from_secs(44),
        host.diagnostic(BridgeDiagnosticCommand::WechatQrcodeStatus {
            qrcode_id: "synthetic-no-account".into(),
        }),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(result["status"], "waiting");
    host.stop();
    tokio::time::timeout(Duration::from_secs(5), host.wait_stopped())
        .await
        .unwrap()
        .unwrap();
    assert_reaped(&home);
}
#[tokio::test]
async fn concurrent_diagnostics_cannot_register_a_process_after_shutdown_returns() {
    let (host, home) = fixture(None);
    let mut jobs = Vec::new();
    for _ in 0..16 {
        let host = host.clone();
        jobs.push(tokio::spawn(async move {
            host.diagnostic(BridgeDiagnosticCommand::Verify {
                platform: "telegram".into(),
                config: json!({"token":"synthetic"}),
            })
            .await
        }));
    }
    tokio::time::timeout(Duration::from_secs(5), async {
        while !home.join("pids").exists() {
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    host.stop();
    tokio::time::timeout(Duration::from_secs(6), host.wait_stopped())
        .await
        .unwrap()
        .unwrap();
    for job in jobs {
        assert!(job.await.unwrap().is_err());
    }
    assert!(host
        .diagnostic(BridgeDiagnosticCommand::WechatQrcode)
        .await
        .is_err());
    assert_reaped(&home);
}
fn assert_reaped(home: &std::path::Path) {
    for pid in std::fs::read_to_string(home.join("pids")).unwrap().lines() {
        #[cfg(unix)]
        assert_eq!(
            unsafe { libc::kill(pid.parse().unwrap(), 0) },
            -1,
            "自有诊断进程仍存活"
        );
    }
}
