//! 受控浏览器的窄端口；不向内核或页面传递窗口、进程或凭据句柄。
use lingxi_protocol::{BrowserCommand, BrowserReply, BrowserSettings, BrowserWorkspace};
use std::{
    future::Future,
    pin::Pin,
    sync::{atomic::AtomicBool, Arc},
};

#[derive(Debug, Clone)]
pub struct BrowserScope {
    pub session_id: String,
    pub owner_subject: String,
    pub epoch: u64,
    pub execution_node: String,
    pub human: bool,
}
/// 打印租约沿真实会话所有者回收；不混用Run generation与浏览器工作区epoch。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PdfScope {
    pub session_id: String,
    pub owner_subject: String,
    pub execution_node: String,
}
#[derive(Debug, Clone)]
pub struct BrowserDownload {
    pub id: String,
    pub suggested_filename: String,
    pub bytes: Option<Vec<u8>>,
    pub completed: bool,
}
#[derive(Debug, Clone)]
pub struct BrowserOutput {
    pub reply: BrowserReply,
    pub screenshot: Option<Vec<u8>>,
    pub downloads: Vec<BrowserDownload>,
}
/// 文档资源由服务先完成权限检查并固定成字节，渲染器不自行打开文件或下载。
#[derive(Debug, Clone)]
pub struct PdfAsset {
    pub url: String,
    pub mime_type: String,
    pub bytes: Vec<u8>,
}
#[derive(Debug, Clone)]
pub struct PdfPrintSettings {
    pub landscape: bool,
    pub print_background: bool,
    pub prefer_css_page_size: bool,
    /// 纸张及页边距沿用现役接口的英寸单位。
    pub paper_width: f64,
    pub paper_height: f64,
    pub margin_top: f64,
    pub margin_bottom: f64,
    pub margin_left: f64,
    pub margin_right: f64,
    pub page_ranges: String,
    pub scale: f64,
}
#[derive(Debug, Clone)]
pub struct PdfRenderJob {
    pub html: String,
    pub base_url: String,
    pub assets: Vec<PdfAsset>,
    pub settings: PdfPrintSettings,
    pub allow_javascript: bool,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub settle_ms: u64,
    pub timeout_ms: u64,
}
#[derive(Debug, Clone)]
pub struct PdfRendered {
    pub bytes: Vec<u8>,
    pub sha256: String,
    pub renderer: String,
}
#[derive(Debug, Clone)]
pub struct BrowserError {
    pub code: String,
    pub message: String,
    pub effect_unknown: bool,
}
impl std::fmt::Display for BrowserError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for BrowserError {}
pub type BrowserFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, BrowserError>> + Send + 'a>>;
pub trait BrowserPort: Send + Sync {
    fn execute<'a>(
        &'a self,
        scope: BrowserScope,
        command: BrowserCommand,
        cancel: Arc<AtomicBool>,
    ) -> BrowserFuture<'a, BrowserOutput>;
    fn workspaces(&self) -> Result<Vec<BrowserWorkspace>, BrowserError>;
    /// 冷恢复只登记元数据；用户明确恢复前不加载旧网址。
    fn restore(&self, scope: BrowserScope, state: BrowserWorkspace) -> Result<(), BrowserError>;
    fn revoke<'a>(&'a self, scope: BrowserScope) -> BrowserFuture<'a, ()>;
    /// 仅在真实会话已永久删除后清除该授权范围的浏览器资料。
    fn forget<'a>(&'a self, scope: BrowserScope) -> BrowserFuture<'a, ()> {
        self.revoke(scope)
    }
    fn configure<'a>(&'a self, settings: BrowserSettings) -> BrowserFuture<'a, ()>;
    fn clear_site_data<'a>(&'a self) -> BrowserFuture<'a, ()>;
    /// 搜索只用临时隔离页面，不能读取或切走用户已有页面；表达式来自可信抽取器。
    fn read_auxiliary_page<'a>(
        &'a self,
        url: String,
        expression: String,
        locale: Option<String>,
        cancel: Arc<AtomicBool>,
    ) -> BrowserFuture<'a, serde_json::Value>;
    /// 与浏览器共用受控 Chromium；能力缺失时明确拒绝，不回落到旧内核。
    fn render_pdf<'a>(
        &'a self,
        _scope: PdfScope,
        _job: PdfRenderJob,
        _cancel: Arc<AtomicBool>,
    ) -> BrowserFuture<'a, PdfRendered> {
        Box::pin(async {
            Err(BrowserError {
                code: "PDF_RENDERER_UNAVAILABLE".into(),
                message: "当前受控浏览器未提供文档渲染能力".into(),
                effect_unknown: false,
            })
        })
    }
    /// 只枚举仍被管理的临时打印租约，包括退出尚未确认的租约，不创建业务工作区。
    fn pdf_scopes(&self) -> Result<Vec<PdfScope>, BrowserError> {
        Ok(Vec::new())
    }
    /// 生命周期变化后精确取消本会话所有者的打印；其他会话和登录资料不受影响。
    fn cancel_pdf_scope<'a>(&'a self, _scope: PdfScope) -> BrowserFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn acknowledge_downloads<'a>(
        &'a self,
        _scope: BrowserScope,
        _ids: Vec<String>,
    ) -> BrowserFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn reap_idle<'a>(&'a self, now_ms: u64) -> BrowserFuture<'a, ()>;
    fn shutdown<'a>(&'a self) -> BrowserFuture<'a, ()>;
}
