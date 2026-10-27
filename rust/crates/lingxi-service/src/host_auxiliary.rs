//! 安装包或开发宿主的外围能力组合；HTTP 和模型不能选择程序、素材或登录目录。
use crate::{
    media_jimeng::JimengLaunch,
    office_tools::{OfficeAssets, OfficeWorkerLaunch},
    ServiceDeps,
};
use std::path::{Path, PathBuf};

fn setting(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from)
}
fn checked(path: PathBuf, directory: bool) -> Result<PathBuf, String> {
    let meta = std::fs::symlink_metadata(&path).map_err(|_| "外围能力路径不存在")?;
    if !path.is_absolute()
        || meta.file_type().is_symlink()
        || if directory {
            !meta.is_dir()
        } else {
            !meta.is_file()
        }
    {
        return Err("外围能力要求真实的绝对文件或目录".into());
    }
    path.canonicalize()
        .map_err(|_| "外围能力路径无法确认".into())
}

pub fn configure(deps: &mut ServiceDeps) -> Result<(), String> {
    let office = [
        "LINGXI_OFFICE_NODE",
        "LINGXI_OFFICE_WORKER",
        "LINGXI_OFFICE_THEMES",
        "LINGXI_OFFICE_COVERS",
    ]
    .map(setting);
    let selected = if office.iter().any(Option::is_some) {
        let [Some(node), Some(worker), Some(themes), Some(covers)] = office else {
            return Err("办公运行环境配置不完整".into());
        };
        Some((node, worker, themes, covers))
    } else {
        // 开发模式仅沿已经选定的产品 SDK 根定位，不能搜索个人目录。
        deps.bridge_worker_launch.as_ref().and_then(|bridge| {
            let entry = Path::new(bridge.args.last()?);
            let source = entry.file_name()?.to_str()? == "bridge-sdk-worker.ts";
            let root = entry.parent()?.parent()?;
            let worker = root.join(if source {
                "scripts/office-document-worker.ts"
            } else {
                "bundle/office-document-worker.mjs"
            });
            let themes = root.join(if source {
                "desktop/src/themes"
            } else {
                "desktop/dist-renderer/themes"
            });
            let covers = root.join("desktop/src/assets/cover-gallery");
            (worker.is_file() && themes.is_dir() && covers.is_dir())
                .then(|| (bridge.program.clone(), worker, themes, covers))
        })
    };
    if let Some((node, worker, themes, covers)) = selected {
        let node = checked(node, false)?;
        let worker = checked(worker, false)?;
        deps.office_document_worker = Some(OfficeWorkerLaunch {
            argv: vec![
                node.to_string_lossy().into_owned(),
                "--max-old-space-size=384".into(),
                worker.to_string_lossy().into_owned(),
            ],
            cwd: worker.parent().ok_or("办公解析器缺少目录")?.to_path_buf(),
        });
        deps.office_assets = Some(OfficeAssets {
            themes_dir: checked(themes, true)?,
            cover_gallery_dir: checked(covers, true)?,
        });
    }
    let executor = setting("LINGXI_JIMENG_EXECUTABLE");
    let private = setting("LINGXI_JIMENG_PRIVATE_ROOT");
    let login = setting("LINGXI_JIMENG_LOGIN_HOME");
    let generation = std::env::var("LINGXI_JIMENG_GRANT_GENERATION").ok();
    if executor.is_some() || private.is_some() || login.is_some() || generation.is_some() {
        let (Some(executor), Some(private_root), Some(external_login_home), Some(generation)) =
            (executor, private, login, generation)
        else {
            return Err("即梦执行器与外部登录许可配置不完整".into());
        };
        let node = setting("LINGXI_MEDIA_NODE").ok_or("即梦纯解析器Node未配置")?;
        let helper = setting("LINGXI_MEDIA_HELPER").ok_or("即梦纯解析器未配置")?;
        let credential_grant_generation = generation
            .parse::<u64>()
            .ok()
            .filter(|n| *n > 0)
            .ok_or("即梦登录许可代次无效")?;
        deps.jimeng_launch = Some(JimengLaunch {
            executable: checked(executor, false)?,
            helper_argv: vec![
                checked(node, false)?.to_string_lossy().into_owned(),
                checked(helper, false)?.to_string_lossy().into_owned(),
            ],
            private_root,
            external_login_home,
            credential_grant_generation,
        });
    }
    Ok(())
}
