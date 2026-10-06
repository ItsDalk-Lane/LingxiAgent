//! R05 RR1 WP-T05 green probe (interface-evolution adaptation of the
//! old-red probe, for the FIXED tree): the SAME four correct-behavior
//! assertions, with ONLY the call shape adapted to the fixed surfaces —
//! ① the `network` section now exists (the old tree's parse refusal was
//! the defect); ② the shared client is built UNDER the SYSTEM policy
//! (`NetworkPlane` + `build_client_under_policy`) instead of the frozen
//! `no_proxy()` builder; ③④ the egress and error-body legs call the
//! unchanged production APIs. No assertion text was weakened.
//!
//! The proxy-environment leg reads the proxy target from
//! `PROXY_ENV_TEST_ADDR` (exported by the runner together with
//! HTTP_PROXY/HTTPS_PROXY before the process starts).

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use lingxi_adapters::models::credentials::ApplicableAuth;
    use lingxi_adapters::models::network::{NetworkPlane, NetworkPolicy};
    use lingxi_adapters::models::{config::ModelPlaneConfig, dispatch, egress};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // ── shared helpers ────────────────────────────────────────────────────

    /// Reads one full HTTP request head+body off a raw connection.
    async fn read_request(conn: &mut tokio::net::TcpStream) -> String {
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

    fn sse_completion_body() -> String {
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"ok\"}}]}\n\ndata: \
         {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: \
         [DONE]\n\n"
            .to_string()
    }

    /// The system-default policy plane (snapshots the exported proxy
    /// environment at construction — the FIXED surface the old tree
    /// lacked).
    fn system_plane() -> Arc<NetworkPlane> {
        NetworkPlane::from_environment()
    }

    // ── F14: the model plane must carry a network policy section ─────────

    #[test]
    fn network_policy_section_parses_and_validates() {
        let plane = ModelPlaneConfig::from_value(serde_json_payload());
        assert!(
            plane.is_ok(),
            "the model plane must carry a validated network policy section: {plane:?}"
        );
    }

    fn serde_json_payload() -> serde_json::Value {
        serde_json::json!({
            "providers": {"main": {"protocol": "openai-completions", "endpoint": "https://api.example.invalid/v1", "auth": {"kind": "apiKey", "apiKey": "dummy-probe-key"}}},
            "models": {"chat": {"provider": "main", "model": "probe-model"}},
            "network": {
                "proxy": {
                    "mode": "manual",
                    "httpProxy": "http://proxy.internal.invalid:8080",
                    "httpsProxy": "http://proxy.internal.invalid:8080",
                    "noProxy": "localhost, 127.0.0.1, ::1"
                }
            }
        })
    }

    // ── F14: the shared HTTP client must honor the system proxy env ──────

    #[tokio::test]
    async fn system_proxy_environment_routes_the_shared_client_through_the_proxy() {
        let addr = std::env::var("PROXY_ENV_TEST_ADDR").expect(
            "the runner exports PROXY_ENV_TEST_ADDR together with HTTP(S)_PROXY before launch",
        );
        let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{}", addr))
            .await
            .expect("the runner reserves this port");
        let hits = Arc::new(AtomicUsize::new(0));
        let state = hits.clone();
        let stub = tokio::spawn(async move {
            let (mut conn, _) = listener.accept().await.unwrap();
            state.fetch_add(1, Ordering::SeqCst);
            let _request = read_request(&mut conn).await;
            let body = sse_completion_body();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\
                 Connection: close\r\n\r\n{body}",
                body.len()
            );
            conn.write_all(response.as_bytes()).await.unwrap();
        });
        let client = dispatch::build_client_under_policy(
            &dispatch::HttpTimeouts::default(),
            system_plane().policy().as_ref(),
        )
        .unwrap();
        let request = client
            .post("http://model-source.probe.invalid/v1/chat/completions")
            .body("{}");
        let deadline = dispatch::unix_ms_now() + 5_000;
        let outcome = dispatch::send_with_timeouts(
            request,
            &dispatch::HttpTimeouts::default(),
            Some(deadline),
            &ApplicableAuth::Bearer("dummy-probe-key".into()),
        )
        .await;
        let _ = outcome;
        stub.abort();
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "the shared HTTP client must route the request through the exported \
             system proxy (the connection target must actually change)"
        );
    }

    // ── F15: egress must validate the RESOLVED address, not the spelling ─

    #[tokio::test]
    async fn domain_resolving_to_loopback_must_not_be_connected() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let guard =
            egress::EgressGuard::from_provider_endpoints(&["https://authorized.example.invalid/v1".into()])
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
            tokio::spawn(async move { guard.download(&url, Some(dispatch::unix_ms_now() + 2_000)).await });
        let connected = tokio::time::timeout(Duration::from_millis(400), hit_rx)
            .await
            .is_ok();
        let _ = download.await;
        stub.abort();
        assert!(
            !connected,
            "egress connected to an unauthorized private TCP destination after resolving the \
             hostname localhost; no credential or TLS bypass was used"
        );
    }

    // ── F16: the error body read must obey the absolute deadline ─────────

    #[tokio::test]
    async fn error_body_must_obey_total_deadline() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let stub = tokio::spawn(async move {
            let (mut conn, _) = listener.accept().await.unwrap();
            let mut buf = [0; 4096];
            let _ = conn.read(&mut buf).await.unwrap();
            conn.write_all(
                b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 1000\r\n\r\nx",
            )
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
                &dispatch::HttpTimeouts::default(),
                Some(deadline),
                &ApplicableAuth::Bearer("dummy-probe-key".into()),
            ),
        )
        .await;
        stub.abort();
        assert!(
            result.is_ok(),
            "error body read exceeded its absolute deadline by >200ms"
        );
    }
}
