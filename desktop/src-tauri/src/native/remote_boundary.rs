//! R09-T04 远端设备访问边界（A08：远端任务不能自动读取本机设备）。
//!
//! 任务书通过条件：桌面连接远端服务且未授权本机采集时，远端请求麦克风/屏幕
//! 数据必须**拒绝或要求明确本机授权**，不能静默获取。T04 叶断言还要求
//! 「远端会话不得借本机应用权限」（computer-use helper 租约同样受此门约束）。
//!
//! 设计：
//! - 主体分类：本地用户、本地 agent、远端 agent、远端服务、未知；
//! - 设备类别：麦克风、屏幕采集、语音识别、辅助功能（输入注入）、摄像头；
//! - 授权（grant）**只能由本机用户手势在本地窗口发起**，远端请求永远不能
//!   自己创建/续期 grant；grant 有 TTL（≤15min）与设备/主体范围；
//! - 远端无有效 grant → `RequireLocalAuthorization`（拒绝并提示本机授权，
//!   A08 的「拒绝或要求明确本机授权」），未知主体 → `Deny`；
//! - 审计记录（谁请求、判定、是否本机授权）供证据使用。

use super::errors::E_REMOTE_DEVICE_DENIED;

pub const MAX_GRANT_TTL_MS: u64 = 15 * 60 * 1000;
pub const DEFAULT_GRANT_TTL_MS: u64 = 5 * 60 * 1000;

/// 请求主体分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrincipalClass {
    /// 本机人类用户（本地窗口 UI 手势）。
    LocalUser,
    /// 本机桌面 agent（本地进程内工具调用）。
    LocalAgent,
    /// 远端服务/远端 agent 发起的任务。
    RemoteAgent,
    /// 远端服务进程直连。
    RemoteService,
    Unknown,
}

impl PrincipalClass {
    pub fn is_remote(self) -> bool {
        matches!(self, Self::RemoteAgent | Self::RemoteService)
    }
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::LocalUser => "local_user",
            Self::LocalAgent => "local_agent",
            Self::RemoteAgent => "remote_agent",
            Self::RemoteService => "remote_service",
            Self::Unknown => "unknown",
        }
    }
}

/// 设备/敏感能力类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceKind {
    Microphone,
    ScreenCapture,
    SpeechRecognition,
    AccessibilityInput,
    Camera,
}

impl DeviceKind {
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::Microphone => "microphone",
            Self::ScreenCapture => "screen_capture",
            Self::SpeechRecognition => "speech_recognition",
            Self::AccessibilityInput => "accessibility_input",
            Self::Camera => "camera",
        }
    }
}

/// grant 的来源：只有本机用户手势可以创建/续期。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantSource {
    LocalUserGesture,
    /// 宿主策略预置（例如安装时明确勾选）；仍必须来自本地配置，不能由远端写入。
    LocalHostPolicy,
}

/// 设备授权记录（带主体/设备范围与过期时间）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceGrant {
    pub device: DeviceKind,
    /// 授权的远端主体标识；None = 与本机 agent/用户共享（本地会话）。
    pub subject: Option<String>,
    pub expires_at_ms: u64,
    pub source: GrantSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceDecision {
    /// 允许（本地用户/本地 agent 且宿主权限层允许）。
    Allow,
    /// 远端请求：必须由本机用户明确授权（A08 的提权路径），本次不采集。
    RequireLocalAuthorization,
    /// 明确拒绝（未知主体等）。
    Deny,
}

impl DeviceDecision {
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::RequireLocalAuthorization => "require_local_authorization",
            Self::Deny => "deny",
        }
    }

    pub fn permitted(self) -> bool {
        self == Self::Allow
    }
}

/// 审计记录（证据/诊断用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceAccessAudit {
    pub principal: PrincipalClass,
    pub device: DeviceKind,
    pub decision: DeviceDecision,
    /// 命中的 grant 过期时间（未命中为 None）。
    pub granted_until_ms: Option<u64>,
}

/// 远端边界策略（宿主进程内状态；真实授权判定，不是替身）。
#[derive(Debug, Default)]
pub struct RemoteDeviceBoundary {
    grants: Vec<DeviceGrant>,
}

impl RemoteDeviceBoundary {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn grants(&self) -> &[DeviceGrant] {
        &self.grants
    }

    fn find_grant(
        &self,
        device: DeviceKind,
        subject: Option<&str>,
        now_ms: u64,
    ) -> Option<&DeviceGrant> {
        self.grants.iter().find(|grant| {
            grant.device == device
                && grant.expires_at_ms > now_ms
                && (grant.subject.is_none() || grant.subject.as_deref() == subject)
        })
    }

    /// 本机用户手势创建/续期授权；远端主体不能创建。
    pub fn grant(
        &mut self,
        requested_by: PrincipalClass,
        source: GrantSource,
        device: DeviceKind,
        subject: Option<&str>,
        now_ms: u64,
        ttl_ms: Option<u64>,
    ) -> Result<DeviceGrant, &'static str> {
        if requested_by.is_remote() || requested_by == PrincipalClass::Unknown {
            return Err(E_REMOTE_DEVICE_DENIED);
        }
        if source == GrantSource::LocalHostPolicy && requested_by != PrincipalClass::LocalUser {
            return Err("LOCAL_POLICY_REQUIRES_LOCAL_USER");
        }
        let ttl = ttl_ms.unwrap_or(DEFAULT_GRANT_TTL_MS).min(MAX_GRANT_TTL_MS);
        let grant = DeviceGrant {
            device,
            subject: subject.map(str::to_owned),
            expires_at_ms: now_ms.saturating_add(ttl),
            source,
        };
        // 同设备/主体旧授权被替换（续期语义）。
        self.grants
            .retain(|item| item.device != grant.device || item.subject != grant.subject);
        self.grants.push(grant.clone());
        Ok(grant)
    }

    pub fn revoke(&mut self, device: DeviceKind, subject: Option<&str>) -> bool {
        let before = self.grants.len();
        self.grants
            .retain(|item| !(item.device == device && item.subject.as_deref() == subject));
        self.grants.len() != before
    }

    /// 判定 + 审计。`local_permission_granted` 是宿主 TCC 层（麦克风/屏幕）的
    /// 真实权限结果；远端边界与 TCC 是**与**关系（两者都过才允许）。
    pub fn evaluate(
        &self,
        principal: PrincipalClass,
        device: DeviceKind,
        subject: Option<&str>,
        now_ms: u64,
        local_permission_granted: bool,
    ) -> (DeviceDecision, DeviceAccessAudit) {
        let audit = |decision: DeviceDecision, granted_until_ms: Option<u64>| {
            (
                decision,
                DeviceAccessAudit {
                    principal,
                    device,
                    decision,
                    granted_until_ms,
                },
            )
        };
        match principal {
            PrincipalClass::Unknown => audit(DeviceDecision::Deny, None),
            PrincipalClass::LocalUser | PrincipalClass::LocalAgent => {
                if local_permission_granted {
                    audit(DeviceDecision::Allow, None)
                } else {
                    audit(DeviceDecision::Deny, None)
                }
            }
            PrincipalClass::RemoteAgent | PrincipalClass::RemoteService => {
                let grant = self.find_grant(device, subject, now_ms);
                match grant {
                    Some(grant) if local_permission_granted => {
                        audit(DeviceDecision::Allow, Some(grant.expires_at_ms))
                    }
                    Some(grant) => audit(DeviceDecision::Deny, Some(grant.expires_at_ms)),
                    None => audit(DeviceDecision::RequireLocalAuthorization, None),
                }
            }
        }
    }
}

/// 设备类别的宿主 TCC 映射（供 UI 提示需要哪类系统权限）。
pub fn tcc_kind_for_device(device: DeviceKind) -> Option<&'static str> {
    match device {
        DeviceKind::Microphone => Some("microphone"),
        DeviceKind::Camera => Some("camera"),
        DeviceKind::ScreenCapture => Some("screenRecording"),
        DeviceKind::SpeechRecognition => Some("speechRecognition"),
        DeviceKind::AccessibilityInput => Some("accessibility"),
    }
}

/// A08 的稳定错误：远端设备请求需要本机授权时返回该错误（不可静默回退）。
pub fn require_local_authorization_error(device: DeviceKind) -> super::errors::HostError {
    super::errors::deny(
        E_REMOTE_DEVICE_DENIED,
        format!(
            "远端任务读取本机设备（{}）需要本机用户明确授权",
            device.wire_name()
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_request_without_grant_requires_local_authorization() {
        let boundary = RemoteDeviceBoundary::new();
        let (decision, audit) = boundary.evaluate(
            PrincipalClass::RemoteAgent,
            DeviceKind::Microphone,
            Some("remote-run-1"),
            1_000,
            true,
        );
        assert_eq!(decision, DeviceDecision::RequireLocalAuthorization);
        assert!(!decision.permitted());
        assert_eq!(audit.decision, DeviceDecision::RequireLocalAuthorization);
        assert_eq!(audit.granted_until_ms, None);
        // 没有通过 = 不应有任何采集副作用（纯判定层保证）
        assert!(boundary.grants().is_empty());
        assert_eq!(
            require_local_authorization_error(DeviceKind::Microphone).code(),
            E_REMOTE_DEVICE_DENIED
        );
        assert_eq!(decision.wire_name(), "require_local_authorization");
    }

    #[test]
    fn remote_cannot_grant_itself() {
        let mut boundary = RemoteDeviceBoundary::new();
        for requester in [
            PrincipalClass::RemoteAgent,
            PrincipalClass::RemoteService,
            PrincipalClass::Unknown,
        ] {
            assert_eq!(
                boundary
                    .grant(
                        requester,
                        GrantSource::LocalUserGesture,
                        DeviceKind::ScreenCapture,
                        Some("remote"),
                        1_000,
                        None
                    )
                    .unwrap_err(),
                E_REMOTE_DEVICE_DENIED
            );
        }
        assert!(boundary.grants().is_empty());
        // 本地用户手势可以创建
        boundary
            .grant(
                PrincipalClass::LocalUser,
                GrantSource::LocalUserGesture,
                DeviceKind::Microphone,
                Some("remote-run-1"),
                1_000,
                None,
            )
            .unwrap();
        assert_eq!(boundary.grants().len(), 1);
        // local agent 不能代替用户写入 host policy
        assert_eq!(
            boundary
                .grant(
                    PrincipalClass::LocalAgent,
                    GrantSource::LocalHostPolicy,
                    DeviceKind::Microphone,
                    None,
                    1_000,
                    None,
                )
                .unwrap_err(),
            "LOCAL_POLICY_REQUIRES_LOCAL_USER"
        );
    }

    #[test]
    fn grant_scopes_device_subject_and_ttl() {
        let mut boundary = RemoteDeviceBoundary::new();
        let grant = boundary
            .grant(
                PrincipalClass::LocalUser,
                GrantSource::LocalUserGesture,
                DeviceKind::ScreenCapture,
                Some("remote-run-1"),
                1_000,
                Some(1_000_000),
            )
            .unwrap();
        // TTL 上限强制 15min
        assert_eq!(grant.expires_at_ms, 1_000 + MAX_GRANT_TTL_MS);
        // 同设备同主体：有效
        let (decision, audit) = boundary.evaluate(
            PrincipalClass::RemoteAgent,
            DeviceKind::ScreenCapture,
            Some("remote-run-1"),
            2_000,
            true,
        );
        assert_eq!(decision, DeviceDecision::Allow);
        assert_eq!(audit.granted_until_ms, Some(grant.expires_at_ms));
        // 其它设备不共享
        let (decision, _) = boundary.evaluate(
            PrincipalClass::RemoteAgent,
            DeviceKind::Microphone,
            Some("remote-run-1"),
            2_000,
            true,
        );
        assert_eq!(decision, DeviceDecision::RequireLocalAuthorization);
        // 其它远端主体不共享
        let (decision, _) = boundary.evaluate(
            PrincipalClass::RemoteAgent,
            DeviceKind::ScreenCapture,
            Some("remote-run-2"),
            2_000,
            true,
        );
        assert_eq!(decision, DeviceDecision::RequireLocalAuthorization);
    }

    #[test]
    fn expired_grant_does_not_allow_and_local_tcc_is_conjunctive() {
        let mut boundary = RemoteDeviceBoundary::new();
        let grant = boundary
            .grant(
                PrincipalClass::LocalUser,
                GrantSource::LocalUserGesture,
                DeviceKind::Microphone,
                Some("remote-run-1"),
                1_000,
                Some(1_000),
            )
            .unwrap();
        let (decision, _) = boundary.evaluate(
            PrincipalClass::RemoteAgent,
            DeviceKind::Microphone,
            Some("remote-run-1"),
            grant.expires_at_ms,
            true,
        );
        assert_eq!(
            decision,
            DeviceDecision::RequireLocalAuthorization,
            "过期 grant 必须重新请求本机授权"
        );
        // grant 有效但本机 TCC 未授权 → 拒绝
        let mut fresh = RemoteDeviceBoundary::new();
        fresh
            .grant(
                PrincipalClass::LocalUser,
                GrantSource::LocalUserGesture,
                DeviceKind::Microphone,
                Some("remote-run-1"),
                0,
                None,
            )
            .unwrap();
        let (decision, _) = fresh.evaluate(
            PrincipalClass::RemoteAgent,
            DeviceKind::Microphone,
            Some("remote-run-1"),
            100,
            false,
        );
        assert_eq!(decision, DeviceDecision::Deny);
    }

    #[test]
    fn local_principals_follow_host_permission_only() {
        let boundary = RemoteDeviceBoundary::new();
        for principal in [PrincipalClass::LocalUser, PrincipalClass::LocalAgent] {
            let (decision, _) = boundary.evaluate(principal, DeviceKind::Camera, None, 0, true);
            assert_eq!(decision, DeviceDecision::Allow);
            let (decision, _) = boundary.evaluate(principal, DeviceKind::Camera, None, 0, false);
            assert_eq!(decision, DeviceDecision::Deny);
        }
        let (decision, _) =
            boundary.evaluate(PrincipalClass::Unknown, DeviceKind::Camera, None, 0, true);
        assert_eq!(decision, DeviceDecision::Deny);
    }

    #[test]
    fn revoke_removes_grant_and_device_tcc_mapping_is_complete() {
        let mut boundary = RemoteDeviceBoundary::new();
        boundary
            .grant(
                PrincipalClass::LocalUser,
                GrantSource::LocalUserGesture,
                DeviceKind::AccessibilityInput,
                Some("remote-run-1"),
                0,
                None,
            )
            .unwrap();
        assert!(boundary.revoke(DeviceKind::AccessibilityInput, Some("remote-run-1")));
        assert!(!boundary.revoke(DeviceKind::AccessibilityInput, Some("remote-run-1")));
        let (decision, _) = boundary.evaluate(
            PrincipalClass::RemoteAgent,
            DeviceKind::AccessibilityInput,
            Some("remote-run-1"),
            1,
            true,
        );
        assert_eq!(decision, DeviceDecision::RequireLocalAuthorization);
        // 每个设备都有 TCC 映射
        for device in [
            DeviceKind::Microphone,
            DeviceKind::Camera,
            DeviceKind::ScreenCapture,
            DeviceKind::SpeechRecognition,
            DeviceKind::AccessibilityInput,
        ] {
            assert!(tcc_kind_for_device(device).is_some());
        }
    }
}
