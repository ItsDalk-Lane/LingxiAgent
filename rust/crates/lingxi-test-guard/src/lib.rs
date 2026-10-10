//! lingxi-test-guard — 测试进程内存/时间守卫（R06-T06 事故防护）。
//!
//! ## 背景
//! R06-T06 事故：`r06_t06_memory` 测试进程因一个无限循环的字符串累积，在
//! ~61 分钟内常驻内存涨到 ~176GB，耗尽 96GB 机器内存导致整机卡死被看门狗
//! 重启。测试自身没有任何资源上限；`cargo test` 也不提供进程级 RSS 上限。
//!
//! ## 机制（默认对全部测试生效）
//! - 本 crate 由 `ctor` 在 **main 之前** 安装一个后台采样线程（"lingxi-test-guard"）。
//! - 采样周期 50ms，读取本进程真实常驻内存：
//!   - macOS：`task_info(MACH_TASK_BASIC_INFO).resident_size`（libc）；
//!   - Linux：`/proc/self/statm` 第 2 字段 × `sysconf(_SC_PAGESIZE)`。
//!   - 其它平台：内存采样返回 None（守卫显式退化为仅时间上限，见下）。
//! - 超限时 **立即**：
//!   1. 以无锁 `write(2)`（同时写 fd 1）打印：进程、超限原因、当前内存/耗时、
//!      生效阈值、以及当前存活的测试线程名（libtest 用测试名命名每个测试
//!      线程，这即是"哪个测试"）；
//!   2. 调用 `std::process::abort()` —— 后台线程 panic 不会终止测试主线程，
//!      必须用 abort 保证进程真正停止，泄漏不可能继续。
//!
//! ## 默认阈值与环境变量
//! | 环境变量 | 默认 | 语义 |
//! |---|---|---|
//! | `TEST_MEM_LIMIT_MB` | `8192`（8 GiB） | 进程常驻内存上限；`0` = 关闭内存检查 |
//! | `TEST_TIME_LIMIT_SEC` | `600`（10 分钟） | 进程墙钟上限；`0` = 关闭时间检查 |
//! | `LINGXI_TEST_GUARD` | 未设置（开启） | `0/off/false/no` = 整个守卫关闭 |
//! | `TEST_GUARD_VERBOSE` | 未设置（静默） | `1/true/yes/on` = 启动时打印生效阈值 |
//!
//! 取值非法（非整数、溢出）时 **不静默**：stderr 打印告警并显式回落到默认值。
//! 阈值刻意留有巨大富余（本机正常测试峰值见仓库文档；事故前统计：除泄漏
//! 进程外全部测试进程 < 1.5GB），命中即代表真实失控，而不是正常负载。
//!
//! ## 触发后的现象
//! 测试进程以 SIGABRT 终止（`cargo test` 报 "test binary ... failed"），
//! stderr/stdout 出现 `LINGXI_TEST_GUARD` 报告块：原因、当前值、阈值、
//! 存活测试线程名。修好泄漏前重复运行会重复在同一阈值处中止。
//!
//! ## 覆盖方式（为什么是 ctor + feature 边）
//! - 自定义 test harness 需要改写 115+ 个测试文件（侵入过大）；
//! - nextest setup 脚本无法在进程内安装监控（且仓库当前未使用 nextest）；
//! - `ctor` 的 pre-main 钩子是"默认随 `cargo test` 生效"的最小侵入方案：
//!   本 crate 只经 `test-guard` feature 边（由各 crate 的 dev-dependencies
//!   开启）进入测试构建；`cargo build` / release 图里没有这条边，零开销、
//!   零全局副作用。
//! - 注意：`cargo test` 期间测试 spawn 的真实服务子进程同样会被守卫覆盖
//!   （默认静默、阈值宽松）。需要完全关闭时用 `LINGXI_TEST_GUARD=0`。
//!
//! ## 已知边界
//! - macOS 线程名枚举使用 `task_threads` + `pthread_from_mach_thread_np`；
//!   libc 0.2.189 未导出 `mach_port_deallocate`，每个线程端口的名引用留给
//!   进程退出回收（本函数只在触发报告与单元测试中调用，量级有界）。
//! - Linux `comm` 截断到 15 字符：测试线程名可能显示为前缀。
//! - 内存采样是常驻内存（resident_size/statm），不包含被压缩/换出的部分。

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Once;
use std::time::{Duration, Instant};

/// 默认内存上限（MB）：8192 = 8 GiB。
pub const DEFAULT_MEM_LIMIT_MB: u64 = 8192;
/// 默认墙钟上限（秒）：600 = 10 分钟。
///
/// 取值依据（R06-T06-FIN2 实测）：既有最慢合法用例
/// `r04_t08_tool_matrix::approval_face_share_cases` 单跑恒需 ~300.5s
/// （T05 时代全量负载下 300.57s，候选 hooks-on 300.48s / hooks-off
/// 300.46s——与 hooks 无关的固有耗时）。原 300s 默认恰好卡在此用例
/// 完成线之下（300.02s abort），属于对已知合法用例的误伤；600s =
/// 已知最慢合法用例的 ~2 倍富余，同时仍远小于事故现场的 61 分钟失控，
/// 触发即代表真实失控。可用 `TEST_TIME_LIMIT_SEC` 覆盖。
pub const DEFAULT_TIME_LIMIT_SEC: u64 = 600;
/// 采样周期。50ms：task_info/statm 采样本身是微秒级开销，周期取小值是为了
/// 压缩"越限后到中止前"的超调窗口（20GB/s 的病态分配器下约 1GB；事故现场
/// 的 450MB/s 下约 22MB）——超调窗口远小于 96GB 机器内存。
const SAMPLE_PERIOD: Duration = Duration::from_millis(50);
/// 1 MiB。
const MIB: u64 = 1024 * 1024;

const STATE_UNINITIALIZED: u8 = 0;
const STATE_ARMED: u8 = 1;
const STATE_DISABLED: u8 = 2;

static STATE: AtomicU8 = AtomicU8::new(STATE_UNINITIALIZED);
static INSTALL: Once = Once::new();

/// 守卫安装状态（单元测试与诊断用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardState {
    /// ctor 尚未运行（正常测试进程里不应出现）。
    Uninitialized,
    /// 采样线程已安装。
    Armed,
    /// 被环境变量显式关闭，或监控线程无法创建（后者会打印告警）。
    Disabled,
}

/// 当前进程的守卫状态。
pub fn state() -> GuardState {
    match STATE.load(Ordering::SeqCst) {
        STATE_ARMED => GuardState::Armed,
        STATE_DISABLED => GuardState::Disabled,
        _ => GuardState::Uninitialized,
    }
}

/// 生效配置。`None` = 该项检查关闭。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuardConfig {
    pub mem_limit_bytes: Option<u64>,
    pub time_limit: Option<Duration>,
    pub verbose: bool,
}

impl GuardConfig {
    /// 纯解析（可单测）：从 (名, 值) 对读取环境变量语义，返回配置与告警。
    /// 非法值告警并回落默认（显式降级，绝不静默）。
    pub fn from_pairs(pairs: &[(String, String)]) -> (GuardConfig, Vec<String>) {
        let lookup = |name: &str| -> Option<&str> {
            pairs
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.as_str())
        };
        let mut warnings = Vec::new();

        if let Some(raw) = lookup("LINGXI_TEST_GUARD") {
            if matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "0" | "off" | "false" | "no"
            ) {
                return (
                    GuardConfig {
                        mem_limit_bytes: None,
                        time_limit: None,
                        verbose: false,
                    },
                    warnings,
                );
            }
        }

        let parse_limit = |name: &str,
                           raw: Option<&str>,
                           default: u64,
                           warnings: &mut Vec<String>|
         -> Option<u64> {
            match raw {
                None => Some(default),
                Some(raw) => match raw.trim().parse::<u64>() {
                    Ok(0) => None,
                    Ok(value) => Some(value),
                    Err(_) => {
                        warnings.push(format!(
                            "{name}={raw:?} is not a whole number; using the default \
                             {default} (set 0 to disable this check)"
                        ));
                        Some(default)
                    }
                },
            }
        };

        let mem_limit_bytes = parse_limit(
            "TEST_MEM_LIMIT_MB",
            lookup("TEST_MEM_LIMIT_MB"),
            DEFAULT_MEM_LIMIT_MB,
            &mut warnings,
        )
        .map(|mb| match mb.checked_mul(MIB) {
            Some(bytes) => bytes,
            None => {
                warnings.push(format!(
                    "TEST_MEM_LIMIT_MB={mb} overflows u64 bytes; using the \
                             default {DEFAULT_MEM_LIMIT_MB} MB"
                ));
                DEFAULT_MEM_LIMIT_MB * MIB
            }
        });
        let time_limit = parse_limit(
            "TEST_TIME_LIMIT_SEC",
            lookup("TEST_TIME_LIMIT_SEC"),
            DEFAULT_TIME_LIMIT_SEC,
            &mut warnings,
        )
        .map(Duration::from_secs);
        let verbose = matches!(
            lookup("TEST_GUARD_VERBOSE")
                .map(|value| value.trim().to_ascii_lowercase())
                .as_deref(),
            Some("1") | Some("true") | Some("yes") | Some("on")
        );

        (
            GuardConfig {
                mem_limit_bytes,
                time_limit,
                verbose,
            },
            warnings,
        )
    }

    /// 内存与时间检查都关闭。
    pub fn is_off(&self) -> bool {
        self.mem_limit_bytes.is_none() && self.time_limit.is_none()
    }
}

/// 一次超限判定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Breach {
    Memory { rss_bytes: u64, limit_bytes: u64 },
    Time { elapsed: Duration, limit: Duration },
}

/// 纯判定：内存超限优先于时间超限。`rss_bytes = None` 表示该平台/本次采样
/// 不可用（守卫显式退化为仅时间上限，绝不静默通过）。
pub fn detect_breach(
    config: &GuardConfig,
    rss_bytes: Option<u64>,
    elapsed: Duration,
) -> Option<Breach> {
    if let (Some(rss), Some(limit)) = (rss_bytes, config.mem_limit_bytes) {
        if rss > limit {
            return Some(Breach::Memory {
                rss_bytes: rss,
                limit_bytes: limit,
            });
        }
    }
    if let Some(limit) = config.time_limit {
        if elapsed > limit {
            return Some(Breach::Time { elapsed, limit });
        }
    }
    None
}

/// ctor 安装入口：main 之前读取环境变量并安装采样线程。
#[ctor::ctor]
fn lingxi_test_guard_ctor() {
    let (config, warnings) = GuardConfig::from_pairs(&std::env::vars().collect::<Vec<_>>());
    install_with(config, &warnings);
}

/// 安装（幂等）。仅由 ctor 使用；测试内调用可验证幂等性。
pub fn install_with(config: GuardConfig, warnings: &[String]) {
    INSTALL.call_once(|| {
        for warning in warnings {
            write_all(2, &format!("LINGXI_TEST_GUARD: {warning}\n"));
        }
        if config.is_off() {
            STATE.store(STATE_DISABLED, Ordering::SeqCst);
            return;
        }
        if config.verbose {
            write_all(
                2,
                &format!(
                    "LINGXI_TEST_GUARD: armed (pid {}, mem_limit={}, time_limit={})\n",
                    std::process::id(),
                    match config.mem_limit_bytes {
                        Some(bytes) => format!("{} MB", bytes / MIB),
                        None => "off".to_string(),
                    },
                    match config.time_limit {
                        Some(limit) => format!("{} s", limit.as_secs()),
                        None => "off".to_string(),
                    },
                ),
            );
        }
        let spawned = std::thread::Builder::new()
            .name("lingxi-test-guard".to_string())
            .spawn(move || monitor_loop(config));
        match spawned {
            Ok(_handle) => STATE.store(STATE_ARMED, Ordering::SeqCst),
            Err(err) => {
                // 线程创建失败 = 守卫不可用：显式告警，绝不静默通过。
                write_all(
                    2,
                    &format!(
                        "LINGXI_TEST_GUARD: cannot spawn the monitor thread ({err}); \
                         the guard is NOT armed for this process\n"
                    ),
                );
                STATE.store(STATE_DISABLED, Ordering::SeqCst);
            }
        }
    });
}

fn monitor_loop(config: GuardConfig) {
    let start = Instant::now();
    loop {
        std::thread::sleep(SAMPLE_PERIOD);
        let elapsed = start.elapsed();
        if let Some(breach) = detect_breach(&config, current_rss_bytes(), elapsed) {
            let report = build_trip_report(&breach, elapsed, &config);
            // 无锁 raw write：奔逃测试线程可能持有 stderr 锁，任何带锁打印
            // 都可能永远等不到；此时必须保证消息一定出得去。stderr 是失败
            // 的常规通道；stdout 只在它指向别处时补写一份（有的 runner 只
            // 捕获其中一路），同一终端/管道下不重复刷屏。
            write_all(2, &report);
            if !same_fd_target(1, 2) {
                write_all(1, &report);
            }
            // 后台线程 panic 不会终止测试主线程 —— 必须 abort 才能保证
            // 泄漏进程真正停止（R06-T06 事故的 176GB 增长不给"优雅"留时间）。
            std::process::abort();
        }
    }
}

fn build_trip_report(breach: &Breach, elapsed: Duration, config: &GuardConfig) -> String {
    let exe = std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "<unknown test binary>".to_string());
    let mut report = String::new();
    report.push_str("\n=== LINGXI_TEST_GUARD: ABORTING TEST PROCESS TO PROTECT THE MACHINE ===\n");
    match breach {
        Breach::Memory {
            rss_bytes,
            limit_bytes,
        } => {
            report.push_str(&format!(
                "  reason  : resident memory {} exceeded the {} limit (TEST_MEM_LIMIT_MB)\n",
                format_bytes(*rss_bytes),
                format_bytes(*limit_bytes),
            ));
        }
        Breach::Time {
            elapsed: hit,
            limit,
        } => {
            report.push_str(&format!(
                "  reason  : wall clock {} exceeded the {} limit (TEST_TIME_LIMIT_SEC)\n",
                format_secs(*hit),
                format_secs(*limit),
            ));
        }
    }
    report.push_str(&format!("  process : {exe} (pid {})\n", std::process::id()));
    match config.mem_limit_bytes {
        Some(limit) => {
            report.push_str(&format!(
                "  memory  : now {} / limit {}\n",
                match current_rss_bytes() {
                    Some(rss) => format_bytes(rss),
                    None => "unavailable".to_string(),
                },
                format_bytes(limit)
            ));
        }
        None => report.push_str("  memory  : check disabled (TEST_MEM_LIMIT_MB=0)\n"),
    }
    report.push_str(&format!(
        "  elapsed : {} / time budget {}\n",
        format_secs(elapsed),
        match config.time_limit {
            Some(limit) => format_secs(limit),
            None => "off".to_string(),
        },
    ));
    let names = running_thread_names();
    report.push_str("  live test threads (libtest names each thread after its test function):\n");
    if names.is_empty() {
        report.push_str("    <thread-name enumeration unavailable on this platform>\n");
    } else {
        for name in names.iter().take(24) {
            report.push_str(&format!("    - {name}\n"));
        }
        if names.len() > 24 {
            report.push_str(&format!("    ... and {} more\n", names.len() - 24));
        }
    }
    report.push_str(
        "  next    : reproduce single-threaded (`cargo test <name> -- --test-threads=1 --nocapture`);\n\
        \x20           adjust or disable: TEST_MEM_LIMIT_MB=16384, TEST_MEM_LIMIT_MB=0,\n\
        \x20           TEST_TIME_LIMIT_SEC=0 (0 = off), LINGXI_TEST_GUARD=0 (off entirely)\n\
         ===========================================================================\n",
    );
    report
}

fn format_bytes(bytes: u64) -> String {
    let gib = bytes as f64 / (1024.0 * 1024.0 * 1024.0);
    format!("{gib:.2} GiB ({bytes} bytes)")
}

fn format_secs(duration: Duration) -> String {
    format!("{:.2}s", duration.as_secs_f64())
}

/// 本进程当前常驻内存（字节）。不支持的平台返回 `None`。
#[cfg(target_os = "macos")]
#[allow(deprecated)] // libc's mach_task_self() is flagged in favor of a new
                     // (mach2) crate; introducing that crate for one trampoline
                     // read is not worth a new dependency — the binding works.
pub fn current_rss_bytes() -> Option<u64> {
    let mut info: libc::mach_task_basic_info = unsafe { std::mem::zeroed() };
    let mut count = libc::MACH_TASK_BASIC_INFO_COUNT;
    // SAFETY: `info` is a live, correctly-typed MACH_TASK_BASIC_INFO buffer;
    // `count` starts at the exact flavor count this kernel call fills.
    let result = unsafe {
        libc::task_info(
            libc::mach_task_self(),
            libc::MACH_TASK_BASIC_INFO,
            &mut info as *mut libc::mach_task_basic_info as libc::task_info_t,
            &mut count,
        )
    };
    if result != libc::KERN_SUCCESS {
        return None;
    }
    Some(info.resident_size)
}

/// 本进程当前常驻内存（字节）。不支持的平台返回 `None`。
#[cfg(target_os = "linux")]
pub fn current_rss_bytes() -> Option<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let resident_pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    let page_size = if page_size > 0 {
        page_size as u64
    } else {
        4096
    };
    resident_pages.checked_mul(page_size)
}

/// 本进程当前常驻内存（字节）。不支持的平台返回 `None`。
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn current_rss_bytes() -> Option<u64> {
    None
}

/// 当前进程内所有线程的名字（尽力而为；用于报告"哪个测试"）。
#[cfg(target_os = "macos")]
#[allow(deprecated)] // see current_rss_bytes() — libc binding over a new crate.
pub fn running_thread_names() -> Vec<String> {
    let mut names = Vec::new();
    unsafe {
        let task = libc::mach_task_self();
        let mut ports: libc::thread_act_array_t = std::ptr::null_mut();
        let mut count: libc::mach_msg_type_number_t = 0;
        if libc::task_threads(task, &mut ports, &mut count) != libc::KERN_SUCCESS {
            return names;
        }
        for index in 0..count as usize {
            let port = *ports.add(index);
            let pthread = libc::pthread_from_mach_thread_np(port);
            // pthread_t is uintptr_t (0 = failure), not a pointer, on Apple.
            if pthread != 0 {
                let mut buffer = [0_i8; 64];
                if libc::pthread_getname_np(pthread, buffer.as_mut_ptr(), buffer.len()) == 0 {
                    let name = std::ffi::CStr::from_ptr(buffer.as_ptr())
                        .to_string_lossy()
                        .into_owned();
                    if !name.is_empty() {
                        names.push(name);
                    }
                }
            }
            // NOTE: per-thread port rights are not individually deallocated —
            // libc 0.2.189 does not expose mach_port_deallocate; the port
            // array itself is released below and the remaining rights die
            // with the (about to abort) process.
        }
        libc::vm_deallocate(
            task,
            ports as libc::vm_address_t,
            (count as usize * std::mem::size_of::<libc::thread_act_t>()) as libc::vm_size_t,
        );
    }
    names.sort();
    names.dedup();
    names.retain(|name| name != "lingxi-test-guard");
    names
}

/// 当前进程内所有线程的名字（尽力而为；用于报告"哪个测试"）。
#[cfg(target_os = "linux")]
pub fn running_thread_names() -> Vec<String> {
    let mut names = Vec::new();
    if let Ok(entries) = std::fs::read_dir("/proc/self/task") {
        for entry in entries.flatten() {
            if let Ok(comm) = std::fs::read_to_string(entry.path().join("comm")) {
                let name = comm.trim_end_matches('\n').to_string();
                if !name.is_empty() {
                    names.push(name);
                }
            }
        }
    }
    names.sort();
    names.dedup();
    names.retain(|name| name != "lingxi-test-guard");
    names
}

/// 当前进程内所有线程的名字（尽力而为；用于报告"哪个测试"）。
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn running_thread_names() -> Vec<String> {
    Vec::new()
}

/// 无锁写 fd（触发路径专用；不参与任何锁竞争）。
fn write_all(fd: i32, text: &str) {
    let mut bytes = text.as_bytes();
    while !bytes.is_empty() {
        // SAFETY: `bytes` is a live slice; `libc::write` only reads it.
        let written =
            unsafe { libc::write(fd, bytes.as_ptr() as *const libc::c_void, bytes.len()) };
        if written <= 0 {
            break;
        }
        bytes = &bytes[written as usize..];
    }
}

/// 两个 fd 是否指向同一终端/管道（用于避免同目标重复输出）。
fn same_fd_target(a: i32, b: i32) -> bool {
    // SAFETY: `fstat` fills caller-owned zeroed buffers and reads no memory
    // beyond them; both stats are plain integer fields on this path.
    unsafe {
        let mut stat_a: libc::stat = std::mem::zeroed();
        let mut stat_b: libc::stat = std::mem::zeroed();
        libc::fstat(a, &mut stat_a) == 0
            && libc::fstat(b, &mut stat_b) == 0
            && stat_a.st_dev == stat_b.st_dev
            && stat_a.st_ino == stat_b.st_ino
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(values: &[(&str, &str)]) -> Vec<(String, String)> {
        values
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    #[test]
    fn defaults_when_nothing_is_set() {
        let (config, warnings) = GuardConfig::from_pairs(&[]);
        assert_eq!(config.mem_limit_bytes, Some(DEFAULT_MEM_LIMIT_MB * MIB));
        assert_eq!(
            config.time_limit,
            Some(Duration::from_secs(DEFAULT_TIME_LIMIT_SEC))
        );
        assert!(!config.verbose);
        assert!(warnings.is_empty());
        assert!(!config.is_off());
    }

    #[test]
    fn zero_disables_each_check_independently() {
        let (config, warnings) = GuardConfig::from_pairs(&pairs(&[("TEST_MEM_LIMIT_MB", "0")]));
        assert_eq!(config.mem_limit_bytes, None);
        assert_eq!(
            config.time_limit,
            Some(Duration::from_secs(DEFAULT_TIME_LIMIT_SEC))
        );
        assert!(warnings.is_empty());

        let (config, warnings) = GuardConfig::from_pairs(&pairs(&[("TEST_TIME_LIMIT_SEC", "0")]));
        assert_eq!(config.time_limit, None);
        assert_eq!(config.mem_limit_bytes, Some(DEFAULT_MEM_LIMIT_MB * MIB));
        assert!(warnings.is_empty());

        let (config, _) = GuardConfig::from_pairs(&pairs(&[
            ("TEST_MEM_LIMIT_MB", "0"),
            ("TEST_TIME_LIMIT_SEC", "0"),
        ]));
        assert!(config.is_off());
    }

    #[test]
    fn explicit_values_override_the_defaults() {
        let (config, warnings) = GuardConfig::from_pairs(&pairs(&[
            ("TEST_MEM_LIMIT_MB", "256"),
            ("TEST_TIME_LIMIT_SEC", "42"),
            ("TEST_GUARD_VERBOSE", "1"),
        ]));
        assert_eq!(config.mem_limit_bytes, Some(256 * MIB));
        assert_eq!(config.time_limit, Some(Duration::from_secs(42)));
        assert!(config.verbose);
        assert!(warnings.is_empty());
    }

    #[test]
    fn master_switch_disables_everything() {
        for raw in ["0", "off", "false", "no", "OFF"] {
            let (config, warnings) = GuardConfig::from_pairs(&pairs(&[("LINGXI_TEST_GUARD", raw)]));
            assert!(
                config.is_off(),
                "LINGXI_TEST_GUARD={raw} must disable both checks"
            );
            assert!(warnings.is_empty());
        }
    }

    #[test]
    fn malformed_values_warn_loudly_and_keep_defaults() {
        let (config, warnings) = GuardConfig::from_pairs(&pairs(&[
            ("TEST_MEM_LIMIT_MB", "8GB"),
            ("TEST_TIME_LIMIT_SEC", "-1"),
        ]));
        assert_eq!(config.mem_limit_bytes, Some(DEFAULT_MEM_LIMIT_MB * MIB));
        assert_eq!(
            config.time_limit,
            Some(Duration::from_secs(DEFAULT_TIME_LIMIT_SEC))
        );
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(warnings[0].contains("TEST_MEM_LIMIT_MB"));
        assert!(warnings[1].contains("TEST_TIME_LIMIT_SEC"));
    }

    #[test]
    fn overflowing_megabytes_warn_and_fall_back() {
        let (config, warnings) =
            GuardConfig::from_pairs(&pairs(&[("TEST_MEM_LIMIT_MB", &u64::MAX.to_string())]));
        assert_eq!(config.mem_limit_bytes, Some(DEFAULT_MEM_LIMIT_MB * MIB));
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("overflows"));
    }

    #[test]
    fn detect_breach_prefers_memory_then_time() {
        let config = GuardConfig {
            mem_limit_bytes: Some(100 * MIB),
            time_limit: Some(Duration::from_secs(10)),
            verbose: false,
        };
        assert_eq!(
            detect_breach(&config, Some(100 * MIB), Duration::from_secs(1)),
            None
        );
        assert!(matches!(
            detect_breach(&config, Some(100 * MIB + 1), Duration::from_secs(1)),
            Some(Breach::Memory { .. })
        ));
        assert!(matches!(
            detect_breach(&config, Some(MIB), Duration::from_secs(11)),
            Some(Breach::Time { .. })
        ));
        // Memory reading unavailable: the time check still protects.
        assert!(matches!(
            detect_breach(&config, None, Duration::from_secs(11)),
            Some(Breach::Time { .. })
        ));
        // Both checks disabled: never trips.
        let off = GuardConfig {
            mem_limit_bytes: None,
            time_limit: None,
            verbose: false,
        };
        assert_eq!(
            detect_breach(&off, Some(u64::MAX), Duration::from_secs(u64::MAX / 2)),
            None
        );
        // Reading unavailable + memory check on: no false trip from `None`.
        assert_eq!(detect_breach(&config, None, Duration::from_secs(1)), None);
    }

    #[test]
    fn rss_sampler_reports_a_plausible_value_for_this_live_process() {
        if cfg!(any(target_os = "macos", target_os = "linux")) {
            let rss = current_rss_bytes().expect("sampler must work on macOS/Linux");
            assert!(
                rss > MIB,
                "a live process has at least a MiB resident, got {rss}"
            );
            assert!(rss < (1_u64 << 40), "sanity: less than 1 TiB, got {rss}");
        }
    }

    #[test]
    fn thread_name_enumeration_sees_a_named_thread() {
        if !cfg!(any(target_os = "macos", target_os = "linux")) {
            return;
        }
        let handle = std::thread::Builder::new()
            .name("guard-selftest-thread".to_string())
            .spawn(|| {
                std::thread::sleep(Duration::from_millis(300));
            })
            .expect("spawn named thread");
        // Bounded polling: at most 10 samples while the thread is alive.
        let mut seen = false;
        for _ in 0..10 {
            std::thread::sleep(Duration::from_millis(20));
            if running_thread_names()
                .iter()
                .any(|name| name.starts_with("guard-selftest"))
            {
                seen = true;
                break;
            }
        }
        handle.join().expect("join named thread");
        assert!(
            seen,
            "thread-name enumeration must see `guard-selftest-thread` (Linux truncates to 15 chars)"
        );
    }

    #[test]
    fn guard_is_armed_by_default_in_this_test_process() {
        // The ctor ran before main: unless the runner disabled it via the
        // master switch or an off-limits process, the guard must be armed
        // in this very binary (this is the "installed by default" proof).
        let master_off = std::env::var("LINGXI_TEST_GUARD")
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "0" | "off" | "false" | "no"
                )
            })
            .unwrap_or(false);
        let (probe, _) = GuardConfig::from_pairs(&std::env::vars().collect::<Vec<_>>());
        if master_off || probe.is_off() {
            assert_eq!(state(), GuardState::Disabled);
        } else {
            assert_eq!(
                state(),
                GuardState::Armed,
                "the ctor must have armed the monitor before the test harness started"
            );
        }
    }

    #[test]
    fn install_is_idempotent() {
        let before = state();
        install_with(
            GuardConfig {
                mem_limit_bytes: Some(MIB),
                time_limit: None,
                verbose: false,
            },
            &[],
        );
        assert_eq!(
            state(),
            before,
            "a second install must not change the state"
        );
    }

    #[test]
    fn same_fd_target_detects_shared_and_distinct_targets() {
        if !cfg!(unix) {
            return;
        }
        // A dup'd fd shares the file description → same (dev, ino).
        let dup = unsafe { libc::dup(1) };
        assert!(dup >= 0, "dup(stdout) failed");
        assert!(same_fd_target(1, dup));
        unsafe { libc::close(dup) };

        // A private temp file is never stdout's/stderr's target in a test run.
        let unique = format!(
            "lingxi-guard-selftest-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        let file = std::fs::File::create(&path).expect("create temp file");
        {
            use std::os::unix::io::AsRawFd;
            assert!(!same_fd_target(1, file.as_raw_fd()));
        }
        drop(file);
        let _ = std::fs::remove_file(&path);
    }
}
