//! 办公美化 HTTP 沿现有字段；身份只来自认证扩展，文档不是授权来源。
use crate::{
    office_tools::{BeautifyGenerationRequest, OfficeApi, OfficeError},
    resourceio::OpContext,
    Principal, ServiceState,
};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use lingxi_protocol::{
    BeautifyApplyRequest as Apply, BeautifyGenerateRequest as Generate,
    BeautifyPresetRequest as Preset, BeautifyWorkbenchTarget as Target,
};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::Path};

fn error(error: OfficeError) -> Response {
    (StatusCode::from_u16(error.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),Json(json!({"ok":false,"error":error.message,"code":error.code,"effectUnknown":error.effect_unknown,"partialSuccess":error.code=="OFFICE_PARTIAL_SUCCESS"}))).into_response()
}
fn target(
    principal: &Principal,
    path: Option<String>,
    target: Option<Target>,
) -> Result<Value, OfficeError> {
    match (path, target) {
        (Some(_), None) if !principal.is_local_owner() => Err(OfficeError {
            status: 403,
            code: "OFFICE_FORBIDDEN",
            message: "filePath requires local owner; use a workbench target for remote clients"
                .into(),
            effect_unknown: false,
        }),
        (Some(path), None) if Path::new(&path).is_absolute() => {
            Ok(json!({"kind":"local-file","path":path}))
        }
        (None, Some(target)) => {
            if !matches!(target.kind.as_str(), "workbench-file" | "mobile-workbench")
                || target.name.is_empty()
                || target.name.contains(['/', '\\'])
                || target.subdir.split(['/', '\\']).any(|s| s == "..")
                || Path::new(&target.subdir).is_absolute()
            {
                return Err(OfficeError::bad("invalid workbench Markdown target"));
            }
            Ok(
                json!({"kind":"mount","mountId":target.mount_id.or(target.root_id).unwrap_or_else(||"default".into()),"path":Path::new(&target.subdir).join(target.name).to_string_lossy()}),
            )
        }
        _ => Err(OfficeError::bad(
            "provide exactly one of filePath or target",
        )),
    }
}
fn context(principal: &Principal) -> Result<OpContext, OfficeError> {
    if !principal.has_scope("resources.write") {
        return Err(OfficeError {
            status: 403,
            code: "OFFICE_FORBIDDEN",
            message: "resources.write permission required".into(),
            effect_unknown: false,
        });
    }
    let actor = crate::sessions::kernel_principal_of(principal);
    Ok(OpContext {
        principal_kind: actor.storage_kind(),
        principal_subject: actor.storage_subject(),
        session_id: String::new(),
        source: "api".into(),
        reason: Some("desk.beautify.cover".into()),
        session_path: None,
        emit: true,
        capture_origin: None,
        engine_plane: false,
    })
}
fn target_response(reference: &Value) -> Option<Value> {
    if reference["kind"] != "mount" {
        return None;
    }
    let path = Path::new(reference["path"].as_str()?);
    Some(
        json!({"kind":"workbench-file","mountId":reference["mountId"],"rootId":reference["mountId"],"subdir":path.parent().unwrap_or(Path::new("")).to_string_lossy(),"name":path.file_name()?.to_string_lossy()}),
    )
}
fn operation(
    state: &ServiceState,
    principal: &Principal,
    request: Option<&str>,
) -> Result<String, OfficeError> {
    let request = request
        .map(str::to_owned)
        .unwrap_or_else(|| state.request_ids().next_request_id());
    if request.is_empty() || request.len() > 1024 || request.chars().any(char::is_control) {
        return Err(OfficeError::bad("invalid cover requestId"));
    }
    Ok(format!(
        "cover-http-{}",
        lingxi_protocol::canon::sha256_hex(
            json!([principal.principal_id, request])
                .to_string()
                .as_bytes()
        )
    ))
}
fn api(state: &ServiceState) -> Result<&std::sync::Arc<OfficeApi>, OfficeError> {
    state.office_api().ok_or_else(|| OfficeError {
        status: 503,
        code: "OFFICE_UNAVAILABLE",
        message: "builtin Office service unavailable".into(),
        effect_unknown: false,
    })
}
async fn status(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    let result=async {
        if !principal.has_scope("resources.read"){return Err(OfficeError{status:403,code:"OFFICE_FORBIDDEN",message:"resources.read permission required".into(),effect_unknown:false});}
        let api=api(&state)?;
        let agent=match query.get("executorAgentId").or_else(||query.get("agentId")).filter(|id|!id.trim().is_empty()) {
            Some(agent)=>Some(agent.trim().to_owned()),
            None=>api.memory.resolve_primary_agent_checked().map_err(OfficeError::bad)?,
        };
        let mut generation=json!({"available":false,"enabled":false,"executorAgentId":agent,"executorAgentName":null,"disabledReason":"agent-unavailable","message":"agent unavailable"});
        if let Some(agent)=agent.as_deref().filter(|agent|api.memory.roots(agent).is_ok()) {
            let config=lingxi_adapters::storage::agent_memory::config::read_agent_config(&api.memory.config_path(agent).map_err(OfficeError::bad)?).map_err(OfficeError::bad)?;
            let scan=config.scan().map_err(OfficeError::bad)?;
            generation["available"]=json!(true);generation["executorAgentName"]=json!(scan.agent_name.unwrap_or_else(||agent.into()));
            if config.string_list("tools","disabled").map_err(OfficeError::bad)?.unwrap_or_default().iter().any(|name|name=="beautify") {
                generation["disabledReason"]=json!("beautify-disabled");generation["message"]=json!("beautify tool is disabled for this agent");
            } else if let Some(port)=api.generation()? {
                let status=port.status(agent).await?;
                let fields=status.as_object().ok_or_else(||OfficeError::bad("media runtime returned invalid generation status"))?;
                generation.as_object_mut().expect("状态对象").extend(fields.clone());
                generation["executorAgentId"]=json!(agent);
            } else {
                generation["disabledReason"]=json!("image-generation-unavailable");generation["message"]=json!("media generation runtime unavailable");
            }
        }
        Ok::<_,OfficeError>(json!({"systemCover":{"available":true},"available":true,"enabled":generation["enabled"],"agentId":agent,"agentGenerate":generation}))
    }.await;
    match result {
        Ok(value) => Json(value).into_response(),
        Err(e) => error(e),
    }
}
async fn apply(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<Apply>,
) -> Response {
    let result=async{let op=context(&principal)?;let api=api(&state)?;let target=target(&principal,body.file_path,body.target)?;if body.image_file_path.is_some()==body.image.is_some(){return Err(OfficeError::bad("provide exactly one existing image or uploaded image"));}
        if body.image_file_path.is_some()&&!principal.is_local_owner(){return Err(OfficeError{status:403,code:"OFFICE_FORBIDDEN",message:"imageFilePath requires local owner; upload image bytes for remote clients".into(),effect_unknown:false});}
        let response_target=target_response(&target);let args=json!({"targetResource":target,"imageFilePath":body.image_file_path,"image":body.image.map(|image|json!({"filename":image.filename.filter(|name|!name.trim().is_empty()).unwrap_or_else(||"cover.png".into()),"contentBase64":image.content_base64}))});
        let result=api.apply_cover(&args,&op,&operation(&state,&principal,body.request_id.as_deref())?).await?;let mut response=json!({"ok":true,"cover":result["cover"],"beautifyCover":result});if let Some(target)=response_target{response["target"]=target;}Ok::<_,OfficeError>(response)}.await;
    match result {
        Ok(value) => Json(value).into_response(),
        Err(e) => error(e),
    }
}
async fn preset(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<Preset>,
) -> Response {
    let result=async{use base64::Engine as _;let op=context(&principal)?;let api=api(&state)?;let target=target(&principal,body.file_path,body.target)?;
        let catalog=include_str!("../../../../shared/cover-gallery-presets.ts");let pattern=regex::Regex::new(r#"(?s)id:\s*"([a-z0-9-]+)".*?fileName:\s*"([a-z0-9-]+\.(?:jpg|png|webp))""#).expect("固定图库表达式");
        let name=pattern.captures_iter(catalog).find(|c|c.get(1).unwrap().as_str()==body.preset_id).map(|c|c.get(2).unwrap().as_str().to_owned()).ok_or_else(||OfficeError::bad("unknown cover preset"))?;
        let root=api.assets.as_ref().ok_or_else(||OfficeError::bad("bundled cover assets unavailable"))?.cover_gallery_dir.canonicalize().map_err(OfficeError::bad)?;let path=root.join(&name).canonicalize().map_err(OfficeError::bad)?;if !path.starts_with(&root){return Err(OfficeError::bad("cover preset leaves trusted assets"));}
        let bytes=crate::file_safety::Snapshot::read(&path).map_err(OfficeError::bad)?.bytes;
        let response_target=target_response(&target);let args=json!({"targetResource":target,"image":{"filename":name,"contentBase64":base64::engine::general_purpose::STANDARD.encode(bytes)}});
        let result=api.apply_cover(&args,&op,&operation(&state,&principal,body.request_id.as_deref())?).await?;let mut response=json!({"ok":true,"cover":result["cover"],"beautifyCover":result});if let Some(target)=response_target{response["target"]=target;}Ok::<_,OfficeError>(response)}.await;
    match result {
        Ok(value) => Json(value).into_response(),
        Err(e) => error(e),
    }
}
async fn generate(
    State(state): State<ServiceState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<Generate>,
) -> Response {
    let result = async {
        let context = context(&principal)?;
        if !principal.has_scope("chat") {
            return Err(OfficeError {
                status: 403,
                code: "OFFICE_FORBIDDEN",
                message: "chat permission required for background generation".into(),
                effect_unknown: false,
            });
        }
        let api = api(&state)?;
        let target = target(&principal, body.file_path, body.target)?;
        let (path, _, _) = api.read_resource(&target, &context).await?;
        if path
            .as_deref()
            .and_then(Path::extension)
            .and_then(|s| s.to_str())
            .is_none_or(|s| !s.eq_ignore_ascii_case("md"))
        {
            return Err(OfficeError::bad(
                "cover target must be an authorized Markdown file",
            ));
        }
        let agent = match body
            .executor_agent_id
            .or(body.agent_id)
            .filter(|id| !id.trim().is_empty())
        {
            Some(agent) => Some(agent.trim().to_owned()),
            None => api
                .memory
                .resolve_primary_agent_checked()
                .map_err(OfficeError::bad)?,
        }
        .ok_or_else(|| OfficeError::bad("actual primary agent unavailable"))?;
        api.enabled(&agent, "beautify")
            .map_err(|message| OfficeError {
                status: 403,
                code: "BEAUTIFY_DISABLED",
                message,
                effect_unknown: false,
            })?;
        let port = api.generation()?.ok_or_else(|| OfficeError {
            status: 503,
            code: "BEAUTIFY_GENERATION_UNAVAILABLE",
            message: "persistent media background runtime unavailable".into(),
            effect_unknown: false,
        })?;
        if body
            .theme_tone
            .as_deref()
            .is_some_and(|v| !["light", "dark"].contains(&v))
        {
            return Err(OfficeError::bad("invalid themeTone"));
        }
        port.generate(BeautifyGenerationRequest {
            principal,
            agent_id: agent,
            target,
            theme_tone: body.theme_tone.unwrap_or_else(|| "light".into()),
            user_guidance: body.user_guidance.unwrap_or_default(),
            request_id: body.request_id,
        })
        .await
    }
    .await;
    match result {
        Ok(value) => Json(value).into_response(),
        Err(e) => error(e),
    }
}
pub fn router() -> Router<ServiceState> {
    Router::new()
        .route("/lingxi/v1/desk/beautify/status", get(status))
        .route(
            "/lingxi/v1/desk/beautify/cover/apply",
            post(apply).layer(axum::extract::DefaultBodyLimit::max(36 * 1024 * 1024)),
        )
        .route("/lingxi/v1/desk/beautify/cover/preset/apply", post(preset))
        .route("/lingxi/v1/desk/beautify/cover", post(generate))
}
