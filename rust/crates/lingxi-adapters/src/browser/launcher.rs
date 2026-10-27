//! 每个工作区持有独立浏览器进程组和profile；任何启动失败均由OwnedFd回收。
use super::{cdp::Cdp, error, BrowserError};
use std::{
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::process::CommandExt,
    },
    path::Path,
    process::{Child, Command, Stdio},
    sync::{atomic::AtomicBool, Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Default)]
pub(super) struct LaunchPolicy {
    #[cfg(test)]
    pub control: Option<Arc<StartupControl>>,
}
#[cfg(test)]
#[derive(Default)]
pub(super) struct StartupControl {
    pub fail_cdp_prepare: AtomicBool,
    pub fail_live_initialize: AtomicBool,
    pub fail_first_target: AtomicBool,
    pub unknown_stops: std::sync::atomic::AtomicUsize,
    pub spawned_pids: Mutex<Vec<u32>>,
    pub created_targets: Mutex<Vec<String>>,
    pub pause_after_spawn: AtomicBool,
    pub spawn_reached: AtomicBool,
    pub release_spawn: AtomicBool,
    pub reaped_pids: Mutex<Vec<u32>>,
}

fn descriptor_limit() -> Result<i32, BrowserError> {
    let mut limits = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limits) } != 0 {
        return Err(error("browser_pipe", "无法核对浏览器继承句柄上限", false));
    }
    let mut limit = limits.rlim_cur;
    // 兼容服务曾下调软限制、但仍持有高编号句柄的情况，只枚举编号而不读取内容。
    for fd in std::fs::read_dir("/dev/fd")
        .map_err(|_| error("browser_pipe", "无法核对浏览器继承句柄", false))?
    {
        let fd = fd.map_err(|_| error("browser_pipe", "无法核对浏览器继承句柄", false))?;
        if let Some(n) = fd
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u64>().ok())
        {
            limit = limit.max(n.saturating_add(1));
        }
    }
    if limit > 1_048_576 {
        return Err(error(
            "browser_fd_limit",
            "服务句柄上限超过安全启动预算",
            false,
        ));
    }
    Ok(limit as i32)
}
fn protect_inherited_descriptors(limit: i32) -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    if unsafe {
        libc::syscall(
            libc::SYS_close_range,
            5_u32,
            u32::MAX,
            libc::CLOSE_RANGE_CLOEXEC,
        )
    } == 0
    {
        return Ok(());
    }
    // 只设置exec时关闭，不提前关闭Rust内部spawn错误回报管道。
    for fd in 5..limit {
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        if flags < 0 {
            if std::io::Error::last_os_error().raw_os_error() == Some(libc::EBADF) {
                continue;
            }
            return Err(std::io::Error::last_os_error());
        }
        if unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

fn pipe() -> Result<(OwnedFd, OwnedFd), BrowserError> {
    let mut fds = [-1; 2];
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(error("browser_pipe", "无法建立浏览器管道", false));
    }
    // pipe成功后立即交给所有权对象；即使后续fcntl或第二条pipe失败也不泄露。
    let pair = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
    let duplicate = |fd: &OwnedFd| {
        let n = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 5) };
        if n < 0 {
            Err(error("browser_pipe", "无法保护浏览器管道句柄", false))
        } else {
            Ok(unsafe { OwnedFd::from_raw_fd(n) })
        }
    };
    Ok((duplicate(&pair.0)?, duplicate(&pair.1)?))
}
pub(super) struct Process {
    child: Mutex<Option<Child>>,
    pub cdp: Arc<Cdp>,
    pgid: i32,
    #[cfg(test)]
    pub control: Option<Arc<StartupControl>>,
}
impl Process {
    pub fn launch(
        executable: &Path,
        profile: &Path,
        headless: bool,
        network: &[String],
        _policy: LaunchPolicy,
    ) -> Result<Self, BrowserError> {
        let descriptor_limit = descriptor_limit()?;
        let (command_read, command_write) = pipe()?;
        let (response_read, response_write) = pipe()?;
        let read_fd = command_read.as_raw_fd();
        let write_fd = response_write.as_raw_fd();
        let mut cmd = Command::new(executable);
        cmd.arg("--remote-debugging-pipe")
            .arg(format!("--user-data-dir={}", profile.display()))
            .arg("--profile-directory=Default")
            .args([
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-session-crashed-bubble",
                "--hide-crash-restore-bubble",
                "--disable-background-networking",
                "--disable-component-update",
                "--disable-sync",
                "--metrics-recording-only",
                "--no-startup-window",
                "--window-size=1280,860",
            ])
            .args(network)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // R09-T05 自动化边界：无钥匙串访问会话下，Chrome 的 "Chrome Safe Storage"
        // 会弹 macOS 系统钥匙串窗口并阻塞自动化。仅当**测试 harness 显式**设置
        // `LINGXI_BROWSER_TEST_NO_KEYCHAIN=1` 时启用 mock keychain；生产/真实用户
        // 会话不设置该变量，cookie 加密语义保持真实默认，不静默改变。
        if std::env::var_os("LINGXI_BROWSER_TEST_NO_KEYCHAIN").is_some() {
            cmd.arg("--use-mock-keychain");
        }
        if headless {
            cmd.arg("--headless=new");
        }
        cmd.process_group(0);
        // 不传no-sandbox、不启用TCP调试，也不接受来自模型/HTTP的启动参数。
        unsafe {
            cmd.pre_exec(move || {
                if libc::dup2(read_fd, 3) < 0 || libc::dup2(write_fd, 4) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                protect_inherited_descriptors(descriptor_limit)
            });
        }
        // 全部会失败的CDP资源准备都在spawn前完成；reader此时由本地response_write
        // 保持管道，不能误收EOF。spawn成功之后只做无失败的所有权封装。
        let cdp = Cdp::new(response_read, command_write)?;
        #[cfg(test)]
        if _policy.control.as_ref().is_some_and(|control| {
            control
                .fail_cdp_prepare
                .swap(false, std::sync::atomic::Ordering::AcqRel)
        }) {
            return Err(error(
                "browser_startup_test_cdp",
                "测试注入CDP准备失败",
                false,
            ));
        }
        let child = cmd
            .spawn()
            .map_err(|_| error("browser_unavailable", "无法启动受控浏览器", false))?;
        drop(command_read);
        drop(response_write);
        // Unix的成功spawn返回正pid_t，Child仅以u32容纳它；这里恢复同一原生编号。
        let pgid = child.id() as libc::pid_t;
        #[cfg(test)]
        if let Some(control) = &_policy.control {
            control
                .spawned_pids
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push(child.id());
        }
        let process = Self {
            child: Mutex::new(Some(child)),
            cdp,
            pgid,
            #[cfg(test)]
            control: _policy.control,
        };
        #[cfg(test)]
        if let Some(control) = process.control.as_ref().filter(|control| {
            control
                .pause_after_spawn
                .load(std::sync::atomic::Ordering::Acquire)
        }) {
            control
                .spawn_reached
                .store(true, std::sync::atomic::Ordering::Release);
            let deadline = Instant::now() + Duration::from_secs(10);
            while !control
                .release_spawn
                .load(std::sync::atomic::Ordering::Acquire)
                && Instant::now() < deadline
            {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        Ok(process)
    }
    pub fn stop(&self, graceful: bool) -> Result<(), BrowserError> {
        self.stop_until(graceful, Instant::now() + Duration::from_secs(3))
    }
    pub fn stop_until(&self, graceful: bool, deadline: Instant) -> Result<(), BrowserError> {
        let mut holder = loop {
            match self.child.try_lock() {
                Ok(holder) => break holder,
                Err(std::sync::TryLockError::Poisoned(p)) => break p.into_inner(),
                Err(std::sync::TryLockError::WouldBlock) => {
                    if Instant::now() >= deadline {
                        return Err(error(
                            "browser_cleanup_unknown",
                            "进程清理占用尚未释放",
                            true,
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        };
        let Some(child) = holder.as_mut() else {
            return Ok(());
        };
        #[cfg(test)]
        if self.control.as_ref().is_some_and(|control| {
            control
                .unknown_stops
                .fetch_update(
                    std::sync::atomic::Ordering::AcqRel,
                    std::sync::atomic::Ordering::Acquire,
                    |remaining| remaining.checked_sub(1),
                )
                .is_ok()
        }) {
            // 不杀进程以证明Unknown时真实Child仍存活，后续同owner必须能重试回收。
            return Err(error(
                "browser_cleanup_unknown",
                "测试注入进程退出尚未确认",
                true,
            ));
        }
        if graceful && !self.cdp.is_closed() {
            let _ = self.cdp.call(
                None,
                "Browser.close",
                serde_json::json!({}),
                deadline.min(Instant::now() + Duration::from_millis(350)),
                &AtomicBool::new(false),
                true,
            );
            std::thread::sleep(
                Duration::from_millis(200).min(deadline.saturating_duration_since(Instant::now())),
            );
        }
        self.cdp.close();
        // 只向本对象创建的、尚未wait回收的child所属组发信号，不从磁盘恢复PID。
        unsafe {
            libc::kill(-self.pgid, libc::SIGTERM);
        }
        // 回收leader之前先终止整个自有组，避免PID回收后再对可能重用的编号发信号。
        std::thread::sleep(
            Duration::from_millis(50).min(deadline.saturating_duration_since(Instant::now())),
        );
        unsafe {
            libc::kill(-self.pgid, libc::SIGKILL);
        }
        let wait_deadline = deadline.min(Instant::now() + Duration::from_secs(2));
        loop {
            match child.try_wait() {
                Ok(Some(_)) => {
                    #[cfg(test)]
                    if let Some(control) = &self.control {
                        control
                            .reaped_pids
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .push(child.id());
                    }
                    holder.take();
                    return Ok(());
                }
                Err(_) => break,
                _ => {}
            }
            if Instant::now() >= wait_deadline {
                break;
            }
            std::thread::sleep(
                Duration::from_millis(10)
                    .min(wait_deadline.saturating_duration_since(Instant::now())),
            );
        }
        Err(error(
            "browser_cleanup_unknown",
            "浏览器进程组退出尚未确认",
            true,
        ))
    }
    #[cfg(test)]
    pub fn is_reaped(&self) -> bool {
        self.child
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_none()
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.stop(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_does_not_inherit_unprotected_service_fds_and_spawn_errors_remain_real() {
        let (read, _write) = pipe().unwrap();
        let raw = unsafe { libc::fcntl(read.as_raw_fd(), libc::F_DUPFD, 32) };
        assert!(raw >= 32);
        let inherited = unsafe { OwnedFd::from_raw_fd(raw) };
        let limit = descriptor_limit().unwrap();
        let mut command = Command::new("/bin/sh");
        command.args([
            "-c",
            "test ! -e /dev/fd/\"$1\"",
            "fd-test",
            &raw.to_string(),
        ]);
        unsafe {
            command.pre_exec(move || protect_inherited_descriptors(limit));
        }
        assert!(command.status().unwrap().success());
        assert!(
            unsafe { libc::fcntl(inherited.as_raw_fd(), libc::F_GETFD) } >= 0,
            "子进程保护不得关闭父服务句柄"
        );
        let mut missing = Command::new("/no-such-lingxi-chromium-executable");
        unsafe {
            missing.pre_exec(move || protect_inherited_descriptors(limit));
        }
        assert_eq!(
            missing.spawn().err().unwrap().kind(),
            std::io::ErrorKind::NotFound,
            "不能吞掉真正exec失败"
        );
    }
}
