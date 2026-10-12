//! R06-T07 KnowledgeService：现役知识库的服务面（笔记本/来源/摄入/检索/引用/
//! TurnScope/HTTP 路由）。
//!
//! 生产链（design.md §0）：用户选择笔记本 → create_turn_scope 冻结来源集合 →
//! 检索只及冻结集 → 引用锚定冻结原文 → 嵌入身份变化显式降级并可重建。
//!
//! 现役锚点：server/routes/knowledge.ts（HTTP 面 19 条）、
//! lib/knowledge/knowledge-manager.ts（编排）、knowledge-query-service.ts
//! （检索与向量门）、ingestion-service.ts（摄入 phase 链）、
//! lib/tools/knowledge-scope.ts（工具 scope 复核链——knowledgetools.rs 承担）。
//!
//! 退役边界：不恢复独立知识研究引擎（research/analysis/coverage 面零实现）；
//! knowledge 内容按来源材料进入上下文（T01 编译器材料机制），不冒充 system
//! 高优先级指令。

use std::sync::{Arc, Mutex, OnceLock};

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use lingxi_adapters::storage::knowledge as ks;
use lingxi_kernel::knowledge::{
    block_fingerprint, build_chunks, chunk_index_variant_id, chunker_config_id,
    locator_heading_path, resolve_chunker_config, sha256_hex, vector_index_variant_id,
    ChunkerBlock, KnowledgeError,
};
use serde_json::{json, Value};

// ── 嵌入端口（R05 ModelGateway/OperationService 链的唯一入口）───────────────

pub use lingxi_kernel::knowledge::EmbeddingModelIdentity;

/// 嵌入端口：identity() 解析当前配置的嵌入身份（None = 未配置——摄入落显式
/// pending_embedding，检索显式降级 FTS，禁静默换模型）；embed() 经
/// ModelGateway + CredentialService + OperationService 真链调用。
/// 一次嵌入调用的异步结果（身份 + 全批向量；批形状非法即错，禁静默截断）。
pub type EmbedFuture<'a> = std::pin::Pin<
    Box<
        dyn std::future::Future<
                Output = Result<(EmbeddingModelIdentity, Vec<Vec<f32>>), KnowledgeError>,
            > + Send
            + 'a,
    >,
>;

use lingxi_adapters::models::dispatch::SendObservation;
pub type EmbeddingSendObserver<'a> =
    &'a (dyn Fn(SendObservation) -> Result<(), KnowledgeError> + Send + Sync);
pub struct OwnedEmbeddingResult {
    pub identity: EmbeddingModelIdentity,
    pub vectors: Vec<Vec<f32>>,
    pub accounting_error: Option<KnowledgeError>,
    pub model_call_id: Option<String>,
    pub reused_paid_response: bool,
}
pub type OwnedEmbedFuture<'a> = std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<OwnedEmbeddingResult, KnowledgeError>> + Send + 'a>,
>;
pub trait KnowledgeEmbedder: Send + Sync {
    fn settle_pending<'a>(
        &'a self,
        _cause: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), KnowledgeError>> + Send + 'a>>
    {
        Box::pin(async { Ok(()) })
    }
    fn settle_pending_owned<'a>(
        &'a self,
        runtime: &'a crate::operations::EmbeddingCallRuntime,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), KnowledgeError>> + Send + 'a>>
    {
        self.settle_pending(&runtime.ownership.cause_ref)
    }
    fn identity(&self) -> Result<Option<EmbeddingModelIdentity>, KnowledgeError>;
    fn embed<'a>(&'a self, texts: Vec<String>, input_type: EmbeddingInputType) -> EmbedFuture<'a>;
    fn embed_owned<'a>(
        &'a self,
        texts: Vec<String>,
        input_type: EmbeddingInputType,
        runtime: &'a crate::operations::EmbeddingCallRuntime,
        observer: Option<EmbeddingSendObserver<'a>>,
    ) -> OwnedEmbedFuture<'a> {
        Box::pin(async move {
            runtime
                .check()
                .map_err(|e| KnowledgeError::new("KNOWLEDGE_MODEL_UNAVAILABLE", e.message))?;
            let (identity, vectors) = self.embed_observed(texts, input_type, observer).await?;
            Ok(OwnedEmbeddingResult {
                identity,
                vectors,
                accounting_error: None,
                model_call_id: None,
                reused_paid_response: false,
            })
        })
    }
    /// 外部嵌入器兼容入口；生产网关通知 transport 可观察事实。
    fn embed_observed<'a>(
        &'a self,
        texts: Vec<String>,
        input_type: EmbeddingInputType,
        observer: Option<EmbeddingSendObserver<'a>>,
    ) -> EmbedFuture<'a> {
        Box::pin(async move {
            if let Some(observer) = observer {
                observer(SendObservation::AttemptStarted)?;
            }
            let result = self.embed(texts, input_type).await;
            if result.is_ok() {
                if let Some(observer) = observer {
                    observer(SendObservation::ResponseHeadersReceived)?;
                }
            }
            result
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EmbeddingInputType {
    #[default]
    Document,
    Query,
}

/// 生产嵌入器：身份经 ModelGateway 解析（gateway trait，公共端口），调用走
/// OperationService::embed（凭证/配额/台账单一链，R05 HANDOFF embedding 行）。
pub struct GatewayKnowledgeEmbedder {
    gateway: Arc<dyn lingxi_kernel::model_exchange::ModelGatewayPort>,
    operations: Arc<crate::operations::OperationService>,
}

impl GatewayKnowledgeEmbedder {
    pub fn new(
        gateway: Arc<dyn lingxi_kernel::model_exchange::ModelGatewayPort>,
        operations: Arc<crate::operations::OperationService>,
    ) -> Self {
        Self {
            gateway,
            operations,
        }
    }

    fn route(&self) -> Result<lingxi_kernel::model_exchange::ResolvedModelRoute, KnowledgeError> {
        self.gateway
            .resolve_route(
                &lingxi_kernel::model_exchange::ModelRouteRequest::for_operation(
                    lingxi_kernel::model_exchange::ModelOperation::Embedding,
                ),
            )
            .map_err(|error| match error {
                lingxi_kernel::model_exchange::ModelGatewayError::RouteNotConfigured { .. } => {
                    KnowledgeError::new(
                        "KNOWLEDGE_MODEL_UNAVAILABLE",
                        "embedding model is not configured",
                    )
                }
                other => KnowledgeError::new("KNOWLEDGE_MODEL_UNAVAILABLE", format!("{other}")),
            })
    }
}

impl KnowledgeEmbedder for GatewayKnowledgeEmbedder {
    fn settle_pending<'a>(
        &'a self,
        cause: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), KnowledgeError>> + Send + 'a>>
    {
        Box::pin(async move {
            self.operations
                .settle_embedding_usage_for_cause(cause)
                .await
                .map_err(|e| KnowledgeError::storage_invalid(e.message))
        })
    }
    fn settle_pending_owned<'a>(
        &'a self,
        runtime: &'a crate::operations::EmbeddingCallRuntime,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), KnowledgeError>> + Send + 'a>>
    {
        Box::pin(async move {
            self.operations
                .settle_embedding_usage_for_runtime(runtime)
                .await
                .map_err(|e| KnowledgeError::storage_invalid(e.message))
        })
    }
    fn embed_owned<'a>(
        &'a self,
        texts: Vec<String>,
        input_type: EmbeddingInputType,
        runtime: &'a crate::operations::EmbeddingCallRuntime,
        observer: Option<EmbeddingSendObserver<'a>>,
    ) -> OwnedEmbedFuture<'a> {
        Box::pin(async move {
            let request = embedding_request(texts, input_type);
            let on_send = |observation| {
                observer
                    .map(|observer| {
                        observer(observation).map_err(|e| {
                            lingxi_protocol::ProtocolError::new(
                                lingxi_protocol::ErrorCode::Internal,
                                e.to_string(),
                                false,
                            )
                        })
                    })
                    .unwrap_or(Ok(()))
            };
            let result = self
                .operations
                .embed_owned_observed(request, runtime, Some(&on_send))
                .await
                .map_err(|e| KnowledgeError::new("KNOWLEDGE_MODEL_UNAVAILABLE", e.message))?;
            let served = result.served;
            let dimensions = served
                .outcome
                .vectors
                .first()
                .map(|v| v.len() as u32)
                .unwrap_or(0);
            let identity = EmbeddingModelIdentity::with_parameters(
                &served.route.provider,
                &served.route.model,
                served.route.protocol.config_name(),
                dimensions,
                served.parameter_fingerprint,
            );
            Ok(OwnedEmbeddingResult {
                identity,
                vectors: served.outcome.vectors,
                accounting_error: result
                    .accounting_error
                    .map(|e| KnowledgeError::storage_invalid(e.message)),
                model_call_id: Some(result.model_call_id),
                reused_paid_response: result.reused_paid_response,
            })
        })
    }
    fn identity(&self) -> Result<Option<EmbeddingModelIdentity>, KnowledgeError> {
        match self.route() {
            // 配置可比，但零维度明确表示尚未发送/实测，不可据此声明 ready 命中。
            Ok(route) => {
                let request = embedding_request(Vec::new(), EmbeddingInputType::Document);
                Ok(Some(EmbeddingModelIdentity::with_parameters(
                    &route.provider,
                    &route.model,
                    route.protocol.config_name(),
                    0,
                    lingxi_adapters::models::operations::embedding::embedding_space_fingerprint(
                        &route, &request,
                    ),
                )))
            }
            Err(err) => match err.code.as_str() {
                "KNOWLEDGE_MODEL_UNAVAILABLE" => Ok(None),
                _ => Err(err),
            },
        }
    }

    fn embed<'a>(
        &'a self,
        texts: Vec<String>,
        input_type: EmbeddingInputType,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<(EmbeddingModelIdentity, Vec<Vec<f32>>), KnowledgeError>,
                > + Send
                + 'a,
        >,
    > {
        self.embed_observed(texts, input_type, None)
    }

    fn embed_observed<'a>(
        &'a self,
        texts: Vec<String>,
        input_type: EmbeddingInputType,
        observer: Option<EmbeddingSendObserver<'a>>,
    ) -> EmbedFuture<'a> {
        Box::pin(async move {
            let request = embedding_request(texts, input_type);
            let on_send = |observation| {
                observer
                    .map(|observer| {
                        observer(observation).map_err(|error| {
                            lingxi_protocol::ProtocolError::new(
                                lingxi_protocol::ErrorCode::Internal,
                                error.to_string(),
                                false,
                            )
                        })
                    })
                    .unwrap_or(Ok(()))
            };
            let outcome = self
                .operations
                .embed_served_observed(request, None, None, Some(&on_send))
                .await
                .map_err(|failure| {
                    KnowledgeError::new("KNOWLEDGE_MODEL_UNAVAILABLE", failure.message)
                })?;
            let route = outcome.route;
            let vectors = outcome.outcome.vectors;
            let Some(first) = vectors.first() else {
                return Err(KnowledgeError::retrieval_unavailable(
                    "Embedding response does not match the requested batch",
                ));
            };
            let dimensions = first.len() as u32;
            if vectors
                .iter()
                .any(|vector| vector.len() as u32 != dimensions)
            {
                return Err(KnowledgeError::retrieval_unavailable(
                    "Embedding response contains an invalid vector",
                ));
            }
            let identity = EmbeddingModelIdentity::with_parameters(
                &route.provider,
                &route.model,
                route.protocol.config_name(),
                dimensions,
                outcome.parameter_fingerprint,
            );
            Ok((identity, vectors))
        })
    }
}

fn embedding_request(
    texts: Vec<String>,
    input_type: EmbeddingInputType,
) -> lingxi_adapters::models::operations::embedding::EmbeddingRequest {
    lingxi_adapters::models::operations::embedding::EmbeddingRequest {
        inputs: texts,
        dimensions: None,
        context_window: None,
        input_type: match input_type {
            EmbeddingInputType::Document => {
                lingxi_adapters::models::operations::embedding::EmbeddingInputType::Document
            }
            EmbeddingInputType::Query => {
                lingxi_adapters::models::operations::embedding::EmbeddingInputType::Query
            }
        },
    }
}

fn validate_embedding_batch(
    identity: &EmbeddingModelIdentity,
    vectors: &[Vec<f32>],
    count: usize,
) -> Result<(), KnowledgeError> {
    if identity.key.is_empty()
        || identity.provider.is_empty()
        || identity.model_id.is_empty()
        || identity.protocol.is_empty()
        || identity.dimensions == 0
        || identity.dimensions > 65_536
        || vectors.len() != count
        || vectors.iter().any(|vector| {
            vector.len() != identity.dimensions as usize || vector.iter().any(|v| !v.is_finite())
        })
    {
        return Err(KnowledgeError::retrieval_unavailable(
            "embedding identity or batch shape is invalid",
        ));
    }
    Ok(())
}

// ── 检索请求/结果 ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchChannel {
    Fts,
    Hybrid,
}

#[derive(Debug, Clone)]
pub struct SearchRequest {
    pub scope_id: String,
    pub session_id: String,
    pub query: String,
    pub channel: SearchChannel,
    pub notebook_ids: Vec<String>,
    pub source_ids: Vec<String>,
    pub section_keys: Vec<String>,
    pub limit: usize,
}

#[derive(Debug, Clone)]
pub struct DegradedScope {
    pub reason: String,
    pub detail: String,
    pub parse_artifact_id: String,
    pub source_id: String,
    pub source_name: String,
}

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub source_id: String,
    pub source_name: String,
    pub parse_artifact_id: String,
    pub chunk_id: String,
    pub section_id: Option<String>,
    pub heading_path: Vec<String>,
    pub page_number: Option<u64>,
    pub grain: String,
    pub score: f64,
    pub chunk: ks::StoredChunk,
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub mode: String,
    pub degraded: Vec<DegradedScope>,
    pub hits: Vec<SearchHit>,
    pub candidates: Vec<ks::StoredChunk>,
    pub searched_vector_variants: Vec<Value>,
}

/// 摄入 worker 句柄（bootstrap 启动；shutdown 停止）。
#[derive(Default)]
struct WorkerHandle {
    stop: OnceLock<Arc<std::sync::atomic::AtomicBool>>,
    lifecycle: Mutex<()>,
    task: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

pub struct KnowledgeService {
    facts: ks::KnowledgeFactStore,
    index: ks::KnowledgeIndexStore,
    vectors: ks::KnowledgeVectorStore,
    studio_id: String,
    embedder: Mutex<Option<Arc<dyn KnowledgeEmbedder>>>,
    worker: WorkerHandle,
    wake: tokio::sync::Notify,
    write_gate: Mutex<()>,
}

impl std::fmt::Debug for KnowledgeService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KnowledgeService")
            .field("studio_id", &self.studio_id)
            .finish()
    }
}

const KNOWLEDGE_PARSER_ID: &str = "lingxi-citation";
const KNOWLEDGE_PARSER_VERSION: &str = "1";
const EMBED_BATCH: usize = 64;
const MAX_IMPORT_BYTES: usize = 50 * 1024 * 1024;

impl KnowledgeService {
    pub fn new(data_home: &std::path::Path, studio_id: String) -> Result<Self, KnowledgeError> {
        let root = data_home.join("knowledge");
        let facts = ks::KnowledgeFactStore::open(&root)?;
        let index = ks::KnowledgeIndexStore::open(&root)?;
        let vectors = ks::KnowledgeVectorStore::open(&root)?;
        Ok(Self {
            facts,
            index,
            vectors,
            studio_id,
            embedder: Mutex::new(None),
            worker: WorkerHandle::default(),
            wake: tokio::sync::Notify::new(),
            write_gate: Mutex::new(()),
        })
    }

    pub fn studio_id(&self) -> &str {
        &self.studio_id
    }

    pub fn has_table(&self, table: &str) -> bool {
        self.facts.has_table(table)
    }

    /// 生产接线（bootstrap）：真实模型链存在时注入 R05 链嵌入器。
    pub fn set_embedder(&self, embedder: Option<Arc<dyn KnowledgeEmbedder>>) {
        *self.embedder.lock().expect("embedder lock") = embedder;
        // 模型就绪信号：pending_embedding 显式终态的补跑重排（engine.ts:2401-2402
        // onModelConfigMayHaveChanged 的同语义承担）。
        match self.facts.requeue_pending_embedding_jobs() {
            Ok(resumed) if resumed > 0 => self.wake.notify_waiters(),
            Ok(_) => {}
            Err(error) => tracing::error!(%error,"pending embedding requeue failed"),
        }
    }

    /// 启动摄入 worker（bootstrap 调用一次）。
    pub fn start_worker(self: &Arc<Self>) -> Result<(), KnowledgeError> {
        let _lifecycle = self.worker.lifecycle.lock().expect("worker lifecycle lock");
        if self.worker.stop.get().is_some() {
            return Ok(());
        }
        // 标准库排他锁由操作系统释放，不用 PID/过期时间猜测仍活的 worker。
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.facts.root().join("ingestion-worker.lock"))
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        lock.try_lock().map_err(|e| {
            KnowledgeError::conflict(format!("another knowledge worker owns this data root: {e}"))
        })?;
        self.facts.recover_interrupted_jobs()?;
        let stop_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.worker
            .stop
            .set(stop_flag.clone())
            .map_err(|_| KnowledgeError::conflict("worker already started"))?;
        let service = Arc::clone(self);
        let task = tokio::spawn(async move {
            let _ownership = lock;
            loop {
                if stop_flag.load(std::sync::atomic::Ordering::Acquire) {
                    break;
                }
                match service.facts.claim_next_job() {
                    Ok(Some(job)) => service.process_job(job).await,
                    Ok(None) => tokio::select! {
                        _=service.wake.notified()=>{},
                        _=tokio::time::sleep(std::time::Duration::from_millis(100))=>{},
                    },
                    Err(error) => {
                        tracing::error!(%error,"knowledge worker claim failed");
                        tokio::select! {
                            _=service.wake.notified()=>{},
                            _=tokio::time::sleep(std::time::Duration::from_millis(500))=>{},
                        }
                    }
                }
            }
        });
        *self
            .worker
            .task
            .try_lock()
            .expect("worker task lock before first start") = Some(task);
        Ok(())
    }

    pub async fn wait_worker_stopped(&self) -> Result<(), KnowledgeError> {
        self.shutdown();
        // &mut 轮询保留 JoinHandle；取消一个等待者不能让另一个假称任务已退出。
        let mut slot = self.worker.task.lock().await;
        if let Some(task) = slot.as_mut() {
            let outcome = task.await;
            slot.take();
            outcome.map_err(|error| {
                KnowledgeError::storage_invalid(format!("knowledge worker exit failed: {error}"))
            })?;
        }
        Ok(())
    }

    fn ensure_job_active(&self, job: &ks::JobRow) -> Result<(), KnowledgeError> {
        if self
            .worker
            .stop
            .get()
            .is_some_and(|s| s.load(std::sync::atomic::Ordering::Acquire))
        {
            return Err(KnowledgeError::conflict(
                "ingestion worker stopped; checkpoint and unknown requests preserved",
            ));
        }
        let current = self.facts.get_job(&job.id)?;
        if current
            .embedding_stats
            .as_ref()
            .and_then(|stats| stats["ownership"]["deadline_unix_ms"].as_u64())
            .is_some_and(|deadline| lingxi_adapters::models::dispatch::unix_ms_now() >= deadline)
        {
            return Err(KnowledgeError::conflict(
                "knowledge job absolute deadline exhausted; paid checkpoints preserved",
            ));
        }
        if current.status != "running"
            || current.cancelled_at.is_some()
            || current.attempt != job.attempt
        {
            return Err(KnowledgeError::conflict(
                "ingestion job cancelled or no longer owned",
            ));
        }
        self.facts.get_source(&job.source_id)?;
        Ok(())
    }

    pub fn shutdown(&self) {
        if let Some(stop) = self.worker.stop.get() {
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
            self.wake.notify_waiters();
        }
    }

    // ── 笔记本 ──────────────────────────────────────────────────────────────

    pub fn create_notebook(&self, name: &str) -> Result<Value, KnowledgeError> {
        let notebook = self.facts.create_notebook(&self.studio_id, name)?;
        Ok(notebook_json(&notebook))
    }

    pub fn list_notebooks(&self) -> Result<Value, KnowledgeError> {
        let mut notebooks = Vec::new();
        for notebook in self.facts.list_notebooks(&self.studio_id)? {
            let config = self.facts.get_notebook_config(&notebook.id)?;
            let sources = self.facts.list_notebook_sources(&notebook.id)?;
            let mut ingestion = json!({"done": 0, "pendingEmbedding": 0, "processing": 0, "failed": 0, "untracked": 0});
            for entry in &sources {
                let latest = self
                    .facts
                    .latest_job_for_source(&notebook.id, &entry.source.id)?;
                match latest {
                    None => {
                        ingestion["untracked"] =
                            json!(ingestion["untracked"].as_i64().unwrap_or(0) + 1)
                    }
                    Some(job) => match job.status.as_str() {
                        "done" => {
                            ingestion["done"] = json!(ingestion["done"].as_i64().unwrap_or(0) + 1)
                        }
                        "pending_embedding" => {
                            ingestion["pendingEmbedding"] =
                                json!(ingestion["pendingEmbedding"].as_i64().unwrap_or(0) + 1)
                        }
                        "failed" => {
                            ingestion["failed"] =
                                json!(ingestion["failed"].as_i64().unwrap_or(0) + 1)
                        }
                        _ => {
                            ingestion["processing"] =
                                json!(ingestion["processing"].as_i64().unwrap_or(0) + 1)
                        }
                    },
                }
            }
            notebooks.push(json!({
                "id": notebook.id,
                "studioId": notebook.studio_id,
                "name": notebook.name,
                "createdAt": notebook.created_at,
                "updatedAt": notebook.updated_at,
                "config": config_json(&config),
                "chunkTargetCharsEffective": self.effective_chunk_target_chars(&config),
                "sourceCount": sources.len(),
                "ingestion": ingestion,
            }));
        }
        Ok(json!({ "notebooks": notebooks }))
    }

    pub fn get_notebook(&self, notebook_id: &str) -> Result<Value, KnowledgeError> {
        let notebook = self.facts.get_notebook(&self.studio_id, notebook_id)?;
        Ok(notebook_json(&notebook))
    }

    pub fn rename_notebook(&self, notebook_id: &str, name: &str) -> Result<Value, KnowledgeError> {
        let notebook = self
            .facts
            .rename_notebook(&self.studio_id, notebook_id, name)?;
        Ok(notebook_json(&notebook))
    }

    pub fn delete_notebook(&self, notebook_id: &str) -> Result<Value, KnowledgeError> {
        let notebook = self.facts.delete_notebook(&self.studio_id, notebook_id)?;
        Ok(notebook_json(&notebook))
    }

    pub fn get_notebook_config(&self, notebook_id: &str) -> Result<Value, KnowledgeError> {
        self.facts.get_notebook(&self.studio_id, notebook_id)?;
        let config = self.facts.get_notebook_config(notebook_id)?;
        Ok(config_json(&config))
    }

    /// PUT settings：更新后嵌入模型/分块尺寸变化 → 全量重建入队（manager.ts
    /// :1774-1810 同语义；rerank/topK 只影响查询行为，不触发重建）。
    pub fn update_notebook_settings(
        &self,
        notebook_id: &str,
        body: &Value,
    ) -> Result<Value, KnowledgeError> {
        self.facts.get_notebook(&self.studio_id, notebook_id)?;
        let parse_ref =
            |value: Option<&Value>| -> Result<Option<Option<ks::ModelRef>>, KnowledgeError> {
                match value {
                    None | Some(Value::Null) => Ok(None),
                    Some(value) => {
                        let provider = value["provider"].as_str().unwrap_or_default().to_string();
                        let id = value["id"].as_str().unwrap_or_default().to_string();
                        Ok(Some(Some(ks::ModelRef { id, provider })))
                    }
                }
            };
        let chunk = match body.get("chunkTargetChars") {
            None | Some(Value::Null) => None,
            Some(value) => Some(Some(value.as_i64().ok_or_else(|| {
                KnowledgeError::invalid_argument("chunkTargetChars must be an integer")
            })?)),
        };
        let top_k = match body.get("retrievalTopK") {
            None | Some(Value::Null) => None,
            Some(value) => Some(Some(value.as_i64().ok_or_else(|| {
                KnowledgeError::invalid_argument("retrievalTopK must be an integer")
            })?)),
        };
        let retention = match body.get("vectorRetentionDays") {
            None | Some(Value::Null) => None,
            Some(value) => Some(Some(value.as_i64().ok_or_else(|| {
                KnowledgeError::invalid_argument("vectorRetentionDays must be an integer")
            })?)),
        };
        let keys: Vec<String> = body
            .as_object()
            .map(|object| object.keys().cloned().collect())
            .unwrap_or_default();
        const ALLOWED: [&str; 5] = [
            "embeddingModelRef",
            "rerankModelRef",
            "chunkTargetChars",
            "retrievalTopK",
            "vectorRetentionDays",
        ];
        if keys.is_empty() || keys.iter().any(|key| !ALLOWED.contains(&key.as_str())) {
            return Err(KnowledgeError::invalid_argument(
                "Notebook settings body fields are invalid",
            ));
        }
        let (before, after) = self.facts.update_notebook_config(
            notebook_id,
            parse_ref(body.get("embeddingModelRef"))?,
            parse_ref(body.get("rerankModelRef"))?,
            chunk,
            top_k,
            retention,
        )?;
        let embedding_changed = serde_json::to_string(&before.embedding_model_ref)
            .unwrap_or_default()
            != serde_json::to_string(&after.embedding_model_ref).unwrap_or_default();
        let chunk_changed = before.chunk_target_chars != after.chunk_target_chars;
        if embedding_changed || chunk_changed {
            self.rebuild_notebook(notebook_id)?;
        }
        Ok(config_json(&after))
    }

    fn effective_chunk_target_chars(&self, config: &ks::NotebookConfigRow) -> u64 {
        config
            .chunk_target_chars
            .unwrap_or(lingxi_kernel::knowledge::KNOWLEDGE_CHUNK_TARGET_CHARS as i64) as u64
    }

    fn notebook_chunk_profile(&self, notebook_id: &str) -> Result<(usize, String), KnowledgeError> {
        let config = self.facts.get_notebook_config(notebook_id)?;
        let target = self.effective_chunk_target_chars(&config) as usize;
        Ok((
            target,
            chunker_config_id(lingxi_kernel::knowledge::ChunkerStrategy::Markdown, target),
        ))
    }

    /// 全量重建入队（变体以身份并建，旧变体不覆盖——任务书 §十）。
    pub fn rebuild_notebook(&self, notebook_id: &str) -> Result<(), KnowledgeError> {
        let (_, profile) = self.notebook_chunk_profile(notebook_id)?;
        for entry in self.facts.list_notebook_sources(notebook_id)? {
            let artifact_id = entry
                .parse_artifact
                .as_ref()
                .map(|artifact| artifact.id.as_str());
            self.facts.enqueue_source_ingestion(
                notebook_id,
                &entry.source.id,
                artifact_id,
                &profile,
            )?;
        }
        self.wake.notify_waiters();
        Ok(())
    }

    // ── 来源导入 ────────────────────────────────────────────────────────────

    pub fn import_pasted_text(
        &self,
        notebook_id: &str,
        text: &str,
        display_name: Option<&str>,
    ) -> Result<Value, KnowledgeError> {
        let notebook = self.facts.get_notebook(&self.studio_id, notebook_id)?;
        let _ = notebook;
        if text.is_empty() {
            return Err(KnowledgeError::invalid_argument(
                "pasted text must not be empty",
            ));
        }
        if text.len() > MAX_IMPORT_BYTES {
            return Err(KnowledgeError::new(
                "KNOWLEDGE_IMPORT_TOO_LARGE",
                "import too large",
            ));
        }
        let display_name = display_name
            .map(str::to_string)
            .unwrap_or_else(|| "粘贴文本".to_string());
        let (source, snapshot) = self
            .facts
            .create_source_with_snapshot(
                &self.studio_id,
                "pasted_text",
                &display_name,
                &json!({"kind": "pasted_text"}),
                text.as_bytes(),
                "text/plain",
            )
            .map_err(|error| KnowledgeError::storage_invalid(format!("create_source: {error}")))?;
        self.facts
            .add_membership(notebook_id, &source.id)
            .map_err(|error| KnowledgeError::storage_invalid(format!("add_membership: {error}")))?;
        let artifact = self
            .parse_source(&source.id, &snapshot)
            .map_err(|error| KnowledgeError::storage_invalid(format!("parse_source: {error}")))?;
        // 导入+解析完成即入队摄入（chunk→fts_index→embed 异步执行），HTTP 立即
        // 返回（routes L417-429）。
        self.enqueue_ingestion(notebook_id, &source.id, artifact["id"].as_str())?;
        Ok(self.notebook_source_payload(notebook_id, &source, &snapshot, Some(&artifact)))
    }

    /// 入队（解析失败的保底路径 artifactId 为 None；入队后唤醒 worker）。
    fn enqueue_ingestion(
        &self,
        notebook_id: &str,
        source_id: &str,
        artifact_id: Option<&str>,
    ) -> Result<(), KnowledgeError> {
        let (_, profile) = self.notebook_chunk_profile(notebook_id)?;
        self.facts
            .enqueue_source_ingestion(notebook_id, source_id, artifact_id, &profile)?;
        self.wake.notify_waiters();
        Ok(())
    }

    /// 文件导入（file-import-security.ts + knowledge-manager.ts resolveSupportedMimeType
    /// 的 Rust 承担；路径检查拒绝软链/敏感目录，校验失败不把用户路径写进错误）。
    pub fn import_file(
        &self,
        notebook_id: &str,
        file_path: &str,
        display_name: Option<&str>,
    ) -> Result<Value, KnowledgeError> {
        self.facts.get_notebook(&self.studio_id, notebook_id)?;
        let (bytes, mime_type, file_name) = read_secure_import_file(file_path, self.facts.root())?;
        let display_name = display_name
            .map(str::to_string)
            .unwrap_or_else(|| file_name.clone());
        let (source, snapshot) = self.facts.create_source_with_snapshot(
            &self.studio_id,
            "file",
            &display_name,
            &json!({"kind": "file", "fileName": file_name, "importedFrom": file_path}),
            &bytes,
            &mime_type,
        )?;
        self.facts.add_membership(notebook_id, &source.id)?;
        let artifact = self.parse_source(&source.id, &snapshot)?;
        self.enqueue_ingestion(notebook_id, &source.id, artifact["id"].as_str())?;
        Ok(self.notebook_source_payload(notebook_id, &source, &snapshot, Some(&artifact)))
    }

    /// 目录导入（routes L437-486）：递归展开、sha 去重、逐源 parse；部分失败
    /// 显式留痕。
    pub fn import_directory(
        &self,
        notebook_id: &str,
        dir_path: &str,
    ) -> Result<Value, KnowledgeError> {
        self.facts.get_notebook(&self.studio_id, notebook_id)?;
        let path = std::path::Path::new(dir_path);
        if !path.is_absolute() {
            return Err(KnowledgeError::new(
                "KNOWLEDGE_IMPORT_PATH_INVALID",
                "Knowledge import path must be absolute",
            ));
        }
        if is_sensitive_path(path, Some(self.facts.root())) {
            return Err(KnowledgeError::new(
                "KNOWLEDGE_IMPORT_PATH_BLOCKED",
                "Knowledge import path is blocked",
            ));
        }
        let meta = std::fs::metadata(path).map_err(|_| {
            KnowledgeError::new("KNOWLEDGE_IMPORT_NOT_FOUND", "import file was not found")
        })?;
        if !meta.is_dir() {
            return Err(KnowledgeError::new(
                "KNOWLEDGE_IMPORT_FILE_REQUIRED",
                "Knowledge import requires a directory",
            ));
        }
        let mut imported = Vec::new();
        let mut skipped: Vec<Value> = Vec::new();
        let mut failed: Vec<Value> = Vec::new();
        let mut seen_shas: Vec<String> = Vec::new();
        collect_directory_files(path, 0, &mut |file_path| {
            let outcome = (|| -> Result<Value, KnowledgeError> {
                let (bytes, mime_type, file_name) =
                    read_secure_import_file(&file_path, self.facts.root())?;
                let sha = sha256_hex(&bytes);
                if seen_shas.contains(&sha) {
                    return Ok(
                        json!({"skipped": true, "reason": "duplicate_content", "fileName": file_name}),
                    );
                }
                seen_shas.push(sha);
                let (source, _snapshot) = self.facts.create_source_with_snapshot(
                    &self.studio_id,
                    "file",
                    &file_name,
                    &json!({"kind": "file", "fileName": file_name}),
                    &bytes,
                    &mime_type,
                )?;
                self.facts.add_membership(notebook_id, &source.id)?;
                Ok(json!({"sourceId": source.id, "fileName": file_name}))
            })();
            match outcome {
                Ok(value) => {
                    if value.get("skipped").is_some() {
                        skipped.push(value);
                    } else {
                        imported.push(value);
                    }
                }
                Err(err) => failed.push(json!({"fileName": file_path, "error": err.code})),
            }
        });
        // 逐源 parse（与单源导入同链）。
        let mut results = Vec::new();
        for entry in imported {
            let source_id = entry["sourceId"].as_str().unwrap_or_default().to_string();
            match self.parse_source(
                &source_id,
                &self.facts.latest_snapshot_for_source(&source_id)?,
            ) {
                Ok(artifact) => {
                    results.push(json!({"sourceId": source_id, "parseArtifactId": artifact["id"], "ingestion": "enqueued"}));
                    let (_, profile) = self.notebook_chunk_profile(notebook_id)?;
                    self.facts.enqueue_source_ingestion(
                        notebook_id,
                        &source_id,
                        artifact["id"].as_str(),
                        &profile,
                    )?;
                }
                Err(err) => {
                    let (_, profile) = self.notebook_chunk_profile(notebook_id)?;
                    self.facts
                        .enqueue_source_ingestion(notebook_id, &source_id, None, &profile)?;
                    results.push(json!({"sourceId": source_id, "ingestion": "parse_failed_enqueued_for_retry", "error": err.code}));
                }
            }
        }
        self.wake.notify_waiters();
        Ok(json!({"imported": results, "skipped": skipped, "failed": failed}))
    }

    /// 刷新本地文件源（changed 语义；sha 相同 → changed=false 不重建）。
    pub fn refresh_file_source(
        &self,
        notebook_id: &str,
        source_id: &str,
    ) -> Result<Value, KnowledgeError> {
        self.facts.get_notebook(&self.studio_id, notebook_id)?;
        let source = self.facts.get_source(source_id)?;
        if source.source_type != "file" {
            return Err(KnowledgeError::invalid_argument(
                "only file sources can be refreshed",
            ));
        }
        let origin_path = source
            .origin_metadata
            .get("importedFrom")
            .and_then(|value| value.as_str())
            .ok_or_else(|| KnowledgeError::not_found("refreshable file source has no origin path"))?
            .to_string();
        let (bytes, mime_type, file_name) =
            read_secure_import_file(&origin_path, self.facts.root())?;
        let sha = sha256_hex(&bytes);
        let previous = self.facts.latest_snapshot_for_source(source_id)?;
        if previous.sha256 == sha {
            return Ok(
                json!({"changed": false, "source": source_json(&source), "snapshot": snapshot_json(&previous)}),
            );
        }
        let (source, snapshot) = self.facts.create_source_with_snapshot(
            &self.studio_id,
            "file",
            &source.display_name,
            &json!({"kind": "file", "fileName": file_name, "importedFrom": origin_path}),
            &bytes,
            &mime_type,
        )?;
        // 换新源行但继承 membership（refresh 语义：同源新版本快照）。
        self.facts
            .remove_source_from_notebook(notebook_id, source_id)?;
        self.facts.add_membership(notebook_id, &source.id)?;
        let artifact = self.parse_source(&source.id, &snapshot)?;
        self.enqueue_ingestion(notebook_id, &source.id, artifact["id"].as_str())?;
        let mut payload =
            self.notebook_source_payload(notebook_id, &source, &snapshot, Some(&artifact));
        if let Some(object) = payload.as_object_mut() {
            object.insert("changed".into(), json!(true));
        }
        Ok(payload)
    }

    pub fn list_notebook_sources(&self, notebook_id: &str) -> Result<Value, KnowledgeError> {
        self.facts.get_notebook(&self.studio_id, notebook_id)?;
        let mut sources = Vec::new();
        for entry in self.facts.list_notebook_sources(notebook_id)? {
            sources.push(json!({
                "source": source_json(&entry.source),
                "snapshot": snapshot_json(&entry.snapshot),
                "membership": membership_json(&entry.membership),
                "parseArtifact": entry.parse_artifact.as_ref().map(artifact_json_public),
            }));
        }
        Ok(json!({ "sources": sources }))
    }

    /// remove membership：不移除其他笔记本的引用，更不删除源物理文件
    ///（leaf C12CD3）。
    pub fn remove_source_from_notebook(
        &self,
        notebook_id: &str,
        source_id: &str,
    ) -> Result<Value, KnowledgeError> {
        self.facts.get_notebook(&self.studio_id, notebook_id)?;
        let membership = self
            .facts
            .remove_source_from_notebook(notebook_id, source_id)?;
        Ok(json!({"membership": membership_json(&membership)}))
    }

    /// delete wins：取消活跃 job + 清变体/快照（活跃 turn 冻结引用时 409）。
    pub fn delete_source(&self, source_id: &str) -> Result<Value, KnowledgeError> {
        let _write = self.write_gate.try_lock().map_err(|_| {
            KnowledgeError::storage_invalid("knowledge write gate busy; no unbounded wait")
        })?;
        if self.facts.any_active_scope_freezes_source(source_id)? {
            return Err(KnowledgeError::conflict(
                "source is referenced by an active turn scope",
            ));
        }
        self.facts.get_source(source_id)?;
        let cancelled = self.facts.cancel_jobs_for_source(source_id)?;
        let snapshot = self.facts.latest_snapshot_for_source(source_id)?;
        if let Some(artifact) = self
            .facts
            .latest_parse_artifact_for_snapshot(&snapshot.id)?
        {
            self.vectors.remove_artifact(&artifact.id)?;
            self.index.remove_artifact(&artifact.id)?;
            let path = self
                .facts
                .root()
                .join("artifacts")
                .join(&snapshot.id)
                .join(format!("{}.json", artifact.id));
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(KnowledgeError::storage_invalid(error.to_string())),
            }
        }
        let _ = self.facts.delete_source(source_id)?;
        self.facts.purge_source(source_id)?;
        Ok(json!({"cancelledJobs": cancelled, "purge": {"snapshots": true, "indexVariants": true}}))
    }

    /// reingest：failed 重试 / 无 job 兜底入队 / 非 failed 409（manager.ts:1833-1846）。
    pub fn reingest_source(
        &self,
        notebook_id: &str,
        source_id: &str,
    ) -> Result<Value, KnowledgeError> {
        self.facts.get_notebook(&self.studio_id, notebook_id)?;
        let (_, profile) = self.notebook_chunk_profile(notebook_id)?;
        let latest = self.facts.latest_job_for_source(notebook_id, source_id)?;
        match latest {
            None => {
                let job =
                    self.facts
                        .enqueue_source_ingestion(notebook_id, source_id, None, &profile)?;
                self.wake.notify_waiters();
                Ok(json!({"job": job_json(&job), "retried": false}))
            }
            Some(job) => {
                let retried = self.facts.requeue_job(&job.id)?;
                self.wake.notify_waiters();
                Ok(json!({"job": job_json(&retried), "retried": true}))
            }
        }
    }

    pub fn list_ingestion_jobs(
        &self,
        notebook_id: Option<&str>,
        source_id: Option<&str>,
    ) -> Result<Value, KnowledgeError> {
        let jobs = self
            .facts
            .list_ingestion_jobs(notebook_id, source_id, 200)?;
        let counts = self.facts.count_jobs_by_status(notebook_id)?;
        Ok(json!({
            "jobs": jobs.iter().map(job_json).collect::<Vec<_>>(),
            "counts": counts,
        }))
    }

    // ── 解析（citation-grade parser，source-adapters.ts 镜像）────────────────

    /// parse_source：引用级解析 → blocks + 产物 JSON 原子落盘；失败落显式 failed。
    fn parse_source(
        &self,
        source_id: &str,
        snapshot: &ks::SnapshotRow,
    ) -> Result<Value, KnowledgeError> {
        let artifact_id = ks::new_id("kpa");
        let parser_config_hash =
            sha256_hex(format!("{KNOWLEDGE_PARSER_ID}/{KNOWLEDGE_PARSER_VERSION}/v1").as_bytes());
        self.facts
            .begin_parse_artifact(
                &artifact_id,
                &snapshot.id,
                KNOWLEDGE_PARSER_ID,
                KNOWLEDGE_PARSER_VERSION,
                &parser_config_hash,
            )
            .map_err(|error| KnowledgeError::storage_invalid(format!("begin_artifact: {error}")))?;
        let bytes = self.facts.read_snapshot_bytes(snapshot)?;
        match parse_citation_grade_snapshot(&snapshot.mime_type, &bytes, &artifact_id) {
            Ok((status, warnings, blocks)) => {
                if matches!(status.as_str(), "ready") && blocks.is_empty() {
                    self.facts
                        .fail_parse_artifact(&artifact_id, "empty_document")?;
                    return Err(KnowledgeError::new(
                        "KNOWLEDGE_PARSE_FAILED",
                        "Knowledge source parsing failed",
                    ));
                }
                self.facts.insert_blocks(&blocks).map_err(|error| {
                    KnowledgeError::storage_invalid(format!("insert_blocks: {error}"))
                })?;
                let semantic = json!({
                    "schemaVersion": 1,
                    "sourceId": source_id,
                    "contentSnapshotId": snapshot.id,
                    "contentSha256": snapshot.sha256,
                    "parserId": KNOWLEDGE_PARSER_ID,
                    "parserVersion": KNOWLEDGE_PARSER_VERSION,
                    "parserConfigHash": parser_config_hash,
                    "fidelity": "citation_grade",
                    "status": status,
                    "warnings": warnings,
                    "blocks": blocks.iter().map(block_json).collect::<Vec<_>>(),
                });
                let relative = self
                    .facts
                    .write_semantic_artifact(&snapshot.id, &artifact_id, &semantic)
                    .map_err(|error| {
                        KnowledgeError::storage_invalid(format!("write_artifact: {error}"))
                    })?;
                let artifact = self
                    .facts
                    .complete_parse_artifact(
                        &artifact_id,
                        &status,
                        &warnings.iter().map(String::as_str).collect::<Vec<_>>(),
                        "citation_grade",
                        Some(&relative),
                    )
                    .map_err(|error| {
                        KnowledgeError::storage_invalid(format!("complete_artifact: {error}"))
                    })?;
                Ok(artifact_json_public(&artifact))
            }
            Err(reason) => {
                self.facts.fail_parse_artifact(&artifact_id, &reason)?;
                Err(KnowledgeError::new(
                    "KNOWLEDGE_PARSE_FAILED",
                    "Knowledge source parsing failed",
                ))
            }
        }
    }

    pub fn list_artifact_blocks(&self, artifact_id: &str) -> Result<Value, KnowledgeError> {
        let artifact = self.facts.get_parse_artifact(artifact_id)?;
        let snapshot = self.facts.get_snapshot(&artifact.content_snapshot_id)?;
        let source = self.facts.get_source(&snapshot.source_id)?;
        if source.studio_id != self.studio_id {
            return Err(KnowledgeError::not_found(
                "Knowledge parse artifact was not found",
            ));
        }
        let blocks = self.facts.list_artifact_blocks(artifact_id)?;
        Ok(json!({"blocks": blocks.iter().map(block_json).collect::<Vec<_>>()}))
    }

    /// 分块卡片：冻结契约 `{chunkerConfigId, chunks:[{id, ordinal 1-based, ...}]}`。
    pub fn list_artifact_chunk_cards(&self, artifact_id: &str) -> Result<Value, KnowledgeError> {
        let artifact = self.facts.get_parse_artifact(artifact_id)?;
        let snapshot = self.facts.get_snapshot(&artifact.content_snapshot_id)?;
        let source = self.facts.get_source(&snapshot.source_id)?;
        if source.studio_id != self.studio_id {
            return Err(KnowledgeError::not_found(
                "Knowledge parse artifact was not found",
            ));
        }
        let blocks = self.facts.list_artifact_blocks(artifact_id)?;
        let chunker_blocks: Vec<ChunkerBlock> = blocks.iter().map(chunker_block_of).collect();
        let (_, target, config_id) = resolve_chunker_config(&chunker_blocks, None)?;
        let chunks = build_chunks(artifact_id, &chunker_blocks, Some(target))?;
        let cards: Vec<Value> = chunks
            .iter()
            .map(|chunk| {
                let first_span = chunk.spans.first();
                let heading = first_span
                    .and_then(|span| blocks.iter().find(|block| block.id == span.block_id))
                    .map(|block| locator_heading_path(&block.locator))
                    .unwrap_or_default();
                json!({
                    "id": chunk.id,
                    "ordinal": chunk.ordinal + 1,
                    "text": chunk.text,
                    "tokenCount": chunk.token_count,
                    "charCount": chunk.text.chars().count(),
                    "headingPath": if heading.is_empty() { Value::Null } else { json!(heading) },
                })
            })
            .collect();
        Ok(json!({"chunkerConfigId": config_id, "chunks": cards}))
    }

    // ── 引用 / 快照 ─────────────────────────────────────────────────────────

    pub fn create_citation(
        &self,
        artifact_id: &str,
        block_id: &str,
        start_offset: usize,
        end_offset: usize,
    ) -> Result<ks::CitationRow, KnowledgeError> {
        // studio 归属复核（引用不可见/不存在错误——不扩大披露范围）。
        let artifact = self.facts.get_parse_artifact(artifact_id)?;
        let snapshot = self.facts.get_snapshot(&artifact.content_snapshot_id)?;
        let source = self.facts.get_source(&snapshot.source_id)?;
        if source.studio_id != self.studio_id {
            return Err(KnowledgeError::not_found(
                "Knowledge citation was not found",
            ));
        }
        self.facts
            .create_citation(artifact_id, block_id, start_offset, end_offset)
    }

    /// 引用解析：citation + block + artifact + snapshot + source + viewer 定位；
    /// 缺 headingPath 时从章节 metadata 补齐（routes L607-611）。
    pub fn resolve_citation(&self, citation_id: &str) -> Result<Value, KnowledgeError> {
        let resolved = self.facts.resolve_citation(citation_id)?;
        if resolved.source.studio_id != self.studio_id {
            return Err(KnowledgeError::not_found(
                "Knowledge citation was not found",
            ));
        }
        let mut locator = resolved.block.locator.clone();
        let heading_empty = locator_heading_path(&locator).is_empty();
        if heading_empty {
            if let Some(artifact) = self
                .index
                .list_artifact_section_metadata(&resolved.block.parse_artifact_id)?
                .into_iter()
                .find(|section| {
                    section.start_block_ordinal <= resolved.block.ordinal
                        && section.end_block_ordinal >= resolved.block.ordinal
                })
            {
                if !artifact.heading_path.is_empty() {
                    locator["headingPath"] = json!(artifact.heading_path);
                }
            }
        }
        let artifact = self
            .facts
            .get_parse_artifact(&resolved.citation.parse_artifact_id)?;
        Ok(json!({
            "citation": citation_json(&resolved.citation),
            "block": block_json(&resolved.block),
            "parseArtifact": artifact_json_public(&artifact),
            "snapshot": snapshot_json(&resolved.snapshot),
            "source": source_json(&resolved.source),
            "viewer": {
                "contentUrl": format!("/lingxi/v1/knowledge/snapshots/{}/content", resolved.snapshot.id),
                "locator": locator,
            },
        }))
    }

    pub fn read_snapshot_bytes(
        &self,
        snapshot_id: &str,
    ) -> Result<(ks::SnapshotRow, Vec<u8>), KnowledgeError> {
        let snapshot = self.facts.get_snapshot(snapshot_id)?;
        let source = self.facts.get_source(&snapshot.source_id)?;
        if source.studio_id != self.studio_id {
            return Err(KnowledgeError::not_found(
                "Knowledge snapshot was not found",
            ));
        }
        let bytes = self.facts.read_snapshot_bytes(&snapshot)?;
        Ok((snapshot, bytes))
    }

    // ── TurnScope ───────────────────────────────────────────────────────────

    /// 创建轮 scope（会话提交链的生产入口；同会话新一轮 supersede 旧 scope）。
    pub fn create_turn_scope(
        &self,
        session_id: &str,
        turn_id: &str,
        notebook_ids: &[String],
    ) -> Result<ks::TurnScopeRow, KnowledgeError> {
        let scope =
            self.facts
                .create_turn_scope(&self.studio_id, session_id, turn_id, notebook_ids)?;
        Ok(scope)
    }

    pub fn get_turn_scope(&self, scope_id: &str) -> Result<Value, KnowledgeError> {
        let scope = self
            .facts
            .get_turn_scope(scope_id)?
            .ok_or_else(|| KnowledgeError::not_found("Knowledge turn scope was not found"))?;
        Ok(scope_json(&scope))
    }

    /// scope 复核链（knowledge-scope.ts:56-77）：存在、active、studio、会话归属。
    pub fn resolve_turn_scope(
        &self,
        scope_id: &str,
        session_id: &str,
    ) -> Result<ks::TurnScopeRow, KnowledgeError> {
        let scope = self
            .facts
            .get_turn_scope(scope_id)?
            .ok_or_else(|| KnowledgeError::scope_violation("Unknown knowledge turn scope"))?;
        if scope.studio_id != self.studio_id {
            return Err(KnowledgeError::scope_violation(
                "Knowledge turn scope belongs to a different studio",
            ));
        }
        if scope.status != "active" {
            return Err(KnowledgeError::scope_violation(
                "Knowledge turn scope is closed (superseded by a newer turn)",
            ));
        }
        if scope.session_id != session_id {
            return Err(KnowledgeError::scope_violation(
                "Knowledge turn scope does not belong to this session",
            ));
        }
        Ok(scope)
    }

    /// sourceId 必须在 scope 冻结集合内（不信任模型传入的任何 id）。
    pub fn require_scope_source<'a>(
        &self,
        scope: &'a ks::TurnScopeRow,
        source_id: &str,
    ) -> Result<&'a ks::TurnScopeSourceRow, KnowledgeError> {
        scope
            .sources
            .iter()
            .find(|frozen| frozen.source_id == source_id)
            .ok_or_else(|| {
                KnowledgeError::scope_violation("Knowledge source is outside this turn's scope")
            })
    }

    // ── 检索（query-service.ts retrieve 的 Rust 承担 + A14 维度门）────────────

    pub async fn search_in_scope(
        &self,
        request: &SearchRequest,
    ) -> Result<SearchResult, KnowledgeError> {
        static QUERY_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let now = lingxi_adapters::models::dispatch::unix_ms_now();
        let cause = format!(
            "knowledge-query:{}:{}:{}",
            self.studio_id,
            now,
            QUERY_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let runtime = crate::operations::EmbeddingCallRuntime {
            ownership: crate::operations::OperationOwnership {
                principal_kind: "local_user".into(),
                principal_subject: lingxi_kernel::LOCAL_OWNER_SUBJECT.into(),
                agent_id: None,
                session_id: None,
                run_id: None,
                attempt: None,
                parent_tool_call_id: None,
                cause_ref: cause.clone(),
                deadline_unix_ms: now.saturating_add(300_000),
                budget_ms: 300_000,
            },
            cancel: crate::cancel::CancelScope::background_root(&cause),
            request_key: cause,
            manual_retry_authorized: false,
        };
        self.search_in_scope_owned(request, &runtime).await
    }

    pub async fn search_in_scope_owned(
        &self,
        request: &SearchRequest,
        runtime: &crate::operations::EmbeddingCallRuntime,
    ) -> Result<SearchResult, KnowledgeError> {
        runtime
            .check()
            .map_err(|e| KnowledgeError::retrieval_unavailable(e.message))?;
        let limit = request.limit.clamp(1, 24);
        let scope = self.resolve_turn_scope(&request.scope_id, &request.session_id)?;
        let query = request.query.trim();
        if query.is_empty() || query.chars().count() > 4000 {
            return Err(KnowledgeError::invalid_argument(
                "query must contain 1 to 4000 characters",
            ));
        }
        let fts_query = ks::build_fts_literal_query(query);

        // scope 编译：冻结源 → ready 检索锚（(artifact, chunkProfileHash, fingerprint)）。
        #[derive(Clone)]
        struct ReadyScope {
            source_id: String,
            source_name: String,
            artifact_id: String,
            chunk_profile_hash: String,
            variant_id: String,
        }
        let mut ready: Vec<ReadyScope> = Vec::new();
        let mut degraded: Vec<DegradedScope> = Vec::new();
        for frozen in &scope.sources {
            if !request.source_ids.is_empty() && !request.source_ids.contains(&frozen.source_id) {
                continue;
            }
            let Some(artifact_id) = &frozen.parse_artifact_id else {
                degraded.push(DegradedScope {
                    reason: "KNOWLEDGE_PARSE_NOT_READY".into(),
                    detail: "source has no parse artifact".into(),
                    parse_artifact_id: String::new(),
                    source_id: frozen.source_id.clone(),
                    source_name: self
                        .facts
                        .get_source(&frozen.source_id)
                        .map(|source| source.display_name)
                        .unwrap_or_default(),
                });
                continue;
            };
            // 归属笔记本：冻结行引用的选中笔记本 ∩ 请求过滤。
            let owning_notebook = frozen
                .notebook_ids
                .iter()
                .find(|notebook_id| {
                    request.notebook_ids.is_empty() || request.notebook_ids.contains(notebook_id)
                })
                .cloned();
            let Some(notebook_id) = owning_notebook else {
                continue;
            };
            let (target, _) = self.notebook_chunk_profile(&notebook_id)?;
            let blocks = self.facts.list_artifact_blocks(artifact_id)?;
            let chunker_blocks: Vec<ChunkerBlock> = blocks.iter().map(chunker_block_of).collect();
            let fingerprint = block_fingerprint(&chunker_blocks);
            // 与摄入 worker 同一策略推导（首块 locatorType 决定 strategy），
            // profileHash 逐字节同源——查询与索引永不各说各话。
            let profile_hash = resolve_chunker_config(&chunker_blocks, Some(target))
                .map(|(_, _, config_id)| config_id)?;
            match self
                .index
                .get_ready_variant_metadata(artifact_id, &profile_hash)?
            {
                Some((variant_id, _))
                    if self
                        .index
                        .get_variant(&variant_id)?
                        .map(|v| v.block_fingerprint)
                        == Some(fingerprint.clone()) =>
                {
                    ready.push(ReadyScope {
                        source_id: frozen.source_id.clone(),
                        source_name: self
                            .facts
                            .get_source(&frozen.source_id)
                            .map(|source| source.display_name)
                            .unwrap_or_default(),
                        artifact_id: artifact_id.clone(),
                        chunk_profile_hash: profile_hash,
                        variant_id,
                    });
                }
                _ => degraded.push(DegradedScope {
                    reason: "KNOWLEDGE_INDEX_BUILDING".into(),
                    detail: "index stale or missing; rebuild pending".into(),
                    parse_artifact_id: artifact_id.clone(),
                    source_id: frozen.source_id.clone(),
                    source_name: self
                        .facts
                        .get_source(&frozen.source_id)
                        .map(|source| source.display_name)
                        .unwrap_or_default(),
                }),
            }
        }

        // FTS 通道（scope 限定变体集合）。
        let variant_refs: Vec<&str> = ready
            .iter()
            .map(|scope| scope.variant_id.as_str())
            .collect();
        let fts_hits = self
            .index
            .search_fts(&variant_refs, &fts_query, limit * 4)?;

        // 向量通道（hybrid；A14 维度门）。
        let mut vector_hits: Vec<(String, ks::VectorHit)> = Vec::new();
        let mut searched_vector_variants: Vec<Value> = Vec::new();
        let mut mode = "fts".to_string();
        if request.channel == SearchChannel::Hybrid {
            let embedder = self.embedder.lock().expect("embedder lock").clone();
            if let Some(embedder) = embedder {
                if !ready.is_empty() {
                    match embedder
                        .embed_owned(
                            vec![query.to_string()],
                            EmbeddingInputType::Query,
                            runtime,
                            None,
                        )
                        .await
                    {
                        Ok(result) => {
                            if let Some(error) = result.accounting_error {
                                return Err(error);
                            }
                            runtime
                                .check()
                                .map_err(|e| KnowledgeError::retrieval_unavailable(e.message))?;
                            let identity = result.identity;
                            let vectors = result.vectors;
                            validate_embedding_batch(&identity, &vectors, 1)?;
                            let query_vector = &vectors[0];
                            let mut viv_ids: Vec<String> = Vec::new();
                            for scope in &ready {
                                let civ = chunk_index_variant_id(
                                    &scope.artifact_id,
                                    &scope.chunk_profile_hash,
                                );
                                let viv = vector_index_variant_id(&civ, &identity.key);
                                let variant = self.vectors.get_variant(&viv)?;
                                let usable = variant.as_ref().is_some_and(|variant| {
                                    variant.status == "ready"
                                        && variant.parse_artifact_id == scope.artifact_id
                                        && variant.chunk_index_variant_id == civ
                                        && variant.model_key == identity.key
                                        && variant.dimensions == identity.dimensions
                                });
                                if usable {
                                    viv_ids.push(viv.clone());
                                    searched_vector_variants.push(json!({
                                        "parseArtifactId": scope.artifact_id,
                                        "chunkProfileHash": scope.chunk_profile_hash,
                                        "chunkIndexVariantId": civ,
                                        "vectorIndexVariantId": viv,
                                        "dimensions": identity.dimensions,
                                        "parameterFingerprint": identity.parameter_fingerprint,
                                    }));
                                } else {
                                    // A14：变体缺失/非 ready/维度不符 → 显式降级
                                    // + 重建提示，绝不取旧维度向量兜底。
                                    degraded.push(DegradedScope {
                                        reason: "KNOWLEDGE_VECTOR_NOT_READY".into(),
                                        detail: format!(
                                            "vector variant missing or incompatible (model={}; dims={}); rebuild the notebook index",
                                            identity.model_id, identity.dimensions
                                        ),
                                        parse_artifact_id: scope.artifact_id.clone(),
                                        source_id: scope.source_id.clone(),
                                        source_name: scope.source_name.clone(),
                                    });
                                }
                            }
                            if !viv_ids.is_empty() {
                                mode = "hybrid".to_string();
                                let hits =
                                    self.vectors.search(&viv_ids, query_vector, limit * 4)?;
                                for hit in hits {
                                    vector_hits.push((hit.parse_artifact_id.clone(), hit));
                                }
                            }
                        }
                        Err(err) => {
                            runtime
                                .check()
                                .map_err(|e| KnowledgeError::retrieval_unavailable(e.message))?;
                            // 查询嵌入失败 → 显式降级 FTS + 留痕（向量是增强层，
                            // 不炸检索，也不静默换模型）。
                            degraded.push(DegradedScope {
                                reason: "KNOWLEDGE_EMBEDDING_FAILED".into(),
                                detail: format!(
                                    "query embedding request failed ({}); kept FTS candidates",
                                    err.message
                                ),
                                parse_artifact_id: String::new(),
                                source_id: String::new(),
                                source_name: String::new(),
                            });
                        }
                    }
                }
            }
        }

        // RRF 融合（k=60）。
        let fts_ranking: Vec<String> = fts_hits
            .iter()
            .map(|hit| hit.chunk.chunk_id.clone())
            .collect();
        let vector_ranking: Vec<String> = vector_hits
            .iter()
            .map(|(_, hit)| hit.chunk_id.clone())
            .collect();
        let channels: Vec<Vec<String>> = if mode == "hybrid" {
            vec![fts_ranking, vector_ranking]
        } else {
            vec![fts_ranking]
        };
        let fused = lingxi_kernel::knowledge::fuse_rankings(&channels, limit);

        // 候选正文（引用回读用）。
        let mut candidates_by_id: std::collections::HashMap<String, ks::StoredChunk> =
            std::collections::HashMap::new();
        for hit in &fts_hits {
            candidates_by_id.insert(hit.chunk.chunk_id.clone(), hit.chunk.clone());
        }
        for (_, hit) in &vector_hits {
            candidates_by_id
                .entry(hit.chunk_id.clone())
                .or_insert_with(|| ks::StoredChunk {
                    chunk_id: hit.chunk_id.clone(),
                    variant_id: hit.variant_id.clone(),
                    ordinal: 0,
                    section_id: None,
                    text: String::new(),
                    token_count: 0,
                    spans: Vec::new(),
                });
        }
        // 命中组装（grain：span 命中带原文跨度；无跨度的纯标题命中为线索）。
        let mut hits: Vec<SearchHit> = Vec::new();
        for (chunk_id, score) in fused {
            // 分数归一：命中的 chunk 必须能回溯到 scope 源。
            let Some(chunk) = candidates_by_id.get(&chunk_id) else {
                continue;
            };
            let owning = ready
                .iter()
                .find(|scope| scope.variant_id == chunk.variant_id)
                .cloned();
            let Some(scope) = owning else { continue };
            let mut heading_path: Vec<String> = Vec::new();
            let mut page_number: Option<u64> = None;
            if let Some(span) = chunk.spans.first() {
                if let Ok(Some(block)) = self
                    .facts
                    .get_blocks_by_ids(std::slice::from_ref(&span.block_id))
                    .map(|blocks| blocks.into_iter().next())
                {
                    heading_path = locator_heading_path(&block.locator);
                    page_number = block.locator.get("page").and_then(|value| value.as_u64());
                }
            }
            hits.push(SearchHit {
                source_id: scope.source_id.clone(),
                source_name: scope.source_name.clone(),
                parse_artifact_id: scope.artifact_id.clone(),
                chunk_id: chunk.chunk_id.clone(),
                section_id: chunk.section_id.clone(),
                heading_path,
                page_number,
                grain: if chunk.spans.is_empty() {
                    "section".into()
                } else {
                    "span".into()
                },
                score,
                chunk: chunk.clone(),
            });
        }
        let candidates: Vec<ks::StoredChunk> = candidates_by_id.into_values().collect();
        runtime
            .check()
            .map_err(|e| KnowledgeError::retrieval_unavailable(e.message))?;
        Ok(SearchResult {
            mode,
            degraded,
            hits,
            candidates,
            searched_vector_variants,
        })
    }

    /// 引用分页读取（readKnowledgeCitationPage 的 Rust 承担）：spans + 引用签发
    ///（程序直接绑定冻结原文，模型只使用返回的链接）。
    #[allow(clippy::too_many_arguments)] // readKnowledgeCitationPage 的参数面镜像（TS 9 参）
    pub fn read_citation_page(
        &self,
        scope: &ks::TurnScopeRow,
        session_id: &str,
        source_id: &str,
        parse_artifact_id: &str,
        ranges: &[(String, usize, usize)],
        offset: usize,
        max_chars: usize,
    ) -> Result<Value, KnowledgeError> {
        // 签发前重查 active（检索过程中可能开始了下一轮）。
        let current = self.resolve_turn_scope(&scope.id, session_id)?;
        let frozen = self.require_scope_source(&current, source_id)?;
        if frozen.parse_artifact_id.as_deref() != Some(parse_artifact_id) {
            return Err(KnowledgeError::scope_violation(
                "Read positions are outside the frozen source",
            ));
        }
        if !(256..=8000).contains(&max_chars) {
            return Err(KnowledgeError::invalid_argument(
                "maxChars must be an integer from 256 to 8000",
            ));
        }
        let block_ids: Vec<String> = ranges
            .iter()
            .map(|(block_id, _, _)| block_id.clone())
            .collect();
        let blocks: std::collections::HashMap<String, ks::BlockRow> = self
            .facts
            .get_blocks_by_ids(&block_ids)?
            .into_iter()
            .map(|block| (block.id.clone(), block))
            .collect();
        let mut ordered: Vec<(ks::BlockRow, usize, usize)> = Vec::new();
        for (block_id, start, end) in ranges {
            let Some(block) = blocks.get(block_id) else {
                return Err(KnowledgeError::index_invalid(
                    "Read range is outside its frozen raw block",
                ));
            };
            let end = *end.min(&(block.text.chars().count()));
            if end < *start {
                return Err(KnowledgeError::index_invalid(
                    "Read range is outside its frozen raw block",
                ));
            }
            ordered.push((block.clone(), *start, end));
        }
        ordered.sort_by(|left, right| {
            left.0
                .ordinal
                .cmp(&right.0.ordinal)
                .then(left.1.cmp(&right.1))
        });
        // 合并重叠区间。
        let mut merged: Vec<(ks::BlockRow, usize, usize)> = Vec::new();
        for (block, start, end) in ordered {
            if end == start {
                continue;
            }
            match merged.last_mut() {
                Some((previous_block, _, previous_end))
                    if previous_block.id == block.id && start <= *previous_end =>
                {
                    *previous_end = (*previous_end).max(end);
                }
                _ => merged.push((block, start, end)),
            }
        }
        let total_chars: usize = merged.iter().map(|(_, start, end)| end - start).sum();
        if offset > total_chars {
            return Err(KnowledgeError::invalid_argument(
                "offset exceeds the selected original text",
            ));
        }
        let mut spans: Vec<Value> = Vec::new();
        let mut skip = offset;
        let mut returned_chars = 0_usize;
        for (block, start, end) in &merged {
            let length = end - start;
            if skip >= length {
                skip -= length;
                continue;
            }
            let span_start = start + skip;
            skip = 0;
            let span_end = (*end).min(span_start + (max_chars - returned_chars));
            if span_end <= span_start {
                break;
            }
            let text: String = block
                .text
                .chars()
                .skip(span_start)
                .take(span_end - span_start)
                .collect();
            let citation = self.facts.create_citation(
                &block.parse_artifact_id,
                &block.id,
                span_start,
                span_end,
            )?;
            spans.push(json!({
                "blockId": block.id,
                "blockOrdinal": block.ordinal,
                "startOffset": span_start,
                "endOffset": span_end,
                "headingPath": locator_heading_path(&block.locator),
                "pageNumber": block.locator.get("page").and_then(|value| value.as_u64()),
                "text": text,
                "citationId": citation.id,
                "citationMarkdown": format!("[来源 · 原文](#knowledge-citation-{})", citation.id),
            }));
            returned_chars += span_end - span_start;
            if returned_chars >= max_chars {
                break;
            }
        }
        let next_offset = if offset + returned_chars < total_chars {
            json!(offset + returned_chars)
        } else {
            Value::Null
        };
        Ok(json!({
            "spans": spans,
            "offset": offset,
            "returnedChars": returned_chars,
            "totalChars": total_chars,
            "truncated": !next_offset.is_null(),
            "nextOffset": next_offset,
        }))
    }

    /// grep：确定性原文扫描（literal / regexp），命中带 provenance。
    pub fn grep_scope(
        &self,
        scope_id: &str,
        session_id: &str,
        pattern: &str,
        source_ids: &[String],
        use_regex: bool,
        max_results: usize,
    ) -> Result<Value, KnowledgeError> {
        let scope = self.resolve_turn_scope(scope_id, session_id)?;
        if pattern.is_empty() || pattern.chars().count() > 512 {
            return Err(KnowledgeError::invalid_argument(
                "pattern must contain 1 to 512 characters",
            ));
        }
        let max_results = max_results.clamp(1, 200);
        let regex = if use_regex {
            Some(simple_regex(pattern)?)
        } else {
            None
        };
        let mut matches: Vec<Value> = Vec::new();
        let mut unavailable: Vec<Value> = Vec::new();
        let mut truncated = false;
        'sources: for frozen in &scope.sources {
            if !source_ids.is_empty() && !source_ids.contains(&frozen.source_id) {
                continue;
            }
            let Some(artifact_id) = &frozen.parse_artifact_id else {
                unavailable.push(
                    json!({"sourceId": frozen.source_id, "reason": "KNOWLEDGE_PARSE_NOT_READY"}),
                );
                continue;
            };
            let blocks = self.facts.list_artifact_blocks(artifact_id)?;
            let source_name = self
                .facts
                .get_source(&frozen.source_id)
                .map(|source| source.display_name)
                .unwrap_or_default();
            for block in blocks {
                let chars: Vec<char> = block.text.chars().collect();
                let mut index = 0_usize;
                while index < chars.len() {
                    let matched_len = if let Some(regex) = &regex {
                        // 简化正则逐位置匹配（literal 常走路径在下方）。
                        regex.find_at(&chars, index)
                    } else {
                        let pattern_chars: Vec<char> = pattern.chars().collect();
                        if chars[index..].starts_with(&pattern_chars[..]) {
                            Some(pattern_chars.len())
                        } else {
                            None
                        }
                    };
                    if let Some(length) = matched_len {
                        let snippet_start = index.saturating_sub(96);
                        let snippet_end = (index + length + 96).min(chars.len());
                        let snippet: String = chars[snippet_start..snippet_end].iter().collect();
                        matches.push(json!({
                            "sourceId": frozen.source_id,
                            "sourceName": source_name,
                            "parseArtifactId": artifact_id,
                            "blockId": block.id,
                            "blockOrdinal": block.ordinal,
                            "startOffset": index,
                            "endOffset": index + length,
                            "lineNumber": block.locator.get("lineStart").and_then(|value| value.as_u64()),
                            "headingPath": locator_heading_path(&block.locator),
                            "snippet": snippet,
                        }));
                        if matches.len() >= max_results {
                            truncated = true;
                            break 'sources;
                        }
                        index += length;
                    } else {
                        index += 1;
                    }
                }
            }
        }
        Ok(json!({"matches": matches, "truncated": truncated, "unavailableSources": unavailable}))
    }

    /// outline：目录元数据（章节 + chunk 计数；不重新拆覆盖单元）。
    pub fn outline_scope(
        &self,
        scope_id: &str,
        session_id: &str,
        notebook_id: Option<&str>,
        source_id: Option<&str>,
    ) -> Result<Value, KnowledgeError> {
        let scope = self.resolve_turn_scope(scope_id, session_id)?;
        let mut notebooks_out: Vec<Value> = Vec::new();
        for notebook in &scope.notebook_ids {
            if notebook_id.is_some() && notebook_id != Some(notebook.as_str()) {
                continue;
            }
            let notebook_row = self.facts.get_notebook(&scope.studio_id, notebook)?;
            let (target, _) = self.notebook_chunk_profile(notebook)?;
            let mut sources_out: Vec<Value> = Vec::new();
            for frozen in scope
                .sources
                .iter()
                .filter(|frozen| frozen.notebook_ids.contains(notebook))
            {
                if source_id.is_some() && source_id != Some(frozen.source_id.as_str()) {
                    continue;
                }
                let source = self.facts.get_source(&frozen.source_id)?;
                let mut sections_out: Vec<Value> = Vec::new();
                let mut chunk_count = 0_usize;
                let mut status = "parse_pending";
                if let Some(artifact_id) = &frozen.parse_artifact_id {
                    status = "ready";
                    let profile_hash = self
                        .facts
                        .list_artifact_blocks(artifact_id)
                        .ok()
                        .map(|blocks| {
                            let chunker_blocks: Vec<ChunkerBlock> =
                                blocks.iter().map(chunker_block_of).collect();
                            resolve_chunker_config(&chunker_blocks, Some(target))
                                .map(|(_, _, config_id)| config_id)
                                .ok()
                        })
                        .unwrap_or_default();
                    if let Some((variant_id, count)) = profile_hash
                        .and_then(|profile_hash| {
                            self.index
                                .get_ready_variant_metadata(artifact_id, &profile_hash)
                                .ok()
                        })
                        .flatten()
                    {
                        chunk_count = count;
                        sections_out = self
                            .index
                            .list_artifact_section_metadata(artifact_id)?
                            .iter()
                            .map(|section| {
                                json!({
                                    "sectionId": section.id,
                                    "ordinal": section.section_ordinal + 1,
                                    "headingPath": section.heading_path,
                                    "startBlockOrdinal": section.start_block_ordinal,
                                    "endBlockOrdinal": section.end_block_ordinal,
                                })
                            })
                            .collect();
                        let _ = variant_id;
                    } else {
                        status = "index_building";
                    }
                }
                sources_out.push(json!({
                    "sourceId": source.id,
                    "sourceName": source.display_name,
                    "status": status,
                    "chunkCount": chunk_count,
                    "sections": sections_out,
                }));
            }
            notebooks_out.push(json!({
                "notebookId": notebook_row.id,
                "notebookName": notebook_row.name,
                "sources": sources_out,
            }));
        }
        Ok(json!({"scopeId": scope.id, "notebooks": notebooks_out}))
    }

    // ── 摄入 worker ─────────────────────────────────────────────────────────

    fn prepare_job_plan(
        &self,
        job: &ks::JobRow,
    ) -> Result<(String, String, String, Vec<ks::StoredChunk>), KnowledgeError> {
        let _write = self.write_gate.try_lock().map_err(|_| {
            KnowledgeError::storage_invalid("knowledge write gate busy; no unbounded wait")
        })?;
        self.ensure_job_active(job)?;
        let (target_chars, _) = self.notebook_chunk_profile(&job.notebook_id)?;
        let artifact_id = match &job.artifact_id {
            Some(id) => id.clone(),
            None => {
                let snapshot = self.facts.latest_snapshot_for_source(&job.source_id)?;
                let artifact = self.parse_source(&job.source_id, &snapshot)?;
                artifact["id"]
                    .as_str()
                    .ok_or_else(|| KnowledgeError::index_invalid("parse produced no artifact id"))?
                    .to_string()
            }
        };
        self.facts.set_job_phase(&job.id, "chunk")?;
        let blocks = self.facts.list_artifact_blocks(&artifact_id)?;
        let blocks = blocks.iter().map(chunker_block_of).collect::<Vec<_>>();
        let fingerprint = block_fingerprint(&blocks);
        let (_, _, profile) = resolve_chunker_config(&blocks, Some(target_chars))?;
        let planned = build_chunks(&artifact_id, &blocks, Some(target_chars))?;
        if !self
            .index
            .has_artifact_fingerprint(&artifact_id, &profile, &fingerprint)?
        {
            let sections = lingxi_kernel::knowledge::build_sections(&artifact_id, &blocks)
                .iter()
                .map(|section| ks::SectionMetaRow {
                    id: section.id.clone(),
                    section_ordinal: section.section_ordinal,
                    heading_path: section.heading_path.clone(),
                    start_block_ordinal: section.start_block_ordinal,
                    end_block_ordinal: section.end_block_ordinal,
                })
                .collect::<Vec<_>>();
            self.index.replace_artifact_chunks(
                &artifact_id,
                &profile,
                &fingerprint,
                &planned,
                &sections,
            )?;
        }
        let civ = chunk_index_variant_id(&artifact_id, &profile);
        let chunks = self.index.list_variant_chunks(&civ)?;
        // 计划来自权威原文；缓存里等量错ID/正文也不能定义另一套“正确计划”。
        if chunks.is_empty()
            || chunks.len() != planned.len()
            || chunks.iter().zip(&planned).any(|(stored, planned)| {
                stored.chunk_id != planned.id
                    || stored.ordinal != planned.ordinal
                    || stored.text != planned.text
            })
        {
            return Err(KnowledgeError::index_invalid(
                "chunk cache does not match the authoritative source plan",
            ));
        }
        let mut hash = String::new();
        for chunk in &chunks {
            hash.push_str(&chunk.chunk_id);
            hash.push('\0');
            hash.push_str(&chunk.text);
            hash.push('\0');
        }
        self.facts.set_job_phase(&job.id, "embed")?;
        Ok((artifact_id, civ, sha256_hex(hash.as_bytes()), chunks))
    }

    async fn process_job(&self, job: ks::JobRow) {
        let result = async {
            let mut initial = job.embedding_stats.clone().unwrap_or_else(|| json!({}));
            if initial.get("ownership").is_none() {
                let now = lingxi_adapters::models::dispatch::unix_ms_now();
                initial["ownership"] =
                    serde_json::to_value(crate::operations::OperationOwnership {
                        principal_kind: "local_user".into(),
                        principal_subject: lingxi_kernel::LOCAL_OWNER_SUBJECT.into(),
                        agent_id: None,
                        session_id: None,
                        run_id: None,
                        attempt: Some(format!("job:{}#a{}", job.id, job.attempt)),
                        parent_tool_call_id: None,
                        cause_ref: format!("knowledge-job:{}:{}", self.studio_id, job.id),
                        deadline_unix_ms: now.saturating_add(300_000),
                        budget_ms: 300_000,
                    })
                    .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
                self.facts.set_job_embedding_stats(&job.id, &initial)?;
            }
            let mut ownership: crate::operations::OperationOwnership =
                serde_json::from_value(initial["ownership"].clone()).map_err(|e| {
                    KnowledgeError::storage_invalid(format!("knowledge job ownership invalid: {e}"))
                })?;
            ownership.attempt = Some(format!("job:{}#a{}", job.id, job.attempt));
            let runtime = crate::operations::EmbeddingCallRuntime {
                cancel: crate::cancel::CancelScope::background_root(&ownership.cause_ref),
                request_key: job.id.clone(),
                manual_retry_authorized: initial["manualRetryAttempt"].as_i64()
                    == Some(job.attempt),
                ownership,
            };
            runtime
                .check()
                .map_err(|e| KnowledgeError::conflict(e.message))?;
            let (artifact_id, civ, fingerprint, chunks) = self.prepare_job_plan(&job)?;
            let embedder = self.embedder.lock().expect("embedder lock").clone();
            let Some(embedder) = embedder else {
                self.facts.mark_job_pending_embedding(&job.id)?;
                return Ok(());
            };
            runtime
                .check()
                .map_err(|e| KnowledgeError::conflict(e.message))?;
            self.ensure_job_active(&job)?;
            // 等待补账时同样监督原作业；取消后等待真实 IO 有界收尾，不能丢 future 冒认停止。
            let settlement = embedder.settle_pending_owned(&runtime);
            tokio::pin!(settlement);
            tokio::select! {
                result=&mut settlement=>result?,
                cancelled=async {
                    loop {
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        if let Err(error)=self.ensure_job_active(&job) {break error;}
                    }
                }=>{
                    runtime.cancel.cancel(&cancelled.to_string());
                    settlement.await?;
                    return Err(cancelled);
                },
            };
            runtime
                .check()
                .map_err(|e| KnowledgeError::conflict(e.message))?;
            self.embed_planned_job(
                &job,
                (&artifact_id, &civ, &fingerprint, &chunks),
                embedder.as_ref(),
                &runtime,
            )
            .await
        }
        .await;
        if let Err(error) = result {
            tracing::error!(job_id=%job.id,%error,"knowledge ingestion job did not complete");
            if !self
                .worker
                .stop
                .get()
                .is_some_and(|s| s.load(std::sync::atomic::Ordering::Acquire))
            {
                let Ok(_write) = self.write_gate.try_lock() else {
                    tracing::error!(job_id=%job.id,%error,"knowledge job failure persistence unconfirmed: write gate busy");
                    return;
                };
                let mut failure = error.to_string();
                if let Ok(current) = self.facts.get_job(&job.id) {
                    if current.status == "running"
                        && current.cancelled_at.is_none()
                        && current.attempt == job.attempt
                    {
                        if let Some(mut stats) = current.embedding_stats {
                            if let Some(variant) =
                                stats["checkpointVariantId"].as_str().map(str::to_string)
                            {
                                match self.vectors.checkpoint_entries(&variant) {
                                    Ok(entries) => {
                                        let reused = stats["chunksResumedFromCheckpoint"]
                                            .as_u64()
                                            .unwrap_or(0)
                                            + stats["chunksReusedFromReadyVariant"]
                                                .as_u64()
                                                .unwrap_or(0);
                                        let written = (entries.len() as u64).saturating_sub(reused);
                                        stats["chunksNewlyEmbedded"] = json!(written);
                                        stats["actualWrittenChunkCount"] = json!(written);
                                        if let Err(error) =
                                            self.facts.set_job_embedding_stats(&job.id, &stats)
                                        {
                                            failure.push_str(&format!("; confirmed checkpoint count={}, persisted statistics unavailable: {error}",entries.len()));
                                        }
                                    }
                                    Err(error) => failure.push_str(&format!(
                                        "; checkpoint validation failed: {error}"
                                    )),
                                }
                                if let Err(error) = self.vectors.fail_build(&variant) {
                                    failure.push_str(&format!(
                                        "; variant failure status unavailable: {error}"
                                    ));
                                }
                            }
                        }
                    }
                }
                if let Err(storage_error) = self.facts.fail_job(&job.id, &failure) {
                    tracing::error!(%storage_error,"knowledge job failure could not be persisted");
                }
            }
        }
    }

    async fn embed_planned_job(
        &self,
        job: &ks::JobRow,
        plan: (&str, &str, &str, &[ks::StoredChunk]),
        embedder: &dyn KnowledgeEmbedder,
        runtime: &crate::operations::EmbeddingCallRuntime,
    ) -> Result<(), KnowledgeError> {
        let (artifact_id, civ, fingerprint, chunks) = plan;
        let Some(configured) = embedder.identity()? else {
            self.facts.mark_job_pending_embedding(&job.id)?;
            return Ok(());
        };
        let plan = chunks
            .iter()
            .map(|c| (c.chunk_id.clone(), c.ordinal))
            .collect::<Vec<_>>();
        let expected = plan
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        if expected.len() != chunks.len() {
            return Err(KnowledgeError::index_invalid("duplicate planned chunks"));
        }
        // 零维度只是未发送配置。只有相同完整指纹下、已实测且字节有效的旧变体
        // 才能提供维度；未知/旧身份不凭同名模型冒认兼容。
        let mut identity = None;
        let mut build = None;
        let mut checkpoint = std::collections::BTreeSet::new();
        let mut ready_reused = false;
        for variant in self.vectors.list_variants_by_artifact(artifact_id)? {
            if variant.chunk_index_variant_id != civ || variant.chunk_fingerprint != fingerprint {
                continue;
            }
            let learned =
                if configured.dimensions == 0 && !configured.parameter_fingerprint.is_empty() {
                    EmbeddingModelIdentity::with_parameters(
                        &configured.provider,
                        &configured.model_id,
                        &configured.protocol,
                        variant.dimensions,
                        configured.parameter_fingerprint.clone(),
                    )
                } else {
                    configured.clone()
                };
            if learned.key != variant.model_key || learned.dimensions != variant.dimensions {
                continue;
            }
            let entries = self.vectors.checkpoint_entries(&variant.variant_id)?;
            checkpoint = entries
                .iter()
                .map(|e| (e.chunk_id.clone(), e.ordinal))
                .collect();
            if checkpoint.len() != entries.len() || !checkpoint.is_subset(&expected) {
                return Err(KnowledgeError::index_invalid(
                    "checkpoint does not match the actual planned chunk set",
                ));
            }
            ready_reused = variant.status == "ready" && checkpoint == expected;
            if variant.status == "ready" && !ready_reused {
                return Err(KnowledgeError::index_invalid("ready variant is incomplete"));
            }
            identity = Some(learned);
            build = Some(ks::BuildBegin {
                variant_id: variant.variant_id,
                reset_stale: false,
            });
            break;
        }
        let reused = checkpoint.len();
        let previous = job.embedding_stats.as_ref();
        let previous_requests = previous
            .and_then(|v| v["requestCount"].as_u64())
            .unwrap_or(0)
            + previous
                .and_then(|v| v["previousRequestCount"].as_u64())
                .unwrap_or(0);
        let previous_unknown = previous
            .and_then(|v| v["unknownRequestsFromPreviousAttempt"].as_u64())
            .unwrap_or(0)
            + previous
                .and_then(|v| v["pendingRequestCount"].as_u64())
                .unwrap_or(0)
            + previous
                .and_then(|v| v["unknownSentRequestCount"].as_u64())
                .unwrap_or(0);
        let previous_transport = previous
            .and_then(|v| v["transportAttemptCount"].as_u64())
            .unwrap_or(0)
            + previous
                .and_then(|v| v["previousTransportAttemptCount"].as_u64())
                .unwrap_or(0);
        let previous_not_sent = previous
            .and_then(|v| v["knownNotSentAttemptCount"].as_u64())
            .unwrap_or(0)
            + previous
                .and_then(|v| v["previousKnownNotSentAttemptCount"].as_u64())
                .unwrap_or(0);
        let mut stats = json!({"chunksNewlyEmbedded":0,"chunksResumedFromCheckpoint":if ready_reused{0}else{reused},"chunksReusedFromReadyVariant":if ready_reused{reused}else{0},"requestCount":0,"requestCountSemantics":"confirmed_or_possibly_sent_excluding_known_not_sent","transportAttemptCount":0,"confirmedResponseRequestCount":0,"knownNotSentAttemptCount":0,"unknownSentRequestCount":0,"failedTransportAttemptCount":0,"callAttemptCount":0,"pendingCallIntentCount":0,"successfulRequestCount":0,"failedRequestCount":0,"failedCallCount":0,"pendingRequestCount":0,"previousRequestCount":previous_requests,"previousTransportAttemptCount":previous_transport,"previousKnownNotSentAttemptCount":previous_not_sent,"unknownRequestsFromPreviousAttempt":previous_unknown,"plannedChunkCount":chunks.len(),"actualWrittenChunkCount":0,"attempt":job.attempt,"resetStaleVectors":false,"model":null,"abandonedStaleVariantId":null});
        stats["ownership"] = serde_json::to_value(&runtime.ownership)
            .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?;
        stats["modelCallIds"] = json!([]);
        stats["previousModelCallIds"] = json!(previous
            .into_iter()
            .flat_map(|v| ["modelCallIds", "previousModelCallIds"]
                .into_iter()
                .flat_map(move |key| v[key].as_array().into_iter().flatten().cloned()))
            .collect::<Vec<_>>());
        stats["paidResponsesReused"] = json!(0);
        {
            let _write = self.write_gate.try_lock().map_err(|_| {
                KnowledgeError::storage_invalid("knowledge write gate busy; no unbounded wait")
            })?;
            self.ensure_job_active(job)?;
            self.facts.set_job_embedding_stats(&job.id, &stats)?;
            self.facts
                .set_job_progress(&job.id, reused as i64, Some(chunks.len() as i64))?;
        }
        let missing = chunks
            .iter()
            .filter(|c| !checkpoint.contains(&(c.chunk_id.clone(), c.ordinal)))
            .collect::<Vec<_>>();
        for batch in missing.chunks(EMBED_BATCH) {
            {
                let _write = self.write_gate.try_lock().map_err(|_| {
                    KnowledgeError::storage_invalid("knowledge write gate busy; no unbounded wait")
                })?;
                self.ensure_job_active(job)?;
                stats["callAttemptCount"] = json!(stats["callAttemptCount"].as_u64().unwrap() + 1);
                stats["pendingCallIntentCount"] = json!(1);
                self.facts.set_job_embedding_stats(&job.id, &stats)?;
            }
            let texts = batch.iter().map(|c| c.text.clone()).collect::<Vec<_>>();
            let requests_before_call = stats["requestCount"].as_u64().unwrap();
            let transport_before_call = stats["transportAttemptCount"].as_u64().unwrap();
            let dispatch_stats = Mutex::new(stats.clone());
            let on_send = |observation| {
                let _write = self.write_gate.try_lock().map_err(|_| {
                    KnowledgeError::storage_invalid("knowledge write gate busy; no unbounded wait")
                })?;
                self.ensure_job_active(job)?;
                let mut stats = dispatch_stats.lock().expect("dispatch statistics lock");
                let mut next = stats.clone();
                match observation {
                    SendObservation::AttemptStarted => {
                        next["transportAttemptCount"] =
                            json!(next["transportAttemptCount"].as_u64().unwrap() + 1);
                        next["requestCount"] = json!(next["requestCount"].as_u64().unwrap() + 1);
                        next["pendingRequestCount"] = json!(1);
                    }
                    SendObservation::ResponseHeadersReceived => {
                        next["confirmedResponseRequestCount"] =
                            json!(next["confirmedResponseRequestCount"].as_u64().unwrap() + 1);
                        next["pendingRequestCount"] = json!(0);
                    }
                    SendObservation::ConnectFailedBeforeSend => {
                        next["knownNotSentAttemptCount"] =
                            json!(next["knownNotSentAttemptCount"].as_u64().unwrap() + 1);
                        next["requestCount"] = json!(next["requestCount"]
                            .as_u64()
                            .unwrap()
                            .checked_sub(1)
                            .expect("transport attempt preceded connect failure"));
                        next["pendingRequestCount"] = json!(0);
                    }
                }
                self.facts.set_job_embedding_stats(&job.id, &next)?;
                *stats = next;
                Ok(())
            };
            let mut batch_runtime = runtime.clone();
            batch_runtime.request_key = format!(
                "{}:{}:{}",
                job.id,
                civ,
                sha256_hex(
                    &serde_json::to_vec(&texts)
                        .map_err(|e| KnowledgeError::storage_invalid(e.to_string()))?
                )
            );
            let result = {
                let future = embedder.embed_owned(
                    texts,
                    EmbeddingInputType::Document,
                    &batch_runtime,
                    Some(&on_send),
                );
                tokio::pin!(future);
                tokio::select! {
                    outcome=&mut future=>outcome,
                    cancelled=async {
                        loop {
                            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                            if let Err(error)=self.ensure_job_active(job) {break error;}
                        }
                    }=>{ batch_runtime.cancel.cancel(&cancelled.to_string()); let settlement=future.await; match settlement { Err(error)=>Err(error),Ok(_)=>Err(cancelled) } },
                }
            };
            stats = dispatch_stats
                .into_inner()
                .expect("dispatch statistics lock");
            stats["pendingCallIntentCount"] = json!(0);
            let potentially_sent_count =
                stats["requestCount"].as_u64().unwrap() - requests_before_call;
            stats["unknownSentRequestCount"] = json!(
                stats["unknownSentRequestCount"].as_u64().unwrap()
                    + stats["pendingRequestCount"].as_u64().unwrap()
            );
            stats["pendingRequestCount"] = json!(0);
            let (served, vectors, accounting_error) = match result {
                Ok(value) => {
                    if let Some(call_id) = &value.model_call_id {
                        stats["modelCallIds"]
                            .as_array_mut()
                            .expect("call identity list")
                            .push(json!(call_id));
                    }
                    if value.reused_paid_response {
                        stats["paidResponsesReused"] =
                            json!(stats["paidResponsesReused"].as_u64().unwrap() + 1);
                    }
                    stats["successfulRequestCount"] = json!(
                        stats["successfulRequestCount"].as_u64().unwrap()
                            + u64::from(!value.reused_paid_response)
                    );
                    (value.identity, value.vectors, value.accounting_error)
                }
                Err(error) => {
                    stats["failedTransportAttemptCount"] = json!(
                        stats["failedTransportAttemptCount"].as_u64().unwrap()
                            + stats["transportAttemptCount"].as_u64().unwrap()
                            - transport_before_call
                    );
                    stats["failedCallCount"] =
                        json!(stats["failedCallCount"].as_u64().unwrap() + 1);
                    stats["failedRequestCount"] = json!(
                        stats["failedRequestCount"].as_u64().unwrap() + potentially_sent_count
                    );
                    self.facts.set_job_embedding_stats(&job.id, &stats)?;
                    if let Some(build) = &build {
                        self.vectors.fail_build(&build.variant_id)?;
                    }
                    return Err(error);
                }
            };
            self.facts.set_job_embedding_stats(&job.id, &stats)?;
            if let Err(error) = validate_embedding_batch(&served, &vectors, batch.len()) {
                if let Some(build) = &build {
                    self.vectors.fail_build(&build.variant_id)?;
                }
                return Err(error);
            }
            if identity
                .as_ref()
                .is_some_and(|identity| *identity != served)
            {
                if let Some(build) = &build {
                    self.vectors.fail_build(&build.variant_id)?;
                }
                return Err(KnowledgeError::conflict("embedding identity changed; preserve old paid results and rebuild a distinct variant"));
            }
            let _write = self.write_gate.try_lock().map_err(|_| {
                KnowledgeError::storage_invalid("knowledge write gate busy; no unbounded wait")
            })?;
            self.ensure_job_active(job)?;
            if identity.is_none() {
                build = Some(self.vectors.begin_variant_build(
                    civ,
                    artifact_id,
                    &served.key,
                    fingerprint,
                    served.dimensions,
                )?);
                identity = Some(served.clone());
            } else if stats["chunksNewlyEmbedded"] == 0 {
                self.vectors.begin_variant_build(
                    civ,
                    artifact_id,
                    &served.key,
                    fingerprint,
                    served.dimensions,
                )?;
            }
            let entries = batch
                .iter()
                .zip(vectors)
                .map(|(c, vector)| ks::VectorEntry {
                    chunk_id: c.chunk_id.clone(),
                    ordinal: c.ordinal,
                    vector,
                })
                .collect::<Vec<_>>();
            let current = build.as_ref().expect("served build identity");
            stats["checkpointVariantId"] = json!(current.variant_id);
            stats["model"] = json!({"key":served.key,"provider":served.provider,"modelId":served.model_id,"protocol":served.protocol,"dimensions":served.dimensions,"parameterFingerprint":served.parameter_fingerprint});
            // 写入前保存变体锚点；写后统计故障可从已确认的字节恢复真实写入数。
            self.facts.set_job_embedding_stats(&job.id, &stats)?;
            if let Err(error) = self.vectors.upsert_batch(&current.variant_id, &entries) {
                self.vectors.fail_build(&current.variant_id)?;
                return Err(error);
            }
            stats["chunksNewlyEmbedded"] =
                json!(stats["chunksNewlyEmbedded"].as_u64().unwrap() + entries.len() as u64);
            stats["actualWrittenChunkCount"] = stats["chunksNewlyEmbedded"].clone();
            self.facts.set_job_embedding_stats(&job.id, &stats)?;
            self.facts.set_job_progress(
                &job.id,
                (reused + stats["chunksNewlyEmbedded"].as_u64().unwrap() as usize) as i64,
                Some(chunks.len() as i64),
            )?;
            if let Some(error) = accounting_error {
                self.vectors.fail_build(&current.variant_id)?;
                return Err(error);
            }
        }
        let _write = self.write_gate.try_lock().map_err(|_| {
            KnowledgeError::storage_invalid("knowledge write gate busy; no unbounded wait")
        })?;
        self.ensure_job_active(job)?;
        let identity = identity
            .ok_or_else(|| KnowledgeError::index_invalid("no confirmed embedding identity"))?;
        let build = build.ok_or_else(|| KnowledgeError::index_invalid("no vector build"))?;
        if let Err(error) = self.vectors.complete_build_plan(&build.variant_id, &plan) {
            self.vectors.fail_build(&build.variant_id)?;
            return Err(error);
        }
        stats["model"] = json!({"key":identity.key,"provider":identity.provider,"modelId":identity.model_id,"protocol":identity.protocol,"dimensions":identity.dimensions,"parameterFingerprint":identity.parameter_fingerprint});
        if let Err(error) = self.facts.complete_job(&job.id, &stats) {
            self.vectors.fail_build(&build.variant_id)?;
            return Err(error);
        }
        Ok(())
    }

    /// A14 证据面：artifact 的向量变体身份清单（model_key/dimensions/status）。
    pub fn vector_variant_report(
        &self,
        artifact_id: &str,
    ) -> Result<Vec<ks::VectorVariantRecord>, KnowledgeError> {
        self.vectors.list_variants_by_artifact(artifact_id)
    }

    fn notebook_source_payload(
        &self,
        notebook_id: &str,
        source: &ks::SourceRow,
        snapshot: &ks::SnapshotRow,
        artifact: Option<&Value>,
    ) -> Value {
        let membership = self.facts.get_membership(notebook_id, &source.id).ok();
        json!({
            "source": source_json(source),
            "snapshot": snapshot_json(snapshot),
            "membership": membership.as_ref().map(membership_json),
            "parseArtifact": artifact,
        })
    }
}

// ── 引用级解析器（source-adapters.ts 镜像）──────────────────────────────────

/// text/plain 与 text/markdown 全保真；html 以标签扫描提取块选择器（jsdom 缺席，
/// D 项）；pdf/处理器格式显式失败（不静默降级为无定位文本）。
pub fn parse_citation_grade_snapshot(
    mime_type: &str,
    bytes: &[u8],
    artifact_id: &str,
) -> Result<(String, Vec<String>, Vec<ks::BlockRow>), String> {
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => text.strip_prefix('\u{feff}').unwrap_or(text).to_string(),
        Err(_) => return Err("invalid_utf8".into()),
    };
    match mime_type {
        "text/plain" => {
            let blocks = parse_line_blocks(&text, "text", false, artifact_id);
            Ok(("ready".into(), Vec::new(), blocks))
        }
        "text/markdown" => {
            let blocks = parse_line_blocks(&text, "markdown", true, artifact_id);
            Ok(("ready".into(), Vec::new(), blocks))
        }
        "text/html" => {
            let blocks = parse_html_blocks(&text, artifact_id);
            Ok(("ready".into(), Vec::new(), blocks))
        }
        other if other.starts_with("application/vnd.lingxi.processor") => {
            Err("processor_unavailable".into())
        }
        "application/pdf" => Err("processor_unavailable".into()),
        _ => Err("unsupported_citation_format".into()),
    }
}

struct LineRef {
    raw: String,
    line_number: usize,
    start: usize,
}

fn normalized_lines(text: &str) -> Vec<LineRef> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut cursor = 0_usize;
    normalized
        .split('\n')
        .enumerate()
        .map(|(index, raw)| {
            let start = cursor;
            cursor += raw.chars().count() + 1;
            LineRef {
                raw: raw.to_string(),
                line_number: index + 1,
                start,
            }
        })
        .collect()
}

fn parse_line_blocks(
    text: &str,
    locator_type: &str,
    markdown: bool,
    artifact_id: &str,
) -> Vec<ks::BlockRow> {
    let mut blocks: Vec<ks::BlockRow> = Vec::new();
    let mut heading_path: Vec<String> = Vec::new();
    for line in normalized_lines(text) {
        let trimmed = line.raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let leading = line.raw.chars().count() - line.raw.trim_start().chars().count();
        let mut text_out = trimmed.to_string();
        let start_in_line = leading;
        if markdown {
            let heading = heading_level(trimmed);
            if let Some((level, title)) = heading {
                heading_path.truncate(level - 1);
                if heading_path.len() < level {
                    heading_path.resize(level, String::new());
                }
                heading_path[level - 1] = title.clone();
                text_out = title;
            }
        }
        let char_start = line.start + start_in_line;
        blocks.push(ks::BlockRow {
            id: format!(
                "kb_{}",
                &sha256_hex(format!("{artifact_id}:{}", blocks.len()).as_bytes())[..24]
            ),
            parse_artifact_id: artifact_id.to_string(),
            ordinal: blocks.len(),
            text: text_out.clone(),
            text_sha256: sha256_hex(text_out.as_bytes()),
            locator_type: locator_type.to_string(),
            locator: json!({
                "headingPath": heading_path.clone(),
                "lineStart": line.line_number,
                "lineEnd": line.line_number,
                "charStart": char_start,
                "charEnd": char_start + text_out.chars().count(),
            }),
        });
    }
    blocks
}

fn heading_level(line: &str) -> Option<(usize, String)> {
    let level = line.chars().take_while(|ch| *ch == '#').count();
    if (1..=6).contains(&level) {
        let rest = line[level..].trim();
        if !rest.is_empty() && line[level..].starts_with(' ') {
            let title = rest.trim_end_matches('#').trim().to_string();
            return Some((level, title));
        }
    }
    None
}

/// HTML 块选择器扫描（h1-h6/p/li/blockquote/pre/td/th；无嵌套 DOM，locator 带
/// headingPath + 序号——结构性 locator 简化，D 项申报）。
fn parse_html_blocks(text: &str, artifact_id: &str) -> Vec<ks::BlockRow> {
    let mut blocks: Vec<ks::BlockRow> = Vec::new();
    let mut heading_path: Vec<String> = Vec::new();
    let bytes = text.as_bytes();
    let mut position = 0_usize;
    let tag_names = [
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "p",
        "li",
        "blockquote",
        "pre",
        "td",
        "th",
    ];
    while position < bytes.len() {
        let Some(open_start) = find_sub(&bytes[position..], b"<") else {
            break;
        };
        let abs = position + open_start;
        let Some(open_end_rel) = find_sub(&bytes[abs..], b">") else {
            break;
        };
        let open_end = abs + open_end_rel;
        let tag = String::from_utf8_lossy(&bytes[abs + 1..open_end]).to_lowercase();
        let name = tag
            .split(|ch: char| ch.is_whitespace() || ch == '>')
            .next()
            .unwrap_or("")
            .to_string();
        let is_block = tag_names.contains(&name.as_str());
        if !is_block || tag.starts_with('/') {
            position = open_end;
            continue;
        }
        let closing = format!("</{name}");
        let Some(close_rel) = find_sub(&bytes[open_end..], closing.as_bytes()) else {
            break;
        };
        let close_abs = open_end + close_rel;
        let inner = String::from_utf8_lossy(&bytes[open_end..close_abs]).to_string();
        let plain = strip_tags(&inner);
        let condensed = plain.split_whitespace().collect::<Vec<_>>().join(" ");
        position = close_abs;
        if condensed.is_empty() {
            continue;
        }
        if name.len() == 2 && name.starts_with('h') {
            if let Ok(level) = name[1..].parse::<usize>() {
                heading_path.truncate(level - 1);
                if heading_path.len() < level {
                    heading_path.resize(level, String::new());
                }
                heading_path[level - 1] = condensed.clone();
            }
        }
        blocks.push(ks::BlockRow {
            id: format!(
                "kb_{}",
                &sha256_hex(format!("{artifact_id}:{}", blocks.len()).as_bytes())[..24]
            ),
            parse_artifact_id: artifact_id.to_string(),
            ordinal: blocks.len(),
            text: condensed.clone(),
            text_sha256: sha256_hex(condensed.as_bytes()),
            locator_type: "html".into(),
            locator: json!({"headingPath": heading_path.clone(), "tag": name}),
        });
    }
    blocks
}

fn strip_tags(input: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for ch in input.chars() {
        match ch {
            '<' => inside = true,
            '>' => inside = false,
            ch if !inside => out.push(ch),
            _ => {}
        }
    }
    out
}

fn find_sub(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// 文件导入安全（file-import-security.ts + path-security.ts + manager.ts
/// resolveSupportedMimeType 的 Rust 承担）。
pub fn read_secure_import_file(
    file_path: &str,
    lingxi_home: &std::path::Path,
) -> Result<(Vec<u8>, String, String), KnowledgeError> {
    let path = std::path::Path::new(file_path);
    if !path.is_absolute() {
        return Err(KnowledgeError::new(
            "KNOWLEDGE_IMPORT_PATH_INVALID",
            "Knowledge import path must be absolute",
        ));
    }
    let meta = std::fs::symlink_metadata(path);
    if meta.is_err() {
        return Err(KnowledgeError::new(
            "KNOWLEDGE_IMPORT_NOT_FOUND",
            "Knowledge import file was not found",
        ));
    }
    if meta.expect("meta").file_type().is_symlink() {
        return Err(KnowledgeError::new(
            "KNOWLEDGE_IMPORT_SYMLINK",
            "Symbolic links cannot be imported",
        ));
    }
    let resolved = std::fs::canonicalize(path).map_err(|_| {
        KnowledgeError::new(
            "KNOWLEDGE_IMPORT_NOT_FOUND",
            "Knowledge import file was not found",
        )
    })?;
    if is_sensitive_path(&resolved, Some(lingxi_home)) {
        return Err(KnowledgeError::new(
            "KNOWLEDGE_IMPORT_PATH_BLOCKED",
            "Knowledge import path is blocked",
        ));
    }
    let real_meta = std::fs::metadata(&resolved).map_err(|_| {
        KnowledgeError::new("KNOWLEDGE_IMPORT_NOT_FOUND", "import file was not found")
    })?;
    if !real_meta.is_file() {
        return Err(KnowledgeError::new(
            "KNOWLEDGE_IMPORT_FILE_REQUIRED",
            "Knowledge import requires a regular file",
        ));
    }
    if real_meta.len() > MAX_IMPORT_BYTES as u64 {
        return Err(KnowledgeError::new(
            "KNOWLEDGE_IMPORT_TOO_LARGE",
            "Knowledge import exceeds the byte limit",
        ));
    }
    let bytes = std::fs::read(&resolved).map_err(|_| {
        KnowledgeError::new("KNOWLEDGE_IMPORT_NOT_FOUND", "import file was not found")
    })?;
    let file_name = resolved
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let mime_type = resolve_supported_mime_type(&file_name, &bytes)?;
    Ok((bytes, mime_type, file_name))
}

fn resolve_supported_mime_type(file_name: &str, bytes: &[u8]) -> Result<String, KnowledgeError> {
    let extension = std::path::Path::new(file_name)
        .extension()
        .map(|ext| ext.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let mime = match extension.as_str() {
        "txt" => Some("text/plain"),
        "md" | "markdown" => Some("text/markdown"),
        "html" | "htm" => Some("text/html"),
        "pdf" => Some("application/pdf"),
        "docx" | "xlsx" | "csv" => Some("application/vnd.lingxi.processor"),
        "ppt" | "pptx" | "pptm" | "epub" | "doc" | "xls" => {
            return Err(KnowledgeError::new(
                "KNOWLEDGE_IMPORT_PROCESSOR_UNAVAILABLE",
                "This file format has no registered knowledge processor",
            ));
        }
        _ => None,
    };
    let Some(mime) = mime else {
        return Err(KnowledgeError::new(
            "KNOWLEDGE_IMPORT_TYPE_UNSUPPORTED",
            "This file type does not yet support citation-grade Knowledge import",
        ));
    };
    if mime == "application/pdf" && !bytes.starts_with(b"%PDF-") {
        return Err(KnowledgeError::new(
            "KNOWLEDGE_IMPORT_TYPE_UNSUPPORTED",
            "The file content does not match its PDF extension",
        ));
    }
    if mime.starts_with("text/") && bytes.contains(&0) {
        return Err(KnowledgeError::new(
            "KNOWLEDGE_IMPORT_TYPE_UNSUPPORTED",
            "Binary content cannot be imported as text",
        ));
    }
    Ok(mime.to_string())
}

/// 敏感路径（path-security.ts isSensitivePath：用户凭证目录 + lingxiHome）。
pub fn is_sensitive_path(value: &std::path::Path, lingxi_home: Option<&std::path::Path>) -> bool {
    const SENSITIVE_DIRS: [&str; 5] = [".ssh", ".gnupg", ".aws", ".config/gcloud", ".kube"];
    let Ok(resolved) = std::fs::canonicalize(value) else {
        return true;
    };
    let Some(home) = std::env::home_dir() else {
        return true;
    };
    for directory in SENSITIVE_DIRS {
        let sensitive = home.join(directory);
        if resolved == sensitive || resolved.starts_with(&sensitive) {
            return true;
        }
    }
    if let Some(lingxi_home) = lingxi_home {
        if let Ok(real_home) = std::fs::canonicalize(lingxi_home) {
            if resolved == real_home || resolved.starts_with(&real_home) {
                return true;
            }
        }
    }
    false
}

fn collect_directory_files(path: &std::path::Path, depth: usize, visit: &mut dyn FnMut(String)) {
    const MAX_DEPTH: usize = 8;
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            collect_directory_files(&path, depth + 1, visit);
        } else if meta.is_file() {
            visit(path.to_string_lossy().to_string());
        }
    }
}

/// 简化正则（literal/前缀/字符类以外的完整正则面不在本任务范围——grep 默认
/// literal；use_regex=true 支持 `.`、`*`、`+`、`?`、字符类与锚点的受限子集）。
struct SimpleRegex {
    pattern: Vec<char>,
}

impl SimpleRegex {
    fn find_at(&self, text: &[char], start: usize) -> Option<usize> {
        for begin in start..text.len() {
            if let Some(length) = match_here(&self.pattern, 0, text, begin) {
                return Some(length);
            }
        }
        None
    }
}

fn match_here(pattern: &[char], pi: usize, text: &[char], ti: usize) -> Option<usize> {
    if pi >= pattern.len() {
        return Some(0);
    }
    let ch = pattern[pi];
    let quantified = pi + 1 < pattern.len();
    match ch {
        '^' if pi == 0 => {
            if ti == 0 {
                match_here(pattern, 1, text, ti)
            } else {
                None
            }
        }
        '$' if pi == pattern.len() - 1 => {
            if ti == text.len() {
                Some(0)
            } else {
                None
            }
        }
        '\\' if quantified => {
            let escaped = pattern[pi + 1];
            if ti < text.len() && text[ti] == escaped {
                match_here(pattern, pi + 2, text, ti + 1).map(|len| len + 1)
            } else {
                None
            }
        }
        _ if quantified && matches!(pattern[pi + 1], '*' | '+' | '?') => {
            let quantifier = pattern[pi + 1];
            let mut count = 0_usize;
            let mut end = ti;
            while end < text.len() && char_matches(ch, text[end]) {
                count += 1;
                end += 1;
                if quantifier == '?' {
                    break;
                }
            }
            // 贪婪回退。
            loop {
                if let Some(len) = match_here(pattern, pi + 2, text, ti + count) {
                    return Some(count + len);
                }
                if count == 0 {
                    return None;
                }
                count -= 1;
            }
        }
        _ => {
            if ti < text.len() && char_matches(ch, text[ti]) {
                match_here(pattern, pi + 1, text, ti + 1).map(|len| len + 1)
            } else {
                None
            }
        }
    }
}

fn char_matches(pattern_ch: char, text_ch: char) -> bool {
    pattern_ch == '.' || pattern_ch == text_ch
}

fn simple_regex(pattern: &str) -> Result<SimpleRegex, KnowledgeError> {
    Ok(SimpleRegex {
        pattern: pattern.chars().collect(),
    })
}

// ── 序列化 ──────────────────────────────────────────────────────────────────

fn notebook_json(notebook: &ks::NotebookRow) -> Value {
    json!({
        "id": notebook.id,
        "studioId": notebook.studio_id,
        "name": notebook.name,
        "createdAt": notebook.created_at,
        "updatedAt": notebook.updated_at,
        "deletedAt": notebook.deleted_at,
    })
}

fn config_json(config: &ks::NotebookConfigRow) -> Value {
    json!({
        "embeddingModelRef": config.embedding_model_ref.as_ref().map(|model_ref| json!({"id": model_ref.id, "provider": model_ref.provider})),
        "rerankModelRef": config.rerank_model_ref.as_ref().map(|model_ref| json!({"id": model_ref.id, "provider": model_ref.provider})),
        "chunkTargetChars": config.chunk_target_chars,
        "retrievalTopK": config.retrieval_top_k,
        "vectorRetentionDays": config.vector_retention_days,
    })
}

fn source_json(source: &ks::SourceRow) -> Value {
    // originMetadata 脱敏（publicOriginMetadata：url 去 query/hash）。
    let mut origin = source.origin_metadata.clone();
    if let Some(url) = origin.get("originalUrl").and_then(|value| value.as_str()) {
        let stripped = url.split(['?', '#']).next().unwrap_or(url).to_string();
        origin["url"] = json!(stripped);
    }
    json!({
        "id": source.id,
        "studioId": source.studio_id,
        "sourceType": source.source_type,
        "displayName": source.display_name,
        "originMetadata": origin,
        "createdAt": source.created_at,
        "deletedAt": source.deleted_at,
        "orphanedAt": source.orphaned_at,
    })
}

fn snapshot_json(snapshot: &ks::SnapshotRow) -> Value {
    // storagePath 不出域（serializeSnapshot）。
    json!({
        "id": snapshot.id,
        "sourceId": snapshot.source_id,
        "sha256": snapshot.sha256,
        "mimeType": snapshot.mime_type,
        "byteSize": snapshot.byte_size,
        "capturedAt": snapshot.captured_at,
    })
}

fn membership_json(membership: &ks::MembershipRow) -> Value {
    json!({
        "notebookId": membership.notebook_id,
        "sourceId": membership.source_id,
        "addedAt": membership.added_at,
        "removedAt": membership.removed_at,
        "relativePath": membership.relative_path,
        "folderNode": membership.folder_node,
        "displayOrder": membership.display_order,
    })
}

fn artifact_json_public(artifact: &ks::ParseArtifactRow) -> Value {
    // semanticArtifactPath 不出域（serializeArtifact）。
    json!({
        "id": artifact.id,
        "contentSnapshotId": artifact.content_snapshot_id,
        "parserId": artifact.parser_id,
        "parserVersion": artifact.parser_version,
        "parserConfigHash": artifact.parser_config_hash,
        "status": artifact.status,
        "warnings": artifact.warnings,
        "fidelity": artifact.fidelity,
        "processingArtifactId": artifact.processing_artifact_id,
        "createdAt": artifact.created_at,
        "completedAt": artifact.completed_at,
    })
}

fn citation_json(citation: &ks::CitationRow) -> Value {
    json!({
        "id": citation.id,
        "parseArtifactId": citation.parse_artifact_id,
        "blockId": citation.block_id,
        "startOffset": citation.start_offset,
        "endOffset": citation.end_offset,
        "canonicalText": citation.canonical_text,
        "canonicalTextSha256": citation.canonical_text_sha256,
        "createdAt": citation.created_at,
    })
}

fn block_json(block: &ks::BlockRow) -> Value {
    json!({
        "id": block.id,
        "parseArtifactId": block.parse_artifact_id,
        "ordinal": block.ordinal,
        "text": block.text,
        "textSha256": block.text_sha256,
        "locatorType": block.locator_type,
        "locator": block.locator,
    })
}

fn job_json(job: &ks::JobRow) -> Value {
    json!({
        "id": job.id,
        "notebookId": job.notebook_id,
        "sourceId": job.source_id,
        "artifactId": job.artifact_id,
        "phase": job.phase,
        "status": job.status,
        "attempt": job.attempt,
        "error": job.error,
        "chunkerConfigId": job.chunker_config_id,
        "cancelledAt": job.cancelled_at,
        "progressDone": job.progress_done,
        "progressTotal": job.progress_total,
        "embeddingStats": job.embedding_stats,
        "createdAt": job.created_at,
        "updatedAt": job.updated_at,
    })
}

fn scope_json(scope: &ks::TurnScopeRow) -> Value {
    json!({
        "id": scope.id,
        "turnId": scope.turn_id,
        "sessionId": scope.session_id,
        "studioId": scope.studio_id,
        "notebookIds": scope.notebook_ids,
        "status": scope.status,
        "createdAt": scope.created_at,
        "sources": scope.sources.iter().map(|frozen| json!({
            "sourceId": frozen.source_id,
            "contentSnapshotId": frozen.content_snapshot_id,
            "parseArtifactId": frozen.parse_artifact_id,
            "notebookIds": frozen.notebook_ids,
        })).collect::<Vec<_>>(),
    })
}

fn chunker_block_of(block: &ks::BlockRow) -> ChunkerBlock {
    ChunkerBlock {
        id: block.id.clone(),
        ordinal: block.ordinal,
        text: block.text.clone(),
        text_sha256: block.text_sha256.clone(),
        locator_type: block.locator_type.clone(),
        locator: block.locator.clone(),
    }
}

// ── HTTP 路由（routes/knowledge.ts 镜像；LocalOnly 归属闸在 auth 层）────────

fn knowledge_error_response(err: &KnowledgeError) -> Response {
    let status = lingxi_kernel::knowledge::knowledge_error_status(&err.code);
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_REQUEST),
        Json(json!({
            "error": err.code,
            "message": err.message,
        })),
    )
        .into_response()
}

#[allow(clippy::result_large_err)] // 失败即整条 HTTP Response（axum handler 约定）
async fn parse_body(response: axum::body::Bytes, limit: usize) -> Result<Value, Response> {
    if response.len() > limit {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(json!({
                "error": "KNOWLEDGE_IMPORT_TOO_LARGE",
                "message": "Knowledge source request body is too large",
            })),
        )
            .into_response());
    }
    serde_json::from_slice(&response).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "KNOWLEDGE_INVALID_ARGUMENT",
                "message": "request body must be valid JSON",
            })),
        )
            .into_response()
    })
}

#[allow(clippy::result_large_err)] // 失败即整条 HTTP Response（axum handler 约定，同 parse_body）
fn require_exact_keys(body: &Value, allowed: &[&str]) -> Result<(), Response> {
    let Some(object) = body.as_object() else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "KNOWLEDGE_INVALID_ARGUMENT",
                "message": "request body must be an object",
            })),
        )
            .into_response());
    };
    if object.len() != allowed.len() || allowed.iter().any(|key| !object.contains_key(*key)) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "KNOWLEDGE_INVALID_ARGUMENT",
                "message": "body fields are invalid",
            })),
        )
            .into_response());
    }
    Ok(())
}

pub async fn notebooks_list_route(State(state): State<crate::ServiceState>) -> Response {
    match state.knowledge().list_notebooks() {
        Ok(value) => Json(value).into_response(),
        Err(err) => knowledge_error_response(&err),
    }
}

pub async fn notebooks_create_route(
    State(state): State<crate::ServiceState>,
    body: axum::body::Bytes,
) -> Response {
    let Ok(body) = parse_body(body, 64 * 1024).await else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if require_exact_keys(&body, &["name"]).is_err() {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": "KNOWLEDGE_INVALID_ARGUMENT", "message": "Notebook create body fields are invalid"})))
            .into_response();
    }
    let name = body["name"].as_str().unwrap_or_default();
    match state.knowledge().create_notebook(name) {
        Ok(notebook) => (StatusCode::CREATED, Json(json!({"notebook": notebook}))).into_response(),
        Err(err) => knowledge_error_response(&err),
    }
}

pub async fn notebook_get_route(
    State(state): State<crate::ServiceState>,
    axum::extract::Path(notebook_id): axum::extract::Path<String>,
) -> Response {
    match state.knowledge().get_notebook(&notebook_id) {
        Ok(notebook) => Json(json!({"notebook": notebook})).into_response(),
        Err(err) => knowledge_error_response(&err),
    }
}

pub async fn notebook_rename_route(
    State(state): State<crate::ServiceState>,
    axum::extract::Path(notebook_id): axum::extract::Path<String>,
    body: axum::body::Bytes,
) -> Response {
    let Ok(body) = parse_body(body, 64 * 1024).await else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if require_exact_keys(&body, &["name"]).is_err() {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": "KNOWLEDGE_INVALID_ARGUMENT", "message": "Notebook rename body fields are invalid"})))
            .into_response();
    }
    match state
        .knowledge()
        .rename_notebook(&notebook_id, body["name"].as_str().unwrap_or_default())
    {
        Ok(notebook) => Json(json!({"notebook": notebook})).into_response(),
        Err(err) => knowledge_error_response(&err),
    }
}

pub async fn notebook_delete_route(
    State(state): State<crate::ServiceState>,
    axum::extract::Path(notebook_id): axum::extract::Path<String>,
) -> Response {
    match state.knowledge().delete_notebook(&notebook_id) {
        Ok(notebook) => Json(json!({"notebook": notebook})).into_response(),
        Err(err) => knowledge_error_response(&err),
    }
}

pub async fn notebook_settings_route(
    State(state): State<crate::ServiceState>,
    axum::extract::Path(notebook_id): axum::extract::Path<String>,
    body: axum::body::Bytes,
) -> Response {
    let Ok(body) = parse_body(body, 64 * 1024).await else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    match state
        .knowledge()
        .update_notebook_settings(&notebook_id, &body)
    {
        Ok(config) => Json(json!({"config": config})).into_response(),
        Err(err) => knowledge_error_response(&err),
    }
}

pub async fn ingestion_list_route(
    State(state): State<crate::ServiceState>,
    axum::extract::RawQuery(query): axum::extract::RawQuery,
) -> Response {
    let (notebook_id, source_id) = parse_ingestion_query(query.as_deref());
    match state
        .knowledge()
        .list_ingestion_jobs(notebook_id.as_deref(), source_id.as_deref())
    {
        Ok(value) => Json(value).into_response(),
        Err(err) => knowledge_error_response(&err),
    }
}

fn parse_ingestion_query(query: Option<&str>) -> (Option<String>, Option<String>) {
    let mut notebook_id = None;
    let mut source_id = None;
    for pair in query.unwrap_or_default().split('&') {
        let mut parts = pair.splitn(2, '=');
        let key = parts.next().unwrap_or_default();
        let value = parts.next().unwrap_or_default();
        match key {
            "notebookId" if !value.is_empty() => notebook_id = Some(value.to_string()),
            "sourceId" if !value.is_empty() => source_id = Some(value.to_string()),
            _ => {}
        }
    }
    (notebook_id, source_id)
}

pub async fn notebook_sources_list_route(
    State(state): State<crate::ServiceState>,
    axum::extract::Path(notebook_id): axum::extract::Path<String>,
) -> Response {
    match state.knowledge().list_notebook_sources(&notebook_id) {
        Ok(value) => Json(value).into_response(),
        Err(err) => knowledge_error_response(&err),
    }
}

pub async fn notebook_sources_import_route(
    State(state): State<crate::ServiceState>,
    axum::extract::Path(notebook_id): axum::extract::Path<String>,
    body: axum::body::Bytes,
) -> Response {
    let body = match parse_body(body, 64 * 1024 * 1024).await {
        Ok(body) => body,
        Err(response) => return response,
    };
    let kind = body["kind"].as_str().unwrap_or_default();
    let result = match kind {
        "file" => state.knowledge().import_file(
            &notebook_id,
            body["filePath"].as_str().unwrap_or_default(),
            body["displayName"].as_str(),
        ),
        "pasted_text" => state.knowledge().import_pasted_text(
            &notebook_id,
            body["text"].as_str().unwrap_or_default(),
            body["displayName"].as_str(),
        ),
        "web_snapshot" => Err(KnowledgeError::new(
            "KNOWLEDGE_WEB_TYPE_UNSUPPORTED",
            "web snapshot import requires the network fetcher surface (R07 外围面)",
        )),
        _ => Err(KnowledgeError::invalid_argument(
            "Knowledge source import body is invalid",
        )),
    };
    match result {
        Ok(payload) => (StatusCode::CREATED, Json(payload)).into_response(),
        Err(err) => knowledge_error_response(&err),
    }
}

pub async fn notebook_import_directory_route(
    State(state): State<crate::ServiceState>,
    axum::extract::Path(notebook_id): axum::extract::Path<String>,
    body: axum::body::Bytes,
) -> Response {
    let body = match parse_body(body, 64 * 1024 * 1024).await {
        Ok(body) => body,
        Err(response) => return response,
    };
    let Some(dir_path) = body["dirPath"].as_str() else {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": "KNOWLEDGE_INVALID_ARGUMENT", "message": "Directory import body is invalid"})))
            .into_response();
    };
    match state.knowledge().import_directory(&notebook_id, dir_path) {
        Ok(value) => (StatusCode::CREATED, Json(value)).into_response(),
        Err(err) => knowledge_error_response(&err),
    }
}

pub async fn source_membership_delete_route(
    State(state): State<crate::ServiceState>,
    axum::extract::Path((notebook_id, source_id)): axum::extract::Path<(String, String)>,
) -> Response {
    match state
        .knowledge()
        .remove_source_from_notebook(&notebook_id, &source_id)
    {
        Ok(value) => Json(value).into_response(),
        Err(err) => knowledge_error_response(&err),
    }
}

pub async fn source_delete_route(
    State(state): State<crate::ServiceState>,
    axum::extract::Path(source_id): axum::extract::Path<String>,
) -> Response {
    match state.knowledge().delete_source(&source_id) {
        Ok(value) => Json(value).into_response(),
        Err(err) => knowledge_error_response(&err),
    }
}

pub async fn source_refresh_route(
    State(state): State<crate::ServiceState>,
    axum::extract::Path((notebook_id, source_id)): axum::extract::Path<(String, String)>,
) -> Response {
    match state
        .knowledge()
        .refresh_file_source(&notebook_id, &source_id)
    {
        Ok(value) => Json(value).into_response(),
        Err(err) => knowledge_error_response(&err),
    }
}

pub async fn source_reingest_route(
    State(state): State<crate::ServiceState>,
    axum::extract::Path((notebook_id, source_id)): axum::extract::Path<(String, String)>,
) -> Response {
    match state.knowledge().reingest_source(&notebook_id, &source_id) {
        Ok(value) => Json(value).into_response(),
        Err(err) => knowledge_error_response(&err),
    }
}

pub async fn artifact_blocks_route(
    State(state): State<crate::ServiceState>,
    axum::extract::Path(artifact_id): axum::extract::Path<String>,
) -> Response {
    match state.knowledge().list_artifact_blocks(&artifact_id) {
        Ok(value) => Json(value).into_response(),
        Err(err) => knowledge_error_response(&err),
    }
}

pub async fn artifact_chunks_route(
    State(state): State<crate::ServiceState>,
    axum::extract::Path(artifact_id): axum::extract::Path<String>,
) -> Response {
    match state.knowledge().list_artifact_chunk_cards(&artifact_id) {
        Ok(value) => Json(value).into_response(),
        Err(err) => knowledge_error_response(&err),
    }
}

pub async fn citation_resolve_route(
    State(state): State<crate::ServiceState>,
    axum::extract::Path(citation_id): axum::extract::Path<String>,
) -> Response {
    match state.knowledge().resolve_citation(&citation_id) {
        Ok(value) => Json(value).into_response(),
        Err(err) => knowledge_error_response(&err),
    }
}

pub async fn snapshot_content_route(
    State(state): State<crate::ServiceState>,
    axum::extract::Path(snapshot_id): axum::extract::Path<String>,
) -> Response {
    match state.knowledge().read_snapshot_bytes(&snapshot_id) {
        Ok((snapshot, bytes)) => {
            // 冻结原文只作证据字节保存，绝不在 Lingxi 自身源下执行其中脚本。
            let content_type = if snapshot.mime_type == "text/html" {
                "text/plain; charset=utf-8"
            } else {
                snapshot.mime_type.as_str()
            };
            (
                [
                    (header::CONTENT_TYPE, content_type.to_string()),
                    (header::CONTENT_LENGTH, bytes.len().to_string()),
                    (header::CACHE_CONTROL, "private, immutable".to_string()),
                    (header::ETAG, format!("\"{}\"", snapshot.sha256)),
                    (
                        axum::http::HeaderName::from_static("x-content-type-options"),
                        "nosniff".to_string(),
                    ),
                    (
                        axum::http::HeaderName::from_static("content-security-policy"),
                        "sandbox; default-src 'none'".to_string(),
                    ),
                ],
                bytes,
            )
                .into_response()
        }
        Err(err) => knowledge_error_response(&err),
    }
}

pub async fn turn_scope_create_route(
    State(state): State<crate::ServiceState>,
    body: axum::body::Bytes,
) -> Response {
    let body = match parse_body(body, 64 * 1024).await {
        Ok(body) => body,
        Err(response) => return response,
    };
    let notebook_ids: Vec<String> = body["notebookIds"]
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.as_str())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let session_id = body["sessionId"].as_str().unwrap_or_default().to_string();
    let turn_id = body["turnId"].as_str().unwrap_or_default().to_string();
    if session_id.is_empty() || turn_id.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": "KNOWLEDGE_INVALID_ARGUMENT", "message": "sessionId and turnId are required"})))
            .into_response();
    }
    match state
        .knowledge()
        .create_turn_scope(&session_id, &turn_id, &notebook_ids)
    {
        Ok(scope) => (
            StatusCode::CREATED,
            Json(json!({"scope": scope_json(&scope)})),
        )
            .into_response(),
        Err(err) => knowledge_error_response(&err),
    }
}

pub async fn turn_scope_get_route(
    State(state): State<crate::ServiceState>,
    axum::extract::Path(scope_id): axum::extract::Path<String>,
) -> Response {
    match state.knowledge().get_turn_scope(&scope_id) {
        Ok(value) => Json(json!({"scope": value})).into_response(),
        Err(err) => knowledge_error_response(&err),
    }
}

/// 路由注册（lib.rs bootstrap 调用）。
pub fn knowledge_routes() -> Router<crate::ServiceState> {
    Router::new()
        .route(
            "/lingxi/v1/knowledge/notebooks",
            get(notebooks_list_route).post(notebooks_create_route),
        )
        .route(
            "/lingxi/v1/knowledge/notebooks/{id}",
            get(notebook_get_route)
                .patch(notebook_rename_route)
                .delete(notebook_delete_route),
        )
        .route(
            "/lingxi/v1/knowledge/notebooks/{id}/settings",
            put(notebook_settings_route),
        )
        .route("/lingxi/v1/knowledge/ingestion", get(ingestion_list_route))
        .route(
            "/lingxi/v1/knowledge/notebooks/{id}/sources",
            get(notebook_sources_list_route).post(notebook_sources_import_route),
        )
        .route(
            "/lingxi/v1/knowledge/notebooks/{id}/import-directory",
            post(notebook_import_directory_route),
        )
        .route(
            "/lingxi/v1/knowledge/notebooks/{id}/sources/{source_id}",
            delete(source_membership_delete_route),
        )
        .route(
            "/lingxi/v1/knowledge/notebooks/{id}/sources/{source_id}/refresh",
            post(source_refresh_route),
        )
        .route(
            "/lingxi/v1/knowledge/notebooks/{id}/sources/{source_id}/reingest",
            post(source_reingest_route),
        )
        .route(
            "/lingxi/v1/knowledge/sources/{source_id}",
            delete(source_delete_route),
        )
        .route(
            "/lingxi/v1/knowledge/parse-artifacts/{id}/blocks",
            get(artifact_blocks_route),
        )
        .route(
            "/lingxi/v1/knowledge/parse-artifacts/{id}/chunks",
            get(artifact_chunks_route),
        )
        .route(
            "/lingxi/v1/knowledge/citations/{citation_id}",
            get(citation_resolve_route),
        )
        .route(
            "/lingxi/v1/knowledge/snapshots/{snapshot_id}/content",
            get(snapshot_content_route),
        )
        .route(
            "/lingxi/v1/knowledge/turn-scopes",
            post(turn_scope_create_route),
        )
        .route(
            "/lingxi/v1/knowledge/turn-scopes/{scope_id}",
            get(turn_scope_get_route),
        )
}
