//! R10 WS1：About 更新链（Tauri 宿主侧 4 命令）。
//!
//! 现役 Electron 语义分散在三个共享模块里，本文件是它们的 Tauri 宿主实现：
//! - 检查更新：`desktop/src/shared/github-release-check.cjs`（GitHub Releases
//!   是发布单一事实源；失败一律降级成 `{status:"error"}`，不抛、不落盘）；
//! - 更新历史：`desktop/src/shared/update-digest-history.cjs`（在线最近五个
//!   已发布版本，失败显式回退随包 `release-digest.v2.json` / `.v1.json`）；
//! - 升级后首启公告：`desktop/src/shared/post-update-announcement.cjs`
//!   （`last-seen-version.json` 是唯一书签；只展示 `(书签, 当前]` 区间）。
//!
//! # 安全边界
//!
//! - **窗口门**：只有 `main` / `settings` 能调用（About 页所在的窗口）。其它
//!   窗口即使拿到 invoke 权限也只能得到 `WINDOW_NOT_ALLOWED: ...`，不能读取
//!   更新状态或写公告书签。
//! - **固定端点**：GitHub owner/repo 与 API/资产 URL 全部硬编码，命令不接受
//!   URL/仓库/路径参数——renderer 无法把宿主变成任意 HTTP 客户端或任意文件读。
//! - **资源上限**：release check 12s 超时 + 64KB 响应上限；历史列表
//!   512KB、单个 digest 128KB，最多扫描 10 个 release、最多返回 5 条，并有
//!   20s 总预算——恶意/异常响应不能把宿主内存或 About 页拖死。
//! - **书签原子写**：`{LINGXI_HOME}/user/last-seen-version.json` 用同目录临时
//!   文件 + rename，避免半截 JSON；失败显式 Err（`ANNOUNCEMENT_IO: ...`）。
//! - 端点只走 HTTPS（`https_only`），阻止重定向把请求降级成明文。
//!
//! # 已知边界（随计划记录，不在本文件修复）
//!
//! Tauri 宿主没有内容 OTA：公告的“当前版本”只能取
//! `app.package_info().version`（壳版本），而 Electron 用已激活内容版本。
//! R10 不做内容 OTA，因此这里按计划 §3.4 的裁决使用壳版本。
//!
//! # 状态与纯函数
//!
//! [`UpdateAboutState`] 只放两个进程内缓存（release 60s / history 10min），
//! 锁只在命令入口/出口短暂持有，绝不在 await 期间持有。
//! 版本解析/比较、digest 归一化、合订本切片、书签路径等全部是无 I/O 的纯
//! 函数（`pub(crate)`），`#[cfg(test)]` 单测不访问网络。

use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tauri::{AppHandle, Manager, State, WebviewWindow};

use crate::native::{install, window_labels};

// ── 固定 GitHub 端点（与 package.json 的 publish 配置一致；不接受参数）──

/// Settings > About「检查更新」最新 release 端点。
const RELEASES_LATEST_API: &str =
    "https://api.github.com/repos/ItsDalk-Lane/LingxiAgent/releases/latest";
/// 更新历史的 release 列表端点。
const RELEASES_LIST_API: &str =
    "https://api.github.com/repos/ItsDalk-Lane/LingxiAgent/releases?per_page=20&page=1";
/// release digest 资产下载基址；`{tag}/release-digest.v1.json` 由固定规则拼接。
const RELEASE_ASSET_BASE: &str = "https://github.com/ItsDalk-Lane/LingxiAgent/releases/download";
/// 在线历史只认这个资产名。
const DIGEST_ASSET_NAME: &str = "release-digest.v1.json";
/// 包内史册（schema 2，多版本合订）。
const DIGEST_V2_NAME: &str = "release-digest.v2.json";
/// 包内单版兜底（schema 1，单个 digest 对象）。
const DIGEST_V1_NAME: &str = "release-digest.v1.json";

/// 检查更新单次请求超时（Electron `REQUEST_TIMEOUT_MS`）。
const RELEASE_REQUEST_TIMEOUT: Duration = Duration::from_secs(12);
/// 检查更新响应上限 64KB（releases/latest 返回体远小于列表）。
const RELEASE_MAX_BYTES: usize = 64 * 1024;
/// 检查更新结果缓存 60s（Electron `RELEASE_CHECK_CACHE_TTL_MS`）。
const RELEASE_CACHE_TTL: Duration = Duration::from_secs(60);

/// 更新历史单请求超时（Electron `REQUEST_TIMEOUT_MS`）。
const HISTORY_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// 在线历史整体预算：最坏情况下最多 10 个资产请求，不让 About 页无限转圈；
/// 超预算或任何失败都显式回退包内史册（`source` 标注 `bundled`）。
const HISTORY_TOTAL_BUDGET: Duration = Duration::from_secs(20);
/// releases 列表响应上限（Electron `MAX_RELEASES_BODY_CHARS`）。
const RELEASES_LIST_MAX_BYTES: usize = 512 * 1024;
/// 单个 release digest 响应上限（Electron `MAX_DIGEST_BODY_CHARS`）。
const DIGEST_MAX_BYTES: usize = 128 * 1024;
/// 在线历史最多扫描的 release 数（Electron `RELEASE_SCAN_LIMIT`）。
const RELEASE_SCAN_LIMIT: usize = 10;
/// 历史/公告最多返回的条目数（Electron `HISTORY_LIMIT`）。
const HISTORY_LIMIT: usize = 5;
/// 更新历史结果缓存 10min（Electron `CACHE_TTL_MS`）；只缓存成功在线结果。
const HISTORY_CACHE_TTL: Duration = Duration::from_secs(10 * 60);

/// GitHub API 要求的 User-Agent（不设置会被 403）。
const RELEASE_USER_AGENT: &str = "LingxiAgent-release-check";
const HISTORY_USER_AGENT: &str = "LingxiAgent-update-history";

/// 公告书签写入失败的稳定机器码；前端当前忽略 ack 拒绝，诊断/证据按 code 断言。
pub(crate) const E_ANNOUNCEMENT_IO: &str = "ANNOUNCEMENT_IO";

// ── 进程内缓存 ─────────────────────────────────────────────────────────

struct TimedValue {
    stored_at: Instant,
    value: Value,
}

/// About 更新链的进程内缓存；由 `lib.rs` 的 `.manage(...)` 托管。
///
/// 两把 `std::sync::Mutex` 只在读取/写入缓存时持有，await 期间一定已释放；
/// 中毒（panic 过的线程）按 `into_inner` 恢复，不让一次渲染层异常永久废掉更新链。
#[derive(Default)]
pub struct UpdateAboutState {
    release_cache: Mutex<Option<TimedValue>>,
    history_cache: Mutex<Option<TimedValue>>,
}

impl UpdateAboutState {
    fn cached_release(&self, ttl: Duration) -> Option<Value> {
        read_cache(&self.release_cache, ttl)
    }

    fn store_release(&self, value: Value) {
        store_cache(&self.release_cache, value);
    }

    fn cached_history(&self, ttl: Duration) -> Option<Value> {
        read_cache(&self.history_cache, ttl)
    }

    fn store_history(&self, value: Value) {
        store_cache(&self.history_cache, value);
    }
}

fn read_cache(cache: &Mutex<Option<TimedValue>>, ttl: Duration) -> Option<Value> {
    let guard = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard
        .as_ref()
        .filter(|cached| cached.stored_at.elapsed() < ttl)
        .map(|cached| cached.value.clone())
}

fn store_cache(cache: &Mutex<Option<TimedValue>>, value: Value) {
    let mut guard = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = Some(TimedValue {
        stored_at: Instant::now(),
        value,
    });
}

// ── 命令面 ─────────────────────────────────────────────────────────────

/// 检查 GitHub 最新 release 并与当前壳版本比较。
///
/// 返回 `{status:"latest"|"available"|"error", ...}`：网络/HTTP/JSON/tag 失败
/// 一律 `status:"error"`（不抛、不落盘），与 `github-release-check.cjs` 同形。
/// 60s 缓存包含失败结果（与 Electron 主进程一致：用户连点不会狂打 GitHub API）。
#[tauri::command]
pub async fn release_check_latest(
    window: WebviewWindow,
    state: State<'_, UpdateAboutState>,
) -> Result<Value, String> {
    authorize_update_window(&window)?;
    if let Some(cached) = state.cached_release(RELEASE_CACHE_TTL) {
        return Ok(cached);
    }
    let current_version = window.package_info().version.to_string();
    let result = fetch_release_check_result(&current_version).await;
    state.store_release(result.clone());
    Ok(result)
}

/// 最近五个已发布版本的更新历史。
///
/// 在线失败（网络/结构/无有效 digest）回退包内史册，返回
/// `{entries, source:"online"|"bundled"|"none", complete}`；`source` 明确标注
/// 当前数据来自在线还是离线备份，前端据此提示——不是静默降级。
#[tauri::command]
pub async fn get_update_digest_history(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, UpdateAboutState>,
) -> Result<Value, String> {
    authorize_update_window(&window)?;
    if let Some(cached) = state.cached_history(HISTORY_CACHE_TTL) {
        return Ok(cached);
    }
    // 总预算超时同样视为在线失败 → 包内回退（只缓存成功在线结果，
    // 与 Electron loader 一致：离线回退不缓存，下次调用还会重试在线）。
    let online = tokio::time::timeout(HISTORY_TOTAL_BUDGET, load_online_history()).await;
    if let Ok(Some(result)) = online {
        state.store_history(result.clone());
        return Ok(result);
    }
    Ok(bundled_history_result(&app))
}

/// 升级后首启公告：`(书签, 当前]` 区间的随包 digest 史册切片，新→旧。
///
/// 返回 `null` 或 `{version, entries}`。契约见 `post-update-announcement.cjs`：
/// 非打包环境不弹也不写；全新安装静默播种当前版本；无书签但已 setup 的老用户
/// 只取当前版本一节。
#[tauri::command]
pub fn get_pending_announcement(window: WebviewWindow, app: AppHandle) -> Result<Value, String> {
    authorize_update_window(&window)?;
    let current_version = app.package_info().version.to_string();
    let home = install::lingxi_home();
    let last_seen = read_bookmark_version(&home);
    let decision = resolve_post_update_announcement(
        Some(current_version.as_str()),
        last_seen.as_deref(),
        is_packaged_like(),
        read_setup_complete(&home),
    );
    if let Some(seed_version) = decision.seed_version {
        // 播种失败不向 renderer 抛：公告组件没有 catch，Electron 语义也是
        // 静默播种（wrapIpcBestEffortHandler）。但按红线显式记录，便于诊断。
        if let Err(error) = write_bookmark_atomic(&home, &seed_version) {
            eprintln!("[update-about] seed last-seen-version failed: {error}");
        }
        return Ok(Value::Null);
    }
    if !decision.pending {
        return Ok(Value::Null);
    }
    let entries = read_bundled_digest_entries(&app);
    let sliced = slice_digest_history(&entries, last_seen.as_deref(), &current_version);
    Ok(json!({
        "version": current_version,
        "entries": sliced,
    }))
}

/// 确认公告：把当前版本原子写进唯一书签 `last-seen-version.json`。
///
/// 失败显式返回 `ANNOUNCEMENT_IO: ...`（不静默吞掉磁盘错误）。
#[tauri::command]
pub fn ack_announcement(window: WebviewWindow, app: AppHandle) -> Result<(), String> {
    authorize_update_window(&window)?;
    let current_version = app.package_info().version.to_string();
    write_bookmark_atomic(&install::lingxi_home(), &current_version)
}

// ── 窗口门 ─────────────────────────────────────────────────────────────

/// 调用窗口门：About 更新链只服务 `main` / `settings`。
///
/// 用精确 label 白名单（而不是“主窗角色”），未来新增窗口不会意外获得更新链
/// 权限；错误串带 `WINDOW_NOT_ALLOWED:` / `WINDOW_LABEL_INVALID:` 稳定前缀。
fn authorize_update_window(window: &WebviewWindow) -> Result<(), String> {
    authorize_update_window_label(window.label())
}

/// 窗口 label 门（纯逻辑，单测覆盖）。
pub(crate) fn authorize_update_window_label(label: &str) -> Result<(), String> {
    window_labels::require_exact_label(label, &[window_labels::MAIN, window_labels::SETTINGS])
        .map_err(|error| error.to_string())
}

// ── 检查更新（release_check_latest）────────────────────────────────────

/// 纯映射：GitHub release JSON + 当前版本 → `ReleaseCheckResult` 同形 JSON。
///
/// 与 `github-release-check.cjs` 逐条对齐：
/// - tag 不是版本号 → error（不能静默当成“已是最新”）；
/// - 当前版本不可解析 → error；
/// - 最新版本 > 当前 → available，否则（含相等/更旧）→ latest。
pub(crate) fn release_check_value(release: &Value, current_version: &str) -> Value {
    let tag_name = release
        .get("tag_name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let latest_version = tag_name.strip_prefix('v').unwrap_or(tag_name);
    let release_url = release
        .get("html_url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|url| !url.is_empty());
    if parse_version_triplet(latest_version).is_none() {
        return json!({ "status": "error", "error": "unparsable tag" });
    }
    let Some(ordering) = compare_versions(latest_version, current_version) else {
        return json!({ "status": "error", "error": "current version unparsable" });
    };
    let status = if ordering == Ordering::Greater {
        "available"
    } else {
        "latest"
    };
    json!({
        "status": status,
        "latestVersion": latest_version,
        "releaseUrl": release_url,
    })
}

/// 查最新 release；任何失败都折成 `{status:"error"}`，本函数不返回 Err。
async fn fetch_release_check_result(current_version: &str) -> Value {
    let client = match build_client(RELEASE_REQUEST_TIMEOUT, RELEASE_USER_AGENT) {
        Ok(client) => client,
        // TLS/解析器初始化失败属于宿主环境问题，仍按协议给 error 态。
        Err(_) => return json!({ "status": "error", "error": "client unavailable" }),
    };
    match fetch_json_capped(&client, RELEASES_LATEST_API, RELEASE_MAX_BYTES).await {
        Ok(release) => release_check_value(&release, current_version),
        Err(error) => json!({
            "status": "error",
            "error": error.release_check_message(),
        }),
    }
}

// ── 更新历史（get_update_digest_history）──────────────────────────────

/// 在线加载历史：列表 → 过滤候选 → 逐个取 digest → 最多 5 条。
///
/// 任一环节失败返回 `None` 交给包内回退，不在命令层抛 Err（About 页要能显示
/// “离线备份”而不是错误空页）。
async fn load_online_history() -> Option<Value> {
    let client = build_client(HISTORY_REQUEST_TIMEOUT, HISTORY_USER_AGENT).ok()?;
    let releases = fetch_json_capped(&client, RELEASES_LIST_API, RELEASES_LIST_MAX_BYTES)
        .await
        .ok()?;
    let mut entries: Vec<Value> = Vec::with_capacity(HISTORY_LIMIT);
    for tag in history_candidate_tags(&releases) {
        if entries.len() >= HISTORY_LIMIT {
            break;
        }
        // 候选已通过 `^v\d+\.\d+\.\d+$` 校验，下面两步仍用 Option 防御，
        // 保证任何畸形列表都不会让宿主 panic。
        let Some(version) = version_from_tag(&tag) else {
            continue;
        };
        let Some(url) = digest_asset_url(&tag) else {
            continue;
        };
        let Ok(payload) = fetch_json_capped(&client, &url, DIGEST_MAX_BYTES).await else {
            // 单个资产缺失/损坏只影响这一条（Electron 同样逐条吞掉）。
            continue;
        };
        if let Some(normalized) = normalize_release_digest(&payload, Some(version)) {
            entries.push(normalized);
        }
    }
    if entries.is_empty() {
        return None;
    }
    let complete = entries.len() == HISTORY_LIMIT;
    Some(json!({
        "entries": entries,
        "source": "online",
        "complete": complete,
    }))
}

/// 更新历史候选 release 的 tag 列表（纯函数）。
///
/// 过滤顺序与 `update-digest-history.cjs` 一致：非 draft → tag 形状合法 →
/// assets 含 `release-digest.v1.json`；最多 [`RELEASE_SCAN_LIMIT`] 个。
pub(crate) fn history_candidate_tags(releases: &Value) -> Vec<String> {
    let Some(items) = releases.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter(|release| release.get("draft").and_then(Value::as_bool) == Some(false))
        .filter_map(|release| {
            let tag = release.get("tag_name").and_then(Value::as_str)?;
            if version_from_tag(tag).is_none() || !has_digest_asset(release) {
                return None;
            }
            Some(tag.to_string())
        })
        .take(RELEASE_SCAN_LIMIT)
        .collect()
}

/// assets 中是否含 [DIGEST_ASSET_NAME]（纯函数）。
pub(crate) fn has_digest_asset(release: &Value) -> bool {
    release
        .get("assets")
        .and_then(Value::as_array)
        .is_some_and(|assets| {
            assets
                .iter()
                .any(|asset| asset.get("name").and_then(Value::as_str) == Some(DIGEST_ASSET_NAME))
        })
}

/// 固定 digest 资产 URL（纯函数）：只有合法 `vX.Y.Z` tag 才能拼进去。
///
/// tag 来自网络响应，必须在这里重新校验后拼 URL：拒绝 `/`、`%`、`?`、`#`
/// 等任何可能改变“固定仓库+固定资产”语义的字符（URL/路径注入）。
pub(crate) fn digest_asset_url(tag: &str) -> Option<String> {
    let trimmed = tag.trim();
    if version_from_tag(trimmed).is_none() {
        return None;
    }
    Some(format!(
        "{RELEASE_ASSET_BASE}/{trimmed}/{DIGEST_ASSET_NAME}"
    ))
}

/// 包内史册回退结果（`source` 显式标注 bundled/none）。
fn bundled_history_result(app: &AppHandle) -> Value {
    let entries: Vec<Value> = read_bundled_digest_entries(app)
        .into_iter()
        .take(HISTORY_LIMIT)
        .collect();
    let source = if entries.is_empty() {
        "none"
    } else {
        "bundled"
    };
    json!({
        "entries": entries,
        "source": source,
        "complete": false,
    })
}

/// 读包内 digest 史册：`resource_dir` → `executable_dir` → 仓库根（仅 dev/证据构建）。
///
/// 每个目录先试 `release-digest.v2.json` 的 `entries`，再试 v1 单版对象；
/// 结果归一化并按版本新→旧排序。全部目录都读不到时返回空数组。
fn read_bundled_digest_entries(app: &AppHandle) -> Vec<Value> {
    for dir in bundled_digest_dirs(app) {
        let [v2_name, v1_name] = bundled_digest_file_names();
        let v2 = read_json_file(&dir.join(v2_name));
        let v1 = read_json_file(&dir.join(v1_name));
        let mut entries =
            normalize_digest_entries(&coerce_digest_history(v2.as_ref(), v1.as_ref()));
        if !entries.is_empty() {
            sort_digest_entries_newest_first(&mut entries);
            return entries;
        }
    }
    Vec::new()
}

/// 包内文件搜索目录（顺序即优先级）。
fn bundled_digest_dirs(app: &AppHandle) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(dir) = app.path().resource_dir() {
        dirs.push(dir);
    }
    // 计划里的 `app_dir()` 在 Tauri 2 没有同名 API：Electron `app.getAppPath()`
    // 的这段语义（应用安装目录）用可执行文件目录承接，资源目录缺失时仍能找到
    // 与可执行文件同放的备份。
    if let Ok(dir) = app.path().executable_dir() {
        dirs.push(dir);
    }
    // 打包配置在 tauri.release.conf.json 里把仓库根的两个文件复制到资源目录；
    // 仓库根只在 dev / 证据构建里作为兜底（`desktop/src-tauri/../..`）。
    dirs.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(".."));
    dirs
}

// ── 公告决策（纯函数）────────────────────────────────────────────────

/// 公告决策结果：`seed_version` 非空表示“静默播种，不弹公告”。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AnnouncementDecision {
    pub pending: bool,
    pub seed_version: Option<String>,
}

/// 与 `resolvePostUpdateAnnouncement` 同义的纯决策。
///
/// 规则：
/// - 非打包环境 → 不弹也不写；
/// - 当前版本缺失 → 不弹；
/// - 书签 == 当前版本 → 不弹；
/// - 无书签且未完成 setup（全新安装）→ 静默播种当前版本；
/// - 其余（含无书签但已 setup 的老用户升级）→ pending。
pub(crate) fn resolve_post_update_announcement(
    current_version: Option<&str>,
    last_seen_version: Option<&str>,
    is_packaged_like: bool,
    setup_complete: bool,
) -> AnnouncementDecision {
    if !is_packaged_like {
        return AnnouncementDecision {
            pending: false,
            seed_version: None,
        };
    }
    let Some(current_version) = current_version.filter(|version| !version.is_empty()) else {
        return AnnouncementDecision {
            pending: false,
            seed_version: None,
        };
    };
    if last_seen_version == Some(current_version) {
        return AnnouncementDecision {
            pending: false,
            seed_version: None,
        };
    }
    // 书签缺失或为空串都算“无书签”（Electron 的 `!lastSeenVersion`）。
    let has_bookmark = last_seen_version.is_some_and(|version| !version.is_empty());
    if !has_bookmark && !setup_complete {
        return AnnouncementDecision {
            pending: false,
            seed_version: Some(current_version.to_string()),
        };
    }
    AnnouncementDecision {
        pending: true,
        seed_version: None,
    }
}

/// packaged-like = release 构建，或开发期显式 `LINGXI_FORCE_ANNOUNCEMENT=1`。
fn is_packaged_like() -> bool {
    resolve_packaged_like(
        cfg!(debug_assertions),
        std::env::var("LINGXI_FORCE_ANNOUNCEMENT").is_ok_and(|value| value == "1"),
    )
}

/// 纯逻辑：`!debug_build || force_env`（与 Electron `app.isPackaged || env==="1"` 同义）。
pub(crate) fn resolve_packaged_like(debug_build: bool, force_env: bool) -> bool {
    !debug_build || force_env
}

// ── Digest 归一化 / 合订本切片（纯函数，与 Electron 共享模块同形）──────

/// 与 Electron `normalizeReleaseDigest` 同形：保留
/// schemaVersion/tag/version/previousTag/generatedAt/noUserFacingChanges/
/// summary/counts/items，并给缺失字段补同一套默认值。
///
/// `expected_version` 对应 Electron 的第二参数：download/公告场景传期望版本，
/// 版本与 tag 都不匹配视为无效；历史列表传 `None`。
pub(crate) fn normalize_release_digest(
    value: &Value,
    expected_version: Option<&str>,
) -> Option<Value> {
    let object = value.as_object()?;
    // JS `value.schemaVersion !== 1`：JSON 里 1 与 1.0 都是同一个数。
    if object.get("schemaVersion").and_then(Value::as_f64) != Some(1.0) {
        return None;
    }
    let tag = object.get("tag").and_then(Value::as_str)?;
    let version = object.get("version").and_then(Value::as_str)?;
    if let Some(expected) = expected_version {
        if version != expected && tag != tag_from_version(expected) {
            return None;
        }
    }
    let summary = object.get("summary")?;
    if !is_localized_text(summary) {
        return None;
    }
    let counts = object.get("counts");
    let items: Vec<Value> = object
        .get("items")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(normalize_digest_item).collect())
        .unwrap_or_default();
    Some(json!({
        "schemaVersion": 1,
        "tag": tag,
        "version": version,
        "previousTag": object.get("previousTag").and_then(Value::as_str).unwrap_or(""),
        "generatedAt": object.get("generatedAt").and_then(Value::as_str).unwrap_or(""),
        "noUserFacingChanges": js_truthy(object.get("noUserFacingChanges")),
        "summary": summary.clone(),
        "counts": {
            "feature": json_integer(counts.and_then(|counts| counts.get("feature"))),
            "fix": json_integer(counts.and_then(|counts| counts.get("fix"))),
            "improvement": json_integer(counts.and_then(|counts| counts.get("improvement"))),
            "migration": json_integer(counts.and_then(|counts| counts.get("migration"))),
        },
        "items": items,
    }))
}

/// 单条 item 归一化（Electron 同规则：title/summary 必须双语，details 逐条过滤，
/// sources 原样保留数组）。
fn normalize_digest_item(item: &Value) -> Option<Value> {
    let object = item.as_object()?;
    let title = object.get("title")?;
    let summary = object.get("summary")?;
    if !is_localized_text(title) || !is_localized_text(summary) {
        return None;
    }
    let details: Vec<Value> = object
        .get("details")
        .and_then(Value::as_array)
        .map(|details| {
            details
                .iter()
                .filter(|detail| is_localized_text(detail))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let sources = match object.get("sources") {
        Some(Value::Array(sources)) => Value::Array(sources.clone()),
        _ => Value::Array(Vec::new()),
    };
    Some(json!({
        "id": object.get("id").and_then(Value::as_str).unwrap_or(""),
        "kind": object.get("kind").and_then(Value::as_str).unwrap_or("improvement"),
        "importance": object.get("importance").and_then(Value::as_str).unwrap_or("medium"),
        "title": title.clone(),
        "summary": summary.clone(),
        "details": details,
        "sources": sources,
    }))
}

/// 双语文本形状：`{zh: string, en: string}`（Electron `isLocalizedText`）。
pub(crate) fn is_localized_text(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    object.get("zh").and_then(Value::as_str).is_some()
        && object.get("en").and_then(Value::as_str).is_some()
}

/// `normalizeReleaseDigest(entry, null)` 的批量版本（过滤无效条目）。
pub(crate) fn normalize_digest_entries(entries: &[Value]) -> Vec<Value> {
    entries
        .iter()
        .filter_map(|entry| normalize_release_digest(entry, None))
        .collect()
}

/// 包内 v2/v1 读取兼容（Electron `coerceDigestHistory`）：
/// v2 `{schema:2, entries:[...]}` 非空优先，否则 v1 单版对象作为单条史册。
pub(crate) fn coerce_digest_history(v2: Option<&Value>, v1: Option<&Value>) -> Vec<Value> {
    if let Some(v2) = v2 {
        let is_v2 = v2.as_object().is_some()
            && v2.get("schema").and_then(Value::as_f64) == Some(2.0)
            && v2
                .get("entries")
                .and_then(Value::as_array)
                .is_some_and(|entries| !entries.is_empty());
        if is_v2 {
            return v2
                .get("entries")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
        }
    }
    // v1 必须是对象；数组/标量都不算单版 digest（Electron 同判）。
    if let Some(v1) = v1.filter(|value| value.is_object()) {
        return vec![v1.clone()];
    }
    Vec::new()
}

/// 包内史册按版本新→旧排序（无法解析版本的条目保持相对顺序；稳定排序）。
pub(crate) fn sort_digest_entries_newest_first(entries: &mut [Value]) {
    entries.sort_by(
        |left, right| match (entry_version(left), entry_version(right)) {
            (Some(left_version), Some(right_version)) => {
                compare_product_versions(right_version, left_version).unwrap_or(Ordering::Equal)
            }
            _ => Ordering::Equal,
        },
    );
}

fn entry_version(entry: &Value) -> Option<&str> {
    let version = entry.get("version")?.as_str()?;
    parse_product_version(version).map(|_| version)
}

/// 合订本切片（Electron `sliceDigestHistory`）：返回 `(lastSeen, current]`
/// 区间内全部条目，新→旧；无书签/书签损坏时只返回当前版本一节；
/// 版本高于当前的条目（史册超前于本地安装）一律排除。
pub(crate) fn slice_digest_history(
    entries: &[Value],
    last_seen_version: Option<&str>,
    current_version: &str,
) -> Vec<Value> {
    let Some(current) = parse_product_version(current_version) else {
        return Vec::new();
    };
    let mut usable: Vec<(Value, [u64; 3])> = entries
        .iter()
        .filter(|entry| entry.is_object())
        .filter_map(|entry| {
            let version = entry.get("version")?.as_str()?;
            let parsed = parse_product_version(version)?;
            (parsed <= current).then(|| (entry.clone(), parsed))
        })
        .collect();
    // 新→旧（Electron 的比较器 `compare(b.version, a.version)`）。
    usable.sort_by(|(_, left), (_, right)| right.cmp(left));
    match last_seen_version.and_then(parse_product_version) {
        // 无书签 / 书签损坏：只展示当前版本一节，避免第一次升级就追溯轰炸。
        None => usable
            .into_iter()
            .filter(|(_, version)| *version == current)
            .map(|(entry, _)| entry)
            .collect(),
        Some(marker) => usable
            .into_iter()
            .filter(|(_, version)| *version > marker)
            .map(|(entry, _)| entry)
            .collect(),
    }
}

// ── 版本解析/比较（与两个 Electron 模块逐条同义）─────────────────────

/// 与 `github-release-check.cjs:parseVersionTriplet` 同义：容忍 `v` 前缀，
/// 前两段必须纯数字，第三段取前导数字（`1.2.3-beta` → `[1,2,3]`）。
pub(crate) fn parse_version_triplet(version: &str) -> Option<[u64; 3]> {
    let text = version.trim();
    let text = text.strip_prefix('v').unwrap_or(text);
    let first_dot = text.find('.')?;
    let major = digits_only(&text[..first_dot])?;
    let rest = &text[first_dot + 1..];
    let second_dot = rest.find('.')?;
    let minor = digits_only(&rest[..second_dot])?;
    let patch = leading_digits(&rest[second_dot + 1..])?;
    Some([major, minor, patch])
}

/// 与 `post-update-announcement.cjs:parseProductVersion` 同义：容忍 `v` 前缀，
/// 但必须是完整 `major.minor.patch`（不允许 prerelease 后缀/多余段）。
pub(crate) fn parse_product_version(version: &str) -> Option<[u64; 3]> {
    let text = version.trim();
    let text = text.strip_prefix('v').unwrap_or(text);
    let mut parts = text.split('.');
    let major = digits_only(parts.next()?)?;
    let minor = digits_only(parts.next()?)?;
    let patch = digits_only(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    Some([major, minor, patch])
}

/// 三段式比较（`github-release-check.cjs:compareVersions`）；任一侧不可解析返回 None。
pub(crate) fn compare_versions(left: &str, right: &str) -> Option<Ordering> {
    Some(parse_version_triplet(left)?.cmp(&parse_version_triplet(right)?))
}

/// 三段式比较（`post-update-announcement.cjs:compareProductVersions`）。
pub(crate) fn compare_product_versions(left: &str, right: &str) -> Option<Ordering> {
    Some(parse_product_version(left)?.cmp(&parse_product_version(right)?))
}

/// 与 digest 里的 `tag` 对齐（Electron `tagFromVersion`）：无 `v` 前缀时补上。
pub(crate) fn tag_from_version(version: &str) -> String {
    let value = version.trim();
    if value.starts_with('v') {
        value.to_string()
    } else {
        format!("v{value}")
    }
}

/// 在线历史要求 tag 严格是 `vX.Y.Z`；返回去掉 `v` 的版本段供归一化校验。
pub(crate) fn version_from_tag(tag: &str) -> Option<&str> {
    let trimmed = tag.trim();
    let version = trimmed.strip_prefix('v')?;
    parse_product_version(version).map(|_| version)
}

fn digits_only(value: &str) -> Option<u64> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse::<u64>().ok()
}

fn leading_digits(value: &str) -> Option<u64> {
    let end = value
        .bytes()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    if end == 0 {
        return None;
    }
    // 前 `end` 个字节都是 ASCII 数字，切分点在字符边界上。
    value[..end].parse::<u64>().ok()
}

// ── 书签 / 偏好读写 ───────────────────────────────────────────────────

/// 公告唯一书签路径（Electron `lastSeenVersionPath`）。
pub(crate) fn last_seen_version_path(home: &Path) -> PathBuf {
    home.join("user").join("last-seen-version.json")
}

/// 用户偏好路径（`setupComplete` 判定来源）。
pub(crate) fn preferences_path(home: &Path) -> PathBuf {
    home.join("user").join("preferences.json")
}

/// 包内史册文件名（顺序即回退优先级）。
pub(crate) fn bundled_digest_file_names() -> [&'static str; 2] {
    [DIGEST_V2_NAME, DIGEST_V1_NAME]
}

fn read_json_file(path: &Path) -> Option<Value> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// 读书签：只认 `{"version": "<非空字符串>"}`；缺失/损坏一律视为无书签。
fn read_bookmark_version(home: &Path) -> Option<String> {
    let value = read_json_file(&last_seen_version_path(home))?;
    let version = value.get("version")?.as_str()?;
    if version.is_empty() {
        return None;
    }
    Some(version.to_string())
}

/// `preferences.json.setupComplete === true`（严格布尔，与 Electron 同判）。
fn read_setup_complete(home: &Path) -> bool {
    read_json_file(&preferences_path(home))
        .map(|value| value.get("setupComplete").and_then(Value::as_bool) == Some(true))
        .unwrap_or(false)
}

/// 原子写书签：同目录临时文件 + rename，失败显式返回 `ANNOUNCEMENT_IO: ...`。
///
/// 不直接覆盖目标文件：公告书签是“看到哪一版”的唯一状态，写一半的 JSON 会让
/// 用户下次启动被当成无书签（可能追溯轰炸或丢公告），所以先完整落临时文件。
fn write_bookmark_atomic(home: &Path, version: &str) -> Result<(), String> {
    static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    let target = last_seen_version_path(home);
    let parent = target
        .parent()
        .ok_or_else(|| format!("{E_ANNOUNCEMENT_IO}: 公告书签路径没有父目录"))?;
    fs::create_dir_all(parent).map_err(|error| {
        format!(
            "{E_ANNOUNCEMENT_IO}: 无法创建公告书签目录 {}: {error}",
            parent.display()
        )
    })?;
    let payload = serde_json::to_vec(&json!({ "version": version }))
        .map_err(|error| format!("{E_ANNOUNCEMENT_IO}: 无法序列化公告书签: {error}"))?;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let sequence = TEMP_SEQUENCE.fetch_add(1, AtomicOrdering::Relaxed);
    let temp = parent.join(format!(
        ".last-seen-version.{}.{nanos}.{sequence}.tmp",
        std::process::id()
    ));
    if let Err(error) = fs::write(&temp, &payload) {
        return Err(format!(
            "{E_ANNOUNCEMENT_IO}: 无法写公告书签临时文件 {}: {error}",
            temp.display()
        ));
    }
    if let Err(error) = fs::rename(&temp, &target) {
        // 失败时清掉临时文件；清理失败不再覆盖原始错误。
        let _ = fs::remove_file(&temp);
        return Err(format!(
            "{E_ANNOUNCEMENT_IO}: 无法替换公告书签 {}: {error}",
            target.display()
        ));
    }
    Ok(())
}

// ── HTTP 边界 ─────────────────────────────────────────────────────────

/// 安装 rustls ring provider（进程级一次）。
///
/// 依赖由 Lead 在 Cargo.toml 里配置为 `reqwest` 的 `rustls-no-provider` +
/// 直接依赖 `rustls(default-features=false, features=["std","tls12","ring"])`：
/// 不安装 provider 时 reqwest 构建 Client 会 panic（它的 fallback 分支是
/// panic 而不是 Err），所以必须在任何 `Client::builder()` 之前显式安装。
/// 重复安装返回 Err 等价于“已就绪”，不视为失败。
fn ensure_crypto_provider() {
    static INSTALL: OnceLock<()> = OnceLock::new();
    INSTALL.get_or_init(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// 构造固定端点用的 HTTP client：整体超时 + UA + 强制 HTTPS。
fn build_client(
    timeout: Duration,
    user_agent: &'static str,
) -> Result<reqwest::Client, reqwest::Error> {
    ensure_crypto_provider();
    reqwest::Client::builder()
        .timeout(timeout)
        .user_agent(user_agent)
        // 固定端点必须 HTTPS；即使服务端 30x 也不能把请求降级成明文。
        .https_only(true)
        .build()
}

/// 有界 JSON 请求失败原因（用于给 renderer 生成稳定、不泄内部细节的 error 文案）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FetchError {
    Timeout,
    Network,
    Http(u16),
    TooLarge,
    Json,
}

impl FetchError {
    /// 与 `github-release-check.cjs` 的用户可见 error 字符串一致。
    fn release_check_message(self) -> String {
        match self {
            Self::Timeout => "timeout".to_string(),
            Self::Network => "network".to_string(),
            Self::Http(status) => format!("request failed ({status})"),
            Self::TooLarge => "request failed (too-large)".to_string(),
            // Electron 里 JSON.parse 抛错也被外层 catch 归为 network。
            Self::Json => "network".to_string(),
        }
    }
}

fn map_request_error(error: &reqwest::Error) -> FetchError {
    if error.is_timeout() {
        FetchError::Timeout
    } else {
        FetchError::Network
    }
}

/// GET 固定 HTTPS URL 并按上限读取 JSON。非 2xx / 超限 / 解析失败都是错误，
/// 由调用方决定降级形态（release check → error 态；history → 包内回退）。
async fn fetch_json_capped(
    client: &reqwest::Client,
    url: &str,
    max_bytes: usize,
) -> Result<Value, FetchError> {
    let response = client
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|error| map_request_error(&error))?;
    let status = response.status();
    if !status.is_success() {
        return Err(FetchError::Http(status.as_u16()));
    }
    let body = read_body_capped(response, max_bytes).await?;
    serde_json::from_slice(&body).map_err(|_| FetchError::Json)
}

/// 流式读取响应体并严格执行上限。
///
/// 先看声明长度（大响应直接拒绝，不下载），再逐块累计——chunked 响应没有
/// Content-Length，不能只信 header；上限同时约束内存与 About 页等待时间。
async fn read_body_capped(
    mut response: reqwest::Response,
    max_bytes: usize,
) -> Result<Vec<u8>, FetchError> {
    if let Some(length) = response.content_length() {
        if length > max_bytes as u64 {
            return Err(FetchError::TooLarge);
        }
    }
    let mut body: Vec<u8> = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if body.len().saturating_add(chunk.len()) > max_bytes {
                    return Err(FetchError::TooLarge);
                }
                body.extend_from_slice(&chunk);
            }
            Ok(None) => return Ok(body),
            Err(error) => return Err(map_request_error(&error)),
        }
    }
}

// ── 小工具（Electron 语义等价）────────────────────────────────────────

/// JS `Boolean(value)` 的 JSON 等价（`normalizeReleaseDigest` 的
/// `noUserFacingChanges: Boolean(value.noUserFacingChanges)`）。
fn js_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number.as_f64().is_none_or(|number| number != 0.0),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(_) | Value::Object(_)) => true,
    }
}

/// JS `Number.isInteger(value) ? value : 0` 的 JSON 等价（counts 默认值）。
fn json_integer(value: Option<&Value>) -> i64 {
    value
        .and_then(|value| {
            value
                .as_i64()
                .or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
                .or_else(|| {
                    value
                        .as_f64()
                        .filter(|number| number.fract() == 0.0)
                        .map(|number| number as i64)
                })
        })
        .unwrap_or(0)
}

// ── 单测（纯函数，无网络/无 tauri App）───────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn localized(zh: &str, en: &str) -> Value {
        json!({ "zh": zh, "en": en })
    }

    fn digest(version: &str) -> Value {
        json!({
            "schemaVersion": 1,
            "tag": format!("v{version}"),
            "version": version,
            "previousTag": "",
            "generatedAt": "",
            "noUserFacingChanges": false,
            "summary": localized("摘要", "summary"),
            "counts": { "feature": 1 },
            "items": [],
        })
    }

    fn versions(entries: &[Value]) -> Vec<&str> {
        entries
            .iter()
            .map(|entry| entry["version"].as_str().unwrap_or(""))
            .collect()
    }

    #[test]
    fn version_triplet_matches_electron_prefix_rules() {
        assert_eq!(parse_version_triplet("0.1.2"), Some([0, 1, 2]));
        assert_eq!(parse_version_triplet("v1.2.3"), Some([1, 2, 3]));
        assert_eq!(parse_version_triplet(" 1.2.3 "), Some([1, 2, 3]));
        // 前缀匹配：第三段后的后缀被忽略，第四段也不影响。
        assert_eq!(parse_version_triplet("1.2.3-beta.1"), Some([1, 2, 3]));
        assert_eq!(parse_version_triplet("1.2.3.4"), Some([1, 2, 3]));
        // 前两段必须完整数字；缺段/非数字都不可解析。
        for invalid in ["", "1", "1.2", "v", "abc", "1.a.3", "1..3", "1.2."] {
            assert_eq!(parse_version_triplet(invalid), None, "{invalid:?}");
        }
    }

    #[test]
    fn product_version_requires_full_match() {
        assert_eq!(parse_product_version("0.1.43"), Some([0, 1, 43]));
        assert_eq!(parse_product_version("v0.1.43"), Some([0, 1, 43]));
        for invalid in ["", "0.1", "0.1.43-beta", "0.1.43.1", "v0.1", "0.x.1"] {
            assert_eq!(parse_product_version(invalid), None, "{invalid:?}");
        }
        // 与 JS `String(version).trim()` 一致：首尾空白会被裁掉。
        assert_eq!(parse_product_version(" 0.1.43 "), Some([0, 1, 43]));
    }

    #[test]
    fn version_comparison_is_numeric_and_none_on_garbage() {
        assert_eq!(compare_versions("1.2.3", "1.2.3"), Some(Ordering::Equal));
        assert_eq!(compare_versions("1.10.0", "1.9.9"), Some(Ordering::Greater));
        assert_eq!(compare_versions("v0.1.2", "0.1.10"), Some(Ordering::Less));
        assert_eq!(
            compare_versions("1.2.3-beta", "1.2.3"),
            Some(Ordering::Equal)
        );
        assert_eq!(compare_versions("dev", "1.2.3"), None);
        assert_eq!(
            compare_product_versions("1.2.3", "1.2.3"),
            Some(Ordering::Equal)
        );
        assert_eq!(compare_product_versions("1.2.3-beta", "1.2.3"), None);
    }

    #[test]
    fn normalize_release_digest_matches_electron_shape() {
        let payload = json!({
            "schemaVersion": 1,
            "tag": "v0.2.0",
            "version": "0.2.0",
            "previousTag": "v0.1.0",
            "generatedAt": "2026-10-01T00:00:00Z",
            "noUserFacingChanges": false,
            "summary": { "zh": "摘要", "en": "Summary" },
            "counts": { "feature": 2, "fix": 1 },
            "items": [
                {
                    "id": "a",
                    "kind": "fix",
                    "importance": "high",
                    "title": { "zh": "标题", "en": "Title" },
                    "summary": { "zh": "说明", "en": "Body" },
                    "details": [ { "zh": "细节", "en": "Detail" }, "not-localized" ],
                    "sources": [ { "type": "pr", "ref": "#1" } ]
                },
                { "title": "not-localized-item" }
            ]
        });
        let normalized =
            normalize_release_digest(&payload, Some("0.2.0")).expect("合法 digest 应通过");
        assert_eq!(normalized["schemaVersion"], 1);
        assert_eq!(normalized["tag"], "v0.2.0");
        assert_eq!(normalized["version"], "0.2.0");
        assert_eq!(normalized["previousTag"], "v0.1.0");
        assert_eq!(normalized["generatedAt"], "2026-10-01T00:00:00Z");
        assert_eq!(normalized["noUserFacingChanges"], false);
        assert_eq!(normalized["counts"]["feature"], 2);
        assert_eq!(normalized["counts"]["fix"], 1);
        // 缺失的 counts 键补 0（Electron 同默认）。
        assert_eq!(normalized["counts"]["improvement"], 0);
        assert_eq!(normalized["counts"]["migration"], 0);
        // 非双语 item 被过滤；details 里的非双语项也被过滤。
        let items = normalized["items"].as_array().expect("items 应是数组");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["details"].as_array().expect("details").len(), 1);
        assert_eq!(items[0]["sources"][0]["ref"], "#1");

        // 期望版本不匹配 → 无效；tag 命中期望版本时也接受。
        assert!(normalize_release_digest(&payload, Some("0.3.0")).is_none());
        let tag_only = json!({
            "schemaVersion": 1,
            "tag": "v0.2.0",
            "version": "0.2.0+build",
            "summary": localized("摘要", "summary"),
        });
        assert!(normalize_release_digest(&tag_only, Some("0.2.0")).is_some());
        // schemaVersion / summary 形状不对 → 无效。
        assert!(
            normalize_release_digest(&json!({ "schemaVersion": 2 }), None).is_none(),
            "schemaVersion 必须是 1"
        );
        let bad_summary = json!({
            "schemaVersion": 1,
            "tag": "v0.2.0",
            "version": "0.2.0",
            "summary": "not-localized",
        });
        assert!(normalize_release_digest(&bad_summary, None).is_none());
    }

    #[test]
    fn coerce_digest_history_prefers_v2_then_v1() {
        let v2 = json!({ "schema": 2, "entries": [digest("0.2.0"), digest("0.1.0")] });
        assert_eq!(coerce_digest_history(Some(&v2), None).len(), 2);

        // v2 结构不对/为空 → 回退 v1 单版对象。
        let empty_v2 = json!({ "schema": 2, "entries": [] });
        let v1 = digest("0.1.0");
        assert_eq!(coerce_digest_history(Some(&empty_v2), Some(&v1)).len(), 1);
        let wrong_schema = json!({ "schema": 1, "entries": [digest("0.2.0")] });
        assert_eq!(
            coerce_digest_history(Some(&wrong_schema), Some(&v1)).len(),
            1
        );

        // 两者都没有 / v1 不是对象 → 空史册。
        assert!(coerce_digest_history(None, None).is_empty());
        let v1_array = json!([digest("0.1.0")]);
        assert!(coerce_digest_history(None, Some(&v1_array)).is_empty());
    }

    #[test]
    fn bundled_entries_sort_and_normalize() {
        let mut entries = vec![digest("0.1.0"), digest("0.3.0"), digest("0.2.0")];
        sort_digest_entries_newest_first(&mut entries);
        assert_eq!(versions(&entries), vec!["0.3.0", "0.2.0", "0.1.0"]);

        let raw = vec![digest("0.2.0"), json!({ "version": "not-a-digest" })];
        assert_eq!(normalize_digest_entries(&raw).len(), 1);
    }

    #[test]
    fn slice_digest_history_handles_bookmarks() {
        let entries = vec![
            digest("0.3.0"),
            digest("0.2.0"),
            digest("0.1.0"),
            // 史册超前于本地安装：高于当前版本的条目必须排除。
            digest("0.4.0"),
            json!({ "version": "nightly" }),
        ];

        // 无书签：只取当前版本一节。
        assert_eq!(
            versions(&slice_digest_history(&entries, None, "0.3.0")),
            vec!["0.3.0"]
        );
        // 有书签：取 (书签, 当前]，新→旧。
        assert_eq!(
            versions(&slice_digest_history(&entries, Some("0.1.0"), "0.3.0")),
            vec!["0.3.0", "0.2.0"]
        );
        // 书签比史册最老条目还老：自然取到全部 ≤ 当前版本。
        assert_eq!(
            versions(&slice_digest_history(&entries, Some("0.0.9"), "0.3.0")),
            vec!["0.3.0", "0.2.0", "0.1.0"]
        );
        // 书签高于当前版本 → 空；当前版本不可解析 → 空。
        assert!(slice_digest_history(&entries, Some("0.5.0"), "0.3.0").is_empty());
        assert!(slice_digest_history(&entries, None, "dev").is_empty());
    }

    #[test]
    fn resolve_post_update_announcement_branches() {
        let pending = |last_seen: Option<&str>, setup: bool| {
            resolve_post_update_announcement(Some("0.3.0"), last_seen, true, setup)
        };
        // 非打包 → 不弹不写。
        assert_eq!(
            resolve_post_update_announcement(Some("0.3.0"), None, false, false),
            AnnouncementDecision {
                pending: false,
                seed_version: None
            }
        );
        // 当前版本缺失 → 不弹。
        assert_eq!(
            resolve_post_update_announcement(None, None, true, true),
            AnnouncementDecision {
                pending: false,
                seed_version: None
            }
        );
        assert_eq!(
            resolve_post_update_announcement(Some(""), None, true, true),
            AnnouncementDecision {
                pending: false,
                seed_version: None
            }
        );
        // 书签 == 当前 → 不弹。
        assert!(!pending(Some("0.3.0"), true).pending);
        // 全新安装（无书签、未 setup）→ 静默播种。
        assert_eq!(
            pending(None, false),
            AnnouncementDecision {
                pending: false,
                seed_version: Some("0.3.0".to_string())
            }
        );
        // 空串书签等同无书签。
        assert_eq!(
            pending(Some(""), false).seed_version,
            Some("0.3.0".to_string())
        );
        // 老用户升级（已 setup 但无书签）→ pending。
        assert!(pending(None, true).pending);
        // 有旧书签 → pending。
        assert!(pending(Some("0.2.0"), false).pending);
    }

    #[test]
    fn packaged_like_resolution_matches_electron() {
        assert!(!resolve_packaged_like(true, false), "debug 构建默认不弹");
        assert!(resolve_packaged_like(false, false), "release 构建视为打包");
        assert!(
            resolve_packaged_like(true, true),
            "LINGXI_FORCE_ANNOUNCEMENT=1 强制"
        );
    }

    #[test]
    fn release_check_value_maps_electron_status() {
        let release = json!({
            "tag_name": "v1.2.4",
            "html_url": " https://github.com/ItsDalk-Lane/LingxiAgent/releases/tag/v1.2.4 ",
        });
        let available = release_check_value(&release, "1.2.3");
        assert_eq!(available["status"], "available");
        assert_eq!(available["latestVersion"], "1.2.4");
        assert_eq!(
            available["releaseUrl"],
            "https://github.com/ItsDalk-Lane/LingxiAgent/releases/tag/v1.2.4"
        );
        assert_eq!(release_check_value(&release, "1.2.4")["status"], "latest");
        // 比当前旧也算 latest（Electron 明确这样处理，不当 error）。
        assert_eq!(release_check_value(&release, "1.2.5")["status"], "latest");
        // tag 不是版本号 → error，不能静默说“已是最新”。
        let bad_tag = json!({ "tag_name": "nightly" });
        assert_eq!(release_check_value(&bad_tag, "1.2.3")["status"], "error");
        assert_eq!(
            release_check_value(&bad_tag, "1.2.3")["error"],
            "unparsable tag"
        );
        // 当前版本不可解析 → error。
        assert_eq!(
            release_check_value(&release, "dev")["error"],
            "current version unparsable"
        );
        // 无 html_url → releaseUrl 为 null（前端按可空处理）。
        let no_url = json!({ "tag_name": "v1.2.4" });
        assert!(release_check_value(&no_url, "1.2.3")["releaseUrl"].is_null());
    }

    #[test]
    fn history_candidates_filter_draft_tag_and_assets() {
        let mut releases = vec![
            json!({ "tag_name": "v0.0.1", "draft": true, "assets": [{ "name": DIGEST_ASSET_NAME }] }),
            json!({ "tag_name": "nightly", "draft": false, "assets": [{ "name": DIGEST_ASSET_NAME }] }),
            json!({ "tag_name": "v0.0.2", "draft": false, "assets": [{ "name": "other.bin" }] }),
            json!({ "tag_name": "v0.0.3", "draft": false }),
        ];
        for index in 1..=12 {
            releases.push(json!({
                "tag_name": format!("v1.0.{index}"),
                "draft": false,
                "assets": [{ "name": DIGEST_ASSET_NAME }, { "name": "app.zip" }],
            }));
        }
        let tags = history_candidate_tags(&Value::Array(releases));
        assert_eq!(tags.len(), RELEASE_SCAN_LIMIT);
        assert_eq!(tags[0], "v1.0.1");
        assert_eq!(tags[9], "v1.0.10");
        // 非数组响应 → 无候选。
        assert!(history_candidate_tags(&json!({ "message": "nope" })).is_empty());
    }

    #[test]
    fn digest_asset_url_only_accepts_release_tags() {
        assert_eq!(
            digest_asset_url("v1.2.3").as_deref(),
            Some("https://github.com/ItsDalk-Lane/LingxiAgent/releases/download/v1.2.3/release-digest.v1.json")
        );
        // 网络响应里的 tag 不能改变 URL 结构（路径/查询/片段注入全部拒绝）。
        for invalid in [
            "1.2.3",
            "v1.2.3/../../evil",
            "v1.2.3?x=1",
            "v1.2.3#frag",
            "v1.2.3%2f..",
            "",
            "v1.2",
            "v1.2.3 ",
        ] {
            // 尾随/前导空白会先 trim，但 "v1.2.3 " 是合法 tag 形状。
            if invalid == "v1.2.3 " {
                assert!(digest_asset_url(invalid).is_some());
            } else {
                assert_eq!(digest_asset_url(invalid), None, "{invalid:?}");
            }
        }
    }

    #[test]
    fn window_gate_allows_only_main_and_settings() {
        assert!(authorize_update_window_label("main").is_ok());
        assert!(authorize_update_window_label("settings").is_ok());
        for denied in [
            "quick-chat",
            "browser-viewer",
            "splash",
            "onboarding",
            "viewer-1",
            "future-window",
            "Main",
        ] {
            let error = authorize_update_window_label(denied).expect_err("应拒绝非 About 窗口");
            assert!(
                error.starts_with("WINDOW_NOT_ALLOWED:"),
                "{denied:?} 应返回 WINDOW_NOT_ALLOWED，实际 {error}"
            );
        }
        // 语法非法（空格/控制字符）由 window_labels 的稳定 code 拒绝。
        let invalid = authorize_update_window_label("main window").expect_err("应拒绝非法 label");
        assert!(invalid.starts_with("WINDOW_LABEL_INVALID:"), "{invalid}");
    }

    #[test]
    fn bookmark_paths_are_fixed_under_lingxi_home() {
        let home = Path::new("/tmp/lingxi-home");
        assert_eq!(
            last_seen_version_path(home),
            home.join("user").join("last-seen-version.json")
        );
        assert_eq!(
            preferences_path(home),
            home.join("user").join("preferences.json")
        );
        assert_eq!(
            bundled_digest_file_names(),
            ["release-digest.v2.json", "release-digest.v1.json"]
        );
    }
}
