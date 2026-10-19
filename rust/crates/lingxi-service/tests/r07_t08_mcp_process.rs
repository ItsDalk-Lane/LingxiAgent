//! 只替换外部MCP响应进程，真实SDK初始化、取消与子进程组回收。
#![cfg(unix)]
use lingxi_service::mcpbridge::{McpEndpoint, McpServer};
use lingxi_test_guard as _;
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf, time::Duration};

struct Fixture {
    root: PathBuf,
    server: std::sync::Arc<McpServer>,
}
impl Fixture {
    fn new(hang: bool) -> Self {
        let root = std::env::temp_dir().join(format!(
            "lingxi-r07-mcp-process-{}",
            lingxi_adapters::storage::agent_memory::random_token_suffix()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let script = root.join("external.mjs");
        std::fs::write(&script, r#"
import fs from 'node:fs';
import readline from 'node:readline';
import {spawn} from 'node:child_process';
const child=spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{stdio:'ignore'});
fs.writeFileSync(process.argv[2],JSON.stringify({parent:process.pid,child:child.pid}));
const input=readline.createInterface({input:process.stdin});
// 刻意在stdin关闭后仍存活，不能靠外部程序配合退出掩盖宿主清理问题。
setInterval(()=>{},1000);
input.on('line',line=>{
 const msg=JSON.parse(line);
 if(msg.method==='initialize'&&process.argv[3]!=='hang')process.stdout.write(JSON.stringify({jsonrpc:'2.0',id:msg.id,result:{protocolVersion:msg.params.protocolVersion,capabilities:{tools:{}},serverInfo:{name:'owned-stubborn-fixture',version:'1'}}})+'\n');
 if(msg.method==='tools/list')process.stdout.write(JSON.stringify({jsonrpc:'2.0',id:msg.id,result:{tools:[]}})+'\n');
});
"#).unwrap();
        let server = McpServer::new(
            "owned-stubborn",
            McpEndpoint::Stdio {
                command: "node".into(),
                args: vec![
                    script.to_string_lossy().into(),
                    root.join("pids.json").to_string_lossy().into(),
                    if hang { "hang" } else { "normal" }.into(),
                ],
                env: BTreeMap::new(),
                cwd: Some(root.clone()),
            },
        );
        Self { root, server }
    }
    async fn pids(&self) -> Vec<i32> {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Ok(bytes) = std::fs::read(self.root.join("pids.json")) {
                    if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
                        return ["parent", "child"]
                            .iter()
                            .map(|key| i32::try_from(value[*key].as_u64().unwrap()).unwrap())
                            .collect();
                    }
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("自有外部进程应写入真实编号")
    }
}
fn alive(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}
async fn gone(pids: &[i32]) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while pids.iter().copied().any(alive) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("断连必须回收自有进程及后代，不能仅关闭管道");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_stubborn_mcp_disconnect_reconnect_and_owner_drop_reap_process_groups() {
    let fixture = Fixture::new(false);
    tokio::time::timeout(Duration::from_secs(3), fixture.server.connect())
        .await
        .unwrap()
        .unwrap();
    let first = fixture.pids().await;
    assert!(first.iter().copied().all(alive));
    // 重连必须先收旧连接与进程，再完成新初始化，不重放工具。
    tokio::time::timeout(Duration::from_secs(3), fixture.server.connect())
        .await
        .unwrap()
        .unwrap();
    gone(&first).await;
    let second = fixture.pids().await;
    assert_ne!(first, second);
    assert!(second.iter().copied().all(alive));
    fixture.server.disconnect_checked().await.unwrap();
    assert!(!fixture.server.is_connected());
    gone(&second).await;
    fixture.server.connect().await.unwrap();
    let final_pids = fixture.pids().await;
    drop(fixture.server);
    gone(&final_pids).await;
    std::fs::remove_dir_all(fixture.root).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_mcp_initialization_cancellation_reaps_process_before_connection_exists() {
    let fixture = Fixture::new(true);
    let pids = {
        let connect = fixture.server.connect();
        tokio::pin!(connect);
        // 同时驱动真实握手和观察外部进程，不能先等待一个尚未轮询的future。
        let pids = tokio::select! {
            result=&mut connect=>panic!("故意不回应的握手不应提前成功：{result:?}"),
            pids=fixture.pids()=>pids,
        };
        assert!(pids.iter().copied().all(alive));
        assert!(
            tokio::time::timeout(Duration::from_millis(80), &mut connect)
                .await
                .is_err()
        );
        // 离开作用域才丢弃被pin的原future，触发真实取消与RAII清理。
        pids
    };
    gone(&pids).await;
    drop(fixture.server);
    std::fs::remove_dir_all(fixture.root).unwrap();
}
