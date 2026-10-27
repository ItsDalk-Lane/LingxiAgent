//! CDP client over Chromium's `--remote-debugging-pipe` transport.
//!
//! Protocol: NUL-terminated JSON messages. fd 3 = browser reads commands,
//! fd 4 = browser writes responses/events (puppeteer pipe transport
//! convention; verified against Chrome 153 on macOS arm64 in this spike).

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Condvar, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

pub struct CdpClient {
    writer: Mutex<File>,
    next_id: AtomicU64,
    pending: Mutex<HashMap<u64, mpsc::Sender<Value>>>,
    events: Mutex<Vec<Value>>,
    event_cond: Condvar,
    /// every raw message seen on the wire (for the evidence transcript)
    wire_log: Mutex<Vec<String>>,
}

#[derive(Debug)]
pub enum CdpError {
    Io(std::io::Error),
    Protocol(String),
    Timeout(String),
    Remote { code: i64, message: String },
}

impl std::fmt::Display for CdpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CdpError::Io(e) => write!(f, "io: {e}"),
            CdpError::Protocol(m) => write!(f, "protocol: {m}"),
            CdpError::Timeout(m) => write!(f, "timeout: {m}"),
            CdpError::Remote { code, message } => write!(f, "cdp error {code}: {message}"),
        }
    }
}

impl std::error::Error for CdpError {}

impl From<std::io::Error> for CdpError {
    fn from(e: std::io::Error) -> Self {
        CdpError::Io(e)
    }
}

impl From<serde_json::Error> for CdpError {
    fn from(e: serde_json::Error) -> Self {
        CdpError::Protocol(format!("json: {e}"))
    }
}

pub type CdpResult<T> = Result<T, CdpError>;

impl CdpClient {
    /// Spawn the reader thread and return the client. `rsp_read` is the pipe
    /// end carrying browser->client messages; `cmd_write` the reverse.
    pub fn new(rsp_read: File, cmd_write: File) -> std::sync::Arc<Self> {
        let client = std::sync::Arc::new(CdpClient {
            writer: Mutex::new(cmd_write),
            next_id: AtomicU64::new(1),
            pending: Mutex::new(HashMap::new()),
            events: Mutex::new(Vec::new()),
            event_cond: Condvar::new(),
            wire_log: Mutex::new(Vec::new()),
        });
        let weak = std::sync::Arc::downgrade(&client);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(rsp_read);
            loop {
                let mut buf: Vec<u8> = Vec::new();
                match reader.read_until(0, &mut buf) {
                    Ok(0) => break, // EOF: browser gone
                    Ok(_) => {}     // one message (may include trailing NUL)
                    Err(_) => break,
                }
                if buf.last() == Some(&0) {
                    buf.pop();
                }
                if buf.is_empty() {
                    continue;
                }
                let text = String::from_utf8_lossy(&buf).to_string();
                let parsed: Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if let Some(c) = weak.upgrade() {
                    c.wire_log.lock().unwrap().push(text);
                    if let Some(id) = parsed.get("id").and_then(|v| v.as_u64()) {
                        let tx = c.pending.lock().unwrap().remove(&id);
                        if let Some(tx) = tx {
                            let _ = tx.send(parsed);
                        }
                    } else {
                        let mut evs = c.events.lock().unwrap();
                        evs.push(parsed);
                        c.event_cond.notify_all();
                    }
                } else {
                    break;
                }
            }
        });
        client
    }

    /// Send a command and wait for its response.
    pub fn call(&self, session: Option<&str>, method: &str, params: Value) -> CdpResult<Value> {
        self.call_timeout(session, method, params, Duration::from_secs(30))
    }

    pub fn call_timeout(
        &self,
        session: Option<&str>,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> CdpResult<Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let mut msg = json!({ "id": id, "method": method, "params": params });
        if let Some(s) = session {
            msg["sessionId"] = Value::String(s.to_string());
        }
        let (tx, rx) = mpsc::channel();
        self.pending.lock().unwrap().insert(id, tx);
        {
            let mut w = self.writer.lock().unwrap();
            let mut bytes = serde_json::to_vec(&msg)?;
            bytes.push(0);
            w.write_all(&bytes)?;
            w.flush()?;
        }
        let resp = rx
            .recv_timeout(timeout)
            .map_err(|_| CdpError::Timeout(format!("{method} id={id}")))?;
        if let Some(err) = resp.get("error") {
            return Err(CdpError::Remote {
                code: err.get("code").and_then(|v| v.as_i64()).unwrap_or(0),
                message: err
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            });
        }
        Ok(resp.get("result").cloned().unwrap_or(Value::Null))
    }

    /// Wait for an event whose `method` matches and `pred` passes. Buffered
    /// events are scanned first, so events that arrived before the call are
    /// still found.
    pub fn wait_event<F>(&self, method: &str, pred: F, timeout: Duration) -> CdpResult<Value>
    where
        F: Fn(&Value) -> bool,
    {
        let deadline = Instant::now() + timeout;
        let mut guard = self.events.lock().unwrap();
        loop {
            if let Some(pos) = guard
                .iter()
                .position(|e| e.get("method").and_then(|m| m.as_str()) == Some(method) && pred(e))
            {
                return Ok(guard.remove(pos));
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(CdpError::Timeout(format!("event {method}")));
            }
            let remaining = deadline - now;
            let (g, _) = self.event_cond.wait_timeout(guard, remaining).unwrap();
            guard = g;
        }
    }

    pub fn wire_log(&self) -> Vec<String> {
        self.wire_log.lock().unwrap().clone()
    }
}

/// Parse the width/height out of a PNG (IHDR) without an image crate.
pub fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 24 || &bytes[0..8] != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    if &bytes[12..16] != b"IHDR" {
        return None;
    }
    let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    Some((w, h))
}

/// Blocking read of an entire stream (used by tests).
pub fn read_all(mut r: impl Read) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    r.read_to_end(&mut out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::io::FromRawFd;

    fn pipe_pair() -> (File, File) {
        let mut fds = [0i32; 2];
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        unsafe { (File::from_raw_fd(fds[0]), File::from_raw_fd(fds[1])) }
    }

    #[test]
    fn roundtrip_command_response() {
        // fake browser: read one NUL-terminated message, answer it
        let (mut cmd_r, cmd_w) = pipe_pair();
        let (rsp_r, mut rsp_w) = pipe_pair();
        let client = CdpClient::new(rsp_r, cmd_w);
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let mut br = BufReader::new(&mut cmd_r);
            br.read_until(0, &mut buf).unwrap();
            let req: Value = serde_json::from_slice(&buf[..buf.len() - 1]).unwrap();
            let resp = json!({"id": req["id"], "result": {"ok": true}});
            rsp_w
                .write_all(serde_json::to_string(&resp).unwrap().as_bytes())
                .unwrap();
            rsp_w.write_all(&[0]).unwrap();
        });
        let r = client.call(None, "Browser.getVersion", json!({})).unwrap();
        assert_eq!(r["ok"], true);
    }

    #[test]
    fn events_buffered_and_waitable() {
        let (cmd_r, cmd_w) = pipe_pair();
        drop(cmd_r);
        let (rsp_r, mut rsp_w) = pipe_pair();
        let client = CdpClient::new(rsp_r, cmd_w);
        let ev = json!({"method":"Page.loadEventFired","params":{"timestamp":1.0}});
        rsp_w
            .write_all(serde_json::to_string(&ev).unwrap().as_bytes())
            .unwrap();
        rsp_w.write_all(&[0]).unwrap();
        rsp_w.flush().unwrap();
        let got = client
            .wait_event("Page.loadEventFired", |_| true, Duration::from_secs(5))
            .unwrap();
        assert_eq!(got["method"], "Page.loadEventFired");
    }

    #[test]
    fn remote_error_surfaces() {
        let (_cmd_r_keep, cmd_w) = pipe_pair();
        let (rsp_r, _rsp_w_keep) = pipe_pair();
        let mut rsp_w = _rsp_w_keep.try_clone().unwrap();
        let client = CdpClient::new(rsp_r, cmd_w);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            let resp = json!({"id": 1, "error": {"code": -32601, "message": "nope"}});
            rsp_w
                .write_all(serde_json::to_string(&resp).unwrap().as_bytes())
                .unwrap();
            rsp_w.write_all(&[0]).unwrap();
            rsp_w.flush().unwrap();
        });
        let err = client.call(None, "Bogus.method", json!({})).unwrap_err();
        match err {
            CdpError::Remote { code, .. } => assert_eq!(code, -32601),
            other => panic!("unexpected {other}"),
        }
    }

    #[test]
    fn png_dims_parse() {
        // minimal PNG header: 8-byte magic + len(4) + "IHDR" + w(4) + h(4)
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend_from_slice(&13u32.to_be_bytes());
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&800u32.to_be_bytes());
        png.extend_from_slice(&600u32.to_be_bytes());
        png.extend_from_slice(&[0u8; 8]);
        assert_eq!(png_dimensions(&png), Some((800, 600)));
        assert_eq!(png_dimensions(b"not a png"), None);
    }
}
