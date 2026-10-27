//! R08-T04: 69 类存储注册表分类（静态表 + 匹配器）。
//!
//! 静态表由 R00 STORES.json / R01 DATA_COMPATIBILITY_MATRIX.json 生成（见下方
//! GENERATED 注释），路径模式语义：`{name}` 匹配单段非斜杠字符序列，`**`
//! 匹配任意深度后缀，裸 `*` 匹配单段内任意字符。分类规则：先按模式精确匹配
//! （含「模式 + `/**`」的目录前缀展开），多命中时取**模式串更长者**（更具体
//! 者优先，如 `plugin-data/office/jobs` 胜过 `plugin-data/{pluginId}`），
//! 再按类 id 字典序定胜负——两次扫描同一源根必得同一分类。

use regex::Regex;
use std::sync::OnceLock;

/// 切换策略（R01 cutover_strategy 的 Rust 侧命名）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    /// switch-to-new-authority：本轮实转（转换副本进新隔离根）。
    Convert,
    /// preserve-format：原样登记 + 哈希 + 抽查校验，不转写。
    Register,
    /// regenerable：登记 + 丢弃收据，不复制。
    Regenerable,
    /// epoch-mechanism：T05 域，out-of-scope。
    OutOfScopeEpoch,
    /// shell-local：壳本地不迁移，out-of-scope。
    OutOfScopeShell,
}

impl Strategy {
    pub fn receipt_name(self) -> &'static str {
        match self {
            Strategy::Convert => "convert",
            Strategy::Register => "register",
            Strategy::Regenerable => "regenerable",
            Strategy::OutOfScopeEpoch | Strategy::OutOfScopeShell => "out-of-scope",
        }
    }
}

#[derive(Debug)]
pub struct ClassDef {
    pub id: &'static str,
    pub strategy: Strategy,
    /// 任一 pattern 以 `(os-tmpdir)` 开头：该类的实体在源根之外，永不参与扫描分类。
    pub external: bool,
    pub patterns: &'static [&'static str],
}

/// 把注册表路径模式编译为正则：`{...}`→`[^/]+`，`**`→`.*`，`*`→`[^/]*`。
fn pattern_to_regex(pattern: &str) -> Regex {
    let mut out = String::from("^");
    let bytes = pattern.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c == '{' {
            let end = pattern[i..].find('}').expect("registry pattern: unbalanced brace");
            i += end + 1;
            out.push_str("[^/]+");
            continue;
        }
        if c == '*' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'*' {
                out.push_str(".*");
                i += 2;
            } else {
                out.push_str("[^/]*");
                i += 1;
            }
            continue;
        }
        if ".+()[]^$|?\\".contains(c) {
            out.push('\\');
        }
        out.push(c);
        i += 1;
    }
    out.push('$');
    Regex::new(&out).expect("registry pattern must compile")
}

struct CompiledClass {
    def: &'static ClassDef,
    /// (pattern, exact-regex, dir-prefix-regex)
    matchers: Vec<(&'static str, Regex, Regex)>,
}

fn compiled() -> &'static [CompiledClass] {
    static COMPILED: OnceLock<Vec<CompiledClass>> = OnceLock::new();
    COMPILED.get_or_init(|| {
        CLASSES
            .iter()
            .filter(|def| !def.external)
            .map(|def| {
                let matchers = def
                    .patterns
                    .iter()
                    .filter(|p| !p.starts_with("(os-tmpdir)"))
                    .map(|p| {
                        let exact = pattern_to_regex(p);
                        // 目录语义：pattern 本身亦可作前缀目录（如 session-files/{sessionHash}）。
                        let dir = if p.ends_with("/**") {
                            pattern_to_regex(&p[..p.len() - 3])
                        } else {
                            pattern_to_regex(&format!("{p}/**"))
                        };
                        (*p, exact, dir)
                    })
                    .collect();
                CompiledClass { def, matchers }
            })
            .collect()
    })
}

/// 模式中的字面字符数（`{...}` 占位符整体不计）。
fn literal_chars(pattern: &str) -> usize {
    let mut count = 0usize;
    let mut in_placeholder = false;
    for ch in pattern.chars() {
        match ch {
            '{' => in_placeholder = true,
            '}' => in_placeholder = false,
            _ if !in_placeholder => count += 1,
            _ => {}
        }
    }
    count
}

/// 把源根相对路径分类到 (类, 策略)。未命中任何类 → None（调用方记 unmapped-class）。
pub fn classify(relpath: &str) -> Option<(&'static ClassDef, &'static str)> {
    let mut best: Option<(&'static ClassDef, &'static str, usize)> = None;
    for cc in compiled() {
        for (pattern, exact, dir) in &cc.matchers {
            if exact.is_match(relpath) || dir.is_match(relpath) {
                // 具体度 = 字面字符数（占位符内文本不计）：plugin-data/mcp(15)
                // 胜过 plugin-data/{pluginId}(12)，与模式串长无关。
                let score = literal_chars(pattern);
                let better = match best {
                    None => true,
                    Some((_, _, best_score)) => score > best_score,
                };
                if better {
                    best = Some((cc.def, pattern, score));
                }
            }
        }
    }
    best.map(|(def, pattern, _)| (def, pattern))
}

// GENERATED from docs/rust-tauri/R00/STORES.json (sha256 6067bdd3…) +
// docs/rust-tauri/R01/DATA_COMPATIBILITY_MATRIX.json (sha256 458bb2da…).
// Regenerate by re-running the extraction; do not hand-edit entries.
// 例外增补（R08-T06 2026-10-23，Lead 裁决②）：provider-state 追加 pattern
// `models-store.json` —— 三方 SDK @earendil-works/pi-coding-agent 的
// FileModelsStore 经 ModelRuntime.create({modelsPath: <home>/models.json})
// 写到数据根顶层（pi-coding-agent model-runtime.js:80 取 dirname(modelsPath)），
// 生产冷启动同写；R00/R01 审计未列（审计漏网），不修则 T07 真实旧根导入
// 必撞 Unclassified 响亮拒。重跑提取时必须保留该 pattern（或先行将其
// 登记进上游注册表）。R00/R01 原文件不动（保 STORAGE_READINESS 输入摘要）。
pub static CLASSES: &[ClassDef] = &[
    ClassDef { id: "agent-authored-records", strategy: Strategy::Register, external: false, patterns: &["user/user.md", "agents/{agentId}/dm/{peerId}.md", "agents/{agentId}/experience.md", "agents/{agentId}/experience/{category}.md", "agents/{agentId}/diary/{date}.md"] },
    ClassDef { id: "agent-facts-sqlite", strategy: Strategy::Register, external: false, patterns: &["agents/{agentId}/memory/facts.db"] },
    ClassDef { id: "agent-memory", strategy: Strategy::Register, external: false, patterns: &["agents/{agentId}/memory/memory.md", "agents/{agentId}/memory/navigation.md", "agents/{agentId}/memory/tenets.json", "agents/{agentId}/memory/pinned-tenets-migration.receipt.json", "agents/{agentId}/memory/pinned-recovery-operations/{operationId}.json", "agents/{agentId}/memory/pinned-migration-backup/{backupFile}", "agents/{agentId}/memory/pinned-migration-backup/{operationId}/{backupFile}", "agents/{agentId}/memory/facts.md", "agents/{agentId}/memory/today.md", "agents/{agentId}/memory/week.md", "agents/{agentId}/memory/longterm.md", "agents/{agentId}/memory/editable-facts-state.json", "agents/{agentId}/memory/today-state.json", "agents/{agentId}/memory/daily-state.json", "agents/{agentId}/memory/summaries/{sessionId}.json", "agents/{agentId}/memory/daily/{date}.md", "agents/{agentId}/memory/dream/state.json", "agents/{agentId}/memory/dream/pending-apply.json", "agents/{agentId}/memory/dream/revisions/{revisionId}.json", "agents/{agentId}/memory/memories.db", "agents/{agentId}/memory/.v2-migrated"] },
    ClassDef { id: "agent-phone", strategy: Strategy::Register, external: false, patterns: &["agents/{agentId}/phone/conversations/{conversationId}.md", "agents/{agentId}/phone/session-runtime/{conversationId}.json"] },
    ClassDef { id: "agent-profile", strategy: Strategy::Register, external: false, patterns: &["agents/{agentId}/config.yaml", "agents/{agentId}/identity.md", "agents/{agentId}/AGENTS.md", "agents/{agentId}/AGENTS.public.md", "agents/{agentId}/appearance-summary.json", "agents/{agentId}/avatars/{fileName}", "agents/{agentId}/channels.md", "user/avatars/{fileName}"] },
    ClassDef { id: "channels", strategy: Strategy::Register, external: false, patterns: &["channels/{channelId}.yaml"] },
    ClassDef { id: "character-card-staging", strategy: Strategy::Regenerable, external: false, patterns: &[".ephemeral/character-card-uploads/{fileName}", ".ephemeral/character-card-imports/{token}", ".ephemeral/character-card-exports/{token}"] },
    ClassDef { id: "conversation-map-layout", strategy: Strategy::Convert, external: false, patterns: &["conversation-map.json"] },
    ClassDef { id: "cron-automation", strategy: Strategy::Register, external: false, patterns: &["agents/{agentId}/desk/cron-jobs.json", "agents/{agentId}/desk/cron-runs/{runId}.json", "studios/{studioId}/desk/cron-jobs.json", "studios/{studioId}/desk/cron-runs/{runId}.json"] },
    ClassDef { id: "data-epoch-checkpoints", strategy: Strategy::OutOfScopeEpoch, external: false, patterns: &["data-epoch-checkpoints/{transitionId}/**"] },
    ClassDef { id: "data-epoch-restore-quarantine", strategy: Strategy::OutOfScopeEpoch, external: false, patterns: &["data-epoch-restore-quarantine/{restoreId}/**", "data-epoch-restores.log"] },
    ClassDef { id: "data-epoch-stamp", strategy: Strategy::OutOfScopeEpoch, external: false, patterns: &["data-epoch.json"] },
    ClassDef { id: "data-epoch-transition-journal", strategy: Strategy::OutOfScopeEpoch, external: false, patterns: &["data-epoch-transition.json"] },
    ClassDef { id: "deferred-result-state", strategy: Strategy::Convert, external: false, patterns: &[".ephemeral/deferred-tasks.json"] },
    ClassDef { id: "desk-activity", strategy: Strategy::Register, external: false, patterns: &["agents/{agentId}/desk/activities.json"] },
    ClassDef { id: "desk-cover-upload-staging", strategy: Strategy::Regenerable, external: false, patterns: &["tmp/markdown-cover-uploads/{token}"] },
    ClassDef { id: "desktop-diagnostics", strategy: Strategy::OutOfScopeShell, external: false, patterns: &["diagnostics/desktop-launch", "crash.log", "browser-cmd.log"] },
    ClassDef { id: "desktop-gpu-startup-state", strategy: Strategy::OutOfScopeShell, external: false, patterns: &["user/gpu-startup.json"] },
    ClassDef { id: "desktop-update-channel", strategy: Strategy::OutOfScopeShell, external: false, patterns: &["update-channel.json"] },
    ClassDef { id: "desktop-win32-install-acl-heal-state", strategy: Strategy::OutOfScopeShell, external: false, patterns: &["user/win32-install-acl-heal.json"] },
    ClassDef { id: "desktop-window-version-state", strategy: Strategy::OutOfScopeShell, external: false, patterns: &["user/window-state.json", "user/quick-chat-window-state.json", "user/last-seen-version.json", "last-update-version"] },
    ClassDef { id: "device-access-registries", strategy: Strategy::Register, external: false, patterns: &["devices.json", "device-credentials.json", "pairing-sessions.json"] },
    ClassDef { id: "ephemeral-scanner-scratch", strategy: Strategy::Regenerable, external: true, patterns: &["(os-tmpdir)/lingxi-*"] },
    ClassDef { id: "execution-leases", strategy: Strategy::Convert, external: false, patterns: &["security/execution-leases.json"] },
    ClassDef { id: "file-history-sqlite", strategy: Strategy::Register, external: false, patterns: &["file-history/{workspaceHash}/history.sqlite", "file-history/{workspaceHash}/history.sqlite-wal", "file-history/{workspaceHash}/history.sqlite-shm"] },
    ClassDef { id: "knowledge-database", strategy: Strategy::Register, external: false, patterns: &["knowledge/knowledge.db", "knowledge/knowledge.db-wal", "knowledge/knowledge.db-shm"] },
    ClassDef { id: "knowledge-indexes", strategy: Strategy::Regenerable, external: false, patterns: &["knowledge/indexes/**"] },
    ClassDef { id: "knowledge-parse-artifacts", strategy: Strategy::Register, external: false, patterns: &["knowledge/artifacts/**"] },
    ClassDef { id: "knowledge-processing-artifacts", strategy: Strategy::Register, external: false, patterns: &["knowledge/processed/**"] },
    ClassDef { id: "knowledge-source-snapshots", strategy: Strategy::Register, external: false, patterns: &["knowledge/sources/**"] },
    ClassDef { id: "legacy-upload-cache", strategy: Strategy::Regenerable, external: false, patterns: &["uploads/{uploadName}"] },
    ClassDef { id: "local-user-auth", strategy: Strategy::Register, external: false, patterns: &["local-user-auth.json"] },
    ClassDef { id: "loop-state", strategy: Strategy::Convert, external: false, patterns: &[".ephemeral/loop-state.json"] },
    ClassDef { id: "managed-runtime-caches", strategy: Strategy::Regenerable, external: false, patterns: &["runtime/pi-sdk/bin/{toolName}", ".ephemeral/win32-sandbox-runtime/{fingerprint}"] },
    ClassDef { id: "mcp-config", strategy: Strategy::Register, external: false, patterns: &["plugin-data/mcp"] },
    ClassDef { id: "model-observability-blobs", strategy: Strategy::Register, external: false, patterns: &["model-observability/blobs/**"] },
    ClassDef { id: "model-observability-db", strategy: Strategy::Register, external: false, patterns: &["model-observability/observability.sqlite", "model-observability/observability.sqlite-wal", "model-observability/observability.sqlite-shm"] },
    ClassDef { id: "office-render-jobs", strategy: Strategy::Regenerable, external: false, patterns: &["plugin-data/office/jobs", "plugin-data/office/generated"] },
    ClassDef { id: "operational-checkpoints", strategy: Strategy::Register, external: false, patterns: &["checkpoints/{checkpointId}"] },
    ClassDef { id: "plugin-runtime-data", strategy: Strategy::Register, external: false, patterns: &["plugin-data/{pluginId}"] },
    ClassDef { id: "plugin-task-registry", strategy: Strategy::Convert, external: false, patterns: &[".ephemeral/plugin-tasks.json"] },
    ClassDef { id: "provider-state", strategy: Strategy::Register, external: false, patterns: &["provider-catalog.json", "added-models.yaml", "models.json", "models-store.json", "auth.json", "provider-plugins/{storageId}/manifest.json", "provider-plugins/{storageId}/providers/{storageId}.json"] },
    ClassDef { id: "runtime-diagnostics", strategy: Strategy::Register, external: false, patterns: &["logs/{timestamp}.log", "browser-worker.log", "switch-error.log", "user/browser-sessions.json", "bridge/wechat/sync-{accountHash}.json", "bridge/wechat/context-{accountHash}.json"] },
    ClassDef { id: "security-audit-log", strategy: Strategy::Register, external: false, patterns: &["logs/security-audit.jsonl"] },
    ClassDef { id: "security-grants", strategy: Strategy::Register, external: false, patterns: &["security/grants.json", "security/grants.json.corrupt-{timestamp}", "security/grants.json.corrupt-{timestamp}.reason.txt"] },
    ClassDef { id: "security-key-material", strategy: Strategy::Register, external: false, patterns: &["security/resource-ticket-key"] },
    ClassDef { id: "server-network-config", strategy: Strategy::Register, external: false, patterns: &["server-network.json"] },
    ClassDef { id: "server-node-identity", strategy: Strategy::Register, external: false, patterns: &["server-node.json"] },
    ClassDef { id: "server-runtime-info", strategy: Strategy::Regenerable, external: false, patterns: &["server-info.json"] },
    ClassDef { id: "session-checkpoints", strategy: Strategy::Regenerable, external: false, patterns: &["agents/{agentId}/sessions/{sessionId}.jsonl.checkpoints.json", "agents/{agentId}/sessions/archived/{sessionId}.jsonl.checkpoints.json"] },
    ClassDef { id: "session-context-notes", strategy: Strategy::Regenerable, external: false, patterns: &["agents/{agentId}/sessions/{sessionId}.jsonl.context-notes.md", "agents/{agentId}/sessions/archived/{sessionId}.jsonl.context-notes.md"] },
    ClassDef { id: "session-drafts-and-projects", strategy: Strategy::Register, external: false, patterns: &["input-drafts.v1.json", "user/session-projects.json"] },
    ClassDef { id: "session-files", strategy: Strategy::Register, external: false, patterns: &["session-files/{sessionHash}"] },
    ClassDef { id: "session-goal", strategy: Strategy::Regenerable, external: false, patterns: &["agents/{agentId}/sessions/{sessionId}.jsonl.goal.json", "agents/{agentId}/sessions/archived/{sessionId}.jsonl.goal.json"] },
    ClassDef { id: "session-jsonl", strategy: Strategy::Convert, external: false, patterns: &["agents/{agentId}/sessions/{sessionId}.jsonl", "agents/{agentId}/sessions/archived/{sessionId}.jsonl", "agents/{agentId}/activity/{sessionId}.jsonl", "agents/{agentId}/subagent-sessions/{sessionId}.jsonl", "agents/{agentId}/.ephemeral/{sessionId}.jsonl"] },
    ClassDef { id: "session-manifest-sqlite", strategy: Strategy::Convert, external: false, patterns: &["session-manifest.db", "session-manifest.db-wal", "session-manifest.db-shm"] },
    ClassDef { id: "session-sidecars", strategy: Strategy::Convert, external: false, patterns: &["agents/{agentId}/sessions/session-meta.json", "agents/{agentId}/sessions/session-titles.json", "agents/{agentId}/sessions/bridge/bridge-sessions.json", "agents/{agentId}/sessions/session-vision-notes.json", "agents/{agentId}/subagent-sessions/session-meta.json"] },
    ClassDef { id: "signed-artifacts", strategy: Strategy::OutOfScopeShell, external: false, patterns: &["artifacts"] },
    ClassDef { id: "skill-state", strategy: Strategy::Register, external: false, patterns: &["skills/{skillName}", "skill-bundles.json", ".ephemeral/skill-bundle-exports/{token}"] },
    ClassDef { id: "skill-translation-cache", strategy: Strategy::Regenerable, external: false, patterns: &[".ephemeral/skill-name-translations.json"] },
    ClassDef { id: "studio-mount-registry", strategy: Strategy::Register, external: false, patterns: &["studio-mounts.json"] },
    ClassDef { id: "subagent-state", strategy: Strategy::Convert, external: false, patterns: &["subagent-runs.json", "subagent-threads.json"] },
    ClassDef { id: "terminal-session-state", strategy: Strategy::Convert, external: false, patterns: &[".ephemeral/terminal-sessions/{terminalId}.json", ".ephemeral/terminal-sessions/{terminalId}.jsonl"] },
    ClassDef { id: "usage-ledger", strategy: Strategy::Register, external: false, patterns: &["usage-ledger.json"] },
    ClassDef { id: "user-preferences", strategy: Strategy::Register, external: false, patterns: &["user/preferences.json", "user/preferences.json.corrupt-{timestamp}"] },
    ClassDef { id: "user-studio-registries", strategy: Strategy::Register, external: false, patterns: &["users.json", "studios.json"] },
    ClassDef { id: "web-session-registry", strategy: Strategy::Register, external: false, patterns: &["web-sessions.json"] },
    ClassDef { id: "workflow-state", strategy: Strategy::Convert, external: false, patterns: &["agents/{agentId}/workflow-journals/{runId}.jsonl", "agents/{agentId}/workflow-sessions/{runId}", "workflow-activity.json"] },
    ClassDef { id: "workspace-snapshots", strategy: Strategy::Regenerable, external: false, patterns: &["workspace-snapshots/{workspaceHash}/**", "agents/{agentId}/sessions/{sessionId}.jsonl.snapshots.json", "agents/{agentId}/sessions/archived/{sessionId}.jsonl.snapshots.json", "agents/{agentId}/activity/{sessionId}.jsonl.snapshots.json", "agents/{agentId}/subagent-sessions/{sessionId}.jsonl.snapshots.json", "agents/{agentId}/.ephemeral/{sessionId}.jsonl.snapshots.json"] },
];
// class count: 69

#[cfg(test)]
mod tests {
    use super::*;

    fn strategy_of(relpath: &str) -> Option<(&'static str, &'static str)> {
        classify(relpath).map(|(def, _)| (def.id, def.strategy.receipt_name()))
    }

    #[test]
    fn fixture_representative_paths_classify() {
        let cases: &[(&str, &str, &str)] = &[
            ("agents/a1/sessions/s1.jsonl", "session-jsonl", "convert"),
            ("agents/a1/sessions/archived/s9.jsonl", "session-jsonl", "convert"),
            ("agents/a2/subagent-sessions/sub1.jsonl", "session-jsonl", "convert"),
            ("session-manifest.db", "session-manifest-sqlite", "convert"),
            ("agents/a1/sessions/session-meta.json", "session-sidecars", "convert"),
            ("model-observability/observability.sqlite", "model-observability-db", "register"),
            ("knowledge/knowledge.db", "knowledge-database", "register"),
            ("knowledge/indexes/fts/x.bin", "knowledge-indexes", "regenerable"),
            ("knowledge/artifacts/doc1/x.md", "knowledge-parse-artifacts", "register"),
            ("security/execution-leases.json", "execution-leases", "convert"),
            ("agents/a1/workflow-journals/run1.jsonl", "workflow-state", "convert"),
            ("workflow-activity.json", "workflow-state", "convert"),
            ("subagent-runs.json", "subagent-state", "convert"),
            (".ephemeral/terminal-sessions/t1.json", "terminal-session-state", "convert"),
            (".ephemeral/deferred-tasks.json", "deferred-result-state", "convert"),
            (".ephemeral/loop-state.json", "loop-state", "convert"),
            (".ephemeral/plugin-tasks.json", "plugin-task-registry", "convert"),
            ("conversation-map.json", "conversation-map-layout", "convert"),
            ("data-epoch.json", "data-epoch-stamp", "out-of-scope"),
            ("user/window-state.json", "desktop-window-version-state", "out-of-scope"),
            ("agents/a1/sessions/s1.jsonl.goal.json", "session-goal", "regenerable"),
            (
                "agents/a1/sessions/s1.jsonl.checkpoints.json",
                "session-checkpoints",
                "regenerable",
            ),
            (
                "agents/a1/sessions/s1.jsonl.snapshots.json",
                "workspace-snapshots",
                "regenerable",
            ),
        ];
        for (relpath, want_class, want_strategy) in cases {
            let got = strategy_of(relpath);
            assert_eq!(
                got,
                Some((*want_class, *want_strategy)),
                "relpath {relpath} classified as {got:?}, want {want_class}/{want_strategy}"
            );
        }
    }

    #[test]
    fn most_specific_pattern_wins_on_overlap() {
        // plugin-data/office/jobs/... 必须归 office-render-jobs 而非 plugin-runtime-data。
        let got = strategy_of("plugin-data/office/jobs/job1/req.json");
        assert_eq!(got, Some(("office-render-jobs", "regenerable")), "got {got:?}");
        let got = strategy_of("plugin-data/some-plugin/state.json");
        assert_eq!(got, Some(("plugin-runtime-data", "register")), "got {got:?}");
        let got = strategy_of("plugin-data/mcp/servers.json");
        assert_eq!(got, Some(("mcp-config", "register")), "got {got:?}");
    }

    #[test]
    fn session_files_directory_prefix_matches() {
        let got = strategy_of("session-files/ab/abcdef123");
        assert_eq!(got, Some(("session-files", "register")), "got {got:?}");
    }

    #[test]
    fn unmapped_returns_none() {
        assert!(classify("totally/unknown/file.bin").is_none());
        assert!(classify("node_modules/x/index.js").is_none());
    }

    // R08-T06 裁决②增补：三方 SDK（pi-coding-agent FileModelsStore）在数据根
    // 顶层写 models-store.json，与 models.json 同目录；归 provider-state(Register)。
    #[test]
    fn models_store_json_amendment_classifies_provider_state() {
        assert_eq!(
            strategy_of("models-store.json"),
            Some(("provider-state", "register"))
        );
        // 增补不得改变既有兄弟路径的分类。
        assert_eq!(strategy_of("models.json"), Some(("provider-state", "register")));
        assert_eq!(strategy_of("auth.json"), Some(("provider-state", "register")));
    }

    #[test]
    fn table_has_69_classes_and_unique_ids() {
        assert_eq!(CLASSES.len(), 69);
        let mut ids: Vec<&str> = CLASSES.iter().map(|c| c.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 69);
    }
}
