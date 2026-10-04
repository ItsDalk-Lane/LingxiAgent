//! REVIEW-T05 C01 connect-hang leg proof (reviewer-authored, isolated).
//!
//! A connect-phase hang (here: the TCP accept succeeds but the TLS
//! handshake never completes — a silent loopback listener) must trip the
//! CONNECT segment (reqwest's `connect_timeout` wraps the whole connector,
//! TCP+TLS, via a tower TimeoutLayer — verified in the vendored reqwest
//! 0.13.5 source), not the first-byte window (30 s) and not the stream idle
//! bound (60 s). Injected connect window: 400 ms.
//!
//! Environment note: a raw SYN-blackhole hang is NOT producible on this
//! machine — a TUN-mode system proxy (127.0.0.1:7897) accepts every TCP
//! connect instantly (`nc -z 192.0.2.1 81` succeeds in 0.00 s), so the
//! TLS-handshake stall is the only honest connect-phase hang available
//! offline. Same segment, same timeout path.
//!
//! Classification pinned empirically: the connect-phase timeout surfaces
//! with `is_connect() == true` AND `is_timeout() == true` on the raw client,
//! so `send_with_timeouts` classifies it RETRYABLE `upstream_unavailable` —
//! the A10-consistent call (a connect-phase failure proves the request never
//! reached the server: no HTTP bytes can have been processed before the TLS
//! handshake completes). The ledger C05 wording ("连接拒绝为唯一可重试传输类")
//! slightly understates: the retryable class is connect-PHASE failures
//! (refusal AND hang), which is exactly the provably-not-accepted set.

use std::time::{Duration, Instant};

use lingxi_adapters::models::dispatch::{
    self, HttpTimeouts, HTTP_CONNECT_TIMEOUT_MS, HTTP_FIRST_BYTE_TIMEOUT_MS,
};

#[tokio::main]
async fn main() {
    // Cross-read: the production constants must equal the pre-registered
    // R05_BASELINE §8 values (connect 10 000 / first_byte 30 000).
    println!("CONST_CONNECT_MS={HTTP_CONNECT_TIMEOUT_MS}");
    println!("CONST_FIRST_BYTE_MS={HTTP_FIRST_BYTE_TIMEOUT_MS}");
    let consts_ok = HTTP_CONNECT_TIMEOUT_MS == 10_000 && HTTP_FIRST_BYTE_TIMEOUT_MS == 30_000;

    // The silent listener: accepts TCP, never answers the TLS handshake.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("local addr").port();
    let holder = tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((socket, _)) = listener.accept().await {
            held.push(socket); // hold the connection open, say nothing
        }
    });

    let timeouts = HttpTimeouts {
        connect: Duration::from_millis(400),
        first_byte: Duration::from_secs(30),
    };
    let client = dispatch::build_client_with_timeouts(&timeouts).expect("client builder");
    let url = format!("https://127.0.0.1:{port}/v1/messages");

    // Phase 1 (diagnostic): the RAW client — timing + reqwest error flags.
    let raw_started = Instant::now();
    match client
        .post(&url)
        .body("{}")
        .header("content-type", "application/json")
        .send()
        .await
    {
        Ok(response) => {
            println!("RAW_UNEXPECTED_SUCCESS status={}", response.status());
            std::process::exit(2);
        }
        Err(err) => {
            println!("RAW_ELAPSED_MS={}", raw_started.elapsed().as_millis());
            println!(
                "RAW_IS_CONNECT={} RAW_IS_TIMEOUT={}",
                err.is_connect(),
                err.is_timeout()
            );
        }
    }

    // Phase 2: the PRODUCTION send path classification.
    let auth = lingxi_adapters::models::credentials::ApplicableAuth::None;
    let started = Instant::now();
    let result = dispatch::send_with_timeouts(
        client
            .post(&url)
            .body("{}")
            .header("content-type", "application/json"),
        &timeouts,
        None,
        &auth,
    )
    .await;
    let elapsed = started.elapsed();

    let verdict = match result {
        Ok(response) => {
            println!("CONNECT_RESULT=UNEXPECTED_SUCCESS status={}", response.status());
            false
        }
        Err((error, retryable)) => {
            println!("CONNECT_ELAPSED_MS={}", elapsed.as_millis());
            println!("CONNECT_ERROR_CODE={}", error.code.wire_name());
            println!("CONNECT_RETRYABLE={retryable}");
            println!("CONNECT_ERROR_MSG={}", error.message);
            // The connect segment fired: the injected 400 ms window elapsed
            // (a refusal returns in single-digit ms), far under the 30 s
            // first-byte window; classification is the A10-consistent
            // retryable upstream_unavailable (connect-phase = provably
            // never accepted).
            elapsed >= Duration::from_millis(350)
                && elapsed < Duration::from_secs(5)
                && retryable
                && error.code == lingxi_protocol::ErrorCode::UpstreamUnavailable
        }
    };
    println!("CONNECT_SEGMENT_FIRED={verdict}");
    holder.abort();
    if verdict && consts_ok {
        println!("VERDICT=connect_hang_segment_distinct_and_consts_pinned");
    } else {
        println!("VERDICT=connect_hang_segment_NOT_proven");
        std::process::exit(2);
    }
}
