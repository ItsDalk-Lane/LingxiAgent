//! lingxi-service binary — headless Rust service entrypoint (R02-T01).
//!
//! Runs with no Tauri/Electron anywhere in the process tree (proved by the
//! dependency-graph gate, acceptance R02-A01). Lifecycle contract:
//! - `--home` is mandatory and absolute; the composition root refuses to
//!   guess a data root (R02-T02 owns the full precedence resolution).
//! - One machine-readable readiness line goes to stdout
//!   (`LINGXI_SERVICE_READY addr=... home=...`); everything else logs to
//!   stderr via tracing, so harnesses can wait deterministically.
//! - SIGINT/SIGTERM trigger graceful shutdown; exit code 0 on clean stop,
//!   non-zero on startup or serve failure. No failure is swallowed.

use std::process::ExitCode;

use lingxi_service::{run, ConfigError, ServiceConfig};

const USAGE: &str = "usage: lingxi-service [--bind <SOCKADDR>] --home <DIR>

Options:
  --bind <SOCKADDR>  Listen address (default 127.0.0.1:0, loopback + ephemeral).
  --home <DIR>       REQUIRED absolute service data root (created if missing).
  --help             Print this help and exit 0.
  --version          Print server identity/version and exit 0.
";

fn print_version() {
    println!(
        "{} {} wire-protocol {}..={} data-epoch {}",
        lingxi_service::SERVER_KIND,
        lingxi_service::server_version(),
        lingxi_protocol::handshake::WIRE_PROTOCOL_MIN_SUPPORTED,
        lingxi_protocol::handshake::WIRE_PROTOCOL_MAX_SUPPORTED,
        lingxi_protocol::ContractVersions::R00_BASELINE.data_epoch,
    );
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.iter().any(|a| a == "--help" || a == "-h") {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    if argv.iter().any(|a| a == "--version") {
        print_version();
        return ExitCode::SUCCESS;
    }

    let config = match ServiceConfig::from_cli_args(argv) {
        Ok(config) => config,
        Err(err @ ConfigError::MissingHome) => {
            eprintln!("error: {err}\n\n{USAGE}");
            return ExitCode::from(2);
        }
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::from(2);
        }
    };

    if let Err(err) = config.prepare_data_home() {
        eprintln!("error: {err}");
        return ExitCode::from(2);
    }

    tracing::info!(
        bind = %config.bind_addr,
        home = %config.data_home.display(),
        "lingxi-service starting"
    );

    let home_display = config.data_home.display().to_string();
    let shutdown = shutdown_signal();
    let result = run(config, shutdown, |addr| {
        // Readiness contract: exactly one stdout line, machine-parseable.
        println!("LINGXI_SERVICE_READY addr={addr} home={home_display}");
        use std::io::Write as _;
        let _ = std::io::stdout().flush();
    })
    .await;

    match result {
        Ok(()) => {
            tracing::info!("lingxi-service stopped cleanly");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

/// Resolves on SIGINT or SIGTERM. Logging the received signal keeps the
/// shutdown path diagnosable without swallowing the shutdown itself.
async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut term =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(term) => term,
                Err(err) => {
                    tracing::warn!(%err, "cannot install SIGTERM handler; Ctrl-C still works");
                    if ctrl_c.await.is_ok() {
                        tracing::info!("shutdown signal: SIGINT");
                    }
                    return;
                }
            };
        tokio::select! {
            _ = ctrl_c => tracing::info!("shutdown signal: SIGINT"),
            _ = term.recv() => tracing::info!("shutdown signal: SIGTERM"),
        }
    }
    #[cfg(not(unix))]
    {
        if ctrl_c.await.is_ok() {
            tracing::info!("shutdown signal: Ctrl-C");
        }
    }
}
