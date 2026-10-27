//! R09-T02 退出清理（C18/C19/C20）。
//!
//! own 路径的常量与 Electron 对齐（`desktop/main.cjs:725-728`）：
//!   SIGTERM → 等 17s → SIGKILL → 等 5s，轮询 200ms。
//! 服务自身 graceful 预算默认 10s（`--shutdown-timeout-ms`），17s 宽限覆盖它。
//!
//! 结构性零接触（C19）：复用/远端模式在 Supervisor 里根本不持有 Child，本模块
//! 只处理 owned 子进程；未确认退出时保留 `residual` 与 pid，绝不假装清理完成。

use std::process::{Child, ExitStatus};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

pub const SHUTDOWN_GRACE: Duration = Duration::from_secs(17);
pub const FORCE_KILL_WAIT: Duration = Duration::from_secs(5);
pub const SHUTDOWN_POLL: Duration = Duration::from_millis(200);

#[derive(Debug)]
pub struct OwnedChild {
    pub child: Child,
    pub pid: u32,
    /// Unix：spawn 时 `process_group(0)` 使 pid == pgid；用它收敛整棵自有进程树。
    pub pgid: Option<i32>,
}

impl OwnedChild {
    pub fn new(child: Child) -> Self {
        let pid = child.id();
        Self {
            child,
            pid,
            pgid: process_group(pid),
        }
    }

    pub fn try_wait(&mut self) -> Option<ExitStatus> {
        match self.child.try_wait() {
            Ok(status) => status,
            Err(error) => {
                eprintln!(
                    "[tauri-service] try_wait failed for pid {pid}: {error}",
                    pid = self.pid
                );
                None
            }
        }
    }

    pub fn kill_now(&mut self) {
        let _ = self.child.kill();
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShutdownReport {
    pub mode: String,
    pub owned_pids: Vec<u32>,
    pub signaled: Vec<String>,
    pub residual: Vec<u32>,
    pub grace_ms: u64,
    pub force_kill_wait_ms: u64,
    pub confirmed: bool,
    pub touched_remote: bool,
    /// 实际生效的退出码/信号（服务优雅退出通常是 0）。
    pub exit: Option<Value>,
}

impl ShutdownReport {
    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }

    /// 复用/远端：不持有 Child，退出路径没有杀进程代码（C19 结构性零接触）。
    pub fn detached(mode: &str) -> Self {
        Self {
            mode: mode.to_string(),
            owned_pids: Vec::new(),
            signaled: Vec::new(),
            residual: Vec::new(),
            grace_ms: 0,
            force_kill_wait_ms: 0,
            confirmed: false,
            touched_remote: false,
            exit: None,
        }
    }
}

#[cfg(unix)]
pub fn process_group(pid: u32) -> Option<i32> {
    if pid == 0 || pid > i32::MAX as u32 {
        return None;
    }
    let pgid = unsafe { libc::getpgid(pid as libc::pid_t) };
    if pgid > 0 {
        Some(pgid)
    } else {
        None
    }
}

#[cfg(not(unix))]
pub fn process_group(_pid: u32) -> Option<i32> {
    None
}

#[cfg(unix)]
fn signal_pid(pid: i32, signal: i32) -> bool {
    pid > 0 && unsafe { libc::kill(pid as libc::pid_t, signal) } == 0
}

#[cfg(unix)]
fn signal_group(pgid: i32, signal: i32) -> bool {
    pgid > 0 && unsafe { libc::kill(-(pgid as libc::pid_t), signal) } == 0
}

#[cfg(not(unix))]
fn signal_pid(_pid: i32, _signal: i32) -> bool {
    false
}

#[cfg(not(unix))]
fn signal_group(_pgid: i32, _signal: i32) -> bool {
    false
}

pub fn sigterm() -> i32 {
    15
}

pub fn sigkill() -> i32 {
    9
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProcessEntry {
    pub pid: u32,
    pub ppid: u32,
    pub pgid: u32,
    pub comm: String,
}

/// 进程表快照（`ps`）——证据面用（A04 进程所有权探针）。
pub fn process_table() -> Vec<ProcessEntry> {
    let output = std::process::Command::new("/bin/ps")
        .args(["-axo", "pid=,ppid=,pgid=,comm="])
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let pid = fields.next()?.parse().ok()?;
            let ppid = fields.next()?.parse().ok()?;
            let pgid = fields.next()?.parse().ok()?;
            let comm = fields.collect::<Vec<_>>().join(" ");
            Some(ProcessEntry {
                pid,
                ppid,
                pgid,
                comm,
            })
        })
        .collect()
}

/// 自有进程组内仍存活的进程（退出报告 residual 用；不杀，只报告）。
pub fn group_survivors(pgid: Option<i32>, exclude: &[u32]) -> Vec<u32> {
    let Some(pgid) = pgid else {
        return Vec::new();
    };
    let pgid = pgid as u32;
    process_table()
        .into_iter()
        .filter(|entry| entry.pgid == pgid && !exclude.contains(&entry.pid))
        .map(|entry| entry.pid)
        .collect()
}

fn wait_exit(child: &mut OwnedChild, budget: Duration, poll: Duration) -> bool {
    let started = Instant::now();
    loop {
        if child.try_wait().is_some() {
            return true;
        }
        if started.elapsed() >= budget {
            return false;
        }
        std::thread::sleep(poll);
    }
}

/// owned 子进程的梯度关停：SIGTERM → grace → SIGKILL（进程组）→ force wait。
pub fn stop_owned(child: &mut OwnedChild, timings: &super::supervisor::Timings) -> ShutdownReport {
    let pid = child.pid;
    let pgid = child.pgid;
    let mut signaled: Vec<String> = Vec::new();
    let mut report = ShutdownReport {
        mode: "local-owned".to_string(),
        owned_pids: vec![pid],
        signaled: Vec::new(),
        residual: Vec::new(),
        grace_ms: timings.shutdown_grace.as_millis() as u64,
        force_kill_wait_ms: timings.force_kill_wait.as_millis() as u64,
        confirmed: false,
        touched_remote: false,
        exit: None,
    };
    // 如果进程已在退出前自然结束，直接 reap 并记录。
    if let Some(status) = child.try_wait() {
        report.exit = Some(status_json(status));
        signaled.push("already-exited".to_string());
        report.signaled = signaled;
        report.confirmed = true;
        return report;
    }
    if signal_pid(pid as i32, sigterm()) {
        signaled.push(format!("SIGTERM:pid={pid}"));
    } else {
        // 进程可能在检查与信号之间退出：交给 wait 循环确认。
        signaled.push(format!("SIGTERM:pid={pid}:not-delivered"));
    }
    let mut exited = wait_exit(child, timings.shutdown_grace, timings.poll);
    if !exited {
        let group_sent = pgid
            .map(|pgid| signal_group(pgid, sigkill()))
            .unwrap_or(false);
        let pid_sent = signal_pid(pid as i32, sigkill());
        signaled.push(format!(
            "SIGKILL:pid={pid}:{pid_sent}:pgid={pgid:?}:{group_sent}"
        ));
        exited = wait_exit(child, timings.force_kill_wait, timings.poll);
    }
    report.signaled = signaled;
    if exited {
        report.exit = child.child.try_wait().ok().flatten().map(status_json);
        report.confirmed = true;
        report.residual = group_survivors(pgid, &[pid]);
    } else {
        report.confirmed = false;
        let mut residual = vec![pid];
        residual.extend(group_survivors(pgid, &[pid]));
        residual.sort_unstable();
        residual.dedup();
        report.residual = residual;
    }
    report
}

pub fn status_json(status: ExitStatus) -> Value {
    let signal = {
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            status.signal()
        }
        #[cfg(not(unix))]
        {
            None
        }
    };
    json!({"code": status.code(), "signal": signal, "success": status.success()})
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    fn timings() -> super::super::supervisor::Timings {
        super::super::supervisor::Timings {
            shutdown_grace: Duration::from_millis(2_000),
            force_kill_wait: Duration::from_millis(2_000),
            poll: Duration::from_millis(50),
            ..super::super::supervisor::Timings::default()
        }
    }

    fn spawn_sleep() -> OwnedChild {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            let mut command = Command::new("/bin/sh");
            command.args(["-c", "sleep 30"]);
            command.stdout(Stdio::null()).stderr(Stdio::null());
            command.process_group(0);
            OwnedChild::new(command.spawn().unwrap())
        }
        #[cfg(not(unix))]
        {
            let child = Command::new("cmd")
                .args(["/C", "timeout", "/T", "30"])
                .spawn()
                .unwrap();
            OwnedChild::new(child)
        }
    }

    #[test]
    fn graceful_sigterm_stops_child_and_reports() {
        let mut child = spawn_sleep();
        // /bin/sh 会等 sleep 结束；SIGTERM 到 sh 后 sh 退出，sleep 可能残留在组里。
        let report = stop_owned(&mut child, &timings());
        assert!(report.confirmed, "report: {}", report.to_json());
        assert_eq!(report.mode, "local-owned");
        assert!(report.owned_pids.contains(&child.pid));
        assert!(!report.touched_remote);
        assert!(report
            .signaled
            .iter()
            .any(|entry| entry.starts_with("SIGTERM")));
        let survivors = group_survivors(child.pgid, &[child.pid]);
        assert!(survivors.is_empty(), "survivors: {survivors:?}");
    }

    #[test]
    fn detached_report_never_touches_remote() {
        let report = ShutdownReport::detached("remote");
        assert_eq!(report.mode, "remote");
        assert!(!report.confirmed);
        assert!(!report.touched_remote);
        assert!(report.owned_pids.is_empty());
        assert!(report.signaled.is_empty());
    }

    #[test]
    fn process_table_contains_self() {
        let table = process_table();
        assert!(!table.is_empty(), "ps snapshot must not be empty");
        let self_pid = std::process::id();
        assert!(table.iter().any(|entry| entry.pid == self_pid));
    }
}
