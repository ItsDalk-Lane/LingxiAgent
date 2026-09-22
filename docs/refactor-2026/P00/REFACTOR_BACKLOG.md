# REFACTOR_BACKLOG — 后续阶段最小改动清单（P00-T07）

版本：1.0｜依据：P00 三张地图 + 基线实测。原则：已有正确实现列「仅回归」；每个缺陷唯一主责阶段；跨阶段只引用依赖。

## P01｜模块边界、Pi 适配与严格类型入口（主责清单）

| # | 事项 | 依据 | 动作 | 受影响消费者 |
|---|---|---|---|---|
| P01-1 | **UNCHANGED_VERIFIED**：Pi 集中适配 | lib/pi-sdk/index.ts（唯一 SDK 入口纪律+深路径补丁注释）；tests/pi-sdk-import-boundary | 仅回归（0.86.0 锁定，无升级） | session-coordinator:2214、agent-executor:552、bridge-session-manager:1305、executeIsolated:8385 |
| P01-2 | LINGXI_HOME 解析双实现统一 | OWNERSHIP_MAP 🔴：shared/hana-runtime-paths.cjs:13 vs cli/local-server.ts:5-15 | cli 侧改为 import shared 实现（最小改动） | cli/local-server 的连接解析 |
| P01-3 | 死别名清退（packages/ 已拆除） | vitest.config.js @hana/*、tsconfig.test.json @lingxi/plugin-* | 核实无引用后删除别名；跑全量 typecheck+test | vitest/tsconfig.test |
| P01-4 | strict 核心范围建立 | tsconfig.node.json strict=false（S03 现状） | 按任务书建立可验证 strict 入口（新契约先行），不做全库 strict | 后续各阶段新契约 |
| P01-5 | 证据目录 lint 策略决策 | F2 附注：eslint . 扫未跟踪文件 | 决定 artifacts/refactor-2026 是否加入 eslint ignores（工程配置，非生产行为） | P01-P08 证据工具 |

## P02｜运行身份、生命周期、取消与恢复

| # | 事项 | 依据 | 动作 | 受影响消费者 |
|---|---|---|---|---|
| P02-1 | taskId 统一铸造厂 | OWNERSHIP_MAP 🔴：5 种自铸格式（subagent:55/workflow:207/rewind:132/speech:86/image-task-runner:15） | 统一 id 铸造（含 attempt/generation 语义），TaskRegistry 校验 | TaskRegistry、loop 守恒、engine 投影 |
| P02-2 | 媒体任务双记账收敛 | core/media/task-store.ts vs TaskRegistry | 单一权威+另一侧改只读投影 | media UI、activity |
| P02-3 | run/终态/取消契约测试扩展 | chat.ts:868/910/1015 已 exactly-once；abortSession:5477 单入口 | 仅补场景测试（进程退出重启恢复≥1 例真实重启） | 前端 run 状态、usage |
| P02-4 | **UNCHANGED_VERIFIED**：ephemeral 执行隔离 | hub.send 路由表+executeIsolated deny_on_prompt | 仅回归 | scheduler/channel-router/dm-router |

## P03｜工具目录、参数契约与统一执行边界

| # | 事项 | 依据 | 动作 | 受影响消费者 |
|---|---|---|---|---|
| P03-1 | **UNCHANGED_VERIFIED**：四常驻+按需目录 | shared/tool-categories.ts:103+三重启动断言；tests 覆盖 | 仅回归（不回到全工具常驻） | 全部会话工具面 |
| P03-2 | **UNCHANGED_VERIFIED**：网关与边界检查 | gateway prepared/generation/capability；AST 白名单脚本 | 仅回归 | engine.buildTools、PTC 子调用 |
| P03-3 | 修复 F2 lint 3 error | run-code-tool.ts:118-119、security-scan-tool.ts:191（恒真条件改明确语义） | 最小生产修复（该阶段授权内） | CI lint 门禁 |
| P03-4 | 外装扩展边界合成适配器测试 | 任务书 T02-3 | 新增测试（不 mock 网关本体） | MCP/plugin 路径 |

## P04｜模型、凭证、流式请求与用量观测

| # | 事项 | 依据 | 动作 | 受影响消费者 |
|---|---|---|---|---|
| P04-1 | 旁路观测完备性核查 | ENTRYPOINT §3：embedding/rerank、call-text、summarizeTitle | 核实全部经 observed-model-call 包装+测试 | 观测库、usage |
| P04-2 | **UNCHANGED_VERIFIED**：mc_/ma_/mt_ 铸造与 trace 规则 | model-call-identity 唯一铸造厂；trace-scope 规则+21 测试 | 仅回归 | 观测、历史 modelCallRef |
| P04-3 | 真供应商验证 | 需用户授权凭证/预算 | BLOCKED 项管理（不默认消耗真实账户） | provider-compat |

P04 执行注记（2026-09-22，详见 docs/refactor-2026/P04/）：P04-1 已核实——全部旁路（embedding/rerank、call-text/sample-text 总线、summarizeTitle、vision/media/speech/probe/diary）均经统一观测包装并有测试（MODEL_CALLSITE_MATRIX.json 全矩阵）；P04-2 回归全绿（28+80 例含 trace 复用 21 例）；P04-3 维持 BLOCKED（无凭证/预算授权），补偿证据为本地 loopback witness 真实 HTTP 全协议族覆盖（OPERATION_COVERAGE_MATRIX.json）。

## P05｜消息语义、历史恢复、资源与数据兼容

| # | 事项 | 依据 | 动作 | 受影响消费者 |
|---|---|---|---|---|
| P05-1 | **UNCHANGED_VERIFIED**：规范化事件+旧正文兼容并存 | assistant-event-normalizer；S10 | 退出兼容前核查全部消费者（任务书要求） | ws 协议、前端 |
| P05-2 | knowledge research 残留表退出决策 | SCOPE excluded（兼容残留） | 评估旧数据读取路径后决定保留/退役（退役属需授权治理变更） | knowledge-store 迁移 |
| P05-3 | 会话双身份（sess_ vs SDK UUID）引用约束 | OWNERSHIP_MAP ⚠️ | 文档化为契约+防误用测试（不合并身份） | 全部 sessionPath 换算点 |

## P06｜上下文、提示词、记忆与能力集成

| # | 事项 | 依据 | 动作 | 受影响消费者 |
|---|---|---|---|---|
| P06-1 | payload 捕获补全提示词预算 | PROMPT_BASELINE not_measured_yet | 用既有 payload 捕获基础设施按同 fixture 截获最终 request 分解 | 预算回归 |
| P06-2 | 人格/记忆/资料边界评测 | golden 双语等价测试已存在 | 扩展动态资料组分测试 | agent 系统提示词 |

## P07｜性能

| # | 事项 | 依据 | 动作 | 受影响消费者 |
|---|---|---|---|---|
| P07-1 | W3/W4 补全 + 30 次口径全量 | BENCHMARK_PROTOCOL 实验受限项 | 用 scenario harness 本地协议 server 执行 | 性能门禁 |
| P07-2 | 启动/历史基线复测对照 | 本 P00 样本为基线 | 同环境成对比较（bench-compare 规则） | — |

## P08｜旧路径退出、全产品回归与发布准备

| # | 事项 | 依据 | 动作 | 受影响消费者 |
|---|---|---|---|---|
| P08-1 | F1 封印推进（前置：用户授权+全量门禁） | BASELINE_FAILURES F1 | 按 PROGRESS.md 流程；不扩白名单 | seal 门禁 |
| P08-2 | F3 四平台 CI 证据取回 | gh/proxy 环境修复后 | 列出并归档真实运行记录 | 发布门禁 |
| P08-3 | skills2set 残留清理（授权后） | SCOPE residue | 删除 pycache-only 目录 | 首启技能同步 |

## 第一条纵向切片（P01 先稳定边界）

**桌面/HTTP 输入 → Pi 适配（createAgentSession）→ 只读工具（read）→ 模型继续 → 历史保存（session JSONL+manifest）→ 界面恢复（history-read 投影）**

- 现状：链路已存在且收敛（ENTRYPOINT E-DESKTOP+X-TOOL-EXEC）；P01 动作=把该链经过的模块边界（pi adapter、LINGXI_HOME、别名清退）纳入 strict 入口+契约测试；不重写任何一环。
- 已有测试直接复用：tests/tool-invocation-gateway、tests/history-run-outcome-edges、tests/desktop-input-history-fixture（helper）。

## 需用户决策的真语义冲突（其余由源码+既定需求解决，不转交用户）

1. F1 封印推进时机与授权（治理流程，见 BASELINE_FAILURES）。
2. knowledge research 兼容残留的最终去向（保留只读兼容 vs 授权退役）。
3. P04 真供应商验证的凭证与预算授权（如需）。
