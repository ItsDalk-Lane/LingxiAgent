# R06-T06 人格、记忆与技能系统 — 设计稿（先于实现冻结）

- 任务书：`Lingxi_Rust_Tauri_Taskbooks_2026-09-23/R06_…md` §R06-T06；验收 R06-A11 / R06-A12。
- 勘察纪律：RC-1 端到端生产链；RC-2 差异锚现役行号（本文件 §3 为勘察时工作区 base `e89c91a2289c0c7182da9e2ca4b1d31dc56ca5b6` 的亲读行号）。
- 工具链：全部 cargo 调用使用 `/Users/study_superior/.cargo/bin/cargo`（1.98.1；PATH 上的 homebrew 1.93.0 不读 rust-toolchain.toml，禁用——见 00_toolchain/toolchain.txt）。
- **R1 修订声明（REPAIR-R06-T06-R1）**：R1 审查 F-01 认定原稿 §2「技能材料按来源进入上下文由既有编译器承担」在实现中不成立（编译器技能槽恒空）。§2 上下文入口一段已按**修后实态**改写（SkillMaterialPort 接线）；其余章节保持冻结原文，§3 现役行号为勘察期基准不作追改。

## 1. 验收映射（Requirement → Production Entry → Owner → Implementation → Positive/Negative Test → Evidence）

### R06-A11「Agent 记忆不串用」
| 项 | 内容 |
|---|---|
| Requirement | AgentA/B 有不同事实与权限；分别询问及写入记忆；只读/改所属范围；子代理不扩大可见性 |
| Production Entry | HTTP `GET/PUT/DELETE /lingxi/v1/memories*`、`/lingxi/v1/agents/{id}/tenets*`、`/lingxi/v1/agents/{id}/pinned`、`/lingxi/v1/agents/{id}/experience*`（显式 agentId 是唯一归属边界，镜像现役 `server/routes/config.ts:419-427` `getStoreForAgent` + `resolveAgentStrict`）；模型入口 `search_memory`/`pin_memory`/`unpin_memory`/`tenet_propose`/`context_notes` 工具经 R04 网关注册（构造时绑定单一 agentDir，镜像现役 `core/agent.ts:1045-1047`「同一 FactStore，不复制数据归属」） |
| Owner | Rust MemoryService（service）+ AgentMemoryStore（adapters，per-agent 独立 SQLite facts.db / tenets.json / compiled md / experience/ / dream revisions，全部按 agent 目录物理隔离） |
| Implementation | `rust/crates/lingxi-service/src/memory.rs` + `rust/crates/lingxi-adapters/src/storage/agent_memory.rs` + `rust/crates/lingxi-kernel/src/memory.rs`（tenets 配额/去重/排序纯逻辑、检索文本构造） |
| Positive | A/B 两 agent 目录各写 facts/tenets → 各自读回一致；import 到 A 不出现在 B；B 的 DELETE 不清 A |
| Negative | 跨 agent 读写被拒（agentId 缺失 400、非法 id 400、未注册 404）；子代理视角 = 父 agent 视角（同一 agentDir 构造的视图，无跨 agent 句柄）；工具视图绑定单一 agent |
| Evidence | `02_red_tests/`（先红）→ `05_targeted_tests/`（转绿）+ 报告 §Acceptance |

### R06-A12「技能审核和执行边界仍有效」
| 项 | 内容 |
|---|---|
| Requirement | 技能含越权文件命令；安装/启用后请求执行；保持原审核行为且工具层仍拒绝越权（双层证据） |
| Production Entry | 审核层 = `install_skill` 工具路径（guard 审查 → `requiresRiskConfirmation` + 一次性 token，镜像 `lib/tools/install-skill.ts:230-279`）+ `POST /skills/install` 手动安装的包结构/文件系统安全校验（`skill-package-installer.ts:117-139/:141-202`）；执行层 = R04 统一工具网关 `exec_command`（workspace 边界授权，bootstrap 已接线） |
| Owner | Rust SkillService（service/src/skills.rs）+ SkillLibraryStore（adapters）+ kernel 纯逻辑（SKILL.md frontmatter、名净化、风险 token 协议、workspace 技能策略） |
| Implementation | guard 审查经 `SkillGuardPort`（生产 = AuxiliaryExecutor guard 槽，fail-closed；测试 = 确定性 stub）；token 绑定 `sourceKey + sha256(skillContent)`、TTL 10min、单次消费 |
| Positive | 越权技能包：首次安装 → `requiresRiskConfirmation`+token；带 token 重试同内容 → 安装成功（目录+metadata 重写 default-enabled:false） |
| Negative | `risk_accepted=true` 无 token → `missing_confirmation_token`；token+内容已变 → `confirmation_target_changed`；token 过期 → `invalid_or_expired_confirmation_token`；工具层：安装后经 exec 请求越 workspace 的路径 → 网关拒绝（A12 第二层） |
| Evidence | 同上双层测试 + `02_red_tests/` 先红记录 |

## 2. 候选拓扑（无平行构造者）

```
HTTP（/lingxi/v1/...）→ auth.rs classify_route（settings.read / settings.write / chat，镜像 route-security.ts）
  → memory.rs / skills.rs / persona.rs handler（agentId 归属闸先于一切实体读取）
    → adapters AgentMemoryStore / SkillLibraryStore（per-agent 文件/SQLite；镜像现役布局）
      → kernel 纯逻辑（tenets 配额、检索文本、技能策略、风险 token）
模型入口：ToolRegistry 注册 memory 工具（bootstrap 真实链；经 R04 网关 prepare→approve→execute）
上下文入口：T01 FileContextMaterialSource 读 tenets.json/memory.md/identity/AGENTS.md/yuan；
  技能目录材料（R06-T06 R1 F-01 修复接线）经 SkillMaterialPort 端口由 SkillService 在编译期解析
  （启停、白名单、工作区边界、用户覆盖顺序；现役 session-coordinator.ts:2102-2110 → pi
  agent-session.js:784-789 语义槽位），条目作为**来源材料**进 T01 冻结的 `skill.catalog` 段
  （digest_only wrapper 带 + 预算钳制）——不新造任何 system 高优先级特权通道；
  appendSystemPrompt / 项目上下文文件两槽在候选无已建生产者（差异报告登记，不伪造）
Pi extension hooks：kernel 纯状态机（AgentLoopGuard）+ service hooks（注入扫描/L1 截断）
  → runs.rs 经 Option<Arc<ExtensionHooks>> 注入（None = 现状零变化，生产 bootstrap 接线）
```

## 3. 现役语义锚点（RC-2，行号亲验于 base 工作区）

### 记忆（D07）
- facts.db：`lib/memory/fact-store.ts` — schema v3 `:33`；facts 表+FTS5 external content+触发器 `:123-170`；`fact_embeddings` v2→v3 `:203-213`；add（scrubPII 先行）`:279-295`；addBatch 事务 `:302-310`；replaceBySession `:316-335`；searchByTags（json_each 精确匹配，matchCount 降序→time 降序）`:347-389`；searchFullText（FTS5 rank；空+CJK→LIKE 兜底；异常→LIKE）`:398-424`；countFullTextMatches（与检索同口径）`:431-453`；tagCounts（条数降序→标签字典序）`:465-475`；semanticSearch（维度不匹配显式跳过）`:546-578`；deleteBySession `:586-590`；clearAll+FTS rebuild `:607-614`；exportAll/importAll `:616-637`；检索文本构造（CJK 2/3-gram+唯一化）`:50-81`；FTS query（词+gram，双引号转义，OR 连接）`:83-92`。
- per-agent 布局：`core/agent.ts:223` `factsDbPath = agents/{id}/memory/facts.db`；`:392` `memory/summaries/`；`core/agent-manager.ts:643` mkdir memory；记忆总开关 `config.memory?.enabled !== false`（`core/agent.ts:380`）。
- 归属边界：`server/routes/config.ts:406-427`（getStoreForAgent；`/[\/\\.]/` 拒绝）；写入路由 `denyWithoutScope(c,"settings.write")` `:478/:502/:525/:562/:609`；读路由 settings.read（`server/http/route-security.ts:361-399`）；写/DELETE/POST 分类 `:464-493`。
- 记忆路由：GET /memories `config.ts:443`；GET/PUT/DELETE compiled `:457-604`；week days `:546-586`；export `:630`；import `:648-681`（兼容 v1 `memories` 字段）。
- tenets：`lib/memory/tenets.ts` — 配额 20/200/30 `:42-46`；长度 300 `:48`；dedupKey `:103-105`；严格读（损坏/未来 schema 响亮）`:137-182`；排序（priority weight→createdAt）`:232-239`；addTenetProposal `:253-285`；addTenetDirect（active 重复→duplicate；pending/rejected 重复仍新增）`:293-324`；decideTenet `:327-348`；removeTenet `:350-357`；prompt 节（# 置顶与原则）`:364-375`；legacy 批量计划 `:436-512`。
- tenets/pinned/experience 路由：`server/routes/agents.ts:936-1000`（pinned shim：GET 只回 active；PUT 全量替换，保留 model_proposed），`:994-1108`（experience：暂停 403；非法分类 400），`:1120-1188`（tenets GET/POST/decide/DELETE；错误码→404/409/500）。
- 工具：pin/unpin `lib/tools/pinned-memory.ts:33-156`（pin=scrubPII+addTenetDirect；unpin=id 或关键词模糊、多命中全删）；tenet_propose `lib/tools/tenet-propose-tool.ts:25-109`（pending、重复 duplicate、PENDING_FULL 显式）；context_notes `lib/tools/context-notes-tool.ts:15-41/:59-102`（16KiB、append 溢出拒绝、session 旁文件）；experience 工具 `lib/tools/experience.ts:153+`。
- 编译记忆：`lib/memory/compile.ts` — readCompiledMemorySections `:748-756`；writeEditableFactsSection `:758-771`；writeToday `:794-810`；writeLongterm `:812-825`；listWeekDayEntries `:827-847`；writeWeekDayEntry `:849-866`（只允许已存在日期）；assembleWeekFromDaily（6 天/1200 字，最老先丢）`:586-611`；assemble+buildCompiledMemoryMarkdown（四标题恒在+空占位；导航节非空才追加）`:1054-1091`；navigation（5 会话/10 标签/30 字标题/500 上限）`lib/memory/navigation.ts:14-17/:42-76`；normalize（strip think、去标题行、折叠 3+ 换行、JSON 数组→列表）`lib/memory/compiled-memory-state.ts:46-83`；reset marker `:15-43`；clear artifacts（5 文件清空+.fingerprint 移除）`:45-57`；legacy editable-facts 迁移 `compile.ts:905-955`。
- dream：路由 `server/routes/memory-dream.ts:54-136`（ticker 缺席→503 dream_unavailable；restore settings.write；revision 404）；revision store `lib/memory/dream/revision-store.ts:21-58`（schemaVersion 1、MAX_REVISIONS=10 `:40`、id 白名单 `:56-58`）；错误码 `lib/memory/dream/state-store.ts:11-32`。
- PII：`lib/pii-guard.ts:10-46`（6 硬模式→[REDACTED]）。

### 技能/人格（D21/D06）
- 发现与白名单：`core/skill-manager.ts` — 可见性过滤 `:126-131`；getAllSkills `:141-156`；runtime infos `:159-183`；runtime 选择（enabled 集合+workspace_disabled+claimedByName 遮蔽）`:199-252`；新 agent 默认 enabled `:260-264`；外部扫描 `:344-411`；runtime enabled（plugin/workspace bypass）`:447-453`。
- workspace 策略：`shared/workspace-skill-paths.ts` — 目录 4 个 `:14-19`；policy from config `:21-26`；candidate states（policy-disabled/user-disabled/shadowed）`:75-118`。
- 安装：包安装器 `lib/skills/skill-package-installer.ts` — SAFE_SKILL_NAME `:9`；sanitize `:23-28`；metadata 重写 `:97-115`；symlink 拒绝+目标在根内 `:117-139`；installFromDirectory `:141-202`；zip/.skill `:231-249`；github（allow_github_fetch 门控在工具层）`:355-408`。
- install_skill 工具：`lib/tools/install-skill.ts` — 常量（20s/50KB/10min TTL）`:43-46`；safetyReview（fail-closed、suspicious/safe 协议）`:86-195`；风险 token 三件套 `:230-264`；needs-confirmation 结果 `:266-279`；整体开关 `:337-343`；github 门控 `:357-372`；skill_content 拒绝 `:576-581`。
- SKILL.md 元数据：`lib/skills/skill-metadata.ts:41-65`（仅 frontmatter 贡献；description 1024 折叠）。
- 删除/孤儿：`lib/skills/skill-removal.ts:22-39/:71-79/:104-138`（readonly/workspace/plugin/种子不可删；scrub 所有 agent config+bundle；整体 reload）。
- bundles：`lib/skill-bundles/store.ts:5-6/:28-34/:105-238`（skill-bundles.json、schemaVersion 1、slugify+6hex 后缀、reorder 全量校验、removeSkills 空组合淘汰）。
- 技能路由：`server/routes/skills.ts`（GET skills `:264-282` agentId 强制；PUT agent skills `:284-310` 交集防御 #397；PATCH `:312-358` workspace→workspace_disabled、shadowed 409；bundle PATCH `:360-379`；install `:382-463`（手动安装不做 LLM 审查 `:411-416`；agentId 可选连带启用 `:429-441`）；external-paths `:466-495`（绝对路径+不得等于内置 skillsDir）；DELETE `:498-545`（agentId 强制 `:510-516`；readonly 403 `:521-523`）；reload `:548-559`；translate `:562-576`）。
- 鉴权：技能读 `route-security.ts:495-499`、技能写 `:501-517`、avatar `:100-101/:139-141`。
- 人格：`core/persona-source.ts` — KIND_CONFIG `:58-69`；public `:72-73`；回落链 `:87-116`；locale 链 `:124-130`。T01 已实现读取侧回落链（context_compiler.rs:475）；本任务交付读/写路由面（写后 T01 下一次编译即读到）。
- avatar：GET/POST/DELETE `/api/agents/:id/avatar`（png/jpg/jpeg/webp 顺序、15MB limit；叶 #9-12/#22-24 断言）。

### Pi extension hooks（步骤 4）
- 实际使用面（PI_REPLACEMENT_MATRIX extension_hooks 全集）：agent-loop-guard-ext（session_start `:105`/agent_start `:110`/session_shutdown `:114`/tool_call `:118`/tool_result `:164`）；compaction-guard-ext（context `:203`/tool_result L1 `:209`/session_before_compact `:237` — 压缩路径 T02 已接管）；deferred-result-ext（session_start `:51`/session_shutdown `:80` — 后台投递属 R07）；model-call-observer-ext（before_provider_request `:30`/after_provider_response `:69` — R05 usage/model_call 已接管）；engine.ts `:3497/:3509/:3542`（context/before_provider_request 内部挂钩 — R05/T01 已接管）。
- 本任务接管：agent-loop-guard 全部 5 hook → kernel 纯状态机 + runs.rs 注入（阻断=发 `Failed` 工具结果，不调用执行器；警告=前置文本）；tool_result L1 硬限（32KiB，`compaction-guard-ext.ts:133-138/:208-234` + `core/compaction-utils.ts:99-131` head 40%/tail 40%/安全 UTF-8 切片）+ 注入扫描警告（`lib/security/injection-scan.ts:22-104`，非 knowledge_ 前缀才扫）→ 同一 hooks 管线。T02 的压缩 hook / R05 的观测 hook 在报告与矩阵中标注 share（非本任务重复实现）。

## 4. 存储设计

- per-agent 记忆 = 现役同布局文件（`{home}/agents/{id}/memory/facts.db` SQLite v3 schema、`tenets.json`、`facts.md/today.md/week.md/longterm.md/memory.md/navigation.md/reset.json`、`daily/*.md`、`experience/`+`experience.md`、`dream/revisions/*.json`）——旧数据保护（R08 割接时同一批文件无需转换）；runs.db 不新增记忆表。
- 例外：context_notes（现役 = 会话 JSONL 旁文件 `{sessionPath}.context-notes.md`；候选会话无文件路径）→ runs.db **v10 迁移**新增 `session_context_notes(session_id PK, notes, updated_at_unix_ms)`，16KiB 上限语义逐字保留（v1–v9 冻结不动）。
- skills = `{home}/skills/{name}/`（用户池）+ 内置种子目录（skills2set，随产品分发）+ 外部路径清单 + workspace `.agents/skills` 等；`{home}/skill-bundles.json`；agent 白名单写在各 agent `config.yaml`（行级子集 patcher，见台账 D 条目）。

## 5. 测试分层（RC-3）

- 纯逻辑：kernel tenets 配额/去重/排序、检索文本（CJK gram 金样）、SKILL.md 元数据、风险 token 协议、loop guard 状态机、workspace 遮蔽、注入扫描规则表、PII 模式、L1 截断（对照 `truncateTextHeadTail` 语义）。
- 服务集成：真实 per-agent SQLite/文件 + 真 HTTP（T05 TestServer 模式）+ 真路由鉴权分类（auth route_policy_table 增补）。
- 现役对照：金样锚（tenets JSON 字节、memory.md 装配形状、FTS query 构造、错误词汇 404/409/503、token 协议文案）逐条来自 §3 锚点文件。
- 禁 mock 掉的对象：MemoryService/SkillService 存储本体、R04 网关鉴权、auth 分类。外部协议替身：guard 模型（stub port）。

## 6. 101 叶预分类原则（详见叶图标注）

- implemented：本任务经真实路由/工具+测试覆盖的整叶。
- share：叶的一部分由本任务交付物承担（如 MOOD 投影= T04 已交付的规范化消息；供应商 config 写= R05 面；翻译= R05 辅助槽；UI/桌面面= R08/R09）。
- deferred：R07（后台 dream ticker/自动调度、事件广播、bridge）、R08（桌面接线）、R09（桌面 IPC/查看器窗口）。
- 三待裁决项（如产生）在报告与叶图如实申报。
