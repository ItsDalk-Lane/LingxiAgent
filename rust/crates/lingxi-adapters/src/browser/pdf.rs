//! PDF 只在独占的临时 Chromium 上打印冻结资源；不访问文档目录或业务标签页。
//!
//! 接线时，Live 的事件入口必须先调用 `intercept_event`，命中后不再走普通
//! Cookie/Fetch 处理。render 会在任何文档加载前确认该入口实际生效；缺失时拒绝。
//! 此文件不持有进程清理权：宿主必须无论成功或失败都 stop_owned，确认后才删目录。
use super::{cdp::Cdp, error, ops, runtime::Live, BrowserError};
use base64::Engine;
use lingxi_kernel::browser::{PdfPrintSettings, PdfRenderJob, PdfRendered};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    net::TcpListener,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};

const MAX_HTML: usize = 8 * 1024 * 1024;
// 单个 fulfillRequest 还需 Base64 编码，须留在 CDP 的 32MiB 单帧预算内。
const MAX_ASSET: usize = 20 * 1024 * 1024;
const MAX_ASSETS: usize = 512;
const MAX_ASSET_TOTAL: usize = 64 * 1024 * 1024;
const MAX_PDF: usize = 50 * 1024 * 1024;
const READ_CHUNK: usize = 64 * 1024;
const MAX_REQUESTS: usize = 4096;
const MAX_LEASES: usize = 2;

/// 专属拒绝代理：没有转发、DNS或请求正文读取；只有当前打印租约持有的监听句柄。
/// 无额外工作线程，检查点有界接受并立即关连接，避免新增不可回收的线程/套接字。
pub(super) struct DenyProxy {
    address: String,
    listener: Mutex<Option<TcpListener>>,
}
impl DenyProxy {
    pub(super) fn new() -> Result<Arc<Self>, BrowserError> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|_| error("PDF_NETWORK_FENCE", "无法建立打印网络隔离", false))?;
        listener
            .set_nonblocking(true)
            .map_err(|_| error("PDF_NETWORK_FENCE", "无法设置打印网络隔离", false))?;
        let address = listener
            .local_addr()
            .map_err(|_| error("PDF_NETWORK_FENCE", "无法确认打印网络隔离", false))?
            .to_string();
        Ok(Arc::new(Self {
            address,
            listener: Mutex::new(Some(listener)),
        }))
    }
    pub(super) fn arguments(&self) -> Vec<String> {
        vec![
            format!("--proxy-server=http://{}", self.address),
            // Chromium默认绕过loopback，打印不能沿用普通浏览器的这一便利。
            "--proxy-bypass-list=<-loopback>".into(),
            // Chrome 155实测：~NOTFOUND同样作用于代理自身的字面loopback地址，
            // 缺EXCLUDE时代理连接失败并被标记bad后静默回退DIRECT，隔离形同虚设。
            "--host-resolver-rules=MAP * ~NOTFOUND,EXCLUDE 127.0.0.1".into(),
            "--force-webrtc-ip-handling-policy=disable_non_proxied_udp".into(),
            "--disable-quic".into(),
        ]
    }
    fn drain(&self) -> Result<(), BrowserError> {
        let listener = self.listener.lock().unwrap_or_else(|p| p.into_inner());
        let listener = listener
            .as_ref()
            .ok_or_else(|| error("PDF_NETWORK_FENCE", "打印网络隔离已关闭", false))?;
        // 即使文档制造大量预连接也不能让一次检查无限占用线程。
        for _ in 0..256 {
            match listener.accept() {
                Ok((connection, _)) => drop(connection),
                Err(cause) if cause.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
                Err(cause) if cause.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return Err(error("PDF_NETWORK_FENCE", "打印网络隔离不可用", false)),
            }
        }
        Ok(())
    }
    /// 只能在真实Chromium确认停止后调用；Unknown仍保留同监听FD，不能被其他进程占用。
    pub(super) fn close(&self) {
        self.listener
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
    }
}

struct FrozenResponse {
    mime: String,
    bytes: Vec<u8>,
}
struct Document {
    target: String,
    session: String,
    frame: String,
}
struct Resources {
    base_url: String,
    responses: BTreeMap<String, FrozenResponse>,
    document: Mutex<Option<Document>>,
    csp: String,
    allow_javascript: bool,
    cancel: Arc<AtomicBool>,
    deadline: Instant,
    hooked: AtomicBool,
    document_served: AtomicBool,
    request_count: AtomicUsize,
    failure: Mutex<Option<BrowserError>>,
    proxy: Arc<DenyProxy>,
}

// 这里只登记最多两个短命执行租约。没有会话业务状态，也没有磁盘副本；由RAII移除。
type Registry = Mutex<BTreeMap<usize, Arc<Resources>>>;
fn registry() -> &'static Registry {
    static LEASES: OnceLock<Registry> = OnceLock::new();
    LEASES.get_or_init(|| Mutex::new(BTreeMap::new()))
}
fn identity(live: &Live) -> usize {
    Arc::as_ptr(live.cdp()) as usize
}
struct ResourceLease {
    identity: usize,
    connection: Arc<Cdp>,
    resources: Arc<Resources>,
}
impl ResourceLease {
    fn register(live: &Live, resources: Resources) -> Result<Self, BrowserError> {
        let identity = identity(live);
        let resources = Arc::new(resources);
        let mut leases = registry().lock().unwrap_or_else(|p| p.into_inner());
        if leases.contains_key(&identity) || leases.len() >= MAX_LEASES {
            return Err(error("PDF_CAPACITY", "文档打印执行器已达上限", false));
        }
        leases.insert(identity, resources.clone());
        Ok(Self {
            identity,
            connection: live.cdp().clone(),
            resources,
        })
    }
}
impl Drop for ResourceLease {
    fn drop(&mut self) {
        // 打印返回与宿主kill/wait之间仍不得恢复普通Fetch放行。专用连接先关闭，
        // 已启用的请求拦截保留在Chrome内，后续请求保持暂停直至宿主真正关闭进程。
        self.connection.close();
        let mut leases = registry().lock().unwrap_or_else(|p| p.into_inner());
        if leases
            .get(&self.identity)
            .is_some_and(|current| Arc::ptr_eq(current, &self.resources))
        {
            leases.remove(&self.identity);
        }
    }
}

fn invalid(message: &str) -> BrowserError {
    error("PDF_INVALID_JOB", message, false)
}
fn document_url(value: &str) -> Result<reqwest::Url, BrowserError> {
    let url = reqwest::Url::parse(value).map_err(|_| invalid("文档资源地址无效"))?;
    if value.len() > 8192
        || url.scheme() != "https"
        || url.host_str() != Some("lingxi-pdf.invalid")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.as_str() != value
    {
        return Err(invalid("文档只能使用宿主固定的隔离资源地址"));
    }
    Ok(url)
}
fn valid_mime(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.contains('/')
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$&^_.+-/;= ".contains(&b))
}
fn check_settings(settings: &PdfPrintSettings) -> Result<(), BrowserError> {
    let positive = |value: f64| value.is_finite() && value > 0.0 && value <= 200.0;
    let margin = |value: f64| value.is_finite() && (0.0..=200.0).contains(&value);
    let (width, height) = if settings.landscape {
        (settings.paper_height, settings.paper_width)
    } else {
        (settings.paper_width, settings.paper_height)
    };
    if !positive(settings.paper_width)
        || !positive(settings.paper_height)
        || !margin(settings.margin_top)
        || !margin(settings.margin_bottom)
        || !margin(settings.margin_left)
        || !margin(settings.margin_right)
        || settings.margin_top + settings.margin_bottom >= height
        || settings.margin_left + settings.margin_right >= width
        || !settings.scale.is_finite()
        || !(0.1..=2.0).contains(&settings.scale)
        || settings.page_ranges.len() > 1024
        || !settings
            .page_ranges
            .bytes()
            .all(|b| b.is_ascii_digit() || b" ,-".contains(&b))
    {
        return Err(invalid("文档纸张、页边距或分页设置无效"));
    }
    Ok(())
}
fn csp(allow_javascript: bool) -> String {
    // 响应头在HTML解析之前生效；不用页面脚本或MutationObserver补装网络边界。
    // sandbox禁止弹窗、表单、子框架、下载和同源权限，字体响应独立提供无凭据CORS。
    let scripts = if allow_javascript {
        "'unsafe-inline' https://lingxi-pdf.invalid"
    } else {
        "'none'"
    };
    let sandbox = if allow_javascript {
        " allow-scripts"
    } else {
        ""
    };
    format!(
        "default-src 'none'; script-src {scripts}; style-src 'unsafe-inline' https://lingxi-pdf.invalid; img-src data: blob: https://lingxi-pdf.invalid; font-src data: https://lingxi-pdf.invalid; media-src data: blob: https://lingxi-pdf.invalid; connect-src 'none'; worker-src 'none'; child-src 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'; manifest-src 'none'; sandbox{sandbox}"
    )
}
pub(super) fn validate(job: &PdfRenderJob) -> Result<(), BrowserError> {
    let base = document_url(&job.base_url)?;
    if job.html.len() > MAX_HTML
        || !(320..=4096).contains(&job.viewport_width)
        || !(320..=4096).contains(&job.viewport_height)
        || !(1000..=300_000).contains(&job.timeout_ms)
        || job.settle_ms > 30_000
        || job.assets.len() > MAX_ASSETS
    {
        return Err(invalid("文档或渲染设置超出执行预算"));
    }
    check_settings(&job.settings)?;
    let directory = base
        .path()
        .rsplit_once('/')
        .map(|(parent, _)| format!("{parent}/"))
        .ok_or_else(|| invalid("文档隔离资源目录无效"))?;
    if directory == "/" || base.path().ends_with('/') {
        return Err(invalid("文档必须有独立的隔离资源目录"));
    }
    let mut responses = BTreeMap::new();
    let mut total = 0usize;
    for asset in &job.assets {
        let url = document_url(&asset.url)?;
        if !url.path().starts_with(&directory)
            || asset.url == job.base_url
            || !valid_mime(&asset.mime_type)
            || asset.bytes.len() > MAX_ASSET
        {
            return Err(invalid("文档资源超出固定目录、格式或单件预算"));
        }
        total = total
            .checked_add(asset.bytes.len())
            .ok_or_else(|| invalid("文档资源大小溢出"))?;
        if total > MAX_ASSET_TOTAL || responses.contains_key(&asset.url) {
            return Err(invalid("文档资源重复或超出总预算"));
        }
        responses.insert(asset.url.clone(), ());
    }
    Ok(())
}
fn freeze(
    job: &mut PdfRenderJob,
    cancel: Arc<AtomicBool>,
    deadline: Instant,
    proxy: Arc<DenyProxy>,
) -> Result<Resources, BrowserError> {
    validate(job)?;
    let mut responses = BTreeMap::new();
    for asset in std::mem::take(&mut job.assets) {
        responses.insert(
            asset.url,
            FrozenResponse {
                mime: asset.mime_type,
                bytes: asset.bytes,
            },
        );
    }
    responses.insert(
        job.base_url.clone(),
        FrozenResponse {
            mime: "text/html; charset=utf-8".into(),
            bytes: std::mem::take(&mut job.html).into_bytes(),
        },
    );
    Ok(Resources {
        base_url: job.base_url.clone(),
        responses,
        document: Mutex::new(None),
        csp: csp(job.allow_javascript),
        allow_javascript: job.allow_javascript,
        cancel,
        deadline,
        hooked: AtomicBool::new(false),
        document_served: AtomicBool::new(false),
        request_count: AtomicUsize::new(0),
        failure: Mutex::new(None),
        proxy,
    })
}

impl Resources {
    fn owns_session(&self, event: &Value) -> bool {
        self.document
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .is_some_and(|d| event["sessionId"] == d.session)
    }

    fn check(&self, live: &Live) -> Result<(), BrowserError> {
        self.proxy.drain()?;
        if let Some(cause) = self
            .failure
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
        {
            return Err(cause);
        }
        if self.cancel.load(Ordering::Acquire) || live.stop.load(Ordering::Acquire) {
            return Err(error("cancelled", "文档打印已取消", false));
        }
        if Instant::now() >= self.deadline {
            return Err(error("PDF_TIMEOUT", "文档打印超出总等待时间", false));
        }
        if live.cdp().is_closed() {
            return Err(error("PDF_RENDERER_CLOSED", "文档打印连接已关闭", false));
        }
        Ok(())
    }
    fn fail(&self, cause: BrowserError) -> BrowserError {
        let mut failure = self.failure.lock().unwrap_or_else(|p| p.into_inner());
        if failure.is_none() {
            *failure = Some(cause.clone());
        }
        cause
    }
    fn request(&self, live: &Live, event: &Value) -> Result<(), BrowserError> {
        self.check(live)?;
        if self.request_count.fetch_add(1, Ordering::AcqRel) >= MAX_REQUESTS {
            return Err(error(
                "PDF_RESOURCE_BUDGET",
                "文档资源请求次数超出上限",
                false,
            ));
        }
        let p = &event["params"];
        let sid = event["sessionId"].as_str();
        let request_id = p["requestId"]
            .as_str()
            .filter(|id| !id.is_empty() && id.len() <= 512)
            .ok_or_else(|| error("PDF_PROTOCOL", "文档资源请求缺少真实身份", false))?;
        let document = self.document.lock().unwrap_or_else(|p| p.into_inner());
        let own_session = document
            .as_ref()
            .is_some_and(|d| Some(d.session.as_str()) == sid);
        let url = p["request"]["url"].as_str().unwrap_or("");
        let is_document = p["resourceType"] == "Document";
        let own_document = document
            .as_ref()
            .is_some_and(|d| p["frameId"] == d.frame && url == self.base_url);
        let response = self.responses.get(url).filter(|_| {
            own_session
                && p["request"]["method"] == "GET"
                && p.get("responseStatusCode").is_none()
                && (!is_document || own_document)
        });
        drop(document);
        let response = if is_document
            && response.is_some()
            && self.document_served.swap(true, Ordering::AcqRel)
        {
            None
        } else {
            response
        };
        if let Some(response) = response {
            live.call(
                sid,
                "Fetch.fulfillRequest",
                json!({
                    "requestId":request_id,"responseCode":200,
                    "responseHeaders":[
                        {"name":"Content-Type","value":response.mime},
                        {"name":"Content-Length","value":response.bytes.len().to_string()},
                        {"name":"Cache-Control","value":"no-store"},
                        {"name":"Content-Security-Policy","value":self.csp},
                        {"name":"X-Content-Type-Options","value":"nosniff"},
                        {"name":"Access-Control-Allow-Origin","value":"*"},
                        {"name":"Cross-Origin-Resource-Policy","value":"cross-origin"}
                    ],
                    "body":base64::engine::general_purpose::STANDARD.encode(&response.bytes)
                }),
                self.deadline,
                &self.cancel,
                false,
            )?;
        } else {
            // 禁止请求也必须明确完成暂停，不能继续到代理、磁盘或真实网络。
            live.call(
                sid,
                "Fetch.failRequest",
                json!({"requestId":request_id,"errorReason":"BlockedByClient"}),
                self.deadline,
                &self.cancel,
                false,
            )?;
            if p["resourceType"] == "Other" {
                // 无关打印的浏览器附带请求同样拒绝网络，但不把缺省favicon当正文失败。
                return Ok(());
            }
            return Err(error(
                "PDF_RESOURCE_FORBIDDEN",
                "文档请求了未冻结的资源或额外页面",
                false,
            ));
        }
        Ok(())
    }
}

/// Live事件线程的第一道分流；命中Fetch后不得再由普通Cookie处理器放行。
/// 注册缺失时只返回false，普通浏览器和搜索不受影响。
pub(super) fn intercept_event(live: &Live, event: &Value) -> Result<bool, BrowserError> {
    let resources = registry()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get(&identity(live))
        .cloned();
    let Some(resources) = resources else {
        return Ok(false);
    };
    resources.hooked.store(true, Ordering::Release);
    let result = match event["method"].as_str() {
        Some("Fetch.requestPaused") => resources.request(live, event).map(|()| true),
        Some("Network.loadingFailed") if resources.owns_session(event) => {
            // CSP可能在Fetch暂停之前拒绝stylesheet/font/image；仍须认定真实加载失败。
            // 浏览器自身的favicon等Other请求不参与打印；主动禁用的页面脚本同理。
            let kind = event["params"]["type"].as_str();
            if kind == Some("Other") || (!resources.allow_javascript && kind == Some("Script")) {
                Ok(false)
            } else {
                Err(error(
                    "PDF_ASSET_LOAD_FAILED",
                    "文档所需资源未成功加载",
                    false,
                ))
            }
        }
        Some("Target.attachedToTarget") if event["params"]["targetInfo"]["type"] == "page" => {
            let document = resources.document.lock().unwrap_or_else(|p| p.into_inner());
            let unwanted = document
                .as_ref()
                .is_some_and(|d| event["params"]["targetInfo"]["targetId"] != d.target);
            drop(document);
            if unwanted {
                live.call(
                    None,
                    "Target.closeTarget",
                    json!({"targetId":event["params"]["targetInfo"]["targetId"]}),
                    resources.deadline,
                    &resources.cancel,
                    false,
                )?;
                Err(error(
                    "PDF_EXTRA_PAGE",
                    "文档尝试创建额外页面，打印已拒绝",
                    false,
                ))
            } else {
                Ok(false)
            }
        }
        _ => Ok(false),
    };
    result.map_err(|cause| resources.fail(cause))
}

const ASSET_READY: &str = r#"(async(assetImages)=>{
  if(document.readyState!=='complete') throw new Error('document_not_complete');
  if(document.fonts){
    await document.fonts.ready;
    for(const face of document.fonts) if(face.status==='error') throw new Error('font_failed');
  }
  const images=Array.from(document.images);
  await Promise.all(images.map(image=>image.complete?Promise.resolve():new Promise((resolve,reject)=>{
    image.addEventListener('load',resolve,{once:true});
    image.addEventListener('error',()=>reject(new Error('image_failed')),{once:true});
  })));
  if(images.some(image=>!image.complete||image.naturalWidth===0)) throw new Error('image_failed');
  // CSS背景不在document.images里；对冻结图片逐个使用浏览器实际解码，不把HTTP成功当图片成功。
  await Promise.all(assetImages.map(async url=>{
    const image=new Image();
    image.src=url;
    await image.decode();
    if(!image.complete||image.naturalWidth===0) throw new Error('background_image_failed');
  }));
  return {ready:true,url:document.URL,images:images.length};
})"#;

fn same_document(
    live: &Live,
    resources: &Resources,
    sid: &str,
    loader: &str,
) -> Result<(), BrowserError> {
    resources.check(live)?;
    let tree = live.call(
        Some(sid),
        "Page.getFrameTree",
        json!({}),
        resources.deadline,
        &resources.cancel,
        false,
    )?;
    let frame = &tree["frameTree"]["frame"];
    let document = resources.document.lock().unwrap_or_else(|p| p.into_inner());
    if !document.as_ref().is_some_and(|d| {
        frame["id"] == d.frame && frame["loaderId"] == loader && frame["url"] == resources.base_url
    }) {
        return Err(error(
            "PDF_DOCUMENT_CHANGED",
            "打印前文档身份发生变化",
            false,
        ));
    }
    Ok(())
}

struct PdfStream<'a> {
    live: &'a Live,
    session: &'a str,
    handle: String,
    closed: bool,
}
impl PdfStream<'_> {
    fn close(&mut self, deadline: Instant, cancel: &AtomicBool) -> Result<(), BrowserError> {
        if self.closed {
            return Ok(());
        }
        self.live.call(
            Some(self.session),
            "IO.close",
            json!({"handle":self.handle}),
            deadline,
            cancel,
            false,
        )?;
        self.closed = true;
        Ok(())
    }
}
impl Drop for PdfStream<'_> {
    fn drop(&mut self) {
        if !self.closed && !self.live.cdp().is_closed() {
            // 失败后的句柄回收另有一秒上限；打印总预算不会被重新开始。
            let _ = self.close(
                Instant::now() + Duration::from_secs(1),
                &AtomicBool::new(false),
            );
        }
    }
}
fn validate_pdf(bytes: &[u8]) -> Result<(), BrowserError> {
    if bytes.len() < 16
        || bytes.len() > MAX_PDF
        || !bytes.starts_with(b"%PDF-")
        || !bytes[bytes.len().saturating_sub(2048)..]
            .windows(5)
            .any(|part| part == b"%%EOF")
    {
        return Err(error(
            "PDF_INVALID_OUTPUT",
            "打印结果不是完整且有界的PDF",
            false,
        ));
    }
    Ok(())
}
fn read_pdf(
    live: &Live,
    sid: &str,
    handle: String,
    resources: &Resources,
) -> Result<Vec<u8>, BrowserError> {
    let mut stream = PdfStream {
        live,
        session: sid,
        handle,
        closed: false,
    };
    let mut bytes = Vec::new();
    let mut empty_chunks = 0;
    loop {
        resources.check(live)?;
        let part = live.call(
            Some(sid),
            "IO.read",
            json!({"handle":stream.handle,"size":READ_CHUNK}),
            resources.deadline,
            &resources.cancel,
            false,
        )?;
        let data = part["data"]
            .as_str()
            .ok_or_else(|| error("PDF_PROTOCOL", "打印流未返回实际字节", false))?;
        if data.len() > READ_CHUNK * 4 / 3 + 8 {
            return Err(error("PDF_OUTPUT_BUDGET", "打印流单块超过上限", false));
        }
        if part.get("base64Encoded").is_some() && !part["base64Encoded"].is_boolean() {
            return Err(error("PDF_PROTOCOL", "打印流编码标记无效", false));
        }
        let chunk = match part["base64Encoded"].as_bool() {
            Some(true) => base64::engine::general_purpose::STANDARD
                .decode(data)
                .map_err(|_| error("PDF_PROTOCOL", "打印流字节编码无效", false))?,
            Some(false) | None => data.as_bytes().to_vec(),
        };
        if chunk.len() > READ_CHUNK || bytes.len().saturating_add(chunk.len()) > MAX_PDF {
            return Err(error("PDF_OUTPUT_BUDGET", "PDF结果超过50MiB上限", false));
        }
        if chunk.is_empty() {
            empty_chunks += 1;
        } else {
            empty_chunks = 0;
        }
        bytes.extend_from_slice(&chunk);
        match part["eof"].as_bool() {
            Some(true) => break,
            Some(false) if empty_chunks < 16 => {}
            Some(false) => return Err(error("PDF_PROTOCOL", "打印流未持续提供实际内容", false)),
            None => return Err(error("PDF_PROTOCOL", "打印流缺少完成状态", false)),
        }
    }
    stream.close(resources.deadline, &resources.cancel)?;
    validate_pdf(&bytes)?;
    Ok(bytes)
}

/// 调用方须提供一个全新、专用、无登录资料的Live，并在返回后确认进程清理。
/// 只返回内存中的真实PDF，不写业务workspace、下载收据或任意路径。
pub(super) fn render(
    live: &Live,
    mut job: PdfRenderJob,
    cancel: Arc<AtomicBool>,
    deadline: Instant,
    proxy: Arc<DenyProxy>,
) -> Result<PdfRendered, BrowserError> {
    let resources = freeze(&mut job, cancel, deadline, proxy)?;
    resources.check(live)?;
    if !live
        .tabs
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .is_empty()
        || !live
            .attached
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_empty()
    {
        return Err(error(
            "PDF_REQUIRES_PRIVATE_LEASE",
            "打印只能使用全新的隔离进程",
            false,
        ));
    }
    let lease = ResourceLease::register(live, resources)?;
    let outcome = render_leased(live, &job, &lease.resources);
    if outcome.is_err() {
        if let Some(cause) = lease
            .resources
            .failure
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
        {
            return Err(cause);
        }
    }
    outcome
}

fn render_leased(
    live: &Live,
    job: &PdfRenderJob,
    resources: &Resources,
) -> Result<PdfRendered, BrowserError> {
    let deadline = resources.deadline;
    let cancel = resources.cancel.clone();
    let target = ops::new_tab(live, "about:blank", false, deadline, &cancel)?;
    let sid = live.attach(&target, deadline, &cancel)?;
    // 创建空白target应已产生事件；未实际接通独占分流时，绝不开始加载文档。
    let hook_deadline = deadline.min(Instant::now() + Duration::from_secs(1));
    while !resources.hooked.load(Ordering::Acquire) {
        resources.check(live)?;
        if Instant::now() >= hook_deadline {
            return Err(error(
                "PDF_RESOURCE_GATE_UNAVAILABLE",
                "隔离资源入口尚未接通",
                false,
            ));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let frame = live.call(
        Some(&sid),
        "Page.getFrameTree",
        json!({}),
        deadline,
        &cancel,
        false,
    )?;
    let frame = frame["frameTree"]["frame"]["id"]
        .as_str()
        .ok_or_else(|| error("PDF_PROTOCOL", "空白文档缺少真实frame", false))?
        .to_owned();
    *resources.document.lock().unwrap_or_else(|p| p.into_inner()) = Some(Document {
        target,
        session: sid.clone(),
        frame: frame.clone(),
    });
    live.call(
        None,
        "Browser.setDownloadBehavior",
        json!({"behavior":"deny"}),
        deadline,
        &cancel,
        false,
    )?;
    live.call(
        Some(&sid),
        "Network.enable",
        json!({}),
        deadline,
        &cancel,
        false,
    )?;
    live.call(
        Some(&sid),
        "Network.setCacheDisabled",
        json!({"cacheDisabled":true}),
        deadline,
        &cancel,
        false,
    )?;
    live.call(
        Some(&sid),
        "Network.setBypassServiceWorker",
        json!({"bypass":true}),
        deadline,
        &cancel,
        false,
    )?;
    // Fetch在真正请求前拦截，仍不能阻止浏览器为iframe/跳转提前建立TCP。
    // 专属执行器在加载文档前进入真实离线网络条件；合法资源只由下方Fetch履约。
    // 不能只改navigator.onLine，也不能在不支持时退回在线打印。
    live.call(
        Some(&sid),
        "Network.emulateNetworkConditions",
        json!({"offline":true,"latency":0,"downloadThroughput":0,"uploadThroughput":0}),
        deadline,
        &cancel,
        false,
    )?;
    live.call(Some(&sid), "Emulation.setDeviceMetricsOverride",
        json!({"width":job.viewport_width,"height":job.viewport_height,"deviceScaleFactor":1,"mobile":false}),
        deadline, &cancel, false)?;
    // 先启用打印媒体，再等待资源；屏幕样式的字体就绪不能代表打印样式也已加载。
    live.call(
        Some(&sid),
        "Emulation.setEmulatedMedia",
        json!({"media":"print"}),
        deadline,
        &cancel,
        false,
    )?;
    live.call(
        Some(&sid),
        "Emulation.setScriptExecutionDisabled",
        json!({"value":!job.allow_javascript}),
        deadline,
        &cancel,
        false,
    )?;
    live.call(
        Some(&sid),
        "Fetch.enable",
        json!({"patterns":[{"urlPattern":"*","requestStage":"Request"}]}),
        deadline,
        &cancel,
        false,
    )?;
    let navigation = ops::navigate(live, &sid, &job.base_url, deadline, &cancel)?;
    resources.check(live)?;
    if !resources.document_served.load(Ordering::Acquire) {
        return Err(error(
            "PDF_DOCUMENT_UNCONFIRMED",
            "隔离文档未经过冻结资源入口",
            false,
        ));
    }
    let loader = navigation["loaderId"]
        .as_str()
        .ok_or_else(|| error("PDF_PROTOCOL", "隔离文档没有真实loader", false))?;
    same_document(live, resources, &sid, loader)?;
    let settle_until = Instant::now()
        .checked_add(Duration::from_millis(job.settle_ms))
        .ok_or_else(|| invalid("文档稳定等待时间溢出"))?;
    while Instant::now() < settle_until {
        resources.check(live)?;
        std::thread::sleep(
            Duration::from_millis(10).min(settle_until.saturating_duration_since(Instant::now())),
        );
    }
    let context = live.call(
        Some(&sid),
        "Page.createIsolatedWorld",
        json!({"frameId":frame,"worldName":"LingxiPdfReadiness","grantUniveralAccess":false}),
        deadline,
        &cancel,
        false,
    )?;
    let context = context["executionContextId"]
        .as_u64()
        .ok_or_else(|| error("PDF_PROTOCOL", "文档等待环境无效", false))?;
    if !job.allow_javascript {
        // CSP响应头仍是script-src none+sandbox；仅允许宿主在隔离world读取加载状态。
        live.call(
            Some(&sid),
            "Emulation.setScriptExecutionDisabled",
            json!({"value":false}),
            deadline,
            &cancel,
            false,
        )?;
    }
    let image_urls: Vec<&str> = resources
        .responses
        .iter()
        .filter(|(_, response)| response.mime.to_ascii_lowercase().starts_with("image/"))
        .map(|(url, _)| url.as_str())
        .collect();
    let image_urls = serde_json::to_string(&image_urls)
        .map_err(|_| error("PDF_PROTOCOL", "冻结图片列表无法编码", false))?;
    let expression = format!("{ASSET_READY}({image_urls})");
    let ready = live.call(Some(&sid), "Runtime.evaluate",
        json!({"contextId":context,"expression":expression,"awaitPromise":true,"returnByValue":true}),
        deadline, &cancel, false)?;
    if ready.get("exceptionDetails").is_some()
        || ready["result"]["subtype"] == "error"
        || ready["result"]["value"]["ready"] != true
        || ready["result"]["value"]["url"] != job.base_url
    {
        return Err(error(
            "PDF_ASSETS_NOT_READY",
            "文档字体或图片尚未成功加载",
            false,
        ));
    }
    if !job.allow_javascript {
        live.call(
            Some(&sid),
            "Emulation.setScriptExecutionDisabled",
            json!({"value":true}),
            deadline,
            &cancel,
            false,
        )?;
    }
    same_document(live, resources, &sid, loader)?;
    let settings = &job.settings;
    let printed = live.call(Some(&sid), "Page.printToPDF", json!({
        "transferMode":"ReturnAsStream", "landscape":settings.landscape,
        "printBackground":settings.print_background,"preferCSSPageSize":settings.prefer_css_page_size,
        "paperWidth":settings.paper_width,"paperHeight":settings.paper_height,
        "marginTop":settings.margin_top,"marginBottom":settings.margin_bottom,
        "marginLeft":settings.margin_left,"marginRight":settings.margin_right,
        "pageRanges":settings.page_ranges,"scale":settings.scale,
        "displayHeaderFooter":false
    }), deadline, &cancel, false)?;
    let handle = printed["stream"]
        .as_str()
        .filter(|handle| {
            !handle.is_empty() && handle.len() <= 512 && !handle.chars().any(char::is_control)
        })
        .ok_or_else(|| error("PDF_STREAM_UNAVAILABLE", "打印未返回真实有界输出流", false))?
        .to_owned();
    let bytes = read_pdf(live, &sid, handle, resources)?;
    same_document(live, resources, &sid, loader)?;
    let version = live.call(
        None,
        "Browser.getVersion",
        json!({}),
        deadline,
        &cancel,
        false,
    )?;
    let product = version["product"]
        .as_str()
        .filter(|v| !v.is_empty() && v.len() <= 128 && !v.chars().any(char::is_control))
        .ok_or_else(|| error("PDF_PROTOCOL", "打印引擎未返回真实版本", false))?;
    resources.check(live)?;
    Ok(PdfRendered {
        sha256: lingxi_protocol::canon::sha256_hex(&bytes),
        bytes,
        renderer: format!("Chromium CDP ({product})"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use lingxi_kernel::browser::PdfAsset;

    fn job() -> PdfRenderJob {
        PdfRenderJob {
            html: "<!doctype html><p>正文</p>".into(),
            base_url: "https://lingxi-pdf.invalid/job/index.html".into(),
            assets: vec![],
            settings: PdfPrintSettings {
                landscape: false,
                print_background: true,
                prefer_css_page_size: true,
                paper_width: 8.27,
                paper_height: 11.69,
                margin_top: 0.0,
                margin_bottom: 0.0,
                margin_left: 0.0,
                margin_right: 0.0,
                page_ranges: String::new(),
                scale: 1.0,
            },
            allow_javascript: false,
            viewport_width: 1280,
            viewport_height: 900,
            settle_ms: 0,
            timeout_ms: 1000,
        }
    }
    fn frozen(mut job: PdfRenderJob) -> Result<Resources, BrowserError> {
        freeze(
            &mut job,
            Arc::new(AtomicBool::new(false)),
            Instant::now() + Duration::from_secs(1),
            DenyProxy::new()?,
        )
    }
    #[test]
    fn frozen_assets_require_exact_owned_url_and_finite_print_settings() {
        let mut valid = job();
        valid.assets.push(PdfAsset {
            url: "https://lingxi-pdf.invalid/job/asset/logo".into(),
            mime_type: "image/png".into(),
            bytes: vec![1, 2, 3],
        });
        let frozen = frozen(valid.clone()).unwrap();
        assert_eq!(frozen.responses.len(), 2);
        for url in [
            "https://elsewhere.invalid/image",
            "file:///tmp/secret",
            "https://lingxi-pdf.invalid/other/logo",
            "https://lingxi-pdf.invalid/job/asset/logo#fragment",
        ] {
            let mut bad = valid.clone();
            bad.assets[0].url = url.into();
            assert!(self::frozen(bad).is_err());
        }
        let mut bad = valid.clone();
        bad.assets.push(bad.assets[0].clone());
        assert!(self::frozen(bad).is_err());
        let mut bad = valid.clone();
        bad.assets[0].mime_type = "image/png\r\nInjected: true".into();
        assert!(self::frozen(bad).is_err());
        let mut bad = valid.clone();
        bad.settings.paper_width = f64::NAN;
        assert!(self::frozen(bad).is_err());
        let mut bad = valid;
        bad.settings.margin_left = 8.27;
        assert!(self::frozen(bad).is_err());
    }
    #[test]
    fn frozen_csp_blocks_network_workers_frames_and_page_javascript_when_disabled() {
        for allow in [false, true] {
            let policy = csp(allow);
            for required in [
                "connect-src 'none'",
                "worker-src 'none'",
                "frame-src 'none'",
                "object-src 'none'",
                "base-uri 'none'",
                "form-action 'none'",
            ] {
                assert!(policy.contains(required));
            }
            assert!(!policy.contains("ws:"));
            assert!(!policy.contains("allow-same-origin"));
            assert_eq!(policy.contains("allow-scripts"), allow);
            assert_eq!(policy.contains("script-src 'none'"), !allow);
        }
    }
    #[test]
    fn pdf_output_requires_real_header_trailer_and_byte_budget() {
        assert!(validate_pdf(b"%PDF-1.7\nreal bytes\n%%EOF\n").is_ok());
        assert!(validate_pdf(b"not a pdf even with %%EOF").is_err());
        assert!(validate_pdf(b"%PDF-1.7\ntruncated content").is_err());
        let mut too_large = vec![b' '; MAX_PDF + 1];
        too_large[..5].copy_from_slice(b"%PDF-");
        let len = too_large.len();
        too_large[len - 5..].copy_from_slice(b"%%EOF");
        assert!(validate_pdf(&too_large).is_err());
    }
}
