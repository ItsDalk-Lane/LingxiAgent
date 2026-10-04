//! REVIEW-T05 fix-r1 F-02 discrimination proof (reviewer-authored,
//! isolated): the implementer's new test
//! `the_credential_bearing_client_never_consults_the_ambient_proxy` claims
//! it fails if `.no_proxy()` is removed from `OAuthHttp::new()`. That claim
//! rests on one mechanism: a DEFAULT reqwest client (system-proxy feature
//! on) built under a poisoned HTTP_PROXY routes even a LOOPBACK plain-HTTP
//! request through the env proxy. This probe exercises exactly that
//! mechanism against a counting listener, without touching product code.
//!
//! Expected: control arm (`no_proxy()`) → 0 proxy hits, direct 200;
//! default arm (no `no_proxy()`) → proxy hit counted, target never reached.
//! If the default arm shows 0 hits, the new test has NO discriminating
//! power and F-02's regression pin is hollow.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

async fn counting_proxy() -> (String, Arc<AtomicU64>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("proxy bind");
    let addr = listener.local_addr().expect("addr");
    let hits = Arc::new(AtomicU64::new(0));
    let counter = Arc::clone(&hits);
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            counter.fetch_add(1, Ordering::SeqCst);
            drop(socket); // answer nothing: a hit is the signal
        }
    });
    (format!("http://{addr}"), hits)
}

async fn plain_target() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("target bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut buf = [0u8; 4096];
            let _ = socket.read(&mut buf).await;
            let body = b"{}";
            let _ = socket
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await;
            let _ = socket.write_all(body).await;
        }
    });
    format!("http://{addr}/token")
}

#[tokio::main]
async fn main() {
    let (proxy_url, hits) = counting_proxy().await;
    let target = plain_target().await;

    // Poison the env (both cases; no NO_PROXY exclusion), mirroring the
    // implementer's ProxyEnvGuard.
    for var in [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
        "NO_PROXY",
        "no_proxy",
    ] {
        std::env::remove_var(var);
    }
    std::env::set_var("HTTP_PROXY", &proxy_url);

    // Arm 1 (control): the discipline — explicit no_proxy().
    let disciplined = reqwest::Client::builder().no_proxy().build().expect("client");
    let direct = disciplined.post(&target).body("x").send().await;
    println!(
        "ARM_NO_PROXY: direct_status={:?} proxy_hits={}",
        direct.as_ref().map(|r| r.status().as_u16()).map_err(|e| e.to_string()),
        hits.load(Ordering::SeqCst)
    );

    // Arm 2: the pre-fix shape — default builder, system-proxy reads the
    // poisoned env at build time.
    let default = reqwest::Client::builder().build().expect("client");
    let proxied = default.post(&target).body("x").send().await;
    let proxied_hits = hits.load(Ordering::SeqCst);
    println!(
        "ARM_DEFAULT: result_err={} proxy_hits={}",
        proxied.is_err(),
        proxied_hits
    );

    let discriminates = proxied_hits > 0;
    println!("DISCRIMINATION_MECHANISM_REAL={discriminates}");
    let direct_ok = matches!(&direct, Ok(r) if r.status().as_u16() == 200);
    if discriminates && direct_ok && proxied.is_err() {
        println!("VERDICT=test_discrimination_sound");
    } else {
        println!("VERDICT=test_discrimination_HOLLOW");
        std::process::exit(2);
    }
}
