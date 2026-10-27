# R09 进度（Tauri 桌面宿主、原生能力与跨平台集成）

状态：**IN_PROGRESS**（实施中；任何场景在真实证据登记前不得写 PASS）
开始：2026-10-26 | 分支 `codex/rust-tauri-migration` | 基点 HEAD `aac8cb9fe6bb7bfa7a41d75528221136bfe55e50`
任务书：`Lingxi_Rust_Tauri_Taskbooks_2026-09-23/R09_Tauri桌面宿主、原生能力与跨平台集成.md`

## 1. 冻结/开工事实

- 原账独立核对（本阶段起点，不从剩余任务反推）：8 任务 / 16 基础场景 / 74 去重补充叶 / 168 条原断言；
  T03 与 T07 共享 46 叶（按叶 ID 去重只计一次），T05 19 叶，T04 9 叶。
- 工具链：`rustup 1.98.1`（`~/.cargo/bin`；默认 PATH 的 Homebrew rustc 1.93.0 **不读** `rust-toolchain.toml`，
  R09 所有 Rust 命令一律 `~/.cargo/bin/rustup run 1.98.1 cargo …`）。Node v24.16.0；python3 3.14.3。
- 平台：仅 macOS arm64（27.0.1）真机；mac x64 / win x64 / linux x64 无机器 → `BLOCKED.md` 平台阻塞（不改写成不支持）。
  Xcode 仅 CLT 27.0（无 Xcode.app）；签名/公证身份缺失 → 本地 ad-hoc 安装包，不冒充正式分发签名。
- 现状：`desktop/src-tauri` 仅为 R06 文件资源宿主（tauri 2.12.1、独立 `[workspace]`、`bundle.active=false`、
  `frontendDist=../dist-tauri-resources`）；完整 React `DesktopHost-Tauri`、服务 supervisor、原生能力矩阵均未接线。
- R08 交接边界：正式门 verify-r08-closeout-08 PASS（22/22 命令、16/16 场景、126/126 叶、binding stable）；
  独审 ADMIT_R09。R08 testedSha=aa99d2ec+工作树，不等于当前提交；仅对 R09 有实质影响的变化做定向复核（本阶段未重开 R08 全面审计）。

## 2. 本轮已完成（2026-10-26 第一轮）

| 项 | 内容 | 证据 |
|---|---|---|
| R09 地图/证据合同 | `rust/crates/xtask/src/stage_maps/R09.json` + `R09_INPUTS.json`：16 场景（specMirror 全量镜像 R00）+ 74 叶 R00 镜像（全部 `required_behavior_pending`）+ 18 命令；`inputDigestSha256`/整图字节自检通过 | `python3 -B scripts/rust-tauri/r09_stage_inputs.py --check` exit 0；R09.json sha256 `d127193c…`；R09_INPUTS.json sha256 `9ec143a5…` |
| 生成器（唯一来源） | `scripts/rust-tauri/r09_stage_inputs.py`：独立读取 R00 两账校验 16/74/168 与任务份额；未绑定叶保持 BLOCKED；禁止命令只注册不引用/引用未注册/空场景 | 同上 |
| xtask 注册 | `main.rs` STAGE_MAPS 增加 R09；`runner_identity.rs` 嵌入 R09.json/R09_INPUTS.json + 源码清单；新增 `check-map <STAGE> [--map FILE]` 子命令（与 verify-stage 共用 cross-check，不执行命令） | xtask 1.98.1 编译通过；`check-map R09` exit 0（16 场景/18 命令/74 叶/168 断言摘要） |
| 地图完整性负例 | `scripts/rust-tauri/r09_map_integrity.py`：19 case（正例 + 缺 1 叶/删 T04 9 叶/删 T03-T07 共享 46 叶/编造叶/改写原断言/基础镜像漂移/孤儿命令/引用未注册命令/空场景/逐断言覆盖不全/命令无产物 + 源码漂移 + 源码树还原 + 原图仍有效）真实调用 `xtask check-map` | 独立运行 `/private/tmp/r09-map-integrity-probe-03`：19/19 PASS；亦作为门内命令 `r09_map_integrity`（绑定 R09-A15 逐断言合同） |
| 红基线正式门 | `verify-stage R09 --evidence artifacts/rust-tauri/R09/verify-r09-map-red-02`：overall **FAIL**（预期红）、candidate binding **stable=true**、18 命令中 2 PASS（inputs_check、map_integrity）/16 FAIL（生产者 `NOT_IMPLEMENTED` 真实 exit 2）、16/16 场景 FAIL、74/74 叶 BLOCKED（非伪造 PASS） | `artifacts/rust-tauri/R09/verify-r09-map-red-02/verify-stage-result.json` |
| 机器清单 | `docs/rust-tauri/R09/R09_LEAF_LEDGER.json`：74 叶逐条「原 ID/全部断言（168）→ 实现入口 → 真实消费者 → 计划套件/命令/证据 → 平台/状态」 | 同文件；sha256 `49dd904d…` |
| 阻塞/回执 | `START_RECEIPT.md`（≤10 行）、`BLOCKED.md`（平台/权限/签名/真实账号，含解除条件与责任） | 同目录 |

## 3. 门禁事实（复跑命令）

```bash
# 地图/证据合同自检（生成器与图逐字节一致）
python3 -B scripts/rust-tauri/r09_stage_inputs.py --check
# 地图合同（不执行命令；负例探针用 --map 指向临时损坏样本）
~/.cargo/bin/rustup run 1.98.1 cargo run --manifest-path rust/Cargo.toml -p xtask -- check-map R09
# 地图完整性负例（真实调用 check-map；写 cases/result/log）
python3 -B scripts/rust-tauri/r09_map_integrity.py --evidence /private/tmp/r09-map-integrity-<n>
# 正式阶段门（证据目录必须全新；已存在非空目录会被拒绝）
~/.cargo/bin/rustup run 1.98.1 cargo run --manifest-path rust/Cargo.toml -p xtask -- \
  verify-stage R09 --evidence artifacts/rust-tauri/R09/verify-r09-<round>-<nn>
```

## 4. 任务状态

| 任务 | 状态 | 说明 |
|---|---|---|
| R09-T01 薄 Tauri 宿主 + 同一 React 构建 | IN_PROGRESS | 设计调查中；`desktop/src-tauri` 扩展、DesktopHost-Tauri、dev/release 配置 |
| R09-T02 服务启动/就绪/退出/远端模式 | IN_PROGRESS | 设计调查中；supervisor、握手、错误二进制拒绝、进程所有权 |
| R09-T03 桌面基础能力等价 | PLANNED | 能力清点进行中；46 叶共享 T07 |
| R09-T04 语音/屏幕/辅助功能权限 | PLANNED | 9 叶；真实撤销权限需人工（BLOCKED-SYSTEM-PERMISSIONS） |
| R09-T05 浏览器/PDF/媒体最终接线 | IN_PROGRESS | 设计调查中；19 叶 |
| R09-T06 权限/CSP/自定义命令收口 | PLANNED | 依赖 T01/T03 命令面成型 |
| R09-T07 跨 WebView/原生 E2E | BLOCKED_PARTIAL | 本机 macOS arm64 可跑；其余三平台 BLOCKED_PLATFORM |
| R09-T08 Tauri 替壳关卡/交接 | PLANNED | 依赖 T01–T07 证据 |

## 5. 实施约定（防假绿/防覆盖）

- 阶段图、`package.json`、锁文件、共享入口（`main.rs`/`runner_identity.rs`/`verify.rs`）由 Lead 唯一维护；
  任务实现者只改各自任务文件与证据生产者（`scripts/rust-tauri/r09_t0X.py`），不手改 `R09.json`/`R09_INPUTS.json`。
- 叶证据只在 `scripts/rust-tauri/r09_stage_inputs.py` 的 `FULL_BINDINGS` 登记（唯一来源），逐条原断言 →
  专属 case（正路径 + 失败/拒绝分支）；一次批量放行视为违规。
- 每个套件真实写 `result.json` + `output.log`（+ 逐断言 `cases.json`，schema `lingxi.leaf-case-results.v1`）；
  禁止 mock 被测宿主/权限边界、禁止 skip 当通过、禁止 `|| true`、禁止用旧日志。
- 证据目录一律 `artifacts/rust-tauri/R09/<run>-<nn>` 全新目录；代码/测试/地图/配置改动后必须结束本轮形成新候选重跑。
- 运行时隔离：`LINGXI_HOME`/`TMPDIR` 指 `/private/tmp/lingxi-r09-*`；不触碰真实 `~/.lingxi`（生产者内建前后摘要断言）。
- 不 push、不合并 main、不发布、不迁移真实数据；缺机器/授权写 BLOCKED，不降标准。

## 5.5 第一轮补充（设计调查 / 服务接缝 / 构建探针 / 并行实现）

| 项 | 内容 | 证据/事实 |
|---|---|---|
| 设计调查交付（只读） | `T01_T02_HOST_SERVICE_DESIGN.md`（582 行，28 契约全 file:line + A01–A04 证据计划）、`T03_HOST_CAPABILITY_INVENTORY.json/md`（50 能力/110 API 成员/91 命令/14 OQ）、`T05_BROWSER_PDF_DESIGN.md`（518 行 + 19 叶附录）、两本 `_EVIDENCE.json` | 均在 `docs/rust-tauri/R09/`；**设计≠验收**，未运行 E2E |
| 实施计划/裁决 | `IMPLEMENTATION_PLAN.md`：D1–D10 裁决（保留 R06 base config + dev/release overlay；同产物=dist-renderer；boot 基线；supervisor 复刻；externalBin；Origin/CSP 接缝；浏览器独立 Chromium 窗口；asset 范围；T06 四集合一致）与写作用域单一写者表 | 同文件 |
| 服务 Origin/CSP 接缝（D6，Lead） | `transport.rs::check_origin` 精确允许 `tauri://localhost`/`http://tauri.localhost`/`https://tauri.localhost`（伪造 scheme/子域/带端口仍拒），`preview.rs` frame-ancestors 同步；targeted 单测 3/3 PASS（`transport::tests::origin_policy_allowlist`、`lan_browser_origin_matches_real_interface_scheme_and_port`、`preview::tests::csp_baseline_matches_incumbent`） | 未提交候选；**尚未进正式门**，T01/T05 需以实际候选复验 |
| Tauri 构建探针（R-01） | CLT 27.0 无 Xcode.app 下 `tauri build --debug --bundles app` 成功：20.42s 编译，产出 `Lingxi Resources.app` 32.49MiB；默认 linker-signed ad-hoc，`codesign -v --strict` exit 1（Tauri 需显式 `signingIdentity="-"` 再验签） | `artifacts/rust-tauri/R09/probes/tauri-bundle-probe-01/` |
| Tauri CLI | `@tauri-apps/cli 2.12.4` 固定版本入 devDependencies + lock（BLOCKED-TAURI-CLI-OFFLINE=RESOLVED） | package.json/package-lock.json 已变更 |
| 并行实现者 | `r09-host-core`（T01+T02，owns desktop/src-tauri、DesktopHost-Tauri、build 身份、r09_t01/t02.py）、`r09-browser`（T05，owns browser/PDF/预览服务与证据生产者，宿主注册点等 Lead 合并） | 已启动；不碰地图/门禁/共享入口 |
| R08 基线绑定 | 本机复算：`desktop/dist-renderer` 树摘要、`rust/target/debug/lingxi-service` sha、`shared/contract-versions.json` sha 均等于 R08-A16 基线（由设计调查复算） | T01 接 host 后重建须两侧重新绑定，不沿用旧摘要 |

## 5.55 服务 Origin/CSP 接缝真实服务验证（D6，2026-10-26）

- 新探针 `scripts/rust-tauri/r09_service_seam_probe.py`：隔离空 home 启动**真实** `rust/target/debug/lingxi-service`
  （重建含接缝，sha256 `0e3645e4…`）→ 解析 READY → 真 HTTP 探针 → SIGTERM 清理。
- 13/13 PASS：`tauri://localhost`/`http://tauri.localhost`/`https://tauri.localhost` 健康面 200 + ACAO 回显 + `Vary: Origin`；
  `tauri://evil`、`evil.tauri.localhost`、`tauri.localhost:8080`、`tauri.localhost.evil.example` → 403 `bad_origin`；
  OPTIONS 预检 204/403；认证面 Bearer+允许来源 200、伪造来源仍 403；SIGTERM exit 0 无残留。
- 证据：`artifacts/rust-tauri/R09/probes/service-origin-seam-01/`（result.json sha256 `77f0e1db…`，含逐请求 transcript + service 日志）。
- 边界：CSP `frame-ancestors` 的浏览器侧放行/拒绝由 T05 真 Chrome 探针覆盖；WS Origin 走同一 transport_guard，留 T07/T05 回归补证；
  本证据属候选工作树（非提交、非冻结），正式门绑定以最终候选为准。

## 5.57 并行实现流开工事实（2026-10-26 21:3x–21:4x）

- `r09-host-core`（T01/T02）：新增 `desktop/src-tauri/tauri.dev.conf.json`、`tauri.release.conf.json`（Lingxi/com.lingxi.app/dist-renderer/externalBin/signingIdentity "-"/CSP）、
  `src/commands.rs`、`src/host_boot.rs`、`src/service/`（env/resolve/manifest/digest/instance/client/supervisor/shutdown/remote/diagnostics/error）、`tests/service_supervisor.rs`、
  `capabilities/r09-host.json`、`desktop/src/react/host/tauri-desktop-host.ts` + 单测、`scripts/tauri-build.mjs`/`tauri-dev.mjs`、package.json tauri 脚本；合并中。
- `r09-browser`（T05）：交付 `desktop/src-tauri/src/browser_viewer.rs`（未注册，5 命令+state+事件+CSPRNG key）；证据 `dev/t05/01-a09-base`、`02-a10-preview`、`old-chain-baseline`；
  已恢复 R01/T05 冻结证据（git `15fa0cee3^` blob）并重生成 old 基线（74 页、逐页网格 |Δ|=0.0、PDF 字节数一致）；`scripts/rust-tauri/r09_t05.py` 已建；服务端测试 r07_browser_routes 10/10、r06_t05_preview 5/5、r07_office_tools 15/15、r07_browser_store 3/3 绿，browser_chromium 9/10（cookie-resume 待串行）、pdf_chromium 待 --test-threads=1 重跑。
- `r09-native-caps`（T03/T04）：裁定使用 `desktop/src-tauri/src/native/**` 目录（与 host-core 的 `host_boot.rs` 零冲突），只写模块+单测+生产者+注册清单；T04 5 叶用隔离服务真 HTTP，TCC 正向人工步骤登记。
- Lead：新增 `scripts/rust-tauri/r09_capability_audit.py`（T06 A11/A12 静态半边：invoke_handler/build.rs/permissions/capabilities/ACL/release config 一致性与 per-window 并集+危险权限 denylist；policy 缺失时 exit 3 PENDING_POLICY，不冒称 PASS）。
  中间态审计发现 registered=44 vs manifest=39、ACL 清单陈旧（需合并后 cargo build 重生成）——已反馈 host-core。

## 5.58 并行实现流第二批交付（2026-10-26 21:4x–21:5x）

- **r09-browser（T05）交付**：`r09_t05_browser_pdf` **PASS exit=0 caseCount=71**（85.5s，真实 Chrome 155 + 真实服务 + 真实 Electron 仅旧链对照；证据 `dev/t05/01-a09-browser-pdf-final/`）；`r09_t05_untrusted_preview` **PASS exit=0 caseCount=53**（`dev/t05/02-a10-preview-final/`）。
  A09：旧链 S1 基线重生成 994832B/74 页，与 R01 冻结逐页 16×16 网格 max|Δ|=0.0、文本 sha256 相同；旧链 vs Rust 链 compare_outputs 35/35 all_pass（74 页/140 行/相似度 0.99910/视觉网格 7.15/三族字体）；r07 browser 10/10、PDF 7/7（串行，并行 PDF_CAPACITY 非产品缺陷，红日志保留）。A10：真实服务+Chrome，Origin 3 放行/6 伪造 403；恶意 HTML 预览脚本运行但 Tauri 桥/父窗口访问全拒、fetch 全 TypeError、loopback canary 0、token 零泄露；frame-ancestors 伪造祖先真实拒绝；SVG img 不执行脚本。
  keychain 用户弹窗已止（测试 Chrome `--use-mock-keychain`+进程组清理 8/8 零残留；产品 launcher 默认不变，仅 test-env 生效）。
  `browser_viewer.rs` 已由 host-core 合并注册，`cargo check --features r09-host` exit 0、模块单测 7/7。
  18/19 叶有服务/Chrome 侧证据基础；**逐断言 case 正在补**（Lead 判定当前建议映射不满足"每条原断言专属证据"，已退回扩展 producer），4 叶 viewer/设置/文件预览待 T01/T06。
- **r09-native-caps（T03/T04）交付**：`desktop/src-tauri/src/native/` 35 文件/约 11k 行（31 能力模块 + argument_guard/ipc_negative_matrix），**179 单测全绿**（独立 rustc harness，不依赖 tauri）；TS 能力片段 `desktop/src/react/host/tauri-host-capabilities.ts`（tsc 全仓 exit 0）；生产者 `r09_t03.py`/`r09_t04.py` 与 4 套件 dev 证据。门真红（待注册）：t03_native 71 cases/63 PASS/8 blocked；t03_ipc 53/50/3；t04_permission 40/33/7；t04_remote 23/20/3。T04 真实服务 HTTP 验证 computer-use 偏好/批准/撤销/负向（未触发 TCC 弹窗）。
  注册清单在 `T03_T04_PROGRESS.md` §4；T04 4 叶逐断言映射 §5.1（Lead 待注册转绿后登记）。T03-REG-01 等阻塞如实登记。

## 5.59 T01 首屏崩溃根因与修复（2026-10-26 22:0x）

- **根因**（host-core 实测）：不是 CSP/asset/缺成员。Tauri 下 i18n 加载成功→locale 状态更新；WKWebView 中 tiptap `EditorContent` 挂载时序慢于该更新，`InputArea` effect 里 `editor.view` getter 在 view 就绪前抛 "The editor view is not available"；render/commit 异常先被 `RegionalErrorBoundary region=input` 捕获，随后 `app-init` 调 `setBrowserConnection`（Tauri host 未实现）再次抛错，整树落入 ErrorBoundary（bodyTextLength=56 恰好等于兜底文案）。Electron 不触发是因为 file:// 下 i18n.fetch 失败、locale 数据为空、effect 不重跑。
- **修复**：`InputArea.tsx` 两处防御（view 未就绪跳过 metrics；placeholder refresh try/catch + rAF 重试一次）；`tauri-desktop-host.ts` 可选成员从"Proxy 可调用函数→抛错"改为真正缺失 + capabilityReason（onMaximizeChange/onTrainUpdateAvailable/onTrainUpdateProgress/trainUpdateStatus/onTrainFallbackNotice/ackTrainFallbackNotice/setBrowserConnection），throw 前记录 `window.__r09HostErrors`；debug-only 初始化脚本捕获 error/unhandledrejection/console.error/resource 列表并投影回传（release 不注入）。
- **实测**：同窗对照归一化 innerText：Tauri 121/1065066033 == Electron 121/1065066033；标题/platform 一致；错误列表只剩 shell 共有噪声（见下）。
- **theme.js 404 根因（Lead 裁决）**：`vite.config.theme.js` 产物写 `desktop/dist-renderer/lib/theme.js`，`build:client` 顺序含 `build:theme`；Tauri wrapper 只跑 `build:renderer` 漏了 `build:theme` → dist 缺 theme.js。**不删 index.html 引用**；修 wrapper/renderer 构建步骤为 renderer+theme 并把 theme sha 纳入回执，重建后重跑 A01（同时修复 release 主题切换）。
- 证据目录 `artifacts/rust-tauri/R09/dev/t01/run-06`（17/18 PASS，唯一红=dom parity，根因已定位）；修复后 run-07 待重跑。

## 5.6 R08 交接边界定向复核（2026-10-26，不重开 R08 全面审计）

- 受测候选关系：R08 收据 `verify-r08-closeout-08/verify-stage-result.json` = `testedSha aa99d2ec` + `worktreeDirty=true` + `overall PASS`；
  `candidateSourceBinding` before/after 树摘要同为 `0f07f2f7…`、fileCount 21212、manifest sha `6c92f702…`；本次复算两份 manifest 文件 sha256
  与收据字段逐一相等（**可复算绑定成立**），`runnerSourceBinding.status=PASS`。
- 实现提交 ↔ 受测工作树：自 aa99d2ec 到当前 HEAD 的**58 个非 docs/artifacts 实质生产变更文件**逐一核对
  `candidate-source-after.json` 的逐文件 sha256：**HEAD 内容 58/58 完全一致**，而 aa99d2ec 提交内容 0/58 一致
  → 证明这些实现由 `ecacab152` 提交且正是受测工作树内容；`307877337`（收据/证据）与 `aac8cb9fe`（文档）为后补提交，不属受测候选，收据未改。
- 结论：R08 交接边界与 HANGDOFF 描述一致；R09 未重开 R08 全面审计，只做上述定向复核；R08 旧账未改。
- R09 对照基线取值：rendererDist `3e5339cc…`、rust debug service `b855e5d6…`、contract-versions `6d869347…`（A16 文件）；
  T01 接 host 后重建的产物必须重绑新摘要并在证据记录 diff 白名单。

## 6. 下一轮入口

1. 设计调查已交付并复核为 `IMPLEMENTATION_PLAN.md`；实现者已启动（见 §5.5）。
2. T01 实施：扩展 `desktop/src-tauri` 加载同一 React 构建产物 + `desktop/src/react/host/tauri-desktop-host.ts` +
   dev/release 配置 + `@tauri-apps/cli` 构建脚本；先跑通 `r09_t01_react_parity`/`r09_t01_release_offline` 两个生产者。
3. T02 同步实现 supervisor 与握手（独立文件 `desktop/src-tauri/src/supervisor.rs` + `r09_t02.py`）。
4. 每完成一个任务：定向回归 → 生成器重生成 → 新目录跑 `verify-stage R09` 记录红→绿。

## 5.60 事件：自动化 GUI 反复弹窗（2026-10-26 22:2x，用户可见）

- 现象：用户屏幕出现大量“Lingxi 启动失败 / RUST_SERVICE_CRASH_RESTART_EXHAUSTED”弹窗与界面反复闪现。
- 根因：r09-host-core 的 T02 崩溃重试/发布包测试在**用户会话里循环启动 release .app/debug 宿主**；每次服务注入崩溃或 READY 失败后宿主按设计弹原生失败对话框（同时暴露两个缺陷：失败记录未把 `{LINGXI_HOME}` 插值为真实路径；测试遗留孤儿 release sidecar 进程）。
- 处置（Lead）：立即 `interrupt_agent` host-core；`pkill` 清理全部测试 app/服务进程（ps 计数 0，孤儿 sidecar 已清理）；在 BLOCKED.md 增 `AUTOMATION-UI-SIDE-EFFECTS`；向 host-core 下硬规则：先实现 `LINGXI_R09_AUTOMATION=1` + `LINGXI_R09_HOST_DIALOG_SINK` 的 UI 抑制与路径插值，崩溃/重试优先无 GUI 集成测试，GUI 冒烟每套件最多一次并断言零残留；**Lead 明确 cleared 之前禁止启动 Tauri GUI**。
- 影响：仅隔离测试目录（`/private/tmp/lingxi-r09-*`）；真实 `~/.lingxi` 与用户安装未触碰；无 commit/push。

## 5.61 阶段 1 automation 验证通过 + 阶段 2 cleared（2026-10-26 22:2x）

- host-core 单发 debug 宿主（automation+sink+hidden 窗口）验证：sink JSON `modalShown:false`、code `RUST_SERVICE_CRASH_RESTART_EXHAUSTED`、
  detailPath/home 为真实绝对路径（`{LINGXI_HOME}` 插值已修）、`failure-dialog-suppressed`/`service-launch-failed` 事件、窗口 `visible:false`+Accessory 无 Dock、
  DOM eval 在隐藏窗口工作（bodyTextLength=121/errors=[]）、`processResidualCount:0`、ps 0。Lead 独立复核 sink/events/summary 通过。
- 阶段 2 cleared：T01 release_offline 1 次 + T02 两套件 10 次单发（hidden+sink、串行、逐步 ps 断言 0、异常即停、不重试）；之后停，native 合并另行 cleared。
- 新增并行：`r09-t06-security`（T06 静态策略/发布攻击面扫描；**禁止 GUI 启动**，不写 desktop/src-tauri 直至 Lead 交接）。

## 5.62 阶段 2 结果与 renderer 非确定性发现（2026-10-26 22:3x）

- `r09_t02_supervisor_ready` **PASS exit 0 / 27 cases**（`dev/t02/phase2-supervisor-03`）：5 发单发（debug 正/release 正/协议注入/摘要注入/fake 服务）全部 hidden+sink；正路径 sink 不存在，负路径 sink `modalShown:false` 且 code 正确（PROTOCOL_INCOMPATIBLE/DIGEST_MISMATCH/RUNTIME_IDENTITY_MISMATCH）、detailPath 真实绝对路径；逐发 ps-clean、最终 ps 0、无 Node 回退。
- `r09_t02_process_ownership` 5 发完成（`dev/t02/phase2-ownership-01`），产品行为全对（owned 退出清理、reused/remote 零接触、release 拒远端 REMOTE_REQUIRES_DEV_BUILD、pid 复用 INSTANCE_INVALID、无孤儿），唯一红为生产者断言取错 ppid（已静态修复，待最终 5 发重跑）。
- **renderer 构建树摘要非确定（重要证据口径）**：同源码连续 `build:renderer+build:theme` 树摘要在 d09359/d5a24a/afcdc1 间变化；根因 Vite 对 `settings.html` CSS `<link>` 顺序非确定（643/644 文件逐字节相同）。裁决：证据一律 **build-once-bind-once**（单次构建内绑定；Electron/Tauri 两侧核对同一 digest），跨构建摘要相等不是不变量；列为 R10 发布复现性已知项，最终报告引用。
- native 合并已给 host-core 一次性 cleared（ownership PASS 后执行：Cargo/注册/编译测试/capability audit；期间禁 GUI，完成后 `desktop/src-tauri` 写权移交 T06）。

## 5.63 阶段 2 完成 + native 核心合并 + 写权移交（2026-10-26 22:4x）

- **T02 两套件 PASS**：`r09_t02_supervisor_ready` exit 0 / 27 cases（`dev/t02/phase2-supervisor-03`，5 发 hidden+sink：debug/release 正路径、协议注入、摘要注入、fake 服务；负路径 sink code/modalShown=false/真实路径；逐发 ps-clean、无 Node 回退）；`r09_t02_process_ownership` exit 0 / 16 cases（`dev/t02/phase2-ownership-02`，5 发：owned+remote/reused/remote 零接触/release 拒远端/pid 复用；`no-orphan-owned-services`、`remote-untouched` 全 PASS）。
- **native 核心合并**（host-core）：Cargo 插件依赖（single-instance/global-shortcut/notification/autostart + objc2 系）与 tauri features `image-png,tray-icon`；`pub mod native;`、`with_plugins` 首行、setup `install(app)?`。验证：`cargo build --locked --features r09-host` 0；`cargo test --lib --features r09-host` **238 passed**；`cargo test --test service_supervisor` 9 passed；R06 base `cargo check` 0。
- **未完成并移交 T06**：28+2 命令 invoke_handler/build.rs 注册、capabilities 逐窗口精确 leaf（当前 `core:default` 展开含 menu/tray/webview devtools → audit 6 红）、Info.plist 权限文案、主窗 CloseRequested 隐藏策略/move-resize 防抖持久化。
- **决策**：新增 Cargo feature **`r09-test-hooks`** 门控 `LINGXI_R09_AUTOMATION`/`LINGXI_R09_HOST_DIALOG_SINK` 与 debug 命令；证据构建启用并记录 feature，交付 release 不启用；A03/A04 用 evidence 构建，A12/A14 用交付 release。capability audit 对应检查改为“feature 声明 + 交付 release 回执 feature 列表不含”。
- `desktop/src-tauri` 写权移交 `r09-t06-security`（capability/权限/attack-surface 收口 + 上述注册/生命周期接线）；`r09-host-core` 转 **T07 原生 E2E**（先无 GUI 产出，GUI 批次需 Lead cleared）。
- renderer 构建非确定性已按裁决记录（build-once-bind-once；R10 已知项）。

## 5.64 并发写冲突事件与恢复（2026-10-26 22:5x）

- 事件：T07（host-core 角色）在 G2 静态前半阶段并发执行 `scripts/tauri-build.mjs`+release 构建，与 T06 对 `desktop/src-tauri` 的命令面/capability 收口冲突（Cargo.toml/tauri conf 并发修改，T06 构建无法验证）。
- 处置：Lead interrupt host-core 并终止其 tauri-build/cargo/rustc 进程（ps=0，清 release `.cargo-lock`）；恢复 T06 独占写权/构建权；host-core 在交接前仅做只读/夹具/映射类无 GUI 工作；G2 静态检查改为只读既有产物。
- 裁决：保留 host-core 加的 `tauri` feature `protocol-asset`（D8 assetProtocol 需要）；保留 T06 对 native IOKit 宏常量→CFString 的最小链接修复（要求 IOPMAssertion 语义等价并注释/证据说明）。
- 教训入规：**desktop/src-tauri 同一时间只允许一个写者+构建者**；任何需要新产物的工作流必须由 Lead 在候选构建阶段统一产出后只读消费。

## 5.65 T06 静态收口 + 46 叶权威分类 + 交付物骨架（2026-10-26 22:5x）

- T06 静态收口接近完成：`audit-04-final` **PASS exit 0 / 55 cases**；`release-surface-03`/`release-scan-04` **PASS**；71 条命令面已注册；capabilities 8 窗口精确 leaf；`r09-test-hooks` feature 分离；交付 release 与 evidence debug 构建回执已产出（delivery exe `17523a17…`、sidecar `852912ef…`、protocol 1..=1 epoch2；evidence debug exe `f6555ef0…`、features `r09-host,r09-test-hooks`、rendererDigest `d09359bd…`）。待补：receipt features/codesign 字段、T07 需要的 `e2e-test` evidence 构建、capability-matrix 外层 blocked 列表、候选交接与冻结声明。
- T03/T04 生产者重跑仍 exit 2（blocked 项=真实系统动作/运行期 ACL，待唯一一次 visible GUI clearance 批次）。
- **46 叶权威分类**（native-caps §9.1）：HOST_NATIVE 22 + HOST_BRIDGE 8 + SERVER_REACT 16 = 46；“24 vs 16”已对账（24=R00 非 DESKTOP_BEHAVIOR 类别数，其中 8 仍属 HOST_BRIDGE）；T07_LEAF_MAPPING 的 30/16 与 §9.1 逐叶一致，仅需修 summary 口径。
- 交付物骨架已落：`R09_REPORT.md`（IN_PROGRESS 草稿）、`R09_HANDOFF.json`（DRAFT）、`SHELL_PARITY_RESULTS.json`（DRAFT，待 T08）。

## 5.66 冻结候选与剩余门禁路径（2026-10-26 23:0x）

**冻结候选（T06 candidate handoff；desktop/src-tauri 已冻结）**
- 交付 release：app 树 `2dc38bc3…`、host exe `17523a17…`、sidecar `55d6edb1…`、build.json `40085e84…`、Info.plist `da9ebff0…`、
  features 仅 `r09-host`、codesign deep --strict exit 0（adhoc+runtime，CDHash `569f177b…`，正式签名 BLOCKED）。
- evidence 构建（T07 G1）：debug exe `c3622805…`、features `e2e-test,r09-host,r09-test-hooks`、receipt `78a588ce…`、同 rendererDigest `d09359bd…`。
- 静态门：capability audit PASS 55 cases / gapLedger 0；release attack surface PASS 45 + 套件 PASS 51；71 命令四集合一致。
- 冻结规则：任何 desktop/src-tauri 写入/构建都会使 T01/T02/T06 证据失效（需重建+重跑）；当前无 GUI/构建残留。

**T03/T04 叶计划（native-caps）**：22 HOST_NATIVE + 8 HOST_BRIDGE = 30 叶/60 断言，`plannedActionCase` 覆盖 60/60；
49 条 implemented（GUI driver `r09_gui_actions.py`）、3 条 manual（拖出 Finder/托盘 i18n 截图/通知点击）、
8 条 blocked（onboarding 服务路由 4：Rust 服务无 setup-complete 等价；DOCX/XLSX 预览命令 4：宿主命令缺口）。T04 4 叶可绑定（真实 HTTP 已绿）。
生产者现以 `LINGXI_R09_GUI_CLEARED=1` 门控：无 env 时逐叶 `blocked::gui-cleared::<leafId>` exit 2（诚实红）。

**运行期 ACL 矩阵（T06）**：`scripts/rust-tauri/r09_t06_runtime_acl.py` 已实现（wdio 驱动 evidence 候选；allow 137/deny 447 计划；token 不入 URL/DOM/storage；
settings/splash 无窗口创建入口 → `blocked::window-open::*` + 静态 audit 覆盖边界）；无 GUI 自检 exit 2 `RUNTIME_ACL_NOT_CLEARED`（未 spawn）。
hidden cleared 批次已发；GUI 批次清单 `docs/rust-tauri/R09/T06_GUI_BATCH_NEEDS.json`（25 项）。

**G1-hidden 首次单发失败（真实红，已停止未重试）**：wdio 未就绪，根因=evidence 主 exe 是**裸二进制**（`tauri build --debug --no-bundle`），
`UNUserNotificationCenter currentNotificationCenter` 要求有效 app bundle → setup 阶段 NSInternalInconsistencyException（创建窗口前崩溃；无可见 UI；ps 0；crash report 在案）。
裁决选 (b)：在 `/private/tmp` 复制交付 `.app` 骨架 + 替换 evidence 主 exe + ad-hoc 签名 + `--verify --deep --strict` 后只读跑 G1-hidden 单发；
失败则退回 T06 正式 evidence `.app`（选 (a)）。证据：`dev/t07/g1-hidden-01/`（诊断/崩溃/app log）。

**下一步（按序）**：① G1-hidden wrapper 单发；② T06 运行期 ACL hidden 单发；③ 统一 visible 批次（预期 3 次可见启动：
G2 delivery `.app`、T07 evidence wrapper `.app`、T03 gui-actions evidence wrapper `.app`；≤10 分钟、失败即停、结束时 ps 0 并报告可见性/截图）；
④ 绑定 30+9 叶（FULL_BINDINGS；3 manual + 8 blocked 之外）；⑤ T08 替壳 A/B + host completeness；⑥ 最终门新目录 + 最终 REPORT/HANDOFF 更新 + 独立复核。

## 5.67 候选 v1 作废 → v2 计划（T08 A16 真实缺口，2026-10-26 23:1x）

- T08 只读盘点发现 A16 真实缺口：`desktop/src/react/host/tauri-host-capabilities.ts`（T03/T04 44 成员能力片段）**从未被 `tauri-desktop-host.ts` 合并** → Tauri 下这些 PlatformApi 成员调用即抛 HostCapabilityError（显式阻断，但“每项真实可执行”不成立）。另有 `host:context-menu-policy` 无实现/登记、`host:file-url-asset` assetProtocol scope 为空且 convertFileSrc 未接、`onboarding_complete NOT_IMPLEMENTED` 未登记。
- 裁决：**候选 v1（app tree `2dc38bc3…`）作废**；出 v2：host-core 只改 `desktop/src/react/host/**`（合并片段/getFileUrl/context-menu 等价或 BLOCKED 依据/补单测），给出 `HOST_V2_PATCHES.md` 给 T06 执行 Rust 侧（asset scope/新命令/capability allow-*），T06 重建 renderer+delivery release+G1 evidence 并重跑静态门；之后重跑 hidden 批次（T01/T02/T06/T07 G1-hidden）→ visible A/B/C → 最终门。
- 已完成且不因 v1 作废：G1-hidden 首绿（26 cases，wrapper 证据形态）；T07 夹具/静态套件；T08 盘点（T08 两套件编写中）；T06 静态 audit/scan PASS（需在 v2 重跑）。
- T06 runtime ACL hidden 尝试中止（wdio parse/aborted，证据 `dev/t06/runtime-acl-cleared-01/`）；待 T06 修复并在 v2 上重跑。
- 阶段最终状态预告：即使 v2 全绿，R09 仍受 `BLOCKED_PLATFORM`（三平台）、`BLOCKED-SYSTEM-PERMISSIONS`（真实 TCC）、`BLOCKED-SIGNING-IDENTITY`、T03 8 条 blocked（onboarding 服务路由/DOCX-XLSX 宿主命令）、T04 helper/截图/ASR 等限制 → 交付为 READY_FOR_REVIEW + 明确阻塞，不声称 R09 完成/R10 准入。

## 5.68 v2 修复批（2026-10-26 23:2x，进行中）

- host-core（renderer 侧，已完成）：`tauri-desktop-host.ts` 正式合并 `createTauriHostCapabilities`（片段 43 成员真实 invoke/listen）；`getFileUrl`=convertFileSrc（缺失显式受限）；context-menu-policy renderer 等价实现；onboarding 显式 NOT_IMPLEMENTED+理由。PlatformApi 109：真实 48 / 显式 no-op 1 / 可选缺失 6 / 显式受限+具体理由 54 / 0 静默 undefined。`tsc` exit 0、host vitest 10/10。patch 清单 `HOST_V2_PATCHES.md`。
- T06（Rust/build 侧，进行中）：按 patch 执行 assetProtocol scope+CSP、capabilities/policy 6 条 allow-* 同步；**onboarding_complete 不实施**（保持 NOT_IMPLEMENTED，登记 T06-ONBOARD-01，不改服务协议）；随后重建 renderer、delivery release、G1 evidence，重跑静态门并产出 `candidate-handoff-v2`；完成后再次冻结。
- T08（并行）：两套件编写中；将吃 v2 renderer digest/成员覆盖表；T06 运行期 ACL harness 已修复（3 次 hidden 尝试均无矩阵结论/无可见窗口/ps 0），并入 v2 hidden 批次。
- 后续批次：v2 hidden（T01/T02/T06/T07 G1-hidden 重跑）→ 候选 v2 冻结 → visible A/B/C（G2 delivery、T07 visible、T03 gui-actions，统一 clearance，≤10 分钟）→ 绑定叶（T05 15 + T04 4 + T03 可绑定子集；blocked/manual 保持 pending）→ 最终门 + T08 汇总 + REPORT/HANDOFF 终稿 + 独立复核。

## 5.69 候选 v3 计划（2026-10-26 23:3x–4x）

- T08 发现 v2 仍有 6 能力族 PlatformApi 成员未接线（file-text-io/detached-viewer/skill-viewer/file-watch/browser-viewer/file-drop-paths）→ 裁决出 **v3**（不登记披露式红）。
- host-core v3 renderer：六族映射到现有 Rust 命令/事件（file-changed、spawn_viewer/viewer_request_load/viewer_close、open_skill_viewer/list/read、5 条 browser 命令 + connection-changed、watch/unwatch、Tauri onDragDropEvent FIFO→getFilePath）；PlatformApi 109：真实≈71 / no-op 1 / 可选 6 / 受限 32 / 0 静默；tsc 0、vitest 15/15。
- T06 v3 Rust patch（最小，对齐 Electron）：viewer-closed 数字 id 事件、viewer 窗口改 `viewer-window.html`、skill viewer 改为向调用窗 emit `show-skill-viewer`（Electron 语义）、watch_workspace/unwatch+workspace-changed、capability allow 联合集；defer 登记：browser-update 全流、file_drop_paths、onboarding NOT_IMPLEMENTED。随后 v3 重建（renderer/delivery/G1 evidence/release-evidence hooks bundle）+ 静态门 + candidate-handoff-v3 冻结。
- 批次暂停：v2 hidden/visible 全部暂停（等待 v3）；T06 ACL hang 已处置（根因 wdio 无 deadline/无进度；加固 10s/45s/8s/300s deadline+progress.jsonl+hidden-safe 子集；selfcheck exit 2）。
- release-evidence hooks bundle 在 v2 已实测可行（隔离 CARGO_TARGET_DIR，repo 生成物复原，delivery 未触；app tree `d28d5661…`/exe `44e30fb0…`/renderer `c0a5719f…`，T02 release hidden 负向不再弹窗）。

## 5.70 候选 v3.1 冻结与 hidden 批次（2026-10-26 23:4x–5x）

- v3.1 冻结（T06 handoff）：renderer `cba1d4f3…`；delivery app tree `1bae1e28…`/exe `248a8e34…`/build.json `f1dc84f9…`/features r09-host/codesign verify 0；
  G1 evidence exe `54f93eac…`（e2e-test,r09-host,r09-test-hooks）；release-evidence hooks bundle `07df2e93…`/exe `fd8b5cd7…`；命令面 73；
  静态门 audit 57 PASS / scan 45 PASS / release suite 51 PASS（capability matrix 仅 21 runtime blocked）；已写入 `R09_HANDOFF.json.frozenCandidate`。
- v3.1 renderer 覆盖：PlatformApi 109 = 真实 76 / no-op 0 / 可选缺失+理由 6 / 显式受限+理由 27 / 0 静默（六能力族 + workspace/viewer-closed/show-skill-viewer 全部真实接线）。
- hidden 批次 H1（T07 G1-hidden v3.1）首跑/重跑均因 `file-url-asset` `<img asset:>` onerror 停在 producer 段（`g1-hidden-v31-01/02`，其余 18/19 cases PASS：shell/IME/选区/滚动/DPI/焦点/context-menu 两项/isolation；wrapper ad-hoc 后 `ac729159…`）。已核对 debug generated config 的 assetProtocol/CSP 正确 → 转 T06 运行期授权/协议注册排查（hooks 探针 + 仅重建 G1 evidence v3.1-G1b）；H2/H3 暂 hold。
- 下一批次：H2（T02 debug hidden 1+3 发）、H3（T02 release hidden，用 release-evidence bundle）、T06 ACL hidden（v3.1 G1 exe，hidden-safe 子集）；之后 visible 修订版 5 次窗口（V1 release_offline/V2 G2/V3 react_parity 双壳/V4 T03 actions），需用户告示后 clearance。

## 5.71 asset 协议失败根因与 v3.2（2026-10-26 23:5x）

- H1（T07 G1-hidden v3.1）`file-url-asset` 失败根因**已定位**：页面级 CSP 覆盖——`desktop/src/modules/connection-csp.js` 用 `document.write` 注入
  `<meta http-equiv="Content-Security-Policy">`，`img-src`/`media-src` 白名单无 `asset:`/`http://asset.localhost`，在 Tauri 下拦截 `convertFileSrc()` 的 asset URL
  （Electron 用 `file:` 白名单所以从未暴露；Rust asset scope/config 均正确，无需改 Rust）。
- 修复（Lead）：connection-csp.js 的 img-src/media-src 增加 `asset: http://asset.localhost`（注释说明 R09-T03/D8）。
- 决定出 **v3.2**：renderer 变更 → delivery/G1/release-evidence 重建 + 静态门 + handoff-v3.2 冻结；随后一次性跑 H1–H3 hidden 批次与 visible 修订版 5 次窗口。
- T06 已加 hooks-only asset 诊断（`debug_asset_probe`、setup scope 日志、单测）供回归，不作为产品路径。

## 5.72 最终门 final-01（2026-10-27 00:0x）与门后修复波

- 正式门命令（v3.2 候选 + env 计划）：`verify-stage R09 --evidence artifacts/rust-tauri/R09/verify-r09-final-01` → **overall FAIL（exit 2）**，证据已落盘。
- 门结果事实：PASS 命令 = inputs_check / t01_release_offline / t02_process_ownership / t05_untrusted_preview / t06_release_attack_surface / capability_audit / t07_native_e2e(H1 hidden 30) / gate_negative；FAIL = t01_react_parity / t02_supervisor_ready / t03_* / t04_* / t05_browser_pdf / t06_capability_matrix / t07_release_smoke / t08_* / map_integrity / service_seam（PASS 但缺 output.log）。
- **candidate stable=false**：门期 T06/T08 写 docs/scripts + `cargo-supervisor-tests`（T02 suite）直接 `cargo test` 覆盖 `target/debug`；已修 T02 为隔离 CARGO_TARGET_DIR。
- 门后修复（Lead/host-core/T06）：Lead 修 r09_t05.py `source` NameError、seam_probe 写 output.log、map_integrity incomplete-contract mutation；host-core 修 commands.rs automation 每次抑制+window 控制守卫（含单测）、T01 白名单补 connection-csp.js、T02 hidden 启动显式注入 automation+sink、T07 确认零构建、恢复 target/receipt；T06 修 policy（main 移除 quick_chat_resize）并重跑 audit PASS。
- 下一步：T06 重建 **v3.3**（delivery+G1+release-evidence；renderer 保持 b0a20250）→ 静态门 → handoff-v3.3 → ACL hidden → host-core 重建 gate wrapper → Lead 跑 **final-02**（所有 teammate 必须停止写入）；如实预期仍 FAIL（T03/T04/T06 runtime/T07 release smoke/T08 依赖 blocked 项），不得转绿。

## 5.73 最终正式门 final-04（2026-10-27 00:5x）——R09 收口事实

- 候选 v3.3（delivery appTree `97f226bf…`/exe `ddbd1fad…`、G1 `27bf075a…`、renderer `b0a20250…`、sidecar `55d6edb1…`、release-evidence `48993e01…`）；`R09_HANDOFF.json.frozenCandidate` 已更新。
- 门命令：`verify-stage R09 --evidence artifacts/rust-tauri/R09/verify-r09-final-04`（env 见 `R09_GATE_ENV_PLAN.md`；所有 teammate 停写；只有 Lead 写证据目录）。
- 结果：**overall FAIL**；**candidate stable=true**（digest `3bc4a35a1b975de178f3936634481d8453edd93b6acec8cfab1f991b0d9ebc84`，28633 files）。
- 命令 **13/21 PASS**：inputs_check、t01_react_parity、t01_release_offline、t02_supervisor_ready、t02_process_ownership、t05_browser_pdf(73)、t05_untrusted_preview、service_seam_probe、t06_release_attack_surface、capability_audit(58)、t07_native_e2e(30)、map_integrity(19)、gate_negative(7)。
- 命令 **8 FAIL（全部为已登记 blocked/pending，不绿洗）**：t03_native_matrix(8 blocked+3 manual)、t03_ipc_negative(3 blocked)、t04_permission_matrix(TCC/helper/截图/ASR)、t04_remote_boundary(真实凭据)、t06_capability_matrix(21 blocked::runtime.*)、t07_release_smoke(release.file-task PENDING)、t08_shell_parity、t08_host_completeness(25 能力运行期 pending)。
- 74 叶：**15 PASS / 59 BLOCKED / 0 FAIL**；16 场景：A01–A04/A09/A10 PASS，A05–A08/A11–A16 FAIL（阻塞原因见 R09_REPORT §5/§8）。
- 产物：`R09_REPORT.md` 终稿、`R09_HANDOFF.json`（status INCOMPLETE、finalGate、leafCoverage、platformMatrix、allowedNextScope.granted=false）、`SHELL_PARITY_RESULTS.json`（PARTIAL_PENDING_RUNTIME_EVIDENCE）、`R09_CLOSEOUT_CHECKLIST.json`（逐命令/逐叶/knownBlockers）。
- 门期问题处置：final-03 发现 15 个 T05 叶因证据缺 `expect` 字段 FAIL → `r09_producer.check` 统一写 `expect=1`；T05 已知 CDP 抖动加一次真实重跑（两次证据都落盘）；final-04 前清理漏留 T03/T01 可见 app/sidecar；final-04 后 target/debug/receipt 仍为 27bf075a/1ac08daf、未漂移。
- 清理：final-04 泄漏的 T03 visible app/service 已终止（ps=0）；无 wdio 端口残留；真实 `~/.lingxi` 未触碰。
- 结论：R09 **未完成**；交付 READY_FOR_REVIEW + 明确阻塞；**R10 不授予**。

## 5.74 独立复核与收口（待复核者回报后补）

- 已安排独立只读复核（subagent）：核对 final-04 回执/报告/handoff/parity/BLOCKED 数字与证据一致性；结论待回报后补记入本文件与 R09_REPORT §11；`R09_HANDOFF.json.independentlyReviewed` 在复核完成前保持 false。

## 5.74 独立复核（14 findings）与修正（2026-10-27 01:0x）

- 独立只读 reviewer 完成复核：gate 数字/10 项关键 SHA 全对；发现 14 项不一致（high 3 / medium 7 / low 4）。
- high：①T03 native_matrix 实为 43 failed（guiStats: 60 executed/6 passed/43 failed/11 blocked/3 manual）+ logic refs 缺 18，文档只写 8 blocked+3 manual；②A06/A07/A08/A12/A14 场景名/理由与 `R09.json` specMirror 错配；③v3.3 ACL preflight 绑定旧 `candidate-handoff-02`（harness 硬编码）。
- medium：T08 shell pending 33/blocked 3 vs 文档 25（25 是 a16Ok 失败数）；T04 remote 根因实为 GUI/TCC（非 REAL_PROVIDER 单独）；T06 ACL blocked 35 vs 21/27 混用；T07 scroll/focus pending 被写 PASS；post-gate 文档未绑定候选；59 BLOCKED 构成未解释；T07 file-task 根因实为硬编码 `PENDING_T06_COMMANDS`。
- low：644/645 笔误；T07 G1 30 聚合 vs cases.json 29；handoverCompleteness 35/priorStageDeferred 43 未解释；T03 18 引用未单列。
- 修正动作：REPORT/HANDOFF/SHELL_PARITY/BLOCKED/CLOSEOUT_CHECKLIST/PROGRESS 全部按 findings 重写/修正；`r09_t06_runtime_acl.py` 改为动态最新 handoff + 归档 hidden 诊断，重跑 `runtime-acl-v33-02` PASS_WITH_BLOCKED_WINDOWS（绑定 v3.3）；T08 两个 producer 红单列为 knownProducerIssues。
- 结论不变：R09 **未完成**（T03 43 failed、T07 file-task 硬失败、T04 TCC/GUI、T06 21 blocked、T08 25 能力+2 producer 红、平台/签名）；R10 不授予；`independentlyReviewed=false`、`independentReviewPassed=false`。

## 5.75 门后诊断实验：T03 GUI 动作驱动修复（2026-10-27 01:1x，非正式门证据）

- 触发：final-04 的 T03 `native_matrix` 有 43 failed，其中大量 detail 为 wdio 插件错误（`done is not a function`、RemoteDisconnected 等），怀疑动作驱动 bug 而非全部产品缺陷。
- 修复（仅 `scripts/rust-tauri/r09_gui_actions.py` 测试工具）：
  - `WebDriver.execute` 由 `execute/sync` 改 `execute/async`（该 wdio 插件在 sync 下不推进页面 IPC Promise，且 done 回调模式直接报错）；
  - restart 用例：容忍 restart 导致的 WebDriver 断连，`_recover_after_restart` 先尝试新进程 session，失败则由 harness 停旧树并原 env 重拉 app；start 前清理同候选 stale batch app/service，避免端口/单实例锁；
- 诊断结果（隐藏环境+可见 wrapper app，证据 `/private/tmp/r09-t03-recheck-04`，**非正式门、未绑定候选**）：
  - `guiStats`: executed 60 / passed **21**（原 6）/ failed **28**（原 43）/ blocked 11 / manual 3；
  - 剩余失败分类：真实断言 13（splash allow 范围、keep-awake IOPMAssertion、shortcut 注册、appearance 截图、window-controls 等）、invoke-empty 6（viewer path 参数等）、JSONDecodeError 5（服务 prefs 读取对重启/切窗不健壮）、通知 delivered 探针 2（swift 裸二进制缺 .app bundle → NSBundle nil）、其余 2；
  - 其中部分仍是 harness/证据工具问题（JSONDecode a、通知探针、viewer 参数），部分是产品/权限面（splash capability 范围、keep-awake、shortcut、TCC 截图）需 T06/T03 处理。
- 结论：T03 仍未完成，**正式门仍以 final-04（43 failed）为准**；本实验证明动作驱动可修复一部分、剩余需产品/工具修复后才可能改观。未改写任何正式回执。

## 5.76 门后工具修复（T08 producer 两项 + 记录，2026-10-27 01:1x，非正式门）

- T08 `host:context-menu-policy` 能力模型由 `missing` 改为 `renderer-policy`（renderer 等价实现 + T07 real-behavior 证据）；`commands::debug-commands-gated` 允许 hooks-only `debug_asset_probe` 且与 delivery capability 断言分离。
- dev 复跑 `r09_t08_host_completeness`（`/private/tmp/r09-t08-dev-02`）：两项 producer case 均绿；剩余 `host::capability::*` 24 项为运行期证据 pending（dev 证据旧候选 candidate-binding 红属预期）。
- T03 动作驱动修复见 §5.75（dev 21 pass/28 fail，正式门仍 final-04 的 6/43）。
- 以上均为证据/工具修复与 dev 诊断：**未重跑正式门**，正式回执仍为 final-04；候选二进制未变。下一步若继续，应冻结新工具后以全新目录重跑正式门，或按阻塞条款收口。

## 5.77 最终正式门 final-05（工具冻结后重跑，2026-10-27 01:2x）

- 候选二进制未变（delivery `ddbd1fad…` / G1 `27bf075a…` / renderer `b0a20250…` / sidecar `55d6edb1…`）；工具（r09_gui_actions.py / r09_t08.py / r09_t06_runtime_acl.py）冻结后重跑。
- 命令：`verify-stage R09 --evidence artifacts/rust-tauri/R09/verify-r09-final-05`（env 同 R09_GATE_ENV_PLAN）。
- 结果：**overall FAIL**；candidate stable=true（digest `d1c7da14…`，29221 files）；命令 **13/21 PASS**；74 叶 **15 PASS / 59 BLOCKED / 0 FAIL**；16 场景 6 PASS（A01–A04/A09/A10）。
- T03：guiStats **executed 60 / passed 21 / failed 28 / blocked 11 / manual 3**（final-04 为 6/43；execute/async+restart 修复生效，但仍 exit2）。
- T08：shell failed **60**、host failed **25**；`evidence::capability::host:context-menu-policy` 与 `commands::debug-commands-gated` 已转绿；`parity::capability::host:context-menu-policy` 与 host 侧矛盾 → shell parity 口径冲突待修。
- 其余 FAIL 原因不变：T03 ipc/T04 TCC-GUI/T06 21 runtime blocked/T07 release file-task 硬编码 False/T08 运行期 pending；平台/签名阻塞未解除。
- 交付文档已同步 final-05：R09_REPORT/HANDOFF/SHELL_PARITY/CLOSEOUT_CHECKLIST/LEAF_LEDGER/PROGRESS.json；正式结论仍为 **R09 未完成、R10 不授予**。

## 5.78 门后 T03 工具修复第二波（2026-10-27 01:4x，dev 证据，未重跑正式门）

- 修复（仅 `scripts/rust-tauri/r09_gui_actions.py`）：
  - `_server_prefs` 服务 instance 重读/重试 + `server_request` 对空/非 JSON body 容错（窗口切换/重启瞬态不再当产品失败）；
  - `spawn_viewer` 按 v3.3 命令签名传 `{path}`（旧代码传 payload 包装，宿主报 missing required key path）；
  - 通知 delivered 探针改为最小 `.app` bundle（裸 swift 二进制 `mainBundle` nil → NSInternalInconsistencyException）。
- dev 结果（`/private/tmp/r09-t03-recheck-05`）：guiStats executed 60 / passed **22** / failed **27** / blocked 11 / manual 3（相对 final-05 的 21/28 小步提升）。
- 剩余失败：15 项真实断言/环境（keep-awake IOPMAssertion、shortcut 注册、appearance/TCC 截图、open-external、window-controls、splash capability 范围等），5 项 `_server_prefs` 仍无有效 JSON（`/lingxi/v1/settings/snapshot` 路由响应待查，疑服务侧/授权瞬态），5 项 invoke 结果，其余 2。
- 正式门 final-05 未重跑；正式回执数字仍为 21/28。R09 未完成结论不变。

## 5.79 R09 完成判定：BLOCKED（round 5，2026-10-27）

- 硬指标②要求 formal gate overall=PASS、四组平台原生验证、零未决阻塞；当前 final-05 为 FAIL（13/21 命令，T03 28 failed 等），且三平台真机、签名/公证凭据、真实 TCC/供应商凭据均缺失（跨 round 持续）。
- 已确认这些阻塞**非临时性**：本环境无 macOS x64/Windows/Linux 机器，仅 ad-hoc 签名，TCC 需人工系统设置会话；因此 R09 无法在本环境完成，按授权交付未完成 + 续跑入口，不称完成或 R10 获准。
- 阻塞登记：`BLOCKED.md`「R09 完成判定：BLOCKED」表（平台/签名/TCC/真实供应商 + 未完成实现证据及各责任/解除条件/截止）。
- 交付状态：READY_FOR_REVIEW + BLOCKED；`R09_HANDOFF.allowedNextScope.granted=false`；`independentReviewPassed=false`（独审完成但 R09 未完成）。
- 可执行部分已最大化：final-05 receipt（`a5b9da43…`）、T05/T06 静态+ACL hidden、T07 G1、T02 hidden、T08 只读矩阵、T03 dev 22/27 等；候选二进制未变；未提交/推送。

## 5.80 业主裁决落实与 TCC 人工授权启动（2026-10-27 08:3x）

- 业主裁决见 `OWNER_DECISIONS.md`（OWNER-01..06）：托盘修复组件移除；R09 范围限定 macOS arm64（其余平台业主延后，不冒充）；不做正式签名（后续也不做）；主窗口允许读启动页信息/头像；本地模型 http://127.0.0.1:1234 / qwen/qwen3.8-27b；本地 ASR 延后；TCC 人工授权开始。
- BLOCKED.md 已按裁决更新对应条目（平台/签名/供应商/ASR/修复组件/context-menu/TCC）。
- 启动可见交付 app（隔离 LINGXI_HOME=/private/tmp/lingxi-r09-owner/home）用于人工授权：电脑控制→请求系统权限（辅助功能/屏幕录制）；麦克风/语音识别按 App 提示授权。
- 待做的代码改动（owner 已授权）：main 窗口 allow get_splash_info/get_avatar_path → 新候选 v3.4 重建+audit；随后按授权补 onboarding/DOCX-XLSX 等（待业主确认项见 OWNER_DECISIONS 末尾）。

## 5.81 OWNER-04 落实（代码改动，待 v3.4 重建；2026-10-27 09:0x）

- 业主裁决主窗口允许读启动页信息/头像；已改：
  - `desktop/src-tauri/capabilities/r09-host.json`（main）增加 `allow-get-avatar-path`、`allow-get-splash-info`；
  - `docs/rust-tauri/R09/R09_COMMAND_POLICY.json` main.allowedCommands 增加 `get_avatar_path`、`get_splash_info`。
- 这两命令原本只授权 splash/onboarding；T03 从 main 调用被 ACL 拒绝（2 cases），Electron ipc 无窗口限制 → 本次对齐 Electron 行为。
- 需在 v3.4 重建 delivery/G1 evidence 后重跑 capability audit/scan/release suite 与正式门；当前 final-05 回执不含此改动。
- TCC 人工授权：服务侧 computer-use 状态已显示 accessibility=true / screenRecording=true（证据 `dev/t04/owner-permission-session-01/computer-use-permissions.json`）；provider available=false 是 computer-use 功能未启用（enabled=false），不是权限未授予；麦克风/语音识别/通知待人工点击。

## 5.82 候选 v3.4 构建（OWNER-04 + 工具修复冻结，2026-10-27 09:0x）

- 变更：main 窗口允许 `get_splash_info`/`get_avatar_path`（capability+policy，OWNER-04）；T03 GUI 动作 execute/async/restart 恢复/stale 清理、snapshot agent_id、窗口控制 R00 语义、通知探针 .app；T08 context-menu/debug-gated 口径；T06 ACL 动态 handoff。renderer 未重建（b0a20250…）。
- 真实构建（exit 0）：
  - delivery release：appTree `b094b15a…`、exe `41c32b1a…`、receipt `3a050cf9…`、features `[r09-host]`、codesign verify 0；sidecar `55d6edb1…`、build.json `372c2b88…`（rendererSha b0a20250…）。
  - G1 evidence：exe `0a9a072f…`、receipt `26389ac6…`、features `[e2e-test,r09-host,r09-test-hooks]`；稳定副本 `/private/tmp/lingxi-r09-evidence/lingxi-resource-desktop-v34-0a9a072f`。
  - handoff：`artifacts/rust-tauri/R09/dev/t06/candidate-handoff-v3.4/candidate-handoff.json`。
- 静态门：audit `v34-audit-01` PASS 58、release scan `v34-scan-01` PASS 45、release suite `v34-release-suite-01` PASS 51、capability matrix `v34-capability-matrix-01` FAIL 91（仅 21 blocked::runtime.*，静态全 PASS）。
- release-evidence hooks bundle 未重建：v3.3 bundle 的 sidecar/build.json 与 v3.4 相同，T02 release 负向待最终门验证；如需严格绑定再重建。
- TCC：v3.4 可见 app 已启动（隔离 home2），computer-use 状态 accessibility=true / screenRecording=true（service 二进制未变，授权保持）；麦克风/语音识别/通知待业主点击。final-05 回执仍未含 v3.4；待跑 final-06。

## 5.83 v3.4 隐藏验收（2026-10-27 09:0x）

- 候选 v3.4（delivery exe `41c32b1a…`/appTree `b094b15a…`、G1 `0a9a072f…`、renderer b0a20250）已构建并冻结；静态 audit 58 PASS / scan 45 PASS / release suite 51 PASS；capability matrix 仅 21 runtime blocked。
- 隐藏套件（真实执行）：
  - T07 G1 hidden `dev/t07/g1-hidden-v34-01`：PASS exit0 / 30 cases（含 asset/context-menu/IME/selection/scroll/DPI/focus/screenshot/graceful/no-leak）。
  - T06 ACL hidden `dev/t06/runtime-acl-v34-01`：PASS_WITH_BLOCKED_WINDOWS exit0 / 105 cases / blocked 35。
  - T02 debug hidden `dev/t02/v34-supervisor-01` PASS 10 + `v34-ownership-01` PASS 13。
  - T08 dev 自检（v3.4 handoff）：host 323 cases/39 failed、shell 342/74 failed（dev fallback candidate-binding 红属预期；in-gate 重跑为准）。
- TCC：v3.4 交付 app 可见运行（隔离 home2）；computer-use 状态 accessibility/screenRecording 仍 true（service 二进制未变）证据 `dev/t04/owner-permission-session-02/`；麦克风/语音识别/通知待业主点击。
- 待做：T03 native matrix visible、T04 permission matrix（人工 TCC）、T01 react parity visible、T05 两套件、T07 release smoke/G2、T06 capability matrix、T08 in-gate；然后 final-06 正式门 + 文档更新。

## 5.84 候选 v3.5：主窗口系统权限卡片（T04 UI 缺口修复，2026-10-27 09:1x）

- 根因：Tauri/Rust 版空数据聊天页无录音入口，设置窗口按安全设计不能调用 speech/notification（caller=main），导致麦克风/语音识别/通知 TCC 无用户手势路径；业主在系统设置看不到可添加项。
- 修复（renderer，仅 Tauri 宿主渲染）：
  - 新增 `desktop/src/react/components/SystemPermissionsCard.tsx`：主窗口右下角“系统权限”卡片，按钮真实触发 ①麦克风 `getUserMedia`（Info.plist 已有 NSMicrophoneUsageDescription）②语音识别 `speech_permission_request`（release host_can_prompt）③通知 `show_notification`（macOS 首次请求通知授权）；失败显式展示；
  - `tauri-host-capabilities.ts` 接线 `speechPermissionStatus/Request`（此前 fragment 标注“待注册”），从 unavailable 理由表移除；
  - `App.tsx` 挂载卡片；五语言 locales 增加 `systemPermissions.*`；T01 renderer 白名单登记新文件/App/locales。
- v3.5 构建（真实执行）：renderer digest `6f84c46c…`（644 files）；delivery appTree `593d9d7f…`/exe `a401a8c4…`/receipt `fd2838b9…`/build.json `bb02e7b3…`/codesign 0；G1 exe `da30a862…`/receipt `068b1016…`；静态 audit 58 PASS / scan 45 PASS / release suite 51 PASS / capability matrix 仅 21 runtime blocked。
- 截图核对：v3.5 主窗右下角权限卡片显示“麦克风/语音识别/通知”三个请求按钮（`/private/tmp/lingxi-v35.png`）；等待业主点击触发系统授权。
- 未重跑正式门；final-06 待可见验收与 TCC 完成后执行。

## 5.85 业主裁决：麦克风/语音识别/通知三项跳过（2026-10-27 09:2x）

- 用户明确：“这三项直接跳过不再测试了”（指麦克风、语音识别、通知的 TCC 授权/点击验收）。
- 处理（不降标准、不伪造）：
  - T04 麦克风/语音识别/通知 TCC 授权态与撤销态：记为 **OWNER_DEFERRED**（未验证，不计 PASS）；
  - T04-ASR-01 维持 deferred；
  - T03 manual“通知点击后聚焦”因通知不测，维持 manual/blocked；
  - 已做且保留：辅助功能/屏幕录制已授权（service_process，真实状态证据）；computer-use 权限状态可查询；
  - v3.5 主窗口“系统权限”卡片代码保留（Tauri-only，作为后续用户自行授权的入口；本次不要求业主点击）；不将其计入验收通过。
- 其余可见验收（托盘、快捷键、窗口控制、拖拽、T01 双壳、T05、T07 交付冒烟）继续执行；最终报告如实写三项 deferred。
- 关闭本次权限会话 app 与隔离 home4 进程（ps=0）。


## 5.86 候选 v3.11 收尾：业主点名功能完成 + 单次可见批次 + 延后项（2026-10-27T10:08:15+08:00）

- 业主决定（2026-10-27）：Word/Excel 预览与首次设置都补；允许可见副作用测试；交付版文件任务用自动点击验收；之后把没做的测试记录、提交推送，并说明这些延后项获业主同意、可开始 R10。
- 已完成代码：
  - Office 预览：`read_docx_html`/`read_xlsx_html` 宿主命令 -> 本机服务 `/lingxi/v1/fs/{docx,xlsx}-html`（20MB/绝对路径/普通文件/symlink 拒绝，失败 null）；T03 4 用例真实样本 PASS。
  - 首次设置：服务 `POST /lingxi/v1/preferences/setup-complete`（LocalOnly，合并写 `setupComplete:true`）+ 宿主 `onboarding_complete`（main/onboarding caller）；保存失败/偏好损坏显式报错；T03 4 用例 PASS。
  - 权限卡与 T01：卡片 Electron/Tauri 同构；A01 DOM 文本投影移除宿主状态节点，T01 A/B 22 用例 PASS。
- 最终候选 v3.11（交付 v3.10 构建 + G1 重建）：delivery exe `8899065e...`、appTree `6a63e819...`、renderer `7adba587...`、G1 `1fc8b00c...`；static audit 58 PASS / scan 45 PASS / release suite 51 PASS。
- 最终可见批次：T03 34 passed/23 failed/3 blocked/3 manual；T01 parity PASS、T01 release offline PASS；T07 hidden E2E PASS；T02 supervisor/ownership PASS；T06 ACL PASS_WITH_BLOCKED_WINDOWS（107/35 blocked）；T05 untrusted PASS、T05 browser_pdf FAIL（Chromium PDF 运行时）；T07 release smoke 仅 release.file-task 未实现自动点击。
- 业主同意延后、不计 PASS：麦克风/语音识别/通知 TCC、ASR、通知点击人工项、交付版文件任务自动点击、T06 可见副作用 full 批次（授权未执行）、T08 最终候选重跑、T05 Chromium PDF、其他三平台、正式签名、final-06 正式门。
- 未跑新正式门 final-06：延后项仍会红，按业主指示以“范围收窄 + 显式延后”交接；R09 不宣称硬指标①/②全绿，R10 准入由业主本轮明确授权。
