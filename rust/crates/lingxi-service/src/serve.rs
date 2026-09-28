//! Transport serving loop with CONNECTION-level admission (R02
//! stage-repair R2 / R2-F04; the serving structure mirrors axum 0.8's
//! `serve`/`with_graceful_shutdown`, which this loop replaces because the
//! framework never exposes the accept edge or the per-connection HTTP/1
//! builder).
//!
//! Connections are served by hyper's EXPLICIT HTTP/1 builder
//! (`hyper::server::conn::http1::Builder`), not hyper-util's `auto`
//! Builder (R02 stage-repair R3 / R3-F01): the auto Builder parks every
//! fresh connection in an untimed version-sniffing state — it waits for
//! the first bytes to tell HTTP/1 from the HTTP/2 preface — and
//! `header_read_timeout` only arms later, inside hyper's HTTP/1 state
//! machine (`proto/h1/conn.rs` `poll_read_head`). A zero-byte client, or
//! one trickling a strict prefix of the HTTP/2 preface, therefore held a
//! connection slot INDEFINITELY despite the configured budget. With
//! explicit HTTP/1 serving there is no sniffing state at all: the
//! header-read timer arms at the first poll, so the budget covers the
//! connection from the moment it is accepted. The transport only ever
//! speaks HTTP/1 (loopback HTTP/1.1 plus the WS upgrade via
//! `hyper::upgrade`, which hyper's own builder supports natively through
//! `Connection::with_upgrades`); an actual HTTP/2 preface is simply
//! malformed HTTP/1 here and is bounded by the same header budget like
//! any other incomplete request.
//!
//! Two budgets bind every connection from the moment it is accepted —
//! BEFORE any header byte is parsed (the request-level
//! [`crate::limits::HttpAdmission`] middleware only runs after hyper has
//! read complete headers, so a slow- or never-headers client used to hold
//! a socket entirely outside the configured limits):
//!
//! 1. **count**: [`ConnectionAdmission`] is a hard cap on concurrently
//!    open connections (derived at the composition root as 2× the request
//!    in-flight cap `http_max_in_flight` — strictly looser than the
//!    request gate, so N in-flight request holders can never consume every
//!    connection slot and the request-level 503 stays reachable). Over the
//!    cap the freshly accepted socket is closed immediately with the
//!    `LINGXI_TRANSPORT_REJECTED` marker logged — never queued silently.
//! 2. **time**: hyper's `header_read_timeout` (driven by an explicit
//!    `TokioTimer` — it is inert without one) gives a connection the
//!    per-request budget (`http_request_budget_ms`) to deliver complete
//!    headers; the connection is closed when the budget expires. This
//!    applies to the first request and to every subsequent keep-alive
//!    request alike.
//!
//! Once the headers arrive, the request-level admission/budget middleware
//! takes over — the two layers compose, they do not overlap.
//!
//! Graceful shutdown is the axum semantics preserved: the signal stops
//! the accept loop, every live connection is told to `graceful_shutdown`
//! (in-flight responses complete), and the loop returns after the last
//! connection task ends. The caller's drain watchdog bounds the whole
//! wait from the signal moment (R02 stage-repair R1 / F03), so a wedged
//! connection can never hold the process past the unified budget.

use std::convert::Infallible;
use std::io;
use std::net::SocketAddr;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::Request;
use axum::response::Response;
use axum::Router;
use hyper::body::Incoming;
use hyper_util::rt::{TokioIo, TokioTimer};
use hyper_util::service::TowerToHyperService;
use rustls::pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio_rustls::TlsAcceptor;

use crate::limits::{ConnectionAdmission, ConnectionAdmissionGuard};
use crate::{redact_line, TRANSPORT_REJECTED_MARKER};

/// 仅由服务端已经完成的 TLS 握手设置，HTTP 请求头不能改变这个值。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SecureTransport(pub bool);

trait ConnectionIo: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> ConnectionIo for T {}

/// 启动前检查证书链与私钥是否能组成可用的 TLS 配置，不打印私钥内容。
pub fn load_tls_acceptor(cert_path: &Path, key_path: &Path) -> io::Result<TlsAcceptor> {
    let cert_pem = std::fs::read(cert_path)?;
    let key_pem = std::fs::read(key_path)?;
    tls_acceptor_from_pem(&cert_pem, &key_pem)
}

fn tls_acceptor_from_pem(cert_pem: &[u8], key_pem: &[u8]) -> io::Result<TlsAcceptor> {
    let certs = CertificateDer::pem_slice_iter(cert_pem)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid TLS certificate: {err}"),
            )
        })?;
    if certs.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "TLS certificate chain is empty",
        ));
    }
    let key = PrivateKeyDer::from_pem_slice(key_pem).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid TLS private key: {err}"),
        )
    })?;
    let mut config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|err| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("TLS certificate and key mismatch: {err}"),
            )
        })?;
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(TlsAcceptor::from(Arc::new(config)))
}

/// Serves `router` on `listener` until `shutdown` resolves, then drains
/// live connections (see the module docs). Errors only from listener
/// failures surfaced by the accept loop; per-connection errors are logged
/// and never abort the loop.
pub async fn serve_with_connection_admission<F>(
    listener: TcpListener,
    router: Router,
    shutdown: F,
    admission: Arc<ConnectionAdmission>,
    header_budget: Duration,
    tls_acceptor: Option<TlsAcceptor>,
) -> io::Result<()>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    // Same signal plumbing as axum's WithGracefulShutdown: the shutdown
    // future runs in its own task; completing it drops `signal_rx`, which
    // `signal_tx.closed()` observes in the accept loop and in every
    // connection task.
    let (signal_tx, signal_rx) = watch::channel(());
    tokio::spawn(async move {
        shutdown.await;
        drop(signal_rx);
    });
    let (close_tx, close_rx) = watch::channel(());

    loop {
        let (stream, remote) = tokio::select! {
            conn = listener.accept() => match conn {
                Ok(pair) => pair,
                Err(err) => {
                    // Transient accept failure (fd pressure, ...): loud,
                    // briefly paced, and the loop continues — one bad
                    // accept must never kill the serving task.
                    tracing::error!(%err, "listener accept failed; retrying");
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    continue;
                }
            },
            _ = signal_tx.closed() => {
                tracing::debug!("shutdown signal: connection accept loop stopping");
                break;
            }
        };

        // Budget 1 (count): the connection-level hard cap, enforced at the
        // accept edge — before hyper reads a single byte.
        let Some(slot) = admission.acquire() else {
            eprintln!(
                "{}",
                redact_line(&format!(
                    "{TRANSPORT_REJECTED_MARKER} reason=connection_limit remote={remote} max_connections={}",
                    admission.max()
                ))
            );
            tracing::warn!(
                %remote,
                max_connections = admission.max(),
                "connection rejected at the transport hard cap (socket closed)"
            );
            drop(stream);
            continue;
        };

        handle_connection(AcceptedConnection {
            router: router.clone(),
            signal_tx: signal_tx.clone(),
            close_rx: close_rx.clone(),
            stream,
            remote,
            slot,
            header_budget,
            tls_acceptor: tls_acceptor.clone(),
        });
    }

    // Drain: every connection task drops its `close_rx` when it ends (the
    // axum `close_tx.closed()` pattern), so this returns once the last
    // connection finished — or the caller's watchdog abandons us, which is
    // the loud, budgeted path.
    drop(close_rx);
    drop(listener);
    close_tx.closed().await;
    Ok(())
}

/// Spawns the per-connection task: hyper HTTP/1 serving with the explicit
/// header-read budget, graceful-shutdown propagation and the admission
/// slot held until the socket work ends (every exit path included).
struct AcceptedConnection {
    router: Router,
    signal_tx: watch::Sender<()>,
    close_rx: watch::Receiver<()>,
    stream: tokio::net::TcpStream,
    remote: SocketAddr,
    slot: ConnectionAdmissionGuard,
    header_budget: Duration,
    tls_acceptor: Option<TlsAcceptor>,
}

fn handle_connection(accepted: AcceptedConnection) {
    let AcceptedConnection {
        router,
        signal_tx,
        close_rx,
        stream,
        remote,
        slot,
        header_budget,
        tls_acceptor,
    } = accepted;

    tokio::spawn(async move {
        // TLS 握手也占用同一个连接预算；超时、错误或关停都释放名额。
        let secure = tls_acceptor.is_some();
        let stream: Box<dyn ConnectionIo> = match tls_acceptor {
            Some(acceptor) => {
                let result = tokio::select! {
                    result = tokio::time::timeout(header_budget, acceptor.accept(stream)) => result,
                    _ = signal_tx.closed() => return,
                };
                match result {
                    Ok(Ok(stream)) => Box::new(stream),
                    Ok(Err(err)) => {
                        tracing::debug!(%remote, %err, "TLS handshake failed");
                        return;
                    }
                    Err(_) => {
                        tracing::warn!(%remote, "TLS handshake timed out");
                        return;
                    }
                }
            }
            None => Box::new(stream),
        };
        let io = TokioIo::new(stream);
        let service = ConnectionService {
            router,
            remote,
            secure,
        };
        // R3-F01: explicit HTTP/1 serving (see the module docs). There is
        // no protocol-version sniffing state, so the header-read timer
        // below is the FIRST thing armed when the connection is polled —
        // a zero-byte or trickling client is inside the budget from the
        // accept moment.
        let mut builder = hyper::server::conn::http1::Builder::new();
        // Budget 2 (time): complete headers must arrive inside the
        // per-request budget — the timer is required for the timeout to
        // exist at all (hyper panics on a configured timeout without one).
        builder
            .timer(TokioTimer::new())
            .header_read_timeout(Some(header_budget));
        let mut conn = std::pin::pin!(builder
            .serve_connection(io, TowerToHyperService::new(service))
            .with_upgrades());
        let mut closing = false;
        loop {
            tokio::select! {
                result = conn.as_mut() => {
                    if let Err(err) = result {
                        // A header-read timeout lands here as a connection
                        // error after hyper closed the socket: explicit in
                        // the log, never silent (the client learns from the
                        // closed socket; there is no response channel left).
                        tracing::debug!(%remote, %err, "connection ended with an error (header timeout or client abort)");
                    }
                    break;
                }
                _ = signal_tx.closed(), if !closing => {
                    closing = true;
                    conn.as_mut().graceful_shutdown();
                }
            }
        }
        drop(close_rx);
        drop(slot);
    });
}

/// Per-connection request service: injects `ConnectInfo` (what axum's
/// `into_make_service_with_connect_info` does — this loop replaces that
/// constructor because the framework's `IncomingStream` has no public
/// constructor) and delegates to the router.
#[derive(Clone)]
struct ConnectionService {
    router: Router,
    remote: SocketAddr,
    secure: bool,
}

impl tower::Service<Request<Incoming>> for ConnectionService {
    type Response = Response;
    type Error = Infallible;
    type Future = Pin<Box<dyn std::future::Future<Output = Result<Response, Infallible>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        // `Router` has several `Service` impls (generic over the body and
        // one for `IncomingStream`); pin the one this loop actually serves.
        <Router as tower::Service<Request<Body>>>::poll_ready(&mut self.router, cx)
    }

    fn call(&mut self, mut req: Request<Incoming>) -> Self::Future {
        req.extensions_mut().insert(ConnectInfo(self.remote));
        req.extensions_mut().insert(SecureTransport(self.secure));
        Box::pin(<Router as tower::Service<Request<Body>>>::call(
            &mut self.router,
            req.map(Body::new),
        ))
    }
}

#[cfg(test)]
mod tls_tests {
    use super::*;
    use axum::{routing::get, Extension};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio_rustls::TlsConnector;

    #[test]
    fn tls_startup_rejects_missing_or_mismatched_key() {
        let first = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
        let second = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
        assert!(tls_acceptor_from_pem(first.cert.pem().as_bytes(), b"not a PEM key").is_err());
        assert!(tls_acceptor_from_pem(
            first.cert.pem().as_bytes(),
            second.signing_key.serialize_pem().as_bytes(),
        )
        .is_err());
        assert!(tls_acceptor_from_pem(
            b"invalid cert",
            first.signing_key.serialize_pem().as_bytes()
        )
        .is_err());
    }

    #[tokio::test]
    async fn secure_marker_requires_a_real_tls_handshake() {
        let generated = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
        let acceptor = tls_acceptor_from_pem(
            generated.cert.pem().as_bytes(),
            generated.signing_key.serialize_pem().as_bytes(),
        )
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let router = Router::new().route(
            "/probe",
            get(|Extension(secure): Extension<SecureTransport>| async move {
                if secure.0 {
                    "secure"
                } else {
                    "clear"
                }
            }),
        );
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(serve_with_connection_admission(
            listener,
            router,
            async move {
                let _ = shutdown_rx.await;
            },
            Arc::new(ConnectionAdmission::new(4)),
            Duration::from_secs(2),
            Some(acceptor),
        ));

        let mut roots = rustls::RootCertStore::empty();
        roots.add(generated.cert.der().clone()).unwrap();
        let client = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let mut tls = TlsConnector::from(Arc::new(client))
            .connect(
                rustls::pki_types::ServerName::try_from("localhost").unwrap(),
                tokio::net::TcpStream::connect(addr).await.unwrap(),
            )
            .await
            .unwrap();
        tls.write_all(b"GET /probe HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut response = Vec::new();
        tokio::time::timeout(Duration::from_secs(2), tls.read_to_end(&mut response))
            .await
            .unwrap()
            .unwrap();
        let response = String::from_utf8(response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(response.ends_with("secure"), "{response}");

        let mut clear = tokio::net::TcpStream::connect(addr).await.unwrap();
        clear.write_all(b"GET /probe HTTP/1.1\r\nHost: localhost\r\nX-Forwarded-Proto: https\r\nConnection: close\r\n\r\n").await.unwrap();
        let mut raw = [0_u8; 128];
        let read = tokio::time::timeout(Duration::from_secs(2), clear.read(&mut raw))
            .await
            .unwrap();
        assert!(!read
            .as_ref()
            .is_ok_and(|count| raw[..*count].starts_with(b"HTTP/1.1 200")));

        let _ = shutdown_tx.send(());
        task.await.unwrap().unwrap();
    }
}
