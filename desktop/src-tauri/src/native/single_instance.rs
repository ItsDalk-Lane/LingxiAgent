//! R09-T03 单实例语义（`host:single-instance`）。
//!
//! 现役 Electron：`requestSingleInstanceLock({lingxiHome})`（userData 已按
//! LINGXI_HOME 命名空间化）→ 第二实例 `exit(0)`，第一实例收到 second-instance
//! 后 `showPrimaryWindow()`（显示/聚焦主窗或 onboarding）。Tauri 的
//! tauri-plugin-single-instance 默认按 app identifier 收敛，不天然按 home 隔离；
//! 任务书要求显式验证「不同 LINGXI_HOME 允许并行、同一 home 第二实例退出码 0」。
//!
//! 本模块只做纯判定：锁命名空间（供 host-core 在 setup 决定是否启用插件 /
//! 自建 home 锁）、第二实例动作计划、renderer 不得提供 home。证据生产者用
//! `lock_namespace` 断言隔离语义，用真实双进程探针验证注册后的行为。

use super::errors::{deny, HostResult, E_INVALID_ARGUMENT, E_PATH_NOT_ABSOLUTE};

/// 第二实例退出码：现役 Electron `app.exit(0)`；Tauri 进程正常退出同为 0。
pub const SECOND_INSTANCE_EXIT_CODE: i32 = 0;

/// Windows 登录启动参数（与 Electron `login-item-settings.cjs` 保持同名同义）。
pub const START_AT_LOGIN_ARG: &str = "--hana-start-at-login";

/// 应用标识的兜底前缀（与 tauri.conf identifier 分开，避免把 identifier 当锁键）。
const LOCK_KEY_PREFIX: &str = "lingxi-desktop";

/// 稳定 64 位 FNV-1a：仅用于日志/锁命名空间的可读键，不作为安全承诺。
pub fn fnv1a64_hex(input: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// 校验 `LINGXI_HOME` 形态：绝对路径、非空、无 NUL/控制字符、长度受控。
/// renderer 永远不能提供 home（由 Rust 进程环境/命令行决定）。
pub fn validate_home(raw: &str, source: &'static str) -> HostResult<()> {
    if raw.is_empty() {
        return Err(deny(E_INVALID_ARGUMENT, format!("{source} 不能为空")));
    }
    if raw.len() > 4096 {
        return Err(deny(
            E_INVALID_ARGUMENT,
            format!("{source} 超过路径长度上限"),
        ));
    }
    if raw.chars().any(|c| c.is_control()) {
        return Err(deny(E_INVALID_ARGUMENT, format!("{source} 含非法控制字符")));
    }
    if !std::path::Path::new(raw).is_absolute() {
        return Err(deny(
            E_PATH_NOT_ABSOLUTE,
            format!("{source} 必须是绝对路径"),
        ));
    }
    Ok(())
}

/// 单实例锁命名空间。同一 home → 同键（第二实例让位）；不同 home → 不同键
/// （隔离目录可并行，START_RECEIPT §7 的验收语义）。
///
/// `canonical_home` 必须由调用方已 `canonicalize`（符号链接/相对段归一），
/// 本函数不做 IO，保持可单测。
pub fn lock_namespace(app_identifier: &str, canonical_home: &str) -> HostResult<String> {
    if app_identifier.trim().is_empty() {
        return Err(deny(E_INVALID_ARGUMENT, "app identifier 不能为空"));
    }
    validate_home(canonical_home, "LINGXI_HOME")?;
    Ok(format!(
        "{LOCK_KEY_PREFIX}:{app_identifier}:{}",
        fnv1a64_hex(canonical_home)
    ))
}

/// 第二实例动作（第一个实例的 second-instance 回调应该做什么）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecondInstanceAction {
    /// 主窗存在：显示/取消最小化/聚焦。
    ShowMain,
    /// 只有引导窗：聚焦引导窗（不新开主窗）。
    ShowOnboarding,
    /// 没有任何窗口（启动中/异常）：仅激活 app，不抢跑创建。
    ActivateOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecondInstancePlan {
    pub action: SecondInstanceAction,
    pub exit_code: i32,
}

/// 现役 `showPrimaryWindow()`：`mainWindow || onboardingWindow` 二选一聚焦。
pub fn plan_second_instance(has_main: bool, has_onboarding: bool) -> SecondInstancePlan {
    let action = if has_main {
        SecondInstanceAction::ShowMain
    } else if has_onboarding {
        SecondInstanceAction::ShowOnboarding
    } else {
        SecondInstanceAction::ActivateOnly
    };
    SecondInstancePlan {
        action,
        exit_code: SECOND_INSTANCE_EXIT_CODE,
    }
}

/// 命令行/环境里不允许出现 renderer 可注入的 home 覆盖：
/// 只接受宿主自己解析出的绝对 home；任何 `LINGXI_HOME` 相对路径都拒绝。
pub fn reject_renderer_home_override(renderer_supplied_home: Option<&str>) -> HostResult<()> {
    match renderer_supplied_home {
        None => Ok(()),
        Some(value) => {
            validate_home(value, "renderer LINGXI_HOME")?;
            Err(deny(
                E_INVALID_ARGUMENT,
                "renderer 不允许指定 LINGXI_HOME（单实例锁与数据目录只能由宿主决定）",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::errors::{E_INVALID_ARGUMENT, E_PATH_NOT_ABSOLUTE};

    #[test]
    fn lock_namespace_is_home_scoped() {
        let production = lock_namespace("com.lingxi.app", "/Users/u/.lingxi").unwrap();
        let dev = lock_namespace("com.lingxi.app", "/Users/u/.lingxi-dev").unwrap();
        let copy = lock_namespace("com.lingxi.app", "/private/tmp/lingxi-r09-a").unwrap();
        assert_ne!(production, dev, "不同 home 必须允许并行（不同命名空间）");
        assert_ne!(production, copy);
        assert_eq!(
            production,
            lock_namespace("com.lingxi.app", "/Users/u/.lingxi").unwrap(),
            "同一 home 必须稳定同键"
        );
        assert_ne!(
            lock_namespace("com.lingxi.app", "/Users/u/.lingxi").unwrap(),
            lock_namespace("dev.lingxi.other", "/Users/u/.lingxi").unwrap(),
            "不同 app identifier 不共享锁"
        );
        assert!(production.contains("lingxi-desktop:com.lingxi.app:"));
        assert_eq!(fnv1a64_hex("abc").len(), 16, "锁键摘要固定 16 位十六进制");
    }

    #[test]
    fn home_validation_rejects_relative_and_control() {
        assert!(validate_home("/private/tmp/lingxi-r09", "LINGXI_HOME").is_ok());
        assert_eq!(
            validate_home("relative/home", "LINGXI_HOME")
                .unwrap_err()
                .code(),
            E_PATH_NOT_ABSOLUTE
        );
        assert_eq!(
            validate_home("", "LINGXI_HOME").unwrap_err().code(),
            E_INVALID_ARGUMENT
        );
        assert_eq!(
            validate_home("/tmp/a\u{0}b", "LINGXI_HOME")
                .unwrap_err()
                .code(),
            E_INVALID_ARGUMENT
        );
        assert_eq!(
            lock_namespace("", "/tmp/x").unwrap_err().code(),
            E_INVALID_ARGUMENT
        );
    }

    #[test]
    fn second_instance_prefers_main_then_onboarding() {
        assert_eq!(
            plan_second_instance(true, true),
            SecondInstancePlan {
                action: SecondInstanceAction::ShowMain,
                exit_code: 0
            }
        );
        assert_eq!(
            plan_second_instance(false, true).action,
            SecondInstanceAction::ShowOnboarding
        );
        assert_eq!(
            plan_second_instance(false, false).action,
            SecondInstanceAction::ActivateOnly
        );
        // 第二实例永远以 0 退出（不是错误退出码）
        for state in [(true, true), (false, true), (false, false)] {
            assert_eq!(plan_second_instance(state.0, state.1).exit_code, 0);
        }
    }

    #[test]
    fn renderer_cannot_choose_home() {
        assert!(reject_renderer_home_override(None).is_ok());
        assert_eq!(
            reject_renderer_home_override(Some("/private/tmp/attacker"))
                .unwrap_err()
                .code(),
            E_INVALID_ARGUMENT
        );
        // 相对路径先按路径错误拒绝，不能一路走到锁逻辑
        assert_eq!(
            reject_renderer_home_override(Some("../escape"))
                .unwrap_err()
                .code(),
            E_PATH_NOT_ABSOLUTE
        );
    }

    #[test]
    fn start_at_login_arg_matches_incumbent() {
        assert_eq!(START_AT_LOGIN_ARG, "--hana-start-at-login");
    }
}
