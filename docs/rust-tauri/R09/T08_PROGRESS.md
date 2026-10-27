# R09-T08 进度：Tauri 替壳关卡与交接（A15 / A16）

状态：**IN_PROGRESS / v3.2 候选自检完成；T07 hidden H1/H2/H3 全部 PASS 并绑定；最终证据待 T01/T05/T03/T04 v3.2 重跑 + visible 批次 + 正式门 in-gate 重跑**
日期：2026-10-26（v3.2 候选 frozenAt 23:54:50）| 分支 `codex/rust-tauri-migration` | 基点 HEAD `aac8cb9fe6bb`
实现者：`r09-t08-parity` | 生产者：`scripts/rust-tauri/r09_t08.py`（经 `r09_suites.py` 分派）
自检证据：`artifacts/rust-tauri/R09/dev/t08/v32-round-01/{01-shell-parity,02-host-completeness}/`
候选权威：`artifacts/rust-tauri/R09/dev/t06/candidate-handoff-v3.2/candidate-handoff.json`（= `docs/R09_HANDOFF.json.frozenCandidate`）
交付建议：本文件 + `T08_PARITY_PLAN.md` + `T08_SHELL_PARITY_RESULTS_SUGGESTION.json`（Lead 汇总进 `SHELL_PARITY_RESULTS.json`/`R09_REPORT.md`）

> 本任务全程**未启动 Tauri/Electron GUI、未构建 desktop/src-tauri、未写冻结候选**；结论来自既有真实证据文件字节 +
> 两套件真实退出码/cases。v1/v2/v3/v3.1 均已作废；**v3.2 为唯一受测候选**。
> **pending 不写 PASS、blocked 不写 PASS、旧候选证据按 `evidence::candidate-binding::*` 显式红。**

## 1. 交付物与证据

| 项 | 路径 | 说明 |
|---|---|---|
| 生产者 | `scripts/rust-tauri/r09_t08.py` | 两套件（A15/A16）；唯一写者 r09-t08-parity；支持 candidate-handoff-v* 动态发现、源码/产物漂移检测、v3.1+ 条件注入成员（getFilePath）解析、PlatformApi 期望差记录 |
| A15 自检 | `artifacts/rust-tauri/R09/dev/t08/v32-round-01/01-shell-parity/` | `result.json`（FAIL exit 2 / **342 cases**）、`cases.json`、`shell-parity-cases.json`、`parity-matrix.json`、`SHELL_PARITY_RESULTS.r09_t08_shell_parity.json` |
| A16 自检 | `artifacts/rust-tauri/R09/dev/t08/v32-round-01/02-host-completeness/` | `result.json`（FAIL exit 2 / **323 cases**）、`cases.json`、`host-cases.json`、`host-matrix.json`（含 `platformApi`）、`placeholder-scan.json`、`context-menu-behavior.log` |
| 历史过程证据 | `dev/t08/{selfcheck-platform-api-01/02,v2-selfcheck-*,v3-round-01,v31-round-01}/` | v1→v2→v3→v3.1 过渡/漂移/修复的真实记录，不作最终证据 |
| SHELL_PARITY_RESULTS 建议 | `docs/rust-tauri/R09/T08_SHELL_PARITY_RESULTS_SUGGESTION.json` | v3.2 版（schema `lingxi.r09.shell-parity.v1`，含 fixedHashes/verdict/leafParity/v32Resolved/pendingBeforePASS） |
| GUI A/B 计划 | `docs/rust-tauri/R09/T08_PARITY_PLAN.md` | 修订版 visible **5 次启动**（V1 release_offline / V2 G2 / V3 react_parity 双壳 / V4 T03 actions；负向移 hidden），等 Lead clearance |

## 2. 方法（固定唯一变量＝宿主）

### 2.1 A15 `r09_t08_shell_parity`

- **固定因子（v3.2，只读复算，全部 PASS）**：
  - delivery release：appTree `2008b71f…`、host exe `8e74aa15…`、sidecar `55d6edb1…`（contentSha256 `73471fbf…`，
    wire `1..=1` dataEpoch 2）、build.json `372c2b88…`、release receipt `167ec17e…`、features `[r09-host]`、codesign verify exit 0；
  - G1 evidence：exe `030607ee…`、receipt `513aba98…`、features `[e2e-test,r09-host,r09-test-hooks]`；
  - renderer：digest `b0a20250…`（含页面 meta CSP asset 修复；磁盘树复算一致）/theme `455d9639…`（build-once-bind-once）；
  - 夹具：v3Base `80a8d2c2…`（R08 a16-versions + T07 fixtures case）；
  - R08 基线：`verify-r08-closeout-08` overall PASS（testedSha `aa99d2ec`、binding `0f07f2f7…`、A/B 71 tasks = 68 parity + 3 recorded + 0 differs）。
- **候选动态加载**：`LINGXI_R09_T08_HANDOFF` env → 最新 `candidate-handoff-v*`（按版本倒序，v3.2 > v3.1 > v3 > v2）→ docs
  `R09_HANDOFF.json.frozenCandidate` → v1 回退；`fixedFactors.candidate`/`host-matrix.candidate` 记录版本/来源/frozenAt。
- **漂移检测（v3.2 自检：无漂移）**：非生成源 fingerprint `3ff76056…`（88 文件）；release/evidence 构建后 0 个非生成源文件更新
  （Cargo.lock 仅 mtime 触碰且内容与 handoff dependencyLocks 一致）；renderer 源码 0 个晚于 dist 构建；交付产物 0 个晚于 frozenAt。
- **evidence 路径污染与恢复（真实事件，已在本次重跑确认）**：00:02 `target/debug/lingxi-resource-desktop` 被外部直接 cargo build
  覆盖为 `7b116ea4…`（hooks-only 形态），wrapper receipt 仍为冻结值 → T08 `fixed::evidence-exe-sha` 曾红；host-core 从 T06 ACL
  冻结窗口备份找到逐字节 `030607ee…` 副本恢复 target/debug，receipt 恢复 `513aba98…`、renderer 仍 `b0a20250…`；
  T08 于 00:07 重跑 `fixed::evidence-exe-sha`/`fixed::receipt-sha::evidenceBuild` **PASS**。稳定副本：
  `/private/tmp/lingxi-r09-evidence/lingxi-resource-desktop-030607ee`（sha `030607ee…`）；**后续 gate/wrapper 只从该副本取，
  不再直接对 desktop crate cargo build**（避免再次覆盖证据路径）。gate wrapper 已备：
  `/private/tmp/lingxi-r09-t07/evidence-app-gate-20261027-000648/Lingxi.app`（manifest `dev/t07/gate-wrapper-01`；主 exe 签名后 `992f0a7b…`，
  候选身份以 `030607ee…` 为准）。T01/T02/T07/T03 的 visible env 将统一指向该 wrapper。
- **实测 A/B 行（13 行全 parity）**：T01 同窗 Electron/Tauri（同一 renderer+同一 `LINGXI_SERVICE_BIN`）renderer 身份/DOM 投影/
  嵌入页/服务身份/退出与隔离；T02 supervisor READY 握手/identity-health/协议·摘要·身份门/退出所有权/错误面；T05 旧链 vs Rust 链
  35 项逐项、T05 恶意预览；T07 G1-hidden 候选真实启动（tauri://localhost/优雅退出/零残留/夹具锚点）。
- **50 能力 / 73 命令 / 46 共享叶矩阵**：状态仅 `parity/recorded/blocked/pending/excluded_non_host`；命令行为状态继承能力、
  另行声明注册面；46 叶按 T07 映射（30 宿主 + 16 SERVER_REACT 单列 excluded）。
- **差异登记**：`RECORDED_APPROVALS` 8 项（页面来源 file://→tauri://、托盘修复项移除、R10 更新链 deferred、截图 deferred、
  卡片导航 guard flag=false、getFilePath 无等价、onboarding 两侧一致 NOT_IMPLEMENTED、T03-FILE-01 文件能力 deferred），
  每项 basisPath+逐字引文由生产者校验；`unapprovedDifferences=[]`。
- **产出**：`parity-matrix.json`（measuredRows 13 / capabilityRows 50 / commandRows **73** / leafRows 46 / approvals / summary）、
  `shell-parity-cases.json`、`SHELL_PARITY_RESULTS.r09_t08_shell_parity.json`（docs 建议稿）。

### 2.2 A16 `r09_t08_host_completeness`

- **输入账**：T03 inventory 50 能力/110 API 成员、v3.2 command policy（**73 命令/8 窗口**）、74 叶/168 断言、T03 计划 30 叶/60 断言、
  T07 46 叶分类、R00 37 入口、T06 GUI 批次 25 项、BLOCKED/T06_PROGRESS 登记。
- **逐能力五维核对**：真实实现入口（native 模块/命令/插件/窗口；73 命令 fn 符号存在）、真实消费者（inventory + 渲染层 PlatformApi
  接线）、capability/permission（四集合 73=73=73=73 独立解析 + policy 窗口授予）、证据 maturity、平台状态；未实现且未登记 → FAIL。
- **platformApi 矩阵（v3.2）**：109 成员，实际解析 **76 real-executable（含 getFilePath 条件注入 1）/ 0 explicit-noop /
  5 absent-optional-reasoned / 28 restricted-reasoned / 0 silent**；`hostCoreExpected`=76/0/6/27（HOST_V2_PATCHES §8.3），
  `expectationDelta={absentOptionalWithReason:-1, restrictedWithReason:+1}`——差异为 `onAutoUpdateState`：我方按渲染层真实行为
  归 restricted（Proxy 调用抛 HostCapabilityError+理由），host-core 文档口径归 absent-optional；已在 matrix 显式披露，不硬断言数字。
- **v3.2 能力接线**：6 能力族 renderer 成员已接线（file-text-io/detached-viewer/skill-viewer/file-watch/file-drop-paths = wired；
  browser-viewer 细粒度成员按 T05 裁决显式受限+理由 = restricted-registered）；workspace/viewer-closed/show-skill-viewer 已接线；
  命令面 71→73（新增 watch_workspace/unwatch_workspace）。
- **context-menu 真实行为**：`npx vitest run … -t 右键策略` 真实执行 exit 0（非编辑区 preventDefault=true、textarea 保留系统菜单），
  日志 `context-menu-behavior.log`；Rust devtools=false 由 T06 audit 静态覆盖。
- **占位/空实现/隐藏按钮扫描**：生产 Rust 源 `NOT_IMPLEMENTED`/`todo!`/`unimplemented!` 13 处 = 登记/允许 10（errors 常量、
  ipc_negative_matrix 测试矩阵、onboarding→**T06-ONBOARD-01 已登记**、screenshot→T04-screenshot deferred）+ fail-closed 3
  （tray 图标缺失显式拒绝）；**unregistered=0**；无 trivial 命令体；no-op/absent 均有理由；**UI dead entrypoints=0**。
- **73 命令逐条矩阵**：四集合一致 73/73/73/73；实现符号存在；deferred 未 grant/未进 release；debug 两命令仅 hooks 构建+dev capability。
- **T02 语义证据（v3.2 hidden）**：T07 H2/H3 以 T02 suite/case 名产出并 PASS（H3 覆盖 release 正/负向、failure-dialog-suppressed、
  no-node-fallback、release 拒远端、pid-reuse、packaged SIGTERM、no-orphan），T08 已将 H2/H3 作为 T02 套件的 v3.2 hidden 证据合并
  （`HIDDEN_SUPPLEMENTS`），A15 measured T02 行与 A16 server-connection-info/app-lifecycle-quit 证据均绑定 v3.2。
- **37 R00 入口**：9 host 相关（desktop_shell 7 + renderer_clients 2）全部映射/登记；28 非宿主显式归 Rust 服务/CLI。
- **74 叶逐叶状态**：bound 15 / pending_gui 23 / manual_pending 4 / blocked 12 / excluded_non_host 16 / evidence_green_unbound 4。

### 2.3 证据口径（防假绿）

- 上游证据同门优先（`{EVIDENCE}/SUITES/<suite>/`），dev 自检回退并记录 `sourceKind=dev-fallback`；
- `evidence::candidate-binding::<suite>`：receipt 有 rendererDigest 必须等于当前 renderer；dev 回退证据必须晚于候选冻结；
- pending 项 case ok=false（exit 2）；blocked 项必须能在 BLOCKED.md/T06_PROGRESS/T03-T04/T05/T06 GUI 清单找到登记；
- Producer 内建真实 `~/.lingxi` 前后摘要断言。

## 3. v3.2 自检结果（真实执行，2026-10-26 23:5x；无 GUI/无构建）

### 3.1 两套件

| 套件 | 命令 | exit | cases | 关键产物 |
|---|---|---|---|---|
| A15 | `LINGXI_R09_T08_AUTO_LATEST=1 python3 -B scripts/rust-tauri/r09_suites.py --suite r09_t08_shell_parity --evidence artifacts/rust-tauri/R09/dev/t08/v32-round-01/01-shell-parity` | **2（FAIL，如实）** | **342** | parity-matrix（13/50/73/46）、SHELL_PARITY_RESULTS 建议稿 |
| A16 | `… --suite r09_t08_host_completeness --evidence …/v32-round-01/02-host-completeness` | **2（FAIL，如实）** | **323** | host-matrix（50/73/37/74 + platformApi 109）、placeholder-scan |

### 3.2 A15 矩阵摘要

| 维度 | parity | recorded | blocked | pending | 其他 |
|---|---|---|---|---|---|
| measured A/B | **13** | 0 | 0 | 0 | differs 0 |
| 50 能力 | **6** | 8 | 3 | 33 | unapproved 0 |
| 73 命令（表面） | 注册面 73/73 | — | — | 行为随能力 | — |
| 46 共享叶 | 0 | 0 | 4 | 26 | excluded_non_host 16 |

- 能力 parity 6：main-window / app-ready / server-connection-info / app-info-platform / app-lifecycle-quit /
  security:release-no-test-control；recorded 8 = RECORDED_APPROVALS；blocked 3 = auto-launch(signing)/notification(T03-NOTIFY-02)/
  speech(BLOCKED-SYSTEM-PERMISSIONS)。
- `pendingCount=59`（33 能力 + 26 叶）；`unapprovedDifferences=[]`；fixed 全部 PASS；
  candidate-binding 红 9 条 = 旧候选/旧批次 dev 证据（T01×2/T05×2/T03×2/T04×2 + T07 release smoke；T07 hidden H1/H2/H3 已绑定 PASS；T06 v3.2 静态门在冻结窗口内；正式门 in-gate 自动满足）。

### 3.3 A16 摘要

| 维度 | 结果 |
|---|---|
| 50 能力 a16Ok | **25 / 50**（failed 25；见 §4，全部为运行期证据 pending） |
| 73 命令 | 四集合 73/73；runtimeAcl=`blocked_registered_by_T06`（21 runtime ACL 项待 hidden） |
| 37 R00 入口 | 37 traversed；host 相关 9 全映射/登记；28 非宿主显式归类 |
| 74 叶 | bound15 / pending_gui23 / manual_pending4 / blocked12 / excluded_non_host16 / evidence_green_unbound4 |
| platformApi | 109 = 76 real（含 getFilePath 条件 1）/ 0 noop / 5 absent+reason / 28 restricted+reason / **0 silent**（期望差记录见 §2.2） |
| 占位扫描 | NOT_IMPLEMENTED 13 处：登记/允许 10、fail-closed 3、**unregistered 0**；UI dead entrypoints 0；无空实现命令 |
| 平台矩阵 | macos-arm64 AVAILABLE；macos-x64/windows-x64/linux-x64 = BLOCKED_PLATFORM（BLOCKED.md 登记）；v3.2 audit 58（含 page-meta-csp-allows-asset）/scan 45/release suite 51 PASS |

## 4. 仍未满足项（v3.2 自检红的原因）

### 4.1 等 hidden 批次

1. **T06 运行期 ACL**：`r09_t06_capability_matrix` exit 2（仅 21 项 `blocked::runtime.*`）；v3.2 静态全 PASS
   （audit 57 / scan 45 / release suite 51）。完成后 A15 `security:*` 行为对照与 A16 runtimeAcl 列可评估。
2. **T03 IPC 负向 3 / T04 remote 3**：真实 invoke/ACL/进程探针与双宿主 E2E。
3. **T07 hidden v3.2 全部完成并绑定**：H1 `g1-hidden-v32-02` PASS 30 cases（file-url-asset img onload/context-menu/shell/IME/选区/DPI/焦点/graceful/ps-clean）；
   H2 `h2-supervisor-02` PASS 10 / `h2-ownership-02` PASS 13；H3 release hidden `h3-supervisor-01` PASS 31 / `h3-ownership-01` PASS 19。
   T08 loader 已把 H2/H3 作为 T02 套件的 v3.2 hidden 证据合并（suite/case 名一致）；T02 语义（READY/协议·摘要·身份拒绝/no-node-fallback/owned+remote/reused/remote zero-touch/pid-reuse/no-orphan）已在 v3.2 复核。
4. **T01/T03 v3.2 证据 = 可见/最终门批次产出**（host-core 2026-10-27 00:0x 同步）：
   - `r09_t01_react_parity`/`r09_t01_release_offline`：需可见窗口（`LINGXI_R09_T01_VISIBLE=1` + delivery/wrapper env），由 host-core 在 visible 批次跑；
   - `r09_t03_native_matrix`：需 `LINGXI_R09_GUI_CLEARED=1` + wrapper + webdriver + manual dir；8 条 blocked 断言使其 exit2（已登记，不假绿）；
   - `T05/T04` 不是 host-core 生产者，其 v3.2 重跑由 owner/Lead 安排；
   - 以上 dev 证据当前按 candidate-binding 红处理，正式门 in-gate 会自动落 `{EVIDENCE}/SUITES/...` 且同轮绑定。
5. **T07 release smoke = 显式登记红（gate 不可能 PASS 的已知原因之一）**：`t07::release.file-task` PENDING（delivery release
   无 hooks/wdio，无法驱动真实读写 UI），该命令在正式门也将 exit2；按登记保留红，**不为过门改绿**。T07 G1（H1）已 PASS，
   v3.2 wrapper 证据在 `g1-hidden-v32-02`。

### 4.2 等 visible 批次（Lead 已定修订版 5 次启动；见 T08_PARITY_PLAN.md）

- V1 release_offline（1）/ V2 G2 交付 release 冒烟（1）/ V3 react_parity 双壳可见 A/B（2）/ V4 T03 gui-actions（1）；
  负向/失败语义全部移 hidden（无预期弹窗）。
- 用途：A16 剩余 25 个运行期证据能力（24 项 wired/host-internal + browser-viewer restricted-registered）、
  T03 26 宿主叶真实动作、T04 TCC/manual、T05 viewer 4 叶、T07 A13/release smoke；3 条 manual 断言人工证据。
- 等 Lead clearance；T08 不发起启动。

### 4.3 file-url-asset —— 已由 T07 v3.2 H1 解决（保留记录）

v3.2 修复页面 meta CSP asset（H1 root cause）；T07 `g1-hidden-v32-02` PASS 30 cases 中 `t07::native-e2e.file-url-asset`
为真实 `convertFileSrc → <img> onload`（scope 内 fixture 1×1 PNG 加载成功，height=1）；A16 `host:file-url-asset` a16Ok=True、gaps 清零。

### 4.4 已由 v3.1/v3.2 解决（保留记录）

- 6 能力族 renderer 成员接线：file-text-io/detached-viewer/skill-viewer/file-watch/file-drop-paths = wired；
  browser-viewer 细粒度命令按 T05 裁决（`browser_go_*`/`browserEmergencyStop` 等收敛为 `rust_browser_request` action）显式受限+理由。
- 命令面 71→73（watch_workspace/unwatch_workspace）；host no-op 归零（onShowSkillViewer 转真实）。
- onboarding_complete 登记 T06-ONBOARD-01；占位扫描 unregisteredGaps=0。
- 期望差 1（onAutoUpdateState 分类口径）已在 matrix 记录，不影响功能与静默性（0 silent）。
- **v3.2**：页面 meta CSP asset 修复（H1 root cause）进入 renderer `b0a20250…`；audit 58 含 `page-meta-csp-allows-asset`；
  T07 hidden v3.2 H1/H2/H3 全 PASS（H1 30 cases、H2 10+13、H3 31+19），file-url-asset/context-menu 与 T02 语义（含 release 负向）真实运行期证据已绑定。

### 4.5 其他真实红/边界

- T07 `r09_t07_release_smoke` 未运行（v1 static 红）；需交付 release 冒烟。
- 平台：macOS x64 / Windows x64 / Linux x64 无真机 → BLOCKED_PLATFORM。
- BLOCKED-SYSTEM-PERMISSIONS / BLOCKED-SIGNING-IDENTITY / BLOCKED-REAL-PROVIDER 未解除。

## 5. 平台矩阵（v3.2 自检）

| 平台 | 状态 | 证据 |
|---|---|---|
| macOS arm64（macOS 27.0.1 build 26A434，M3 Ultra） | AVAILABLE / v3.2 自检实跑（无 GUI） | v3.2 handoff 静态复算 + T01/T02/T05/T06/T07 既有证据 |
| macOS x64 | BLOCKED_PLATFORM | `BLOCKED-PLATFORM-MACOS-X64` |
| Windows x64 | BLOCKED_PLATFORM | `BLOCKED-PLATFORM-WINDOWS-X64` |
| Linux x64 | BLOCKED_PLATFORM | `BLOCKED-PLATFORM-LINUX-X64` |

## 6. SHELL_PARITY_RESULTS.json 最终填充建议（v3.2）

见 `T08_SHELL_PARITY_RESULTS_SUGGESTION.json`，要点：

- `status` 当前建议 `DRAFT_T08_V32_SELFCHECK_PENDING_GUI_EVIDENCE`；**只有 pending=0 且 unapprovedDifferences=0** 才可
  `READY_FOR_REVIEW`/PASS；不得把 blocked/pending 写 parity。
- `fixedHashes`（v3.2）：rustServiceSha256 `55d6edb1…`、contentSha256 `73471fbf…`、rendererDigest `b0a20250…`、
  themeBundleSha256 `455d9639…`、fixturesManifestSha256 `80a8d2c2…`、electronBaselineReceipt=verify-r08-closeout-08
  （aa99d2ec/0f07f2f7/71=68+3+0）、tauriCandidateReceipt（appTree `2008b71f…`、hostExe `8e74aa15…`、G1 `030607ee…`）。
- `recordedApprovals` 8 项；`unapprovedDifferences=[]`；measured parity 13；能力 parity 6/recorded 8/blocked 3/pending 33；
  73 命令表面；46 叶 excluded16/blocked4/pending26；hostPlaceholders（unregisteredGaps 0）；leafCoverage bound 15。
- `v32Resolved` 记录 6 族接线/命令面 73/PlatformApi 实际数字/页面 meta CSP 修复/T07 hidden H1–H3 PASS；`pendingBeforePASS` 见 suggestion。

## 7. R10 交接素材（构建 / 安装 / 调试 / 故障排查）

> 任务书 T08 步骤 4 要求；均来自 R09 已落盘文档与证据，不新增结论。

### 7.1 构建（v3.2 冻结后不得重建；重建=新候选+全部证据重跑）

```bash
npm run build:renderer && npm run build:theme          # build-once-bind-once，两壳共用
node scripts/tauri-build.mjs --release --no-renderer-build --receipt <evidence>/tauri-build-release.json
node scripts/tauri-build.mjs --debug --features r09-test-hooks,e2e-test --no-renderer-build --receipt <evidence>/tauri-build-debug.json
node scripts/build-rust-desktop-service.mjs --verify   # 服务 stage + build.json(protocol/contentSha)
```

v3.2 产物：`desktop/src-tauri/target/release/bundle/macos/Lingxi.app`（Contents/MacOS/{lingxi-resource-desktop,
lingxi-service}、Resources/rust-service/build.json、_CodeSignature/CodeResources、Info.plist）；
`desktop/src-tauri/target/debug/lingxi-resource-desktop`（G1 evidence）；回执在 `desktop/src-tauri/.tauri-build/`。

### 7.2 安装/权限

- ad-hoc 签名（codesign --verify --deep --strict exit 0）；无 Developer ID/公证 → `BLOCKED-SIGNING-IDENTITY`；
  安装验证只在隔离目录/隔离 `LINGXI_HOME`。
- Info.plist 含麦克风/语音识别用途说明；TCC 授权/撤销需人工（`BLOCKED-SYSTEM-PERMISSIONS`）。
- 自启动（SMAppService）与正式签名形态相关；R09 只做状态读写与显式 unsupported/registered 语义。

### 7.3 调试/故障排查（R09 实际踩坑）

| 现象 | 根因/处理 |
|---|---|
| 首屏白屏/ErrorBoundary | WKWebView i18n locale 更新早于 tiptap view 就绪；InputArea 两处防御（T01 白名单业务组件改动） |
| `Unexpected token '<'`/theme 丢失 | 构建链缺 `build:theme`（lib/theme.js）；wrapper 固定 renderer→theme 并校验 theme sha |
| READY 失败/协议不符 | supervisor 用 Rust 常量协议门（wire 1..=1/epoch2），显式 `RUST_SERVICE_PROTOCOL_INCOMPATIBLE`/`DIGEST_MISMATCH`/`RUNTIME_IDENTITY_MISMATCH`，release 不回退 Node |
| 自动化弹窗/残留 | `LINGXI_R09_AUTOMATION=1`+`LINGXI_R09_HOST_DIALOG_SINK`（r09-test-hooks feature，仅 evidence 构建）；套件 finally 清理 + 进程表断言 |
| renderer 摘要跨构建不稳定 | Vite settings.html CSS link 顺序非确定 → build-once-bind-once；R10 发布复现性已知项 |
| ad-hoc 后 sidecar sha≠manifest | contentSha256 复算一致 + codesign 通过放行（signedMutation） |
| asset:// 图片失败 | v3.1 scope+CSP；v3.2 修页面 meta CSP asset（H1 root cause）；运行期证据待 T07 v3.2 H1 |
| GUI 自动化 | 仅 e2e-test feature 构建；交付 release strings 中 wdio/devUrl/5173=0（T06 scan PASS） |
| 候选被后续构建覆盖 | T08 漂移检测（非生成源 mtime、renderer 源码 vs dist、产物 vs frozenAt）会立即红；v3→v3.1 已实际发生并如实记录 |

### 7.4 T08 复跑

```bash
export LINGXI_R09_T08_AUTO_LATEST=1   # dev 自检：自动取最新 suite 证据；正式门不需要
export LINGXI_R09_T08_HANDOFF=artifacts/rust-tauri/R09/dev/t06/candidate-handoff-v3.2/candidate-handoff.json  # 可选显式指定
python3 -B scripts/rust-tauri/r09_suites.py --suite r09_t08_shell_parity \
  --evidence artifacts/rust-tauri/R09/dev/t08/<run>/01-shell-parity
python3 -B scripts/rust-tauri/r09_suites.py --suite r09_t08_host_completeness \
  --evidence artifacts/rust-tauri/R09/dev/t08/<run>/02-host-completeness
# 正式门（最终证据；全新目录，in-gate 自动使用同轮上游证据）
~/.cargo/bin/rustup run 1.98.1 cargo run --manifest-path rust/Cargo.toml -p xtask -- \
  verify-stage R09 --evidence artifacts/rust-tauri/R09/<final-run>-<nn>
```

## 8. 写作用域/纪律

- 本轮只写：`scripts/rust-tauri/r09_t08.py`、`docs/rust-tauri/R09/T08_*`、`artifacts/rust-tauri/R09/dev/t08/`；
- 未改 `desktop/**`、`rust/**`、Lead 的 `PROGRESS*/BLOCKED/PLAN/REPORT/HANDOFF`、其它 R09 生产者/地图；
- 未启动任何 Tauri/Electron GUI、未构建 desktop/src-tauri（只读证据 + 静态分析 + vitest jsdom 行为断言）；
- 不提交/push/发布；不迁移真实数据（生产者内建真实 `~/.lingxi` 前后摘要断言）。

## 9. 版本沿革（T08 输入变化）

| 候选 | handoff | 结果 | T08 处理 |
|---|---|---|---|
| v1 | candidate-handoff-02 | appTree `2dc38bc3…` | T08 首轮自检后作废（T03/T04 片段未 merge） |
| v2 | candidate-handoff-v2 | appTree `d01ece20…`、renderer `c0a5719f…` | 片段 merge；T08 v2-round-01 自检（失败 6 族未接线） |
| v3 | candidate-handoff-v3 | appTree `266b1f4b…`、renderer `60a6591c…` | 被 v3.1 构建覆盖（T08 漂移检测抓到） |
| v3.1 | candidate-handoff-v3.1 | appTree `1bae1e28…`、renderer `cba1d4f3…`、命令 73 | H1 file-url asset 探针失败（页面 meta CSP root cause，v3.2 修复）；v31-round-01 自检历史 |
| **v3.2** | candidate-handoff-v3.2 | appTree `2008b71f…`、renderer `b0a20250…`、命令 73、页面 meta CSP asset 修复 | **当前受测候选**；v32-round-01 自检；audit 58；T07 hidden H1–H3 PASS |
