# R10 功能补齐执行计划（验收测试全部延后）

> 状态：`IMPLEMENTATION_ONLY_TESTS_DEFERRED`
> 依据：2026-10-27 业主指示「R10 阶段明确需要测试的项目全部延后，主要做功能实现」，
> 并在范围确认中选择 A 方案：补齐用户可见功能，不实现自动更新/OTA 与 Chromium 打包。
> 本文件是执行契约；R10 任务书 `T01–T08` / `A01–A16` 全部 DEFERRED，**不声称 R10 完成/通过**。

## 1. 完成标准

1. Tauri 宿主真实实现 8 个命令：
   - About 更新链：`release_check_latest`、`get_update_digest_history`、
     `get_pending_announcement`、`ack_announcement`；
   - 可观测导出：`observability_export_begin/write/end/abort`。
2. 修复已确认的 T03 宿主/服务缺陷（见 §4），并有模块级单元测试。
3. `settings.html` 两次独立构建字节一致（CSS link 顺序确定）。
4. 注册面（`build.rs`、`lib.rs`、capabilities、R10 命令策略、TS host 片段）同步；
   桌面宿主 `cargo check`、renderer `tsc --noEmit` 通过。
5. `docs/rust-tauri/R10/` 交付实现记录、T03 定性、延后台账、Chromium 决策、报告。
6. 不提交、不推送、不发布；不建 R10 stage map/正式门。

## 2. 范围

### 本轮做

| 工作流 | 内容 |
|---|---|
| WS1 | About 更新链 4 命令 |
| WS2 | 可观测导出 4 命令 |
| WS3 | T03 已确认产品缺陷修复 |
| WS4 | Vite 产物 CSS link 顺序确定性 |
| WS5 | 注册面、R10 命令策略、文档（Lead） |

### 明确延后

- R10-T01～T08 / A01–A16 全部验收、对抗、故障注入、迁移、性能、安装升级、独立复核。
- 自动更新与 OTA 列车剩余 7 命令及 installer/updater 通道（发布通道/签名裁决）。
- Chromium 打包/分发（沿用系统 Chrome）。
- T04 TCC、T05 Chromium PDF 运行时测试、T06 可见副作用批次、T07 文件任务自动点击、
  T08 parity 重跑、三平台、正式签名、final gate。
- 提交/推送/PR/tag/release。

## 3. WS1：About 更新链（新文件 `desktop/src-tauri/src/update_about.rs`）

公开接口（由 `lib.rs` 注册）：

```rust
pub struct UpdateAboutState { /* 60s release cache + 10min history cache */ }
impl Default for UpdateAboutState

#[tauri::command]
pub async fn release_check_latest(
    window: WebviewWindow,
    state: State<'_, UpdateAboutState>,
) -> Result<serde_json::Value, String>;

#[tauri::command]
pub async fn get_update_digest_history(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, UpdateAboutState>,
) -> Result<serde_json::Value, String>;

#[tauri::command]
pub fn get_pending_announcement(
    window: WebviewWindow,
    app: AppHandle,
) -> Result<serde_json::Value, String>;

#[tauri::command]
pub fn ack_announcement(window: WebviewWindow, app: AppHandle) -> Result<(), String>;
```

行为契约：

1. **窗口门**：只允许 `main` / `settings`（`crate::native::window_labels`）。
   非法窗口返回 `WINDOW_NOT_ALLOWED: ...`（字符串错误，前端 hook 会转 error 态）。
2. `release_check_latest`：
   - 固定 `https://api.github.com/repos/ItsDalk-Lane/LingxiAgent/releases/latest`，
     不接受 URL/仓库参数；
   - 12s 超时、64KB 响应上限、60s 进程内缓存；
   - 返回 `{status:"latest"|"available"|"error", latestVersion?, releaseUrl?, error?}`；
   - 网络/HTTP/JSON/tag 不可解析 → `status:"error"`（不抛 Err、不落盘）。
3. `get_update_digest_history`：
   - 固定 releases 列表 `?per_page=20&page=1`；
   - 过滤 `draft=false`、tag 匹配 `^v\d+\.\d+\.\d+$`、assets 含 `release-digest.v1.json`；
   - 最多扫描 10 个 release，最多返回 5 条；资产 URL 固定为
     `https://github.com/ItsDalk-Lane/LingxiAgent/releases/download/{tag}/release-digest.v1.json`；
   - 10 分钟进程内缓存；
   - 失败回退包内 `release-digest.v2.json`（`entries`），再回退 `release-digest.v1.json`
     单条；返回 `{entries, source:"online"|"bundled"|"none", complete}`；
   - 包内文件读取顺序：`app.path().resource_dir()` → `app.path().app_dir()` →
     `CARGO_MANIFEST_DIR/../..`（仅 dev/证据构建兜底）。
4. `get_pending_announcement`：
   - 读 `{LINGXI_HOME}/user/last-seen-version.json`（`{"version": "x.y.z"}`）；
   - packaged-like = `!cfg!(debug_assertions)` 或 `LINGXI_FORCE_ANNOUNCEMENT=1`；
   - `setupComplete` 读 `{LINGXI_HOME}/user/preferences.json` 的 `setupComplete === true`；
   - 非 packaged-like / 无版本 → null；无书签且未 setup → 静默播种当前版本并返回 null；
     无书签但已 setup → 只取当前版本一条；有书签 → 取 `(书签, 当前]`，新→旧；
   - 返回 null 或 `{version, entries:[ReleaseDigest...]}`；当前版本用
     `app.package_info().version.to_string()`（Tauri 无内容 OTA，文档记录该边界）。
5. `ack_announcement`：原子写 `{version}`（同目录临时文件 + rename），失败显式 Err。
6. Digest 归一化与 Electron `normalizeReleaseDigest` 同形：保留
   schemaVersion/tag/version/previousTag/generatedAt/noUserFacingChanges/summary/counts/items。
   纯函数放 `pub(crate)` 供单测。

依赖：Lead 在 `desktop/src-tauri/Cargo.toml` 把 reqwest 增加 rustls TLS 特性；
不新增其它 HTTP 客户端。

## 4. WS2：可观测导出（新文件 `desktop/src-tauri/src/observability_export.rs`）

```rust
pub struct ObservabilityExportState { /* HashMap<exportId, Session> */ }
impl Default for ObservabilityExportState

#[tauri::command]
pub async fn observability_export_begin(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, ObservabilityExportState>,
    default_file_name: Option<String>,
) -> Result<serde_json::Value, String>;

#[tauri::command]
pub async fn observability_export_write(
    window: WebviewWindow,
    state: State<'_, ObservabilityExportState>,
    export_id: String,
    chunk: String, // base64；前端 adapter 负责 Uint8Array -> base64
) -> Result<serde_json::Value, String>;

#[tauri::command]
pub fn observability_export_end(
    window: WebviewWindow,
    state: State<'_, ObservabilityExportState>,
    export_id: String,
) -> Result<serde_json::Value, String>;

#[tauri::command]
pub fn observability_export_abort(
    window: WebviewWindow,
    state: State<'_, ObservabilityExportState>,
    export_id: String,
) -> Result<serde_json::Value, String>;

pub fn cleanup_window(app: &AppHandle, label: &str); // 由 lib.rs Destroyed 调用
```

行为：

- 窗口门：`main` / `settings`。
- `begin`：系统保存对话框（`tauri-plugin-dialog`）；默认名只接受
  `^[A-Za-z0-9._-]{1,128}$`，否则 `lingxi-model-observability.jsonl`；
  取消 `{canceled:true}`；成功 `{canceled:false, exportId, filePath}`；
  `exportId = Uuid::new_v4()`（新增 uuid v4 依赖），绑定发起窗口 label。
- `write`：只有同一窗口能写；未知/跨窗 `TOKEN_INVALID: unknown or foreign exportId`；
  base64 解码失败 `INVALID_ARGUMENT`；解码后 >4MB `INVALID_ARGUMENT`；
  返回 `{bytesWritten}`（累计）。
- `end`：关闭文件，返回 `{bytesWritten, filePath}`；未知/跨窗报错。
- `abort`：关闭并删除部分文件，返回 `{aborted:true}`；跨窗返回 `{aborted:false}`。
- 窗口销毁：删除该窗口全部半成品。
- 磁盘错误显式返回错误，不静默。

## 5. WS3：T03 已确认缺陷

| 文件 | 修复 |
|---|---|
| `native/splash.rs` + `native_commands.rs` | main 窗口允许 `get_splash_info`/`get_avatar_path`：main 只允许精确 `index.html` + 可信来源；splash/onboarding 原页面门不变；仍拒绝伪造 origin/非法 query |
| `native/install.rs` | macOS keep-awake 的 `PreventUserIdleSystemSleep` 改为 NUL 结尾 `c"..."`；失败保持 enabled=false |
| `native/global_shortcut.rs` + `native_commands.rs` | `plan_test_register` 接收当前注册表；本宿主已注册 → `selfRegistered:true`，不重复注册；冲突仍不可用 |
| `commands.rs` | `window_minimize/maximize/close/is_maximized` 增加可选 `window_id`；出现即 `reject_window_id_argument` 拒绝 |
| `commands.rs` | `window_theme_changed` 只允许 `dark/light/auto/system`；非法输入返回错误且不 `set_theme` |
| `native_commands.rs` | `show_notification` 兼容 `desktopFocusPolicy`/`desktop_focus_policy`/`focusPolicy`/`focus_policy` |
| `rust/crates/lingxi-service/src/automation.rs` | 通知偏好 PUT patch 白名单（chatCompletion/turnCompletion/scheduledTaskCompletion/patrolCompletion）+ 值校验；未知键 400 不落盘；保留既有未知历史字段 |

其余 T03 失败项由 `native-fixes` 写入 `R10_T03_TRIAGE.md` 定性（工具面/人工/GUI 项延后，
不通过改断言或跳过变绿），不重跑 T03。

## 6. WS4：构建确定性

`vite.config.ts` 新增 post HTML 插件：只排序 Vite 注入的 `<link rel="stylesheet" href="./assets/*.css">`，
保留 `styles.css` / `themes/*.css` 等 legacy 链接原顺序。验证两次
`vite build --outDir /tmp/...` 的 `settings.html` 字节一致。

## 7. WS5：Lead 注册与文档

- `build.rs` 命令清单 +8；`lib.rs` 两套 `generate_handler!` + `pub mod` + `.manage(...)`
  + Destroyed 清理。
- `capabilities/r09-host.json` / `settings.json` 增加 8 个 `allow-*`。
- `docs/rust-tauri/R10/R10_COMMAND_POLICY.json`（R09 派生；R09 原策略不动）。
- TS `desktop/src/react/host/tauri-host-capabilities.ts`：
  实现 4+4 成员；移除对应 `TAURI_CAPABILITY_REASONS`；auto-update/train 理由改为
  “发布通道/签名裁决未授权，本轮不实现”。
- `tauri.release.conf.json` 打包 `release-digest.v1.json` / `v2.json`。
- 文档：`R10_PROGRESS.md`、`R10_T03_TRIAGE.md`、`R10_DEFERRED_TESTS.md`、
  `R10_CHROMIUM_DECISION.md`、`R10_REPORT.md`。

## 8. 验证（实现自检，不是 R10 验收）

```bash
~/.cargo/bin/rustup run 1.98.1 cargo check --manifest-path desktop/src-tauri/Cargo.toml --features r09-host
~/.cargo/bin/rustup run 1.98.1 cargo test --manifest-path desktop/src-tauri/Cargo.toml --features r09-host --lib
~/.cargo/bin/rustup run 1.98.1 cargo check --manifest-path rust/Cargo.toml -p lingxi-service
npx tsc --noEmit
# 两次 vite build 到 /tmp 并比较 settings.html
git status --short
```

不执行：R10 A01–A16、T01–T08、verify-stage、e2e/UI、安装包、签名、性能、迁移、真实供应商、三平台。
