# R09-T07 进度：跨 WebView 界面与原生 E2E

状态：**IN_PROGRESS / GUI 未启动**（无 GUI 骨架已就绪；任何窗口启动需 Lead cleared）
负责人：r09-host-core（Lead 指派）| 分支 `codex/rust-tauri-migration` | 验收：R09-A13 / R09-A14
证据根：`artifacts/rust-tauri/R09/dev/t07/`（无 GUI 自检当前在 `/private/tmp/t07-selfcheck/`）

## 1. 范围与已交付（无 GUI 阶段）

| 文件 | 内容 |
|---|---|
| `desktop/src-tauri/tests/t07_native_e2e.rs` + `t07_native_e2e_helpers.rs` | wdio WebDriver 驱动**产品主窗**（`LINGXI_R09_T07_APP_BIN` 注入 wrapper 产物；automation hidden+sink）；断言：产品壳加载、中文 IME 合成 composition、Range 选区、草稿存储可观察、滚动容器、DPI/visualViewport、焦点事件、wdio 截图；`t07-cases.json` 落证据。**已用 `--features r09-host,e2e-test` 编译通过（未运行）**。 |
| `scripts/rust-tauri/r09_t07.py` | 两套件：`r09_t07_native_e2e`（debug+e2e-test 构建、真实 cargo test、平台矩阵、46 叶映射）、`r09_t07_release_smoke`（release 无 e2e-test、strings 无 wdio/devUrl/5173、codesign、单发启动/连接/页面来源/优雅退出；文件任务在 T06 命令面注册前**显式红**不冒充）。**GUI 硬门**：无 `LINGXI_R09_ALLOW_GUI=1` 时两套件 exit 2（`GUI_NOT_CLEARED`），不启动窗口。 |
| 无 GUI 自检证据 | 未设置 marker 时两套件均 exit 2、cases 见 `gui-cleared=false`+`isolation::no-residual-processes=true`、ps 0（`/private/tmp/t07-selfcheck/`）。 |

## 2. 四平台矩阵（A13 environment）

| 平台 | 状态 | 说明 |
|---|---|---|
| macOS arm64 | **可实测**（本机 macOS 27.0.1 arm64，WKWebView） | 本机 GUI 批次唯一实跑平台；证据标 `macos-arm64` |
| macOS x64 | `BLOCKED_PLATFORM` | 无 x64 真机/VM；交叉编译不替代原生 E2E/安装验收 |
| Windows x64 | `BLOCKED_PLATFORM` | 无 Windows 真机；WebView2/DPI/字体/IME 未验证 |
| Linux x64 | `BLOCKED_PLATFORM` | 无 Linux 真机；WebKitGTK/输入法/驱动未验证 |

记录项：OS 版本/build、arch、kernel、WebKit CFBundleVersion、显示器（`system_profiler SPDisplaysDataType`）、构建回执（tauri-cli/toolchain/rendererDigest/themeBundleSha256/features）。

## 3. GUI 批次计划（需 Lead cleared，单发串行、失败即停）

| 批次 | 启动次数 | 步骤 | 断言 | 清理 |
|---|---|---|---|---|
| G0（无 GUI） | 0 | `node scripts/tauri-build.mjs --debug --features e2e-test --receipt …`；`cargo test --features r09-host,e2e-test --no-run` | 编译/回执 features | — |
| G1 native_e2e | **1**（cargo test 内 spawn 1 个产品 app） | 隔离 `LINGXI_HOME/TMPDIR`；`LINGXI_R09_AUTOMATION=1` + sink；`TAURI_WEBDRIVER_PORT` 随机；`cargo test --features r09-host,e2e-test --test t07_native_e2e -- --nocapture` | `t07-cases.json` 全 true；wdio DOM 断言；平台矩阵 | 测试 Drop kill app；套件 finally 清理 sidecar + ps 断言 0；sink 无内容 |
| G2 release_smoke | **1** | release `.app`（无 e2e-test）hidden+sink 启动 → 等 app-ready/service-ready → SIGTERM | strings 0 wdio/devUrl/5173；embedded page；exit 0；ps 0；文件任务（T06 后） | 套件 finally 清理 + `no-residual` 断言 |
启动前均先跑 `python3 -m py_compile` + 生产者 import 自检 + `LINGXI_R09_ALLOW_GUI` 门；任一步可见窗口/弹窗迹象/非预期 exit → 立即终止并回报。**预计 GUI app 启动总次数：2（+ 失败即停，无重试）**。

## 4. 46 片共享叶映射（T03/T07；平台说明见 §2）

T07 为 E2E 证据层；叶的归属与 backlog 见 `R09_LEAF_LEDGER.json`（`plannedSuite=r09_t03_native_matrix`）。`分类` 依据 `implementationEntry`：host/ui 面走 T07 E2E；server/React 面按 native-caps §9 附录标非宿主，不在 T07 E2E 冒充通过。

| # | leaf id | 分类 | 断言摘要 |
|---|---|---|---|
| 1 | `R00-T02-LA-0DEEE4813B0A` | host/ui | 新 BrowserWindow 按窗口 ID 拉取文件载荷并显示，关闭后清除 pinned 状态 |
| 2 | `R00-T02-LA-1120B2AA320D` | host/ui | server PreferencesManager 持久化后打开主窗口并注册快捷键 |
| 3 | `R00-T02-LA-1329EA46F72C` | host/ui | 逐动作核对实际状态与页面投影；失败不伪装成空数据 |
| 4 | `R00-T02-LA-15DFB1AED8F2` | host/ui | Electron 原生拖拽携带单个或多个路径 |
| 5 | `R00-T02-LA-15E163FBFBE8` | host/ui | 主窗、设置窗、浏览器查看器收到同类变更并刷新相关显示 |
| 6 | `R00-T02-LA-1629CDD9EA4F` | host/ui | 写 user.md 并重建 system prompt |
| 7 | `R00-T02-LA-173C6332527D` | host/ui | 强制重探并返回新报告 |
| 8 | `R00-T02-LA-18FE3FDFF9A5` | host/ui | 返回主题/字体等外观状态，不改动设置 |
| 9 | `R00-T02-LA-1C41525D379C` | host/ui | 仅打开或聚焦引导窗口 |
| 10 | `R00-T02-LA-1D4C7E9C713C` | host/ui | 从 registry 返回名称、别名、说明、权限、作用域和来源；未就绪返回 503 |
| 11 | `R00-T02-LA-1E6EDB731186` | host/ui | 预览窗显示引导步骤 |
| 12 | `R00-T02-LA-322E02C7F2F9` | host/ui | 主窗/设置窗/viewer 从激活目录重载 |
| 13 | `R00-T02-LA-37E6C3B9A7B8` | host/ui | DOCX 转 HTML 后在预览中显示 |
| 14 | `R00-T02-LA-394DD07D2968` | host/ui | 保存快捷键映射并返回新状态 |
| 15 | `R00-T02-LA-429D86C4F952` | host/ui | 返回共享 user.md 内容 |
| 16 | `R00-T02-LA-57728C0A6D0A` | host/ui | 创建或聚焦设置窗，切到指定标签 |
| 17 | `R00-T02-LA-58822B6B138B` | host/ui | splash/onboarding 关闭后主窗显示 |
| 18 | `R00-T02-LA-59987671E5AD` | host/ui | 返回按 workspace 与 surface 保存的界面状态，不改动设置 |
| 19 | `R00-T02-LA-5D5938D61A00` | host/ui | 有效组合被 OS 注册并可拉起快捷聊天；冲突反馈不可注册 |
| 20 | `R00-T02-LA-6006FD0BF07D` | host/ui | 逐动作核对实际状态与页面投影；失败不伪装成空数据 |
| 21 | `R00-T02-LA-6774E79F6714` | host/ui | 持久设置 setupComplete=true；后续启动不再进入初设流程 |
| 22 | `R00-T02-LA-70B07E378F42` | host/ui | 仅旧值 false 时比较后删除；其他当前值原样保留，返回是否改动 |
| 23 | `R00-T02-LA-713E908B7B2F` | host/ui | 逐动作核对实际状态与页面投影；失败不伪装成空数据 |
| 24 | `R00-T02-LA-8FE26C132D8B` | host/ui | 返回分组/显示状态，不改动设置 |
| 25 | `R00-T02-LA-9350C3E12165` | host/ui | 应用按既定退出/重启路径重新启动 |
| 26 | `R00-T02-LA-98B458390B7E` | host/ui | 返回快捷键映射，不改动设置 |
| 27 | `R00-T02-LA-9C679D21A188` | host/ui | 逐动作核对实际状态与页面投影；失败不伪装成空数据 |
| 28 | `R00-T02-LA-A14D3027C881` | host/ui | 逐动作核对实际状态与页面投影；失败不伪装成空数据 |
| 29 | `R00-T02-LA-A2E507EBDEF2` | host/ui | 读取/修改 OS 登录项并回显真实状态 |
| 30 | `R00-T02-LA-AC5B6EDA3FBD` | host/ui | 显示启动所需名称/头像等资料 |
| 31 | `R00-T02-LA-ACA2C4C1158D` | host/ui | 仅 http/https 交由系统浏览器打开 |
| 32 | `R00-T02-LA-B6A97A19F2CB` | host/ui | 主界面设置弹窗打开并切到目标 tab |
| 33 | `R00-T02-LA-B72A6D3886B7` | host/ui | 系统 powerSaveBlocker 状态与开关结果一致 |
| 34 | `R00-T02-LA-BEC2074CC33C` | host/ui | zh/en/ja/ko/zh-TW 文案随偏好更新且重启保留 |
| 35 | `R00-T02-LA-C5AAF360D10F` | host/ui | 保存按 workspace 与 surface 保存的界面状态并返回新状态 |
| 36 | `R00-T02-LA-CD873C715D96` | host/ui | 读取缓存报告或 refresh=1 强制重探 |
| 37 | `R00-T02-LA-D15E0ADC0608` | host/ui | 保存通知设置并返回新状态 |
| 38 | `R00-T02-LA-D29DE479076C` | host/ui | 主窗及派生窗口收到主题消息并重新绘制 |
| 39 | `R00-T02-LA-DF4B5168ABB5` | host/ui | 未聚焦且 OS 支持时弹通知；点击通知聚焦主窗 |
| 40 | `R00-T02-LA-E6FB08FBBFAA` | host/ui | 返回通知设置，不改动设置 |
| 41 | `R00-T02-LA-E84F01456B6C` | host/ui | 首工作表转换为转义 HTML 表格 |
| 42 | `R00-T02-LA-F5DBF2A80E28` | host/ui | 保存主题/字体等外观状态并返回新状态 |
| 43 | `R00-T02-LA-F6235778F816` | host/ui | 返回配置、人格、偏好、接入及 bridge 状态 |
| 44 | `R00-T02-LA-F9786414DD11` | host/ui | 仅调用者所属 BrowserWindow 发生相应状态变化 |
| 45 | `R00-T02-LA-FB5C78308878` | host/ui | 保存分组/显示状态并返回新状态 |
| 46 | `R00-T02-LA-FF1B331F1019` | host/ui | SettingsContent activeTab 更新并渲染目标页 |

## 5. 依赖 T06 的接口点（GUI 批次前必须完成）

1. `native::install::INVOKE_COMMANDS` 28 条 + debug 2 条注册（invoke_handler/build.rs/capabilities）；否则文件任务/窗口控制/设置等 E2E 用例无命令可调。
2. window-state 持久化（move/resize 防抖 + `load_window_state` 恢复）→ A13 的“窗口恢复”断言。
3. 长会话夹具：需服务侧确定性会话种子（A13 “真实 Tauri 应用加载长会话”），T07 使用服务替身协议生成，不 mock 宿主/服务边界。
4. 文件任务（A14）：read_file/写入/预览命令走 T06 收口后的 host 命令面；当前 `t07::release.file-task` 显式红（PENDING_T06_COMMANDS），不冒充。
5. 测试版/交付版隔离：wdio 插件仅 `e2e-test` feature；release 冒烟用无 feature 构建 + strings 断言（A14/A12）。

## 6. 已知边界/风险

- 合成 `CompositionEvent` 不等于真实系统 IME：记录“合成 vs 真键盘”的边界；若需人工视频/真实输入法验证，需 Lead 授权交互式 GUI 会话。
- automation 隐藏窗口下 wdio 截图可能为空/黑屏：截图仅作辅助，主证据为 DOM 投影 + cases；如为空如实记录，不冒充视觉回归。
- wdio 插件在 WKWebView 下需要本机 GUI 会话与 WebDriver 端口；本机 CLT-only 已由 R06 验证可行，但 T07 需真实候选重建后复验。
- 平台阻塞沿用 `BLOCKED.md`（mac x64/win x64/linux x64），不改写为通过。


## 7. Lead 硬停止后的候选消费规则（2026-10-26T22:5x）

- **T07 期间禁止**在 `desktop/src-tauri/**` 做任何写/`cargo build|test|check`/`scripts/tauri-build.mjs`/启动 GUI；
  T07 不再自行构建候选。原因：与 T06 命令面收口并发冲突（Lead 已 interrupt 并终止构建进程，ps=0）。
- 生产者已改为**只读消费**：
  - G1：`LINGXI_R09_T07_APP_BIN` = T06/Lead 产出的 `features≥[r09-host,e2e-test]` evidence 构建可执行；
    可选 `LINGXI_R09_T07_E2E_RECEIPT` 记录 features/sha；缺失 → `CANDIDATE_REQUIRED` 诚实红，不构建。
  - G2：`LINGXI_R09_T07_RELEASE_APP` + `LINGXI_R09_T07_RELEASE_EXE` = 交付 release `.app`/可执行；
    可选 `LINGXI_R09_T07_RELEASE_RECEIPT`；缺失 → `CANDIDATE_REQUIRED`，不构建。
- 候选交接需求（向 Lead/T06 提出）：G1 evidence 构建须与 G2 交付 release 使用**同一 renderer 摘要候选**；提供路径、sha256、
  feature 列表、tauri-cli/toolchain、rendererDigest/themeBundleSha256；G1/G2 期间 T06 不得再改 desktop 产物。

## 8. 无 GUI 阶段证据（当前）

| 项 | 结果 | 证据 |
|---|---|---|
| 夹具生成器 | **PASS**：R08 既有生成器同种子两次 rootHash 全等 = v3Base 锚点 `80a8d2c2…`；staged tree `3bbb5d4b…`（36 文件）；1000 条消息长会话 + 工具卡 3 种（候选 schema 明示未验证） | `/private/tmp/t07-fixture-run/evidence/fixtures-manifest.json`；套件 `static-04-native_e2e/fixtures/fixtures-manifest.json` |
| native_e2e 静态前半 | **PASS exit 0 / 11 cases**（夹具确定性/锚点/长会话/工具卡边界、平台矩阵、隔离零残留） | `artifacts/rust-tauri/R09/dev/t07/static-04-native_e2e` |
| release 静态前半 | **PENDING_CANDIDATE**（T07 不再构建；需 Lead/T06 交付候选后只读检查） | 代码已切换只读消费；缺失 env → `CANDIDATE_REQUIRED` |
| 只读既有产物登记（mid-T06，仅记录不作最终候选） | release exe sha256 `8c26577e…`、sidecar `55d6edb1…`、`build.json.rendererSha256=d5a24adf…`、codesign deep --strict exit 0、strings `wdio/devUrl/5173` 全 0；debug 产品 bin sha256 `d0c738a0…`（可能被 T06 重建覆盖，最终以 Lead 交接候选为准） | 本轮只读命令输出（无构建） |
| wdio/IME 边界 | 合成 `CompositionEvent` ≠ 真实系统输入法；隐藏窗口截图可能黑屏；wdio 仅 evidence 构建；WebView 切换/多显示器受 macOS 会话限制 | 见 §6 |

## 9. G1/G2 启动批次清单（更新：A13 可见交互需单独 cleared）

| 批次 | 窗口模式 | 次数 | 内容 |
|---|---|---|---|
| G1-hidden | hidden+sink+Accessory | 1 | `cargo test --features r09-host,e2e-test --test t07_native_e2e`（`LINGXI_R09_T07_APP_BIN` 指向 T06 evidence bin）；DOM/IME 合成/选区/滚动/DPI/焦点/截图投影 |
| G1-visible | **可见窗口**（单独 cleared；结束后立即关闭并告知用户） | 1 | A13 真实窗口/滚动/焦点/IME 用例：可见窗口下重复关键投影 + `screencapture` 截图；**不用 hidden 冒充可见交互** |
| G2 | hidden+sink | 1 | release `.app`（T06 交付候选）：启动/连接/页面来源/`strings`/codesign/优雅退出；文件任务在 T06 命令面后补 |

## 10. 可见窗口批次清单（A13 真实窗口/IME/滚动/焦点；与 T03 真实动作合并，需单独 cleared）

触发条件：Lead 在“用户可见窗口批次”统一 clearance，且 T06 候选冻结、G1-hidden/G2 已完成或同批。

| 步 | 操作 | 预期（可观察） | 清理/截图 |
|---|---|---|---|
| V0 | 预检：ps=0、候选 sha 与 G1/G2 相同、`LINGXI_R09_AUTOMATION` **不设置**（可见窗口需要真实显示） | 无残留、候选一致 | 记录 ps 快照 |
| V1 | 启动产品 app（T06 evidence 构建，`LINGXI_R09_T07_APP_BIN`），隔离 `LINGXI_HOME/TMPDIR`，可见窗口 1 个 | `window-created.visible=false` → `app-ready.windowVisible=true`；Dock 图标出现（非 Accessory） | `screencapture -x` 全屏 1 张 + 窗口截图；记录窗口标题/位置 |
| V2 | wdio 连接 → 产品主窗投影（title/URI/`data-format`） | 与 hidden G1 投影同值（除窗口可见性字段） | 保存 DOM 投影 JSON |
| V3 | **真实键盘 IME**：用 wdio `actions` 输入中文（优先系统输入法路径；若自动化无法触发真实 IME，改由人工在 cleared 会话内输入并截屏/录屏，脚本记录“人工步骤”） | 文本进入 composer，内容不丢；composition 事件计数>0；与合成 composition 结果对照差异记录 | 截屏 + 事件计数；人工步骤录屏（需用户授权） |
| V4 | 复杂文本选择/引用：真实鼠标拖选 + 选区投影 | `getSelection()` 非空；引用/草稿状态可观察 | 截屏 + selection JSON |
| V5 | 滚动长列表/超长会话：真实滚轮（wdio actions 或人工），观察 3 次 | scrollTop 变化、无异常滚动/无 ErrorBoundary；长会话 fixture 在“服务侧读取接线后”才可出现，否则如实标 PENDING_NON_HOST | 截屏 3 张 |
| V6 | 工具卡展开：真实点击（若会话/卡片可见） | 卡片展开态；否则记 PENDING（fixture schema 未验证/渲染链未接线） | 截屏 |
| V7 | 切窗焦点：Cmd+Tab/点击其他窗口再回来 | window blur/focus 计数、composer 焦点恢复或按策略不抢焦点 | 事件 JSON + 截屏 |
| V8 | 缩放/DPI：系统缩放/窗口 resize | devicePixelRatio/visualViewport 记录、布局不破 | JSON + 截屏 |
| V9 | 关窗/退出：真实关闭窗口 | `RunEvent::ExitRequested` → 清理 owned 服务 → exit 0；ps 0；sink 不存在 | ps 快照 + shutdown 诊断 |

时间盒：≤5 分钟可见窗口；V1 与 V9 之间若出现任何失败立即关闭窗口并停止，不重试；结束后立即向 Lead/用户报告“可见会话结束 + ps 0”。

## 11. G1-hidden 最终结果 + visible A/B/C 定稿（2026-10-26T23:0x）

### G1-hidden（PASS，单发）
- 命令（候选只读消费；wrapper `.app` = 交付骨架 + evidence exe，ad-hoc 仅 /tmp）：
  `LINGXI_R09_ALLOW_GUI=1 LINGXI_R09_T07_APP_BIN=/private/tmp/lingxi-r09-t07/evidence-app-20261026-230352/Lingxi.app/Contents/MacOS/lingxi-resource-desktop LINGXI_R09_T07_E2E_RECEIPT=<repo>/desktop/src-tauri/.tauri-build/build-receipt-debug.json python3 -B scripts/rust-tauri/r09_suites.py --suite r09_t07_native_e2e --evidence artifacts/rust-tauri/R09/dev/t07/g1-hidden-04`
- **PASS exit 0 / 26 cases**；证据 `artifacts/rust-tauri/R09/dev/t07/g1-hidden-04`：
  - 候选：wrapper main exe 签名后 sha `8fe7733e…`（上游 evidence exe `c3622805…`），features `e2e-test,r09-host,r09-test-hooks`；receipt `78a588ce…`。
  - 平台记录 mac-arm64（macOS 27.0.1 build 26A434、WebKit 22625.1.29.11.28、M3 Ultra 2560×1440）；mac-x64/win-x64/linux-x64 `BLOCKED_PLATFORM`。
  - 夹具确定性（v3Base `80a8d2c2…`、1000 条消息、3 工具卡候选 schema）。
  - DOM：title `Lingxi · Rust 预览`、`tauri://localhost`、hasTauri=true/hasHana=false、dpr=1；中文 IME 合成 composition（observed 文本 matches，标注“合成非系统输入法”）；Range 选区 selected=9；滚动指标记录（长列表 PENDING_NON_HOST）；焦点监听；截图 96,180B base64 → 解码 72,133B PNG 960×820 RGBA 131 色（hidden 窗口真实渲染，不作为可见交互证据）。
  - 清理：graceful-exit exit 0、wdio 端口释放、dialog sink clean、无进程残留、ps=0、真实 ~/.lingxi 未触达。

### visible 批次定稿（A/B/C 三次可见启动；Lead 统一 clearance；总 ≤10 分钟；启动间 ps 断言 0）

| 批次 | app / 可见性 | env（除隔离 `LINGXI_HOME`/`TMPDIR` 外） | 步骤与截图点 | 清理/断言 |
|---|---|---|---|---|
| **A｜G2 交付 release 可见** | `desktop/src-tauri/target/release/bundle/macos/Lingxi.app`（冻结 appTree `2dc38bc3…`，exe `17523a17…`，features 仅 `r09-host`）；**可见窗口**（无 hooks，真实显示） | 不设 automation/sink；不设 `LINGXI_SERVICE_BIN`（packaged=true 用包内 sidecar）；`LINGXI_HOME`/`TMPDIR` 隔离 | 启动 → 断言 `app-ready.windowVisible=true`、`service-ready(manifest 1..=1 epoch2, signedMutation)`、page-load `tauri://localhost`、无 5173；文件任务用可见 UI 操作（AppleScript 不可用则 MANUAL_STEP+截图，不冒充）；截图 2 张 | SIGTERM → exit 0；ps=0；诊断 shutdown residual=[]；5173/残留扫描 |
| **B｜T07 evidence wrapper 可见（A13 UI）** | `/private/tmp/lingxi-r09-t07/evidence-app-20261026-230352/Lingxi.app`（main exe evidence `c3622805…` → ad-hoc 后 `8fe7733e…`；features `e2e-test,r09-host,r09-test-hooks`）；**可见窗口**（需要真实交互/滚动/焦点） | `LINGXI_R09_ALLOW_GUI=1`；`LINGXI_SERVICE_BIN=<repo>/rust/target/debug/lingxi-service`；随机 `TAURI_WEBDRIVER_PORT`；**不设 automation/sink**（保留正常失败弹窗语义，失败即关窗停） | wdio shell 投影 → 真实键盘 IME（不可自动化则清窗内人工输入+截屏/录屏）→ 拖选/引用 → 滚轮长列表（PENDING_NON_HOST 如实）→ 工具卡（PENDING）→ 切窗焦点 → DPI/resize；每步截图 1 张 | SIGTERM/关窗 → ps 0；wdio 端口释放；无 sidecar 残留 |
| **C｜T03 gui-actions 可见动作** | 同一 evidence wrapper；**可见窗口** | 在 B 的 env 基础上：`LINGXI_R09_GUI_CLEARED=1`、`LINGXI_R09_TAURI_APP=<wrapper>/Contents/MacOS/lingxi-resource-desktop`（gui_actions 期望 exe 路径，内部以 `--r09-visible-batch` 启动）、`LINGXI_R09_WEBDRIVER_PORT=<random>`、`LINGXI_R09_MANUAL_EVIDENCE_DIR=<dir>`；**不设 automation/sink** | 运行 `python3 -B scripts/rust-tauri/r09_suites.py --suite r09_t03_native_matrix --evidence <new-dir>`（visible 分支走 `r09_gui_actions.run_gui_plan`，plan=`docs/rust-tauri/R09/T03_LEAF_CASE_PLAN.json`）：托盘菜单/快捷键→快聊/通知展示+点击聚焦/文件面板/窗口控制/主题广播；每动作系统可见结果 + 截图 | 真实关窗 → `gui::app-shutdown-clean` residualPids 为空；ps=0 |
- 录屏仅用户明确授权；任一步意外窗口/弹窗/非预期退出 → 立即关窗停止整批；A/B/C 各自证据目录独立，不继承旧目录。
- 依赖：wrapper 路径最终由 Lead 确认（我这份或 T06 正式 evidence `.app`，同一 evidence exe）；T03 动作脚本路径已完成（`scripts/rust-tauri/r09_gui_actions.py`，入口/环境见上）；T06 运行期 ACL 走 hidden 单发，不占可见批次。

## 12. v2 候选新增 G1 用例（T08 缺口修复配套）

- `t07::native-e2e.context-menu-policy`：真实 DOM `contextmenu` 事件——非可编辑区域 `defaultPrevented=true`、
  `textarea` 可编辑区域 `defaultPrevented=false`（与 Electron `main.cjs:490-507` 等价；hidden 窗口无需可见）。
- `t07::native-e2e.file-url-asset`：`window.__TAURI__.core.convertFileSrc` → `fetch(assetUrl)` 内容校验；
  夹具写在 `producer.tmp`（Tauri `$TEMP` 若尊重 TMPDIR 应命中 T06 v2 `assetProtocol.scope`）。
  失败如实红：scope 未覆盖时属 T06 patch 未生效，不得用静态配置冒充通过。
- v2 handoff 到达后 G1 命令与 T08 重跑命令见 `HOST_V2_PATCHES.md §7`。

## 13. G1-hidden v2 首跑与 context-menu 语义修正（2026-10-26T23:2x）

- v2 首跑（`g1-hidden-v2-02`）exit 2：wrapper/候选/shell/IME/选区/DPI/焦点/隔离均 PASS，
  唯一 FAIL 是 `t07::native-e2e.context-menu-policy` 中 `editablePrevented=true` 的**断言语义**：
  renderer 的 `InputContextMenu`（React 自定义剪切/复制/粘贴菜单）对输入框默认接管并
  `preventDefault`，与 Electron 同构（Electron 主进程 `context-menu` 策略只兜底非编辑区）。
- 修正：G1 断言改为“非编辑区必须 prevented=true”（宿主策略）；编辑区记录
  `context-menu-editable-managed`（由应用自定义菜单接管，not 宿主策略）。T08 的
  `a16::context-menu-policy.real-behavior` 仍在干净 jsdom 中验证“非编辑区 prevent、编辑区不 prevent”
  （策略本身），两者不冲突。
- file-url-asset 用例因首跑在 context-menu 断言处中止未执行，待 cleared 重跑验证。

## 14. G1-hidden v2 重试（context-menu 修复后）与 file-url 方法修正（2026-10-26T23:2x）

- `g1-hidden-v2-03` exit 2：**context-menu 两项 PASS**（非编辑区 prevented；编辑区由 InputContextMenu 接管并记录），
  其余 shell/IME/选区/DPI/焦点/候选/隔离 PASS；失败在 `file-url-asset`：
  `fetch(asset://...)` → `TypeError: Load failed`（`fetch` 受 CSP `connect-src` 限制；asset 协议是 `img-src` 通道），
  且夹具放在 `producer.tmp` 而非 T06 v2 `host.rs allow_directory(home)` 授权根。
- 修正：夹具改为隔离 LINGXI_HOME 下的真实 1x1 PNG，用 `<img src={convertFileSrc(path)}>` 的 `onload/onerror`
  判定 scope 命中（3s 超时）；不再用 fetch。**等待 Lead cleared 重试**（同时确认 T06 ACL hidden 单发已结束、ps 干净）。
- 过程教训：本轮启动前 preflight 已检测到 T06 `r09_t06_runtime_acl.py` 及其 app 在跑，但未中止即启动——
  违反“串行、T06 先结束”纪律；实际未造成资源冲突（我方 app/service 正常启动），但后续必须 ps 干净才启动。

## 15. 启动纪律（Lead 2026-10-26 收紧；任何 hidden/visible app 启动前必查）

1. `ps` 必须无任何 `lingxi-resource-desktop` / `Lingxi.app/Contents/MacOS/*` / `lingxi-service --home /private/tmp/lingxi-r09*` 进程；
2. 若有其他套件（如 T06 runtime ACL）在跑，**等其结束并收到 Lead/T06 明确“已结束”**后再启动；
3. 启动前把 preflight `ps` 快照写入本轮证据（`preflight-ps.json`）；
4. 违反即**整批作废、不重跑**（记录在 T07_PROGRESS 并回报 Lead）。

## 16. visible 批次启动清单（Lead 修订版 2026-10-26；≤10 分钟；用户可见窗口 5 次、无预期弹窗）

前置：T06 ACL hidden 已结束（Lead/T06 明确确认）+ G1-hidden v2-04 与 T02 debug/release hidden 子集全绿 + Lead 对用户明确告示后 visible clearance。
纪律（写入启动纪律 §15 的补充）：visible 批次**用户可见窗口 5 次、无预期弹窗**；T02 release 负向/所有权 release 用例已移入 hidden（见 §16.2），visible 只保留正路径/真实动作；每步前 `preflight-ps.json`、后 `post-ps.json`（0 残留）、sink/端口复检；异常立即关窗停整批；录屏仅用户明确授权。

### 16.1 visible（5 次启动，≤10 分钟）
| # | 套件/用例（证据目录） | app / 可见性 | 窗口数 | 步骤与预期 | 截图点 | 时长 |
|---|---|---|---|---|---|---|
| V1 | `r09_t01_release_offline`（visible-01） | v2 delivery `.app`；可见 | 1 | 启动→app-ready/page-load `tauri://localhost`/manifest/identity；`VITE_DEV_URL=127.0.0.1:5173` 无效；canary 0；SIGTERM exit0 | 启动后 1 张 | ~2min |
| V2 | G2 release 冒烟（visible-02） | v2 delivery `.app`；可见 | 1 | app-ready windowVisible=true；service-ready manifest；page-load；文件任务（可见 UI/必要人工步骤，截图）；SIGTERM exit0 | 2 张 | ~2min |
| V3 | `r09_t01_react_parity`（visible-03，`LINGXI_R09_T01_VISIBLE=1`） | Electron + T01 Tauri evidence wrapper；均可见 | 2 同时 | Electron CDP 投影 + Tauri DOM 投影逐项相等；两壳服务身份/health；两壳 SIGTERM | 1 张（双窗） | ~3min |
| V4 | T03 `r09_t03_native_matrix` gui-actions 分支（visible-04） | T07 evidence wrapper `.app`；可见 | 1 | 托盘菜单/快捷键→快聊/通知展示+点击聚焦/文件面板/窗口控制/主题广播；每动作系统可见结果 | 每动作 1 张（~6） | ~3min |

- **用户可见窗口总数 5**（V1 1 + V2 1 + V3 2 + V4 1）；预计 ≤10 分钟；顺序 V1(delivery)→V2(delivery)→V3(Electron+wrapper)→V4(wrapper)，减少换壳次数。
- 无预期弹窗：所有失败/负向路径不在 visible 执行；若出现任何意外弹窗/可见性异常/非预期 exit → 立即关窗、整批停止并回报。

### 16.2 hidden（无可见窗口；负向与 release 用例）
| # | 内容 | 对象 | 预期 |
|---|---|---|---|
| H1 | G1-hidden v2-04 | v2 evidence wrapper（hidden+automation+sink） | 全部 G1 cases，含 `file-url-asset`（img onload/naturalWidth）与 context-menu 两项 |
| H2 | T02 debug hidden 子集 | v2 evidence wrapper debug（hidden+sink） | supervisor 1 发 + ownership 3 发；release/可见用例登记 blocked（整轮 FAIL 不冒充） |
| H3 | T02 release 负向/所有权 release 子集 | **T06 即将构建的「带 hooks 的 release evidence bundle」**（isolated CARGO_TARGET_DIR，不动交付候选） | release 正/负向 + 所有权：hidden+automation+sink，无原生弹窗；env `LINGXI_R09_T02_RELEASE_APP` 指向该 evidence bundle |
- H1/H2/H3 均需 ps 快照+串行确认（§15）。

## 17. v3 renderer 映射与 H1–H3 候选消费（等 T06 handoff-v3）

### 17.1 v3 renderer 覆盖面（`desktop/src/react/host/**`；tsc 0 / vitest 15/15）
- 六能力族 23 成员已真实接线：file-text-io / detached-viewer / skill-viewer / browser-viewer / file-watch / file-drop-paths（映射表见 `HOST_V2_PATCHES.md §8.1`）。
- PlatformApi 109：**真实 70 静态 + getFilePath 条件注入 = 71 / no-op 1 / 可选缺失+理由 6 / 显式受限+理由 32**。
- 测试：`r09-t01-tauri-desktop-host.test.tsx` 15/15（新增 5 个 v3 用例：每族至少一条真实 invoke/event 映射；file-drop 用 fake `webviewWindow.onDragDropEvent` 验证顺序消费）。

### 17.2 T06 v3 Rust patch 后的预期变化（handoff 冻结点）
- **已完成（v3.1 renderer）**：`onViewerClosed` 监听 `viewer-closed`（数字 windowId）；`onShowSkillViewer` 监听 `show-skill-viewer`（宿主 no-op 移除）；`watchWorkspace/unwatchWorkspace` invoke `watch_workspace/unwatch_workspace` + `onWorkspaceChanged` 监听 `workspace-changed`；viewer URL 由 T06 改 `viewer-window.html`；
- capability allow 联合集：main/viewer/skill-viewer/browser-viewer 窗口核对（T06 audit）；
- **deferred/blocked（T06 裁决登记，renderer 保持显式受限+理由）**：完整 `browser-update` 流（当前 `browser:connection-changed` 部分映射）、`native/file_drop.rs`/`file_drop_paths`（当前 renderer drop 队列可用，命令面 deferred）。
→ **v3.1 renderer final**：`tsc --noEmit` exit 0；host vitest **16/16 pass**；PlatformApi=109：**真实 76 / no-op 0 / 可选缺失+理由 6 / 受限+理由 27**。等 T06 重建 v3.1（renderer/delivery/G1/release-evidence）+ handoff-v3.1 冻结后重建 wrapper 跑 H1。

### 17.3 v3 wrapper 重建步骤（handoff 到达后执行；只读候选、不构建）
1. 读取 `artifacts/rust-tauri/R09/dev/t06/candidate-handoff-v3/candidate-handoff.json`（或 `LINGXI_R09_T07_HANDOFF` 指定）；
2. 复制 v3 delivery `.app` 骨架到 `/private/tmp/lingxi-r09-t07/evidence-app-v3-<stamp>/Lingxi.app`；
3. 用 v3 G1 evidence exe（handoff `g1Evidence.executableSha256`）替换 `Contents/MacOS/lingxi-resource-desktop`，记录替换前后 sha；
4. `codesign --force --deep -s -` + `codesign --verify --deep --strict`（仅 /tmp 副本，ad-hoc）；
5. manifest 落 `artifacts/rust-tauri/R09/dev/t07/g1-hidden-v3-01/wrapper-manifest.json`；写 `wrapper-path.txt`。
6. H1 命令：`LINGXI_R09_ALLOW_GUI=1 LINGXI_R09_T07_APP_BIN=<wrapper exe> LINGXI_R09_T07_E2E_RECEIPT=<v3 debug receipt>` → `r09_t07_native_e2e --evidence .../g1-hidden-v3-02`（含 file-url img onload/context-menu）。
7. H2：`LINGXI_R09_ALLOW_GUI=1 LINGXI_R09_T02_HIDDEN_ONLY=1 LINGXI_R09_T02_DEBUG_APP=<wrapper> LINGXI_R09_T02_DEBUG_RECEIPT=<v3 debug receipt>` → 两 T02 套件；
8. H3：`LINGXI_R09_T02_RELEASE_APP=<T06 release-evidence hooks bundle>` → T02 release hidden 子集。
每轮启动前 `preflight-ps.json`（ps 空）；结束 `post-ps.json` 0；串行、失败即停、不重试。

## 18. H1 v3.1 首跑（2026-10-26T23:4x）

- wrapper 已按 handoff-v3.1 重建：delivery 骨架 + G1 exe `54f93eac…`（替换前 `248a8e34…`）→ ad-hoc 签名后 `ac729159…`；sign/verify exit 0；manifest `g1-hidden-v31-01/wrapper-manifest.json`；preflight/post ps 均 0（`preflight-ps.json`/`post-ps.json`）。
- H1 `g1-hidden-v31-01` exit 2：候选/shell/IME/选区/DPI/焦点/**context-menu 两项**/isolation 全 PASS；`file-url-asset` 段因**生产者 NameError `home`**（`run_g1_js_matrix` 内引用未定义变量）中止，非候选/asset 问题；已修（函数内取 `producer.runtime/home-native-e2e`，与 suite 启动 home 一致）。**按“失败即停”未重跑，等 cleared 重试。**

## 19. H1 v3.2 PASS（2026-10-26T23:55）

- 根因修复（Lead）：页面级 `connection-csp.js` meta CSP img/media 缺 `asset:`；v3.2 renderer `b0a20250…` 后恢复。
- v3.2 wrapper：delivery 主 exe `8e74aa15…` → G1 exe `030607ee…` → ad-hoc 后 `992f0a7b…`（sign/verify 0）；manifest `g1-hidden-v32-01/wrapper-manifest.json`。
- **H1 `g1-hidden-v32-02` PASS exit 0 / 30 cases**：file-url-asset `<img>` loaded=true（1×1 PNG）、`debug_asset_probe` `allowedRaw/allowedCanonical=true`、runtimePatternCount=6（`assetProtocolEnable` 字段 false 为 hooks 诊断字段语义，不代表协议未生效，img 实测加载成功）；context-menu 两项 PASS；graceful-exit 0、wdio 端口释放、sink clean、无进程残留、真实 home 摘要不变；hidden 截图 72,133B PNG 960×820。
- 期间 T06 runtime ACL v3.2 并发运行（非本套件进程）；H1 完成后 H2 未实际启动（详见 §20）。

## 20. H2/H3 v3.2 hidden 结果（2026-10-26T00:0x）

T06 ACL v3.2 自然结束后（ps 干净）串行执行；每轮 preflight/post `ps` 快照均 0（`preflight-ps.json`/`post-ps.json`），无失败、无重试。

| 批次 | 套件（证据） | 候选 env | 结果 |
|---|---|---|---|
| H2 | `r09_t02_supervisor_ready`（`h2-supervisor-02`） | DEBUG_APP=v3.2 wrapper、HIDDEN_ONLY=1 | **PASS exit 0 / 10 cases**：debug READY 116ms、no-fixed-sleep、graceful exit0、ps-clean、automation no-modal |
| H2 | `r09_t02_process_ownership`（`h2-ownership-02`） | 同上 | **PASS exit 0 / 13 cases**：owned+remote coexists/exit owns only own tree、reused detached、remote zero-touch、ps-clean×3 |
| H3 | `r09_t02_supervisor_ready`（`h3-supervisor-01`） | DEBUG_APP=v3.2 wrapper + RELEASE_APP=T06 release-evidence hooks bundle | **PASS exit 0 / 31 cases**：release 正路径/graceful exit0、protocol/digest 注入拒绝、failure dialog suppressed（hooks+sink）、no-node-fallback、ps-clean 每发 |
| H3 | `r09_t02_process_ownership`（`h3-ownership-01`） | 同上 | **PASS exit 0 / 19 cases**：release 拒远端、pid-reuse safe、packaged SIGTERM、no-orphan、ps-clean 每发 |
- T02 套件无截图机制（进程/退出/sink 语义验证，非 UI）；H1 已提供 hidden 截图（72,133B PNG 960×820）。
- H2 hidden-only 语义：release/可见用例不执行、记 `pendingVisible`（不产生假红/不冒充）；H3 用 release-evidence hooks bundle 覆盖 release 正/负向（hidden+sink，无原生弹窗）。

## 21. G1 evidence 路径污染与恢复（2026-10-27T00:02–00:06）

- 现象：`desktop/src-tauri/target/debug/lingxi-resource-desktop` 在 00:02 被外部构建覆盖为 `7b116ea4…`（55,384,664B，**无 e2e-test/wdio**，hooks-only 形态），但 wrapper receipt 仍保留冻结值（`030607ee…`/`513aba98…`）→ T08 `fixed::evidence-exe-sha` 红。
- 恢复尝试：按 Lead 指令运行 `node scripts/tauri-build.mjs --debug --features r09-test-hooks,e2e-test --force --no-renderer-build` → 产物 `82be2520…` **≠ 冻结 `030607ee…`（wrapper 构建非确定性）**；按指令停止，未改 handoff。
- 最终恢复：从 T06 ACL 在 23:55 冻结窗口留下的 wrapper 备份
  `/private/tmp/lingxi-r09-t06-acl-53257-1793030150/LingxiRuntimeAcl.app/Contents/MacOS/lingxi-resource-desktop`
  找到逐字节冻结副本（sha `030607ee…`）→ 恢复 target/debug；receipt 恢复为冻结 `513aba98…`；稳定副本
  `/private/tmp/lingxi-r09-evidence/lingxi-resource-desktop-030607ee`（sha 一致）。renderer 仍 `b0a20250…`（未重建）。
- 污染源判断：wrapper receipt 未被更新、二进制为 hooks-only 形态、`.fingerprint/lingxi-resource-desktop-{df4934b89177f9ed,cec7fb3dd89dc054}` 在 00:00 后被更新 → 系**直接 `cargo build`（非 wrapper `tauri build`）**；具体调用者未在进程/日志中留存（T06 报告其观察到外部 hooks-only 构建；待 T06/Lead 最后确认）。
# 纪律（追加）
- 除 `scripts/tauri-build.mjs` 外**禁止直接 `cargo build` desktop crate**；`target/debug/lingxi-resource-desktop` 是**证据路径**，不得被任何非 wrapper 构建覆盖。
- wrapper 构建非确定性（同命令可产生不同 sha）；证据消费者只认 handoff sha + **稳定副本**（如 `/private/tmp/lingxi-r09-evidence/lingxi-resource-desktop-030607ee`），不直接绑 target 路径；重建 wrapper 只从稳定副本取。
- 任何构建/门运行前先 `ps` 确认无其他套件；发现 evidence 路径 sha 漂移立即停止并报告，不改 handoff。

## 22. final-01 门后：target/debug 污染根因与修复（2026-10-27）

- 根因确认：**`r09_t02_supervisor_ready` 末尾的 `cargo test --manifest-path desktop/src-tauri/Cargo.toml
  --features r09-host --test service_supervisor`**（final-01 旧值）会编译 `CARGO_BIN_EXE_lingxi-resource-desktop`
  并把产物写到共享 `target/debug`，覆盖冻结 G1 证据路径；证据：final-01 T02 suite 结束时间
  00:10:04 与 target/debug mtime 同秒，污染二进制 55,384,664B 且 wdio strings=0（无 e2e-test）。
  非 T07 release_smoke（代码 0 构建调用，final-01 log 仅 strings/codesign）。
- 修复：T02 该 cargo test 改为隔离 `CARGO_TARGET_DIR` + `--features r09-host,e2e-test`；
  T07 保持只读；T06 后续 v3.3 重建前 target/debug 从稳定副本恢复（`030607ee…`/`513aba98…`）。
- 纪律：`target/debug/lingxi-resource-desktop` 是证据路径；除 `tauri-build.mjs` 外任何 cargo/tauri 构建
  不得写该路径；测试构建一律隔离 CARGO_TARGET_DIR；消费只认 handoff sha + 稳定副本。
