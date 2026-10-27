//! R09-T04 系统权限（TCC）与 computer-use helper 租约（`host:speech-permission`
//! 的权限基础 + 9 叶中的 computer-use 授权/批准/租约）。
//!
//! 现役事实（R09 检查）：
//! - Electron 主进程没有 computer-use IPC；电脑操作偏好/批准/撤销/申请权限已由
//!   **Rust 服务**路由承载（`rust/crates/lingxi-service/src/computer_use_settings.rs`：
//!   GET/PUT `/lingxi/v1/preferences/computer-use`、POST/DELETE `.../approvals`、
//!   POST `.../request-permissions`），mac 侧用 CoreGraphics/ApplicationServices
//!   FFI 查询/请求 accessibility + screenRecording（`permissionSubject:"service_process"`）；
//! - 服务当前明确报告控制引擎不可用：`provider.status.available=false,
//!   reason="computer_control_engine_unavailable"`，且**没有**输入注入 helper；
//! - T04 任务书要求「computer use 所需 helper」「租约」「撤权后控制失败」
//!   「远端会话不得借本机应用权限」。Tauri 宿主侧新增本模块：TCC 查询/请求
//!   状态机 + 应用批准 + helper 租约（真实实现纯逻辑；系统探针通过 trait 注入，
//!   测试替身只替**外部系统响应**，不替宿主自己的授权判定）。
//!
//! 真实 TCC 撤销/授权需要人工操作（BLOCKED-SYSTEM-PERMISSIONS），证据生产者只跑
//! 可自动化的查询/负向分支并登记人工步骤。

use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use super::errors::{
    deny, HostResult, E_PLATFORM_UNSUPPORTED, E_REMOTE_DEVICE_DENIED, E_TOKEN_INVALID,
};
use super::window_labels::{require_role, WindowRole};

/// 需要的 TCC 权限类别（与服务 `system_permissions()` 的 name 对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PermissionKind {
    Accessibility,
    ScreenRecording,
}

impl PermissionKind {
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::Accessibility => "accessibility",
            Self::ScreenRecording => "screenRecording",
        }
    }
}

/// 六态权限归一（服务侧返回 granted 布尔；这里保留更细的状态以支持 UI）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionState {
    NotDetermined,
    Authorized,
    Denied,
    Restricted,
    /// 平台没有该 TCC（非 macOS）。
    Unsupported,
    /// 探针/桥不可用（fail-closed）。
    ProbeUnavailable,
}

impl PermissionState {
    /// 终结态：再次请求不重复弹窗（现役 speech 语义；computer-use 请求同理）。
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Authorized | Self::Denied | Self::Restricted | Self::Unsupported
        )
    }

    pub fn granted(self) -> bool {
        self == Self::Authorized
    }

    pub fn wire_name(self) -> &'static str {
        match self {
            Self::NotDetermined => "not_determined",
            Self::Authorized => "authorized",
            Self::Denied => "denied",
            Self::Restricted => "restricted",
            Self::Unsupported => "unsupported",
            Self::ProbeUnavailable => "probe_unavailable",
        }
    }
}

/// 外部系统 TCC 探针（macOS FFI / 服务进程）。测试可注入替身，
/// **宿主自己的授权判定不经测试替身绕过**。
pub trait SystemPermissionProbe {
    fn query(&self, kind: PermissionKind) -> PermissionState;
    /// 触发系统授权弹窗；返回值必须保持系统原样（弹窗不等于已授权）。
    fn request(&mut self, kind: PermissionKind) -> PermissionState;
}

/// 权限快照（供 UI/服务投影）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PermissionSnapshot {
    pub accessibility: PermissionState,
    pub screen_recording: PermissionState,
}

impl PermissionSnapshot {
    pub fn all_granted(self) -> bool {
        self.accessibility.granted() && self.screen_recording.granted()
    }
}

/// 查询（只读，不弹窗）。调用窗口：主窗/设置窗（computer-use 设置入口）。
pub fn query_permissions<P: SystemPermissionProbe>(
    caller: &str,
    platform: &str,
    probe: &P,
) -> HostResult<PermissionSnapshot> {
    require_role(caller, &[WindowRole::Main, WindowRole::Settings])?;
    if platform != "darwin" {
        return Ok(PermissionSnapshot {
            accessibility: PermissionState::Unsupported,
            screen_recording: PermissionState::Unsupported,
        });
    }
    Ok(PermissionSnapshot {
        accessibility: probe.query(PermissionKind::Accessibility),
        screen_recording: probe.query(PermissionKind::ScreenRecording),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestOutcome {
    pub before: PermissionState,
    pub after: PermissionState,
    /// 是否真正触发了系统弹窗（终结态不重复弹）。
    pub prompted: bool,
}

/// 用户手势触发的权限请求（现役语义：终结态不重复弹窗；非 mac 显式 unsupported）。
pub fn request_permission<P: SystemPermissionProbe>(
    caller: &str,
    platform: &str,
    probe: &mut P,
    kind: PermissionKind,
) -> HostResult<RequestOutcome> {
    require_role(caller, &[WindowRole::Main, WindowRole::Settings])?;
    if platform != "darwin" {
        return Ok(RequestOutcome {
            before: PermissionState::Unsupported,
            after: PermissionState::Unsupported,
            prompted: false,
        });
    }
    let before = probe.query(kind);
    if before.is_terminal() || before == PermissionState::ProbeUnavailable {
        return Ok(RequestOutcome {
            before,
            after: before,
            prompted: false,
        });
    }
    let after = probe.request(kind);
    Ok(RequestOutcome {
        before,
        after,
        prompted: true,
    })
}

// ── 应用批准与 helper 租约 ─────────────────────────────────────────────

/// 电脑操作动作集（T04 叶：get_app_state 给无障碍/截图；点击/输入/按键/滚动）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ComputerUseAction {
    GetAppState,
    Click,
    TypeText,
    KeyPress,
    Scroll,
}

impl ComputerUseAction {
    pub fn requires_accessibility(self) -> bool {
        // get_app_state 走 AX；输入注入同样需要 AX 信任。
        true
    }

    pub fn requires_screen_recording(self) -> bool {
        matches!(self, Self::GetAppState)
    }

    pub fn wire_name(self) -> &'static str {
        match self {
            Self::GetAppState => "get_app_state",
            Self::Click => "click",
            Self::TypeText => "type",
            Self::KeyPress => "key",
            Self::Scroll => "scroll",
        }
    }
}

/// 应用批准记录（与 Rust 服务 `computer_use.app_approvals` 同形）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppApproval {
    pub provider_id: String,
    pub app_id: String,
    pub app_name: String,
}

fn valid_id(value: &str, field: &'static str) -> HostResult<()> {
    if value.is_empty()
        || value.len() > 256
        || value.chars().any(|c| c.is_control())
        || value.contains('/')
        || value.contains('\\')
        || value.contains("..")
    {
        return Err(deny(E_INVALID_ARGUMENT_ID, format!("{field} 形态非法")));
    }
    Ok(())
}

use super::errors::E_INVALID_ARGUMENT as E_INVALID_ARGUMENT_ID;

/// helper 租约（进程内状态；服务侧另有偏好持久化）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperLease {
    pub lease_id: u64,
    pub provider_id: String,
    pub app_id: String,
    pub expires_at_ms: u64,
    /// 控件版本：应用重启/窗口重建后旧控件必须失效（叶断言「控件过期时拒绝」）。
    pub controls_generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaseRejection {
    /// 无 helper/租约未启动。
    NoHost,
    /// 应用未获批准。
    NotApproved,
    /// 租约过期。
    Expired,
    /// 控件过期/换代。
    StaleControls,
    /// 权限被撤销。
    PermissionRevoked,
    /// 远端会话借用本机权限。
    RemoteSession,
}

impl LeaseRejection {
    pub fn code(self) -> &'static str {
        match self {
            Self::NoHost => "COMPUTER_USE_NO_HOST",
            Self::NotApproved => "COMPUTER_USE_NOT_APPROVED",
            Self::Expired => "COMPUTER_USE_LEASE_EXPIRED",
            Self::StaleControls => "COMPUTER_USE_STALE_CONTROLS",
            Self::PermissionRevoked => "COMPUTER_USE_PERMISSION_REVOKED",
            Self::RemoteSession => E_REMOTE_DEVICE_DENIED,
        }
    }
}

/// 租约管理器（宿主进程内；真实状态机，不是替身）。
#[derive(Debug, Default)]
pub struct ComputerUseLeaseManager {
    approvals: Vec<AppApproval>,
    lease: Option<HelperLease>,
    next_lease: u64,
    controls_generation: u64,
}

pub const DEFAULT_LEASE_TTL_MS: u64 = 10 * 60 * 1000;
pub const MAX_LEASE_TTL_MS: u64 = 30 * 60 * 1000;

impl ComputerUseLeaseManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn approvals(&self) -> &[AppApproval] {
        &self.approvals
    }

    pub fn approve(&mut self, approval: AppApproval) -> HostResult<()> {
        valid_id(&approval.provider_id, "providerId")?;
        valid_id(&approval.app_id, "appId")?;
        if approval.app_name.len() > 512 || approval.app_name.chars().any(|c| c.is_control()) {
            return Err(deny(E_INVALID_ARGUMENT_ID, "appName 形态非法"));
        }
        self.approvals.retain(|item| {
            item.provider_id != approval.provider_id || item.app_id != approval.app_id
        });
        self.approvals.push(approval);
        Ok(())
    }

    pub fn revoke(&mut self, provider_id: &str, app_id: &str) -> HostResult<bool> {
        valid_id(provider_id, "providerId")?;
        valid_id(app_id, "appId")?;
        let before = self.approvals.len();
        // 撤权立即终止该应用的租约（叶：撤权后控制失败）。
        if self
            .lease
            .as_ref()
            .is_some_and(|lease| lease.provider_id == provider_id && lease.app_id == app_id)
        {
            self.lease = None;
        }
        self.approvals
            .retain(|item| item.provider_id != provider_id || item.app_id != app_id);
        Ok(self.approvals.len() != before)
    }

    fn approval_for(&self, provider_id: &str, app_id: &str) -> bool {
        self.approvals
            .iter()
            .any(|item| item.provider_id == provider_id && item.app_id == app_id)
    }

    /// 启动租约：必须真实获批 + TCC 已授权 + 非远端会话。
    pub fn start(
        &mut self,
        provider_id: &str,
        app_id: &str,
        permissions: PermissionSnapshot,
        remote_session: bool,
        now_ms: u64,
        ttl_ms: Option<u64>,
    ) -> Result<HelperLease, LeaseRejection> {
        if remote_session {
            return Err(LeaseRejection::RemoteSession);
        }
        if !self.approval_for(provider_id, app_id) {
            return Err(LeaseRejection::NotApproved);
        }
        if !permissions.accessibility.granted() || !permissions.screen_recording.granted() {
            return Err(LeaseRejection::PermissionRevoked);
        }
        let ttl = ttl_ms.unwrap_or(DEFAULT_LEASE_TTL_MS).min(MAX_LEASE_TTL_MS);
        self.controls_generation = self.controls_generation.wrapping_add(1);
        self.next_lease = self.next_lease.wrapping_add(1);
        let lease = HelperLease {
            lease_id: self.next_lease,
            provider_id: provider_id.to_owned(),
            app_id: app_id.to_owned(),
            expires_at_ms: now_ms.saturating_add(ttl),
            controls_generation: self.controls_generation,
        };
        self.lease = Some(lease.clone());
        Ok(lease)
    }

    pub fn active_lease(&self) -> Option<&HelperLease> {
        self.lease.as_ref()
    }

    /// 租约失效（应用退出/窗口重建/权限撤销）→ 后续动作全部拒绝。
    pub fn invalidate(&mut self, reason: LeaseRejection) -> LeaseRejection {
        if self.lease.take().is_some() {
            return reason;
        }
        reason
    }

    pub fn stop(&mut self, lease_id: u64) -> HostResult<bool> {
        match &self.lease {
            Some(lease) if lease.lease_id == lease_id => {
                self.lease = None;
                Ok(true)
            }
            Some(_) => Err(deny(E_TOKEN_INVALID, "租约 id 不属于当前租约")),
            None => Ok(false),
        }
    }

    /// 执行动作前的完整门（叶断言：无宿主/未批准/租约失效/控件过期时拒绝）。
    pub fn authorize_action(
        &self,
        lease_id: u64,
        action: ComputerUseAction,
        controls_generation: u64,
        permissions: PermissionSnapshot,
        now_ms: u64,
        remote_session: bool,
    ) -> Result<&HelperLease, LeaseRejection> {
        if remote_session {
            return Err(LeaseRejection::RemoteSession);
        }
        let Some(lease) = &self.lease else {
            return Err(LeaseRejection::NoHost);
        };
        if lease.lease_id != lease_id {
            return Err(LeaseRejection::NoHost);
        }
        if now_ms >= lease.expires_at_ms {
            return Err(LeaseRejection::Expired);
        }
        if controls_generation != lease.controls_generation
            || controls_generation != self.controls_generation
        {
            return Err(LeaseRejection::StaleControls);
        }
        if action.requires_accessibility() && !permissions.accessibility.granted() {
            return Err(LeaseRejection::PermissionRevoked);
        }
        if action.requires_screen_recording() && !permissions.screen_recording.granted() {
            return Err(LeaseRejection::PermissionRevoked);
        }
        if !self.approval_for(&lease.provider_id, &lease.app_id) {
            return Err(LeaseRejection::NotApproved);
        }
        Ok(lease)
    }

    /// 应用退出/崩溃后的控件换代（旧控件永久失效）。
    pub fn bump_controls_generation(&mut self) -> u64 {
        self.controls_generation = self.controls_generation.wrapping_add(1);
        if self
            .lease
            .as_ref()
            .is_some_and(|lease| lease.controls_generation != self.controls_generation)
        {
            self.lease = None;
        }
        self.controls_generation
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

/// `request-permissions` 路由语义的宿主侧投影：Linux Preview 显式拒绝。
pub fn platform_supports_computer_control(platform: &str) -> HostResult<()> {
    if platform == "darwin" || platform == "win32" {
        return Ok(());
    }
    Err(deny(
        E_PLATFORM_UNSUPPORTED,
        "Computer Use is not supported on Linux Preview.",
    ))
}

/// 租约/批准命令的调用窗口门：设置窗与主窗。
pub fn authorize_settings_caller(caller: &str) -> HostResult<WindowRole> {
    require_role(caller, &[WindowRole::Main, WindowRole::Settings])
}

// 供 tests 与 glue 复用的 HashSet 便捷导入（避免 dead_code 警告）
#[allow(dead_code)]
fn approvals_set(manager: &ComputerUseLeaseManager) -> HashSet<(String, String)> {
    manager
        .approvals()
        .iter()
        .map(|item| (item.provider_id.clone(), item.app_id.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::errors::{E_CONFLICT, E_PERMISSION_DENIED, E_WINDOW_NOT_ALLOWED};

    struct FakeProbe {
        accessibility: PermissionState,
        screen: PermissionState,
        prompt_result: Option<PermissionState>,
        prompt_calls: usize,
    }

    impl FakeProbe {
        fn granted() -> Self {
            Self {
                accessibility: PermissionState::Authorized,
                screen: PermissionState::Authorized,
                prompt_result: None,
                prompt_calls: 0,
            }
        }
        fn not_determined() -> Self {
            Self {
                accessibility: PermissionState::NotDetermined,
                screen: PermissionState::NotDetermined,
                prompt_result: Some(PermissionState::Authorized),
                prompt_calls: 0,
            }
        }
    }

    impl SystemPermissionProbe for FakeProbe {
        fn query(&self, kind: PermissionKind) -> PermissionState {
            match kind {
                PermissionKind::Accessibility => self.accessibility,
                PermissionKind::ScreenRecording => self.screen,
            }
        }
        fn request(&mut self, kind: PermissionKind) -> PermissionState {
            self.prompt_calls += 1;
            let result = self.prompt_result.unwrap_or(match kind {
                PermissionKind::Accessibility => self.accessibility,
                PermissionKind::ScreenRecording => self.screen,
            });
            match kind {
                PermissionKind::Accessibility => self.accessibility = result,
                PermissionKind::ScreenRecording => self.screen = result,
            }
            result
        }
    }

    fn snapshot(granted: bool) -> PermissionSnapshot {
        let state = if granted {
            PermissionState::Authorized
        } else {
            PermissionState::Denied
        };
        PermissionSnapshot {
            accessibility: state,
            screen_recording: state,
        }
    }

    #[test]
    fn query_is_read_only_and_window_gated() {
        let probe = FakeProbe::not_determined();
        let snapshot = query_permissions("main", "darwin", &probe).unwrap();
        assert_eq!(snapshot.accessibility, PermissionState::NotDetermined);
        assert_eq!(probe.prompt_calls, 0, "查询绝不弹窗");
        for caller in ["quick-chat", "viewer-1", "splash"] {
            assert_eq!(
                query_permissions(caller, "darwin", &probe)
                    .unwrap_err()
                    .code(),
                E_WINDOW_NOT_ALLOWED
            );
        }
        // 非 mac：显式 unsupported
        let snapshot = query_permissions("main", "linux", &probe).unwrap();
        assert_eq!(snapshot.accessibility, PermissionState::Unsupported);
        assert!(!snapshot.all_granted());
    }

    #[test]
    fn request_only_prompts_in_not_determined_and_user_gesture_window() {
        let mut probe = FakeProbe::not_determined();
        let outcome = request_permission(
            "settings",
            "darwin",
            &mut probe,
            PermissionKind::Accessibility,
        )
        .unwrap();
        assert!(outcome.prompted);
        assert_eq!(outcome.before, PermissionState::NotDetermined);
        assert_eq!(outcome.after, PermissionState::Authorized);
        assert_eq!(probe.prompt_calls, 1);

        // 终结态不重复弹窗
        let mut granted = FakeProbe::granted();
        let outcome = request_permission(
            "main",
            "darwin",
            &mut granted,
            PermissionKind::ScreenRecording,
        )
        .unwrap();
        assert!(!outcome.prompted);
        assert_eq!(granted.prompt_calls, 0);
        // denied 同样不重复弹
        let mut denied = FakeProbe::granted();
        denied.accessibility = PermissionState::Denied;
        let outcome =
            request_permission("main", "darwin", &mut denied, PermissionKind::Accessibility)
                .unwrap();
        assert!(!outcome.prompted);
        assert_eq!(outcome.after, PermissionState::Denied);
        // 非 mac 无 TCC
        let mut probe = FakeProbe::not_determined();
        let outcome =
            request_permission("main", "win32", &mut probe, PermissionKind::Accessibility).unwrap();
        assert!(!outcome.prompted);
        assert_eq!(outcome.after, PermissionState::Unsupported);
        assert_eq!(probe.prompt_calls, 0);
    }

    fn approval(provider: &str, app: &str) -> AppApproval {
        AppApproval {
            provider_id: provider.to_string(),
            app_id: app.to_string(),
            app_name: "TextEdit".to_string(),
        }
    }

    #[test]
    fn lease_requires_real_approval_and_permissions() {
        let mut manager = ComputerUseLeaseManager::new();
        assert_eq!(
            manager
                .start(
                    "macos:cua",
                    "com.apple.TextEdit",
                    snapshot(true),
                    false,
                    1_000,
                    None
                )
                .unwrap_err(),
            LeaseRejection::NotApproved
        );
        manager
            .approve(approval("macos:cua", "com.apple.TextEdit"))
            .unwrap();
        assert_eq!(
            manager
                .start(
                    "macos:cua",
                    "com.apple.TextEdit",
                    snapshot(false),
                    false,
                    1_000,
                    None
                )
                .unwrap_err(),
            LeaseRejection::PermissionRevoked
        );
        let lease = manager
            .start(
                "macos:cua",
                "com.apple.TextEdit",
                snapshot(true),
                false,
                1_000,
                None,
            )
            .unwrap();
        assert_eq!(manager.active_lease().unwrap().lease_id, lease.lease_id);
        assert!(lease.expires_at_ms > 1_000);
    }

    #[test]
    fn remote_sessions_can_never_start_leases() {
        let mut manager = ComputerUseLeaseManager::new();
        manager
            .approve(approval("macos:cua", "com.apple.TextEdit"))
            .unwrap();
        assert_eq!(
            manager
                .start(
                    "macos:cua",
                    "com.apple.TextEdit",
                    snapshot(true),
                    true,
                    1_000,
                    None
                )
                .unwrap_err(),
            LeaseRejection::RemoteSession
        );
        assert!(manager.active_lease().is_none());
        assert_eq!(
            LeaseRejection::RemoteSession.code(),
            crate::native::errors::E_REMOTE_DEVICE_DENIED
        );
    }

    #[test]
    fn actions_are_gated_by_lease_expiry_permissions_and_controls() {
        let mut manager = ComputerUseLeaseManager::new();
        manager
            .approve(approval("macos:cua", "com.apple.TextEdit"))
            .unwrap();
        let lease = manager
            .start(
                "macos:cua",
                "com.apple.TextEdit",
                snapshot(true),
                false,
                1_000,
                Some(1_000),
            )
            .unwrap();
        // 有效
        assert!(manager
            .authorize_action(
                lease.lease_id,
                ComputerUseAction::Click,
                lease.controls_generation,
                snapshot(true),
                1_500,
                false
            )
            .is_ok());
        // 过期
        assert_eq!(
            manager
                .authorize_action(
                    lease.lease_id,
                    ComputerUseAction::Click,
                    lease.controls_generation,
                    snapshot(true),
                    2_000,
                    false
                )
                .unwrap_err(),
            LeaseRejection::Expired
        );
        // 控件过期
        assert_eq!(
            manager
                .authorize_action(
                    lease.lease_id,
                    ComputerUseAction::Click,
                    lease.controls_generation + 1,
                    snapshot(true),
                    1_500,
                    false
                )
                .unwrap_err(),
            LeaseRejection::StaleControls
        );
        // 撤权
        assert_eq!(
            manager
                .authorize_action(
                    lease.lease_id,
                    ComputerUseAction::Scroll,
                    lease.controls_generation,
                    snapshot(false),
                    1_500,
                    false
                )
                .unwrap_err(),
            LeaseRejection::PermissionRevoked
        );
        // get_app_state 需要截屏权限
        let partial = PermissionSnapshot {
            accessibility: PermissionState::Authorized,
            screen_recording: PermissionState::Denied,
        };
        assert_eq!(
            manager
                .authorize_action(
                    lease.lease_id,
                    ComputerUseAction::GetAppState,
                    lease.controls_generation,
                    partial,
                    1_500,
                    false
                )
                .unwrap_err(),
            LeaseRejection::PermissionRevoked
        );
        // Click 只要求 accessibility
        assert!(manager
            .authorize_action(
                lease.lease_id,
                ComputerUseAction::Click,
                lease.controls_generation,
                partial,
                1_500,
                false
            )
            .is_ok());
        // 错误租约 id → 无宿主
        assert_eq!(
            manager
                .authorize_action(
                    lease.lease_id + 1,
                    ComputerUseAction::Click,
                    lease.controls_generation,
                    snapshot(true),
                    1_500,
                    false
                )
                .unwrap_err(),
            LeaseRejection::NoHost
        );
    }

    #[test]
    fn stop_revoke_and_app_exit_invalidate_leases() {
        let mut manager = ComputerUseLeaseManager::new();
        manager
            .approve(approval("macos:cua", "com.apple.TextEdit"))
            .unwrap();
        let lease = manager
            .start(
                "macos:cua",
                "com.apple.TextEdit",
                snapshot(true),
                false,
                0,
                None,
            )
            .unwrap();
        // 错误 id
        assert_eq!(
            manager.stop(lease.lease_id + 5).unwrap_err().code(),
            crate::native::errors::E_TOKEN_INVALID
        );
        assert!(manager.stop(lease.lease_id).unwrap());
        assert!(manager.active_lease().is_none());
        // 撤权终止租约
        let lease = manager
            .start(
                "macos:cua",
                "com.apple.TextEdit",
                snapshot(true),
                false,
                0,
                None,
            )
            .unwrap();
        assert!(manager.revoke("macos:cua", "com.apple.TextEdit").unwrap());
        assert!(manager.active_lease().is_none());
        assert_eq!(
            manager
                .authorize_action(
                    lease.lease_id,
                    ComputerUseAction::Click,
                    lease.controls_generation,
                    snapshot(true),
                    0,
                    false
                )
                .unwrap_err(),
            LeaseRejection::NoHost
        );
        // 应用退出换代
        manager
            .approve(approval("macos:cua", "com.apple.TextEdit"))
            .unwrap();
        let lease = manager
            .start(
                "macos:cua",
                "com.apple.TextEdit",
                snapshot(true),
                false,
                0,
                None,
            )
            .unwrap();
        let generation = manager.bump_controls_generation();
        assert_ne!(generation, lease.controls_generation);
        assert!(manager.active_lease().is_none());
    }

    #[test]
    fn approvals_are_validated_and_deduped() {
        let mut manager = ComputerUseLeaseManager::new();
        manager
            .approve(approval("macos:cua", "com.apple.TextEdit"))
            .unwrap();
        manager
            .approve(approval("macos:cua", "com.apple.TextEdit"))
            .unwrap();
        assert_eq!(manager.approvals().len(), 1, "同 provider+app 去重");
        for bad in ["", "a/b", "a\u{0}"] {
            assert_eq!(
                manager.approve(approval(bad, "app")).unwrap_err().code(),
                crate::native::errors::E_INVALID_ARGUMENT
            );
        }
        assert!(!manager.revoke("macos:cua", "other").unwrap());
        assert_eq!(approvals_set(&manager).len(), 1);
    }

    #[test]
    fn platform_and_caller_gates() {
        assert!(platform_supports_computer_control("darwin").is_ok());
        assert!(platform_supports_computer_control("win32").is_ok());
        assert_eq!(
            platform_supports_computer_control("linux")
                .unwrap_err()
                .code(),
            E_PLATFORM_UNSUPPORTED
        );
        assert!(authorize_settings_caller("main").is_ok());
        assert!(authorize_settings_caller("settings").is_ok());
        assert_eq!(
            authorize_settings_caller("quick-chat").unwrap_err().code(),
            E_WINDOW_NOT_ALLOWED
        );
    }

    #[test]
    fn state_and_rejection_codes_are_stable() {
        assert_eq!(PermissionState::Authorized.wire_name(), "authorized");
        assert_eq!(
            PermissionState::ProbeUnavailable.wire_name(),
            "probe_unavailable"
        );
        assert!(PermissionState::Authorized.is_terminal());
        assert!(PermissionState::Denied.is_terminal());
        assert!(!PermissionState::NotDetermined.is_terminal());
        assert_eq!(LeaseRejection::NoHost.code(), "COMPUTER_USE_NO_HOST");
        assert_eq!(
            LeaseRejection::NotApproved.code(),
            "COMPUTER_USE_NOT_APPROVED"
        );
        assert_eq!(
            PermissionKind::ScreenRecording.wire_name(),
            "screenRecording"
        );
        // 未使用但保留错误码常量引用，确保一处定义
        assert_eq!(E_PERMISSION_DENIED, "PERMISSION_DENIED");
        assert_eq!(
            crate::native::errors::E_PERMISSION_STATE,
            "PERMISSION_STATE"
        );
        assert_eq!(E_CONFLICT, "CONFLICT");
    }
}
