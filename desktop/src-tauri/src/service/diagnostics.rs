//! R09-T02 诊断写者：`{LINGXI_HOME}/diagnostics/tauri-launch/events.jsonl`。
//!
//! 与 Electron 的 `diagnostics/desktop-launch/renderer.log` 同构但独立目录
//! （设计 Q-09：Tauri 使用 `diagnostics/tauri-launch/`，crash log 用
//! `tauri-crash.log`，避免与 Electron 的 `crash.log` 互相覆盖）。
//!
//! **数据根写入纪律（真机实测教训）**：Rust 服务的 data-epoch 门只接受
//! “可证明是新的” home（空目录或只含 `lingxi-service/` 运行时脚手架）。
//! 宿主若在 spawn 前就把 `diagnostics/` 写进一个尚未 stamp 的新 home，
//! 服务会以 `unstamped-home-with-data` 明确拒绝启动。因此诊断是**惰性**的：
//! - home 已存在 `data-epoch.json`（服务已 stamp）→ 直接写 home；
//! - 服务在本次启动中到达 READY（服务在监听前完成 stamp）→ 激活并回填
//!   启动期缓冲事件；
//! - 尚未 stamp 且启动失败 → 失败记录写 TMPDIR 下的 fallback 目录，并把
//!   实际路径打到 stderr（证据可定位），**绝不污染用户数据根**。
//!
//! 写入前统一脱敏：任何已登记 secret（服务 token、远端 token）在输出里被替换
//! 为 `[redacted]`。诊断是证据面（A01/A02/A03/A04 直接读取），因此写者是
//! 宿主的正式实现，不是测试桩。

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

pub const EVENTS_DIR: &str = "diagnostics/tauri-launch";
pub const EVENTS_FILE: &str = "events.jsonl";
pub const CRASH_FILE: &str = "tauri-crash.log";
pub const FAILURE_FILE: &str = "failure.json";
pub const STAMP_FILE: &str = "data-epoch.json";
pub const FALLBACK_DIR: &str = "lingxi-tauri-launch";

struct Inner {
    home: PathBuf,
    events_path: PathBuf,
    crash_path: PathBuf,
    fallback_dir: PathBuf,
    active: Mutex<bool>,
    pending: Mutex<Vec<String>>,
    secrets: Mutex<Vec<String>>,
}

#[derive(Clone)]
pub struct Diagnostics {
    inner: Arc<Inner>,
}

fn home_hash(home: &Path) -> String {
    // 稳定短哈希：同一 home 的 fallback 目录跨进程/跨构建可定位（不依赖盐）。
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in home.to_string_lossy().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

impl Diagnostics {
    pub fn for_home(home: &Path) -> Self {
        let dir = home.join(EVENTS_DIR);
        let fallback_dir = std::env::temp_dir()
            .join(FALLBACK_DIR)
            .join(home_hash(home));
        Self {
            inner: Arc::new(Inner {
                home: home.to_path_buf(),
                events_path: dir.join(EVENTS_FILE),
                crash_path: home.join(CRASH_FILE),
                fallback_dir,
                active: Mutex::new(false),
                pending: Mutex::new(Vec::new()),
                secrets: Mutex::new(Vec::new()),
            }),
        }
    }

    pub fn dir(&self) -> PathBuf {
        self.inner.home.join(EVENTS_DIR)
    }

    /// home 已由服务 stamp（可安全写入宿主诊断）。
    fn home_stamped(&self) -> bool {
        self.inner.home.join(STAMP_FILE).is_file()
    }

    pub fn is_active(&self) -> bool {
        *self
            .inner
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 服务 READY（或 home 已 stamp）后激活 home 诊断并回填启动期缓冲。
    /// 幂等；未 stamp 的 home 不会被创建目录。
    pub fn activate(&self) -> bool {
        {
            let active = self
                .inner
                .active
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if *active {
                return true;
            }
        }
        if !self.home_stamped() {
            return false;
        }
        if let Err(error) = fs::create_dir_all(
            self.events_path()
                .parent()
                .unwrap_or_else(|| Path::new(".")),
        ) {
            eprintln!("[tauri-diagnostics] cannot create diagnostics dir: {error}");
            return false;
        }
        let mut active = self
            .inner
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if *active {
            return true;
        }
        *active = true;
        let pending: Vec<String> = {
            let mut pending = self
                .inner
                .pending
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            std::mem::take(&mut *pending)
        };
        for line in pending {
            if let Err(error) = append_line(&self.inner.events_path, &line) {
                eprintln!("[tauri-diagnostics] flush failed: {error}");
            }
        }
        true
    }

    pub fn events_path(&self) -> PathBuf {
        self.inner.events_path.clone()
    }

    pub fn crash_path(&self) -> PathBuf {
        if self.is_active() || self.home_stamped() {
            self.inner.crash_path.clone()
        } else {
            self.inner.fallback_dir.join("tauri-crash.log")
        }
    }

    pub fn failure_path(&self) -> PathBuf {
        if self.is_active() || self.home_stamped() {
            self.dir().join(FAILURE_FILE)
        } else {
            self.inner.fallback_dir.join(FAILURE_FILE)
        }
    }

    /// 证据/测试用：home 与 fallback 两个可能的失败记录位置（home 优先）。
    pub fn failure_path_candidates(&self) -> [PathBuf; 2] {
        [
            self.dir().join(FAILURE_FILE),
            self.inner.fallback_dir.join(FAILURE_FILE),
        ]
    }

    pub fn fallback_failure_path(&self) -> PathBuf {
        self.inner.fallback_dir.join(FAILURE_FILE)
    }

    /// 登记需要脱敏的 secret（永不写入诊断）。
    pub fn register_secret(&self, secret: &str) {
        if secret.is_empty() {
            return;
        }
        let mut secrets = self
            .inner
            .secrets
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !secrets.iter().any(|known| known == secret) {
            secrets.push(secret.to_string());
        }
    }

    pub fn redact(&self, text: &str) -> String {
        let mut redacted = text.to_string();
        let secrets = self
            .inner
            .secrets
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        for secret in secrets.iter() {
            if !secret.is_empty() {
                redacted = redacted.replace(secret.as_str(), "[redacted]");
            }
        }
        redacted
    }

    fn render(&self, event: &str, fields: Value) -> String {
        let mut record = json!({
            "schema": "lingxi.r09.tauri-launch.v1",
            "event": event,
            "unixMs": now_unix_ms(),
        });
        if let Some(fields) = fields.as_object() {
            for (key, value) in fields {
                record[key] = redact_value(&|text| self.redact(text), value.clone());
            }
        }
        serde_json::to_string(&record).unwrap_or_else(|error| {
            eprintln!("[tauri-diagnostics] serialize failed: {error}");
            json!({"schema": "lingxi.r09.tauri-launch.v1", "event": event, "serializeError": true})
                .to_string()
        })
    }

    /// 追加一条 JSONL 事件。未激活时缓冲 + 镜像 stderr（证据可从宿主 stderr 读到），
    /// 激活后回填。写入失败不阻塞启动/退出主流程。
    pub fn log(&self, event: &str, fields: Value) {
        let line = self.render(event, fields);
        if self.is_active() || self.home_stamped() {
            self.activate();
            if let Err(error) = append_line(&self.inner.events_path, &line) {
                eprintln!("[tauri-diagnostics] append failed: {error}");
            }
            return;
        }
        {
            let mut pending = self
                .inner
                .pending
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            pending.push(line.clone());
        }
        eprintln!("[tauri-diag] {line}");
    }

    fn route_file(&self, base_name: &str, body: &str) -> PathBuf {
        let target = if self.is_active() || self.home_stamped() {
            self.dir().join(base_name)
        } else {
            self.inner.fallback_dir.join(base_name)
        };
        if let Some(parent) = target.parent() {
            let _ = fs::create_dir_all(parent);
        }
        match fs::write(&target, body) {
            Ok(()) => {}
            Err(error) => eprintln!("[tauri-diagnostics] write {target:?} failed: {error}"),
        }
        target
    }

    pub fn crash(&self, text: &str) {
        let line = format!(
            "[{}] {}\n",
            now_unix_ms(),
            self.redact(text).replace('\n', "\n    ")
        );
        let target = self.route_file("tauri-crash.log", &line);
        eprintln!("[tauri-diagnostics] crash log: {}", target.display());
    }

    /// 启动失败记录（A03 直接消费）：先落盘再弹窗/退出。
    pub fn write_failure(&self, record: &Value) {
        let payload = json!({
            "schema": "lingxi.r09.service-launch-failure.v1",
            "recordedAtUnixMs": now_unix_ms(),
            "record": redact_value(&|text| self.redact(text), record.clone()),
        });
        let text = match serde_json::to_string_pretty(&payload) {
            Ok(text) => format!("{text}\n"),
            Err(error) => {
                eprintln!("[tauri-diagnostics] failure record serialize failed: {error}");
                return;
            }
        };
        let target = self.route_file(FAILURE_FILE, &text);
        eprintln!("[tauri-diagnostics] failure record: {}", target.display());
    }
}

fn redact_value(redact: &impl Fn(&str) -> String, value: Value) -> Value {
    match value {
        Value::String(text) => Value::String(redact(&text)),
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|item| redact_value(redact, item))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| (key, redact_value(redact, value)))
                .collect(),
        ),
        other => other,
    }
}

fn append_line(path: &Path, line: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")?;
    file.flush()
}

pub fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_home(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lingxi-r09-diag-{name}-{}-{}",
            std::process::id(),
            now_unix_ms()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn unstamped_home_gets_no_directories_and_buffers_events() {
        let home = temp_home("unstamped");
        let diagnostics = Diagnostics::for_home(&home);
        diagnostics.log("resolve", json!({"home": home.display().to_string()}));
        assert!(
            !home.join(EVENTS_DIR).exists(),
            "unstamped home must stay provably-new for the Rust service epoch gate"
        );
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn activate_after_stamp_flushes_pending_events_and_redacts() {
        let home = temp_home("stamped");
        let diagnostics = Diagnostics::for_home(&home);
        diagnostics.register_secret("deadbeefdeadbeefdeadbeefdeadbeef");
        diagnostics.log(
            "identity",
            json!({"token": "deadbeefdeadbeefdeadbeefdeadbeef", "port": 42}),
        );
        fs::write(home.join(STAMP_FILE), b"{\"epoch\":1}").unwrap();
        assert!(diagnostics.activate());
        let text = fs::read_to_string(diagnostics.events_path()).unwrap();
        assert!(text.contains("\"event\":\"identity\""));
        assert!(!text.contains("deadbeefdeadbeefdeadbeefdeadbeef"));
        assert!(text.contains("[redacted]"));
        diagnostics.log("ready", json!({"addr": "127.0.0.1:43120"}));
        let text = fs::read_to_string(diagnostics.events_path()).unwrap();
        assert_eq!(text.lines().count(), 2);
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn failure_on_unstamped_home_goes_to_fallback_not_home() {
        let home = temp_home("failure");
        let diagnostics = Diagnostics::for_home(&home);
        diagnostics.write_failure(&json!({"reason": "RUNTIME_IDENTITY_MISMATCH"}));
        assert!(
            !home.join(EVENTS_DIR).exists(),
            "failure record must not poison a fresh home"
        );
        let candidate = diagnostics.fallback_failure_path();
        assert!(
            candidate.is_file(),
            "fallback failure record must exist at {candidate:?}"
        );
        let parsed: Value = serde_json::from_str(&fs::read_to_string(&candidate).unwrap()).unwrap();
        assert_eq!(
            parsed["record"]["reason"],
            Value::String("RUNTIME_IDENTITY_MISMATCH".into())
        );
        let fallback_root = candidate.parent().unwrap().to_path_buf();
        let _ = fs::remove_dir_all(home);
        let _ = fs::remove_dir_all(fallback_root);
    }

    #[test]
    fn stamped_home_failure_record_lives_in_home() {
        let home = temp_home("stamped-failure");
        fs::write(home.join(STAMP_FILE), b"{\"epoch\":1}").unwrap();
        let diagnostics = Diagnostics::for_home(&home);
        diagnostics.write_failure(&json!({"reason": "REMOTE_REQUIRES_DEV_BUILD"}));
        assert!(diagnostics.failure_path().is_file());
        assert!(diagnostics
            .failure_path()
            .starts_with(home.join(EVENTS_DIR)));
        let _ = fs::remove_dir_all(home);
    }
}
