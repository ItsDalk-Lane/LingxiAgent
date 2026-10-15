//! 知识文档的受控解析：原始字节只经管道进入独立进程，位置来自成熟解析器真实回调。
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

pub const WORKER_FLAG: &str = "--knowledge-parser-worker";
const MAX_INPUT: usize = 50 * 1024 * 1024;
const MAX_OUTPUT: usize = 16 * 1024 * 1024;
const MAX_PAGES: usize = 1000;
const MAX_ITEMS: usize = 200_000;
const MAX_BLOCKS: usize = 50_000;
const MAX_TEXT: usize = 8 * 1024 * 1024;
const MAX_RSS: u64 = 512 * 1024 * 1024;
// 一个宿主进程只持有一个解析租约，避免并发导入把每进程上限相乘。
static PARSER_SLOT: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedBlock {
    pub text: String,
    pub locator_type: String,
    pub locator: Value,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedDocument {
    pub status: String,
    pub warnings: Vec<String>,
    pub fidelity: String,
    pub blocks: Vec<ParsedBlock>,
}
#[derive(Serialize, Deserialize)]
struct WorkerReply {
    version: u32,
    document: Option<ParsedDocument>,
    error: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct WorkerRequest {
    version: u32,
    mime: String,
}

/// 宿主传入本次候选服务可执行文件；文件路径、URL和权限均不交给解析进程。
pub fn parse_document(
    worker_binary: &Path,
    mime: &str,
    bytes: &[u8],
) -> Result<ParsedDocument, String> {
    parse_document_while_active(worker_binary, mime, bytes, || true)
}
/// 调用方只传可信任务存活状态；取消后先杀死并回收子进程，再返回。
pub fn parse_document_while_active(
    worker_binary: &Path,
    mime: &str,
    bytes: &[u8],
    active: impl Fn() -> bool,
) -> Result<ParsedDocument, String> {
    if !active() {
        return Err("document parser cancelled before spawn".into());
    }
    let _lease = PARSER_SLOT.try_lock().map_err(|_| {
        "document parser is busy; retry after the active parse finishes".to_string()
    })?;
    if !cfg!(any(target_os = "macos", target_os = "linux")) {
        return Err("document parser supervision is unsupported on this platform".into());
    }
    if !worker_binary.is_absolute() {
        return Err("parser binary must be absolute".into());
    }
    if bytes.len() > MAX_INPUT {
        return Err("document input exceeds 50MiB".into());
    }
    let mut command = Command::new(worker_binary);
    command
        .arg(WORKER_FLAG)
        .env_clear()
        .env("TOKIO_WORKER_THREADS", "1")
        .current_dir("/")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // 真实CPU限制；macOS不声称地址空间/文件系统沙盒，内存另由宿主监测。
        unsafe {
            command.pre_exec(|| {
                let core = libc::rlimit {
                    rlim_cur: 0,
                    rlim_max: 0,
                };
                if libc::setrlimit(libc::RLIMIT_CORE, &core) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                let limit = libc::rlimit {
                    rlim_cur: 5,
                    rlim_max: 5,
                };
                if libc::setrlimit(libc::RLIMIT_CPU, &limit) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                #[cfg(target_os = "linux")]
                {
                    let memory = libc::rlimit {
                        rlim_cur: MAX_RSS,
                        rlim_max: MAX_RSS,
                    };
                    if libc::setrlimit(libc::RLIMIT_AS, &memory) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
    }
    let mut child = OwnedChild {
        child: command
            .spawn()
            .map_err(|e| format!("parser spawn failed: {e}"))?,
        reaped: false,
    };
    let mut stdin = child.child.stdin.take().ok_or("parser stdin missing")?;
    let stdout = child.child.stdout.take().ok_or("parser stdout missing")?;
    let stderr = child.child.stderr.take().ok_or("parser stderr missing")?;
    let header = serde_json::to_vec(&WorkerRequest {
        version: 1,
        mime: mime.into(),
    })
    .map_err(|e| e.to_string())?;
    let input = bytes.to_vec();
    let writer = std::thread::spawn(move || -> std::io::Result<()> {
        stdin.write_all(&header)?;
        stdin.write_all(b"\n")?;
        stdin.write_all(&input)
    });
    let overflow = Arc::new(AtomicBool::new(false));
    let marker = overflow.clone();
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        let result = stdout.take(MAX_OUTPUT as u64 + 1).read_to_end(&mut out);
        if out.len() > MAX_OUTPUT {
            marker.store(true, Ordering::Release);
        }
        (result, out)
    });
    let errors = std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = stderr.take(65537).read_to_end(&mut out);
        out
    });
    let started = Instant::now();
    let mut observation_missing = None;
    let verdict = loop {
        if !active() {
            break Err("document parser cancelled".into());
        }
        if overflow.load(Ordering::Acquire) {
            break Err("document parser output exceeds 16MiB".into());
        }
        if started.elapsed() > Duration::from_secs(5) {
            break Err("document parser exceeded five second deadline".into());
        }
        match child.child.try_wait() {
            Ok(Some(status)) => {
                child.reaped = true;
                break if status.success() {
                    Ok(())
                } else {
                    Err(format!("document parser exited unsuccessfully: {status}"))
                };
            }
            Ok(None) => {}
            Err(e) => break Err(format!("parser wait failed: {e}")),
        }
        match resident_bytes(child.child.id()) {
            Ok(rss) if rss > MAX_RSS => {
                break Err("document parser exceeded 512MiB observed RSS watermark".into())
            }
            Ok(_) => {
                observation_missing = None;
            }
            Err(error) => match child.child.try_wait() {
                Ok(Some(status)) => {
                    child.reaped = true;
                    break if status.success() {
                        Ok(())
                    } else {
                        Err(format!("document parser exited unsuccessfully: {status}"))
                    };
                }
                _ => {
                    let missing = observation_missing.get_or_insert_with(Instant::now);
                    if missing.elapsed() > Duration::from_millis(50) {
                        break Err(error);
                    }
                }
            },
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    if !child.reaped {
        child.kill_and_reap()?;
    }
    let _ = writer.join().map_err(|_| "parser input writer panicked")?;
    let (read, output) = reader.join().map_err(|_| "parser output reader panicked")?;
    read.map_err(|e| format!("parser output failed: {e}"))?;
    let _stderr = errors.join().map_err(|_| "parser stderr reader panicked")?;
    verdict?;
    if output.len() > MAX_OUTPUT {
        return Err("document parser output exceeds 16MiB".into());
    }
    let reply: WorkerReply =
        serde_json::from_slice(&output).map_err(|_| "document parser response is invalid")?;
    if reply.version != 1 {
        return Err("document parser protocol mismatch".into());
    }
    match (reply.document, reply.error) {
        (Some(document), None) => {
            validate_document(&document)?;
            Ok(document)
        }
        (None, Some(error)) => Err(error),
        _ => Err("document parser response is ambiguous".into()),
    }
}
struct OwnedChild {
    child: Child,
    reaped: bool,
}
impl OwnedChild {
    fn kill_and_reap(&mut self) -> Result<(), String> {
        if self.reaped {
            return Ok(());
        }
        let kill = self.child.kill();
        let wait = self.child.wait();
        match wait {
            Ok(_) => {
                self.reaped = true;
                Ok(())
            }
            Err(error) => Err(format!(
                "parser cleanup is unconfirmed: kill={kill:?}, wait={error}"
            )),
        }
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if !self.reaped {
            if let Err(error) = self.kill_and_reap() {
                tracing::error!(%error,"parser cleanup failed");
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn resident_bytes(pid: u32) -> Result<u64, String> {
    let mut info = std::mem::MaybeUninit::<libc::proc_taskinfo>::zeroed();
    // 句柄仍由父进程持有且未reap；这里只观测本次真实子进程的内存。
    let size = std::mem::size_of::<libc::proc_taskinfo>();
    let result = unsafe {
        libc::proc_pidinfo(
            pid as i32,
            libc::PROC_PIDTASKINFO,
            0,
            info.as_mut_ptr().cast(),
            size as i32,
        )
    };
    if result == size as i32 {
        Ok(unsafe { info.assume_init() }.pti_resident_size)
    } else {
        Err("parser RSS observation failed".into())
    }
}
#[cfg(target_os = "linux")]
fn resident_bytes(pid: u32) -> Result<u64, String> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status"))
        .map_err(|e| format!("parser RSS observation failed: {e}"))?;
    let kb = status
        .lines()
        .find_map(|s| s.strip_prefix("VmRSS:"))
        .and_then(|s| s.split_whitespace().next())
        .and_then(|s| s.parse::<u64>().ok())
        .ok_or("parser RSS unavailable")?;
    Ok(kb.saturating_mul(1024))
}
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn resident_bytes(_pid: u32) -> Result<u64, String> {
    Err("document parser memory supervision unavailable on this platform".into())
}

/// 仅早期服务子命令调用：不初始化业务home、数据库、网络或模型。
pub fn worker_main() -> i32 {
    let result = (|| -> Result<ParsedDocument, String> {
        let mut raw = Vec::new();
        std::io::stdin()
            .take((MAX_INPUT + 1025) as u64)
            .read_to_end(&mut raw)
            .map_err(|e| e.to_string())?;
        let newline = raw
            .iter()
            .position(|b| *b == b'\n')
            .ok_or("parser request header missing")?;
        if newline > 1024 {
            return Err("parser request header too large".into());
        }
        let request: WorkerRequest =
            serde_json::from_slice(&raw[..newline]).map_err(|_| "parser request header invalid")?;
        if request.version != 1 {
            return Err("parser request version unsupported".into());
        }
        let bytes = &raw[newline + 1..];
        if bytes.len() > MAX_INPUT {
            return Err("parser input exceeds 50MiB".into());
        }
        match request.mime.as_str() {
            "application/pdf" => parse_pdf(bytes),
            "text/html" => parse_html(bytes, false),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => {
                let document = crate::office::docx(bytes)?;
                parse_html(document.html.as_bytes(), true)
            }
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => {
                parse_xlsx(bytes)
            }
            "text/csv" => parse_csv(bytes),
            _ => Err("document processor format unsupported".into()),
        }
    })();
    let reply = match result {
        Ok(document) => WorkerReply {
            version: 1,
            document: Some(document),
            error: None,
        },
        Err(error) => WorkerReply {
            version: 1,
            document: None,
            error: Some(error.chars().take(1024).collect()),
        },
    };
    let output = match serde_json::to_vec(&reply) {
        Ok(output) if output.len() <= MAX_OUTPUT => output,
        _ => {
            b"{\"version\":1,\"document\":null,\"error\":\"parser output limit exceeded\"}".to_vec()
        }
    };
    if std::io::stdout().write_all(&output).is_ok() {
        0
    } else {
        2
    }
}
fn validate_document(document: &ParsedDocument) -> Result<(), String> {
    if !matches!(document.status.as_str(), "ready" | "needs_ocr")
        || !matches!(document.fidelity.as_str(), "citation_grade" | "structural")
    {
        return Err("parser status or fidelity invalid".into());
    }
    if document.blocks.len() > MAX_BLOCKS
        || document.blocks.iter().map(|b| b.text.len()).sum::<usize>() > MAX_TEXT
    {
        return Err("parser block budget exceeded".into());
    }
    if document.status == "needs_ocr" && !document.blocks.is_empty() {
        return Err("OCR status cannot carry invented text".into());
    }
    for block in &document.blocks {
        if block.text.trim().is_empty() || block.text.contains('\0') {
            return Err("parser block text invalid".into());
        }
    }
    Ok(())
}

#[derive(Default)]
struct PdfCollector {
    page: u32,
    cursor: usize,
    item: usize,
    start_item: usize,
    line: String,
    boxes: Vec<Value>,
    blocks: Vec<ParsedBlock>,
    total_text: usize,
    total_items: usize,
    last_x: Option<f64>,
    last_y: Option<f64>,
    last_height: f64,
}
impl PdfCollector {
    fn fail() -> pdf_extract::OutputError {
        std::io::Error::other("PDF position or output budget exceeded").into()
    }
    fn flush(&mut self) -> Result<(), pdf_extract::OutputError> {
        let text = self.line.split_whitespace().collect::<Vec<_>>().join(" ");
        if !text.is_empty() {
            self.total_text += text.len();
            if self.blocks.len() >= MAX_BLOCKS || self.total_text > MAX_TEXT {
                return Err(Self::fail());
            }
            let length = text.encode_utf16().count();
            self.blocks.push(ParsedBlock{text,locator_type:"pdf".into(),locator:json!({"page":self.page,"pageCharStart":self.cursor,"pageCharEnd":self.cursor+length,"itemStart":self.start_item,"itemEnd":self.item,"itemUnit":"glyph","boxes":std::mem::take(&mut self.boxes)})});
            self.cursor += length + 1;
        }
        self.line.clear();
        self.boxes.clear();
        self.last_x = None;
        self.last_y = None;
        Ok(())
    }
}
impl pdf_extract::OutputDev for PdfCollector {
    fn begin_page(
        &mut self,
        page: u32,
        _media: &pdf_extract::MediaBox,
        _art: Option<(f64, f64, f64, f64)>,
    ) -> Result<(), pdf_extract::OutputError> {
        self.flush()?;
        self.page = page;
        self.cursor = 0;
        self.item = 0;
        self.start_item = 0;
        Ok(())
    }
    fn end_page(&mut self) -> Result<(), pdf_extract::OutputError> {
        self.flush()
    }
    fn output_character(
        &mut self,
        trm: &pdf_extract::Transform,
        width: f64,
        _spacing: f64,
        size: f64,
        text: &str,
    ) -> Result<(), pdf_extract::OutputError> {
        let (x, y) = (trm.m31, trm.m32);
        let w = (trm.m11.hypot(trm.m12) * width * size).abs();
        let h = (trm.m21.hypot(trm.m22) * size).abs();
        if [x, y, w, h, size]
            .iter()
            .any(|v| !v.is_finite() || v.abs() > 10_000_000.)
            || self.total_items >= MAX_ITEMS
            || text.len() > 65536
            || self
                .total_text
                .saturating_add(self.line.len())
                .saturating_add(text.len())
                > MAX_TEXT
        {
            return Err(Self::fail());
        }
        if self
            .last_y
            .is_some_and(|last| (last - y).abs() > self.last_height.max(h) * 0.5)
        {
            self.flush()?;
        }
        if self.line.is_empty() {
            self.start_item = self.item;
        } else if self.last_x.is_some_and(|last| x - last > h * 0.1)
            && !self.line.ends_with(char::is_whitespace)
        {
            self.line.push(' ');
        }
        self.item += 1;
        self.total_items += 1;
        self.line.push_str(text);
        self.boxes.push(json!({"x":x,"y":y,"width":w,"height":h,"fontSize":h,"dir":"unknown","transform":[trm.m11,trm.m12,trm.m21,trm.m22,trm.m31,trm.m32],"boxKind":"advance_cell"}));
        self.last_x = Some(x + w);
        self.last_y = Some(y);
        self.last_height = h;
        Ok(())
    }
    fn begin_word(&mut self) -> Result<(), pdf_extract::OutputError> {
        Ok(())
    }
    fn end_word(&mut self) -> Result<(), pdf_extract::OutputError> {
        Ok(())
    }
    fn end_line(&mut self) -> Result<(), pdf_extract::OutputError> {
        self.flush()
    }
}
fn parse_pdf(bytes: &[u8]) -> Result<ParsedDocument, String> {
    if !bytes.starts_with(b"%PDF-") {
        return Err("invalid PDF signature".into());
    }
    let document = pdf_extract::Document::load_mem(bytes).map_err(|_| "PDF decode failed")?;
    if document.is_encrypted() {
        return Err("encrypted PDF requires an unavailable password".into());
    }
    if document.objects.len() > MAX_ITEMS {
        return Err("PDF object count exceeds limit".into());
    }
    let pages = document.get_pages();
    if pages.is_empty() {
        return Err("PDF contains no pages".into());
    }
    if pages.len() > MAX_PAGES {
        return Err("PDF page count exceeds limit".into());
    }
    let mut collector = PdfCollector::default();
    for page in pages.keys() {
        pdf_extract::output_doc_page(&document, &mut collector, *page)
            .map_err(|e| format!("PDF text decode failed: {e}"))?;
    }
    let no_text = collector.blocks.is_empty();
    Ok(ParsedDocument {
        status: if no_text { "needs_ocr" } else { "ready" }.into(),
        warnings: if no_text {
            vec!["needs_ocr".into()]
        } else {
            vec![]
        },
        fidelity: "citation_grade".into(),
        blocks: collector.blocks,
    })
}

fn normalized(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
fn structural_document(
    rows: Vec<(String, Value)>,
    warnings: Vec<String>,
) -> Result<ParsedDocument, String> {
    let mut cursor = 0;
    let mut blocks = Vec::new();
    let mut total = 0usize;
    for (text, source) in rows {
        let text = normalized(&text);
        if text.is_empty() {
            continue;
        }
        total = total.saturating_add(text.len());
        if total > MAX_TEXT || blocks.len() >= MAX_BLOCKS {
            return Err("processed document output exceeds limit".into());
        }
        let len = text.encode_utf16().count();
        let line = blocks.len() + 1;
        blocks.push(ParsedBlock{text,locator_type:"text".into(),locator:json!({"lineStart":line,"lineEnd":line,"charStart":cursor,"charEnd":cursor+len,"source":source})});
        cursor += len + 1;
    }
    Ok(ParsedDocument {
        status: "ready".into(),
        warnings,
        fidelity: "structural".into(),
        blocks,
    })
}
fn csv_bytes(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    let mut out = Vec::with_capacity(bytes.len());
    let (mut i, mut quoted, mut closed, mut start, mut row_bytes) = (0, false, false, true, 0);
    while i < bytes.len() {
        let b = bytes[i];
        if quoted {
            out.push(b);
            row_bytes += 1;
            if b == b'"' {
                if bytes.get(i + 1) == Some(&b'"') {
                    i += 1;
                    out.push(b'"');
                    row_bytes += 1;
                } else {
                    quoted = false;
                    closed = true;
                }
            }
        } else if matches!(b, b'\r' | b'\n') {
            if row_bytes == 0 {
                out.extend_from_slice(b"\"\"");
            }
            out.push(b);
            if b == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
                i += 1;
                out.push(b'\n');
            }
            closed = false;
            start = true;
            row_bytes = 0;
        } else if b == b',' {
            out.push(b);
            closed = false;
            start = true;
            row_bytes += 1;
        } else if b == b'"' {
            if !start || closed {
                return Err("CSV unexpected quote".into());
            }
            out.push(b);
            quoted = true;
            start = false;
            row_bytes += 1;
        } else {
            if closed && !b.is_ascii_whitespace() {
                return Err("CSV unexpected data after closing quote".into());
            }
            out.push(b);
            start = false;
            row_bytes += 1;
        }
        i += 1;
    }
    if quoted {
        return Err("CSV missing closing quote".into());
    }
    Ok(out)
}
fn csv_cell(value: &str) -> String {
    use chrono::TimeZone;
    if value.is_empty() {
        return String::new();
    }
    let trimmed = value.trim();
    let number = if trimmed.is_empty() {
        Some(0.)
    } else if let Some((radix, digits)) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .map(|v| (16, v))
        .or_else(|| {
            trimmed
                .strip_prefix("0b")
                .or_else(|| trimmed.strip_prefix("0B"))
                .map(|v| (2, v))
        })
        .or_else(|| {
            trimmed
                .strip_prefix("0o")
                .or_else(|| trimmed.strip_prefix("0O"))
                .map(|v| (8, v))
        })
    {
        if digits.is_empty() {
            None
        } else {
            digits.chars().try_fold(0., |n, c| {
                c.to_digit(radix)
                    .map(|v| n * f64::from(radix) + f64::from(v))
            })
        }
    } else if matches!(trimmed, "inf" | "-inf" | "+inf" | "NaN") {
        None
    } else {
        trimmed.parse::<f64>().ok().filter(|n| n.is_finite())
    };
    if let Some(n) = number {
        if n == 0. {
            return "0".into();
        }
        if n.abs() >= 1e21 || n.abs() < 1e-6 {
            let text = format!("{n:e}");
            if let Some((mantissa, exponent)) = text.split_once('e') {
                return format!(
                    "{mantissa}e{}{exponent}",
                    if exponent.starts_with('-') { "" } else { "+" }
                );
            }
        }
        return n.to_string();
    }
    let local = if value.len() == 10 {
        ["%Y-%m-%d", "%m-%d-%Y"]
            .into_iter()
            .find_map(|format| {
                chrono::NaiveDate::parse_from_str(value, format)
                    .ok()
                    .filter(|d| d.format(format).to_string() == value)
            })
            .and_then(|date| date.and_hms_opt(0, 0, 0))
    } else if value.len() == 19 {
        chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S").ok()
    } else {
        None
    };
    if let Some(local) = local.and_then(|d| chrono::Local.from_local_datetime(&d).earliest()) {
        return local
            .with_timezone(&chrono::Utc)
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    }
    if matches!(
        value,
        "#N/A" | "#REF!" | "#NAME?" | "#DIV/0!" | "#NULL!" | "#VALUE!" | "#NUM!"
    ) {
        return "[object Object]".into();
    }
    value.into()
}
fn parse_csv(bytes: &[u8]) -> Result<ParsedDocument, String> {
    let bytes = csv_bytes(bytes)?;
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(bytes.as_slice());
    let mut rows = Vec::new();
    let mut cells = 0;
    let mut warnings = Vec::new();
    'records: for (i, record) in reader.records().enumerate() {
        let record = record.map_err(|e| format!("CSV decode failed: {e}"))?;
        if i >= 50_000 {
            warnings.push("row_count_truncated_at_50000".into());
            break;
        }
        let (mut values, mut refs) = (Vec::new(), Vec::new());
        for (column, value) in record.iter().enumerate() {
            if value.is_empty() {
                continue;
            }
            if cells >= 100_000 {
                warnings.push("cell_count_truncated_at_100000".into());
                break 'records;
            }
            cells += 1;
            let text = normalized(&csv_cell(value));
            if !text.is_empty() {
                values.push(text);
                refs.push(format!("R{}C{}", i + 1, column + 1));
            }
        }
        rows.push((
            values.join(" | "),
            json!({"kind":"csv_row","sheet":"sheet1","row":i+1,"cells":refs}),
        ));
    }
    structural_document(rows, warnings)
}
fn parse_xlsx(bytes: &[u8]) -> Result<ParsedDocument, String> {
    let (rows, warnings) = crate::office::xlsx_for_processing(bytes)?;
    structural_document(
        rows.into_iter()
            .map(|r| {
                (
                    r.values.join(" | "),
                    json!({"kind":"sheet_row","sheet":r.sheet,"row":r.row,"cells":r.cells}),
                )
            })
            .collect(),
        warnings,
    )
}
fn parse_html(bytes: &[u8], docx: bool) -> Result<ParsedDocument, String> {
    use scraper::{ElementRef, Html, Selector};
    let source = std::str::from_utf8(bytes).map_err(|_| "invalid_utf8")?;
    let document = Html::parse_document(source);
    let selector = Selector::parse("h1,h2,h3,h4,h5,h6,p,li,blockquote,pre,td,th")
        .map_err(|_| "HTML selector invalid")?;
    let mut rows = Vec::new();
    let mut headings: Vec<String> = Vec::new();
    for element in document.select(&selector) {
        if element.ancestors().filter_map(ElementRef::wrap).any(|a| {
            matches!(
                a.value().name(),
                "script" | "style" | "noscript" | "template"
            )
        }) {
            continue;
        }
        if element
            .select(&selector)
            .any(|child| child.id() != element.id())
        {
            continue;
        }
        let text = normalized(
            &element
                .descendants()
                .filter(|node| {
                    !node.ancestors().filter_map(ElementRef::wrap).any(|e| {
                        matches!(
                            e.value().name(),
                            "script" | "style" | "noscript" | "template"
                        )
                    })
                })
                .filter_map(|node| node.value().as_text().map(|text| text.to_string()))
                .collect::<Vec<_>>()
                .join(" "),
        );
        if text.is_empty() {
            continue;
        }
        let tag = element.value().name();
        if let Some(level) = tag
            .strip_prefix('h')
            .and_then(|n| n.parse::<usize>().ok())
            .filter(|n| (1..=6).contains(n))
        {
            headings.truncate(level - 1);
            headings.resize(level, String::new());
            headings[level - 1] = text.clone();
        }
        if rows.len() >= MAX_BLOCKS {
            return Err("HTML block count exceeds limit".into());
        }
        let heading = headings
            .iter()
            .filter(|s| !s.is_empty())
            .cloned()
            .collect::<Vec<_>>();
        if docx {
            let mut source = json!({"kind":"docx_paragraph","paragraphIndex":rows.len(),"tag":tag});
            if !heading.is_empty() {
                source["headingPath"] = json!(heading);
            }
            rows.push((text, source));
        } else {
            let mut path = Vec::new();
            let mut current = Some(element);
            while let Some(e) = current {
                let name = e.value().name();
                let index = e
                    .prev_siblings()
                    .filter_map(ElementRef::wrap)
                    .filter(|s| s.value().name() == name)
                    .count()
                    + 1;
                path.push(format!("{name}:nth-of-type({index})"));
                current = e.parent().and_then(ElementRef::wrap);
            }
            path.reverse();
            let length = text.encode_utf16().count();
            rows.push((text,json!({"structuralPath":path.join(" > "),"headingPath":heading,"characterRange":{"start":0,"end":length}})));
        }
    }
    if docx {
        structural_document(rows, Vec::new())
    } else {
        Ok(ParsedDocument {
            status: "ready".into(),
            warnings: Vec::new(),
            fidelity: "citation_grade".into(),
            blocks: rows
                .into_iter()
                .map(|(text, locator)| ParsedBlock {
                    text,
                    locator_type: "html".into(),
                    locator,
                })
                .collect(),
        })
    }
}
