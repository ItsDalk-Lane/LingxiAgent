//! 单个受控进程的页面、事件和下载；不保存业务数据库事实。
use super::{
    cdp::{command_context, safe_identifier, Cdp},
    error,
    launcher::Process,
    BrowserError,
};
use lingxi_kernel::browser::BrowserDownload;
use lingxi_protocol::BrowserTab;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc, Mutex,
    },
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub(super) const MAX_TABS: usize = 32;
pub(super) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
pub(super) fn timestamp() -> String {
    // 协议要求RFC3339，时间计算沿UTC；不引入额外日期依赖。
    let seconds = (now_ms() / 1000) as i64;
    let days = seconds / 86400;
    let time = seconds % 86400;
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    y += if m <= 2 { 1 } else { 0 };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        time / 3600,
        time / 60 % 60,
        time % 60
    )
}
pub(super) fn allowed_url(url: &str) -> bool {
    if url == "about:blank" {
        return true;
    }
    reqwest::Url::parse(url).is_ok_and(|u| {
        matches!(u.scheme(), "http" | "https")
            && u.host_str().is_some()
            && u.username().is_empty()
            && u.password().is_none()
    })
}
#[derive(Clone)]
pub(super) struct Attached {
    pub session: String,
    pub snapshot_loader: Option<String>,
    pub reference_range: Option<(u32, u32)>,
    pub reference_context: Option<u64>,
}
type Authority = Arc<dyn Fn() -> Result<(), BrowserError> + Send + Sync>;
pub(super) struct PreparedLive {
    download_dir: PathBuf,
    download_root: std::fs::File,
}
pub(super) struct Live {
    pub process: Process,
    pub tabs: Mutex<Vec<BrowserTab>>,
    pub attached: Mutex<HashMap<String, Attached>>,
    pub active: Mutex<Option<String>>,
    pub downloads: Mutex<Vec<BrowserDownload>>,
    pub visible: AtomicBool,
    pub takeover: AtomicBool,
    pub accept_cookies: AtomicBool,
    pub stop: Arc<AtomicBool>,
    pub network_generation: u64,
    pub download_dir: PathBuf,
    pub download_root: std::fs::File,
    pub failure: Mutex<Option<BrowserError>>,
    pub refs: AtomicU32,
    authority: Mutex<Option<Authority>>,
    operation_deadline: Mutex<Option<Instant>>,
    worker: Mutex<Option<JoinHandle<()>>>,
    refreshing: Mutex<()>,
}
impl Live {
    /// 目录固定会失败，必须在spawn前完成。
    pub fn prepare(download_dir: PathBuf) -> Result<PreparedLive, BrowserError> {
        use std::os::unix::fs::OpenOptionsExt;
        let download_root = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&download_dir)
            .map_err(|_| error("browser_download", "无法固定下载目录", false))?;
        Ok(PreparedLive {
            download_dir,
            download_root,
        })
    }
    /// spawn成功后无失败地建立owner；宿主先登记它，再调用initialize。
    pub fn allocate(
        process: Process,
        prepared: PreparedLive,
        network_generation: u64,
        accept_cookies: bool,
    ) -> Arc<Self> {
        Arc::new(Self {
            process,
            tabs: Mutex::new(Vec::new()),
            attached: Mutex::new(HashMap::new()),
            active: Mutex::new(None),
            downloads: Mutex::new(Vec::new()),
            visible: AtomicBool::new(false),
            takeover: AtomicBool::new(false),
            accept_cookies: AtomicBool::new(accept_cookies),
            stop: Arc::new(AtomicBool::new(false)),
            network_generation,
            download_dir: prepared.download_dir,
            download_root: prepared.download_root,
            failure: Mutex::new(None),
            refs: AtomicU32::new(65536),
            authority: Mutex::new(None),
            operation_deadline: Mutex::new(None),
            worker: Mutex::new(None),
            refreshing: Mutex::new(()),
        })
    }
    pub fn initialize(self: &Arc<Self>, startup_cancel: &AtomicBool) -> Result<(), BrowserError> {
        self.initialize_until(startup_cancel, Instant::now() + Duration::from_secs(8))
    }
    pub fn initialize_until(
        self: &Arc<Self>,
        startup_cancel: &AtomicBool,
        deadline: Instant,
    ) -> Result<(), BrowserError> {
        let deadline = deadline.min(Instant::now() + Duration::from_secs(8));
        self.call(
            None,
            "Browser.getVersion",
            json!({}),
            deadline,
            startup_cancel,
            false,
        )?;
        #[cfg(test)]
        if self
            .process
            .control
            .as_ref()
            .is_some_and(|control| control.fail_live_initialize.swap(false, Ordering::AcqRel))
        {
            return Err(error(
                "browser_startup_test_live",
                "测试注入Live初始化失败",
                false,
            ));
        }
        self.call(
            None,
            "Target.setDiscoverTargets",
            json!({"discover":true}),
            deadline,
            startup_cancel,
            false,
        )?;
        self.call(None,"Browser.setDownloadBehavior",json!({"behavior":"allowAndName","downloadPath":self.download_dir,"eventsEnabled":true}),deadline,startup_cancel,false)?;
        self.call(None,"Target.setAutoAttach",json!({"autoAttach":true,"waitForDebuggerOnStart":true,"flatten":true,"filter":[{"type":"page","exclude":false},{"exclude":true}]}),deadline,startup_cancel,false)?;
        let weak = Arc::downgrade(self);
        let worker = std::thread::Builder::new()
            .name("browser-events".into())
            .spawn(move || {
                let mut cursor = 0;
                let mut next_refresh = Instant::now();
                'worker: loop {
                    let Some(live) = weak.upgrade() else {
                        break;
                    };
                    if live.stop.load(Ordering::Acquire) {
                        break;
                    }
                    if live.process.cdp.is_closed() {
                        if let Some(cause) = live.process.cdp.terminal_cause() {
                            live.fail_worker(cause, "closed", cursor, None);
                        }
                        break;
                    }
                    let events = match live.process.cdp.events(cursor) {
                        Ok(e) => e,
                        Err(cause) => {
                            live.fail_worker(cause, "events", cursor, None);
                            break;
                        }
                    };
                    for event in events {
                        cursor = event.sequence;
                        if let Err(cause) = live.handle_event(&event.value) {
                            live.fail_worker(cause, "handle_event", cursor, Some(&event.value));
                            // 退出整个事件任务，不能在失败后继续当轮刷新制造次生错误。
                            break 'worker;
                        }
                    }
                    if Instant::now() >= next_refresh {
                        if let Err(cause) = live.refresh_inner(
                            Instant::now() + Duration::from_secs(2),
                            &live.stop,
                            false,
                        ) {
                            live.fail_worker(cause, "refresh", cursor, None);
                            break;
                        }
                        next_refresh = Instant::now() + Duration::from_millis(500);
                    }
                    drop(live);
                    std::thread::sleep(Duration::from_millis(20));
                }
            })
            .map_err(|_| error("browser_unavailable", "无法建立浏览器事件任务", false))?;
        *self.worker.lock().unwrap_or_else(|p| p.into_inner()) = Some(worker);
        Ok(())
    }
    pub fn cdp(&self) -> &Arc<Cdp> {
        &self.process.cdp
    }
    fn fail_worker(
        &self,
        mut cause: BrowserError,
        phase: &'static str,
        sequence: u64,
        event: Option<&Value>,
    ) {
        cause.message = format!(
            "{} [worker_phase={phase}, event_sequence={sequence}]",
            cause.message
        );
        let cause = self.process.cdp.close_with(cause);
        tracing::warn!(
            phase,
            sequence,
            method = %safe_identifier(event.and_then(|v| v["method"].as_str()).unwrap_or("")),
            session = %safe_identifier(event.and_then(|v| v["sessionId"].as_str()).unwrap_or("browser")),
            code = %safe_identifier(&cause.code),
            "浏览器事件任务因原错误退出"
        );
        self.failure
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get_or_insert(cause);
    }
    pub fn set_authority(&self, authority: Authority) {
        *self.authority.lock().unwrap_or_else(|p| p.into_inner()) = Some(authority);
    }
    pub fn set_deadline(&self, deadline: Instant) {
        *self
            .operation_deadline
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(deadline);
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
        let authority = self
            .authority
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if let Some(authority) = authority {
            if let Err(cause) = authority() {
                tracing::warn!(
                    method = %safe_identifier(method),
                    session = %safe_identifier(session.unwrap_or("browser")),
                    code = %safe_identifier(&cause.code),
                    phase = "authority",
                    "浏览器命令授权检查失败"
                );
                return Err(command_context(cause, method, session));
            }
        }
        let deadline = self
            .operation_deadline
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .map_or(deadline, |limit| deadline.min(limit));
        let outcome = self
            .process
            .cdp
            .call(session, method, params, deadline, cancel, effect);
        if let Err(cause) = &outcome {
            tracing::warn!(
                method = %safe_identifier(method),
                session = %safe_identifier(session.unwrap_or("browser")),
                code = %safe_identifier(&cause.code),
                phase = "call",
                "浏览器命令失败"
            );
        }
        #[cfg(test)]
        if method == "Target.createTarget" && outcome.is_ok() {
            if let Some(control) = self
                .process
                .control
                .as_ref()
                .filter(|control| control.fail_first_target.swap(false, Ordering::AcqRel))
            {
                if let Some(target) = outcome.as_ref().ok().and_then(|v| v["targetId"].as_str()) {
                    control
                        .created_targets
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .push(target.into());
                }
                return Err(error(
                    "browser_startup_test_target",
                    "测试注入首个target受理后失败",
                    true,
                ));
            }
        }
        outcome
    }
    pub fn stop(&self, graceful: bool) -> Result<(), BrowserError> {
        self.stop_until(graceful, Instant::now() + Duration::from_secs(3))
    }
    pub fn stop_until(&self, graceful: bool, deadline: Instant) -> Result<(), BrowserError> {
        self.stop.store(true, Ordering::Release);
        let result = self.process.stop_until(graceful, deadline);
        let mut holder = loop {
            match self.worker.try_lock() {
                Ok(holder) => break holder,
                Err(std::sync::TryLockError::Poisoned(p)) => break p.into_inner(),
                Err(std::sync::TryLockError::WouldBlock) => {
                    if Instant::now() >= deadline {
                        return Err(error(
                            "browser_cleanup_unknown",
                            "事件任务清理占用尚未释放",
                            true,
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        };
        while holder.as_ref().is_some_and(|worker| {
            worker.thread().id() != std::thread::current().id() && !worker.is_finished()
        }) {
            if Instant::now() >= deadline {
                return Err(error(
                    "browser_cleanup_unknown",
                    "浏览器事件任务退出尚未确认",
                    true,
                ));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        if let Some(worker) = holder.take() {
            if worker.thread().id() != std::thread::current().id() {
                let _ = worker.join();
            }
        }
        result
    }
    pub fn hidden_for_reaping(
        &self,
        headless: bool,
        deadline: Instant,
    ) -> Result<bool, BrowserError> {
        if headless {
            return Ok(!self.visible.load(Ordering::Acquire));
        }
        let tabs = self.tabs.lock().unwrap_or_else(|p| p.into_inner()).clone();
        for tab in tabs {
            let window = self.call(
                None,
                "Browser.getWindowForTarget",
                json!({"targetId":tab.tab_id}),
                deadline,
                &self.stop,
                false,
            )?;
            let id = window["windowId"].as_i64().ok_or_else(|| {
                error(
                    "browser_visibility_unknown",
                    "真实窗口身份未确认，保留工作区",
                    false,
                )
            })?;
            let bounds = self.call(
                None,
                "Browser.getWindowBounds",
                json!({"windowId":id}),
                deadline,
                &self.stop,
                false,
            )?;
            match bounds["bounds"]["windowState"].as_str() {
                Some("minimized") => {}
                Some("normal" | "maximized" | "fullscreen") => {
                    self.visible.store(true, Ordering::Release);
                    return Ok(false);
                }
                _ => {
                    return Err(error(
                        "browser_visibility_unknown",
                        "真实窗口状态未确认，保留工作区",
                        false,
                    ))
                }
            }
        }
        self.visible.store(false, Ordering::Release);
        Ok(true)
    }
    pub fn refresh(&self, deadline: Instant, cancel: &AtomicBool) -> Result<(), BrowserError> {
        self.refresh_inner(deadline, cancel, true)
    }
    fn refresh_inner(
        &self,
        deadline: Instant,
        cancel: &AtomicBool,
        foreground: bool,
    ) -> Result<(), BrowserError> {
        let _refresh = loop {
            match self.refreshing.try_lock() {
                Ok(guard) => break guard,
                Err(std::sync::TryLockError::Poisoned(p)) => break p.into_inner(),
                Err(std::sync::TryLockError::WouldBlock) if !foreground => return Ok(()),
                Err(std::sync::TryLockError::WouldBlock) => {}
            }
            if cancel.load(Ordering::Acquire) {
                return Err(error("cancelled", "刷新标签已取消", false));
            }
            if Instant::now() >= deadline {
                return Err(error("browser_timeout", "刷新标签超时", false));
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        let result = self.call(
            None,
            "Target.getTargets",
            json!({}),
            deadline,
            cancel,
            false,
        )?;
        let targets = result["targetInfos"]
            .as_array()
            .ok_or_else(|| error("browser_protocol", "浏览器标签列表无效", false))?;
        let previous = self.tabs.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let mut tabs = Vec::new();
        let mut popup_active = None;
        for t in targets.iter().filter(|t| t["type"] == "page") {
            let id = t["targetId"]
                .as_str()
                .ok_or_else(|| error("browser_protocol", "浏览器标签身份无效", false))?;
            // 新建且暂停在首段脚本前的page暂时尚无URL，仍是受控空白页。
            let url = t["url"]
                .as_str()
                .filter(|url| !url.is_empty())
                .unwrap_or("about:blank");
            if !allowed_url(url) || tabs.len() >= MAX_TABS {
                self.call(
                    None,
                    "Target.closeTarget",
                    json!({"targetId":id}),
                    deadline,
                    cancel,
                    true,
                )?;
                continue;
            }
            let prior = previous.iter().find(|p| p.tab_id == id);
            if prior.is_none()
                && t["openerId"]
                    .as_str()
                    .is_some_and(|opener| previous.iter().any(|p| p.tab_id == opener))
            {
                popup_active = Some(id.to_string());
            }
            let title = t["title"].as_str().unwrap_or("");
            if title.len() > 32768 || url.len() > 8192 {
                return Err(error("browser_budget", "浏览器标签元数据超出上限", false));
            }
            tabs.push(BrowserTab {
                tab_id: id.into(),
                title: title.into(),
                url: Some(url.into()),
                can_go_back: prior.is_some_and(|p| p.can_go_back),
                can_go_forward: prior.is_some_and(|p| p.can_go_forward),
                created_at: prior
                    .map(|p| p.created_at.clone())
                    .unwrap_or_else(timestamp),
                updated_at: if prior
                    .is_some_and(|p| p.title == title && p.url.as_deref() == Some(url))
                {
                    prior.unwrap().updated_at.clone()
                } else {
                    timestamp()
                },
            });
        }
        let mut active = self.active.lock().unwrap_or_else(|p| p.into_inner());
        if popup_active.is_some() {
            *active = popup_active;
        }
        if !active
            .as_ref()
            .is_some_and(|id| tabs.iter().any(|t| &t.tab_id == id))
        {
            *active = tabs.last().map(|t| t.tab_id.clone());
        }
        drop(active);
        self.attached
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .retain(|id, _| tabs.iter().any(|t| &t.tab_id == id));
        *self.tabs.lock().unwrap_or_else(|p| p.into_inner()) = tabs;
        let attached = self
            .attached
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        for (id, a) in attached {
            let history = match self.call(
                Some(&a.session),
                "Page.getNavigationHistory",
                json!({}),
                deadline,
                cancel,
                false,
            ) {
                Ok(history) => history,
                Err(cause) => {
                    // 关闭target和CDP session拆除并非同一步；仅以真实target不存在收口此竞态。
                    let actual = self.call(
                        None,
                        "Target.getTargets",
                        json!({}),
                        deadline,
                        cancel,
                        false,
                    )?;
                    let absent = actual["targetInfos"].as_array().is_some_and(|targets| {
                        !targets.iter().any(|target| target["targetId"] == id)
                    });
                    if !absent {
                        return Err(cause);
                    }
                    self.attached
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .remove(&id);
                    self.tabs
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .retain(|tab| tab.tab_id != id);
                    let mut active = self.active.lock().unwrap_or_else(|p| p.into_inner());
                    if active.as_ref() == Some(&id) {
                        *active = self
                            .tabs
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .last()
                            .map(|tab| tab.tab_id.clone());
                    }
                    continue;
                }
            };
            if let (Some(index), Some(entries)) = (
                history["currentIndex"].as_u64(),
                history["entries"].as_array(),
            ) {
                if let Some(tab) = self
                    .tabs
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .iter_mut()
                    .find(|t| t.tab_id == id)
                {
                    tab.can_go_back = index > 0;
                    tab.can_go_forward = (index as usize) + 1 < entries.len();
                }
            }
            // 事件线程不能等待页面JS，否则alert会堵住负责关闭该alert的同一线程。
            if foreground {
                let focused=self.call(Some(&a.session),"Runtime.evaluate",json!({"expression":"document.visibilityState==='visible'&&document.hasFocus()","returnByValue":true}),deadline,cancel,false)?;
                if focused["result"]["value"] == true {
                    *self.active.lock().unwrap_or_else(|p| p.into_inner()) = Some(id);
                }
            }
        }
        Ok(())
    }
    pub fn attach(
        &self,
        id: &str,
        deadline: Instant,
        cancel: &AtomicBool,
    ) -> Result<String, BrowserError> {
        if !self
            .tabs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .any(|t| t.tab_id == id)
        {
            return Err(error(
                "browser_stale_tab",
                "标签已关闭或不属于本会话",
                false,
            ));
        }
        // 所有page均由autoAttach暂停并统一配置；不得再次附着产生第二个Fetch拦截者。
        loop {
            if let Some(a) = self
                .attached
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .get(id)
            {
                return Ok(a.session.clone());
            }
            if let Some(cause) = self
                .failure
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone()
            {
                return Err(cause);
            }
            if cancel.load(Ordering::Acquire) || self.stop.load(Ordering::Acquire) {
                return Err(error("cancelled", "附着浏览器标签已取消", false));
            }
            if self.cdp().is_closed() {
                return Err(error("browser_closed", "附着浏览器标签前连接已关闭", false));
            }
            if Instant::now() >= deadline {
                return Err(error(
                    "browser_timeout",
                    "浏览器标签未在预算内完成附着",
                    false,
                ));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    pub fn cookie_policy(
        &self,
        sid: &str,
        deadline: Instant,
        cancel: &AtomicBool,
    ) -> Result<(), BrowserError> {
        let accept = self.accept_cookies.load(Ordering::Acquire);
        self.call(
            Some(sid),
            "Emulation.setDocumentCookieDisabled",
            json!({"disabled":!accept}),
            deadline,
            cancel,
            false,
        )?;
        if accept {
            self.call(
                Some(sid),
                "Fetch.disable",
                json!({}),
                deadline,
                cancel,
                false,
            )?;
        } else {
            self.call(Some(sid),"Fetch.enable",json!({"patterns":[{"urlPattern":"*","requestStage":"Request"},{"urlPattern":"*","requestStage":"Response"}]}),deadline,cancel,false)?;
        }
        Ok(())
    }
    fn handle_event(&self, event: &Value) -> Result<(), BrowserError> {
        if super::pdf::intercept_event(self, event)? {
            return Ok(());
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        let p = &event["params"];
        match event["method"].as_str().unwrap_or("") {
            "Target.attachedToTarget" if p["targetInfo"]["type"] == "page" => {
                let sid = p["sessionId"]
                    .as_str()
                    .ok_or_else(|| error("browser_protocol", "新页面附着身份无效", true))?;
                for method in ["Page.enable", "Runtime.enable", "DOM.enable"] {
                    self.call(Some(sid), method, json!({}), deadline, &self.stop, false)?;
                }
                self.call(
                    Some(sid),
                    "Page.setLifecycleEventsEnabled",
                    json!({"enabled":true}),
                    deadline,
                    &self.stop,
                    false,
                )?;
                self.cookie_policy(sid, deadline, &self.stop)?;
                self.call(
                    Some(sid),
                    "Runtime.runIfWaitingForDebugger",
                    json!({}),
                    deadline,
                    &self.stop,
                    false,
                )?;
                if let Some(id) = p["targetInfo"]["targetId"].as_str() {
                    self.attached
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .insert(
                            id.to_string(),
                            Attached {
                                session: sid.into(),
                                snapshot_loader: None,
                                reference_range: None,
                                reference_context: None,
                            },
                        );
                }
            }

            "Page.javascriptDialogOpening" if !self.takeover.load(Ordering::Acquire) => {
                self.call(
                    event["sessionId"].as_str(),
                    "Page.handleJavaScriptDialog",
                    json!({"accept":p["type"]=="alert"}),
                    deadline,
                    &self.stop,
                    true,
                )?;
            }
            "Fetch.requestPaused" => {
                let sid = event["sessionId"].as_str();
                let mut headers = Vec::new();
                if let Some(status) = p["responseStatusCode"].as_u64() {
                    if let Some(values) = p["responseHeaders"].as_array() {
                        for h in values {
                            if !h["name"]
                                .as_str()
                                .is_some_and(|n| n.eq_ignore_ascii_case("set-cookie"))
                            {
                                headers.push(h.clone());
                            }
                        }
                    }
                    self.call(sid,"Fetch.continueResponse",json!({"requestId":p["requestId"],"responseCode":status,"responseHeaders":headers}),deadline,&self.stop,false)?;
                } else {
                    if let Some(values) = p["request"]["headers"].as_object() {
                        for (k, v) in values {
                            if !k.eq_ignore_ascii_case("cookie") {
                                headers.push(json!({"name":k,"value":v}));
                            }
                        }
                    }
                    // 省略Cookie会允许Chromium从CookieStore重新合并，空覆盖才禁止实际凭据发送。
                    headers.push(json!({"name":"Cookie","value":""}));
                    self.call(
                        sid,
                        "Fetch.continueRequest",
                        json!({"requestId":p["requestId"],"headers":headers}),
                        deadline,
                        &self.stop,
                        false,
                    )?;
                }
            }
            "Browser.downloadWillBegin" => {
                let guid = p["guid"]
                    .as_str()
                    .ok_or_else(|| error("browser_protocol", "下载编号无效", true))?;
                if guid.is_empty()
                    || guid.len() > 128
                    || !guid.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                {
                    return Err(error("browser_protocol", "下载编号无效", true));
                }
                let filename = p["suggestedFilename"].as_str().unwrap_or("download");
                let safe = Path::new(filename)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .filter(|n| {
                        !n.is_empty() && *n != "." && *n != ".." && !n.chars().any(char::is_control)
                    })
                    .unwrap_or("download");
                let mut downloads = self.downloads.lock().unwrap_or_else(|q| q.into_inner());
                if downloads.len() >= 64 {
                    drop(downloads);
                    self.call(
                        None,
                        "Browser.cancelDownload",
                        json!({"guid":guid}),
                        deadline,
                        &self.stop,
                        false,
                    )?;
                } else {
                    downloads.push(BrowserDownload {
                        id: guid.into(),
                        suggested_filename: safe.chars().take(255).collect(),
                        bytes: None,
                        completed: false,
                    });
                }
            }
            "Browser.downloadProgress" => {
                if p["receivedBytes"].as_u64().unwrap_or(0) > 50 * 1024 * 1024 {
                    self.call(
                        None,
                        "Browser.cancelDownload",
                        json!({"guid":p["guid"]}),
                        deadline,
                        &self.stop,
                        false,
                    )?;
                }
                if p["receivedBytes"].as_u64().unwrap_or(0) > 50 * 1024 * 1024 {
                    return Err(error(
                        "browser_download_budget",
                        "下载超过50MiB，已取消",
                        true,
                    ));
                }
                if p["state"] == "completed" {
                    let mut downloads = self.downloads.lock().unwrap_or_else(|q| q.into_inner());
                    let used = downloads
                        .iter()
                        .filter_map(|d| d.bytes.as_ref())
                        .map(Vec::len)
                        .sum::<usize>();
                    if let Some(d) = downloads
                        .iter_mut()
                        .find(|d| Some(d.id.as_str()) == p["guid"].as_str())
                    {
                        if d.completed && d.bytes.is_some() {
                            return Ok(());
                        }
                        let bytes = read_download(
                            &self.download_root,
                            &d.id,
                            (64 * 1024 * 1024usize)
                                .saturating_sub(used)
                                .min(50 * 1024 * 1024),
                        )?;
                        d.bytes = Some(bytes);
                        d.completed = true;
                    }
                    if downloads
                        .iter()
                        .filter_map(|d| d.bytes.as_ref())
                        .map(Vec::len)
                        .sum::<usize>()
                        > 64 * 1024 * 1024
                    {
                        return Err(error(
                            "browser_download_budget",
                            "待确认下载总量超过64MiB",
                            true,
                        ));
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}
impl Drop for Live {
    fn drop(&mut self) {
        let _ = self.stop(false);
    }
}

fn read_download(dir: &std::fs::File, id: &str, limit: usize) -> Result<Vec<u8>, BrowserError> {
    use std::{
        io::Read,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::fs::MetadataExt,
        },
    };
    let name = std::ffi::CString::new(id)
        .map_err(|_| error("browser_download_unknown", "下载编号无效", true))?;
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(error(
            "browser_download_unknown",
            "下载完成文件尚未确认",
            true,
        ));
    }
    let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
    let before = file
        .metadata()
        .map_err(|_| error("browser_download_unknown", "无法确认下载文件", true))?;
    if !before.is_file() || before.len() > limit as u64 {
        return Err(error(
            "browser_download_budget",
            "下载文件超过50MiB或类型无效",
            true,
        ));
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| error("browser_download_unknown", "下载文件读取未完成", true))?;
    let after = file
        .metadata()
        .map_err(|_| error("browser_download_unknown", "下载文件状态未确认", true))?;
    if bytes.len() > limit
        || before.ino() != after.ino()
        || before.len() != after.len()
        || after.len() != bytes.len() as u64
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
    {
        return Err(error(
            "browser_download_unknown",
            "下载文件读取期间发生变化",
            true,
        ));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_download_parent_rejects_symlinks_and_enforces_both_file_and_cache_limits() {
        use std::os::unix::fs::{symlink, OpenOptionsExt};
        let mut random = [0u8; 12];
        getrandom::getrandom(&mut random).unwrap();
        let suffix: String = random.iter().map(|b| format!("{b:02x}")).collect();
        let root = std::env::temp_dir().join(format!("browser-fixed-download-{suffix}"));
        std::fs::create_dir(&root).unwrap();
        let directory = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(&root)
            .unwrap();
        std::fs::write(root.join("proof"), b"real bytes").unwrap();
        symlink(root.join("proof"), root.join("link")).unwrap();
        assert_eq!(
            read_download(&directory, "proof", 100).unwrap(),
            b"real bytes"
        );
        assert!(read_download(&directory, "proof", 5).is_err());
        assert!(read_download(&directory, "link", 100).is_err());
        let moved = root.with_extension("moved");
        std::fs::rename(&root, &moved).unwrap();
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("proof"), b"replacement must not be read").unwrap();
        assert_eq!(
            read_download(&directory, "proof", 100).unwrap(),
            b"real bytes"
        );
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(moved).unwrap();
    }
}
