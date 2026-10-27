//! GitHub 技能包只从固定归档入口下载，沿用网络护栏、完整包审查和取消边界。
use super::*;
use lingxi_adapters::models::{egress::EgressGuard, network::NetworkPlane};

#[derive(Debug, PartialEq)]
struct GithubSource {
    owner: String,
    repo: String,
    subpath: String,
}
impl GithubSource {
    fn parse(value: &str) -> Result<Self, MemoryError> {
        let invalid = || {
            MemoryError::InvalidRequest(
                "expected https://github.com/owner/repo or /tree/branch/skill-path".into(),
            )
        };
        let path = value
            .trim()
            .strip_prefix("https://github.com/")
            .ok_or_else(invalid)?;
        if path.contains(['?', '#', '%', '\\', '@']) {
            return Err(invalid());
        }
        let parts: Vec<_> = path.trim_end_matches('/').split('/').collect();
        if parts.len() < 2
            || parts.iter().any(|part| {
                part.is_empty()
                    || matches!(*part, "." | "..")
                    || !part
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            })
        {
            return Err(invalid());
        }
        if parts[0].len() > 39
            || parts[1].len() > 100
            || (parts.len() > 2 && (parts.len() < 4 || parts[2] != "tree"))
        {
            return Err(invalid());
        }
        let repo = parts[1].strip_suffix(".git").unwrap_or(parts[1]);
        if repo.is_empty() {
            return Err(invalid());
        }
        Ok(Self {
            owner: parts[0].into(),
            repo: repo.into(),
            subpath: parts.get(4..).unwrap_or(&[]).join("/"),
        })
    }
    fn archive_url(&self) -> String {
        format!(
            "https://codeload.github.com/{}/{}/zip/HEAD",
            self.owner, self.repo
        )
    }
    fn source_key(&self) -> String {
        format!("github:{}/{}:{}", self.owner, self.repo, self.subpath)
    }
}

pub(crate) fn check_cancelled(
    cancel: Option<&crate::cancel::CancelScope>,
) -> Result<(), MemoryError> {
    if cancel.is_some_and(crate::cancel::CancelScope::is_cancelled) {
        Err(MemoryError::InvalidRequest(
            "skill installation cancelled".into(),
        ))
    } else {
        Ok(())
    }
}
async fn download_package(
    source: &GithubSource,
    guard: &EgressGuard,
    url: &str,
    cancel: &crate::cancel::CancelScope,
) -> Result<completion::PreparedPackage, MemoryError> {
    check_cancelled(Some(cancel))?;
    let bytes = tokio::select! { biased;
        _ = cancel.cancelled() => return Err(MemoryError::InvalidRequest("skill installation cancelled".into())),
        result = tokio::time::timeout(std::time::Duration::from_secs(15), guard.download_skill_archive(url, None)) => {
            result.map_err(|_| MemoryError::InvalidRequest("GitHub archive download timed out".into()))?
                .map_err(|(error, _)| MemoryError::InvalidRequest(error.message))?
        }
    };
    check_cancelled(Some(cancel))?;
    completion::PreparedPackage::from_archive(&bytes, &source.subpath)
}
impl SkillService {
    #[cfg(test)]
    pub(crate) fn set_github_test_transport(&self, base: String, guard: Arc<EgressGuard>) {
        *self.github_test_transport.lock().unwrap() = Some((base, guard));
    }
    #[cfg(test)]
    pub(crate) fn clear_github_test_transport(&self) {
        *self.github_test_transport.lock().unwrap() = None;
    }
    pub fn set_github_network(&self, network: Arc<NetworkPlane>) {
        *self.github_network.lock().unwrap() = network;
    }
    pub(crate) async fn prepare_github_package(
        &self,
        url: &str,
        cancel: &crate::cancel::CancelScope,
    ) -> Result<(completion::PreparedPackage, String), MemoryError> {
        // 开关在任何解析、DNS和下载之前读取；关闭时绝不触网。
        let flags = self.learn_skills_flags()?;
        if !flags.0 {
            return Err(MemoryError::InvalidRequest(
                "skill installation is disabled (capabilities.learn_skills.enabled)".into(),
            ));
        }
        if !flags.1 {
            return Err(MemoryError::InvalidRequest(
                "GitHub fetching is disabled (capabilities.learn_skills.allow_github_fetch)".into(),
            ));
        }
        let source = GithubSource::parse(url)?;
        #[cfg(test)]
        {
            let transport = self.github_test_transport.lock().unwrap().clone();
            if let Some((base, guard)) = transport {
                let archive_url = format!("{base}/{}/{}/zip/HEAD", source.owner, source.repo);
                let prepared = download_package(&source, &guard, &archive_url, cancel).await?;
                return Ok((prepared, source.source_key()));
            }
        }
        let network = self.github_network.lock().unwrap().clone();
        let guard = EgressGuard::from_provider_endpoints_and_network(&[], network)
            .map_err(|error| MemoryError::InvalidRequest(error.message))?;
        let prepared = download_package(&source, &guard, &source.archive_url(), cancel).await?;
        Ok((prepared, source.source_key()))
    }
}

/// 安装先在同盘隐藏目录准备，最后替换；复制失败或提交前取消不暴露半个技能包。
impl SkillService {
    pub(crate) fn install_reviewed_package(
        &self,
        source: &Path,
        subpath: &str,
        cancel: Option<&crate::cancel::CancelScope>,
    ) -> Result<sl::InstalledSkill, MemoryError> {
        let _lock = self.mutation_lock.lock().unwrap();
        let _publication = am::ContextMaterialMutation::for_home(&self.home);
        check_cancelled(cancel)?;
        let pool = self.skills_dir();
        std::fs::create_dir_all(&pool)?;
        let staging = pool.join(format!(".install-{}", am::random_token_suffix()));
        std::fs::create_dir(&staging)?;
        struct Cleanup(Option<PathBuf>);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                if let Some(path) = &self.0 {
                    if let Err(error) = std::fs::remove_dir_all(path) {
                        tracing::warn!(%error, "技能准备目录清理失败");
                    }
                }
            }
        }
        let mut cleanup = Cleanup(Some(staging.clone()));
        let mut installed =
            sl::install_skill_package_from_directory(source, &staging, subpath, Some(false))?;
        let target = pool.join(&installed.name);
        // 同名更新保留原有非覆盖文件；目标不是完整目录时明确失败，不动原资料。
        if target.symlink_metadata().is_ok() {
            let original = sl::prepare_local_skill_package(&target)?;
            let merged = staging.join("merged");
            std::fs::create_dir(&merged)?;
            sl::install_skill_package_from_directory(&original, &merged, "", Some(false))?;
            installed =
                sl::install_skill_package_from_directory(source, &merged, subpath, Some(false))?;
        }
        check_cancelled(cancel)?;
        let backup = staging.join("previous");
        let had_target = target.symlink_metadata().is_ok();
        if had_target {
            std::fs::rename(&target, &backup)?;
        }
        if let Err(error) = std::fs::rename(&installed.dir, &target) {
            if had_target {
                if let Err(restore) = std::fs::rename(&backup, &target) {
                    cleanup.0 = None;
                    return Err(MemoryError::Io(format!("skill commit failed: {error}; previous skill retained at {} but restore failed: {restore}", backup.display())));
                }
            }
            return Err(MemoryError::Io(error.to_string()));
        }
        installed.dir = target.clone();
        installed.file_path = target.join("SKILL.md");
        Ok(installed)
    }
}
