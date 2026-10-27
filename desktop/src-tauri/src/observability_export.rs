//! R10 WS2：Model Observatory 导出流式保存（Tauri 宿主侧 4 命令）。
//!
//! 现役 Electron 语义见 `desktop/main.cjs:6052-6139`：`begin` 弹系统保存对话框并
//! 发放不透明 `exportId`；`write` 按块（单块 ≤4MB）顺序写；`end` 关闭文件并返回
//! 累计字节；`abort` / 窗口销毁关闭句柄并删除半成品。
//!
//! # 安全模型（与 Electron 对齐，不新增任意文件写能力）
//!
//! - 路径只来自用户亲手操作的系统保存对话框（`save_file` + `set_parent`）；
//! - renderer 只拿到绑定发起窗口 label 的不透明 `exportId`（capability token）：
//!   没有 `write_file(path)` 形状的命令面；未知/跨窗 token 一律 `TOKEN_INVALID`；
//! - 单块 base64 解码后 ≤4MB，顺序 append；超限显式 `INVALID_ARGUMENT`，不截断、
//!   不丢块；
//! - `end` 关闭句柄；`abort` / 窗口销毁删除半成品——宁可没有文件，也不留“看起来
//!   完整”的半成品；磁盘失败显式返回错误（不静默降级）。
//!
//! 会话表逻辑（打开/写入/结束/中止/按窗口清理）全部是 [`ObservabilityExportState`]
//! 的纯方法，不依赖 `AppHandle`/事件循环，单测直接构造 state 配临时目录验证；
//! 只有 [`cleanup_window`] 需要 `AppHandle` 取宿主托管的 state。

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager, State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

use crate::native::errors::{deny, E_INVALID_ARGUMENT, E_TOKEN_INVALID};
use crate::native::window_labels::{require_role, WindowRole};

/// 单块写入上限（解码后字节数；Electron `OBSERVABILITY_EXPORT_CHUNK_MAX_BYTES`）。
pub const CHUNK_MAX_BYTES: usize = 4 * 1024 * 1024;
/// 默认导出文件名（renderer 传入名非法/缺失时的回退；Electron 同值）。
pub const DEFAULT_FILE_NAME: &str = "lingxi-model-observability.jsonl";
/// 默认文件名白名单长度上限（Electron 正则 `^[A-Za-z0-9._-]{1,128}$`）。
pub const MAX_FILE_NAME_LEN: usize = 128;

/// 磁盘失败（创建/写入/flush/删除部分文件）的稳定机器码；不静默降级。
pub const E_EXPORT_IO: &str = "EXPORT_IO";
/// 会话表不可用（内部状态中毒）的稳定机器码。
pub const E_EXPORT_STATE: &str = "EXPORT_STATE";

/// 4MB 解码上限对应的 base64 编码长度上界（含 padding）：`ceil(n/3)*4`。
/// 先按编码长度拒绝超长输入，避免超大字符串在解码前就占用内存。
const ENCODED_CHUNK_MAX_LEN: usize = (CHUNK_MAX_BYTES + 2) / 3 * 4;

/// 单个导出会话（capability token 对应的宿主侧状态）。
struct Session {
    /// 用户选定路径上已打开的写句柄（`w` 语义：begin 时 truncate）。
    file: File,
    file_path: PathBuf,
    /// 发起窗口 label：只有同一窗口能继续 write/end/abort。
    owner_label: String,
    bytes_written: u64,
}

/// 导出会话表：`exportId -> Session`，由 `lib.rs` 的 `.manage(...)` 托管。
///
/// 命令与窗口销毁清理共享同一份状态。所有方法不依赖 tauri 运行时；`write/end/
/// abort` 的 token 归属校验、4MB 判定、关闭/删除文件都在这里完成，命令层只负责
/// 窗口门、对话框与 JSON 形状。
#[derive(Default)]
pub struct ObservabilityExportState {
    sessions: Mutex<HashMap<String, Session>>,
}

impl ObservabilityExportState {
    fn lock_sessions(&self) -> Result<MutexGuard<'_, HashMap<String, Session>>, String> {
        self.sessions
            .lock()
            .map_err(|_| format!("{E_EXPORT_STATE}: 导出会话状态不可用"))
    }

    /// `begin` 的存储侧：打开（截断）用户选定路径并登记 token。
    ///
    /// 磁盘错误显式返回；失败时不会留下会话（文件已打开则随作用域关闭）。
    pub(crate) fn open_session(
        &self,
        export_id: String,
        file_path: PathBuf,
        owner_label: String,
    ) -> Result<(), String> {
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&file_path)
            .map_err(|error| {
                format!(
                    "{E_EXPORT_IO}: 无法创建导出文件 {}: {error}",
                    file_path.display()
                )
            })?;
        let mut sessions = self.lock_sessions()?;
        sessions.insert(
            export_id,
            Session {
                file,
                file_path,
                owner_label,
                bytes_written: 0,
            },
        );
        Ok(())
    }

    /// 解码/写入前的 token 归属预检（与 Electron 的检查顺序一致：非法 token
    /// 优先于 chunk 参数错误）。
    pub(crate) fn ensure_owner(&self, export_id: &str, owner_label: &str) -> Result<(), String> {
        let sessions = self.lock_sessions()?;
        if session_owned_by(&sessions, export_id, owner_label) {
            Ok(())
        } else {
            Err(token_invalid())
        }
    }

    /// `write` 的存储侧：校验绑定窗口 → 校验解码后 4MB 上限 → 顺序写。
    ///
    /// 返回累计字节数。写入失败保留会话（与 Electron 一致，便于随后 abort 清理）。
    pub(crate) fn write_chunk(
        &self,
        export_id: &str,
        owner_label: &str,
        bytes: &[u8],
    ) -> Result<u64, String> {
        if !chunk_len_allowed(bytes.len()) {
            return Err(invalid_argument("导出块超过 4MB 上限"));
        }
        let mut sessions = self.lock_sessions()?;
        if !session_owned_by(&sessions, export_id, owner_label) {
            return Err(token_invalid());
        }
        let session = sessions.get_mut(export_id).ok_or_else(token_invalid)?;
        let file_path = session.file_path.clone();
        session.file.write_all(bytes).map_err(|error| {
            format!(
                "{E_EXPORT_IO}: 写入导出文件 {} 失败: {error}",
                file_path.display()
            )
        })?;
        session.bytes_written += bytes.len() as u64;
        Ok(session.bytes_written)
    }

    /// `end` 的存储侧：flush 后移出会话（drop 关闭句柄），返回累计字节与路径。
    ///
    /// flush 失败时保留会话（返回显式错误），让后续 abort 仍能删除半成品。
    /// Rust 的 `File` 没有 fallible close：flush 用于暴露待写错误，close 的 OS
    /// 级失败在 drop 中不可观测（Electron `closeSync` 同样只在显式调用时抛错）。
    pub(crate) fn end_session(
        &self,
        export_id: &str,
        owner_label: &str,
    ) -> Result<(u64, PathBuf), String> {
        let mut sessions = self.lock_sessions()?;
        if !session_owned_by(&sessions, export_id, owner_label) {
            return Err(token_invalid());
        }
        let (bytes_written, file_path) = {
            let session = sessions.get_mut(export_id).ok_or_else(token_invalid)?;
            session
                .file
                .flush()
                .map_err(|error| format!("{E_EXPORT_IO}: 关闭导出文件失败（flush）: {error}"))?;
            (session.bytes_written, session.file_path.clone())
        };
        sessions.remove(export_id);
        Ok((bytes_written, file_path))
    }

    /// `abort` 的存储侧：未知/跨窗 token → `Ok(false)`（Electron 语义，不报错）；
    /// 已知 token → 关闭句柄后删除半成品，成功 `Ok(true)`。
    ///
    /// 删除失败显式返回错误（部分文件残留必须可见，不能伪装成“已清理”）。
    pub(crate) fn abort_session(&self, export_id: &str, owner_label: &str) -> Result<bool, String> {
        let file_path = {
            let mut sessions = self.lock_sessions()?;
            if !session_owned_by(&sessions, export_id, owner_label) {
                return Ok(false);
            }
            let session = sessions.remove(export_id).ok_or_else(token_invalid)?;
            let file_path = session.file_path.clone();
            // 先关闭文件句柄再删：Windows 上打开中的文件不可删除（Electron 同序）。
            drop(session);
            file_path
        };
        remove_partial_file(&file_path)?;
        Ok(true)
    }

    /// 窗口销毁清理的存储侧：移出该 label 的全部会话（drop 关闭句柄），返回
    /// 待删除的半成品路径。其它窗口的会话不受影响。
    pub(crate) fn drain_window(&self, label: &str) -> Result<Vec<PathBuf>, String> {
        let mut sessions = self.lock_sessions()?;
        let export_ids: Vec<String> = sessions
            .iter()
            .filter(|(_, session)| session.owner_label == label)
            .map(|(export_id, _)| export_id.clone())
            .collect();
        let mut partials = Vec::with_capacity(export_ids.len());
        for export_id in export_ids {
            if let Some(session) = sessions.remove(&export_id) {
                partials.push(session.file_path);
                // session 在此 drop → 文件句柄已关闭，随后可安全删除。
            }
        }
        Ok(partials)
    }
}

/// 窗口销毁清理（由 `lib.rs` 的 `WindowEvent::Destroyed` 调用）：关闭该窗口所有
/// 会话的文件句柄并删除半成品。
///
/// 事件回调没有错误通道：删除失败写 stderr 显式记录（不静默），且不影响其它
/// 窗口的会话清理。
pub fn cleanup_window(app: &AppHandle, label: &str) {
    let Some(state) = app.try_state::<ObservabilityExportState>() else {
        // 未托管该 state 的构建（如 R06 文件资源宿主）根本没有导出会话可清理。
        return;
    };
    let partials = match state.drain_window(label) {
        Ok(partials) => partials,
        Err(error) => {
            eprintln!("[observability-export] 窗口 {label} 导出会话清理失败: {error}");
            return;
        }
    };
    for path in partials {
        if let Err(error) = remove_partial_file(&path) {
            eprintln!("[observability-export] 窗口 {label} 部分导出文件清理失败: {error}");
        }
    }
}

// ── 命令面 ─────────────────────────────────────────────────────────────

/// `begin`：窗口门 → 系统保存对话框（模态归属调用窗口）→ 登记 capability token。
///
/// 取消返回 `{"canceled": true}`；成功返回
/// `{"canceled": false, "exportId", "filePath"}`。
#[tauri::command]
pub async fn observability_export_begin(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, ObservabilityExportState>,
    default_file_name: Option<String>,
) -> Result<Value, String> {
    let owner_label = authorize_export_window(&window)?;
    let file_name = sanitize_default_file_name(default_file_name.as_deref());
    let (tx, rx) = tokio::sync::oneshot::channel();
    // AppHandle 提供 dialog 插件实例；模态归属仍是调用窗口（set_parent）。
    app.dialog()
        .file()
        .set_parent(&window)
        .set_file_name(file_name)
        .add_filter("JSONL", &["jsonl"])
        .save_file(move |file_path| {
            let _ = tx.send(file_path);
        });
    let picked = rx
        .await
        .map_err(|_| format!("{E_EXPORT_IO}: 原生保存对话框未返回"))?;
    let Some(picked) = picked else {
        return Ok(json!({ "canceled": true }));
    };
    let file_path = picked
        .into_path()
        .map_err(|error| format!("{E_EXPORT_IO}: 保存对话框返回的路径不可用: {error}"))?;
    let export_id = uuid::Uuid::new_v4().to_string();
    state.open_session(export_id.clone(), file_path.clone(), owner_label)?;
    Ok(json!({
        "canceled": false,
        "exportId": export_id,
        "filePath": file_path.to_string_lossy().to_string(),
    }))
}

/// `write`：owner-only 分块写入（base64 解码后单块 ≤4MB），返回累计字节。
#[tauri::command]
pub async fn observability_export_write(
    window: WebviewWindow,
    state: State<'_, ObservabilityExportState>,
    export_id: String,
    chunk: String,
) -> Result<Value, String> {
    let owner_label = authorize_export_window(&window)?;
    // 检查顺序与 Electron 对齐：token 优先于 chunk 参数错误。
    state.ensure_owner(&export_id, &owner_label)?;
    let bytes = decode_export_chunk(&chunk)?;
    let bytes_written = state.write_chunk(&export_id, &owner_label, &bytes)?;
    Ok(json!({ "bytesWritten": bytes_written }))
}

/// `end`：owner-only；flush/关闭文件并返回 `{bytesWritten, filePath}`。
#[tauri::command]
pub fn observability_export_end(
    window: WebviewWindow,
    state: State<'_, ObservabilityExportState>,
    export_id: String,
) -> Result<Value, String> {
    let owner_label = authorize_export_window(&window)?;
    let (bytes_written, file_path) = state.end_session(&export_id, &owner_label)?;
    Ok(json!({
        "bytesWritten": bytes_written,
        "filePath": file_path.to_string_lossy().to_string(),
    }))
}

/// `abort`：owner-only；关闭并删除半成品。未知/跨窗 token 返回
/// `{"aborted": false}`（Electron 语义），已知 token 返回 `{"aborted": true}`。
#[tauri::command]
pub fn observability_export_abort(
    window: WebviewWindow,
    state: State<'_, ObservabilityExportState>,
    export_id: String,
) -> Result<Value, String> {
    let owner_label = authorize_export_window(&window)?;
    let aborted = state.abort_session(&export_id, &owner_label)?;
    Ok(json!({ "aborted": aborted }))
}

// ── 窗口门 / 参数校验（纯函数，单测覆盖）──────────────────────────────

/// 窗口门：只有 `main` / `settings` 允许导出（与 R10 命令策略一致）。
fn authorize_export_window(window: &WebviewWindow) -> Result<String, String> {
    let label = window.label();
    authorize_export_label(label)?;
    Ok(label.to_string())
}

/// 窗口 label 门（纯逻辑）：main / settings 之外一律 `WINDOW_NOT_ALLOWED: ...`。
pub(crate) fn authorize_export_label(label: &str) -> Result<(), String> {
    require_role(label, &[WindowRole::Main, WindowRole::Settings])
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// 默认文件名白名单（Electron 同规则）：只接受 `^[A-Za-z0-9._-]{1,128}$`，
/// 否则回退 [`DEFAULT_FILE_NAME`]。该名字只作为保存对话框的默认名，不直接拼路径。
pub(crate) fn sanitize_default_file_name(requested: Option<&str>) -> String {
    match requested {
        Some(name) if valid_default_file_name(name) => name.to_string(),
        _ => DEFAULT_FILE_NAME.to_string(),
    }
}

fn valid_default_file_name(name: &str) -> bool {
    (1..=MAX_FILE_NAME_LEN).contains(&name.len())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// 单块解码后大小上限判定（纯函数；Electron 同界）。
pub(crate) fn chunk_len_allowed(len: usize) -> bool {
    len <= CHUNK_MAX_BYTES
}

/// 前端 adapter 负责 `Uint8Array -> base64`；这里解码并复核 4MB 上限。
pub(crate) fn decode_export_chunk(chunk: &str) -> Result<Vec<u8>, String> {
    if chunk.len() > ENCODED_CHUNK_MAX_LEN {
        return Err(invalid_argument("导出块超过 4MB 上限"));
    }
    let bytes = STANDARD
        .decode(chunk)
        .map_err(|error| invalid_argument(&format!("导出块不是合法 base64: {error}")))?;
    if !chunk_len_allowed(bytes.len()) {
        return Err(invalid_argument("导出块超过 4MB 上限"));
    }
    Ok(bytes)
}

// ── 会话辅助 ───────────────────────────────────────────────────────────

fn session_owned_by(
    sessions: &HashMap<String, Session>,
    export_id: &str,
    owner_label: &str,
) -> bool {
    sessions
        .get(export_id)
        .is_some_and(|session| session.owner_label == owner_label)
}

/// 删除半成品：已不存在视为清理成功；其它删除失败显式返回错误。
fn remove_partial_file(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "{E_EXPORT_IO}: 无法删除部分导出文件 {}: {error}",
            path.display()
        )),
    }
}

fn invalid_argument(message: &str) -> String {
    format!("{E_INVALID_ARGUMENT}: {message}")
}

fn token_invalid() -> String {
    deny(E_TOKEN_INVALID, "unknown or foreign exportId").to_string()
}

// ── 单测（无 tauri App：直接构造 state + 临时目录）─────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);

    /// 测试用临时目录（Drop 自动清理；名字带 pid + 自增序号，测试并行也唯一）。
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(tag: &str) -> Self {
            let unique = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "lingxi-observability-export-{tag}-{}-{unique}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).expect("创建临时目录");
            Self { path }
        }

        fn file(&self, name: &str) -> PathBuf {
            self.path.join(name)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn begin_session(
        state: &ObservabilityExportState,
        file_path: &Path,
        owner_label: &str,
    ) -> String {
        let unique = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let export_id = format!("test-export-{unique}");
        state
            .open_session(
                export_id.clone(),
                file_path.to_path_buf(),
                owner_label.to_string(),
            )
            .expect("创建导出会话");
        export_id
    }

    fn code(error: &str) -> &str {
        error.split(':').next().unwrap_or_default()
    }

    #[test]
    fn default_file_name_whitelist_matches_electron() {
        assert_eq!(
            sanitize_default_file_name(Some("lingxi-model-observability-20261027-1043.jsonl")),
            "lingxi-model-observability-20261027-1043.jsonl"
        );
        assert_eq!(sanitize_default_file_name(Some("a")), "a");
        let longest = "a".repeat(MAX_FILE_NAME_LEN);
        assert_eq!(sanitize_default_file_name(Some(&longest)), longest);

        for requested in [
            None,
            Some(""),
            Some("export.jsonl "),
            Some("  export.jsonl"),
            Some("../../etc/passwd"),
            Some("a/b.jsonl"),
            Some("a:b.jsonl"),
            Some("a\n.jsonl"),
            Some("a\0.jsonl"),
            Some("导出.jsonl"),
        ] {
            assert_eq!(
                sanitize_default_file_name(requested),
                DEFAULT_FILE_NAME,
                "{requested:?} 必须回退默认名"
            );
        }
        let too_long = "a".repeat(MAX_FILE_NAME_LEN + 1);
        assert_eq!(
            sanitize_default_file_name(Some(&too_long)),
            DEFAULT_FILE_NAME
        );
    }

    #[test]
    fn window_gate_allows_only_main_and_settings() {
        assert!(authorize_export_label("main").is_ok());
        assert!(authorize_export_label("settings").is_ok());
        for denied in ["quick-chat", "viewer-1", "splash", "unknown-window"] {
            let error = authorize_export_label(denied).expect_err("应拒绝非导出窗口");
            assert_eq!(code(&error), "WINDOW_NOT_ALLOWED", "{denied}: {error}");
        }
        // 非法 label 语法走 WINDOW_LABEL_INVALID（不是能力门问题）。
        let invalid = authorize_export_label("main\n").expect_err("控制字符应拒绝");
        assert_eq!(code(&invalid), "WINDOW_LABEL_INVALID");
    }

    #[test]
    fn session_owner_binding_rejects_foreign_and_unknown_tokens() {
        let dir = TempDir::new("owner");
        let state = ObservabilityExportState::default();
        let path = dir.file("export.jsonl");
        let export_id = begin_session(&state, &path, "main");

        assert_eq!(
            state
                .write_chunk(&export_id, "settings", b"foreign")
                .unwrap_err(),
            token_invalid(),
            "跨窗写入必须是 TOKEN_INVALID"
        );
        assert_eq!(
            state.ensure_owner(&export_id, "settings").unwrap_err(),
            token_invalid()
        );
        assert_eq!(
            state.end_session(&export_id, "settings").unwrap_err(),
            token_invalid()
        );
        assert!(
            !state.abort_session(&export_id, "settings").unwrap(),
            "跨窗 abort 返回 aborted:false 且不能消费会话"
        );

        assert_eq!(
            state
                .write_chunk("no-such-export", "main", b"x")
                .unwrap_err(),
            token_invalid(),
            "未知 exportId 必须是 TOKEN_INVALID"
        );
        assert!(state.end_session("no-such-export", "main").is_err());
        assert!(!state.abort_session("no-such-export", "main").unwrap());

        // 跨窗尝试没有破坏 owner 会话：owner 仍可写完并 end。
        assert_eq!(state.write_chunk(&export_id, "main", b"ok").unwrap(), 2);
        let (bytes_written, ended_path) = state
            .end_session(&export_id, "main")
            .expect("owner 应可 end");
        assert_eq!(bytes_written, 2);
        assert_eq!(ended_path, path);
        assert_eq!(std::fs::read(&path).unwrap(), b"ok");
    }

    #[test]
    fn chunk_size_bound_is_four_megabytes_after_decode() {
        assert!(chunk_len_allowed(0));
        assert!(chunk_len_allowed(CHUNK_MAX_BYTES));
        assert!(!chunk_len_allowed(CHUNK_MAX_BYTES + 1));
        assert!(!chunk_len_allowed(usize::MAX));

        // base64 解码路径：合法块可解，非法/超长编码显式 INVALID_ARGUMENT。
        assert_eq!(decode_export_chunk("aGVsbG8=").unwrap(), b"hello");
        for malformed in ["not base64!", "你好", "aGVsbG8"] {
            let error = decode_export_chunk(malformed).expect_err("非法 base64 应拒绝");
            assert_eq!(code(&error), "INVALID_ARGUMENT", "{malformed}: {error}");
        }
        let oversized_encoded = "A".repeat(ENCODED_CHUNK_MAX_LEN + 1);
        let error = decode_export_chunk(&oversized_encoded).expect_err("编码超长应先拒绝");
        assert_eq!(code(&error), "INVALID_ARGUMENT");

        // 存储侧同样拒绝解码后超过 4MB 的块，且拒绝的块不落盘、不累计。
        let dir = TempDir::new("bound");
        let state = ObservabilityExportState::default();
        let export_id = begin_session(&state, &dir.file("bound.jsonl"), "main");
        let oversized = vec![b'x'; CHUNK_MAX_BYTES + 1];
        let error = state
            .write_chunk(&export_id, "main", &oversized)
            .expect_err("超过 4MB 必须拒绝");
        assert_eq!(code(&error), "INVALID_ARGUMENT");
        assert_eq!(state.write_chunk(&export_id, "main", b"ok").unwrap(), 2);
        assert!(state.abort_session(&export_id, "main").unwrap());
    }

    #[test]
    fn abort_closes_and_deletes_partial_file() {
        let dir = TempDir::new("abort");
        let path = dir.file("partial.jsonl");
        let state = ObservabilityExportState::default();
        let export_id = begin_session(&state, &path, "main");
        assert_eq!(
            state.write_chunk(&export_id, "main", b"partial").unwrap(),
            7
        );
        assert!(path.exists());

        assert!(state.abort_session(&export_id, "main").unwrap());
        assert!(!path.exists(), "abort 必须删除部分文件");
        assert!(
            !state.abort_session(&export_id, "main").unwrap(),
            "已消费 token 再 abort 是未知 token → aborted:false"
        );

        // 部分文件被外部删除后再 abort 仍算清理成功（NotFound 容忍）。
        let second = begin_session(&state, &path, "main");
        assert!(state.write_chunk(&second, "main", b"partial2").is_ok());
        std::fs::remove_file(&path).expect("外部删除模拟");
        assert!(state.abort_session(&second, "main").unwrap());
        assert!(!path.exists());

        // begin 是 "w" 语义：同路径再次 begin 会截断旧内容。
        let third = begin_session(&state, &path, "main");
        assert_eq!(state.write_chunk(&third, "main", b"fresh").unwrap(), 5);
        let (bytes_written, ended_path) = state.end_session(&third, "main").unwrap();
        assert_eq!(bytes_written, 5);
        assert_eq!(ended_path, path);
        assert_eq!(std::fs::read(&path).unwrap(), b"fresh");
    }

    #[test]
    fn end_flushes_bytes_and_removes_session() {
        let dir = TempDir::new("end");
        let path = dir.file("complete.jsonl");
        let state = ObservabilityExportState::default();
        let export_id = begin_session(&state, &path, "settings");
        assert_eq!(
            state
                .write_chunk(&export_id, "settings", b"line-1\n")
                .unwrap(),
            7
        );
        assert_eq!(
            state
                .write_chunk(&export_id, "settings", b"line-2\n")
                .unwrap(),
            14
        );

        let (bytes_written, ended_path) = state.end_session(&export_id, "settings").unwrap();
        assert_eq!(bytes_written, 14);
        assert_eq!(ended_path, path);
        assert_eq!(std::fs::read(&path).unwrap(), b"line-1\nline-2\n");
        assert!(path.exists(), "end 不能删除已完成导出文件");
        // token 已消费：再次 end 是未知 token。
        assert_eq!(
            state.end_session(&export_id, "settings").unwrap_err(),
            token_invalid()
        );
    }

    #[test]
    fn window_cleanup_drains_only_owner_sessions() {
        let dir = TempDir::new("cleanup");
        let main_path = dir.file("main.jsonl");
        let settings_path = dir.file("settings.jsonl");
        let state = ObservabilityExportState::default();
        let main_id = begin_session(&state, &main_path, "main");
        let settings_id = begin_session(&state, &settings_path, "settings");
        assert_eq!(state.write_chunk(&main_id, "main", b"main").unwrap(), 4);

        let partials = state.drain_window("main").unwrap();
        assert_eq!(partials, vec![main_path.clone()]);
        for path in &partials {
            remove_partial_file(path).unwrap();
        }
        assert!(!main_path.exists(), "窗口销毁必须删除半成品");
        assert_eq!(
            state.write_chunk(&main_id, "main", b"x").unwrap_err(),
            token_invalid(),
            "被清理窗口的 token 立即失效"
        );

        // 其它窗口会话不受影响：继续写、end 正常。
        assert_eq!(
            state
                .write_chunk(&settings_id, "settings", b"still")
                .unwrap(),
            5
        );
        let (bytes_written, ended_path) = state.end_session(&settings_id, "settings").unwrap();
        assert_eq!(bytes_written, 5);
        assert_eq!(ended_path, settings_path);
        assert!(settings_path.exists());
    }
}
