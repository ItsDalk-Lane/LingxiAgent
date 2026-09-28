use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use lingxi_service::{
    prepare_layout, run_with_tls, HomeSource, NetworkMode, ServiceConfig, ServiceState,
};
use rustls::pki_types::pem::PemObject;
use rustls::RootCertStore;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_rustls::{TlsAcceptor, TlsConnector};

struct Running {
    addr: SocketAddr,
    stop: tokio::sync::oneshot::Sender<()>,
    task:
        tokio::task::JoinHandle<Result<lingxi_service::ServeOutcome, lingxi_service::ServiceError>>,
}

async fn start(home: PathBuf, tls: Option<TlsAcceptor>) -> (Running, String) {
    let layout = prepare_layout(&home).unwrap();
    let state = ServiceState::bootstrap(
        ServiceConfig {
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            data_home: home,
            home_source: HomeSource::Cli,
            network_mode: NetworkMode::Lan,
            shutdown_timeout_ms: 10_000,
        },
        &layout,
    )
    .await
    .unwrap();
    let token = state.auth().local_token();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(run_with_tls(
        state,
        async move {
            let _ = stop_rx.await;
        },
        move |addr| {
            let _ = ready_tx.send(addr);
        },
        Some(Duration::from_secs(5)),
        tls,
    ));
    (
        Running {
            addr: ready_rx.await.unwrap(),
            stop: stop_tx,
            task,
        },
        token,
    )
}

impl Running {
    async fn stop(self) {
        let _ = self.stop.send(());
        tokio::time::timeout(Duration::from_secs(10), self.task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
}

fn request(host: &str, method: &str, path: &str, headers: &str, body: &str) -> String {
    format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{headers}\r\n{body}", body.len())
}

async fn over_tls(connector: &TlsConnector, addr: SocketAddr, raw: String) -> String {
    let stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let mut stream = connector
        .connect(
            rustls::pki_types::ServerName::try_from("localhost").unwrap(),
            stream,
        )
        .await
        .unwrap();
    stream.write_all(raw.as_bytes()).await.unwrap();
    let mut bytes = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut bytes))
        .await
        .unwrap()
        .unwrap();
    String::from_utf8(bytes).unwrap()
}

async fn over_http(addr: SocketAddr, raw: String) -> String {
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(raw.as_bytes()).await.unwrap();
    let mut bytes = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut bytes))
        .await
        .unwrap()
        .unwrap();
    String::from_utf8(bytes).unwrap()
}

#[tokio::test]
async fn lan_password_login_requires_actual_tls_and_sets_secure_cookie() {
    let generated = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    let server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![generated.cert.der().clone()],
            rustls::pki_types::PrivateKeyDer::from_pem_slice(
                generated.signing_key.serialize_pem().as_bytes(),
            )
            .unwrap(),
        )
        .unwrap();
    let mut roots = RootCertStore::empty();
    roots.add(generated.cert.der().clone()).unwrap();
    let connector = TlsConnector::from(Arc::new(
        rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    ));
    let home = std::env::temp_dir().join(format!(
        "lingxi-r02-tls-web-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));

    let (tls, local_token) = start(
        home.clone(),
        Some(TlsAcceptor::from(Arc::new(server_config))),
    )
    .await;
    let set_password = over_tls(
        &connector,
        tls.addr,
        request(
            "127.0.0.1",
            "PUT",
            "/lingxi/v1/access/account/password",
            &format!("Authorization: Bearer {local_token}\r\n"),
            r#"{"password":"TestPassw0rd!"}"#,
        ),
    )
    .await;
    assert!(set_password.starts_with("HTTP/1.1 200"), "{set_password}");

    let public_origin = "https://lan.example.test";
    let saved_public_url = over_tls(
        &connector,
        tls.addr,
        request(
            "127.0.0.1",
            "PUT",
            "/lingxi/v1/access/network",
            &format!("Authorization: Bearer {local_token}\r\n"),
            &serde_json::json!({"mode":"lan", "listenPort": tls.addr.port(), "publicBaseUrl": public_origin}).to_string(),
        ),
    )
    .await;
    assert!(
        saved_public_url.starts_with("HTTP/1.1 200"),
        "{saved_public_url}"
    );

    let foreign_origin = over_tls(
        &connector,
        tls.addr,
        request(
            "lan.example.test",
            "POST",
            "/lingxi/v1/web-auth/login",
            "Origin: https://evil.example\r\n",
            r#"{"username":"local","password":"TestPassw0rd!","clientKind":"desktop"}"#,
        ),
    )
    .await;
    assert!(
        foreign_origin.starts_with("HTTP/1.1 403"),
        "{foreign_origin}"
    );
    assert!(!foreign_origin
        .to_ascii_lowercase()
        .contains("access-control-allow-origin"));

    let login = over_tls(
        &connector,
        tls.addr,
        request(
            "lan.example.test",
            "POST",
            "/lingxi/v1/web-auth/login",
            "Origin: https://lan.example.test\r\n",
            r#"{"username":"local","password":"TestPassw0rd!","clientKind":"desktop"}"#,
        ),
    )
    .await;
    assert!(login.starts_with("HTTP/1.1 200"), "{login}");
    assert!(
        login
            .to_ascii_lowercase()
            .contains("access-control-allow-origin: https://lan.example.test"),
        "{login}"
    );
    let cookie = login
        .lines()
        .find(|line| line.to_ascii_lowercase().starts_with("set-cookie:"))
        .expect("successful login must set a cookie");
    assert!(
        cookie.contains("HttpOnly")
            && cookie.contains("SameSite=Strict")
            && cookie.contains("Max-Age=1209600")
            && cookie.contains("Secure"),
        "{cookie}"
    );
    let cookie_pair = cookie
        .split_once(':')
        .unwrap()
        .1
        .trim()
        .split(';')
        .next()
        .unwrap();
    let session = over_tls(
        &connector,
        tls.addr,
        request(
            "lan.example.test",
            "GET",
            "/lingxi/v1/web-auth/session",
            &format!("Origin: https://lan.example.test\r\nCookie: {cookie_pair}\r\n"),
            "",
        ),
    )
    .await;
    assert!(
        session.starts_with("HTTP/1.1 200") && session.contains("\"authenticated\":true"),
        "{session}"
    );
    let logout = over_tls(
        &connector,
        tls.addr,
        request(
            "lan.example.test",
            "POST",
            "/lingxi/v1/web-auth/logout",
            &format!("Origin: https://lan.example.test\r\nCookie: {cookie_pair}\r\n"),
            "",
        ),
    )
    .await;
    assert!(logout.starts_with("HTTP/1.1 200"), "{logout}");
    assert!(
        logout
            .to_ascii_lowercase()
            .contains("access-control-allow-origin: https://lan.example.test"),
        "{logout}"
    );
    let revoked = over_tls(
        &connector,
        tls.addr,
        request(
            "lan.example.test",
            "GET",
            "/lingxi/v1/web-auth/session",
            &format!("Origin: https://lan.example.test\r\nCookie: {cookie_pair}\r\n"),
            "",
        ),
    )
    .await;
    assert!(
        revoked.starts_with("HTTP/1.1 200") && revoked.contains("\"authenticated\":false"),
        "{revoked}"
    );
    tls.stop().await;

    let (plain, _) = start(home.clone(), None).await;
    let forged = over_http(
        plain.addr,
        request(
            "lan.example.test",
            "POST",
            "/lingxi/v1/web-auth/login",
            "X-Forwarded-Proto: https\r\n",
            r#"{"username":"local","password":"TestPassw0rd!","clientKind":"desktop"}"#,
        ),
    )
    .await;
    assert!(!forged.starts_with("HTTP/1.1 200"), "{forged}");
    assert!(
        forged.contains("password_login_requires_secure_context"),
        "{forged}"
    );
    plain.stop().await;

    // 给阶段门禁保存真实 HTTPS 与明文拒绝的逐项观察；绝不记录 Cookie 密钥。
    if let Ok(target) = std::env::var("R02_TLS_WEB_LOGIN_CASES_PATH") {
        let case = |name: &str, actual: bool, observed: serde_json::Value| {
            serde_json::json!({
                "case": name, "expect": 1, "actual": u8::from(actual),
                "ok": actual, "observed": observed,
            })
        };
        let cases = vec![
            case(
                "web-login-https-password-secure-14day-cookie",
                login.starts_with("HTTP/1.1 200")
                    && cookie.contains("HttpOnly")
                    && cookie.contains("SameSite=Strict")
                    && cookie.contains("Max-Age=1209600")
                    && cookie.contains("Secure"),
                serde_json::json!({"httpStatus": 200, "httpOnly": true,
                    "sameSiteStrict": true, "maxAgeSeconds": 1209600, "secure": true}),
            ),
            case(
                "web-login-https-session-logout-revoked",
                session.contains("\"authenticated\":true")
                    && logout.starts_with("HTTP/1.1 200")
                    && revoked.contains("\"authenticated\":false"),
                serde_json::json!({"sessionAuthenticated": true,
                    "logoutStatus": 200, "sameCookieRevoked": true}),
            ),
            case(
                "web-login-foreign-origin-denied",
                foreign_origin.starts_with("HTTP/1.1 403")
                    && !foreign_origin
                        .to_ascii_lowercase()
                        .contains("access-control-allow-origin"),
                serde_json::json!({"httpStatus": 403, "corsGrant": false}),
            ),
            case(
                "web-login-http-forwarded-proto-denied",
                !forged.starts_with("HTTP/1.1 200")
                    && forged.contains("password_login_requires_secure_context"),
                serde_json::json!({"passwordAccepted": false,
                    "forwardedProtoBypass": false}),
            ),
        ];
        std::fs::write(
            target,
            format!(
                "{}\n",
                serde_json::to_string_pretty(&serde_json::json!({
                    "schema": "lingxi.leaf-case-results.v1", "cases": cases,
                }))
                .expect("serialize TLS Web login evidence")
            ),
        )
        .expect("write TLS Web login evidence");
    }
    std::fs::remove_dir_all(home).unwrap();
}
