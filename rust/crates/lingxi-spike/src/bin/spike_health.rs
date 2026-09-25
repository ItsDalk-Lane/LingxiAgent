//! R01-T03 hello/health prototype (taskbook step 4): a minimal axum HTTP
//! service answering /hello and /health on 127.0.0.1 (ephemeral port), plus a
//! reqwest client that performs the loopback self-check. Both run in one
//! process so the run is deterministic and needs no external service.
//!
//! Exit 0 only when both endpoints return HTTP 200 with the expected bodies.

use axum::{routing::get, Router};
use serde_json::json;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    if let Err(e) = run().await {
        tracing::error!(error = %e, "health prototype FAILED");
        eprintln!("SPIKE_HEALTH_FAIL {e}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let app = Router::new()
        .route("/hello", get(|| async { "hello from lingxi rust core" }))
        .route(
            "/health",
            get(|| async {
                axum::Json(json!({
                    "status": "ok",
                    "component": "lingxi-spike-health",
                    "protocol": "lingxi.wire",
                    "protocolVersion": 1
                }))
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    tracing::info!(%addr, "health prototype listening");

    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("axum serve");
    });

    let client = reqwest::Client::builder()
        // TLS verification stays ON (red line); loopback HTTP needs no TLS.
        .timeout(std::time::Duration::from_secs(5))
        .build()?;

    let hello = client.get(format!("http://{addr}/hello")).send().await?;
    let hello_status = hello.status();
    let hello_body = hello.text().await?;
    tracing::info!(status = %hello_status, body = %hello_body, "GET /hello");
    if hello_status != reqwest::StatusCode::OK || hello_body != "hello from lingxi rust core" {
        return Err(format!("unexpected /hello: {hello_status} {hello_body:?}").into());
    }

    let health = client.get(format!("http://{addr}/health")).send().await?;
    let health_status = health.status();
    let health_json: serde_json::Value = health.json().await?;
    tracing::info!(status = %health_status, body = %health_json, "GET /health");
    if health_status != reqwest::StatusCode::OK || health_json["status"] != "ok" {
        return Err(format!("unexpected /health: {health_status} {health_json}").into());
    }

    server.abort();
    println!("SPIKE_HEALTH_OK addr={} hello=200 health=200", addr);
    Ok(())
}
