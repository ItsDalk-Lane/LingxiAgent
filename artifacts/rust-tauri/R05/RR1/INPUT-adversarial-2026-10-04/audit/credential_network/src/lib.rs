#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use lingxi_adapters::models::{config::ModelPlaneConfig, credentials::{ApplicableAuth, ProviderCredentialPort}, gateway::ConfigModelGateway, provider::GatewayedProvider, dispatch};
    use lingxi_kernel::model_exchange::{ModelGatewayPort, ModelRouteRequest, ModelOperation, ModelTurnInput, ToolDeclaration, ToolDeclarationSnapshot, InputImage};
    use lingxi_kernel::ports::{TurnDeltaSink, TurnDeltaSinkClosed, ModelTurnDelta, TurnProviderPort};
    use lingxi_service::credentials::{CredentialService, ProductionRefreshDriver};
    use lingxi_service::inject::ManualClock;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn config(endpoint: &str, key: &str) -> ModelPlaneConfig {
        ModelPlaneConfig::from_value(serde_json::json!({
            "providers": {"main": {"protocol": "openai-completions", "endpoint": endpoint, "auth": {"kind": "apiKey", "apiKey": key}}},
            "models": {"chat": {"provider": "main", "model": "synthetic-model-with-no-declared-capabilities"}}
        })).unwrap()
    }
    fn credentials(cfg: &ModelPlaneConfig) -> CredentialService {
        CredentialService::new(cfg, None, Arc::new(ProductionRefreshDriver::new(Duration::from_secs(1)).unwrap()), Arc::new(ManualClock::new(1000)))
    }
    #[tokio::test]
    async fn old_handle_must_not_resolve_reloaded_secret() {
        let old = config("https://old.example.invalid/v1", "dummy-old-key");
        let next = config("https://new.example.invalid/v1", "dummy-new-key");
        let svc = credentials(&old);
        let handle = svc.mint_handle("main").await.unwrap();
        svc.reload(&next).await;
        let result = svc.resolve_handle(&handle).await;
        assert!(result.is_err(), "old credential handle survived key rotation and returned {result:?}");
    }
    #[tokio::test]
    async fn removed_then_readded_provider_must_not_revive_old_handle() {
        let old = config("https://old.example.invalid/v1", "dummy-old-key");
        let next = config("https://new.example.invalid/v1", "dummy-new-key");
        let svc = credentials(&old);
        let handle = svc.mint_handle("main").await.unwrap();
        svc.revoke("main").await.unwrap();
        svc.reload(&ModelPlaneConfig::default()).await;
        svc.reload(&next).await;
        let result = svc.resolve_handle(&handle).await;
        assert!(result.is_err(), "revoked handle revived after provider recreation and returned {result:?}");
    }
    #[tokio::test]
    async fn old_route_must_not_receive_new_endpoint_credential() {
        let old = config("https://old.example.invalid/v1", "dummy-old-key");
        let next = config("https://new.example.invalid/v1", "dummy-new-key");
        let gateway = ConfigModelGateway::from_validated(old.clone());
        let svc = credentials(&old);
        let old_route = gateway.resolve_route(&ModelRouteRequest::for_operation(ModelOperation::Chat)).unwrap();
        svc.reload(&next).await;
        gateway.reload(next);
        assert_eq!(old_route.endpoint, "https://old.example.invalid/v1");
        let resolved = svc.resolve(&old_route).await;
        assert!(!matches!(resolved, Ok(ApplicableAuth::Bearer(ref key)) if key == "dummy-new-key"), "old endpoint route was handed the new endpoint's key: {resolved:?}");
    }

    struct Sink;
    impl TurnDeltaSink for Sink {
        fn emit<'a>(&'a self, _: ModelTurnDelta) -> std::pin::Pin<Box<dyn std::future::Future<Output=Result<(),TurnDeltaSinkClosed>>+Send+'a>> { Box::pin(async { Ok(()) }) }
    }
    fn ctx() -> lingxi_kernel::RunContext {
        lingxi_kernel::RunContext {
            principal: lingxi_kernel::Principal::LocalUser,
            session_id: lingxi_protocol::SessionId::new("audit_session"),
            run_id: lingxi_protocol::RunId::new("audit_run"),
            attempt: lingxi_protocol::AttemptId::new("audit_attempt"),
            generation: 1,
        }
    }
    async fn capture_stub() -> (String, Arc<Mutex<Vec<String>>>, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let state = captured.clone();
        let handle = tokio::spawn(async move {
            let (mut conn, _) = listener.accept().await.unwrap();
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            loop {
                let n = conn.read(&mut tmp).await.unwrap();
                if n == 0 { break; }
                buf.extend_from_slice(&tmp[..n]);
                if let Some(end) = buf.windows(4).position(|x| x == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buf[..end]).to_lowercase();
                    let count: usize = head.lines().find_map(|x| x.strip_prefix("content-length:").map(|v| v.trim().parse().unwrap())).unwrap_or(0);
                    if buf.len() >= end + 4 + count { break; }
                }
            }
            state.lock().unwrap().push(String::from_utf8_lossy(&buf).into_owned());
            let body = "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"ok\"}}]}\n\ndata: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            conn.write_all(response.as_bytes()).await.unwrap();
        });
        (format!("http://{addr}/v1"), captured, handle)
    }

    async fn read_request(conn: &mut tokio::net::TcpStream) -> String {
        let mut buf = Vec::new();
        let mut tmp = [0u8; 4096];
        loop {
            let n = conn.read(&mut tmp).await.unwrap();
            if n == 0 { break; }
            buf.extend_from_slice(&tmp[..n]);
            if let Some(end) = buf.windows(4).position(|x| x == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&buf[..end]).to_lowercase();
                let count: usize = head.lines().find_map(|x| x.strip_prefix("content-length:").map(|v| v.trim().parse().unwrap())).unwrap_or(0);
                if buf.len() >= end + 4 + count { break; }
            }
        }
        String::from_utf8_lossy(&buf).into_owned()
    }
    #[tokio::test]
    async fn reload_during_401_must_not_send_new_key_to_old_endpoint() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let old_addr = listener.local_addr().unwrap();
        let next_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let next_addr = next_listener.local_addr().unwrap();
        let (first_tx, first_rx) = tokio::sync::oneshot::channel();
        let (gate_tx, gate_rx) = tokio::sync::oneshot::channel();
        let stub = tokio::spawn(async move {
            let (mut first, _) = listener.accept().await.unwrap();
            let old_request = read_request(&mut first).await;
            first_tx.send(old_request.clone()).unwrap();
            gate_rx.await.unwrap();
            first.write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
            drop(first);
            let (mut second, _) = listener.accept().await.unwrap();
            let new_request = read_request(&mut second).await;
            let body = "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"ok\"}}]}\n\ndata: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            second.write_all(response.as_bytes()).await.unwrap();
            (old_request,new_request)
        });
        let cfg = config(&format!("http://{old_addr}/v1"), "dummy-old-key");
        let svc = Arc::new(credentials(&cfg));
        let gateway = Arc::new(ConfigModelGateway::from_validated(cfg));
        let provider = GatewayedProvider::new(gateway.clone(), svc.clone(), lingxi_kernel::toolcatalog::SchemaBudget::default()).unwrap();
        let call_task = tokio::spawn(async move {
            let context = ctx();
            let call = lingxi_protocol::ModelCallId::new("audit_401_call");
            let input = ModelTurnInput::first_turn("hello", ToolDeclarationSnapshot::empty());
            provider.next_turn(&context,&call,&input,&Sink).await
        });
        let first = tokio::time::timeout(Duration::from_secs(2),first_rx).await.unwrap().unwrap();
        assert!(first.contains("Bearer dummy-old-key"));
        let next = config(&format!("http://{next_addr}/v1"), "dummy-new-key");
        svc.reload(&next).await;
        gateway.reload(next);
        gate_tx.send(()).unwrap();
        let _result = tokio::time::timeout(Duration::from_secs(2),call_task).await.unwrap().unwrap();
        let (_, second) = stub.await.unwrap();
        assert!(!second.contains("Bearer dummy-new-key"), "old HTTP endpoint captured NEW endpoint's key on 401 retry: {}", second.lines().find(|s| s.to_lowercase().starts_with("authorization:")).unwrap_or("<missing>"));
    }

    #[tokio::test]
    async fn domain_resolving_to_loopback_must_not_be_connected() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let guard = lingxi_adapters::models::egress::EgressGuard::from_provider_endpoints(&["https://authorized.example.invalid/v1".into()]).unwrap();
        let url = format!("https://localhost:{}/private-image.png", addr.port());
        let (hit_tx, hit_rx) = tokio::sync::oneshot::channel();
        let stub = tokio::spawn(async move {
            let (mut conn, _) = listener.accept().await.unwrap();
            let mut b = [0u8;1];
            let _ = conn.read(&mut b).await;
            let _ = hit_tx.send(());
        });
        let download = tokio::spawn(async move { guard.download(&url,Some(dispatch::unix_ms_now()+1000)).await });
        let connected = tokio::time::timeout(Duration::from_millis(400),hit_rx).await.is_ok();
        let _ = download.await;
        stub.abort();
        assert!(!connected,"egress guard connected to an unauthorized private TCP destination after resolving hostname localhost; no credential or TLS bypass was used");
    }

    #[tokio::test]
    async fn protocol_error_echo_must_not_carry_complete_secret_into_kernel() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let stub = tokio::spawn(async move {
            let (mut conn, _) = listener.accept().await.unwrap();
            let _ = read_request(&mut conn).await;
            let body = "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_dummy\",\"type\":\"function\",\"function\":{\"name\":\"dummy-cobalt-key\",\"arguments\":\"{}\"}}]}}]}\n\ndata: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n";
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            conn.write_all(response.as_bytes()).await.unwrap();
        });
        let cfg = config(&format!("http://{addr}/v1"), "dummy-cobalt-key");
        let svc = Arc::new(credentials(&cfg));
        let gateway = Arc::new(ConfigModelGateway::from_validated(cfg));
        let provider = GatewayedProvider::new(gateway,svc,lingxi_kernel::toolcatalog::SchemaBudget::default()).unwrap();
        let context = ctx();
        let call = lingxi_protocol::ModelCallId::new("audit_protocol_echo_call");
        let input = ModelTurnInput::first_turn("hello",ToolDeclarationSnapshot::empty());
        let result = tokio::time::timeout(Duration::from_secs(2),provider.next_turn(&context,&call,&input,&Sink)).await.unwrap();
        stub.await.unwrap();
        let error = match result.turn { lingxi_kernel::ports::ProviderTurn::Failed{error,..} => error, other => panic!("unexpected {other:?}") };
        let redacted = lingxi_service::redact_line(&error.message);
        assert!(!redacted.contains("dummy-cobalt-key"), "full credential escaped into kernel ProtocolError and survived service redaction: {redacted}");
    }
    #[tokio::test]
    async fn undeclared_tool_capability_must_make_zero_requests() {
        let (endpoint, observed, stub) = capture_stub().await;
        let cfg = config(&endpoint, "dummy-key");
        let svc = Arc::new(credentials(&cfg));
        let gateway = Arc::new(ConfigModelGateway::from_validated(cfg));
        let provider = GatewayedProvider::new(gateway, svc, lingxi_kernel::toolcatalog::SchemaBudget::default()).unwrap();
        let tools = ToolDeclarationSnapshot { catalog_generation: 1, declarations: vec![ToolDeclaration {
            target: "read".into(), wire_name: "read".into(), description: "Read".into(),
            input_schema: lingxi_protocol::ToolSchemaDocument { dialect: "https://json-schema.org/draft/2020-12/schema".into(), schema: serde_json::json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}) }
        }]};
        let input = ModelTurnInput::first_turn("read a file", tools);
        let context = ctx();
        let call = lingxi_protocol::ModelCallId::new("audit_model_call");
        let _result = tokio::time::timeout(Duration::from_secs(2), provider.next_turn(&context, &call, &input, &Sink)).await.unwrap();
        stub.abort();
        assert_eq!(observed.lock().unwrap().len(), 0, "gateway sent tools for a model with no declared tools capability");
    }
    #[tokio::test]
    async fn undeclared_image_capability_must_make_zero_requests() {
        let (endpoint, observed, stub) = capture_stub().await;
        let cfg = config(&endpoint, "dummy-key");
        let svc = Arc::new(credentials(&cfg));
        let gateway = Arc::new(ConfigModelGateway::from_validated(cfg));
        let provider = GatewayedProvider::new(gateway, svc, lingxi_kernel::toolcatalog::SchemaBudget::default()).unwrap();
        let mut input = ModelTurnInput::first_turn("describe image", ToolDeclarationSnapshot::empty());
        input.images.push(InputImage { bytes: vec![137,80,78,71], mime: "image/png".into() });
        let context = ctx();
        let call = lingxi_protocol::ModelCallId::new("audit_model_call");
        let _result = tokio::time::timeout(Duration::from_secs(2), provider.next_turn(&context, &call, &input, &Sink)).await.unwrap();
        stub.abort();
        assert_eq!(observed.lock().unwrap().len(), 0, "gateway sent image for a model with no declared image capability");
    }
    #[test]
    fn truncation_must_not_expose_credential_prefix() {
        let auth = ApplicableAuth::Bearer("secret-cobalt-pearl".into());
        let echoed = format!("{}secret-cobalt-pearl", " ".repeat(504));
        let excerpt = dispatch::scrubbed_excerpt(&echoed, &auth);
        let redacted = lingxi_service::redact_line(&excerpt);
        assert!(!redacted.contains("secret-c"), "credential prefix leaked after truncation and both redaction layers: {redacted:?}");
    }
    #[tokio::test]
    async fn error_body_must_obey_total_deadline() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let stub = tokio::spawn(async move {
            let (mut conn, _) = listener.accept().await.unwrap();
            let mut buf = [0; 4096];
            let _ = conn.read(&mut buf).await.unwrap();
            conn.write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 1000\r\n\r\nx").await.unwrap();
            std::future::pending::<()>().await;
        });
        let client = dispatch::build_client().unwrap();
        let req = client.post(format!("http://{addr}/v1/chat/completions")).body("{}");
        let deadline = dispatch::unix_ms_now()+50;
        let result = tokio::time::timeout(Duration::from_millis(250), dispatch::send_with_timeouts(req, &dispatch::HttpTimeouts::default(), Some(deadline), &ApplicableAuth::Bearer("dummy-key".into()))).await;
        stub.abort();
        assert!(result.is_ok(), "error body read exceeded its absolute deadline by >200ms");
    }
}
