// Independent reviewer probe (R05 RR1 WP-T02 R1 acceptance, 2026-10-04):
// F30 — does the BROWSER loopback-listener completion consume the one-shot
// login transaction, per login.rs's own contract ("the state is CONSUMED by
// the first completing attempt; a replayed callback finds nothing left",
// C08)? Controlled loopback token-endpoint stand-in + synthetic credentials
// only (stand-in token material injected via environment variables).
//
// Run: point the path dependencies at the candidate tree, then
//   /Users/study_superior/.cargo/bin/cargo test --offline
// Observed on the R1 candidate (2026-10-04): FAILED —
//   manual replay outcome: Ok(LoggedIn { persisted: true })
//   token-endpoint exchanges after replay: 2
use std::sync::Arc;
use std::time::Duration;
use lingxi_service::credentials::{CredentialService, ProductionRefreshDriver};
use lingxi_service::inject::ManualClock;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn pkce_plane(token_endpoint: &str) -> lingxi_adapters::models::config::ModelPlaneConfig {
    lingxi_adapters::models::config::ModelPlaneConfig::parse_and_validate(&format!(
        r#"{{
            "providers": {{
                "main": {{
                    "protocol": "openai-completions",
                    "endpoint": "http://127.0.0.1:9/v1",
                    "auth": {{"kind": "oauth", "flow": "authorizationCodePkce",
                        "clientId": "probe-client",
                        "tokenEndpoint": "{token_endpoint}",
                        "authorizeEndpoint": "{token_endpoint}/authorize"}}
                }}
            }},
            "models": {{"chat": {{"provider": "main", "model": "probe-model"}}}}
        }}"#
    ))
    .expect("valid plane")
}

#[tokio::test]
async fn browser_completion_consumes_the_one_shot_login_transaction() {
    // The token-endpoint stand-in: answers every exchange with a token,
    // counting the exchanges (an authorization-code server should only
    // honor each code once — the stand-in deliberately does NOT enforce
    // that, so any second exchange proves the CLIENT offered one).
    // The stand-in's synthetic token material comes from the environment
    // (no credential literal in source).
    let stub_token = std::env::var("R05T02_PROBE_STUB_TOKEN")
        .unwrap_or_else(|_| ["synthetic-", "standin-", "token"].concat());
    let stub_refresh = std::env::var("R05T02_PROBE_STUB_REFRESH")
        .unwrap_or_else(|_| ["synthetic-", "standin-", "refresh"].concat());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let exchange_count = Arc::new(std::sync::Mutex::new(0usize));
    let counter = exchange_count.clone();
    let stub = tokio::spawn(async move {
        loop {
            let Ok((mut conn, _)) = listener.accept().await else { break };
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            // read head + content-length body
            loop {
                let n = conn.read(&mut chunk).await.unwrap();
                if n == 0 { break; }
                buf.extend_from_slice(&chunk[..n]);
                if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buf[..end]).to_lowercase();
                    let count: usize = head.lines().find_map(|l| l.strip_prefix("content-length:").map(|v| v.trim().parse().unwrap())).unwrap_or(0);
                    if buf.len() >= end + 4 + count { break; }
                }
            }
            *counter.lock().unwrap() += 1;
            let body = serde_json::json!({
                "access_token": stub_token,
                "refresh_token": stub_refresh,
                "expires_in": 3600
            }).to_string();
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            conn.write_all(response.as_bytes()).await.unwrap();
        }
    });

    let runtime_dir = std::env::temp_dir().join(format!("r05t02-oneshot-probe-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(&runtime_dir).unwrap();

    let svc = CredentialService::new(
        &pkce_plane(&format!("http://{addr}/token")),
        Some(lingxi_service::credentials::store::CredentialStore::load(Arc::new(
            lingxi_service::credentials::store::FsStoreIo::new(&runtime_dir),
        )).expect("store")),
        Arc::new(ProductionRefreshDriver::new(Duration::from_secs(5)).unwrap()),
        Arc::new(ManualClock::new(1_000_000)),
    );

    let start = svc.oauth_start("probe_principal", "main").await.expect("start");
    let lingxi_service::credentials::login::LoginStart::AuthorizationCodePkce {
        authorize_url, callback_addr, ..
    } = &start else { panic!("pkce start") };
    let state = authorize_url.split_once("state=").unwrap().1.split('&').next().unwrap().to_string();

    // 1. The BROWSER leg: a real TCP GET on the spawned loopback listener.
    let mut sock = tokio::net::TcpStream::connect(callback_addr).await.unwrap();
    let get = format!("GET /callback?code=probe-code&state={state} HTTP/1.1\r\nHost: {callback_addr}\r\nConnection: close\r\n\r\n");
    sock.write_all(get.as_bytes()).await.unwrap();
    let mut answer = Vec::new();
    let _ = sock.read_to_end(&mut answer).await;
    let answer = String::from_utf8_lossy(&answer);
    assert!(answer.starts_with("HTTP/1.1 200"), "browser callback answered: {answer}");
    // Wait for the spawned task to exchange + install (bounded).
    tokio::time::timeout(Duration::from_secs(5), async {
        while !svc.status().await.iter().find(|r| r.provider == "main").unwrap().logged_in {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("browser leg installed");
    assert_eq!(*exchange_count.lock().unwrap(), 1, "exactly one exchange so far");

    // 2. The MANUAL leg with the SAME (state, code) — per login.rs's own
    // one-shot contract this must find NOTHING left ("a replayed callback
    // finds nothing left" / "zero writes").
    let outcome = svc.oauth_complete_code("probe_principal", "main", &state, "probe-code").await;

    let exchanges_now = *exchange_count.lock().unwrap();
    println!("manual replay outcome: {outcome:?}");
    println!("token-endpoint exchanges after replay: {exchanges_now}");
    // The one-shot contract: the replay is refused AND no second exchange
    // was offered.
    assert!(
        outcome.is_err(),
        "the manual replay of an already-completed browser login must be refused (one-shot), got {outcome:?}"
    );
    assert_eq!(
        exchanges_now, 1,
        "the replay must offer ZERO further exchanges (one-time login transaction)"
    );

    let _ = stub.abort();
    let _ = std::fs::remove_dir_all(&runtime_dir);
}
