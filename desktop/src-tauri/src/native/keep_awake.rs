//! R09-T03 保持唤醒（`host:keep-awake`，OQ-09）。
//!
//! 现役 Electron（desktop/keep-awake.cjs + main.cjs:179/5693-5694/6685/6844）：
//! `powerSaveBlocker.start("prevent-app-suspension")`，状态
//! `{enabled, active, blockerId, type}`；`setEnabled(false)` 停止并回状态；
//! 重复 enable 幂等；will-quit dispose。
//!
//! Tauri 无官方 keep-awake 插件，三平台需自写原生：
//! - macOS：`IOPMAssertionCreateWithName(kIOPMAssertionTypePreventUserIdleSystemSleep)`；
//! - Windows：`SetThreadExecutionState(ES_CONTINUOUS|ES_SYSTEM_REQUIRED)`；
//! - Linux：systemd-inhibit/DBus 未实测 → 显式 `Unsupported`（BLOCKED_PLATFORM），
//!   不做假成功。
//!
//! 本模块是与平台无关的状态机 + 后端 trait：后端（外部系统）可注入，宿主自己的
//! 授权/状态一致性逻辑是真实实现（不是 mock 被测边界）。失败时状态必须回到
//! 真实观察值，不允许「报告 enabled 但 blocker 未启动」。

use super::errors::{deny, HostResult, E_PLATFORM_UNSUPPORTED};

pub const KEEP_AWAKE_TYPE: &str = "prevent-app-suspension";

/// 平台能力：显式区分已实现/未实现（Linux 未实测）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeepAwakePlatform {
    Macos,
    Windows,
    Linux,
}

impl KeepAwakePlatform {
    pub fn from_str(raw: &str) -> Option<Self> {
        match raw {
            "darwin" | "macos" => Some(Self::Macos),
            "win32" | "windows" => Some(Self::Windows),
            "linux" => Some(Self::Linux),
            _ => None,
        }
    }

    /// Linux 现役无已验证实现：显式 unsupported（不是静默 no-op）。
    pub fn supported(self) -> bool {
        !matches!(self, Self::Linux)
    }
}

/// 实际 blocker 后端（外部系统）。mac/win 由宿主原生实现，测试注入替身。
pub trait KeepAwakeBackend {
    fn start(&mut self) -> Result<u64, String>;
    fn stop(&mut self, blocker_id: u64) -> Result<(), String>;
    fn is_started(&self, blocker_id: u64) -> Option<bool>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeepAwakeStatus {
    pub enabled: bool,
    pub active: bool,
    pub blocker_id: Option<u64>,
    pub kind: &'static str,
}

/// 与 Electron `createKeepAwakeManager` 语义一致的状态机。
#[derive(Debug, Default)]
pub struct KeepAwakeMachine {
    enabled: bool,
    blocker_id: Option<u64>,
}

impl KeepAwakeMachine {
    pub fn new() -> Self {
        Self::default()
    }

    fn active<B: KeepAwakeBackend>(&self, backend: &B) -> bool {
        let Some(blocker_id) = self.blocker_id else {
            return false;
        };
        // 后端查询失败视为未激活（fail-closed），不猜成激活。
        backend.is_started(blocker_id).unwrap_or(false)
    }

    pub fn status<B: KeepAwakeBackend>(&self, backend: &B) -> KeepAwakeStatus {
        KeepAwakeStatus {
            enabled: self.enabled,
            active: self.active(backend),
            blocker_id: self.blocker_id,
            kind: KEEP_AWAKE_TYPE,
        }
    }

    fn stop_current<B: KeepAwakeBackend>(&mut self, backend: &mut B) {
        if let Some(blocker_id) = self.blocker_id.take() {
            // 停止失败时也把本地句柄清空：现役实现同样清 empty，只记日志。
            let _ = backend.stop(blocker_id);
        }
    }

    /// 设置状态；失败时状态与错误一致（不会出现 enabled=true 但未启动）。
    pub fn set_enabled<B: KeepAwakeBackend>(
        &mut self,
        backend: &mut B,
        next_enabled: bool,
        platform: KeepAwakePlatform,
    ) -> HostResult<KeepAwakeStatus> {
        if next_enabled && !platform.supported() {
            return Err(deny(
                E_PLATFORM_UNSUPPORTED,
                "Linux 预览构建的保持唤醒尚无已验证的 systemd-inhibit/DBus 实现",
            ));
        }
        if !next_enabled {
            self.enabled = false;
            self.stop_current(backend);
            return Ok(self.status(backend));
        }
        if self.active(backend) {
            self.enabled = true;
            return Ok(self.status(backend));
        }
        self.stop_current(backend);
        self.enabled = false;
        let blocker_id = backend
            .start()
            .map_err(|error| deny(E_PLATFORM_UNSUPPORTED, format!("保存唤醒失败：{error}")))?;
        self.blocker_id = Some(blocker_id);
        self.enabled = true;
        Ok(self.status(backend))
    }

    /// will-quit 的 dispose：尽力停止，不让失败阻止退出。
    pub fn dispose<B: KeepAwakeBackend>(&mut self, backend: &mut B) {
        self.enabled = false;
        self.stop_current(backend);
    }
}

/// 设置窗/主窗都能改（设置页可能以主窗 modal 呈现）。
pub fn authorize_caller(caller: &str) -> HostResult<super::window_labels::WindowRole> {
    super::window_labels::require_role(
        caller,
        &[
            super::window_labels::WindowRole::Main,
            super::window_labels::WindowRole::Settings,
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::errors::E_PLATFORM_UNSUPPORTED;

    #[derive(Default)]
    struct FakeBackend {
        next_id: u64,
        started: Vec<u64>,
        start_failure: Option<String>,
        query_failure: bool,
        stop_calls: usize,
    }

    impl KeepAwakeBackend for FakeBackend {
        fn start(&mut self) -> Result<u64, String> {
            if let Some(error) = &self.start_failure {
                return Err(error.clone());
            }
            self.next_id += 1;
            self.started.push(self.next_id);
            Ok(self.next_id)
        }
        fn stop(&mut self, blocker_id: u64) -> Result<(), String> {
            self.stop_calls += 1;
            self.started.retain(|id| *id != blocker_id);
            Ok(())
        }
        fn is_started(&self, blocker_id: u64) -> Option<bool> {
            if self.query_failure {
                return None;
            }
            Some(self.started.contains(&blocker_id))
        }
    }

    #[test]
    fn enable_starts_blocker_and_is_idempotent() {
        let mut backend = FakeBackend::default();
        let mut machine = KeepAwakeMachine::new();
        let status = machine
            .set_enabled(&mut backend, true, KeepAwakePlatform::Macos)
            .unwrap();
        assert!(status.enabled);
        assert!(status.active);
        assert_eq!(status.blocker_id, Some(1));
        assert_eq!(status.kind, "prevent-app-suspension");
        // 重复 enable 不重复 start
        let again = machine
            .set_enabled(&mut backend, true, KeepAwakePlatform::Macos)
            .unwrap();
        assert_eq!(again.blocker_id, Some(1));
        assert_eq!(backend.started.len(), 1);
    }

    #[test]
    fn disable_stops_and_returns_real_status() {
        let mut backend = FakeBackend::default();
        let mut machine = KeepAwakeMachine::new();
        machine
            .set_enabled(&mut backend, true, KeepAwakePlatform::Windows)
            .unwrap();
        let status = machine
            .set_enabled(&mut backend, false, KeepAwakePlatform::Windows)
            .unwrap();
        assert!(!status.enabled);
        assert!(!status.active);
        assert_eq!(status.blocker_id, None);
        assert!(backend.started.is_empty());
        // 再次 disable 幂等
        let status = machine
            .set_enabled(&mut backend, false, KeepAwakePlatform::Windows)
            .unwrap();
        assert!(!status.enabled);
        assert_eq!(backend.stop_calls, 1, "无 blocker 时不应调用 stop");
    }

    #[test]
    fn start_failure_never_reports_enabled() {
        let mut backend = FakeBackend {
            start_failure: Some("assertion failed".to_string()),
            ..Default::default()
        };
        let mut machine = KeepAwakeMachine::new();
        let error = machine
            .set_enabled(&mut backend, true, KeepAwakePlatform::Macos)
            .unwrap_err();
        assert_eq!(error.code(), E_PLATFORM_UNSUPPORTED);
        assert!(!machine.status(&backend).enabled);
        assert!(!machine.status(&backend).active);
    }

    #[test]
    fn linux_is_explicitly_unsupported() {
        assert!(KeepAwakePlatform::Macos.supported());
        assert!(KeepAwakePlatform::Windows.supported());
        assert!(!KeepAwakePlatform::Linux.supported());
        let mut backend = FakeBackend::default();
        let mut machine = KeepAwakeMachine::new();
        assert_eq!(
            machine
                .set_enabled(&mut backend, true, KeepAwakePlatform::Linux)
                .unwrap_err()
                .code(),
            E_PLATFORM_UNSUPPORTED
        );
        // 关闭在 Linux 上是允许的（返回真实 unsupported 状态，不假装）
        let status = machine
            .set_enabled(&mut backend, false, KeepAwakePlatform::Linux)
            .unwrap();
        assert!(!status.enabled);
        assert_eq!(
            KeepAwakePlatform::from_str("linux"),
            Some(KeepAwakePlatform::Linux)
        );
        assert_eq!(KeepAwakePlatform::from_str("freebsd"), None);
    }

    #[test]
    fn backend_query_failure_is_fail_closed() {
        let mut backend = FakeBackend {
            query_failure: true,
            ..Default::default()
        };
        let mut machine = KeepAwakeMachine::new();
        machine
            .set_enabled(&mut backend, true, KeepAwakePlatform::Macos)
            .unwrap();
        let status = machine.status(&backend);
        assert!(status.enabled);
        assert!(!status.active, "查询失败不能猜成 active");
    }

    #[test]
    fn dispose_releases_blocker() {
        let mut backend = FakeBackend::default();
        let mut machine = KeepAwakeMachine::new();
        machine
            .set_enabled(&mut backend, true, KeepAwakePlatform::Macos)
            .unwrap();
        machine.dispose(&mut backend);
        assert!(!machine.status(&backend).enabled);
        assert!(backend.started.is_empty());
        assert_eq!(backend.stop_calls, 1);
    }

    #[test]
    fn keep_awake_caller_policy() {
        assert!(authorize_caller("main").is_ok());
        assert!(authorize_caller("settings").is_ok());
        assert_eq!(
            authorize_caller("quick-chat").unwrap_err().code(),
            crate::native::errors::E_WINDOW_NOT_ALLOWED
        );
    }
}
