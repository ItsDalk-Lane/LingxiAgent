# P06-T04｜人格、记忆、Skill 与知识功能兼容报告（FEATURE_CONTEXT_COMPAT）

日期：2026-09-22｜HEAD `93b8b7265`｜新测试：`tests/p06-feature-context-boundaries.test.ts`（3 例绿，日志 `artifacts/refactor-2026/P06/logs/P06-T04-boundaries.{out,err}`）＋ T01/T03 测试中的边界断言。本阶段**零业务设计变更**：人格/记忆/Skill/知识按现行机制接入，仅验证接线。

## 1. 人格（persona）

- **只读使用**：装配多次（3×4 变体）后 persona 模板/user.md/memory.md 内容与 mtime 均不变（T04 测试例 1）。`{{userName}}` 替换只发生在渲染输出，模板文件不被写回。
- **不被改写**：记忆开关×subagent 四变体下人格段字节一致（T04 测试例 2）；人格优先级/开关链路未动。
- **MOOD/输出约定保持**：思考区块名（MOOD/PULSE/沉思）由 tests/yuan-metadata.test.ts 锁定；真实模板触发锚（「每条新消息后的第一段输出以 MOOD 区块开头」+ `<mood>` 标签）由 tests/yuan-trigger-anchor.test.ts 锁定；输出侧保留协议（ThinkTag/Mood parser → normalizer）是 P05 锁定面，本阶段零触碰。
- **样貌**：appearance 段按模型视觉能力条件注入，subagent 变体不注入（T01 捕获测试）。

## 2. 记忆（memory）

- **开关语义**：master 开关立即重建（无残留）；per-session 开关只影响该 session 自己的快照，不污染 master 缓存（T03 测试例 5/6；core/agent.ts:1295 注释契约保持）。
- **注入边界**：记忆三段（rules/tenets/longterm）在 cache 分界线之后（动态尾）；subagent 不携带长期记忆与 pinned（T01/T04 断言，与 agent.ts:1536 注释契约一致）。
- **未新增查询拆解/研究代理**：记忆检索仍走 search_memory 等工具面（CORE/STANDARD 目录），无 prompt 内嵌检索指令。

## 3. Skill

- **清单注入**：只由 Pi SDK `formatSkillsForPrompt` 生成 `<available_skills>` 段（#399 修复后 Lingxi 基座不再自行拼接——T04 测试例 3 断言基座无清单）；常驻规则只有「读全文再动手」的使用纪律（platform.skill-usage）。
- **文本不授权文件执行**：SKILL.md 内容经 read 工具读取，install_skill 有安全审查（tests/install-skill-safety-review.test.ts）；技能不增加授权（learn-skills 段明文）。
- **启用范围/审查机制**：skillsResult per-session 冻结 + 指针快照；源删除显式 unavailable 诊断、不悄悄混入（tests/session-skill-snapshot.test.ts "omits pointer skills whose source file was removed"）。
- **subagent 复用批准隔离**：subagent 工具访问沿 access+父档策略（tests/subagent-tool-policy.test.ts）；blocked 工具集拦截写侧知识/记忆操作（SUBAGENT_BLOCKED_TOOLS）。

## 4. 知识（knowledge）

- **scope 冻结**：knowledge_outline/grep/search 的 scopeId 冻结集合、跨 session/伪造拒绝、scope 外源整单拒绝、subagent 继承父 scope——tests/knowledge-agent-tools.test.ts 全套（映射，本阶段未改知识面）。
- **基座不含知识材料**：canonical 基座在所有变体下无 knowledge_* 引用、无知识来源 provenance 段（T04 测试例 3）——知识能力经工具面按需触达，不混入未选择来源。
- **资料不足/索引不可用**：未解析源进 unavailableSources 显式单列（不静默省略、不整单失败）——既有套件保护。

## 5. 与 P05 语义面的关系

本阶段一切上下文注入不改消息语义链（normalizer 唯一裁决/streamId/分页投影/资源授权——P05 NEXT_STAGE_HANDOFF §9 锁定面）。发现该面问题应登记回 P05 范畴；本阶段未发现需登记项。

## 6. 结论

已采纳产品能力（人格/MOOD/记忆/Skill/知识/subagent/Bridge）不丢；已排除功能（专用子代理目录、独立研究代理、复杂知识规划）未被重新加入。开关/来源/隐私测试全部绿。
