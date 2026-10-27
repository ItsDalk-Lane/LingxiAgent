//! BrowserPort生产实现；profile原生数据以外的权威事实仍归服务数据库。
#[cfg(test)]
#[path = "manual_capture.rs"]
mod manual_capture;
#[cfg(test)]
#[path = "pdf_lifecycle_tests.rs"]
mod pdf_lifecycle_tests;
#[cfg(test)]
#[path = "startup_tests.rs"]
mod startup_tests;
use super::{
    error,
    launcher::{LaunchPolicy, Process},
    managed_files, ops, pdf,
    runtime::{allowed_url, now_ms, Live},
    BrowserError,
};
use crate::models::network::{NetworkPlane, ProxyPolicy};
use lingxi_kernel::browser::{
    BrowserFuture, BrowserOutput, BrowserPort, BrowserScope, PdfRenderJob, PdfRendered, PdfScope,
};
use lingxi_protocol::{
    BrowserAction, BrowserCommand, BrowserReply, BrowserSettings, BrowserWorkspace,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

/// 只由组合根提供；HTTP、模型和页面均不能传入可执行文件或启动参数。
pub struct ChromiumConfig {
    pub executable: PathBuf,
    pub root: PathBuf,
    pub execution_node: String,
    pub headless: bool,
    pub network: Arc<NetworkPlane>,
}
struct Entry {
    owner: String,
    epoch: u64,
    state: BrowserWorkspace,
    live: Option<Arc<Live>>,
    busy: Arc<AtomicBool>,
    last_agent: u64,
    last_human: u64,
}
struct Pool {
    entries: HashMap<String, Entry>,
    settings: BrowserSettings,
    launching: usize,
    aux: usize,
    auxiliary: HashMap<String, Arc<Live>>,
    clearing: bool,
    configuring: bool,
    stopping: bool,
    retired: HashSet<String>,
    pending_cleanup: Vec<Arc<Live>>,
    pdf: HashMap<String, PdfLease>,
}
struct PdfLease {
    scope: PdfScope,
    cancel: Arc<AtomicBool>,
    live: Option<Arc<Live>>,
    proxy: Option<Arc<pdf::DenyProxy>>,
    deadline: Instant,
    network_generation: u64,
    busy: bool,
    cleaned: bool,
}
pub(super) struct Core {
    config: ChromiumConfig,
    pool: Mutex<Pool>,
    startup_cancel: AtomicBool,
    profiles_root: File,
    downloads_root: File,
    auxiliary_root: File,
    #[cfg(test)]
    startup_control: Mutex<Option<Arc<super::launcher::StartupControl>>>,
}
pub struct ChromiumHost {
    core: Arc<Core>,
}
struct Busy(Arc<AtomicBool>);
impl Drop for Busy {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
struct CancelOnDrop {
    cancel: Arc<AtomicBool>,
    armed: bool,
}
struct PdfGuard {
    core: Arc<Core>,
    identity: String,
    attempted_cleanup: bool,
}
impl Drop for PdfGuard {
    fn drop(&mut self) {
        if !self.attempted_cleanup
            && self
                .core
                .finish_pdf(&self.identity, Instant::now() + Duration::from_secs(3))
                .is_err()
        {
            self.core
                .pool
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .stopping = true;
        }
        let mut pool = self.core.pool.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(lease) = pool.pdf.get_mut(&self.identity) {
            lease.busy = false;
        }
        if pool
            .pdf
            .get(&self.identity)
            .is_some_and(|lease| lease.cleaned)
        {
            pool.pdf.remove(&self.identity);
        }
        pool.aux -= 1;
    }
}
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if self.armed {
            self.cancel.store(true, Ordering::Release);
        }
    }
}

fn private_dir(path: &Path) -> Result<(), BrowserError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(path)
        .map_err(|_| error("browser_profile", "无法建立浏览器私有目录", false))?;
    let m = std::fs::symlink_metadata(path)
        .map_err(|_| error("browser_profile", "浏览器私有目录不可读", false))?;
    if !m.is_dir() || m.file_type().is_symlink() {
        return Err(error(
            "browser_profile",
            "浏览器私有目录不是安全目录",
            false,
        ));
    }
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .map_err(|_| error("browser_profile", "无法保护浏览器私有目录", false))
}
fn key(scope: &BrowserScope) -> String {
    identity_key(
        &scope.execution_node,
        &scope.owner_subject,
        &scope.session_id,
    )
}
fn identity_key(node: &str, owner: &str, session: &str) -> String {
    Sha256::digest(serde_json::to_vec(&json!([node, owner, session])).expect("固定身份数组"))
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn empty(scope: &BrowserScope, headless: bool) -> BrowserWorkspace {
    BrowserWorkspace {
        session_id: scope.session_id.clone(),
        session_path: format!("native:{}", scope.session_id),
        execution_node: scope.execution_node.clone(),
        running: false,
        resumable: false,
        unavailable_reason: None,
        url: None,
        title: None,
        active_tab_id: None,
        tabs: Vec::new(),
        suspended: false,
        user_takeover: false,
        revoked: false,
        visible_capable: !headless,
    }
}
fn sync(entry: &mut Entry) {
    if entry.state.unavailable_reason.as_deref() == Some("browser_cleanup_unknown") {
        return;
    }
    if let Some(live) = &entry.live {
        entry.state.running = !live.cdp().is_closed();
        if !entry.state.running {
            entry.state.unavailable_reason = Some(
                live.failure
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .as_ref()
                    .map(|e| e.code.clone())
                    .unwrap_or_else(|| "browser_host_exited".into()),
            );
        }
        entry.state.tabs = live.tabs.lock().unwrap_or_else(|p| p.into_inner()).clone();
        entry.state.active_tab_id = live
            .active
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        entry.state.user_takeover = live.takeover.load(Ordering::Acquire);
        let active = entry
            .state
            .tabs
            .iter()
            .find(|t| Some(&t.tab_id) == entry.state.active_tab_id.as_ref());
        entry.state.url = active.and_then(|t| t.url.clone());
        entry.state.title = active.map(|t| t.title.clone());
        entry.state.resumable = !entry.state.tabs.is_empty();
    }
}
impl ChromiumHost {
    pub fn new(mut config: ChromiumConfig) -> Result<Arc<Self>, BrowserError> {
        if !config.executable.is_absolute()
            || !config.executable.is_file()
            || !config.root.is_absolute()
            || config.execution_node.is_empty()
            || config.execution_node.len() > 1024
        {
            return Err(error(
                "browser_configuration",
                "浏览器需要可信绝对程序/目录及执行节点",
                false,
            ));
        }
        private_dir(&config.root)?;
        config.root = std::fs::canonicalize(&config.root)
            .map_err(|_| error("browser_profile", "无法固定浏览器根目录", false))?;
        private_dir(&config.root.join("profiles"))?;
        private_dir(&config.root.join("downloads"))?;
        private_dir(&config.root.join("aux"))?;
        // 启动前验证代理，配置错误不得落回直连。
        proxy_arguments(&config.network)?;
        let profiles_root = managed_files::directory(&config.root.join("profiles"))?;
        let downloads_root = managed_files::directory(&config.root.join("downloads"))?;
        let auxiliary_root = managed_files::directory(&config.root.join("aux"))?;
        Ok(Arc::new(Self {
            core: Arc::new(Core {
                config,
                pool: Mutex::new(Pool {
                    entries: HashMap::new(),
                    settings: BrowserSettings::default(),
                    launching: 0,
                    aux: 0,
                    auxiliary: HashMap::new(),
                    clearing: false,
                    configuring: false,
                    stopping: false,
                    retired: HashSet::new(),
                    pending_cleanup: Vec::new(),
                    pdf: HashMap::new(),
                }),
                startup_cancel: AtomicBool::new(false),
                profiles_root,
                downloads_root,
                auxiliary_root,
                #[cfg(test)]
                startup_control: Mutex::new(None),
            }),
        }))
    }
    fn run<T: Send + 'static>(
        &self,
        f: impl FnOnce(Arc<Core>) -> Result<T, BrowserError> + Send + 'static,
    ) -> BrowserFuture<'_, T> {
        let core = self.core.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || f(core))
                .await
                .map_err(|_| error("browser_host_failed", "浏览器宿主任务未完成", true))?
        })
    }
}
impl Core {
    fn launch_policy(&self) -> LaunchPolicy {
        LaunchPolicy {
            #[cfg(test)]
            control: self
                .startup_control
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone(),
        }
    }
    fn check_pdf_scope(&self, scope: &PdfScope) -> Result<(), BrowserError> {
        if scope.execution_node != self.config.execution_node {
            return Err(error(
                "browser_wrong_node",
                "打印只能在指定服务节点执行",
                false,
            ));
        }
        if scope.session_id.is_empty()
            || scope.session_id.len() > 1024
            || scope.owner_subject.is_empty()
            || scope.owner_subject.len() > 1024
        {
            return Err(error("PDF_SCOPE", "打印会话归属无效", false));
        }
        Ok(())
    }
    fn pdf_current(&self, identity: &str) -> Result<(), BrowserError> {
        let pool = self.pool.lock().unwrap_or_else(|p| p.into_inner());
        let lease = pool
            .pdf
            .get(identity)
            .ok_or_else(|| error("PDF_REVOKED", "打印租约已结束", false))?;
        if pool.stopping
            || pool.clearing
            || pool.configuring
            || lease.cancel.load(Ordering::Acquire)
        {
            return Err(error("cancelled", "打印已取消或宿主状态已改变", false));
        }
        if Instant::now() >= lease.deadline {
            return Err(error("PDF_TIMEOUT", "打印已超出总等待时间", false));
        }
        if lease.network_generation != self.config.network.generation() {
            return Err(error(
                "browser_network_changed",
                "网络设置改变，打印已停止",
                false,
            ));
        }
        Ok(())
    }
    fn finish_pdf(&self, identity: &str, deadline: Instant) -> Result<(), BrowserError> {
        let live = self
            .pool
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .pdf
            .get(identity)
            .and_then(|lease| lease.live.clone());
        if let Some(live) = live {
            self.stop_owned_until(&live, false, deadline)?;
        }
        // 无进程的准备失败与已经确认退出的路径都使用同一固定目录句柄清理。
        if let Err(cause) = managed_files::purge(&self.auxiliary_root, &[identity]) {
            self.pool.lock().unwrap_or_else(|p| p.into_inner()).stopping = true;
            return Err(cause);
        }
        let mut pool = self.pool.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(lease) = pool.pdf.get_mut(identity) {
            if let Some(proxy) = lease.proxy.take() {
                proxy.close();
            }
            lease.cleaned = true;
        }
        Ok(())
    }
    fn cancel_pdf(&self, scope: &PdfScope, deadline: Instant) -> Result<(), BrowserError> {
        self.check_pdf_scope(scope)?;
        {
            let pool = self.pool.lock().unwrap_or_else(|p| p.into_inner());
            for lease in pool.pdf.values().filter(|lease| &lease.scope == scope) {
                lease.cancel.store(true, Ordering::Release);
            }
        }
        loop {
            let pending = {
                let mut pool = self.pool.lock().unwrap_or_else(|p| p.into_inner());
                pool.pdf.retain(|_, lease| lease.busy || !lease.cleaned);
                pool.pdf
                    .iter()
                    .filter(|(_, lease)| &lease.scope == scope)
                    .map(|(id, lease)| (id.clone(), lease.busy, lease.live.clone()))
                    .collect::<Vec<_>>()
            };
            if pending.is_empty() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                self.pool.lock().unwrap_or_else(|p| p.into_inner()).stopping = true;
                return Err(error(
                    "browser_cleanup_unknown",
                    "本会话的打印退出尚未确认",
                    true,
                ));
            }
            for (id, busy, live) in pending {
                let result = if let Some(live) = live {
                    self.stop_owned_until(&live, false, deadline)
                } else if !busy {
                    self.finish_pdf(&id, deadline)
                } else {
                    continue;
                };
                if let Err(cause) = result {
                    self.pool.lock().unwrap_or_else(|p| p.into_inner()).stopping = true;
                    return Err(cause);
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn render_pdf(
        self: &Arc<Self>,
        scope: PdfScope,
        job: PdfRenderJob,
        cancel: Arc<AtomicBool>,
    ) -> Result<PdfRendered, BrowserError> {
        self.check_pdf_scope(&scope)?;
        pdf::validate(&job)?;
        let deadline = Instant::now()
            .checked_add(Duration::from_millis(job.timeout_ms))
            .ok_or_else(|| error("PDF_INVALID_JOB", "打印等待时间溢出", false))?;
        if cancel.load(Ordering::Acquire) {
            return Err(error("cancelled", "打印在启动前已取消", false));
        }
        let mut random = [0u8; 16];
        getrandom::getrandom(&mut random)
            .map_err(|_| error("browser_random", "无法分配打印租约", false))?;
        let identity: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
        {
            let mut pool = self.pool.lock().unwrap_or_else(|p| p.into_inner());
            if pool.stopping || pool.clearing || pool.configuring || pool.aux >= 2 {
                return Err(error(
                    "browser_capacity",
                    "辅助浏览器已达上限或当前不可启动",
                    false,
                ));
            }
            if pool.retired.contains(&identity_key(
                &scope.execution_node,
                &scope.owner_subject,
                &scope.session_id,
            )) {
                return Err(error("PDF_REVOKED", "该会话已永久删除", false));
            }
            pool.aux += 1;
            pool.pdf.insert(
                identity.clone(),
                PdfLease {
                    scope,
                    cancel: cancel.clone(),
                    live: None,
                    proxy: None,
                    deadline,
                    network_generation: self.config.network.generation(),
                    busy: true,
                    cleaned: false,
                },
            );
        }
        let mut guard = PdfGuard {
            core: self.clone(),
            identity: identity.clone(),
            attempted_cleanup: false,
        };
        let operation = (|| {
            self.pdf_current(&identity)?;
            let proxy = pdf::DenyProxy::new()?;
            self.pool
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .pdf
                .get_mut(&identity)
                .expect("打印租约保持到本操作退出")
                .proxy = Some(proxy.clone());
            let path = self.config.root.join("aux").join(&identity);
            private_dir(&path)?;
            let downloads = path.join("downloads");
            private_dir(&downloads)?;
            let prepared = Live::prepare(downloads)?;
            let process = Process::launch(
                &self.config.executable,
                &path,
                true,
                &proxy.arguments(),
                self.launch_policy(),
            )?;
            let live = Live::allocate(process, prepared, self.config.network.generation(), true);
            live.set_deadline(deadline);
            {
                let mut pool = self.pool.lock().unwrap_or_else(|p| p.into_inner());
                pool.pending_cleanup.push(live.clone());
                pool.auxiliary
                    .insert(path.to_string_lossy().to_string(), live.clone());
                pool.pdf
                    .get_mut(&identity)
                    .expect("打印租约保持到本操作退出")
                    .live = Some(live.clone());
            }
            let weak = Arc::downgrade(self);
            let id = identity.clone();
            live.set_authority(Arc::new(move || {
                weak.upgrade()
                    .ok_or_else(|| error("browser_closed", "打印宿主已停止", false))?
                    .pdf_current(&id)
            }));
            self.pdf_current(&identity)?;
            live.initialize_until(&cancel, deadline)?;
            pdf::render(&live, job, cancel, deadline, proxy)
        })();
        guard.attempted_cleanup = true;
        self.finish_pdf(&identity, Instant::now() + Duration::from_secs(3))?;
        // 页面只在无外发的临时进程中改变；已确认退出且未交付任何PDF时是明确失败。
        // 真实清理Unknown在上面直接返回，不能被此处改写成已确认失败。
        operation.map_err(|mut cause| {
            cause.effect_unknown = false;
            cause
        })
    }
    fn stop_owned(&self, live: &Arc<Live>, graceful: bool) -> Result<(), BrowserError> {
        self.stop_owned_until(live, graceful, Instant::now() + Duration::from_secs(3))
    }
    fn stop_owned_until(
        &self,
        live: &Arc<Live>,
        graceful: bool,
        deadline: Instant,
    ) -> Result<(), BrowserError> {
        {
            let mut p = self.pool.lock().unwrap_or_else(|p| p.into_inner());
            if !live.cdp().is_closed() {
                for entry in p.entries.values_mut().filter(|entry| {
                    entry
                        .live
                        .as_ref()
                        .is_some_and(|current| Arc::ptr_eq(current, live))
                }) {
                    sync(entry);
                }
            }
        }
        let result = live.stop_until(graceful, deadline).and_then(|()| {
            let paths: Vec<String> = self
                .pool
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .auxiliary
                .iter()
                .filter(|(_, current)| Arc::ptr_eq(current, live))
                .map(|(path, _)| path.clone())
                .collect();
            for path in paths {
                let path = Path::new(&path);
                if path.parent() != Some(self.config.root.join("aux").as_path()) {
                    return Err(error(
                        "browser_cleanup_unknown",
                        "辅助资料归属无法确认",
                        true,
                    ));
                }
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or_else(|| error("browser_cleanup_unknown", "辅助资料身份无效", true))?;
                managed_files::purge(&self.auxiliary_root, &[name])?;
            }
            Ok(())
        });
        let mut p = self.pool.lock().unwrap_or_else(|p| p.into_inner());
        for entry in p.entries.values_mut().filter(|entry| {
            entry
                .live
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, live))
        }) {
            if result.is_ok() {
                entry.live = None;
                entry.state.running = false;
                entry.state.suspended = true;
                entry.state.unavailable_reason = None;
            } else {
                entry.state.unavailable_reason = Some("browser_cleanup_unknown".into());
            }
        }
        if result.is_ok() {
            for lease in p.pdf.values_mut().filter(|lease| {
                lease
                    .live
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(current, live))
            }) {
                lease.live = None;
                if let Some(proxy) = lease.proxy.take() {
                    proxy.close();
                }
                lease.cleaned = true;
            }
            p.pdf.retain(|_, lease| lease.busy || !lease.cleaned);
            p.pending_cleanup
                .retain(|current| !Arc::ptr_eq(current, live));
            p.auxiliary.retain(|_, current| !Arc::ptr_eq(current, live));
        } else {
            p.stopping = true;
            if !p
                .pending_cleanup
                .iter()
                .any(|current| Arc::ptr_eq(current, live))
            {
                p.pending_cleanup.push(live.clone());
            }
        }
        result
    }
    fn check_scope(&self, scope: &BrowserScope) -> Result<(), BrowserError> {
        if scope.execution_node != self.config.execution_node {
            return Err(error(
                "browser_wrong_node",
                "浏览器只能在指定服务节点执行",
                false,
            ));
        }
        if scope.session_id.is_empty()
            || scope.session_id.len() > 1024
            || scope.owner_subject.is_empty()
            || scope.owner_subject.len() > 1024
            || scope.epoch == 0
        {
            return Err(error("browser_scope", "浏览器会话授权无效", false));
        }
        Ok(())
    }
    pub(super) fn current(
        &self,
        scope: &BrowserScope,
        cancel: &AtomicBool,
    ) -> Result<(), BrowserError> {
        if cancel.load(Ordering::Acquire) {
            return Err(error("cancelled", "浏览器操作已取消", false));
        }
        let pool = self.pool.lock().unwrap_or_else(|p| p.into_inner());
        let e = pool
            .entries
            .get(&scope.session_id)
            .ok_or_else(|| error("browser_revoked", "浏览器会话授权不存在", false))?;
        if pool.stopping
            || pool.clearing
            || e.owner != scope.owner_subject
            || e.epoch != scope.epoch
            || e.state.revoked
        {
            return Err(error("browser_revoked", "浏览器授权已撤销或更新", false));
        }
        Ok(())
    }
    fn reserve(&self, scope: &BrowserScope) -> Result<Busy, BrowserError> {
        self.check_scope(scope)?;
        let mut pool = self.pool.lock().unwrap_or_else(|p| p.into_inner());
        if pool.retired.contains(&key(scope)) {
            return Err(error("browser_revoked", "浏览器会话已永久删除", false));
        }
        if pool.retired.len() >= 16_384 {
            return Err(error(
                "browser_capacity",
                "退休会话已达宿主安全上限，需要重新启动",
                false,
            ));
        }
        if pool.stopping || pool.clearing || pool.configuring {
            return Err(error("browser_closed", "浏览器宿主已停止", false));
        }
        let e = pool
            .entries
            .entry(scope.session_id.clone())
            .or_insert_with(|| Entry {
                owner: scope.owner_subject.clone(),
                epoch: scope.epoch,
                state: empty(scope, self.config.headless),
                live: None,
                busy: Arc::new(AtomicBool::new(false)),
                last_agent: 0,
                last_human: 0,
            });
        if e.owner != scope.owner_subject || scope.epoch < e.epoch {
            return Err(error("browser_revoked", "浏览器授权已过期", false));
        }
        if e.busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(error("browser_busy", "本会话已有浏览器操作", false));
        }
        let busy = Busy(e.busy.clone());
        if scope.epoch > e.epoch {
            e.epoch = scope.epoch;
            e.state.revoked = false;
            e.state.user_takeover = false;
            if let Some(live) = e.live.clone() {
                drop(pool);
                self.stop_owned(&live, false)?;
                return Ok(busy);
            }
        }
        if e.state.revoked {
            return Err(error(
                "browser_revoked",
                "用户已停止本会话的浏览器授权",
                false,
            ));
        }
        Ok(busy)
    }
    fn get_live(
        self: &Arc<Self>,
        scope: &BrowserScope,
        launch: bool,
        restore: bool,
        cancel: &AtomicBool,
    ) -> Result<Arc<Live>, BrowserError> {
        self.current(scope, cancel)?;
        let mut pool = self.pool.lock().unwrap_or_else(|p| p.into_inner());
        let existing = pool
            .entries
            .get(&scope.session_id)
            .and_then(|e| e.live.clone());
        if let Some(live) = existing {
            if live.network_generation != self.config.network.generation() {
                let e = pool.entries.get_mut(&scope.session_id).expect("授权已核实");
                sync(e);
                drop(pool);
                self.stop_owned(&live, true)?;
                if let Some(entry) = self
                    .pool
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .entries
                    .get_mut(&scope.session_id)
                {
                    entry.state.unavailable_reason = Some("network_configuration_changed".into());
                }
                return Err(error(
                    "browser_network_changed",
                    "网络设置已改变，请明确恢复浏览器",
                    false,
                ));
            }
            if !live.cdp().is_closed() {
                return Ok(live);
            }
            drop(pool);
            self.stop_owned(&live, false)?;
            return Err(error("browser_closed", "浏览器已退出，请明确恢复", false));
        }
        if pool.configuring {
            return Err(error("browser_busy", "浏览器设置正在更新", false));
        }
        if !launch {
            return Err(error("browser_not_running", "此会话浏览器未运行", false));
        }
        let mut evicted = None;
        while pool.entries.values().filter(|e| e.live.is_some()).count() + pool.launching >= 5 {
            let candidate = pool
                .entries
                .iter()
                .filter(|(id, entry)| {
                    id.as_str() != scope.session_id
                        && !entry.busy.load(Ordering::Acquire)
                        && entry
                            .live
                            .as_ref()
                            .is_some_and(|live| !live.visible.load(Ordering::Acquire))
                })
                .min_by_key(|(_, entry)| entry.last_agent.max(entry.last_human))
                .map(|(id, _)| id.clone());
            let Some(id) = candidate else {
                return Err(error(
                    "browser_capacity",
                    "浏览器已达5个工作区，当前窗口不可回收",
                    false,
                ));
            };
            let entry = pool.entries.get_mut(&id).expect("候选存在");
            entry.busy.store(true, Ordering::Release);
            let held = Busy(entry.busy.clone());
            let live = entry.live.as_ref().expect("候选有进程").clone();
            drop(pool);
            // Native最小化/还原不经过本API；真正回收前必须重新询问窗口管理器。
            let hidden = live.hidden_for_reaping(
                self.config.headless,
                Instant::now() + Duration::from_secs(2),
            );
            pool = self.pool.lock().unwrap_or_else(|p| p.into_inner());
            let hidden = hidden.map_err(|_| {
                error(
                    "browser_visibility_unknown",
                    "候选窗口状态未确认，不能回收",
                    false,
                )
            })?;
            if pool.stopping || pool.clearing || pool.configuring {
                return Err(error("browser_busy", "回收期间宿主状态改变", false));
            }
            if cancel.load(Ordering::Acquire) {
                return Err(error("cancelled", "浏览器启动已取消", false));
            }
            if pool.entries.get(&scope.session_id).is_none_or(|entry| {
                entry.owner != scope.owner_subject
                    || entry.epoch != scope.epoch
                    || entry.state.revoked
            }) {
                return Err(error("browser_revoked", "回收前浏览器授权已改变", false));
            }
            if pool
                .entries
                .values()
                .filter(|entry| entry.live.is_some())
                .count()
                + pool.launching
                < 5
            {
                drop(held);
                break;
            }
            if !hidden {
                drop(held);
                continue;
            }
            let Some(entry) = pool.entries.get_mut(&id) else {
                drop(held);
                continue;
            };
            if !entry
                .live
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &live))
            {
                drop(held);
                continue;
            }
            sync(entry);
            evicted = Some((live, held));
            break;
        }
        pool.launching += 1;
        let settings = pool.settings.clone();
        let saved = pool.entries[&scope.session_id].state.clone();
        drop(pool);
        let result: Result<Arc<Live>, BrowserError> = (|| {
            if let Some((live, _held)) = evicted {
                self.stop_owned(&live, true)?;
            }
            self.current(scope, cancel)?;
            let profile = self.config.root.join("profiles").join(key(scope));
            private_dir(&profile)?;
            managed_files::configure_cookies(
                &self.profiles_root,
                &key(scope),
                settings.accept_cookies,
            )?;
            let download = self.config.root.join("downloads").join(key(scope));
            private_dir(&download)?;
            let prepared = Live::prepare(download)?;
            let process = Process::launch(
                &self.config.executable,
                &profile,
                self.config.headless,
                &proxy_arguments(&self.config.network)?,
                self.launch_policy(),
            )?;
            let live = Live::allocate(
                process,
                prepared,
                self.config.network.generation(),
                settings.accept_cookies,
            );
            {
                // 从此任何可能失败的CDP/页面初始化之前，真实进程均有正式owner。
                let mut pool = self.pool.lock().unwrap_or_else(|p| p.into_inner());
                pool.pending_cleanup.push(live.clone());
                if let Some(entry) = pool.entries.get_mut(&scope.session_id) {
                    entry.live = Some(live.clone());
                    entry.state.running = true;
                    entry.state.unavailable_reason = Some("browser_starting".into());
                }
            }
            let weak = Arc::downgrade(self);
            let grant = scope.clone();
            live.set_authority(Arc::new(move || {
                weak.upgrade()
                    .ok_or_else(|| error("browser_closed", "浏览器宿主已停止", false))?
                    .current(&grant, &AtomicBool::new(false))
            }));
            let initialized: Result<(), BrowserError> = (|| {
                live.initialize(&self.startup_cancel)?;
                let deadline = Instant::now() + Duration::from_secs(20);
                if restore && !saved.tabs.is_empty() {
                    let mut selected = None;
                    for tab in saved.tabs.iter().take(32) {
                        let target = ops::new_tab(
                            &live,
                            tab.url.as_deref().unwrap_or("about:blank"),
                            !self.config.headless,
                            deadline,
                            cancel,
                        )?;
                        if saved.active_tab_id.as_ref() == Some(&tab.tab_id) {
                            selected = Some(target);
                        }
                    }
                    if let Some(selected) = selected {
                        live.call(
                            None,
                            "Target.activateTarget",
                            json!({"targetId":selected}),
                            deadline,
                            cancel,
                            false,
                        )?;
                        *live.active.lock().unwrap_or_else(|p| p.into_inner()) = Some(selected);
                    }
                } else {
                    ops::new_tab(
                        &live,
                        "about:blank",
                        !self.config.headless,
                        deadline,
                        cancel,
                    )?;
                }
                self.current(scope, cancel)?;
                Ok(())
            })();
            if let Err(cause) = initialized {
                *live.failure.lock().unwrap_or_else(|p| p.into_inner()) = Some(cause.clone());
                self.stop_owned(&live, false)?;
                return Err(cause);
            }
            Ok(live)
        })();
        let mut pool = self.pool.lock().unwrap_or_else(|p| p.into_inner());
        pool.launching -= 1;
        let live = result?;
        if pool.stopping
            || pool.clearing
            || pool
                .entries
                .get(&scope.session_id)
                .is_none_or(|e| e.epoch != scope.epoch || e.state.revoked)
        {
            drop(pool);
            self.stop_owned(&live, false)?;
            return Err(error("browser_revoked", "浏览器启动期间授权已撤销", false));
        }
        let e = pool.entries.get_mut(&scope.session_id).expect("授权已核实");
        e.live = Some(live.clone());
        e.state.suspended = false;
        e.state.unavailable_reason = None;
        sync(e);
        pool.pending_cleanup
            .retain(|current| !Arc::ptr_eq(current, &live));
        Ok(live)
    }
    fn execute(
        self: Arc<Self>,
        scope: BrowserScope,
        command: BrowserCommand,
        cancel: Arc<AtomicBool>,
    ) -> Result<BrowserOutput, BrowserError> {
        command
            .validate()
            .map_err(|s| error("invalid_browser_command", s, false))?;
        if matches!(
            command.action,
            BrowserAction::Takeover | BrowserAction::Release
        ) && !scope.human
        {
            return Err(error(
                "browser_human_required",
                "接管或归还浏览器只能由用户操作",
                false,
            ));
        }
        let _busy = self.reserve(&scope)?;
        self.current(&scope, &cancel)?;
        if matches!(command.action, BrowserAction::Stop | BrowserAction::Destroy) {
            let live = {
                let mut p = self.pool.lock().unwrap_or_else(|q| q.into_inner());
                let e = p.entries.get_mut(&scope.session_id).unwrap();
                e.live.clone()
            };
            if let Some(live) = live {
                self.stop_owned(&live, true)?;
            }
            if let Some(entry) = self
                .pool
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .entries
                .get_mut(&scope.session_id)
                .filter(|entry| {
                    entry.owner == scope.owner_subject
                        && entry.epoch == scope.epoch
                        && !entry.state.revoked
                })
            {
                entry.state = empty(&scope, self.config.headless);
            }
            return self.output(&scope, json!({"stopped":true}), None);
        }
        if matches!(
            command.action,
            BrowserAction::Show | BrowserAction::Takeover
        ) && self.config.headless
        {
            return Err(error(
                "browser_visible_unavailable",
                "当前服务没有可见浏览器能力",
                false,
            ));
        }
        let launches = matches!(
            command.action,
            BrowserAction::Start
                | BrowserAction::Resume
                | BrowserAction::Navigate
                | BrowserAction::NewTab
                | BrowserAction::Show
                | BrowserAction::Takeover
        );
        let live = self.get_live(
            &scope,
            launches,
            command.action != BrowserAction::Start,
            &cancel,
        )?;
        let settings = self
            .pool
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .settings
            .clone();
        let deadline =
            Instant::now() + Duration::from_millis(u64::from(command.timeout.unwrap_or(30_000)));
        let result = ops::execute(&self, &scope, &live, &command, &settings, deadline, &cancel);
        let mut p = self.pool.lock().unwrap_or_else(|q| q.into_inner());
        if let Some(e) = p.entries.get_mut(&scope.session_id) {
            if scope.human {
                e.last_human = now_ms();
            } else {
                e.last_agent = now_ms();
            }
            e.state.suspended = command.action == BrowserAction::Suspend;
            sync(e);
        }
        drop(p);
        let (result, screenshot) = result?;
        self.current(&scope, &cancel)?;
        self.output(&scope, result, screenshot)
    }
    fn output(
        &self,
        scope: &BrowserScope,
        result: Value,
        screenshot: Option<Vec<u8>>,
    ) -> Result<BrowserOutput, BrowserError> {
        let mut p = self.pool.lock().unwrap_or_else(|q| q.into_inner());
        let e = p
            .entries
            .get_mut(&scope.session_id)
            .ok_or_else(|| error("browser_scope", "浏览器会话不存在", false))?;
        sync(e);
        let downloads = e
            .live
            .as_ref()
            .map(|l| {
                l.downloads
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .clone()
            })
            .unwrap_or_default();
        Ok(BrowserOutput {
            reply: BrowserReply {
                state: e.state.clone(),
                result,
                files: vec![],
                downloads: vec![],
            },
            screenshot,
            downloads,
        })
    }
    fn close_all(&self, permanent: bool) -> Result<(), BrowserError> {
        let live = {
            let mut p = self.pool.lock().unwrap_or_else(|q| q.into_inner());
            p.stopping |= permanent;
            self.startup_cancel.store(true, Ordering::Release);
            for lease in p.pdf.values() {
                lease.cancel.store(true, Ordering::Release);
            }
            let mut live = p
                .entries
                .values()
                .filter_map(|e| e.live.clone())
                .collect::<Vec<_>>();
            for candidate in p.auxiliary.values().chain(p.pending_cleanup.iter()) {
                if !live.iter().any(|current| Arc::ptr_eq(current, candidate)) {
                    live.push(candidate.clone());
                }
            }
            live
        };
        let failures = Mutex::new(None);
        std::thread::scope(|scope| {
            for l in live {
                let failures = &failures;
                scope.spawn(move || {
                    if let Err(e) = self.stop_owned(&l, true) {
                        *failures.lock().unwrap_or_else(|p| p.into_inner()) = Some(e);
                    }
                });
            }
        });
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            let p = self.pool.lock().unwrap_or_else(|p| p.into_inner());
            if p.launching == 0 && p.aux == 0 {
                break;
            }
            drop(p);
            if Instant::now() >= deadline {
                self.pool.lock().unwrap_or_else(|p| p.into_inner()).stopping = true;
                return Err(error(
                    "browser_cleanup_unknown",
                    "浏览器启动或辅助任务清理尚未确认",
                    true,
                ));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut result = failures.into_inner().unwrap_or_else(|p| p.into_inner());
        let uncleaned = self
            .pool
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .pdf
            .iter()
            .filter(|(_, lease)| !lease.busy && lease.live.is_none() && !lease.cleaned)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in uncleaned {
            if let Err(cause) = self.finish_pdf(&id, Instant::now() + Duration::from_secs(3)) {
                result.get_or_insert(cause);
            }
        }
        self.pool
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .pdf
            .retain(|_, lease| lease.busy || !lease.cleaned);
        if let Some(e) = result {
            self.pool.lock().unwrap_or_else(|p| p.into_inner()).stopping = true;
            Err(e)
        } else {
            let mut pool = self.pool.lock().unwrap_or_else(|p| p.into_inner());
            if !pool.pending_cleanup.is_empty()
                || !pool.auxiliary.is_empty()
                || !pool.pdf.is_empty()
            {
                pool.stopping = true;
                Err(error(
                    "browser_cleanup_unknown",
                    "启动期间的浏览器清理仍未确认",
                    true,
                ))
            } else {
                Ok(())
            }
        }
    }
}
impl BrowserPort for ChromiumHost {
    fn execute<'a>(
        &'a self,
        scope: BrowserScope,
        command: BrowserCommand,
        cancel: Arc<AtomicBool>,
    ) -> BrowserFuture<'a, BrowserOutput> {
        let core = self.core.clone();
        Box::pin(async move {
            let mut guard = CancelOnDrop {
                cancel: cancel.clone(),
                armed: true,
            };
            let result = tokio::task::spawn_blocking(move || core.execute(scope, command, cancel))
                .await
                .map_err(|_| error("browser_host_failed", "浏览器宿主任务未完成", true))?;
            guard.armed = false;
            result
        })
    }
    fn workspaces(&self) -> Result<Vec<BrowserWorkspace>, BrowserError> {
        let mut p = self.core.pool.lock().unwrap_or_else(|q| q.into_inner());
        Ok(p.entries
            .values_mut()
            .map(|e| {
                sync(e);
                e.state.clone()
            })
            .collect())
    }
    fn restore(
        &self,
        scope: BrowserScope,
        mut state: BrowserWorkspace,
    ) -> Result<(), BrowserError> {
        self.core.check_scope(&scope)?;
        if state.session_id != scope.session_id
            || state.execution_node != scope.execution_node
            || state.tabs.len() > 32
            || state
                .tabs
                .iter()
                .any(|t| t.url.as_deref().is_some_and(|u| !allowed_url(u)))
        {
            return Err(error(
                "browser_restore",
                "浏览器恢复事实与授权不一致",
                false,
            ));
        }
        state.session_path = format!("native:{}", scope.session_id);
        state.running = false;
        state.suspended = true;
        state.user_takeover = false;
        state.visible_capable = !self.core.config.headless;
        state.resumable = !state.tabs.is_empty();
        let mut p = self.core.pool.lock().unwrap_or_else(|q| q.into_inner());
        if p.stopping {
            return Err(error("browser_closed", "浏览器宿主已停止", false));
        }
        if p.retired.contains(&key(&scope)) || p.retired.len() >= 16_384 {
            return Err(error("browser_revoked", "已删除会话不能恢复浏览器", false));
        }
        if let Some(e) = p.entries.get(&scope.session_id) {
            if e.owner != scope.owner_subject || e.epoch > scope.epoch {
                return Err(error("browser_revoked", "浏览器恢复授权已过期", false));
            }
            if e.live.is_some() {
                return Ok(());
            }
        }
        p.entries.insert(
            scope.session_id,
            Entry {
                owner: scope.owner_subject,
                epoch: scope.epoch,
                state,
                live: None,
                busy: Arc::new(AtomicBool::new(false)),
                last_agent: 0,
                last_human: 0,
            },
        );
        Ok(())
    }
    fn revoke<'a>(&'a self, scope: BrowserScope) -> BrowserFuture<'a, ()> {
        self.run(move |core| {
            core.check_scope(&scope)?;
            let deadline = Instant::now() + Duration::from_secs(4);
            {
                let mut p = core.pool.lock().unwrap_or_else(|q| q.into_inner());
                let e = p
                    .entries
                    .entry(scope.session_id.clone())
                    .or_insert_with(|| Entry {
                        owner: scope.owner_subject.clone(),
                        epoch: 0,
                        state: empty(&scope, core.config.headless),
                        live: None,
                        busy: Arc::new(AtomicBool::new(false)),
                        last_agent: 0,
                        last_human: 0,
                    });
                if e.owner != scope.owner_subject
                    || scope.epoch < e.epoch
                    || scope.epoch == e.epoch && !e.state.revoked
                {
                    return Err(error("browser_revoked", "浏览器撤销代次无效", false));
                }
                sync(e);
                e.epoch = scope.epoch;
                e.state.revoked = true;
                e.state.user_takeover = false;
            }
            core.cancel_pdf(
                &PdfScope {
                    session_id: scope.session_id.clone(),
                    owner_subject: scope.owner_subject.clone(),
                    execution_node: scope.execution_node.clone(),
                },
                deadline,
            )?;
            loop {
                let live = {
                    let mut p = core.pool.lock().unwrap_or_else(|p| p.into_inner());
                    let entry = p
                        .entries
                        .get_mut(&scope.session_id)
                        .ok_or_else(|| error("browser_revoked", "急停期间工作区已移除", false))?;
                    if entry.owner != scope.owner_subject || entry.epoch != scope.epoch {
                        return Err(error("browser_revoked", "急停期间授权代次已改变", false));
                    }
                    // busy覆盖current检查通过到spawn/owner登记的间隙。只看live=None
                    // 不能证明进程不存在；原操作真正退出后才允许返回清空成功。
                    if entry.live.is_none() && !entry.busy.load(Ordering::Acquire) {
                        entry.state.running = false;
                        entry.state.resumable = false;
                        entry.state.tabs.clear();
                        entry.state.active_tab_id = None;
                        entry.state.url = None;
                        entry.state.title = None;
                        entry.state.unavailable_reason = None;
                        return Ok(());
                    }
                    entry.live.clone()
                };
                let attempt = if Instant::now() >= deadline {
                    Err(error(
                        "browser_cleanup_unknown",
                        "急停时正在启动的浏览器尚未收口",
                        true,
                    ))
                } else if let Some(live) = live {
                    core.stop_owned_until(&live, false, deadline)
                } else {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                };
                if let Err(cause) = attempt {
                    let mut p = core.pool.lock().unwrap_or_else(|p| p.into_inner());
                    p.stopping = true;
                    if let Some(entry) = p.entries.get_mut(&scope.session_id).filter(|entry| {
                        entry.owner == scope.owner_subject && entry.epoch == scope.epoch
                    }) {
                        entry.state.unavailable_reason = Some("browser_cleanup_unknown".into());
                    }
                    return Err(cause);
                }
            }
        })
    }
    fn configure<'a>(&'a self, settings: BrowserSettings) -> BrowserFuture<'a, ()> {
        self.run(move |core| {
            let previous = {
                let mut p = core.pool.lock().unwrap_or_else(|q| q.into_inner());
                if p.stopping || p.clearing || p.configuring {
                    return Err(error("browser_busy", "浏览器设置当前不可更新", false));
                }
                p.configuring = true;
                p.settings.clone()
            };
            struct Reset(Arc<Core>);
            impl Drop for Reset {
                fn drop(&mut self) {
                    let mut p = self.0.pool.lock().unwrap_or_else(|p| p.into_inner());
                    p.configuring = false;
                    if !p.stopping && !p.clearing {
                        self.0.startup_cancel.store(false, Ordering::Release);
                    }
                }
            }
            let _reset = Reset(core.clone());
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let p = core.pool.lock().unwrap_or_else(|p| p.into_inner());
                if p.stopping || p.clearing {
                    return Err(error("browser_closed", "设置更新时宿主已停止", false));
                }
                if p.launching == 0 {
                    break;
                }
                drop(p);
                if Instant::now() >= deadline {
                    return Err(error("browser_busy", "浏览器启动未在设置预算内收口", false));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            if previous.accept_cookies != settings.accept_cookies {
                // 原生网络Cookie策略只能在进程退出后可靠更改；保留冷状态，等待用户显式重开。
                core.close_all(false)?;
                for profile in managed_files::profile_names(&core.profiles_root)? {
                    managed_files::configure_cookies(
                        &core.profiles_root,
                        &profile,
                        settings.accept_cookies,
                    )?;
                }
                let mut p = core.pool.lock().unwrap_or_else(|p| p.into_inner());
                for entry in p
                    .entries
                    .values_mut()
                    .filter(|entry| !entry.state.revoked && entry.state.resumable)
                {
                    entry.state.unavailable_reason = Some("cookie_policy_changed".into());
                }
            }
            let mut p = core.pool.lock().unwrap_or_else(|p| p.into_inner());
            if p.stopping || p.clearing {
                return Err(error("browser_closed", "设置提交时宿主已停止", true));
            }
            p.settings = settings;
            Ok(())
        })
    }
    fn forget<'a>(&'a self, scope: BrowserScope) -> BrowserFuture<'a, ()> {
        self.run(move |core| {
            core.check_scope(&scope)?;
            let identity = key(&scope);
            let live = {
                let mut p = core.pool.lock().unwrap_or_else(|p| p.into_inner());
                if p.clearing || p.configuring {
                    return Err(error("browser_busy", "资料维护中，暂不能删除工作区", false));
                }
                if let Some(entry) = p.entries.get(&scope.session_id) {
                    if entry.owner != scope.owner_subject || entry.epoch > scope.epoch {
                        return Err(error(
                            "browser_revoked",
                            "不能删除其他授权的浏览器资料",
                            false,
                        ));
                    }
                }
                // 退休记录仅保存不可逆摘要；上限后reserve/restore明确拒绝，不能丢墓碑复活旧scope。
                if p.retired.len() < 16_384 {
                    p.retired.insert(identity.clone());
                }
                p.entries.get_mut(&scope.session_id).and_then(|entry| {
                    entry.epoch = scope.epoch;
                    entry.state.revoked = true;
                    entry.live.clone()
                })
            };
            core.cancel_pdf(
                &PdfScope {
                    session_id: scope.session_id.clone(),
                    owner_subject: scope.owner_subject.clone(),
                    execution_node: scope.execution_node.clone(),
                },
                Instant::now() + Duration::from_secs(4),
            )?;
            if let Some(live) = live {
                if let Err(cause) = core.stop_owned(&live, true) {
                    core.pool.lock().unwrap_or_else(|p| p.into_inner()).stopping = true;
                    return Err(cause);
                }
            }
            // 等待这个会话在删除前已经开始的launch真正结束，避免清完又落盘profile。
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let p = core.pool.lock().unwrap_or_else(|p| p.into_inner());
                if p.entries
                    .get(&scope.session_id)
                    .is_none_or(|entry| !entry.busy.load(Ordering::Acquire))
                {
                    break;
                }
                drop(p);
                if Instant::now() >= deadline {
                    core.pool.lock().unwrap_or_else(|p| p.into_inner()).stopping = true;
                    return Err(error(
                        "browser_cleanup_unknown",
                        "工作区启动尚未收口，资料保留待重试",
                        true,
                    ));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            {
                let mut p = core.pool.lock().unwrap_or_else(|p| p.into_inner());
                if let Some(entry) = p.entries.get_mut(&scope.session_id) {
                    if entry.owner != scope.owner_subject || entry.epoch != scope.epoch {
                        return Err(error("browser_revoked", "删除期间工作区授权已改变", false));
                    }
                    if entry.live.is_some() || entry.busy.load(Ordering::Acquire) {
                        // 初次读取live=None后仍可能有迟到的startup owner；它的清理
                        // Unknown不能因launching归零而被当成已退出，更不能直接删profile。
                        entry.state.unavailable_reason = Some("browser_cleanup_unknown".into());
                        p.stopping = true;
                        return Err(error(
                            "browser_cleanup_unknown",
                            "删除前浏览器退出尚未确认，资料保留",
                            true,
                        ));
                    }
                }
            }
            managed_files::purge(&core.profiles_root, &[&identity])?;
            managed_files::purge(&core.downloads_root, &[&identity])?;
            core.pool
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .entries
                .remove(&scope.session_id);
            Ok(())
        })
    }
    fn clear_site_data<'a>(&'a self) -> BrowserFuture<'a, ()> {
        self.run(move |core| {
            {
                let mut p = core.pool.lock().unwrap_or_else(|p| p.into_inner());
                if p.stopping || p.clearing || p.configuring {
                    return Err(error("browser_busy", "浏览器宿主正在清理", false));
                }
                p.clearing = true;
            }
            struct Reset(Arc<Core>);
            impl Drop for Reset {
                fn drop(&mut self) {
                    let mut p = self.0.pool.lock().unwrap_or_else(|p| p.into_inner());
                    p.clearing = false;
                    if !p.stopping {
                        self.0.startup_cancel.store(false, Ordering::Release);
                    }
                }
            }
            let _reset = Reset(core.clone());
            core.close_all(false)?;
            // 只清Cookie与站点存储，不删除密码、书签或浏览器历史。
            for profile in managed_files::profile_names(&core.profiles_root)? {
                for relative in [
                    "Default/Cookies",
                    "Default/Cookies-journal",
                    "Default/Cookies-wal",
                    "Default/Cookies-shm",
                    "Default/Network/Cookies",
                    "Default/Network/Cookies-journal",
                    "Default/Network/Cookies-wal",
                    "Default/Network/Cookies-shm",
                    "Default/Local Storage",
                    "Default/IndexedDB",
                    "Default/Service Worker",
                    "Default/CacheStorage",
                    "Default/Session Storage",
                    "Default/WebStorage",
                    "Default/Storage",
                ] {
                    let mut components = vec![profile.as_str()];
                    components.extend(relative.split('/'));
                    managed_files::purge(&core.profiles_root, &components)?;
                }
            }
            Ok(())
        })
    }
    fn pdf_scopes(&self) -> Result<Vec<PdfScope>, BrowserError> {
        let pool = self.core.pool.lock().unwrap_or_else(|p| p.into_inner());
        let mut scopes = pool
            .pdf
            .values()
            .map(|lease| lease.scope.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        scopes.sort_by(|left, right| {
            (&left.session_id, &left.owner_subject, &left.execution_node).cmp(&(
                &right.session_id,
                &right.owner_subject,
                &right.execution_node,
            ))
        });
        Ok(scopes)
    }
    fn cancel_pdf_scope<'a>(&'a self, scope: PdfScope) -> BrowserFuture<'a, ()> {
        self.run(move |core| core.cancel_pdf(&scope, Instant::now() + Duration::from_secs(4)))
    }
    fn render_pdf<'a>(
        &'a self,
        scope: PdfScope,
        job: PdfRenderJob,
        cancel: Arc<AtomicBool>,
    ) -> BrowserFuture<'a, PdfRendered> {
        let core = self.core.clone();
        Box::pin(async move {
            let mut guard = CancelOnDrop {
                cancel: cancel.clone(),
                armed: true,
            };
            let result = tokio::task::spawn_blocking(move || core.render_pdf(scope, job, cancel))
                .await
                .map_err(|_| {
                    error(
                        "browser_host_failed",
                        "打印任务退出，资源回收结果需核验",
                        true,
                    )
                })?;
            guard.armed = false;
            result
        })
    }
    fn read_auxiliary_page<'a>(
        &'a self,
        url: String,
        expression: String,
        locale: Option<String>,
        cancel: Arc<AtomicBool>,
    ) -> BrowserFuture<'a, Value> {
        self.run(move |core| {
            if !allowed_url(&url) || url == "about:blank" || expression.len() > 100_000 {
                return Err(error(
                    "browser_auxiliary",
                    "辅助页面地址或可信表达式无效",
                    false,
                ));
            }
            {
                let mut p = core.pool.lock().unwrap_or_else(|q| q.into_inner());
                if p.stopping || p.clearing || p.configuring || p.aux >= 2 {
                    return Err(error("browser_capacity", "辅助浏览器已达上限", false));
                }
                p.aux += 1;
            }
            struct Auxiliary(Arc<Core>, PathBuf, Arc<AtomicBool>);
            impl Drop for Auxiliary {
                fn drop(&mut self) {
                    let mut p = self.0.pool.lock().unwrap_or_else(|p| p.into_inner());
                    if self.2.load(Ordering::Acquire) {
                        p.auxiliary.remove(&self.1.to_string_lossy().to_string());
                    }
                    p.aux -= 1;
                    drop(p);
                    if self.2.load(Ordering::Acquire) {
                        if let Some(name) = self.1.file_name().and_then(|name| name.to_str()) {
                            let _ = managed_files::purge(&self.0.auxiliary_root, &[name]);
                        }
                    }
                }
            }
            let mut random = [0u8; 16];
            let random_result = getrandom::getrandom(&mut random);
            if random_result.is_err() {
                core.pool.lock().unwrap_or_else(|p| p.into_inner()).aux -= 1;
                return Err(error("browser_random", "无法分配辅助浏览器身份", false));
            }
            let name: String = random.iter().map(|b| format!("{b:02x}")).collect();
            let path = core.config.root.join("aux").join(name);
            let cleanup = Arc::new(AtomicBool::new(true));
            let _guard = Auxiliary(core.clone(), path.clone(), cleanup.clone());
            private_dir(&path)?;
            let downloads = path.join("downloads");
            private_dir(&downloads)?;
            let prepared = Live::prepare(downloads)?;
            let process = Process::launch(
                &core.config.executable,
                &path,
                true,
                &proxy_arguments(&core.config.network)?,
                core.launch_policy(),
            )?;
            cleanup.store(false, Ordering::Release);
            let live = Live::allocate(process, prepared, core.config.network.generation(), true);
            {
                let mut p = core.pool.lock().unwrap_or_else(|p| p.into_inner());
                // 初始化、首target或并发关机失败都必须能找到这个实际owner。
                p.pending_cleanup.push(live.clone());
                p.auxiliary
                    .insert(path.to_string_lossy().to_string(), live.clone());
            }
            let operation: Result<Value, BrowserError> = (|| {
                {
                    let p = core.pool.lock().unwrap_or_else(|p| p.into_inner());
                    if p.stopping || p.clearing || p.configuring {
                        return Err(error(
                            "browser_closed",
                            "辅助浏览器启动期间宿主状态已改变",
                            false,
                        ));
                    }
                }
                live.initialize(&core.startup_cancel)?;
                let deadline = Instant::now() + Duration::from_secs(30);
                let tab = ops::new_tab(&live, "about:blank", false, deadline, &cancel)?;
                let sid = live.attach(&tab, deadline, &cancel)?;
                if let Some(locale) = locale {
                    if locale.len() > 64
                        || locale.is_empty()
                        || !locale
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                    {
                        return Err(error("browser_locale", "辅助浏览器语言无效", false));
                    }
                    live.call(
                        Some(&sid),
                        "Network.enable",
                        json!({}),
                        deadline,
                        &cancel,
                        false,
                    )?;
                    live.call(
                        Some(&sid),
                        "Network.setExtraHTTPHeaders",
                        json!({"headers":{"Accept-Language":locale}}),
                        deadline,
                        &cancel,
                        false,
                    )?;
                }
                ops::navigate(&live, &sid, &url, deadline, &cancel)?;
                let value = ops::evaluate(&live, &sid, &expression, deadline, &cancel, false)?;
                if serde_json::to_vec(&value)
                    .map_err(|_| error("browser_protocol", "辅助浏览器结果无效", false))?
                    .len()
                    > 2 * 1024 * 1024
                {
                    return Err(error("browser_budget", "辅助页面正文超出上限", false));
                }
                Ok(value)
            })();
            let stopped = core.stop_owned(&live, true);
            if stopped.is_ok() {
                cleanup.store(true, Ordering::Release);
            }
            stopped?;
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| error("browser_cleanup_unknown", "辅助资料身份无效", true))?;
            managed_files::purge(&core.auxiliary_root, &[name])?;
            cleanup.store(false, Ordering::Release);
            operation
        })
    }
    fn acknowledge_downloads<'a>(
        &'a self,
        scope: BrowserScope,
        ids: Vec<String>,
    ) -> BrowserFuture<'a, ()> {
        self.run(move |core| {
            use std::os::fd::AsRawFd;
            if ids.len() > 64 {
                return Err(error(
                    "browser_download_budget",
                    "确认下载数量超过上限",
                    false,
                ));
            }
            core.check_scope(&scope)?;
            core.current(&scope, &AtomicBool::new(false))?;
            let live = core
                .pool
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .entries
                .get(&scope.session_id)
                .and_then(|e| e.live.clone());
            if let Some(live) = live {
                let mut downloads = live.downloads.lock().unwrap_or_else(|p| p.into_inner());
                for id in ids {
                    if let Some(index) = downloads
                        .iter()
                        .position(|d| d.id == id && d.completed && d.bytes.is_some())
                    {
                        let name = std::ffi::CString::new(id)
                            .map_err(|_| error("browser_download", "下载编号无效", false))?;
                        if unsafe {
                            libc::unlinkat(live.download_root.as_raw_fd(), name.as_ptr(), 0)
                        } != 0
                            && std::io::Error::last_os_error().kind()
                                != std::io::ErrorKind::NotFound
                        {
                            return Err(error(
                                "browser_download_cleanup",
                                "已登记下载文件尚未清理",
                                true,
                            ));
                        }
                        downloads.remove(index);
                    }
                }
            }
            Ok(())
        })
    }
    fn reap_idle<'a>(&'a self, now: u64) -> BrowserFuture<'a, ()> {
        self.run(move |core| {
            let candidates = {
                let mut p = core.pool.lock().unwrap_or_else(|p| p.into_inner());
                p.entries
                    .iter_mut()
                    .filter_map(|(id, entry)| {
                        let live = entry.live.as_ref()?;
                        if entry.busy.load(Ordering::Acquire)
                            || live.visible.load(Ordering::Acquire)
                            || now.saturating_sub(entry.last_agent.max(entry.last_human))
                                < 30 * 60_000
                        {
                            return None;
                        }
                        entry.busy.store(true, Ordering::Release);
                        Some((id.clone(), live.clone(), Busy(entry.busy.clone())))
                    })
                    .collect::<Vec<_>>()
            };
            for (id, live, _held) in candidates {
                if !live.hidden_for_reaping(
                    core.config.headless,
                    Instant::now() + Duration::from_secs(2),
                )? {
                    continue;
                }
                let eligible = {
                    let p = core.pool.lock().unwrap_or_else(|p| p.into_inner());
                    if p.configuring || p.clearing {
                        return Err(error("browser_busy", "资料维护期间不能回收", false));
                    }
                    p.entries.get(&id).is_some_and(|entry| {
                        entry
                            .live
                            .as_ref()
                            .is_some_and(|current| Arc::ptr_eq(current, &live))
                    })
                };
                if eligible {
                    core.stop_owned(&live, true)?;
                }
            }
            Ok(())
        })
    }
    fn shutdown<'a>(&'a self) -> BrowserFuture<'a, ()> {
        self.run(move |core| core.close_all(true))
    }
}
impl Drop for ChromiumHost {
    fn drop(&mut self) {
        let _ = self.core.close_all(true);
    }
}

fn proxy_arguments(plane: &NetworkPlane) -> Result<Vec<String>, BrowserError> {
    let policy = plane.policy();
    if policy.trusted_ca_pem.is_some() {
        return Err(error(
            "browser_proxy_unsupported",
            "当前Chromium未提供自定义CA的可信安装，拒绝忽略此设置",
            false,
        ));
    }
    let (http, https, bypass) = match &policy.proxy {
        ProxyPolicy::Direct => return Ok(vec!["--no-proxy-server".into()]),
        ProxyPolicy::Manual {
            http,
            https,
            no_proxy,
        } => (http.clone(), https.clone(), no_proxy.clone()),
        ProxyPolicy::System { env } => {
            let read = |a: &str, b: &str| {
                env.get(a)
                    .or_else(|| env.get(b))
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            };
            let all = read("ALL_PROXY", "all_proxy");
            let http = read("HTTP_PROXY", "http_proxy").or_else(|| all.clone());
            let https = read("HTTPS_PROXY", "https_proxy")
                .or_else(|| http.clone())
                .or(all);
            (
                http,
                https,
                read("NO_PROXY", "no_proxy").unwrap_or_default(),
            )
        }
    };
    let normalize = |raw: Option<String>| -> Result<Option<String>, BrowserError> {
        raw.map(|s| {
            let url = reqwest::Url::parse(&s)
                .map_err(|_| error("browser_proxy_invalid", "浏览器代理地址无效", false))?;
            if !matches!(url.scheme(), "http" | "https" | "socks" | "socks5")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
                || !matches!(url.path(), "" | "/")
            {
                return Err(error(
                    "browser_proxy_unsupported",
                    "浏览器代理不支持此凭据或地址形态",
                    false,
                ));
            }
            Ok(url
                .to_string()
                .trim_end_matches('/')
                .replace("socks://", "socks5://"))
        })
        .transpose()
    };
    let http = normalize(http)?;
    let https = normalize(https)?;
    let h = http.as_ref().or(https.as_ref());
    let s = https.as_ref().or(http.as_ref());
    if h.is_none() && s.is_none() {
        return Ok(vec!["--no-proxy-server".into()]);
    }
    if bypass.chars().any(|c| c.is_control() || c == ';') {
        return Err(error(
            "browser_proxy_invalid",
            "浏览器代理绕过范围无效",
            false,
        ));
    }
    Ok(vec![
        format!("--proxy-server=http={};https={}", h.unwrap(), s.unwrap()),
        format!(
            "--proxy-bypass-list=localhost;127.*;[::1];{}",
            bypass.replace(',', ";")
        ),
    ])
}
