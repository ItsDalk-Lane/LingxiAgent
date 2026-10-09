//! R06-T03 会话树纯逻辑（无副作用、可单测）：谱系深度闸、fork 保留段规划、
//! 具名检查点策略、文件恢复冲突判定、内容哈希。
//!
//! 现役锚点（对照式语义来源）：
//! - 谱系深度上限：`server/routes/sessions.ts:1738` `MAX_FORK_LINEAGE_DEPTH = 2`
//!   （主对话=0，派生最多 2 层；超限显式拒绝，不静默改挂父级）。
//! - fork 复制 root→boundary 保留段：`core/session-coordinator.ts:3639-3641`
//!   `retainedEntries = sourceBranch.slice(0, boundaryIndex + 1)`（含边界，不物理截断源）。
//! - 具名检查点：`core/session-checkpoints.ts:14-16`（保留名 `latest` 覆盖、
//!   上限 `SESSION_CHECKPOINT_MAX_RECORDS = 200`）、`:74-82`（非 latest 重名 → 抛错）。
//! - 文件恢复冲突判定是对现役缺口的有意加固（报告差异 D1）：现役
//!   `core/workspace-snapshots.ts:584-620`/`lib/checkpoint-store.ts:94-108` 盲写回、
//!   不做外部修改检测；R06-A06 要求检测冲突并保护用户修改。

/// 现役谱系深度上限（sessions.ts:1738）：主=0，最多再派生 2 层。
pub const MAX_FORK_LINEAGE_DEPTH: u32 = 2;

/// 现役具名检查点保留名（session-checkpoints.ts:14）：重复创建覆盖。
pub const CHECKPOINT_LATEST_NAME: &str = "latest";

/// 现役具名检查点记录上限（session-checkpoints.ts:16）。
pub const SESSION_CHECKPOINT_MAX_RECORDS: usize = 200;

/// fork 深度闸：来源会话已在 [`MAX_FORK_LINEAGE_DEPTH`] 层时，再派生即拒绝。
/// 返回 `None` 由调用方映射为响亮拒绝（不静默改挂父级，sessions.ts:1795-1801）。
pub fn check_fork_depth(source_lineage_depth: u32) -> Option<u32> {
    let child_depth = source_lineage_depth.saturating_add(1);
    if source_lineage_depth >= MAX_FORK_LINEAGE_DEPTH {
        return None;
    }
    Some(child_depth)
}

/// fork 保留段规划：给定分支消息 id 链（root→…→leaf 有序）与分叉边界 id，
/// 返回 root→boundary（含边界）的保留段子序列。边界不在链上 → `None`
/// （调用方映射为 400 fork target invalid，session-turn-actions.ts resolveSessionNodeTarget）。
pub fn plan_fork_retained_ids<I, S>(branch_ids: I, boundary_id: &str) -> Option<Vec<String>>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let ids: Vec<String> = branch_ids.into_iter().map(Into::into).collect();
    let pos = ids.iter().position(|id| id == boundary_id)?;
    Some(ids[..=pos].to_vec())
}

/// 具名检查点 upsert 决策。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckpointUpsert {
    /// 写入新记录。
    Insert,
    /// 覆盖既有记录（仅 `latest`）。
    Overwrite,
    /// 重名冲突：拒绝（非 latest）。
    Conflict,
}

/// 具名检查点 upsert 策略：`latest` 覆盖既有 latest；非 latest 重名 → 冲突拒绝。
pub fn decide_checkpoint_upsert(name: &str, exists: bool) -> CheckpointUpsert {
    if !exists {
        return CheckpointUpsert::Insert;
    }
    if name == CHECKPOINT_LATEST_NAME {
        CheckpointUpsert::Overwrite
    } else {
        CheckpointUpsert::Conflict
    }
}

/// 窗口化裁减：超出上限时，裁掉最老的非 `latest` 记录（session-checkpoints.ts:96-101）。
/// `names_oldest_first` 为按创建时间升序的名字。返回应删除的名字（可能为空）。
pub fn plan_checkpoint_eviction(names_oldest_first: &[String]) -> Vec<String> {
    if names_oldest_first.len() <= SESSION_CHECKPOINT_MAX_RECORDS {
        return Vec::new();
    }
    let excess = names_oldest_first.len() - SESSION_CHECKPOINT_MAX_RECORDS;
    let mut out = Vec::new();
    for name in names_oldest_first {
        if out.len() >= excess {
            break;
        }
        if name != CHECKPOINT_LATEST_NAME {
            out.push(name.clone());
        }
    }
    out
}

/// 文件恢复冲突判定（A06 加固 + REPAIR-R1 内容级恢复）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileRestoreVerdict {
    /// 当前文件与目标检查点记录版本一致 → 无需写回（收据标 skipped）。
    Skipped,
    /// 当前内容不同于目标检查点，但属于本会话任一检查点见证过的版本
    /// （检查点之后的改动被系统后续检查点记录过，如模型自己的后续回合）
    /// → 安全写回存档内容（收据标 restored）。
    Restore,
    /// 当前内容是系统从未见证过的版本（外部修改），或文件已被删除
    /// → 拒绝覆盖该文件，保护用户修改（收据标 conflicted；D1 加固保留）。
    Conflicted,
}

/// 判定单个文件在内容级回滚中的结局。`recorded_sha256` 是目标检查点记录的
/// 版本哈希，`current_sha256` 是恢复前实时读到的当前内容哈希，
/// `witnessed_sha256` 是该文件在本会话全部检查点中被记录过的所有哈希
/// （含目标检查点自身那条）。判定序：已是目标态 → skipped；当前态被系统
/// 见证过 → restore（写回存档内容）；从未见过 → conflicted（绝不盲写，
/// 对照现役 `workspace-snapshots.ts:584-620` 盲写缺口的 D1 加固）。
pub fn judge_file_restore(
    _file_path: &str,
    recorded_sha256: &str,
    current_sha256: &str,
    witnessed_sha256: &[String],
) -> FileRestoreVerdict {
    if recorded_sha256 == current_sha256 {
        FileRestoreVerdict::Skipped
    } else if witnessed_sha256.iter().any(|s| s == current_sha256) {
        FileRestoreVerdict::Restore
    } else {
        FileRestoreVerdict::Conflicted
    }
}

/// 分支上的有序节点（root→…→head，仅真实 message 条目）。
///  retry/rewind 目标解析的纯逻辑输入。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BranchNode<'a> {
    pub message_id: &'a str,
    pub parent_message_id: Option<&'a str>,
    pub role: &'a str,
}

/// retry/rewind 服务端解析出的分支重置点（对照现役
/// `core/session-turn-actions.ts:78-165 resolveSessionNodeTarget` mode=retry）：
/// `new_head_message_id` = 目标回合输入的 parent（现役 retryBranchParentId；
/// 根回合 → `None`），`turn_input_message_id` = 该回合的用户输入消息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResetPoint {
    pub turn_input_message_id: String,
    pub new_head_message_id: Option<String>,
}

/// 目标解析的响亮失败（调用方映射为 400；绝不静默猜一个目标）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveTargetError {
    /// 目标不在当前分支（现役 "Requested session node is not on the active branch"）。
    NotOnBranch,
    /// 目标是 assistant 消息，但其前方没有任何 user 回合输入
    /// （现役 "Assistant node has no preceding turn input on the active branch"）。
    NoPrecedingUserTurn,
    /// latest-user 形态下当前分支没有任何 user 消息
    /// （现役 "No latest user message to replay"）。
    NoUserTurn,
}

/// 解析 retry/rewind 的分支重置点。`target_message_id`：
/// - `Some(user 消息)`：该回合即目标回合；
/// - `Some(assistant 消息)`：目标回合 = 其前方最近的 user 回合
///   （现役 findPrecedingTurnInputIndex）；
/// - `None`：latest-user 兼容形态（现役 replayLatestUserTurn /
///   `session-turn-actions.ts:346-356` 的 latestUserOnly）——最近一条 user 消息。
pub fn resolve_retry_reset_point(
    branch: &[BranchNode<'_>],
    target_message_id: Option<&str>,
) -> Result<ResetPoint, ResolveTargetError> {
    let turn_input_index = match target_message_id {
        Some(target) => {
            let index = branch
                .iter()
                .position(|node| node.message_id == target)
                .ok_or(ResolveTargetError::NotOnBranch)?;
            if branch[index].role == "user" {
                index
            } else {
                // assistant 目标 → 前方最近的 user 回合输入。
                branch[..index]
                    .iter()
                    .rposition(|node| node.role == "user")
                    .ok_or(ResolveTargetError::NoPrecedingUserTurn)?
            }
        }
        None => branch
            .iter()
            .rposition(|node| node.role == "user")
            .ok_or(ResolveTargetError::NoUserTurn)?,
    };
    let turn_input = branch[turn_input_index];
    Ok(ResetPoint {
        turn_input_message_id: turn_input.message_id.to_string(),
        new_head_message_id: turn_input.parent_message_id.map(str::to_string),
    })
}

/// 授权目录归属判定（组件级，不是字符串前缀——`/work` 不吞 `/work2`）。
/// 含 `..` 组件的路径直接判否（`Path::starts_with` 不解析 `..`，
/// 不在这里挡住就会逃出授权根）；符号链接由调用方（服务层）先
/// canonicalize 两侧再传入——纯函数不做 IO。
pub fn path_within_folders(file: &std::path::Path, folders: &[String]) -> bool {
    use std::path::Component;
    if file.components().any(|c| matches!(c, Component::ParentDir)) {
        return false;
    }
    folders
        .iter()
        .any(|folder| file.starts_with(std::path::Path::new(folder)))
}

/// 内容哈希（SHA-256，小写 hex）。纯函数，无 IO——输入由调用方读取。
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fork_depth_allows_two_layers_and_rejects_third() {
        assert_eq!(check_fork_depth(0), Some(1));
        assert_eq!(check_fork_depth(1), Some(2));
        assert_eq!(check_fork_depth(2), None);
        assert_eq!(check_fork_depth(99), None);
    }

    #[test]
    fn fork_retained_is_root_to_boundary_inclusive() {
        let ids = vec!["m1", "m2", "m3"];
        assert_eq!(
            plan_fork_retained_ids(ids.clone(), "m2"),
            Some(vec!["m1".to_string(), "m2".to_string()])
        );
        assert_eq!(
            plan_fork_retained_ids(ids.clone(), "m3"),
            Some(vec!["m1".to_string(), "m2".to_string(), "m3".to_string()])
        );
        assert_eq!(plan_fork_retained_ids(ids, "nope"), None);
    }

    #[test]
    fn checkpoint_latest_overwrites_others_conflict() {
        assert_eq!(
            decide_checkpoint_upsert("latest", false),
            CheckpointUpsert::Insert
        );
        assert_eq!(
            decide_checkpoint_upsert("latest", true),
            CheckpointUpsert::Overwrite
        );
        assert_eq!(
            decide_checkpoint_upsert("v1", false),
            CheckpointUpsert::Insert
        );
        assert_eq!(
            decide_checkpoint_upsert("v1", true),
            CheckpointUpsert::Conflict
        );
    }

    #[test]
    fn checkpoint_eviction_drops_oldest_non_latest() {
        let names: Vec<String> = (0..201).map(|i| format!("c{i}")).collect();
        let evict = plan_checkpoint_eviction(&names);
        assert_eq!(evict, vec!["c0".to_string()]);
        // latest 永不裁。
        let mut with_latest: Vec<String> = (0..201).map(|i| format!("c{i}")).collect();
        with_latest[0] = CHECKPOINT_LATEST_NAME.to_string();
        let evict2 = plan_checkpoint_eviction(&with_latest);
        assert_eq!(evict2, vec!["c1".to_string()]);
        // 未超限不裁。
        let small: Vec<String> = (0..10).map(|i| format!("c{i}")).collect();
        assert!(plan_checkpoint_eviction(&small).is_empty());
    }

    #[test]
    fn file_restore_conflict_detection() {
        let witnessed = vec!["x".to_string(), "y".to_string()];
        // 当前 == 目标 → skipped（无需写回）。
        assert_eq!(
            judge_file_restore("a", "x", "x", &witnessed),
            FileRestoreVerdict::Skipped
        );
        // 当前 != 目标但被系统见证过（模型后续回合改过并被检查点记录）→ restore。
        assert_eq!(
            judge_file_restore("a", "x", "y", &witnessed),
            FileRestoreVerdict::Restore
        );
        // 当前是系统从未见过的版本（外部修改）→ conflicted（绝不覆盖）。
        assert_eq!(
            judge_file_restore("a", "x", "z", &witnessed),
            FileRestoreVerdict::Conflicted
        );
        // 见证集为空时任何分歧都判冲突。
        assert_eq!(
            judge_file_restore("a", "x", "y", &[]),
            FileRestoreVerdict::Conflicted
        );
    }

    #[test]
    fn sha256_hex_is_64_lower_hex() {
        let h = sha256_hex(b"abc");
        assert_eq!(h.len(), 64);
        assert!(h
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        // 已知值：sha256("abc")
        assert_eq!(
            h,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn retry_reset_point_resolves_user_assistant_and_latest_user() {
        let branch = [
            BranchNode {
                message_id: "u1",
                parent_message_id: None,
                role: "user",
            },
            BranchNode {
                message_id: "a1",
                parent_message_id: Some("u1"),
                role: "assistant",
            },
            BranchNode {
                message_id: "u2",
                parent_message_id: Some("a1"),
                role: "user",
            },
            BranchNode {
                message_id: "a2",
                parent_message_id: Some("u2"),
                role: "assistant",
            },
        ];
        // user 目标：重置点 = 该回合输入，新头 = 输入的 parent。
        let point = resolve_retry_reset_point(&branch, Some("u2")).unwrap();
        assert_eq!(point.turn_input_message_id, "u2");
        assert_eq!(point.new_head_message_id.as_deref(), Some("a1"));
        // assistant 目标：前方最近 user 回合（现役 findPrecedingTurnInputIndex）。
        let point = resolve_retry_reset_point(&branch, Some("a2")).unwrap();
        assert_eq!(point.turn_input_message_id, "u2");
        assert_eq!(point.new_head_message_id.as_deref(), Some("a1"));
        // latest-user 兼容形态：最近 user 消息。
        let point = resolve_retry_reset_point(&branch, None).unwrap();
        assert_eq!(point.turn_input_message_id, "u2");
        // 根回合：新头 = None（分支清空到根之前）。
        let point = resolve_retry_reset_point(&branch, Some("u1")).unwrap();
        assert_eq!(point.turn_input_message_id, "u1");
        assert_eq!(point.new_head_message_id, None);
    }

    #[test]
    fn retry_reset_point_loud_failures() {
        let branch = [BranchNode {
            message_id: "a1",
            parent_message_id: None,
            role: "assistant",
        }];
        assert_eq!(
            resolve_retry_reset_point(&branch, Some("nope")),
            Err(ResolveTargetError::NotOnBranch)
        );
        assert_eq!(
            resolve_retry_reset_point(&branch, Some("a1")),
            Err(ResolveTargetError::NoPrecedingUserTurn)
        );
        assert_eq!(
            resolve_retry_reset_point(&branch, None),
            Err(ResolveTargetError::NoUserTurn)
        );
        assert_eq!(
            resolve_retry_reset_point(&[], None),
            Err(ResolveTargetError::NoUserTurn)
        );
    }

    #[test]
    fn path_within_folders_is_component_aware() {
        let folders = vec!["/work".to_string(), "/data dir/x".to_string()];
        assert!(path_within_folders(
            std::path::Path::new("/work/a.rs"),
            &folders
        ));
        assert!(path_within_folders(
            std::path::Path::new("/work/deep/nested/b"),
            &folders
        ));
        assert!(path_within_folders(
            std::path::Path::new("/data dir/x/y"),
            &folders
        ));
        // 字符串前缀陷阱：/work2 不以组件边界落在 /work 内。
        assert!(!path_within_folders(
            std::path::Path::new("/work2/a.rs"),
            &folders
        ));
        assert!(!path_within_folders(
            std::path::Path::new("/etc/passwd"),
            &folders
        ));
        assert!(!path_within_folders(
            std::path::Path::new("/work/../etc/passwd"),
            &folders
        ));
    }
}
