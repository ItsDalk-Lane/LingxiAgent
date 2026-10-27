//! 真实 stdio 背压检查；由 mcpbridge 的 Unix 测试子模块挂载。
//!
//! 本文件不替换 SDK、队列、时钟或存储。单线程运行时的一轮同步轮询
//! 使 SDK 的真实 1024 项发送队列可确定填满；交还运行时后，SDK 可以继续
//! 把队列转为等待写管道的任务，不能把外端暂停读取称为队列持续满。

use super::*;

use std::path::Path;
use std::task::{Wake, Waker};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const QUEUE_CAPACITY: usize = 1024;
const SEND_COUNT: usize = 1100;
const PAYLOAD_BYTES: usize = 64 * 1024;

const EXTERNAL: &str = r#"
import fs from 'node:fs';
import readline from 'node:readline';
const [arrivals, paused, resume] = process.argv.slice(2);
const reply = (id,result) => process.stdout.write(JSON.stringify({jsonrpc:'2.0',id,result})+'\n');
const lines = readline.createInterface({input:process.stdin});
let stopped = false;
let resumed = false;
lines.on('line', line => {
  const m = JSON.parse(line);
  fs.appendFileSync(arrivals, JSON.stringify({id:m.id,method:m.method,name:m.params?.name,nonce:m.params?.arguments?.nonce,payloadBytes:m.params?.arguments?.payload?.length ?? 0})+'\n');
  if (m.method === 'initialize') {
    reply(m.id,{protocolVersion:m.params.protocolVersion,capabilities:{tools:{}},serverInfo:{name:'owned-backpressure',version:'1'}});
  } else if (m.method === 'notifications/initialized' && !stopped) {
    stopped = true;
    lines.pause();
    process.stdin.pause();
    // 真实暂停后先写完整进程号，再在同目录原子发布确认，避免读到空文件。
    const pausedTemp = `${paused}.${process.pid}.tmp`;
    fs.writeFileSync(pausedTemp, String(process.pid), {flag:'wx'});
    fs.renameSync(pausedTemp, paused);
  } else if (m.method === 'tools/call') {
    reply(m.id,{content:[{type:'text',text:String(m.params.arguments.nonce)}]});
  }
});
const control = setInterval(() => {
  if (stopped && !resumed && fs.existsSync(resume)) {
    resumed = true;
    lines.resume();
    process.stdin.resume();
  }
}, 5);
lines.on('close', () => { clearInterval(control); process.exit(0); });
"#;

struct PollSignal(AtomicUsize);

impl Wake for PollSignal {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

struct OwnedExternal {
    root: PathBuf,
    arrivals: PathBuf,
    resume: PathBuf,
    server: Arc<McpServer>,
    pid: u32,
}

impl OwnedExternal {
    async fn boot() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lingxi-r07-mcp-real-backpressure-{}",
            lingxi_adapters::storage::agent_memory::random_token_suffix()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let script = root.join("external.mjs");
        let arrivals = root.join("arrivals.ndjson");
        let paused = root.join("paused.pid");
        let resume = root.join("resume");
        std::fs::write(&script, EXTERNAL).unwrap();
        let server = McpServer::new(
            "real-backpressure",
            McpEndpoint::Stdio {
                command: "node".into(),
                args: [&script, &arrivals, &paused, &resume]
                    .into_iter()
                    .map(|path| path.to_string_lossy().into_owned())
                    .collect(),
                env: BTreeMap::new(),
                cwd: Some(root.clone()),
            },
        );
        server.connect().await.unwrap();
        wait_until("外端完成 initialize 后真实暂停读取", || {
            paused.exists()
        })
        .await;
        let pid = std::fs::read_to_string(paused)
            .unwrap()
            .parse::<u32>()
            .unwrap();
        {
            let connection = server.connection.lock().await;
            assert_eq!(
                connection
                    .as_ref()
                    .unwrap()
                    .child
                    .as_ref()
                    .unwrap()
                    .process
                    .id(),
                Some(pid),
                "暂停标记必须来自本连接实际拥有的子进程"
            );
        }
        eprintln!("MCP 真实背压现场：{}", root.display());
        Self {
            root,
            arrivals,
            resume,
            server,
            pid,
        }
    }

    fn records(&self) -> Vec<Value> {
        records(&self.arrivals)
    }

    async fn close(&self) {
        self.server.disconnect_checked().await.unwrap();
        assert!(!self.server.is_connected());
        assert!(self.server.connection.lock().await.is_none());
        let alive = unsafe { libc::kill(self.pid as i32, 0) };
        assert_eq!(alive, -1, "子进程必须由真实 wait 回收");
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH),
            "不把无法确认进程状态视为已退出"
        );
    }
}

impl Drop for OwnedExternal {
    fn drop(&mut self) {
        // 断言失败也让真实 SDK 和所拥有的子进程走现役所有者回收，不留外端。
        self.server.begin_stop();
    }
}

fn records(path: &Path) -> Vec<Value> {
    match std::fs::read_to_string(path) {
        Ok(text) => text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => panic!("读取实际到达记录失败：{error}"),
    }
}

async fn wait_until(label: &str, mut condition: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("等待真实状态超时：{label}"));
}

fn arguments(nonce: impl ToString, large: bool) -> serde_json::Map<String, Value> {
    json!({"nonce":nonce.to_string(),"payload":if large {"x".repeat(PAYLOAD_BYTES)} else {String::new()}})
        .as_object()
        .unwrap()
        .clone()
}

fn lifetime(cancel: crate::cancel::CancelScope) -> crate::toolgateway::TrustedToolLifetime {
    crate::toolgateway::TrustedToolLifetime {
        workspace: None,
        cancel,
        deadline_unix_ms: u64::MAX,
        budget_ms: 30_000,
    }
}

#[tokio::test(flavor = "current_thread")]
async fn real_sdk_full_queue_honors_request_deadline_and_scope_cancel_without_replay() {
    let external = OwnedExternal::boot().await;
    let peer = {
        let connection = external.server.connection.lock().await;
        connection.as_ref().unwrap().service.peer().clone()
    };

    let already_cancelled = crate::cancel::CancelScope::run_root("already-cancelled-mcp");
    assert!(already_cancelled.cancel("开始前已经取消"));
    let result = crate::toolgateway::TOOL_LIFETIME
        .scope(lifetime(already_cancelled), async {
            external
                .server
                .call_raw(
                    "already-cancelled",
                    arguments("already-cancelled", true),
                    1000,
                )
                .await
        })
        .await;
    assert_eq!(result.unwrap_err(), McpCallFailure::Cancelled);

    let signal = Arc::new(PollSignal(AtomicUsize::new(0)));
    let waker = Waker::from(Arc::clone(&signal));
    let mut cx = TaskContext::from_waker(&waker);
    let mut accepted = Vec::new();
    let mut pending = Vec::new();
    // 这里直到明确取消结果前没有 await，SDK 的服务任务不能插队消费队列。
    // unconstrained 排除 Tokio 每轮协作预算耗尽，Pending 才确实来自满队列。
    for nonce in 0..SEND_COUNT {
        let request = ClientRequest::CallToolRequest(CallToolRequest::new(
            CallToolRequestParams::new("fill").with_arguments(arguments(nonce, true)),
        ));
        let mut send = Box::pin(tokio::task::unconstrained(peer.send_cancellable_request(
            request,
            rmcp::service::PeerRequestOptions::no_options(),
        )));
        match send.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(handle)) => accepted.push((nonce, handle)),
            Poll::Ready(Err(error)) => panic!("真实 SDK 发送提前失败：{error}"),
            Poll::Pending => pending.push(send),
        }
    }
    assert_eq!(
        accepted.len(),
        QUEUE_CAPACITY,
        "必须直接观察真实 SDK 队列容量"
    );
    assert_eq!(pending.len(), SEND_COUNT - QUEUE_CAPACITY);
    assert!(external
        .records()
        .iter()
        .all(|row| row["method"] != "tools/call"));
    drop(pending);

    let cancel = crate::cancel::CancelScope::run_root("queued-mcp");
    let mut cancelled_call = Box::pin(tokio::task::unconstrained(
        crate::toolgateway::TOOL_LIFETIME.scope(lifetime(cancel.clone()), async {
            external
                .server
                .call_raw("queued-cancel", arguments("queued-cancel", true), 30_000)
                .await
        }),
    ));
    assert!(cancelled_call.as_mut().poll(&mut cx).is_pending());
    let cancel_started = Instant::now();
    assert!(cancel.cancel("真实排队期间取消"));
    match cancelled_call.as_mut().poll(&mut cx) {
        Poll::Ready(Err(McpCallFailure::Cancelled)) => {}
        other => panic!("满队列取消必须在交还运行时前返回 Cancelled：{other:?}"),
    }
    assert!(cancel_started.elapsed() < Duration::from_secs(1));
    drop(cancelled_call);

    let deadline_started = Instant::now();
    let mut deadline_call = Box::pin(tokio::task::unconstrained(external.server.call_raw(
        "queued-deadline",
        arguments("queued-deadline", true),
        80,
    )));
    assert!(deadline_call.as_mut().poll(&mut cx).is_pending());
    // 这里只证明开始时确在排队；让出运行时后 SDK 可以把队列转成写任务。
    // 因此上界包含现役精确请求取消通知最多 2 秒，不谎称全程保持满队列。
    let result = tokio::time::timeout(Duration::from_secs(4), deadline_call)
        .await
        .expect("发送背压不能绕过整个请求的 deadline 与有界取消");
    assert_eq!(result.unwrap_err(), McpCallFailure::DeadlineExceeded);
    assert!(deadline_started.elapsed() < Duration::from_secs(4));
    let original_lifetime = {
        let connection = external.server.connection.lock().await;
        connection.as_ref().unwrap().requests.clone()
    };
    assert!(
        original_lifetime
            .cancellation_unconfirmed
            .load(Ordering::Acquire),
        "外端尚未读取，原单次取消投递超时必须继续保留未知事实"
    );
    assert_eq!(
        original_lifetime.active.load(Ordering::Acquire),
        0,
        "已返回deadline的本地调用必须完成有界收尾"
    );
    assert!(external
        .records()
        .iter()
        .all(|row| row["method"] != "tools/call"));
    eprintln!(
        "MCP 真实背压：accepted={} pending={} payloadBytes={} deadlineMs=80 elapsedMs={}；取消请求在满队列同轮返回，外端仍未读取",
        accepted.len(), SEND_COUNT - QUEUE_CAPACITY, PAYLOAD_BYTES,
        deadline_started.elapsed().as_millis()
    );

    std::fs::write(&external.resume, "继续读取真实请求").unwrap();
    wait_until("所有确实入队的请求到达同一个外端", || {
        external
            .records()
            .iter()
            .filter(|row| row["name"] == "fill")
            .count()
            >= QUEUE_CAPACITY
    })
    .await;
    // 测试自身没有丢下后台发送任务；每个成功入队句柄都等到外端真实回执。
    tokio::time::timeout(Duration::from_secs(10), async {
        for (nonce, handle) in accepted {
            let response = handle.await_response().await.unwrap();
            let ServerResult::CallToolResult(result) = response else {
                panic!("填队列请求必须得到原工具回执");
            };
            assert_eq!(
                serde_json::to_value(result).unwrap()["content"][0]["text"],
                nonce.to_string()
            );
        }
    })
    .await
    .expect("所有旧请求句柄必须收尾");
    let echo = external
        .server
        .call_raw("echo", arguments("echo-after-pressure", false), 3000)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(echo).unwrap()["content"][0]["text"],
        "echo-after-pressure"
    );
    assert_eq!(external.server.connect_count(), 1, "不能靠重连重放旧请求");
    drop(peer);
    assert!(
        original_lifetime
            .cancellation_unconfirmed
            .load(Ordering::Acquire),
        "恢复后的echo不能改写旧调用的未知事实"
    );
    external.close().await;
    assert_eq!(original_lifetime.active.load(Ordering::Acquire), 0);
    eprintln!("MCP 真实背压退出：历史取消通知未确认仍保留，当前active=0；close成功，实际SDK连接为空，owned pid={} 已由wait回收且kill0=ESRCH", external.pid);

    let rows = external.records();
    let calls = rows
        .iter()
        .filter(|row| row["method"] == "tools/call")
        .collect::<Vec<_>>();
    let mut ids = BTreeSet::new();
    let mut nonces = BTreeMap::<String, usize>::new();
    for row in &calls {
        assert!(ids.insert(row["id"].to_string()), "真实请求编号不得重放");
        *nonces
            .entry(row["nonce"].as_str().unwrap().into())
            .or_default() += 1;
        if row["name"] == "fill" {
            assert_eq!(row["payloadBytes"], PAYLOAD_BYTES);
        }
    }
    for nonce in 0..QUEUE_CAPACITY {
        assert_eq!(
            nonces.get(&nonce.to_string()),
            Some(&1),
            "已入队旧请求必须恰好一次"
        );
    }
    for nonce in QUEUE_CAPACITY..SEND_COUNT {
        assert!(
            !nonces.contains_key(&nonce.to_string()),
            "未入队 future 丢弃后不得晚投递"
        );
    }
    for nonce in ["already-cancelled", "queued-cancel"] {
        assert!(
            !nonces.contains_key(nonce),
            "取消发生在入队前，外端必须零到达"
        );
    }
    assert_eq!(nonces.get("echo-after-pressure"), Some(&1));
    let deadline_arrivals = nonces.get("queued-deadline").copied().unwrap_or(0);
    assert!(deadline_arrivals <= 1, "截止请求可能已发，但绝不重放");
    assert_eq!(calls.len(), QUEUE_CAPACITY + 1 + deadline_arrivals);
    std::fs::remove_dir_all(&external.root).unwrap();
}
