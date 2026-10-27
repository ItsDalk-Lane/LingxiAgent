//! 模型按需目录入口；发现不是授权，执行仍完整经过原网关。
use crate::toolgateway::{GatewayRefusal, ToolInvocationGateway};
use lingxi_kernel::ports::{ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest};
use lingxi_kernel::toolcatalog::{
    Availability, CatalogPin, DeclaredPermission, PermissionContract, PermissionKind, SchemaBudget,
    ToolManifest, ToolOrigin, ToolRegistry, ToolTargetId, ToolTargetRef,
};
use lingxi_kernel::RunContext;
use lingxi_protocol::{ErrorCode, ProtocolError, ToolCallId, ToolSchemaDocument};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub const RESIDENT_NAMES: [&str; 7] = [
    "read",
    "write",
    "edit",
    "exec_command",
    "mcp_search_tools",
    "mcp_describe_tool",
    "mcp_call",
];
type RunKey = (String, String, String, String, String, u64);
fn run_key(ctx: &RunContext) -> RunKey {
    (
        ctx.principal.storage_kind().to_owned(),
        ctx.principal.storage_subject(),
        ctx.session_id.to_string(),
        ctx.run_id.to_string(),
        ctx.attempt.to_string(),
        ctx.generation,
    )
}
#[derive(Default)]
pub struct CatalogDefinitions {
    definitions: Mutex<HashMap<RunKey, HashMap<ToolTargetId, u64>>>,
}
impl CatalogDefinitions {
    pub fn finish(&self, ctx: &RunContext) {
        self.definitions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&run_key(ctx));
    }
    fn remember(
        &self,
        ctx: &RunContext,
        target: ToolTargetId,
        generation: u64,
    ) -> Result<(), String> {
        let mut all = self.definitions.lock().unwrap_or_else(|p| p.into_inner());
        if !all.contains_key(&run_key(ctx)) && all.len() >= 1024 {
            return Err("目录说明容量已满".into());
        }
        let run = all.entry(run_key(ctx)).or_default();
        if !run.contains_key(&target) && run.len() >= 1024 {
            return Err("本轮目录说明容量已满".into());
        }
        run.insert(target, generation);
        Ok(())
    }
    pub fn normalize(
        &self,
        registry: &ToolRegistry,
        ctx: &RunContext,
        request: &ToolRequest,
        pin: Option<CatalogPin>,
    ) -> Result<(ToolRequest, Option<CatalogPin>), GatewayRefusal> {
        if request.target != "tool:first-party:mcp_call" {
            return Ok((request.clone(), pin));
        }
        if !request.digest_matches_arguments() {
            return Err(GatewayRefusal::DigestMismatch {
                declared: request.args_digest.hex.clone(),
                computed: request.arguments.digest().hex,
            });
        }
        let bridge = registry
            .prepare_invocation(
                &ToolTargetRef::ByTargetId {
                    target_id: ToolTargetId::parse(&request.target),
                },
                pin,
                request.arguments.as_value(),
                &SchemaBudget::default(),
            )
            .map_err(crate::toolgateway::map_catalog_error)?;
        let raw = bridge.effective.as_value();
        let target = registry
            .resolve(&reference(
                raw,
                request.target == "tool:first-party:mcp_describe_tool",
            ))
            .map_err(crate::toolgateway::map_catalog_error)?;
        if target.as_str() == "tool:first-party:mcp_call" {
            return Err(GatewayRefusal::ArgumentsInvalid {
                violations: vec!["目录调度不能递归包装调度入口".into()],
            });
        }
        let held = self
            .definitions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&run_key(ctx))
            .and_then(|r| r.get(&target))
            .copied()
            .ok_or_else(|| GatewayRefusal::ArgumentsInvalid {
                violations: vec![
                    "必须先在本轮读取目标的正式说明；模型自报代次不能替代说明来源".into(),
                ],
            })?;
        let actual_pin = Some(CatalogPin {
            catalog_generation: held,
        });
        let prepared = registry
            .prepare_invocation(
                &ToolTargetRef::ByTargetId { target_id: target },
                actual_pin,
                &raw["arguments"],
                &SchemaBudget::default(),
            )
            .map_err(crate::toolgateway::map_catalog_error)?;
        let mut normalized = prepared.into_tool_request();
        normalized.provider_call_id = request.provider_call_id.clone();
        Ok((normalized, actual_pin))
    }
}
fn reference(raw: &Value, describing: bool) -> ToolTargetRef {
    let name = raw[if describing { "name" } else { "tool" }]
        .as_str()
        .unwrap_or("")
        .to_owned();
    match raw
        .get("server")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        Some(source) => ToolTargetRef::BySourceAndName {
            source_id: source.to_owned(),
            name,
        },
        None => ToolTargetRef::ByName { name },
    }
}
pub struct CatalogRunLease {
    pub definitions: Arc<CatalogDefinitions>,
    pub context: RunContext,
}
impl Drop for CatalogRunLease {
    fn drop(&mut self) {
        self.definitions.finish(&self.context);
    }
}
struct CatalogExecutor {
    registry: Arc<ToolRegistry>,
    definitions: Arc<CatalogDefinitions>,
}
impl ToolExecutorPort for CatalogExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        _call: &'a ToolCallId,
        request: &'a ToolRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        Box::pin(async move {
            let raw = request.arguments.as_value();
            let result: Result<Value, String> = match request.target.as_str() {
                "tool:first-party:mcp_search_tools" => {
                    let snapshot = self.registry.snapshot();
                    let query = raw["query"].as_str().unwrap_or("").to_lowercase();
                    let limit = raw["limit"].as_u64().unwrap_or(5) as usize;
                    Ok(
                        json!({"catalogGeneration":snapshot.catalog_generation,"tools":snapshot.tools.iter().filter(|d| d.local_name.to_lowercase().contains(&query) || d.display_name.to_lowercase().contains(&query) || d.description.to_lowercase().contains(&query)).take(limit).map(|d|json!({"tool":d.local_name,"server":d.source_id,"targetId":d.target_id.as_str(),"description":d.description,"availability":d.availability.wire_name(),"targetGeneration":d.target_generation})).collect::<Vec<_>>()}),
                    )
                }
                "tool:first-party:mcp_describe_tool" => (|| {
                    let target = self
                        .registry
                        .resolve(&reference(
                            raw,
                            request.target == "tool:first-party:mcp_describe_tool",
                        ))
                        .map_err(|e| e.to_string())?;
                    let (generation, full) = self
                        .registry
                        .describe_full_at_generation(&target)
                        .map_err(|e| e.to_string())?;
                    self.definitions.remember(ctx, target, generation)?;
                    Ok(
                        json!({"tool":full.listing.local_name,"server":full.listing.source_id,"targetId":full.listing.target_id.as_str(),"description":full.listing.description,"inputSchema":full.input_schema.schema,"dialect":full.input_schema.dialect,"catalogGeneration":generation,"targetGeneration":full.listing.target_generation,"availability":full.listing.availability.wire_name()}),
                    )
                })(),
                _ => Err("调度入口必须由可信运行驱动解析后经过目标网关，不能直接执行".into()),
            };
            ToolExecutionResult::of_ctx(
                ctx,
                match result {
                    Ok(value) => ToolOutcome::success_text(value.to_string()),
                    Err(message) => ToolOutcome::Failed {
                        error: ProtocolError::new(ErrorCode::InvalidMessage, message, false),
                    },
                },
            )
        })
    }
}
pub fn register(
    registry: &Arc<ToolRegistry>,
    gateway: &ToolInvocationGateway,
    budget: &SchemaBudget,
) -> Arc<CatalogDefinitions> {
    let definitions = Arc::new(CatalogDefinitions::default());
    let executor: Arc<dyn ToolExecutorPort> = Arc::new(CatalogExecutor {
        registry: Arc::clone(registry),
        definitions: Arc::clone(&definitions),
    });
    for (name, description, schema) in [
        ("mcp_search_tools", "搜索当前工具目录。返回工具名、真实来源和可用状态；发现不赋予调用权限。",json!({"type":"object","properties":{"query":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":50,"default":5}},"required":["query"],"additionalProperties":false})),
        ("mcp_describe_tool", "读取指定工具的完整正式说明和参数约束。重名时必须使用搜索返回的server来源；目录变化后重新读取。",json!({"type":"object","properties":{"name":{"type":"string","minLength":1},"server":{"type":"string"}},"required":["name"],"additionalProperties":false})),
        ("mcp_call", "调用本轮已读取正式说明的工具。tool和server沿用目录返回值；目标参数必须置于arguments对象。原权限、批准、资源和取消规则全部继续生效。",json!({"type":"object","properties":{"tool":{"type":"string","minLength":1},"server":{"type":"string"},"arguments":{"type":"object","default":{}}},"required":["tool"],"additionalProperties":false})),
    ] {
        let manifest=ToolManifest {origin:ToolOrigin::FirstParty,local_name:name.into(),display_name:name.into(),aliases:vec![],version:"1.0.0".into(),description:description.into(),input_schema:ToolSchemaDocument {dialect:"json-schema/2020-12".into(),schema},output_schema:None,permission:PermissionContract {kind:PermissionKind::Read,capability_base:"tool_catalog".into()},availability:Availability::Available,timeout_ms:Some(60_000),max_concurrency:None,declared_permission:DeclaredPermission::ReadOnly,recovery:lingxi_kernel::invocation::ToolRecoveryCapability::CONSERVATIVE};
        let receipt=registry.register(manifest,budget).expect("目录基础设施登记失败");
        gateway.bind_executor(receipt.target_id,Arc::clone(&executor),"生产按需目录基础设施");
    }
    registry.configure_model_residents(
        RESIDENT_NAMES
            .iter()
            .map(|n| ToolTargetId::parse(&format!("tool:first-party:{n}")))
            .collect(),
    );
    gateway.attach_catalog_definitions(Arc::clone(&definitions));
    definitions
}
