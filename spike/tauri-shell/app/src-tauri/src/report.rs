//! 宿主侧 ndjson 证据记录器：写入 $T06_HOST_REPORT 指定的文件（loopback 之外的
//! 第二证据通道；页面侧经 fetch 上报到证据服务器）。不写则降级到 stderr，不静默丢弃。

use std::io::Write;
use std::sync::{Mutex, OnceLock};

pub struct Reporter {
    file: Option<Mutex<std::fs::File>>,
}

static GLOBAL: OnceLock<Reporter> = OnceLock::new();

impl Reporter {
    pub fn init_from_env() {
        let file = std::env::var("T06_HOST_REPORT").ok().and_then(|path| {
            std::fs::File::create(&path)
                .map_err(|e| eprintln!("[t06] cannot create host report {path}: {e}"))
                .ok()
        });
        let reporter = Reporter {
            file: file.map(Mutex::new),
        };
        let _ = GLOBAL.set(reporter);
    }

    pub fn global() -> &'static Reporter {
        GLOBAL.get_or_init(|| Reporter { file: None })
    }

    pub fn event(&self, kind: &str, data: serde_json::Value) {
        let line = serde_json::json!({
            "ts_ms": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            "event": kind,
            "data": data,
        });
        let text = serde_json::to_string(&line).unwrap_or_else(|_| "{\"event\":\"ser-fail\"}".into());
        match &self.file {
            Some(f) => {
                if let Ok(mut guard) = f.lock() {
                    let _ = guard.write_all(text.as_bytes());
                    let _ = guard.write_all(b"\n");
                    let _ = guard.flush();
                }
            }
            None => eprintln!("[t06-host-report] {text}"),
        }
    }
}
