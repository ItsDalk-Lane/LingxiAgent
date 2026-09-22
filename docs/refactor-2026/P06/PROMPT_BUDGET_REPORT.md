# P06-T03｜提示词预算、缓存与必要上下文报告（PROMPT_BUDGET_REPORT）

日期：2026-09-22｜HEAD `93b8b7265`｜测试：`tests/p06-prompt-budget-and-invalidation.test.ts`（7 例绿，命令日志 `artifacts/refactor-2026/P06/logs/P06-T03-budget-invalidation.{out,err}`）。

## 1. 预算账本（口径：P00 PROMPT_BASELINE，estimateTextTokens 估算）

| 组件 | 内容 | P00 基线 | P06 实测 | 结论 |
|---|---|---|---|---|
| A1 | 系统提示词正文（golden zh） | 5194 B / 1747 tok | 5194 B / 1747 tok | 相等（golden 字节锁定） |
| A2 | 系统提示词正文（golden en） | 6114 B / 1529 tok | 6114 B / 1529 tok | 相等 |
| B1 | 按需目录引导（zh，engine toolCatalogIntroLine） | 172 B / 57 tok | 172 B / 57 tok | 相等 |
| B2 | 按需目录引导（en） | 186 B / 47 tok | 186 B / 47 tok | 相等 |
| C | PTC 入口 run_tools 描述 | 1684 B / 419 tok | 1682 B / 1658 chars | **≤ 基线**（-2 B；见 §2 计量备注） |
| **D（本轮新增）** | 常驻工具 schema（4 Pi 基础 + 3 目录桥，请求边界 name+description+parameters 估值） | 未计量（not_measured_yet） | 2420 tok（明细见下） | 登记进账本供 P07/P08 回归 |
| E（单列） | 动态用户资料（人格/记忆/样貌/档案） | not_measured_yet | T01 样本索引单列（desktop-main 基座 5062 B，含动态段） | 不入常驻预算 |

**常驻平台总预算 A+B+C 与基线相等或更低；未以截断人格/用户指令/资料达标**（人格与用户档案在所有开关变体下保留，tests/p06-feature-context-boundaries.test.ts 断言）。

**D 组件明细**（engine.buildTools 生产入口实测，defer 开启）：

| 工具 | 估算 tokens |
|---|---|
| exec_command | 1044 |
| edit | 440 |
| read | 328 |
| write | 253 |
| mcp_call | 144 |
| mcp_search_tools | 115 |
| mcp_describe_tool | 96 |
| **合计** | **2420** |

注：D 属请求边界工具 schema（进入请求体但不进入 system prompt 字符串）；按需工具 schema 不进请求（目录 manifest 为扩展消息，schemaRef 惰性），故不计入。

## 2. 计量备注（如实登记）

- C 组件 P00 记录 1684 B/1660 chars，本轮实测 1682 B/1658 chars。`lib/tools/ptc-tool.ts` 自研究 SHA `8037fae7` 起**零改动**（git log 仅一个先于基线的提交 59c131276），差异为 P00 研究期计量口径差（2 字节），非本阶段变更。预算规则按「不得超过」判定：通过。
- 本阶段唯一生产改动（schema-validator message 字段化）不进入任何常驻文案组件；A/B/C 不受影响。

## 3. 缓存与开关失效契约（实测）

| 契约 | 机制 | 证据 |
|---|---|---|
| 记忆 master 关闭立即失效 | `setMemoryMasterEnabled(false)` 重建 `_systemPrompt`，无 memory_context 残留；人格/档案保留 | T03 测试例 5 |
| per-session 记忆开关隔离 | session 快照按自己开关单独构建；master 缓存不受影响；开关差异只落在 memory_context 段（静态前缀一致） | T03 测试例 6 |
| skill 集变化重建 | `setEnabledSkills` 重建 master prompt（skills 清单本身由 SDK 注入，不进基座） | T03 测试例 7 |
| prompt 跨会话冻结 | buildSessionPromptSnapshot per-session 冻结；恢复会话用当时快照（P05 §四十七契约） | T01 捕获 + session-coordinator.test.ts |
| 权限/工具 generation | 撤销按 lifecycleGeneration 拒绝（TARGET_REVOKED）；漂移检测续签不越权 | tests/tool-lifecycle-revocation.test.ts（映射，未重复） |
| 压缩后必要上下文 | `<skill-recall>`/技能重读/工具结果关系保护 | tests/session-compactor*.test.ts、session-compactor-skill-recall.test.ts（映射） |
| provider 缓存亲和 | providerCacheAffinityKey（fork lineage）+ cache prefix 契约漂移检测 | lib/llm/provider-cache-affinity.ts + tests/cache-prefix-contract-drift.test.ts（映射） |

**缓存不跨权限泄漏**：prompt 快照冻结的是「装配时的合法内容」；执行面授权由分类器/网关逐调用裁决，与快照无关（评测 F-26..F-29 + 撤销套件）。

## 4. 压缩与必要上下文（要求 4）

压缩后工具结果关系/来源身份/当前任务信息的保护由既有压缩套件承担（session-compactor / cache-preserving-compaction / skill-recall）；本阶段未改压缩器，未引入无界读回全历史的路径（P05 HISTORY_PARITY 的 O(K+|Dpage|) 有界读取契约继续有效）。

## 5. 结论

相同 fixture/配置下常驻预算不高于基线；动态内容差异可解释（开关差异只落在 memory_context）；缓存失效按契约立即生效。预算回归已纳入默认测试集（新测试文件随 `npm test` 执行）。
