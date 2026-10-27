//! SDK 管道分片只生成受限字节材料，不能认领宿主磁盘路径。
use std::{
    collections::HashMap,
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};
static GLOBAL_RESERVED: AtomicUsize = AtomicUsize::new(0);
const GLOBAL_PENDING_BYTES: usize = 128 * 1024 * 1024;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::Value;

pub const MAX_ATTACHMENT_BYTES: usize = 50 * 1024 * 1024;
pub const MAX_PENDING_MEDIA_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_ATTACHMENTS: usize = 8;
pub const MAX_CHUNK_BYTES: usize = 48 * 1024;

pub struct ReceivedMedia {
    pub message_id: String,
    pub filename: String,
    pub mime: String,
    pub bytes: Vec<u8>,
    _reservation: MediaReservation,
}
// 不能把字节移走后提前释放预算；托管登记完成或失败后整体释放。
impl Drop for ReceivedMedia {
    fn drop(&mut self) {}
}
struct MediaReservation(usize);
impl Drop for MediaReservation {
    fn drop(&mut self) {
        GLOBAL_RESERVED.fetch_sub(self.0, Ordering::AcqRel);
    }
}
struct PendingMedia {
    media: ReceivedMedia,
    expected: usize,
    next_seq: u64,
    complete: bool,
    created: Instant,
}
#[derive(Default)]
pub struct BridgeMediaPipe {
    pending: HashMap<String, PendingMedia>,
    reserved: usize,
}
fn string<'a>(frame: &'a Value, key: &str) -> Result<&'a str, String> {
    frame[key]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 4096 && !s.contains('\0'))
        .ok_or_else(|| format!("invalid Bridge media {key}"))
}
impl BridgeMediaPipe {
    /// connectorId/generation 必须先由持有该管道的宿主核验。
    pub fn push(&mut self, frame: &Value) -> Result<(), String> {
        let id = string(frame, "attachmentId")?.to_owned();
        match string(frame, "kind")? {
            "media_begin" => {
                let size = frame["sizeBytes"]
                    .as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .ok_or("invalid Bridge media size")?;
                if size > MAX_ATTACHMENT_BYTES
                    || self.pending.len() >= MAX_ATTACHMENTS
                    || self
                        .reserved
                        .checked_add(size)
                        .is_none_or(|n| n > MAX_PENDING_MEDIA_BYTES)
                {
                    return Err("Bridge media budget exceeded".into());
                }
                if self.pending.contains_key(&id) {
                    return Err("duplicate Bridge attachment identity".into());
                }
                let filename = string(frame, "filename")?.to_owned();
                if filename.contains('/')
                    || filename.contains('\\')
                    || filename == "."
                    || filename == ".."
                {
                    return Err("Bridge media filename must be a basename".into());
                }
                let mime = string(frame, "mime")?.to_owned();
                if mime.len() > 256 || mime.contains(['\r', '\n']) {
                    return Err("invalid Bridge media MIME".into());
                }
                let message_id = string(frame, "messageId")?.to_owned();
                GLOBAL_RESERVED
                    .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                        used.checked_add(size)
                            .filter(|next| *next <= GLOBAL_PENDING_BYTES)
                    })
                    .map_err(|_| "Bridge global media budget exceeded")?;
                self.reserved += size;
                self.pending.insert(
                    id,
                    PendingMedia {
                        media: ReceivedMedia {
                            message_id,
                            filename,
                            mime,
                            bytes: Vec::with_capacity(size),
                            _reservation: MediaReservation(size),
                        },
                        expected: size,
                        next_seq: 0,
                        complete: false,
                        created: Instant::now(),
                    },
                );
            }
            "media_chunk" => {
                let row = self
                    .pending
                    .get_mut(&id)
                    .ok_or("Bridge attachment has no begin frame")?;
                let seq = frame["seq"]
                    .as_u64()
                    .ok_or("invalid Bridge media sequence")?;
                if row.complete || seq != row.next_seq {
                    return Err("Bridge media sequence mismatch".into());
                }
                let data = frame["data"].as_str().ok_or("invalid Bridge media bytes")?;
                if data.len() > MAX_CHUNK_BYTES.div_ceil(3) * 4 {
                    return Err("Bridge media chunk exceeds budget".into());
                }
                let bytes = STANDARD
                    .decode(data)
                    .map_err(|_| "invalid Bridge media base64")?;
                if bytes.is_empty()
                    || bytes.len() > MAX_CHUNK_BYTES
                    || row
                        .media
                        .bytes
                        .len()
                        .checked_add(bytes.len())
                        .is_none_or(|n| n > row.expected)
                {
                    return Err("Bridge media bytes exceed declared size".into());
                }
                row.media.bytes.extend_from_slice(&bytes);
                row.next_seq = row
                    .next_seq
                    .checked_add(1)
                    .ok_or("Bridge media sequence overflow")?;
            }
            "media_end" => {
                let row = self
                    .pending
                    .get_mut(&id)
                    .ok_or("Bridge attachment has no begin frame")?;
                if row.complete || row.media.bytes.len() != row.expected {
                    return Err("Bridge media is incomplete".into());
                }
                row.complete = true;
            }
            _ => return Err("unknown Bridge media frame".into()),
        }
        Ok(())
    }
    pub fn take(
        &mut self,
        id: &str,
        message_id: &str,
        size_bytes: u64,
        mime: &str,
    ) -> Result<ReceivedMedia, String> {
        let row = self
            .pending
            .get(id)
            .ok_or("Bridge attachment bytes unavailable")?;
        if !row.complete
            || row.media.message_id != message_id
            || row.expected as u64 != size_bytes
            || row.media.mime != mime
        {
            return Err("Bridge attachment metadata does not match its completed bytes".into());
        }
        let row = self.pending.remove(id).expect("checked attachment");
        self.reserved -= row.expected;
        Ok(row.media)
    }
    pub fn expired(&self) -> bool {
        self.pending
            .values()
            .any(|row| row.created.elapsed() > Duration::from_secs(60))
    }
    pub fn clear(&mut self) {
        self.pending.clear();
        self.reserved = 0;
    }
}

impl Drop for BridgeMediaPipe {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    static MEDIA_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    fn begin(id: &str, message: &str, bytes: usize) -> Value {
        json!({"kind":"media_begin","attachmentId":id,"messageId":message,"sizeBytes":bytes,"filename":"hello.txt","mime":"text/plain"})
    }
    #[test]
    fn exact_bytes_only_after_complete_and_bound_to_message() {
        let _guard = MEDIA_TEST_LOCK.lock().unwrap();
        let mut pipe = BridgeMediaPipe::default();
        pipe.push(&begin("a", "m", 3)).unwrap();
        pipe.push(&json!({"kind":"media_chunk","attachmentId":"a","seq":0,"data":"YWJj"}))
            .unwrap();
        assert!(pipe.take("a", "m", 3, "text/plain").is_err());
        pipe.push(&json!({"kind":"media_end","attachmentId":"a"}))
            .unwrap();
        assert!(pipe.take("a", "other", 3, "text/plain").is_err());
        assert!(pipe.take("a", "m", 4, "text/plain").is_err());
        assert_eq!(pipe.take("a", "m", 3, "text/plain").unwrap().bytes, b"abc");
        assert!(pipe.take("a", "m", 3, "text/plain").is_err());
    }
    #[test]
    fn out_of_order_oversize_and_interrupted_media_are_refused() {
        let _guard = MEDIA_TEST_LOCK.lock().unwrap();
        let mut pipe = BridgeMediaPipe::default();
        pipe.push(&begin("a", "m", 1)).unwrap();
        assert!(pipe
            .push(&json!({"kind":"media_chunk","attachmentId":"a","seq":1,"data":"YQ=="}))
            .is_err());
        assert!(pipe
            .push(&json!({"kind":"media_chunk","attachmentId":"a","seq":0,"data":"YWJj"}))
            .is_err());
        assert!(pipe
            .push(&json!({"kind":"media_end","attachmentId":"a"}))
            .is_err());
        assert!(pipe
            .push(&begin("large", "m", MAX_ATTACHMENT_BYTES + 1))
            .is_err());
        pipe.clear();
        assert!(pipe.take("a", "m", 1, "text/plain").is_err());
        pipe.push(&begin("a", "m", 1)).unwrap();
    }
    #[test]
    fn aggregate_budget_and_local_paths_cannot_be_smuggled() {
        let _guard = MEDIA_TEST_LOCK.lock().unwrap();
        let mut pipe = BridgeMediaPipe::default();
        let mut frame = begin("bad", "m", 1);
        frame["filename"] = json!("../secret");
        assert!(pipe.push(&frame).is_err());
        pipe.push(&begin("a", "m", MAX_ATTACHMENT_BYTES)).unwrap();
        assert!(pipe.push(&begin("b", "m", MAX_ATTACHMENT_BYTES)).is_err());
    }
    #[test]
    fn transferred_media_keeps_its_reservation_until_bytes_are_released() {
        let _guard = MEDIA_TEST_LOCK.lock().unwrap();
        let mut first = BridgeMediaPipe::default();
        let mut second = BridgeMediaPipe::default();
        let mut third = BridgeMediaPipe::default();
        for pipe in [&mut first, &mut second] {
            pipe.push(&begin("big", "m", MAX_ATTACHMENT_BYTES)).unwrap();
            let row = pipe.pending.get_mut("big").unwrap();
            row.media.bytes.resize(MAX_ATTACHMENT_BYTES, b'x');
            row.complete = true;
        }
        let bytes = first
            .take("big", "m", MAX_ATTACHMENT_BYTES as u64, "text/plain")
            .unwrap();
        first.clear();
        assert!(
            third
                .push(&begin("big", "m", MAX_ATTACHMENT_BYTES))
                .is_err(),
            "登记尚未结束的真实字节继续占用全局预算"
        );
        drop(bytes);
        third
            .push(&begin("big", "m", MAX_ATTACHMENT_BYTES))
            .unwrap();
    }
    #[test]
    fn zero_byte_document_has_a_bounded_attachment_identity() {
        let _guard = MEDIA_TEST_LOCK.lock().unwrap();
        let mut pipe = BridgeMediaPipe::default();
        pipe.push(&begin("empty", "m", 0)).unwrap();
        pipe.push(&json!({"kind":"media_end","attachmentId":"empty"}))
            .unwrap();
        assert!(pipe
            .take("empty", "m", 0, "text/plain")
            .unwrap()
            .bytes
            .is_empty());
    }
}
