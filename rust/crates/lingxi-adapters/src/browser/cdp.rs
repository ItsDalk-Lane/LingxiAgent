//! 有界的NUL分帧CDP管道；退出、取消与超时都清理等待者。
use super::{error, BrowserError};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, VecDeque},
    os::fd::{AsRawFd, OwnedFd},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Condvar, Mutex,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

const FRAME_LIMIT: usize = 32 * 1024 * 1024;
const EVENT_LIMIT: usize = 512;
const EVENT_BYTES: usize = 4 * 1024 * 1024;
const PENDING_LIMIT: usize = 16;

fn remote_error(method: &str, remote: &Value) -> BrowserError {
    let code = remote["code"].as_i64().unwrap_or(0);
    // 仅保留CDP自身诊断，不转储请求参数、error.data或任意页面正文。
    let raw = remote["message"].as_str().unwrap_or("CDP command failed");
    let message: String = raw.chars().filter(|c| !c.is_control()).take(256).collect();
    let message = message
        .split_whitespace()
        .map(|word| {
            if word.contains("://") || word.starts_with('/') || word.len() > 64 {
                "[已隐藏]"
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    error(
        "browser_remote",
        &format!("浏览器命令失败: {method} (CDP {code}: {message})"),
        false,
    )
}

#[derive(Clone)]
pub(super) struct Event {
    pub sequence: u64,
    pub value: Value,
    bytes: usize,
}
struct State {
    pending: HashMap<u64, mpsc::Sender<Result<Value, BrowserError>>>,
    events: VecDeque<Event>,
    event_bytes: usize,
    sequence: u64,
    terminal: Option<BrowserError>,
}
struct Inner {
    state: Mutex<State>,
    wake: Condvar,
    stop: AtomicBool,
}
pub(super) struct Cdp {
    inner: Arc<Inner>,
    writer: Mutex<OwnedFd>,
    next: AtomicU64,
    reader: Mutex<Option<JoinHandle<()>>>,
}

fn fail(inner: &Inner, cause: BrowserError) -> BrowserError {
    let mut state = inner.state.lock().unwrap_or_else(|p| p.into_inner());
    let cause = state.terminal.get_or_insert(cause).clone();
    // 先固定首因，再发布关闭；所有等待者和后续调用必须看见同一原因。
    inner.stop.store(true, Ordering::Release);
    for (_, sender) in state.pending.drain() {
        let _ = sender.send(Err(cause.clone()));
    }
    inner.wake.notify_all();
    cause
}
pub(super) fn safe_identifier(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(*c, '.' | '_' | '-'))
        .take(128)
        .collect()
}
pub(super) fn command_context(
    mut cause: BrowserError,
    method: &str,
    session: Option<&str>,
) -> BrowserError {
    // 只记录协议标识，绝不把参数、页面正文或输入内容拼入诊断。
    cause.message = format!(
        "{} [method={}, session={}]",
        cause.message,
        safe_identifier(method),
        safe_identifier(session.unwrap_or("browser"))
    );
    cause
}
fn nonblocking(fd: &OwnedFd) -> Result<(), BrowserError> {
    // 句柄已由当前对象持有，fcntl不转移所有权。
    let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
    if flags < 0
        || unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        return Err(error("browser_pipe", "无法设置浏览器管道", false));
    }
    Ok(())
}
fn ready(fd: &OwnedFd, flags: i16, timeout: Duration) -> Result<bool, BrowserError> {
    let mut p = libc::pollfd {
        fd: fd.as_raw_fd(),
        events: flags,
        revents: 0,
    };
    let ms = timeout.as_millis().clamp(1, 25) as i32;
    let n = unsafe { libc::poll(&mut p, 1, ms) };
    if n < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
        return Err(error("browser_pipe", "浏览器管道不可读写", false));
    }
    Ok(n > 0)
}
fn interrupted(cancel: &AtomicBool, deadline: Instant, unknown: bool) -> Result<(), BrowserError> {
    if cancel.load(Ordering::Acquire) {
        return Err(error("cancelled", "浏览器操作已取消", unknown));
    }
    if Instant::now() >= deadline {
        return Err(error("browser_timeout", "浏览器操作超时", unknown));
    }
    Ok(())
}

impl Cdp {
    pub fn new(read: OwnedFd, write: OwnedFd) -> Result<Arc<Self>, BrowserError> {
        nonblocking(&read)?;
        nonblocking(&write)?;
        let inner = Arc::new(Inner {
            state: Mutex::new(State {
                pending: HashMap::new(),
                events: VecDeque::new(),
                event_bytes: 0,
                sequence: 0,
                terminal: None,
            }),
            wake: Condvar::new(),
            stop: AtomicBool::new(false),
        });
        let reader_inner = inner.clone();
        let reader = std::thread::Builder::new()
            .name("browser-cdp-reader".into())
            .spawn(move || {
                let mut buffer = Vec::new();
                let mut chunk = [0_u8; 16384];
                let outcome = (|| -> Result<(), BrowserError> {
                    while !reader_inner.stop.load(Ordering::Acquire) {
                        if !ready(&read, libc::POLLIN, Duration::from_millis(25))? {
                            continue;
                        }
                        let n = unsafe {
                            libc::read(read.as_raw_fd(), chunk.as_mut_ptr().cast(), chunk.len())
                        };
                        if n == 0 {
                            return Err(error("browser_closed", "浏览器读取管道已到EOF", true));
                        }
                        if n < 0 {
                            let e = std::io::Error::last_os_error();
                            if matches!(
                                e.kind(),
                                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                            ) {
                                continue;
                            }
                            return Err(error("browser_pipe", "读取浏览器失败", true));
                        }
                        for byte in &chunk[..n as usize] {
                            if *byte != 0 {
                                if buffer.len() >= FRAME_LIMIT {
                                    return Err(error(
                                        "browser_budget",
                                        "浏览器消息超出上限",
                                        true,
                                    ));
                                }
                                buffer.push(*byte);
                                continue;
                            }
                            if buffer.is_empty() {
                                continue;
                            }
                            let value: Value = serde_json::from_slice(&buffer).map_err(|_| {
                                error("browser_protocol", "浏览器消息格式错误", true)
                            })?;
                            let bytes = buffer.len();
                            buffer.clear();
                            let mut s =
                                reader_inner.state.lock().unwrap_or_else(|p| p.into_inner());
                            if let Some(id) = value["id"].as_u64() {
                                if let Some(tx) = s.pending.remove(&id) {
                                    let _ = tx.send(Ok(value));
                                }
                            } else {
                                if bytes > EVENT_BYTES {
                                    return Err(error(
                                        "browser_budget",
                                        "浏览器事件超出上限",
                                        true,
                                    ));
                                }
                                s.sequence = s.sequence.checked_add(1).ok_or_else(|| {
                                    error("browser_budget", "浏览器事件序号耗尽", true)
                                })?;
                                while s.events.len() >= EVENT_LIMIT
                                    || s.event_bytes + bytes > EVENT_BYTES
                                {
                                    if let Some(old) = s.events.pop_front() {
                                        s.event_bytes -= old.bytes;
                                    } else {
                                        break;
                                    }
                                }
                                let sequence = s.sequence;
                                s.events.push_back(Event {
                                    sequence,
                                    value,
                                    bytes,
                                });
                                s.event_bytes += bytes;
                                reader_inner.wake.notify_all();
                            }
                        }
                    }
                    Ok(())
                })();
                fail(
                    &reader_inner,
                    outcome
                        .err()
                        .unwrap_or_else(|| error("browser_closed", "浏览器连接已停止", true)),
                );
            })
            .map_err(|_| error("browser_pipe", "无法建立浏览器读取线程", false))?;
        Ok(Arc::new(Self {
            inner,
            writer: Mutex::new(write),
            next: AtomicU64::new(1),
            reader: Mutex::new(Some(reader)),
        }))
    }

    pub fn close(&self) {
        self.close_with(error("browser_closed", "宿主主动关闭浏览器连接", true));
    }
    pub fn close_with(&self, cause: BrowserError) -> BrowserError {
        fail(&self.inner, cause)
    }
    pub fn terminal_cause(&self) -> Option<BrowserError> {
        self.inner
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .terminal
            .clone()
    }
    fn closed_for_call(&self, unknown: bool) -> BrowserError {
        let mut cause = self
            .terminal_cause()
            .unwrap_or_else(|| error("browser_closed", "浏览器已退出", false));
        cause.effect_unknown = unknown;
        cause
    }
    pub fn is_closed(&self) -> bool {
        self.inner.stop.load(Ordering::Acquire)
    }
    pub fn cursor(&self) -> u64 {
        self.inner
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .sequence
    }
    pub fn call(
        &self,
        session: Option<&str>,
        method: &str,
        params: Value,
        deadline: Instant,
        cancel: &AtomicBool,
        effect: bool,
    ) -> Result<Value, BrowserError> {
        interrupted(cancel, deadline, false)?;
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let mut request = json!({"id":id,"method":method,"params":params});
        if let Some(sid) = session {
            request["sessionId"] = json!(sid);
        }
        let mut encoded = serde_json::to_vec(&request)
            .map_err(|_| error("browser_protocol", "无法编码浏览器命令", false))?;
        if encoded.len() >= FRAME_LIMIT {
            return Err(error("browser_budget", "浏览器命令超出上限", false));
        }
        encoded.push(0);
        let (tx, rx) = mpsc::channel();
        {
            let mut state = self.inner.state.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(e) = &state.terminal {
                let mut cause = e.clone();
                cause.effect_unknown = false;
                return Err(cause);
            }
            if state.pending.len() >= PENDING_LIMIT {
                return Err(error("browser_busy", "浏览器请求已达上限", false));
            }
            state.pending.insert(id, tx);
        }
        let mut written = false;
        let result = (|| {
            let writer = loop {
                interrupted(cancel, deadline, false)?;
                if self.is_closed() {
                    return Err(self.closed_for_call(false));
                }
                match self.writer.try_lock() {
                    Ok(g) => break g,
                    Err(std::sync::TryLockError::Poisoned(p)) => break p.into_inner(),
                    Err(std::sync::TryLockError::WouldBlock) => {
                        std::thread::sleep(Duration::from_millis(2))
                    }
                }
            };
            let mut offset = 0;
            while offset < encoded.len() {
                interrupted(cancel, deadline, written && effect)?;
                if self.is_closed() {
                    return Err(self.closed_for_call(written && effect));
                }
                if !ready(
                    &writer,
                    libc::POLLOUT,
                    deadline.saturating_duration_since(Instant::now()),
                )? {
                    continue;
                }
                let n = unsafe {
                    libc::write(
                        writer.as_raw_fd(),
                        encoded[offset..].as_ptr().cast(),
                        encoded.len() - offset,
                    )
                };
                if n < 0 {
                    let e = std::io::Error::last_os_error();
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) {
                        continue;
                    }
                    return Err(error(
                        "browser_pipe",
                        "发送浏览器命令失败",
                        written && effect,
                    ));
                }
                if n == 0 {
                    return Err(error("browser_pipe", "浏览器管道已关闭", written && effect));
                }
                offset += n as usize;
                written = true;
            }
            drop(writer);
            loop {
                interrupted(cancel, deadline, written && effect)?;
                match rx.recv_timeout(
                    Duration::from_millis(20)
                        .min(deadline.saturating_duration_since(Instant::now())),
                ) {
                    Ok(Ok(response)) => {
                        if let Some(remote) = response.get("error") {
                            return Err(remote_error(method, remote));
                        }
                        return Ok(response.get("result").cloned().unwrap_or(Value::Null));
                    }
                    Ok(Err(mut e)) => {
                        e.effect_unknown = written && effect;
                        return Err(e);
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        return Err(self.closed_for_call(written && effect))
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
        })();
        let result = result.map_err(|cause| command_context(cause, method, session));
        self.inner
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .pending
            .remove(&id);
        // 半帧不能与下一条命令拼接；关闭整个连接使真实效果保持未知。
        if let Some(cause) = result.as_ref().err().filter(|cause| {
            written
                && matches!(
                    cause.code.as_str(),
                    "browser_pipe" | "cancelled" | "browser_timeout"
                )
        }) {
            self.close_with(cause.clone());
        }
        result
    }

    pub fn events(&self, after: u64) -> Result<Vec<Event>, BrowserError> {
        let s = self.inner.state.lock().unwrap_or_else(|p| p.into_inner());
        if s.events
            .front()
            .is_some_and(|e| after.saturating_add(1) < e.sequence)
        {
            return Err(error("browser_events_lost", "浏览器事件窗口已过期", true));
        }
        Ok(s.events
            .iter()
            .filter(|e| e.sequence > after)
            .cloned()
            .collect())
    }
    pub fn wait_event(
        &self,
        after: u64,
        predicate: impl Fn(&Value) -> bool,
        deadline: Instant,
        cancel: &AtomicBool,
    ) -> Result<Value, BrowserError> {
        loop {
            interrupted(cancel, deadline, true)?;
            if let Some(event) = self
                .events(after)?
                .into_iter()
                .find(|e| predicate(&e.value))
            {
                return Ok(event.value);
            }
            let s = self.inner.state.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(e) = &s.terminal {
                return Err(e.clone());
            }
            drop(
                self.inner
                    .wake
                    .wait_timeout(
                        s,
                        Duration::from_millis(20)
                            .min(deadline.saturating_duration_since(Instant::now())),
                    )
                    .unwrap_or_else(|p| p.into_inner()),
            );
        }
    }
}
impl Drop for Cdp {
    fn drop(&mut self) {
        self.close();
        if let Some(reader) = self.reader.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let _ = reader.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs::File,
        io::{Read, Write},
        os::fd::FromRawFd,
    };
    fn pair() -> (OwnedFd, OwnedFd) {
        let mut fds = [-1; 2];
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) }
    }
    fn read_command(file: &mut File) -> Value {
        let mut frame = Vec::new();
        loop {
            let mut byte = [0_u8; 1];
            file.read_exact(&mut byte).unwrap();
            if byte[0] == 0 {
                return serde_json::from_slice(&frame).unwrap();
            }
            frame.push(byte[0]);
            assert!(frame.len() < FRAME_LIMIT);
        }
    }
    #[test]
    fn first_close_cause_reaches_written_and_unwritten_calls_with_their_own_effects() {
        let (read, write) = pair();
        let (response, _answer) = pair();
        let cdp = Cdp::new(response, write).unwrap();
        let (ready, reached) = mpsc::channel();
        let peer = std::thread::spawn(move || {
            let mut file = File::from(read);
            for _ in 0..2 {
                let request = read_command(&mut file);
                assert_eq!(request["params"]["password"], "fixture-private-marker");
            }
            ready.send(()).unwrap();
        });
        let calls: Vec<_> = [true, false]
            .into_iter()
            .map(|effect| {
                let cdp = cdp.clone();
                std::thread::spawn(move || {
                    cdp.call(
                        Some("fixture-session"),
                        "Runtime.evaluate",
                        json!({"password":"fixture-private-marker"}),
                        Instant::now() + Duration::from_secs(10),
                        &AtomicBool::new(false),
                        effect,
                    )
                    .unwrap_err()
                })
            })
            .collect();
        // peer实际读到两个完整帧后才终止，不能靠sleep猜测是否写出。
        reached.recv_timeout(Duration::from_secs(5)).unwrap();
        let first = error("browser_events_lost", "原事件窗口已失效", true);
        assert_eq!(cdp.close_with(first.clone()).message, first.message);
        assert_eq!(
            cdp.close_with(error("browser_timeout", "后来的错误", true))
                .message,
            first.message
        );
        cdp.close();
        for (call, effect) in calls.into_iter().zip([true, false]) {
            let cause = call.join().unwrap();
            assert_eq!(cause.code, first.code);
            assert!(cause.message.starts_with(&first.message));
            assert!(cause.message.contains("method=Runtime.evaluate"));
            assert!(cause.message.contains("session=fixture-session"));
            assert!(!cause.message.contains("fixture-private-marker"));
            assert_eq!(cause.effect_unknown, effect);
        }
        let unwritten = cdp
            .call(
                None,
                "Target.createTarget",
                json!({}),
                Instant::now() + Duration::from_secs(10),
                &AtomicBool::new(false),
                true,
            )
            .unwrap_err();
        assert_eq!(unwritten.code, first.code);
        assert_eq!(unwritten.message, first.message);
        assert!(!unwritten.effect_unknown);
        assert!(cdp.inner.state.lock().unwrap().pending.is_empty());
        peer.join().unwrap();
    }
    #[test]
    fn written_cooperative_cancellation_keeps_its_cause_for_later_unwritten_calls() {
        let (read, write) = pair();
        let (response, _answer) = pair();
        let cdp = Cdp::new(response, write).unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let peer_cancel = cancel.clone();
        let peer = std::thread::spawn(move || {
            let request = read_command(&mut File::from(read));
            assert_eq!(request["method"], "Runtime.evaluate");
            // 完整请求确已写出，随后取消；peer故意不发送响应。
            peer_cancel.store(true, Ordering::Release);
        });
        let cancelled = cdp
            .call(
                Some("cancel-session"),
                "Runtime.evaluate",
                json!({}),
                Instant::now() + Duration::from_secs(10),
                &cancel,
                true,
            )
            .unwrap_err();
        assert_eq!(cancelled.code, "cancelled");
        assert!(cancelled.effect_unknown);
        assert!(cancelled.message.contains("method=Runtime.evaluate"));
        assert!(cdp.is_closed());
        let later = cdp
            .call(
                None,
                "Target.createTarget",
                json!({}),
                Instant::now() + Duration::from_secs(10),
                &AtomicBool::new(false),
                true,
            )
            .unwrap_err();
        assert_eq!(later.code, cancelled.code);
        assert_eq!(later.message, cancelled.message);
        assert!(!later.effect_unknown);
        assert!(cdp.inner.state.lock().unwrap().pending.is_empty());
        peer.join().unwrap();
    }
    #[test]
    fn eof_wakes_pending_commands_without_waiting_for_their_full_deadline() {
        let (read, write) = pair();
        let (response, answer) = pair();
        let cdp = Cdp::new(response, write).unwrap();
        let worker = std::thread::spawn(move || {
            let mut file = File::from(read);
            let mut b = [0u8; 256];
            let _ = file.read(&mut b);
            drop(answer);
        });
        let started = Instant::now();
        let e = cdp
            .call(
                None,
                "Runtime.evaluate",
                json!({}),
                started + Duration::from_secs(30),
                &AtomicBool::new(false),
                true,
            )
            .unwrap_err();
        assert_eq!(e.code, "browser_closed");
        assert!(e.effect_unknown);
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(cdp.inner.state.lock().unwrap().pending.is_empty());
        assert!(e.message.contains("EOF"));
        let later = cdp
            .call(
                None,
                "Target.createTarget",
                json!({}),
                Instant::now() + Duration::from_secs(10),
                &AtomicBool::new(false),
                true,
            )
            .unwrap_err();
        assert_eq!(later.code, "browser_closed");
        assert!(later.message.contains("EOF"));
        assert!(!later.effect_unknown);
        worker.join().unwrap();
    }
    #[test]
    fn a_stalled_pipe_write_obeys_the_same_deadline_and_removes_its_pending_id() {
        let (_read, write) = pair();
        let (response, _answer) = pair();
        let cdp = Cdp::new(response, write).unwrap();
        let started = Instant::now();
        let e = cdp
            .call(
                None,
                "Runtime.evaluate",
                json!({"expression":"x".repeat(1024*1024)}),
                started + Duration::from_millis(60),
                &AtomicBool::new(false),
                true,
            )
            .unwrap_err();
        assert_eq!(e.code, "browser_timeout");
        assert!(e.effect_unknown);
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(cdp.inner.state.lock().unwrap().pending.is_empty());
        assert!(cdp.is_closed());
        let later = cdp
            .call(
                None,
                "Target.createTarget",
                json!({}),
                Instant::now() + Duration::from_secs(10),
                &AtomicBool::new(false),
                true,
            )
            .unwrap_err();
        assert_eq!(later.code, "browser_timeout");
        assert!(!later.effect_unknown);
    }
    #[test]
    fn buffered_events_have_monotonic_cursors_and_old_windows_fail_explicitly() {
        let (_read, write) = pair();
        let (response, answer) = pair();
        let cdp = Cdp::new(response, write).unwrap();
        let mut answer = File::from(answer);
        for i in 0..600 {
            let mut bytes =
                serde_json::to_vec(&json!({"method":"Page.test","params":{"index":i}})).unwrap();
            bytes.push(0);
            answer.write_all(&bytes).unwrap();
        }
        let until = Instant::now() + Duration::from_secs(2);
        while cdp.cursor() < 600 {
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(cdp.events(0).err().unwrap().code, "browser_events_lost");
        assert_eq!(cdp.events(599).unwrap().len(), 1);
        assert!(cdp.inner.state.lock().unwrap().events.len() <= EVENT_LIMIT);
    }
    #[test]
    fn malformed_messages_wake_both_event_and_command_waiters() {
        let (_read, write) = pair();
        let (response, answer) = pair();
        let cdp = Cdp::new(response, write).unwrap();
        let mut answer = File::from(answer);
        answer.write_all(b"not-json\0").unwrap();
        let e = cdp
            .wait_event(
                0,
                |_| false,
                Instant::now() + Duration::from_secs(10),
                &AtomicBool::new(false),
            )
            .unwrap_err();
        assert_eq!(e.code, "browser_protocol");
        assert!(cdp.inner.state.lock().unwrap().pending.is_empty());
    }
}
