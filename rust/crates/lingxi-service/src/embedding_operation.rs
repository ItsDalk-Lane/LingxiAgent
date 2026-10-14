use super::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// 仅可信宿主构造；后台作业不捏造 Session 或 Run。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationOwnership {
    pub principal_kind: String,
    pub principal_subject: String,
    pub agent_id: Option<String>,
    pub session_id: Option<String>,
    pub run_id: Option<String>,
    pub attempt: Option<String>,
    pub parent_tool_call_id: Option<String>,
    pub cause_ref: String,
    pub deadline_unix_ms: u64,
    pub budget_ms: u64,
}

#[derive(Debug, Clone)]
pub struct EmbeddingCallRuntime {
    pub ownership: OperationOwnership,
    pub cancel: crate::cancel::CancelScope,
    /// 同一已付批次跨重启复用；身份指纹在 operation 内再次绑定。
    pub request_key: String,
    /// 仅可信手动重试入口授权当前真实尝试，不用于未知发送结果。
    pub manual_retry_authorized: bool,
}

impl EmbeddingCallRuntime {
    pub fn check(&self) -> Result<(), OperationFailure> {
        if self.cancel.is_cancelled() {
            return Err(OperationFailure {
                code: ErrorCode::Cancelled,
                message: self
                    .cancel
                    .reason()
                    .unwrap_or_else(|| "embedding cancelled".into()),
                retryable: false,
            });
        }
        if lingxi_adapters::models::dispatch::unix_ms_now() >= self.ownership.deadline_unix_ms {
            return Err(OperationFailure {
                code: ErrorCode::BudgetExceeded,
                message: "embedding absolute deadline exhausted".into(),
                retryable: false,
            });
        }
        Ok(())
    }
}

pub struct EmbeddingOwnedOutcome {
    pub served: ServedEmbeddingOutcome,
    pub accounting_error: Option<OperationFailure>,
    pub model_call_id: String,
    pub reused_paid_response: bool,
}

fn internal(message: impl Into<String>) -> OperationFailure {
    OperationFailure {
        code: ErrorCode::Internal,
        message: message.into(),
        retryable: false,
    }
}

async fn bounded<T>(
    deadline: Option<u64>,
    cancel: Option<&crate::cancel::CancelScope>,
    future: impl std::future::Future<Output = Result<T, OperationFailure>>,
) -> Result<T, OperationFailure> {
    let expiry = async {
        match deadline {
            Some(deadline) => {
                tokio::time::sleep(std::time::Duration::from_millis(
                    deadline.saturating_sub(lingxi_adapters::models::dispatch::unix_ms_now()),
                ))
                .await
            }
            None => std::future::pending::<()>().await,
        }
        OperationFailure {
            code: ErrorCode::BudgetExceeded,
            message: "embedding absolute deadline exhausted during wait".into(),
            retryable: false,
        }
    };
    let cancellation = async {
        match cancel {
            Some(scope) => scope.cancelled().await,
            None => std::future::pending::<()>().await,
        }
        OperationFailure {
            code: ErrorCode::Cancelled,
            message: "embedding parent cancelled during wait".into(),
            retryable: false,
        }
    };
    tokio::select! { biased; error=cancellation=>Err(error), error=expiry=>Err(error), result=future=>result }
}

type IoLease = Arc<tokio::sync::OwnedMutexGuard<()>>;

/// 有限 paid 恢复槽在发送前预留；未落盘的结果绝不因取消或容量压力被淘汰。
struct PaidReservation {
    records: Arc<std::sync::Mutex<std::collections::BTreeMap<String, Option<Value>>>>,
    key: String,
}
impl Drop for PaidReservation {
    fn drop(&mut self) {
        let mut records = self.records.lock().unwrap_or_else(|p| p.into_inner());
        if records.get(&self.key).is_some_and(Option::is_none) {
            records.remove(&self.key);
        }
    }
}

#[derive(Clone)]
struct IoFence {
    deadline: std::time::Instant,
    cancel: Option<crate::cancel::CancelScope>,
}
impl IoFence {
    fn check(&self) -> Result<(), OperationFailure> {
        if self
            .cancel
            .as_ref()
            .is_some_and(|scope| scope.is_cancelled())
        {
            return Err(OperationFailure {
                code: ErrorCode::Cancelled,
                message: "embedding native IO parent cancelled".into(),
                retryable: false,
            });
        }
        if std::time::Instant::now() >= self.deadline {
            return Err(OperationFailure {
                code: ErrorCode::BudgetExceeded,
                message: "embedding native IO absolute deadline exhausted".into(),
                retryable: false,
            });
        }
        Ok(())
    }
}

/// 每个实际系统调用前检查同一取消/期限；取消后的计费恢复尾只使用原首次锚点。
fn save_native(path: &Path, value: &Value, fence: &IoFence) -> Result<(), OperationFailure> {
    use std::io::Write;
    let parent = path
        .parent()
        .ok_or_else(|| internal("embedding journal has no parent"))?;
    fence.check()?;
    match std::fs::read(path) {
        Ok(bytes) => {
            let existing: Value = serde_json::from_slice(&bytes).map_err(|e| {
                internal(format!(
                    "embedding journal existing receipt invalid; preserved: {e}"
                ))
            })?;
            if existing["schema"] != 1 {
                return Err(internal("embedding journal schema unsupported; preserved"));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(internal(format!(
                "embedding journal existing receipt read: {error}"
            )))
        }
    }

    fence.check()?;
    std::fs::create_dir_all(parent)
        .map_err(|e| internal(format!("embedding journal directory: {e}")))?;
    let call = value["modelCallId"]
        .as_str()
        .ok_or_else(|| internal("embedding journal call identity missing"))?;
    let temporary = path.with_extension(format!(
        "{}.pending",
        lingxi_kernel::knowledge::sha256_hex(call.as_bytes())
    ));
    fence.check()?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // 不跟随替换后的链接；确认实际句柄是普通文件后才允许截断。
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|e| internal(format!("embedding journal open: {e}")))?;
    fence.check()?;
    // 已打开的对象必须是真实普通文件；FIFO 的 sync/rename 成功不代表响应已持久保存。
    if !file
        .metadata()
        .map_err(|e| internal(format!("embedding journal file metadata: {e}")))?
        .is_file()
    {
        return Err(internal(
            "embedding journal temporary is not a regular file; paid bytes retained",
        ));
    }
    fence.check()?;
    file.set_len(0)
        .map_err(|e| internal(format!("embedding journal truncate: {e}")))?;
    fence.check()?;
    file.write_all(value.to_string().as_bytes())
        .map_err(|e| internal(format!("embedding journal persist: {e}")))?;
    fence.check()?;
    file.sync_all()
        .map_err(|e| internal(format!("embedding journal persist: {e}")))?;
    fence.check()?;
    let temporary_metadata = std::fs::symlink_metadata(&temporary)
        .map_err(|e| internal(format!("embedding journal temporary identity: {e}")))?;
    if !temporary_metadata.is_file() {
        return Err(internal(
            "embedding journal temporary replaced; paid bytes retained",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let opened = file.metadata().map_err(|e| internal(e.to_string()))?;
        if opened.dev() != temporary_metadata.dev() || opened.ino() != temporary_metadata.ino() {
            return Err(internal(
                "embedding journal temporary identity changed; paid bytes retained",
            ));
        }
    }
    fence.check()?;
    std::fs::rename(&temporary, path)
        .map_err(|e| internal(format!("embedding journal replace: {e}")))?;
    fence.check()?;
    let persisted_metadata = std::fs::symlink_metadata(path)
        .map_err(|e| internal(format!("embedding journal persisted identity: {e}")))?;
    if !persisted_metadata.is_file() {
        return Err(internal(
            "embedding journal persisted file replaced; paid bytes retained",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let opened = file.metadata().map_err(|e| internal(e.to_string()))?;
        if opened.dev() != persisted_metadata.dev() || opened.ino() != persisted_metadata.ino() {
            return Err(internal(
                "embedding journal persisted identity changed; paid bytes retained",
            ));
        }
    }
    fence.check()?;
    let directory = std::fs::File::open(parent)
        .map_err(|e| internal(format!("embedding journal directory open: {e}")))?;
    fence.check()?;
    directory
        .sync_all()
        .map_err(|e| internal(format!("embedding journal directory sync: {e}")))
}

struct EmbeddingSendObserver<'a> {
    service: &'a OperationService,
    path: &'a Path,
    runtime: &'a EmbeddingCallRuntime,
    receipt: &'a std::sync::Mutex<Value>,
    lease: IoLease,
    outer: Option<lingxi_adapters::models::dispatch::SendObserver<'a>>,
}
impl lingxi_adapters::models::dispatch::AsyncSendObserver for EmbeddingSendObserver<'_> {
    fn rejected_response_status(
        &self,
        status: u16,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), ProtocolError>> + Send + '_>>
    {
        Box::pin(async move {
            if !(300..=599).contains(&status) {
                return Err(ProtocolError::new(
                    ErrorCode::Internal,
                    "embedding rejected response status invalid",
                    false,
                ));
            }
            // 已知服务端拒绝与未收到结果不同；状态只由真实 transport 注入。
            let mut rejected = self.receipt.lock().expect("embedding receipt lock").clone();
            rejected["state"] = json!("rejected_response");
            rejected["responseStatus"] = json!(status);
            rejected["outcome"] = json!("failed");
            self.service
                .save_journal(
                    self.path,
                    &rejected,
                    Some(self.runtime),
                    true,
                    Some(self.lease.clone()),
                )
                .await
                .map_err(|error| ProtocolError::new(error.code, error.message, false))?;
            *self.receipt.lock().expect("embedding receipt lock") = rejected;
            Ok(())
        })
    }

    fn observe(
        &self,
        observation: lingxi_adapters::models::dispatch::SendObservation,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), ProtocolError>> + Send + '_>>
    {
        Box::pin(async move {
            use lingxi_adapters::models::dispatch::SendObservation;
            if observation == SendObservation::AttemptStarted {
                self.runtime
                    .check()
                    .map_err(|e| ProtocolError::new(e.code, e.message, false))?;
                let mut sending = self.receipt.lock().expect("embedding receipt lock").clone();
                sending["state"] = json!("inflight");
                sending["transportAttempts"] = json!(1);
                sending["knownNotSent"] = json!(false);
                self.service
                    .save_journal(
                        self.path,
                        &sending,
                        Some(self.runtime),
                        false,
                        Some(self.lease.clone()),
                    )
                    .await
                    .map_err(|e| ProtocolError::new(e.code, e.message, false))?;
                *self.receipt.lock().expect("embedding receipt lock") = sending;
                // 落盘完成后再次确认父树，取消后的真实 HTTP 不得开始。
                if let Err(error) = self.runtime.check() {
                    let mut record = self.receipt.lock().expect("embedding receipt lock").clone();
                    record["state"] = json!("not_sent");
                    record["transportAttempts"] = json!(0);
                    record["knownNotSent"] = json!(true);
                    self.service
                        .save_journal(
                            self.path,
                            &record,
                            Some(self.runtime),
                            true,
                            Some(self.lease.clone()),
                        )
                        .await
                        .map_err(|e| ProtocolError::new(e.code, e.message, false))?;
                    *self.receipt.lock().expect("embedding receipt lock") = record;
                    return Err(ProtocolError::new(error.code, error.message, false));
                }
            }
            if observation == SendObservation::ConnectFailedBeforeSend {
                let mut record = self.receipt.lock().expect("embedding receipt lock").clone();
                record["knownNotSent"] = json!(true);
                self.service
                    .save_journal(
                        self.path,
                        &record,
                        Some(self.runtime),
                        true,
                        Some(self.lease.clone()),
                    )
                    .await
                    .map_err(|e| ProtocolError::new(e.code, e.message, false))?;
                *self.receipt.lock().expect("embedding receipt lock") = record;
            }
            if let Some(observer) = self.outer {
                if let Err(error) = observer(observation) {
                    if observation == SendObservation::AttemptStarted {
                        let mut record =
                            self.receipt.lock().expect("embedding receipt lock").clone();
                        record["state"] = json!("not_sent");
                        record["transportAttempts"] = json!(0);
                        record["knownNotSent"] = json!(true);
                        self.service
                            .save_journal(
                                self.path,
                                &record,
                                Some(self.runtime),
                                true,
                                Some(self.lease.clone()),
                            )
                            .await
                            .map_err(|e| ProtocolError::new(e.code, e.message, false))?;
                        *self.receipt.lock().expect("embedding receipt lock") = record;
                    }
                    return Err(error);
                }
            }
            Ok(())
        })
    }
}

impl OperationService {
    pub(crate) fn with_embedding_supervisor(
        mut self,
        supervisor: Arc<crate::task_supervisor::TaskSupervisor>,
    ) -> Self {
        self.embedding_io = supervisor;
        self
    }
    fn embedding_claim(&self, key: &str) -> Result<Arc<tokio::sync::Mutex<()>>, OperationFailure> {
        let mut locks = self
            .embedding_locks
            .lock()
            .map_err(|_| internal("embedding claim lock poisoned"))?;
        locks.retain(|_, lock| lock.strong_count() > 0);
        Ok(match locks.get(key).and_then(std::sync::Weak::upgrade) {
            Some(lock) => lock,
            None => {
                let lock = Arc::new(tokio::sync::Mutex::new(()));
                locks.insert(key.into(), Arc::downgrade(&lock));
                lock
            }
        })
    }
    async fn journal_io<T, F>(
        &self,
        runtime: Option<&EmbeddingCallRuntime>,
        cleanup: bool,
        lease: Option<IoLease>,
        work: F,
    ) -> Result<T, OperationFailure>
    where
        T: Send + 'static,
        F: FnOnce(IoFence) -> Result<T, OperationFailure> + Send + 'static,
    {
        let now = std::time::Instant::now();
        let grace = self.embedding_io.cooperative_grace();
        let original = runtime.map(|r| {
            let unix_now = lingxi_adapters::models::dispatch::unix_ms_now();
            if r.ownership.deadline_unix_ms >= unix_now {
                now + std::time::Duration::from_millis(r.ownership.deadline_unix_ms - unix_now)
            } else {
                now.checked_sub(std::time::Duration::from_millis(
                    unix_now - r.ownership.deadline_unix_ms,
                ))
                .unwrap_or(now)
            }
        });
        let cleanup_deadline = runtime
            .and_then(|r| r.cancel.cancelled_at())
            .map(|at| at + grace)
            .unwrap_or(if cleanup {
                now + grace
            } else {
                original
                    .map(|deadline| deadline + grace)
                    .unwrap_or(now + grace)
            })
            .min(
                original
                    .map(|deadline| deadline + grace)
                    .unwrap_or(now + grace),
            );
        let fence = IoFence {
            deadline: if cleanup {
                cleanup_deadline
            } else {
                original.unwrap_or(now + grace)
            },
            cancel: if cleanup {
                None
            } else {
                runtime.map(|r| r.cancel.clone())
            },
        };
        fence.check()?;
        let paid_retention = self.embedding_paid.clone();
        let task = self
            .embedding_io
            .spawn_blocking_owned(
                runtime.and_then(|r| r.ownership.run_id.clone()),
                format!(
                    "embedding-native-io:{}",
                    runtime
                        .map(|r| r.ownership.cause_ref.as_str())
                        .unwrap_or("recovery")
                ),
                move || {
                    // 强引用到物理 closure 退出；等待 future 被丢弃也不能释放 request claim。
                    let _physical_lease = lease;
                    let _paid_retention=paid_retention;
                    let result=work(fence);
                if let Err(error)=&result { tracing::error!(%error,"embedding native IO physically exited with failure"); }
                result
                },
            )
            .map_err(|e| internal(format!("embedding native IO admission refused: {e}")))?;
        let id = task.task_id();
        let waiting = task.wait();
        tokio::pin!(waiting);
        let deadline = if cleanup {
            cleanup_deadline
        } else {
            original.unwrap_or(now + grace)
        };
        let cancellation = async {
            match runtime.filter(|_| !cleanup) {
                Some(r) => r.cancel.cancelled().await,
                None => std::future::pending::<()>().await,
            }
        };
        let error = tokio::select! {biased;
            _=cancellation=>OperationFailure{code:ErrorCode::Cancelled,message:"embedding native IO parent cancelled".into(),retryable:false},
            _=tokio::time::sleep_until(deadline.into())=>OperationFailure{code:ErrorCode::BudgetExceeded,message:"embedding native IO deadline exhausted".into(),retryable:false},
            result=&mut waiting=>return result.map_err(|e|internal(format!("embedding native IO supervised exit: {e:?}")))?,
        };
        // 不续期：父取消发生在等待中，使用它的首次锚点重新取原剩余清理时间。
        let first = runtime
            .and_then(|r| r.cancel.cancelled_at())
            .map(|at| at + grace)
            .unwrap_or(cleanup_deadline)
            .min(cleanup_deadline);
        match tokio::time::timeout_at(first.into(),&mut waiting).await {
            Ok(result)=>{result.map_err(|e|internal(format!("embedding native IO supervised exit: {e:?}")))??;Err(error)},
            Err(_)=>Err(internal(format!("embedding native IO stop unconfirmed; physical task {id} and request claim retained"))),
        }
    }
    async fn save_journal(
        &self,
        path: &Path,
        value: &Value,
        runtime: Option<&EmbeddingCallRuntime>,
        cleanup: bool,
        lease: Option<IoLease>,
    ) -> Result<(), OperationFailure> {
        let path = path.to_path_buf();
        let value = value.clone();
        self.journal_io(runtime, cleanup, lease, move |fence| {
            save_native(&path, &value, &fence)
        })
        .await
    }

    fn paid_path(path: &Path, receipt: &Value) -> Result<PathBuf, OperationFailure> {
        let key = path
            .file_name()
            .and_then(|s| s.to_str())
            .and_then(|s| s.split('-').next())
            .and_then(|s| s.split('.').next())
            .ok_or_else(|| internal("embedding receipt filename invalid"))?;
        let call = receipt["modelCallId"]
            .as_str()
            .ok_or_else(|| internal("embedding receipt call id missing"))?;
        Ok(path.with_file_name(format!("{key}-{call}.paid.json")))
    }
    async fn paid_receipt(
        &self,
        path: &Path,
        receipt: Value,
        runtime: Option<&EmbeddingCallRuntime>,
        lease: IoLease,
    ) -> Result<(PathBuf, Value), OperationFailure> {
        if receipt["schema"] != 1 {
            return Err(internal("embedding journal schema unsupported; preserved"));
        }
        let paid = Self::paid_path(path, &receipt)?;
        let bytes = self
            .journal_io(runtime, runtime.is_none(), Some(lease), {
                let paid = paid.clone();
                move |fence| {
                    fence.check()?;
                    match std::fs::read(paid) {
                        Ok(bytes) => Ok(Some(bytes)),
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                        Err(e) => Err(internal(e.to_string())),
                    }
                }
            })
            .await?;
        if let Some(bytes) = bytes {
            let complete: Value = serde_json::from_slice(&bytes)
                .map_err(|e| internal(format!("paid embedding receipt invalid: {e}")))?;
            if complete["schema"] != 1 {
                return Err(internal("embedding journal schema unsupported; preserved"));
            }
            for field in [
                "schema",
                "modelCallId",
                "requestKey",
                "requestDigest",
                "ownerBinding",
                "parameterFingerprint",
                "provider",
                "model",
                "protocol",
            ] {
                if complete[field] != receipt[field] {
                    return Err(internal(
                        "paid embedding receipt exact binding invalid; preserved",
                    ));
                }
            }
            if complete["state"] != "response" {
                return Err(internal(
                    "paid embedding receipt is not a confirmed response",
                ));
            }
            return Ok((paid, complete));
        }
        Ok((path.to_path_buf(), receipt))
    }

    async fn record_embedding_receipt(
        &self,
        receipt: &Value,
        runtime: Option<&EmbeddingCallRuntime>,
    ) -> Result<(), OperationFailure> {
        // 取消发生在任何传输尝试之前：没有可能已付费的字节，也不写 unknown
        // 费用行——等待真实发生、归属由调用方错误与作业统计保留（W09 C02
        // 配额等待取消；真正上过线路的取消/中断仍如实记 unknown）。
        if receipt["outcome"] == "cancelled"
            && receipt["transportAttempts"].as_u64() == Some(0)
            && receipt["knownNotSent"] == true
        {
            return Ok(());
        }
        let Some(sink) = &self.usage_sink else {
            return Ok(());
        };
        let ownership: Option<OperationOwnership> = receipt
            .get("ownership")
            .filter(|v| !v.is_null())
            .map(|v| serde_json::from_value(v.clone()))
            .transpose()
            .map_err(|e| internal(format!("embedding receipt ownership invalid: {e}")))?;
        let context = ownership
            .as_ref()
            .and_then(|o| match (&o.session_id, &o.run_id) {
                (Some(session), Some(run)) => Some(OperationCallContext {
                    session_id: session.clone(),
                    run_id: run.clone(),
                    attempt: o.attempt.clone(),
                    cause_ref: Some(o.cause_ref.clone()),
                }),
                _ => None,
            });
        let usage_report = match receipt.get("usage").filter(|v| !v.is_null()) {
            Some(raw) => match lingxi_adapters::models::usage::decode_operation_usage(raw) {
                lingxi_adapters::models::usage::UsageDecode::Usage(fact) => {
                    ReportedUsage::Known(fact)
                }
                lingxi_adapters::models::usage::UsageDecode::Invalid { detail } => {
                    ReportedUsage::Invalid { detail }
                }
                lingxi_adapters::models::usage::UsageDecode::Absent => ReportedUsage::Unknown,
            },
            None => ReportedUsage::Unknown,
        };
        let required = |key: &str| {
            receipt[key]
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| internal(format!("embedding receipt missing {key}")))
        };
        let fact = OperationUsageFact {
            stable_call_id: Some(required("modelCallId")?),
            ownership,
            transport_unknown: receipt["transportAttempts"].is_null(),
            operation: "embedding".into(),
            provider: required("provider")?,
            model: required("model")?,
            protocol: required("protocol")?,
            usage_report,
            transport_attempts: receipt["transportAttempts"].as_u64().unwrap_or(0) as u32,
            outcome: CallOutcome::parse(&required("outcome")?)
                .ok_or_else(|| internal("embedding receipt outcome invalid"))?,
            started_at_unix_ms: receipt["startedAt"].as_u64(),
            settled_at_unix_ms: receipt["settledAt"].as_u64(),
            context,
        };
        // 补账使用原首次取消锚和绝对截止时间的剩余清理尾，不按每行重新续期。
        let now = std::time::Instant::now();
        let mut until =
            now + std::time::Duration::from_millis(crate::operations::SYSTEM_SPEECH_KILL_GRACE_MS);
        if let Some(runtime) = runtime {
            let unix_now = lingxi_adapters::models::dispatch::unix_ms_now();
            let original = if runtime.ownership.deadline_unix_ms >= unix_now {
                now + std::time::Duration::from_millis(
                    runtime.ownership.deadline_unix_ms - unix_now,
                )
            } else {
                now.checked_sub(std::time::Duration::from_millis(
                    unix_now - runtime.ownership.deadline_unix_ms,
                ))
                .unwrap_or(now)
            };
            until = until.min(original + self.embedding_io.cooperative_grace());
            if let Some(first) = runtime.cancel.cancelled_at() {
                until = until.min(first + self.embedding_io.cooperative_grace());
            }
        }
        if until <= now {
            return Err(internal(
                "embedding original accounting cleanup deadline exhausted; paid bytes retained",
            ));
        }
        tokio::time::timeout_at(until.into(), sink.record(fact))
            .await
            .map_err(|_| {
                internal(
                "embedding usage settlement exceeded its bounded cleanup tail; journal retained",
            )
            })?
            .map_err(|e| {
                internal(format!(
                    "the usage ledger refused the embedding accounting row: {e}"
                ))
            })
    }

    /// 真实启动先结算遗留调用；未确认窗口只记未知，不编造零费用。
    pub async fn recover_embedding_usage(&self) -> Result<(), OperationFailure> {
        self.recover_embedding_usage_filtered(None, None).await
    }

    /// 同一作业重试只补已结算凭据，绝不将并行请求误判为进程中断。
    pub async fn settle_embedding_usage_for_cause(
        &self,
        cause: &str,
    ) -> Result<(), OperationFailure> {
        self.recover_embedding_usage_filtered(Some(cause), None)
            .await
    }

    /// 作业重试沿用原宿主的取消树和绝对期限，不给补账另开续期预算。
    pub async fn settle_embedding_usage_for_runtime(
        &self,
        runtime: &EmbeddingCallRuntime,
    ) -> Result<(), OperationFailure> {
        runtime.check()?;
        self.recover_embedding_usage_filtered(Some(&runtime.ownership.cause_ref), Some(runtime))
            .await
    }

    async fn recover_embedding_usage_filtered(
        &self,
        cause: Option<&str>,
        runtime: Option<&EmbeddingCallRuntime>,
    ) -> Result<(), OperationFailure> {
        let root = self.products_root.join("embedding-calls-v1");
        let paths = self
            .journal_io(runtime, runtime.is_none(), None, move |fence| {
                fence.check()?;
                if !root.exists() {
                    return Ok(Vec::new());
                }
                std::fs::read_dir(root)
                    .map_err(|e| internal(format!("embedding journal read: {e}")))?
                    .map(|entry| {
                        fence.check()?;
                        entry.map(|e| e.path()).map_err(|e| internal(e.to_string()))
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .await?;
        let retained = self
            .embedding_paid
            .lock()
            .map_err(|_| internal("embedding paid retention lock poisoned"))?
            .iter()
            .filter_map(|(key, record)| record.clone().map(|record| (key.clone(), record)))
            .collect::<Vec<_>>();
        for (key, mut receipt) in retained {
            if let Some(runtime) = runtime {
                runtime.check()?;
            }
            if cause.is_some_and(|cause| receipt["ownership"]["cause_ref"].as_str() != Some(cause))
            {
                continue;
            }
            let claim = self.embedding_claim(&key)?;
            let lease=Arc::new(claim.try_lock_owned().map_err(|_|internal("embedding call physically active; retained paid bytes cannot release its lease"))?);
            let path = self
                .products_root
                .join("embedding-calls-v1")
                .join(format!("{key}.json"));
            let paid_path = Self::paid_path(&path, &receipt)?;
            self.save_journal(
                &paid_path,
                &receipt,
                runtime,
                runtime.is_none(),
                Some(lease.clone()),
            )
            .await?;
            self.record_embedding_receipt(&receipt, runtime).await?;
            receipt["ledgerCommitted"] = json!(true);
            self.save_journal(
                &paid_path,
                &receipt,
                runtime,
                runtime.is_none(),
                Some(lease.clone()),
            )
            .await?;
            self.embedding_paid
                .lock()
                .map_err(|_| internal("embedding paid retention lock poisoned"))?
                .remove(&key);
        }
        for path in paths {
            if let Some(runtime) = runtime {
                runtime.check()?;
            }
            if path.extension().and_then(|v| v.to_str()) != Some("json") {
                continue;
            }
            if let Some(cause) = cause {
                let preliminary = self
                    .journal_io(runtime, runtime.is_none(), None, {
                        let path = path.clone();
                        move |fence| {
                            fence.check()?;
                            std::fs::read(path).map_err(|e| internal(e.to_string()))
                        }
                    })
                    .await?;
                let preliminary: Value =
                    serde_json::from_slice(&preliminary).map_err(|e| internal(e.to_string()))?;
                if preliminary["schema"] != 1 {
                    return Err(internal("embedding journal schema unsupported; preserved"));
                }
                // 只读取原子文件以筛选归属，不将其它尚活调用分类为中断。
                if preliminary["ownership"]["cause_ref"].as_str() != Some(cause) {
                    continue;
                }
            }
            let key = path
                .file_name()
                .and_then(|s| s.to_str())
                .and_then(|s| s.split('-').next())
                .and_then(|s| s.split('.').next())
                .unwrap_or("");
            let claim = self.embedding_claim(key)?;
            let lease=Arc::new(claim.try_lock_owned().map_err(|_|internal("embedding call physically active; recovery must not classify it as interrupted"))?);
            let bytes = self
                .journal_io(runtime, runtime.is_none(), Some(lease.clone()), {
                    let path = path.clone();
                    move |fence| {
                        fence.check()?;
                        std::fs::read(path).map_err(|e| internal(e.to_string()))
                    }
                })
                .await?;
            let original: Value = serde_json::from_slice(&bytes)
                .map_err(|e| internal(format!("embedding journal invalid: {e}")))?;
            let (path, mut receipt) = self
                .paid_receipt(&path, original, runtime, lease.clone())
                .await?;
            if receipt["schema"] != 1 {
                return Err(internal("embedding journal schema unsupported; preserved"));
            }
            if cause.is_some_and(|cause| receipt["ownership"]["cause_ref"].as_str() != Some(cause))
            {
                continue;
            }
            if receipt["ledgerCommitted"] == true {
                continue;
            }
            if matches!(
                receipt["state"].as_str(),
                Some("inflight" | "rejected_response" | "failed")
            ) {
                if let Some(sink) = &self.usage_sink {
                    let call = receipt["modelCallId"]
                        .as_str()
                        .ok_or_else(|| internal("embedding recovery call missing"))?
                        .to_string();
                    let existing = bounded(
                        runtime.map(|runtime| runtime.ownership.deadline_unix_ms),
                        runtime.map(|runtime| &runtime.cancel),
                        async {
                            tokio::time::timeout(
                                std::time::Duration::from_millis(SYSTEM_SPEECH_KILL_GRACE_MS),
                                sink.recorded(call),
                            )
                            .await
                            .map_err(|_| {
                                internal(
                                    "embedding prior accounting lookup exceeded cleanup budget",
                                )
                            })?
                            .map_err(|e| {
                                internal(format!("embedding prior accounting lookup failed: {e}"))
                            })
                        },
                    )
                    .await?;
                    if let Some(existing) = existing {
                        let owner: OperationOwnership =
                            serde_json::from_value(receipt["ownership"].clone())
                                .map_err(|e| internal(e.to_string()))?;
                        if existing.purpose != "embedding"
                            || existing.provider != receipt["provider"].as_str().unwrap_or("")
                            || existing.model != receipt["model"].as_str().unwrap_or("")
                            || existing.protocol != receipt["protocol"].as_str().unwrap_or("")
                            || existing.session_id != owner.session_id
                            || existing.run_id != owner.run_id
                            || existing.attempt != owner.attempt
                            || existing.parent_tool_call_id != owner.parent_tool_call_id
                            || existing.cause_ref.as_deref() != Some(&owner.cause_ref)
                            || existing.started_at_unix_ms != receipt["startedAt"].as_u64()
                        {
                            return Err(internal(
                                "embedding prior accounting exact ownership mismatch; preserved",
                            ));
                        }
                        // 原账已落地时保留其终态；明确拒绝仍须有新手动授权才可重试。
                        let rejected = receipt["responseStatus"]
                            .as_u64()
                            .is_some_and(|status| (300..=599).contains(&status));
                        receipt["state"] = json!(if rejected
                            && matches!(
                                existing.outcome,
                                CallOutcome::Failed | CallOutcome::Cancelled
                            ) {
                            "failed"
                        } else {
                            "paid_unrecoverable"
                        });
                        receipt["ledgerCommitted"] = json!(true);
                        receipt["outcome"] = json!(existing.outcome.wire_name());
                        receipt["transportAttempts"] = json!(existing.transport_attempts);
                        receipt["settledAt"] = json!(existing.settled_at_unix_ms);
                        receipt["ledgerKnownUsage"] = json!({"inputTokens":existing.usage.as_ref().and_then(|u|u.input_tokens),"outputTokens":existing.usage.as_ref().and_then(|u|u.output_tokens),"state":existing.usage.as_ref().map(|u|u.provenance.state_name())});
                        self.save_journal(
                            &path,
                            &receipt,
                            runtime,
                            runtime.is_none(),
                            Some(lease.clone()),
                        )
                        .await?;
                        continue;
                    }
                }
            }
            if receipt["state"] == "rejected_response" {
                if !receipt["responseStatus"]
                    .as_u64()
                    .is_some_and(|status| (300..=599).contains(&status))
                {
                    return Err(internal(
                        "embedding rejected response status invalid; preserved",
                    ));
                }
                receipt["state"] = json!("failed");
                receipt["outcome"] = json!("failed");
                receipt["settledAt"] = json!(lingxi_adapters::models::dispatch::unix_ms_now());
            }
            if receipt["state"] == "inflight" {
                receipt["outcome"] = json!("unknown");
                receipt["transportAttempts"] = Value::Null;
                receipt["settledAt"] = json!(lingxi_adapters::models::dispatch::unix_ms_now());
                receipt["state"] = json!("interrupted");
                self.save_journal(
                    &path,
                    &receipt,
                    runtime,
                    runtime.is_none(),
                    Some(lease.clone()),
                )
                .await?;
            }
            self.record_embedding_receipt(&receipt, runtime).await?;
            receipt["ledgerCommitted"] = json!(true);
            self.save_journal(&path, &receipt, runtime, runtime.is_none(), Some(lease))
                .await?;
        }
        Ok(())
    }

    pub async fn embed_owned_observed(
        &self,
        request: dialects::embedding::EmbeddingRequest,
        runtime: &EmbeddingCallRuntime,
        observer: Option<lingxi_adapters::models::dispatch::SendObserver<'_>>,
    ) -> Result<EmbeddingOwnedOutcome, OperationFailure> {
        runtime.check()?;
        dialects::embedding::validate_embedding_request(&request)
            .map_err(OperationFailure::from_protocol)?;
        let started = lingxi_adapters::models::dispatch::unix_ms_now();
        let route = self
            .gateway
            .resolve_route(&ModelRouteRequest::for_operation(ModelOperation::Embedding))
            .map_err(|e| OperationFailure {
                code: match &e {
                    ModelGatewayError::UnknownProvider { .. }
                    | ModelGatewayError::RouteNotConfigured { .. } => ErrorCode::NotFound,
                    ModelGatewayError::MissingCredential { .. } => ErrorCode::Unauthorized,
                    _ => ErrorCode::InvalidMessage,
                },
                message: e.to_string(),
                retryable: false,
            })?;
        let fingerprint = dialects::embedding::embedding_space_fingerprint(&route, &request);
        let request_digest=lingxi_kernel::knowledge::sha256_hex(json!({"inputs":request.inputs,"dimensions":request.dimensions,"contextWindow":request.context_window,"inputType":format!("{:?}",request.input_type)}).to_string().as_bytes());
        // 后台同一作业可跨 attempt 补账；用户工具必须绑定真实 attempt 与父工具。
        let owner_binding = json!({"principalKind":runtime.ownership.principal_kind,"principalSubject":runtime.ownership.principal_subject,"agentId":runtime.ownership.agent_id,"sessionId":runtime.ownership.session_id,"runId":runtime.ownership.run_id,"attempt":if runtime.ownership.run_id.is_some(){runtime.ownership.attempt.clone()}else{None},"parentToolCallId":runtime.ownership.parent_tool_call_id,"causeRef":runtime.ownership.cause_ref});
        let key = lingxi_kernel::knowledge::sha256_hex(json!({"requestKey":runtime.request_key,"requestDigest":request_digest,"space":fingerprint,"owner":owner_binding}).to_string().as_bytes());
        let claim = self.embedding_claim(&key)?;
        let lease = Arc::new(
            bounded(
                Some(runtime.ownership.deadline_unix_ms),
                Some(&runtime.cancel),
                async { Ok(claim.lock_owned().await) },
            )
            .await?,
        );
        runtime.check()?;
        let path = self
            .products_root
            .join("embedding-calls-v1")
            .join(format!("{key}.json"));
        let retained = self
            .embedding_paid
            .lock()
            .map_err(|_| internal("embedding paid retention lock poisoned"))?
            .get(&key)
            .cloned()
            .flatten();
        let prior = if let Some(retained) = retained {
            Some(retained)
        } else {
            let bytes = self
                .journal_io(Some(runtime), false, Some(lease.clone()), {
                    let path = path.clone();
                    move |fence| {
                        fence.check()?;
                        match std::fs::read(path) {
                            Ok(bytes) => Ok(Some(bytes)),
                            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                            Err(error) => Err(internal(format!(
                                "embedding journal existing receipt read: {error}"
                            ))),
                        }
                    }
                })
                .await?;
            bytes
                .map(|bytes| {
                    serde_json::from_slice::<Value>(&bytes).map_err(|e| internal(e.to_string()))
                })
                .transpose()?
        };
        let (path, prior) = match prior {
            Some(receipt) => {
                let (path, receipt) = self
                    .paid_receipt(&path, receipt, Some(runtime), lease.clone())
                    .await?;
                (path, Some(receipt))
            }
            None => (path, None),
        };
        if prior.as_ref().is_some_and(|receipt| {
            receipt["schema"] != 1
                || receipt["parameterFingerprint"] != fingerprint
                || receipt["requestKey"] != runtime.request_key
                || receipt["requestDigest"] != request_digest
                || receipt["ownerBinding"] != owner_binding
        }) {
            return Err(internal(
                "embedding receipt schema or exact ownership binding invalid; preserved",
            ));
        }
        if let Some(mut receipt) = prior.clone().filter(|r| r["state"] == "response") {
            if receipt["schema"] != 1
                || receipt["parameterFingerprint"] != fingerprint
                || receipt["requestKey"] != runtime.request_key
                || receipt["requestDigest"] != request_digest
                || receipt["ownerBinding"] != owner_binding
            {
                return Err(internal("paid embedding receipt binding invalid"));
            }
            let outcome = dialects::embedding::EmbeddingOutcome {
                vectors: serde_json::from_value(receipt["vectors"].clone())
                    .map_err(|e| internal(format!("paid embedding vectors invalid: {e}")))?,
                usage: receipt.get("usage").filter(|v| !v.is_null()).cloned(),
            };
            let mut persistence_error = None;
            if self
                .embedding_paid
                .lock()
                .map_err(|_| internal("embedding paid retention lock poisoned"))?
                .contains_key(&key)
            {
                let paid_path = Self::paid_path(&path, &receipt)?;
                match self
                    .save_journal(
                        &paid_path,
                        &receipt,
                        Some(runtime),
                        true,
                        Some(lease.clone()),
                    )
                    .await
                {
                    Ok(()) => {
                        self.embedding_paid
                            .lock()
                            .map_err(|_| internal("embedding paid retention lock poisoned"))?
                            .remove(&key);
                    }
                    Err(error) => persistence_error = Some(error),
                }
            }
            let accounting_error = if receipt["ledgerCommitted"] == true {
                None
            } else {
                match self.record_embedding_receipt(&receipt, Some(runtime)).await {
                    Ok(()) => {
                        receipt["ledgerCommitted"] = json!(true);
                        self.save_journal(&path, &receipt, Some(runtime), true, Some(lease.clone()))
                            .await
                            .err()
                    }
                    Err(error) => Some(error),
                }
            };
            let mut original_route = route;
            original_route.config_generation = receipt["servedConfigGeneration"]
                .as_u64()
                .ok_or_else(|| internal("paid embedding route generation missing"))?;
            return Ok(EmbeddingOwnedOutcome {
                served: ServedEmbeddingOutcome {
                    outcome,
                    route: original_route,
                    parameter_fingerprint: fingerprint,
                },
                accounting_error: match (accounting_error, persistence_error) {
                    (Some(a), Some(p)) => Some(internal(format!("{}; {}", a.message, p.message))),
                    (Some(e), None) | (None, Some(e)) => Some(e),
                    (None, None) => None,
                },
                model_call_id: receipt["modelCallId"]
                    .as_str()
                    .ok_or_else(|| internal("paid embedding call identity missing"))?
                    .into(),
                reused_paid_response: true,
            });
        }
        if let Some(mut interrupted) = prior {
            let rejected_response = interrupted["responseStatus"]
                .as_u64()
                .is_some_and(|status| (300..=599).contains(&status));
            let manual_retry_allowed = runtime.manual_retry_authorized
                && runtime.ownership.attempt.is_some()
                && interrupted["ownership"]["attempt"].as_str()
                    != runtime.ownership.attempt.as_deref();
            let unconfirmed_paid = interrupted["state"] == "paid_unrecoverable"
                || interrupted["state"] == "inflight"
                || interrupted["state"] == "interrupted"
                || (interrupted["state"] == "failed"
                    && interrupted["transportAttempts"]
                        .as_u64()
                        .is_some_and(|n| n > 0)
                    && interrupted["knownNotSent"] != true
                    && !interrupted["responseStatus"]
                        .as_u64()
                        .is_some_and(|status| (300..=599).contains(&status)));
            // 跨 claim 的旧 attempt 未确认调用：旧进程/旧驱动已经退出或被 join
            // （claim 互斥同时保证旧物理 IO 已结束），迟到响应无法再提交，其
            // unknown 账目已在下方结算并归档；新 attempt 以全新 call identity
            // 补缺口，不是同一调用的自动重发。同 attempt 内仍拒绝自动重发。
            let prior_attempt_closed = unconfirmed_paid
                && runtime.ownership.attempt.is_some()
                && interrupted["ownership"]["attempt"]
                    .as_str()
                    .is_some_and(|prior| Some(prior) != runtime.ownership.attempt.as_deref());
            if interrupted["state"] == "inflight" {
                interrupted["state"] = json!("interrupted");
                interrupted["outcome"] = json!("unknown");
                interrupted["transportAttempts"] = Value::Null;
                interrupted["settledAt"] = json!(started);
            }
            if interrupted["ledgerCommitted"] != true {
                self.record_embedding_receipt(&interrupted, Some(runtime))
                    .await?;
            }
            interrupted["ledgerCommitted"] = json!(true);
            let archive = path.with_file_name(format!(
                "{}-{}.json",
                key,
                interrupted["modelCallId"]
                    .as_str()
                    .ok_or_else(|| internal("interrupted call id missing"))?
            ));
            self.save_journal(
                &archive,
                &interrupted,
                Some(runtime),
                true,
                Some(lease.clone()),
            )
            .await?;
            if rejected_response && !unconfirmed_paid && !manual_retry_allowed {
                self.save_journal(
                    &path,
                    &interrupted,
                    Some(runtime),
                    true,
                    Some(lease.clone()),
                )
                .await?;
                return Err(internal("embedding rejected response retained; a new explicitly authorized manual attempt is required"));
            }
            if unconfirmed_paid && !prior_attempt_closed {
                // 已发但无成功结果凭据，原计费不可确认，不能自动再付一次。
                self.save_journal(
                    &path,
                    &interrupted,
                    Some(runtime),
                    true,
                    Some(lease.clone()),
                )
                .await?;
                return Err(internal("embedding acceptance or paid result is unknown; preserved original call and refused automatic resubmission"));
            }
        }
        let mut random = [0_u8; 16];
        getrandom::getrandom(&mut random)
            .map_err(|e| internal(format!("embedding call identity: {e}")))?;
        let call_id = format!(
            "op-embedding-{}",
            lingxi_kernel::knowledge::sha256_hex(&random)
        );
        let receipt = std::sync::Mutex::new(
            json!({"schema":1,"requestKey":runtime.request_key,"requestDigest":request_digest,"ownerBinding":owner_binding,"modelCallId":call_id,"ownership":runtime.ownership,"provider":route.provider,"model":route.model,"protocol":route.protocol.config_name(),"servedConfigGeneration":route.config_generation,"parameterFingerprint":fingerprint,"startedAt":started,"settledAt":null,"transportAttempts":0,"knownNotSent":true,"outcome":"unknown","state":"not_sent","ledgerCommitted":false,"usage":null}),
        );
        let capacity = {
            let mut paid = self
                .embedding_paid
                .lock()
                .map_err(|_| internal("embedding paid retention lock poisoned"))?;
            if !paid.contains_key(&key) && paid.len() >= 8 {
                false
            } else {
                paid.entry(key.clone()).or_insert(None);
                true
            }
        };
        if !capacity {
            let mut not_sent = receipt.lock().expect("embedding receipt lock").clone();
            not_sent["outcome"] = json!("failed");
            not_sent["settledAt"] = json!(lingxi_adapters::models::dispatch::unix_ms_now());
            self.record_embedding_receipt(&not_sent, Some(runtime))
                .await?;
            return Err(internal("embedding paid recovery capacity full; no HTTP submitted and no paid result evicted"));
        }
        let _paid_reservation = PaidReservation {
            records: self.embedding_paid.clone(),
            key: key.clone(),
        };
        let initial = receipt.lock().expect("embedding receipt lock").clone();
        if let Err(error) = self
            .save_journal(&path, &initial, Some(runtime), false, Some(lease.clone()))
            .await
        {
            let mut not_sent = initial;
            not_sent["outcome"] = json!(if runtime.cancel.is_cancelled() {
                "cancelled"
            } else {
                "failed"
            });
            not_sent["settledAt"] = json!(lingxi_adapters::models::dispatch::unix_ms_now());
            let error = match self
                .record_embedding_receipt(&not_sent, Some(runtime))
                .await
            {
                Ok(()) => error,
                Err(accounting) => internal(format!("{}; {}", error.message, accounting.message)),
            };
            return Err(error);
        }
        let deadline = Some(runtime.ownership.deadline_unix_ms);
        let auth_result = bounded(deadline, Some(&runtime.cancel), async {
            self.credentials
                .resolve(&route)
                .await
                .map_err(|e| OperationFailure {
                    code: ErrorCode::Unauthorized,
                    message: e.to_string(),
                    retryable: false,
                })
        })
        .await;
        let execution = async {
            let auth = auth_result?;
            let _permit = bounded(deadline, Some(&runtime.cancel), async {
                let agent = runtime
                    .ownership
                    .agent_id
                    .as_deref()
                    .unwrap_or("knowledge-ingestion");
                let lane = runtime
                    .ownership
                    .session_id
                    .as_deref()
                    .unwrap_or(&runtime.ownership.cause_ref);
                self.quotas
                    .acquire(QuotaResource::Model, agent, lane)
                    .await
                    .map_err(|e| OperationFailure {
                        code: ErrorCode::BudgetExceeded,
                        message: e.to_string(),
                        retryable: false,
                    })
            })
            .await?;
            runtime.check()?;
            let plan = dialects::embedding::build_embedding(&route, &auth, &request)
                .map_err(OperationFailure::from_protocol)?;
            let on_send = EmbeddingSendObserver {
                service: self,
                path: &path,
                runtime,
                receipt: &receipt,
                lease: lease.clone(),
                outer: observer,
            };
            let body = bounded(deadline, Some(&runtime.cancel), async {
                self.dispatcher
                    .execute_json_async_observed(&plan, deadline, &auth, &on_send)
                    .await
                    .map(|(body, _)| body)
                    .map_err(OperationFailure::from_pair)
            })
            .await?;
            dialects::embedding::parse_embedding(route.protocol, &request, &body)
                .map_err(OperationFailure::from_protocol)
        }
        .await;
        let mut record = receipt.into_inner().expect("embedding receipt lock");
        record["settledAt"] = json!(lingxi_adapters::models::dispatch::unix_ms_now());
        match execution {
            Ok(outcome) => {
                record["state"] = json!("response");
                record["outcome"] = json!("succeeded");
                record["vectors"] = json!(outcome.vectors);
                record["usage"] = json!(outcome.usage);
                // 先保住真正收到的字节；物理 IO 持续阻塞时，父任务最后被丢弃仍可恢复。
                self.embedding_paid
                    .lock()
                    .map_err(|_| internal("embedding paid retention lock poisoned"))?
                    .insert(key.clone(), Some(record.clone()));

                // 已付响应先保存到独立、绑定原 call 的恢复凭据，再记原真实用量。
                // canonical 意图写入受阻时不能把已知计费降成未知，或丢掉 paid 字节。
                let paid_path = Self::paid_path(&path, &record)?;
                if let Err(error) = self
                    .save_journal(
                        &paid_path,
                        &record,
                        Some(runtime),
                        true,
                        Some(lease.clone()),
                    )
                    .await
                {
                    let error = match self.record_embedding_receipt(&record, Some(runtime)).await {
                        Ok(()) => error,
                        Err(accounting) => {
                            internal(format!("{}; {}", error.message, accounting.message))
                        }
                    };
                    return Ok(EmbeddingOwnedOutcome {
                        served: ServedEmbeddingOutcome {
                            outcome,
                            route,
                            parameter_fingerprint: fingerprint,
                        },
                        accounting_error: Some(error),
                        model_call_id: call_id,
                        reused_paid_response: false,
                    });
                }
                // paid 文件已真实落盘，内存副本才可释放；记账失败仍由该文件保留原事实。
                self.embedding_paid
                    .lock()
                    .map_err(|_| internal("embedding paid retention lock poisoned"))?
                    .remove(&key);
                let mut accounting_error =
                    match self.record_embedding_receipt(&record, Some(runtime)).await {
                        Ok(()) => {
                            record["ledgerCommitted"] = json!(true);
                            self.save_journal(
                                &paid_path,
                                &record,
                                Some(runtime),
                                true,
                                Some(lease.clone()),
                            )
                            .await
                            .err()
                        }
                        Err(error) => Some(error),
                    };
                if let Err(error) = self
                    .save_journal(&path, &record, Some(runtime), true, Some(lease.clone()))
                    .await
                {
                    accounting_error = Some(match accounting_error {
                        Some(prior) => internal(format!("{}; {}", prior.message, error.message)),
                        None => error,
                    });
                }
                Ok(EmbeddingOwnedOutcome {
                    served: ServedEmbeddingOutcome {
                        outcome,
                        route,
                        parameter_fingerprint: fingerprint,
                    },
                    accounting_error,
                    model_call_id: call_id,
                    reused_paid_response: false,
                })
            }
            Err(error) => {
                record["state"] = json!("failed");
                record["outcome"] = json!(if error.code == ErrorCode::Cancelled {
                    "cancelled"
                } else {
                    "failed"
                });
                self.save_journal(&path, &record, Some(runtime), true, Some(lease.clone()))
                    .await?;
                self.record_embedding_receipt(&record, Some(runtime))
                    .await?;
                record["ledgerCommitted"] = json!(true);
                self.save_journal(&path, &record, Some(runtime), true, Some(lease.clone()))
                    .await?;
                Err(error)
            }
        }
    }
}
