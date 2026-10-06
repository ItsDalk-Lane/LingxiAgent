//! R05 RR1 F14/F15/F16 permanent regressions (WP-T05): the unified network
//! policy, the resolved-egress boundary, and the absolute-deadline
//! discipline — every leg drives the REAL production surfaces (the five
//! chat families through [`GatewayedProvider`], the OAuth transport, the
//! operation dispatcher, the egress guard, the run driver) against
//! counting proxies, TLS stubs signed by a generated test CA and loopback
//! sentinels. No real provider, no real credential, no LIVE deferral:
//! every assertion is offline-deterministic.
//!
//! Counterexample provenance: the four old-red legs of the audit
//! (`artifacts/rust-tauri/R05/RR1/WP-T05-E01/old-red-probe`) are migrated
//! here 1:1 in behavior (never weakened), adapted only to the fixed
//! surfaces per the RR1 migration rule.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use lingxi_adapters::models::config::ModelPlaneConfig;
use lingxi_adapters::models::credentials::{
    ApplicableAuth, CredentialError, ProviderCredentialPort, RefreshVerdict,
};
use lingxi_adapters::models::dispatch::{self, HttpTimeouts};
use lingxi_adapters::models::egress::EgressGuard;
use lingxi_adapters::models::gateway::ConfigModelGateway;
use lingxi_adapters::models::network::{NetworkPlane, NetworkPolicy, ProxyPolicy};
use lingxi_adapters::models::provider::GatewayedProvider;
use lingxi_kernel::model_exchange::{ModelTurnInput, ToolDeclarationSnapshot};
use lingxi_kernel::ports::{
    ModelTurnDelta, ProviderTurn, ProviderTurnResult, TurnDeltaSink, TurnDeltaSinkClosed,
    TurnProviderPort,
};
use lingxi_service::credentials::{CredentialService, ProductionRefreshDriver};
use lingxi_service::inject::ManualClock;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// ── shared helpers ─────────────────────────────────────────────────────────

struct Sink;
impl TurnDeltaSink for Sink {
    fn emit<'a>(
        &'a self,
        _: ModelTurnDelta,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), TurnDeltaSinkClosed>> + Send + 'a>>
    {
        Box::pin(async { Ok(()) })
    }
}

fn ctx() -> lingxi_kernel::RunContext {
    lingxi_kernel::RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new("rr1_t05_session"),
        run_id: lingxi_protocol::RunId::new("rr1_t05_run"),
        attempt: lingxi_protocol::AttemptId::new("rr1_t05_attempt"),
        generation: 1,
    }
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// Reads one full HTTP request (head + declared body) off a readable,
/// writable, unpin connection (raw TCP or TLS).
async fn read_request<Stream>(conn: &mut Stream) -> String
where
    Stream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        let n = conn.read(&mut tmp).await.unwrap();
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(end) = buf.windows(4).position(|x| x == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..end]).to_lowercase();
            let count: usize = head
                .lines()
                .find_map(|x| {
                    x.strip_prefix("content-length:")
                        .map(|v| v.trim().parse().unwrap())
                })
                .unwrap_or(0);
            if buf.len() >= end + 4 + count {
                break;
            }
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

fn sse_openai_body(text: &str) -> String {
    format!(
        "data: {{\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"{text}\"}}}}]}}\n\ndata: \
         {{\"choices\":[{{\"index\":0,\"delta\":{{}},\"finish_reason\":\"stop\"}}]}}\n\ndata: \
         [DONE]\n\n"
    )
}

/// A counting FORWARD proxy: plain-http proxying answers the absolute-form
/// request itself (proving the connection target changed); `CONNECT`
/// tunnels bytes to the real target (end-to-end TLS still verified by the
/// CLIENT through the tunnel). Both shapes count.
struct CountingProxy {
    hits: Arc<AtomicUsize>,
    #[allow(dead_code)] // kept for symmetry with the stub tasks; the loop
    // lives as long as the test runtime
    task: tokio::task::JoinHandle<()>,
    addr: SocketAddr,
}

impl CountingProxy {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let hits = Arc::new(AtomicUsize::new(0));
        let state = hits.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut conn, _)) = listener.accept().await else {
                    break;
                };
                let hits = state.clone();
                tokio::spawn(async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    let request = read_request(&mut conn).await;
                    if request.starts_with("CONNECT ") {
                        // `CONNECT host:port HTTP/1.1` → dumb byte tunnel.
                        let target = request.split_whitespace().nth(1).unwrap().to_string();
                        let Ok(mut upstream) = tokio::net::TcpStream::connect(&target).await else {
                            let _ = conn.write_all(b"HTTP/1.1 502 Bad Gateway\r\n\r\n").await;
                            return;
                        };
                        if conn
                            .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                            .await
                            .is_err()
                        {
                            return;
                        }
                        let (mut cr, mut cw) = conn.split();
                        let (mut ur, mut uw) = upstream.split();
                        let _ = tokio::io::copy(&mut cr, &mut uw).await;
                        let _ = tokio::io::copy(&mut ur, &mut cw).await;
                    } else {
                        // Absolute-form http request: the proxy IS the
                        // destination the client dialed — answer it.
                        let body = sse_openai_body("proxied");
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
                             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = conn.write_all(response.as_bytes()).await;
                    }
                });
            }
        });
        Self { hits, task, addr }
    }

    fn count(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }
}

// ── TLS stubs (rcgen CA / server certs; test-only material) ────────────────

/// One TLS server identity: SANs (DNS names and/or IP addresses), signed
/// by the given CA, optionally inside a custom validity window.
struct TestCert {
    cert_pem: String,
    server_config: tokio_rustls::TlsAcceptor,
}

fn san(dns_or_ip: &str) -> rcgen::SanType {
    if let Ok(ip) = dns_or_ip.parse::<std::net::IpAddr>() {
        rcgen::SanType::IpAddress(ip)
    } else {
        rcgen::SanType::DnsName(dns_or_ip.try_into().unwrap())
    }
}

/// The test CA: its params (for `Issuer::from_params`), its key and its
/// certificate (the PEM the policy trusts).
struct TestCa {
    params: rcgen::CertificateParams,
    key: rcgen::KeyPair,
    certificate: rcgen::Certificate,
}

fn ca_certificate() -> TestCa {
    let mut params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
    params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    let key = rcgen::KeyPair::generate().unwrap();
    let certificate = params.self_signed(&key).unwrap();
    TestCa {
        params,
        key,
        certificate,
    }
}

fn server_cert_by(
    ca: &TestCa,
    sans: &[&str],
    validity: Option<(std::time::SystemTime, std::time::SystemTime)>,
) -> TestCert {
    let mut params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
    params.subject_alt_names = sans.iter().map(|s| san(s)).collect();
    if let Some((before, after)) = validity {
        params.not_before = before.into();
        params.not_after = after.into();
    }
    let key = rcgen::KeyPair::generate().unwrap();
    let issuer = rcgen::Issuer::from_params(&ca.params, &ca.key);
    let cert = params.signed_by(&key, &issuer).unwrap();
    let cert_pem = cert.pem();
    let server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.der().clone()],
            rustls::pki_types::pem::PemObject::from_pem_slice(key.serialize_pem().as_bytes())
                .unwrap(),
        )
        .unwrap();
    TestCert {
        cert_pem,
        server_config: tokio_rustls::TlsAcceptor::from(Arc::new(server_config)),
    }
}

/// One TLS stub answering every request with the SSE chat completion.
async fn tls_sse_stub(acceptor: tokio_rustls::TlsAcceptor) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((conn, _)) = listener.accept().await else {
                break;
            };
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let mut tls = match acceptor.accept(conn).await {
                    Ok(tls) => tls,
                    Err(_) => return,
                };
                let _request = read_request(&mut tls).await;
                let body = sse_openai_body("secure");
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: \
                     {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = tls.write_all(response.as_bytes()).await;
            });
        }
    });
    addr
}

/// One plain stub answering every request with the SSE chat completion.
async fn http_sse_stub() -> (SocketAddr, Arc<AtomicUsize>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let state = hits.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut conn, _)) = listener.accept().await else {
                break;
            };
            let hits = state.clone();
            tokio::spawn(async move {
                hits.fetch_add(1, Ordering::SeqCst);
                let _request = read_request(&mut conn).await;
                let body = sse_openai_body("direct");
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: \
                     {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = conn.write_all(response.as_bytes()).await;
            });
        }
    });
    (addr, hits)
}

fn credentials_of(cfg: &ModelPlaneConfig) -> Arc<CredentialService> {
    Arc::new(CredentialService::new(
        cfg,
        None,
        Arc::new(ProductionRefreshDriver::new(Duration::from_secs(1)).unwrap()),
        Arc::new(ManualClock::new(1000)),
    ))
}

fn plane_config(endpoint: &str, key: &str) -> ModelPlaneConfig {
    ModelPlaneConfig::from_value(serde_json::json!({
        "providers": {"main": {"protocol": "openai-completions", "endpoint": endpoint, "auth": {"kind": "apiKey", "apiKey": key}}},
        "models": {"chat": {"provider": "main", "model": "rr1-t05-model"}}
    }))
    .unwrap()
}

fn manual_policy(proxy_addr: &SocketAddr, no_proxy: &str) -> NetworkPolicy {
    NetworkPolicy {
        proxy: ProxyPolicy::Manual {
            http: Some(format!("http://{proxy_addr}")),
            https: Some(format!("http://{proxy_addr}")),
            no_proxy: no_proxy.to_string(),
        },
        trusted_ca_pem: None,
    }
}

async fn drive_chat(provider: &GatewayedProvider, deadline_unix_ms: u64) -> ProviderTurnResult {
    let context = ctx();
    let call = lingxi_protocol::ModelCallId::new("rr1_t05_call");
    let input = ModelTurnInput::first_turn("hello", ToolDeclarationSnapshot::empty());
    let mut input = input;
    input.deadline_unix_ms = Some(deadline_unix_ms);
    tokio::time::timeout(
        Duration::from_secs(30),
        provider.next_turn(&context, &call, &input, &Sink),
    )
    .await
    .expect("the chat turn settles within the observation window")
}

// ── F14: the unified network policy ────────────────────────────────────────

mod rr1_f14 {
    use super::*;

    /// The config carrier parses and validates (the old-red probe's first
    /// leg: `deny_unknown_fields` refused the section outright).
    #[test]
    fn network_policy_section_parses_and_validates() {
        let plane = ModelPlaneConfig::from_value(serde_json::json!({
            "providers": {"main": {"protocol": "openai-completions", "endpoint": "https://api.example.invalid/v1", "auth": {"kind": "apiKey", "apiKey": "dummy-key"}}},
            "models": {"chat": {"provider": "main", "model": "m"}},
            "network": {
                "proxy": {
                    "mode": "manual",
                    "httpProxy": "http://proxy.internal.invalid:8080",
                    "httpsProxy": "http://proxy.internal.invalid:8080",
                    "noProxy": "localhost, 127.0.0.1, ::1"
                }
            }
        }));
        assert!(plane.is_ok(), "{plane:?}");
        let bad = ModelPlaneConfig::from_value(serde_json::json!({
            "providers": {"main": {"protocol": "openai-completions", "endpoint": "https://api.example.invalid/v1", "auth": {"kind": "apiKey", "apiKey": "dummy-key"}}},
            "models": {"chat": {"provider": "main", "model": "m"}},
            "network": {"proxy": {"mode": "manual", "httpProxy": "http://user:pass@proxy.invalid:8080"}}
        }));
        assert!(bad.is_err(), "proxy userinfo is a loud config error");
    }

    /// A manual proxy GENUINELY changes the connection target: the real
    /// chat turn leaves through the counting proxy (absolute-form) and the
    /// synthetic source host never needs to resolve client-side.
    #[tokio::test]
    async fn manual_proxy_routes_a_real_chat_turn_through_the_proxy() {
        let proxy = CountingProxy::start().await;
        let cfg = plane_config("http://model-source.rr1.test:9443/v1", "dummy-key");
        let gateway = Arc::new(ConfigModelGateway::from_validated(cfg.clone()));
        let plane = NetworkPlane::isolated(manual_policy(&proxy.addr, ""));
        let provider = GatewayedProvider::new_with_network(
            gateway,
            credentials_of(&cfg),
            lingxi_kernel::toolcatalog::SchemaBudget::default(),
            plane,
        )
        .unwrap();
        let result = drive_chat(&provider, unix_ms() + 30_000).await;
        assert!(
            matches!(result.turn, ProviderTurn::Final { .. }),
            "the proxied chat turn completes: {:?}",
            result.turn
        );
        assert_eq!(proxy.count(), 1, "the request dialed the proxy");
    }

    /// FORCED loopback bypass: a proxy configured with an EMPTY no-proxy
    /// list still never carries traffic to a 127.0.0.1 endpoint — the
    /// source receives the request directly (the incumbent's
    /// FORCED_LOCAL_PROXY_BYPASS).
    #[tokio::test]
    async fn forced_loopback_bypass_keeps_local_endpoints_direct() {
        let proxy = CountingProxy::start().await;
        let (addr, hits) = http_sse_stub().await;
        let cfg = plane_config(&format!("http://{addr}/v1"), "dummy-key");
        let gateway = Arc::new(ConfigModelGateway::from_validated(cfg.clone()));
        let plane = NetworkPlane::isolated(manual_policy(&proxy.addr, ""));
        let provider = GatewayedProvider::new_with_network(
            gateway,
            credentials_of(&cfg),
            lingxi_kernel::toolcatalog::SchemaBudget::default(),
            plane,
        )
        .unwrap();
        let result = drive_chat(&provider, unix_ms() + 30_000).await;
        assert!(matches!(result.turn, ProviderTurn::Final { .. }));
        assert_eq!(proxy.count(), 0, "loopback never goes through the proxy");
        assert_eq!(hits.load(Ordering::SeqCst), 1, "the source got it direct");
    }

    /// Direct mode never consults anything (the pre-F14 behavior, now an
    /// explicit configuration).
    #[tokio::test]
    async fn direct_policy_dials_the_source_itself() {
        let proxy = CountingProxy::start().await;
        let (addr, hits) = http_sse_stub().await;
        let cfg = plane_config(&format!("http://{addr}/v1"), "dummy-key");
        let gateway = Arc::new(ConfigModelGateway::from_validated(cfg.clone()));
        let plane = NetworkPlane::isolated(NetworkPolicy {
            proxy: ProxyPolicy::Direct,
            trusted_ca_pem: None,
        });
        let provider = GatewayedProvider::new_with_network(
            gateway,
            credentials_of(&cfg),
            lingxi_kernel::toolcatalog::SchemaBudget::default(),
            plane,
        )
        .unwrap();
        let result = drive_chat(&provider, unix_ms() + 30_000).await;
        assert!(matches!(result.turn, ProviderTurn::Final { .. }));
        assert_eq!(proxy.count(), 0);
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    /// System mode resolves from the SNAPSHOT the plane was built with:
    /// the exported HTTPS_PROXY routes the chat turn through the proxy
    /// (the old-red probe's env leg, deterministically injected).
    #[tokio::test]
    async fn system_mode_routes_through_the_snapshotted_environment() {
        let proxy = CountingProxy::start().await;
        let env =
            BTreeMap::from_iter([("HTTPS_PROXY".to_string(), format!("http://{}", proxy.addr))]);
        let cfg = plane_config("https://model-source.rr1.test:9443/v1", "dummy-key");
        let gateway = Arc::new(ConfigModelGateway::from_validated(cfg.clone()));
        let plane = NetworkPlane::isolated(NetworkPolicy {
            proxy: ProxyPolicy::System { env },
            trusted_ca_pem: None,
        });
        let provider = GatewayedProvider::new_with_network(
            gateway,
            credentials_of(&cfg),
            lingxi_kernel::toolcatalog::SchemaBudget::default(),
            plane,
        )
        .unwrap();
        // The https target never resolves client-side through the proxy;
        // the tunnel target is dead → the turn fails, but ONLY after the
        // proxy was dialed (the routing proof).
        let result = drive_chat(&provider, unix_ms() + 30_000).await;
        assert!(matches!(result.turn, ProviderTurn::Failed { .. }));
        assert!(proxy.count() >= 1, "system mode dialed the exported proxy");
    }

    /// The explicit private CA: chat over https to a CA-signed stub
    /// succeeds ONLY when the policy carries the CA — without it the
    /// default verification refuses (no accept-invalid fallback anywhere).
    #[tokio::test]
    async fn private_ca_authorizes_chat_and_default_verification_refuses_it() {
        let ca = ca_certificate();
        let cert = server_cert_by(&ca, &["127.0.0.1", "localhost"], None);
        let cert_pem = cert.cert_pem.clone();
        let addr = tls_sse_stub(cert.server_config).await;
        let endpoint = format!("https://127.0.0.1:{}/v1", addr.port());
        let with_ca = {
            let cfg = plane_config(&endpoint, "dummy-key");
            let gateway = Arc::new(ConfigModelGateway::from_validated(cfg.clone()));
            let plane = NetworkPlane::isolated(NetworkPolicy {
                proxy: ProxyPolicy::Direct,
                trusted_ca_pem: Some(cert_pem.clone()),
            });
            GatewayedProvider::new_with_network(
                gateway,
                credentials_of(&cfg),
                lingxi_kernel::toolcatalog::SchemaBudget::default(),
                plane,
            )
            .unwrap()
        };
        let result = drive_chat(&with_ca, unix_ms() + 30_000).await;
        assert!(
            matches!(result.turn, ProviderTurn::Final { .. }),
            "the explicit CA authorizes the private chain: {:?}",
            result.turn
        );
        // The SAME stub without the CA in the policy: the default platform
        // roots refuse the unknown issuer — loudly, never a bypass.
        let without_ca = {
            let cfg = plane_config(&endpoint, "dummy-key");
            let gateway = Arc::new(ConfigModelGateway::from_validated(cfg.clone()));
            let plane = NetworkPlane::isolated(NetworkPolicy {
                proxy: ProxyPolicy::Direct,
                trusted_ca_pem: None,
            });
            GatewayedProvider::new_with_network(
                gateway,
                credentials_of(&cfg),
                lingxi_kernel::toolcatalog::SchemaBudget::default(),
                plane,
            )
            .unwrap()
        };
        let result = drive_chat(&without_ca, unix_ms() + 30_000).await;
        assert!(
            matches!(result.turn, ProviderTurn::Failed { .. }),
            "an unlisted private chain is refused by default verification"
        );
    }

    /// TLS negatives through the SAME CA-authorized policy: a wrong-host
    /// certificate, an expired certificate and a wrong issuer are all
    /// refused — the explicit CA addition never degrades verification.
    #[tokio::test]
    async fn wrong_hostname_expired_and_wrong_chain_are_refused() {
        let ca = ca_certificate();
        let other_ca = ca_certificate();
        let ca_pem = ca.certificate.pem();
        // Wrong hostname: cert valid only for a DIFFERENT IP.
        let wrong_host = server_cert_by(&ca, &["127.0.0.2"], None);
        let addr = tls_sse_stub(wrong_host.server_config).await;
        let endpoint = format!("https://127.0.0.1:{}/v1", addr.port());
        let cfg = plane_config(&endpoint, "dummy-key");
        let gateway = Arc::new(ConfigModelGateway::from_validated(cfg.clone()));
        let plane = NetworkPlane::isolated(NetworkPolicy {
            proxy: ProxyPolicy::Direct,
            trusted_ca_pem: Some(ca_pem.clone()),
        });
        let provider = GatewayedProvider::new_with_network(
            gateway,
            credentials_of(&cfg),
            lingxi_kernel::toolcatalog::SchemaBudget::default(),
            plane,
        )
        .unwrap();
        let result = drive_chat(&provider, unix_ms() + 30_000).await;
        assert!(matches!(result.turn, ProviderTurn::Failed { .. }));

        // Expired: a CA-valid cert whose validity window is entirely in
        // the past.
        let past = std::time::SystemTime::now() - Duration::from_secs(86_400 * 400);
        let earlier = past - Duration::from_secs(86_400 * 30);
        let expired = server_cert_by(&ca, &["127.0.0.1", "localhost"], Some((earlier, past)));
        let addr = tls_sse_stub(expired.server_config).await;
        let endpoint = format!("https://127.0.0.1:{}/v1", addr.port());
        let cfg = plane_config(&endpoint, "dummy-key");
        let gateway = Arc::new(ConfigModelGateway::from_validated(cfg.clone()));
        let plane = NetworkPlane::isolated(NetworkPolicy {
            proxy: ProxyPolicy::Direct,
            trusted_ca_pem: Some(ca_pem.clone()),
        });
        let provider = GatewayedProvider::new_with_network(
            gateway,
            credentials_of(&cfg),
            lingxi_kernel::toolcatalog::SchemaBudget::default(),
            plane,
        )
        .unwrap();
        let result = drive_chat(&provider, unix_ms() + 30_000).await;
        assert!(matches!(result.turn, ProviderTurn::Failed { .. }));

        // Wrong chain: a well-formed cert from a DIFFERENT CA.
        let wrong_chain = server_cert_by(&other_ca, &["127.0.0.1"], None);
        let addr = tls_sse_stub(wrong_chain.server_config).await;
        let endpoint = format!("https://127.0.0.1:{}/v1", addr.port());
        let cfg = plane_config(&endpoint, "dummy-key");
        let gateway = Arc::new(ConfigModelGateway::from_validated(cfg.clone()));
        let plane = NetworkPlane::isolated(NetworkPolicy {
            proxy: ProxyPolicy::Direct,
            trusted_ca_pem: Some(ca_pem.clone()),
        });
        let provider = GatewayedProvider::new_with_network(
            gateway,
            credentials_of(&cfg),
            lingxi_kernel::toolcatalog::SchemaBudget::default(),
            plane,
        )
        .unwrap();
        let result = drive_chat(&provider, unix_ms() + 30_000).await;
        assert!(matches!(result.turn, ProviderTurn::Failed { .. }));
    }

    /// OAuth refresh, an operation-plane request and an egress download
    /// all consume the SAME manual-proxy policy (one plane, one proxy).
    #[tokio::test]
    async fn oauth_operation_and_download_share_the_proxy_policy() {
        let proxy = CountingProxy::start().await;
        let plane = NetworkPlane::isolated(manual_policy(&proxy.addr, ""));

        // OAuth refresh: the token endpoint answers a grant over the proxy.
        let http =
            lingxi_adapters::models::oauth::OAuthHttp::new_with_network(plane.clone()).unwrap();
        let flow = lingxi_adapters::models::config::OAuthFlowConfig {
            flow: lingxi_adapters::models::config::OAuthFlowKind::DeviceCode,
            client_id: "rr1-client".into(),
            token_endpoint: format!("http://oauth.rr1.test:{}/token", proxy.addr.port()),
            authorize_endpoint: None,
            device_authorization_endpoint: None,
            scopes: None,
        };
        let clock = lingxi_adapters::models::oauth::system_flow_clock(Duration::from_secs(5));
        // The counting proxy answers the grant request with SSE text — a
        // protocol-level failure is fine; the ROUTING fact is the count.
        let _ =
            lingxi_adapters::models::oauth::refresh(&http, &flow, "dummy-refresh-token", &clock)
                .await;
        assert!(
            proxy.count() >= 1,
            "the OAuth token request dialed the shared proxy"
        );

        // Operation plane: one JSON GET through the same policy.
        let dispatcher =
            lingxi_adapters::models::operations::OperationDispatcher::with_timeouts_and_network(
                HttpTimeouts::default(),
                plane.clone(),
            )
            .unwrap();
        let plan = lingxi_adapters::models::operations::OperationRequestPlan::get(format!(
            "http://ops.rr1.test:{}/v1/embeddings",
            proxy.addr.port()
        ));
        let _ = dispatcher
            .execute_json(
                &plan,
                Some(unix_ms() + 30_000),
                &ApplicableAuth::Bearer("dummy-key".into()),
            )
            .await;
        assert!(
            proxy.count() >= 2,
            "the operation request dialed the shared proxy"
        );
    }

    /// A policy generation published on the plane is observed by the NEXT
    /// request of an existing provider (the reload surface's atomic swap):
    /// the direct loopback endpoint serves the first turn; the plane then
    /// publishes a manual proxy and the gateway's route moves to a
    /// NON-loopback host — the next turn leaves through the proxy (which
    /// answers it), and the direct source sees nothing new. (A loopback
    /// route under a proxy would be force-bypassed — that rule has its own
    /// leg above.)
    #[tokio::test]
    async fn publishing_a_policy_generation_reroutes_the_next_request() {
        let proxy = CountingProxy::start().await;
        let (addr, hits) = http_sse_stub().await;
        let cfg = plane_config(&format!("http://{addr}/v1"), "dummy-key");
        let gateway = Arc::new(ConfigModelGateway::from_validated(cfg.clone()));
        let plane = NetworkPlane::isolated(NetworkPolicy {
            proxy: ProxyPolicy::Direct,
            trusted_ca_pem: None,
        });
        let provider = GatewayedProvider::new_with_network(
            gateway.clone(),
            credentials_of(&cfg),
            lingxi_kernel::toolcatalog::SchemaBudget::default(),
            plane.clone(),
        )
        .unwrap();
        let first = drive_chat(&provider, unix_ms() + 30_000).await;
        assert!(matches!(first.turn, ProviderTurn::Final { .. }));
        assert_eq!(proxy.count(), 0);
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        // Publish the manual proxy AND move the route to a non-loopback
        // host (the same pair the management reload performs atomically).
        plane.apply(manual_policy(&proxy.addr, ""));
        let rerouted = plane_config(
            &format!("http://reroute.rr1.test:{}/v1", addr.port()),
            "dummy-key",
        );
        gateway.reload(rerouted);
        let second = drive_chat(&provider, unix_ms() + 30_000).await;
        assert!(
            matches!(second.turn, ProviderTurn::Final { .. }),
            "the proxy answers the rerouted turn: {:?}",
            second.turn
        );
        assert_eq!(proxy.count(), 1, "the next request used the new policy");
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "the direct source saw nothing new"
        );
    }
}

// ── F15: the resolved-egress boundary ──────────────────────────────────────

mod rr1_f15 {
    use super::*;
    use lingxi_adapters::models::egress::EgressResolver;
    use std::net::SocketAddr as Addr;

    fn resolver_of(calls: std::sync::Mutex<Vec<Vec<Addr>>>) -> (EgressResolver, Arc<AtomicUsize>) {
        let count = Arc::new(AtomicUsize::new(0));
        let counter = count.clone();
        let resolver: EgressResolver = Arc::new(move |host: &str, _port: u16| {
            let calls = calls.lock().unwrap();
            let index = counter.fetch_add(1, Ordering::SeqCst);
            let answer = calls.get(index).cloned().unwrap_or_default();
            let host = host.to_string();
            Box::pin(async move {
                if answer.is_empty() {
                    Err(format!("no record for {host}"))
                } else {
                    Ok(answer)
                }
            })
        });
        (resolver, count)
    }

    fn addr_of(ip: &str, port: u16) -> Addr {
        format!("{ip}:{port}").parse().unwrap()
    }

    /// The audit's CN-F06 counterexample, migrated verbatim in behavior:
    /// a `https://localhost:<port>/` download makes ZERO connections to
    /// the loopback sentinel (the REAL OS resolver resolves localhost).
    #[tokio::test]
    async fn localhost_download_makes_zero_connections() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let guard =
            EgressGuard::from_provider_endpoints(&["https://authorized.example.invalid/v1".into()])
                .unwrap();
        let url = format!("https://localhost:{}/private-image.png", addr.port());
        let (hit_tx, hit_rx) = tokio::sync::oneshot::channel::<()>();
        let stub = tokio::spawn(async move {
            let (mut conn, _) = listener.accept().await.unwrap();
            let mut b = [0u8; 1];
            let _ = conn.read(&mut b).await;
            let _ = hit_tx.send(());
        });
        let download =
            tokio::spawn(async move { guard.download(&url, Some(unix_ms() + 2_000)).await });
        let connected = tokio::time::timeout(Duration::from_millis(400), hit_rx)
            .await
            .is_ok();
        let outcome = download.await.unwrap();
        stub.abort();
        assert!(!connected, "an unauthorized loopback connection happened");
        assert!(
            outcome.is_err(),
            "the download is refused loudly, not dialed"
        );
    }

    /// A hostname resolving to a PRIVATE candidate refuses BEFORE any
    /// connection (the audit's "normal domain → restricted address" leg,
    /// with a controlled resolver for the record set).
    #[tokio::test]
    async fn private_resolved_candidate_refuses_with_zero_connections() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (resolver, calls) = resolver_of(std::sync::Mutex::new(vec![vec![addr_of(
            "127.0.0.1",
            addr.port(),
        )]]));
        let guard =
            EgressGuard::from_provider_endpoints(&["https://authorized.example.invalid/v1".into()])
                .unwrap()
                .with_resolver(resolver);
        let url = "https://internal.rr1.test/private-image.png".to_string();
        let (hit_tx, hit_rx) = tokio::sync::oneshot::channel::<()>();
        let stub = tokio::spawn(async move {
            let (mut conn, _) = listener.accept().await.unwrap();
            let mut b = [0u8; 1];
            let _ = conn.read(&mut b).await;
            let _ = hit_tx.send(());
        });
        let outcome = guard.download(&url, Some(unix_ms() + 2_000)).await;
        let connected = tokio::time::timeout(Duration::from_millis(200), hit_rx)
            .await
            .is_ok();
        stub.abort();
        assert!(!connected);
        let (error, _) = outcome.expect_err("guarded candidates refuse");
        assert!(error.message.contains("guarded"), "{error}");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    /// A MIXED public+private record set refuses (the transport could dial
    /// the guarded candidate) — and an all-public set passes the judgment.
    #[tokio::test]
    async fn mixed_candidates_refuse_and_public_candidates_pass_judgment() {
        let (resolver, _) = resolver_of(std::sync::Mutex::new(vec![vec![
            addr_of("93.184.216.34", 443),
            addr_of("10.1.2.3", 443),
        ]]));
        let guard =
            EgressGuard::from_provider_endpoints(&["https://authorized.example.invalid/v1".into()])
                .unwrap()
                .with_resolver(resolver);
        let outcome = guard
            .download("https://mixed.rr1.test/asset.png", Some(unix_ms() + 2_000))
            .await;
        let (error, _) = outcome.expect_err("mixed records refuse");
        assert!(error.message.contains("guarded"), "{error}");
        // The same hostname with an all-public set passes the RESOLUTION
        // judgment (the dial then fails to the unreachable public IP — the
        // refusal reason distinguishes the two).
        let (resolver, _) = resolver_of(std::sync::Mutex::new(vec![vec![addr_of(
            "93.184.216.34",
            443,
        )]]));
        let guard =
            EgressGuard::from_provider_endpoints(&["https://authorized.example.invalid/v1".into()])
                .unwrap()
                .with_resolver(resolver);
        let outcome = guard
            .download("https://public.rr1.test/asset.png", Some(unix_ms() + 1_000))
            .await;
        let (error, _) = outcome.expect_err("the unreachable public IP fails the DIAL");
        assert!(
            !error.message.contains("guarded"),
            "the judgment passed; only the dial failed: {error}"
        );
    }

    /// The verified candidates are PINNED: a resolver that answers the
    /// FIRST (judgment) call with a public address and every LATER call
    /// with the loopback sentinel proves the transport never re-resolves —
    /// the sentinel receives ZERO connections (RFC 5737 documentation
    /// range: public, unroutable in test).
    #[tokio::test]
    async fn the_pinned_dial_never_re_resolves() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (resolver, calls) = resolver_of(std::sync::Mutex::new(vec![
            vec![addr_of("192.0.2.1", 443)],
            vec![addr_of("127.0.0.1", addr.port())],
            vec![addr_of("127.0.0.1", addr.port())],
        ]));
        let guard =
            EgressGuard::from_provider_endpoints(&["https://authorized.example.invalid/v1".into()])
                .unwrap()
                .with_resolver(resolver);
        let (hit_tx, hit_rx) = tokio::sync::oneshot::channel::<()>();
        let stub = tokio::spawn(async move {
            let (mut conn, _) = listener.accept().await.unwrap();
            let mut b = [0u8; 1];
            let _ = conn.read(&mut b).await;
            let _ = hit_tx.send(());
        });
        let outcome = guard
            .download("https://rebind.rr1.test/asset.png", Some(unix_ms() + 800))
            .await;
        let connected = tokio::time::timeout(Duration::from_millis(300), hit_rx)
            .await
            .is_ok();
        stub.abort();
        assert!(!connected, "a re-resolution reached the loopback sentinel");
        let (error, _) = outcome.expect_err("the pinned public IP is unroutable in test");
        assert!(
            !error.message.contains("guarded"),
            "the judgment passed: {error}"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "the resolver answered exactly once — the dial never re-resolved"
        );
    }

    /// The explicit local-origin exception still delivers real bytes (the
    /// minimal legitimate local-model case — never widened to any private
    /// destination).
    #[tokio::test]
    async fn the_same_origin_exception_downloads_real_bytes() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let png = b"\x89PNG\r\n\x1a\nbody".to_vec();
        let payload = png.clone();
        let stub = tokio::spawn(async move {
            let (mut conn, _) = listener.accept().await.unwrap();
            let _request = read_request(&mut conn).await;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\n\
                 Connection: close\r\n\r\n",
                payload.len()
            );
            conn.write_all(response.as_bytes()).await.unwrap();
            conn.write_all(&payload).await.unwrap();
        });
        let origin = format!("http://127.0.0.1:{}", addr.port());
        let guard = EgressGuard::from_provider_endpoints(&[format!("{origin}/v1")]).unwrap();
        let (bytes, mime) = guard
            .download(
                &format!("{origin}/files/product.png"),
                Some(unix_ms() + 2_000),
            )
            .await
            .expect("the configured endpoint origin downloads");
        stub.await.unwrap();
        assert_eq!(bytes, png);
        assert_eq!(mime, "image/png");
        // A DIFFERENT port on the same host stays refused.
        let err = guard
            .download(
                &format!("http://127.0.0.1:{}/x.png", addr.port() + 1),
                Some(unix_ms() + 2_000),
            )
            .await;
        assert!(err.is_err(), "the exception is origin-exact");
    }

    /// Under a PROXY policy the download routes through the proxy (the
    /// proxy becomes the resolution authority — no local pinning), and the
    /// guarded-candidate judgment still refuses a name that resolves into
    /// private space locally (defense in depth; the proxy never sees it).
    #[tokio::test]
    async fn a_proxied_download_routes_through_the_proxy_and_still_refuses_guarded_names() {
        let proxy = CountingProxy::start().await;
        let plane = NetworkPlane::isolated(manual_policy(&proxy.addr, ""));
        // A public record set: the download dials THROUGH the proxy (the
        // CONNECT target is the hostname; the proxy answers 502 to the
        // unresolvable name — the ROUTING fact is the count).
        let (resolver, _) = resolver_of(std::sync::Mutex::new(vec![vec![addr_of(
            "93.184.216.34",
            443,
        )]]));
        let guard = EgressGuard::from_provider_endpoints_and_network(
            &["https://authorized.example.invalid/v1".into()],
            plane,
        )
        .unwrap()
        .with_resolver(resolver);
        let outcome = guard
            .download(
                "https://assets.rr1.test/asset.png",
                Some(unix_ms() + 30_000),
            )
            .await;
        assert!(outcome.is_err(), "the tunnel target is unresolvable");
        assert_eq!(
            proxy.count(),
            1,
            "the proxied download dialed the proxy, not the target"
        );
        // A guarded record set refuses BEFORE any dial, proxy included.
        let plane = NetworkPlane::isolated(manual_policy(&proxy.addr, ""));
        let (resolver, _) =
            resolver_of(std::sync::Mutex::new(vec![vec![addr_of("10.0.0.5", 443)]]));
        let guard = EgressGuard::from_provider_endpoints_and_network(
            &["https://authorized.example.invalid/v1".into()],
            plane,
        )
        .unwrap()
        .with_resolver(resolver);
        let outcome = guard
            .download(
                "https://internal.rr1.test/asset.png",
                Some(unix_ms() + 30_000),
            )
            .await;
        let (error, _) = outcome.expect_err("guarded candidates refuse");
        assert!(error.message.contains("guarded"), "{error}");
        assert_eq!(
            proxy.count(),
            1,
            "no second dial: the guarded name never reached the proxy either"
        );
    }
}

// ── F16: the absolute deadline over every stage ────────────────────────────

mod rr1_f16 {
    use super::*;

    /// The audit's CN-F07 counterexample, migrated verbatim: a 401 head +
    /// ONE body byte + stall, deadline 50 ms — the send path RETURNS.
    #[tokio::test]
    async fn error_body_must_obey_total_deadline() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let stub = tokio::spawn(async move {
            let (mut conn, _) = listener.accept().await.unwrap();
            let mut buf = [0; 4096];
            let _ = conn.read(&mut buf).await.unwrap();
            conn.write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 1000\r\n\r\nx")
                .await
                .unwrap();
            std::future::pending::<()>().await;
        });
        let client = dispatch::build_client().unwrap();
        let request = client
            .post(format!("http://{addr}/v1/chat/completions"))
            .body("{}");
        let deadline = dispatch::unix_ms_now() + 50;
        let result = tokio::time::timeout(
            Duration::from_millis(250),
            dispatch::send_with_timeouts(
                request,
                &HttpTimeouts::default(),
                Some(deadline),
                &ApplicableAuth::Bearer("dummy-key".into()),
            ),
        )
        .await;
        stub.abort();
        assert!(result.is_ok(), "the error-body read outlived its deadline");
        let (error, retryable) = result.unwrap().unwrap_err();
        assert_eq!(error.code, lingxi_protocol::ErrorCode::Unauthorized);
        assert!(!retryable);
        assert!(
            error.message.contains("deadline"),
            "the abandonment fact is preserved: {error}"
        );
    }

    /// An oversized stalled error body is CUT at the cap with the
    /// truncation fact preserved (never an unbounded buffer).
    #[tokio::test]
    async fn oversized_error_body_is_capped_with_the_fact_preserved() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let stub = tokio::spawn(async move {
            let (mut conn, _) = listener.accept().await.unwrap();
            let mut buf = [0; 4096];
            let _ = conn.read(&mut buf).await.unwrap();
            conn.write_all(b"HTTP/1.1 500 Internal Server Error\r\n\r\n")
                .await
                .unwrap();
            // Stream 128 KiB more than the 64 KiB cap, then stall.
            let chunk = vec![b'a'; 8192];
            for _ in 0..24 {
                conn.write_all(&chunk).await.unwrap();
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            std::future::pending::<()>().await;
        });
        let client = dispatch::build_client().unwrap();
        let request = client
            .post(format!("http://{addr}/v1/chat/completions"))
            .body("{}");
        let deadline = dispatch::unix_ms_now() + 3_000;
        let result = tokio::time::timeout(
            Duration::from_secs(8),
            dispatch::send_with_timeouts(
                request,
                &HttpTimeouts::default(),
                Some(deadline),
                &ApplicableAuth::Bearer("dummy-key".into()),
            ),
        )
        .await;
        stub.abort();
        let (error, retryable) = result
            .expect("capped within the observation window")
            .unwrap_err();
        assert_eq!(error.code, lingxi_protocol::ErrorCode::UpstreamUnavailable);
        assert!(retryable, "a 5xx stays retryable");
        assert!(
            error.message.contains("truncated"),
            "the truncation fact is preserved: {error}"
        );
    }

    /// A mid-read transport failure is PRESERVED in the classified error
    /// (the pre-fix shape swallowed every read failure into an empty
    /// excerpt).
    #[tokio::test]
    async fn error_body_read_failure_is_preserved_not_swallowed() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let stub = tokio::spawn(async move {
            let (mut conn, _) = listener.accept().await.unwrap();
            let mut buf = [0; 4096];
            let _ = conn.read(&mut buf).await.unwrap();
            conn.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 1000\r\n\r\npartial")
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(50)).await;
            // Close mid-body: with Content-Length: 1000 and only a partial
            // delivery, the transport surfaces an incomplete-body read
            // failure the classification must PRESERVE.
            drop(conn);
        });
        let client = dispatch::build_client().unwrap();
        let request = client
            .post(format!("http://{addr}/v1/chat/completions"))
            .body("{}");
        let deadline = dispatch::unix_ms_now() + 5_000;
        let result = tokio::time::timeout(
            Duration::from_secs(8),
            dispatch::send_with_timeouts(
                request,
                &HttpTimeouts::default(),
                Some(deadline),
                &ApplicableAuth::Bearer("dummy-key".into()),
            ),
        )
        .await;
        stub.abort();
        let (error, _) = result.expect("settles within the window").unwrap_err();
        assert_eq!(error.code, lingxi_protocol::ErrorCode::Forbidden);
        assert!(
            error.message.contains("read failed"),
            "the read failure travels with the classification: {error}"
        );
    }

    /// A parked refresh wait (401 → report_unauthorized never returns)
    /// cannot outlive the call's absolute deadline.
    struct ParkedRefresh {
        release: tokio::sync::watch::Receiver<bool>,
    }

    impl ProviderCredentialPort for ParkedRefresh {
        fn resolve<'a>(
            &'a self,
            _route: &'a lingxi_kernel::model_exchange::ResolvedModelRoute,
        ) -> Pin<
            Box<
                dyn std::future::Future<Output = Result<ApplicableAuth, CredentialError>>
                    + Send
                    + 'a,
            >,
        > {
            Box::pin(async { Ok(ApplicableAuth::Bearer("dummy-key".into())) })
        }

        fn report_unauthorized<'a>(
            &'a self,
            _route: &'a lingxi_kernel::model_exchange::ResolvedModelRoute,
            _used: &'a ApplicableAuth,
        ) -> Pin<
            Box<
                dyn std::future::Future<Output = Result<RefreshVerdict, CredentialError>>
                    + Send
                    + 'a,
            >,
        > {
            let mut release = self.release.clone();
            Box::pin(async move {
                loop {
                    if *release.borrow_and_update() {
                        return Ok(RefreshVerdict::NotRefreshable);
                    }
                    if release.changed().await.is_err() {
                        return Ok(RefreshVerdict::NotRefreshable);
                    }
                }
            })
        }
    }

    #[tokio::test]
    async fn a_parked_refresh_wait_obeys_the_call_deadline() {
        // The stub answers the first chat request with 401; the credential
        // port parks the refresh verdict forever.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let stub = tokio::spawn(async move {
            let (mut conn, _) = listener.accept().await.unwrap();
            let _request = read_request(&mut conn).await;
            conn.write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n")
                .await
                .unwrap();
            std::future::pending::<()>().await;
        });
        let (_tx, release) = tokio::sync::watch::channel(false);
        let cfg = plane_config(&format!("http://{addr}/v1"), "dummy-key");
        let gateway = Arc::new(ConfigModelGateway::from_validated(cfg));
        let plane = NetworkPlane::isolated(NetworkPolicy {
            proxy: ProxyPolicy::Direct,
            trusted_ca_pem: None,
        });
        let provider = GatewayedProvider::new_with_network(
            gateway,
            Arc::new(ParkedRefresh { release }),
            lingxi_kernel::toolcatalog::SchemaBudget::default(),
            plane,
        )
        .unwrap();
        let started = std::time::Instant::now();
        let result = drive_chat(&provider, unix_ms() + 300).await;
        let elapsed = started.elapsed();
        stub.abort();
        assert!(
            matches!(result.turn, ProviderTurn::Failed { ref error, .. }
                if error.code == lingxi_protocol::ErrorCode::BudgetExceeded),
            "the parked refresh settles as the honest budget failure: {:?}",
            result.turn
        );
        assert!(
            error_message(&result).contains("credential refresh"),
            "the failure names the refresh stage: {}",
            error_message(&result)
        );
        assert!(
            elapsed < Duration::from_secs(2),
            "the wait ended at the deadline ({elapsed:?}), not at the refresh driver's 30 s"
        );
    }

    fn error_message(result: &ProviderTurnResult) -> String {
        match &result.turn {
            ProviderTurn::Failed { error, .. } => error.message.clone(),
            other => format!("{other:?}"),
        }
    }

    /// The audit's F-WU06 queue leg: an operation call whose model permit
    /// is queued behind a sibling holding the LAST permit must surface the
    /// honest `BudgetExceeded` AT the call's total deadline — not wait the
    /// quota manager's own 30 s window on an already-spent budget, and not
    /// with a phantom transport attempt (HTTP count stays at the parked
    /// sibling's single request).
    #[tokio::test]
    async fn operations_queue_wait_obeys_the_call_budget() {
        use lingxi_adapters::models::operations::embedding::EmbeddingRequest;
        use lingxi_service::operations::OperationService;
        use lingxi_service::quotas::{LayeredQuotaLimits, QuotaLimits};

        // The provider stub accepts the first embedding request and parks
        // (holding the single model permit).
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let hits = Arc::new(AtomicUsize::new(0));
        let hits_state = hits.clone();
        tokio::spawn(async move {
            let (mut conn, _) = listener.accept().await.unwrap();
            hits_state.fetch_add(1, Ordering::SeqCst);
            let _request = read_request(&mut conn).await;
            std::future::pending::<()>().await;
        });
        let plane_json = format!(
            r#"{{
                "providers": {{"emb": {{"protocol": "openai-embeddings", "endpoint": "http://{addr}", "auth": {{"kind": "apiKey", "apiKey": "dummy-emb-key"}}}}}},
                "models": {{"embedding": {{"provider": "emb", "model": "emb-model"}}}}
            }}"#
        );
        let plane = ModelPlaneConfig::parse_and_validate(&plane_json).unwrap();
        let gateway = Arc::new(ConfigModelGateway::from_validated(plane.clone()));
        let runtime_dir =
            std::env::temp_dir().join(format!("rr1-t05-opsq-{}-{}", std::process::id(), unix_ms()));
        std::fs::create_dir_all(&runtime_dir).unwrap();
        let products = runtime_dir.join("products");
        let credentials = Arc::new(
            CredentialService::bootstrap(&plane, &runtime_dir, Arc::new(ManualClock::new(1000)))
                .unwrap(),
        );
        let quotas = Arc::new(lingxi_service::quotas::QuotaManager::new(QuotaLimits {
            model: LayeredQuotaLimits {
                global: 1,
                per_agent: 1,
                per_session: 1,
            },
            tool: LayeredQuotaLimits::DEFAULT_TOOL,
            wait_queue_capacity: 8,
            wait_timeout_ms: 30_000,
            max_agent_lanes: lingxi_service::quotas::QuotaLimits::DEFAULT_MAX_AGENT_LANES,
            max_session_lanes: lingxi_service::quotas::QuotaLimits::DEFAULT_MAX_SESSION_LANES,
        }));
        let operations = Arc::new(
            OperationService::new(
                gateway,
                credentials,
                quotas,
                vec![format!("http://{addr}")],
                products,
            )
            .unwrap(),
        );
        let request = EmbeddingRequest {
            inputs: vec!["one text".to_string()],
            dimensions: None,
            context_window: None,
            input_type: Default::default(),
        };
        // The parked sibling holds the permit.
        let parked = tokio::spawn({
            let operations = operations.clone();
            let request = request.clone();
            async move {
                let _ = operations
                    .embed(request, Some(unix_ms() + 60_000), None)
                    .await;
            }
        });
        // Let the sibling ACQUIRE and reach the stub first.
        tokio::time::sleep(Duration::from_millis(300)).await;
        let started = std::time::Instant::now();
        let outcome = operations.embed(request, Some(unix_ms() + 250), None).await;
        let elapsed = started.elapsed();
        parked.abort();
        let failure = outcome.expect_err("the queued call cannot settle successfully");
        assert_eq!(failure.code, lingxi_protocol::ErrorCode::BudgetExceeded);
        assert!(
            failure.message.contains("admission wait outlived"),
            "the failure names the queue stage: {}",
            failure.message
        );
        assert!(
            elapsed < Duration::from_secs(2),
            "the wait ended at the call budget ({elapsed:?}), not the 30 s quota window"
        );
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "the queued call made ZERO transport attempts of its own"
        );
    }

    /// The chat driver's OWN queue wait obeys the same rule: with ONE model
    /// permit and a first run parked at the provider, a second run's model
    /// call settles at its total budget (`failed.provider_error` with the
    /// `budget_exceeded` code) instead of queueing the quota manager's full
    /// 30 s window. Driven through the REAL service composition (storage,
    /// events, supervisor, quota manager) with a deterministic provider
    /// double at the `TurnProviderPort` seam.
    mod chat_queue {
        use super::*;
        use lingxi_kernel::model_exchange::ModelTurnInput;
        use lingxi_kernel::ports::{
            ProviderDescriptor, ProviderTurn, ProviderTurnResult, TurnDeltaSink, TurnProviderPort,
        };
        use lingxi_protocol::{ContentBlock, ModelCallId, NormalizedMessage};
        use lingxi_service::runs::ModelCallTuning;
        use lingxi_service::{
            prepare_layout, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
        };

        /// A provider whose FIRST call parks until released (holding the
        /// model permit), then answers normally.
        struct ParkOnceProvider {
            release: tokio::sync::watch::Receiver<bool>,
        }

        impl TurnProviderPort for ParkOnceProvider {
            fn descriptor(&self) -> ProviderDescriptor {
                ProviderDescriptor {
                    provider: "stub.provider".to_string(),
                    model: "stub.model".to_string(),
                    operation: "chat".to_string(),
                }
            }

            fn next_turn<'a>(
                &'a self,
                ctx: &'a lingxi_kernel::RunContext,
                _call: &'a ModelCallId,
                _input: &'a ModelTurnInput,
                _deltas: &'a dyn TurnDeltaSink,
            ) -> Pin<Box<dyn std::future::Future<Output = ProviderTurnResult> + Send + 'a>>
            {
                let mut release = self.release.clone();
                let ctx = ctx.clone();
                Box::pin(async move {
                    loop {
                        if *release.borrow_and_update() {
                            return ProviderTurnResult::of_ctx(
                                &ctx,
                                ProviderTurn::Final {
                                    message: NormalizedMessage {
                                        role: "assistant".to_string(),
                                        content: vec![ContentBlock::Text {
                                            text: "released".to_string(),
                                        }],
                                        model_call_id: Some(ModelCallId::new("mc-rel")),
                                    },
                                },
                            );
                        }
                        if release.changed().await.is_err() {
                            continue;
                        }
                    }
                })
            }
        }

        fn owner_principal() -> lingxi_service::Principal {
            lingxi_service::Principal {
                schema_version: 1,
                principal_id: "principal_local".to_string(),
                kind: lingxi_service::PrincipalKind::LocalUser,
                user_id: Some(lingxi_service::LOCAL_OWNER_USER_ID.to_string()),
                studio_id: None,
                server_node_id: None,
                device_id: None,
                credential_id: None,
                web_session_id: None,
                connection_kind: lingxi_service::auth::ConnectionKindSerde::Local,
                credential_kind: lingxi_service::CredentialKind::LoopbackToken,
                trust_state: lingxi_service::TrustState::Local,
                scopes: vec!["chat".to_string()],
            }
        }

        #[tokio::test]
        async fn the_run_drivers_queue_wait_obeys_the_call_budget() {
            use lingxi_service::quotas::{LayeredQuotaLimits, QuotaLimits};
            let home = std::env::temp_dir().join(format!(
                "rr1-t05-chatq-{}-{}",
                std::process::id(),
                unix_ms()
            ));
            let layout = prepare_layout(&home).expect("layout");
            let (release_tx, release) = tokio::sync::watch::channel(false);
            let provider = Arc::new(ParkOnceProvider { release });
            let state = ServiceState::bootstrap_with_deps(
                ServiceConfig {
                    bind_addr: "127.0.0.1:0".parse::<SocketAddr>().expect("static"),
                    data_home: home.clone(),
                    home_source: HomeSource::Cli,
                    network_mode: NetworkMode::Loopback,
                    shutdown_timeout_ms: 10_000,
                },
                &layout,
                ServiceDeps {
                    turn_provider: Some(provider as Arc<dyn TurnProviderPort>),
                    quota_limits: QuotaLimits {
                        model: LayeredQuotaLimits {
                            global: 1,
                            per_agent: 1,
                            per_session: 1,
                        },
                        tool: LayeredQuotaLimits::DEFAULT_TOOL,
                        wait_queue_capacity: 8,
                        wait_timeout_ms: 30_000,
                        max_agent_lanes: QuotaLimits::DEFAULT_MAX_AGENT_LANES,
                        max_session_lanes: QuotaLimits::DEFAULT_MAX_SESSION_LANES,
                    },
                    model_call_tuning: ModelCallTuning {
                        total_budget: Duration::from_millis(400),
                        retry_backoff_base: Duration::from_millis(500),
                        retry_backoff_cap: Duration::from_millis(8_000),
                    },
                    ..ServiceDeps::default()
                },
            )
            .await
            .expect("bootstrap");

            async fn execute_on(state: &ServiceState, session: &'static str) -> String {
                let storage = Arc::clone(state.storage());
                state
                    .sessions()
                    .execute_for(
                        storage.as_ref(),
                        state.events(),
                        state.runs(),
                        &owner_principal(),
                        session,
                        "hello",
                        1_790_409_600_000,
                    )
                    .await
                    .expect("execute accepted")
                    .run_id
            }

            // Run A parks at the provider, holding the single model permit
            // (the fresh database's seeded sessions).
            let state_a = state.clone();
            let run_a = tokio::spawn(async move { execute_on(&state_a, "sess_local_alpha").await });
            tokio::time::sleep(Duration::from_millis(300)).await;
            // Run B (the other seeded session — the GLOBAL model lane is
            // what it queues on) hits its 400 ms call budget in the queue.
            let started = std::time::Instant::now();
            let run_b_id = execute_on(&state, "sess_local_beta").await;
            let elapsed = started.elapsed();
            let status_value = state
                .storage()
                .query_one_text(
                    "SELECT status FROM runs WHERE run_id = ?1",
                    vec![run_b_id.clone()],
                )
                .await
                .expect("run row")
                .unwrap_or_default();
            let reason = state
                .storage()
                .query_one_text(
                    "SELECT terminal_reason FROM runs WHERE run_id = ?1",
                    vec![run_b_id],
                )
                .await
                .expect("reason row");
            // Release A and let it settle (its OWN budget already spent at
            // 400 ms inside the parked provider — the injected double
            // ignores deadlines, so the release path proves liveness).
            let _ = release_tx.send(true);
            let _ = tokio::time::timeout(Duration::from_secs(10), run_a).await;
            state.storage().close().await.expect("close");
            let _ = std::fs::remove_dir_all(&home);
            assert_eq!(status_value, "failed", "run b settled failed");
            assert_eq!(
                reason.as_deref(),
                Some("failed.provider_error"),
                "the budget-fired queue wait classifies as the budget failure, not \
                 quota_exhausted"
            );
            assert!(
                elapsed < Duration::from_secs(2),
                "run b settled at its 400 ms budget ({elapsed:?}), not the 30 s quota window"
            );
        }
    }
}
