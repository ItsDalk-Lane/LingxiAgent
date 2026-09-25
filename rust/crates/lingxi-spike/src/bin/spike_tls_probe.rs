//! R01-T03 TLS probe (taskbook step 4-5, W08): performs ONE real HTTPS request
//! with certificate verification fully enabled (red line: never disabled) to
//! prove the rustls + platform-native-root path works on this machine.
//!
//! Target: https://index.crates.io/config.json — the same static config cargo
//! itself fetches; a fixed, dependency-free URL. Run with proxy variables
//! stripped (dev machine proxy at 127.0.0.1:7890 is dead); the command line
//! used is recorded in the evidence log.
//!
//! Exit 0 = HTTPS 200 with verification on. Any TLS/verify error exits 1 with
//! the full error chain.

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let url = "https://index.crates.io/config.json";
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("SPIKE_TLS_FAIL client build: {e}");
            std::process::exit(1);
        }
    };

    match client.get(url).send().await {
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            tracing::info!(%status, "https response");
            if status == reqwest::StatusCode::OK && body.contains("\"dl\"") {
                println!(
                    "SPIKE_TLS_OK url={} status={} verification=enabled backend=rustls-platform-verifier",
                    url, status
                );
            } else {
                eprintln!(
                    "SPIKE_TLS_FAIL url={} unexpected status={} body={:.120}",
                    url, status, body
                );
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("SPIKE_TLS_FAIL url={} error={:?}", url, e);
            std::process::exit(1);
        }
    }
}
