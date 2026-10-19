//! 真正 SDK worker、平台适配器、Rust 服务、运行和存储；仅替换外部平台 SDK 与模型响应。
use lingxi_kernel::{
    model_exchange::ModelTurnInput,
    ports::{
        ModelTurnDelta, ProviderDescriptor, ProviderTurn, ProviderTurnResult, TurnDeltaSink,
        TurnProviderPort,
    },
};
use lingxi_protocol::{ContentBlock, ModelCallId, NormalizedMessage};
use lingxi_service::{
    bridge_worker::BridgeWorkerLaunch, inject::ManualClock, prepare_layout, HomeSource,
    NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
};
use lingxi_test_guard as _;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
const BODY: &str = "这是来自实际运行的完整回答。内容逐步到达，最终只投递一次。";
struct Provider {
    calls: AtomicUsize,
    hold: bool,
    inputs: Mutex<Vec<String>>,
}
impl TurnProviderPort for Provider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            provider: "r07.external.fixture".into(),
            model: "stream".into(),
            operation: "chat".into(),
        }
    }
    fn next_turn<'a>(
        &'a self,
        ctx: &'a lingxi_kernel::RunContext,
        call: &'a ModelCallId,
        input: &'a ModelTurnInput,
        sink: &'a dyn TurnDeltaSink,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inputs.lock().unwrap().push(input.submission.clone());
        Box::pin(async move {
            if sink
                .emit(ModelTurnDelta::Reasoning(
                    "PRIVATE_REASONING_MUST_NOT_BE_SENT".into(),
                ))
                .await
                .is_err()
            {
                return std::future::pending().await;
            }
            for part in [
                "<t> 10-16 ",
                "09:30 </t>",
                "这是来自实际运行的",
                "完整回答。",
                "内容逐步到达，",
                "最终只投递一次。",
            ] {
                if sink.emit(ModelTurnDelta::Text(part.into())).await.is_err() {
                    return std::future::pending().await;
                }
                tokio::time::sleep(Duration::from_millis(220)).await;
            }
            if self.hold {
                std::future::pending::<()>().await;
            }
            ProviderTurnResult::of_ctx(
                ctx,
                ProviderTurn::Final {
                    message: NormalizedMessage {
                        role: "assistant".into(),
                        content: vec![ContentBlock::Text {
                            text: format!("<t> 10-16 09:30 </t>{BODY}"),
                        }],
                        model_call_id: Some(call.clone()),
                    },
                },
            )
        })
    }
}
struct Harness {
    home: PathBuf,
    state: ServiceState,
    provider: Arc<Provider>,
    url: String,
    token: String,
    stop: tokio::sync::oneshot::Sender<()>,
    server: tokio::task::JoinHandle<()>,
}
impl Harness {
    async fn boot(existing: Option<PathBuf>, receipt: bool, rich: bool, hold: bool) -> Self {
        let _ = tracing_subscriber::fmt()
            .with_env_filter("lingxi_service=debug")
            .with_test_writer()
            .try_init();
        let home = existing.unwrap_or_else(|| {
            std::env::temp_dir().join(format!(
                "r07-real-sdk-{}",
                lingxi_adapters::storage::agent_memory::random_token_suffix()
            ))
        });
        std::fs::create_dir_all(home.join("agents/lingxi")).unwrap();
        std::fs::write(
            home.join("agents/lingxi/config.yaml"),
            "agent:\n  name: 合成助手\nmemory:\n  enabled: false\n",
        )
        .unwrap();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize()
            .unwrap();
        // hook 仅换掉外部 Telegram SDK；产品 worker、adapter、effects 均直接加载仓库正式文件。
        std::fs::write(home.join("platform-loader.mjs"),"import {registerHooks} from 'node:module';const originalFetch=globalThis.fetch;globalThis.fetch=(url,options)=>String(url)==='https://api.telegram.org/r07-fixture-file.txt'?Promise.resolve(new Response('BRIDGE_ACTUAL_FILE_BODY',{headers:{'content-type':'text/plain'}})):originalFetch(url,options);registerHooks({resolve(specifier,context,nextResolve){if(specifier==='node-telegram-bot-api')return {url:new URL('./telegram-platform.mjs',import.meta.url).href,shortCircuit:true};return nextResolve(specifier,context);}});").unwrap();
        std::fs::write(home.join("telegram-platform.mjs"),r#"import fs from 'node:fs';
const record=(kind,data={})=>fs.appendFileSync(new URL('./platform-operations.jsonl',import.meta.url),JSON.stringify({kind,...data})+'\n');
let id=100;export default class TelegramBot {
 constructor(){this.handlers={};record('construct');}
 on(kind,handler){this.handlers[kind]=handler;if(kind==='message')this.timer=setTimeout(()=>handler({message_id:1,chat:{id:'chat-1',type:'private'},from:{id:'sender-1',first_name:'合成发送者'},text:'实际消息',...(fs.existsSync(new URL('./incoming-document',import.meta.url))?{document:{file_id:'platform-file-1',file_name:'incoming.txt',mime_type:'text/plain',file_size:23}}:{})}),60);return this;}
 async getMe(){return {id:'synthetic-bot'};}
 async getUserProfilePhotos(){return {photos:[]};}
 async getFileLink(){return 'https://api.telegram.org/r07-fixture-file.txt';}
 async sendMessage(chat,text,options){record('sendMessage',{chat,text,options});return {message_id:++id};}
 async sendChatAction(chat,action,options){record('typing',{chat,action,options});return true;}
 async _request(method,request){record(method,request);return method==='sendRichMessage'?{message_id:++id}:true;}
 removeAllListeners(){clearTimeout(this.timer);this.handlers={};}
 async stopPolling(){record('stopPolling');return true;}
}
"#).unwrap();
        let node = std::process::Command::new("which")
            .arg("node")
            .output()
            .unwrap();
        assert!(node.status.success());
        let layout = prepare_layout(&home).unwrap();
        let provider = Arc::new(Provider {
            calls: AtomicUsize::new(0),
            hold,
            inputs: Mutex::new(Vec::new()),
        });
        let state = ServiceState::bootstrap_with_deps(
            ServiceConfig {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                data_home: home.clone(),
                home_source: HomeSource::Cli,
                network_mode: NetworkMode::Loopback,
                shutdown_timeout_ms: 8000,
            },
            &layout,
            ServiceDeps {
                bridge_worker_launch: Some(BridgeWorkerLaunch {
                    program: PathBuf::from(String::from_utf8(node.stdout).unwrap().trim()),
                    args: vec![
                        "--import".into(),
                        home.join("platform-loader.mjs")
                            .to_string_lossy()
                            .into_owned(),
                        root.join("scripts/bridge-sdk-worker.ts")
                            .to_string_lossy()
                            .into_owned(),
                    ],
                    cwd: home.clone(),
                }),
                clock: Arc::new(ManualClock::new(1_800_000_000_000)),
                turn_provider: Some(provider.clone()),
                ..ServiceDeps::default()
            },
        )
        .await
        .unwrap();
        let token = state.auth().local_token();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let router = lingxi_service::build_router(state.clone());
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
        });
        let h = Self {
            home,
            state,
            provider,
            url,
            token,
            stop,
            server,
        };
        h.request(
            "POST",
            "/lingxi/v1/bridge/settings",
            json!({"receiptEnabled":receipt,"richStreamingEnabled":rich}),
        )
        .await;
        h.state.bridge().start(h.state.clone());
        h
    }
    async fn request(&self, method: &str, path: &str, body: Value) -> Value {
        let response = reqwest::Client::new()
            .request(method.parse().unwrap(), format!("{}{path}", self.url))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let value: Value = response.json().await.unwrap();
        assert!(status.is_success(), "{status}: {value}");
        value
    }
    async fn enable(&self) {
        self.request("POST","/lingxi/v1/bridge/config?agentId=lingxi",json!({"platform":"telegram","enabled":false,"credentials":{"token":"synthetic-token"}})).await;
        self.request(
            "POST",
            "/lingxi/v1/bridge/owner?agentId=lingxi",
            json!({"platform":"telegram","userId":"sender-1"}),
        )
        .await;
        self.request(
            "POST",
            "/lingxi/v1/bridge/config?agentId=lingxi",
            json!({"platform":"telegram","enabled":true}),
        )
        .await;
    }
    async fn scalar(&self, sql: &str) -> String {
        self.state
            .storage()
            .query_one_text(sql, vec![])
            .await
            .unwrap()
            .unwrap()
    }
    async fn wait(&self, sql: &str, want: &str) {
        tokio::time::timeout(Duration::from_secs(18), async {
            while self.scalar(sql).await != want {
                tokio::time::sleep(Duration::from_millis(30)).await;
            }
        })
        .await
        .unwrap_or_else(|_| {
            panic!(
                "真实SDK检查未达到 {want}: {sql}; ops={:?}",
                self.operations()
            )
        });
    }
    fn operations(&self) -> Vec<Value> {
        std::fs::read_to_string(self.home.join("platform-operations.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|r| serde_json::from_str(r).unwrap())
            .collect()
    }
    async fn verify_real_session_response_in_actual_frontend_parser(&self) {
        let response = self
            .request(
                "GET",
                "/lingxi/v1/bridge/sessions?agentId=lingxi&platform=telegram",
                json!({}),
            )
            .await;
        let rows = response["sessions"].as_array().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["chatId"], "chat-1");
        assert_eq!(rows[0]["displayName"], "合成发送者");
        assert_eq!(rows[0]["isOwner"], true);
        assert!(rows[0]["lastActive"].as_i64().unwrap() > 0);
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize()
            .unwrap();
        let mut node=tokio::process::Command::new("node").args(["--input-type=module","--eval",r#"
import {build} from 'esbuild';
let input='';for await(const chunk of process.stdin)input+=chunk;
const built=await build({entryPoints:['desktop/src/react/services/bridge-response.ts'],bundle:true,platform:'node',format:'esm',write:false,logLevel:'silent'});
const {bridgeSessions}=await import('data:text/javascript;base64,'+Buffer.from(built.outputFiles[0].text).toString('base64'));
const sessions=bridgeSessions(JSON.parse(input));
if(sessions.length!==1||sessions[0].chatId!=='chat-1')throw new Error('真实会话响应未被页面接受');
process.stdout.write(JSON.stringify({accepted:sessions.length}));
"#]).current_dir(root).kill_on_drop(true).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
        use tokio::io::AsyncWriteExt;
        node.stdin
            .take()
            .unwrap()
            .write_all(response.to_string().as_bytes())
            .await
            .unwrap();
        let result = tokio::time::timeout(Duration::from_secs(10), node.wait_with_output())
            .await
            .unwrap()
            .unwrap();
        assert!(
            result.status.success(),
            "页面真实解析失败: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&result.stdout).unwrap()["accepted"],
            1
        );
    }
    async fn close(self) -> PathBuf {
        self.state.bridge().stop();
        tokio::time::timeout(Duration::from_secs(12), self.state.bridge().wait_stopped())
            .await
            .unwrap()
            .unwrap();
        let _ = self.stop.send(());
        self.server.await.unwrap();
        self.state.storage().close().await.unwrap();
        self.home
    }
}
#[tokio::test]
async fn actual_sdk_native_receipt_rich_draft_and_final_have_separate_durable_proofs() {
    let h = Harness::boot(None, true, true, false).await;
    h.enable().await;
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_outbox_v1 WHERE status='confirmed'",
        "1",
    )
    .await;
    assert_eq!(h.provider.calls.load(Ordering::SeqCst), 1);
    h.verify_real_session_response_in_actual_frontend_parser()
        .await;
    assert_eq!(h.scalar("SELECT CAST(COUNT(*) AS TEXT) FROM bridge_effects_v1 WHERE operation='stream_finish' AND status='confirmed'").await,"1");
    let ops = h.operations();
    assert!(ops.iter().any(|r| r["kind"] == "typing"));
    assert!(ops.iter().any(|r| r["kind"] == "sendRichMessageDraft"));
    assert_eq!(
        ops.iter()
            .filter(|r| r["kind"] == "sendRichMessage")
            .count(),
        1
    );
    assert!(!ops.iter().any(|r| r["kind"] == "sendMessage"));
    let raw = serde_json::to_string(&ops).unwrap();
    assert!(!raw.contains("PRIVATE_REASONING"));
    assert!(!raw.contains("<t>"));
    let home = h.close().await;
    let restarted = Harness::boot(Some(home), true, true, false).await;
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(restarted.provider.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        restarted
            .operations()
            .iter()
            .filter(|r| r["kind"] == "sendRichMessage")
            .count(),
        1
    );
    restarted.close().await;
}
#[tokio::test]
async fn actual_settings_disable_waiting_receipt_and_choose_plain_native_draft() {
    let h = Harness::boot(None, false, false, false).await;
    h.enable().await;
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_outbox_v1 WHERE status='confirmed'",
        "1",
    )
    .await;
    let ops = h.operations();
    assert!(!ops.iter().any(|r| r["kind"] == "typing"));
    assert!(!ops.iter().any(|r| r["kind"] == "sendRichMessageDraft"));
    assert!(ops.iter().any(|r| r["kind"] == "sendMessageDraft"));
    assert_eq!(ops.iter().filter(|r| r["kind"] == "sendMessage").count(), 1);
    h.close().await;
}
#[tokio::test]
async fn disabling_connector_cancels_actual_run_and_cleans_the_original_native_draft() {
    let h = Harness::boot(None, true, true, true).await;
    h.enable().await;
    tokio::time::timeout(Duration::from_secs(15), async {
        while !h
            .operations()
            .iter()
            .any(|r| r["kind"] == "sendRichMessageDraft")
        {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    h.request(
        "POST",
        "/lingxi/v1/bridge/stop?agentId=lingxi",
        json!({"platform":"telegram"}),
    )
    .await;
    let status = h
        .request(
            "GET",
            "/lingxi/v1/bridge/status?agentId=lingxi",
            Value::Null,
        )
        .await;
    assert_eq!(status["telegram"]["enabled"], false);
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM runs WHERE status='cancelled'",
        "1",
    )
    .await;
    h.wait("SELECT CAST(COUNT(*) AS TEXT) FROM bridge_effects_v1 WHERE operation='stream_cancel' AND status IN ('confirmed','skipped')","1").await;
    assert!(!h
        .operations()
        .iter()
        .any(|r| r["kind"] == "sendRichMessage" || r["kind"] == "sendMessage"));
    h.close().await;
}

#[tokio::test]
async fn actual_platform_document_bytes_are_extracted_into_the_real_model_submission() {
    let h = Harness::boot(None, false, false, false).await;
    std::fs::write(h.home.join("incoming-document"), "").unwrap();
    h.enable().await;
    h.wait(
        "SELECT CAST(COUNT(*) AS TEXT) FROM bridge_outbox_v1 WHERE status='confirmed'",
        "1",
    )
    .await;
    assert_eq!(h.provider.calls.load(Ordering::SeqCst), 1);
    {
        let inputs = h.provider.inputs.lock().unwrap();
        assert!(
            inputs[0].contains("BRIDGE_ACTUAL_FILE_BODY"),
            "真实文件正文必须进入同一模型输入: {}",
            inputs[0]
        );
        assert!(inputs[0].contains("<attachment-content>"));
    }
    assert_eq!(
        h.scalar("SELECT CAST(COUNT(*) AS TEXT) FROM session_files WHERE status='available'")
            .await,
        "1"
    );
    h.close().await;
}
