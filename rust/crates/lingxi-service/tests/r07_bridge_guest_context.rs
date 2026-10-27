//! 真实公开材料采集，私人资料故意损坏；访客不能靠模型输入读取私人信息。
use lingxi_service::context_compiler::{
    ContextCompilerService, FileContextMaterialSource, HostFacts,
};
use lingxi_test_guard as _;
use std::{path::PathBuf, sync::Arc};
fn fixture() -> (PathBuf, ContextCompilerService) {
    let home = std::env::temp_dir().join(format!(
        "r07-bridge-guest-{}",
        lingxi_adapters::storage::agent_memory::random_token_suffix()
    ));
    for path in ["agents/lingxi/memory/dream", "product/yuan", "user"] {
        std::fs::create_dir_all(home.join(path)).unwrap();
    }
    std::fs::write(
        home.join("agents/lingxi/config.yaml"),
        "agent:\n  name: 公开助手\nyuan: lingxi\nlocale: zh-CN\n",
    )
    .unwrap();
    std::fs::write(
        home.join("product/yuan/lingxi.md"),
        "PUBLIC_PERSONA {{agentName}} {{userName}}",
    )
    .unwrap();
    std::fs::write(
        home.join("agents/lingxi/AGENTS.public.md"),
        "PUBLIC_AGENT_GUIDANCE",
    )
    .unwrap();
    for path in [
        "agents/lingxi/identity.md",
        "agents/lingxi/AGENTS.md",
        "agents/lingxi/memory/memory.md",
        "user/user.md",
    ] {
        std::fs::write(home.join(path), "PRIVATE_MATERIAL_MUST_NOT_ENTER_GUEST").unwrap();
    }
    for path in [
        "agents/lingxi/memory/dream/pending-apply.json",
        "agents/lingxi/memory/tenets.json",
        "user/preferences.json",
    ] {
        std::fs::write(home.join(path), "{corrupt-private-json").unwrap();
    }
    let home = home.canonicalize().unwrap();
    let source = FileContextMaterialSource::from_home(
        &home,
        home.join("product"),
        HostFacts {
            platform: "PRIVATE_HOST_PLATFORM".into(),
            os_type: "PrivateOS".into(),
            os_release: "private-release".into(),
            shell_label: "private-shell".into(),
            workspace: home.to_string_lossy().into_owned(),
        },
    );
    (home, ContextCompilerService::new(Arc::new(source)))
}
#[test]
fn guest_reads_only_public_persona_and_never_private_preferences_or_memory() {
    let (home, compiler) = fixture();
    let first = compiler
        .compile_guest_snapshot("lingxi", 1_800_000_000_000)
        .unwrap();
    assert!(first.render().contains("PUBLIC_PERSONA 公开助手 访客"));
    assert!(first.render().contains("PUBLIC_AGENT_GUIDANCE"));
    assert!(!first.render().contains("PRIVATE"));
    assert!(!first.render().contains(home.to_string_lossy().as_ref()));
    assert_eq!(first.workspace(), None);
    assert!(
        compiler
            .compile_snapshot("owned", "lingxi", false, 1_800_000_000_000, true)
            .is_err(),
        "同一真实材料的私人读取确实失败，访客没有先读后裁剪"
    );
    std::fs::write(
        home.join("agents/lingxi/AGENTS.public.md"),
        "PUBLIC_UPDATED",
    )
    .unwrap();
    assert!(compiler
        .compile_guest_snapshot("lingxi", 1_800_000_000_000)
        .unwrap()
        .render()
        .contains("PUBLIC_UPDATED"));
    assert!(!first.render().contains("PUBLIC_UPDATED"));
}
#[test]
fn damaged_public_material_is_a_visible_error_and_cannot_fall_back_to_private_files() {
    let (home, compiler) = fixture();
    std::fs::remove_file(home.join("agents/lingxi/AGENTS.public.md")).unwrap();
    std::fs::create_dir(home.join("agents/lingxi/AGENTS.public.md")).unwrap();
    assert!(compiler
        .compile_guest_snapshot("lingxi", 1_800_000_000_000)
        .is_err());
    assert!(compiler
        .compile_guest_snapshot("../lingxi", 1_800_000_000_000)
        .is_err());
}
