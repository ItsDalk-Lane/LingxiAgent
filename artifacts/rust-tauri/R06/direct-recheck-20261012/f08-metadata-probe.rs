use lingxi_kernel::ports::{ToolOutcome, ToolSuccess};
use lingxi_protocol::{ContentBlock, ResourceId, ResourceKind, ResourceRef, ToolResultStatus, ToolResultWire};
use serde_json::json;
fn main() {
    // 复现worker真实构造的多文件结果形状；100个短于系统路径限制的名称仍在worker线体上限内。
    let paths = (0..100).map(|i| format!("/synthetic-worker/{}/{}/f{i}.txt", "a".repeat(190), "b".repeat(190))).collect::<Vec<_>>();
    let worker_line = json!({"kind":"result","id":"synth","ok":true,"content":[{"type":"text","text":"created"}],"claimed_files":paths});
    assert!(worker_line.to_string().len()<lingxi_service::workerrpc::WORKER_MAX_LINE_BYTES);
    let mut original = ToolSuccess::from_content(vec![ContentBlock::Text { text:"created".into() }]);
    original.resource_refs = paths.iter().map(|path| ResourceRef {resource_id:ResourceId::new(format!("worker:synth:{path}")),kind:ResourceKind::Artifact,display_name:Some(path.clone()),uri:Some(format!("file://{path}")),digest:None,size_bytes:Some(1)}).collect();
    let outcome=lingxi_service::hooks::project_tool_success(&original,&[],original.content.clone(),false);
    let wire = match &outcome { ToolOutcome::Failed {error} => ToolResultWire{status:ToolResultStatus::Failed,content:vec![],resource_refs:vec![],truncated:false,error:Some(error.clone())}, other=>panic!("expected explicit over-budget projection refusal, got {other:?}") };
    // 按runs.rs当前记录表达式重现，不修改任何生产函数。
    let projection=json!({"type":"tool_result_model_projection","schema":"lingxi.tool-result-model-projection.v1","toolCallId":"synth","target":"plugin:synth","providerCallId":"provider-synth","rawEventId":"synth-run-synth-done","rawContentDigest":original.content_digest,"projectionContentDigest":null,"processStatus":null,"result":wire,"renderedBytes":lingxi_adapters::models::tool_render::render_tool_outcome_text(&outcome).len(),"successBudgetBytes":lingxi_service::hooks::MAX_TOOL_SUCCESS_PROJECTION_BYTES,"errorBudgetPolicy":"untruncated","scope":"model_context_projection_not_raw_tool_archive"});
    let err=lingxi_service::model_history::projected_tool_item(&projection).expect_err("现有投影验证必须暴露metadata衔接错误");
    println!("worker_line_bytes={};refs={};max_path_bytes={};projected_outcome=failed;projection_parse_error={}",worker_line.to_string().len(),original.resource_refs.len(),paths.iter().map(String::len).max().unwrap(),err);
    assert!(err.to_string().contains("non-success projection contains success metadata"));
}
