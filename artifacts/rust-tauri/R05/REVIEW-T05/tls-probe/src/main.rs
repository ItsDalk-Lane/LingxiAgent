//! REVIEW-T05 C12 proof: the PRODUCTION client builder
//! (`lingxi_adapters::models::dispatch::build_client` — the exact function
//! every chat adapter uses) must REFUSE a self-signed certificate instead of
//! skipping verification. Control: the same client speaks plain HTTP to the
//! same loopback port shape fine (proving the refusal is TLS verification,
//! not a connectivity artifact).

use lingxi_adapters::models::dispatch;

#[tokio::main]
async fn main() {
    let tls_port: u16 = std::env::args()
        .nth(1)
        .expect("usage: tls-probe <TLS_PORT> <HTTP_PORT>")
        .parse()
        .expect("port");
    let http_port: u16 = std::env::args()
        .nth(2)
        .expect("usage: tls-probe <TLS_PORT> <HTTP_PORT>")
        .parse()
        .expect("port");

    let client = dispatch::build_client().expect("the production client builder");

    // 1. self-signed TLS endpoint must be REFUSED.
    let tls_url = format!("https://127.0.0.1:{tls_port}/v1/messages");
    match client
        .post(&tls_url)
        .body("{}")
        .header("content-type", "application/json")
        .send()
        .await
    {
        Ok(response) => {
            println!("TLS_RESULT=CONNECTED status={}", response.status());
            std::process::exit(2);
        }
        Err(err) => {
            let chain = {
                let mut text = format!("{err:?}");
                let mut source = std::error::Error::source(&err);
                while let Some(cause) = source {
                    text.push_str(&format!(" <- {cause}"));
                    source = cause.source();
                }
                text
            };
            let certificate_related = chain.contains("certificate")
                || chain.contains("Certificate")
                || chain.contains("UnknownIssuer")
                || chain.contains("tls");
            println!("TLS_RESULT=REFUSED certificate_related={certificate_related}");
            println!("TLS_ERROR={chain}");
            if !certificate_related {
                std::process::exit(3);
            }
        }
    }

    // 2. control: plain HTTP over loopback must succeed with the same client
    // (the refusal above is certificate verification, not a broken client).
    let http_url = format!("http://127.0.0.1:{http_port}/v1/messages");
    match client
        .post(&http_url)
        .body("{}")
        .header("content-type", "application/json")
        .send()
        .await
    {
        Ok(response) => println!("HTTP_CONTROL=OK status={}", response.status()),
        Err(err) => {
            println!("HTTP_CONTROL=FAILED {err:?}");
            std::process::exit(4);
        }
    }
    println!("VERDICT=tls_verification_enforced");
}
