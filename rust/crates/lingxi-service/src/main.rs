//! lingxi-service binary — headless Rust service entrypoint (R02-T01,
//! configuration/path/instance startup chain extended by R02-T02).
//!
//! Runs with no Tauri/Electron anywhere in the process tree (proved by the
//! dependency-graph gate, acceptance R02-A01). Lifecycle contract:
//! - data-root precedence `--test-mode` > `--home <DIR>` > `LINGXI_HOME` > `--config <FILE>` `home` entry; no source is a loud refusal (exit 2); the effective root, its source and every ignored source are logged to the safe log (stderr) before anything is written.
//! - single-writer lock (`{home}/lingxi-service/instance.lock`, OS advisory
//!   lock) is taken before the listener binds; a live peer => explicit
//!   rejection with the `LINGXI_SERVICE_SINGLE_WRITER_BLOCKED` stderr marker
//!   and exit code 3 (the lock is the liveness authority; recorded PIDs are
//!   diagnostics only). A leftover record with a free lock is a stale
//!   record: archived, logged with `LINGXI_SERVICE_STALE_RECORD_TAKEN_OVER`,
//!   and taken over.
//! - One machine-readable readiness line goes to stdout
//!   (`LINGXI_SERVICE_READY addr=... home=... source=...`); everything else
//!   logs to stderr via tracing, so harnesses can wait deterministically.
//! - SIGINT/SIGTERM trigger graceful shutdown; the instance record is
//!   removed only if it is still ours; exit code 0 on clean stop, 4 if
//!   shutdown record-cleanup failed, 1 on serve failure. Nothing swallowed.

use std::io::IsTerminal as _;
use std::process::ExitCode;

use lingxi_service::instance::InstanceLockError;
use lingxi_service::{
    acquire, parse_cli, prepare_layout, run, InstanceRecord, ServiceConfig, ServiceState,
    HOME_ENV_VAR, SINGLE_WRITER_BLOCKED_MARKER, STALE_RECORD_MARKER,
};

const USAGE: &str = r#"usage: lingxi-service [--bind <SOCKADDR>] [--home <DIR>] \
[--config <FILE>] [--test-mode] [--network-mode <loopback|lan>]

Options:
  --bind <SOCKADDR>       Listen address (default 127.0.0.1:0, loopback + ephemeral).
  --home <DIR>            Absolute service data root (created if missing).
  --config <FILE>         Strict JSON config file ({"home": "/absolute/path"}).
  --test-mode             Force an isolated synthetic home under the system temp
                          dir; --home/LINGXI_HOME/--config home are ignored.
  --network-mode <MODE>   loopback (default) or lan. LAN exposure is an explicit
                          opt-in: a non-loopback --bind without --network-mode lan
                          is a startup error, and even loopback requests require
                          authentication (no loopback-trust exemption).
  --help                  Print this help and exit 0.
  --version               Print server identity/version and exit 0.

Data-root precedence: --test-mode > --home > LINGXI_HOME (env) > --config home.
Every explicitly given source is validated even when it loses precedence.
No source at all is a startup error (exit 2) - there is no default into a
real user directory.

Exit codes: 0 clean stop; 1 serve failure; 2 configuration/startup error;
3 single-writer lock held by another instance (or lock IO failure);
4 shutdown instance-record cleanup failed.
"#;

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
        // ANSI only on a real terminal: the safe log must stay
        // machine-greppable when redirected (evidence scripts key on it).
        .with_ansi(std::io::stderr().is_terminal())
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

    // ---- configuration resolution (R02-T02 step 1) ----
    let cli = match parse_cli(argv) {
        Ok(cli) => cli,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::from(2);
        }
    };
    // The environment is read HERE, at the composition root only (DEP-08
    // keeps the domain free of std::env by machine-checked rule).
    let env_home = std::env::var(HOME_ENV_VAR).ok();
    let temp_base = std::env::temp_dir();
    let config = match ServiceConfig::from_sources(&cli, env_home.as_deref(), &temp_base) {
        Ok(config) => config,
        Err(err @ lingxi_service::ConfigError::MissingHome) => {
            eprintln!("error: {err}\n\n{USAGE}");
            return ExitCode::from(2);
        }
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::from(2);
        }
    };

    // ---- safe log: the effective path and why (before anything is written) ----
    {
        let ignored = config_ignored_summary(&cli, env_home.as_deref());
        tracing::info!(
            effective_home = %config.data_home.display(),
            source = %config.home_source,
            ignored_sources = %ignored,
            "data root resolved"
        );
    }

    // ---- normalized layout, permissions, canonicalization (step 2) ----
    let layout = match prepare_layout(&config.data_home) {
        Ok(layout) => layout,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::from(2);
        }
    };
    tracing::info!(
        canonical_home = %layout.home.display(),
        runtime_dir = %layout.runtime_dir.display(),
        "data root prepared (canonicalized; runtime dir private 0700)"
    );

    // ---- single-writer lock + instance identity (steps 3–4) ----
    let (guard, stale) = match acquire(&layout) {
        Ok(acquired) => acquired,
        Err(err @ InstanceLockError::HeldByPeer(_)) => {
            // Machine-readable marker first (mirrors the Node gate's
            // LINGXI_* stderr markers), then the human diagnostic.
            eprintln!("{}", blocked_marker(&err));
            eprintln!("error: {err}");
            return ExitCode::from(3);
        }
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::from(3);
        }
    };
    tracing::info!(
        instance_id = %guard.identity().instance_id,
        start_nonce = %guard.identity().start_nonce,
        pid = guard.identity().pid,
        entropy = lingxi_service::instance::entropy_source(),
        lock = %layout.lock_path.display(),
        network_mode = %config.network_mode,
        "single-writer lock acquired"
    );
    if let Some(stale) = stale {
        let mismatches = stale.handshake_mismatches();
        eprintln!(
            "{STALE_RECORD_MARKER} home={} previousInstanceId={} previousPid={} \
             previousStartNonce={} previousStartedAtUnixMs={} handshakeMismatches={}",
            layout.home.display(),
            stale.instance_id,
            stale.pid,
            stale.start_nonce,
            stale.started_at_unix_ms,
            if mismatches.is_empty() {
                "none".to_string()
            } else {
                mismatches.join("; ")
            }
        );
        tracing::warn!(
            stale_instance_id = %stale.instance_id,
            stale_archive = %layout.stale_archive_path.display(),
            "stale instance record taken over (lock was free; decision never used pid liveness)"
        );
    }

    // ---- auth bootstrap (R02-T03): loopback token + registries ----
    // Runs after the lock, before the bind: a broken auth store is a
    // startup error (exit 2), never an auth-less serve.
    let home_display = layout.home.display().to_string();
    let source_display = config.home_source.to_string();
    let state = match ServiceState::bootstrap(config, &layout) {
        Ok(state) => state,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::from(2);
        }
    };
    tracing::info!(
        local_token_file = %state.auth().local_token_path().display(),
        "auth service bootstrapped (per-start loopback token, owner-only)"
    );

    // ---- serve; publish the instance record once the address is known ----
    // The guard is shared with the on_ready callback (which runs exactly
    // once, synchronously, after the listener is bound and before serving
    // starts); after `run` returns, this task is its only user again.
    let guard = std::sync::Arc::new(std::sync::Mutex::new(guard));
    let ready_guard = std::sync::Arc::clone(&guard);
    let shutdown = shutdown_signal();
    let result = run(state, shutdown, move |addr| {
        let mut guard = ready_guard
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Err(err) = guard.publish(addr) {
            // Startup-critical state cannot be persisted: fail loudly
            // instead of serving without a published instance record.
            eprintln!("error: cannot publish instance record: {err}");
            std::process::exit(3);
        }
        tracing::info!(
            record = %layout.record_path.display(),
            bind_addr = %addr,
            "instance record published (atomic write)"
        );
        // Readiness contract: exactly one stdout line, machine-parseable.
        println!("LINGXI_SERVICE_READY addr={addr} home={home_display} source={source_display}");
        use std::io::Write as _;
        let _ = std::io::stdout().flush();
    })
    .await;

    match result {
        Ok(()) => {
            // Shutdown cleanup: remove OUR record only, then unlock.
            let mut guard = guard
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match guard.release() {
                Ok(()) => {
                    tracing::info!(
                        "lingxi-service stopped cleanly (own record removed, lock released)"
                    );
                    ExitCode::SUCCESS
                }
                Err(err) => {
                    eprintln!("error: shutdown instance-record cleanup failed: {err}");
                    ExitCode::from(4)
                }
            }
        }
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn blocked_marker(err: &InstanceLockError) -> String {
    match err {
        InstanceLockError::HeldByPeer(diag) => {
            let (home, record, probe) = (&diag.home, &diag.record, &diag.probe);
            let (pid, instance_id, nonce, addr) = match record {
                Some(InstanceRecord {
                    pid,
                    instance_id,
                    start_nonce,
                    bind_addr,
                    ..
                }) => (
                    pid.to_string(),
                    instance_id.clone(),
                    start_nonce.clone(),
                    bind_addr.clone().unwrap_or_else(|| "none".to_string()),
                ),
                None => (
                    "unknown".to_string(),
                    "unknown".to_string(),
                    "unknown".to_string(),
                    "unknown".to_string(),
                ),
            };
            let probe = match probe {
                Some(lingxi_service::PeerProbe::Live) => "live",
                _ => "unreachable",
            };
            format!(
                "{SINGLE_WRITER_BLOCKED_MARKER} home={} recordedPid={pid} \
                 recordedInstanceId={instance_id} recordedStartNonce={nonce} \
                 recordedAddr={addr} probe={probe} authority=os-file-lock",
                home.display()
            )
        }
        InstanceLockError::Io { .. } => {
            format!("{SINGLE_WRITER_BLOCKED_MARKER} authority=os-file-lock phase=lock-io")
        }
    }
}

fn config_ignored_summary(cli: &lingxi_service::CliOptions, env_home: Option<&str>) -> String {
    let mut parts: Vec<String> = Vec::new();
    if cli.test_mode {
        if let Some(home) = &cli.home {
            parts.push(format!("cli(--home)={}", home.display()));
        }
        if let Some(value) = env_home {
            parts.push(format!("env({HOME_ENV_VAR})={value}"));
        }
        if let Some(path) = &cli.config {
            parts.push(format!("config-file={}", path.display()));
        }
    } else if cli.home.is_some() {
        if let Some(value) = env_home {
            parts.push(format!("env({HOME_ENV_VAR})={value}"));
        }
        if let Some(path) = &cli.config {
            parts.push(format!("config-file={}", path.display()));
        }
    } else if env_home.is_some() {
        if let Some(path) = &cli.config {
            parts.push(format!("config-file={}", path.display()));
        }
    }
    if parts.is_empty() {
        "none".to_string()
    } else {
        parts.join(",")
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
