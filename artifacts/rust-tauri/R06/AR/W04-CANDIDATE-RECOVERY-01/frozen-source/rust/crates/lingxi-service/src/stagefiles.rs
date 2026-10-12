//! 文件交付通过正式网关登记；来源路径仍受当前主体和会话授权约束。
use crate::resourceaccess::{ResourceAccess, ResourceOp};
use crate::sessionfiles::SessionFileService;
use crate::toolgateway::{ResourceExtractor, ToolInvocationGateway};
use lingxi_kernel::ports::{
    ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest, ToolSuccess,
};
use lingxi_kernel::toolcatalog::{
    Availability, DeclaredPermission, PermissionContract, PermissionKind, SchemaBudget,
    ToolManifest, ToolOrigin, ToolRegistry,
};
use lingxi_protocol::{ContentBlock, ErrorCode, ProtocolError, ToolCallId, ToolSchemaDocument};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn normalize(args: &Value) -> Result<(Vec<String>, Vec<String>), String> {
    let values = |array: &str, single: &str| -> Result<Vec<String>, String> {
        let source = match args.get(array) {
            Some(Value::Array(items)) if !items.is_empty() => items.clone(),
            Some(Value::Array(_)) | None => args.get(single).cloned().into_iter().collect(),
            _ => return Err(format!("{array} must be an array")),
        };
        let mut out = Vec::new();
        for item in source {
            let Some(item) = item.as_str() else {
                return Err(format!("{array} requires strings"));
            };
            if item.trim().is_empty() {
                return Err(format!("{array} requires non-empty strings"));
            }
            if !out.iter().any(|v| v == item) {
                out.push(item.to_string());
            }
        }
        Ok(out)
    };
    let ids = values("fileIds", "fileId")?;
    if ids.iter().any(|id| id.trim() != id) {
        return Err("fileIds require exact strings".into());
    }
    let raw = values("filepaths", "filePath")?;
    let mut paths = Vec::new();
    for raw in raw {
        let stripped = raw.trim().trim_matches(['\'', '"']).replace("\\ ", " ");
        let path = if stripped.contains("%20") {
            percent_encoding::percent_decode_str(&stripped)
                .decode_utf8()
                .map_err(|e| e.to_string())?
                .into_owned()
        } else {
            stripped
        };
        if !Path::new(&path).is_absolute() {
            return Err("filepaths must be absolute".into());
        }
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    if ids.is_empty() && paths.is_empty() {
        return Err("stage_files requires fileIds or filepaths".into());
    }
    Ok((ids, paths))
}

struct StageFiles {
    files: Arc<SessionFileService>,
    access: Arc<ResourceAccess>,
    cwd: PathBuf,
}
fn failed(message: String) -> ToolOutcome {
    ToolOutcome::Failed {
        error: ProtocolError::new(ErrorCode::InvalidMessage, message, false),
    }
}
impl ToolExecutorPort for StageFiles {
    fn execute<'a>(
        &'a self,
        ctx: &'a lingxi_kernel::RunContext,
        _call: &'a ToolCallId,
        request: &'a ToolRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        Box::pin(async move {
            let args = request.arguments.as_value();
            let result = async {
                // 查身份、授权、检查实际文件以及整批提交共享同一正式租约。
                let lease = self.files.payload_lease().await;
                let (ids, paths) = normalize(args)?;
                let mut delivered = Vec::new();
                for id in ids {
                    let file = self
                        .files
                        .get_file(ctx.session_id.as_str(), &id)
                        .await
                        .map_err(|e| e.to_string())?
                        .ok_or_else(|| format!("SessionFile not found: {id}"))?;
                    if file.status != "available" || !Path::new(&file.real_path).exists() {
                        return Err(format!("SessionFile payload missing: {id}"));
                    }
                    delivered.push(file);
                }
                for path in paths {
                    let scope = self
                        .access
                        .authorize(
                            ctx.principal.storage_kind(),
                            &ctx.principal.storage_subject(),
                            ctx.session_id.as_str(),
                            Path::new(&path),
                            &self.cwd,
                            ResourceOp::Read,
                        )
                        .map_err(|e| e.to_string())?;
                    let file = self
                        .files
                        .register_file_under_lease(
                            ctx.session_id.as_str(),
                            &scope.path,
                            None,
                            args.get("label").and_then(Value::as_str),
                            "stage_files",
                            &lease,
                        )
                        .await
                        .map_err(|e| e.to_string())?;
                    delivered.push(file);
                }
                self.files
                    .commit_stage_references(
                        ctx.session_id.as_str(),
                        delivered.iter().map(|file| file.file_id.clone()).collect(),
                        _call.as_str(),
                        lease,
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                let text = delivered
                    .iter()
                    .map(|file| {
                        format!(
                            "[SessionFile] {}",
                            json!({"fileId":file.file_id,"filename":file.filename})
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                Ok::<_, String>(ToolOutcome::Success {
                    result: ToolSuccess::from_content(vec![ContentBlock::Text { text }]),
                })
            }
            .await;
            ToolExecutionResult::of_ctx(ctx, result.unwrap_or_else(failed))
        })
    }
}

pub fn register_stage_files(
    registry: &ToolRegistry,
    gateway: &ToolInvocationGateway,
    files: Arc<SessionFileService>,
    access: Arc<ResourceAccess>,
    cwd: PathBuf,
    budget: &SchemaBudget,
) {
    let manifest = ToolManifest {
        origin: ToolOrigin::FirstParty,
        local_name: "stage_files".into(),
        display_name: "Stage Files".into(),
        aliases: vec![],
        version: "1.0.0".into(),
        description:
            "Deliver registered SessionFile ids or authorized absolute file paths to the user."
                .into(),
        input_schema: ToolSchemaDocument {
            dialect: "json-schema/2020-12".into(),
            schema: json!({"type":"object","properties":{"fileIds":{"type":"array","items":{"type":"string"},"minItems":1},"fileId":{"type":"string"},"filepaths":{"type":"array","items":{"type":"string"},"minItems":1},"filePath":{"type":"string"},"label":{"type":"string"}},"additionalProperties":false}),
        },
        output_schema: None,
        permission: PermissionContract {
            kind: PermissionKind::Read,
            capability_base: "stage_files.stage".into(),
        },
        availability: Availability::Available,
        timeout_ms: Some(60000),
        max_concurrency: None,
        declared_permission: DeclaredPermission::ReadOnly,
        recovery: lingxi_kernel::invocation::ToolRecoveryCapability::CONSERVATIVE,
    };
    let target = registry
        .register(manifest, budget)
        .expect("正式文件交付工具注册")
        .target_id;
    let access2 = access.clone();
    let cwd2 = cwd.clone();
    let extract: ResourceExtractor = Arc::new(move |input, args| {
        let (_, paths) = normalize(args.as_value())?;
        paths
            .iter()
            .map(|path| {
                access2
                    .authorize(
                        input.principal_kind,
                        &input.principal_subject,
                        &input.session_id,
                        Path::new(path),
                        &cwd2,
                        ResourceOp::Read,
                    )
                    .map_err(|e| e.to_string())
            })
            .collect()
    });
    gateway.bind_executor_with_resources(
        target,
        Arc::new(StageFiles { files, access, cwd }),
        Some(extract),
        "正式文件交付与引用登记",
    );
}
