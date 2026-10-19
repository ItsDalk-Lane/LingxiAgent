//! 尚无等价管道/进程组实现的平台明确报告能力不可用。
use super::{error, BrowserError};
use crate::models::network::NetworkPlane;
use lingxi_kernel::browser::{BrowserFuture, BrowserOutput, BrowserPort, BrowserScope};
use lingxi_protocol::{BrowserCommand, BrowserSettings, BrowserWorkspace};
use std::{
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc},
};
pub struct ChromiumConfig {
    pub executable: PathBuf,
    pub root: PathBuf,
    pub execution_node: String,
    pub headless: bool,
    pub network: Arc<NetworkPlane>,
}
pub struct ChromiumHost;
fn unavailable<T>() -> Result<T, BrowserError> {
    Err(error(
        "browser_platform_unavailable",
        "此平台尚未提供受控Chromium管道宿主",
        false,
    ))
}
impl ChromiumHost {
    pub fn new(_: ChromiumConfig) -> Result<Arc<Self>, BrowserError> {
        unavailable()
    }
}
impl BrowserPort for ChromiumHost {
    fn execute<'a>(
        &'a self,
        _: BrowserScope,
        _: BrowserCommand,
        _: Arc<AtomicBool>,
    ) -> BrowserFuture<'a, BrowserOutput> {
        Box::pin(async { unavailable() })
    }
    fn workspaces(&self) -> Result<Vec<BrowserWorkspace>, BrowserError> {
        unavailable()
    }
    fn restore(&self, _: BrowserScope, _: BrowserWorkspace) -> Result<(), BrowserError> {
        unavailable()
    }
    fn revoke<'a>(&'a self, _: BrowserScope) -> BrowserFuture<'a, ()> {
        Box::pin(async { unavailable() })
    }
    fn configure<'a>(&'a self, _: BrowserSettings) -> BrowserFuture<'a, ()> {
        Box::pin(async { unavailable() })
    }
    fn clear_site_data<'a>(&'a self) -> BrowserFuture<'a, ()> {
        Box::pin(async { unavailable() })
    }
    fn read_auxiliary_page<'a>(
        &'a self,
        _: String,
        _: String,
        _: Option<String>,
        _: Arc<AtomicBool>,
    ) -> BrowserFuture<'a, serde_json::Value> {
        Box::pin(async { unavailable() })
    }
    fn reap_idle<'a>(&'a self, _: u64) -> BrowserFuture<'a, ()> {
        Box::pin(async { unavailable() })
    }
    fn shutdown<'a>(&'a self) -> BrowserFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
}
