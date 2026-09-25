//! lingxi-proto-server — deterministic local handshake prototype for
//! `lingxi.wire` v1 (R01-T02 step 6, acceptance R01-A04).
//!
//! A minimal, dependency-light HTTP/1.1 + WebSocket (RFC 6455) server built
//! on `std::net` only. It exists to produce real HTTP/WS interaction
//! records against a deterministic local stand-in — it is not the future
//! service transport (that is R02 scope) and binds loopback only.
//!
//! Surface:
//!   POST /lingxi/v1/handshake   body = ClientHello JSON
//!        → 200 ServerHello JSON | 400 ProtocolError JSON
//!   GET  /lingxi/v1/ws          Upgrade: websocket
//!        → 101; first text frame = ClientHello JSON
//!        → text frame ServerHello JSON + close(1000)
//!          | text frame ProtocolError JSON + close(4409, "version_incompatible")
//!
//! `--connections N` makes the process exit 0 after exactly N connections,
//! so transcripts are deterministic.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::ExitCode;

use base64::Engine;
use lingxi_protocol::canon::canonical_bytes;
use lingxi_protocol::handshake::{
    accept_hello, negotiate_protocol, WIRE_PROTOCOL_MAX_SUPPORTED, WIRE_PROTOCOL_MIN_SUPPORTED,
};
use lingxi_protocol::{ClientHello, ErrorCode, ProtocolError};
use sha1::{Digest, Sha1};

const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
const CLOSE_VERSION_INCOMPATIBLE: u16 = 4409;

const KNOWN_CAPS: &[&str] = &["events.v1"];
const SERVER_KIND: &str = "lingxi-proto-server";
const SERVER_VERSION: &str = "0.0.0-r01t02";
const DATA_EPOCH: u32 = 1;

fn log(line: &str) {
    println!("{line}");
    std::io::stdout().flush().ok();
}

// ── Minimal HTTP/1.1 ───────────────────────────────────────────────────────

struct HttpRequest {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl HttpRequest {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

fn read_http_request(stream: &mut TcpStream) -> std::io::Result<Option<HttpRequest>> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end;
    loop {
        match stream.read(&mut chunk)? {
            0 if buf.is_empty() => return Ok(None), // client went away
            0 => return Ok(None),
            n => {
                buf.extend_from_slice(&chunk[..n]);
                if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
                    head_end = pos;
                    break;
                }
                if buf.len() > 64 * 1024 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "headers too large",
                    ));
                }
            }
        }
    }
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();
    let mut headers = Vec::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_string(), value.trim().to_string()));
        }
    }
    let content_length: usize = headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    let mut body: Vec<u8> = buf[head_end + 4..].to_vec();
    while body.len() < content_length {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(content_length);
    Ok(Some(HttpRequest {
        method,
        path,
        headers,
        body,
    }))
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn write_http_response(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    content_type: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

// ── Minimal WebSocket (RFC 6455) ───────────────────────────────────────────

fn websocket_accept(key: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(key.as_bytes());
    hasher.update(WS_GUID.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(hasher.finalize())
}

enum WsFrame {
    Text(Vec<u8>),
    Close(u16, String),
    Ping(Vec<u8>),
    Pong,
}

fn read_ws_frame(stream: &mut TcpStream) -> std::io::Result<Option<WsFrame>> {
    let mut header = [0u8; 2];
    match stream.read_exact(&mut header) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let opcode = header[0] & 0x0f;
    let masked = header[1] & 0x80 != 0;
    let mut len = (header[1] & 0x7f) as u64;
    if len == 126 {
        let mut ext = [0u8; 2];
        stream.read_exact(&mut ext)?;
        len = u16::from_be_bytes(ext) as u64;
    } else if len == 127 {
        let mut ext = [0u8; 8];
        stream.read_exact(&mut ext)?;
        len = u64::from_be_bytes(ext);
    }
    if len > 4 * 1024 * 1024 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "frame too large",
        ));
    }
    let mask = if masked {
        let mut m = [0u8; 4];
        stream.read_exact(&mut m)?;
        Some(m)
    } else {
        None
    };
    let mut payload = vec![0u8; len as usize];
    stream.read_exact(&mut payload)?;
    if let Some(m) = mask {
        for (i, b) in payload.iter_mut().enumerate() {
            *b ^= m[i % 4];
        }
    }
    Ok(Some(match opcode {
        0x1 => WsFrame::Text(payload),
        0x8 => {
            let code = if payload.len() >= 2 {
                u16::from_be_bytes([payload[0], payload[1]])
            } else {
                1005
            };
            let reason = String::from_utf8_lossy(&payload[2.min(payload.len())..]).into_owned();
            WsFrame::Close(code, reason)
        }
        0x9 => WsFrame::Ping(payload),
        0xA => WsFrame::Pong,
        other => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unsupported opcode {other:#x}"),
            ))
        }
    }))
}

fn write_ws_frame(stream: &mut TcpStream, opcode: u8, payload: &[u8]) -> std::io::Result<()> {
    let mut out = vec![0x80 | opcode];
    if payload.len() < 126 {
        out.push(payload.len() as u8);
    } else if payload.len() <= u16::MAX as usize {
        out.push(126);
        out.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    } else {
        out.push(127);
        out.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    }
    out.extend_from_slice(payload);
    stream.write_all(&out)?;
    stream.flush()
}

fn write_ws_close(stream: &mut TcpStream, code: u16, reason: &str) -> std::io::Result<()> {
    let mut payload = code.to_be_bytes().to_vec();
    payload.extend_from_slice(reason.as_bytes());
    write_ws_frame(stream, 0x8, &payload)
}

// ── Handshake application logic ────────────────────────────────────────────

fn handle_hello(body: &[u8]) -> (u16, &'static str, Vec<u8>) {
    let hello: ClientHello = match serde_json::from_slice(body) {
        Ok(h) => h,
        Err(e) => {
            let err = ProtocolError::new(
                ErrorCode::InvalidMessage,
                format!("handshake body is not a valid ClientHello: {e}"),
                false,
            );
            return (400, "Bad Request", canonical_bytes(&err));
        }
    };
    match negotiate_protocol(&hello) {
        Ok(selected) => {
            let reply = accept_hello(
                &hello,
                selected,
                SERVER_KIND,
                SERVER_VERSION,
                DATA_EPOCH,
                KNOWN_CAPS,
            );
            (200, "OK", canonical_bytes(&reply))
        }
        Err(err) => {
            // All handshake rejections are HTTP 400 (version-incompatible
            // included); the wire error body carries the specific code.
            (400, "Bad Request", canonical_bytes(&err))
        }
    }
}

fn handle_connection(mut stream: TcpStream, index: usize) -> std::io::Result<()> {
    let request = match read_http_request(&mut stream)? {
        Some(r) => r,
        None => return Ok(()),
    };
    let path = request.path.split('?').next().unwrap_or("");

    if request.method == "POST" && path == "/lingxi/v1/handshake" {
        let (status, reason, body) = handle_hello(&request.body);
        let summary = summarize_body(&body);
        log(&format!(
            "conn#{index} HTTP POST /lingxi/v1/handshake -> {status} {summary}"
        ));
        write_http_response(&mut stream, status, reason, "application/json", &body)?;
        return Ok(());
    }

    if request.method == "GET"
        && path == "/lingxi/v1/ws"
        && request
            .header("upgrade")
            .is_some_and(|v| v.eq_ignore_ascii_case("websocket"))
    {
        let key = request.header("sec-websocket-key").unwrap_or_default();
        let accept = websocket_accept(key);
        let head = format!(
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
        );
        stream.write_all(head.as_bytes())?;
        stream.flush()?;
        log(&format!("conn#{index} WS /lingxi/v1/ws -> 101 upgraded"));

        // First text frame must be the ClientHello.
        loop {
            match read_ws_frame(&mut stream)? {
                Some(WsFrame::Text(payload)) => {
                    let (status, _reason, body) = handle_hello(&payload);
                    let summary = summarize_body(&body);
                    write_ws_frame(&mut stream, 0x1, &body)?;
                    if status == 200 {
                        log(&format!(
                            "conn#{index} WS hello -> selected=1, close=1000 {summary}"
                        ));
                        write_ws_close(&mut stream, 1000, "handshake complete")?;
                    } else {
                        log(&format!(
                            "conn#{index} WS hello -> rejected, close={CLOSE_VERSION_INCOMPATIBLE} {summary}"
                        ));
                        write_ws_close(
                            &mut stream,
                            CLOSE_VERSION_INCOMPATIBLE,
                            "version_incompatible",
                        )?;
                    }
                    // Drain the client's close echo, then finish.
                    while let Some(frame) = read_ws_frame(&mut stream)? {
                        if let WsFrame::Close(code, reason) = frame {
                            log(&format!(
                                "conn#{index} WS client close code={code} reason={reason:?}"
                            ));
                            break;
                        }
                    }
                    return Ok(());
                }
                Some(WsFrame::Ping(p)) => write_ws_frame(&mut stream, 0xA, &p)?,
                Some(WsFrame::Pong) => {}
                Some(WsFrame::Close(code, reason)) => {
                    log(&format!(
                        "conn#{index} WS closed before hello code={code} reason={reason:?}"
                    ));
                    return Ok(());
                }
                None => return Ok(()),
            }
        }
    }

    let body = canonical_bytes(&ProtocolError::new(
        ErrorCode::NotFound,
        format!("no such endpoint: {} {path}", request.method),
        false,
    ));
    log(&format!("conn#{index} {} {path} -> 404", request.method));
    write_http_response(&mut stream, 404, "Not Found", "application/json", &body)
}

fn summarize_body(body: &[u8]) -> String {
    let v: serde_json::Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return "(unparseable body)".into(),
    };
    if let Some(code) = v.get("code").and_then(|c| c.as_str()) {
        let mut s = format!("error={code}");
        if let Some(details) = v.get("details") {
            s.push_str(&format!(" details={details}"));
        }
        return s;
    }
    if let Some(selected) = v.get("selectedProtocol") {
        return format!("selectedProtocol={selected}");
    }
    "(ok)".into()
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let port: u16 = args
        .windows(2)
        .find(|w| w[0] == "--port")
        .and_then(|w| w[1].parse().ok())
        .unwrap_or(0);
    let connections: usize = args
        .windows(2)
        .find(|w| w[0] == "--connections")
        .and_then(|w| w[1].parse().ok())
        .unwrap_or(1);

    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("bind failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    let actual = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    log(&format!("LISTENING {actual}"));
    log(&format!(
        "supported {}..={} (lingxi.wire)",
        WIRE_PROTOCOL_MIN_SUPPORTED, WIRE_PROTOCOL_MAX_SUPPORTED
    ));

    for i in 0..connections {
        match listener.accept() {
            Ok((stream, _)) => {
                if let Err(e) = handle_connection(stream, i + 1) {
                    log(&format!("conn#{} error: {e}", i + 1));
                }
            }
            Err(e) => {
                eprintln!("accept failed: {e}");
                return ExitCode::FAILURE;
            }
        }
    }
    log("DONE");
    ExitCode::SUCCESS
}
