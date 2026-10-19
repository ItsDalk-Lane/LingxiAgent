//! 现役五个媒体工具的正式网关入口；生成必须绑定真实供应商与批准参数。
use crate::{
    media::{MediaService, MediaSpec},
    toolgateway::ToolInvocationGateway,
};
use lingxi_kernel::{
    ports::{ToolExecutionResult, ToolExecutorPort, ToolOutcome, ToolRequest, ToolSuccess},
    toolcatalog::{
        Availability, DeclaredPermission, PermissionContract, PermissionKind, SchemaBudget,
        ToolManifest, ToolOrigin,
    },
    RunContext,
};
use lingxi_protocol::{ErrorCode, ProtocolError, ToolCallId, ToolSchemaDocument};
use serde_json::{json, Value};
use std::sync::Arc;

pub const MEDIA_GUIDE:&str="# 媒体生成\n\n先用 mcp_describe_tool 读取 media 来源工具的正式说明，再通过 mcp_call 调用；本会话首次生成前先读取 get-guide。图片、视频和语音生成沿当前会话的后台任务执行。工具返回已提交或等待中，只代表任务受理；只有 status=completed 且有 files 时才是生成并交付成功。结果会通过当前会话的媒体回执更新。不要把任务编号当作文件，不要重复提交，不要调用 stage_files。\n\n普通生成省略 provider 和 model，由宿主使用配置的默认供应商。指定模型时同时指定 provider。即梦 CLI 必须先由宿主配置独立执行器和登录授权，再通过 media_describe-options 探测当前版本与四种模式；工具不会自动安装或登录。即梦查询命令内部包含下载，宿主会另行核验实际文件。高级参数先通过 media_describe-options 查询；候选列表不是必须选择的菜单。供应商失败后不会自动换平台。\n\nmedia_generate-image：prompt 必填，可用 count、ratio、resolution、quality；编辑或融合图片只接受当前会话的 {kind:session_file,fileId}。media_generate-video：prompt 必填，可用 image、duration、ratio、resolution。media_generate-speech：prompt 必填，可用 voice、speed、format。\n\n取消只停止本地等待与交付；远端取消不受支持时，远端任务仍可能继续。重启遇到未确认的提交，不会为了追求成功再次付费提交。";
struct Executor {
    media: Arc<MediaService>,
    name: &'static str,
}
impl ToolExecutorPort for Executor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RunContext,
        call: &'a ToolCallId,
        request: &'a ToolRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ToolExecutionResult> + Send + 'a>> {
        Box::pin(async move {
            let result=async{
                if request.target!=format!("tool:plugin:media:{}",self.name){return Err("媒体工具身份不正确".to_string());}
                let value=if self.name=="get-guide"{self.media.caller(ctx,call,request).await?;json!({"guide":MEDIA_GUIDE,"version":2})}
                else if self.name=="describe-options"{self.media.caller(ctx,call,request).await?;self.media.describe_options(ctx,call,request.arguments.as_value()).await?}
                else{let kind=self.name.strip_prefix("generate-").ok_or("媒体工具不受支持")?;self.media.submit(ctx,call,request,kind).await?};
                let mut success=ToolSuccess::text(if self.name=="get-guide"{MEDIA_GUIDE.into()}else{serde_json::to_string_pretty(&value).map_err(|_|"媒体回执编码失败")?});
                success.details=Some(if self.name.starts_with("generate-"){json!({"mediaGeneration":{"kind":value["kind"],"batchId":value["taskId"],"prompt":request.arguments.as_value()["prompt"],"delivery":"response","tasks":[value]}}).as_object().expect("媒体回执").clone()}else if self.name=="describe-options"{json!({"mediaOptions":value}).as_object().expect("媒体选项").clone()}else{json!({"version":2}).as_object().expect("媒体指南").clone()});Ok::<_,String>(success)
            }.await;
            ToolExecutionResult::of_ctx(
                ctx,
                match result {
                    Ok(result) => ToolOutcome::Success { result },
                    Err(message) => ToolOutcome::Failed {
                        error: ProtocolError::new(ErrorCode::UpstreamUnavailable, message, false),
                    },
                },
            )
        })
    }
}
pub fn register(
    gateway: &ToolInvocationGateway,
    media: Arc<MediaService>,
    budget: &SchemaBudget,
) -> Result<(), String> {
    for name in [
        "get-guide",
        "describe-options",
        "generate-image",
        "generate-video",
        "generate-speech",
    ] {
        let generate = name.starts_with("generate-");
        let receipt=gateway.registry().register(ToolManifest{origin:ToolOrigin::Plugin{plugin_id:"media".into()},local_name:name.into(),display_name:format!("media_{name}"),aliases:vec![format!("media_{name}")],version:"0.1.0".into(),description:if generate{"异步生成媒体；返回受理回执，最终只有文件交付后才完成。必须使用当前会话文件引用，保持供应商与批准范围一致。"}else{"读取当前已配置的媒体生成选项或使用指南，不发起媒体生成。"}.into(),input_schema:ToolSchemaDocument{dialect:"json-schema/2020-12".into(),schema:schema(name)},output_schema:None,permission:PermissionContract{kind:if generate{PermissionKind::Review}else{PermissionKind::Read},capability_base:format!("media.{name}")},availability:Availability::Available,timeout_ms:Some(30_000),max_concurrency:Some(32),declared_permission:if generate{DeclaredPermission::Other}else{DeclaredPermission::ReadOnly},recovery:if generate{lingxi_kernel::invocation::ToolRecoveryCapability::CONSERVATIVE}else{lingxi_kernel::invocation::ToolRecoveryCapability{read_only:true,honors_idempotency_key:false,externally_verifiable:false}}},budget).map_err(|e|format!("{e:?}"))?;
        let review = media.clone();
        gateway.bind_executor_with_review(receipt.target_id,Arc::new(Executor{media:media.clone(),name}),Arc::new(move|input,args|{
            review.enabled(input.agent_id,"media")?;
            let mut scope=json!({"plugin":"media","sourceSessionId":input.session_id,"actorId":input.agent_id,"action":name});
            if let Some(kind)=name.strip_prefix("generate-"){
                let spec=MediaSpec::parse(kind,args.as_value())?;
                scope["mediaRoute"]=review.review_route(kind,&spec)?;
                scope["sideEffect"]=json!({"kind":"external_generation","provider":scope["mediaRoute"]["provider"],"model":scope["mediaRoute"]["model"],"count":spec.count.unwrap_or(1)});
                scope["summary"]=json!(format!("向 {} 提交{}生成；此操作可能产生费用。",scope["mediaRoute"]["provider"].as_str().unwrap_or("未配置"),kind));
            }
            Ok(scope)
        }),"媒体来源、供应商、模型、参数与批准范围绑定");
    }
    Ok(())
}
fn schema(name: &str) -> Value {
    if name == "get-guide" {
        return json!({"type":"object","properties":{},"additionalProperties":false});
    }
    let text = json!({"type":"string","minLength":1,"maxLength":256});
    if name == "describe-options" {
        return json!({"type":"object","properties":{"kind":{"type":"string","enum":["image","video"]},"provider":text,"model":text,"mode":text},"required":["kind"],"additionalProperties":false});
    }
    let reference = json!({"type":"object","properties":{"kind":{"type":"string","enum":["session_file"]},"fileId":{"type":"string","minLength":4,"maxLength":256}},"required":["kind","fileId"],"additionalProperties":false});
    let mut properties = json!({"prompt":{"type":"string","minLength":1,"maxLength":32000},"provider":text,"model":text});
    if name == "generate-speech" {
        properties["voice"] = text.clone();
        properties["speed"] = json!({"type":"number","exclusiveMinimum":0,"maximum":4});
        properties["format"] =
            json!({"type":"string","enum":["mp3","opus","aac","flac","wav","pcm","m4a"]});
    } else {
        properties["image"] = reference.clone();
        properties["ratio"] = text.clone();
        properties["resolution"] = text.clone();
        properties["mode"] = text.clone();
        properties["options"] = json!({"type":"object","additionalProperties":true});
        if name == "generate-image" {
            properties["count"] = json!({"type":"integer","minimum":1,"maximum":10});
            properties["referenceImages"] = json!({"type":"array","items":reference,"maxItems":10});
            properties["quality"] = text.clone();
            properties["suggestedFilename"] =
                json!({"type":"string","minLength":1,"maxLength":200});
        } else {
            properties["duration"] = json!({"type":"number","exclusiveMinimum":0,"maximum":600});
        }
    }
    json!({"type":"object","properties":properties,"required":["prompt"],"additionalProperties":false})
}
