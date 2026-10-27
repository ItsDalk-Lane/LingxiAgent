# R09 阶段报告（Tauri 桌面宿主、原生能力与跨平台集成）

> 状态：**BLOCKED / READY_FOR_REVIEW**（`goalStatus=BLOCKED`；不声称 R09 完成，不授予 R10 准入）
> 最终正式门：`artifacts/rust-tauri/R09/verify-r09-final-05/verify-stage-result.json` → **overall: FAIL**
> 候选：v3.3（delivery app tree `97f226bf…`、host exe `ddbd1fad…`、G1 evidence exe `27bf075a…`、renderer `b0a20250…`、sidecar `55d6edb1…`）
> 候选绑定：final-05 门内 **stable=true**（before/after candidateSourceDigest 一致 `d1c7da14…`，29221 files，testedSha `aac8cb9f` + uncommitted worktree）
> 文档时点：final-05 于 2026-10-27 01:2x 结束（门后工具修复冻结后重跑）；随后仅同步文档/证据包，候选二进制未变。T06 ACL 隐藏证据 `runtime-acl-v33-02` 为门后补强（绑定 v3.3，见 §7）。
> 独立复核：独立只读 reviewer 已完成（14 项发现）；文档已逐项修正；`independentReviewPassed=false`（R09 未完成，且 final-05 回执晚于复核）。

## 1. 范围与授权

- 目标：把现有 React 渲染层 + Rust 服务接入完整 Tauri 2.12 宿主，做到**可安装、原生能力等价、权限可靠**。
- 本阶段只做 R09；R10 未开始；不推送/合并 main/不发布/不迁移真实数据。
- 依据 R00 冻结：8 tasks / 16 base scenarios / 74 dedup 补充叶 / 168 原始断言 / 任务 0 的 46 共享叶权威分类（HOST_NATIVE 22 + HOST_BRIDGE 8 + SERVER_REACT 16）。

## 2. 源码与依赖

| 项 | 值 |
|---|---|
| 分支 / HEAD / 研究基线 | `codex/rust-tauri-migration` / `aac8cb9f…`（工作区未提交）/ R00 基线 `aac8cb9f` |
| Tauri | 2.12.1（CLI 2.12.4，tauri-build 2.7.2） |
| Rust | 1.98.1（`rustup run 1.98.1`；PATH 上 Homebrew 1.93 不遵循 rust-toolchain.toml） |
| Node / React / Vite | v24.16.0 / React 19 / Vite 7 |
| 宿主边界 | `desktop/src/react/host/desktop-host.ts`（kinds electron/tauri/web/mobile）；Tauri 实现 `tauri-desktop-host.ts` + 能力片段 `tauri-host-capabilities.ts`（v3 起正式合并） |
| 服务接缝 | `rust/crates/lingxi-service/src/transport.rs::check_origin` + `preview.rs` frame-ancestors 精确允许 `tauri://localhost`/`http(s)://tauri.localhost`；伪造 scheme/子域/端口 403 |
| 特征门控 | 交付 release 仅 `r09-host`；证据构建 `r09-host,r09-test-hooks[,e2e-test]`；hooks-only 命令不进交付 |

## 3. 环境与证据条件

- 机器：macOS 27.0.1 arm64，CLT 27.0（无 Xcode.app），仅 ad-hoc 签名（`BLOCKED-SIGNING-IDENTITY`）。
- 隔离：所有测试 home/TMPDIR 位于 `/private/tmp/lingxi-r09-*`；真实 `~/.lingxi` 未触碰。
- 自动化：`LINGXI_R09_AUTOMATION=1` + `LINGXI_R09_HOST_DIALOG_SINK` 抑制原生 modal/窗口；visible 批次显式 clearance、串行、preflight/post ps 快照、异常即停。
- final-05 门内除 Lead 写证据目录外无其他写入者；candidate binding stable=true。
- 已处置的环境/工具事件（证据留痕）：
  - 隐藏 ACL 首轮 wdio 挂起（无 deadline/进度）→ harness 加 10s/8s/45s/300s 超时与 progress.jsonl；撤回“probe timeout 当 allow”的错误分类。
  - 页面 meta CSP 未放行 `asset:` → `desktop/src/modules/connection-csp.js` 修复，asset 本地预览真实加载（H1 聚合 30 cases PASS）。
  - `target/debug` 被 T02 suite 内 `cargo test` 覆盖 → T02 改隔离 `CARGO_TARGET_DIR`；证据只从 handoff sha + `/private/tmp` 稳定副本消费。
  - T05 PDF Chromium 在负载下出现 CDP `Not attached to an active page` 环境抖动 → 生产者仅对该已知错误做一次真实重跑（两次过程取证），final-05 T05 73 cases PASS。
  - 门后 T06 ACL harness 仍硬编码旧 handoff → 改为动态最新 handoff + 归档 hidden 诊断证据，重跑 `runtime-acl-v33-02`（绑定 v3.3，见 §7）。
  - 门后工具修复已冻结进 final-05：T03 GUI 动作驱动 `execute/sync→execute/async`、restart 断连容忍/恢复、stale batch 清理；T08 context-menu 能力模型改 renderer-policy、debug-commands-gated 允许 hooks-only `debug_asset_probe` 并排除 dev capability。final-05 中 T03 `guiStats` = 21 passed / 28 failed / 11 blocked / 3 manual；T08 context-menu evidence 与 debug-commands-gated 已绿，shell parity 行 context-menu 仍有口径冲突。final-05 后的第二批工具修复（prefs 重试/容错 JSON、spawn_viewer 按 `{path}` 签名、通知 delivered 探针改 `.app` bundle）在 dev 复跑得到 T03 22 passed/27 failed（`/private/tmp/r09-t03-recheck-05`），**未重跑正式门**，正式回执数字仍为 final-05 的 21/28。

## 4. 完成项（逐 T-ID）

| T-ID | 状态 | 要点 |
|---|---|---|
| T01/T02 宿主与监督 | ✅ final-05 命令 PASS | T01 react_parity + release_offline PASS（Electron↔Tauri 同 renderer 对照）；T02 supervisor_ready / process_ownership PASS（debug hidden 10+13、release hidden 31+19；READY 116ms、协议/摘要/身份注入拒绝、无 Node 回退、SIGTERM 清理、owned/reused/remote zero-touch） |
| T03 原生能力 | ⚠️ **真实失败 + blocked** | final-05 `r09_t03_native_matrix` exit2：`guiStats` executed 60 / **passed 21 / failed 28** / blocked 11 / manual 3；`plan::logic-case-references-exist` 仍缺 18 个引用。28 条失败含真实断言（splash allow 范围、keep-awake IOPMAssertion、shortcut 注册、appearance 截图、window-controls 等）、工具面（prefs JSON 解析、通知 delivered 探针缺 .app、viewer 参数）等。`r09_t03_ipc_negative` exit2（blocked 为主）。 |
| T04 TCC/helper/截图/ASR | ⛔ blocked | permission_matrix blockedKind `BLOCKED_SYSTEM_PERMISSIONS|BLOCKED_GUI_SMOKE|BLOCKED_REAL_PROVIDER`；remote_boundary blockedKind `BLOCKED_GUI_SMOKE|BLOCKED_SYSTEM_PERMISSIONS`（GUI/TCC，不是“真实凭据缺失”单独导致） |
| T05 浏览器/PDF/媒体 | ✅ PASS | final-05 `r09_t05_browser_pdf` 73 cases PASS（PDF 74 页/140 表格行/相似度 0.99910/三族字体）；`untrusted_preview` 53 cases PASS；asset 本地预览真实加载 |
| T06 权限/CSP/发布攻击面 | ⚠️ 静态全绿 + 运行期部分 | 静态：capability audit 58 PASS（gapLedger=0、73 命令、四集合一致）、release attack surface 45 PASS、release suite 51 PASS。运行期 ACL（门后补强证据）：`runtime-acl-v33-02` PASS_WITH_BLOCKED_WINDOWS exit0 / 105 cases / 0 non-blocked failures；blocked **35** = 27 allow-side-effect + 7 window-open + 1 union-full-side-effect；hidden 证据两次 app_ready 均 windowVisible=false。 |
| T07 原生 E2E | ⚠️ G1 绿 / release smoke 红 | final-05 `r09_t07_native_e2e` PASS（聚合 30 cases = cases.json 29 条 + 1 聚合条目）+ `t07::release_smoke` exit2：`t07::release.file-task` 为**硬编码失败**（detail `PENDING_T06_COMMANDS`，未真实执行）；G1 中 scroll 仅 maxOverflow=0/scrollContainers=0、focus `documentHasFocus=false`，长列表/工具卡 pending（不得算 PASS）。 |
| T08 替壳对照/宿主完整性 | ⚠️ 未清零 + shell 口径冲突 | final-05 shell_parity failed 60（capability pending 33/blocked 3/recorded 8/parity 6；pendingCount 59；`a15::parity-not-yet-complete` + `parity::capability::host:context-menu-policy` 与 host 侧转绿证据矛盾）；host_completeness failed 25（a16Ok 25/50 + 24 能力运行期 pending；context-menu evidence 与 debug-commands-gated 已绿）。宿主 a16Ok **25/50**；PlatformApi 109 = 76 real / 0 noop / 5 absent+reason / 28 restricted+reason / 0 silent；73 命令四集合 73/73；74 叶 bound15/pending_gui23/blocked12/manual_pending4/excluded_non_host16/green_unbound4；占位 unregistered 0。 |

## 5. 验收（A01–A16；final-05 为准；场景名取自 `stage_maps/R09.json` specMirror）

| 场景 | 名称（地图） | 状态 | 依据/阻塞 |
|---|---|---|---|
| A01 | 同一React产物可运行 | PASS | T01 react_parity + release_offline |
| A02 | 生产不依赖开发服务器 | PASS | T01 release_offline |
| A03 | 错误二进制明确拒绝 | PASS | T02 supervisor_ready（注入拒绝） |
| A04 | 退出清理只影响自有进程 | PASS | T02 process_ownership |
| A05 | 用户动作真实生效 | FAIL | T03 native_matrix：21 PASS / **28 FAIL** / 11 blocked / 3 manual |
| A06 | 不可信参数无法执行任意命令 | FAIL | T03 native_matrix 28 failed + T03 ipc_negative blocked 为主 |
| A07 | 撤销权限后不继续采集 | FAIL | T04 permission_matrix blocked（TCC/GUI/真实供应商）、remote_boundary blocked（GUI/TCC） |
| A08 | 远端任务不能自动读取本机设备 | FAIL | T04 remote_boundary blockedKind `BLOCKED_GUI_SMOKE|BLOCKED_SYSTEM_PERMISSIONS` |
| A09 | 移除Electron后能力仍工作 | PASS | T05 browser_pdf 73 + untrusted_preview 53 |
| A10 | 恶意预览内容无宿主权限 | PASS | T05 两套件 + service seam 13/13 |
| A11 | capability叠加不意外扩权 | FAIL | T06 capability_matrix 21 blocked::runtime.*（运行期 GUI/ACL 矩阵），release surface/audit 绿 |
| A12 | 生产无测试控制面 | FAIL | T06 capability_matrix 21 blocked 运行期项；T08 debug-commands-gated 已绿（final-05），但运行期矩阵仍未执行 |
| A13 | 中文输入和卡片交互可用 | FAIL | T07 native_e2e 聚合 30 绿（IME 合成已披露；scroll/focus/长列表/工具卡 pending）但 release_smoke `file-task` 硬失败 → 场景 FAIL |
| A14 | 测试版通过仍需release验证 | FAIL | T07 release_smoke exit2：`t07::release.file-task` 硬编码 False（PENDING_T06_COMMANDS） |
| A15 | 同内核同前端对照成立 | FAIL | T08 parity：13 measured 全 parity、unapproved=[]，但 capability pending 33/blocked 3、pendingCount 59；A15 未成立 |
| A16 | 宿主功能无占位 | FAIL | T08 a16Ok 25/50（25 失败均为运行期证据 pending）；占位扫描 unregistered 0 |

**门汇总（final-05）**：overall FAIL；命令 **13/21 PASS**；74 叶 **15 PASS / 59 BLOCKED / 0 FAIL**；candidate stable=true（`d1c7da14…`，29221 files）。
（注：`handoverCompleteness` 35 项/`priorStageDeferred` 43 项是 R00 408 叶透镜的“承诺/递延”视图，非本轮 74 叶重复计数；gate 因 overall FAIL 记录 verifiedPass=0、gaps=[]，详见回执。）

## 6. 补充叶覆盖（74 叶 / 168 断言）

- 74 叶构成（T08 hostSummary）：bound 15（=本门 15 PASS 契约叶）、pending_gui 23、blocked 12、manual_pending 4、excluded_non_host 16、green_unbound 4 → gate 的 59 BLOCKED = 23+12+4+16+4。
- 15 叶（`FULL_BINDINGS`：T05 浏览器/PDF/媒体契约）final-05 **PASS**（r09_t05_browser_pdf 73 cases 全绿，逐断言 case 的 ok/actual/expect 由门禁机器核对）。
- 59 叶 BLOCKED：含 **16 excluded_non_host**（SERVER_REACT 等非宿主归属，不计宿主完成度）与 **4 green_unbound**（证据已绿但未绑定契约），其余为 T03 GUI/真实动作、T04 权限、T06 运行期 ACL、T07 release/file-task 等未执行；不冒充 PASS。
- 46 共享叶分类（HOST_NATIVE 22 + HOST_BRIDGE 8 + SERVER_REACT 16）见 `T07_LEAF_MAPPING.json` 与 `R09_LEAF_LEDGER.json`。
- 合同/负例自检：map-check 19 cases PASS；gate-negative 7 cases PASS；inputs digest/byte 一致。

## 7. 安全与数据

- CSP 收紧：`default-src 'self'`，connect-src 仅 `ipc:`/`127.0.0.1`；`img-src/media-src` 精确加 `asset:` 与 `http://asset.localhost`（页面 meta CSP 与 Tauri 配置双源一致，audit 有回归 case）。
- 服务 origin 接缝：仅精确允许 3 个 Tauri origin；伪造 scheme/子域/端口仍 403；13/13 真实服务探针验证。
- 命令面：73 条，invoke_handler / build.rs AppManifest / permissions / capabilities 四集合一致；交付构建 0 个 hooks/调试命令（审计 case）。
- asset protocol：静态 scope 三档 LINGXI_HOME+`$TEMP/**`，运行时仅授权已验证真实目录（拒绝相对/文件/根/符号链接），无全盘 `/**`。
- ACL：逐窗口 capability 精确叶子（不用 `core:default`）；`runtime-acl-v33-02` 实测 main allow 40/40、deny 9/9、union==policy、token-not-in-url-dom、hidden 双 app_ready windowVisible=false；**blocked 35**（27 side-effect allow + 7 window-open + 1 union-full side-effect）在 hidden-only 策略下未执行，如实披露；诊断原始事件已归档于该证据目录 `hidden-automation-events.json`。
- 数据：真实 `~/.lingxi` 未触碰；测试 home 全隔离；无密钥入代码/报告。

## 8. 已知缺陷/边界（阻塞项与不阻塞项分开）

**阻塞（gate FAIL 根因，按命令）**：
1. **T03 native_matrix 真失败**：28 failed（真实断言/权限面/工具面）、11 blocked、3 manual；`plan::logic-case-references-exist` 缺 18 引用 → A05/A06 FAIL。
2. **T03 ipc_negative**：exit2（负向矩阵以 blocked 为主，未全部执行）。
3. **T04 permission/remote**：TCC 撤销/helper/截图/ASR/GUI smoke 需真实系统与人工（`BLOCKED_SYSTEM_PERMISSIONS`/`BLOCKED_GUI_SMOKE`）；permission 另含 `BLOCKED_REAL_PROVIDER`。
4. **T06 capability_matrix**：21 个 `blocked::runtime.*`（hidden-only 下 GUI/侧效应矩阵未执行）。
5. **T07 release_smoke**：`t07::release.file-task` 硬编码 False（PENDING_T06_COMMANDS），非真实通过；G1 scroll/focus 仅记录指标/监听，长列表/工具卡 pending。
6. **T08 两套件**：host 25 failed（a16Ok 25/50 + 24 能力运行期 pending）；shell 60 failed（pending 33/blocked 3/pendingCount 59 + `a15::parity-not-yet-complete` + `parity::capability::host:context-menu-policy` 与 host 侧转绿证据矛盾 → shell parity 口径冲突）。context-menu evidence / debug-commands-gated 两项原 producer 红已在 final-05 转绿。
7. **平台**：macos-x64 / windows-x64 / linux-x64 无真机（`BLOCKED_PLATFORM`）。
8. **签名**：仅 ad-hoc，无正式签名/公证/安装器验证（`BLOCKED-SIGNING-IDENTITY`）。
9. **实现缺口（R10 前）**：onboarding_complete NOT_IMPLEMENTED（无服务路由）、browser-update 全量流 deferred、file_drop_paths 可选未做、DOCX/XLSX 宿主读取命令缺失、拖出/通知点击等 manual。

**不阻塞/已接受工程决定**：
- renderer 树含 644 文件；跨构建非确定性（settings.html CSS link 顺序）→ 每轮 build-once-bind-once，R10 决定是否规范化。
- IME 为 WebDriver 合成事件（已披露，非系统输入法端到端）。
- asset 运行时 scope 依赖 LINGXI_HOME/workspace preferences。
- notification 点击聚焦仅 macOS UNUserNotificationCenter delegate，真实点击待 manual；win/linux unsupported。
- 托盘“修复组件”已移除（T03-REPAIR-02）。
- host SIGKILL 后 sidecar 孤儿由下次启动清理（已记录）。
- T05 CDP 抖动重试为**两轮真实取证**的测试稳定性措施（仅限 `Not attached to an active page`/browser_remote 已知错误），不是放宽断言。

## 9. 未执行/BLOCKED

见 `BLOCKED.md` 与 `R09_CLOSEOUT_CHECKLIST.json` knownBlockers；未执行项：
- T03 28 failed 中未完成的真实动作（含 viewer/settings/onboarding/通知/快捷键/主题/窗口控制等）+ 11 blocked + 3 manual；
- T04 TCC 撤销/helper/截图/ASR 与 remote GUI smoke；T04 remote 与 permission 的真实供应商部分；
- T06 运行期 ACL side-effect 27 条 + window-open 7 条 + union full side-effect 1 条（hidden-only 不执行）；
- T07 release file-task（硬编码未执行）与长列表/工具卡/scroll/focus 真实交互；
- T08 25 能力运行期证据 + shell parity context-menu 口径冲突；
- 三平台安装/原生等价/权限验证；正式签名/公证/自动更新/卸载；
- 通知点击、托盘五语言、拖出 Finder 等人工动作。

## 10. 回退

- 交付 release `.app` 为 ad-hoc 签名；回退 R08 Electron 宿主只需恢复 `desktop/main.cjs` 启动路径（未删除）；Tauri 改动均在 git 工作区可 `git checkout` 回退；Rust 服务接缝改动（transport/preview）为新增允许项，回退不影响既有 Electron/Rust 路径。
- 数据：无真实数据迁移；测试 home 全在 `/private/tmp`。

## 11. 独立审查

- 独立只读 reviewer 已完成（不构建/不启动 GUI/不写文件），发现 **14 项**（high：T03 43 failed 被 8+3 覆盖、5 个场景名/理由错配、v3.3 ACL 证据绑定旧候选；medium：T08 25 vs 33、T04 remote 根因、T06 ACL 35 vs 21/27、T07 scroll/focus 写 PASS、post-gate 文档未绑定候选、59 BLOCKED 构成未解释、T07 file-task 根因；low：644/645、30/29、handoverCompleteness、18 引用未单列）。
- 本版报告/handoff/parity/BLOCKED/PROGRESS 已按发现逐项修正；ACL 证据已重跑重绑（`runtime-acl-v33-02`）；T08 两个 producer 红在 final-05 已转绿/单列。
- 结论：**独立复核完成，但 R09 未完成，不构成“独立复核通过/准予 R10”**；`R09_HANDOFF.json.independentlyReviewed=false`、`independentReviewPassed=false`。
- 待办：shell parity context-menu 口径修复；T03 剩余 28 失败修复；最终再次独立复核。

## 12. 下一阶段

- `allowedNextScope`：**R09 未完成，R10 不授予**。
- R10 前置（按优先级）：① T03 28 failed/11 blocked/3 manual 真实动作修复；② T07 release file-task 真实实现与长列表/工具卡/scroll/focus；③ T04 TCC/GUI smoke 人工会话 + 真实供应商凭据；④ T06 visible/runtime ACL side-effect 批次；⑤ T08 shell parity 口径修复与 24 能力运行期证据；⑥ 三平台真机 + 正式签名/公证；⑦ DOCX/XLSX/onboarding 服务路由等实现缺口；⑧ renderer 确定性与 Chromium 分发决策。
- **完成判定：BLOCKED（2026-10-27，round 5）**——三平台真机、签名/公证凭据、真实 TCC/供应商凭据与部分实现证据在当前环境不可达（详见 `BLOCKED.md`「R09 完成判定：BLOCKED」）；交付为未完成+续跑入口，不称完成、不授予 R10。
- 续跑入口：`PROGRESS.md` §5.73–5.79 + `R09_CLOSEOUT_CHECKLIST.json` + `BLOCKED.md`。

---

## 附录 A：正式门与关键命令（final-05）

```bash
# 地图/证据合同自检
python3 -B scripts/rust-tauri/r09_stage_inputs.py --check
python3 -B scripts/rust-tauri/r09_map_integrity.py --evidence /private/tmp/r09-map-int-check   # PASS 19
python3 -B scripts/rust-tauri/r09_gate_negative_probe.py --evidence /private/tmp/r09-neg-check # PASS 7

# 正式门（全新证据目录；env 见 docs/rust-tauri/R09/R09_GATE_ENV_PLAN.md）
~/.cargo/bin/rustup run 1.98.1 cargo run --manifest-path rust/Cargo.toml -p xtask -- \
  verify-stage R09 --evidence artifacts/rust-tauri/R09/verify-r09-final-05
# → overall FAIL（13/21 命令 PASS；74 叶 15 PASS / 59 BLOCKED / 0 FAIL；candidate stable=true=d1c7da14…）

# 门后补强：T06 隐藏 ACL（绑定 v3.3，1 次 hidden app）
LINGXI_R09_T06_RUNTIME_ACL_CLEARED=1 LINGXI_R09_T06_RUNTIME_ACL_HIDDEN_ONLY=1 \
  LINGXI_R09_AUTOMATION=1 python3 -B scripts/rust-tauri/r09_t06_runtime_acl.py \
  --evidence artifacts/rust-tauri/R09/dev/t06/runtime-acl-v33-02 \
  --app /private/tmp/lingxi-r09-evidence/lingxi-resource-desktop-v33-27bf075a --wdio-port 19535
# → PASS_WITH_BLOCKED_WINDOWS exit0 / 105 cases / blocked 35
```

## 附录 B：证据索引

| 证据 | 路径 |
|---|---|
| 最终正式门 | `artifacts/rust-tauri/R09/verify-r09-final-05/verify-stage-result.json` |
| T01/T02 门内证据 | 同目录 `SUITES/r09_t01_*`、`SUITES/r09_t02_*` |
| T03 失败/blocked 明细 | `SUITES/r09_t03_native_matrix/result.json`（guiStats 21/28/11/3）与 `cases.json`（logic refs 18） |
| T04 blocked 明细 | `SUITES/r09_t04_permission_matrix/result.json`、`r09_t04_remote_boundary/result.json` |
| T05 | `SUITES/r09_t05_browser_pdf`（73）、`r09_t05_untrusted_preview`（53） |
| T06 | `dev/t06/{v33-audit-01,v33-scan-01,v33-release-suite-01,v33-capability-matrix-01,policy-fix-audit-01}`；`runtime-acl-v33-02`（门后补强，含 hidden diagnostics） |
| T07 | `SUITES/r09_t07_native_e2e`（聚合 30）、`SUITES/r09_t07_release_smoke`（file-task 红）；dev：`g1-hidden-v32-02`（29 条 cases.json + 聚合）、`h2-*`、`h3-*` |
| T08 | 门内 `SUITES/r09_t08_*`（shell 60 failed / host 25 failed；context-menu evidence 与 debug-gated 绿）；dev 自检 `dev/t08/v32-round-01` |
| 候选事实 | `dev/t06/candidate-handoff-v3.3/candidate-handoff.json` + `release-evidence.json` |
| 地图/合同 | `rust/crates/xtask/src/stage_maps/R09.json`、`R09_INPUTS.json`、`scripts/rust-tauri/r09_stage_inputs.py` |
| 清单 | `R09_CLOSEOUT_CHECKLIST.json`、`SHELL_PARITY_RESULTS.json`、`R09_HANDOFF.json` |


## 13. v3.11 收尾交接（2026-10-27T10:08:15+08:00，业主授权提交/推送）

- 状态：R09 **未达到正式门全绿**；以“业主点名功能完成 + 可执行验证 + 显式延后”交接，不宣称硬指标①/②全部达成。
- 候选：delivery exe `8899065e330ca88f22b16ae70cbbad1f4eacefc5c85cdb179b3a75dd40e4cb8b`，appTree `6a63e81966d68213...`，renderer `7adba587...`，G1 `1fc8b00cb342eddd...`；static gates：audit 58 PASS、scan 45 PASS、release suite 51 PASS。
- 本轮完成：Word/Excel 宿主预览、首次设置完成（服务+宿主，真实失败分支）、权限卡 Electron/Tauri 同构与 T01 A/B 22 PASS；T02/T05 untrusted/T06 hidden ACL/T07 G1 hidden 均 PASS。
- 最终可见批次：T03 34P/23F/3B/3M；T01 parity PASS；T01 release offline PASS；T05 browser_pdf FAIL（Chromium PDF 运行时）；T07 release smoke 仅 release.file-task 未实现自动点击。
- 业主同意延后（不冒充 PASS）：麦克风/语音识别/通知 TCC、ASR、通知点击人工项、交付版文件任务自动点击、T06 可见副作用 full 批次（授权未执行）、T08 最终候选重跑、T05 Chromium PDF、其他三平台、正式签名、final-06 正式门。
- 结论：R09 实现与独立可执行验证已按业主要求收尾；**R10 阶段任务由业主明确授权可以开始**。后续若要求正式门全绿，需先补齐上述延后项并重跑全新证据目录。
