# R07-T07 工作台、文件历史与工作流能力 — 设计与断点状态

状态：IN_PROGRESS（C全11中10过/adapter101；D针对7项6过/adapter101。自有服务1080/页面495已验证，浏览器失败交T12复查，完整门禁未通过；T08现已IMPLEMENTING）

> 接续结论：T05 t05c、T06 t06b 的归档/逐文件摘要/门禁日志已复核吻合；这些历史候选不等于当前工作区已通过。T07 原有 w1/d1/d2/g1 四份开发回执可信，后续 δ 与本轮修补仍须统一验收。以下早期“缺/半成”文字是盘点起点，最终状态以本段及后续回执为准。核算详情见 RESUME_AUDIT_2026-10-18.md。

### 接续实现与验证（未作整项通过登记）

- 页面实际使用方：补齐工作台/Git/便笺/日记/资源事件/循环/工作流路径；欢迎页可读批准目录，挂载读写不再被旧会话文件接口阻断；重连和连接切换释放旧订阅，拒绝迟到数据覆盖。前端分批16套374项和三个类型检查通过；后续循环保存错误展示仍须并入最终候选。
- 活动与日志：过期活动先取消真实任务、确认真实终态再显示 timeout；中断仍标未知并说明 interrupted。工作台日志限量脱敏、反复读取不造重复；日记接正式活动材料。
- A13：新 local_fs 挂载明确声明 watch；Rust UI 使用挂载引用，停用与摘除监听/订阅、事件发布共用同一闸，响应后不再泄露旧事件；退出立即清理且拒绝新增监听。旧挂载无 watch 不自动扩权限，需重新添加后才能启用。无 native root 的远端/虚拟挂载不宣称监听生效。
- A14：旧 experiments 字典、未知兼容字段、单键关闭、展示快照只读及重启均纳入真实服务检查；T08 的实际导入/导出路径仍需追加验证，不提前拿快照展示充当导入已通过。
- 工作流：新增真实后台登记、统一任务查询/停止、会话清理和服务退出；同步及 Promise 死循环在执行限制内收场并回收线程；节点写目录实际传入文件权限及系统沙箱，禁止交互终端绕过；工具筛选和逐次撤权复核。持久活动归属明确，恢复日志仅同会话同助手可读；日志保存失败不承诺可恢复。完成清理后主动唤醒空闲循环，取消不自动续跑。
- 目标/循环存储：保存失败不报成功、不发送未持久确认的超支提醒；坏侧车原件保留。循环计数/清槽/顺延失败阻止投递，停止覆盖闹钟投递与退避全过程；同键替换不丢取消句柄；超大延时有界分片。循环本机阻断须明确 resume 才解除，不假称磁盘已经 paused；全部存储不可写时无法承诺故障标记跨崩溃持久化。
- 本轮失败全部保留：resume-delta / integrated1 / integrated2 编译失败；integrated3 的凭证恢复、循环观察时序、工作流夹具及真实 Promise 挂起失败；UI r1 与 types r1。修复或重跑结果另建回执，不改写原退出结果。凭证恢复单跑原二进制通过；随后已定位根因：looptools测试夹具的清理路径多取一层，实际指向共享TMPDIR，导致并行auth/bridge夹具文件消失。已改为只清自有目录并断言清理边界，新增兄弟哨兵存活检查，最终候选须整体复验。

基线：T06 已收口（LOCAL_VERIFIED，候选 t06b，指纹 190b0c25…b49bb）。

## 范围与叶清单（65 未闭叶 + A13/A14；另 22 叶已 PASS_LOCAL_T03 不重领）

### 家族一：git 面板（22 叶，Rust 全绿field）
现役面：`server/routes/git-environment.ts`（22 路由，全挂 `/api/git/*`）+ `server/git/git-command.ts`。
- GET status / worktree-info / worktrees / branches / log / file-diff；
  POST log-stats / checkout / create-branch / stash / unstash / discard / worktree-create /
  commit / push / pull / stage / unstage / amend / fetch / ai-commit-message。
- 授权两层：HTTP scope（GET+log-stats=files.read，其余 POST=files.write，route-security.ts:84-89）+
  路由内 `isApprovedDir`（desk 根 + 会话工作区 + cwd_history + studio 挂载根，realpath 前缀判定）；
  GET 系拒绝=403 `dir not allowed`，POST 系拒绝=400 `invalid dir`。
- 执行：`execFile("git", argv)`（shell:false，无注入面），GIT_OPTIONAL_LOCKS=0/GIT_TERMINAL_PROMPT=0，
  15s 默认超时（push/fetch/pull/worktree-add/log-stats 120s），maxBuffer 16MB。
- ai-commit-message：经 utility `summarize` 槽（hub.eventBus `utility:call-text`，45s 超时）；
  只读 status+diff HEAD -U3+untracked 头部，**绝不 commit**；503 llm 不可用。
- 客户端：desktop/src/react/utils/git-env-api.ts（lingxiFetch 逐路由封装）。
- 现役行为预言机测试：tests/git-command.test.ts、tests/git-environment-route.test.ts。
- Rust 现状：无任何 git 路由/模块（全绿field）。

### 家族二：工作台文件树/挂载（~25 叶，Rust 半成）
现役面：`server/routes/studio-workspaces.ts`（309 行）+ `server/routes/mobile-workbench.ts`（323 行，
`/api/workbench/*` 与 `/api/mobile/workbench/*` 同处理器）+ `core/mount-aware-file-service.ts`（672 行）
+ `core/studio-mounts.ts`（219 行注册表）。
- 挂载注册表 `~/.lingxi/studio-mounts.json`：schemaVersion 1；upsert/disable/listForStudio；
  mountId=`local_fs_<sha256(studioId\0rootPath)[:16]>`；capabilities 序固定
  ["list","read","write","watch","materialize","execute"] 过滤序；写盘 JSON 2 空格缩进 + 尾换行 + 原子写；
  保留 createdAt/updatedAt（upsert 时 createdAt 沿用旧值）。
- GET /api/studio/workspaces：default 工作区合成（resolveRoot("default")，no_workspace 时省略）+
  active 挂载；nativeRootPath 仅 local owner（isLocalOwnerPrincipal）。
- POST /api/studio/workspaces：files.write + local owner 403 `local_owner_required`；
  path 必须绝对且存在目录；win32 大小写折叠去重（非 win32 不做）；label 缺省=basename||"Workspace"。
- DELETE /api/studio/workspaces/:mountId：files.write + local owner；`default`→400 default_workspace；
  非 active→404 workspace_not_found；非 local_fs→400 unsupported_workspace_mount；仅翻 status=disabled。
- workbench：files（listFiles）、search（q 空→空结果；80 上限 name 模式）、content GET/HEAD
  （serveFileContent，404 file_not_found）、actions（mkdir/create|writeText/rename/move/movePaths/
  safeDelete；unknown→400 unknown_action；版本冲突 HTTP200 {ok:false,conflict:true,version}）、
  upload（files[].contentBase64，单文件 25MB 解码上限→413 file_too_large，逐项 results[]）。
- 写动作配写租约台账（execution-lease-service，5min，local owner no-op）+ 安全审计事件
  （mobile_workbench.<action>，ok:false→failed）。
- 名称/子目录规范：normalizePlainNameOrThrow（拒空/斜杠/"."/".."，点开头合法）；
  normalizeSubdirOrThrow（拒 \ 与 .. 段，点开头目录合法）；resolveInsideRoot realpath 收容
  （含缺失路径向上走查 resolveMissingPath）。
- listItemsForWorkbench：{name,isDir,size(目录null),mtime ISO}，目录优先 + localeCompare zh 排序。
- Rust 现状：resource_mounts.rs 只读 MountProvider（校验与现役同表但无 createdAt/updatedAt 字段、无写API）；
  resourceio.rs 有 stat/read/list/search/write/write_expected_version/rename/move/trash/watch/subscribe
  （**缺 mkdir/copy**）；workbench/actions 仅 writeText|create（resourceio.rs:3901，仅 workspace_root）；
  无 studio/workspaces 路由、无 workbench files/search/content/upload 路由。
- 默认根：Rust 侧 `state.resource_io().workspace_root()`（现役 defaultDeskCwd||homeCwd||deskCwd 等价）。
- studioId：`state.management.identity_ids().1`（lib.rs:4280 同径）。

### 家族三：desk 文件/便笺 + 日记（~7 叶）
现役面：`server/routes/desk.ts:1425-1684`（files GET/POST、search-files、jian GET/POST）
+ `server/routes/diary.ts`（63 行）+ `lib/diary/diary-writer.ts`。
- desk files GET：`dir` 须过 isApprovedDir（同 git 族），subdir 拒 `\`/`..`；POST 动作开关
  upload（local owner + 绝对源路径 + 敏感路径拒绝）/create(mustNotExist)/mkdir/rename/move{names[],destFolder}/
  movePaths(remove 走 safeDelete)；{ok,files,trashId?}，批量逐项 results[]。
- jian：GET {content|null}；POST content 空白→safeDeleteIfExists 返 {ok,content:null}，否则写回 {ok,content}；
  心跳广播 jian_update（WS；Rust 面走 app-events/WS 桥）。
- diary write：logical day **04:00 边界**（lib/time-utils.ts）；目录 OH-Works/{日记|Diary}（locale）；
  会话摘要收集（缺失/过期→临时 compaction，warnings 收集不致命）→PII 擦除→memory 槽 callText
  （temp 0.7，120s）→剥 <mood|pulse|reflect>→补 `# ` 标题→文件名 `{logicalDate} { sanitizedTitle≤60 }.md`；
  零材料/失败→400 {error,warnings?}。diary list：目录不存在→{files:[]}，`.md` 文件名倒序。
- Rust 现状：无 diary 模块/路由；无 desk files/jian 路由（desk 现仅 skills/activities/cron/heartbeat/beautify）。

### 家族四：goal/loop/workflow 深运行时（~7 叶，Rust 全缺）
- goal：`lib/goal/goal-engine.ts` —— 按 sessionPath 副车 `${sessionPath}.goal.json`；状态
  active⇄paused→completed/dropped；token 累计超预算恰好通知一次；turn_start 惰性时间预算核查
  （暂停时间不计）；abort 暂停；创建须至少一项预算；原子写。工具 lib/tools/goal-tool.ts
  （create|status|pause|resume|complete|drop；capability goal.status/goal.<action>）。
  goal 偏好 GET/PUT 已有（userprefs.rs，等价 preferences.ts:410-433）。
- loop：`lib/loop/loop-controller.ts` + loop-store.ts（`.ephemeral/loop-state.json`，
  LOOP_DEFAULT_LIMITS{maxTurns:50,maxConsecutiveFailures:3,minDelaySec:60,guardedMinDelaySec:1200,
  fallbackDelaySec:1200}，腐败改名 .corrupt-<ts>）+ alarm-service.ts（钳 minDelaySec；
  后台活跃时短闹钟拒 short_alarm_with_live_work；投递重试 [5s,25s,125s]，忙 defer 60s×30；
  过期闹钟重水合 jitter 1-5s）。slash /loop（bridge-commands.ts:222-284，owner+session 域）；
  模型工具 loop-control（schedule|complete，仅 loop 运行/暂停中可用）；loop_status WS 广播；
  recoverAtBoot（running 失 target→stopped target_unavailable + 兜底闹钟）。
- workflow：`lib/tools/workflow-tool.ts` —— runId==taskId（mintTaskId("workflow")）；deferred 结果 +
  ActivityHub 条目（workflow|workflow_agent|workflow_step）+ workflow_progress 事件；
  journal 复用未变节点（cache key sha256(prompt+opts)[:16]/nodeSeq，首个不匹配起全部重跑）；
  限制：节点 15min、空闲 10min watchdog、总 4h、并发 16、重试 2；失败=abort-first；
  无 deferred/会话时同步回退 _syncRun；持久化 workflow-activity.json（72h prune）。
- Rust 现状：goal 仅偏好路由；loop/workflow 全缺（名字只在 subagent blocklist/permission 分类出现）。

### 家族五：活动/日志（~4 叶，Rust 半成）
- automation_activity.rs 已有 list/{id}/session/{id}/promote（LocalOnly）；**缺 reconcileOverdueRunning**
  （list 前把超 executionTimeoutMs=20min 的 running 项打 error:"timeout" + 广播 activity_update，
  activity-store.ts:49-56,142-155；构造时孤儿 running→error:"interrupted"）。
- 缺 GET /api/desk/logs（engine.getDevLogs 内存环，STUDIO_OWNER 兜底→LocalOnly）。
- 叶 01631175277C（过期运行项先标记超时）、23824E784D71（日志列表）、F6F578E62D72（活动消息）、
  F6CEE2A4A3CE（promote）等。

### A13｜文件监听撤权及时停止
- 现役 TS **没有**服务端撤权→停监链（registry 仅 release/subscription 删除释放；
  DELETE /studio/workspaces 不触碰 watch registry）——Rust 候选已实现更强语义：
  resourceio.rs poll_watches_once 对挂载 watch 每次 checked_mount_snapshot 复核，失败即
  revoke_watch→retire_watch_locked（摘 watch、全部 watch_ids、引用订阅，记 lastErrorCode）。
- T07 须交付：挂载 disable 路由与 watch 吊销的**同路径接通**（disable→立即 retire 相关 watch）
  + A13 验收测试（撤权后改文件不泄露事件、watch 计数归零）。
- 另叶 CECACC73463E：无 native root 时不得宣称 watch 生效（现役 WorkspaceFileChangeBridge 语义：
  远端/虚拟挂载不订阅）。

### A14｜实验开关不被迁移重置
- Rust 实验面已在 agent_config_readback.rs:3455-3680（4 定义默认值与 TS 一致，退役
  cache_snapshot_reflection 钉 "off"，instant_simple legacy lossy_local 回退，PATCH 只写单键）。
- 风险面：settings_ui.rs:320-327 快照把 preferences["experiments"] 替换为解析后**数组**——
  任何把快照载荷原样回写的导入路径会毁 map。A14 验收：导入配置并启动后关闭仍关闭、
  默认值与旧版一致（对照 preferences.json experiments 键前后）。

## 切片计划
- β1：挂载注册表写 API + studio workspaces 3 路由 + 挂载文件列表 + auth 注册
- β2：io.mkdir/copy + workbench files/search/content/actions 全动作/upload
- β3：desk files/search-files/jian
- β4：diary write/list（memory 槽模型链）
- γ：git 22 路由（共享 approved_dirs 判定）
- δ：goal 引擎 + loop 控制器 + workflow 工具
- ε：活动超时 reconcile + desk logs + A13/A14 验收测试
- ζ：候选冻结 + 门禁 + 登记

## δ 设计定稿（goal/loop/workflow；盘点 2026-10-17，6 叶）

叶：goal 工具 E09C077F0100、loop_control 工具 1DFC5E1E130A、workflow 工具 C1BA7A7C2737、
slash /loop 125BFE1EE56C、preferences goal GET 1F41AFB4207D / PUT 2CB7D4AE4478（后两者路由
已在 userprefs.rs，δ 补行为测试后随 ζ 登记）。现役源：lib/goal/goal-engine.ts(246)+
lib/tools/goal-tool.ts(113)+lib/loop/{loop-controller(397),loop-store(126),alarm-service(167),
loop-messages}.ts+lib/tools/loop-control-tool.ts(76)+lib/tools/workflow-tool.ts(501)+
lib/workflow/{sandbox(85),meta(87),host-api(402),concurrency(41),journal(119),run-limits(105),
node-folder-scope(155),structured-output(34)}.ts+core/slash-commands/bridge-commands.ts:222-284+
core/engine.ts:1420-1516（loop 装配）。

### δ1 goal 引擎（goal.rs，新模块）
- GoalState JSON 与现役逐字段同形（camelCase：schemaVersion/name/tokenBudget/timeBudgetMs/
  tokensUsed/startedAt/pausedMs/currentPausedAt/status/pausedReason/completedAt/
  overBudgetNotified/lastActiveAt；2 空格缩进；tmp+rename 原子写）。
- 侧车位置登记偏差：现役 `${sessionPath}.goal.json`；Rust 无 sessionPath（会话是 DB 行，
  session_locator.rs），侧车落 `data_home/goal-sidecars/<session_id>.goal.json`，JSON 内容
  平价。孤儿侧车不清理（现役语义）。
- 状态机逐函数平移：create（active 排他 + 至少一项预算）/pause/resume（pausedMs 累计）/
  complete/drop/status/recordTokenUsage（active 才记，四舍五入非负；超预算恰好一次
  overBudgetNotified + notifyOverBudget 回调）/tickTimeBudget（检查点懒查）/pauseOnAbort。
  describeGoal 文本逐行同构（goal "name" — status / tokens / active time fmtMs hm /
  paused reason / OVER BUDGET 行）。now 注入可测。
- goalProgress：pct=Math.round(used/budget*100)（budget 0/None→None）；over 严格大于。

### δ2 goal 钩子接线（runs.rs/sessions.rs，最小侵入）
- token 进账钩：persist_model_call_completed（runs.rs:5207）在 record_model_call_usage
  落盘成功后调用 GoalEngine::record_token_usage(session_id, total)；total =
  usage 各 Some 分量（input/output/cache_read/cache_write/reasoning）求和，全 None 不记
  （R05「provider 未报不虚构」口径沿用）。persist_model_call_cancelled_in_flight 同钩
  （落盘才记账=现役「中止且未落盘不记账」）。钩子对引擎错误只记 tracing，不吞运行账本。
- turn 检查点：每轮迭代顶（runs.rs:2169 turns 循环）tick_time_budget(session_id)。
- 用户中止：sessions.rs:2030 cancel_run(run_id,"user") 成功后 pause_on_abort(session_id)。
- notifyOverBudget 回调（恰好一次）：模型可见面 = SessionSupervisor::deliver_retained
  （steering 通道，下轮前排出）；用户面 = app_events.publish("goal_over_budget",
  {sessionId, name, kind, tokensUsed, tokenBudget, activeMs, timeBudgetMs})。注入失败仅
  tracing（现役 try/catch 不吞账本语义）。

### δ3 goal 工具（goaltools.rs；网关正式注册）
- manifest：name "goal"，description/参数 schema 逐字平移（action/name/token_budget/
  time_budget_minutes），input_schema 手写 JSON（现役 TypeBox 形状同义）。
- 权限：kernel 新增 PermissionKind::ArgumentAwareGoal（先例 ArgumentAwareCheckpoint/
  Channel/Browser）：action 缺省或 "status" → Read + capability_base "goal.status"；其余 →
  Execute + "goal.<action>"（现役 routine→Execute 映射，与 R04 既定 kind 映射一致）。
- executor：RunContext.session_id 直接得会话键（现役 getSessionPath 解析的 Rust 对应；
  无会话上下文不可能——工具只能在轮内调用）。默认预算从 preferences.goal（userprefs 同
  存储）读取。返回形状 {content:[{type:"text",text}], details}——Rust ToolOutcome 文本 +
  details JSON；逐文案平移（含 created/paused/resumed/completed+Final readout/dropped 与
  两条失败文案）。

### δ4 loop 存储与消息（loopx.rs 存储半 + 消息构造器）
- LoopStore：`<data_home>/.ephemeral/loop-state.json`（与现役 lingxiHome/.ephemeral 同位；
  character_cards.rs:127 同基目录先例）。schemaVersion 1 + loops[]；字段逐名平移
  （key/target/status/prompt/turnCount/consecutiveFailures/alarm/limits/pausedReason/
  completedSummary/createdAt/updatedAt）；JSON 2 空格 + 尾换行 + 原子写；腐败改名
  .corrupt-<ts> 并 tracing::error；ACTIVE=running|paused 排他 create → loop_already_active。
  LOOP_DEFAULT_LIMITS 原值（50/3/60/1200/1200）。
- 消息构造器逐行平移：kickoff/wakeup 的 <hana-loop kind=...> 正文（含 Task/Budget/Progress
  行）、details{schemaVersion 1, kind, prompt, reason?, turnCount, maxTurns}、notice。
  customType loop-turn/loop-notice/loop-user-prompt 仅作 details 携带（Rust 无 custom
  entry 类型，落 model_history 见 δ5）。
- target 形状：现役 {kind:"desktop",sessionId} | {kind:"bridge",...}；Rust δ 只承接
  desktop 面（桥接 loop 依赖现役桥挂钩，Rust 桥运行时无对应物——登记偏差；桥接会话
  resolveTargetFromSessionRef 返回 None → slash 报「无法定位当前会话」）。

### δ5 loop 控制器 + 闹钟（loopx.rs；tokio 定时器）
- 控制器 deps 的 Rust 映射：resolveSessionIdForPath→session_locator::resolve；
  isTargetMidStream→SessionAdmission::is_busy（仅守恒检查用）；isTargetRunnable→
  storage.get_session 存在；hasLiveBackgroundWork→TaskTools::list(session) 有 pending
  （subagent_threads + operations 两源，与现役 taskRegistry+deferredStore 两源对应）；
  emitLoopStatus→app_events.publish("loop_status",{target,loop})（app-events 轮询面，
  无 WS 广播通道——appevents.rs:11-13 已注；桌面按 sessionId 路由）。
- deliverLoopMessage/recordNotice 的投递语义映射（核心登记项）：现役写 display:false
  custom_message（模型上下文可见）+ triggerTurn；Rust 对应 = ModelHistory::
  record_automation_input 式落库（模型可见归档，model_history.rs:1141）+ 空闲时
  spawn background drive（background.rs:326-378 先例，drive_run_with_started 可内部发起），
  会话忙 → {mode:"busy"}（闹钟 60s 顺延 ×30）；recordNotice 只落库不起轮。
  recordUserPrompt：写 messages 树 "message" 行 role=user + display_text=prompt
  （session_tree.rs:1608 展示型气泡通道；model_history 不写 → 永不进模型输入，与现役
  appendCustomEntry 语义一致）。
- 状态机逐方法平移：start（排他+先推状态+kickoff 失败 loop_target_reset 收尾映射=
  drive 无法发起时 stopped+session_reset）/stop/pause/resume（budget_exhausted 复位
  turnCount）/toolSchedule/toolComplete/onTurnStart/onMessageEnd(stopReason error)/
  onTurnEnd（轮记账+预算/连败暂停+守恒兜底）/recoverAtBoot（running 失 target→stopped
  target_unavailable + rehydrate + 无闹钟兜底）。
  onTurnStart/onMessageEnd/onTurnEnd 的 Rust 挂点：runs.rs 每轮迭代顶=turn_start；
  provider Failed/重试=message_end error；run finalize=turn_end（aborted=Cancelled）。
  现役「turn」≈Rust「run」：注入轮标记 pendingLoopTurn/activeLoopTurn 按 run 归位。
- 闹钟：tokio Sleep；现役 MAX_TIMER_DELAY_MS=2^31-1 链式续接是 JS setTimeout 产物——
  tokio 无此限，登记偏差不移植（wakeAt 语义不变）。投递重试 [5s,25s,125s]、
  busy 顺延 60s×30、过期重水合 jitter 1-5s（rand 用 auth::random 字节推导，不引新依赖）、
  short_alarm_with_live_work 拒绝文案逐字、_fire 先清槽再投递、
  onDeliveryExhausted→pauseForDeliveryFailure。错误码 LoopError/LoopAlarmError 同
  （loop_already_active/no_loop/loop_not_running/loop_not_paused/no_active_loop/
  short_alarm_with_live_work）。
- **实施纠偏（as-built，2026-10-17 登记）**：
  1. emitLoopStatus → 改经 `ChatWire::publish_loop_status` 做 WS 广播（现役扁平帧
     {type:"loop_status", sessionId, status, turnCount, maxTurns, pausedReason, prompt}，
     chat.ts:1853-1868 形状；sessionPath=session_locator::native + sessionRefVersion:2 +
     ts；plain tx.send 不进 replay 缓存）。设计时误判「无 WS 广播通道」，实施时发现
     ChatWire::publish_session_created 的无 run 广播先例，取高保真面；app_events 轮询
     面不再承担。冷注入面不变（sessions 列表 loopStatus 仅 running/paused）。
  2. recordUserPrompt → 现役是 custom entry（recordCustomEntry type:"custom"
     customType:"loop-user-prompt"），不进模型输入、加载层投影 user 气泡。δ5 原设计的
     「messages 行 entry_type="message" role=user」会漏进模型上下文
     （model_history.rs:917 只滤 entry_type!="message"）——as-built 改
     entry_type="loop-user-prompt" 行 + history_projection.rs 新增
     MessageClass::LoopUserPrompt 投影臂（classify 先于通用 != "message" → Unknown 臂；
     content_json.prompt→user 气泡，RunAssociation::unknown，解析失败 LegacyRaw），
     与现役 custom-entry+加载层投影语义逐点对齐。
  3. recordNotice → 现役 display:false custom_message（重启后仍在历史中可见）；Rust 无
     custom entry 类型，as-built = SessionSupervisor::deliver_retained 回灌（易失、
     下轮前排出）——登记偏差（notice 不再持久化进历史）。
  4. hasLiveBackgroundWork/isTargetRunnable → 异步 `TargetProbe` 别名：注册源需 DB
     访问，且后台工作实为**三源**（subagent_threads::background_tasks +
     operations::owned_background_tasks + media::list_session_unsettled 任一
     pending/unsettled；media 源是 Rust 侧等价后台工作面的补全，登记）。探针错误
     fail-soft（tracing::warn + false）。`LoopAlarmService::schedule` 保持同步纯函数，
     收预计算的 has_live_work: bool；tool_schedule/守恒检查/recover_at_boot 三个
     调用点均为 async。

### δ6 slash /loop（slash.rs 新臂）
- dispatch 增 "loop" 臂（/loop <任务>|stop|pause|resume|status），owner+session 域语义
  由 slash 既有 principal/session 解析保证；_renderLoopStatus 逐行平移（循环状态/任务/
  轮数/闹钟/完成摘要；闹钟时间格式化为 RFC3339——现役 toLocaleString 环境相关，登记
  偏差）；LoopError.message 中文案逐字；用法错误文案逐字。WS Slash 与 HTTP slash_route
  同臂生效。
- **实施注记（as-built）**：唤醒轮经 kernel 新增 `RunOrigin::Loop`（wire "loop"）走
  execute_scheduled_for（trigger_id=会话键）；grant Full →
  InvocationPermissionContext::UserSession，交互批准面保留（现役循环轮=普通会话轮语义）。
  **刻意不**入 runs.rs 的 Cron|Heartbeat→Automation 匹配与 automation_review 闸；
  media_source_authority 通配拒绝对其 fail-closed（正确姿态）。RunSupervisor 经
  `Mutex<Weak<LoopController>>` + attach_loop_controller 晚绑定（消费式 builder 与
  Arc<RunSupervisor> 循环依赖不可行；in-file browser 字段先例）；钩子前 None 保持
  原形状。桥未附着会话 fail-closed 文案 "loop: bridge delivery is not available"（δ4
  的「无法定位当前会话」仅用于 target 解析失败路径）；owner 闸文案
  「管理循环任务需要本机所有者权限」。闹钟行格式化 RFC3339（toLocaleString 环境相关，
  登记偏差沿设计）。前端零改动：桌面 /loop 已走 server-command 通道
  （desktop slash-commands.ts:305-310），ws-message-handler 已处理
  loop_status/session_user_message。

### δ7 loop_control 工具（goaltools.rs 同文件或 looptools.rs）
- manifest：name "loop_control"，label "Loop Control"，description 逐字，参数
  action(schedule|complete)/delay_seconds/reason。权限 Execute + "loop_control.control"
  （现役 routine）。execute：无控制器→服务不可用文案；无活跃循环→对应文案；
  schedule 缺 delay_seconds→文案；教育性拒绝（short_alarm_with_live_work 等）以文本
  工具结果返回不抛栈（现役 try/catch 语义）；scheduled/completed 文案经 i18n——Rust 侧
  工具文案按现役 en 源文案直出（现役 t() 在服务端解析 en 默认；登记说明）。
- i18n key 不移植（Rust 无 t() 运行时）；文案取现役 locales en 值。
- **实施注记（as-built）**：delay_seconds 为 JSON number → `.max(0.0).round() as u64`
  （Math.round 同义，goal-budget 先例）；schedule 成功文案 wakeAt =
  to_rfc3339_opts(SecondsFormat::Millis, true)（≡ 现役 toISOString）。现役「无控制器
  →循环服务未就绪」与「无会话→无法定位当前会话」两分支在 Rust 组合根恒装配
  （LoopServices 总是构建）+ 工具必在轮内会话上下文执行的形态下**不可达**，不留死
  代码（登记）。全部结果（含教育性拒绝 err.message 透传）走正常文本 ToolOutcome——
  现役 text() 从不置 isError。测试经真 registry+gateway 断言 Execute/
  loop_control.control 解析。

### δ8 workflow 引擎（workflow.rs + workflowjs.rs；**新依赖 boa_engine = "=0.20.0"**）
- JS 沙箱：boa_engine 0.20（纯 Rust，符合依赖选型先例 quick-xml/zip-zlib-rs/jieba；
  v8/deno_core 过重、rquickjs 引 C 均不取）。已探针验证（/private/tmp/boa-probe，
  4/4 通过）：meta 对象字面量纯净 context 求值；async IIFE + 原生异步宿主函数；
  Promise.all 并发原生 future；对象结果 to_json 回宿主。
- 驱动：每 run 一个专用 OS 线程 + current_thread tokio runtime（boa Context !Send；
  与主运行时经 Arc+CancellationToken+tokio 通道通信）。定制 TokioJobQueue：
  promise job 同步排空；FutureJob(!Send) 入 FuturesUnordered 在当前任务驱动——
  boa 默认 SimpleJobQueue 用 pollster::block_on 会死锁当前线程 runtime（已复现）。
- meta.rs 平移：正则定位 export const meta、matchBrace（跳过三种字符串字面量内花括号）、
  纯净 context 50ms 等价（boa eval 无超时参数——同步求值字面量，恶意深套由宿主脚
  本整体 deadline 兜；登记偏差）、name/description 必填、normalizeExports（export
  default→__wf_default 尾调、其余 export 剥前缀）。declarativeNodesUnsupported 拒绝。
- sandbox 语义：null 原型全局 + 只注入宿主 API（boa realm 固有内建对象在——禁网/禁 fs
  靠不注入 require/process/fs/net 与宿主函数面收窄，同现役机制）；Math.random/Date.now
  覆写抛错（逐字文案）；包装 async IIFE；非确定 API 禁止。raceDeadline → 驱动循环
  select(CancellationToken, totalTimer+slack)，脚本 promise 状态机收尾。
- host API：agent/parallel/pipeline/workflow/phase/log/budget/args 全量。原生侧只暴露
  __native_* 基元（JSON 字符串信封进出）；JS bootstrap 组装现役语义（await 强制、
  result 属性陷阱、unawaited 检测由 runtime contract 记账实现）。
- agent 节点执行：节点 = 父会话上的子 run（RunGrant::Subagent，lineage.parent_run_id=
  调用轮 run，cause_ref=workflow taskId——复用 R04/R05 既定 subagent-as-child-run 模型，
  即「工作流动作复用 Supervisor 与工具网关」；现役 isolated child session 的 Rust 对应，
  登记映射）。dispatch 后 await drive_run 得 RunFinish 取终稿文本（CompletedWithFinal
  message；无终稿=空串与现役 replyText??"" 同义；Failed/Cancelled→错误分类）。
  节点 15min 超时=独立 CancellationToken 链父 token；重试分类 isRetryableNodeError
  逐表平移（8 code + 8 正则；超时措辞避开「中止/aborted」）；退避 5s/20s ±20% jitter；
  MAX_NODE_RETRIES=5 钳。并发 limiter 信号量 maxConcurrent（默认16 钳[1,64]）+
  maxTotal=1000 backstop。budget.total= args.budgetTokens；spent()=query_model_call_usage
  （session 行 in-memory 滤 cause_ref==taskId；ModelUsageQuery 无 cause_ref 过滤器，
  登记实现注记）+ usageTokens 汇总；remaining()<=0 拒新节点。
- journal：`<agent_dir>/workflow-journals/<runId>.jsonl`（agent_dir=data_home/agents/<id>，
  core/agent.ts:967 同位）；computeKey=sha256({p:prompt,o:sanitizedOpts})[:16]（sanitized
  去 signal/onSessionReady/函数——Rust opts 经 JSON 入，天然无函数）；tryReplay
  invalidatedAfter 语义、record 追加行、replayHits 计数、JSONL 逐行容错加载全平移。
- 运行骨架：mintTaskId("workflow") 格式平移（task_workflow_<base36 now>_<base36 seq>_
  <rand6>，task-identity.ts:38-54）；hub/ActivityHub→app_events "workflow_progress" +
  runs 表血缘投影（现役 workflow-activity.json 无对应，登记偏差：活动条目由 run 血缘
  + app events 承担）；watchdog 无进展 10min + 总量 4h（钳表平移）+「报什么就是什么」
  abort-first；deferred → store 无对应：终稿/失败经 SessionSupervisor::deliver_retained
  回灌父会话（subagent-result 同通道，文本前缀 [workflow-result task=...] 登记形状）+
  工具结果立即返回 {taskId, runId, workflow, streamStatus:"running", startedAt}；
  无父会话/无 deferred 基础设施→同步 _syncRun 平移（无 journal，失败不给 resume 指引）。
- node-folder-scope：shape 校验/default-deny WRITE_FOLDERS_REQUIRED/realpath+attenuation
  逐项平移（错误码逐字，供重试分类）。**登记偏差**：收窄后的写作用域不落节点沙箱——
  Rust 写授权是服务级 workspace+grant（resourceaccess.rs:15-51），无每轮 scope 通道；
  节点以父会话 scope 运行（声明与越界校验仍 fail-closed 强制）。工具 description 逐字
  保留（模型契约不变），此偏差写入 ζ 的覆盖注记。
- structured_output：schema→一次性工具注入节点 extraCustomTools——Rust 无
  extraCustomTools 通道；映射：节点 prompt 追加现役同文案 + structured_output 调用
  约定经子 run 的正式工具面无法满足——登记偏差：schema 节点的结果取终稿文本按
  JSON 解析尝试（失败回退原文），不注入真工具。MVP 无深度校验与现役一致。
- 嵌套 workflow：共享 limiter/token/budget 限一层平移。
- 工作流工具 manifest：name "workflow"，label "Workflow"，WORKFLOW_DESCRIPTION 逐行
  （含 runtime model 段与 least-privilege 段），参数 script/args/resumeFromRunId/limits。
  权限 Execute + "workflow.run"（现役未声明 sessionPermission——routine 默认；登记）。
- **实施注记（as-built，2026-10-17 登记）**：
  1. `_syncRun` 结构不可达：RunContext 恒带 session_id 且组合根恒装配 deliver
     通道；会话行缺失响亮失败（"Workflow dispatch failed: parent session row is
     unavailable"），不留死代码（δ7 先例）。
  2. resumeFromRunId 增加词表校验（[A-Za-z0-9_-]，即 mint 产物词表），词表外
     按「无旧 journal」处理——防 path.join 路径逃逸，比现役严格的硬化注记。
  3. budgetTokens：非有限数→无预算；负数→0（现役 remaining()<=0 首节点即拒的
     等价效果）；分数按 Math.round 同义（goal 先例）。spent 查询失败 fail-soft
     到最近快照 + tracing::warn（不阻塞流程、不静默）。
  4. 终态通道：deliver_retained 文本 "[workflow-result task={id} status=done|
     failed]\n\n…"（登记形状）；workflow_progress 仅承载 log/phase/journalResuming
     （payload {taskId, message|title}——现役 {...evt, type:"workflow_progress"}
     的 type 覆盖语义同形，evt 内部 type 子类本就丢失）。block_update/ActivityHub
     条目不移植（runs 表血缘承担，设计节已登记）。
  5. 喂狗保真：仅 agent 节点生命周期/step/log 事件喂狗；phase 不喂（现役
     onProgress 不喂狗，仅 onAgentEvent 喂）。
  6. agentType 解析：SubagentRuntime::workflow_roster（candidates("") 传永不匹配
     空 current——现役 resolveAgentId 不排除调用方自身）。花名册读取失败 →
     "cannot be resolved in this environment."（不可重试正则命中）；现役
     「resolveAgentId 不可用」分支在 Rust 结构不可达。
  7. RunFinish 映射：CompletedWithoutFinal→""；Failed→"agent 失败:
     {reason_segment}"；InterruptedNeedsAttention→"agent 失败: {detail}"
     （as-built 登记）；Cancelled→detail（scope 已取消时重试分类被
     !scope.is_cancelled() 主导，与现役 !signal.aborted 同义）。
  8. 节点许可：活父 Weak 升级继承，父已收场 gate.try_acquire 新取
     （"session_busy" 可重试——不在不可重试表）；retain 先于 spawn_linked
     （spawn_linked_with_settlement 在 spawn 时升级弱许可）。
  9. rustc 递归 async 经 spawn 的 Send 环：Nested 臂处理收进同步函数
     handle_host_request（类型环在同步边界断开），无 Boxing hack。
  10. TokioJobQueue 跨 await 不持 RefCell borrow（FuturesUnordered 换出驱动、
      完成后换回 + debug_assert 不变式）——clippy 合规无 allow。
  11. bootstrap 修正（as-built 纠偏）：log() 消耗与 agent 节点同一 nodeSeq
      计数器（现役如此——跨引擎 journal resume 的序号对齐前提）；驱动侧
      cancel/超时/挂起三处 "{name}" 字面量占位修正为 format! 实际值。
  12. bootstrap writeFolders 绝对性预检为 startsWith("/") POSIX 近似；Windows
      绝对路径会被预检误拒（原生侧 Path::is_absolute 平台正确，但到不了）——
      边角登记（现役桌面主平台 macOS/Windows，Windows 下写域声明需以 Rust
      侧为准的后续项，先登记不阻塞）。
  13. 重试日志消息用 .chars().take(120)（码点）对应现役 slice(0,120)（UTF-16
      码元）——代理对边角差，登记。
  14. limiter 槽位等待挂 scope 取消（现役队列等待不响应 abort、靠 thunk 首行
      signal 检查兜底；结局一致、响应更快）。

### δ 验收测试形态
- 单元：goal 状态机全转换+超支恰好一次+暂停时长排除；loop store CRUD+腐败改名；
  闹钟钳制/拒绝/重试编排（注入 now/定时器双）；journal 复用/失效；meta 提取含恶意
  输入；重试分类表；mint_task_id 形状；limits 钳表。
- 集成（tests/r07_goal_loop/mod.rs + tests/r07_workflow/mod.rs，隔离 home）：
  goal 工具六动作经 WS/HTTP 工具调用闭环（真 runs 驱动，模型面确定性替身含
  token_usage 上报 → 预算累计 → 超支事件恰好一次）；/loop 五子命令作用于真会话
  （kickoff 驱动一个替身模型轮；schedule→闹钟触发唤醒轮；complete→状态可见）；
  loop_control schedule/complete；workflow：替身模型下 script 起 2 节点 fan-out →
  runId 即回、workflow_progress 事件流、终稿回灌、journal 文件生成；resumeFromRunId
  复用未变节点（替身调用计数断言）+ 首处变更起全重跑；writeFolders 校验矩阵；
  meta 非法同步拒绝。
- 回执 dev-t07d3（三命令：测试组+fmt+clippy，指纹前后稳定）。

## 切片进展
- **β1+β2 = LOCAL_VERIFIED（回执 dev-t07w1，日志 sha256 7532256a…3e32，57 项全绿）**：
  - 新增 `workbench.rs`（~1100 行）：studio workspaces list/create/delete + files，
    workbench files/search/content（serveFileContent 全平价：28 MIME/弱 ETag/Range 206·416/304/
    Content-Disposition 双语文件名）/actions（mkdir·writeText·rename·move·movePaths·safeDelete，
    create==writeText 无 mustNotExist——与现役 workbench 路由一致，desk 路由的 create 才有）/
    upload（路由级 80MiB body 限 = 现役 MAX_UPLOAD_BODY_BYTES，单文件解码 25MiB 逐项 results）。
  - `resource_mounts.rs` 挂载注册表写 API（upsert/disable/校验/2空格+尾换行原子写，
    mountId 公式与现役同）；`resourceio.rs` 增补 `mkdir` op。
  - `checkpoint_store.rs::save_user_edit`（覆盖前存档，reason=mobile-workbench-edit，
    utf-8 roundtrip 判定 content|content_b64）。
  - 写租约台账 `security/execution-leases.json`（现役字段全集，issued→consumed/revoked，
    local owner 不落台账——字节级断言）。
  - `security_audit::project` 增补可选 `result`/`leaseId` 透传（缺省形状不变，既有调用方
    逐字节不变；新增单测钉死）。target 按投影契约改字符串（studio:{id}[:mount:{id}]），
    现役 target 对象信息完整保留在 metadata。
  - auth.rs：删除死分类（sessions 前缀块内不可达的 workbench 分支），按 route-security.ts
    注册真实分类（files/search/content GET|HEAD→files.read；actions/upload POST→files.write；
    workspaces GET→files.read、POST→files.write、{id}/files GET→files.read、{id} DELETE→
    files.write（现役为 /api/* 兜底 STUDIO_OWNER，处理器内 local-owner 闸是决定性控制，
    差异已注释登记））。
  - 中间件 scope 拒绝信封 = EndpointError 统一形状（details.reason/requiredScope），与现役
    处理器内 {error,reason,capability} 形状不同——中间件层差异，测试按 Rust 实际形状断言。
  - 排序：目录优先 + 码点序（现役 localeCompare "zh" 的登记偏差）。
  - **顺修回归（T05/T06 时代引入）**：run() 关停链新增宿主 stop 在队列已关闭时把
    QueueClosed 传成 Serve 错误，打破 auth_matrix a07 契约（存储中途死亡不应让关停双重
    报错）。media.stop / beautify.shutdown 的 list_open 对 QueueClosed 显式降级并标注
    （tracing::error + 继续内存收尾；在役 run 由 Phase 2.5a 统一取消，存储状态由
    Phase 3 如实上报）。auth_matrix 25/25 回绿。
  - 测试：`tests/r07_workbench/` 6 项（workspaces CRUD+注册表平价+审计、投影+闸、
    全动作+checkpoint+六审计动作、upload+content 全域、scope 闸+租约台账+local_owner
    闸、挂载根端到端含 disable 后 404 unknown_root）。
- **β3 = LOCAL_VERIFIED（回执 dev-t07d1，日志 sha256 338df5ac…9c38，553 项全绿 +
  fmt/clippy 干净）**：
  - 新增 `desk_files.rs`（~700 行）：GET /desk/path、GET /desk/files、
    GET /desk/search-files、GET|POST /desk/jian、POST /desk/files（upload/create/mkdir/
    rename/move/movePaths/remove 七动作）——现役 desk.ts :1414-1688 全语义端口。
  - 批准目录门禁：isApprovedDir 双段（selectedAgentDeskRoots 严格 realpath +
    isApprovedDeskDir 父目录回退解析），根 = 助手 home_folder（missing 即弃，不惰性清洗
    配置——登记偏差，GC 归 prune_missing_workspace_config）→ 服务 workspace_root →
    焦点会话 cwd → 会话 workspace_folders → desk.cwd_history；`isInsidePath`、
    `isSensitivePath`（~/.ssh 等五目录 + realpath(lingxiHome)）、`classifyWorkspacePathForGc`、
    `normalizeWorkspacePath`、`resolveKey` 五语言文案表全平价。
  - `resourceaccess.rs::authorize_engine_plane`（现役 resource-access-policy.ts
    sandbox-off 引擎面：真实路径解析 + 受保护根/`.lingxi-recovery-` 拒绝，不按工作区
    根收窄）；`OpContext.engine_plane`（默认 false，仅 desk 面置位——desk 根可在
    workspace_root 之外，决定性控制在路由层门禁）。
  - `resourceio.rs::copy` 新原语：kind 不等 → 501 cross_provider_copy_unsupported；
    挂载源读租约（跨挂载允许）；本地源引擎面授权；copy_tree 全平价（符号链接重建、
    目录递归、文件内容复制、特殊文件响亮报错）；按 mkdir/write 约定补 capture_history
    （现役 copy 不记，收紧登记）。
  - `workbench.rs` 服务层参数化重构（action/service 函数改收 &ResolvedRoot，处理器先
    解析根），新增 readText/safeDeleteIfExists/copyLocalPathIntoDirectory 与
    `must_not_exist`（409 target_already_exists 先于改前存档）——全部 pub(crate)
    复用于 desk 面；既有 6 项 workbench 测试与 R06 兼容测试原样回绿。
  - 三种错误面逐字节对齐：守卫字符串 200（dirNotAllowed/invalid subdir/invalid path/
    noWorkspace 五语言）、deskRouteError {error:{code,message}} 显式状态、服务错误
    {error:映射消息, code} err.status||400；unknown action 的 JS 模板化
    （undefined/42/true/[object Object]）与 parseInt 前缀语义都平价。
  - 登记偏差：查询值单次解码（现役 Hono 已解码后再 decodeURIComponent 的双解码不复
    制）；非字符串 content 的 TypeError 消息保持但无 code 字段；搜索/列表在目录中途
    消失的竞态下现役吞错返回空、候选人响亮 io 错误（与 workbench 面同口径）。
  - auth.rs：desk 读面仅 GET → files.read（现役无 HEAD，与 workbench 块的 GET||HEAD
    不同，按现役口径）；desk/files+desk/jian POST → files.write。
  - 测试：`tests/r07_desk_files/` 8 项（path 门禁含父目录回退怪癖+home_folder+
    cwd_history+missing 回落不改配置、列表+子目录守卫+符号链接逃逸、搜索+limit 口径、
    jian 读写删循环+三语言文案+非字符串 content、upload 全门类含 local-owner 403 与
    符号链接重建、七动作全守卫形态、无工作台回落、scope 门禁）。
- **β4 设计定稿（实施前侦察结论）**：
  - 现役面：POST /api/diary/write + GET /api/diary/list（server/routes/diary.ts 全文 +
    lib/diary/diary-writer.ts 854 行 + lib/time-utils.ts + shared/workspace-output.cjs +
    pi-coding-agent compaction.js/utils.js 提示词契约已逐行取证）。
  - Rust 已有可复用件：AuxiliaryExecutor + `AuxiliarySlot::Memory` 路由（dream_service
    availability 模式）、`am::scrub_pii`、`read_session_summary_record`（summaries 读面
    同契约）、`cms::read_full_branch`（分页到底不截断）、`get_session_row`（含
    memory_enabled）、`list_sessions`（active + 排除 automation 内部会话——恰等价现役
    listSessionFiles(sessionDir)，因现役归档移出活跃目录、活动会话另存 activity/）、
    context_compiler 的 resolve_persona_source/read_yuan 回落链、dream 的 04:00 逻辑日
    先例、OH-Works 根常量先例。
  - 架构性适配（登记）：现役材料来自 session .jsonl + 分支投影游标（lineageHash）；
    Rust 会话存于 run DB——材料改读 DB 全分支投影，摘要游标不可校验一律走现役
    "legacy" 关系回落路径（现役自身在 projection 缺失时的行为）；rollingSummary
    补写写回面未移植（摘要 ticker 属记忆子系统缺口，history_content.rs:1181 已登记），
    缺摘要会话直走临时补齐（等价现役 backfill 失败回落，日记产物不变，摘要不回写）；
    活动段（巡检/定时）依赖 desk ActivityStore（T07-ε 范围）——β4 等价现役
    activityStore=null 路径（无活动段），T07-ε 落地后接线；AuxiliaryRequest 无
    temperature 字段（现役终稿 0.7）——走模型默认，登记；语义 provenance 元数据粒度
    不移植（正文逐字节一致，观测归属经 record_model_call_usage origin=memory.diary.*）。
  - 平价端口：逻辑日（04:00 界、DATE_RE+字段校验回落今天）、summaries 范围过滤
    （updated_at||created_at ISO 字符串含端点比较）、messageCount/陈旧度补充判定、
    serializeConversation（[User]:/[Assistant]:/[Assistant thinking]:；工具
    结果条目在 Rust 存储为事件而非消息，2000 截断臂不可达、未留死代码）、
    SUMMARIZATION_PROMPT/UPDATE_SUMMARIZATION_PROMPT/SUMMARIZATION_SYSTEM_PROMPT
    与 diary 写作指导/约束全文、scrubPII、`<mood|pulse|reflect>` 剥离、标题兜底与
    文件名后缀（UTF-16 60 截断 + 非法字符剥离）、OH-Works/{日记|日記|日記|일기|Diary}、
    列表 .md UTF-16 序倒排、missing dir→{files:[]}、错误信封（400 材料/LLM 失败、
    500 槽位不可解析；zh/en 双语文案——现役 isZh 二值，ja/ko 落 en）。
  - auth：现役 /api/diary/* 无显式分类 → /api/* 兜底 STUDIO_OWNER → 按 R06 口径取
    最严格档 LocalOnly（双路由全动词），注释登记。
- **β4 = LOCAL_VERIFIED（回执 dev-t07d2，日志 sha256 7b7cbf01…8eedab，567 项全绿 +
  fmt/clippy 干净）**：
  - 新增 `diary.rs`（~1150 行）：POST /lingxi/v1/diary/write + GET /lingxi/v1/diary/list
    ——现役 diary.ts 63 行 + diary-writer.ts 854 行全语义端口。逻辑日 04:00 界
    （chrono Local，DST 04:00 病理响亮失败登记；targetDate DATE_RE+日历校验+
    TS 0-99 年 quirk 回落今天）；材料 = summaries 落盘范围扫描 + run DB 会话
    分支（list_sessions active+排除 automation 内部会话 ≡ 现役目录扫描；
    read_full_branch 分页到底）；legacy 关系路径（无 TS 谱系游标，登记）；
    messageCount/陈旧度补充判定与 selectSupplement 切片怪癖原样；临时补齐 =
    pi SUMMARIZATION/UPDATE_SUMMARIZATION 提示词契约 + 日记 customInstructions；
    终稿 = 填充后人格系统提示 + 分段用户提示（摘要/长期背景/写作指导/约束全文
    逐字）；scrub_pii、mood 块剥离（开闭独立配对非贪婪）、标题兜底、文件名
    后缀（日期前缀剥离 + 非法字符 + UTF-16 60 截断）、OH-Works 五语言目录、
    list UTF-16 序倒排 missing→[]。
  - 模型链：memory 槽 AuxiliaryExecutor（dream availability 同构），quota 获取 +
    RunContext + record_model_call_usage（purpose=auxiliary.memory，
    origin=memory.diary.temporary_summary/write）；工具请求响应 → 响亮失败
    （"Summarization attempted to call a tool"）。
  - 信封：400 {error, warnings?}（零材料双语、材料全失败含 formatMaterialWarnings
    详情、LLM 失败无 warnings 字段——现役形状）；500 {error}（槽位/人格装配）；
    成功 {filePath, content, logicalDate, warnings?}（空 warnings 不投影）。
  - context_compiler.rs 四助手转 pub(crate)（read_material/parse_agent_material/
    resolve_persona_source/read_yuan）；persona.rs 增 material_roots()；
    desk_files.rs 三函数转 pub(crate)（current_agent_id/home_cwd/desk_locale_key）。
  - 测试：`tests/r07_diary/` 14 项（list 缺目录+UTF-16 倒排+locale 目录名、零材料
    zh/en+targetDate 合法过去/非法回落/非字符串/非 JSON body、摘要材料端到端
    （人格填充进系统提示+材料/约束/逻辑日进用户提示+账本 origin）、无摘要会话
    临时补齐（SUMMARIZATION 系统提示+[User]:/[Assistant]: 序列化+无 previous-
    summary 臂+临时标记进终稿）、陈旧摘要 messageCount 切片补充+UPDATE 臂、
    范围外会话零材料且不触模型、mood 剥离+标题兜底、日期前缀+非法字符文件名、
    LLM 失败 400 双语、临时失败 400+warnings 详情、memory 槽未配置 500、
    LocalOnly 401/403）。模型面全部确定性 TCP 替身（SSE openai-completions），
    会话消息经真实 execute 入 run DB。
  - 登记偏差汇总（详见上 β4 设计定稿）：材料源 = run DB 分支（非 .jsonl+游标）、
    rollingSummary 写回不移植（摘要不回写、临时标记代替已补写标记）、活动段缺
    （T07-ε 接线）、temperature 0.7 无槽携带、provenance 元数据粒度不移植、
    长度截断的临时摘要无法识别（现役 stopReason=length 判失败）、500 文案来自
    候选解析器。
- **γ 设计定稿（git 面板 22 路由；现役 server/routes/git-environment.ts 563 行 +
  server/git/git-command.ts 1438 行全量通读）**：
  - 路由清单：GET status / worktree-info / worktrees / branches / log / stashes /
    file-diff（7）；POST log-stats / checkout / create-branch / stash / unstash /
    discard / worktree-create / commit / push / pull / stage / unstage / amend /
    fetch / ai-commit-message（15）。
  - 执行底座：tokio::process::Command argv 直跑 git（无 shell），env 继承 +
    GIT_OPTIONAL_LOCKS=0 + GIT_TERMINAL_PROMPT=0，kill_on_drop + 分级超时
    （默认 15s；push/fetch/pull/worktree-add/log-stats 120s），输出 16MiB 截获
    上限（超限杀进程按失败处理 ≡ execFile maxBuffer）；try_git 探测不抛 /
    run_git 抛 GitError（message=`git {sub} failed (exit N)`，stderr trim 随身上行，
    路由侧 error = stderr||message）。
  - 纯函数全端口：parse_numstat_z / parse_name_status_z / parse_for_each_branch_ref /
    parse_worktree_porcelain / parse_log_records（\x00 字段 \x1e 记录 + numstat 归
    属上一提交）/ stats_from_numstat_lines / is_valid_branch_name /
    is_valid_worktree_name / is_safe_rel_path / top_literal_pathspec /
    normalize_path_list / first_stderr_lines / count_untracked_lines（512KiB 截读 +
    NUL 判二进制 + 尾换行不计行）/ build_untracked_patch / cap_patch（256KiB+"\n…"）/
    state_from_numstat / sanitize_commit_message（剥围栏+前缀+行尾空白+压连续
    空行+600 截断）。
  - status 聚合：staged(cachedNumstat 零提交回退空树)/unstaged numstat +
    name-status 双通道状态字母（A/M/D 优先于 numstat 猜测）；同路径 staged+
    unstaged 合并展示、总量分账；未跟踪逐文件计行双预算（2000 文件/1000ms，
    超预算列路径记 0）；files 排序 = localeCompare → 候选人两级排序近似
    （主级小写化 UTF-16 比较 + 第三级同位小写优先；非 ASCII 全套 ICU  collation
    与 pinyin 本地化不移植，登记）；upstream/ahead/behind/remote → commitable/
    pushable。
  - 门禁：resolve_dir（path.resolve 语义——相对路径拼进程 cwd 后词法归一，
    stat 须目录；不 realpath）；GET 未获批 403 "dir not allowed" / POST 400
    "invalid dir"（无效与未获批同信封）；is_approved_dir_git = ①
    selectedAgentDeskRoots（严格 realpath，β3 同款）→ ② approved_desk_roots
    （父目录回退 realpath，β3 同款；现役 isApprovedWorkspaceDir 分支是其子集
    = 死代码不移植，登记）→ ③ [deskCwd, homeCwd, studioMountRoots,
    顶层 cwd_history]（严格 realpath）。studioMountRoots = studio-mounts.json
    里 active+storage+local_fs+rootLocator.path 非空（MountRegistry
    load_or_create——现役 ensureStudioMountRegistry 同款读时创建副作用；
    任何错误 → []）。
  - **β3 修复（本切片顺带）**：现役 cwd_history 在 agent config.yaml **顶层**
    （config.ts:233/sessions.ts:2114/agents.ts:326 均写顶层；engine.ts:4931 读
    顶层），β3 误读 desk.cwd_history——修为顶层并同步 β3 夹具与注释。
  - 操作族结构化分桶原样：commit（empty message→nothing_to_commit；add -A；
    diff --cached --quiet 探测）、amend（零提交 nothing_to_commit；-m/--no-edit）、
    stage/unstage（invalid_path；零提交 rm --cached）、push（no_remote/
    nothing_to_push/push_failed；无 upstream → push -u）、fetch、pull
    （no_remote/no_upstream/fetch_failed/already_up_to_date/diverged/
    local_changes（文件重叠结构预判，不靠 stderr 文本）/pull_failed；pulled=behind）、
    stash（nothing_to_stash 双判：porcelain 预探 + stdout 文本）、stashes（limit 20
    + stash show + ^3 ls-tree 未跟踪）、unstash（path 单文件：path_dirty/no_stash/
    not_in_stash/^3 源选择；无 path：pop + conflict 文本判）、discard（空树基线
    零提交；restore --staged --worktree；clean -f [-d]）、worktree-create
    （^[A-Za-z0-9][A-Za-z0-9._-]*$；exists/branch_exists/invalid_base；wt/<name>；
    <主树父级>/worktrees/<name>；mkdir 失败交 git 报错）。
  - log/log-stats：LOG_FORMAT %H%x00%h%x00%s%x00%B%x00%an%x00%at%x00%D%x00%P%x1e；
    --date-order --max-count=clamp(limit,1,1000)（parseInt 失败→300）；stats 两段
    加载（40 位十六进制白名单+去重+进程级缓存 cap 20000 插入序驱逐+每批 100+
    全零统计跳过）。
  - file-diff：is_safe_rel_path（400 invalid file path）；未跟踪合成全新增 patch
    （二进制 → {patch:"",binary:true}）；tracked = numstat HEAD 判 binary +
    diff HEAD（零提交回退空树）+ /^Binary files .* differ$/m 补判 + capPatch。
  - ai-commit-message：collectChangeContext（numstat 摘要 ≤80 文件 + diff HEAD
    -U3 截 12000 字符 + 未跟踪 ≤8 文件 diff --no-index 摘 + 行预算 4000/60）→
    空 400 "no changes"；summarize 槽 AuxiliaryExecutor（diary 同构；max_output
    =500；45s 超时 → "ai commit message timed out" 500；槽位解析失败 → 现役原文
    "summarize slot unavailable (no model configured and no chat fallback)" 500；
    网关/凭证/网络缺 → 503 "llm unavailable"）；sanitize 后空 → 502
    "ai produced empty message"；成功 {message}；用量 origin=utility.call-text.
    git-commit-message（purpose=auxiliary.summarize；sessionPath→sessionId 归属
    解析不移植，登记）。
  - auth：route-security.ts:84-89 + isGitEnvironmentRoute:438-462——log-stats 任意
    动词 files.read；其余 21 路径 GET→files.read / POST→files.write；GET|POST 以外
    动词落 /api/* 兜底 STUDIO_OWNER → LocalOnly（R06 口径）。
  - 登记偏差：temperature 0.3 无槽携带（AuxiliaryRequest 无字段，同 diary）；
    per-agent 槽位解析 → 活跃网关快照（agentId 仅门禁+归属，同 diary）；windowsHide
     unix 无对应；localeCompare 排序近似（见上）。
- **γ = LOCAL_VERIFIED（回执 dev-t07g1，日志 sha256 见回执；620 项全绿：lib 505
  含 git_panel 12 单测 + r07_git_panel 22 + r07_diary 15 + r07_desk_files 8 +
  r06_dream_completion 18 + r07_workbench 25 + r06_workbench_write 6 +
  auth_matrix 1 + r07_media_admin 11 + r07_t05_media 9；fmt/clippy 退绿）**：
  - 实施 = 设计定稿原样落地：`git_panel.rs`（~2900 行，模块 doc 内全量 fidelity
    注记与登记偏差）+ auth.rs 分类块 + lib.rs 接线（`pub mod git_panel` +
    `.merge(git_panel::router())` 于 diary 之后）。
  - **修复 A（β3 纠偏）**：`agent_desk_config` 的 cwd_history 改读 config.yaml
    **顶层**（现役 config.ts:233 / sessions.ts:2114 / agents.ts:326 写顶层、
    engine.ts:4931 读顶层；β3 误读 desk.cwd_history）；r07_desk_files 夹具
    yaml 同步改顶层，8 项回绿。
  - **修复 B（β4/梦补缺）**：现役 auxiliary-slots.ts:92-97 给 memory 槽
    fallback="chat"，diary `memory_slot` 与 dream_service `availability` 原先把
    RouteNotConfigured 当失败——两侧补 chat 回退臂（persona.rs 同款模式），双缺
    才报"记忆模型未配置"。dream 夹具 "unconfigured" 重定义为双缺、新增
    "fallback" 档（仅缺 memory）+ 新测试 `memory_slot_unconfigured_falls_back_to_chat`；
    diary 侧未配置测试改写为回退成功 + 双缺 500 两例。18+15 回绿。
  - 集成测试 `tests/r07_git_panel/`（22 项）：真实 git 二进制对临时仓（被移植面
    基板，本地无网络），远程 = 本地 bare 仓，模型面 = 确定性 TCP
    openai-completions 替身（diary 夹具同构）。覆盖：status 占位/三类合计/
    detached 短哈希/ahead-behind-pushable（behind 需先 fetch，现役同口径）、
    worktree-info/list、branches（checkedOutElsewhere）、log 钳制+refs、
    log-stats 白名单/全零跳过/合并 first-parent、file-diff 四形态+非法路径、
    checkout/create-branch 矩阵、commit/amend/stage/unstage（含未出生 HEAD 的
    rm --cached 臂）、stash 全流（^3 未跟踪恢复/path_dirty/not_in_stash/pop
    冲突/按路径储藏）、discard、push/pull/fetch 七桶（no_remote/no_upstream/
    nothing_to_push/already_up_to_date/diverged/local_changes/fetch_failed）、
    worktree-create 矩阵、门禁（GET 403/POST 400/不存在 400/studio 挂载根/
    agentId 显式 home）、ai-commit-message 净化+账本+400/500/502/503/chat 回退、
    scope 门禁（log-stats=files.read 特例、其它动词 LocalOnly）。
  - 实施期行为取证（真实 git 输出/语义）：① `--no-walk --numstat` 字节形状
    （\x1e\n 后纯 \n numstat 行、块间空行、尾块归最后提交）修正单测 fixture；
    ② `restore --source HEAD --staged --worktree` 对已暂存新增文件连工作区文件
    一并移除（discard 整仓回退依赖此语义）；③ stash pop 冲突报文含 CONFLICT →
    unstash_conflict；④ 本机 tag.gpgsign=true 会漏进测试 → 测试进程内
    GIT_CONFIG_GLOBAL/SYSTEM=/dev/null 隔离（已在回执 environment 登记）。
  - 信封纠偏：commit/push/pull/stage/unstage/amend/fetch 的 400 是 plain 信封
    （result 原样，无 error 字段）；create-branch/stash/unstash/discard/
    worktree-create 是 spread 信封（补 error=message||code）。另：外层
    error-enrichment 中间件对含 code 的错误体注入 details.requestId——测试按
    字段断言，不做整体等值。
  - clippy：γ 新增/改动文件零 warning；resolve_query_dir 定点
    `#[allow(clippy::result_large_err)]`（Err=完整 axum Response 直返，
    chat_wire.rs:1753 等先例）。diary.rs 残留 3 条 warning（954/1011/1362）为
    β4 作者代码、dev-t07d2.log 已含同款，非 γ 引入，原样登记。
- **δ1+δ2+δ3 = 实现完成（goal 家族；正式回执随 δ 家族 dev-t07d3 出具，当前证据
  2026-10-17：goal 8/8 + goaltools 13/13 单测绿，全量 lib 549/549）**：
  - 新增 `goal.rs`（891 行，8 单测）：GoalState camelCase 逐字段同形 + 2 空格缩进 +
    tmp+rename 原子写；侧车 `data_home/goal-sidecars/<session_id>.goal.json`（登记
    偏差，孤儿不清理）；状态机 create（active 排他 + 至少一项预算）/pause/resume
    （pausedMs 累计）/complete/drop/status/record_token_usage（active 才记 + 四舍五入
    非负 + 超支恰好一次回调）/tick_time_budget（懒检查点）/pause_on_abort；
    describe_goal 逐行同构；goal_progress pct=round(used/budget*100)（budget 0/None
    →None，over 严格大于）；now 注入可测。
  - 新增 `goaltools.rs`（670 行，13 单测）：goal 工具 manifest/description/schema
    逐字平移；kernel 新增 `PermissionKind::ArgumentAwareGoal`（toolcatalog.rs:444——
    action 缺省或 status → Read+"goal.status"，其余 → Execute+"goal.<action>"）；
    默认预算从 preferences.goal（userprefs 同存储）读取；六动作文案逐字（含 created/
    paused/resumed/completed+Final readout/dropped 与失败文案）。
  - δ2 钩子（最小侵入）：runs.rs `record_goal_token_usage` 观察缝（五分量求和、全
    None 不记——R05「未报不虚构」沿用；落盘后才记账=现役「中止未落盘不记账」）；
    每轮迭代顶 tick_time_budget（runs.rs:2217-2220）；sessions.rs:2052 cancel_run
    成功后 pause_on_abort；超支恰好一次 = deliver_retained（模型可见面）+
    app_events "goal_over_budget"（用户面），注入失败仅 tracing。
- **δ4+δ5 = 实现完成（loop 存储/消息/控制器/闹钟；同回执口径，loopx 15/15 单测绿）**：
  - 新增 `loopx.rs`（2116 行，15 单测）：LoopStore（`<data_home>/.ephemeral/
    loop-state.json`，schemaVersion 1 + loops[] 字段逐名、2 空格+尾换行原子写、
    腐败改名 .corrupt-<ts>+tracing::error、ACTIVE 排他 loop_already_active、
    LOOP_DEFAULT_LIMITS 原值 50/3/60/1200/1200）；kickoff/wakeup 的 `<hana-loop
    kind=...>` 正文（Task/Budget/Progress 行）与 details{schemaVersion,kind,prompt,
    reason?,turnCount,maxTurns} 逐行；LoopController 状态机全方法（start 排他+先推
    状态+kickoff 失败 loop_target_reset 收尾、stop/pause/resume budget_exhausted 复位
    轮数、tool_schedule/tool_complete/on_turn_start/on_message_end/on_turn_end 轮记账
    +预算暂停+连败暂停+守恒兜底闹钟、recover_at_boot 失 target→stopped
    target_unavailable+rehydrate+无闹钟兜底）；LoopAlarmService（tokio Sleep——现役
    MAX_TIMER_DELAY_MS 链式续接不移植，登记；投递重试 [5s,25s,125s]、busy 顺延
    60s×30、过期重水合 jitter 1-5s（auth::random 字节推导）、
    short_alarm_with_live_work 拒绝文案逐字、先清槽再投递、耗尽→
    pause_for_delivery_failure）；LoopError/LoopAlarmError 错误码表全平移。
  - 15 单测：store CRUD+腐败改名、start/stop 顺序与排他文案、pause/resume+
    budget_exhausted 复位、轮记账+守恒兜底闹钟、预算暂停/连败暂停 notice 文案逐字、
    tool_schedule/tool_complete 错误码、短闹钟拒绝+长兜底、闹钟触发清槽+投递、
    重试耗尽暂停、busy 顺延、开机恢复双路径。
  - 实施纠偏四条已并入上方 δ5 设计节（ChatWire WS 广播 / loop-user-prompt entry 类型+
    投影臂 / recordNotice→deliver_retained 易失 / 三源异步 TargetProbe），不再重复。
- **δ6+δ7 = 实现完成（slash /loop + loop_control + 组合根装配；loopsvc 2/2 +
  looptools 5/5 单测绿，全量 lib 549/549，kernel 126/126，fmt 干净，clippy 仅余
  β4 diary.rs 3 条既有 warning，workspace --no-run 全测试目标编译通过）**：
  - kernel：`RunOrigin::Loop`（wire "loop"）——唤醒轮 grant Full → UserSession 权限
    上下文（交互批准保留）；刻意不入 Cron|Heartbeat→Automation 匹配与
    automation_review 闸；media_source_authority 通配拒绝对其 fail-closed。
  - 新增 `loopsvc.rs`（485 行，2 单测）：build_loop_services 组合根装配（LoopStore
    加载；hooks：deliver=is_busy→Busy / execute_scheduled_for(origin=Loop,
    trigger_id=会话键, principal=scheduler_principal)→TriggerTurn / NotFound→
    loop_target_reset / 其余→deliver_failed；record_user_prompt 落
    entry_type="loop-user-prompt" 行 + ChatWire::publish_loop_user_prompt；三源
    live_background_work fail-soft；deliver_wakeup 经 Weak 控制器；
    on_delivery_exhausted→pause_for_delivery_failure）；render_loop_status 逐行
    （闹钟 RFC3339）；loop_status_inject 仅 running/paused。
  - 新增 `looptools.rs`（406 行，5 单测）：loop_control manifest 逐字（Execute+
    "loop_control.control"，30s 超时，CONSERVATIVE）；门禁→noActiveLoop 文案；
    schedule 缺 delay 文案、`.max(0.0).round() as u64` 钳（60s 下限经闹钟层）、成功
    文案含 RFC3339-millis wakeAt、教育性拒绝 err.message 透传；complete→
    "Loop completed and closed."；全部结果正常文本 outcome（现役不置 isError）；
    真 registry+gateway 断言权限解析。
  - `runs.rs`：loop_controller 字段 = Mutex<Weak<LoopController>> 晚绑定 +
    attach_loop_controller；turn 顶 on_turn_start（2222-2225）、ProviderTurn::Failed
    on_message_end("error")、adjudicated_finalize 统一结算后 on_turn_end
    （aborted=Cancelled）。
  - `sessions.rs`：execute_scheduled_for origin 闸放行 Loop（trigger_id=会话键）；
    会话列表注入 "loopStatus" 冷注入（sessions.ts:839，running/paused only）。
  - `slash.rs`：/loop 臂（owner 闸、桥未附着 fail-closed 文案、status/stop/pause/
    resume/start 全文案逐字，LoopError.message 透传 error 级；用法错误文案逐字）。
  - `chat_wire.rs`：publish_loop_status（扁平现役帧，plain send 无 replay）+
    publish_loop_user_prompt（session_user_message 帧）；publish_session_created
    先例。
  - `history_projection.rs`：MessageClass::LoopUserPrompt 投影臂。
  - `lib.rs`：loop_services 字段+访问器、组合根构建（auth.scheduler_principal()=
    local_owner(0)）、loop_control 工具注册（deps.tool_gateway 域内，与 goaltools
    同可达性）、attach_loop_controller + recover_at_boot（server/index.ts:997 同位）。
  - 修一处自用 lint：loopx 测试 attach_alarm 在异步探针改造后残留未用 host 参数，
    移除（rustc unused_variable；非现役行为差异）。
- **δ8 = 实现完成（workflow 引擎；570/570 lib 全绿，fmt 干净，clippy 仅 3 项既有
  diary，workspace --no-run 全编译）**：
  - `workflowjs.rs`（~1150 行，9 烟测）：boa_engine 0.20 沙箱——TokioJobQueue
    （promise 同步排空 + FuturesUnordered 当前任务驱动；跨 await 不持 RefCell
    borrow，换出/换回+debug_assert 不变式）；extract_meta/normalize_exports/
    match_brace 逐字平移；HostRequest 通道协议 + thread_local 宿主态（boa 原生
    async fn 只收 fn 指针）；JS_BOOTSTRAP 全量移植 host-api.ts（agent/parallel/
    pipeline/workflow/log/phase/budget/args + runtime contract 陷阱，文案逐字）；
    run_workflow_script 专用线程 + current_thread runtime + raceDeadline select
    （scope.cancelled/sleep/队列静默）。烟测：meta 缺失/返回值+args+budget 回合、
    agent 回合+log 计数器对齐、parallel 并发顺序、非确定 API 禁、unawaited 陷阱、
    嵌套闸、错误信封含 code、scope 取消「被中止」收场。
  - `workflow.rs`（~1150 行，12 单测）：WORKFLOW_DESCRIPTION 逐字 manifest
    （Execute+"workflow.run"）；mint_task_id/base36/rand6；normalize_run_limits
    钳表；is_retryable_node_error 8 code+8 正则、retry_delay_ms ±20% jitter；
    WorkflowJournal（load 容错/tryReplay invalidatedAfter/record 追加/key=
    sha256hex[:16]，payload 由 bootstrap 真 JSON.stringify 构造）；写域校验
    7 code 逐字；WorkflowLimiter（total++ 先查 backstop 文案逐字）；Orchestrator
    ——failWith 首写胜+abort、idle watchdog（log/step/agent 喂、phase 不喂）、
    总时长定时器、drive_script 请求循环（handle_host_request 同步边界断 Send
    环）、handle_agent（回放→limiter→中止/预算→opts 校验→重试循环→journal
    入账）、attempt_node（spawn_child 同款许可继承/fresh-acquire、retain、
    child scope、spawn_linked、超时链父 scope 后再等 settle——超时文案逐字
    全角括号无右括号）、budget 实时查询+快照、嵌套一层共享闸、终态
    deliver_retained "[workflow-result task=...]" 回灌。单测：mint 形状/词表、
    base36 对拍 toString(36)、limits 钳表、分类表全码全正则+超时措辞钉住、
    退避区间、journal 三形态、key 形状、写域矩阵（UNAVAILABLE/NOT_FOUND/
    NOT_DIRECTORY/OUTSIDE/INVALID/通过）、result_to_text、meta 提取含恶意输入。
  - `subagents.rs`：workflow_roster（candidates("")，现役 resolveAgentId 含自身）。
  - `lib.rs`：组合根构建 WorkflowService（storage/events/runs/sessions/subagent_
    runtime/app_events/agents_root/clock）+ register_workflow_tool（deps.tool_gateway
    域内，与 goaltools/looptools 同可达性）。
  - 登记：δ8 节实施注记 14 条（_syncRun 不可达、resumeFromRunId 词表硬化、
    budget 边角与 fail-soft、终态通道形状、喂狗保真、agentType 解析、RunFinish
    映射、许可仪式、Send 环解法、队列 borrow、bootstrap 纠偏×2、UTF-16/码点
    边角、limiter 取消挂接）。

## 红线沿用
隔离 home；不 push/PR/发布；外部一律确定性替身；不删用例不改预期；失败证据保留。

### 接续补漏

- 逐叶核对发现 `4AC692C4C7C8` 活动快捷摘要未在早期切片书中列入；已补正式辅助模型链与实际HTTP检查，需随最终候选验收。目标偏好 GET/PUT 另补只读不写盘、单键保留预算与未知设置、实际goal创建读取默认预算和坏输入不改文件的检查。
- 循环存储故障的实时消息/冷快照携带可选 runtimeError，页面只提醒一次，不把磁盘 running 冒充为 paused；`/loop status` 明确告知本机已停止自动推进。
