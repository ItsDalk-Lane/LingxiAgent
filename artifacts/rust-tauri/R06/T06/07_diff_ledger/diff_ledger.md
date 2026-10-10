# R06-T06 差异台账（Design → Implementation → Incumbent）

Task Base: `e89c91a2289c0c7182da9e2ca4b1d31dc56ca5b6`
本台账分三部分：D 项（对现役的行为差异，逐条锚定现役 file:line）、R 项（实现相对设计稿的细化/偏差）、F 项（RED→GREEN 修复记录，含前两棒 RED.md 已记录的 ①-⑤ 与续作本轮新修的 ⑥-⑨）。现役锚点来自设计稿 §3（RC-1/RC-2 勘察期亲验于 base 工作区），本轮续作对新增修复涉及的锚点（`lib/memory/dream/revision-store.ts:243-278`、`server/routes/skills.ts:284-310`、`lib/tools/context-notes-tool.ts:15-45`、`lib/memory/memory-search.ts:148`）逐行复读。

## D 项：对现役（incumbent）的行为差异

### D1 路由前缀现代化
- 现役：`/api/agents/:id/tenets|pinned|experience|avatar`（agents.ts:936-1188）、`/api/memories*`（config.ts:406-681）、`/api/skills*`（skills.ts:264-576）、`/api/memory-dream`（memory-dream.ts:54-136）。
- 候选：统一 `/lingxi/v1/...`（`memories*`、`agents/{id}/tenets|pinned|experience|config|config-flags|agents-md|ishiki|public-*-md|identity|avatar|avatar-path|skills*|skill-bundles/{id}`、`skills*`、`sessions/{id}/context-notes`；lib.rs 路由注册块）。
- 锁定测试：`r06_t06_memory`（7 用例真 HTTP）、`r06_t06_skills`（5 用例）、auth `route_policy_table` T06 断言族。

### D2 context notes 存储位置：会话旁文件 → 私有运行时目录
- 现役：`{sessionPath}.context-notes.md` 旁文件（`lib/tools/context-notes-tool.ts:18-20` contextNotesPath）；读侧兜底 16KiB 截断，marker 逐字节 `"\n…(context notes truncated at 16KiB cap)"`（:9-10）。
- 候选：`{runtime_dir}/context-notes/{session_id}.md`（`agent_memory.rs` ContextNotesStore；候选会话无 JSONL 路径，模块头注册 diff）。16KiB 写入上限（溢出 409 `CONTEXT_NOTES_OVERSIZE`，等价现役工具层 append 溢出拒绝语义）与截断 marker 逐字保留；session_id 字符白名单防路径逃逸。
- 设计稿 §4 原计划 runs.db v10 迁移，实现改为运行时目录文件（R2 项双申报）。runs.db v1–v9 冻结未动（无 v10）。
- 锁定测试：`context_notes_session_scoped_roundtrip`（往返 + 409 溢出）。

### D3 dream ticker 缺席面：自动调度顺延 R07，缺席如实 503
- 现役：ticker 存在时 `ticker.getDreamStatus`；缺席时同码 503（memory-dream.ts:57-58）。
- 候选：status/runs 一律 503 `dream_unavailable`（候选无 ticker，与现役 ticker 缺席分支同码同义；`reason: dream_ticker_absent_in_candidate` 字段如实标注，不伪造健康）；revisions 存储面（revision-store.ts:21-58 对应物）真实迁移。
- 锁定测试：`dream_faces_report_absence_and_revisions_serve_from_store`；memory health 面 `ticker_absent_in_candidate` 步骤同理（`memories_health_route`）。

### D4 dream restore：pending journal + 失败回滚机制缺席
- 现役：restore 写 pending journal（revision-store.ts:258-264）→ applyAllFiles → 失败回滚（:266-274）→ `recoverPendingDreamApply` 崩溃恢复（:282+）；hash 相同短路（:246-247）；安全修订 `pre_restore` 先于写入、`before` = 恢复前状态（:248-255）。
- 候选：先快照恢复前状态存 `pre_restore`（时序与 `before` 内容镜像 :244-253，`snapshot_sections()` Value 全等实现 hash 短路等价语义）；pending journal/回滚缺席（单进程 Rust store + atomic_write；申报为顺延项，非静默——restore 仍返回真实结果或错误）。
- 锁定测试：`dream_revisions_list_read_restore`（断言安全修订 `before.facts` = 被替换状态 + `unchanged` 短路不落新修订）。

### D5 dream prune 的 retainRevisionIds 保护缺席
- 现役：createDreamRevision 支持 `retainRevisionIds`（revision-store.ts:105），restore 期间目标修订豁免裁剪（:255）。
- 候选：prune 按修订 id 字典序（时间戳前缀）保留最新 `MAX_REVISIONS = 10`（同值 :40）；restore 产生的安全修订是最新文件、目标修订与其紧邻，正常路径不受裁剪影响；无显式 retain 标记。
- 锁定测试：`dream_revisions_list_read_restore`（list 计数）。

### D6 技能包格式收窄：zip/.skill/github 缺席 → 显式拒绝
- 现役：zip/.skill 解压安装（skill-package-installer.ts:231-249）、github 拉取（install-skill.ts:357-372 由 `capabilities.learn_skills.allow_github_fetch` 门控）。
- 候选：目录包安装（installFromDirectory :141-202 全量镜像：包根探测 :50-74、symlink 拒绝 :117-139、目标在根内 :117-139 语义、metadata 重写 :97-115）；非目录源 → 400 `SKILL_UNSUPPORTED_FORMAT`；github 分支 → 如实文案（能力未启用，等价现役门控关闭分支，不伪造拉取）。
- 锁定测试：`skills_http_faces...`（相对路径 400、目录安装 ok）；kernel `skill_name_sanitization_matches_incumbent_regex`。

### D7 install_skill 的 skill_content 分支拒绝
- 现役：`skill_content` 参数拒绝（install-skill.ts:576-581「只装完整包」）。
- 候选：同语义文案拒绝（memorytools.rs run_install_skill）。github/skill_content/无源三向分支保留现役文案语义。
- 锁定测试：`a12_layer1_*`（完整协议链）。

### D8 tenets 写路由分类：现役 STUDIO_OWNER 兜底 → LocalOnly
- 现役：tenets POST/decide/DELETE 落 studio 面兜底（agents.ts:1120-1188 + route-security 写分类）。
- 候选：无 studio 面 → `LocalOnly`（与 R02 以来未注册面最严档一致；本地属主可用，绝不降为 settings.write 外发）。
- 锁定测试：auth `route_policy_table` tenets 三断言。

### D9 agents/{id}/skill-bundles/{id} PATCH 分类修复（续作）
- 现役：bundle PATCH = settings.write（skills.ts:360-379 + route-security.ts:501-517）。
- 候选：前位实现以 `rest.contains("/skills/")` 匹配，而 `"skill-bundles"` 不含该子串 → PATCH 落全局 LocalOnly 兜底（本地属主不受影响、scoped 设备被拒——分类缺口）。续作修复为显式 `/skill-bundles/` 匹配（auth.rs T06 块），锁定于 `route_policy_table`（PATCH = settings.write、GET = LocalOnly）。

### D10 SKILL.md 元数据解析：注册子集解析器
- 现役：gray-matter 全量 YAML（skill-metadata.ts:41-65 仅消费 frontmatter 的 name/description/disable-model-invocation/default-enabled）。
- 候选：手写子集解析（顶层 + `metadata:` 嵌套块、同名键、引号标量、注释剥离、description 1024 折叠）——只消费同字段集；非字段 frontmatter 键不参与行为（与现役信任边界一致）。
- 锁定测试：kernel `skill_metadata_frontmatter_golden`。

### D11 experience 标题 stem：NFKC 缺席 → sha256 后缀
- 现役：`buildExperienceStorageFileName` NFKC 归一 + 48 字符 stem（experience.ts）。
- 候选：NFKC 在锁定依赖集不可用 → 小写字母数字 stem + 10 字符 sha256(title) 后缀（kernel memory.rs 注册注释）；同标题稳定、异标题无碰撞。
- 锁定测试：kernel `experience_index_block_and_entry_numbering`。

### D12 agent config PATCH 面收窄为 T06-owned 键
- 现役：config.ts/agents.ts 的 agent 配置写面覆盖 providers/models/tools 全量。
- 候选：`PATCH /agents/{id}/config` 只接受 `memory.enabled`/`experience.enabled`/`workspace_context.discover_*`；未知键响亮 400（无静默忽略）；宽配置面归 R05/R07 所有者。
- 锁定测试：`experience_gating_and_feedback_shape`（PATCH 开 experience → flags 读回 true）。

### D13 config.yaml 编辑为行级子集 patcher
- 现役：config-loader.ts:114-134 全量 YAML 反序列化→重 dump。
- 候选：行级 patcher（`AgentConfigPatch`），skills 列表/布尔键 upsert，其余行逐字节保留；列表项 JSON 引号转义。
- 锁定测试：adapters `config_patcher_preserves_unrelated_content_and_lists`；`skills_http_faces...` 断言 PUT 白名单后 `locale: zh`/`memory:` 节保留。

### D14 search_memory 语义检索分支如实缺报
- 现役：semanticSearch 命中时并入结果、维度不匹配显式跳过（fact-store.ts:546-578）；零结果诊断（memory-search.ts:70-94）。
- 候选：embedding 存储表随 schema v3 建表（`fact_embeddings`）但语义检索未接线 → `semantic_status = "not_configured"`（结构上预留报告位；绝不伪造语义结果）。tag→FTS 两策略、date_to+`T23:59` 边界（memory-search.ts:148）、零结果逐词计数与改写建议逐条镜像。
- 锁定测试：`a11_memory_tools_are_bound_to_one_agent_at_construction`（检索/写入链）；kernel CJK gram 金样。

### D15 记忆 health 步骤面收窄
- 现役：buildMemoryHealth 多步骤（config.ts:133-160，含 ticker/编译链步骤）。
- 候选：fact_store 步骤 + memory 开关 + ticker 缺席如实步骤（`ticker_absent_in_candidate` → healthy=false 归属于开关而非缺席）；不伪造绿色步骤。
- 锁定测试：`memories_health_route` 响应形状（r06_t06_memory 用例覆盖路由分类链；健康形状断言在 HTTP 套件）。

### D16 install 审核的 guard 失败路径同 token 协议
- 现役：guard 不可用（无模型/槽位缺失）fail-closed 走 suspicious 同路径（install-skill.ts:96-107）；验收 token 三件套（:230-264）；`riskAcceptanceRejection` 经 details 浮出（:417/:529）。
- 候选：无 guard 槽 → `Unavailable` → requiresRiskConfirmation + token（同协议）；token 绑定 `sourceKey + sha256(content)`、TTL 10min、单次消费、target-changed 即焚。
- 锁定测试（R06-T06 R1 收窄口径）：`a12_layer1_install_review_token_protocol_end_to_end` 锁定的是 **token 协议链**（missing/target-changed/expired/accept 四分支 + 一次性消费）；guard **槽缺席/执行器错误 → Unavailable** 分支由 R1 修复新增的 `a12_layer1_guard_slot_absent_fails_closed_to_risk_confirmation` 直接锁定（缺席 → requiresRiskConfirmation + token + 不落盘 + 缺席原因浮出 + 显式接受后放行；RED 证据 = 变异运行：把 None 分支临时改 Safe 后既有套件全绿、新增测试转红——`02_red_tests/repair_R1_F02_red_mutant.log`，证明修复前该分支零覆盖）。

### D17 workspace 技能遮蔽判定源
- 现役：workspace candidate states 由 `workspace-skill-paths.ts:75-118` 以全部来源（用户池/内置/插件/workspace）构造 claimed 顺序。
- 候选：`resolve_runtime_selection` 以 catalog 顺序（用户池 → 内置 → 外部 → workspace）构造 claimed 集合，同名非 workspace 技能先占；policy-disabled/user-disabled/shadowed 三态与 `WORKSPACE_SKILL_DIRS` 四目录（:14-19）逐字同表。
- 锁定测试：kernel `workspace_candidate_states_shadow_and_policy`；HTTP `dupe` 遮蔽断言。

## R 项：实现相对设计稿的细化

### R1 context notes 存储：runs.db v10 → 运行时目录文件
- design.md §4 计划 runs.db v10 迁移建 `session_context_notes` 表；实现改为 `{runtime_dir}/context-notes/{session_id}.md`（D2）。理由：候选会话无 JSONL 路径，旁文件等价物进私有运行时目录比进 runs.db 更贴近现役「会话旁文件」的独立生命周期（会话行删除不影响笔记文件），且避免触碰冻结的迁移链。16KiB 语义与 marker 逐字保留。

### R2 dream 面拆分：存储面先行，调度面顺延
- design.md §6 已预分类（deferred → R07）；实现将 revisions 存储面（save/list/read/restore）完整迁移，ticker/自动调度/事件广播缺席以显式 503/健康步骤申报（D3）。

### R3 guard 槽生产接线
- design.md §1（A12 行）计划 guard 经 `SkillGuardPort`（生产 = AuxiliaryExecutor guard 槽）；实现 `GatewaySkillGuard` 在 bootstrap `wired_real_model_chain` 时构造（lib.rs），网络面复用 `deps.network_plane`；无链时 guard 槽为空 → fail-closed（`Unavailable`），与现役 :96-107 同向。

### R4 hooks 管线单点注入
- design.md §2 计划 runs.rs 经 `Option<Arc<ExtensionHooks>>` 注入；实现为 `with_extension_hooks(bool)` 开关 + run 内局部 `t06_hooks: Option<ExtensionHooks>`（每 run 新建 = agent_start reset 语义），`record_tool_result!` 宏与派发前 Block 两个注入点；测试默认关（pre-T06 形状逐字节保留，对照腿断言）。

### R5 记忆工具 target 解析
- R05 协议适配器给出的 wire target 是全 id（`tool:first-party:search_memory`）；工具执行器与 loop guard 均取末段本地名（memorytools.rs:566、runs.rs 两处），与现役扩展按工具名（"read"）观察的语义对齐（RED.md 缺陷 ②④ 的修复泛化）。

## F 项：RED→GREEN 修复记录

前两棒（RED.md 为权威，5 项）：
- F-① `riskAcceptanceRejection` 未浮出 → skills.rs 补齐（install-skill.ts:417/:529）。
- F-② memorytools 按短名匹配 wire target → 全链 unknown target → 取末段本地名。
- F-③ tool_result_guard 对错误结果也截断/扫描 → 错误豁免（compaction-guard-ext.ts:211-212）。
- F-④ run 循环把全 id target 传给 name-keyed loop guard → 传本地名。
- F-⑤ Failed 结果丢弃 loop-guard 失败连击警告 → 警告前置进 error.message。

续作本轮（4 项，全部由盘点/新增断言暴露）：
- F-⑥ `SkillService::set_enabled_skills` 曾以空文本作为 config.yaml 打底重写（PUT 白名单即清空 locale/memory 等全部无关节——数据丢失）→ 改为以读到的原文打行级补丁（skills.ts:284-310 只重写 skills 列表的语义）；新增断言：PUT 后 `locale: zh`、`memory:` 节、`enabled: true` 逐字保留。
- F-⑦ `DreamRevisionStore::restore` 曾在写入后才创建 `pre_restore` 安全修订，其 `before` 存的是恢复后内容（安全修订失去撤销能力，违反 revision-store.ts:244-253）→ 改为先快照恢复前状态再写入，且内容全等时短路不落修订（现役 hash 守卫等价）；新增断言：安全修订 `before.facts` = 被替换状态 + `unchanged` 短路。
- F-⑧ auth.rs T06 块的 `/sessions/{id}/context-notes` 分类分支不可达（sessions 子树终态 fail-closed return 先行覆盖，行为同为 LocalOnly）→ 移除死分支，留注释指向覆盖点；`route_policy_table` 补 5 动词 LocalOnly 断言锁定。
- F-⑨ `route_policy_table` 缺 T06 断言（前两棒只写了分类块）→ 补全记忆面/agents 人格面/skills 面/context-notes 全族断言；断言过程暴露并修复 D9（skill-bundles PATCH 分类缺口）与 POST `/memories/compiled`（写动词在已注册写面 = settings.write，路由层 405 兜未注册形状）两处期望校准。

### D18 技能目录装配进上下文：T01 恒空 → R1 经 SkillService 运行时选择接线（REPAIR-R06-T06-R1 F-01）
- 现役：`core/session-coordinator.ts:2102-2110`（getSkillsForAgent → skillsResult 会话快照）→ resourceLoader `getSkills` 代理（:2181-2182）→ pi `agent-session.js:784-789`（loadedSkills 进 buildSystemPromptOptions）→ pi `skills.js:275-298` formatSkillsForPrompt（`disableModelInvocation` 过滤在渲染前 :276；`<location>` = SKILL.md 路径 :293）。
- 候选（R1 修复后）：编译期技能材料经 `SkillMaterialPort` 端口由 SkillService 解析——context_compiler.rs:117 端口 + :887-897 `compiled_for_run` 端口覆盖（解析失败与材料读取失败同权响亮）+ skills.rs:932-981 `runtime_skill_catalog`（启停白名单/`|| []` 空 config 形状/workspace policy-遮蔽契约/`disable_model_invocation` 服务侧过滤）+ lib.rs:1461-1475 bootstrap 绑定；条目以**来源材料**身份进 T01 冻结 `skill.catalog` 段（digest_only wrapper 带 + 预算钳制），无新增 system 特权通道。修复前 `ContextCompileInput.skills` 恒空、`resolve_runtime_selection` 无编译器消费者（skills.rs:377 仅 HTTP 视图）；T01 报告差异项⑥（skill.catalog 生产默认空、SkillService 归 T06）由此闭合。
- 配套修复（同一次消费发现的真缺陷）：`resolve_runtime_selection` 的活跃集原为纯名字表，按名过滤目录会把同名遮蔽副本双激活——skill_library.rs:228-305 增加 `active_entries`（解析序的活跃 SkillEntry 本体）；`list_for_agent` 的状态配对从按名 map 改为按条目位置配对（skills.rs:388-424，同名用户池副本不再吃到 workspace 副本的 shadowed 状态——现役 getRuntimeSkillInfos 逐条映射语义）。
- 锁定测试：`r06_t06_skill_context`（4 用例）——`enabled_skill_catalog_reaches_the_production_system_prompt`（bootstrap→run→compile 生产链 + 真实 FileContextMaterialSource，启用技能 name/description/SKILL.md location 进发送文本 + 观测 skill.catalog 段）、`disabled_skill_never_reaches_the_production_system_prompt`、`runtime_skill_catalog_respects_whitelist_filter_and_workspace_shadow`（白名单序/disable-model-invocation 过滤/遮蔽去重/空 config workspace policy）、`memory_tool_registration_failure_is_loud_not_silent`（F-03）。RED 证据 `02_red_tests/repair_R1_F01_F03_red.log`。

### D19 avatar GET 的 ETag/304 条件请求缺席（OBS-1 补记）
- 现役：`GET /api/agents/:id/avatar` 返回 ETag，ETag 相符 304（叶 E4BEE8 断言）。
- 候选：字节/MIME/404 已实现（persona.rs:204-228），ETag/304 未实现；叶图原错引 D13（config.yaml patcher），R1 修正为指向本条目；随 R09 桌面接线复核。
- 锁定测试：无（缺席面，share 申报——见叶 E4BEE8 anchor）。

### D20 HTTP 手动安装的 agentId 连带启用未接线（R1 补记）
- 现役：`POST /skills/install` 的 agentId 可选连带启用（skills.ts:429-441）。
- 候选：HTTP 手动安装不触碰助手 enabled（skills.rs:1170-1206，只做包结构校验安装）；模型侧 install_skill 工具面的安装后连带启用已交付（memorytools.rs:447-553 + enable_for_agent :555-576）。叶 SKILL-INSTALL-AND-ENABLE 原错引 D11（现为 experience stem 条目），R1 修正指向本条目。
- 锁定测试：`a12_layer1_install_review_token_protocol_end_to_end`（工具面安装成功断言）；HTTP 半面为 share 申报（无行为断言）。

## R1 修复轮追加 F 项（REVIEW-R06-T06-R1 → REPAIR-R06-T06-R1）
- F-⑩（审查 F-01）技能材料未进上下文 + 报告 §五.5/design §3 失实：接线上述 D18（含 active_entries/按条目配对配套缺陷修复）；报告与设计稿措辞以修后实态重写；叶图 ENABLE/DISABLE/PUT 三叶 anchor 回填编译消费锚（R00 提取中「刷新运行时/prompt」断言的承载叶）。横向扫描结论：`ContextCompileInput` 全部恒空槽中仅 skills 属「服务已建、零编译消费」；`append_system_lines`（现役 resourceLoader appendSystemPrompt/provider patches/deferred-result 组装）与 `project_context_files`（现役 resourceLoader.getAgentsFiles 工作区 AGENTS.md 类文件）在候选**无对应已建服务**（deferred-result 归 R07、工作区文件发现无 owner），维持 T01 差异登记而非新造生产者；`compute_default_enabled`（skill-manager.ts:260-264 新 agent 默认白名单）已建但生产消费者（agent 创建面）归 R07——helper 先行非静默缺口，消费接线时复核。
- F-⑪（审查 F-02）guard 槽缺席 fail-closed 分支零测试：新增直接断言测试（D16 收窄口径 + RED 变异运行证据）。
- F-⑫（审查 F-03）memorytools 注册失败静默跳过（`if let Ok` 丢弃 Err）：改 `.expect("the resident memory/skill tools register")`（filetools.rs 既有先例；memorytools.rs:753）；同根因横向扫描全服务面 `.register(` 调用（events/toolgateway/cancel）均有 `?`/expect 处理，无第二处静默跳过。锁定测试：`memory_tool_registration_failure_is_loud_not_silent`（重复注册必须 panic）。
