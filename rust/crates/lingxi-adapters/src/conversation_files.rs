//! 现役频道、双份私信和手机文档的兼容文件适配；数据根必须由组合根明确传入。
use crate::storage::{migrations::map_rusqlite, run_store::with_write_txn, RunDatabase};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use lingxi_kernel::ports::StorageError;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf};
use std::{
    io::{Read, Write},
    path::{Component, Path},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMessage {
    pub sender: String,
    pub timestamp: String,
    pub body: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhoneActivity {
    pub timestamp: String,
    pub state: String,
    pub summary: String,
    pub details: Option<Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationDocument {
    pub meta: Value,
    pub messages: Vec<ConversationMessage>,
    pub activities: Vec<PhoneActivity>,
    pub content_sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ConversationMutation {
    CreateChannel {
        id: String,
        meta: Value,
        intro: Option<String>,
        timestamp: String,
    },
    PatchChannel {
        id: String,
        patch: Value,
    },
    DeleteChannel {
        id: String,
    },
    AppendChannel {
        id: String,
        actor: Option<String>,
        sender: String,
        body: String,
        timestamp: String,
    },
    AppendDm {
        from: String,
        to: String,
        body: String,
        timestamp: String,
    },
    Bookmark {
        actor: String,
        channel: String,
        timestamp: Option<String>,
    },
    BookmarkUser {
        channel: String,
        timestamp: Option<String>,
    },
    PhonePatch {
        actor: String,
        conversation: String,
        conversation_type: String,
        patch: Value,
        timestamp: String,
    },
    PhoneReset {
        actor: String,
        conversation: String,
        conversation_type: String,
        visible_after_timestamp: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        visible_after_message_count: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        visible_after_prefix_sha256: Option<String>,
        reset_by: String,
        timestamp: String,
    },
    PhoneActivity {
        actor: String,
        conversation: String,
        conversation_type: String,
        state: String,
        summary: String,
        details: Option<Value>,
        timestamp: String,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileReceiptStatus {
    Prepared,
    Applied,
    Partial,
    Failed,
    Unknown,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileTargetReceipt {
    pub relative_path: String,
    pub status: FileReceiptStatus,
    pub error: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileOperationReceipt {
    pub operation_id: String,
    pub status: FileReceiptStatus,
    pub targets: Vec<FileTargetReceipt>,
}
#[derive(Debug, Clone)]
pub struct ConversationFiles {
    root: PathBuf,
}

const MAX_DOCUMENT_BYTES: usize = 8 * 1024 * 1024;
fn invalid(detail: impl Into<String>) -> StorageError {
    StorageError::InvalidRequest {
        detail: detail.into(),
    }
}
fn io(error: std::io::Error) -> StorageError {
    StorageError::Io {
        detail: format!("conversation file I/O: {error}"),
    }
}
fn conflict(detail: impl Into<String>) -> StorageError {
    StorageError::Conflict {
        detail: detail.into(),
    }
}
fn digest(text: &str) -> String {
    lingxi_protocol::canon::sha256_hex(text.as_bytes())
}
fn identity(id: &str) -> Result<(), StorageError> {
    if id.is_empty()
        || id.len() > 128
        || id.contains("..")
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-.".contains(c))
    {
        Err(invalid("invalid conversation file identity"))
    } else {
        Ok(())
    }
}
fn line(text: &str) -> Result<(), StorageError> {
    if text.len() > 4096 || text.chars().any(char::is_control) {
        Err(invalid("conversation header contains control characters"))
    } else {
        Ok(())
    }
}
fn timestamp(text: &str) -> bool {
    let valid =
        regex::Regex::new(r"^\d{4}-\d{2}-\d{2} \d{2}:\d{2}(:\d{2})?$").expect("fixed regex");
    if !valid.is_match(text) {
        return false;
    }
    let n = |range: std::ops::Range<usize>| text[range].parse::<u32>().unwrap_or(9999);
    let (y, m, d, h, min) = (n(0..4), n(5..7), n(8..10), n(11..13), n(14..16));
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) {
                29
            } else {
                28
            }
        }
        _ => 0,
    };
    y > 0 && d > 0 && d <= days && h < 24 && min < 60 && (text.len() == 16 || n(17..19) < 60)
}
fn raw_parts(content: &str) -> Result<(Vec<String>, String), StorageError> {
    if content.len() > MAX_DOCUMENT_BYTES {
        return Err(invalid("conversation document exceeds 8 MiB"));
    }
    if !content
        .lines()
        .next()
        .is_some_and(|line| line.trim() == "---")
    {
        return Ok((vec![], content.into()));
    }
    let mut offset = content
        .find('\n')
        .ok_or_else(|| invalid("unterminated conversation frontmatter"))?
        + 1;
    let start = offset;
    for part in content[offset..].split_inclusive('\n') {
        if part.trim() == "---" {
            return Ok((
                content[start..offset].lines().map(str::to_owned).collect(),
                content[offset + part.len()..].into(),
            ));
        }
        offset += part.len();
    }
    Err(invalid("unterminated conversation frontmatter"))
}
fn decode_uri(text: &str) -> Result<String, StorageError> {
    let bytes = text.as_bytes();
    let mut result = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err(invalid("damaged phone JSON frontmatter encoding"));
            }
            let s = std::str::from_utf8(&bytes[i + 1..i + 3])
                .map_err(|_| invalid("damaged phone encoding"))?;
            result.push(u8::from_str_radix(s, 16).map_err(|_| invalid("damaged phone encoding"))?);
            i += 3;
        } else {
            result.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(result).map_err(|_| invalid("invalid phone frontmatter UTF-8"))
}
fn encode_uri(text: &str) -> String {
    let mut result = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            result.push(byte as char);
        } else {
            result.push_str(&format!("%{byte:02X}"));
        }
    }
    result
}
fn metadata(lines: &[String], phone: bool) -> Result<Value, StorageError> {
    let mut map = serde_json::Map::new();
    for line in lines {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        let value = value.trim();
        if map.contains_key(key)
            && matches!(
                key,
                "id" | "members"
                    | "peer"
                    | "agentId"
                    | "conversationId"
                    | "conversationType"
                    | "visibleAfterTimestamp"
                    | "visibleAfterMessageCount"
                    | "visibleAfterPrefixSha256"
            )
        {
            return Err(invalid(
                "duplicate authoritative conversation frontmatter key",
            ));
        }
        let value = if phone
            && matches!(
                key,
                "promptSnapshot" | "effectiveModel" | "modelOverrideRequested"
            ) {
            let decoded = decode_uri(value)?;
            serde_json::from_str(&decoded).unwrap_or_else(|_| Value::String(value.into()))
        } else if value.starts_with('[') && value.ends_with(']') {
            Value::Array(
                value[1..value.len() - 1]
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| Value::String(s.into()))
                    .collect(),
            )
        } else {
            Value::String(value.into())
        };
        map.insert(key.into(), value);
    }
    Ok(Value::Object(map))
}
fn parse(content: &str, phone: bool) -> Result<ConversationDocument, StorageError> {
    let (fm, body) = raw_parts(content)?;
    let meta = metadata(&fm, phone)?;
    let pattern = regex::Regex::new(r"^### (.+?) \| (\d{4}-\d{2}-\d{2} \d{2}:\d{2}(?::\d{2})?)$")
        .expect("fixed regex");
    let activity = regex::Regex::new(r"^- ([^\s]+) \[([^\]]+)\] (.*)$").expect("fixed regex");
    let mut messages = Vec::<ConversationMessage>::new();
    let mut current = None::<ConversationMessage>;
    let mut chunks = Vec::new();
    let mut activities = Vec::new();
    for line in body.lines() {
        if let Some(found) = pattern.captures(line) {
            if !timestamp(&found[2]) {
                return Err(invalid("invalid conversation message date"));
            }
            if let Some(mut old) = current.take() {
                old.body = message_body(&chunks.join("\n"))?;
                messages.push(old);
                chunks.clear();
            }
            current = Some(ConversationMessage {
                sender: found[1].into(),
                timestamp: found[2].into(),
                body: String::new(),
            });
        } else if current.is_some() && line.trim() != "---" {
            chunks.push(line);
        }
        if phone {
            if let Some(found) = activity.captures(line) {
                let (summary, details) =
                    if let Some((summary, encoded)) = found[3].split_once(" <!-- details: ") {
                        let encoded = encoded
                            .strip_suffix(" -->")
                            .ok_or_else(|| invalid("damaged phone activity details comment"))?;
                        let details = serde_json::from_str(encoded)
                            .map_err(|_| invalid("damaged phone activity details JSON"))?;
                        (summary.trim().to_owned(), Some(details))
                    } else {
                        (found[3].trim().to_owned(), None)
                    };
                activities.push(PhoneActivity {
                    timestamp: found[1].into(),
                    state: found[2].into(),
                    summary,
                    details,
                });
            }
        }
    }
    if let Some(mut last) = current {
        last.body = message_body(&chunks.join("\n"))?;
        messages.push(last);
    }
    Ok(ConversationDocument {
        meta,
        messages,
        activities,
        content_sha256: digest(content),
    })
}
fn scalar(key: &str, value: &Value, phone: bool) -> Result<String, StorageError> {
    if phone
        && matches!(
            key,
            "promptSnapshot" | "effectiveModel" | "modelOverrideRequested"
        )
    {
        return Ok(encode_uri(&value.to_string()));
    }
    match value {
        Value::String(s) => {
            line(s)?;
            Ok(s.clone())
        }
        Value::Bool(v) => Ok(v.to_string()),
        Value::Number(v) => Ok(v.to_string()),
        Value::Array(items) => {
            let values = items
                .iter()
                .map(|v| {
                    v.as_str()
                        .ok_or_else(|| invalid("conversation metadata array must contain strings"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            for s in &values {
                line(s)?;
                if s.contains([',', '[', ']']) {
                    return Err(invalid("invalid frontmatter array item"));
                }
            }
            Ok(format!("[{}]", values.join(", ")))
        }
        _ => Err(invalid(
            "conversation metadata must use legacy scalar/array format",
        )),
    }
}
fn patch(
    content: &str,
    changes: &Value,
    remove: &[&str],
    phone: bool,
) -> Result<String, StorageError> {
    let (mut fm, body) = raw_parts(content)?;
    metadata(&fm, phone)?;
    let values = changes
        .as_object()
        .ok_or_else(|| invalid("frontmatter patch must be an object"))?;
    for key in remove {
        fm.retain(|line| !line.starts_with(&format!("{key}:")));
    }
    for (key, value) in values {
        if key.is_empty() || key.contains([':', '\n', '\r']) {
            return Err(invalid("invalid frontmatter key"));
        }
        let prefix = format!("{key}:");
        if value.is_null() {
            fm.retain(|line| !line.starts_with(&prefix));
            continue;
        }
        let replacement = format!("{key}: {}", scalar(key, value, phone)?);
        if let Some(index) = fm.iter().position(|line| line.starts_with(&prefix)) {
            fm[index] = replacement;
        } else {
            fm.push(replacement);
        }
    }
    Ok(format!("---\n{}\n---\n{body}", fm.join("\n")))
}
fn block(sender: &str, body: &str, time: &str) -> Result<String, StorageError> {
    line(sender)?;
    if sender.trim().is_empty()
        || sender.contains(" | ")
        || !timestamp(time)
        || body.trim().is_empty()
        || body.len() > 100_000
        || body.contains('\0')
    {
        return Err(invalid("invalid conversation message"));
    }
    let header = regex::Regex::new(r"^### (.+?) \| (\d{4}-\d{2}-\d{2} \d{2}:\d{2}(?::\d{2})?)$")
        .expect("fixed regex");
    // 仅有歧义的正文使用隐藏封套；旧历史和普通消息字节形状不改，显示仍为原 Markdown。
    let visible = if body != body.trim()
        || body
            .lines()
            .any(|line| header.is_match(line) || line.trim() == "---")
        || body.trim_start().starts_with("<!-- lingxi-body-v1:")
    {
        format!(
            "<!-- lingxi-body-v1:{} -->\n{}",
            STANDARD.encode(body.as_bytes()),
            escape_headers(body.trim())
        )
    } else {
        body.trim().into()
    };
    Ok(format!("\n### {sender} | {time}\n\n{visible}\n\n---\n"))
}
fn escape_headers(body: &str) -> String {
    let header = regex::Regex::new(r"^### (.+?) \| (\d{4}-\d{2}-\d{2} \d{2}:\d{2}(?::\d{2})?)$")
        .expect("fixed regex");
    body.lines()
        .map(|line| {
            if header.is_match(line) || line.trim() == "---" {
                format!("\\{line}")
            } else {
                line.into()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
fn message_body(raw: &str) -> Result<String, StorageError> {
    let body = raw.trim();
    if let Some(rest) = body.strip_prefix("<!-- lingxi-body-v1:") {
        let (encoded, visible) = rest
            .split_once(" -->\n")
            .ok_or_else(|| invalid("damaged conversation body envelope"))?;
        if encoded.len() > 140_000 {
            return Err(invalid("conversation body envelope exceeds bound"));
        }
        let bytes = STANDARD
            .decode(encoded)
            .map_err(|_| invalid("damaged conversation body envelope"))?;
        let original = String::from_utf8(bytes)
            .map_err(|_| invalid("conversation body envelope is not UTF-8"))?;
        if original.len() > 100_000 || escape_headers(original.trim()) != visible {
            return Err(invalid(
                "conversation body envelope differs from visible content",
            ));
        }
        return Ok(original);
    }
    Ok(body.into())
}
fn phone_stem(conversation: &str) -> Result<String, StorageError> {
    let conversation = conversation.trim();
    line(conversation)?;
    if conversation.trim().is_empty() {
        return Err(invalid("phone conversation identity missing"));
    }
    let readable = regex::Regex::new(r"[^a-zA-Z0-9._-]+")
        .expect("fixed regex")
        .replace_all(conversation, "_")
        .trim_matches('_')
        .chars()
        .take(80)
        .collect::<String>();
    Ok(format!(
        "{}-{}",
        if readable.is_empty() {
            "conversation"
        } else {
            &readable
        },
        &digest(conversation)[..8]
    ))
}
fn bookmarks(content: &str) -> BTreeMap<String, String> {
    let pattern = regex::Regex::new(r"^- (.+?) \(last: (.+?)\)$").expect("fixed regex");
    content
        .lines()
        .filter_map(|line| {
            pattern
                .captures(line)
                .map(|c| (c[1].to_owned(), c[2].to_owned()))
        })
        .collect()
}
fn bookmark_text(content: &str, channel: &str, time: Option<&str>) -> Result<String, StorageError> {
    identity(channel)?;
    if let Some(time) = time {
        if time != "never" && !timestamp(time) {
            return Err(invalid("invalid channel bookmark timestamp"));
        }
    }
    let mut lines: Vec<_> = content.lines().map(str::to_owned).collect();
    let prefix = format!("- {channel} (last: ");
    let index = lines
        .iter()
        .position(|line| line.starts_with(&prefix) && line.ends_with(')'));
    match (index, time) {
        (Some(index), Some(time)) => lines[index] = format!("- {channel} (last: {time})"),
        (Some(index), None) => {
            lines.remove(index);
        }
        (None, Some(time)) => {
            if lines.is_empty() {
                lines.extend(["# 频道".into(), String::new()]);
            }
            lines.push(format!("- {channel} (last: {time})"));
        }
        _ => {}
    }
    Ok(format!("{}\n", lines.join("\n")))
}
fn members(meta: &Value) -> Result<Vec<String>, StorageError> {
    let values = meta["members"]
        .as_array()
        .ok_or_else(|| invalid("channel members frontmatter is missing or damaged"))?;
    let mut result = Vec::new();
    for value in values {
        let id = value
            .as_str()
            .ok_or_else(|| invalid("channel member must be an ID"))?;
        identity(id)?;
        if !result.contains(&id.to_owned()) {
            result.push(id.into());
        }
    }
    if result.len() > 64 {
        return Err(invalid("channel member count exceeds bound"));
    }
    Ok(result)
}

#[cfg(unix)]
mod bounded_fs {
    use super::*;
    use std::{
        ffi::{CStr, CString},
        fs::{File, OpenOptions},
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::fs::OpenOptionsExt,
        },
    };
    fn relative(path: &str) -> std::io::Result<Vec<&str>> {
        let parts = Path::new(path).components();
        let mut values = Vec::new();
        for part in parts {
            match part {
                Component::Normal(s) => values.push(
                    s.to_str()
                        .ok_or_else(|| std::io::Error::other("non UTF-8 conversation path"))?,
                ),
                _ => return Err(std::io::Error::other("invalid relative conversation path")),
            }
        }
        if values.is_empty() {
            return Err(std::io::Error::other("empty conversation path"));
        }
        Ok(values)
    }
    fn parent(root: &Path, path: &str, create: bool) -> std::io::Result<(File, CString)> {
        let parts = relative(path)?;
        let mut dir = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(root)?;
        for part in &parts[..parts.len() - 1] {
            let name = CString::new(*part).map_err(std::io::Error::other)?;
            let mut fd = unsafe {
                libc::openat(
                    dir.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            if fd < 0
                && create
                && std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound
            {
                let made = unsafe { libc::mkdirat(dir.as_raw_fd(), name.as_ptr(), 0o700) };
                if made < 0
                    && std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists
                {
                    return Err(std::io::Error::last_os_error());
                }
                fd = unsafe {
                    libc::openat(
                        dir.as_raw_fd(),
                        name.as_ptr(),
                        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                    )
                };
            }
            if fd < 0 {
                return Err(std::io::Error::last_os_error());
            }
            dir = unsafe { File::from_raw_fd(fd) };
        }
        Ok((
            dir,
            CString::new(*parts.last().expect("nonempty")).map_err(std::io::Error::other)?,
        ))
    }
    fn read_at(dir: &File, name: &CStr) -> std::io::Result<Option<String>> {
        let fd = unsafe {
            libc::openat(
                dir.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            let error = std::io::Error::last_os_error();
            return if error.kind() == std::io::ErrorKind::NotFound {
                Ok(None)
            } else {
                Err(error)
            };
        }
        let file = unsafe { File::from_raw_fd(fd) };
        if !file.metadata()?.is_file() {
            return Err(std::io::Error::other(
                "conversation target is not a regular file",
            ));
        }
        let mut bytes = Vec::new();
        file.take((MAX_DOCUMENT_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_DOCUMENT_BYTES {
            return Err(std::io::Error::other("conversation exceeds 8 MiB"));
        }
        String::from_utf8(bytes)
            .map(Some)
            .map_err(std::io::Error::other)
    }
    pub fn read(root: &Path, path: &str) -> std::io::Result<Option<String>> {
        match parent(root, path, false) {
            Ok((dir, name)) => read_at(&dir, &name),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }
    pub fn directory(root: &Path, path: &str) -> std::io::Result<()> {
        let (dir, _) = parent(root, &format!("{path}/.probe"), false)?;
        if !dir.metadata()?.is_dir() {
            return Err(std::io::Error::other("agent directory missing"));
        }
        Ok(())
    }
    pub fn replace(
        root: &Path,
        path: &str,
        pre: Option<&str>,
        post: Option<&str>,
    ) -> std::io::Result<()> {
        let (dir, name) = parent(root, path, post.is_some())?;
        let current = read_at(&dir, &name)?;
        if current.as_deref().map(digest).as_deref() != pre {
            return Err(std::io::Error::other("conversation preimage changed"));
        }
        if let Some(post) = post {
            let mut random = [0u8; 16];
            getrandom::getrandom(&mut random).map_err(|e| std::io::Error::other(e.to_string()))?;
            let tmp = CString::new(format!(
                ".lingxi-conversation-{}",
                random
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
            ))
            .expect("safe generated name");
            let fd = unsafe {
                libc::openat(
                    dir.as_raw_fd(),
                    tmp.as_ptr(),
                    libc::O_WRONLY
                        | libc::O_CREAT
                        | libc::O_EXCL
                        | libc::O_NOFOLLOW
                        | libc::O_CLOEXEC,
                    0o600,
                )
            };
            if fd < 0 {
                return Err(std::io::Error::last_os_error());
            }
            let mut file = unsafe { File::from_raw_fd(fd) };
            let result = (|| {
                file.write_all(post.as_bytes())?;
                file.sync_all()?;
                let fresh = read_at(&dir, &name)?;
                if fresh.as_deref().map(digest).as_deref() != pre {
                    return Err(std::io::Error::other(
                        "conversation changed before atomic replace",
                    ));
                }
                if unsafe {
                    libc::renameat(
                        dir.as_raw_fd(),
                        tmp.as_ptr(),
                        dir.as_raw_fd(),
                        name.as_ptr(),
                    )
                } < 0
                {
                    return Err(std::io::Error::last_os_error());
                }
                dir.sync_all()
            })();
            if result.is_err() {
                unsafe { libc::unlinkat(dir.as_raw_fd(), tmp.as_ptr(), 0) };
            }
            result
        } else {
            if unsafe { libc::unlinkat(dir.as_raw_fd(), name.as_ptr(), 0) } < 0 {
                return Err(std::io::Error::last_os_error());
            }
            dir.sync_all()
        }
    }
    pub fn list(root: &Path, path: &str) -> std::io::Result<Vec<String>> {
        let (dir, _) = parent(root, &format!("{path}/.probe"), false)?;
        let duplicate = unsafe { libc::dup(dir.as_raw_fd()) };
        if duplicate < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let stream = unsafe { libc::fdopendir(duplicate) };
        if stream.is_null() {
            unsafe { libc::close(duplicate) };
            return Err(std::io::Error::last_os_error());
        }
        let mut names = Vec::new();
        loop {
            let entry = unsafe { libc::readdir(stream) };
            if entry.is_null() {
                break;
            }
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }
                .to_str()
                .map(str::to_owned)
                .map_err(std::io::Error::other);
            match name {
                Ok(name) if name != "." && name != ".." => names.push(name),
                Ok(_) => {}
                Err(error) => {
                    unsafe { libc::closedir(stream) };
                    return Err(error);
                }
            }
        }
        unsafe { libc::closedir(stream) };
        names.sort();
        Ok(names)
    }
}

struct Plan {
    path: String,
    pre: Option<String>,
    post: Option<String>,
}
fn file_wire(status: FileReceiptStatus) -> String {
    serde_json::to_value(status)
        .expect("enum")
        .as_str()
        .expect("enum string")
        .into()
}
fn read_optional(root: &Path, path: &str) -> Result<Option<String>, StorageError> {
    bounded_fs::read(root, path).map_err(io)
}
fn read_required(root: &Path, path: &str) -> Result<String, StorageError> {
    read_optional(root, path)?.ok_or_else(|| invalid(format!("conversation file missing: {path}")))
}
fn actor(root: &Path, id: &str) -> Result<(), StorageError> {
    identity(id)?;
    bounded_fs::directory(root, &format!("agents/{id}")).map_err(io)
}
fn channel_path(id: &str) -> Result<String, StorageError> {
    identity(id)?;
    Ok(format!("channels/{id}.md"))
}
fn phone_path(actor: &str, conversation: &str) -> Result<String, StorageError> {
    identity(actor)?;
    Ok(format!(
        "agents/{actor}/phone/conversations/{}.md",
        phone_stem(conversation)?
    ))
}
fn add_plan(
    root: &Path,
    plans: &mut Vec<Plan>,
    path: String,
    post: Option<String>,
) -> Result<(), StorageError> {
    if post.as_ref().is_some_and(|s| s.len() > MAX_DOCUMENT_BYTES) {
        return Err(invalid("conversation postimage exceeds 8 MiB"));
    }
    if let Some(old) = plans.iter_mut().find(|old| old.path == path) {
        old.post = post;
        return Ok(());
    }
    plans.push(Plan {
        pre: read_optional(root, &path)?,
        path,
        post,
    });
    Ok(())
}
fn add_bookmark(
    root: &Path,
    plans: &mut Vec<Plan>,
    who: &str,
    id: &str,
    time: Option<&str>,
) -> Result<(), StorageError> {
    actor(root, who)?;
    let path = format!("agents/{who}/channels.md");
    let current = plans
        .iter()
        .find(|plan| plan.path == path)
        .and_then(|plan| plan.post.clone())
        .or(read_optional(root, &path)?)
        .unwrap_or_default();
    add_plan(root, plans, path, Some(bookmark_text(&current, id, time)?))
}
fn ensure_phone(
    root: &Path,
    who: &str,
    conversation: &str,
    kind: &str,
    time: &str,
) -> Result<(String, String), StorageError> {
    actor(root, who)?;
    line(time)?;
    if !matches!(kind, "channel" | "dm") {
        return Err(invalid("invalid phone conversation type"));
    }
    let path = phone_path(who, conversation)?;
    let content = if let Some(content) = read_optional(root, &path)? {
        let doc = parse(&content, true)?;
        if doc.meta["agentId"] != who
            || doc.meta["conversationId"] != conversation
            || doc.meta["conversationType"] != kind
        {
            return Err(conflict(
                "phone projection identity differs from requested scope",
            ));
        }
        content
    } else {
        patch(
            &format!("# Agent Phone: {kind} {conversation}\n\n## Activity\n"),
            &serde_json::json!({"agentId":who,"conversationId":conversation,"conversationType":kind,"state":"idle","summary":"idle","createdAt":time,"updatedAt":time}),
            &[],
            true,
        )?
    };
    Ok((path, content))
}
fn plans(root: &Path, mutation: &ConversationMutation) -> Result<Vec<Plan>, StorageError> {
    let mut result = Vec::new();
    match mutation {
        ConversationMutation::CreateChannel {
            id,
            meta,
            intro,
            timestamp: time,
        } => {
            if !timestamp(time) {
                return Err(invalid("invalid channel creation timestamp"));
            }
            let id = if id.starts_with("ch_") {
                id.clone()
            } else {
                format!("ch_{id}")
            };
            let path = channel_path(&id)?;
            if read_optional(root, &path)?.is_some() {
                return Err(conflict("channel already exists"));
            }
            let mut meta = meta.clone();
            if !meta.is_object() {
                return Err(invalid("channel metadata must be an object"));
            }
            meta["id"] = Value::String(id.clone());
            let peers = members(&meta)?;
            if peers.len() < 2 {
                return Err(invalid("channel requires at least two agent members"));
            }
            for peer in &peers {
                actor(root, peer)?;
            }
            let mut content = patch("", &meta, &[], false)?;
            if let Some(intro) = intro {
                if !intro.trim().is_empty() {
                    content.push_str(&block("system", intro, time)?);
                }
            }
            add_plan(root, &mut result, path, Some(content))?;
            for peer in peers {
                add_bookmark(root, &mut result, &peer, &id, Some("never"))?;
            }
        }
        ConversationMutation::PatchChannel { id, patch: changes } => {
            let path = channel_path(id)?;
            let content = read_required(root, &path)?;
            let old = members(&parse(&content, false)?.meta)?;
            if changes
                .get("id")
                .is_some_and(|value| value.as_str() != Some(id))
            {
                return Err(invalid("channel ID cannot change through metadata"));
            }
            let post = patch(&content, changes, &[], false)?;
            let next = members(&parse(&post, false)?.meta)?;
            for peer in &next {
                actor(root, peer)?;
            }
            add_plan(root, &mut result, path, Some(post))?;
            for peer in next.iter().filter(|peer| !old.contains(peer)) {
                add_bookmark(root, &mut result, peer, id, Some("never"))?;
            }
            for peer in old.iter().filter(|peer| !next.contains(peer)) {
                add_bookmark(root, &mut result, peer, id, None)?;
            }
        }
        ConversationMutation::DeleteChannel { id } => {
            let path = channel_path(id)?;
            if let Some(content) = read_optional(root, &path)? {
                members(&parse(&content, false)?.meta)?;
                add_plan(root, &mut result, path, None)?;
                // 删除时清掉真实助手目录中的旧书签；成员曾被移出或删除也不会留下悬空条目。
                let peers = match bounded_fs::list(root, "agents") {
                    Ok(v) => v,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
                    Err(e) => return Err(io(e)),
                };
                for peer in peers {
                    if identity(&peer).is_err() {
                        continue;
                    }
                    match bounded_fs::directory(root, &format!("agents/{peer}")) {
                        Ok(()) => {}
                        Err(e)
                            if matches!(
                                e.kind(),
                                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                            ) =>
                        {
                            continue
                        }
                        Err(e) => return Err(io(e)),
                    }
                    let path = format!("agents/{peer}/channels.md");
                    if let Some(content) = read_optional(root, &path)? {
                        if bookmarks(&content).contains_key(id) {
                            add_plan(
                                root,
                                &mut result,
                                path,
                                Some(bookmark_text(&content, id, None)?),
                            )?;
                        }
                    }
                }
                if let Some(content) = read_optional(root, "user/channel-bookmarks.md")? {
                    add_plan(
                        root,
                        &mut result,
                        "user/channel-bookmarks.md".into(),
                        Some(bookmark_text(&content, id, None)?),
                    )?;
                }
            }
        }
        ConversationMutation::AppendChannel {
            id,
            actor: who,
            sender,
            body,
            timestamp: time,
        } => {
            let path = channel_path(id)?;
            let mut content = read_required(root, &path)?;
            let peers = members(&parse(&content, false)?.meta)?;
            if let Some(who) = who {
                actor(root, who)?;
                if !peers.contains(who) || sender != who {
                    return Err(conflict("channel reply actor is not a current member"));
                }
            }
            content.push_str(&block(sender, body, time)?);
            add_plan(root, &mut result, path, Some(content))?;
        }
        ConversationMutation::AppendDm {
            from,
            to,
            body,
            timestamp: time,
        } => {
            actor(root, from)?;
            actor(root, to)?;
            if from == to {
                return Err(invalid("cannot send DM to self"));
            }
            let block = block(from, body, time)?;
            for (owner, peer) in [(from, to), (to, from)] {
                let path = format!("agents/{owner}/dm/{peer}.md");
                let mut content = read_optional(root, &path)?
                    .unwrap_or_else(|| format!("---\npeer: {peer}\n---\n"));
                let doc = parse(&content, false)?;
                if doc.meta["peer"] != peer.as_str() {
                    return Err(conflict("DM file peer identity mismatch"));
                }
                content.push_str(&block);
                add_plan(root, &mut result, path, Some(content))?;
            }
        }
        ConversationMutation::Bookmark {
            actor: who,
            channel,
            timestamp: time,
        } => add_bookmark(root, &mut result, who, channel, time.as_deref())?,
        ConversationMutation::BookmarkUser { channel, timestamp } => {
            let current = read_optional(root, "user/channel-bookmarks.md")?.unwrap_or_default();
            add_plan(
                root,
                &mut result,
                "user/channel-bookmarks.md".into(),
                Some(bookmark_text(&current, channel, timestamp.as_deref())?),
            )?;
        }
        ConversationMutation::PhonePatch {
            actor: who,
            conversation,
            conversation_type,
            patch: changes,
            timestamp: time,
        } => {
            let (path, content) = ensure_phone(root, who, conversation, conversation_type, time)?;
            if changes.as_object().is_none() {
                return Err(invalid("phone patch must be an object"));
            }
            for (key, expected) in [
                ("agentId", who),
                ("conversationId", conversation),
                ("conversationType", conversation_type),
            ] {
                if changes
                    .get(key)
                    .is_some_and(|value| value.as_str() != Some(expected))
                {
                    return Err(conflict("phone patch cannot replace its identity"));
                }
            }
            let mut changes = changes.clone();
            changes["updatedAt"] = Value::String(time.clone());
            add_plan(
                root,
                &mut result,
                path,
                Some(patch(&content, &changes, &[], true)?),
            )?;
        }
        ConversationMutation::PhoneReset {
            actor: who,
            conversation,
            conversation_type,
            visible_after_timestamp,
            visible_after_message_count,
            visible_after_prefix_sha256,
            reset_by,
            timestamp: time,
        } => {
            if !visible_after_timestamp.is_empty() && !timestamp(visible_after_timestamp) {
                return Err(invalid("invalid phone visible-after boundary"));
            }
            line(reset_by)?;
            let (path, content) = ensure_phone(root, who, conversation, conversation_type, time)?;
            let mut changes = serde_json::json!({"visibleAfterTimestamp":visible_after_timestamp,"resetAt":time,"resetBy":reset_by,"updatedAt":time});
            if visible_after_message_count.is_some() != visible_after_prefix_sha256.is_some() {
                return Err(invalid(
                    "phone reset count and prefix digest must be paired",
                ));
            }
            if let (Some(count), Some(sha)) =
                (visible_after_message_count, visible_after_prefix_sha256)
            {
                if sha.len() != 64 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Err(invalid("invalid phone reset prefix digest"));
                }
                changes["visibleAfterMessageCount"] = serde_json::json!(count);
                changes["visibleAfterPrefixSha256"] = serde_json::json!(sha);
            }
            let post = patch(
                &content,
                &changes,
                &[
                    "phoneSessionFile",
                    "promptSnapshot",
                    "toolNames",
                    "lastRefreshedDate",
                    "lastPhoneSessionUsedAt",
                    "phoneSessionStartedAt",
                ],
                true,
            )?;
            add_plan(root, &mut result, path, Some(post))?;
        }
        ConversationMutation::PhoneActivity {
            actor: who,
            conversation,
            conversation_type,
            state,
            summary,
            details,
            timestamp: time,
        } => {
            line(state)?;
            line(summary)?;
            if state.is_empty() || time.contains(' ') {
                return Err(invalid("invalid phone activity"));
            }
            let (path, content) = ensure_phone(root, who, conversation, conversation_type, time)?;
            let mut changes = serde_json::json!({"state":state,"summary":summary,"updatedAt":time});
            if let Some(last) = details
                .as_ref()
                .and_then(|value| value.get("lastMessageTimestamp"))
            {
                changes["lastViewedTimestamp"] = last.clone();
            }
            let mut post = patch(&content, &changes, &[], true)?;
            let detail = details
                .as_ref()
                .map(|value| format!(" <!-- details: {value} -->"))
                .unwrap_or_default();
            post.push_str(&format!("- {time} [{state}] {summary}{detail}\n"));
            add_plan(root, &mut result, path, Some(post))?;
        }
    }
    Ok(result)
}
fn receipt_conn(
    conn: &rusqlite::Connection,
    id: &str,
) -> Result<Option<FileOperationReceipt>, StorageError> {
    let status: Option<String> = conn
        .query_row(
            "SELECT status FROM conversation_file_operations_v1 WHERE operation_id=?1",
            [id],
            |row| row.get(0),
        )
        .optional()
        .map_err(map_rusqlite)?;
    let Some(status) = status else {
        return Ok(None);
    };
    let parse = |value: String| {
        serde_json::from_str::<FileReceiptStatus>(&format!("\"{value}\"")).map_err(|_| {
            StorageError::Corrupted {
                detail: "conversation file receipt status damaged".into(),
            }
        })
    };
    let mut stmt=conn.prepare("SELECT relative_path,status,error FROM conversation_file_targets_v1 WHERE operation_id=?1 ORDER BY rowid").map_err(map_rusqlite)?;
    let rows = stmt
        .query_map([id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(map_rusqlite)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_rusqlite)?;
    let mut targets = Vec::new();
    for (path, status, error) in rows {
        targets.push(FileTargetReceipt {
            relative_path: path,
            status: parse(status)?,
            error,
        });
    }
    Ok(Some(FileOperationReceipt {
        operation_id: id.into(),
        status: parse(status)?,
        targets,
    }))
}
pub async fn get_operation(
    db: &RunDatabase,
    operation_id: &str,
) -> Result<Option<FileOperationReceipt>, StorageError> {
    let id = operation_id.to_owned();
    db.queue().submit(move |conn| receipt_conn(conn, &id)).await
}
pub async fn operation_mutation(
    db: &RunDatabase,
    operation_id: &str,
) -> Result<Option<ConversationMutation>, StorageError> {
    let id = operation_id.to_owned();
    db.queue().submit(move|conn|{let raw:Option<String>=conn.query_row("SELECT mutation_json FROM conversation_file_operations_v1 WHERE operation_id=?1",[id],|r|r.get(0)).optional().map_err(map_rusqlite)?;raw.map(|raw|serde_json::from_str(&raw).map_err(|_|StorageError::Corrupted{detail:"frozen conversation mutation damaged".into()})).transpose()}).await
}
fn prepare_conn(
    conn: &rusqlite::Connection,
    root: &Path,
    id: &str,
    mutation: &ConversationMutation,
    now: i64,
    busy: u64,
) -> Result<FileOperationReceipt, StorageError> {
    let input = serde_json::to_string(mutation).map_err(|e| invalid(e.to_string()))?;
    let sha = lingxi_protocol::canon::canonical_sha256_hex(
        &serde_json::to_value(mutation).map_err(|e| invalid(e.to_string()))?,
    );
    with_write_txn(conn, busy, |conn| {
        let old: Option<String> = conn
            .query_row(
                "SELECT request_sha256 FROM conversation_file_operations_v1 WHERE operation_id=?1",
                [id],
                |r| r.get(0),
            )
            .optional()
            .map_err(map_rusqlite)?;
        if let Some(old) = old {
            if old != sha {
                return Err(conflict(
                    "conversation operation ID binds a different mutation",
                ));
            }
            return receipt_conn(conn, id)?
                .ok_or_else(|| invalid("conversation receipt disappeared"));
        }
        let targets = plans(root, mutation)?;
        // 重置代次和文件意图共用一笔数据库事务，重启补文件时不会复活旧绑定。
        if let ConversationMutation::PhoneReset {
            actor,
            conversation,
            ..
        } = mutation
        {
            let owner = lingxi_kernel::LOCAL_OWNER_SUBJECT;
            let prior:Option<i64>=conn.query_row("SELECT epoch FROM phone_binding_epochs_v1 WHERE actor_id=?1 AND conversation_id=?2 AND owner_subject=?3",params![actor,conversation,owner],|r|r.get(0)).optional().map_err(map_rusqlite)?;
            let next = prior
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| invalid("phone reset epoch exhausted"))?;
            conn.execute("DELETE FROM phone_bindings_v1 WHERE actor_id=?1 AND conversation_id=?2 AND owner_subject=?3",params![actor,conversation,owner]).map_err(map_rusqlite)?;
            conn.execute("INSERT INTO phone_binding_epochs_v1 VALUES(?1,?2,?3,?4) ON CONFLICT(actor_id,conversation_id,owner_subject) DO UPDATE SET epoch=excluded.epoch",params![actor,conversation,owner,next]).map_err(map_rusqlite)?;
        }
        conn.execute(
            "INSERT INTO conversation_file_operations_v1 VALUES(?1,?2,?3,'prepared',?4,?4)",
            params![id, sha, input, now],
        )
        .map_err(map_rusqlite)?;
        for target in targets {
            conn.execute(
                "INSERT INTO conversation_file_targets_v1 VALUES(?1,?2,?3,?4,?5,'prepared',NULL)",
                params![
                    id,
                    target.path,
                    target.pre.as_deref().map(digest),
                    target.post.as_deref().map(digest),
                    target.post
                ],
            )
            .map_err(map_rusqlite)?;
        }
        receipt_conn(conn, id)?.ok_or_else(|| invalid("conversation preparation receipt missing"))
    })
}
fn aggregate(receipt: &FileOperationReceipt) -> FileReceiptStatus {
    let applied = receipt
        .targets
        .iter()
        .filter(|target| target.status == FileReceiptStatus::Applied)
        .count();
    if applied == receipt.targets.len() {
        FileReceiptStatus::Applied
    } else if applied > 0 {
        FileReceiptStatus::Partial
    } else if receipt
        .targets
        .iter()
        .any(|target| target.status == FileReceiptStatus::Unknown)
    {
        FileReceiptStatus::Unknown
    } else if receipt
        .targets
        .iter()
        .any(|target| target.status == FileReceiptStatus::Failed)
    {
        FileReceiptStatus::Failed
    } else {
        FileReceiptStatus::Prepared
    }
}
fn relative_allowed(path: &str) -> bool {
    let parts: Vec<_> = path.split('/').collect();
    let valid = |name: &str| {
        name.strip_suffix(".md")
            .is_some_and(|id| identity(id).is_ok())
    };
    match parts.as_slice() {
        ["channels", name] => valid(name),
        ["user", "channel-bookmarks.md"] => true,
        ["agents", who, "channels.md"] => identity(who).is_ok(),
        ["agents", who, "dm", name] => identity(who).is_ok() && valid(name),
        ["agents", who, "phone", "conversations", name] => identity(who).is_ok() && valid(name),
        _ => false,
    }
}
fn resume_conn(
    conn: &rusqlite::Connection,
    root: &Path,
    id: &str,
    now: i64,
    busy: u64,
    recover: bool,
) -> Result<FileOperationReceipt, StorageError> {
    if let Some(receipt) = receipt_conn(conn, id)? {
        if receipt.status == FileReceiptStatus::Applied {
            return Ok(receipt);
        }
    }
    let raw: String = conn
        .query_row(
            "SELECT mutation_json FROM conversation_file_operations_v1 WHERE operation_id=?1",
            [id],
            |r| r.get(0),
        )
        .map_err(map_rusqlite)?;
    let mutation: ConversationMutation =
        serde_json::from_str(&raw).map_err(|_| StorageError::Corrupted {
            detail: "conversation operation mutation damaged".into(),
        })?;
    // 授权不从旧计划继承：真实频道文件发生成员撤回后，不能借旧收据补发。
    let authority = if let ConversationMutation::AppendChannel {
        id,
        actor: Some(who),
        sender,
        ..
    } = &mutation
    {
        match read_optional(root, &channel_path(id)?) {
            Ok(None) => Some((
                FileReceiptStatus::Failed,
                "channel removed before prepared message".to_owned(),
            )),
            Ok(Some(content)) => match parse(&content, false).and_then(|doc| members(&doc.meta)) {
                Ok(peers) if peers.contains(who) && sender == who => None,
                Ok(_) => Some((
                    FileReceiptStatus::Failed,
                    "channel member authority revoked".to_owned(),
                )),
                Err(error) => Some((
                    FileReceiptStatus::Unknown,
                    format!("channel authority cannot be verified: {error}"),
                )),
            },
            Err(error) => Some((
                FileReceiptStatus::Unknown,
                format!("channel authority cannot be read: {error}"),
            )),
        }
    } else {
        None
    };
    let mut stmt=conn.prepare("SELECT relative_path,preimage_sha256,postimage_sha256,postimage,status FROM conversation_file_targets_v1 WHERE operation_id=?1 ORDER BY rowid").map_err(map_rusqlite)?;
    let targets = stmt
        .query_map([id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, String>(4)?,
            ))
        })
        .map_err(map_rusqlite)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_rusqlite)?;
    drop(stmt);
    for (path, pre, post_sha, post, status) in targets {
        if status == "applied" || status == "unknown" || (recover && status == "failed") {
            continue;
        }
        if !relative_allowed(&path) || post.as_deref().map(digest) != post_sha {
            return Err(StorageError::Corrupted {
                detail: "conversation file target postimage or path damaged".into(),
            });
        }
        let (status, error) = if let Some((status, error)) = &authority {
            (*status, Some(error.clone()))
        } else {
            match read_optional(root, &path) {
                Ok(current) if current.as_deref().map(digest) == post_sha => {
                    (FileReceiptStatus::Applied, None)
                }
                Ok(current) if current.as_deref().map(digest) == pre => {
                    match bounded_fs::replace(root, &path, pre.as_deref(), post.as_deref()) {
                        Ok(()) => (FileReceiptStatus::Applied, None),
                        Err(error) => {
                            let unchanged = read_optional(root, &path)
                                .ok()
                                .is_some_and(|current| current.as_deref().map(digest) == pre);
                            (
                                if unchanged {
                                    FileReceiptStatus::Failed
                                } else {
                                    FileReceiptStatus::Unknown
                                },
                                Some(error.to_string()),
                            )
                        }
                    }
                }
                Ok(_) => (
                    FileReceiptStatus::Unknown,
                    Some("conversation pre/postimage differs; refusing blind replay".into()),
                ),
                Err(error) => (FileReceiptStatus::Unknown, Some(error.to_string())),
            }
        };
        with_write_txn(conn, busy, |conn| {
            conn.execute("UPDATE conversation_file_targets_v1 SET status=?3,error=?4 WHERE operation_id=?1 AND relative_path=?2",params![id,path,file_wire(status),error]).map_err(map_rusqlite)?;
            Ok(())
        })?;
    }
    let mut receipt = receipt_conn(conn, id)?
        .ok_or_else(|| invalid("conversation receipt missing after file write"))?;
    receipt.status = aggregate(&receipt);
    with_write_txn(conn, busy, |conn| {
        conn.execute("UPDATE conversation_file_operations_v1 SET status=?2,updated_at=?3 WHERE operation_id=?1",params![id,file_wire(receipt.status),now]).map_err(map_rusqlite)?;
        Ok(())
    })?;
    Ok(receipt)
}
impl ConversationFiles {
    pub fn new(root: PathBuf) -> Result<Self, StorageError> {
        if !root.is_absolute() {
            return Err(invalid(
                "conversation data root must be explicit and absolute",
            ));
        }
        let root = root.canonicalize().map_err(io)?;
        if !root.is_dir() {
            return Err(invalid("conversation data root is not a directory"));
        }
        Ok(Self { root })
    }
    pub fn read_channel(&self, id: &str) -> Result<ConversationDocument, StorageError> {
        let mut document = parse(&read_required(&self.root, &channel_path(id)?)?, false)?;
        // 现役频道身份由真实文件名决定；兼容缺 id 或重命名的旧文件，原字节保持不变。
        document.meta["id"] = Value::String(id.into());
        Ok(document)
    }
    pub fn list_channels(&self) -> Result<Vec<ConversationDocument>, StorageError> {
        let names = match bounded_fs::list(&self.root, "channels") {
            Ok(names) => names,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(io(error)),
        };
        names
            .into_iter()
            .filter(|name| name.ends_with(".md"))
            .map(|name| self.read_channel(name.strip_suffix(".md").expect("checked")))
            .collect()
    }
    pub fn channel_membership(&self, id: &str) -> Result<(Vec<String>, String), StorageError> {
        let doc = self.read_channel(id)?;
        let values = members(&doc.meta)?;
        let sha = lingxi_protocol::canon::canonical_sha256_hex(
            &serde_json::to_value(&values).map_err(|e| invalid(e.to_string()))?,
        );
        Ok((values, sha))
    }
    pub fn read_dm(
        &self,
        actor_id: &str,
        peer: &str,
    ) -> Result<ConversationDocument, StorageError> {
        identity(actor_id)?;
        identity(peer)?;
        let doc = parse(
            &read_required(&self.root, &format!("agents/{actor_id}/dm/{peer}.md"))?,
            false,
        )?;
        if doc.meta["peer"] != peer {
            return Err(conflict("DM peer metadata differs from file identity"));
        }
        Ok(doc)
    }
    pub fn read_user_bookmarks(&self) -> Result<BTreeMap<String, String>, StorageError> {
        Ok(bookmarks(
            &read_optional(&self.root, "user/channel-bookmarks.md")?.unwrap_or_default(),
        ))
    }
    pub fn read_bookmarks(&self, actor_id: &str) -> Result<BTreeMap<String, String>, StorageError> {
        identity(actor_id)?;
        Ok(bookmarks(
            &read_optional(&self.root, &format!("agents/{actor_id}/channels.md"))?
                .unwrap_or_default(),
        ))
    }
    pub fn read_phone(
        &self,
        actor_id: &str,
        conversation: &str,
    ) -> Result<ConversationDocument, StorageError> {
        let doc = parse(
            &read_required(&self.root, &phone_path(actor_id, conversation)?)?,
            true,
        )?;
        if doc.meta["agentId"] != actor_id || doc.meta["conversationId"] != conversation {
            return Err(conflict(
                "phone projection scope differs from file identity",
            ));
        }
        Ok(doc)
    }
    pub async fn receipt(
        &self,
        db: &RunDatabase,
        operation_id: &str,
    ) -> Result<Option<FileOperationReceipt>, StorageError> {
        let id = operation_id.to_owned();
        db.queue().submit(move |conn| receipt_conn(conn, &id)).await
    }
    pub async fn prepare(
        &self,
        db: &RunDatabase,
        operation_id: &str,
        mutation: &ConversationMutation,
        now: u64,
    ) -> Result<FileOperationReceipt, StorageError> {
        if operation_id.trim().is_empty()
            || operation_id.len() > 1024
            || operation_id.contains('\0')
        {
            return Err(invalid("invalid conversation file operation ID"));
        }
        let (root, id, mutation, busy, now) = (
            self.root.clone(),
            operation_id.to_owned(),
            mutation.clone(),
            db.options().busy_timeout_ms,
            i64::try_from(now).map_err(|_| invalid("conversation timestamp overflow"))?,
        );
        db.queue()
            .submit(move |conn| prepare_conn(conn, &root, &id, &mutation, now, busy))
            .await
    }
    pub async fn apply(
        &self,
        db: &RunDatabase,
        operation_id: &str,
        mutation: &ConversationMutation,
        now: u64,
    ) -> Result<FileOperationReceipt, StorageError> {
        if operation_id.trim().is_empty()
            || operation_id.len() > 1024
            || operation_id.contains('\0')
        {
            return Err(invalid("invalid conversation file operation ID"));
        }
        let (root, id, mutation, busy, now) = (
            self.root.clone(),
            operation_id.to_owned(),
            mutation.clone(),
            db.options().busy_timeout_ms,
            i64::try_from(now).map_err(|_| invalid("conversation timestamp overflow"))?,
        );
        db.queue()
            .submit(move |conn| {
                prepare_conn(conn, &root, &id, &mutation, now, busy)?;
                resume_conn(conn, &root, &id, now, busy, false)
            })
            .await
    }
    pub async fn resume(
        &self,
        db: &RunDatabase,
        operation_id: &str,
        now: u64,
    ) -> Result<FileOperationReceipt, StorageError> {
        let (root, id, busy, now) = (
            self.root.clone(),
            operation_id.to_owned(),
            db.options().busy_timeout_ms,
            i64::try_from(now).map_err(|_| invalid("conversation timestamp overflow"))?,
        );
        db.queue()
            .submit(move |conn| resume_conn(conn, &root, &id, now, busy, false))
            .await
    }
    pub async fn recover(
        &self,
        db: &RunDatabase,
        now: u64,
    ) -> Result<Vec<FileOperationReceipt>, StorageError> {
        let (root, busy, now) = (
            self.root.clone(),
            db.options().busy_timeout_ms,
            i64::try_from(now).map_err(|_| invalid("conversation timestamp overflow"))?,
        );
        db.queue().submit(move|conn|{let mut stmt=conn.prepare("SELECT operation_id FROM conversation_file_operations_v1 WHERE status IN ('prepared','partial') ORDER BY created_at,operation_id").map_err(map_rusqlite)?;let ids=stmt.query_map([],|r|r.get::<_,String>(0)).map_err(map_rusqlite)?.collect::<Result<Vec<_>,_>>().map_err(map_rusqlite)?;drop(stmt);ids.into_iter().map(|id|resume_conn(conn,&root,&id,now,busy,true)).collect()}).await
    }
}
#[cfg(not(unix))]
mod bounded_fs {
    use super::*;
    fn unavailable() -> std::io::Error {
        std::io::Error::other(
            "conversation fixed-directory file operations are unavailable on this platform",
        )
    }
    pub fn read(_: &Path, _: &str) -> std::io::Result<Option<String>> {
        Err(unavailable())
    }
    pub fn directory(_: &Path, _: &str) -> std::io::Result<()> {
        Err(unavailable())
    }
    pub fn replace(_: &Path, _: &str, _: Option<&str>, _: Option<&str>) -> std::io::Result<()> {
        Err(unavailable())
    }
    pub fn list(_: &Path, _: &str) -> std::io::Result<Vec<String>> {
        Err(unavailable())
    }
}
