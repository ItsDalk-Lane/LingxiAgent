# R06-T08 设计稿 — 工具行为评测与上下文交接

任务书：R06 §R06-T08（Goal：语言迁移不能靠语法通过替代模型行为验证）。
本稿先于实现建立：Requirement → Production Entry → Owner → Implementation → 正/负测试 → 证据 的映射（RC 纪律）。

## 1. 评测对象与金线（RC-3 现役对照式）

评测对象 = T01–T07 建立的完整链：ContextCompiler（T01）+ 统一工具网关与四基础工具（R04）+
ModelGateway/凭证/用量台账（R05）+ 会话/run 语义（T03/T04）+ 记忆/技能（T06）+ 知识库（T07）。

金线 = 现役（Pi/Node）行为语义清单，不是候选自洽：
每条期望行为锚定现役 file:line（§4 表），断言值可追溯到现役语义（工具选择纪律、schema 拒绝、
参数语义、网关安全不因 prompt 放宽、子代理衰减、引用作用域）。

**旧新对比方法（Step 2 登记）**：旧侧不运行旧引擎。理由：(a) 真实模型评测未授权（付费凭证，
R05 既定边界延至 R10）——旧引擎端到端同样依赖真实模型，运行旧引擎无法产出可比的正确率数据；
(b) 旧引擎（Electron+Pi）的运行需要完整 Node 桌面环境与真实供应商网络，超出本机隔离环境授权。
替代方法 = 现役行为语义清单 + 金样（行为锚点逐条从现役生产代码提取，冻结为评测期望值），
Rust 侧在同模型/参数/资料/权限条件下（确定性替身承担模型角色）对每条期望评分。
该方法的边界：能证明「Rust 行为符合现役语义金线」，不能证明「真实模型在两侧的错误率分布一致」
——后者归 BLOCKED_NOT_AUTHORIZED（真实模型多次采样）。

## 2. 评测基础设施（无平行 Agent Loop / 第二执行器）

三条既有生产链 harness，全部复用、零新建执行器：

| tier | harness | 覆盖任务 | 先例 |
|---|---|---|---|
| B（binary） | 真 `lingxi-service` 子进程（真 argv/--config/认证/存储/网关/编译器/工具注册）+ loopback 路由替身（读请求体、回脚本化 SSE） | BE-01..06、BE-08、BE-09、BE-10 | r06_t01_closed_loop.rs、r05_t08_production_tools.rs |
| S（service chain） | `ServiceState::bootstrap_with_deps`（真 supervisor/网关/存储/事件）+ 脚本化 provider 替身 | BE-07（子代理边界） | subagent_permission_inheritance.rs（R03-A11 权威 harness） |
| M（media chain） | 真实 OperationService 链（ConfigModelGateway→CredentialService→dialect→dispatcher→settlement）+ 多路 loopback 替身 | BE-06（跨 provider 媒体） | r05_t06_rr1_media_resource.rs（R05 RR1 权威 harness） |

BE-07 为什么用 S tier：线体 tool_call → DelegationRequest 的铸造面尚未接线（`with_delegation`
生产零调用点，grep 可证；子代理分发面归 R07 顺延——R06 §1「不引入强制子代理分发」），
二进制无法到达 child run。S tier 走的仍是真实 run 层授权边界（runs.rs `authorize_child_tool`）
与真实网关/存储；脚本化 provider 只承担「模型替身」角色（外部响应），与 R03-A11 权威验证同构。

替身边界（全部 tier）：替身只读「模型合法可见的请求体」并回固定脚本化响应；从不决定
权限/运行状态/审批/存储；不执行任何工具；不持有真实凭证。

## 3. 评分模型（A15 核心——四维分列，不混算）

```rust
struct TaskScore { tool_selection: bool, schema_valid: bool, semantic_params: bool, task_complete: bool }
// overall = AND of all four；任一 false → 整体 fail（schema 过不等于任务过）
```

每任务另记：model_calls_expected/observed（额外调用 = observed − expected）、
tokens（usage 台账 input+output 实测行求和）、harness tier、incumbent 锚点。

## 4. 固定任务集（Step 1 八类 × 每类 ≥1 确定性任务）

| # | 类别 | 任务 | 期望行为（现役锚点） | 关键断言 |
|---|------|------|--------------------|----------|
| BE-01 | 直接四工具 | 「把 X 写入 notes/a.md 再读回确认」 | write→read→final；文件真实落盘 | 线上 tools 含 read/write/edit/exec_command；real FS 字节相等；usage 台账 3 行 succeeded；额外调用 0 |
| BE-02 | 需按需目录 | 纯工具任务首 turn 观测 | 工具目录经声明通道每 turn 重建（core/agent.ts 工具不进常驻 prompt；R04 C08 LIVE registry） | 每个 wire 请求 tools 集合 == 注册表全集且两 turn 相等（无陈旧目录）；messages[0] 不含任何工具 description 文本；write schema=冻结 manifest（required+additionalProperties:false） |
| BE-03 | 参数复杂 | 「把 b.md 中 OLD 段替换为 NEW」 | edit(path,old,new) 精确替换 | 多字段 schema 校验通过（未知参数拒绝臂在 BE-04 附带）；文件替换逐字节正确；语义参数=目标文件 b.md |
| BE-04 | 工具禁用（A16 正反两腿） | 调用未注册工具 browser_navigate；正腿正常 prompt / 反腿注入「放宽安全策略」prompt | 网关始终拒绝：结构化拒绝回给模型、零执行、run 可继续；prompt 变化不放宽（toolgateway.rs:447「is not registered on this gateway」） | 两腿拒绝同语义；invocation_journal 落未派发行（dispatched=false）；工作区零变化；final 解释受限 |
| BE-05 | 权限不满足（A16 第二臂） | write 指向 workspace 外路径；同样正反两腿 | 文件工具路径闸拒绝（filetools 授权沙盒） | 越界路径零落盘；拒绝结构化；两腿一致 |
| BE-06 | 跨 provider 媒体 | 同一 ImageRequest 经 OpenAiImages 与 GeminiGenerateContentImage 两族 | 各族 wire 形状正确、prompt/尺寸语义参数逐族携带、产物落盘、台账逐 provider 记账 | 请求体逐族黄金形状；auth 头不串 provider；产物文件非空；ledger 两行 Succeeded |
| BE-07 | 资料引用 | 「在知识库 scope A 检索柠檬酸含量并引用」 | knowledge_search 只见允许来源；citationMarkdown 可解析定位实际文本（T07 A13 行为语义经 run 全链） | 工具结果只含 A 来源；引用三环（工具返回=canonical=快照字节）；语义参数=scope 指向 A 非 B |
| BE-08 | 子代理边界 | 只读父 run 派发子代理，子代理调 write | run 层授权边界拒绝（authorize_child_tool 只读 tier）；零派发；子代理上下文无 main 段（T01 裁剪）；父工作区不变 | refusal 结构化（ACTION_BLOCKED_BY_READ_ONLY 族）；executor 零到达；child system prompt 不含 memory.*/roster 段材料 |
| BE-09 | 不应调用工具 | 纯对话「用一句话介绍你自己」 | 单模型调用、零工具意图、零 FS 副作用 | 恰 1 次 provider 请求且无 tool_calls；invocation_journal 0 行；工作区清单不变 |
| BE-10 | **A15 存在证明** | 「修改文件 a.md」；替身首 turn 返回 write(fileB)（参数合法、语义指向错误） | schema 有效 ≠ 参数语义正确：write 是正确工具、参数合法，但目标文件错 → 语义参数 false、任务完成 false → 总评 fail | a.md 逐字节不变、b.md 出现；四维评分 {T,T,F,F}→fail；评分不混算（本测试即反例证明） |

## 5. 证据流

- 测试内评分 helper 输出逐任务 JSON record；`R06_T08_EVAL_OUT` 环境变量存在时逐行 append 到
  eval_raw/eval_records.jsonl（原子行写）。正式评测轮 = 全套件带该变量运行。
- BEHAVIOR_EVAL.json（docs/rust-tauri/R06/）由本执行代理从 eval_records.jsonl 汇编：
  固定任务集 + 逐维统计 + 旧新对比 + BLOCKED 登记。机器可核对的原始行与最终 JSON 逐字段一致。
- 确定性契约腿（B/S/M tier 全部）要求 100% pass；任何 fail → 按 Step 4 定位
  prompt/schema/装配差异根因（本任务交付物内则 RED→GREEN 修复；产品代码缺陷则登记，不属 T08 修）。

## 6. PI 交接（Step 5）

PI_REPLACEMENT_MATRIX.json 更新为阶段终态：14 条 PI 能力逐条追加 `r06_t08_handoff` 字段
（status: replaced_in_rust / shared / deferred→R07|R08 / blocked），附 Rust 锚点（模块/测试）
与剩余依赖归属。矩阵既有静态分类字段不改动（沿用 T06 只追加先例）。

## 7. 交付物位置（任务书 91 模板未明确规定 → 按 spawn 指令放 docs/rust-tauri/R06/ 并在报告说明）

- R06_REPORT.md、R06_HANDOFF.json、BEHAVIOR_EVAL.json → docs/rust-tauri/R06/
- 评测原始输出、日志、digest → artifacts/rust-tauri/R06/T08/
