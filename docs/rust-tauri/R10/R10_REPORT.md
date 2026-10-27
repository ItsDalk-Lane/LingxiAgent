# R10 功能补齐报告（验收测试延后）

> 状态：`IMPLEMENTATION_ONLY_TESTS_DEFERRED`
> 结论：按业主 2026-10-27 指示，R10 的验收/对抗/故障注入/迁移/性能/安装升级/独立复核
> **全部延后**；本轮完成 A 方案的 Tauri 宿主功能补齐与已确认 T03 缺陷修复。
> **不声称 R10 完成、不声称 R10 验收通过。**

## 1. 本轮完成

### 1.1 About 更新链（4 命令）

新增 `desktop/src-tauri/src/update_about.rs`：

- `release_check_latest`：固定 GitHub `releases/latest`；12s 超时、64KB 上限、60s 缓存；
  网络/解析失败返回 `{status:"error"}`，不落盘、不伪造最新版本。
- `get_update_digest_history`：固定 releases 列表 + `release-digest.v1.json` 资产；
  最多扫描 10、返回 5；10min 缓存；失败显式回退包内 v2→v1，`source` 标注
  `online|bundled|none`。
- `get_pending_announcement` / `ack_announcement`：`{LINGXI_HOME}/user/last-seen-version.json`
  唯一书签；全新安装静默播种；老用户按 `(书签, 当前]` 切片；原子写。
- 仅 `main`/`settings` 可调用；端点/资产 URL 硬编码；HTTPS + 重定向下限。
- 依赖：reqwest `rustls-no-provider` + rustls ring provider（显式安装，避免 aws-lc-sys/cmake）。

### 1.2 可观测导出（4 命令）

新增 `desktop/src-tauri/src/observability_export.rs`：

- `begin` 系统保存对话框、文件名消毒、UUID v4 capability token 绑定发起窗口；
- `write` base64 → ≤4MB → 顺序 append；跨窗/未知 token 拒绝；
- `end` flush/close 返回累计字节与路径；`abort` 删除半成品；
- 窗口销毁清理半成品；磁盘/状态错误显式（`EXPORT_IO`/`EXPORT_STATE`），不静默降级。

### 1.3 T03 已确认缺陷修复

| 修复 | 文件 |
|---|---|
| main 窗口可读 splash 资料/头像（origin + 精确 index.html 页门仍保留） | `native/splash.rs`、`native_commands.rs` |
| keep-awake 断言类型改为 NUL 结尾 `c"PreventUserIdleSystemSleep"`（修复 -536870206） | `native/install.rs` |
| `keybindings_test_register` 识别宿主已注册 → `self`/`selfRegistered`，不重复注册 | `native/global_shortcut.rs`、`native_commands.rs` |
| 窗口控制四命令增加 `windowId` 显式拒绝，只作用调用者窗口 | `commands.rs` |
| `window_theme_changed` 主题形态/白名单校验，非法不 `set_theme` | `commands.rs` |
| `show_notification` 兼容 `desktopFocusPolicy` 等四个键名 | `native_commands.rs` |
| 通知偏好 PUT 白名单/类型/枚举校验，未知键 400 不落盘 | `rust/crates/lingxi-service/src/automation.rs` |

其余 T03 v39 失败项已在 [R10_T03_TRIAGE.md](R10_T03_TRIAGE.md) 逐条定性：
产品修复 8、harness-only 15、manual-GUI 3、deferred 1。**不重跑 T03，不声称任何 case 已转绿。**

### 1.4 构建确定性

`vite.config.ts` 新增 `hana-sort-injected-css-links`：只排序 Vite 注入的
`./assets/*.css` 链接；legacy `styles.css` / `themes/warm-paper.css` 保持在前，
script/modulepreload 与其它 markup 不动。

两次独立构建对比（独立复跑）：

- `settings.html`：`7aa849dd9047f7b919f81346c07b7d64f6ac041d7122ca53572426f2729359af` ×2
- `index.html` / `quick-chat.html` / `onboarding.html` / `browser-viewer.html` /
  `viewer-window.html` / `mobile.html`：两次逐字节一致

## 2. 验证

| 检查 | 结果 |
|---|---|
| desktop `cargo check --features r09-host` | PASS |
| desktop `cargo test --lib` | 267 passed / 0 failed |
| lingxi-service `cargo check` | PASS |
| lingxi-service `notification_patch_whitelist` 单测 | 1 passed |
| `npx tsc --noEmit` | PASS |
| 两次 renderer build 对比 | 7/7 HTML 页一致 |
| `git diff --check` | clean |

`cargo fmt --check`（desktop）仅剩 3 处**既有**格式差异（非本轮新增行）；
本轮新增代码由各实现者 rustfmt 检查。

## 3. 未验证/已知边界（如实）

1. **R10 验收测试全部未执行**：A01–A16、T01–T08、formal gate、e2e/UI、安装包、签名、
   性能、迁移、真实供应商、三平台。
2. **GUI 未跑**：About 页交互、保存对话框、系统通知、快捷键真实注册、keep-awake
   `pmset` 观测均未在本轮启动宿主验证。
3. **网络未实测**：GitHub release check / digest asset 拉取只有编译与纯函数测试，
   未在真实网络下调用；失败路径依赖代码审查。
4. **自动更新/OTA 未实现**：`auto_update_*` / `train_update_*` 仍受限，理由已改为
   发布通道/签名裁决（OWNER-03：不做正式签名、不声称自动更新）。
5. **Chromium 未打包**：沿用系统 Chrome；见 [R10_CHROMIUM_DECISION.md](R10_CHROMIUM_DECISION.md)。
6. **版本语义**：Tauri 无内容 OTA，公告/ack 使用壳版本；开发配置 `0.0.0` 是已知边界。
7. **`window_theme_changed` 只接受 dark/light/auto/system**：concrete 主题 id 未接原生
   appearance；当前 Tauri renderer 未通过该命令同步具体主题，后续接 `theme.ts` 时需补
   具体 id→dark/light 映射。
8. **窗口控制严格拒绝 vs R09 断言**：R09 `F9786414DD11` 期望注入参数被“忽略且 ok=true”；
   本轮按安全规格显式拒绝；harness 需调整（已在 T03 triage 登记）。
9. **Vite 排序**：模块 CSS 相对顺序改为 href 升序（此前本身非确定）；未做视觉回归。
10. **发布配置资源未打包验证**：`release-digest.*.json` 已加入 `tauri.release.conf.json`
    resources，但本轮未构建 release `.app` 验证装入位置。

## 3b. 追加补齐（业主 2026-10-27 授权）

- **外观设置异常内容**：`appearance_put_route` 在合并写入前校验 theme；非字符串/
  空/控制字符/超长返回 400，不再把坏值写进 `preferences.json`。未知主题 id
  仍允许（客户端主题表可扩展）。单测通过。
- **正式版文件任务入口**：欢迎页新增固定“文件任务”按钮，复用输入区实际附件
  选择/快照路径；五语言文案、`data-testid="file-task-entry"` 便于后续自动点击验收。
  桥接单测 3/3 通过。
- **随包浏览器组件**：`scripts/fetch-chromium.cjs` 固定 Chromium revision 与
  SHA-256，下载后 ad-hoc 重签；Tauri release resources + 宿主环境注入。
  详见 [R11/CHROMIUM_BUNDLE.md](../R11/CHROMIUM_BUNDLE.md)。
- **旧版应用生产链退出**：`package.json` 删除 Electron 入口/打包脚本/依赖/
  electron-builder 配置，Pi 运行时包移到 devDependencies；详见
  [R11/R11_OLD_CHAIN_RETIREMENT.md](../R11/R11_OLD_CHAIN_RETIREMENT.md)。
  旧内核源码尚未删除（790 个测试文件与 Node worker 仍依赖），R11 未完成。

## 4. 关键文件

- `desktop/src-tauri/src/update_about.rs`
- `desktop/src-tauri/src/observability_export.rs`
- `desktop/src-tauri/src/lib.rs`、`build.rs`、`Cargo.toml`、`capabilities/*.json`、
  `tauri.release.conf.json`
- `desktop/src/react/host/tauri-host-capabilities.ts`
- `desktop/src-tauri/src/native/{splash,install,global_shortcut}.rs`、
  `native_commands.rs`、`commands.rs`
- `rust/crates/lingxi-service/src/automation.rs`
- `vite.config.ts`
- `docs/rust-tauri/R10/R10_COMMAND_POLICY.json`

## 5. 后续（需新的批准/裁决）

1. 恢复 R10 验收测试并重建候选后复跑；
2. 裁决自动更新/OTA 发布通道与签名；
3. 裁决具体主题 id 的原生 appearance 映射；
4. 打包 release `.app` 验证 digest 资源装入；
5. 对 T03 triage 的 15 条 harness-only 做工具修复后重跑（属测试项）。
