# P06-T01｜最终请求装配来源地图（CONTEXT_ASSEMBLY_MAP）

日期：2026-09-22｜坐标：HEAD `93b8b7265`（工作区改动见 P06_REPORT §差异）｜口径：当前源码实测 + 确定性捕获测试 `tests/p06-final-request-assembly.test.ts`（5 例全绿，命令日志 `artifacts/refactor-2026/P06/logs/P06-T01-assembly-capture.{out,err}`）。

## 0. 结论摘要

1. **单一 canonical 装配成立**：`Agent.buildSystemPromptArtifact`（core/agent.ts:1552）是唯一基座拼装点；`buildSystemPrompt` 只是 `artifact.text` 的取值 API（agent.ts:1541）。桌面主会话、subagent、Bridge owner、后台(automation/巡检/cron)全部消费同一函数，无第二套拼装。
2. **最终请求边界**：Pi 真实路径的 streamFn context 只带 `{messages, tools}`（pi-agent-core `createContextSnapshot`），system prompt 以 `messages[0]` 的**结构化 sections**（preamble/addendum/project_context/skills/cwd）进入请求；渲染文本 = sections 按 `"\n\n"` 连接。观测库 `semantic_request` payload 捕获同一 messages 数组（`lib/pi-sdk/model-call-stream-observer.ts:288-298`）。
3. **捕获测试证明**：结构化 `preamble` 与 canonical `artifact.text` **逐字节相等**；Lingxi 基座/append 各段/skills 目录/project_context 在渲染后的最终请求中各出现且仅出现一次；witness（truth oracle）收到的 system 消息与观测捕获一致。
4. **无过时工具名**：提示词正文中出现的 27 个工具名全部对应真实注册源（shared/tool-categories.ts 或 core/tool-catalog-bridge.ts `BRIDGE_TOOL_NAMES`），无虚构能力（核验命令见 §5）。

## 1. 装配链（从 canonical 到最终 request）

```text
Agent.buildSystemPromptArtifact(options)          ← 唯一基座拼装（20 个 provenance section）
  │  options: forSubagent / forceMemoryEnabled / forceExperienceEnabled / targetModel
  ├─ 桌面主会话：core/session-coordinator.ts:2067-2121
  │    快照 = buildSessionPromptSnapshot({systemPrompt, appendSystemPrompt, skillsResult,
  │            agentsFilesResult, systemPromptProvenance})（core/session-prompt-snapshot.ts:49）
  │    append = buildAppendSystemPromptSnapshot（session-coordinator.ts:837）：
  │      base append + providerPromptPatches + makeBackgroundTaskPrompt(有 deferred result store 时)
  │      + formatWorkspaceScopePrompt + buildWorkspaceInstructionPrompt
  │    → createPromptSnapshotResourceLoader（session-prompt-snapshot.ts:85）→ createAgentSession
  ├─ subagent：session-coordinator.ts:8339-8345（forSubagent:true，同装配跳过记忆/团队）
  ├─ Bridge owner：core/bridge-session-manager.ts:894-921
  │    buildSystemPrompt(forceMemoryEnabled: master) + appendBridgePromptLine
  │    + workspace scope/instructions → buildSessionPromptSnapshot
  ├─ Bridge guest：bridge-session-manager.ts:923-931
  │    yuanPrompt + publicAgentsMd + contextTag + bridge 行；skills/agentsFiles 置空
  └─ 后台（automation/巡检/cron/频道）：session-coordinator.ts:8347-8349
       targetAgent.systemPrompt（master 缓存，由 buildSystemPrompt 构建，agent.ts:545/988）
       ↓ 全部汇入
Pi SDK AgentSession._rebuildSystemPrompt（agent-session.js:773）
  = buildSystemPromptSections({customPrompt: 基座, appendSystemPrompt, contextFiles, skills,
      cwd, selectedTools, toolSnippets})   ← customPrompt 非空 → SDK 自带 preamble/rules/docs 全部跳过
最终 request：messages[0] = {role:"system", sections:{preamble, addendum?, project_context?,
  skills?, cwd}}；渲染 = sections.join("\n\n")
```

**最终请求里的段序**（渲染后）：基座(preamble，无标签) → `<addendum>`(append 各段) → `<project_context>`(agentsFiles) → `<skills>`(SDK `formatSkillsForPrompt`，含 `<available_skills>`) → `<cwd>`。

## 2. 逐段来源表（canonical 基座，zh/en 同构）

条件=注入开关；动态性=是否自动漂移；缓存性=静态前缀/动态尾部分界（cache 分界线，core/agent.ts:1848）。

| # | section（source.id） | category | 来源 | 条件 | 动态性 | 缓存性 |
|---|---|---|---|---|---|---|
| 1 | platform.intro | platform_instruction | 硬编码双语常量 | 恒注入 | 静态 | 前缀 |
| 2 | platform.environment | platform_instruction | `getPlatformPromptNote({platform})`（core/platform-prompt.ts） | platform 有注记时 | 随 OS | 前缀 |
| 3 | user.profile | user_profile | `user.md`（resolveUserName 显式覆盖→全局 preferences→语言兜底） | 恒注入（名字行）；user.md 有内容时追加正文 | 事件驱动 | 前缀 |
| 4 | persona | persona | yuan 模板（`lib/yuan/<type>.md`）+ identity + AGENTS.md（`this.personality` getter） | 恒注入 | 事件驱动 | 前缀 |
| 5 | agent.appearance | persona | 样貌 summary（`readAgentAppearanceProfileResource`） | `!forSubagent && _canInjectAppearancePrompt(targetModel)`（模型视觉能力） | 事件驱动 | 前缀 |
| 6 | platform.output-discipline | platform_instruction | 硬编码 | 恒注入 | 静态 | 前缀 |
| 7 | platform.tool-discipline | platform_instruction | 硬编码（直调/目录桥接双路协议） | 恒注入 | 静态 | 前缀 |
| 8 | platform.session-files | platform_instruction | 硬编码（fileId/label/stage_files 交付） | 恒注入 | 静态 | 前缀 |
| 9 | platform.ui-context | platform_instruction | 硬编码（current_status 的 ui_context） | 恒注入 | 静态 | 前缀 |
| 10 | platform.subagent-collaboration | platform_instruction | 硬编码 + `PROACTIVE_SUBAGENT_EXPERIMENT_ID` 实验开关 | `!forSubagent` | 静态+实验开关 | 前缀 |
| 11 | platform.computer-use | platform_instruction | 硬编码 | `!forSubagent` 隐含 + `_isComputerUseAvailableForThisAgent()` | 能力驱动 | 前缀 |
| 12 | platform.action-discipline | platform_instruction | 硬编码（并行/参数自纠/授权边界） | 恒注入 | 静态 | 前缀 |
| 13 | platform.web-tool-priority | platform_instruction | 硬编码 | 恒注入 | 静态 | 前缀 |
| 14 | platform.learn-skills | platform_instruction | 硬编码 | `learn_skills.enabled && allow_github_fetch`（全局 preferences） | 配置驱动 | 前缀（配置内稳定） |
| 15 | platform.skill-usage | platform_instruction | 硬编码（读全文再动手/`<skill-recall>` 压缩互引） | 恒注入 | 静态 | 前缀 |
| 16 | agent.roster | agent_roster | `_formatTeamRoster`（其他 agent 名单） | `!forSubagent && roster 非空` | 事件驱动 | 前缀 |
| 17 | memory.rules | memory_context | 硬编码双语 | `memoryEnabled && !forSubagent && (hasMemory‖tenets)` | 静态 | **动态尾** |
| 18 | memory.tenets | memory_context | `memory/tenets.json` active 条目 | 同上 && tenets 非空 | 后台 compile 漂移 | **动态尾** |
| 19 | memory.longterm | memory_context | `memory/memory.md` | 同上 && memory 非占位 | 后台 compile 漂移 | **动态尾** |
| 20 | session.time | session_instruction | `Intl.DateTimeFormat`（用户时区偏好） | 恒注入 | 每次构建漂移 | **动态尾** |

**基座之外、最终请求内的段**：

| 段 | 来源 | 条件 | 备注 |
|---|---|---|---|
| `<addendum>` | base append（engine loader 默认 []）+ providerPromptPatches + `makeBackgroundTaskPrompt`（session-coordinator.ts:816）+ workspace scope/instructions | deferred result store 存在时注入后台任务说明；workspace 有范围时注入 | per-session 冻结 |
| `<project_context>` | resourceLoader `getAgentsFiles().agentsFiles`（`{path,content}` 列表） | 快照有 agentsFiles 时 | SDK `renderProjectContext` |
| `<skills>` | resourceLoader `getSkills().skills` → SDK `formatSkillsForPrompt` | selectedTools 含 `read`/`bash` 且 skills 非空 | 只出现一次（#399：Lingxi 不再自行拼接，agent.ts:1713 注释） |
| `<cwd>` | SDK cwd | 恒注入 | 渲染为正斜杠路径 |

## 3. 变体差异矩阵（同一装配的条件分支，非第二实现）

| 变体 | 入口 | 与桌面主会话的差异 | 捕获证据 |
|---|---|---|---|
| 桌面主会话 | session-coordinator.ts:2067 | 全量 20 段 + append/skills/project_context | 测试例 1：preamble 逐字节==artifact.text；各段各一次 |
| subagent | session-coordinator.ts:8339 | 无 memory_context(x3)、agent.roster、agent.appearance、subagent-collaboration、computer-use 段；**用户档案保留**（现行设计，见 T04） | 测试例 2 |
| Bridge owner | bridge-session-manager.ts:894 | 基座=buildSystemPrompt(master 记忆开关) + bridge 行；append 增 workspace | 测试例 3 |
| Bridge guest | bridge-session-manager.ts:923 | 基座=yuan 模板+publicAgentsMd+contextTag+bridge 行；**无用户档案/记忆/样貌/skills/agentsFiles** | 测试例 3（隐私边界断言） |
| 后台(automation/巡检/cron) | session-coordinator.ts:8347 | 基座=master `agent.systemPrompt` 缓存（同一 buildSystemPrompt 产物） | 测试例 4 |

## 4. 过时工具名 / 重复规则 / 冲突指令核查（T01 第 3 项）

- **工具名核查**：提示词正文（platform.tool-discipline / session-files / ui-context / subagent-collaboration / computer-use / action-discipline / web-tool-priority / learn-skills / skill-usage 段）出现的工具名逐一对照注册源：27/27 全部真实（read/write/edit/exec_command/grep/find/ls/file/materialize/stage_files/current_status/security_scan/subagent/subagent_reply/subagent_close/workflow/todo_write/web_search/web_fetch/browser/computer/install_skill/ask_user/stop_task/run_tools 在 shared/tool-categories.ts；mcp_search_tools/mcp_describe_tool/mcp_call 在 core/tool-catalog-bridge.ts `BRIDGE_TOOL_NAMES`；computer 的 start/list_apps 动作在 lib/tools/computer-use-tool.ts）。**结论：无过时工具名、无虚构能力**。
- **重复规则核查**：output-discipline（任务结束正文交代）与 action-discipline（排队/运行≠完成）语义互补不重叠；tool-discipline 与 skill-usage 各管一层（调用协议 vs 读全文顺序），后者的"按需工具遵循上述调用协议"显式回指，无第二份 schema 文案。skills 注入只有 SDK 一处（#399 修复后），Lingxi 侧不重复拼接。
- **冲突指令核查**：未发现互相冲突的指令。已审阅文案（golden 双语锁定）按任务书不是默认删除对象，本阶段不改写。
- **run_tools（PTC 入口）**：在 OPTIONAL 目录（P00 基线组件 C 为其描述常量），经 mcp_call 触达，不占直挂面（tests/ptc-engine-assembly.test.ts 已锁定）。

## 5. 样本索引（脱敏：只含来源与长度）

运行 `npx vitest run tests/p06-final-request-assembly.test.ts`（stdout `P06_SAMPLE_INDEX`，归档于 logs/P06-T01-assembly-capture.out）：

| 变体 | canonical 基座(bytes) | append workspace-scope(bytes) | skills listing(bytes) | 最终 system(bytes) | provenance 段数 |
|---|---|---|---|---|---|
| desktop-main（zh fixture，含样貌/记忆/团队/技能/上下文文件） | 5062 | 173 | 625 | 6178 | 20 |

注：fixture 为合成数据（受控临时目录），长度用于结构对照，不与生产用户资料比较。敏感 payload（witness 请求体）仅存在于测试进程内存与临时观测库，测试结束删除；公开日志只含上述长度与标记计数。

## 6. 复用与不新增

- 复用 canonical artifact + provenance（P04/P05 交接面）：`semanticInputProvenance.sections` 由 stream observer 在边界构造（promptSnapshot 优先），本阶段未新建第二个 prompt builder、未改任何装配实现。
- 本任务**零生产代码改动**；新增仅测试文件 `tests/p06-final-request-assembly.test.ts`。
