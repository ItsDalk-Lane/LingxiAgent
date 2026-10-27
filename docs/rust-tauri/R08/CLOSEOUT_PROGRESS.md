> **最终状态（2026-10-26 20:35）：R08 完成。** 正式门 `verify-r08-closeout-08`：
> 命令 22/22、场景 16/16、补充叶 126/126 PASS（0 BLOCKED / 0 FAIL）、候选绑定 stable=true、
> overall=**PASS**；独立上下文验收 **ADMIT_R09**。本文件按时间顺序记录续跑过程，**最终权威结论与
> 证据索引见文末「独立上下文验收」与「最终回归与工作区状态」两节**；早段「状态总览/断点」为历史快照。

# R08 收尾进度台账（CLOSEOUT_PROGRESS.md）

本文件是收尾会话（/goal 直接实施）的进度账。上一会话终态见 PROGRESS-t08.md 第十一轮
（verify-r08-final-15：命令 17/17、场景 16/16、叶 20 PASS / 106 pending、overall=FAIL）。
本会话任务：修复验收基础设施（A）、恢复链（B）、客户端语义（C），补齐 106 叶（D），
取得可信 R09 准入结论。开工回执：CLOSEOUT_START_RECEIPT.md。

## 状态总览

| 域 | 范围 | 状态 |
|---|---|---|
| 任务0 | 事实核对、回执、三件套 | 完成（2026-10-25；机器清单 CLOSEOUT_CHECKLIST.json 126 项） |
| A 验收基础设施 | A04 接入、命令引用完整性、t07 差异进判定、t08 隔离校验 | 完成（负例红→绿已证，见下） |
| B 恢复链 | 附件路径/身份、Register 类文件、旧会话可变更新 | 完成（真实 d3 演习红→绿已证） |
| C 客户端语义 | pinnedAt 适配、continue-deleted-agent/workspace-disposal、D-10 查找 + 绑定表 | 完成（红→绿已证） |
| D 106 叶 | D01 23 / D02 48 / D03 18 / D04 11 / D13 3 / D20 3 | **部分完成**：+27 叶（47/126 PASS，79 pending）；续跑断点见文末 |
| 正式门 | verify-r08-closeout-01 / 02 | 02：命令 20/20、场景 16/16、stable=true、叶 47/79/0、overall=FAIL（仅因 79 pending） |
| 回归入口 | typecheck / cargo build / lib 单测 / npm test | 0 错 / 通过 / 50+8 通过 / 20 文件历史红与基线逐数相同（无关） |
| 结论 | R09 准入 | **未达成**（硬指标①：47/126）；不授予 R09 准入 |

## 逐项记录（根因→修复→同类检查→命令/退出码/关键输出）

### A① A04 未执行
- 根因：门只执行被场景/叶引用的命令；`r08_a04_no_final_ws` 仅注册未引用，`r08-a04-card-expansion.test.tsx` 不在任何套件 argv。
- 修复：`r08_stage_inputs.py` 新增套件 `r08_a04_ui_vitest`（verbose 报告→`ui-cases.json` 合同）；A04 场景精确绑定两套件 + `suiteTestAnchors`（cargo `test … ok` 锚）+ `assertionContract`（3 个 UI 案例）。
- 实跑：`cargo test … --test r08_a04_no_final_ws` → `test result: ok. 1 passed`；`npx vitest run …r08-a04-card-expansion.test.tsx` → 3 passed。（首次 cargo 构建红：TMPDIR 指向不存在目录致 aws-lc-sys 汇编"unable to make temporary file"——环境误配非产品问题，建目录后绿。）

### A② 只注册不引用 / 关键 skip
- 发现：上轮 final-15 门内 `r08-session-legacy-compat.test.ts` 因缺 `LINGXI_R07_SERVICE_BIN` 被 `skipIf` 静默跳过（日志 `1 skipped`）仍 PASS。
- 修复：run_suite 注入该变量；vitest 验收套件任何 skipped → `guardFailures` 非零退出；生成器校验"命令只注册未引用"与"引用未注册"直接 SystemExit；`--check` 增加整图逐字节重生成比对（手改地图即红）。
- 负例：注册孤儿命令 → `命令只注册未引用: ['x_unreferenced_suite']` exit 1；手改 A13 案例名 → `inputs digest MISMATCH; graph bytes MISMATCH` exit 1；恢复后 `inputs digest match` exit 0。

### A③ A/B 差异进 A13/A14
- 修复：`r08_t07_ab` 产出 `ab-parity-cases.json`（35 任务逐条，parity=1；recorded 须在 `RECORDED_APPROVALS` 登记：sessions.switch=D-3、sessions.create=D-2/D-4）；A13 合同绑全部任务，A14 绑负路径任务；未批准差异/缺观测 → 0 → 场景 FAIL。
- 实跑（修 C③ 后）：`AB_RESULT tasks=35 parity=29 differs=4 recorded=2`；`GUARD_FAIL` 4 项（cleanup.boundary / memory-snapshot / permission.session-mode / sessions.maintenance）→ 待本轮修复（见"A/B 差异修复"）。

### A④ 隔离检查从不失败
- 根因：`r08_t08_isolation_check.mjs` 只产摘要；`defaultOff=false` 仍 exit 0；无前后比较；合成根使用为固定声明。
- 修复：重写为 V1–V5 硬校验（前置快照由 `r08_inputs_check --evidence` 写 `R08_INPUTS/real-home-before.json`，a15 事后比对；预览默认关以 `rust-local-service.cjs` 的 env 门控源码判定；逐套件回执 `argv` 实查合成根；进程 env 不指真实 home）。
- 负例：无前置快照 exit 1（V1）；回执 argv 指 `~/.lingxi` exit 1（V4）；真实 home 条目变化 exit 1（V1）；绿例 `verdict=ISOLATED violations=0` exit 0。

### B①②③④ 恢复链（`rollback_delta.rs` / `cutover_rollback.rs`）
- B① 根因：新身份 `session_files` 行原样 INSERT，`file_path/real_path` 指原新根；修复：包 lineage 记 `originNewRoot(+Canonical)`，回灌对 managed_cache 行按原根前缀重基到恢复根（`PathRebase`，external 不改写），`pathRebased` 计数入收据；二次回灌比对有效行→already_present（零增长成立）。
- B② 根因：`diff_files` 只收三类；修复：按 T04 分类表把全部 Register 策略类（agent-profile/agent-memory/…/channels/file-history）纳入通用 `register` 类目（排除 session-files/knowledge 专属映射与 T06 §4.2 明示的 device-access-registries 安全域）；回灌发散=整文件覆盖。
- B③ 根因：`export_db_delta` 对 sessions 按旧身份整行跳过；修复：sessions（可变元数据表）旧身份行也入包，`oldUiReadable=true`；账簿表仍按身份跳过（不可改账）。
- 同类检查：B④ 与 B② 合并（系统性按 Strategy::Register 纳入，非逐个特例）。
- 单元：`cargo test -p lingxi-adapters rollback_delta` → 8 passed（含新增 `session_files_rows_rebase_managed_cache_paths_only`、更新 `export_filters_legacy_identities_and_bookkeeping`）。
- 端到端红→绿（`r08_t06_d3_drill.mjs` 扩展：切后改名旧会话 / 附件 fileId / 先隔离原新根再 reforward --recover-new-root / 恢复根起服务经正式接口验收）：旧二进制（stash 两文件）`D3_RESULT failures=8 EXIT=1`（B1 路径仍指 `new-root`、字节不可读；B2 助手未列/404/不可切/不可建；B3 标题退回"父会话"）→ 修复二进制 `D3_RESULT failures=0 EXIT=0`。日志：`artifacts/rust-tauri/R08/closeout-proofs-01/b-domain/d3-{red-prefix,green-fixed}-binary.log`。

### C① pinnedAt 形状断链
- 根因：Rust 列表 `pinnedAt` 数字、pin 响应 `pinnedAtUnixMs`，store/组件只认字符串。修复：`session-actions.ts` 唯一边界 `pinnedAtFromWire`（列表映射 + pin 响应）。原断言"已删 Agent 会话禁止置顶"改用真实 `.deleted-agent.json` 标记（Rust 路由已有 `gate_agent_not_deleted`），不再以不存在会话顶替。
- 验收：`r08-pin-projection-compat.test.ts`（真实 Rust 子进程 + 真实 `buildSessionSections` 分组：即时/重载/双置顶排序/取消清除 + 已删助手 409）2 passed；红证（归一禁用）`expected 'object' to be 'string'` 1 failed。
- 顺带发现：预置 `agents/` 的无印 home 触发数据纪元闸拒启（产品红线行为正确），测试改为就绪后落助手。

### C② continue-deleted-agent / workspace-disposal
- 根因：两路由不在翻译表；Rust 续接响应无 `path`。修复：翻译表两条（sessionId 定位 / 同形 body）；动作层 `targetPath = path ?? native:<sessionId>`。
- 验收：`r08-continue-disposal-compat.test.ts`（真实动作→Rust→列表/切换；负路径 400 无副作用）2 passed；红证（翻译禁用）2 failed；`use-hana-fetch.test.ts` 104 passed。

### C③ D-10 会话内查找
- 根因：D-10 把 Rust 已实现的 `/sessions/{id}/find?q=` 记错为行定位路由 `/sessions/find`；客户端 `serviceRoute` 把 `/api/sessions/find` 粗映射到行定位路由（拒 q 400）。修复：翻译表 `find` 读端口 → `/sessions/{id}/find?q=`；从粗映射移除 `find`；Rust 路由接线 Bridge 脱敏（`loop_conversation_by_session` 判 bridge 身份）。
- 验收：cargo `r08_c3_find_bridge_redaction`（真桥会话表挂接 vs 普通会话对照）ok；红证（接线禁用）`bridge 会话命中片段必须脱去时间标签` FAILED。A/B：`leaf.find.in-session` 由 differs → parity。
- 同类接缝：绑定表生成器 POST 不带 body 致 14 条写端口误判 DUAL_BRANCH_ONLY → 修正探测形状后 `MAPPED 347→361 / DUAL 38→24`；find 行归 `legacySessionTranslation → /sessions/__p__/find`；旧 1:1 钉 `find?text=y` 的测试改为真实消费者形状。
- 叶 4F1F69CCA760 升级 full_original_behavior（A0: hit + bridge-redaction〔cargo 旁证经 r08_t07_ab 聚合〕；A1: invalid-rejected）。
- 另发现待处理：Rust 删除项目回 `assignment.sessionIds` 而 UI 读 `sessionPaths`（批次 1 一并修）。

### D 域（106 叶补证）——线束泛化与批次 1/D04

- 线束泛化：套件任务产出 `cases:{name:bool}` → 演习 `casesParity`（双腿同键全真=平价）→ 生成器 `derive_ab_cases` 自动派生 `leaf::<task>.<name>` 图钉。新叶只需套件案例 + `FULL_BINDINGS` 登记。
- 模型替身固定要素补齐（A13）：A 腿 `agents/a1/config.yaml` 持久 `models.chat:{id,provider}`；B 腿服务 `--config providers` 注入同一桩（d3 同款）+ 正式入口补建 a1。D-2/D-3/D-5 环境前提消除，`sessions.switch`/`sessions.create` 改真实对拍。
- 夹具对齐：A 腿归档文件 mtime 对齐 createdAt（D-9 消除）；夹具增至 5 个真实格式会话（破坏性任务殿后仍有材料）；B 腿 `--http-rate-max` 测试准入覆盖（默认 240/10s 被密集套件打成 429）。
- **Rust 缺陷修复（本轮新发现，均有红→绿）**：
  1. `new-detached` 对不存在 agentId 建会话（201）→ 补 `agent_dir_checked`（lib.rs）。
  2. UI 默认偏好 `permissionMode:"auto"` 建会话被 `sessions/new` 400 → session_focus 解析 auto 为默认具体模式（userprefs 既定裁决）。真实 UI 流 createNewSession 在 Rust 腿此前不能建会话。
  3. **迁移真实会话无法压缩/续聊**：T04 把 Pi `model_change`/`compaction` 物化为 `legacy-unknown-block`，model_history 遇到判"损坏"→ 跳过不透明遗留块（控制类同口径）。
  4. **迁移消息行全字段透传致模型上下文解码失败**（`unknown field id`）→ model_history 投影到规范化 role/content/modelCallId 再解码。红→绿探针：`artifacts/rust-tauri/R08/closeout-proofs-01/ab-drill/fresh-compact-{red-legacy-unknown-block,green-after-fix}.txt`。
- 客户端接缝：删除项目 `assignment.sessionIds`→本地路径映射（session-project-actions）。
- 新叶绑定（full_original_behavior，经 A/B 真实对拍）：D02 projects×10、conversation-map×5、checkpoints×4、workspace-rollback×2、sessions new/new-detached×2、find（C③）；D04 permission-mode×3。A/B 演习逐轮：run-02 35 任务 → run-09 43 任务（见 `/private/tmp/r08-closeout-ab-0N/ab.log`；最终以正式门 `SUITES/r08_t07_ab` 为准）。
- 新差异登记：D-11（旧栈启动自建会话）、D-12（非法 mode 旧栈静默接受 / Rust 400）、FIXTURE-PROMPT-SNAPSHOT（recorded，叶保持 pending）；P-ASK-DEFAULT 需产品裁决（见 CLOSEOUT_BLOCKED.md）。

### 续跑断点（未完成项，不冒充完成）

| 域 | 剩余 | 断点/所需线束 |
|---|---|---|
| D02 | 约 25 叶：fork、连同子会话归档/解除谱系归档、延迟内容、sweep-orphaned（需运行中会话）、rollback-preview、turns/retry、switch 浏览器恢复、todos×3（需计划面板内容）、prompt-snapshot、slash new/reset、tools checkpoint/rewind/session/session-folders、UI 地图页 | 多数需真实模型回合（stub 已在双腿就位，待 WS prompt 线束）；fork/archive-children 可先用 fixture 子会话（需核对 node fork body `target` 形状）。 |
| D03 MCP | 18 叶 | 需确定性 MCP stdio 替身（connectors CRUD/start/stop/refresh/resources/oauth/bulk/apps/enabled/defer/session-permissions/agent 配置/设置页）。 |
| D04 | 8 叶：confirm/reject×4（需待批动作=模型工具回合）、plan-mode read/toggle（Rust 无 /plan-mode 路由，需补或映射到 permission-mode read_only 语义）、默认 ASK（P-ASK-DEFAULT 裁决）、安全设置页矩阵 | — |
| D01 | 23 叶 | WS 聊天线束（prompt/steer/interject/abort/compact/resume-stream/slash、草稿读写、快捷聊天偏好、工具 ask_user/current_status/todo_write、A2C385 快捷窗口桌面行为）。 |
| D13 | 3 叶 | 终端 WS 线束（close/snapshot/tail）。 |
| D20 | 3 叶 | CLI 子命令线束（checkpoints/data restore/diagnose）。 |

## 续跑第二轮（2026-10-26，业主五项裁决后）

- 裁决落实：CLOSEOUT_ADJUDICATIONS.md（ADJ-1 改说明书 / ADJ-2 等效接受 / ADJ-3 保留严格 / ADJ-4、ADJ-5 以新版为准关闭）。裁决解锁绑定 3 叶：/new 485B2D0A8A02、/reset 3FCB259FF69C（Rust-authoritative 口径：案例判真取 Rust 腿，A 腿观测留档）、默认权限 25C4FF66FEE5（按实际语义 auto）。
- 新增 Rust 实现：/lingxi/v1/plan-mode GET/POST（userprefs.rs，现役旧 /api/plan-mode 同语义：无 mode 由 !!enabled 切 read_only/operate、有 mode 优先）+ 客户端粗映射（serviceRoute）。
- 新任务实跑绿：leaf.prefs.quick-chat（偏好读写+负路径）、leaf.drafts（读/home 存/session 存/清空/负路径）、leaf.ws.compact（compaction_accepted→result 双帧+无会话拒）、leaf.plan-mode（读面/两向切换/mode 优先/无权拒）。
- 修复的客户端缺陷（同 D01 族）：旧内核专用 wire 帧（context_usage、terminal_snapshot_request、assistant_run_end 后补 context_usage）此前会无条件发到 Rust 连接→ Rust close 1008 打断会话/回合；三处均补 Rust 连接门（websocket.ts / app-init.ts / session-actions.ts，run-19 帧级红证）。
- A/B 演习迭代 run-10..30（逐轮修 jsdom WebSocket shim、模型替身注入、夹具 Pi 形状 assistant 消息带 provider/model、Windows 帧）；终态 run-30：**51 任务 = 48 平价 + 3 recorded（slash.new、slash、prompt-snapshot——均有登记依据）+ 0 差异**；131 图钉案例 130 绿（唯一 bridge-redaction 旁证由门内 cargo 套件先行运行补齐）。
- 中途修复自身缺陷：套件 leaf.slash 块重复声明（二次观测覆盖致 before/after 失真）已删除并补 chat.abort 包装行。

### 正式门 verify-r08-closeout-03（2026-10-26）

- 命令 **19/20** PASS——唯一红 = `r08_t03_a05_drill`（row1-hide：Cmd+W 后 visibilityState 仍 visible）；**连续 3 次同签名**（门内 + 两次单跑，hideMechanism=visibility-appleevent-fallback）：宿主自动化焦点策略回退（本 ZCode 进程的 System Events 焦点变更再次被丢弃；同族问题既往已定名，closeout-02 与更早 final-14/15 均有键盘路径绿证）。列环境阻塞（见 CLOSEOUT_BLOCKED.md），不把阻塞算完成。
- 场景 **15/16**（A05 连带）；候选绑定 **stable=true**；叶 **61 PASS / 65 BLOCKED（required_behavior_pending）/ 0 FAIL**；overall=FAIL（唯一原因：65 叶 pending + A05 环境红）。
- 回归入口（同一候选）：typecheck 0 错；cargo --locked 构建通过；rollback_delta 8/8、触及模块 lib 单测 11/11；npm test 20 文件/121 失败与纯净基线 aa99d2ec1 逐数相同（无关历史红，证据 closeout-proofs-01/regression/）。

### 当前叶分布（closeout-03）

- PASS 61：D02 44 / D01 9 / D04 7 / D03 1
- pending 65：D02 22 / D01 14 / D03 18 / D04 5 / D13 3 / D20 3

### 续跑断点（65 pending）

| 域 | 叶数 | 所需 |
|---|---|---|
| D02 | 22 | todos×3（需模型工具回合 todo_write）、fork、连同子会话/解除谱系归档×2、延迟内容、sweep-orphaned、rollback-preview、turns/retry、switch 浏览器恢复、slash compact/fresh（模型替身 A05 读腿可扩）、tools checkpoint/rewind/session/session-folders、UI 地图页 |
| D01 | 14 | steer/interject/resume-stream、abort 排队、ask_user、todo_write、current_status、快捷窗口桌面行为（Electron）、abort POST task |
| D03 | 18 | 确定性 MCP stdio/http 替身（连接器 CRUD/启停/刷新/资源/OAuth/批量/apps/开关/延迟/会话权限/设置页） |
| D04 | 5 | 确认/拒绝×4（需待批动作=模型工具回合）、安全设置页矩阵 |
| D13 | 3 | 终端 WS 线束（close/snapshot/tail；Rust wire 的 terminal_* 帧族尚缺——旧帧到 Rust 会被拒，需补 Rust 终端域或专用 WS） |
| D20 | 3 | CLI 子命令线束（checkpoints / data restore / diagnose） |

## 续跑第三轮（2026-10-26，授权继续推进剩余叶）

### 本轮闭合（+15 叶：61→76）

- **D03 MCP CRUD 12 叶**：确定性 stdio MCP 替身 `scripts/rust-tauri/r08_mcp_standin.cjs`（零依赖 newline-JSON-RPC：initialize/tools/resources/ping）双内核真实启动；A/B 任务 `leaf.mcp.crud` 16 案例双腿全绿：全局开关/延迟设置/添加+异步启动（真实 running+工具目录）/刷新工具/停止→启动/修改/批量（合法行入库+坏行整批拒）/apps 列表/agent 连接器配置/会话一次性 MCP 能力授权（Rust 走精确 tool:mcp 目标+capability，grant 前置于 update——普通编辑沿语义停旧连接）/删除/非法连接器拒/无权读拒。
  - 修复过程中的真实差异定位：旧栈 add 不带 id 时用 name 生成 id（补 id 后对齐）；Rust 会话授权要求目标工具当前已注册（时序在案）。
- **D20 CLI 3 叶**：产品测试 `tests/cli-data.test.ts`（既有 19 用例 + 本轮补 1 用例「corrupt 印章显式报告且不修复」）经新套件 `r08_cli_data_vitest` 逐用例合同绑定（12 案例 12 绿，verbose 报告解析）。checkpoints/restore/diagnose 三叶 full_original_behavior。
- **D13 终端帧族传输面（3 叶保持 pending，负路径+传输闭合）**：Rust WS 识别 terminal_snapshot/tail/close 三帧（此前 unknown-variant→close 1008 断连）；close 回现役「引擎未接线」同款 rejected/terminal_unavailable，snapshot/tail 回显式 error 帧（优于旧栈未处理异常）；A 腿 snapshot 空 terminals/close rejected 真实形状对拍；双腿传输存活断言。正路径（真实 PTY 终端输出/终止 killed）需 Rust 终端会话子系统（PTY+转录日志，R09+ 桌面终端域）——D15-TERMINAL 登记为 recorded 基础，叶保持 pending 不降级。
- **MCP 会话权限真实差异发现**：Rust 需 targetId（tool:mcp:…:…）+capability（mcp.<tool>.invoke）精确匹配目录；旧栈仅 capability 字符串。语义都是「仅本会话放行不持久化」，形状差异在 A/B 案例中按各自契约断言。

### 正式门 verify-r08-closeout-04（2026-10-26）

- 命令 **20/21** PASS（唯一红 = a05 ENV-A05-FOCUS 持续，未重复执行无效重试；新增 r08_cli_data_vitest PASS）
- 场景 **15/16**（A05 连带）· 候选绑定 **stable=true** · 叶 **76 PASS / 50 BLOCKED / 0 FAIL** · overall=FAIL（唯一原因：50 pending + a05）
- A/B 演习 run-31..36：终态 **53 任务 = 49 parity + 4 recorded（slash.new/slash/prompt-snapshot/ws.terminal）+ 0 differs**。
- 回归：触及模块 cargo lib 30/30；cli-data vitest 20/20；typecheck 0 错；inputs digest match。

### 当前叶分布（closeout-04）

- PASS 76：D02 44 / D03 13 / D01 9 / D04 7 / D20 3
- pending 50：D02 22 / D01 14 / D03 6（OAuth×3/apps-launch/resources-read/UI 设置页）/ D04 5 / D13 3

### 剩余 50 叶续跑断点

| 域 | 叶数 | 所需 |
|---|---|---|
| D02 | 22 | todos×3（模型 todo_write 工具回合）、fork、连同子会话/解除谱系归档×2、延迟内容、sweep、rollback-preview、turns-retry、switch 浏览器恢复、工具×4（checkpoint/rewind/session/session-folders）、UI 地图页 |
| D01 | 14 | steer/interject/resume-stream/排队 abort/ask_user/todo_write/current_status/task-abort POST/快捷窗口桌面行为 |
| D03 | 6 | OAuth start/cancel/logout（需 OAuth 替身或 http 连接器+loopback 授权服务器）、apps launch、resources read、设置页 UI 矩阵 |
| D04 | 5 | confirm/reject×4（需待批动作=模型工具回合）、安全设置页 UI |
| D13 | 3 | Rust 终端会话子系统（PTY+转录）后正路径补证 |

## 续跑第四轮（2026-10-26，D02 会话管理无模型叶 + 补实现）

### 本轮闭合（+8 叶：76→84）

- **D02 会话管理 8 叶**（`leaf.sessions.admin` 19 案例双腿实跑，run-37..49 十二轮迭代收敛）：
  - **fork**（带谱系新会话+广播）：目标锚点统一走分支真值（Rust 新补 `turns/rollback-preview` 的 `turnInputEntryId`；旧栈 preview 带 role/entryId 目标）。深度过限（现役 MAX=2）链式复现拒。
  - **连同子会话归档 / 解除谱系后归档**：fork 链（base→P1→G1）真实谱系；cascade 主+子入归档、detach 子留活动顶层（Rust 归档回执含 detachedChildren=1 逐项披露）。
  - **rollback-preview**：**补 Rust 实现** `POST /lingxi/v1/sessions/{id}/turns/rollback-preview`（session_tree `preview_turn_rollback`：偏好关→enabled:false/file_rollback_disabled（现役同款）；开→检查点存在列受影响文件、无检查点→available:false/no_turn_checkpoint）+ 客户端翻译（此前该路径在 Rust 连接下被硬闸拒）。预览只读（不动文件、不发布事件）。
  - **sweep-orphaned**：真实目录删除→会话归档（Rust 经 PUT workspace 写 cwd 身份；旧栈 sessions/new cwd-at-create）。
  - **workspace-disposal delete**：matched=2/disposed=2、活动列表+归档均不可见（永久删除）。
  - **fresh-compact**：双内核 200 返回结果（noop 合法）。
  - **turns-retry**：真实重放最近用户消息（桩标记回合）→ A 腿 HTTP 内联回复文本；B 腿重置点+新头合同 + 服务日志 background drive `completed.with_final`。负路径：fileRollback=workspace 未开偏好→403/409 显式拒（偏好作用域内打，复原）。
- 迭代中定位并修复的真实缺陷/接缝：
  1. **Rust 删除助手不留墓碑**（agent_deletions.rs 只移目录入事务）→ `continue-deleted-agent` 的 `is_agent_deleted_checked` 永假。修复：提交删除后在原位补写 `.deleted-agent.json` 墓碑（现役同款约定）；`validate_deletion_materials` 相应视"只剩墓碑"为无 agent 数据。
  2. **D-16（真实产品缺口，未闭合）**：官方 `DELETE /lingxi/v1/agents` 流程后源会话行 `get_for NotFound`（分离焦点/独立会话/detached 均复现，run-45..48 服务日志）。continue 路由本身已由 r08-continue-disposal-compat（真实 Rust 子进程+直接墓碑）证明工作。叶 3818ADEAFA7A 保持 pending，D-16 登记 recorded 基础。
  3. 旧栈投影行双标识辨析：`id`=序号（非分支条目）、`entryId`=真分支 id（fork/retry 目标必须用后者——run-43 retryRowDbg 实证）。
  4. 旧栈续接前置：primary agent 指向夹具不存在的 "lingxi"→切主助手 a1；源会话需有可显示 transcript（先跑一轮标记回合）。
  5. 409 session_busy=引擎收尾窗口：有界退避重试（6×2.5s），与现役客户端语义一致。

### 正式门 verify-r08-closeout-05（2026-10-26）

- 命令 **21/21 PASS**（**a05 本轮转绿**——ENV-A05-FOCUS 环境回退自愈，未做任何宿主操作；键盘/AppleEvent 路径恢复）
- 场景 **16/16** · 候选绑定 **stable=true** · 叶 **84 PASS / 42 BLOCKED / 0 FAIL** · overall=FAIL（唯一原因：42 pending）
- A/B 演习 run-49 终态：**54 任务 = 49 parity + 5 recorded + 0 differs**；160 图钉案例 159 绿（唯一 bridge-redaction 旁证由门内 cargo 套件先行运行补齐，门内绿）。
- 回归：触及模块 cargo lib 34/34；typecheck 0 错；inputs digest match。

### 当前叶分布（closeout-05）

- PASS 84：D02 52 / D01 9 / D03 13 / D04 7 / D20 3
- pending 42：D02 14 / D01 14 / D03 6 / D04 5 / D13 3

### 剩余 42 叶续跑断点

| 域 | 叶数 | 所需 |
|---|---|---|
| D02 | 14 | todos×3、内容延迟正路径、switch 浏览器恢复、slash compact/fresh 命令族、工具×4（checkpoint/rewind/session/session-folders）、UI 地图页 |
| D01 | 14 | steer/interject/resume-stream/排队 abort/ask_user/todo_write/current_status/task-abort POST/快捷窗口桌面行为 |
| D03 | 6 | resources read（rmcp 资源面）、apps launch、OAuth×3（需授权服务器替身）、设置页 UI 矩阵 |
| D04 | 5 | confirm/reject×4（需待批动作=模型工具回合）、安全设置页 UI |
| D13 | 3 | Rust 终端会话子系统（PTY+转录）后正路径补证（传输面已闭合） |

## 续跑第五轮（2026-10-26，工具回合线束 + 双内核缺陷修复）

### 本轮闭合（+13 叶：84→97）

- **线束新增**：替身供应商 `R08-TOOL-PLAN <token> <base64url{steps}>` 多步工具计划（`scripts/rust-tauri/r08_t03_stub_provider.mjs`）。双内核对模型只常驻 read/write/edit/exec_command + mcp_search_tools/mcp_describe_tool/mcp_call（旧栈 `shared/tool-categories` RESIDENT_CORE_TOOL_NAMES；Rust `toolcatalog_bridge` RESIDENT_NAMES），一方工具一律走同一按需目录协议 `mcp_describe_tool → mcp_call`，线束逐步给出计划，替身逐步发出；终结回显 `R08-TOOL-FINAL <token> steps=<n> last=<末工具回包>`（截断 1200 字节于延迟内容阈值下）。内核自发续轮（延迟回退结果回送/Pi steer 插入）给显式 `R08-TOOL-CONT`。
- **六个新任务**（`r08-t07-kernel-ab.test.ts`，73 案例双腿全真，演习 60 任务 = 55 平价 + 5 recorded + 0 差异）：
  - `leaf.tool.todos`（22）：todo_write 回合 → 线上面板事件 + 真实 store 面板 + 历史三者一致；错误清单（blocked 缺理由 / 非法 status）显式拒且不覆盖；complete/cancel/dismiss 正路径（完成/取消语义、幂等不重复写、广播、收纳仅已结束、历史保留）+ 版本失配 409 + 无效目标拒。→ 叶 45C5CE/5CD35D/17FCFE/B34F2B。
  - `leaf.tool.checkpoint`（4）：create→list 可见、drop→list 不可见、snapshot:false 不写文件快照、drop 不存在不伪报。→ 6207F7。
  - `leaf.tool.rewind`（22）：checkpoint 锚点 → rewind 确认卡（旧栈 content_block.session_confirmation；Rust approvals 面）：拒绝不回退 + 如实「未回档」；HTTP confirm 路由 confirmed/rejected（无效 action 400、已处理 404、越权 401/403、广播 confirmation_resolved（Rust 以 app 事件 `confirmation-resolved` 映射））；斜杠 /confirm /reject（缺 ID 用法、已处理不再决策/不重复执行）；确认后分支回到锚点（双内核 retry 重置点语义：锚点用户轮及其后全部丢弃）；流式中已确认回档为 scheduled 挂到流结束；restoreFiles=false 外部改动保留。→ E1625F / EC0331 / EF43AD / 75C0AE / CD1524。
  - `leaf.tool.session`（9）：list/read 范围受控、未知会话拒、send 只产目标草稿卡（目标历史未被直写、拒自发、未知目标拒）、create 产草稿句柄、未知 agent 拒。→ 5DCB03。
  - `leaf.tool.current-status`（8）：list 键目录 → 逐键 get 全部可解（旧栈 10 键 / Rust 9 键，各自目录与 get 一致）；model/agent/time 与运行时一致；非 Bridge 会话 isBridgeSession=false；未知键/缺键显式错误。→ 386675。
  - `leaf.tool.session-folders`（7）：list 范围形状；operate 下 add 仅改当前会话（另一会话不变）；ask 下 add 需确认、拒绝后范围不变；remove 复原。→ 7C959E。
- **迭代中定位并修复的真实缺陷（按归属）**：
  1. **旧栈 todo 子系统对桥接结果失明**（lib/tools/todo-compat.ts、server/history-read/incremental.ts、desktop utils/todo-compat.ts、ws-message-handler.ts）：面板/三条收尾路由/增量读取/实时 tool_end 只按工具名（todo/todo_write）筛选，而 todo_write 非常驻、默认经 `mcp_call` 桥调用，tool_result 以桥名落档——默认路径下整条清单功能失明（dev-01 实证：旧栈 complete 回 removed 空面板、UI 面板不更新）。修复：`isTodoToolResult(toolName, details)` 按名或版本化载荷形状（`todoVersion` + todos 数组 / `invalid_todos` 拒绝）识别，Rust 目标 id 前缀同口径；单测 tests/todo-constants.test.ts（+5）、ws-todo-panel-lifecycle.test.ts（+4）。
  2. **旧栈 checkpoint/rewind 工具用焦点回调取会话**（lib/tools/checkpoint-tool.ts、rewind-tool.ts）：非焦点会话上的回合把存档点写进焦点会话（ab-real-alpha）的侧车、确认卡打到焦点会话流（dev-03 实证 `ab-real-alpha.jsonl.checkpoints.json`）。修复：按执行上下文 `getToolSessionPath(ctx)` 取会话（仓库 tool-session.ts 既定口径），无 ctx 回退焦点。
  3. **旧栈流式中已确认回档永远「session is busy」**（core/agent.ts rewind deps）：`getDeferredStore/getTaskRegistry` 经 `getEngine()?.getDeferredResultStore?.()` 取值，Engine 类无该方法 → 延迟路径死码（dev-04 实证）。修复：改走 agent 回调 `getDeferredResults/getTaskRegistry`。
  4. **Rust 用户收尾 todo 条目使后续运行崩**（model_history.rs）：todos 路由写入的 `todo-state-v1`（无源 Run）被模型上下文投影判 `interactive UI entry lacks a source Run` → 下一 run `background drive FAILED`（dev-01 Rust 日志）。修复：action ∈ {complete_all,cancel_remaining,dismiss} 的无 Run 条目按 UI 状态恒定排除（现役 lingxi.todo_state source:"user" 同口径）；分支改写检查同放行。
  5. **Rust 补实现**：斜杠 `/confirm(/approve)` `/reject(/deny)`（slash.rs，现役 bridge-commands 逐字文本）；`current_status`、`session_folders` 一方工具（status_tools.rs；此前仅有 HTTP 面，模型经目录调用落 unknown tool）+ 内核 `ArgumentAwareSessionFolders` 按 action 解析（list 只读/add·remove 写）；approvals answer 已处理改回原语义 404（原 200）；`current_status.model` 在 agent 未选定时回退服务已发布 chat 路由；confirm 后广播 `confirmation-resolved` app 事件。
  6. **D-16 翻案（线束缺陷）**：续接请求硬编码客户端 session id `r08-cont-s1`，Rust `new-detached` 返回服务端铸 id → `get_for NotFound` 是线束错 id 所致；独立探针（DELETE 助手 → 续接）201 正常。测试已改用返回的真 id 并补「助手存活时续接 400 拒」负例，待第六轮重跑后把叶 3818ADEAFA7A 绑定、撤 D-16 recorded。
- **记录的新差异（不改语义、不入 cases）**：session_folders add 不存在目录——旧栈工具路径只 path.resolve 不校验（HTTP 路由会校验，旧栈自身两面不一致）、Rust 工具与路由同口径拒绝；属 ADJ-3 同族（Rust 更严格/诚实），观测字段 `missingFolderAccepted` 如实入观测。

### 正式门 verify-r08-closeout-06（2026-10-26）

- 命令 **21/21 PASS** · 场景 **16/16** · 候选绑定 **stable=true** · 叶 **97 PASS / 29 BLOCKED / 0 FAIL** · overall=FAIL（唯一原因：29 pending）
- A/B 演习门内终态：**60 任务 = 55 parity + 5 recorded + 0 differs**；inputs digest match；xtask 随图重建。
- 回归：todo/工具族单测 207+137 用例通过；lingxi-service/lingxi-kernel 构建通过。

### 当前叶分布（closeout-06）

- PASS 97：D02 59 / D01 11 / D03 13 / D04 11 / D20 3
- pending 29：D01 12 / D02 7 / D03 6 / D04 1 / D13 3

### 剩余 29 叶续跑断点

| 域 | 叶数 | 所需 |
|---|---|---|
| D01 | 12 | WS 入站帧族 prompt/steer/interject/resume_stream/slash(ws)、/compact /fresh-compact /stop、task abort POST、ask_user 工具（5 断言）、聊天页 UI、快捷窗口桌面行为 |
| D02 | 7 | 续接已删 Agent（线束已修待跑）、cleanup、延迟内容正路径、latest-user-message 重放、switch 焦点+浏览器恢复、prompt-snapshot（真回合会话）、地图页 UI |
| D03 | 6 | resources read、apps launch、OAuth×3（授权服务器替身）、MCP 设置页 UI |
| D04 | 1 | 安全设置页 UI |
| D13 | 3 | Rust 终端会话子系统（PTY）真实正路径 |

## 续跑第六轮（2026-10-26 晚，DeepSeek Harness 接替 ZCode）

### 接管事实（已实查）

- 工作区 `/Users/study_superior/Desktop/Code/LingxiAgent`，分支 `codex/rust-tauri-migration`，
  HEAD `aa99d2ec1dae7f0aef5971bab049734f22fcf7d8`；工作树 66 项改动（含本会话接手前
  未封存的批次 6/7/8 扩展与 05/06 前序成果），**未提交、未推送**，保留原样继续。
- 正式门最新收据仍是 `verify-r08-closeout-06`（命令 21/21、场景 16/16、绑定 stable、
  叶 97 PASS / 29 BLOCKED，overall=FAIL）。`python3 scripts/rust-tauri/r08_stage_inputs.py --check`
  当前报 `inputs digest MISMATCH; graph bytes MISMATCH`：工作树生成器/演习 RULES 已含
  批次 6/7/8 任务，但阶段图仍是 closeout-06 产物 —— 属未封存中间态，收尾时统一重建。
- 旧栈修复（core/、lib/tools/、server/history-read/、desktop 相关）为 closeout 第五轮
  D-17..D-21 记录在案的 R08 缺陷修复；保留并记账，不视为扩域授权，不回滚。
- 在跑进程实查：仅遗留一个与本仓无关的 Vault-Agent 进程；无本仓遗留测试进程/端口。
- 工具链：Node v24.16.0；cargo 必须 `PATH="$HOME/.cargo/bin:$PATH"`（rustup 1.98.1），
  当前 shell 默认 Homebrew cargo 1.93 不可用于正式构建。
- 协作记录 `/Users/study_superior/.codex/coordination/lingxi-r08-r09.json` 只读核对：
  仍为 watching_r08 / R09 未启动；Codex 监控方维护，本会话只读。

### 暂停前最新开发演习（ab-dev-13）讲评

`/private/tmp/lingxi-r08-verify/ab-dev-13.log`，观测
`/private/tmp/lingxi-r08-verify/r08_t07_ab-dev-13/out/{ab-node.json,ab-rust.json,ab-compare.json}`：
`tasks=68 parity=63 differs=2 recorded=3`，EXIT=0 只是线束不退出非 0（差异是数据）。

两个 FAIL 经双方观测+内核日志+源码核实，**均为线束误用，不是产品缺陷**：

1. `leaf.lazy-content`（B 腿 final=false、histLen=1014）：Rust 服务配置无 `workspace`，
   file/exec 工具未注册（`lib.rs` 仅 `Some((access, workspace))` 时注册四工具），B 腿模型
   声明工具表无 `exec_command`（stub journal seq 568 的 `request.tools` 实证），替身按计划
   直调 `exec_command` → `openai_completions.rs` "provider requested unknown tool" →
   run `failed.provider_error`。A 腿机制（旧栈 deferred locator）本身为真。
2. `leaf.mcp.face`（A 腿 resources 200 期望 → 实 400；B 腿 start 404）：
   - 旧栈 `core/mcp/manager.ts:readResource` 明确要求 `ui://` 资源 URI（`r08://` 必 400）；
     Rust `mcp_apps.rs` 同约束。测试请求了非 `ui://` URI → 误用。
   - Rust 无 `/mcp/connectors/{id}/start` REST 路由（只有 `mcp/settings-action`，crud 任务
     已用对），测试误调 REST start → 404 → 应用目录不可用 → launch 409。

结论：按原断言修**线束真实机制**，不放宽断言；Rust 侧如确缺「历史引用的大内容按
contentId 取回」的可达数据源，按产品缺口补最小实现（见下）。

### 本会话任务分解（126 项目标）

- 已 PASS 待绑叶（dev-13 双腿全真，需逐断言核对后入 FULL_BINDINGS）：`leaf.ws.stream`
  （覆盖 D01 8 叶）、`leaf.cleanup.expiry`（B0F05B）、`leaf.replay-latest`（4DD3F7）、
  `leaf.switch.focus`（FBDEED）、`leaf.prompt-snapshot.real`（FF4F06）、`leaf.mcp.oauth`
  （2DA782/E7F852/C70E81）以及 D-16 翻案后的 `leaf.sessions.admin` 续接案例（3818ADF）。
- 需修线束后再绑：`leaf.lazy-content`（ED0891）、`leaf.mcp.face`（04A6A2 + 7FF8D4）。
- 需实现/补证：D13 真实终端 3 叶（PTY+转录+WS 三帧正路径）、D01 ask_user 5 断言、
  D01 task abort POST 2 断言、UI 5 叶（聊天页 6 / 地图页 7 / MCP 设置页 7 / 安全设置页 4 /
  快捷窗口 2）。
- 收尾：阶段图重建 + `--check` 一致、正式门 `verify-r08-closeout-07` 全绿、独立上下文
  验收、R09 准入结论、文档数字统一（R08_REPORT 末段旧数字）。

### 下一步（即时顺序）

1. 演习线束修正（B 腿 `workspace` + 多套件支持、两条 FAIL 按真实机制修复）。
2. 并行：终端子系统、UI 页证据、ask_user/task-abort 子代理实施（写入范围互不重叠）。
3. 逐叶 FULL_BINDINGS + 生成器/阶段图/清单重建；正式门跑 07；独立复核后交接。

### 第六轮进展（2026-10-26 19:00-19:20）

- 演习线束升级（`r08_t07_ab_drill.mjs`）：`--suite-files` 多套件合并观测（重复任务键响亮失败）、
  `LINGXI_R08_AB_TEST_FILTER` 单任务红绿迭代、B 腿 `--config workspace` 注册 file/exec 四工具
  （此前 B 腿无 workspace，模型声明面缺 exec_command，延迟大内容/终端类叶没有真实机制）、
  未登记任务自动用通用 casesParity 判定。
- **leaf.lazy-content 红→绿**：根因 = 线束误用（B 腿直调未注册的 exec_command → provider_error），
  另发现真实可达性缺口：Rust 大内容以工具产物资源（`exec-output:<file>`）落史，而
  `/sessions/{id}/content/{contentId}` 只解析 `res_` 会话文件资源 → 404。修复：
  `history_content.rs` 增加「历史引用 + data_home 内本地产物文件」回退解析（8MiB 上限、canonical
  路径必须落在 data_home 内，locator 不是授权凭证的契约不变）。双腿全绿。
- **leaf.mcp.face 红→绿**：根因 = 两处线束误用（两内核 resources 读取都只接受 `ui://` 资源；
  Rust 无 REST `/mcp/connectors/{id}/start`，启动走 `mcp/settings-action`）。按真实机制修测试，
  未放宽断言；双腿全绿。
- 全量主 drill（`/private/tmp/lingxi-r08-verify/r08_t07_ab-lead-full-01`）：
  **68 任务 = 65 parity + 3 recorded（ADJ-4/5 + 终端传输面）+ 0 differs**；
  已把批次 6/7/8 + D-16 翻案覆盖的 20 叶写入 FULL_BINDINGS（预览生成器：116/126 已绑）。
- 红→绿证据归档：`artifacts/rust-tauri/R08/closeout-proofs-02/lead/`（dev-13 红、lead-04 绿、
  全量观测与日志）。
- 并行实施中（子代理，写入范围互斥）：D13 真实终端子系统（Rust terminals.rs + WS 三帧 + A/B 证据）、
  UI 5 叶客户端证据（聊天/地图/MCP 设置/安全设置/快捷窗口）、ask_user + task abort POST 双内核证明。

### 第六轮进展（2026-10-26 19:20-19:45）

- **D13 终端子系统完成（子代理交付，Lead 已核）**：Rust 新增 `terminals.rs`（身份链
  `session_locator::resolve → sessions.get_for → record owner` + 三帧 + close 收口）；
  `procsupervisor.rs` 新增只读 `TerminalLog`/终端视图/tail 查询（与 write_stdin 交付游标独立）；
  `lib.rs` WS 会话循环接线，删除旧三帧硬回显。独立套件 `r08-t07-terminal-ab.test.ts`：
  真实 PTY（exec_command tty=true）+ write_stdin + 心跳文件证 close 真实终止，双腿 14/14
  case 全 true、`tasks=1 parity=1`（dev-01、dev-02 两次）。证据：
  `artifacts/rust-tauri/R08/closeout-proofs-02/terminal/`。回归：terminals 5/5、
  procsupervisor 23/23、ws 13/13、exectools 9/9；全量 lib 706 pass/3 fail 均为既有环境前置。
  三叶 CE6906/4C35E6/CFD9F0 已按 14 case 写入 FULL_BINDINGS（快照/尾读/关闭各含正负路径）。
- 旧任务 `leaf.ws.terminal`（仅帧族传输面历史回归）verdict 仍 recorded；其 Rust tail 显式
  回复匹配面已按新子系统真实帧（terminal_not_found_or_mismatch error）收敛，传输不断连判定不变。
- D-16 recorded 登记已撤（续接案例双腿真、叶 3818ADEAFA7A 已绑定）；`leaf.sessions.admin`
  不再需要 recorded 批准。
- 预览生成器：仅剩 UI 5 叶 + ask_user/task-abort 2 叶未绑（子代理实施中）。

### 第六轮进展（2026-10-26 19:45-19:55）：全部 126 叶绑定 + 合并预验

- **D01 交互两叶完成（子代理交付，Lead 已核）**：`leaf.tool.ask-user` 29 case / 5 断言、
  `leaf.chat.task-abort` 9 case / 2 断言，双腿全 true（`tasks=2 parity=2`，dev-03）；
  证据 `artifacts/rust-tauri/R08/closeout-proofs-02/interactive/`。AB 差异如实记录：
  二次 abort 旧栈 404 / Rust 200 already_stopped、dismiss 动作词 rejected+value / dismissed+values、
  卡片面旧栈 WS content_block / Rust interactions 轮询面（各自真实 UI 绑定层）。
  产品代码零改动（两侧实现本就真实存在）。A4「ConfirmStore 不可用」「工具级无会话」无公开
  活体开关，由两内核既有专属测试覆盖（tests/ask-user-tool.test.ts、
  rust/crates/lingxi-service/tests/r06_interactive_completion.rs），已在证据 README/进度账披露。
- **UI 5 叶完成（子代理两轮交付，Lead 已核）**：`r08_ui_pages` 生产者 **87 cases / 87 ok**
  （chat 25 / map 19 / mcp-settings 21 / security 14 / quick-chat 8）；map 与 quick-chat 为
  真 Electron live（19/9 checks PASS），chat 为组件级真实投影+既有专属单测（仓库无 chat-live
  runner，如实降级披露）；补洞轮关闭 8 个未覆盖子句（批量 HTTP200 混合部分成功、权限保存失败
  不伪造授权、loadSettingsConfig 守卫、通知失败不冒充完成、WS 错误页面呈现、恢复流控件态）。
  证据 `artifacts/rust-tauri/R08/closeout-proofs-02/ui-pages/`（含覆盖矩阵与未覆盖/差异清单）。
- **生成器：126/126 叶 full_original_behavior，0 pending**；`--check` digest match。
  新增命令 `r08_ui_pages`（A13 场景内执行）；A/B 演习 `--suite-files`=主+终端+交互三套件。
- **合并全量预验**（`/private/tmp/lingxi-r08-verify/r08_t07_ab-merged-01`）：
  **71 任务 = 68 parity + 3 recorded + 0 differs**；按生成器 derive 核对，118 个 AB 绑定叶的
  全部图钉 case 均 1（唯一 cargo 旁证由门内套件提供）。
- 旧任务 leaf.ws.terminal 仍 recorded（判定面不变）。正式门执行前要求独占环境（C-CONCURRENCY）。

### 正式门 verify-r08-closeout-07 启动前状态（2026-10-26 19:55 冻结）

- 121? 不：**126/126 叶 full_original_behavior、0 pending**；阶段图 22 命令 / 16 场景，`--check` digest match。
- 绑定生产者：r08_t07_ab 118 叶（主套件+终端/交互扩展套件合并观测）、r08_ui_pages 5 叶、r08_cli_data_vitest 3 叶（D20）。
- 合并预验：71 任务 68 parity + 3 recorded + 0 differs；UI 生产者 gate 等价入口实跑 87/87 ok。
- 回归：typecheck（root/node/test 三配置）0 错；history_content 内容回退安全边界单测 6/6 绿。
- 冻结约束：正式门执行期间无候选写入、无并发演习（C-CONCURRENCY）。

### 正式门 verify-r08-closeout-07（2026-10-26 20:00，真实红，保留）

- 命令 **21/22 PASS**（唯一红 r08_a15_isolation）、场景 **14/16**（A15/A16 连带）、
  叶 **126/126 PASS**、候选绑定 stable=true、in-gate A/B 71 任务 68 parity+3 recorded+0 differs。
- 唯一红根因：A15 隔离检查 V4 发现 **r08_ui_pages 套件回执 argv 含仓库内证据绝对路径**
  （生产者 --out 直接指向 {EVIDENCE}/SUITES/r08_ui_pages）。判定正确（argv 不得指向真实
  home/仓库工作树）——不是放宽门禁，而是改生产者调用方式。

### 正式门 verify-r08-closeout-08（2026-10-26 20:10，最终）

- 修复：`r08_ui_pages` 生产者输出先落 `/private/tmp` 合成 scratch（argv 仅合成根），跑完把
  `ui-cases.json`/raw/live 拷入证据目录；阶段图/生成器 `--check` digest match。
- 结果：**overall=PASS** · 命令 **22/22** · 场景 **16/16** · 叶 **126 PASS / 0 BLOCKED / 0 FAIL**
  · 候选绑定 **stable=true**（before/after digest 一致、逐命令 checkpoints 全 stable）
  · runnerSourceBinding 一致 · A15 verdict=ISOLATED violations=0 · A16 head=aa99d2ec rust=b855e5d6
  · in-gate A/B：**71 任务 = 68 parity + 3 recorded（ADJ-4/5 + 终端传输面旧任务）+ 0 differs**。
- 域分布：D01 23 / D02 66 / D03 19 / D04 12 / D13 3 / D20 3 = 126。
- 收据：`artifacts/rust-tauri/R08/verify-r08-closeout-08/verify-stage-result.json`；
  07 红证据同目录保留（`verify-r08-closeout-07/`）。

### 独立上下文验收（2026-10-26 20:14-20:30，独立审查者，实施/独审分离）

- 结论：**ADMIT_R09**。报告与可复算证据：
  `artifacts/rust-tauri/R08/closeout-proofs-02/independent-review/`
  （INDEPENDENT_REVIEW_R08_CLOSEOUT.md/.json + tooling/ 3 个不导入生成器的只读脚本 + results/）。
- 独立反证成立项：gate-08 22/22 命令、16/16 场景、126 PASS/0 BLOCKED/0 FAIL、binding stable、
  before/after manifest 字节相同、证据 mtime 全在门窗口内；**126 叶逐断言复核（281 原断言 /
  487 断言→case 实例 / 449 不同 case，叶内 0 复用，每 case 原始证据 ok=1，findings=[]）**；
  A/B 原始观测反算 349/350 一致（余 1 为 cargo 单证）、71=68 parity+3 已登记 recorded+0 differs；
  独立复跑 `r08_ui_pages` exit 0（87/87 与门内 diff=0）与 terminal 单任务 A/B parity=1（14/14）；
  无门禁放水（RULES 只增不减、stage map 18→22 命令、pending 106→0、xtask 源码未改、07 真红保留）；
  隔离（a15 violations=0、真实 ~/.lingxi before/after 相同）；门后仅收尾文档变化、源码/脚本/测试与
  候选 manifest 逐文件相同。
- 保留记录项（不阻断门级判定，已按审查建议落账并转 R09）：
  F1 slash 族「接管态/无历史」子句未直接取证（/new 485B2D0A8A02 接管态与无历史分支无测试；
  /reset、/fresh-compact 接管态由既有 rust/r06_slash_completion.rs 覆盖但不在 R08 门内命令；
  /stop 仅非接管态 other-session 案例）→ 本文件与 R08_HANDOFF 记录，R09 补 bridge-takeover A/B 场景；
  F2 checklist 缺 ADJ 注记 → 已在本轮给 CLOSEOUT_CHECKLIST.json 注入 evidenceRequired
  （3 条 ADJ 订正 + 逐叶要求说明）；
  F3 RECORDED_APPROVALS['leaf.prompt-snapshot'] 文本过时（叶已由 leaf.prompt-snapshot.real 绑定）→
  属脚本内注释性登记，改它会改变受测候选源码，故仅记录为 R09 文本修订项；
  F4 UI 两处负向子句按现役 alert+retry 验收（矩阵已披露）→ R09 按 ADJ-1 机制对账 R00 文本；
  F5 A13 合同自同一 RULES 派生 + subsetMode 的理论缩容风险（本轮 RULES 只增不减、68+3 全观测未触发）→ R09 评估；
  F6 无法证边界（未逐子句复读全部 281 断言、未重跑正式门、ADJ 裁定原文仅文档可核）如实保留。

### 最终回归与工作区状态（2026-10-26 20:35）

- `npm run typecheck`（root + node + test 三配置）→ 0 错误。
- `python3 -B scripts/rust-tauri/r08_stage_inputs.py --check` → inputs digest match（整图字节再生一致）。
- 工作区：HEAD `aa99d2ec1` 未变、未提交/未推送；`git status` 79 项（含本轮全部源码/测试/证据/文档与
  前序成果）。受测候选 = HEAD + 未提交工作树（门回执 worktreeDirty=true）；本文件与其它收尾文档
  在门后写作，与受测候选分离（独立审查已逐文件确认门后仅文档变化）。

### 提交与推送记录（2026-10-26 20:45，业主「直接提交并推送」指令）

- 工作树拆两提交并已推送到 `origin/codex/rust-tauri-migration`（推送范围
  `aa99d2ec1..307877337`）：
  1. `ecacab152` `feat(r08): complete closeout — 126/126 leaf evidence, real terminal WS subsystem`
     —— 69 文件（源码/测试/脚本/文档），+26075/−2217。
  2. `307877337` `chore(r08): file closeout gate receipts + round-6 evidence`
     —— 827 文件（closeout-01..08 收据/合同案例/日志 + round-6 红绿与独立审查证据），+314187。
- 未入库但保留在磁盘（遵循既有收据提交先例与 .gitignore）：
  - `verify-r08-closeout-*/candidate-source-*.json` 机器快照（.gitignore 第 185 行已排除；
    候选绑定摘要与 before/after digest 在 verify-stage-result.json 内）；
  - `verify-r08-closeout-07/08/SUITES/r08_ui_pages/{live,raw}` 大型 Electron/Vite 运行缓存
    （约 250MB；live 回执与截图已在 closeout-proofs-02/ui-pages/live 归档，raw vitest JSON
    在 ui-pages/raw 归档）。
- **审计封印坐标未推进**：`.sync-audit/verified-source-sha.txt` 仍为 `ab4f2281…`；本次提交后
  `tests/post-verification-audit-seal.test.ts` 会把 R08 变更视为非白名单差异（预期红）。按
  AGENTS.md/PROGRESS.md 纪律，坐标推进/封印是**另行授权**的流程；本会话未修改审计白名单、
  未退役门禁、未改验证语义。R08 验证证据绑定的是被推提交的同一工作树（门回执
  worktreeDirty=true，候选绑定 stable）。
- 推送后复核：本地 HEAD == `origin/codex/rust-tauri-migration` == `307877337`；工作区仅剩上述
  未入库缓存/快照（未删除，唯一副本保留）。未推送 main、未发布、未合并。
