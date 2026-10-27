//! 生产受控Chromium适配器；业务元数据由服务的单写通道保存。
use lingxi_kernel::browser::BrowserError;

#[cfg(unix)]
mod cdp;
#[cfg(unix)]
mod host;
#[cfg(unix)]
mod launcher;
#[cfg(unix)]
mod managed_files;
#[cfg(unix)]
mod ops;
#[cfg(unix)]
mod pdf;
#[cfg(unix)]
mod runtime;
#[cfg(unix)]
pub use host::{ChromiumConfig, ChromiumHost};
#[cfg(not(unix))]
mod unavailable;
#[cfg(not(unix))]
pub use unavailable::{ChromiumConfig, ChromiumHost};

pub(super) fn error(code: &str, message: &str, effect_unknown: bool) -> BrowserError {
    BrowserError {
        code: code.into(),
        message: message.into(),
        effect_unknown,
    }
}
