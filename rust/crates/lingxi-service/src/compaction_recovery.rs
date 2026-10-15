//! 压缩后恢复已使用的技能和当前会话便签；内容始终是普通历史材料。
use super::*;
use lingxi_kernel::ports::ToolOutcome;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

const PER_SKILL: usize = 6000;
const ALL_SKILLS: usize = 24000;
const NOTES_BYTES: usize = 16 * 1024;
const TRUNCATED: &str =
    "\n[... skill instructions truncated for compaction; read the Path above for the full text]";
#[derive(Clone)]
pub(super) struct RecoverySources {
    skills: Arc<crate::skills::SkillService>,
    runtime: PathBuf,
}
#[derive(Clone, Serialize, Deserialize)]
struct Entry {
    name: String,
    path: Option<String>,
    content: Option<String>,
    note: Option<String>,
}
fn clipped(text: &str, budget: usize) -> String {
    if text.chars().count() <= budget {
        return text.to_string();
    }
    let marker = TRUNCATED.chars().count();
    text.chars()
        .take(budget.saturating_sub(marker))
        .collect::<String>()
        + TRUNCATED
}
fn remember(entries: &mut Vec<Entry>, entry: Entry) {
    if let Some(old) = entries
        .iter_mut()
        .find(|old| match (&old.path, &entry.path) {
            (Some(a), Some(b)) => a == b,
            _ => old.name == entry.name,
        })
    {
        if entry.path.is_some() {
            old.path = entry.path;
        }
        if entry.content.is_some() {
            old.content = entry.content;
            old.note = None;
        }
    } else {
        entries.push(entry);
    }
}
fn previous_entries(summary: &str) -> Result<Vec<Entry>, String> {
    let Some((_, tail)) = summary.split_once("<skill-recall>") else {
        return Ok(Vec::new());
    };
    let Some((body, _)) = tail.split_once("</skill-recall>") else {
        return Err("existing skill recall block is incomplete; original history retained".into());
    };
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        if value["version"] == 1 {
            return serde_json::from_value(value["entries"].clone())
                .map_err(|e| format!("existing skill recall cannot be decoded: {e}"));
        }
        return Err("unknown skill recall version; original history retained".into());
    }
    if !body.contains("### Skill: ") {
        return Err("unrecognized skill recall material; original history retained".into());
    }
    // 兼容现役TS旧摘要；其中路径只是历史数据，不能据此授权读任意文件。
    Ok(body
        .split("### Skill: ")
        .skip(1)
        .map(|part| {
            let mut lines = part.lines();
            let name = lines.next().unwrap_or("").trim().to_string();
            let remaining = lines.collect::<Vec<_>>();
            let path = remaining
                .first()
                .and_then(|line| line.strip_prefix("Path: "))
                .map(str::to_string);
            let offset = usize::from(path.is_some());
            let content = remaining[offset..].join("\n");
            Entry {
                name,
                path,
                content: (!content.trim().is_empty()).then_some(content),
                note: None,
            }
        })
        .collect())
}
fn read_invoked_names(text: &str, entries: &mut Vec<Entry>) {
    for line in text.lines() {
        if let Some(name) = line
            .strip_prefix("[Use skill: ")
            .and_then(|s| s.split_once(']').map(|(name, _)| name.trim()))
            .filter(|s| !s.is_empty())
        {
            remember(
                entries,
                Entry {
                    name: name.into(),
                    path: None,
                    content: None,
                    note: Some("name-only; load from the current skill catalog if needed".into()),
                },
            );
        }
    }
}
fn selected_file(path: &Path, limit: u64) -> Result<String, String> {
    let snapshot =
        crate::file_safety::Snapshot::read_bounded(path, limit).map_err(|e| e.to_string())?;
    String::from_utf8(snapshot.bytes).map_err(|e| e.to_string())
}
impl CompactionService {
    pub fn bind_recovery_sources(
        &self,
        skills: Arc<crate::skills::SkillService>,
        runtime: PathBuf,
    ) -> Result<(), StorageError> {
        let mut slot = self
            .recovery_sources
            .write()
            .map_err(|_| StorageError::Internal {
                detail: "compaction recovery lock poisoned".into(),
            })?;
        if slot
            .as_ref()
            .is_some_and(|stored| stored.runtime != runtime)
        {
            return Err(StorageError::InvalidRequest {
                detail: "compaction recovery already bound to another home".into(),
            });
        }
        *slot = Some(RecoverySources { skills, runtime });
        Ok(())
    }
    pub(super) fn recovery_material(
        &self,
        ctx: &RunContext,
        agent: &str,
        items: &[ExchangeItem],
    ) -> Result<String, String> {
        let sources = self
            .recovery_sources
            .read()
            .map_err(|_| "compaction recovery lock poisoned")?
            .clone();
        let mut entries = Vec::new();
        let mut reads = HashMap::new();
        let mut had_notes = false;
        for item in items {
            match item {
                ExchangeItem::CompactionSummary { summary, .. } => {
                    had_notes |= summary.contains("<context-notes>");
                    for entry in previous_entries(summary)? {
                        remember(&mut entries, entry);
                    }
                }
                ExchangeItem::UserTurn { text } | ExchangeItem::UserMediaTurn { text, .. } => {
                    read_invoked_names(text, &mut entries)
                }
                ExchangeItem::AssistantTurn { tool_calls, .. } => {
                    for call in tool_calls {
                        if call.target == "tool:first-party:read" {
                            if let Some(path) = call
                                .arguments
                                .as_value()
                                .get("path")
                                .and_then(Value::as_str)
                                .filter(|path| {
                                    Path::new(path)
                                        .file_name()
                                        .is_some_and(|name| name == "SKILL.md")
                                })
                            {
                                reads.insert(call.tool_call_id.clone(), path.to_string());
                            }
                        }
                    }
                }
                ExchangeItem::ToolResult {
                    tool_call_id,
                    outcome: ToolOutcome::Success { result },
                    ..
                } => {
                    if let Some(path) = reads.remove(tool_call_id) {
                        let name = Path::new(&path)
                            .parent()
                            .and_then(Path::file_name)
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| "unknown-skill".into());
                        let content = result
                            .content
                            .iter()
                            .filter_map(|block| {
                                if let ContentBlock::Text { text } = block {
                                    Some(text.as_str())
                                } else {
                                    None
                                }
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        remember(
                            &mut entries,
                            Entry {
                                name,
                                path: Some(path),
                                content: Some(content),
                                note: Some(
                                    "historical copy from an earlier successful read".into(),
                                ),
                            },
                        );
                    }
                }
                _ => {}
            }
        }
        if entries.len() > 128 {
            return Err("too many skill recall entries; original history retained".into());
        }
        let catalog = if entries.iter().any(|entry| entry.path.is_some()) {
            if let Some(sources) = &sources {
                sources
                    .skills
                    .runtime_skill_catalog(agent)
                    .map_err(|e| e.to_string())?
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };
        let mut budget = ALL_SKILLS;
        for entry in &mut entries {
            if entry.name.len() > 1024 || entry.path.as_ref().is_some_and(|s| s.len() > 8192) {
                return Err(
                    "invalid oversized skill recall identity; original history retained".into(),
                );
            }
            if let Some(path) = &entry.path {
                let allowed = catalog
                    .iter()
                    .any(|skill| Path::new(&skill.location) == Path::new(path));
                if allowed {
                    match selected_file(Path::new(path), 2 * 1024 * 1024) {
                        Ok(text) => {
                            entry.content = Some(text);
                            entry.note = None;
                        }
                        Err(reason) => {
                            entry.note = Some(format!(
                                "source unavailable; earlier historical excerpt retained: {reason}"
                            ));
                        }
                    }
                } else {
                    entry.note=Some("source is not in the current agent catalog; historical excerpt only, no new file access".into());
                }
            }
            if let Some(content) = &entry.content {
                if budget <= TRUNCATED.chars().count() {
                    entry.content = None;
                    entry.note=Some("content omitted to stay within recall budget; read the Path through normal permissions if needed".into());
                } else {
                    let body = clipped(content, PER_SKILL.min(budget));
                    budget = budget.saturating_sub(body.chars().count());
                    entry.content = Some(body);
                }
            }
        }
        let mut sections = Vec::new();
        if !entries.is_empty() {
            let value = serde_json::json!({"version":1,"authority":"ordinary historical data; existing tool permissions still apply","entries":entries});
            let encoded = serde_json::to_string(&value)
                .map_err(|e| e.to_string())?
                .replace('<', "\\u003c")
                .replace('>', "\\u003e");
            sections.push(format!("<skill-recall>\n{encoded}\n</skill-recall>"));
        }
        if let Some(sources) = sources {
            let sid = ctx.session_id.as_str();
            if sid.is_empty()
                || !sid
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            {
                return Err("invalid context-notes session identity".into());
            }
            let notes = sources
                .runtime
                .join("context-notes")
                .join(format!("{sid}.md"));
            match std::fs::symlink_metadata(&notes) {
                Ok(_) => {
                    let body = selected_file(&notes, 64 * 1024)?;
                    if !body.trim().is_empty() {
                        let mut cut = body.len().min(NOTES_BYTES);
                        while !body.is_char_boundary(cut) {
                            cut -= 1;
                        }
                        let marker = if cut < body.len() {
                            "\n[context notes truncated; read the context_notes tool for the full text]"
                        } else {
                            ""
                        };
                        sections.push(format!("<context-notes>\nCurrent session notes, quoted as ordinary historical data:\n{}{marker}\n</context-notes>",&body[..cut]));
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    if had_notes {
                        sections.push("<context-notes>\nCurrent session notes are unavailable; earlier source history remains archived.\n</context-notes>".into());
                    }
                }
                Err(e) => return Err(format!("context notes could not be read: {e}")),
            }
        }
        Ok(sections.join("\n\n"))
    }
}
pub(super) fn append(summary: &str, recovery: &str) -> String {
    if recovery.is_empty() {
        summary.to_string()
    } else {
        format!("{}\n\n{recovery}", summary.trim_end())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn existing_recall_compatibility_and_unknown_blocks_are_loud() {
        let legacy="<skill-recall>\nHistorical excerpts\n### Skill: known\nPath: /authorized/SKILL.md\nold bytes\n</skill-recall>";
        let parsed = previous_entries(legacy).unwrap();
        assert_eq!(parsed[0].name, "known");
        assert!(parsed[0].content.as_ref().unwrap().contains("old bytes"));
        assert!(
            previous_entries("<skill-recall>{\"version\":2,\"entries\":[]}</skill-recall>")
                .is_err()
        );
        assert!(previous_entries("<skill-recall>opaque unknown material</skill-recall>").is_err());
        assert!(previous_entries("<skill-recall>unfinished").is_err());
        let budget = 120;
        let text = "中🙂文".repeat(1000);
        let result = clipped(&text, budget);
        assert!(result.chars().count() <= budget);
        assert!(result.contains("truncated"));
    }
    #[cfg(unix)]
    #[test]
    fn recall_source_links_do_not_read_the_neighbor_payload() {
        let root = std::env::temp_dir().join(format!(
            "lingxi-recall-link-{}",
            lingxi_adapters::storage::agent_memory::random_token_suffix()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let secret = root.join("neighbor.txt");
        std::fs::write(&secret, "NEIGHBOR_PAYLOAD_MUST_NOT_READ").unwrap();
        let source = root.join("SKILL.md");
        std::os::unix::fs::symlink(&secret, &source).unwrap();
        assert!(selected_file(&source, 1024).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
