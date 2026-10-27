//! R09-T04 语音识别权限与采集状态机（`host:speech-permission`）。
//!
//! 现役 Electron（desktop/speech-permissions.cjs 231 行 + main.cjs:5716-5723）：
//! - 六态：not_determined / authorized / denied / restricted / unsupported /
//!   bridge_unavailable；
//! - status 只读查询，**绝不**触发系统弹窗；
//! - request 仅由渲染层用户手势（按下录音键）发起；已终结状态直接回包不重复弹窗；
//! - 并发请求单飞合并；dispose 后在途请求按 bridge_unavailable 结算；
//! - `hostCanPrompt`：只有打包壳（Info.plist 带 NSSpeechRecognitionUsageDescription）
//!   才允许触发真实 TCC 请求；开发壳触发会被 macOS TCC SIGABRT 杀进程（实测），
//!   开发环境一律返回 not_determined 由前端走设置引导；
//! - 调用方必须是本壳窗口（现役 `BrowserWindow.fromWebContents`）。
//!
//! T04 任务书还要求「首次请求、拒绝、后续系统撤销、设备缺失和录制中取消，UI
//! 显示准确状态并停止相关资源」。本模块把权限状态机 + 采集会话状态机放在一起，
//! 纯逻辑可单测；真实 TCC 桥（SFSpeechRecognizer/AVCaptureDevice）与音频采集由
//! host-core/adapter 接线，真实授权/撤销需人工（BLOCKED-SYSTEM-PERMISSIONS）。

use super::errors::{
    deny, HostResult, E_PERMISSION_STATE, E_PLATFORM_UNSUPPORTED, E_WINDOW_NOT_ALLOWED,
};
use super::window_labels::{require_role, WindowRole};

pub const STATE_NOT_DETERMINED: &str = "not_determined";
pub const STATE_AUTHORIZED: &str = "authorized";
pub const STATE_DENIED: &str = "denied";
pub const STATE_RESTRICTED: &str = "restricted";
pub const STATE_UNSUPPORTED: &str = "unsupported";
pub const STATE_BRIDGE_UNAVAILABLE: &str = "bridge_unavailable";

/// 与现役 `SPEECH_PERMISSION_STATES` 完全一致的六态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeechPermissionState {
    NotDetermined,
    Authorized,
    Denied,
    Restricted,
    Unsupported,
    BridgeUnavailable,
}

impl SpeechPermissionState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotDetermined => STATE_NOT_DETERMINED,
            Self::Authorized => STATE_AUTHORIZED,
            Self::Denied => STATE_DENIED,
            Self::Restricted => STATE_RESTRICTED,
            Self::Unsupported => STATE_UNSUPPORTED,
            Self::BridgeUnavailable => STATE_BRIDGE_UNAVAILABLE,
        }
    }

    /// 已终结状态：不得重复触发系统授权弹窗。
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Authorized | Self::Denied | Self::Restricted)
    }

    pub fn authorized(self) -> bool {
        self == Self::Authorized
    }

    /// SFSpeechRecognizerAuthorizationStatus rawValue → 六态（现役 RAW_STATE_MAP）。
    pub fn from_raw_status(raw: i64) -> Self {
        match raw {
            0 => Self::NotDetermined,
            1 => Self::Denied,
            2 => Self::Restricted,
            3 => Self::Authorized,
            _ => Self::BridgeUnavailable,
        }
    }
}

/// 外部系统 TCC 桥（SFSpeechRecognizer）。`None` = 桥不可用（fail-closed）。
pub trait SpeechPermissionBackend {
    fn authorization_status(&self) -> Option<i64>;
    /// 发起系统授权请求；返回最终 raw status。错误返回 None（bridge_unavailable）。
    fn request_authorization(&mut self) -> Option<i64>;
}

/// 权限控制器（单飞/终结态/开发壳不弹窗/dispose 语义）。
#[derive(Debug, Default)]
pub struct SpeechPermissionController {
    disposed: bool,
    pending: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestDecision {
    /// 已终结：直接返回状态，不弹窗。
    Terminal(SpeechPermissionState),
    /// 开发壳不能弹真实 TCC：返回 not_determined（前端走设置引导）。
    HostCannotPrompt,
    /// 已有在途请求：单飞合并（调用方等待同一结果）。
    JoinPending,
    /// 允许发起系统授权请求。
    Prompt,
    /// 桥不可用/平台不支持：fail-closed。
    Unavailable(SpeechPermissionState),
}

impl SpeechPermissionController {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn dispose(&mut self) {
        self.disposed = true;
        self.pending = false;
    }

    pub fn is_disposed(&self) -> bool {
        self.disposed
    }

    /// 只读状态：绝不请求系统授权。
    pub fn status<B: SpeechPermissionBackend>(
        &self,
        platform: &str,
        backend: &B,
    ) -> SpeechPermissionState {
        if self.disposed || platform != "darwin" {
            return SpeechPermissionState::Unsupported;
        }
        match backend.authorization_status() {
            Some(raw) => SpeechPermissionState::from_raw_status(raw),
            None => SpeechPermissionState::BridgeUnavailable,
        }
    }

    /// request 入口决策（用户手势路径）。
    pub fn begin_request<B: SpeechPermissionBackend>(
        &mut self,
        platform: &str,
        host_can_prompt: bool,
        backend: &B,
    ) -> RequestDecision {
        if self.disposed {
            return RequestDecision::Unavailable(SpeechPermissionState::BridgeUnavailable);
        }
        if platform != "darwin" {
            return RequestDecision::Unavailable(SpeechPermissionState::Unsupported);
        }
        let state = self.status(platform, backend);
        if state.is_terminal() {
            return RequestDecision::Terminal(state);
        }
        if state != SpeechPermissionState::NotDetermined {
            return RequestDecision::Unavailable(state);
        }
        if !host_can_prompt {
            // 开发壳缺 usage description 时触发 TCC 会被 SIGABRT 杀掉；
            // 一律不请求，返回 not_determined 由 UI 引导去系统设置。
            return RequestDecision::HostCannotPrompt;
        }
        if self.pending {
            return RequestDecision::JoinPending;
        }
        self.pending = true;
        RequestDecision::Prompt
    }

    /// 系统授权请求返回后结算（晚于 dispose 的回调必须丢弃）。
    pub fn finish_request(&mut self, raw_status: Option<i64>) -> SpeechPermissionState {
        self.pending = false;
        if self.disposed {
            return SpeechPermissionState::BridgeUnavailable;
        }
        match raw_status {
            Some(raw) => SpeechPermissionState::from_raw_status(raw),
            None => SpeechPermissionState::BridgeUnavailable,
        }
    }

    /// 单飞中（供测试/诊断）。
    pub fn is_pending(&self) -> bool {
        self.pending
    }
}

// ── 采集会话状态机（设备缺失 / 录制中撤销 / 取消 / 超时）────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureState {
    Idle,
    Starting,
    Recording,
    Stopping,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureError {
    PermissionDenied,
    PermissionRevoked,
    DeviceMissing,
    Timeout,
    Cancelled,
    BridgeUnavailable,
}

impl CaptureError {
    pub fn code(self) -> &'static str {
        match self {
            Self::PermissionDenied => "SPEECH_PERMISSION_DENIED",
            Self::PermissionRevoked => "SPEECH_PERMISSION_REVOKED",
            Self::DeviceMissing => "SPEECH_DEVICE_MISSING",
            Self::Timeout => "SPEECH_TIMEOUT",
            Self::Cancelled => "SPEECH_CANCELLED",
            Self::BridgeUnavailable => "SPEECH_BRIDGE_UNAVAILABLE",
        }
    }
}

/// 采集会话（真实资源生命周期由 adapter 执行；这里是其状态机与错误语义）。
#[derive(Debug, Default)]
pub struct CaptureSession {
    state: CaptureState,
}

impl Default for CaptureState {
    fn default() -> Self {
        Self::Idle
    }
}

impl CaptureSession {
    pub fn new() -> Self {
        Self {
            state: CaptureState::Idle,
        }
    }

    pub fn state(&self) -> CaptureState {
        self.state
    }

    /// 开始采集：权限/设备检查必须先于资源分配；失败不得进入 Recording。
    pub fn start(
        &mut self,
        permission: SpeechPermissionState,
        device_available: bool,
    ) -> Result<(), CaptureError> {
        if self.state != CaptureState::Idle {
            return Err(CaptureError::Cancelled);
        }
        if !permission.authorized() {
            self.state = CaptureState::Failed;
            return Err(match permission {
                SpeechPermissionState::Denied | SpeechPermissionState::Restricted => {
                    CaptureError::PermissionDenied
                }
                SpeechPermissionState::BridgeUnavailable | SpeechPermissionState::Unsupported => {
                    CaptureError::BridgeUnavailable
                }
                _ => CaptureError::PermissionDenied,
            });
        }
        if !device_available {
            self.state = CaptureState::Failed;
            return Err(CaptureError::DeviceMissing);
        }
        self.state = CaptureState::Recording;
        Ok(())
    }

    /// 系统权限在录制中被撤销：必须立即停止并报准确错误（A07 核心）。
    pub fn on_permission_revoked(&mut self) -> Result<(), CaptureError> {
        if self.state == CaptureState::Recording {
            self.state = CaptureState::Failed;
            return Err(CaptureError::PermissionRevoked);
        }
        Ok(())
    }

    /// 用户在录制中取消：取消不是成功，不得返回识别结果。
    pub fn cancel(&mut self) {
        if matches!(
            self.state,
            CaptureState::Recording | CaptureState::Starting | CaptureState::Stopping
        ) {
            self.state = CaptureState::Cancelled;
        }
    }

    pub fn on_timeout(&mut self) -> Result<(), CaptureError> {
        if matches!(self.state, CaptureState::Recording | CaptureState::Starting) {
            self.state = CaptureState::Failed;
            return Err(CaptureError::Timeout);
        }
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), CaptureError> {
        if self.state != CaptureState::Recording {
            return Err(CaptureError::Cancelled);
        }
        self.state = CaptureState::Stopping;
        Ok(())
    }

    /// 识别结果结算：只有正常 Stopping 后的结果算成功；
    /// 取消/超时/失败一律不能记成功（叶断言）。
    pub fn finish(&mut self, recognized: Option<&str>) -> Result<String, CaptureError> {
        match self.state {
            CaptureState::Stopping => {
                let Some(text) = recognized.filter(|text| !text.trim().is_empty()) else {
                    self.state = CaptureState::Failed;
                    return Err(CaptureError::Timeout);
                };
                self.state = CaptureState::Idle;
                Ok(text.to_owned())
            }
            CaptureState::Cancelled => Err(CaptureError::Cancelled),
            CaptureState::Failed => Err(CaptureError::PermissionRevoked),
            _ => Err(CaptureError::Cancelled),
        }
    }

    /// 资源释放（幂等；任何状态都可回 Idle）。
    pub fn release(&mut self) {
        self.state = CaptureState::Idle;
    }
}

/// T04 权限 UI 状态（首次/拒绝/撤销/设备缺失/录制中取消）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeechUiState {
    /// 首次使用：需要用户手势触发系统授权。
    FirstUse,
    /// 已授权，可录音。
    Ready,
    /// 已拒绝：显示去系统设置的引导。
    Denied,
    /// 受限（家长控制/MDM）。
    Restricted,
    /// 录制中权限被撤销：采集已停止并显示错误。
    RevokedDuringRecording,
    /// 设备缺失（无麦克风/识别器不可用）。
    DeviceMissing,
    /// 录制中用户取消。
    CancelledDuringRecording,
    /// 平台/桥不支持，显式降级文案。
    Unsupported,
}

pub fn ui_state(
    permission: SpeechPermissionState,
    capture: CaptureState,
    device_available: bool,
    last_error: Option<CaptureError>,
) -> SpeechUiState {
    match last_error {
        Some(CaptureError::PermissionRevoked) => return SpeechUiState::RevokedDuringRecording,
        Some(CaptureError::DeviceMissing) => return SpeechUiState::DeviceMissing,
        Some(CaptureError::Cancelled) => return SpeechUiState::CancelledDuringRecording,
        _ => {}
    }
    if !device_available && permission.authorized() {
        return SpeechUiState::DeviceMissing;
    }
    match permission {
        SpeechPermissionState::NotDetermined => SpeechUiState::FirstUse,
        SpeechPermissionState::Authorized => {
            if capture == CaptureState::Idle {
                SpeechUiState::Ready
            } else {
                SpeechUiState::Ready
            }
        }
        SpeechPermissionState::Denied => SpeechUiState::Denied,
        SpeechPermissionState::Restricted => SpeechUiState::Restricted,
        SpeechPermissionState::Unsupported | SpeechPermissionState::BridgeUnavailable => {
            SpeechUiState::Unsupported
        }
    }
}

/// 命令调用窗口门：拥有录音 UI 的主窗（语音输入在主聊天输入区）。
pub fn authorize_speech_caller(caller: &str) -> HostResult<()> {
    require_role(caller, &[WindowRole::Main]).map(|_| ())
}

pub fn unsupported_platform_error(platform: &str) -> super::errors::HostError {
    deny(
        E_PLATFORM_UNSUPPORTED,
        format!("平台 {platform} 没有系统语音识别 TCC"),
    )
}

pub fn state_error(state: SpeechPermissionState) -> super::errors::HostError {
    deny(
        E_PERMISSION_STATE,
        format!("语音权限状态不允许该操作: {}", state.as_str()),
    )
}

pub fn caller_denied() -> super::errors::HostError {
    deny(E_WINDOW_NOT_ALLOWED, "语音权限命令只允许主窗口调用")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::errors::{E_PLATFORM_UNSUPPORTED, E_WINDOW_NOT_ALLOWED};

    struct FakeBackend {
        status: Option<i64>,
        request_result: Option<i64>,
        request_calls: usize,
    }

    impl SpeechPermissionBackend for FakeBackend {
        fn authorization_status(&self) -> Option<i64> {
            self.status
        }
        fn request_authorization(&mut self) -> Option<i64> {
            self.request_calls += 1;
            self.request_result
        }
    }

    #[test]
    fn raw_status_mapping_matches_electron() {
        assert_eq!(
            SpeechPermissionState::from_raw_status(0),
            SpeechPermissionState::NotDetermined
        );
        assert_eq!(
            SpeechPermissionState::from_raw_status(1),
            SpeechPermissionState::Denied
        );
        assert_eq!(
            SpeechPermissionState::from_raw_status(2),
            SpeechPermissionState::Restricted
        );
        assert_eq!(
            SpeechPermissionState::from_raw_status(3),
            SpeechPermissionState::Authorized
        );
        assert_eq!(
            SpeechPermissionState::from_raw_status(99),
            SpeechPermissionState::BridgeUnavailable
        );
        assert_eq!(SpeechPermissionState::Denied.as_str(), "denied");
        assert_eq!(STATE_BRIDGE_UNAVAILABLE, "bridge_unavailable");
    }

    #[test]
    fn status_is_read_only_and_platform_gated() {
        let controller = SpeechPermissionController::new();
        let backend = FakeBackend {
            status: Some(3),
            request_result: None,
            request_calls: 0,
        };
        assert_eq!(
            controller.status("darwin", &backend),
            SpeechPermissionState::Authorized
        );
        assert_eq!(backend.request_calls, 0, "status 不触发系统弹窗");
        assert_eq!(
            controller.status("linux", &backend),
            SpeechPermissionState::Unsupported
        );
        let unavailable = FakeBackend {
            status: None,
            request_result: None,
            request_calls: 0,
        };
        assert_eq!(
            controller.status("darwin", &unavailable),
            SpeechPermissionState::BridgeUnavailable
        );
    }

    #[test]
    fn request_policy_first_terminal_devshell_and_single_flight() {
        // 首次 + 可弹 → Prompt
        let backend = FakeBackend {
            status: Some(0),
            request_result: None,
            request_calls: 0,
        };
        let mut controller = SpeechPermissionController::new();
        assert_eq!(
            controller.begin_request("darwin", true, &backend),
            RequestDecision::Prompt
        );
        assert!(controller.is_pending());
        // 单飞
        assert_eq!(
            controller.begin_request("darwin", true, &backend),
            RequestDecision::JoinPending
        );
        // 结算
        assert_eq!(
            controller.finish_request(Some(3)),
            SpeechPermissionState::Authorized
        );
        assert!(!controller.is_pending());
        // 终结态不再弹（真实系统中 status 已变为 authorized）
        let authorized_backend = FakeBackend {
            status: Some(3),
            request_result: None,
            request_calls: 0,
        };
        assert_eq!(
            controller.begin_request("darwin", true, &authorized_backend),
            RequestDecision::Terminal(SpeechPermissionState::Authorized)
        );
        // 开发壳：not_determined + hostCanPrompt=false → 不弹
        let fresh = FakeBackend {
            status: Some(0),
            request_result: None,
            request_calls: 0,
        };
        let mut controller = SpeechPermissionController::new();
        assert_eq!(
            controller.begin_request("darwin", false, &fresh),
            RequestDecision::HostCannotPrompt
        );
        // 非 mac
        assert_eq!(
            controller.begin_request("win32", true, &fresh),
            RequestDecision::Unavailable(SpeechPermissionState::Unsupported)
        );
    }

    #[test]
    fn dispose_settles_as_bridge_unavailable_and_late_callbacks_are_dropped() {
        let backend = FakeBackend {
            status: Some(0),
            request_result: Some(3),
            request_calls: 0,
        };
        let mut controller = SpeechPermissionController::new();
        assert_eq!(
            controller.begin_request("darwin", true, &backend),
            RequestDecision::Prompt
        );
        controller.dispose();
        assert_eq!(
            controller.finish_request(Some(3)),
            SpeechPermissionState::BridgeUnavailable,
            "dispose 后晚到的原生回调必须按 bridge_unavailable 结算"
        );
        assert_eq!(
            controller.begin_request("darwin", true, &backend),
            RequestDecision::Unavailable(SpeechPermissionState::BridgeUnavailable)
        );
    }

    #[test]
    fn capture_requires_authorized_permission_and_device() {
        let mut session = CaptureSession::new();
        assert_eq!(
            session.start(SpeechPermissionState::NotDetermined, true),
            Err(CaptureError::PermissionDenied)
        );
        assert_eq!(session.state(), CaptureState::Failed);
        session.release();
        assert_eq!(
            session.start(SpeechPermissionState::Authorized, false),
            Err(CaptureError::DeviceMissing)
        );
        session.release();
        assert_eq!(
            session.start(SpeechPermissionState::BridgeUnavailable, true),
            Err(CaptureError::BridgeUnavailable)
        );
        session.release();
        assert!(session
            .start(SpeechPermissionState::Authorized, true)
            .is_ok());
        assert_eq!(session.state(), CaptureState::Recording);
    }

    #[test]
    fn revocation_during_recording_stops_and_reports() {
        let mut session = CaptureSession::new();
        session
            .start(SpeechPermissionState::Authorized, true)
            .unwrap();
        assert_eq!(
            session.on_permission_revoked(),
            Err(CaptureError::PermissionRevoked)
        );
        assert_eq!(session.state(), CaptureState::Failed);
        assert_eq!(
            session.finish(Some("不该出现的识别结果")),
            Err(CaptureError::PermissionRevoked),
            "撤权后不得返回成功识别"
        );
        assert_eq!(
            ui_state(
                SpeechPermissionState::Authorized,
                session.state(),
                true,
                Some(CaptureError::PermissionRevoked)
            ),
            SpeechUiState::RevokedDuringRecording
        );
        assert_eq!(
            CaptureError::PermissionRevoked.code(),
            "SPEECH_PERMISSION_REVOKED"
        );
    }

    #[test]
    fn cancel_and_timeout_never_count_as_success() {
        let mut cancelled = CaptureSession::new();
        cancelled
            .start(SpeechPermissionState::Authorized, true)
            .unwrap();
        cancelled.cancel();
        assert_eq!(cancelled.state(), CaptureState::Cancelled);
        assert_eq!(
            cancelled.finish(Some("hello")),
            Err(CaptureError::Cancelled)
        );

        let mut timed_out = CaptureSession::new();
        timed_out
            .start(SpeechPermissionState::Authorized, true)
            .unwrap();
        assert_eq!(timed_out.on_timeout(), Err(CaptureError::Timeout));
        assert_eq!(
            timed_out.finish(Some("hello")),
            Err(CaptureError::PermissionRevoked),
            "超时后的结果不能记成功"
        );

        let mut completed = CaptureSession::new();
        completed
            .start(SpeechPermissionState::Authorized, true)
            .unwrap();
        completed.stop().unwrap();
        assert_eq!(completed.state(), CaptureState::Stopping);
        assert_eq!(completed.finish(Some("你好")).unwrap(), "你好");
        assert_eq!(completed.state(), CaptureState::Idle);
        // 空识别结果不算成功
        let mut empty = CaptureSession::new();
        empty
            .start(SpeechPermissionState::Authorized, true)
            .unwrap();
        empty.stop().unwrap();
        assert_eq!(empty.finish(Some("   ")), Err(CaptureError::Timeout));
    }

    #[test]
    fn ui_states_cover_first_denied_restricted_missing_cancel() {
        assert_eq!(
            ui_state(
                SpeechPermissionState::NotDetermined,
                CaptureState::Idle,
                true,
                None
            ),
            SpeechUiState::FirstUse
        );
        assert_eq!(
            ui_state(
                SpeechPermissionState::Authorized,
                CaptureState::Idle,
                true,
                None
            ),
            SpeechUiState::Ready
        );
        assert_eq!(
            ui_state(
                SpeechPermissionState::Denied,
                CaptureState::Idle,
                true,
                None
            ),
            SpeechUiState::Denied
        );
        assert_eq!(
            ui_state(
                SpeechPermissionState::Restricted,
                CaptureState::Idle,
                true,
                None
            ),
            SpeechUiState::Restricted
        );
        assert_eq!(
            ui_state(
                SpeechPermissionState::Authorized,
                CaptureState::Idle,
                false,
                None
            ),
            SpeechUiState::DeviceMissing
        );
        assert_eq!(
            ui_state(
                SpeechPermissionState::Authorized,
                CaptureState::Recording,
                true,
                Some(CaptureError::Cancelled)
            ),
            SpeechUiState::CancelledDuringRecording
        );
        assert_eq!(
            ui_state(
                SpeechPermissionState::BridgeUnavailable,
                CaptureState::Idle,
                true,
                None
            ),
            SpeechUiState::Unsupported
        );
    }

    #[test]
    fn caller_and_platform_errors() {
        assert!(authorize_speech_caller("main").is_ok());
        for caller in ["settings", "quick-chat", "viewer-1"] {
            assert_eq!(
                authorize_speech_caller(caller).unwrap_err().code(),
                E_WINDOW_NOT_ALLOWED
            );
        }
        assert_eq!(
            unsupported_platform_error("linux").code(),
            E_PLATFORM_UNSUPPORTED
        );
        assert_eq!(
            state_error(SpeechPermissionState::Denied).code(),
            E_PERMISSION_STATE
        );
        assert_eq!(caller_denied().code(), E_WINDOW_NOT_ALLOWED);
    }
}
