//! R01-T03 SQLite bounded-queue prototype driver (taskbook step 2): opens a
//! real database file in a temp dir, runs put/get/count through the bounded
//! single-writer queue, reports the negotiated journal mode and SQLite
//! version. Exit 0 only when every assertion holds.

use std::path::PathBuf;

use lingxi_spike::queue::{BoundedQueue, QueueOp, QueueResult};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    if let Err(e) = run().await {
        tracing::error!(error = %e, "sqlite queue prototype FAILED");
        eprintln!("SPIKE_SQLITE_QUEUE_FAIL {e}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("lingxi-spike-queue-demo-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let db = dir.join("queue.db");
    let _ = std::fs::remove_file(&db);

    let queue = BoundedQueue::open(&db, 8).await?;
    tracing::info!(db = %db.display(), capacity = queue.capacity(), "queue open");

    queue
        .submit(QueueOp::Put {
            key: "greeting".into(),
            value: "你好，灵犀 — sqlite bounded queue".into(),
        })
        .await?;
    let got = queue
        .submit(QueueOp::Get {
            key: "greeting".into(),
        })
        .await?;
    if got != QueueResult::Value(Some("你好，灵犀 — sqlite bounded queue".into())) {
        return Err(format!("unexpected read-back: {got:?}").into());
    }

    // Fill beyond capacity through the awaiting submit path: proves
    // backpressure works without losing a single job.
    const N: usize = 200;
    for i in 0..N {
        queue
            .submit(QueueOp::Put {
                key: format!("k{i:04}"),
                value: format!("v{i}"),
            })
            .await?;
    }
    let count = queue.submit(QueueOp::Count).await?;
    if count != QueueResult::Count(N + 1) {
        return Err(format!("expected {} rows, got {count:?}", N + 1).into());
    }

    let stats = queue.submit(QueueOp::Stats).await?;
    let (sqlite_version, journal_mode) = match stats {
        QueueResult::Stats {
            sqlite_version,
            journal_mode,
        } => (sqlite_version, journal_mode),
        other => return Err(format!("unexpected stats: {other:?}").into()),
    };
    if journal_mode != "wal" {
        return Err(format!("expected WAL journal mode, got {journal_mode}").into());
    }

    queue.close().await?;
    let ok = db.exists();
    std::fs::remove_dir_all(&dir)?;
    if !ok {
        return Err("db file missing after close".into());
    }
    println!(
        "SPIKE_SQLITE_QUEUE_OK sqlite_version={} journal_mode={} jobs={} capacity=8",
        sqlite_version,
        journal_mode,
        N + 1
    );
    Ok(())
}
