//! 现役 server/routes/git-environment.ts + server/git/git-command.ts 的
//! git 环境信息卡面（R07-T07 γ，22 路由全语义端口）：
//! GET /lingxi/v1/git/status | worktree-info | worktrees | branches | log |
//! stashes | file-diff（7），POST /lingxi/v1/git/log-stats | checkout |
//! create-branch | stash | unstash | discard | worktree-create | commit |
//! push | pull | stage | unstage | amend | fetch | ai-commit-message（15）。
//!
//! 与现役对齐的关键语义：
//! - 执行底座：argv 直跑 git 二进制（无 shell，天然免疫注入），env 继承 +
//!   GIT_OPTIONAL_LOCKS=0 + GIT_TERMINAL_PROMPT=0；默认超时 15s，
//!   push/fetch/pull/worktree-add/log-stats 120s；stdout/stderr 各 16MiB
//!   截获上限，超限杀进程按失败处理（≡ execFile maxBuffer）；try_git 探测
//!   不抛、run_git 抛 GitFailure（message=`git {sub} failed (exit N)`，
//!   stderr trim 随身；路由侧 error = stderr||message）。登记偏差：
//!   ① 超时路径不携带部分输出（现役 err.stdout/stderr 带部分捕获，罕见）；
//!   ② stdin 接 null（现役 execFile 挂空管道，这些 argv 均不读 stdin；
//!   GIT_TERMINAL_PROMPT=0 保证远端操作缺凭据快速失败）；③ windowsHide
//!   在 unix 无对应。
//! - 门禁：resolve_dir（path.resolve 语义——相对路径拼进程 cwd 后词法归一
//!   （多余 .. 到根即弃），stat 须目录；不 realpath）；GET 未获批 403
//!   "dir not allowed" / POST 无效或未获批均 400 "invalid dir"。
//!   is_approved_dir_git = ① selectedAgentDeskRoots（严格 realpath，β3
//!   同款）→ ② approved_desk_roots（父目录回退 realpath；现役
//!   isApprovedWorkspaceDir 分支是其根集子集 = 死代码，不移植，登记）→
//!   ③ [deskCwd, homeCwd, studioMountRoots, 顶层 cwd_history]（严格
//!   realpath）。studioMountRoots = studio-mounts.json 里
//!   active+storage+local_fs+rootLocator.path 非空（MountRegistry
//!   load_or_create = 现役 ensureStudioMountRegistry 同款读时创建副作用；
//!   任何错误 → []）。
//! - status 聚合：staged（cachedNumstat 零提交回退空树）/unstaged numstat
//!   以及 name-status 双通道状态字母（A/M/D 优先于 numstat 猜测）；同路径
//!   staged+unstaged 合并展示、总量分账；未跟踪逐文件计行双预算（2000
//!   文件/1000ms，超预算列路径记 0——现役显式降级）；files 排序 =
//!   localeCompare → 候选人两级近似（主级小写化 UTF-16 比较 + 同位小写
//!   优先第三级；完整 ICU collation 与 pinyin 本地化不移植，登记）；
//!   现役 staged 合并臂的 `binary` 局部变量是死代码（state 只读
//!   entry.binary），候选人不保留（登记）。
//! - 操作族结构化分桶原样：commit（空信息 nothing_to_commit "empty
//!   commit message"；includeUnstaged 默认 true → add -A；diff --cached
//!   --quiet 探测，零提交 128 视为有内容）、amend（零提交
//!   nothing_to_commit；-m/--no-edit；直接 GitError 的 stderr 不 trim——
//!   现役形状）、stage/unstage（invalid_path；零提交 rm --cached）、
//!   push（no_remote/nothing_to_push/push_failed；无 upstream → push -u
//!   remote HEAD）、fetch、pull（no_remote/no_upstream/fetch_failed/
//!   already_up_to_date/diverged/local_changes（文件重叠结构预判，不靠
//!   本地化 stderr 文本）/pull_failed；pulled=behind）、stash（
//!   nothing_to_stash 双判：porcelain 预探 + stdout 文本）、stashes
//!   （limit 20 + stash show + ^3 ls-tree 未跟踪）、unstash（path 单文件：
//!   path_dirty/no_stash/not_in_stash/^3 源选择；无 path：pop +
//!   stdout+stderr conflict 文本判）、discard（零提交空树基线；restore
//!   --staged --worktree；clean -f [-d]）、worktree-create（
//!   ^[A-Za-z0-9][A-Za-z0-9._-]*$；exists/branch_exists/invalid_base；
//!   wt/<name>；<主树父级>/worktrees/<name>；mkdir 失败交 git 报错）。
//! - log/log-stats：LOG_FORMAT（\x00 字段 \x1e 记录 + numstat 归属上一
//!   提交）；--date-order --max-count=clamp(limit,1,1000)（parseInt 失败
//!   → 300）；stats 两段加载（40 位十六进制白名单 + 去重 + 进程级缓存
//!   cap 20000 插入序驱逐 + 每批 100 + 全零统计跳过）。
//! - file-diff：is_safe_rel_path（400 invalid file path）；未跟踪合成全
//!   新增 patch（读上限 256KiB 字节、NUL 判二进制 → {patch:"",
//!   binary:true}）；tracked = numstat HEAD 判 binary + diff HEAD（零提交
//!   回退空树）+ /^Binary files .* differ$/m 补判 + capPatch（256KiB
//!   UTF-16 码元 + "\n…"）。
//! - ai-commit-message：collectChangeContext（numstat 摘要 ≤80 文件 +
//!   diff HEAD -U3 截 12000 UTF-16 码元 + 未跟踪 ≤8 文件 diff --no-index
//!   摘 + 行预算 4000/60——未跟踪摘录臂在现役是死代码（--no-index 有差异
//!   恒 exit 1 → `!res.ok` 恒跳过），原样端口并注释）→ 空 400
//!   "no changes"；summarize 槽 AuxiliaryExecutor（fallback=chat 现役
//!   口径：RouteNotConfigured → chat，双缺 → 500 现役原文 "summarize
//!   slot unavailable (no model configured and no chat fallback)"；
//!   网关/凭证/网络缺 → 503 "llm unavailable"）；45s 竞时 →
//!   "ai commit message timed out" 500；sanitize 后空 → 502
//!   "ai produced empty message"；成功 {message}。登记偏差：①
//!   temperature 0.3 无槽携带（AuxiliaryRequest 无字段，同 diary）；②
//!   per-agent 槽位解析 → 活跃网关快照（agentId 仅门禁 + 归属，同
//!   diary）；③ 现役 Promise.race 败北的 callText 仍在后台结算并入账，
//!   候选人超时即取消、不落账；④ sessionPath→sessionId 归属解析不移植
//!   （账本 cause_ref 只记 agent）；⑤ 用量账本 purpose=auxiliary.summarize
//!   以及 origin=git-commit-message（现役 subsystem=auxiliary +
//!   operation=git-commit-message 的映射）。
//! - auth：route-security.ts:84-89 + isGitEnvironmentRoute:438-462——
//!   log-stats 恒 files.read；其余 21 路径 GET→files.read /
//!   POST→files.write；GET|POST 以外动词落现役 /api/* 兜底
//!   STUDIO_OWNER → LocalOnly（R06 口径）。

use axum::{
    body::Bytes,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use lingxi_adapters::models::auxiliary::{AuxiliaryExecutor, AuxiliaryRequest, AuxiliaryStep};
use lingxi_kernel::{
    model_exchange::{AuxiliarySlot, ModelGatewayError, ModelOperation, ModelRouteRequest},
    ports::StoragePort,
    usage::{CallOutcome, ModelCallUsageRecord, ReportedUsage},
};

use crate::{desk_files, ServiceState};

// ── 常量（git-command.ts :20-32 + git-environment.ts :127-131）──

const DEFAULT_TIMEOUT_MS: u64 = 15_000;
const PUSH_TIMEOUT_MS: u64 = 120_000;
const WORKTREE_TIMEOUT_MS: u64 = 120_000;
const STATS_TIMEOUT_MS: u64 = 120_000;
const MAX_BUFFER: usize = 16 * 1024 * 1024;
/// 空树对象哈希：git diff --cached 在零提交仓库里的对照基线。
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
/// 未跟踪文件行数计数的读取上限（更大文件按截断内容计行，轻微低估）。
const UNTRACKED_READ_CAP: usize = 512 * 1024;
const UNTRACKED_COUNT_FILE_BUDGET: usize = 2000;
const UNTRACKED_COUNT_TIME_BUDGET_MS: u64 = 1_000;
/// 单文件 patch 的读/截上限：读按字节（现役 Buffer 长度），截按 UTF-16
/// 码元（现役 string.length）。
const PATCH_CAP: usize = 256 * 1024;
/// log 记录格式：字段以 \x00 分隔、记录以 \x1e 结束。
const LOG_FORMAT: &str = "%H%x00%h%x00%s%x00%B%x00%an%x00%at%x00%D%x00%P%x1e";
const COMMIT_STATS_CACHE_CAP: usize = 20_000;
const AI_MESSAGE_TIMEOUT_MS: u64 = 45_000;
const DIFF_EXCERPT_CHAR_CAP: usize = 12_000;
const UNTRACKED_EXCERPT_TOTAL_CAP: usize = 4_000;
const UNTRACKED_EXCERPT_LINE_CAP: usize = 60;
const MAX_PROMPT_FILES: usize = 80;

/// 现役 ai-commit-message 系统提示全文（git-environment.ts :537-541）。
const AI_COMMIT_SYSTEM_PROMPT: &str = "你是 git 提交信息生成器。根据给定的变更统计与 diff 摘要，生成中文提交信息，采用 Conventional Commits 的标题+正文格式：\n1. 第一行标题：type(scope): 一句话概括本次变更（type 用 feat/fix/refactor/docs/chore/test 等，scope 尽量取模块名，标题不超过 50 个字）；\n2. 标题后空一行，正文用「- 」开头的要点逐条列出主要改动（每条不超过 30 个字，最多 8 条，按重要性排序）；\n只输出提交信息本身，不要解释、不要引号、不要 markdown 代码块、不要署名。";

// ── 执行底座（tryGit / runGit / gitEnv）──

struct GitRunResult {
    ok: bool,
    stdout: String,
    stderr: String,
    exit_code: Option<i32>,
}

struct GitFailure {
    message: String,
    stderr: String,
    #[allow(dead_code)] // exit_code 与现役 GitError 同形状携带；路由侧只 surfacing 文本
    exit_code: Option<i32>,
}

impl GitFailure {
    fn simple(message: &str) -> Self {
        Self {
            message: message.to_string(),
            stderr: String::new(),
            exit_code: None,
        }
    }
    /// 现役路由 catch：err instanceof GitError ? err.stderr || err.message
    /// : err.message。stderr 非空（哪怕只是空白）即 surfacing stderr。
    fn surfaced(&self) -> String {
        if self.stderr.is_empty() {
            self.message.clone()
        } else {
            self.stderr.clone()
        }
    }
}

/// 每条流 16MiB 截获：返回（已捕获字节， 超限/读错标记）。Node maxBuffer
/// 是 stdout/stderr 各自的上限。
async fn read_capped(
    mut reader: impl tokio::io::AsyncRead + Unpin,
    cap: usize,
) -> (Vec<u8>, Option<String>) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk).await {
            Ok(0) => return (buf, None),
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.len() > cap {
                    return (buf, Some("maxBuffer exceeded".to_string()));
                }
            }
            Err(error) => return (buf, Some(error.to_string())),
        }
    }
}

/// 失败不抛错的探测型调用（git-command.ts :63-82）。
async fn try_git<S: AsRef<str>>(dir: &Path, args: &[S], timeout_ms: u64) -> GitRunResult {
    let spawned = Command::new("git")
        .args(args.iter().map(|a| a.as_ref()))
        .current_dir(dir)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => {
            return GitRunResult {
                ok: false,
                stdout: String::new(),
                stderr: error.to_string(),
                exit_code: None,
            }
        }
    };
    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");
    let capture = async move {
        let (out, err) = tokio::join!(
            read_capped(stdout, MAX_BUFFER),
            read_capped(stderr, MAX_BUFFER)
        );
        let capped = out.1.is_some() || err.1.is_some();
        let status = if capped {
            // 超限即杀（≡ execFile maxBuffer 杀进程），避免孩子堵在满管道上
            let _ = child.kill().await;
            child.wait().await.ok()
        } else {
            child.wait().await.ok()
        };
        (out, err, status, capped)
    };
    match tokio::time::timeout(Duration::from_millis(timeout_ms), capture).await {
        Ok(((out, _), (err, _), Some(status), false)) => GitRunResult {
            ok: status.success(),
            stdout: String::from_utf8_lossy(&out).into_owned(),
            stderr: String::from_utf8_lossy(&err).into_owned(),
            exit_code: status.code(),
        },
        Ok(((out, _), (err, _), _, true)) => {
            // maxBuffer 超限：现役 err.stdout/stderr 带部分捕获、exitCode null
            GitRunResult {
                ok: false,
                stdout: String::from_utf8_lossy(&out).into_owned(),
                stderr: String::from_utf8_lossy(&err).into_owned(),
                exit_code: None,
            }
        }
        Ok((_, _, None, false)) => GitRunResult {
            ok: false,
            stdout: String::new(),
            stderr: "git process wait failed".to_string(),
            exit_code: None,
        },
        Err(_) => GitRunResult {
            ok: false,
            stdout: String::new(),
            stderr: format!(
                "git {} timed out",
                args.first().map(|a| a.as_ref()).unwrap_or("?")
            ),
            exit_code: None,
        },
    }
}

/// 面向用户的操作型调用：失败抛 GitFailure（git-command.ts :85-95）。
async fn run_git<S: AsRef<str>>(
    dir: &Path,
    args: &[S],
    timeout_ms: u64,
) -> Result<String, GitFailure> {
    let result = try_git(dir, args, timeout_ms).await;
    if !result.ok {
        return Err(GitFailure {
            message: format!(
                "git {} failed{}",
                args.first().map(|a| a.as_ref()).unwrap_or("?"),
                result
                    .exit_code
                    .map(|c| format!(" (exit {c})"))
                    .unwrap_or_default()
            ),
            stderr: result.stderr.trim().to_string(),
            exit_code: result.exit_code,
        });
    }
    Ok(result.stdout)
}

async fn is_git_work_tree(dir: &Path) -> bool {
    let probe = try_git(
        dir,
        &["rev-parse", "--is-inside-work-tree"],
        DEFAULT_TIMEOUT_MS,
    )
    .await;
    probe.ok && probe.stdout.trim() == "true"
}

// ── 纯函数（解析器与校验器；git-command.ts :97-248 / :1364-1370）──

/// Number.parseInt(s, 10) 的前缀解析：空白 + 可选符号 + 数字前缀；无数字
/// → None（NaN）；调用方 ||0 / isFinite 分流。
fn parse_int_opt(raw: &str) -> Option<i64> {
    let t = raw.trim_start();
    let (negative, digits) = match t.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let num: String = digits.chars().take_while(|c| c.is_ascii_digit()).collect();
    if num.is_empty() {
        return None;
    }
    num.parse::<i64>()
        .ok()
        .map(|v| if negative { -v } else { v })
}

/// parseInt(...)||0 的合流形态。
fn parse_int_or_zero(raw: &str) -> i64 {
    parse_int_opt(raw).unwrap_or(0)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NumstatEntry {
    additions: i64,
    deletions: i64,
    path: String,
    binary: bool,
}

/// `git diff --numstat -z --no-renames` 输出：`<add>\t<del>\t<path>\0` 序列。
fn parse_numstat_z(output: &str) -> Vec<NumstatEntry> {
    let mut out = Vec::new();
    for field in output.split('\0') {
        if field.is_empty() {
            continue;
        }
        let Some(tab1) = field.find('\t') else {
            continue;
        };
        let Some(tab2_rel) = field[tab1 + 1..].find('\t') else {
            continue;
        };
        let tab2 = tab1 + 1 + tab2_rel;
        let add_raw = &field[..tab1];
        let del_raw = &field[tab1 + 1..tab2];
        let file_path = &field[tab2 + 1..];
        if file_path.is_empty() {
            continue;
        }
        let binary = add_raw == "-" || del_raw == "-";
        out.push(NumstatEntry {
            additions: if binary {
                0
            } else {
                parse_int_or_zero(add_raw)
            },
            deletions: if binary {
                0
            } else {
                parse_int_or_zero(del_raw)
            },
            path: file_path.to_string(),
            binary,
        });
    }
    out
}

/// `git diff --name-status -z --no-renames` 输出：`<status>\0<path>\0` 交替。
fn parse_name_status_z(output: &str) -> Vec<(String, String)> {
    let fields: Vec<&str> = output.split('\0').collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < fields.len() {
        let status = fields[i];
        let path = fields[i + 1];
        if !path.is_empty() && (status == "A" || status == "D" || status == "M") {
            out.push((path.to_string(), status.to_string()));
        }
        i += 2;
    }
    out
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BranchEntry {
    name: String,
    current: bool,
    checked_out_elsewhere: bool,
}

/// `git for-each-ref --format=%(HEAD)%00%(refname:short) refs/heads` 输出。
fn parse_for_each_branch_ref(output: &str) -> Vec<BranchEntry> {
    let mut branches = Vec::new();
    for line in output.split('\n') {
        if line.is_empty() {
            continue;
        }
        let Some(nul) = line.find('\0') else { continue };
        let marker = &line[..nul];
        let name = line[nul + 1..].trim();
        if name.is_empty() {
            continue;
        }
        branches.push(BranchEntry {
            name: name.to_string(),
            current: marker.contains('*'),
            checked_out_elsewhere: false,
        });
    }
    branches
}

#[derive(Debug, Clone, Serialize)]
struct WorktreeEntry {
    path: String,
    head: Option<String>,
    branch: Option<String>,
    detached: bool,
    bare: bool,
}

/// `git worktree list --porcelain` 输出：空行分隔的块，首块为主工作树。
fn parse_worktree_porcelain(output: &str) -> Vec<WorktreeEntry> {
    let mut entries: Vec<WorktreeEntry> = Vec::new();
    let mut cur: Option<usize> = None;
    for line in output.split('\n') {
        if line.trim().is_empty() {
            cur = None;
            continue;
        }
        let space = line.find(' ');
        let key = space.map_or(line, |i| &line[..i]);
        let value = space.map_or("", |i| &line[i + 1..]);
        if key == "worktree" {
            entries.push(WorktreeEntry {
                path: value.to_string(),
                head: None,
                branch: None,
                detached: false,
                bare: false,
            });
            cur = Some(entries.len() - 1);
            continue;
        }
        let Some(index) = cur else { continue };
        match key {
            "branch" => entries[index].branch = Some(value.replacen("refs/heads/", "", 1)),
            "HEAD" => entries[index].head = Some(value.to_string()),
            "detached" => entries[index].detached = true,
            "bare" => entries[index].bare = true,
            _ => {}
        }
    }
    entries
}

/// 校验分支名：拒绝选项注入（前导 -）与空白；存在性由 rev-parse 验证。
fn is_valid_branch_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && !name.chars().any(char::is_whitespace)
        && !name.contains("..")
        && !name.starts_with('/')
}

/// worktree 目录名准入（比分支名更严：目录名同时拼 wt/<name> 分支名）。
fn is_valid_worktree_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
        && name != "."
        && name != ".."
}

/// 校验 repo 相对路径：拒绝绝对路径、`..` 穿越、反斜杠与前导 `-`。
fn is_safe_rel_path(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    if value.starts_with('-') || value.contains('\\') || Path::new(value).is_absolute() {
        return false;
    }
    !value
        .split('/')
        .any(|seg| seg.is_empty() || seg == "." || seg == "..")
}

/// `:(top,literal)`：按仓库根解释路径 + 关掉 glob/attr magic。
fn top_literal_pathspec(rel: &str) -> String {
    format!(":(top,literal){rel}")
}

/// `null` = 不带 pathspec（整仓）；非法路径与空数组分别返回 invalid / null。
fn normalize_path_list(raw: Option<&Value>) -> (Option<Vec<String>>, bool) {
    match raw {
        Some(Value::Array(items)) if !items.is_empty() => {
            let mut paths: Vec<String> = Vec::new();
            for item in items {
                match item.as_str() {
                    Some(s) if is_safe_rel_path(s) => {
                        if !paths.iter().any(|p| p == s) {
                            paths.push(s.to_string());
                        }
                    }
                    _ => return (None, true),
                }
            }
            (Some(paths), false)
        }
        _ => (None, false),
    }
}

fn first_stderr_lines(stderr: &str) -> String {
    stderr
        .split('\n')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .take(4)
        .collect::<Vec<_>>()
        .join("\n")
}

/// UTF-16 码元截断（现役 string.slice 的码元语义；切在代理对中间时
/// lossy 置换孤立半代理——登记，与 diary 文件名同一口径）。
fn utf16_take(text: &str, cap: usize) -> String {
    if text.encode_utf16().count() <= cap {
        return text.to_string();
    }
    let units: Vec<u16> = text.encode_utf16().take(cap).collect();
    String::from_utf16_lossy(&units)
}

fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// files.sort((a,b) => a.path.localeCompare(b.path)) 的候选人两级近似：
/// 主级 = 小写化 UTF-16 码元比较；第三级 = 首个大小写差异位小写优先。
/// 登记：完整 ICU collation（变音/CJK/pinyin 本地化）不移植。
fn locale_compare_approx(a: &str, b: &str) -> std::cmp::Ordering {
    let primary = a
        .to_lowercase()
        .encode_utf16()
        .cmp(b.to_lowercase().encode_utf16());
    if primary != std::cmp::Ordering::Equal {
        return primary;
    }
    let mut ac = a.chars();
    let mut bc = b.chars();
    loop {
        match (ac.next(), bc.next()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(x), Some(y)) if x != y => {
                // 主级相等 ⇒ 同字母大小写差：小写在前（DUCET 第三级）
                return match (x.is_lowercase(), y.is_lowercase()) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => x.cmp(&y),
                };
            }
            _ => {}
        }
    }
}

/// 未跟踪文件按行计新增（git-command.ts :301-321）：结尾换行不额外计
/// 行；二进制/读不到记 0；>512KiB 截断低估。
fn count_untracked_lines(abs: &Path) -> i64 {
    use std::io::Read;
    let Ok(meta) = std::fs::metadata(abs) else {
        return 0;
    };
    if !meta.is_file() {
        return 0;
    }
    let Ok(mut file) = std::fs::File::open(abs) else {
        return 0;
    };
    let length = std::cmp::min(meta.len(), UNTRACKED_READ_CAP as u64) as usize;
    let mut buf = vec![0u8; length];
    let read = if length > 0 {
        file.read(&mut buf).unwrap_or(0)
    } else {
        0
    };
    let slice = &buf[..read];
    if slice.is_empty() || slice.contains(&0) {
        return 0;
    }
    let text = String::from_utf8_lossy(slice);
    let lines: Vec<&str> = text.split('\n').collect();
    if lines.last() == Some(&"") {
        (lines.len() - 1) as i64
    } else {
        lines.len() as i64
    }
}

fn state_from_numstat(entry: &NumstatEntry) -> &'static str {
    if entry.additions > 0 && entry.deletions == 0 {
        "added"
    } else if entry.deletions > 0 && entry.additions == 0 {
        "deleted"
    } else {
        "modified"
    }
}

// ── status 聚合（git-command.ts :346-498）──

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct GitFileChange {
    path: String,
    additions: i64,
    deletions: i64,
    state: String,
    staged: bool,
}

#[derive(Debug, Clone, Serialize)]
struct GitChangeTotals {
    additions: i64,
    deletions: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct GitStatusSummary {
    is_repo: bool,
    current_branch: Option<String>,
    detached: bool,
    total: GitChangeTotals,
    staged_total: GitChangeTotals,
    unstaged_total: GitChangeTotals,
    files: Vec<GitFileChange>,
    has_upstream: bool,
    has_remote: bool,
    ahead: i64,
    behind: i64,
    commitable: bool,
    pushable: bool,
}

fn empty_status() -> GitStatusSummary {
    GitStatusSummary {
        is_repo: false,
        current_branch: None,
        detached: false,
        total: GitChangeTotals {
            additions: 0,
            deletions: 0,
        },
        staged_total: GitChangeTotals {
            additions: 0,
            deletions: 0,
        },
        unstaged_total: GitChangeTotals {
            additions: 0,
            deletions: 0,
        },
        files: Vec::new(),
        has_upstream: false,
        has_remote: false,
        ahead: 0,
        behind: 0,
        commitable: false,
        pushable: false,
    }
}

fn numstat_args(extra: &[&str]) -> Vec<String> {
    let mut args = vec![
        "-c".to_string(),
        "core.quotepath=false".to_string(),
        "diff".to_string(),
        "--numstat".to_string(),
        "-z".to_string(),
        "--no-renames".to_string(),
    ];
    args.extend(extra.iter().map(|s| s.to_string()));
    args
}

fn name_status_args(extra: &[&str]) -> Vec<String> {
    let mut args = vec![
        "-c".to_string(),
        "core.quotepath=false".to_string(),
        "diff".to_string(),
        "--name-status".to_string(),
        "-z".to_string(),
        "--no-renames".to_string(),
    ];
    args.extend(extra.iter().map(|s| s.to_string()));
    args
}

/// cachedNumstat：零提交仓库 `--cached` 默认对照 HEAD 会报 ambiguous，
/// 改对照空树。
async fn cached_numstat(dir: &Path) -> Vec<NumstatEntry> {
    let direct = try_git(dir, &numstat_args(&["--cached"]), DEFAULT_TIMEOUT_MS).await;
    if direct.ok {
        return parse_numstat_z(&direct.stdout);
    }
    let vs_empty = try_git(
        dir,
        &numstat_args(&["--cached", EMPTY_TREE]),
        DEFAULT_TIMEOUT_MS,
    )
    .await;
    if vs_empty.ok {
        return parse_numstat_z(&vs_empty.stdout);
    }
    Vec::new()
}

async fn cached_name_status(dir: &Path) -> Vec<(String, String)> {
    let direct = try_git(dir, &name_status_args(&["--cached"]), DEFAULT_TIMEOUT_MS).await;
    if direct.ok {
        return parse_name_status_z(&direct.stdout);
    }
    let vs_empty = try_git(
        dir,
        &name_status_args(&["--cached", EMPTY_TREE]),
        DEFAULT_TIMEOUT_MS,
    )
    .await;
    if vs_empty.ok {
        return parse_name_status_z(&vs_empty.stdout);
    }
    Vec::new()
}

fn state_for(entry: &NumstatEntry, letters: &HashMap<String, String>) -> String {
    if entry.binary {
        return "binary".to_string();
    }
    match letters.get(&entry.path).map(String::as_str) {
        Some("A") => "added".to_string(),
        Some("D") => "deleted".to_string(),
        Some("M") => "modified".to_string(),
        _ => state_from_numstat(entry).to_string(),
    }
}

async fn collect_git_status(dir: &Path) -> GitStatusSummary {
    if !is_git_work_tree(dir).await {
        return empty_status();
    }
    let unstaged_numstat_args = numstat_args(&[]);
    let unstaged_name_status_args = name_status_args(&[]);
    let (
        show_current,
        staged_numstat,
        unstaged_numstat_res,
        staged_name_status,
        unstaged_name_status_res,
        untracked_out,
        remote_out,
    ) = tokio::join!(
        try_git(dir, &["branch", "--show-current"], DEFAULT_TIMEOUT_MS),
        cached_numstat(dir),
        try_git(dir, &unstaged_numstat_args, DEFAULT_TIMEOUT_MS),
        cached_name_status(dir),
        try_git(dir, &unstaged_name_status_args, DEFAULT_TIMEOUT_MS),
        try_git(
            dir,
            &[
                "-c",
                "core.quotepath=false",
                "ls-files",
                "--others",
                "--exclude-standard",
                "-z",
            ],
            DEFAULT_TIMEOUT_MS
        ),
        try_git(dir, &["remote"], DEFAULT_TIMEOUT_MS),
    );
    let unstaged_numstat = parse_numstat_z(&unstaged_numstat_res.stdout);
    let unstaged_name_status = parse_name_status_z(&unstaged_name_status_res.stdout);

    let mut current_branch: Option<String> = if show_current.ok {
        let trimmed = show_current.stdout.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    } else {
        None
    };
    let detached = current_branch.is_none();
    if detached {
        let short = try_git(dir, &["rev-parse", "--short", "HEAD"], DEFAULT_TIMEOUT_MS).await;
        // 现役 short.ok ? trim : null（trim 出空串也原样保留）
        current_branch = if short.ok {
            Some(short.stdout.trim().to_string())
        } else {
            None
        };
    }

    let untracked_paths: Vec<String> = if untracked_out.ok {
        untracked_out
            .stdout
            .split('\0')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect()
    } else {
        Vec::new()
    };
    let staged_by_path: HashMap<&str, &NumstatEntry> = staged_numstat
        .iter()
        .map(|e| (e.path.as_str(), e))
        .collect();
    let unstaged_by_path: HashMap<&str, &NumstatEntry> = unstaged_numstat
        .iter()
        .map(|e| (e.path.as_str(), e))
        .collect();
    let staged_letter: HashMap<String, String> = staged_name_status.into_iter().collect();
    let unstaged_letter: HashMap<String, String> = unstaged_name_status.into_iter().collect();

    let mut files: Vec<GitFileChange> = Vec::new();
    let mut staged_total = GitChangeTotals {
        additions: 0,
        deletions: 0,
    };
    let mut unstaged_total = GitChangeTotals {
        additions: 0,
        deletions: 0,
    };

    // 已暂存（index vs HEAD），同路径未暂存量合并展示
    for entry in &staged_numstat {
        let unstaged_entry = unstaged_by_path.get(entry.path.as_str());
        files.push(GitFileChange {
            path: entry.path.clone(),
            additions: entry.additions + unstaged_entry.map_or(0, |e| e.additions),
            deletions: entry.deletions + unstaged_entry.map_or(0, |e| e.deletions),
            state: state_for(entry, &staged_letter),
            staged: true,
        });
        staged_total.additions += entry.additions;
        staged_total.deletions += entry.deletions;
        if let Some(e) = unstaged_entry {
            unstaged_total.additions += e.additions;
            unstaged_total.deletions += e.deletions;
        }
    }
    // 仅未暂存（worktree vs index，未出现在已暂存集合）
    for entry in &unstaged_numstat {
        if staged_by_path.contains_key(entry.path.as_str()) {
            continue;
        }
        files.push(GitFileChange {
            path: entry.path.clone(),
            additions: entry.additions,
            deletions: entry.deletions,
            state: state_for(entry, &unstaged_letter),
            staged: false,
        });
        unstaged_total.additions += entry.additions;
        unstaged_total.deletions += entry.deletions;
    }
    // 未跟踪：逐文件读内容数行（双预算显式降级：超预算列路径记 0）
    let untracked_deadline = Instant::now() + Duration::from_millis(UNTRACKED_COUNT_TIME_BUDGET_MS);
    for (index, rel_path) in untracked_paths.iter().enumerate() {
        let mut additions = 0;
        if index < UNTRACKED_COUNT_FILE_BUDGET && Instant::now() < untracked_deadline {
            let abs = rel_path
                .split('/')
                .fold(dir.to_path_buf(), |p, seg| p.join(seg));
            additions = count_untracked_lines(&abs);
        }
        files.push(GitFileChange {
            path: rel_path.clone(),
            additions,
            deletions: 0,
            state: "untracked".to_string(),
            staged: false,
        });
        unstaged_total.additions += additions;
    }

    files.sort_by(|a, b| locale_compare_approx(&a.path, &b.path));
    let total = GitChangeTotals {
        additions: staged_total.additions + unstaged_total.additions,
        deletions: staged_total.deletions + unstaged_total.deletions,
    };

    let upstream = try_git(
        dir,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
        DEFAULT_TIMEOUT_MS,
    )
    .await;
    let has_upstream = upstream.ok && !upstream.stdout.trim().is_empty();
    let mut ahead = 0;
    let mut behind = 0;
    if has_upstream {
        let ahead_res = try_git(
            dir,
            &["rev-list", "--count", "@{upstream}..HEAD"],
            DEFAULT_TIMEOUT_MS,
        )
        .await;
        let behind_res = try_git(
            dir,
            &["rev-list", "--count", "HEAD..@{upstream}"],
            DEFAULT_TIMEOUT_MS,
        )
        .await;
        ahead = if ahead_res.ok {
            parse_int_or_zero(ahead_res.stdout.trim())
        } else {
            0
        };
        behind = if behind_res.ok {
            parse_int_or_zero(behind_res.stdout.trim())
        } else {
            0
        };
    }
    let has_remote = remote_out.ok && !remote_out.stdout.trim().is_empty();

    GitStatusSummary {
        is_repo: true,
        current_branch,
        detached,
        total,
        staged_total,
        unstaged_total,
        commitable: !files.is_empty(),
        files,
        has_upstream,
        has_remote,
        ahead,
        behind,
        pushable: has_remote && (!has_upstream || ahead > 0),
    }
}

// ── worktree-info / worktrees / branches（:506-540 / :1042-1084 / :189-214）──

fn real_path_str(p: &str) -> Option<String> {
    std::fs::canonicalize(p)
        .ok()
        .map(|c| c.to_string_lossy().into_owned())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GitWorktreeInfo {
    is_repo: bool,
    is_main: bool,
    name: Option<String>,
    branch: Option<String>,
    path: Option<String>,
    main_path: Option<String>,
}

async fn worktree_info(dir: &Path) -> GitWorktreeInfo {
    if !is_git_work_tree(dir).await {
        return GitWorktreeInfo {
            is_repo: false,
            is_main: true,
            name: None,
            branch: None,
            path: None,
            main_path: None,
        };
    }
    let top = try_git(dir, &["rev-parse", "--show-toplevel"], DEFAULT_TIMEOUT_MS).await;
    let list = try_git(
        dir,
        &["worktree", "list", "--porcelain"],
        DEFAULT_TIMEOUT_MS,
    )
    .await;
    let self_path = if top.ok {
        top.stdout.trim().to_string()
    } else {
        dir.to_string_lossy().into_owned()
    };
    if !list.ok {
        return GitWorktreeInfo {
            is_repo: true,
            is_main: true,
            name: None,
            branch: None,
            path: Some(self_path.clone()),
            main_path: Some(self_path),
        };
    }
    let entries = parse_worktree_porcelain(&list.stdout);
    let main = entries.first();
    let resolved_self = real_path_str(&self_path).unwrap_or_else(|| self_path.clone());
    let self_entry = entries
        .iter()
        .find(|e| real_path_str(&e.path).unwrap_or_else(|| e.path.clone()) == resolved_self);
    let is_main = match (main, self_entry) {
        (Some(m), Some(s)) => {
            real_path_str(&m.path).unwrap_or_else(|| m.path.clone())
                == real_path_str(&s.path).unwrap_or_else(|| s.path.clone())
        }
        _ => false,
    };
    let name = if is_main {
        None
    } else {
        Some(
            self_entry
                .and_then(|s| s.branch.clone())
                .unwrap_or_else(|| {
                    Path::new(&self_path)
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default()
                }),
        )
    };
    GitWorktreeInfo {
        is_repo: true,
        is_main,
        name,
        branch: self_entry.and_then(|s| s.branch.clone()),
        path: Some(self_path),
        main_path: main.map(|m| m.path.clone()),
    }
}

/// worktree 落地根目录约定：<主工作树父级>/worktrees。
fn worktrees_root_for(main_worktree_path: &str) -> PathBuf {
    Path::new(main_worktree_path)
        .parent()
        .unwrap_or(Path::new("/"))
        .join("worktrees")
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GitWorktreeListResult {
    is_repo: bool,
    worktrees: Vec<GitWorktreeListEntry>,
    root: Option<String>,
    main_path: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GitWorktreeListEntry {
    path: String,
    head: Option<String>,
    branch: Option<String>,
    detached: bool,
    bare: bool,
    is_main: bool,
    current: bool,
}

async fn list_worktrees(dir: &Path) -> GitWorktreeListResult {
    if !is_git_work_tree(dir).await {
        return GitWorktreeListResult {
            is_repo: false,
            worktrees: Vec::new(),
            root: None,
            main_path: None,
        };
    }
    let top = try_git(dir, &["rev-parse", "--show-toplevel"], DEFAULT_TIMEOUT_MS).await;
    let self_path = if top.ok {
        top.stdout.trim().to_string()
    } else {
        dir.to_string_lossy().into_owned()
    };
    let list = try_git(
        dir,
        &["worktree", "list", "--porcelain"],
        DEFAULT_TIMEOUT_MS,
    )
    .await;
    if !list.ok {
        return GitWorktreeListResult {
            is_repo: true,
            worktrees: Vec::new(),
            root: None,
            main_path: None,
        };
    }
    let entries = parse_worktree_porcelain(&list.stdout);
    let resolved_self = real_path_str(&self_path).unwrap_or_else(|| self_path.clone());
    let main = entries.first();
    GitWorktreeListResult {
        is_repo: true,
        worktrees: entries
            .iter()
            .enumerate()
            .map(|(index, entry)| GitWorktreeListEntry {
                path: entry.path.clone(),
                head: entry.head.clone(),
                branch: entry.branch.clone(),
                detached: entry.detached,
                bare: entry.bare,
                is_main: index == 0,
                current: real_path_str(&entry.path).unwrap_or_else(|| entry.path.clone())
                    == resolved_self,
            })
            .collect(),
        root: main.map(|m| worktrees_root_for(&m.path).to_string_lossy().into_owned()),
        main_path: main.map(|m| m.path.clone()),
    }
}

async fn list_branches(dir: &Path) -> Result<(Vec<BranchEntry>, bool, Option<String>), GitFailure> {
    let ref_out = run_git(
        dir,
        &[
            "for-each-ref",
            "--format=%(HEAD)%00%(refname:short)",
            "refs/heads",
        ],
        DEFAULT_TIMEOUT_MS,
    )
    .await?;
    let mut branches = parse_for_each_branch_ref(&ref_out);
    let current = branches.iter().find(|b| b.current).map(|b| b.name.clone());

    let detached_probe = try_git(dir, &["branch", "--show-current"], DEFAULT_TIMEOUT_MS).await;
    let detached = detached_probe.ok && detached_probe.stdout.trim().is_empty();

    let (wt_out, top_out) = tokio::join!(
        try_git(
            dir,
            &["worktree", "list", "--porcelain"],
            DEFAULT_TIMEOUT_MS
        ),
        try_git(dir, &["rev-parse", "--show-toplevel"], DEFAULT_TIMEOUT_MS),
    );
    if wt_out.ok && top_out.ok {
        let self_real = real_path_str(top_out.stdout.trim())
            .unwrap_or_else(|| top_out.stdout.trim().to_string());
        let elsewhere: HashSet<String> = parse_worktree_porcelain(&wt_out.stdout)
            .into_iter()
            .filter(|e| {
                e.branch.is_some()
                    && real_path_str(&e.path).unwrap_or_else(|| e.path.clone()) != self_real
            })
            .filter_map(|e| e.branch)
            .collect();
        for branch in &mut branches {
            if elsewhere.contains(&branch.name) {
                branch.checked_out_elsewhere = true;
            }
        }
    }
    Ok((branches, detached, current))
}

// ── 操作族（:544-1157）──

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OpResult {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    head: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    paths: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pulled: Option<i64>,
}

impl OpResult {
    fn fail(code: &str) -> Self {
        Self {
            ok: false,
            code: Some(code.to_string()),
            message: None,
            head: None,
            branch: None,
            path: None,
            stash: None,
            paths: None,
            pulled: None,
        }
    }
    fn ok() -> Self {
        Self {
            ok: true,
            code: None,
            message: None,
            head: None,
            branch: None,
            path: None,
            stash: None,
            paths: None,
            pulled: None,
        }
    }
}

async fn checkout_branch(dir: &Path, branch: &str) -> Result<(), GitFailure> {
    if !is_valid_branch_name(branch) {
        return Err(GitFailure::simple("invalid branch name"));
    }
    let refspec = format!("refs/heads/{branch}");
    let verify = try_git(
        dir,
        &["rev-parse", "--verify", "--quiet", &refspec],
        DEFAULT_TIMEOUT_MS,
    )
    .await;
    if !verify.ok {
        return Err(GitFailure {
            message: "branch not found".to_string(),
            stderr: String::new(),
            exit_code: verify.exit_code,
        });
    }
    run_git(dir, &["checkout", branch], DEFAULT_TIMEOUT_MS).await?;
    Ok(())
}

async fn commit_changes(
    dir: &Path,
    message: &str,
    include_unstaged: bool,
) -> Result<OpResult, GitFailure> {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        let mut result = OpResult::fail("nothing_to_commit");
        result.message = Some("empty commit message".to_string());
        return Ok(result);
    }
    if include_unstaged {
        run_git(dir, &["add", "-A"], DEFAULT_TIMEOUT_MS).await?;
    }
    // --quiet 退出码 1 = 有暂存差异；0 = 干净（零提交仓库报 128，视为有
    // 暂存内容，交给 commit 本身成败说话）
    let quiet = try_git(dir, &["diff", "--cached", "--quiet"], DEFAULT_TIMEOUT_MS).await;
    if quiet.ok {
        return Ok(OpResult::fail(if include_unstaged {
            "nothing_to_commit"
        } else {
            "nothing_staged"
        }));
    }
    let result = try_git(dir, &["commit", "-m", trimmed], DEFAULT_TIMEOUT_MS).await;
    if !result.ok {
        if result.stderr.to_lowercase().contains("nothing to commit") {
            return Ok(OpResult::fail("nothing_to_commit"));
        }
        // 直接 GitError（非 runGit）：stderr 不 trim（现役形状）
        return Err(GitFailure {
            message: "commit failed".to_string(),
            stderr: result.stderr,
            exit_code: result.exit_code,
        });
    }
    let head = try_git(dir, &["rev-parse", "HEAD"], DEFAULT_TIMEOUT_MS).await;
    let mut result_out = OpResult::ok();
    if head.ok {
        result_out.head = Some(head.stdout.trim().to_string());
    }
    Ok(result_out)
}

async fn amend_commit(dir: &Path, message: Option<&str>) -> Result<OpResult, GitFailure> {
    let verify = try_git(
        dir,
        &["rev-parse", "--verify", "--quiet", "HEAD^{commit}"],
        DEFAULT_TIMEOUT_MS,
    )
    .await;
    if !verify.ok {
        let mut result = OpResult::fail("nothing_to_commit");
        result.message = Some("no commit to amend".to_string());
        return Ok(result);
    }
    let trimmed = message.map(str::trim).unwrap_or("");
    let args: Vec<String> = if trimmed.is_empty() {
        vec!["commit".into(), "--amend".into(), "--no-edit".into()]
    } else {
        vec![
            "commit".into(),
            "--amend".into(),
            "-m".into(),
            trimmed.to_string(),
        ]
    };
    let result = try_git(dir, &args, DEFAULT_TIMEOUT_MS).await;
    if !result.ok {
        if result.stderr.to_lowercase().contains("nothing to commit") {
            return Ok(OpResult::fail("nothing_to_commit"));
        }
        return Err(GitFailure {
            message: "amend failed".to_string(),
            stderr: result.stderr,
            exit_code: result.exit_code,
        });
    }
    let head = try_git(dir, &["rev-parse", "HEAD"], DEFAULT_TIMEOUT_MS).await;
    let mut result_out = OpResult::ok();
    if head.ok {
        result_out.head = Some(head.stdout.trim().to_string());
    }
    Ok(result_out)
}

async fn stage_paths(dir: &Path, paths: Option<&Value>) -> OpResult {
    let (wanted, invalid) = normalize_path_list(paths);
    if invalid {
        return OpResult::fail("invalid_path");
    }
    let args: Vec<String> = match &wanted {
        Some(list) => {
            let mut a = vec!["add".to_string(), "--".to_string()];
            a.extend(list.iter().map(|p| top_literal_pathspec(p)));
            a
        }
        None => vec!["add".to_string(), "-A".to_string()],
    };
    let result = try_git(dir, &args, DEFAULT_TIMEOUT_MS).await;
    if !result.ok {
        let mut out = OpResult::fail("stage_failed");
        out.message = Some(first_stderr_lines(&result.stderr));
        out.paths = Some(wanted.clone().unwrap_or_default());
        return out;
    }
    let mut out = OpResult::ok();
    out.paths = Some(wanted.unwrap_or_default());
    out
}

async fn unstage_paths(dir: &Path, paths: Option<&Value>) -> OpResult {
    let (wanted, invalid) = normalize_path_list(paths);
    if invalid {
        return OpResult::fail("invalid_path");
    }
    let spec: Vec<String> = match &wanted {
        Some(list) => list.iter().map(|p| top_literal_pathspec(p)).collect(),
        None => vec![":/".to_string()],
    };
    let head = try_git(
        dir,
        &["rev-parse", "--verify", "--quiet", "HEAD^{commit}"],
        DEFAULT_TIMEOUT_MS,
    )
    .await;
    let args: Vec<String> = if head.ok {
        let mut a = vec![
            "restore".to_string(),
            "--staged".to_string(),
            "--".to_string(),
        ];
        a.extend(spec.iter().cloned());
        a
    } else {
        let mut a = vec![
            "rm".to_string(),
            "-r".to_string(),
            "--cached".to_string(),
            "--ignore-unmatch".to_string(),
            "--quiet".to_string(),
            "--".to_string(),
        ];
        a.extend(spec.iter().cloned());
        a
    };
    let result = try_git(dir, &args, DEFAULT_TIMEOUT_MS).await;
    if !result.ok {
        let mut out = OpResult::fail("unstage_failed");
        out.message = Some(first_stderr_lines(&result.stderr));
        out.paths = Some(wanted.clone().unwrap_or_default());
        return out;
    }
    let mut out = OpResult::ok();
    out.paths = Some(wanted.unwrap_or_default());
    out
}

async fn push_changes(dir: &Path) -> OpResult {
    let remote_out = try_git(dir, &["remote"], DEFAULT_TIMEOUT_MS).await;
    let remote = if remote_out.ok {
        remote_out
            .stdout
            .split('\n')
            .map(str::trim)
            .find(|s| !s.is_empty())
            .map(str::to_string)
    } else {
        None
    };
    let Some(remote) = remote else {
        return OpResult::fail("no_remote");
    };
    let upstream = try_git(
        dir,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
        DEFAULT_TIMEOUT_MS,
    )
    .await;
    let has_upstream = upstream.ok && !upstream.stdout.trim().is_empty();
    if has_upstream {
        let ahead_res = try_git(
            dir,
            &["rev-list", "--count", "@{upstream}..HEAD"],
            DEFAULT_TIMEOUT_MS,
        )
        .await;
        let ahead = if ahead_res.ok {
            parse_int_or_zero(ahead_res.stdout.trim())
        } else {
            0
        };
        if ahead == 0 {
            return OpResult::fail("nothing_to_push");
        }
        let result = try_git(dir, &["push"], PUSH_TIMEOUT_MS).await;
        if !result.ok {
            let mut out = OpResult::fail("push_failed");
            out.message = Some(first_stderr_lines(&result.stderr));
            return out;
        }
        return OpResult::ok();
    }
    // 无 upstream：push -u 建立跟踪
    let result = try_git(dir, &["push", "-u", &remote, "HEAD"], PUSH_TIMEOUT_MS).await;
    if !result.ok {
        let mut out = OpResult::fail("push_failed");
        out.message = Some(first_stderr_lines(&result.stderr));
        return out;
    }
    OpResult::ok()
}

async fn fetch_remote(dir: &Path) -> OpResult {
    let remote_out = try_git(dir, &["remote"], DEFAULT_TIMEOUT_MS).await;
    let remote = if remote_out.ok {
        remote_out
            .stdout
            .split('\n')
            .map(str::trim)
            .find(|s| !s.is_empty())
            .map(str::to_string)
    } else {
        None
    };
    let Some(remote) = remote else {
        return OpResult::fail("no_remote");
    };
    let result = try_git(dir, &["fetch", &remote], PUSH_TIMEOUT_MS).await;
    if !result.ok {
        let mut out = OpResult::fail("fetch_failed");
        out.message = Some(first_stderr_lines(&result.stderr));
        return out;
    }
    OpResult::ok()
}

/// 本地未提交/未跟踪文件与待并入文件重叠：重叠则快进会被 git 拒绝
/// （结构性预判，不靠本地化的 stderr 文本）。
async fn dirty_files_clash_with_incoming(dir: &Path) -> bool {
    let (dirty_res, untracked_res, incoming_res) = tokio::join!(
        try_git(
            dir,
            &["-c", "core.quotepath=false", "diff", "--name-only", "HEAD"],
            DEFAULT_TIMEOUT_MS
        ),
        try_git(
            dir,
            &[
                "-c",
                "core.quotepath=false",
                "ls-files",
                "--others",
                "--exclude-standard",
            ],
            DEFAULT_TIMEOUT_MS
        ),
        try_git(
            dir,
            &[
                "-c",
                "core.quotepath=false",
                "diff",
                "--name-only",
                "HEAD..@{upstream}",
            ],
            DEFAULT_TIMEOUT_MS
        ),
    );
    if !incoming_res.ok {
        return false;
    }
    let incoming: Vec<String> = incoming_res
        .stdout
        .split('\n')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    let mut local: HashSet<String> = HashSet::new();
    if dirty_res.ok {
        local.extend(dirty_res.stdout.split('\n').map(str::to_string));
    }
    if untracked_res.ok {
        local.extend(untracked_res.stdout.split('\n').map(str::to_string));
    }
    let local: HashSet<String> = local
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    incoming.iter().any(|p| local.contains(p))
}

async fn pull_changes(dir: &Path) -> OpResult {
    let remote_out = try_git(dir, &["remote"], DEFAULT_TIMEOUT_MS).await;
    let remote = if remote_out.ok {
        remote_out
            .stdout
            .split('\n')
            .map(str::trim)
            .find(|s| !s.is_empty())
            .map(str::to_string)
    } else {
        None
    };
    let Some(remote) = remote else {
        return OpResult::fail("no_remote");
    };
    let upstream = try_git(
        dir,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
        DEFAULT_TIMEOUT_MS,
    )
    .await;
    if !upstream.ok || upstream.stdout.trim().is_empty() {
        return OpResult::fail("no_upstream");
    }
    let fetch = try_git(dir, &["fetch", &remote], PUSH_TIMEOUT_MS).await;
    if !fetch.ok {
        let mut out = OpResult::fail("fetch_failed");
        out.message = Some(first_stderr_lines(&fetch.stderr));
        return out;
    }
    let behind_res = try_git(
        dir,
        &["rev-list", "--count", "HEAD..@{upstream}"],
        DEFAULT_TIMEOUT_MS,
    )
    .await;
    let behind = if behind_res.ok {
        parse_int_or_zero(behind_res.stdout.trim())
    } else {
        0
    };
    if behind == 0 {
        return OpResult::fail("already_up_to_date");
    }
    let ahead_res = try_git(
        dir,
        &["rev-list", "--count", "@{upstream}..HEAD"],
        DEFAULT_TIMEOUT_MS,
    )
    .await;
    let ahead = if ahead_res.ok {
        parse_int_or_zero(ahead_res.stdout.trim())
    } else {
        0
    };
    if ahead > 0 {
        return OpResult::fail("diverged");
    }
    if dirty_files_clash_with_incoming(dir).await {
        return OpResult::fail("local_changes");
    }
    let merge = try_git(
        dir,
        &["merge", "--ff-only", "@{upstream}"],
        DEFAULT_TIMEOUT_MS,
    )
    .await;
    if !merge.ok {
        let mut out = OpResult::fail("pull_failed");
        out.message = Some(first_stderr_lines(&merge.stderr));
        return out;
    }
    let mut out = OpResult::ok();
    out.pulled = Some(behind);
    out
}

async fn create_branch(dir: &Path, name: &str, base: Option<&str>) -> OpResult {
    let branch = name.trim();
    if !is_valid_branch_name(branch) {
        return OpResult::fail("invalid_name");
    }
    let refspec = format!("refs/heads/{branch}");
    let existing = try_git(
        dir,
        &["rev-parse", "--verify", "--quiet", &refspec],
        DEFAULT_TIMEOUT_MS,
    )
    .await;
    if existing.ok {
        let mut out = OpResult::fail("already_exists");
        out.branch = Some(branch.to_string());
        return out;
    }
    let mut args = vec!["checkout".to_string(), "-b".to_string(), branch.to_string()];
    let start = base.map(str::trim).unwrap_or("");
    if !start.is_empty() {
        if !is_valid_branch_name(start) {
            let mut out = OpResult::fail("invalid_base");
            out.branch = Some(branch.to_string());
            return out;
        }
        let commit_spec = format!("{start}^{{commit}}");
        let verify = try_git(
            dir,
            &["rev-parse", "--verify", "--quiet", &commit_spec],
            DEFAULT_TIMEOUT_MS,
        )
        .await;
        if !verify.ok {
            let mut out = OpResult::fail("invalid_base");
            out.branch = Some(branch.to_string());
            return out;
        }
        args.push(start.to_string());
    }
    let result = try_git(dir, &args, DEFAULT_TIMEOUT_MS).await;
    if !result.ok {
        let mut out = OpResult::fail("branch_failed");
        out.branch = Some(branch.to_string());
        out.message = Some(first_stderr_lines(&result.stderr));
        return out;
    }
    let mut out = OpResult::ok();
    out.branch = Some(branch.to_string());
    out
}

async fn stash_changes(dir: &Path, message: Option<&str>, paths: Option<&Value>) -> OpResult {
    let (wanted, invalid) = normalize_path_list(paths);
    if invalid {
        return OpResult::fail("invalid_path");
    }
    let trimmed = message.map(str::trim).unwrap_or("");
    let spec: Option<Vec<String>> = wanted
        .as_ref()
        .map(|list| list.iter().map(|p| top_literal_pathspec(p)).collect());

    let mut dirty_args = vec!["status".to_string(), "--porcelain".to_string()];
    if let Some(spec) = &spec {
        dirty_args.push("--".to_string());
        dirty_args.extend(spec.iter().cloned());
    }
    let dirty = try_git(dir, &dirty_args, DEFAULT_TIMEOUT_MS).await;
    if dirty.ok && dirty.stdout.trim().is_empty() {
        return OpResult::fail("nothing_to_stash");
    }

    let mut args = vec![
        "stash".to_string(),
        "push".to_string(),
        "--include-untracked".to_string(),
    ];
    if !trimmed.is_empty() {
        args.push("-m".to_string());
        args.push(trimmed.to_string());
    }
    if let Some(spec) = &spec {
        args.push("--".to_string());
        args.extend(spec.iter().cloned());
    }
    let result = try_git(dir, &args, DEFAULT_TIMEOUT_MS).await;
    if !result.ok {
        let mut out = OpResult::fail("stash_failed");
        out.message = Some(first_stderr_lines(&result.stderr));
        return out;
    }
    if result
        .stdout
        .to_lowercase()
        .contains("no local changes to save")
    {
        return OpResult::fail("nothing_to_stash");
    }
    let mut out = OpResult::ok();
    if let Some(wanted) = wanted {
        out.paths = Some(wanted);
    }
    out
}

/// 一条储藏覆盖的改动路径：已跟踪部分（stash show）+ 未跟踪部分（第三
/// 个父提交）。
async fn stash_changed_paths(dir: &Path, stash_ref: &str) -> (Vec<String>, Vec<String>) {
    let untracked_source = format!("{stash_ref}^3");
    let tracked_args = ["stash", "show", "--name-only", "--no-renames", stash_ref];
    let untracked_args = [
        "ls-tree",
        "-r",
        "--full-tree",
        "--name-only",
        "-z",
        untracked_source.as_str(),
    ];
    let (tracked, untracked) = tokio::join!(
        try_git(dir, &tracked_args, DEFAULT_TIMEOUT_MS),
        try_git(dir, &untracked_args, DEFAULT_TIMEOUT_MS),
    );
    let lines = |out: &str, sep: char| {
        out.split(sep)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    (
        if tracked.ok {
            lines(&tracked.stdout, '\n')
        } else {
            Vec::new()
        },
        if untracked.ok {
            lines(&untracked.stdout, '\0')
        } else {
            Vec::new()
        },
    )
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GitStashListResult {
    is_repo: bool,
    stashes: Vec<GitStashEntry>,
}

#[derive(Serialize)]
struct GitStashEntry {
    #[serde(rename = "ref")]
    stash_ref: String,
    message: String,
    tracked: Vec<String>,
    untracked: Vec<String>,
}

/// 储藏栈（新→旧）。limit 限制条数：每条要走 git 取路径。
async fn list_stashes(dir: &Path, limit: usize) -> GitStashListResult {
    if !is_git_work_tree(dir).await {
        return GitStashListResult {
            is_repo: false,
            stashes: Vec::new(),
        };
    }
    let list = try_git(
        dir,
        &["stash", "list", "--format=%gd%x00%gs"],
        DEFAULT_TIMEOUT_MS,
    )
    .await;
    if !list.ok {
        return GitStashListResult {
            is_repo: true,
            stashes: Vec::new(),
        };
    }
    let mut entries = Vec::new();
    for line in list
        .stdout
        .split('\n')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .take(limit)
    {
        let mut parts = line.split('\0');
        let Some(stash_ref) = parts.next() else {
            continue;
        };
        if stash_ref.is_empty() {
            continue;
        }
        let message = parts.collect::<Vec<_>>().join("\0");
        let (tracked, untracked) = stash_changed_paths(dir, stash_ref).await;
        entries.push(GitStashEntry {
            stash_ref: stash_ref.to_string(),
            message,
            tracked,
            untracked,
        });
    }
    GitStashListResult {
        is_repo: true,
        stashes: entries,
    }
}

async fn restore_stashed_path(dir: &Path, rel_path: &str) -> OpResult {
    if !is_safe_rel_path(rel_path) {
        return OpResult::fail("invalid_path");
    }
    let spec = top_literal_pathspec(rel_path);
    let dirty = try_git(
        dir,
        &["status", "--porcelain", "--", &spec],
        DEFAULT_TIMEOUT_MS,
    )
    .await;
    if dirty.ok && !dirty.stdout.trim().is_empty() {
        let mut out = OpResult::fail("path_dirty");
        out.path = Some(rel_path.to_string());
        return out;
    }
    let list = try_git(dir, &["stash", "list", "--format=%gd"], DEFAULT_TIMEOUT_MS).await;
    let refs: Vec<String> = if list.ok {
        list.stdout
            .split('\n')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect()
    } else {
        Vec::new()
    };
    if refs.is_empty() {
        let mut out = OpResult::fail("no_stash");
        out.path = Some(rel_path.to_string());
        return out;
    }
    for stash_ref in refs {
        let (tracked, untracked) = stash_changed_paths(dir, &stash_ref).await;
        let from_untracked =
            !tracked.iter().any(|p| p == rel_path) && untracked.iter().any(|p| p == rel_path);
        if !tracked.iter().any(|p| p == rel_path) && !from_untracked {
            continue;
        }
        let source = if from_untracked {
            format!("{stash_ref}^3")
        } else {
            stash_ref.clone()
        };
        let restored = try_git(
            dir,
            &["restore", "--source", &source, "--worktree", "--", &spec],
            DEFAULT_TIMEOUT_MS,
        )
        .await;
        if !restored.ok {
            let mut out = OpResult::fail("unstash_failed");
            out.path = Some(rel_path.to_string());
            out.stash = Some(stash_ref);
            out.message = Some(first_stderr_lines(&restored.stderr));
            return out;
        }
        let mut out = OpResult::ok();
        out.path = Some(rel_path.to_string());
        out.stash = Some(stash_ref);
        return out;
    }
    let mut out = OpResult::fail("not_in_stash");
    out.path = Some(rel_path.to_string());
    out
}

async fn pop_stash(dir: &Path) -> OpResult {
    let list = try_git(dir, &["stash", "list", "--format=%gd"], DEFAULT_TIMEOUT_MS).await;
    if !list.ok || list.stdout.trim().is_empty() {
        return OpResult::fail("no_stash");
    }
    let result = try_git(dir, &["stash", "pop"], DEFAULT_TIMEOUT_MS).await;
    if !result.ok {
        let detail = format!("{}\n{}", result.stdout, result.stderr);
        let mut out = OpResult::fail(if detail.to_lowercase().contains("conflict") {
            "unstash_conflict"
        } else {
            "unstash_failed"
        });
        out.message = Some(first_stderr_lines(if result.stderr.is_empty() {
            &result.stdout
        } else {
            &result.stderr
        }));
        return out;
    }
    OpResult::ok()
}

async fn discard_paths(dir: &Path, paths: Option<&Value>) -> OpResult {
    let (wanted, invalid) = normalize_path_list(paths);
    if invalid {
        return OpResult::fail("invalid_path");
    }
    let spec: Vec<String> = match &wanted {
        Some(list) => list.iter().map(|p| top_literal_pathspec(p)).collect(),
        None => vec![":/".to_string()],
    };
    // 零提交仓库拿空树当基线（与 cachedNumstat 同款兜底）
    let head = try_git(
        dir,
        &["rev-parse", "--verify", "--quiet", "HEAD^{commit}"],
        DEFAULT_TIMEOUT_MS,
    )
    .await;
    let source = if head.ok { "HEAD" } else { EMPTY_TREE };

    let mut unstaged_args = vec![
        "-c".to_string(),
        "core.quotepath=false".to_string(),
        "diff".to_string(),
        "--name-only".to_string(),
        source.to_string(),
        "--".to_string(),
    ];
    unstaged_args.extend(spec.iter().cloned());
    let mut staged_args = vec![
        "-c".to_string(),
        "core.quotepath=false".to_string(),
        "diff".to_string(),
        "--cached".to_string(),
        "--name-only".to_string(),
        source.to_string(),
        "--".to_string(),
    ];
    staged_args.extend(spec.iter().cloned());
    let mut untracked_args = vec![
        "-c".to_string(),
        "core.quotepath=false".to_string(),
        "ls-files".to_string(),
        "--others".to_string(),
        "--exclude-standard".to_string(),
        "-z".to_string(),
    ];
    if wanted.is_some() {
        untracked_args.push("--".to_string());
        untracked_args.extend(spec.iter().cloned());
    }
    let (unstaged, staged, untracked) = tokio::join!(
        try_git(dir, &unstaged_args, DEFAULT_TIMEOUT_MS),
        try_git(dir, &staged_args, DEFAULT_TIMEOUT_MS),
        try_git(dir, &untracked_args, DEFAULT_TIMEOUT_MS),
    );
    let has_tracked = !unstaged.stdout.trim().is_empty() || !staged.stdout.trim().is_empty();
    let untracked_paths: Vec<&str> = if untracked.ok {
        untracked
            .stdout
            .split('\0')
            .filter(|s| !s.is_empty())
            .collect()
    } else {
        Vec::new()
    };
    if !has_tracked && untracked_paths.is_empty() {
        let mut out = OpResult::fail("nothing_to_discard");
        out.paths = Some(wanted.clone().unwrap_or_default());
        return out;
    }
    if has_tracked {
        let mut restore_args = vec![
            "restore".to_string(),
            "--source".to_string(),
            source.to_string(),
            "--staged".to_string(),
            "--worktree".to_string(),
            "--".to_string(),
        ];
        restore_args.extend(spec.iter().cloned());
        let result = try_git(dir, &restore_args, DEFAULT_TIMEOUT_MS).await;
        if !result.ok {
            let mut out = OpResult::fail("discard_failed");
            out.message = Some(first_stderr_lines(&result.stderr));
            out.paths = Some(wanted.clone().unwrap_or_default());
            return out;
        }
    }
    // 未跟踪：删除文件。整仓回退带 -d 连未跟踪目录一起清（ignored 不动）
    if !untracked_paths.is_empty() {
        let clean_args: Vec<String> = if wanted.is_some() {
            let mut a = vec!["clean".to_string(), "-f".to_string(), "--".to_string()];
            a.extend(spec.iter().cloned());
            a
        } else {
            vec!["clean".to_string(), "-f".to_string(), "-d".to_string()]
        };
        let clean = try_git(dir, &clean_args, DEFAULT_TIMEOUT_MS).await;
        if !clean.ok {
            let mut out = OpResult::fail("discard_failed");
            out.message = Some(first_stderr_lines(&clean.stderr));
            out.paths = Some(wanted.clone().unwrap_or_default());
            return out;
        }
    }
    let mut out = OpResult::ok();
    out.paths = Some(wanted.unwrap_or_default());
    out
}

async fn create_worktree(dir: &Path, name: &str, base: Option<&str>) -> OpResult {
    let raw = name.trim();
    if !is_valid_worktree_name(raw) {
        return OpResult::fail("invalid_name");
    }
    let list = list_worktrees(dir).await;
    let (Some(root), Some(main_path)) = (list.root.clone(), list.main_path.clone()) else {
        return OpResult::fail("create_failed");
    };
    if !list.is_repo {
        return OpResult::fail("create_failed");
    }
    let target_path = Path::new(&root).join(raw);
    let target = target_path.to_string_lossy().into_owned();
    let branch = format!("wt/{raw}");
    if std::fs::metadata(&target_path).is_ok() {
        let mut out = OpResult::fail("exists");
        out.path = Some(target);
        out.branch = Some(branch);
        return out;
    }
    let refspec = format!("refs/heads/{branch}");
    let branch_exists = try_git(
        dir,
        &["rev-parse", "--verify", "--quiet", &refspec],
        DEFAULT_TIMEOUT_MS,
    )
    .await;
    if branch_exists.ok {
        let mut out = OpResult::fail("branch_exists");
        out.path = Some(target);
        out.branch = Some(branch);
        return out;
    }
    let mut args = vec![
        "worktree".to_string(),
        "add".to_string(),
        "-b".to_string(),
        branch.clone(),
        target.clone(),
    ];
    let start = base.map(str::trim).unwrap_or("");
    if !start.is_empty() {
        if !is_valid_branch_name(start) {
            let mut out = OpResult::fail("invalid_base");
            out.branch = Some(branch);
            return out;
        }
        let commit_spec = format!("{start}^{{commit}}");
        let verify = try_git(
            dir,
            &["rev-parse", "--verify", "--quiet", &commit_spec],
            DEFAULT_TIMEOUT_MS,
        )
        .await;
        if !verify.ok {
            let mut out = OpResult::fail("invalid_base");
            out.branch = Some(branch);
            return out;
        }
        args.push(start.to_string());
    }
    // 建目录失败就交给 git worktree add 报错，保持单一失败来源
    let _ = std::fs::create_dir_all(&root);
    let result = try_git(dir, &args, WORKTREE_TIMEOUT_MS).await;
    if !result.ok {
        let mut out = OpResult::fail("create_failed");
        out.path = Some(target);
        out.branch = Some(branch);
        out.message = Some(first_stderr_lines(&result.stderr));
        return out;
    }
    let _ = main_path;
    let mut out = OpResult::ok();
    out.path = Some(target);
    out.branch = Some(branch);
    out
}

// ── 提交历史（:1159-1354）──

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct GitCommitRef {
    kind: String,
    name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct GitCommit {
    hash: String,
    short_hash: String,
    subject: String,
    message: String,
    author_name: String,
    committed_at: i64,
    refs: Vec<GitCommitRef>,
    parents: Vec<String>,
    additions: i64,
    deletions: i64,
    changed_files: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct GitCommitStats {
    additions: i64,
    deletions: i64,
    changed_files: i64,
}

fn stats_from_numstat_lines(lines: &[String]) -> GitCommitStats {
    let mut additions = 0;
    let mut deletions = 0;
    let mut changed_files = 0;
    for line in lines {
        let Some(tab1) = line.find('\t') else {
            continue;
        };
        let Some(tab2_rel) = line[tab1 + 1..].find('\t') else {
            continue;
        };
        let tab2 = tab1 + 1 + tab2_rel;
        changed_files += 1;
        let add_raw = &line[..tab1];
        let del_raw = &line[tab1 + 1..tab2];
        if add_raw != "-" {
            additions += parse_int_or_zero(add_raw);
        }
        if del_raw != "-" {
            deletions += parse_int_or_zero(del_raw);
        }
    }
    GitCommitStats {
        additions,
        deletions,
        changed_files,
    }
}

fn apply_commit_stats(commit: &mut GitCommit, lines: &[String]) {
    let stats = stats_from_numstat_lines(lines);
    commit.additions = stats.additions;
    commit.deletions = stats.deletions;
    commit.changed_files = stats.changed_files;
}

/// `git log [--numstat] --pretty=format:…%x1e` 输出解析（git-command.ts
/// :1225-1283）：numstat 块出现在下一个 \x1e 块的开头，归给上一条提交；
/// 最后一条提交的统计是纯 numstat 尾块。
fn parse_log_records(output: &str) -> Vec<GitCommit> {
    let mut commits: Vec<GitCommit> = Vec::new();
    let mut last: Option<usize> = None;
    for record in output.split('\u{1e}') {
        if record.trim().is_empty() {
            continue;
        }
        let stripped = record.strip_prefix('\n').unwrap_or(record);
        let lines: Vec<&str> = stripped.split('\n').collect();
        let mut start = 0;
        let mut stats: Vec<String> = Vec::new();
        while start < lines.len() && !lines[start].contains('\0') {
            if !lines[start].trim().is_empty() {
                stats.push(lines[start].to_string());
            }
            start += 1;
        }
        if start >= lines.len() {
            if let Some(index) = last {
                apply_commit_stats(&mut commits[index], &stats);
            }
            continue;
        }
        if let Some(index) = last {
            apply_commit_stats(&mut commits[index], &stats);
        }
        let joined = lines[start..].join("\n");
        let fields: Vec<&str> = joined.split('\0').collect();
        if fields.len() < 8 {
            last = None;
            continue;
        }
        let (
            hash,
            short_hash,
            subject,
            message_raw,
            author_name,
            committed_raw,
            refs_raw,
            parents_raw,
        ) = (
            fields[0], fields[1], fields[2], fields[3], fields[4], fields[5], fields[6], fields[7],
        );
        if hash.is_empty() || short_hash.is_empty() {
            last = None;
            continue;
        }
        let mut refs: Vec<GitCommitRef> = Vec::new();
        for entry in refs_raw.split(',') {
            let name = entry.trim();
            if name.is_empty() {
                continue;
            }
            if let Some(target) = name.strip_prefix("HEAD -> ") {
                refs.push(GitCommitRef {
                    kind: "head".to_string(),
                    name: target.to_string(),
                });
                continue;
            }
            if name == "HEAD" {
                refs.push(GitCommitRef {
                    kind: "head".to_string(),
                    name: "HEAD".to_string(),
                });
                continue;
            }
            if let Some(tag) = name.strip_prefix("tag: ") {
                refs.push(GitCommitRef {
                    kind: "tag".to_string(),
                    name: tag.to_string(),
                });
                continue;
            }
            refs.push(GitCommitRef {
                kind: if name.contains('/') {
                    "remote"
                } else {
                    "branch"
                }
                .to_string(),
                name: name.to_string(),
            });
        }
        commits.push(GitCommit {
            hash: hash.to_string(),
            short_hash: short_hash.to_string(),
            subject: subject.to_string(),
            message: {
                let stripped_newlines = message_raw.trim_end_matches('\n');
                stripped_newlines.trim().to_string()
            },
            author_name: author_name.to_string(),
            committed_at: parse_int_or_zero(committed_raw),
            refs,
            parents: if parents_raw.is_empty() {
                Vec::new()
            } else {
                parents_raw
                    .split(' ')
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect()
            },
            additions: 0,
            deletions: 0,
            changed_files: 0,
        });
        last = Some(commits.len() - 1);
    }
    commits
}

async fn list_commits(dir: &Path, limit: i64) -> Vec<GitCommit> {
    let safe_limit = limit.clamp(1, 1000);
    let max_count = format!("--max-count={safe_limit}");
    let pretty = format!("--pretty=format:{LOG_FORMAT}");
    let res = try_git(
        dir,
        &["log", "--date-order", &max_count, &pretty],
        DEFAULT_TIMEOUT_MS,
    )
    .await;
    if res.ok {
        parse_log_records(&res.stdout)
    } else {
        Vec::new()
    }
}

/// 提交统计缓存：同一提交的 diff 永不变化，进程内缓存跨弹窗打开复用
/// （git-command.ts :1301-1306 的模块级 Map + 插入序驱逐）。
struct CommitStatsCache {
    map: HashMap<String, GitCommitStats>,
    order: VecDeque<String>,
}

impl CommitStatsCache {
    fn get(&self, key: &str) -> Option<GitCommitStats> {
        self.map.get(key).cloned()
    }
    fn insert(&mut self, key: String, stats: GitCommitStats) {
        if !self.map.contains_key(&key) {
            self.order.push_back(key.clone());
        }
        self.map.insert(key, stats);
        while self.map.len() > COMMIT_STATS_CACHE_CAP {
            if let Some(oldest) = self.order.pop_front() {
                self.map.remove(&oldest);
            } else {
                break;
            }
        }
    }
}

static COMMIT_STATS_CACHE: OnceLock<Mutex<CommitStatsCache>> = OnceLock::new();

fn commit_stats_cache() -> &'static Mutex<CommitStatsCache> {
    COMMIT_STATS_CACHE.get_or_init(|| {
        Mutex::new(CommitStatsCache {
            map: HashMap::new(),
            order: VecDeque::new(),
        })
    })
}

fn commit_stats_cache_key(dir: &Path, hash: &str) -> String {
    format!("{}\0{hash}", dir.to_string_lossy())
}

fn is_full_hex_hash(value: &str) -> bool {
    value.len() == 40
        && value
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

/// 批量取提交变更统计（git-command.ts :1315-1354）：--no-walk=unsorted
/// --numstat --diff-merges=first-parent 一次进程算一批；失败按空处理
/// （UI 隐藏统计行的显式降级）。
async fn list_commit_stats(dir: &Path, hashes: &[Value]) -> HashMap<String, GitCommitStats> {
    let mut result: HashMap<String, GitCommitStats> = HashMap::new();
    let mut pending: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for value in hashes {
        let Some(hash) = value.as_str() else { continue };
        if !is_full_hex_hash(hash) || seen.contains(hash) {
            continue;
        }
        seen.insert(hash.to_string());
        let key = commit_stats_cache_key(dir, hash);
        let cached = commit_stats_cache().lock().unwrap().get(&key);
        if let Some(stats) = cached {
            result.insert(hash.to_string(), stats);
        } else {
            pending.push(hash.to_string());
        }
    }
    for chunk in pending.chunks(100) {
        let mut args = vec![
            "log".to_string(),
            "--no-walk=unsorted".to_string(),
            "--numstat".to_string(),
            "--diff-merges=first-parent".to_string(),
            format!("--pretty=format:{LOG_FORMAT}"),
        ];
        args.extend(chunk.iter().cloned());
        let res = try_git(dir, &args, STATS_TIMEOUT_MS).await;
        if !res.ok {
            continue;
        }
        for commit in parse_log_records(&res.stdout) {
            if commit.additions == 0 && commit.deletions == 0 && commit.changed_files == 0 {
                continue;
            }
            let stats = GitCommitStats {
                additions: commit.additions,
                deletions: commit.deletions,
                changed_files: commit.changed_files,
            };
            commit_stats_cache()
                .lock()
                .unwrap()
                .insert(commit_stats_cache_key(dir, &commit.hash), stats.clone());
            result.insert(commit.hash, stats);
        }
    }
    result
}

// ── 单文件 diff（:1356-1438）──

#[derive(Serialize)]
struct GitFileDiff {
    path: String,
    patch: String,
    binary: bool,
}

fn cap_patch(patch: &str) -> String {
    if utf16_len(patch) <= PATCH_CAP {
        return patch.to_string();
    }
    format!("{}\n…", utf16_take(patch, PATCH_CAP))
}

/// 未跟踪文件合成 unified diff：整文件作为新增行。
fn build_untracked_patch(rel_path: &str, abs_path: &Path) -> Option<String> {
    use std::io::Read;
    let meta = std::fs::metadata(abs_path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let mut file = std::fs::File::open(abs_path).ok()?;
    let length = std::cmp::min(meta.len(), PATCH_CAP as u64) as usize;
    let mut buf = vec![0u8; length];
    let read = if length > 0 {
        file.read(&mut buf).unwrap_or(0)
    } else {
        0
    };
    let slice = &buf[..read];
    if !slice.is_empty() && slice.contains(&0) {
        return None; // 二进制
    }
    let content = String::from_utf8_lossy(slice);
    let mut lines: Vec<&str> = content.split('\n').collect();
    // 结尾换行产生的空尾行不算内容行
    if lines.last() == Some(&"") {
        lines.pop();
    }
    let body = lines
        .iter()
        .map(|line| format!("+{line}"))
        .collect::<Vec<_>>()
        .join("\n");
    Some(
        [
            format!("diff --git a/{rel_path} b/{rel_path}"),
            "new file mode 100644".to_string(),
            "--- /dev/null".to_string(),
            format!("+++ b/{rel_path}"),
            format!("@@ -0,0 +1,{} @@", lines.len()),
            body,
        ]
        .join("\n"),
    )
}

async fn file_diff(dir: &Path, rel_path: &str) -> Result<GitFileDiff, GitFailure> {
    if !is_safe_rel_path(rel_path) {
        return Err(GitFailure::simple("invalid file path"));
    }
    // 未跟踪文件：git diff 不含它，合成全新增 patch（现役此处不带
    // quotepath=false——非 ASCII 路径匹配失败的怪癖原样保留）
    let untracked = try_git(
        dir,
        &[
            "ls-files",
            "--others",
            "--exclude-standard",
            "-z",
            "--",
            rel_path,
        ],
        DEFAULT_TIMEOUT_MS,
    )
    .await;
    let is_untracked = untracked.ok
        && untracked
            .stdout
            .split('\0')
            .filter(|s| !s.is_empty())
            .any(|p| p == rel_path);
    if is_untracked {
        let abs = rel_path
            .split('/')
            .fold(dir.to_path_buf(), |p, seg| p.join(seg));
        return Ok(match build_untracked_patch(rel_path, &abs) {
            None => GitFileDiff {
                path: rel_path.to_string(),
                patch: String::new(),
                binary: true,
            },
            Some(patch) => GitFileDiff {
                path: rel_path.to_string(),
                patch: cap_patch(&patch),
                binary: false,
            },
        });
    }
    let numstat = try_git(
        dir,
        &numstat_args(&["HEAD", "--", rel_path]),
        DEFAULT_TIMEOUT_MS,
    )
    .await;
    let entry = if numstat.ok {
        parse_numstat_z(&numstat.stdout).into_iter().next()
    } else {
        None
    };
    let mut diff_res = try_git(
        dir,
        &["-c", "core.quotepath=false", "diff", "HEAD", "--", rel_path],
        DEFAULT_TIMEOUT_MS,
    )
    .await;
    if !diff_res.ok {
        // 零提交仓库：对照空树
        diff_res = try_git(
            dir,
            &[
                "-c",
                "core.quotepath=false",
                "diff",
                EMPTY_TREE,
                "--",
                rel_path,
            ],
            DEFAULT_TIMEOUT_MS,
        )
        .await;
    }
    let patch = if diff_res.ok {
        diff_res.stdout
    } else {
        String::new()
    };
    let binary = entry.is_some_and(|e| e.binary)
        || patch
            .lines()
            .any(|line| line.starts_with("Binary files ") && line.ends_with(" differ"));
    Ok(GitFileDiff {
        path: rel_path.to_string(),
        patch: cap_patch(&patch),
        binary,
    })
}

// ── 门禁（git-environment.ts :43-125）──

/// path.resolve 的词法归一（多余 .. 到根即弃；不 realpath）。
fn lexical_normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in p.components() {
        match component {
            Component::Prefix(prefix) => out.push(prefix.as_os_str()),
            Component::RootDir => out.push(Path::new("/")),
            Component::CurDir => {}
            Component::ParentDir => {
                let _ = out.pop();
            }
            Component::Normal(seg) => out.push(seg),
        }
    }
    out
}

/// 现役 resolveDir（git-environment.ts :110-120）：path.resolve（相对路
/// 径拼进程 cwd）→ 须存在且为目录。
fn resolve_dir(raw: Option<&str>) -> Option<PathBuf> {
    let raw = raw?;
    if raw.trim().is_empty() {
        return None;
    }
    let path = Path::new(raw);
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(path)
    };
    let dir = lexical_normalize(&joined);
    if !dir.is_absolute() {
        return None;
    }
    match std::fs::metadata(&dir) {
        Ok(meta) if meta.is_dir() => Some(dir),
        _ => None,
    }
}

/// 工作台注册表（studio-mounts.json）里 active 的 local_fs 挂载根目录
/// （git-environment.ts :74-90）；任何错误 → []。
fn studio_mount_roots(state: &ServiceState) -> Vec<PathBuf> {
    let home = state.config().data_home.clone();
    let now = state.clock().now_unix_ms();
    let Ok(registry) = crate::resource_mounts::MountRegistry::new(&home).load_or_create(now) else {
        return Vec::new();
    };
    registry["mounts"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|m| {
            m["status"].as_str() == Some("active")
                && m["sourceKind"].as_str() == Some("storage")
                && m["provider"].as_str() == Some("local_fs")
                && m["rootLocator"]["path"]
                    .as_str()
                    .is_some_and(|p| !p.is_empty())
        })
        .filter_map(|m| m["rootLocator"]["path"].as_str().map(PathBuf::from))
        .collect()
}

/// 现役 isApprovedDir（git-environment.ts :93-108）：① 指定 agentId 的
/// explicit+effective home（严格 realpath）→ ② isApprovedDeskDir（父目录
/// 回退 realpath；isApprovedWorkspaceDir 是其子集 = 死代码不移植）→ ③
/// [deskCwd, homeCwd, studioMountRoots, 顶层 cwd_history]（严格 realpath）。
fn is_approved_dir_git(state: &ServiceState, dir: &Path, agent_id: Option<&str>) -> bool {
    if let Some(agent) = agent_id {
        if desk_files::is_inside_any_root(dir, &desk_files::selected_agent_desk_roots(state, agent))
        {
            return true;
        }
    }
    let Some(resolved) = desk_files::real_path_for_workspace_check(dir) else {
        return false;
    };
    if desk_files::approved_desk_roots(state).iter().any(|root| {
        match desk_files::real_path_for_workspace_check(root) {
            Some(base) => resolved == base || resolved.starts_with(base.join("")),
            None => false,
        }
    }) {
        return true;
    }
    let agent = desk_files::current_agent_id(state);
    let mut approved: Vec<PathBuf> = Vec::new();
    if let Some(desk) = desk_files::desk_cwd(state, agent.as_deref()) {
        approved.push(desk);
    }
    if let Some(home) = desk_files::home_cwd(state, agent.as_deref()) {
        approved.push(home);
    }
    approved.extend(studio_mount_roots(state));
    if let Some(agent) = agent {
        let (_, history) = desk_files::agent_desk_config(state, &agent);
        approved.extend(history.into_iter().map(PathBuf::from));
    }
    desk_files::is_inside_any_root(dir, &approved)
}

struct ResolvedDir {
    dir: PathBuf,
    agent_id: Option<String>,
}

/// Err = 完整的 403/400 响应（处理器早退直返）；装箱只增噪声，定点
/// allow（chat_wire.rs / lib.rs 同款先例）。
#[allow(clippy::result_large_err)]
fn resolve_query_dir(
    state: &ServiceState,
    query: &HashMap<String, String>,
) -> Result<ResolvedDir, Response> {
    let dir = match resolve_dir(query.get("dir").map(String::as_str)) {
        Some(dir) => dir,
        None => {
            return Err(json_response(
                StatusCode::BAD_REQUEST,
                json!({"error": "invalid dir"}),
            ))
        }
    };
    let agent_id = query
        .get("agentId")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    if !is_approved_dir_git(state, &dir, agent_id.as_deref()) {
        return Err(json_response(
            StatusCode::FORBIDDEN,
            json!({"error": "dir not allowed"}),
        ));
    }
    Ok(ResolvedDir { dir, agent_id })
}

/// 现役 resolveBodyDir：无效与未获批同归 null → 400 "invalid dir"。
fn resolve_body_dir(state: &ServiceState, body: &Value) -> Option<ResolvedDir> {
    let dir = resolve_dir(body.get("dir").and_then(Value::as_str))?;
    let agent_id = body
        .get("agentId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    if is_approved_dir_git(state, &dir, agent_id.as_deref()) {
        Some(ResolvedDir { dir, agent_id })
    } else {
        None
    }
}

// ── AI 提交信息（git-environment.ts :127-200 + :522-560）──

fn sanitize_regexes() -> (&'static regex::Regex, &'static regex::Regex) {
    static FENCES: OnceLock<regex::Regex> = OnceLock::new();
    static PREFIX: OnceLock<regex::Regex> = OnceLock::new();
    (
        FENCES.get_or_init(|| regex::Regex::new(r"```[a-zA-Z]*\n?").unwrap()),
        PREFIX.get_or_init(|| {
            regex::Regex::new(r"(?i)^(commit( message)?|提交信息)\s*[:：]\s*").unwrap()
        }),
    )
}

/// 净化模型输出（git-environment.ts :181-200）：剥代码围栏、去首行前
/// 缀，保留多行结构；压行尾空白与连续空行，整体 600 UTF-16 码元限长。
fn sanitize_commit_message(raw: &str) -> String {
    let (fences, prefix) = sanitize_regexes();
    let without_fences = fences.replace_all(raw, "");
    let without_prefix = prefix.replace(without_fences.trim(), "");
    let mut cleaned: Vec<String> = Vec::new();
    let mut pending_blank = false;
    for line in without_prefix.split('\n') {
        if line.trim().is_empty() {
            if !cleaned.is_empty() {
                pending_blank = true;
            }
            continue;
        }
        if pending_blank {
            cleaned.push(String::new());
            pending_blank = false;
        }
        cleaned.push(line.trim().to_string());
    }
    utf16_take(&cleaned.join("\n"), 600).trim().to_string()
}

/// 收集用于 AI 提交信息的变更上下文（git-environment.ts :134-175）：
/// numstat 摘要 + 截断 diff + 未跟踪文件开头（未跟踪摘录臂在现役是死
/// 代码——`diff --no-index` 有差异恒 exit 1 → `!res.ok` 恒跳过——原样
/// 端口）。
async fn collect_change_context(dir: &Path, include_unstaged: bool) -> String {
    let status = collect_git_status(dir).await;
    let files: Vec<&GitFileChange> = if include_unstaged {
        status.files.iter().collect()
    } else {
        status.files.iter().filter(|f| f.staged).collect()
    };
    if files.is_empty() {
        return String::new();
    }
    let stat_lines = files
        .iter()
        .take(MAX_PROMPT_FILES)
        .map(|f| format!("+{} -{} [{}] {}", f.additions, f.deletions, f.state, f.path))
        .collect::<Vec<_>>()
        .join("\n");
    let more_note = if files.len() > MAX_PROMPT_FILES {
        format!("\n… 另有 {} 个文件", files.len() - MAX_PROMPT_FILES)
    } else {
        String::new()
    };
    let mut parts: Vec<String> = vec![format!(
        "变更文件（共 {} 个）：\n{stat_lines}{more_note}",
        files.len()
    )];

    let tracked_diff = try_git(
        dir,
        &["-c", "core.quotepath=false", "diff", "HEAD", "-U3"],
        DEFAULT_TIMEOUT_MS,
    )
    .await;
    if tracked_diff.ok && !tracked_diff.stdout.trim().is_empty() {
        parts.push(format!(
            "已跟踪文件 diff（截断）：\n{}",
            utf16_take(&tracked_diff.stdout, DIFF_EXCERPT_CHAR_CAP)
        ));
    }

    if include_unstaged {
        let untracked: Vec<&&GitFileChange> = files
            .iter()
            .filter(|f| f.state == "untracked")
            .take(8)
            .collect();
        let mut budget = UNTRACKED_EXCERPT_TOTAL_CAP;
        let mut excerpts: Vec<String> = Vec::new();
        for f in untracked {
            if budget == 0 {
                break;
            }
            let res = try_git(
                dir,
                &[
                    "-c",
                    "core.quotepath=false",
                    "diff",
                    "--no-index",
                    "--",
                    "/dev/null",
                    &f.path,
                ],
                DEFAULT_TIMEOUT_MS,
            )
            .await;
            if !res.ok || res.stdout.is_empty() {
                continue;
            }
            let body = res
                .stdout
                .split('\n')
                .filter(|line| line.starts_with('+') && !line.starts_with("+++"))
                .take(UNTRACKED_EXCERPT_LINE_CAP)
                .collect::<Vec<_>>()
                .join("\n");
            let clipped = utf16_take(&body, budget);
            budget -= utf16_len(&clipped);
            excerpts.push(format!("新文件 {} 开头：\n{clipped}", f.path));
        }
        if !excerpts.is_empty() {
            parts.push(excerpts.join("\n\n"));
        }
    }
    parts.join("\n\n")
}

enum SummarizeSlotError {
    /// 现役 hub.eventBus.request 缺失臂 → 503 "llm unavailable"。
    Unavailable,
    /// 槽位解析/执行器装配失败 → 500（双缺时携带现役原文）。
    Slot(String),
}

/// summarize 槽装配（utility:call-text 的 resolveAuxiliaryModelFresh(
/// "summarize")；fallback=chat 现役口径：RouteNotConfigured → chat，双缺
/// → 现役原文报文）。
fn summarize_slot(
    state: &ServiceState,
) -> Result<(AuxiliaryExecutor, ModelOperation), SummarizeSlotError> {
    let gateway = state
        .model_gateway()
        .cloned()
        .ok_or(SummarizeSlotError::Unavailable)?;
    let credentials = state
        .credential_service()
        .cloned()
        .ok_or(SummarizeSlotError::Unavailable)?;
    let slot = ModelOperation::Auxiliary(AuxiliarySlot::Summarize);
    let operation = match gateway.resolve_dispatch(&ModelRouteRequest::for_operation(slot)) {
        Ok(_) => slot,
        Err(ModelGatewayError::RouteNotConfigured { .. }) => {
            gateway
                .resolve_dispatch(&ModelRouteRequest::for_operation(ModelOperation::Chat))
                .map_err(|_| {
                    SummarizeSlotError::Slot(
                        "summarize slot unavailable (no model configured and no chat fallback)"
                            .to_string(),
                    )
                })?;
            ModelOperation::Chat
        }
        Err(error) => return Err(SummarizeSlotError::Slot(error.to_string())),
    };
    let network = state
        .network_plane()
        .cloned()
        .ok_or(SummarizeSlotError::Unavailable)?;
    let executor = AuxiliaryExecutor::new_with_network(
        gateway,
        credentials,
        lingxi_kernel::toolcatalog::SchemaBudget::default(),
        network,
    )
    .map_err(|error| SummarizeSlotError::Slot(error.to_string()))?;
    Ok((executor, operation))
}

/// summarize 槽单次文本调用 + 45s 竞时 + 用量落账（diary call_memory_text
/// 同构；超时即取消不落账——登记，与现役 race 败北后后台结算不同）。
async fn call_summarize_text(
    state: &ServiceState,
    agent_id: &str,
    executor: &AuxiliaryExecutor,
    operation: ModelOperation,
    request: &AuxiliaryRequest,
) -> Result<String, String> {
    let run_id = format!(
        "git-commit-message-{}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        lingxi_adapters::storage::agent_memory::random_token_suffix()
    );
    let _permit = state
        .runs()
        .quotas()
        .acquire(crate::quotas::QuotaResource::Model, agent_id, &run_id)
        .await
        .map_err(|err| err.to_string())?;
    let action = format!(
        "{run_id}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    );
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let ctx = lingxi_kernel::RunContext {
        principal: lingxi_kernel::Principal::LocalUser,
        session_id: lingxi_protocol::SessionId::new("git-commit-message"),
        run_id: lingxi_protocol::RunId::new(&run_id),
        attempt: lingxi_protocol::AttemptId::new(&run_id),
        generation: 1,
    };
    let model_call_id = lingxi_protocol::ModelCallId::new(&action);
    let call = executor.complete_step_for_operation(
        &ctx,
        operation,
        "git-commit-message",
        &model_call_id,
        request,
    );
    let result =
        match tokio::time::timeout(Duration::from_millis(AI_MESSAGE_TIMEOUT_MS), call).await {
            Ok(result) => result,
            Err(_) => return Err("ai commit message timed out".to_string()),
        };
    let (usage, attempts, served, protocol, outcome) = match &result {
        Ok(AuxiliaryStep::Text(answer)) => (
            answer.usage_report.clone(),
            answer.transport_attempts,
            answer.served_by.clone(),
            answer.served_protocol.clone(),
            CallOutcome::Succeeded,
        ),
        Ok(AuxiliaryStep::ToolRequests {
            usage_report,
            transport_attempts,
            served_by,
            served_protocol,
            ..
        }) => (
            usage_report.clone(),
            *transport_attempts,
            served_by.clone(),
            served_protocol.clone(),
            CallOutcome::Failed,
        ),
        Err(error) => (
            error.usage_report.clone(),
            error.transport_attempts,
            error.served_by.clone(),
            error.served_protocol.clone(),
            CallOutcome::Failed,
        ),
    };
    let settled = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    state
        .storage()
        .record_model_call_usage(
            ModelCallUsageRecord {
                session_id: None,
                run_id: None,
                attempt: None,
                model_call_id: action,
                purpose: "auxiliary.summarize".into(),
                origin: "git-commit-message".into(),
                parent_run_id: Some(run_id.clone()),
                cause_ref: Some(format!("agent:{agent_id}")),
                parent_tool_call_id: None,
                provider: served.provider,
                model: served.model,
                protocol: protocol.clone().unwrap_or_else(|| "unknown".into()),
                usage: if let ReportedUsage::Known(value) = usage.clone() {
                    Some(value)
                } else {
                    None
                },
                invalid_detail: if let ReportedUsage::Invalid { detail } = usage {
                    Some(detail)
                } else {
                    None
                },
                transport_attempts: Some(attempts),
                outcome,
                started_at_unix_ms: Some(at),
                settled_at_unix_ms: Some(settled),
                emitted_tool_calls: vec![],
                cost_basis: None,
            },
            settled,
        )
        .await
        .map_err(|err| err.to_string())?;
    match result {
        Ok(AuxiliaryStep::Text(answer)) => Ok(answer.text),
        Ok(AuxiliaryStep::ToolRequests { .. }) => {
            Err("utility call-text attempted to call a tool".to_string())
        }
        Err(error) => Err(error.to_string()),
    }
}

// ── HTTP 面 ──

fn json_response(status: StatusCode, body: Value) -> Response {
    (status, Json(body)).into_response()
}

fn bad_request(message: &str) -> Response {
    json_response(StatusCode::BAD_REQUEST, json!({"error": message}))
}

/// 现役 GitError catch：{error: stderr||message} 500。
fn failure_response(err: &GitFailure) -> Response {
    json_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        json!({"error": err.surfaced()}),
    )
}

/// 现役 create-branch/stash/unstash/discard/worktree-create 信封：
/// !ok → 400 {...result, error: message||code}（message 空串按 falsy 落
/// code）；ok → 200 result 原样。
fn spread_result_response(result: OpResult) -> Response {
    let mut value = serde_json::to_value(&result).unwrap_or_else(|_| json!({"ok": false}));
    if result.ok {
        return json_response(StatusCode::OK, value);
    }
    let error = result
        .message
        .as_deref()
        .filter(|s| !s.is_empty())
        .or(result.code.as_deref())
        .unwrap_or("error")
        .to_string();
    value["error"] = json!(error);
    json_response(StatusCode::BAD_REQUEST, value)
}

/// 现役 commit/push/pull/stage/unstage/amend/fetch 信封：!ok → 400
/// result 原样（不补 error 字段）。
fn plain_result_response(result: OpResult) -> Response {
    if result.ok {
        json_response(StatusCode::OK, serde_json::to_value(&result).unwrap())
    } else {
        json_response(
            StatusCode::BAD_REQUEST,
            serde_json::to_value(&result).unwrap(),
        )
    }
}

async fn git_status(
    State(state): State<ServiceState>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let resolved = match resolve_query_dir(&state, &query) {
        Ok(r) => r,
        Err(response) => return response,
    };
    let status = collect_git_status(&resolved.dir).await;
    json_response(StatusCode::OK, serde_json::to_value(&status).unwrap())
}

async fn git_worktree_info(
    State(state): State<ServiceState>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let resolved = match resolve_query_dir(&state, &query) {
        Ok(r) => r,
        Err(response) => return response,
    };
    let info = worktree_info(&resolved.dir).await;
    json_response(StatusCode::OK, serde_json::to_value(&info).unwrap())
}

async fn git_worktrees(
    State(state): State<ServiceState>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let resolved = match resolve_query_dir(&state, &query) {
        Ok(r) => r,
        Err(response) => return response,
    };
    let result = list_worktrees(&resolved.dir).await;
    json_response(StatusCode::OK, serde_json::to_value(&result).unwrap())
}

async fn git_branches(
    State(state): State<ServiceState>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let resolved = match resolve_query_dir(&state, &query) {
        Ok(r) => r,
        Err(response) => return response,
    };
    if !is_git_work_tree(&resolved.dir).await {
        return json_response(
            StatusCode::OK,
            json!({"isRepo": false, "branches": [], "detached": false, "current": null}),
        );
    }
    match list_branches(&resolved.dir).await {
        Ok((branches, detached, current)) => json_response(
            StatusCode::OK,
            json!({"isRepo": true, "branches": branches, "detached": detached, "current": current}),
        ),
        Err(error) => failure_response(&error),
    }
}

async fn git_log(
    State(state): State<ServiceState>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let resolved = match resolve_query_dir(&state, &query) {
        Ok(r) => r,
        Err(response) => return response,
    };
    if !is_git_work_tree(&resolved.dir).await {
        return json_response(StatusCode::OK, json!({"isRepo": false, "commits": []}));
    }
    let limit = query
        .get("limit")
        .and_then(|raw| parse_int_opt(raw))
        .unwrap_or(300);
    let commits = list_commits(&resolved.dir, limit).await;
    json_response(StatusCode::OK, json!({"isRepo": true, "commits": commits}))
}

async fn git_log_stats(State(state): State<ServiceState>, body: Bytes) -> Response {
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let Some(resolved) = resolve_body_dir(&state, &body) else {
        return bad_request("invalid dir");
    };
    if !is_git_work_tree(&resolved.dir).await {
        return json_response(StatusCode::OK, json!({"isRepo": false, "stats": {}}));
    }
    let hashes: Vec<Value> = body
        .get("hashes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let stats = list_commit_stats(&resolved.dir, &hashes).await;
    let stats_value: serde_json::Map<String, Value> = stats
        .into_iter()
        .map(|(hash, stats)| (hash, serde_json::to_value(&stats).unwrap()))
        .collect();
    json_response(
        StatusCode::OK,
        json!({"isRepo": true, "stats": Value::Object(stats_value)}),
    )
}

async fn git_checkout(State(state): State<ServiceState>, body: Bytes) -> Response {
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let Some(resolved) = resolve_body_dir(&state, &body) else {
        return bad_request("invalid dir");
    };
    let branch = body
        .get("branch")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let Some(branch) = branch else {
        return bad_request("branch required");
    };
    if !is_git_work_tree(&resolved.dir).await {
        return bad_request("not a git repo");
    }
    match checkout_branch(&resolved.dir, branch).await {
        Ok(()) => json_response(StatusCode::OK, json!({"ok": true, "branch": branch})),
        Err(error) => failure_response(&error),
    }
}

async fn git_create_branch(State(state): State<ServiceState>, body: Bytes) -> Response {
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let Some(resolved) = resolve_body_dir(&state, &body) else {
        return bad_request("invalid dir");
    };
    let name = body
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let Some(name) = name else {
        return bad_request("name required");
    };
    if !is_git_work_tree(&resolved.dir).await {
        return bad_request("not a git repo");
    }
    let base = body.get("base").and_then(Value::as_str);
    let result = create_branch(&resolved.dir, name, base).await;
    spread_result_response(result)
}

async fn git_stash(State(state): State<ServiceState>, body: Bytes) -> Response {
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let Some(resolved) = resolve_body_dir(&state, &body) else {
        return bad_request("invalid dir");
    };
    if !is_git_work_tree(&resolved.dir).await {
        return bad_request("not a git repo");
    }
    let message = body.get("message").and_then(Value::as_str);
    let result = stash_changes(&resolved.dir, message, body.get("paths")).await;
    spread_result_response(result)
}

async fn git_stashes(
    State(state): State<ServiceState>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let resolved = match resolve_query_dir(&state, &query) {
        Ok(r) => r,
        Err(response) => return response,
    };
    let result = list_stashes(&resolved.dir, 20).await;
    json_response(StatusCode::OK, serde_json::to_value(&result).unwrap())
}

async fn git_unstash(State(state): State<ServiceState>, body: Bytes) -> Response {
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let Some(resolved) = resolve_body_dir(&state, &body) else {
        return bad_request("invalid dir");
    };
    if !is_git_work_tree(&resolved.dir).await {
        return bad_request("not a git repo");
    }
    let path = body
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let result = match path {
        Some(path) => restore_stashed_path(&resolved.dir, path).await,
        None => pop_stash(&resolved.dir).await,
    };
    spread_result_response(result)
}

async fn git_discard(State(state): State<ServiceState>, body: Bytes) -> Response {
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let Some(resolved) = resolve_body_dir(&state, &body) else {
        return bad_request("invalid dir");
    };
    if !is_git_work_tree(&resolved.dir).await {
        return bad_request("not a git repo");
    }
    let result = discard_paths(&resolved.dir, body.get("paths")).await;
    spread_result_response(result)
}

async fn git_worktree_create(State(state): State<ServiceState>, body: Bytes) -> Response {
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let Some(resolved) = resolve_body_dir(&state, &body) else {
        return bad_request("invalid dir");
    };
    let name = body
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let Some(name) = name else {
        return bad_request("name required");
    };
    if !is_git_work_tree(&resolved.dir).await {
        return bad_request("not a git repo");
    }
    let base = body.get("base").and_then(Value::as_str);
    let result = create_worktree(&resolved.dir, name, base).await;
    spread_result_response(result)
}

async fn git_file_diff(
    State(state): State<ServiceState>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let resolved = match resolve_query_dir(&state, &query) {
        Ok(r) => r,
        Err(response) => return response,
    };
    let file = query.get("file").cloned().unwrap_or_default();
    if !is_git_work_tree(&resolved.dir).await {
        return bad_request("not a git repo");
    }
    match file_diff(&resolved.dir, &file).await {
        Ok(diff) => json_response(StatusCode::OK, serde_json::to_value(&diff).unwrap()),
        Err(error) => {
            // 现役 /invalid file path/ → 400，其余 GitError → 500
            if error.message.contains("invalid file path") {
                json_response(StatusCode::BAD_REQUEST, json!({"error": error.surfaced()}))
            } else {
                failure_response(&error)
            }
        }
    }
}

async fn git_commit(State(state): State<ServiceState>, body: Bytes) -> Response {
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let Some(resolved) = resolve_body_dir(&state, &body) else {
        return bad_request("invalid dir");
    };
    if !is_git_work_tree(&resolved.dir).await {
        return bad_request("not a git repo");
    }
    let message = body.get("message").and_then(Value::as_str).unwrap_or("");
    let include_unstaged = body.get("includeUnstaged") != Some(&Value::Bool(false));
    match commit_changes(&resolved.dir, message, include_unstaged).await {
        Ok(result) => plain_result_response(result),
        Err(error) => failure_response(&error),
    }
}

async fn git_push(State(state): State<ServiceState>, body: Bytes) -> Response {
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let Some(resolved) = resolve_body_dir(&state, &body) else {
        return bad_request("invalid dir");
    };
    if !is_git_work_tree(&resolved.dir).await {
        return bad_request("not a git repo");
    }
    plain_result_response(push_changes(&resolved.dir).await)
}

async fn git_pull(State(state): State<ServiceState>, body: Bytes) -> Response {
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let Some(resolved) = resolve_body_dir(&state, &body) else {
        return bad_request("invalid dir");
    };
    if !is_git_work_tree(&resolved.dir).await {
        return bad_request("not a git repo");
    }
    plain_result_response(pull_changes(&resolved.dir).await)
}

async fn git_stage(State(state): State<ServiceState>, body: Bytes) -> Response {
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let Some(resolved) = resolve_body_dir(&state, &body) else {
        return bad_request("invalid dir");
    };
    if !is_git_work_tree(&resolved.dir).await {
        return bad_request("not a git repo");
    }
    plain_result_response(stage_paths(&resolved.dir, body.get("paths")).await)
}

async fn git_unstage(State(state): State<ServiceState>, body: Bytes) -> Response {
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let Some(resolved) = resolve_body_dir(&state, &body) else {
        return bad_request("invalid dir");
    };
    if !is_git_work_tree(&resolved.dir).await {
        return bad_request("not a git repo");
    }
    plain_result_response(unstage_paths(&resolved.dir, body.get("paths")).await)
}

async fn git_amend(State(state): State<ServiceState>, body: Bytes) -> Response {
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let Some(resolved) = resolve_body_dir(&state, &body) else {
        return bad_request("invalid dir");
    };
    if !is_git_work_tree(&resolved.dir).await {
        return bad_request("not a git repo");
    }
    let message = body.get("message").and_then(Value::as_str);
    match amend_commit(&resolved.dir, message).await {
        Ok(result) => plain_result_response(result),
        Err(error) => failure_response(&error),
    }
}

async fn git_fetch(State(state): State<ServiceState>, body: Bytes) -> Response {
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let Some(resolved) = resolve_body_dir(&state, &body) else {
        return bad_request("invalid dir");
    };
    if !is_git_work_tree(&resolved.dir).await {
        return bad_request("not a git repo");
    }
    plain_result_response(fetch_remote(&resolved.dir).await)
}

async fn git_ai_commit_message(State(state): State<ServiceState>, body: Bytes) -> Response {
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let Some(resolved) = resolve_body_dir(&state, &body) else {
        return bad_request("invalid dir");
    };
    if !is_git_work_tree(&resolved.dir).await {
        return bad_request("not a git repo");
    }
    let include_unstaged = body.get("includeUnstaged") != Some(&Value::Bool(false));
    let context = collect_change_context(&resolved.dir, include_unstaged).await;
    if context.is_empty() {
        return bad_request("no changes");
    }
    let (executor, operation) = match summarize_slot(&state) {
        Ok(pair) => pair,
        Err(SummarizeSlotError::Unavailable) => {
            return json_response(
                StatusCode::SERVICE_UNAVAILABLE,
                json!({"error": "llm unavailable"}),
            )
        }
        Err(SummarizeSlotError::Slot(message)) => {
            return json_response(StatusCode::INTERNAL_SERVER_ERROR, json!({"error": message}))
        }
    };
    let agent = resolved
        .agent_id
        .clone()
        .or_else(|| desk_files::current_agent_id(&state))
        .unwrap_or_else(|| "unknown".to_string());
    let deadline = state.clock().now_unix_ms() + AI_MESSAGE_TIMEOUT_MS;
    let request = AuxiliaryRequest {
        prompt: context,
        images: Vec::new(),
        max_output_tokens: Some(500),
        deadline_unix_ms: Some(deadline),
        system_prompt: Some(AI_COMMIT_SYSTEM_PROMPT.to_string()),
        prior: Vec::new(),
        tools: None,
    };
    match call_summarize_text(&state, &agent, &executor, operation, &request).await {
        Ok(raw) => {
            let message = sanitize_commit_message(&raw);
            if message.is_empty() {
                return json_response(
                    StatusCode::BAD_GATEWAY,
                    json!({"error": "ai produced empty message"}),
                );
            }
            json_response(StatusCode::OK, json!({"message": message}))
        }
        Err(error) => json_response(StatusCode::INTERNAL_SERVER_ERROR, json!({"error": error})),
    }
}

pub fn router() -> Router<ServiceState> {
    Router::new()
        .route("/lingxi/v1/git/status", get(git_status))
        .route("/lingxi/v1/git/worktree-info", get(git_worktree_info))
        .route("/lingxi/v1/git/worktrees", get(git_worktrees))
        .route("/lingxi/v1/git/branches", get(git_branches))
        .route("/lingxi/v1/git/log", get(git_log))
        .route("/lingxi/v1/git/log-stats", post(git_log_stats))
        .route("/lingxi/v1/git/file-diff", get(git_file_diff))
        .route("/lingxi/v1/git/checkout", post(git_checkout))
        .route("/lingxi/v1/git/create-branch", post(git_create_branch))
        .route("/lingxi/v1/git/stash", post(git_stash))
        .route("/lingxi/v1/git/stashes", get(git_stashes))
        .route("/lingxi/v1/git/unstash", post(git_unstash))
        .route("/lingxi/v1/git/discard", post(git_discard))
        .route("/lingxi/v1/git/worktree-create", post(git_worktree_create))
        .route("/lingxi/v1/git/commit", post(git_commit))
        .route("/lingxi/v1/git/push", post(git_push))
        .route("/lingxi/v1/git/pull", post(git_pull))
        .route("/lingxi/v1/git/stage", post(git_stage))
        .route("/lingxi/v1/git/unstage", post(git_unstage))
        .route("/lingxi/v1/git/amend", post(git_amend))
        .route("/lingxi/v1/git/fetch", post(git_fetch))
        .route(
            "/lingxi/v1/git/ai-commit-message",
            post(git_ai_commit_message),
        )
}

// ── 纯函数单测（现役 git-command.ts 解析器的 vitest 口径）──

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_numstat_z_handles_binary_and_plain() {
        // 与现役 vitest fixture 平价（\u0000 分隔；第三记录 10/0/b.md）
        let entries = parse_numstat_z("3\t1\tsrc/a.rs\u{0}-\t-\tbin/logo.png\u{0}10\t0\tb.md\u{0}");
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].additions, 3);
        assert_eq!(entries[0].deletions, 1);
        assert!(!entries[0].binary);
        assert!(entries[1].binary);
        assert_eq!(entries[1].additions, 0);
        assert_eq!(entries[2].additions, 10);
        assert_eq!(entries[2].deletions, 0);
        assert!(!entries[2].binary);
        assert_eq!(entries[2].path, "b.md");
    }

    #[test]
    fn parse_name_status_z_pairs_fields() {
        let entries = parse_name_status_z("A\0new.rs\0M\0old.rs\0X\0ignored.rs\0");
        assert_eq!(
            entries,
            vec![
                ("new.rs".to_string(), "A".to_string()),
                ("old.rs".to_string(), "M".to_string())
            ]
        );
    }

    #[test]
    fn parse_log_records_attributes_numstat_to_previous_commit() {
        let record = |hash: &str, short: &str| {
            format!("{hash}\0{short}\0subject {short}\0body {short}\n\n多行正文\0作者\01700000000\0HEAD -> main, tag: v1, origin/main\0\0")
        };
        // 真实字节形状（git log --no-walk --numstat，不带 -z；已用真实 git
        // 2.x 输出取证）：记录以 \x1e\n 收尾，numstat 行纯 \n 分隔、无 NUL，
        // 块间再空一行；尾块归属最后一条已解析提交。
        let output = format!(
            "{}\u{1e}\n3\t2\tfile-a.rs\n\n{}\u{1e}\n1\t0\tfile-b.rs\n",
            record("a".repeat(40).as_str(), "aaaaaaa"),
            record("b".repeat(40).as_str(), "bbbbbbb")
        );
        let commits = parse_log_records(&output);
        assert_eq!(commits.len(), 2);
        // 第一条提交的统计来自第二个 \x1e 块的开头
        assert_eq!(commits[0].additions, 3);
        assert_eq!(commits[0].deletions, 2);
        assert_eq!(commits[0].changed_files, 1);
        // 最后一条的统计是纯 numstat 尾块
        assert_eq!(commits[1].additions, 1);
        assert_eq!(commits[1].changed_files, 1);
        assert_eq!(commits[0].message, "body aaaaaaa\n\n多行正文");
        assert_eq!(commits[0].refs.len(), 3);
        assert_eq!(commits[0].refs[0].kind, "head");
        assert_eq!(commits[0].refs[0].name, "main");
        assert_eq!(commits[0].refs[1].kind, "tag");
        assert_eq!(commits[0].refs[2].kind, "remote");
        assert!(commits[0].parents.is_empty());
    }

    #[test]
    fn parse_worktree_porcelain_blocks() {
        let output = "worktree /repo/main\nHEAD abc123\nbranch refs/heads/main\n\nworktree /repo/wt\nHEAD def456\ndetached\n\n";
        let entries = parse_worktree_porcelain(output);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].branch.as_deref(), Some("main"));
        assert_eq!(entries[0].head.as_deref(), Some("abc123"));
        assert!(entries[1].detached);
        assert!(entries[1].branch.is_none());
    }

    #[test]
    fn branch_and_worktree_name_validation() {
        assert!(is_valid_branch_name("feat/x-1"));
        assert!(!is_valid_branch_name("-n"));
        assert!(!is_valid_branch_name("a b"));
        assert!(!is_valid_branch_name("a..b"));
        assert!(!is_valid_branch_name("/abs"));
        assert!(is_valid_worktree_name("wt-1.x_y"));
        assert!(!is_valid_worktree_name(".hidden"));
        assert!(!is_valid_worktree_name("a/b"));
    }

    #[test]
    fn safe_rel_path_rules() {
        assert!(is_safe_rel_path("src/a.rs"));
        assert!(!is_safe_rel_path("../x"));
        assert!(!is_safe_rel_path("a//b"));
        assert!(!is_safe_rel_path("a/./b"));
        assert!(!is_safe_rel_path("/abs"));
        assert!(!is_safe_rel_path("a\\b"));
        assert!(!is_safe_rel_path("-f"));
    }

    #[test]
    fn normalize_path_list_dedupes_and_rejects() {
        let (paths, invalid) = normalize_path_list(Some(&json!(["a.rs", "a.rs", "b.rs"])));
        assert!(!invalid);
        assert_eq!(paths.unwrap(), vec!["a.rs", "b.rs"]);
        let (_, invalid) = normalize_path_list(Some(&json!(["../escape"])));
        assert!(invalid);
        let (paths, invalid) = normalize_path_list(Some(&json!([])));
        assert!(!invalid && paths.is_none());
        let (paths, invalid) = normalize_path_list(Some(&json!("not-array")));
        assert!(!invalid && paths.is_none());
    }

    #[test]
    fn sanitize_commit_message_strips_fences_prefix_and_blanks() {
        let raw = "```markdown\ncommit: feat(git): 新增面板\n\n\n- 要点一  \n- 要点二\n```";
        assert_eq!(
            sanitize_commit_message(raw),
            "feat(git): 新增面板\n\n- 要点一\n- 要点二"
        );
        assert_eq!(sanitize_commit_message("```\n```"), "");
        assert_eq!(
            sanitize_commit_message("提交信息：fix: 修一下"),
            "fix: 修一下"
        );
    }

    #[test]
    fn locale_compare_approx_two_levels() {
        assert_eq!(
            locale_compare_approx("a.txt", "B.txt"),
            std::cmp::Ordering::Less
        );
        assert_eq!(locale_compare_approx("ab", "aB"), std::cmp::Ordering::Less);
        assert_eq!(
            locale_compare_approx("Z.md", "a.md"),
            std::cmp::Ordering::Greater
        );
        let mut files = vec!["z.rs".to_string(), "A.rs".to_string(), "b.rs".to_string()];
        files.sort_by(|a, b| locale_compare_approx(a, b));
        assert_eq!(files, vec!["A.rs", "b.rs", "z.rs"]);
    }

    #[test]
    fn utf16_take_counts_units() {
        assert_eq!(utf16_take("你好世界", 3), "你好世");
        assert_eq!(utf16_len("a🙂b"), 4);
        assert_eq!(utf16_take("短", 10), "短");
    }

    #[test]
    fn parse_int_semantics() {
        assert_eq!(parse_int_opt("12abc"), Some(12));
        assert_eq!(parse_int_opt("abc"), None);
        assert_eq!(parse_int_opt("  -7"), Some(-7));
        assert_eq!(parse_int_or_zero(""), 0);
    }

    #[test]
    fn lexical_normalize_drops_extra_parent_dirs() {
        assert_eq!(
            lexical_normalize(Path::new("/a/b/../c")),
            PathBuf::from("/a/c")
        );
        assert_eq!(
            lexical_normalize(Path::new("/a/../../b")),
            PathBuf::from("/b")
        );
        assert_eq!(
            lexical_normalize(Path::new("/a/./b/")),
            PathBuf::from("/a/b")
        );
    }
}
