//! 真实子进程和管道验证，不访问平台账户或网络。
use lingxi_service::bridge_worker::{BridgeWorkerLaunch, BridgeWorkerProcess, MAX_FRAME_BYTES};
use lingxi_test_guard as _;
use serde_json::json;
use std::{path::PathBuf, time::Duration};

fn fixture(body: &str) -> BridgeWorkerLaunch {
    let dir = std::env::temp_dir().join(format!(
        "r07-bridge-worker-{}",
        lingxi_adapters::storage::agent_memory::random_token_suffix()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("fixture.js");
    std::fs::write(&script, body).unwrap();
    let node = std::process::Command::new("which")
        .arg("node")
        .output()
        .unwrap();
    assert!(node.status.success());
    BridgeWorkerLaunch {
        program: PathBuf::from(String::from_utf8(node.stdout).unwrap().trim()),
        args: vec![script.to_string_lossy().into_owned()],
        cwd: dir,
    }
}
#[tokio::test]
async fn worker_uses_real_pipe_and_does_not_inherit_service_secrets() {
    let spec = fixture("process.stdin.once('data', () => { console.log(JSON.stringify({kind:'ready',home:process.env.HOME??null,nodeOptions:process.env.NODE_OPTIONS??null})); });setInterval(()=>{},1000)");
    let mut worker = BridgeWorkerProcess::spawn(&spec, &json!({"kind":"init"}))
        .await
        .unwrap();
    let pid = worker.process_id().unwrap();
    assert!(pid > 0);
    assert_eq!(
        worker.next().await.unwrap(),
        Some(json!({"kind":"ready","home":null,"nodeOptions":null}))
    );
    worker.stop().await.unwrap();
    assert_eq!(worker.process_id(), None);
    worker.stop().await.unwrap();
}
#[tokio::test]
async fn oversized_output_is_bounded_and_worker_is_reaped() {
    let spec = fixture(&format!("process.stdin.once('data',()=>process.stdout.write('x'.repeat({})));setInterval(()=>{{}},1000)",MAX_FRAME_BYTES+100));
    let mut worker = BridgeWorkerProcess::spawn(&spec, &json!({"kind":"init"}))
        .await
        .unwrap();
    assert!(worker.next().await.unwrap_err().contains("byte limit"));
    worker.stop().await.unwrap();
    assert_eq!(worker.process_id(), None);
}
#[tokio::test]
async fn cancelled_read_preserves_partial_frame() {
    let spec = fixture("process.stdin.once('data',()=>{process.stdout.write('{\"kind\":');setTimeout(()=>process.stdout.write('\"ready\"}\\n'),100)});setInterval(()=>{},1000)");
    let mut worker = BridgeWorkerProcess::spawn(&spec, &json!({"kind":"init"}))
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(50), worker.next())
            .await
            .is_err()
    );
    assert_eq!(worker.next().await.unwrap(), Some(json!({"kind":"ready"})));
    worker.stop().await.unwrap();
}
#[tokio::test]
async fn incomplete_frame_and_malformed_output_never_become_messages() {
    for script in [
        "process.stdin.once('data',()=>{process.stdout.write('{');process.exit(0)})",
        "process.stdin.once('data',()=>console.log('not-json'));setInterval(()=>{},1000)",
    ] {
        let spec = fixture(script);
        let mut worker = BridgeWorkerProcess::spawn(&spec, &json!({"kind":"init"}))
            .await
            .unwrap();
        assert!(worker.next().await.is_err());
        worker.stop().await.unwrap();
    }
}
#[tokio::test]
async fn command_limit_refuses_dispatch_and_existing_worker_remains_stoppable() {
    let spec = fixture("process.stdin.once('data',()=>console.log('{\"kind\":\"ready\"}'));setInterval(()=>{},1000)");
    let mut worker = BridgeWorkerProcess::spawn(&spec, &json!({"kind":"init"}))
        .await
        .unwrap();
    assert_eq!(worker.next().await.unwrap(), Some(json!({"kind":"ready"})));
    assert!(worker
        .send(&json!({"kind":"send","text":"x".repeat(MAX_FRAME_BYTES)}))
        .await
        .unwrap_err()
        .contains("byte limit"));
    worker.stop().await.unwrap();
}
