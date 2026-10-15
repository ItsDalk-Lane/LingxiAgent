//! 原生ls/find/grep：真实授权目录/文件，同一正式网关与Run消费者，纯读取且无文件产物登记。
use crate::{
    resourceaccess::{ResourceAccess, ResourceOp},
    toolgateway::ToolInvocationGateway,
};
use lingxi_kernel::{
    invocation::ToolRecoveryCapability,
    ports::{ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest},
    toolcatalog::{
        Availability, DeclaredPermission, PermissionContract, PermissionKind, SchemaBudget,
        ToolManifest, ToolOrigin, ToolRegistry,
    },
    RunContext,
};
use lingxi_protocol::{ErrorCode, ProtocolError, ToolCallId, ToolSchemaDocument};
use serde_json::{json, Value};
use std::{path::PathBuf, pin::Pin, sync::Arc};

#[derive(Clone, Copy)]
enum Kind {
    Ls,
    Find,
    Grep,
}
impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Ls => "ls",
            Self::Find => "find",
            Self::Grep => "grep",
        }
    }
}
fn failure(code: ErrorCode, message: impl Into<String>) -> ToolOutcome {
    ToolOutcome::Failed {
        error: ProtocolError::new(code, message.into(), false),
    }
}

/// Windows句柄路径的扩展前缀不属于用户glob的盘符或UNC路径正文。
#[cfg(any(windows, test))]
fn windows_glob_path(text: &str) -> String {
    let normalized = text.replace('\\', "/");
    if normalized
        .get(..8)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("//?/UNC/"))
    {
        format!("//{}", &normalized[8..])
    } else if let Some(rest) = normalized.strip_prefix("//?/") {
        rest.to_owned()
    } else {
        normalized
    }
}

/// 不改变R04固定三工具注册函数；组合根另外登记实际发现工具。
pub fn register_discovery_tools(
    registry: &ToolRegistry,
    gateway: &ToolInvocationGateway,
    access: Arc<ResourceAccess>,
    cwd: PathBuf,
    budget: &SchemaBudget,
) {
    for kind in [Kind::Ls, Kind::Find, Kind::Grep] {
        let mut props = json!({"path":{"type":"string","minLength":1},"limit":{"type":"integer","minimum":1,"maximum":10000}});
        if !matches!(kind, Kind::Ls) {
            props["pattern"] = json!({"type":"string","maxLength":4096});
        }
        if matches!(kind, Kind::Grep) {
            props["glob"] = json!({"type":"string","maxLength":4096});
            props["literal"] = json!({"type":"boolean"});
            props["ignoreCase"] = json!({"type":"boolean"});
            props["context"] = json!({"type":"integer","minimum":0,"maximum":1000});
            props["offset"] = json!({"type":"integer","minimum":0,"maximum":100000});
            props["output_mode"] = json!({"type":"string","enum":["content","files","count"]});
        }
        let mut schema = json!({"type":"object","properties":props,"additionalProperties":false});
        if !matches!(kind, Kind::Ls) {
            schema["required"] = json!(["pattern"]);
        }
        let availability = if cfg!(any(target_os = "macos", target_os = "linux", windows)) {
            Availability::Available
        } else {
            Availability::Disabled {
                reason: "safe native directory handles are unavailable on this platform".into(),
            }
        };
        let manifest=ToolManifest{
            origin:ToolOrigin::FirstParty,local_name:kind.name().into(),display_name:kind.name().into(),aliases:Vec::new(),version:"1.0.0".into(),
            description:match kind {Kind::Ls=>"List authorized directory entries and metadata, including dotfiles; read-only.",Kind::Find=>"Find authorized paths by glob, relative to search root. Skips .git and node_modules; read-only.",Kind::Grep=>"Search authorized UTF-8 text with Rust regex or literal matching. Reports line/column/byte positions; content/files/count, context and pagination. Read-only."}.into(),
            input_schema:ToolSchemaDocument{dialect:"json-schema/2020-12".into(),schema},output_schema:None,
            permission:PermissionContract{kind:PermissionKind::Read,capability_base:format!("{}.files",kind.name())},availability,timeout_ms:Some(60_000),max_concurrency:Some(4),declared_permission:DeclaredPermission::ReadOnly,
            recovery:ToolRecoveryCapability{read_only:true,honors_idempotency_key:false,externally_verifiable:false},
        };
        let target = registry
            .register(manifest, budget)
            .expect("native discovery manifest registers")
            .target_id;
        let extraction_access = access.clone();
        let extraction_cwd = cwd.clone();
        let extractor = Arc::new(
            move |input: &crate::toolgateway::ResourceExtractionInput,
                  args: &lingxi_kernel::toolcatalog::EffectiveArguments| {
                let raw = args.as_value()["path"].as_str().unwrap_or(".");
                let actual_cwd =
                    crate::toolgateway::workspace().unwrap_or_else(|| extraction_cwd.clone());
                extraction_access
                    .authorize(
                        input.principal_kind,
                        &input.principal_subject,
                        &input.session_id,
                        std::path::Path::new(raw),
                        &actual_cwd,
                        ResourceOp::Read,
                    )
                    .map(|scope| vec![scope])
                    .map_err(|_| "NATIVE_DISCOVERY_PATH_NOT_AUTHORIZED".to_string())
            },
        );
        gateway.bind_executor_with_resources(
            target,
            Arc::new(Executor {
                kind,
                access: access.clone(),
                cwd: cwd.clone(),
            }),
            Some(extractor),
            "native authorized read-only discovery",
        );
    }
}
struct Executor {
    kind: Kind,
    access: Arc<ResourceAccess>,
    cwd: PathBuf,
}
impl ToolExecutorPort for Executor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ToolCallId,
        request: &'a ToolRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        let ctx = ctx.clone();
        let kind = self.kind;
        let access = self.access.clone();
        let cwd = crate::toolgateway::workspace().unwrap_or_else(|| self.cwd.clone());
        let args = request.arguments.as_value().clone();
        Box::pin(async move {
            #[cfg(any(target_os = "macos", target_os = "linux", windows))]
            let outcome = {
                let permit = match crate::task_supervisor::retain_blocking_run_permit(&ctx) {
                    Ok(p) => p,
                    Err(_) => {
                        return ToolExecutionResult::of_ctx(
                            &ctx,
                            failure(ErrorCode::Conflict, "native discovery permit mismatch"),
                        )
                    }
                };
                let cancel = match crate::task_supervisor::blocking_run_cancel_scope(&ctx) {
                    Ok(c) => c,
                    Err(_) => {
                        return ToolExecutionResult::of_ctx(
                            &ctx,
                            failure(
                                ErrorCode::Conflict,
                                "native discovery cancellation scope mismatch",
                            ),
                        )
                    }
                };
                let work = ctx.clone();
                tokio::task::spawn_blocking(move || {
                    let _physical_permit = permit;
                    native::execute(kind, access, cwd, &work, &args, cancel)
                })
                .await
                .unwrap_or_else(|_| failure(ErrorCode::Internal, "native discovery worker failed"))
            };
            #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
            let outcome = {
                let _ = (kind, access, cwd, args);
                failure(
                    ErrorCode::UpstreamUnavailable,
                    "safe native discovery is unavailable on this platform",
                )
            };
            ToolExecutionResult::of_ctx(&ctx, outcome)
        })
    }
}

#[cfg(windows)]
#[path = "discovery_windows.rs"]
mod windows_readonly;

#[cfg(any(target_os = "macos", target_os = "linux", windows))]
mod native {
    #[cfg(windows)]
    use super::windows_readonly::{file_identity, DirFd, EntryMetadata};
    use super::*;
    use crate::cancel::CancelScope;
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    use crate::filetools::fsio::{file_identity, DirFd, EntryMetadata};
    use lingxi_kernel::ports::ToolSuccess;
    use regex::{Regex, RegexBuilder};
    use std::{
        collections::{BTreeMap, BTreeSet},
        ffi::OsStr,
        io::Read,
        path::{Path, PathBuf},
    };
    const MAX_ENTRIES: usize = 100_000;
    const MAX_FILE: usize = 16 * 1024 * 1024;
    const MAX_LINES: usize = 200_000;
    // 正文与相同结构化字段合计仍在既有32KiB模型投影预算内，不靠后处理静默裁剪JSON。
    const OUTPUT_BYTES: usize = 14 * 1024;
    type Result<T> = std::result::Result<T, ToolOutcome>;
    struct Engine<'a> {
        access: Arc<ResourceAccess>,
        cwd: PathBuf,
        ctx: &'a RunContext,
        cancel: Option<CancelScope>,
        visited: usize,
        directories: BTreeSet<(u64, u128)>,
        skipped_cycles: usize,
        skipped_binary: usize,
    }
    impl Engine<'_> {
        fn check(&self) -> Result<()> {
            if self.cancel.as_ref().is_some_and(CancelScope::is_cancelled) {
                Err(ToolOutcome::Cancelled)
            } else {
                Ok(())
            }
        }
        fn authorize(&self, path: &Path) -> Result<PathBuf> {
            self.check()?;
            self.access
                .authorize(
                    self.ctx.principal.storage_kind(),
                    &self.ctx.principal.storage_subject(),
                    self.ctx.session_id.as_str(),
                    path,
                    &self.cwd,
                    ResourceOp::Read,
                )
                .map(|s| s.path)
                .map_err(|_| failure(ErrorCode::Forbidden, "NATIVE_DISCOVERY_PATH_NOT_AUTHORIZED"))
        }
        fn io<T>(&self, op: &str, value: std::io::Result<T>) -> Result<T> {
            self.check()?;
            value.map_err(|error| {
                failure(
                    if error.kind() == std::io::ErrorKind::NotFound {
                        ErrorCode::NotFound
                    } else {
                        ErrorCode::Internal
                    },
                    format!(
                        "NATIVE_DISCOVERY_{op}_FAILED: {:?} (os={:?})",
                        error.kind(),
                        error.raw_os_error()
                    ),
                )
            })
        }
        fn verify(&self, path: &Path, dir: &DirFd) -> Result<()> {
            self.authorize(path)?;
            if !self.io("DIRECTORY_IDENTITY", dir.same_directory(path))? {
                return Err(failure(
                    ErrorCode::Conflict,
                    "NATIVE_DISCOVERY_DIRECTORY_CHANGED",
                ));
            }
            Ok(())
        }
        fn directory(&self, path: &Path) -> Result<DirFd> {
            self.io("OPEN_DIRECTORY", DirFd::open(path))
        }
        fn verify_parent(&self, path: &Path, dir: &DirFd) -> Result<()> {
            if !self.io("DIRECTORY_IDENTITY", dir.same_directory(path))? {
                return Err(failure(
                    ErrorCode::Conflict,
                    "NATIVE_DISCOVERY_DIRECTORY_CHANGED",
                ));
            }
            Ok(())
        }
        fn names(&mut self, path: &Path, dir: &DirFd) -> Result<Vec<std::ffi::OsString>> {
            self.verify(path, dir)?;
            let mut names = self.io(
                "READ_DIRECTORY",
                dir.entry_names(|| self.cancel.as_ref().is_some_and(CancelScope::is_cancelled)),
            )?;
            self.visited = self
                .visited
                .checked_add(names.len())
                .ok_or_else(|| failure(ErrorCode::Internal, "native scan counter overflow"))?;
            if self.visited > MAX_ENTRIES {
                return Err(failure(
                    ErrorCode::InvalidMessage,
                    "NATIVE_DISCOVERY_SCAN_BUDGET_EXCEEDED",
                ));
            }
            for name in &names {
                if name.to_str().is_none() {
                    return Err(failure(
                        ErrorCode::InvalidMessage,
                        "NATIVE_DISCOVERY_NON_UTF8_NAME",
                    ));
                }
            }
            names.sort_by(|a, b| {
                a.to_str()
                    .unwrap()
                    .to_lowercase()
                    .cmp(&b.to_str().unwrap().to_lowercase())
                    .then(a.cmp(b))
            });
            self.verify(path, dir)?;
            Ok(names)
        }
        fn entry(
            &self,
            path: &Path,
            dir: &DirFd,
            name: &OsStr,
        ) -> Result<(PathBuf, EntryMetadata)> {
            self.verify(path, dir)?;
            let before = self.io("STAT_ENTRY", dir.entry_metadata(name))?;
            let target = self.authorize(&path.join(name))?;
            let parent = target
                .parent()
                .ok_or_else(|| failure(ErrorCode::InvalidMessage, "native entry has no parent"))?;
            // 已授权且仍为同一父目录时复用持有句柄，避免深目录每项再次从根打开整条链。
            // 原入口/返回前的目录身份检查和目标重新授权继续保留。
            let meta = if parent == path && before.kind != "symlink" {
                self.io(
                    "STAT_ENTRY",
                    dir.entry_metadata(target.file_name().unwrap()),
                )?
            } else {
                let actual_dir = self.directory(parent)?;
                self.io(
                    "STAT_ENTRY",
                    actual_dir.entry_metadata(target.file_name().unwrap()),
                )?
            };
            if before.kind != "symlink" && before != meta {
                return Err(failure(
                    ErrorCode::Conflict,
                    "NATIVE_DISCOVERY_ENTRY_CHANGED",
                ));
            }
            if self.io("STAT_ENTRY", dir.entry_metadata(name))? != before {
                return Err(failure(
                    ErrorCode::Conflict,
                    "NATIVE_DISCOVERY_LINK_CHANGED",
                ));
            }
            self.verify(path, dir)?;
            Ok((target, meta))
        }
        fn read(&mut self, path: &Path) -> Result<Option<(String, String)>> {
            let target = self.authorize(path)?;
            let parent = target
                .parent()
                .ok_or_else(|| failure(ErrorCode::InvalidMessage, "native file has no parent"))?;
            let dir = self.directory(parent)?;
            let name = target.file_name().unwrap();
            let before = self.io("STAT_FILE", dir.entry_metadata(name))?;
            if before.kind != "file" {
                return Err(failure(
                    ErrorCode::InvalidMessage,
                    "NATIVE_GREP_NON_REGULAR_FILE",
                ));
            }
            let mut file = self.io(
                "OPEN_FILE",
                dir.open_attachment_nofollow(name)
                    .unwrap_or_else(|| Err(std::io::Error::from(std::io::ErrorKind::NotFound))),
            )?;
            let opened = self.io("STAT_OPEN_FILE", file.metadata())?;
            if !opened.is_file()
                || self.io("OPEN_FILE_IDENTITY", file_identity(&file))?
                    != (before.device, before.inode)
            {
                return Err(failure(
                    ErrorCode::Conflict,
                    "NATIVE_DISCOVERY_FILE_CHANGED",
                ));
            }
            let mut bytes = Vec::new();
            let mut buffer = [0u8; 64 * 1024];
            loop {
                self.check()?;
                let got = self.io("READ_FILE", file.read(&mut buffer))?;
                if got == 0 {
                    break;
                }
                if bytes.len() + got > MAX_FILE {
                    return Err(failure(
                        ErrorCode::InvalidMessage,
                        "NATIVE_GREP_FILE_BUDGET_EXCEEDED",
                    ));
                }
                bytes.extend_from_slice(&buffer[..got]);
            }
            if self.io("STAT_FILE", dir.entry_metadata(name))? != before {
                return Err(failure(
                    ErrorCode::Conflict,
                    "NATIVE_DISCOVERY_FILE_CHANGED",
                ));
            }
            self.verify_parent(parent, &dir)?;
            self.authorize(&target)?;
            if bytes.contains(&0) {
                self.skipped_binary += 1;
                return Ok(None);
            }
            use sha2::{Digest, Sha256};
            let digest = Sha256::digest(&bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            match String::from_utf8(bytes) {
                Ok(text) => Ok(Some((text, digest))),
                Err(_) => {
                    self.skipped_binary += 1;
                    Ok(None)
                }
            }
        }
        fn walk(
            &mut self,
            current: &Path,
            relative: &Path,
            visit: &mut impl FnMut(&mut Self, &Path, &Path, &EntryMetadata) -> Result<()>,
        ) -> Result<()> {
            // 遍历帧放在堆上，不能用合法深目录耗尽阻塞线程栈。
            // 帧只保留路径、身份和剩余名称；目录句柄不跨层保留。
            struct Frame {
                path: PathBuf,
                relative: PathBuf,
                identity: (u64, u128),
                names: std::vec::IntoIter<std::ffi::OsString>,
            }
            let mut stack = Vec::<Frame>::new();
            let mut enter = Some((current.to_path_buf(), relative.to_path_buf(), None));
            loop {
                self.check()?;
                let mut entered_dir = None;
                if let Some((path, relative, expected)) = enter.take() {
                    let canonical = self.authorize(&path)?;
                    let dir = self.directory(&canonical)?;
                    let identity = self.io("DIRECTORY_IDENTITY", dir.directory_identity())?;
                    if expected.is_some_and(|expected| expected != identity) {
                        return Err(failure(
                            ErrorCode::Conflict,
                            "NATIVE_DISCOVERY_DIRECTORY_CHANGED",
                        ));
                    }
                    if self.directories.insert(identity) {
                        let names = self.names(&canonical, &dir)?;
                        // 刚进入的同一帧已经逐项授权/核对身份，首项沿用该句柄。
                        entered_dir = Some(dir);
                        stack.push(Frame {
                            path: canonical,
                            relative,
                            identity,
                            names: names.into_iter(),
                        });
                    } else {
                        self.skipped_cycles += 1;
                    }
                }
                let Some(frame) = stack.last_mut() else { break };
                let name = frame.names.next();
                let path = frame.path.clone();
                let relative = frame.relative.clone();
                let expected = frame.identity;
                let (canonical, dir) = match entered_dir {
                    Some(dir) => (path.clone(), dir),
                    None => {
                        let canonical = self.authorize(&path)?;
                        let dir = self.directory(&canonical)?;
                        (canonical, dir)
                    }
                };
                if self.io("DIRECTORY_IDENTITY", dir.directory_identity())? != expected {
                    return Err(failure(
                        ErrorCode::Conflict,
                        "NATIVE_DISCOVERY_DIRECTORY_CHANGED",
                    ));
                }
                if let Some(name) = name {
                    if name == OsStr::new(".git") || name == OsStr::new("node_modules") {
                        continue;
                    }
                    let (target, meta) = self.entry(&canonical, &dir, &name)?;
                    let relative = relative.join(&name);
                    visit(self, &target, &relative, &meta)?;
                    self.verify(&canonical, &dir)?;
                    if meta.kind == "directory" {
                        enter = Some((target, relative, Some((meta.device, meta.inode))));
                    }
                } else {
                    self.verify(&canonical, &dir)?;
                    stack.pop();
                }
            }
            Ok(())
        }
    }

    // 当前执行平台为Unix，反斜杠是合法文件名字符，必须原样保留以便回指。
    fn path_text(path: &Path) -> Result<String> {
        path.to_str()
            .map(|text| {
                #[cfg(windows)]
                {
                    super::windows_glob_path(text)
                }
                #[cfg(not(windows))]
                {
                    text.to_owned()
                }
            })
            .ok_or_else(|| failure(ErrorCode::InvalidMessage, "NATIVE_DISCOVERY_NON_UTF8_NAME"))
    }
    fn glob(pattern: &str, ignore_case: bool) -> Result<Regex> {
        #[cfg(windows)]
        let windows_pattern = super::windows_glob_path(pattern);
        #[cfg(windows)]
        let pattern = windows_pattern.as_str();
        let mut output = String::from("^");
        let mut chars = pattern.chars().peekable();
        let mut braces = 0;
        while let Some(c) = chars.next() {
            match c {
                '*' => {
                    if chars.peek() == Some(&'*') {
                        chars.next();
                        if chars.peek() == Some(&'/') {
                            chars.next();
                            output.push_str("(?:.*/)?");
                        } else {
                            output.push_str(".*");
                        }
                    } else {
                        output.push_str("[^/]*");
                    }
                }
                '?' => output.push_str("[^/]"),
                '[' => {
                    output.push('[');
                    if chars.peek() == Some(&'!') {
                        chars.next();
                        output.push('^');
                    }
                    let mut closed = false;
                    for ch in chars.by_ref() {
                        output.push(ch);
                        if ch == ']' {
                            closed = true;
                            break;
                        }
                    }
                    if !closed {
                        return Err(failure(
                            ErrorCode::InvalidMessage,
                            "NATIVE_FIND_INVALID_GLOB",
                        ));
                    }
                }
                '{' => {
                    braces += 1;
                    output.push_str("(?:");
                }
                '}' => {
                    if braces == 0 {
                        return Err(failure(
                            ErrorCode::InvalidMessage,
                            "NATIVE_FIND_INVALID_GLOB",
                        ));
                    }
                    braces -= 1;
                    output.push(')');
                }
                ',' if braces > 0 => output.push('|'),
                '\\' => {
                    let escaped = chars.next().ok_or_else(|| {
                        failure(ErrorCode::InvalidMessage, "NATIVE_FIND_INVALID_GLOB")
                    })?;
                    output.push_str(&regex::escape(&escaped.to_string()));
                }
                _ => output.push_str(&regex::escape(&c.to_string())),
            }
        }
        if braces != 0 {
            return Err(failure(
                ErrorCode::InvalidMessage,
                "NATIVE_FIND_INVALID_GLOB",
            ));
        }
        output.push('$');
        RegexBuilder::new(&output)
            .case_insensitive(ignore_case)
            .build()
            .map_err(|_| failure(ErrorCode::InvalidMessage, "NATIVE_FIND_INVALID_GLOB"))
    }
    fn glob_matches(
        regex: &Regex,
        pattern: &str,
        relative: &Path,
        absolute: &Path,
    ) -> Result<bool> {
        #[cfg(windows)]
        let windows_pattern = super::windows_glob_path(pattern);
        #[cfg(windows)]
        let pattern = windows_pattern.as_str();
        let path = if Path::new(pattern).is_absolute() {
            path_text(absolute)?
        } else if pattern.contains('/') {
            path_text(relative)?
        } else {
            relative.file_name().unwrap().to_str().unwrap().into()
        };
        Ok(regex.is_match(&path))
    }
    struct Page {
        rows: Vec<Value>,
        total: usize,
        offset: usize,
        limit: usize,
        bytes: usize,
        byte_limited: bool,
    }
    impl Page {
        fn new(offset: usize, limit: usize) -> Self {
            Self {
                rows: Vec::new(),
                total: 0,
                offset,
                limit,
                bytes: 0,
                byte_limited: false,
            }
        }
        fn push(&mut self, row: impl FnOnce() -> Result<Value>) -> Result<()> {
            let at = self.total;
            self.total += 1;
            if at < self.offset || self.rows.len() >= self.limit || self.byte_limited {
                return Ok(());
            }
            let row = row()?;
            let bytes = serde_json::to_vec(&row)
                .map_err(|_| failure(ErrorCode::Internal, "native result serialization failed"))?
                .len();
            if self.bytes + bytes > OUTPUT_BYTES - 1024 {
                if self.rows.is_empty() {
                    return Err(failure(
                        ErrorCode::InvalidMessage,
                        "NATIVE_DISCOVERY_RESULT_ROW_BUDGET_EXCEEDED",
                    ));
                }
                self.byte_limited = true;
                return Ok(());
            }
            self.bytes += bytes;
            self.rows.push(row);
            Ok(())
        }
    }
    fn success(kind: Kind, key: &str, page: Page, engine: &Engine<'_>) -> ToolOutcome {
        let next = page.offset + page.rows.len();
        fn clipped(value: &Value) -> bool {
            match value {
                Value::Object(map) => map.iter().any(|(key, value)| {
                    key.ends_with("Truncated") && value == &Value::Bool(true) || clipped(value)
                }),
                Value::Array(rows) => rows.iter().any(clipped),
                _ => false,
            }
        }
        let truncated = next < page.total || page.rows.iter().any(clipped);
        let data = json!({"kind":kind.name(),key:page.rows,"total":page.total,"offset":page.offset,"nextOffset":if next < page.total {Some(next)}else{None},"truncated":truncated,"byteLimitReached":page.byte_limited,"skippedBinaryFiles":engine.skipped_binary,"skippedDuplicateDirectories":engine.skipped_cycles});
        let text = data.to_string();
        let mut result = ToolSuccess::text(text);
        result.details = data.as_object().cloned();
        result.truncated = truncated;
        ToolOutcome::Success { result }
    }
    pub(super) fn execute(
        kind: Kind,
        access: Arc<ResourceAccess>,
        cwd: PathBuf,
        ctx: &RunContext,
        args: &Value,
        cancel: Option<CancelScope>,
    ) -> ToolOutcome {
        let mut engine = Engine {
            access,
            cwd,
            ctx,
            cancel,
            visited: 0,
            directories: BTreeSet::new(),
            skipped_cycles: 0,
            skipped_binary: 0,
        };
        let result = (|| -> Result<ToolOutcome> {
            let raw = args["path"].as_str().unwrap_or(".");
            let root = engine.authorize(Path::new(raw))?;
            let limit = args["limit"].as_u64().unwrap_or(match kind {
                Kind::Ls => 500,
                Kind::Find => 1000,
                Kind::Grep => 100,
            }) as usize;
            let offset = args["offset"].as_u64().unwrap_or(0) as usize;
            let mut page = Page::new(offset, limit);
            if matches!(kind, Kind::Ls) {
                let dir = engine.directory(&root)?;
                for name in engine.names(&root, &dir)? {
                    let (_, meta) = engine.entry(&root, &dir, &name)?;
                    let name = name.to_str().unwrap().to_string();
                    page.push(||Ok(json!({"name":name,"isDirectory":meta.kind=="directory","type":meta.kind,"size":meta.size,"mtimeMs":meta.modified_ms})))?;
                }
                engine.verify(&root, &dir)?;
                return Ok(success(kind, "entries", page, &engine));
            }
            let pattern = args["pattern"].as_str().ok_or_else(|| {
                failure(
                    ErrorCode::InvalidMessage,
                    "native discovery requires pattern",
                )
            })?;
            if matches!(kind, Kind::Find) {
                #[cfg(windows)]
                let windows_pattern = super::windows_glob_path(pattern);
                #[cfg(windows)]
                let pattern = windows_pattern.as_str();
                let effective = if pattern.contains('/')
                    && !Path::new(pattern).is_absolute()
                    && !pattern.starts_with("**/")
                {
                    format!("**/{pattern}")
                } else {
                    pattern.into()
                };
                let matcher = glob(&effective, !pattern.chars().any(char::is_uppercase))?;
                engine.walk(
                    &root,
                    Path::new(""),
                    &mut |_engine, target, relative, meta| {
                        if glob_matches(&matcher, &effective, relative, target)? {
                            let mut path = path_text(relative)?;
                            if meta.kind == "directory" {
                                path.push('/');
                            }
                            page.push(|| Ok(Value::String(path)))?;
                        }
                        Ok(())
                    },
                )?;
                return Ok(success(kind, "paths", page, &engine));
            }
            let literal = args["literal"].as_bool().unwrap_or(false);
            let matcher = RegexBuilder::new(&if literal {
                regex::escape(pattern)
            } else {
                pattern.into()
            })
            .case_insensitive(args["ignoreCase"].as_bool().unwrap_or(false))
            .size_limit(2 * 1024 * 1024)
            .build()
            .map_err(|error| {
                failure(
                    ErrorCode::InvalidMessage,
                    format!("NATIVE_GREP_INVALID_REGEX: {error}"),
                )
            })?;
            let filter = args["glob"]
                .as_str()
                .map(|pattern| glob(pattern, false).map(|regex| (pattern, regex)))
                .transpose()?;
            let context = args["context"].as_u64().unwrap_or(0) as usize;
            if offset > 0 && context > 0 {
                return Err(failure(
                    ErrorCode::InvalidMessage,
                    "GREP_OFFSET_WITH_CONTEXT",
                ));
            }
            let mode = args["output_mode"].as_str().unwrap_or("content");
            let mut counts = BTreeMap::<String, (usize, i64)>::new();
            let mut visit = |engine: &mut Engine<'_>,
                             target: &Path,
                             relative: &Path,
                             meta: &EntryMetadata|
             -> Result<()> {
                if meta.kind == "directory" {
                    return Ok(());
                }
                if meta.kind != "file" {
                    return Err(failure(
                        ErrorCode::InvalidMessage,
                        "NATIVE_GREP_NON_REGULAR_FILE",
                    ));
                }
                if let Some((pattern, regex)) = &filter {
                    if !glob_matches(regex, pattern, relative, target)? {
                        return Ok(());
                    }
                }
                let Some((text, source_digest)) = engine.read(target)? else {
                    return Ok(());
                };
                let path = path_text(relative)?;
                let mut lines = Vec::new();
                let mut absolute_byte = 0usize;
                for (n, raw) in text.split_inclusive('\n').enumerate() {
                    if n >= MAX_LINES {
                        return Err(failure(
                            ErrorCode::InvalidMessage,
                            "NATIVE_GREP_LINE_BUDGET_EXCEEDED",
                        ));
                    }
                    let raw = raw.strip_suffix('\n').unwrap_or(raw);
                    let content = raw.strip_suffix('\r').unwrap_or(raw);
                    let bom = if n == 0 && content.starts_with('\u{feff}') {
                        3
                    } else {
                        0
                    };
                    lines.push((&content[bom..], absolute_byte + bom));
                    absolute_byte += raw.len()
                        + usize::from(
                            text.as_bytes().get(absolute_byte + raw.len()) == Some(&b'\n'),
                        );
                }
                for (index, (line, start_byte)) in lines.iter().enumerate() {
                    engine.check()?;
                    if !matcher.is_match(line) {
                        continue;
                    }
                    counts
                        .entry(path.clone())
                        .or_insert((0, meta.modified_ms))
                        .0 += 1;
                    if mode != "content" {
                        continue;
                    }
                    page.push(|| {
                        let mut positions=Vec::new();let mut positions_truncated=false;
                        for hit in matcher.find_iter(line) {engine.check()?;if positions.len()==64 {positions_truncated=true;break;}
                            positions.push(json!({"column":line[..hit.start()].chars().count()+1,"endColumn":line[..hit.end()].chars().count()+1,"byteStart":start_byte+hit.start(),"byteEnd":start_byte+hit.end(),"match":hit.as_str().chars().take(256).collect::<String>(),"matchTextTruncated":hit.as_str().chars().count()>256}));}
                        let surrounding=lines.iter().enumerate().take(if context == 0 {0} else {(index+context+1).min(lines.len())}).skip(index.saturating_sub(context)).map(|(n,(text,_))|json!({"line":n+1,"text":text.chars().take(500).collect::<String>(),"textTruncated":text.chars().count()>500})).collect::<Vec<_>>();
                        Ok(json!({"path":path,"sourceSha256":source_digest,"line":index+1,"text":line.chars().take(500).collect::<String>(),"textTruncated":line.chars().count()>500,"positions":positions,"positionsTruncated":positions_truncated,"context":surrounding}))
                    })?;
                }
                Ok(())
            };
            if let Some(name) = root.file_name() {
                let parent = root.parent().unwrap_or(Path::new("/"));
                let dir = engine.directory(parent)?;
                let meta = engine.io("STAT_ROOT", dir.entry_metadata(name))?;
                if meta.kind == "directory" {
                    engine.walk(&root, Path::new(""), &mut visit)?;
                } else {
                    let name = Path::new(raw).file_name().unwrap_or(name);
                    visit(&mut engine, &root, Path::new(name), &meta)?;
                }
            } else {
                engine.walk(&root, Path::new(""), &mut visit)?;
            }
            if mode != "content" {
                let mut rows = counts.into_iter().collect::<Vec<_>>();
                rows.sort_by(|(path_a, (count_a, time_a)), (path_b, (count_b, time_b))| {
                    if mode == "files" {
                        time_b.cmp(time_a)
                    } else {
                        count_b.cmp(count_a)
                    }
                    .then(path_a.cmp(path_b))
                });
                for (path, (count, _)) in rows {
                    page.push(|| {
                        Ok(if mode == "files" {
                            json!(path)
                        } else {
                            json!({"path":path,"count":count})
                        })
                    })?;
                }
            }
            engine.authorize(&root)?;
            engine.check()?;
            Ok(success(
                kind,
                if mode == "content" {
                    "matches"
                } else if mode == "files" {
                    "files"
                } else {
                    "counts"
                },
                page,
                &engine,
            ))
        })();
        result.unwrap_or_else(|outcome| outcome)
    }

    #[cfg(test)]
    mod windows_prefix_tests {
        use super::glob;
        use crate::discovery_tools::windows_glob_path;

        #[test]
        fn windows_canonical_prefix_preserves_ordinary_absolute_glob_matches() {
            // 纯路径语义回归，在本机执行；不冒充Windows句柄或运行验收。
            for (pattern, canonical) in [
                ("C:/root/*.txt", r"\\?\C:\root\a.txt"),
                (
                    "//server/share/root/*.txt",
                    r"\\?\UNC\server\share\root\a.txt",
                ),
                (r"\\?\C:\root\*.txt", "C:/root/a.txt"),
            ] {
                let matcher = glob(&windows_glob_path(pattern), false).unwrap();
                assert!(matcher.is_match(&windows_glob_path(canonical)));
                assert!(!matcher.is_match(&windows_glob_path("C:/other/private.txt")));
            }
            assert_eq!(windows_glob_path(r"sub\*.txt"), "sub/*.txt");
            assert_eq!(windows_glob_path("*.txt"), "*.txt");
        }
    }
}
