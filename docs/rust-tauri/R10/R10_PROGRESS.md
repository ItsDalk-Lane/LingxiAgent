# R10 功能补齐执行记录

> 状态：`IMPLEMENTATION_ONLY_TESTS_DEFERRED`
> 计划：[R10_IMPLEMENTATION_PLAN.md](R10_IMPLEMENTATION_PLAN.md)
> 延后台账：[R10_DEFERRED_TESTS.md](R10_DEFERRED_TESTS.md)
> T03 定性：[R10_T03_TRIAGE.md](R10_T03_TRIAGE.md)
> Chromium 决策：[R10_CHROMIUM_DECISION.md](R10_CHROMIUM_DECISION.md)
> 本文件只记录实际完成的实现与自检；**不声明 R10 验收通过**。

## 1. 注册面（Lead）

- [x] `desktop/src-tauri/Cargo.toml`：reqwest `rustls-no-provider` + `rustls`/`ring`，uuid v4；
      `Cargo.lock` 同步。
- [x] `desktop/src-tauri/build.rs`：命令清单 +8。
- [x] `desktop/src-tauri/src/lib.rs`：`update_about` / `observability_export` 模块、state、
      两套 `generate_handler!`、Destroyed 清理；`__cmd__*` 宏要求裸函数名（已按此注册）。
- [x] `capabilities/r09-host.json`（main）与 `settings.json`：8 个 `allow-*`。
- [x] `tauri.release.conf.json`：打包 `release-digest.v1.json` / `v2.json`。
- [x] `tauri-host-capabilities.ts`：4+4 成员实现；8 条受限理由移除；
      auto-update/OTA 理由改为发布通道/签名裁决。
- [x] `R10_COMMAND_POLICY.json`：R09 策略派生，8 命令入注册集，自动更新/OTA 保留 deferred；
      R09 原策略未改。

## 2. WS1 About 更新链

- [x] `desktop/src-tauri/src/update_about.rs`（新文件）
- [x] release check：固定 GitHub、12s/64KB/60s cache、error 降级
- [x] update digest history：releases + release-digest.v1 资产、10min cache、bundled v2→v1 fallback
- [x] pending announcement / ack：书签播种/`(书签, 当前]` 切片/原子写
- [x] 纯函数单测（版本比较/digest 归一化/切片/窗口门等）

## 3. WS2 可观测导出

- [x] `desktop/src-tauri/src/observability_export.rs`（新文件）
- [x] begin 保存对话框 + 文件名消毒 + exportId 绑定窗口
- [x] write base64/≤4MB/owner-only；end/flush；abort 删除半成品
- [x] Destroyed 清理；会话逻辑单测 7 例

## 4. WS3 T03 修复

- [x] splash main 页面门（OWNER-04；origin/index.html 精确页门不放宽）
- [x] keep-awake `c"PreventUserIdleSystemSleep"` 断言类型
- [x] shortcut test-register self（`plan_test_register(raw, current)`）
- [x] window control `windowId` 注入显式拒绝
- [x] `window_theme_changed` 主题形态/白名单校验（dark/light/auto/system）
- [x] `show_notification` focus policy 四别名兼容
- [x] 通知偏好 PUT 未知键/非法值 400 不落盘（lingxi-service）
- [x] `R10_T03_TRIAGE.md`：v39-native-03 的 27 条 `ok=false` 全部分类
      （产品修复 8 / harness-only 15 / manual-GUI 3 / deferred 1；不重跑、不声称转绿）

## 4b. R10 追加补齐（业主 2026-10-27 授权）

- [x] 外观设置异常内容校验（`userprefs.rs`）：theme 非字符串/空/控制字符/超长
      在合并写入前 400 拒绝，`preferences.json` 不再落坏值；未知主题 id 仍允许。
      单测 `appearance_patch_requires_safe_theme_string` 通过。
- [x] 交付版固定“文件任务”入口：欢迎页新增 `data-testid="file-task-entry"` 按钮，
      经 `file-task-actions` 桥复用输入区真实的附件选择/快照流程；五语言文案补齐。
      单测 `desktop/src/react/__tests__/stores/file-task-actions.test.ts` 3/3 通过。
- [x] 随包 Chromium：`scripts/fetch-chromium.cjs` 固定 revision/SHA-256、解压后
      ad-hoc 重签；`tauri.release.conf.json` 资源映射；宿主启动前注入
      `LINGXI_CHROMIUM_PATH`。详见 [R11/CHROMIUM_BUNDLE.md](../R11/CHROMIUM_BUNDLE.md)。

## 5. WS4 构建确定性

- [x] Vite post HTML 插件 `hana-sort-injected-css-links`：只排序 Vite 注入的
      `./assets/*.css` 链接；legacy `styles.css`/`themes/*.css` 顺序不变。
- [x] 独立两次构建：`settings.html` 均 `7aa849dd9047f7b9…`；7 个 HTML 页面全部一致。

## 6. 自检结果（实现自检，不是 R10 验收）

| 命令 | 结果 |
|---|---|
| `cargo check --manifest-path desktop/src-tauri/Cargo.toml --features r09-host` | PASS |
| `cargo test --manifest-path desktop/src-tauri/Cargo.toml --features r09-host --lib` | **267 passed / 0 failed** |
| `cargo check --manifest-path rust/Cargo.toml -p lingxi-service` | PASS |
| `cargo test -p lingxi-service --lib notification_patch_whitelist` | 1 passed |
| `npx tsc --noEmit` | PASS |
| 两次 `vite build` 到 /tmp 对比 | settings.html 与其余 6 页逐字节一致 |
| `git diff --check` | 无空白错误 |
| `cargo fmt --check`（desktop） | 仅 3 处**既有**格式差异（lib.rs:123 / native_commands.rs:893 / resource_bridge.rs:96），本轮新增代码无格式差异 |

未执行：R10 A01–A16、T01–T08、R09 formal gate、e2e/UI、安装包、签名、性能、迁移、
真实供应商、三平台、自动更新/OTA、Chromium 打包。

## 7. 未提交/未推送

本轮未 commit、未 push、未 tag、未 release。
