//! R09-T03 OQ-03 有界 spike：macOS 通知点击聚焦（UNUserNotificationCenter delegate）。
//!
//! 背景（见 `notification.rs` 文档与 OQ-03）：`tauri-plugin-notification` 桌面端
//! 走 `notify-rust`（`NSUserNotificationCenter` 旧路径），其点击回调只有**阻塞式**
//! `NotificationHandle::wait_for_response`（要求主 run loop，不能嵌进 Tauri 事件
//! 循环）；插件 API 不暴露 handle，因此无法在插件之上接点击 → 原方案 Unsupported。
//!
//! 本 spike 的可行路径：**不使用插件展示**，宿主直接用
//! `UNUserNotificationCenter`（objc2-user-notifications 0.3.2）：
//! - 安装自己的 delegate（`willPresent` / `didReceive`）；
//! - `didReceive`（用户点击）→ 激活 app 并聚焦主窗；
//! - 通过 `requestAuthorization` 获取授权、`addNotificationRequest` 展示。
//!
//! 验证边界：编译与 delegate 安装逻辑在真实 crate 验证；**「真实点击后聚焦」需
//! 注册后的人工/GUI 证据**（点击系统通知无法在无 UI 自动化时自动触发），因此在
//! `T03_T04_PROGRESS.md` 登记为「已实现待真实点击验证」，未验证前不得声称 A05 通过。
//!
//! 平台：仅 macos；非 mac 由调用方回落插件路径（点击聚焦仍 Unsupported）。

use std::sync::OnceLock;

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, ClassType};
use objc2_foundation::{NSObject, NSObjectProtocol, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification,
    UNNotificationPresentationOptions, UNNotificationRequest, UNNotificationResponse,
    UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};

use tauri::AppHandle;

/// 点击通知时聚焦的 app handle（Wry 运行时；宿主只有一个 app）。
static CLICK_APP: OnceLock<AppHandle> = OnceLock::new();
/// delegate 必须被强引用保活（UNUserNotificationCenter.delegate 是 weak）。
static DELEGATE: OnceLock<Retained<ClickDelegate>> = OnceLock::new();

define_class!(
    // SAFETY: NSObject 无子类化约束；delegate 不实现 Drop。
    #[unsafe(super(NSObject))]
    struct ClickDelegate;

    // SAFETY: NSObjectProtocol 无安全要求。
    unsafe impl NSObjectProtocol for ClickDelegate {}

    // SAFETY: 方法签名与协议声明一致（见 objc2-user-notifications 生成代码）。
    #[allow(non_snake_case)]
    unsafe impl UNUserNotificationCenterDelegate for ClickDelegate {
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn userNotificationCenter_willPresentNotification_withCompletionHandler(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            completion_handler: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            // 前台也显示通知（与现役 Electron 的 focus 抑制策略在 Rust 侧另行判定；
            // 到这里说明已决定展示）。
            completion_handler.call((UNNotificationPresentationOptions::Banner
                | UNNotificationPresentationOptions::List
                | UNNotificationPresentationOptions::Sound,));
        }

        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn userNotificationCenter_didReceiveNotificationResponse_withCompletionHandler(
            &self,
            _center: &UNUserNotificationCenter,
            _response: &UNNotificationResponse,
            completion_handler: &block2::DynBlock<dyn Fn()>,
        ) {
            // 点击 → 激活 app 并聚焦主窗（现役 Electron notif.on("click") 等价）。
            if let Some(app) = CLICK_APP.get() {
                crate::native::install::focus_main(app);
            }
            completion_handler.call(());
        }
    }
);

/// 安装 delegate 并保存 app handle；重复调用幂等。
pub fn install_click_handler(app: &AppHandle) -> Result<(), String> {
    let _ = CLICK_APP.set(app.clone());
    if DELEGATE.get().is_some() {
        return Ok(());
    }
    let delegate: Retained<ClickDelegate> = unsafe { msg_send![ClickDelegate::class(), new] };
    let center = UNUserNotificationCenter::currentNotificationCenter();
    center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    let _ = DELEGATE.set(delegate);
    Ok(())
}

/// 确保通知授权（首次会弹系统授权；已终结状态不重复弹）。
pub fn ensure_authorization() -> Result<bool, String> {
    let center = UNUserNotificationCenter::currentNotificationCenter();
    let (sender, receiver) = std::sync::mpsc::channel::<bool>();
    let block = block2::RcBlock::new(
        move |granted: objc2::runtime::Bool, _error: *mut objc2_foundation::NSError| {
            let _ = sender.send(granted.as_bool());
        },
    );
    center.requestAuthorizationWithOptions_completionHandler(
        UNAuthorizationOptions::Alert
            | UNAuthorizationOptions::Sound
            | UNAuthorizationOptions::Badge,
        &block,
    );
    receiver
        .recv_timeout(std::time::Duration::from_secs(120))
        .map_err(|_| "通知授权等待超时（用户未响应）".to_string())
}

/// 用 UNUserNotificationCenter 展示通知；返回后点击由 delegate 聚焦主窗。
pub fn show(title: &str, body: &str) -> Result<(), String> {
    if DELEGATE.get().is_none() {
        return Err("通知 delegate 未安装（install_click_handler 未调用）".to_string());
    }
    let content: Retained<UNMutableNotificationContent> =
        unsafe { msg_send![UNMutableNotificationContent::class(), new] };
    content.setTitle(&NSString::from_str(title));
    content.setBody(&NSString::from_str(body));
    let content: Retained<objc2_user_notifications::UNNotificationContent> =
        unsafe { Retained::cast_unchecked(content) };
    let identifier = NSString::from_str(&format!(
        "lingxi-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0)
    ));
    let request =
        UNNotificationRequest::requestWithIdentifier_content_trigger(&identifier, &content, None);
    let center = UNUserNotificationCenter::currentNotificationCenter();
    center.addNotificationRequest_withCompletionHandler(&request, None);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delegate_class_registered() {
        // define_class 在加载时注册 Objective-C 类；能取到 class 对象即注册成功。
        let class = ClickDelegate::class();
        assert!(!class.name().is_empty());
    }
}
