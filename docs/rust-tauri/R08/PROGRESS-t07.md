# R08-T07 进度台账｜现有界面全功能验收

- 任务：task-9（R08-T07），续接会话 2026-10-23 开工并完成线束与修复
- 状态：**READY_FOR_REVIEW**（A/B 线束终版绿 + 两个真修复落地 + 差异报告/平价结果交付；叶级全量覆盖未完成，如实登记）

## 交付物

1. **A/B 线束**：`scripts/rust-tauri/r08_t07_ab_drill.mjs`（双内核准备/启动/双跑/三分判定比较器：parity/differs/recorded+归因）+ `desktop/src/react/__tests__/r08/r08-t07-kernel-ab.test.ts`（15 任务固定集：同一 React 构建源的真实 store 动作/lingxiFetch，jsdom，观测落 JSON）。
2. **真实格式探针**：`scripts/rust-tauri/r08_t07_real_format_probe.mjs`（A 腿旧栈列出+可读真实 Pi 会话；B 腿 legacy-import 对账 conversational/unknownBlocks）。
3. **修复 F-T07-1（阻断级）**：legacy_import session_jsonl.rs + convert_sessions.rs 真实 Pi SDK 格式识别（头行≡sessionMeta、message.role 嵌套、RFC3339→unix ms、toolResult→已知非消息、message.content 提升）；加性，夹具路径零变化（t07-pi-fix-amend-proof-01：435/435/5206 + 80a8d2c2/40f42dc1 双锚保持 + 双跑全等）；新单测 pi_sdk_real_format_entries_recognized；探针红→绿（01/02 红 → 03 绿 conversational=3 unknownBlocks=0）；adapters 178/178、svc-bin 16/16、D1/D2/D3 重绑绿（t07-pi-fix-*）。
4. **修复 F-T07-2**：use-hana-fetch.ts 翻译表补 10 条会话管理写端口（rename/pin/archive/restore/archived/delete/cleanup/sweep/pin-order/todos×3/fresh-compact）+ **连接门**（仅 rust 连接翻译；无门时旧腿 404 = 修复过程引入的真 bug，drill-03 红实证后修掉）。
5. **报告**：`T07_UI_DIFFERENCE_REPORT.md`（F-T07-1/F-T07-2/D-1..D-5 + A14 证据 + 覆盖诚实声明）；`FEATURE_PARITY_RESULTS.json`（R08 增量）。

## 红证据链（全保留）

t07-ab-drill-01（夹具 entryType 会话对旧栈不可见 0 行——引出 F-T07-1）→ 02（B 腿会话管理写端口全拒——引出 F-T07-2；A 腿 0 会话根因）→ 03（翻译无连接门，A 腿误译 404）→ 04（pin 平价；剩余归因）→ **final-05（绑定绿：15 任务 parity=12 differs=0 recorded=2 B-failed=1）**；t07-real-format-probe-01（缺 ⑤a 准备，旧栈崩）→ 02（决定性红：conversational=0/unknownBlocks=4）→ 03（绿）。

## 关键定性

- **夹具格式自造性**：T04 夹具会话用 `entryType` 自造格式，真实旧栈是 Pi SDK 格式（lib/session-jsonl.ts + tests 先例）。T04–T06 的绿证据原本只对自造格式成立——F-T07-1 修复 + 探针后链条对真实格式亦闭合（夹具证据不受影响，路径未动）。此事实按红线如实登记：**早前"迁移链已验证"的表述对真实数据不成立（在修复前）**。
- D-4（B 侧 rename 标题回落 + 会话数变化）未决，post-R08 定位。
- 叶覆盖：约 20 叶入口通路实测平价；其余叶（D01/D03/D13/D20 及 D02 余量）required_behavior_pending，见 stage map。

## 验证

- adapters **178/178**（t07-pi-fix-adapters-02）；svc-bin 16/16（t07-pi-fix-svcbin-01）；D1/D2/D3 重绑（t07-pi-fix-d1/d2-01、t07-pi-fix-d3-02；d3-01 瞬态 abort 根因未证、红证据在案）；amend 双跑全等（t07-pi-fix-amend-proof-01）。
- 无 commit/push；合成 HOME；真实 ~/.lingxi 未触碰。

## 移交

- D-4 定位、D-1 产品裁决、D-3 扩腿（旧格式 models.json 注入后含聊天回合的全量 A/B）、D-2 补偿步骤入切换 runbook（已入 T06_ROLLBACK_RUNBOOK 语义域）。

### 2026-10-24 叶级补证轮（验证器指令：9 叶负路径/边界 → full_original_behavior）

1. **套件 v3**（t07-ab-drill-leafcases-01..06 红链全保留）：受控顺序（只读→负路径→轻变更→破坏性→终态）+ observe 记录后继续 + 破坏性守卫 + pending 复位。过程发现并修三处线束缺陷：vite SSR 循环引用（合并 it 块触发）、serverConnections map 整体替换毒化 B 侧 native 连接属主（只换 activeServerConnection）、旧栈默认权限端点预热抖动（有界重试 ≤3 记录尝试数）。
2. **叶案例值双内核闭合**（leafcases-06）：rename 负路径（空标题 400/无效会话 403|404 双内核拒）、pin/todos 无效会话显式拒、list 投影字段（revision/归属/摘要-徽章面）、unauthorized 列表路由 401|403、permissions 读幂等同值。
3. **升级集（诚实上限）**：门规 verify.rs later_stages_of——R08 末位无 later stage → stage_share_satisfied 恒 FAIL（R05 RR1 F25）；full_original_behavior 须逐断言专属案例。**rename（保存成功+持久化 | 空标题拒绝 | 无效会话明确错）与 list（计数/平价/revision/摘要徽章 | 鉴权拒绝）两叶达成**→升级 full_original_behavior；**cleanup 不可升**：期限边界呈真差异（**D-6**：A 侧 30 天窗保留新归档 vs B 侧立即清除——疑 B 归档时间基差异，未决）→保持 pending+注记；pin/todos×3/sweep/permissions 断言含不可合成前提（已删 Agent 禁置顶/计划面板内容语义/孤儿挂载/ASK 缺省）→pending+注记。
4. **cases.json 门合同化**：lingxi.leaf-case-results.v1（{case,expect,actual,ok}），只收合同图钉集（门规整份记录须成立；D-6 差异不入合同文件、留差异报告）。gate-02 红（schema 不符）→gate-03 红（整记录规则吃进 D-6 案例）→修复。

### 2026-10-24 第二轮：PPG-2 真修复 + MCP 叶 + a06 根因定名

1. **家族#9 定性更正（重大）**：a05 读腿红**不是环境红**——根因=`lib.rs` 会话授权根接线缺陷：运行时 `replace_session_roots` 只写 workspace_access 实例，而预览（无 workspace）下 read 工具用**另一个**零根实例 `session_read_access`（lib.rs:1838 注册）→ 会话 authorized_folders 永不生效 = PPG-2 的机制成因。探针链：t09-a05-auth-probe-01..03（authorizedFolders 落库 ✓ + 读腿拒）+ t09-a05-standalone-01（确定性复现）。
2. **PPG-2 修复（真产品修复）**：`with_session_resource_access` 接 `.or_else(session_read_access)`——预览模式运行时根刷新落到 read 工具实际使用的同一实例；会话级授权目录经正规孔道（CreateSessionBody.authorizedFolders）生效，零根策略不变（不授予 workspace）。**a05 独立复跑绿**（t09-a05-standalone-02 verdict=read-pass__write-blocked-PPG-1 failures=0：读腿端到端真实往返 ✓，写腿按 PPG-1 呈 BLOCKED 诚实不变）。svc-bin 16/16。
3. **a06 环境红根因定名**：① System Events keystroke 路径失效（quit-b 键盘路径穷尽→SIGTERM 兜底，journal 实证）；② 客户端 A 渲染目标中途死亡（connInfoA2 ECONNREFUSED → 重解析 90s 超时）。线束侧加固：cdpEvaluateClient 目标回收重解析（t09-a06-standalone-05）+ finish 后迟到异步错误不翻转判定。**语义断言至 health-after-b 全过；A06 场景维持环境受阻 FAIL（如实）**。
4. **MCP 叶（D03 只读）**：leaf.mcp.state（连接器状态+延迟加载开关投影形状：enabled bool/connectors array/defer 标志/agent 作用域——双内核 200 平价）+ leaf.mcp.state.unauthorized（无效令牌 403|401 双内核显式拒）。线束缺陷链：无效令牌换装需连 serverConnections map 一起换（requireServerConnection 读 map 活动项）且必须保全量复原（整体重建 map 破坏 native 连接属主毒化后续动作，t09-ab-drill-mcp-02/03 实证）→ 定稿：换装用例全部置于套件最末（无受害者）。**查看 MCP 连接状态叶（DD47275AC5AE）升级 full_original_behavior**（终版 t09-ab-drill-mcp-04：23 任务 parity=18 differs=3〔皆已知 D-4/D-6 级联〕recorded=2）。
5. 门图：3 叶 full_original_behavior（rename/list/mcp-state）+ 123 pending；gate-05 运行中。

### 2026-10-24 第三轮：D-4 定性推翻 + D04 权限叶 + 新差异 D-7

1. **D-4 定性推翻（重要）**：探针 t10-d4-probe-01 证明服务端 rename→DB 持久化→studio-list 重载（0/500/2000ms）全部返回新标题——**服务端无回落缺陷**。原「Rust rename 标题跨重载回落」定性错误；套件内 B-failed 真因=任务序列级联（rename 目标 sessions[0] 与 boundary 任务固定 byTitle 假设冲突）。修复：boundary 目标选取改为「标题未被本套件改过的会话」（与 rename 目标严格不相交）。原 D-4 从未决差异清单改判为「套件缺陷已修，服务端无缺陷」。
2. **D04 叶**：设置新会话默认权限（8D3CEB6133E1）**双内核全绿**（PUT 200 + 回读一致 + 非法值 400 拒）→ 升级 full_original_behavior。查看当前会话权限模式（BC2FBD618278）**不可绑定**：A 腿旧栈 switch 无模型 500（D-3 同根）；B 腿 200 但 permissionMode 为 DB 原值 null——**新差异 D-7**：旧栈 switch 投影归一化默认 mode vs Rust 透传 DB 值（null）。如实留债。
3. 本轮 A/B 终态（t10-ab-drill-d04-01）：25 任务 parity=19 differs=4（archive=D-6 族级联、drift=D-6 级联、boundary=D-6、session-mode=D-3/D-7）recorded=2。

### 2026-10-24 第四轮：D-6/D-7 处置 + 归档链三真缺陷修复 → A/B 线束实质收敛

1. **D-6 改判（伪差异）**：探针 t11-archive-probe-01/02 定位——① 有子会话的顶层归档被 **409 child_sessions_present 正确拒绝**（现役语义，套件此前误当「Rust 立删」）；② cleanup(30) 删除的是 fixture 自带超期归档（archived_at=2025-10，>30 天，正当删除）。cleanup 产品行为正确。
2. **归档链三真缺陷发现与修复**：
   - **归档列表 500（真产品缺陷）**：Rust `/lingxi/v1/sessions/archived` 预览提取对 legacy 导入消息 content_json 形状不匹配时整体 Corrupted→500（t11-archive-probe-02 实证）。修复：预览装饰性字段单条形状不匹配跳过（session_admin.rs，t11-archive-probe-03 全绿：列表 200/含目标/cleanup(30) 保留新归档）。
   - **F-T07-3（客户端真缺口）**：Rust 归档列表 `{sessions:[…]}` 包裹形 vs 旧栈裸数组——`listArchivedSessions` 未解包，UI 归档列表在 Rust 下拿到对象全断。修复：Array.isArray 解包。
   - **F-T07-2 补遗**：`/api/sessions/restore` 翻译缺失 + Rust 归档行无 path 字段 → 定位符回退裸 sessionId（连接门内回退，旧腿不受影响）。
3. **D-7 修复**：switch 投影 permissionMode null/空 → 归一化 "ask"（对齐旧栈；t11-ab-drill-fixes-01 B 腿 modeValue=ask 幂等 ✓）。BC2FBD618278 叶仍不可绑定（A 腿旧栈无模型 500=D-3 环境前置）。
4. **ASK 缺省态对拍**：双内核全新无偏好根 GET default permission **同为 "auto"**（t11-ask-default-01）——R00 断言「未设置时 ASK」系旧语义漂移，两内核一致偏离，如实记录不伪造。
5. **套件目标修正**：boundary/archive 目标加「无子会话」过滤 + 409 时带 childMode 重试（两内核同规则）；比较器规则对齐（archive 核心四断言；boundary 核心=保留+形状，A 侧显式删腿差记录 D-8）。
6. **A/B 线束终态（t11-ab-drill-fixes-06）**：25 任务 **parity=22 / differs=1（唯 session-mode A 腿 D-3 环境前置）/ recorded=2** —— 实质收敛。红链 t11-ab-drill-fixes-01..05 全保留。

### 2026-10-24 第五轮：D-3 模型替身扩腿（A 腿 catalog 注入）+ 归档族三叶绑定

1. **D-3 扩腿实施**（红链 t12-ab-drill-d3-01..06 全保留）：t03 桩供应商接入 A/B 线束（--read-path 必填实证）。A 腿注入路径三次演进：根 models.json（被启动期 syncModels 覆写，无效）→ provider-catalog.json（catalogVersion:2 必填实证；catalog→models.json 单向投影生效，模型面可用）→ 夹具会话补 model_change 条目 + Pi 叶链 parent 接续（真实会话由 SDK 维护）。**结果：A 腿模型可用性已解锁**（错误从「No models available」推进到「Could not restore model undefined/undefined」——session-restore-model-ref 链仍未通，旧栈对无冻结模型 ref 的会话 restore 响亮拒，系旧栈产品语义而非注入缺陷；**D-3 残余=旧栈 switch 需真实使用过的会话**，真实旧根的会话有真实 model_change）。B 腿注入实证回归（switch 500）→ 撤注入。
2. **归档族负路径与计数**：leaf.archive.negative（无效归档/恢复目标双内核显式拒 403|404；非法 maxAgeDays 双内核同行为 200+默认档——如实记录不判差异）；sessions.maintenance 记 cleanup 删除计数（fixture 超期归档 B 删 1 vs A 删 0——旧栈计数语义未对齐，D-9 登记）。
3. **叶绑定**：查看归档会话（43FFB75F252C）+ 恢复归档会话（31CDA283A11A）升级 full_original_behavior（归档正向全链双内核绿：入列/不可见/恢复/回可见 + 无效目标拒）。清理过期归档（B0F05BC32D62）降回注记（D-9 计数差）。session-mode 叶维持不可绑定（A 腿 D-3 残余）。
4. 门图：6 叶 full_original_behavior + 120 pending；gate-09 运行中。

### 2026-10-24 第六轮：D02 第二批三叶 + D-9 根因定位

1. **D-9 根因定位（已结案为语义差）**：旧栈 cleanup 计数条件=**文件 mtime**（server/routes/sessions.ts:2536 `stat.mtime.getTime() < cutoff`），Rust=**DB archived_at_unix_ms** 列。夹具归档文件的 mtime=生成时刻（新鲜）而 DB archived_at=夹具时间戳（2025-10）→ A 计 0 / B 计 1。真实旧根上两者近似一致（归档时 mtime≈归档时刻）；夹具放大了该语义差。**处置：如实记录为双内核语义差（mtime vs archived_at），不静默对齐任一侧**——产品裁决归 post-R08。
2. **三叶案例双内核全绿**（t13-ab-drill-batch2-02，29 任务 parity=24，真差异仅 D-9/session-mode）：
   - **重排置顶会话（2ED065BED168）**：正路径=先置顶两会话再整体重编号（reorderOk 双真——首跑红根因=未先置顶）；负路径=重复 id 400 拒（双内核同 400）。
   - **永久删除归档会话（52226FEA5997）**：归档→显式删除→归档列表消失→恢复接口不可找回（restore 409|404 各内核显式拒）。首跑 A 腿 409 红根因=旧栈要求**归档目录下 path**（isArchivedDesktopSessionPath），修 body 用归档行 path。
   - **跨会话搜索（A18187567AF9）**：phase=content 命中≥1（双内核）+ 空查询空结果（双内核）。附带差异如实记录：无效 phase 旧栈归一 200 vs Rust 严格 400（严格性差，非绑定面）。
3. **叶绑定**：三叶升级 full_original_behavior（门图 9 bound）；D-9 语义差随 maintenance/boundary 观测如实保留。

### 2026-10-24 第七轮：D02 第三批（pin 投影升级 + 归档工作台）+ D-10 新发现 + A06 解锁手册

1. **pin 叶升级（0D4F0EFC9DAE）**：补投影断言——置顶后列表行带置顶时间戳/顺序位（旧栈投影无 pinned 布尔只有 pinnedAt/pinOrder 字段，双内核同族；首跑红根因=误断言 pinned 布尔，改从投影字段推导）；取消置顶投影清除。断言1=无效目标 403|404 拒 + 清除。升级 full_original_behavior（五案例唯一身份）。
2. **归档工作台会话（2D4EBE0FF3C2）**：正路径=工作台身份会话归档入可恢复列表（双内核 row 命中 + 恢复保全）+ 负路径=无效目标 403|404 显式拒。升级 full_original_behavior。
3. **D-10 新发现（会话内查找未迁移）**：旧栈 `/api/sessions/find?path=&q=`（会话内文本查找，返回命中位置）在 Rust 无对应——`/lingxi/v1/sessions/find` 仅按 id 定位且**拒绝 q 键**（400 unknown query key；t14-ab-drill-batch3-01 实证 A 200+命中 vs B 400）。客户端亦无翻译（chat-find-actions 直连旧端点）。叶 4F1F69CCA760 不可绑定，如实登记 D-10（post-R08 迁移项）。
4. **A06 解锁手册**：`docs/rust-tauri/R08/A06_HOST_AUTOMATION_UNLOCK.md`——宿主 TCC 权限（辅助功能+自动化→System Events）授予步骤、单套件验证命令、解锁后重跑门的预期变化、以及"不得为转绿弱化断言"红线。语义面事实重申：a06 语义断言 100% 过，红仅退出/终态段。
5. 本轮 A/B 终态（t14-ab-drill-batch3-02）：32 任务 parity=26，真差异=D-9（maintenance/boundary）+D-3 残余（session-mode）+D-10（find）——全部已定名登记。

### 2026-10-25 第八轮：D02 第四批（消息页/概览/摘要三叶）读端口对拍

1. **读端口三叶案例**（t15-ab-drill-batch4-01..02 红链保留）：统一 leaf.read-ports 任务覆盖——消息页（旧栈客户端真实面 `/api/sessions/messages?path=&reconciliation=1`〔session-actions.ts:226——首跑红根因=误用 content/{id} 404，contentId 非会话 id〕vs Rust 统一历史读面 `/lingxi/v1/sessions/{id}/history?all=1`〔R06-T04〕）+ 历史概览（已翻译读端口 GET history-overview）+ 单会话摘要（已翻译读端口 GET summary）+ 负路径（无效会话三端口 403|404 显式拒）。**双内核全绿**（200/200/200 + 拒绝）。
2. **叶绑定**：阅读会话消息页（AA9EB3FD034D）、查看会话历史概览（CDBFE6616EB9）、查看单会话摘要（F0319FC4F93D）三叶升级 full_original_behavior（断言0=各读面返回、断言1=无效会话拒——案例唯一身份）。
3. **D-10 未迁移确认补证**：`/api/sessions/messages` 在 T01 绑定表为 UNMAPPED_REFERENCED_DUAL_BRANCH_ONLY（客户端双分支自管面，rust-chat-history.ts 直接消费旧路径语义）——不属 D-10；D-10 维持（`/api/sessions/find?path=&q=` 未迁移）。
4. 门图：14 叶 full_original_behavior + 112 pending；gate-12 运行中。

### 2026-10-25 第九轮：D02 授权目录四叶（收口后再挖掘的合法存量）

1. **收口审计发现存量**：终态审计时复核发现授权目录四叶（查看 98DDC94776F1 / 增加 604C64F4828A / 撤销 1E0D477755CD / 替换 73B631D77531）在现有 A/B 线束覆盖域内（读端口已翻译 + PATCH 写端口翻译既有）——**非 R09+ 新线束**，属 R08 范围合法增量。
2. **四操作案例**（t18-ab-drill-authf-01）：读（GET authorized-folders 已翻译读端口双内核 200）+ 增（add 真实目录 /tmp 双 200）+ 撤（remove 同目录双 200）+ 替换（set 空集合整体替换双 200 且结果为空）+ 负路径（add 不存在目录双内核 **同 400** 拒——验证真实目录语义在两端一致）。**全绿**。
3. **四叶升级 full_original_behavior**（断言0=各操作正路径、断言1=无效目录显式拒——案例唯一身份）；门图 18 bound + 108 pending；gate-13 运行中。

### 2026-10-25 第十轮：记忆开关两叶 + 快照叶如实留债

1. **记忆开关两叶**（51905CECDD1B 查看 / 9616596B5720 设置）：GET/PATCH /api/sessions/memory 已翻译端口双内核全绿（读 200+memoryEnabled / 设回原值 200+回读一致 / 无效会话 403|404 拒）。升级 full_original_behavior。
2. **提示词快照叶（FF4F06011B48）如实留债**：A 腿 500——合成会话无冻结快照 sidecar（需真实模型回合产生；B 腿 200）——产品侧合成前提，注记入图不绑定（t19-ab-drill-memsnap-01 实证）。
3. 门图 20 bound + 106 pending；gate-14 运行中。
