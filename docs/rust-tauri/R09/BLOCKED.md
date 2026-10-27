# R09 阻塞记录

登记状态：BLOCKED（完成判定见文末「R09 完成判定：BLOCKED」）

状态取值：OPEN / RESOLVED / WAIVED（WAIVED 仅限业主书面裁决，本文件不自行豁免）。
每条必须写：受影响 ID、已做步骤、解除条件、责任、截止阶段。“R00 登记最晚 R10”不等于 R09 自动豁免。

| ID | 状态 | 阻塞内容 | 受影响 ID | 已做步骤 | 解除条件 | 责任 | 截止 |
|---|---|---|---|---|---|---|---|
| BLOCKED-PLATFORM-MACOS-X64 | OPEN | 本机仅 macOS arm64，无 macOS x64 真机/VM | R09-T07 全部平台场景；R09-A13 的 macOS x64 组；依赖四平台的 A15 对照 | 已确认 host `aarch64-apple-darwin`；mac x64 交叉目标构建可在本机验证编译，但原生 E2E/系统权限/安装验证不能替代 | 提供 macOS x64 真机/受控 VM 并跑原生 E2E 与安装冒烟 | 业主/环境 | R10 前（R09 记 BLOCKED_PLATFORM） |
| BLOCKED-PLATFORM-WINDOWS-X64 | OPEN | 无 Windows x64 机器 | R09-T07 全部 Windows 场景；T03/T04/T06 的 Windows 安装/权限/快捷键/托盘验证；T07 release 冒烟 | 已确认无 win 环境；交叉编译不能替代原生验收 | 提供 Windows x64 真机并跑原生 E2E/安装/权限 | 业主/环境 | R10 前 |
| BLOCKED-PLATFORM-LINUX-X64 | OPEN | 无 Linux x64 机器 | 同上（Linux 组） | 已确认无 linux 环境 | 提供 Linux x64 真机（含 WebView/图形驱动记录） | 业主/环境 | R10 前 |
| BLOCKED-SYSTEM-PERMISSIONS | OPEN | macOS 系统级权限（麦克风/屏幕录制/辅助功能/语音识别）在无 GUI 自动化与真实签名身份时无法完整撤销-恢复演练；本机 TCC 需要人工授权/系统设置操作 | R09-T04（A07/A08 的系统侧）、T03 依赖系统权限的能力 | 将先完成权限状态检测、拒绝态、请求态与自动恢复检测；真实撤销需人工在系统设置执行 | 有可交互桌面会话与明确授权执行 TCC 变更；或由业主/人工按脚本执行撤销步骤并保留录屏 | 业主/人工 + 实现 | R09-T04 验收前 |
| BLOCKED-SIGNING-IDENTITY | OPEN | 无 Apple Developer 正式签名/公证身份；release 安装包只能 ad-hoc 签名（与 Electron R08 本地包同构），不能声称正式分发签名/公证 | R09-T06 产物签名配置证据、T07 release 冒烟中的签名项、发布授权项 | 先以 ad-hoc 签名 + 本地隔离安装做功能证据；签名配置代码与文档落盘 | 提供 Developer ID 证书与公证凭据，或业主明确接受本地功能证据边界（不冒充分发验证） | 业主 | R10/R11（发布授权独立） |
| BLOCKED-TAURI-CLI-OFFLINE | RESOLVED | 初始无 tauri CLI；网络可达 npm registry，已按锁定版本安装 `@tauri-apps/cli`（记录进 lockfile） | 构建命令 | 已确认 curl npm 200；安装版本固定 | 已解除 | 实现 | — |
| BLOCKED-REAL-PROVIDER | OPEN | 真实模型供应商凭据未授权 | 非 R09 主路径；R09 使用确定性替身（不属 R09 特有阻塞） | 沿用替身协议 | 业主授权真实账号 | 业主 | R10 |

## 平台状态说明

- 平台缺失一律按任务书 04 §3 写 `BLOCKED_PLATFORM`，不用交叉编译或 Linux/macOS 截图代替其他平台的原生/权限/安装验证。
- R09 门禁（`verify-stage R09`）只执行本机可执行证据；平台缺失不进入“PASS”，在 `R09_HANDOFF.json.envBlockers` 与上表登记，阶段状态至多 `READY_FOR_REVIEW`。

## 自动化环境边界（2026-10-26 增补）

| ID | 状态 | 内容 | 处理 |
|---|---|---|---|
| ENV-KEYCHAIN-PROMPT | OPEN(受控) | 自动化会话内以隔离 profile 启动真实 Google Chrome 时，macOS 弹“找不到用于储存“Chrome”的钥匙串”对话框（Chrome Safe Storage Keychain，非 Lingxi 弹窗）。会阻塞自动化并打扰用户。 | 测试/诊断 Chrome 启动统一加 `--use-mock-keychain`；正式产品 launcher 保持真实钥匙串默认（不降级 cookie 加密）；测试 harness 负责进程树清理并断言无残留。产品真实用户会话不触发该边界。 |

| T03-REPAIR-02 | OPEN | 托盘「修复组件…」原能力在 Tauri 无等价实现（Electron triggerArtifactRepairFlow）；按 Lead 裁决已从托盘菜单移除，不保留 no-op 点击项 | R09-T03/T08（宿主矩阵该能力条目） | 已核对 Electron 实现与 Tauri 可用 API；菜单现为 show/settings/—/quit 4 项并登记缺失 | 实现等价修复流程（R11 清退/修复入口迁移后接线），或业主裁决该能力退出产品 | Lead/R09-T08 | R10 前给出裁决/实现 |
| T03-NOTIFY-02 | OPEN | macOS 通知点击聚焦：已实现 UNUserNotificationCenter delegate（willPresent/didReceive→激活 app+focus_main），但真实点击→聚焦尚未在注册后的 GUI 会话验证；Windows/Linux 点击聚焦仍 unsupported | R09-A05 通知叶、T03 通知能力 | spike 结论排除插件路径；delegate 代码+临时 crate 编译测试 182/182 | 宿主注册后真实投递通知并点击，录屏/窗口前台证据；win/linux 真机适配或 BLOCKED_PLATFORM 记录 | r09-native-caps/T07 | R09-A05 验收前 |

## 自动化 UI 副作用边界（2026-10-26 增补，用户可见事件后立规）

| ID | 状态 | 内容 | 处理 |
|---|---|---|---|
| AUTOMATION-UI-SIDE-EFFECTS | OPEN(受控) | R09 宿主在自动化套件中会按“真实用户路径”弹原生对话框（已发生：T02 A03 类崩溃重试耗尽对话框，且文案 `{LINGXI_HOME}` 未插值）、可能投递通知/显示托盘等，打扰用户。 | 宿主增加测试注入 sink：`LINGXI_R09_HOST_DIALOG_SINK=<file>` 时失败对话框写 JSON 不弹 modal（生产默认不变）；通知等系统可见动作在自动化中只做最小可观察样本并立即清理；每个 producer 的 finally 断言无 app/服务/对话框残留；文案必须插值真实路径（脱敏 token）。责任人：r09-host-core（T01/T02）、后续 T03/T07 套件同规。 |

## 候选 v2 前登记的实现缺口（T08 A16 盘点，2026-10-26 23:1x）

| ID | 状态 | 内容 | 受影响 | 解除条件 | 责任 | 截止 |
|---|---|---|---|---|---|---|
| HOST-V2-GAP-01 | OPEN→FIXING | `tauri-host-capabilities.ts` 44 成员片段从未被 `tauri-desktop-host.ts` 合并 → Tauri 下非 boot PlatformApi 成员调用即抛 HostCapabilityError（A16 缺口；候选 v1 因此作废） | R09-A05/A16、T03/T04 叶 | host-core 合并片段 + 单测；重建候选 v2 并重跑 hidden/visible 证据 | r09-host-core + T06 | R09 最终门 |
| T06-ONBOARD-01 | OPEN | `onboarding_complete` 返回 NOT_IMPLEMENTED（Rust 服务无 `/api/preferences/setup-complete` 等价路由；与 Electron Rust 模式一致） | T03 onboarding 4 断言（1120B2/6774E7） | 服务补路由/新契约（另立任务）或产品裁决迁移首设流程 | T03/T08 | R10 前 |
| T06-CONTEXT-MENU-01 | OPEN | `host:context-menu-policy` 无实现/登记（清点 50 能力之一） | T03 上下文菜单能力、A16 | 实现等价策略命令或 drop-with-reason 裁决 | T06/T08 | R10 前 |
| T06-FILE-URL-01 | OPEN | `host:file-url-asset` 未接线（assetProtocol enable+空静态 scope 已配，运行时授权根/getFileUrl 等价缺失） | 文件预览/媒体叶（4173A0 等）、D8 | T03/T05/T06 实现运行时受限 scope + convertFileSrc；重建候选并验证 | T03/T05/T06 | R09 最终门（预览相关叶） |

## 最终门 final-05 未满足项（2026-10-27 工具冻结后重跑；独立复核修正保留，如实登记，不绿洗）

| ID | 状态 | 内容 | 受影响 | 解除条件 | 责任 |
|---|---|---|---|---|---|
| GATE-R09-T03-01 | OPEN | `r09_t03_native_matrix` exit2：guiStats executed 60 / passed 21 / **failed 28** / blocked 11 / manual 3（final-05；execute/async+restart 修复后）；`plan::logic-case-references-exist` 缺 18 引用；`r09_t03_ipc_negative` exit2（blocked 为主） | A05/A06、T03 叶 | 修复剩余 28 失败（真实断言/权限/工具面）；补齐 18 逻辑引用；manual 3 条取证 | T03/T08 |
| GATE-R09-T04-01 | OPEN | `permission_matrix` blockedKind `BLOCKED_SYSTEM_PERMISSIONS|BLOCKED_GUI_SMOKE|BLOCKED_REAL_PROVIDER`；`remote_boundary` `BLOCKED_GUI_SMOKE|BLOCKED_SYSTEM_PERMISSIONS`（TCC/GUI 与真实供应商，不是仅凭据问题） | A07/A08、T04 叶 | 真实 TCC 人工会话 + GUI smoke + 凭据 | T04 |
| GATE-R09-T06-01 | OPEN | `capability_matrix` 21 个 `blocked::runtime.*`；门后 ACL hidden `runtime-acl-v33-02` PASS_WITH_BLOCKED_WINDOWS / 105 cases / **blocked 35**（27 allow-side-effect + 7 window-open + 1 union-full-side-effect） | A11/A12 | visible/受控 side-effect 批次或产品裁决 | T06 |
| GATE-R09-T07-01 | OPEN | `r09_t07_release_smoke` exit2：`t07::release.file-task` **硬编码 False**（detail `PENDING_T06_COMMANDS`，未真实执行）；G1 scroll 仅 maxOverflow=0/scrollContainers=0、focus documentHasFocus=false，长列表/工具卡 pending | A13/A14 | 实现 release 文件任务断言 + 长列表/工具卡/scroll/focus 真实交互 | T07/T06 |
| GATE-R09-T08-01 | OPEN | T08 final-05：shell failed 60（capability pending 33 / blocked 3 / pendingCount 59 + `a15::parity-not-yet-complete` + `parity::capability::host:context-menu-policy` 与 host 侧转绿证据矛盾 → shell parity 口径冲突），host failed 25（a16Ok 25/50 + 24 能力运行期 pending）；原 producer 红 context-menu evidence 与 debug-commands-gated 已转绿 | A15/A16 | 修复 shell parity context-menu 口径 + 24 能力运行期证据 | T08 |
| GATE-R09-PLATFORM-01 | OPEN | macos-x64 / windows-x64 / linux-x64 无真机，四平台安装/原生等价/权限未验证 | 硬指标①平台等价 | 真机/CI 环境 | 项目 |
| GATE-R09-SIGN-01 | OPEN | 仅 ad-hoc 签名，无正式签名/公证/安装器/自动更新验证 | A14 | Apple/平台签名凭据 | 项目 |
| GATE-R09-REVIEW-01 | RESOLVED | 独立复核 14 项发现（T03 43 failed 漏写、场景名/根因错配、ACL 绑定旧候选、T08 25/33、T04 remote 根因、T06 35、T07 scroll/focus、post-gate 文档、59 叶构成、file-task 根因、644、30/29、handover 35/43、18 引用） | 文档/证据 | 已逐项修正；ACL 重跑重绑 runtime-acl-v33-02 | Lead |

**门后补强证据**：`artifacts/rust-tauri/R09/dev/t06/runtime-acl-v33-02`（PASS_WITH_BLOCKED_WINDOWS exit0 / 105 cases / blocked 35；绑定候选 `candidate-handoff-v3.3`；hidden 诊断原始事件已归档 `hidden-automation-events.json`）。**候选二进制未变**：delivery exe `ddbd1fad…` / G1 `27bf075a…` / renderer `b0a20250…`；**formal gate 回执为 final-05**（`artifacts/rust-tauri/R09/verify-r09-final-05/`）。

## R09 完成判定：BLOCKED（2026-10-27，round 5）

**结论**：R09 在当前环境**无法达到完成条件**（硬指标②要求 formal gate overall=PASS、四组平台原生验证、缺证/未决阻塞=0），已按授权完成全部可执行部分并留下续跑入口；**不声称 R09 完成、不授予 R10**。

| 阻塞组 | 具体事实（已做步骤 → 解除条件） | 受影响 ID | 责任 | 截止 |
|---|---|---|---|---|
| 平台真机缺失 | macOS x64 / Windows x64 / Linux x64 三组无机器；已完成 macOS arm64 全部可执行验证（T01/T02/T05/T06 静态与 hidden、T07 G1 hidden、T08 只读矩阵），三组平台的安装/原生等价/权限验证未执行 → 提供真机或 CI 才能解除 | A13/A14、平台拆分叶、硬指标① | 项目/发布环境 | R10 前 |
| 签名/公证凭据缺失 | 仅有 ad-hoc 签名；无 Developer ID/notarization/安装器/自动更新验证凭据 → 提供 Apple 签名与公证凭据才能解除 | A14、安装/分发叶 | 项目/发布 | R10 |
| 真实系统授权会话缺失 | TCC 撤销/授权、屏幕录制、辅助功能、麦克风/语音、通知点击聚焦等需人工在系统设置操作；自动化不得伪造授权态 → 人工会话或授权环境才能解除 | A07/A08/A12、T04 叶、T03 manual 3 条 | 人工 + T04 | R10 |
| 真实供应商凭据缺失 | T04 permission 的 `BLOCKED_REAL_PROVIDER` 分支（真实供应商/远端）无凭据 | A07/A08、T04 叶 | 项目/供应商 | R10 |
| 未完成实现/证据（可在 R10 前继续，但本环境未能收敛） | T03 正式门 final-05 仍 28 failed（真实断言/权限/工具面：splash capability 范围、keep-awake IOPMAssertion、shortcut 注册、TCC 截图、settings/snapshot 响应等）+ 11 blocked + 3 manual；T07 `release.file-task` 硬编码 False；T06 21 runtime blocked；T08 shell parity context-menu 口径冲突 + 24 能力运行期 pending；DOCX/XLSX 宿主命令、onboarding 服务路由缺失 → 修复后需新候选+全新目录重跑正式门 | A05/A06/A11/A12/A13/A14/A15/A16 | T03/T04/T06/T07/T08 | R10 前 |

**已交付的可执行部分**：候选 v3.3 冻结（delivery `ddbd1fad…` / G1 `27bf075a…` / renderer `b0a20250…`）；final-05 正式门 receipt `artifacts/rust-tauri/R09/verify-r09-final-05/`（sha256 `a5b9da43…`，overall FAIL，candidate stable=true，13/21 命令 PASS，74 叶 15 PASS/59 BLOCKED/0 FAIL）；T05/T06 静态/ACL hidden/T07 G1/T02 hidden 等可执行验收证据齐备；独立复核 14 项发现已修正。

**续跑入口**：`docs/rust-tauri/R09/PROGRESS.md` §5.73–5.79；`R09_CLOSEOUT_CHECKLIST.json`（逐命令/逐叶）；本文件上表；`R09_HANDOFF.json`（finalGate/blockers/allowedNextScope.granted=false）。

## 业主裁决落实（2026-10-27）

| 原条目 | 新状态 | 依据 | 执行 |
|---|---|---|---|
| T03-REPAIR-02 | RESOLVED | OWNER-01：业主确认菜单永久移除 | 文档/清单关闭，不实现 |
| BLOCKED-PLATFORM-MACOS-X64 / WINDOWS-X64 / LINUX-X64 | DEFERRED_BY_OWNER（不阻塞本机 R09 收口） | OWNER-02：当前只做 macOS Apple 平台验证 | 报告声明“四平台未完成，业主延后”；不冒充 |
| BLOCKED-SIGNING-IDENTITY | WAIVED_BY_OWNER（R09 及后续不做正式签名） | OWNER-03 | 只用 ad-hoc 本机证据；不声称分发/公证/自动更新 |
| BLOCKED-REAL-PROVIDER（LLM 部分） | RESOLVED_BY_LOCAL_MODEL | OWNER-05：http://127.0.0.1:1234 / qwen/qwen3.8-27b | 记录配置；T04-ASR-01 仍另列 deferred |
| T04-ASR-01 | DEFERRED_BY_OWNER | OWNER-05：本地语音识别延后 | 不伪造 ASR 结果，报告如实写 deferred |
| T06-CONTEXT-MENU-01 | RESOLVED | 已由 renderer 等价实现（final-05 T08 绿） | 文档纠正，无产品裁决需求 |
| BLOCKED-SYSTEM-PERMISSIONS | IN_PROGRESS_OWNER_SESSION | OWNER-06：业主正在进行人工授权 | 启动可见 app 引导点击；授权后补证据 |


## 业主同意延后（2026-10-27，记录后 R10 可开始）

1. T04 麦克风/语音识别/通知 TCC 授权态与撤销态；ASR 真实识别。
2. T03 通知点击聚焦人工项（通知本身按业主决定不测）。
3. T07 交付版文件任务“自动点击验收”：交付 UI 目前无稳定文件任务入口，实现未完成；如实延后。
4. T06 可见副作用 full 并集批次：业主已授权，但本轮未执行；hidden 批次已 PASS_WITH_BLOCKED_WINDOWS。
5. T08 最终候选 v3.11 的 Electron/Tauri 全量 A/B 重跑。
6. T05 Chromium PDF：本机 Chromium PDF 运行时失败（环境/发行物），untrusted 预览 PASS。
7. 其他三平台、正式签名/公证（此前已由业主决定延后/放弃）。
8. 新正式门 final-06：延后项仍红，按业主指示提交交接，不冒充 PASS。

以上均由业主在 2026-10-27 明确同意延后；R09 以“代码实现 + 可执行验证 + 显式延后”交接，R10 准入由业主授权。
