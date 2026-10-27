//! R08-T04: 批次计划。粒度 = (store_class, 源文件)，键 =
//! `{store_class}/{relpath}`；两次扫描同根必得同一计划（manifest 已排序）。

use super::classes::{classify, Strategy};
use super::manifest::Manifest;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlannedBatch {
    pub(crate) batch_key: String,
    pub(crate) store_class: String,
    pub(crate) strategy: Strategy,
    pub(crate) source_relpath: String,
    pub(crate) source_sha256: String,
    pub(crate) size: u64,
}

/// 未命中任何注册表类的文件：不静默忽略，逐条列出供 issues 使用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UnmappedFile {
    pub(crate) relpath: String,
    pub(crate) size: u64,
    pub(crate) sha256: String,
}

pub(crate) struct Plan {
    pub(crate) batches: Vec<PlannedBatch>,
    pub(crate) unmapped: Vec<UnmappedFile>,
}

pub(crate) fn build_plan(manifest: &Manifest) -> Plan {
    let mut batches = Vec::new();
    let mut unmapped = Vec::new();
    for entry in &manifest.entries {
        match classify(&entry.relpath) {
            Some((def, _pattern)) => batches.push(PlannedBatch {
                batch_key: format!("{}/{}", def.id, entry.relpath),
                store_class: def.id.to_string(),
                strategy: def.strategy,
                source_relpath: entry.relpath.clone(),
                source_sha256: entry.sha256.clone(),
                size: entry.size,
            }),
            None => unmapped.push(UnmappedFile {
                relpath: entry.relpath.clone(),
                size: entry.size,
                sha256: entry.sha256.clone(),
            }),
        }
    }
    batches.sort_by(|a, b| a.batch_key.cmp(&b.batch_key));
    // A07 附件实转：session-files 文件级批次是 register（字节收据）；每个
    // 含 *.meta.json 描述符的 session-files/{hash} 目录追加一个目录级
    // convert 批次（payload 校验 + 坏因 + canonical 登记，由 service 侧
    // sessionfiles 扩展执行）。两粒度并存，batch_key 不冲突。
    let mut dirs: std::collections::BTreeMap<String, Vec<(&str, u64)>> =
        std::collections::BTreeMap::new();
    for entry in &manifest.entries {
        let mut segments = entry.relpath.split('/');
        if segments.next() != Some("session-files") {
            continue;
        }
        let (Some(hash), Some(_file), None) = (segments.next(), segments.next(), segments.next())
        else {
            continue;
        };
        dirs.entry(format!("session-files/{hash}"))
            .or_default()
            .push((entry.sha256.as_str(), entry.size));
    }
    for (dir_rel, members) in dirs {
        if !members.is_empty()
            && manifest.entries.iter().any(|entry| {
                entry.relpath.starts_with(&format!("{dir_rel}/"))
                    && entry.relpath.ends_with(".meta.json")
            })
        {
            let mut fingerprint = members
                .iter()
                .map(|(sha, size)| format!("{sha}:{size}"))
                .collect::<Vec<_>>();
            fingerprint.sort();
            let size = members.iter().map(|(_, size)| size).sum::<u64>();
            batches.push(PlannedBatch {
                batch_key: format!("session-files/{dir_rel}"),
                store_class: "session-files".to_string(),
                strategy: Strategy::Convert,
                source_relpath: format!("{dir_rel}/"),
                source_sha256: super::manifest::sha256_hex(
                    fingerprint.join("\n").as_bytes(),
                ),
                size,
            });
        }
    }
    batches.sort_by(|a, b| a.batch_key.cmp(&b.batch_key));
    Plan { batches, unmapped }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::legacy_import::manifest::ManifestEntry;

    #[test]
    fn plan_is_sorted_and_keys_are_stable() {
        let manifest = Manifest {
            entries: vec![
                ManifestEntry {
                    relpath: "agents/a1/sessions/s2.jsonl".into(),
                    size: 10,
                    sha256: "b".repeat(64),
                },
                ManifestEntry {
                    relpath: "session-manifest.db".into(),
                    size: 20,
                    sha256: "a".repeat(64),
                },
                ManifestEntry {
                    relpath: "mystery.bin".into(),
                    size: 1,
                    sha256: "c".repeat(64),
                },
            ],
            skipped_special: vec![],
        };
        let plan = build_plan(&manifest);
        assert_eq!(plan.batches.len(), 2);
        assert_eq!(plan.batches[0].batch_key, "session-jsonl/agents/a1/sessions/s2.jsonl");
        assert_eq!(
            plan.batches[1].batch_key,
            "session-manifest-sqlite/session-manifest.db"
        );
        assert_eq!(plan.batches[0].strategy, Strategy::Convert);
        assert_eq!(plan.unmapped.len(), 1);
        assert_eq!(plan.unmapped[0].relpath, "mystery.bin");
    }
}
