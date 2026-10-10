//! R06-T05：文件元数据纯函数，逐条镜像现役 `lib/file-metadata.ts`
//! （MAGIC_TABLE / EXT_MIME / extOfName / detectMime / inferFileKind）与
//! `shared/image-mime.ts`、`shared/audio-mime.ts`、`shared/video-mime.ts`
//! 的上传白名单，外加 `server/routes/upload.ts` 的文件名清洗
//! （sanitizeBlobName / uniqueUploadName）。RC-3：表项与顺序逐字节对照，
//! 任何行为差异都进差异台账。

/// 现役 MAGIC_TABLE（file-metadata.ts:8-17），顺序敏感：detectMime 按表
/// 序返回首个命中。
struct MagicEntry {
    bytes: &'static [u8],
    mime: &'static str,
    /// RIFF→WEBP 的二次校验：偏移与附加字节。
    extra: Option<(usize, &'static [u8])>,
    /// MP4 的 ftyp 校验（现役 check 回调）。
    ftyp_check: bool,
    min_len: usize,
}

const MAGIC_TABLE: &[MagicEntry] = &[
    MagicEntry {
        bytes: &[0xFF, 0xD8, 0xFF],
        mime: "image/jpeg",
        extra: None,
        ftyp_check: false,
        min_len: 0,
    },
    MagicEntry {
        bytes: &[0x89, 0x50, 0x4E, 0x47],
        mime: "image/png",
        extra: None,
        ftyp_check: false,
        min_len: 0,
    },
    MagicEntry {
        bytes: &[0x47, 0x49, 0x46, 0x38],
        mime: "image/gif",
        extra: None,
        ftyp_check: false,
        min_len: 0,
    },
    MagicEntry {
        bytes: &[0x52, 0x49, 0x46, 0x46],
        mime: "image/webp",
        extra: Some((8, &[0x57, 0x45, 0x42, 0x50])),
        ftyp_check: false,
        min_len: 0,
    },
    MagicEntry {
        bytes: &[0x25, 0x50, 0x44, 0x46],
        mime: "application/pdf",
        extra: None,
        ftyp_check: false,
        min_len: 0,
    },
    MagicEntry {
        bytes: &[0x49, 0x44, 0x33],
        mime: "audio/mpeg",
        extra: None,
        ftyp_check: false,
        min_len: 0,
    },
    MagicEntry {
        bytes: &[0x4F, 0x67, 0x67, 0x53],
        mime: "audio/ogg",
        extra: None,
        ftyp_check: false,
        min_len: 0,
    },
    MagicEntry {
        bytes: &[0x00, 0x00, 0x00],
        mime: "video/mp4",
        extra: None,
        ftyp_check: true,
        min_len: 8,
    },
];

/// 现役 EXT_MIME（file-metadata.ts:19-32）。
fn ext_mime(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "txt" => "text/plain",
        "md" | "markdown" => "text/markdown",
        "json" => "application/json",
        "csv" => "text/csv",
        "xml" => "text/xml",
        "html" | "htm" => "text/html",
        "svg" => "image/svg+xml",
        "yml" | "yaml" => "text/yaml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "zip" => "application/zip",
        "gz" => "application/gzip",
        "tar" => "application/x-tar",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "m4a" => "audio/mp4",
        "weba" => "audio/webm",
        "flac" => "audio/flac",
        _ => return None,
    })
}

/// 现役 extOfName（file-metadata.ts:34-40）：最后一段小写扩展名；
/// 无点或点收尾为空串。
pub fn ext_of_name(name: &str) -> String {
    let Some(dot) = name.rfind('.') else {
        return String::new();
    };
    if dot == name.len() - 1 {
        return String::new();
    }
    name[dot + 1..].to_lowercase()
}

/// 现役 detectMime（file-metadata.ts:42-58）：魔数表优先，扩展名其次，
/// 最后回退 fallback。
pub fn detect_mime(sample: &[u8], fallback: &str, filename: &str) -> String {
    for entry in MAGIC_TABLE {
        let min_len = if entry.min_len > 0 {
            entry.min_len
        } else {
            entry.bytes.len()
        };
        if sample.len() < min_len {
            continue;
        }
        if sample.len() < entry.bytes.len() || sample[..entry.bytes.len()] != *entry.bytes {
            continue;
        }
        if let Some((offset, extra)) = entry.extra {
            if sample.len() < offset + extra.len() || sample[offset..offset + extra.len()] != *extra
            {
                continue;
            }
        }
        if entry.ftyp_check {
            // 现役：b.toString("ascii", 4, 8) === "ftyp"
            if sample.len() < 8 || &sample[4..8] != b"ftyp" {
                continue;
            }
        }
        return entry.mime.to_string();
    }
    let ext = ext_of_name(filename);
    if !ext.is_empty() {
        if let Some(mime) = ext_mime(&ext) {
            return mime.to_string();
        }
    }
    fallback.to_string()
}

/// 现役 inferFileKind（file-metadata.ts:60-75）。
pub fn infer_file_kind(mime: &str, ext: &str, is_directory: bool) -> &'static str {
    if is_directory {
        return "directory";
    }
    let lower_mime = mime.to_lowercase();
    if lower_mime.starts_with("image/") {
        return "image";
    }
    if lower_mime.starts_with("video/") {
        return "video";
    }
    if lower_mime.starts_with("audio/") {
        return "audio";
    }
    let lower_ext = ext.to_lowercase();
    if ["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"].contains(&lower_ext.as_str()) {
        return "image";
    }
    if ["mp4", "mov", "webm"].contains(&lower_ext.as_str()) {
        return "video";
    }
    if ["mp3", "wav", "ogg", "m4a", "weba", "flac"].contains(&lower_ext.as_str()) {
        return "audio";
    }
    if [
        "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "md", "markdown", "txt", "csv", "json",
        "yaml", "yml", "xml", "html", "htm",
    ]
    .contains(&lower_ext.as_str())
    {
        return "document";
    }
    if !lower_mime.is_empty() && lower_mime != "application/octet-stream" {
        return "document";
    }
    "unknown"
}

// ── 上传白名单（shared/image-mime.ts / audio-mime.ts / video-mime.ts） ──

/// 现役 MAX_CHAT_IMAGE_BASE64_CHARS。
pub const MAX_CHAT_IMAGE_BASE64_CHARS: usize = 20 * 1024 * 1024;
/// 现役 MAX_CHAT_AUDIO_BASE64_CHARS。
pub const MAX_CHAT_AUDIO_BASE64_CHARS: usize = 50 * 1024 * 1024;
/// 现役 MAX_CHAT_VIDEO_BASE64_CHARS。
pub const MAX_CHAT_VIDEO_BASE64_CHARS: usize = 20 * 1024 * 1024;

const ALLOWED_CHAT_IMAGE_MIME: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];
const ALLOWED_CHAT_AUDIO_MIME: &[&str] = &[
    "audio/mpeg",
    "audio/mp3",
    "audio/wav",
    "audio/x-wav",
    "audio/mp4",
    "audio/ogg",
    "audio/flac",
];
const ALLOWED_UPLOAD_AUDIO_MIME_EXTRA: &[&str] = &["audio/webm"];
const ALLOWED_CHAT_VIDEO_MIME: &[&str] = &["video/mp4", "video/webm", "video/quicktime"];

fn normalize_mime(mime: &str) -> String {
    mime.trim().to_lowercase()
}

pub fn is_allowed_chat_image_mime(mime: &str) -> bool {
    ALLOWED_CHAT_IMAGE_MIME.contains(&normalize_mime(mime).as_str())
}

pub fn is_allowed_upload_audio_mime(mime: &str) -> bool {
    let normalized = normalize_mime(mime);
    ALLOWED_CHAT_AUDIO_MIME.contains(&normalized.as_str())
        || ALLOWED_UPLOAD_AUDIO_MIME_EXTRA.contains(&normalized.as_str())
}

pub fn is_allowed_chat_video_mime(mime: &str) -> bool {
    ALLOWED_CHAT_VIDEO_MIME.contains(&normalize_mime(mime).as_str())
}

pub fn is_allowed_upload_blob_mime(mime: &str) -> bool {
    is_allowed_chat_image_mime(mime)
        || is_allowed_upload_audio_mime(mime)
        || is_allowed_chat_video_mime(mime)
}

/// 现役 extFromMime：image → audio → video 顺序查表。
pub fn ext_from_mime(mime: &str) -> &'static str {
    match normalize_mime(mime).as_str() {
        "image/png" => ".png",
        "image/jpeg" => ".jpg",
        "image/gif" => ".gif",
        "image/webp" => ".webp",
        "audio/mpeg" | "audio/mp3" => ".mp3",
        "audio/wav" | "audio/x-wav" => ".wav",
        "audio/mp4" => ".m4a",
        "audio/ogg" => ".ogg",
        "audio/flac" => ".flac",
        "audio/webm" => ".weba",
        "video/mp4" => ".mp4",
        "video/webm" => ".webm",
        "video/quicktime" => ".mov",
        _ => "",
    }
}

/// 现役 isUploadBlobBase64WithinLimit：按 mime 族取各自上限。
pub fn upload_blob_base64_within_limit(base64_data: &str, mime: &str) -> bool {
    if is_allowed_chat_image_mime(mime) {
        return base64_data.len() <= MAX_CHAT_IMAGE_BASE64_CHARS;
    }
    if is_allowed_upload_audio_mime(mime) {
        return base64_data.len() <= MAX_CHAT_AUDIO_BASE64_CHARS;
    }
    if is_allowed_chat_video_mime(mime) {
        return base64_data.len() <= MAX_CHAT_VIDEO_BASE64_CHARS;
    }
    false
}

pub fn upload_blob_max_base64_chars(mime: &str) -> usize {
    if is_allowed_upload_audio_mime(mime) {
        MAX_CHAT_AUDIO_BASE64_CHARS
    } else if is_allowed_chat_video_mime(mime) {
        MAX_CHAT_VIDEO_BASE64_CHARS
    } else {
        MAX_CHAT_IMAGE_BASE64_CHARS
    }
}

/// 现役 isChatVideoBytesCompatible（video-mime.ts）：webm 看 EBML 头，
/// mp4/mov 看 ftyp。
pub fn is_chat_video_bytes_compatible(bytes: &[u8], mime: &str) -> bool {
    match normalize_mime(mime).as_str() {
        "video/webm" => {
            bytes.len() >= 4
                && bytes[0] == 0x1a
                && bytes[1] == 0x45
                && bytes[2] == 0xdf
                && bytes[3] == 0xa3
        }
        "video/mp4" | "video/quicktime" => {
            bytes.len() >= 12
                && bytes[4] == 0x66
                && bytes[5] == 0x74
                && bytes[6] == 0x79
                && bytes[7] == 0x70
        }
        _ => false,
    }
}

// ── 文件名清洗与唯一命名（upload.ts:47-183,269-272） ──

/// 现役 MAX_FILENAME_BYTES。
pub const MAX_FILENAME_BYTES: usize = 255;

const WINDOWS_RESERVED_CHARS: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
const WINDOWS_RESERVED_DEVICE_NAMES: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

fn is_control_code_point(cp: u32) -> bool {
    (0x00..=0x1f).contains(&cp) || (0x80..=0x9f).contains(&cp)
}

/// 现役 truncateUtf8Bytes：按 UTF-8 字节预算截断，不劈开字符。
pub fn truncate_utf8_bytes(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let mut used = 0usize;
    let mut out = String::new();
    for ch in value.chars() {
        let len = ch.len_utf8();
        if used + len > max_bytes {
            break;
        }
        out.push(ch);
        used += len;
    }
    out
}

fn strip_unsafe_file_name_chars(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !is_control_code_point(*ch as u32) && !WINDOWS_RESERVED_CHARS.contains(ch))
        .collect()
}

fn trim_windows_trailing_chars(value: &str) -> String {
    value.trim_end_matches([' ', '.']).to_string()
}

fn normalize_windows_device_name(filename: &str) -> String {
    let ext = ext_of_name(filename);
    let base = if ext.is_empty() {
        filename
    } else {
        &filename[..filename.len() - ext.len() - 1]
    };
    if !WINDOWS_RESERVED_DEVICE_NAMES.contains(&base.to_lowercase().as_str()) {
        return filename.to_string();
    }
    format!("file-{filename}")
}

fn sanitize_file_name_candidate(value: &str) -> String {
    // 跨平台 basename：先把反斜杠当分隔符处理，再取最后一段。
    let unified = value.replace('\\', "/");
    let base = unified.rsplit('/').next().unwrap_or("");
    let stripped = strip_unsafe_file_name_chars(base);
    let trimmed = trim_windows_trailing_chars(stripped.trim());
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        return String::new();
    }
    trimmed
}

/// 现役 sanitizeBlobName：用户传入 name 不可信；缺省按 mime 族给
/// recording/video/pasted 基底；扩展名强制匹配 mime。
pub fn sanitize_blob_name(name: Option<&str>, mime: &str) -> String {
    let fallback_base = if is_allowed_chat_audio_mime(mime) {
        "recording"
    } else if is_allowed_chat_video_mime(mime) {
        "video"
    } else {
        "pasted"
    };
    let ext = ext_from_mime(mime);
    let fallback = format!(
        "{fallback_base}{}",
        if ext.is_empty() { ".bin" } else { ext }
    );
    let Some(name) = name else {
        return fallback;
    };
    let mut base = sanitize_file_name_candidate(name);
    if base.is_empty() {
        return fallback;
    }
    if !ext.is_empty() {
        let current = ext_of_name(&base);
        if format!(".{current}") != ext {
            let stem = if current.is_empty() {
                base.as_str()
            } else {
                &base[..base.len() - current.len() - 1]
            };
            base = format!("{stem}{ext}");
        }
    }
    base = normalize_windows_device_name(&base);
    let truncated = truncate_utf8_bytes(&base, MAX_FILENAME_BYTES);
    if truncated.is_empty() {
        fallback
    } else {
        truncated
    }
}

fn is_allowed_chat_audio_mime(mime: &str) -> bool {
    ALLOWED_CHAT_AUDIO_MIME.contains(&normalize_mime(mime).as_str())
}

/// 现役 uniqueUploadName：`{base}_{ts36}_{4hex}{ext}`，base 按字节预算
/// 截断（255 - suffix 字节数）。
pub fn unique_upload_name(base: &str, ext: &str, now_ms: u64, random_hex4: &str) -> String {
    let suffix = format!("_{}_{}", to_base36(now_ms), random_hex4);
    let budget = MAX_FILENAME_BYTES
        .saturating_sub(suffix.len() + ext.len())
        .max(1);
    format!("{}{}{}", truncate_utf8_bytes(base, budget), suffix, ext)
}

/// 现役 Date.now().toString(36) 的小写 base36。
/// u64 → base36（现役 `Number.prototype.toString(36)` 的整数域；R06-T05
/// 资源 etag `"{mtimeMs36}-{size36}"` 复用）。
pub fn to_base36(mut value: u64) -> String {
    if value == 0 {
        return "0".to_string();
    }
    let mut digits = Vec::new();
    while value > 0 {
        let rem = (value % 36) as usize;
        digits.push(b"0123456789abcdefghijklmnopqrstuvwxyz"[rem] as char);
        value /= 36;
    }
    digits.iter().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_mime_magic_beats_extension() {
        // PNG 魔数 + .txt 扩展名 → image/png（魔数优先，现役同序）。
        let png = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        assert_eq!(
            detect_mime(&png, "application/octet-stream", "a.txt"),
            "image/png"
        );
        assert_eq!(
            detect_mime(b"hello", "application/octet-stream", "a.txt"),
            "text/plain"
        );
        assert_eq!(
            detect_mime(
                b"\x00\x00\x00\x18ftypmp42",
                "application/octet-stream",
                "v.bin"
            ),
            "video/mp4"
        );
        // ftyp 校验失败不落 video/mp4。
        assert_eq!(
            detect_mime(b"\x00\x00\x00\x18xxxx", "application/octet-stream", "v.bin"),
            "application/octet-stream"
        );
        // RIFF 无 WEBP 附加字节不落 image/webp。
        assert_eq!(
            detect_mime(b"RIFF....WAVE", "application/octet-stream", "a.bin"),
            "application/octet-stream"
        );
        assert_eq!(
            detect_mime(b"RIFF....WEBP", "application/octet-stream", "a.bin"),
            "image/webp"
        );
    }

    #[test]
    fn infer_file_kind_order_matches_incumbent() {
        assert_eq!(infer_file_kind("inode/directory", "", true), "directory");
        assert_eq!(infer_file_kind("image/png", "", false), "image");
        assert_eq!(infer_file_kind("application/pdf", "pdf", false), "document");
        assert_eq!(
            infer_file_kind("application/octet-stream", "", false),
            "unknown"
        );
        assert_eq!(infer_file_kind("", "weba", false), "audio");
    }

    #[test]
    fn sanitize_blob_name_strips_untrusted_input() {
        assert_eq!(sanitize_blob_name(None, "image/png"), "pasted.png");
        assert_eq!(
            sanitize_blob_name(Some("../evil.exe"), "image/png"),
            "evil.png"
        );
        assert_eq!(
            sanitize_blob_name(Some("CON"), "audio/mpeg"),
            "file-CON.mp3"
        );
        assert_eq!(sanitize_blob_name(Some(".."), "video/mp4"), "video.mp4");
        assert_eq!(
            sanitize_blob_name(Some("a/b\\c.txt"), "image/jpeg"),
            "c.jpg"
        );
    }

    #[test]
    fn unique_upload_name_shape() {
        let name = unique_upload_name("photo", ".png", 1_700_000_000_000, "a1b2c3d4");
        assert!(name.starts_with("photo_"));
        assert!(name.ends_with(".png"));
        assert!(name.contains("_a1b2c3d4"));
        assert_eq!(to_base36(0), "0");
        assert_eq!(to_base36(35), "z");
        assert_eq!(to_base36(36), "10");
    }

    #[test]
    fn video_bytes_compatibility() {
        assert!(is_chat_video_bytes_compatible(
            &[0x1a, 0x45, 0xdf, 0xa3],
            "video/webm"
        ));
        assert!(!is_chat_video_bytes_compatible(
            &[0x1a, 0x45, 0xdf],
            "video/webm"
        ));
        let mut mp4 = vec![0u8; 12];
        mp4[4..8].copy_from_slice(b"ftyp");
        assert!(is_chat_video_bytes_compatible(&mp4, "video/mp4"));
        assert!(!is_chat_video_bytes_compatible(&mp4, "video/webm"));
        assert!(!is_chat_video_bytes_compatible(&mp4, "text/plain"));
    }
}
