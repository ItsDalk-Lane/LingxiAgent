//! 五平台 SDK 子进程的受控管道；身份、运行和回执的权威保留在 Rust。
use serde_json::Value;
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// 仅可信组合根可选择程序；消息和 HTTP 请求不能提供命令或环境。
#[derive(Clone, Debug)]
pub struct BridgeWorkerLaunch {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}
impl BridgeWorkerLaunch {
    /// 程序路径来自受信任的安装包/启动环境，绝不读取消息中的路径。
    pub fn discover() -> Option<Self> {
        let explicit_node = std::env::var_os("LINGXI_BRIDGE_NODE").map(PathBuf::from);
        let explicit_worker = std::env::var_os("LINGXI_BRIDGE_WORKER").map(PathBuf::from);
        if explicit_node.is_some() || explicit_worker.is_some() {
            let (program, worker) = (explicit_node?, explicit_worker?);
            if !worker.is_absolute() || !worker.is_file() {
                return None;
            }
            let launch = Self {
                program,
                args: vec![worker.to_string_lossy().into_owned()],
                cwd: worker.parent()?.to_path_buf(),
            };
            return launch.validate().ok().map(|()| launch);
        }
        // 开发环境使用仓库内既有 Node SDK；安装包由桌面提供经过包验证的路径。
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize()
            .ok()?;
        let worker = root.join("scripts/bridge-sdk-worker.ts");
        if !worker.is_file() {
            return None;
        }
        let paths = std::env::var_os("PATH")?;
        let program = std::env::split_paths(&paths)
            .map(|path| path.join(if cfg!(windows) { "node.exe" } else { "node" }))
            .find(|path| path.is_absolute() && path.is_file())?
            .canonicalize()
            .ok()?;
        Some(Self {
            program,
            args: vec![worker.to_string_lossy().into_owned()],
            cwd: root,
        })
    }
    pub fn diagnostic(&self) -> Result<Self, String> {
        let mut launch = self.clone();
        let script = PathBuf::from(
            launch
                .args
                .last()
                .ok_or("Bridge diagnostic entry is unavailable")?,
        );
        let name = match script.file_name().and_then(|name| name.to_str()) {
            Some("bridge-sdk-worker.ts") => "bridge-sdk-diagnostic.ts",
            Some("bridge-sdk-worker.mjs") => "bridge-sdk-diagnostic.mjs",
            _ => return Err("Bridge diagnostic entry is unavailable".into()),
        };
        let script = script.with_file_name(name);
        if !script.is_file() {
            return Err("Bridge diagnostic entry is unavailable".into());
        }
        *launch
            .args
            .last_mut()
            .ok_or("Bridge diagnostic entry is unavailable")? =
            script.to_string_lossy().into_owned();
        Ok(launch)
    }
    pub fn validate(&self) -> Result<(), String> {
        if !self.program.is_absolute()
            || !self.program.is_file()
            || !self.cwd.is_absolute()
            || !self.cwd.is_dir()
        {
            return Err(
                "Bridge worker requires an existing absolute executable and directory".into(),
            );
        }
        Ok(())
    }
}

pub const MAX_FRAME_BYTES: usize = 256 * 1024;
const PIPE_WRITE_TIMEOUT: Duration = Duration::from_secs(5);
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(3);

/// 缓冲区属于进程；select 取消读取时不会丢失已读的半行。
struct FrameReader {
    pipe: tokio::process::ChildStdout,
    pending: Vec<u8>,
}
impl FrameReader {
    async fn next(&mut self) -> Result<Option<Value>, String> {
        loop {
            if let Some(end) = self.pending.iter().position(|b| *b == b'\n') {
                let line: Vec<_> = self.pending.drain(..=end).collect();
                return serde_json::from_slice(&line)
                    .map(Some)
                    .map_err(|_| "Bridge worker emitted malformed JSON".into());
            }
            if self.pending.len() >= MAX_FRAME_BYTES {
                return Err("Bridge worker frame exceeds its byte limit".into());
            }
            let mut bytes = [0_u8; 8192];
            let remaining = (MAX_FRAME_BYTES - self.pending.len()).min(bytes.len());
            let count = self
                .pipe
                .read(&mut bytes[..remaining])
                .await
                .map_err(|_| "Bridge worker pipe read failed")?;
            if count == 0 {
                return if self.pending.is_empty() {
                    Ok(None)
                } else {
                    Err("Bridge worker closed with an incomplete frame".into())
                };
            }
            self.pending.extend_from_slice(&bytes[..count]);
        }
    }
}

/// 宿主循环独占子进程。任何异常出口都会终止自己创建的进程组并保留清理观察。
pub struct BridgeWorkerProcess {
    child: Option<tokio::process::Child>,
    input: Option<tokio::process::ChildStdin>,
    reader: FrameReader,
    dispatch_sequence: u32,
}
impl BridgeWorkerProcess {
    pub async fn spawn(launch: &BridgeWorkerLaunch, init: &Value) -> Result<Self, String> {
        launch.validate()?;
        // 当前正式回收机制依赖 Unix 的进程组；其他平台不能假称已受控。
        #[cfg(not(unix))]
        {
            let _ = init;
            return Err(
                "Bridge worker process-group cleanup is not implemented on this platform".into(),
            );
        }
        #[cfg(unix)]
        {
            let mut command = tokio::process::Command::new(&launch.program);
            command
                .args(&launch.args)
                .current_dir(&launch.cwd)
                .env_clear()
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .process_group(0);
            // 不传 HOME、NODE_OPTIONS、代理凭据或服务令牌。连接所需凭据只经初始化管道传入。
            for key in ["PATH", "LANG", "LC_ALL", "LC_CTYPE", "TZ"] {
                if let Ok(value) = std::env::var(key) {
                    command.env(key, value);
                }
            }
            let mut child = command
                .spawn()
                .map_err(|_| "Bridge worker could not start")?;
            let input = child
                .stdin
                .take()
                .ok_or("Bridge worker stdin unavailable")?;
            let pipe = child
                .stdout
                .take()
                .ok_or("Bridge worker stdout unavailable")?;
            let mut owned = Self {
                child: Some(child),
                input: Some(input),
                reader: FrameReader {
                    pipe,
                    pending: Vec::new(),
                },
                dispatch_sequence: 0,
            };
            if let Err(error) = owned.send(init).await {
                let _ = owned.stop().await;
                return Err(error);
            }
            Ok(owned)
        }
    }
    pub async fn send(&mut self, frame: &Value) -> Result<(), String> {
        let mut frame = frame.clone();
        let sequence = if matches!(frame["kind"].as_str(), Some("send" | "effect")) {
            let sequence = self
                .dispatch_sequence
                .checked_add(1)
                .ok_or("Bridge pipe dispatch sequence exhausted")?;
            frame["dispatchSeq"] = serde_json::json!(sequence);
            Some(sequence)
        } else {
            None
        };
        let mut bytes = serde_json::to_vec(&frame).map_err(|_| "Bridge command encoding failed")?;
        if bytes.len() >= MAX_FRAME_BYTES {
            return Err("Bridge command exceeds its byte limit".into());
        }
        bytes.push(b'\n');
        // 有效帧开始写入后序号不再复用；半写/超时只能按未知发送结果清理。
        if let Some(sequence) = sequence {
            self.dispatch_sequence = sequence;
        }
        let input = self
            .input
            .as_mut()
            .ok_or("Bridge worker input already closed")?;
        tokio::time::timeout(PIPE_WRITE_TIMEOUT, input.write_all(&bytes))
            .await
            .map_err(|_| "Bridge worker command timed out")?
            .map_err(|_| "Bridge worker command pipe closed".to_string())
    }
    pub async fn next(&mut self) -> Result<Option<Value>, String> {
        self.reader.next().await
    }
    pub fn process_id(&self) -> Option<u32> {
        self.child.as_ref().and_then(|child| child.id())
    }
    pub async fn stop(&mut self) -> Result<(), String> {
        self.input.take();
        let Some(child) = self.child.as_mut() else {
            return Ok(());
        };
        // try_wait 一旦真实回收便不再发送组信号，避免 PID 复用。
        match child.try_wait() {
            Ok(Some(_)) => {
                self.child.take();
                return Ok(());
            }
            Ok(None) => kill_group(child),
            Err(_) => {
                kill_group(child);
            }
        }
        match tokio::time::timeout(CLEANUP_TIMEOUT, child.wait()).await {
            Ok(Ok(_)) => {
                self.child.take();
                Ok(())
            }
            _ => Err("Bridge worker cleanup remains unconfirmed".into()),
        }
    }
}
fn kill_group(child: &mut tokio::process::Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        // 只持有尚未回收的自有 child；没有按外部 PID 认领进程的入口。
        unsafe {
            libc::killpg(pid as i32, libc::SIGKILL);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = child.start_kill();
    }
}
impl Drop for BridgeWorkerProcess {
    fn drop(&mut self) {
        self.input.take();
        let Some(mut child) = self.child.take() else {
            return;
        };
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        kill_group(&mut child);
        // 调用方 future 被取消也不能放弃实际 wait；正常路径必须显式 stop 并核验结果。
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                if !matches!(
                    tokio::time::timeout(CLEANUP_TIMEOUT, child.wait()).await,
                    Ok(Ok(_))
                ) {
                    tracing::error!("Bridge worker drop cleanup remains unconfirmed");
                }
            });
        } else {
            tracing::error!("Bridge worker cleanup could not be observed without an async runtime");
        }
    }
}
