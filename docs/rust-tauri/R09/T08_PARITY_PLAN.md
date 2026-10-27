# R09-T08 A15/A16 GUI A/B 补充证据计划（等 Lead clearance；**本文件不触发任何启动**）

状态：**PLAN / 未执行**（当前 v2 自检全部为无 GUI 聚合；本计划是 A15/A16 转绿所需的最小 GUI 批次提案）
日期：2026-10-26 | 生产者：`r09-t08-parity` | 关联：`T08_PROGRESS.md` §4、`docs/rust-tauri/R09/T07_PROGRESS.md` §9–§11、
`docs/rust-tauri/R09/T06_GUI_BATCH_NEEDS.json`、`docs/rust-tauri/R09/HOST_V2_PATCHES.md` §7.2

## 0. 结论先行

> 状态更新（2026-10-27 00:0x）：v3.2 候选下 **T07 hidden H1/H2/H3 已全部 PASS 并绑定 T08**（H1 30 cases 含 file-url-asset/context-menu；
> H2 supervisor 10 + ownership 13；H3 release hidden supervisor 31 + ownership 19），A15 measured T02 行与 A16 file-url-asset 已用 hidden 证据转绿；
> 本计划剩余部分即 **visible 5 次启动**（V1–V4）+ T01/T05/T03/T04 v3.2 证据重跑。

1. **A15 的“同内核同前端”部分已经由无 GUI 证据成立**：T01 同窗 Electron/Tauri（同一 renderer + 同一 `LINGXI_SERVICE_BIN`）
   的 DOM/服务身份/退出与 T02 supervisor 语义、T05 旧链 vs Rust 链 35 项、T07 G1-hidden 均已实测 → A15 measured 13 行 parity。
2. **A15/A16 剩余红是“宿主真实动作与运行期边界”证据缺失**，不是静态接线缺失：33 个能力 pending 中 19 个是
   “已接线但无真实动作结果”，26 个共享宿主叶 pending。这些只能由 hidden/visible GUI 批次产生。
3. **不需要为 T08 单独设计一套新的 GUI A/B**：按 Lead 2026-10-26 23:3x 裁决，把 T08 观察点并入**修订版 visible
   5 次启动**：V1 release_offline（1）、V2 G2 交付 release（1）、V3 react_parity **双壳**（Electron+Tauri 各 1）、
   V4 T03 gui-actions（1）——合计 **5 次可见启动**；**无预期弹窗**（失败/崩溃/协议负向全部移到 hidden 批次）。
   T08 不额外新增启动。
4. 3 条人工观察断言（拖出到 Finder、托盘菜单语言截图、通知点击聚焦）在任何自动化批次后仍需人工/录屏证据
   （`LINGXI_R09_MANUAL_EVIDENCE_DIR`），不冒充自动化通过。

## 1. 修订版 visible 5 次启动（Lead 裁决；T08 只读消费）

| 批次 | 启动 | 应用/模式 | 预期可见结果（无预期弹窗） | T08 取用行 |
|---|---|---|---|---|
| **V1 release_offline** | 1 | 交付 release `.app`（v3.2 候选 appTree `2008b71f…`；含页面 meta CSP asset 修复）离线启动（无 dev server） | 页面 `tauri://localhost`、无 5173、服务 READY、优雅退出；失败走 sink/日志不弹 modal | A15 fixed factors 复核（app tree/exe/build.json/protocol/renderer）；`host:main-window`/`app-ready`/`server-connection-info`/`app-lifecycle-quit` 运行期再验；A16 release artifact 行（command surface/无 test hooks） |
| **V2 G2 交付 release 冒烟** | 1 | 交付 release 可见窗口，真实 Tauri 产品 UI | 启动/连接/页面来源/文件任务（能自动化则自动化，否则 MANUAL_STEP+截图）/退出 exit 0、ps 0 | T07 `r09_t07_release_smoke`（T08 只读消费，不重复跑）；A16 交付 release 能力行（`host:main-window/app-ready/file-text-io?` 仅作 release 面复验）；A15 页面来源/嵌入页 recorded 项复核 |
| **V3 react_parity 双壳** | **2**（Electron 先启动保持、再 Tauri） | 同一 v3.2 renderer dist（`b0a20250…`） + 同一 `LINGXI_SERVICE_BIN`；Electron 与 Tauri 各 1 个可见主窗 | 两壳同窗归一化 DOM 投影（title/platform/React 根/bodyTextHash/长度）、服务 health 五字段、退出清理一致；无异常弹窗 | A15 measured 行从 hidden 升级为**可见同窗**：renderer identity/DOM projection/page embedding/service identity/exit-and-isolation；A16 `host:main-window`/`app-ready`/`server-connection-info`/`app-info-platform`/`app-lifecycle-quit` 的 real-behavior 证据（覆盖 T01 hidden 行，形成可见 A/B） |
| **V4 T03 gui-actions** | 1 | evidence wrapper 可见窗口，按 `T03_LEAF_CASE_PLAN.json` 执行 | 托盘菜单/快捷键→快聊/通知投递（点击聚焦按人工）/文件面板/拖出拖入（人工）/窗口控制/主题广播/设置窗/快捷聊天动作 | A15 33 个 pending 能力中的 19 个（tray/global-shortcut/notification/file-dialogs/clipboard/drag-out/external-open/trash/keep-awake/auto-launch/theme-sync/settings×2/quick-chat×2/reload-restart/splash/window-controls/window-state/single-instance/i18n-main）；A16 对应能力 evidenceMaturity `support-cases-only → real-behavior`；T03/T07 共享 26 叶的 plannedActionCase 真跑 |

**负向/失败语义**（崩溃重试耗尽、协议/摘要/身份不符、IPC 越权、ACL deny、canary/debug 命令不可达、恶意预览）**一律移 hidden 批次**：
不占可见启动、不产生预期弹窗；T08 复用 T02/T03/T05/T06 既有 hidden 证据（v3.2 后重跑）。

**新增可见启动总计：5（V1+V2+V3 双壳+V4）**；T08 自身 0 次。串行、失败即停、批次间 ps=0。

### 1.1 T08 行 ← 批次证据映射（重跑时直接更新 UPSTREAM/矩阵）

- V1 → `r09_t01_release_offline` 证据（可见批次；`LINGXI_R09_T01_VISIBLE=1` + delivery/wrapper env）→ A15 fixed factors +
  `host:main-window/app-ready` 运行期行；A16 release artifact 行。
- V2 → `r09_t07_release_smoke` 证据（交付 release 可见冒烟）→ A15 页面来源/嵌入页；A16 交付 release 面。
  **注意**：T07 `r09_t07_release_smoke` 因 `release.file-task` PENDING（delivery 无 hooks/wdio）为**显式登记红**，
  正式门也将 exit2（gate 不可能 PASS 的已知原因之一）；T08 保留其红，不绿洗。
- V3 → `r09_t01_react_parity` 证据（双壳可见；`LINGXI_R09_T01_VISIBLE=1`）→ A15 measured 行升级 visible A/B；A16 real-behavior caps。
- V4 → `r09_t03_native_matrix`（visible 分支：`LINGXI_R09_GUI_CLEARED=1` + gate wrapper + webdriver + manual dir；8 blocked 断言 exit2 已登记）
  + `r09_gui_actions` 产物 → A15 19 能力 + 26 叶；A16 real-behavior。
- T05/T04 非 host-core 生产者：其 v3.2 重跑由 owner/Lead 安排（T05 服务侧 Chrome 证据/T04 TCC 人工证据），落盘后 T08 按同门/in-gate 消费。
- hidden（v3.2 后）：T02 supervisor、T03 IPC、T04 remote/permission（TCC 人工除外）、T05、T06 ACL、T07 G1-hidden → T08 对应 pending/blocked 行。

## 2. T08 需要的观察点（并入 B/C 批次；逐项绑定 A15/A16 行）

### 2.1 宿主动作（A16 能力 → A15 行为对照行）

| # | 动作 | 预期可观察结果 | 采集 | 对应能力/叶 |
|---|---|---|---|---|
| 1 | 托盘：菜单 4 项（show/settings/—/quit）+ tooltip + 双击 | 菜单栏图标存在、菜单项文案随 locale、双击显示/聚焦主窗、quit 走清理 | `r09_gui_actions` CGWindowList + AX 菜单枚举；截图 | host:tray（T03-REPAIR-02 缺失项按 RECORDED_APPROVALS 记录，不要求修复项存在） |
| 2 | 全局快捷键（注册/冲突/拉起 quick-chat） | 注册成功状态、冲突回执、快捷键触发 quick-chat 可见 | CGEvent 合成快捷键 + CGWindowList；`keybindings_test_register` 回执 | host:global-shortcut、quick-chat-window |
| 3 | 通知：投递 + 点击聚焦（mac） | 通知横幅出现；点击后主窗前台/聚焦；win/linux 显式 unsupported | UNUserNotificationCenter 投递记录 + 点击（人工或 CGEvent）+ 前台探针 | host:notification（T03-NOTIFY-02 人工/点击） |
| 4 | 原生文件面板（select_folder/select_files/select_skill） | 面板出现、取消返回空、选择返回绝对路径 | AX 面板存在 + 取消/选择回执；截图 | host:file-dialogs |
| 5 | 拖出到 Finder / 拖入（文件 drop） | Finder 收到文件；drop 后渲染层拿到路径（原生 drop 适配） | 人工观察 + Finder 目标目录 diff；drop 事件 | host:drag-out（manual）、host:file-drop-paths |
| 6 | 窗口控制：最小化/最大化/关闭/恢复 | 状态切换正确、`window-is-maximized` 回执、关闭到托盘策略 | CGWindowList bounds + 事件；截图 | host:window-controls、host:window-state |
| 7 | 主题/外观广播：主窗+设置窗+viewer 同步 | 同一次 `settings_changed` 后各窗口重绘且持久化 | 截图 + 事件日志 | host:theme-sync、host:settings-broadcast |
| 8 | 设置窗：打开/聚焦/切 tab | 设置窗出现并切到指定 tab；重复调用聚焦 | CGWindowList + wdio DOM tab 投影 | host:settings-window |
| 9 | 快捷聊天：show/hide/resize/会话交接 | 窗口显示/隐藏、尺寸只增不缩、ack 后焦点回主窗 | CGWindowList + `quick-chat-shown` 事件 + DOM | host:quick-chat-window、quick-chat-session-handoff |
| 10 | splash/onboarding 流程 | splash 信息/头像展示；onboarding 完成路径返回显式错误（T06-ONBOARD-01） | 截图 + 命令错误码 | host:splash、host:onboarding |
| 11 | 单实例：第二实例启动 | 仅聚焦既有主窗、第二进程 exit 0、无第二服务 | 双进程 + ps/CGWindowList | host:single-instance |
| 12 | auto-launch 状态读/写 | 状态回显真实；写操作在 ad-hoc 下显式受限/失败（BLOCKED-SIGNING-IDENTITY） | 系统登录项读回 + 回执 | host:auto-launch |
| 13 | keep-awake 开关 | `pmset -g assertions` 出现/释放断言；失败不假报 | `pmset` 前后快照 | host:keep-awake |
| 14 | file URL asset（`getFileUrl`→`<img>`） | scope 内文件 `onload` 成功；scope 外路径加载失败 | wdio DOM `img.complete` + naturalWidth；CSP violation 日志 | host:file-url-asset（T07 G1 v2 已在做；可见批次可复核） |
| 15 | 右键菜单策略 | 非编辑区无默认菜单；textarea/contenteditable 保留系统菜单（右键粘贴） | GEvent 右键 + 截图；`r09_t01` 右键策略 vitest 已覆盖 renderer 行为 | host:context-menu-policy（已有 jsdom 行为证据，可见批次只作可选复核） |
| 16 | 文件 IO / viewer / skill-viewer / browser-viewer（v3.2 renderer 成员已接线；browser-viewer 细粒度命令按 T05 显式受限+理由） | 文件预览/viewer 窗口/skill viewer/browser viewer 面板在可见批次可观察；运行期证据由本批次与 hidden 共同补齐 | 截图 + wdio DOM + 命令回执 | A16 6 族能力（当前 a16Ok 仅等运行期证据） |

### 2.2 A13 真实输入（T07 batch B 已含，T08 只读复用）

中文 IME 合成/真实输入、Range 选区引用、长列表滚动、焦点切换、DPI/resize、工具卡展开、关窗退出——
T08 不重复要求，直接复用 `t07::native-e2e.*` v2 G1/visible cases；对应 A15 的 UI 行为行只作为
`host:main-window/app-ready` 的补充，不单列新 parity 行。

## 3. 启动次数与时间盒（修订版；总可见 5 次）

| 批次 | 启动 | 可见性 | 预计耗时 | T08 用途 |
|---|---|---|---|---|
| hidden（v3.2 后，已有批次复用） | hidden：T02 supervisor/ownership（已有模式）、T03 IPC、T04 remote、T06 runtime ACL 1 发、T07 G1-hidden 1 发 | 不可见（automation+sink） | 各 2–10 min | A15 负向/监督/ACL 行；A16 运行期 ACL / file-url asset |
| **V1 release_offline** | 1 | 可见 | ~1 min | A15 fixed factors/主窗/服务/退出 |
| **V2 G2 release 冒烟** | 1 | 可见 | ~2 min | T07 release smoke / A16 release 面 |
| **V3 react_parity 双壳** | 2（Electron+Tauri） | 可见 | ~3 min | A15 measured 可见 A/B |
| **V4 T03 gui-actions** | 1 | 可见 | ~4 min | A15 19 能力 / A16 real-behavior / 26 叶 |
| 人工断言（3 条） | 0（人工） | 可见（人工操作） | 依人工 | drag-out/托盘截图/通知点击 |
| **合计** | **可见 5 + hidden ≤5** | — | ≤10 min 可见 | 无预期弹窗；失败即停、不重试 |

## 4. 逐步观察点（修订版：先 V1/V2 冒烟，再 V3 双壳 A/B，最后 V4 动作；全部无预期弹窗）

1. **V0 预检（每批）**：ps=0；候选 sha 与 v2 handoff 一致（`d01ece20…`/`63250ddb…`/`f69a8100…`）；隔离 `LINGXI_HOME`/`TMPDIR`；
   `LINGXI_R09_ALLOW_GUI=1` 与 visible clearance 已记录；**不设** automation/sink（visible 语义）。
2. **V1 启动**：1 个可见主窗；`window-created`→`app-ready.windowVisible=true`；页面 `tauri://localhost`；无 5173。
   截图 1 张；记录窗口标题/位置。
3. **V2 托盘/菜单**：按 §2.1#1 观察；截图；记录 locale 文案。
4. **V3 快捷键→quick-chat**：注册回执 + 合成快捷键 + quick-chat 可见；截图。
5. **V4 通知投递（+人工点击）**：投递可见；点击后主窗前台；人工证据槽写入。
6. **V5 文件面板**：打开→取消→再打开→选择；三次回执 + 截图。
7. **V6 拖出/拖入（人工）**：拖到 Finder；drop 后路径回执。
8. **V7 窗口控制/窗口状态**：最小化/最大化/关闭到托盘；重启后窗口位置恢复（host:window-state）。
9. **V8 主题/设置广播**：主窗改主题→设置窗/viewer 同步；切 tab；截图。
10. **V9 快捷聊天控制 + 会话交接**：show/hide/resize/ack 焦点。
11. **V10 splash/onboarding**：splash 信息；onboarding 完成（应显式 NOT_IMPLEMENTED，**不得假装成功**）。
12. **V11 keep-awake/auto-launch 状态**：pmset 前后断言；登录项状态读回（写操作按签名边界记录）。
13. **V12 关窗退出**：`RunEvent::ExitRequested` → 清理 owned 服务 → exit 0；ps=0；sink 不存在；诊断 residual=[]。
14. 每步失败立即关闭窗口、停止整批、报告；**不重试**。

## 5. 证据映射（批次完成后 T08 自动升级的矩阵项）

- **A15 measured 行**：V3 双壳 react_parity（v3.2 新证据）、V1 release_offline、hidden G1/ACL/`cases.json` 会成为 `UPSTREAM` 直接输入；
  T08 生产者只读消费后：能力行 `pending → parity`（若 A 侧 Electron 引用成立）或 `blocked`（若登记 blocked）；
  v3.2 已无「未接线」缺口（6 族 wired / browser-viewer restricted-registered），剩余红仅为运行期证据 pending。
- **A16 能力行**：`evidenceMaturity` 从 `support-cases-only` 升级为 `real-behavior` 的能力需同时满足
  (a) 真实动作 cases PASS、(b) 消费者/接线成立；`host:file-url-asset` 需 T07 `file-url-asset` case PASS。
- **74 叶**：T03 GUI 行动计划（`T03_LEAF_CASE_PLAN.json` 60 断言）与 T04 真实 TCC/服务证据、T05 viewer 窗口证据
  完成后，由 Lead 在 `r09_stage_inputs.py::FULL_BINDINGS` 登记；**T08 不自行登记叶绑定**，只校验已登记 case 真实 PASS。
- **A15/A16 最终证据**：可见批次 + hidden ACL + G1 完成后，用**全新正式门目录**跑 `verify-stage R09`
  （in-gate 上游证据自动绑定 v2 候选），T08 两套件作为最后命令组产出最终 `result.json`/`cases`/matrix；
  Lead 据此汇总 `SHELL_PARITY_RESULTS.json`/`R09_REPORT.md`/`R09_HANDOFF.json`。

## 5.1 已知 gate-blocking 红（登记，不为过门改绿）

| 项 | 状态 | 说明 |
|---|---|---|
| T07 `r09_t07_release_smoke` | 显式红（exit 2） | `t07::release.file-task` PENDING：delivery release 无 hooks/wdio，无法驱动真实读写 UI；正式门亦然 |
| T03 `r09_t03_native_matrix` | exit 2（visible 批次后仍可能保留 blocked） | 8 条 blocked 真实系统动作/运行期 ACL 断言已登记（T06_GUI_BATCH_NEEDS / T03_T04_PROGRESS） |
| T06 `r09_t06_capability_matrix` | exit 2 | 仅 `blocked::runtime.*` / `blocked::allow-side-effect::*`（副作用动作需 visible 批次；已登记） |
| T04 TCC/helper/ASR/截图 | blocked | BLOCKED-SYSTEM-PERMISSIONS / BLOCKED-REAL-PROVIDER / T04 OQ-04 已登记 |

T08 对上述项只做「已登记红」披露（candidate-binding/upstream-status 如实红），不冒充 PASS。

## 6. Clearance 与安全边界（必须遵守）

- 任何 GUI 启动前必须有 Lead 书面 clearance；一次只跑一个批次、串行、失败即停、不循环重试。
- visible 批次使用真实应用窗口（不设 `LINGXI_R09_AUTOMATION`/sink）；hidden 批次必须设 automation+sink。
- 每批次结束：`ps` 0 残留、窗口关闭、诊断 `residual:[]`、托盘/通知等系统可见动作最小化并清理；
  任一步出现意外窗口/弹窗/非预期退出 → 立即停止并报告 Lead/用户。
- 录屏仅在用户明确授权时进行；截图不得包含用户隐私桌面内容（只截应用窗口或说明）。
- 3 条 manual 断言必须真实人工观察/录屏，不冒充自动化；TCC 变更需人工在系统设置执行并保留记录。
- 平台缺失（mac x64/win/linux）仍按 `BLOCKED_PLATFORM`，不得用截图/交叉编译替代原生验证。

## 7. 若 Lead 不批准任何 GUI 批次（或后续候选未就绪）

- A15/A16 保持 `READY_FOR_REVIEW`-with-pending 或如实 FAIL：`SHELL_PARITY_RESULTS.json` 不得写 PASS；
  最终报告按 `BLOCKED`/pending 披露（T08 生产者当前红证据即为如实状态）。
- 无 GUI 可继续完成的独立项：T01/T02/T05 v2 证据重跑（hidden 但非 GUI，已由 owner 执行）、T06 静态门（已 PASS）、
  T07 G1-hidden v2（hidden）、T07 release smoke（需要 1 次启动，属 visible 语义）。
