# R06-T06 一次性独立对抗性审查报告（REVIEWER-R06-T06-R1）

- 审查对象：分支 `codex/rust-tauri-migration`，TASK_BASE_SHA `e89c91a2289c0c7182da9e2ca4b1d31dc56ca5b6`，候选 = 未提交工作树。
- 审查人：独立代理，未参加实现；执行者报告仅作待核验线索。
- 工具链（审查人自录）：`/Users/study_superior/.cargo/bin/cargo` 1.98.1 (797e8a9bc)，rustc 1.98.1 (48a229cea)。与执行者 `00_toolchain/toolchain.txt` 一致。
- 复跑纪律：全部 cargo 命令 `-j 8`；>5 分钟命令后台运行；`lingxi-test-guard` 全程保持 armed，未关守卫硬跑。

## 0. 候选摘要（digest）

| 时点 | 命令 | 值 |
|---|---|---|
| 开工 | `bash scripts/rust-tauri/r06_candidate_digest.sh` | `7b4e79110c9dcc2938a21518b8e2c912a77dd2fabf4e74f9ec75ea6871b6d5b9` |
| 收尾 | 同上（见 §7） | （见 §7） |

开工值与执行者声称一致。脚本口径 = `git diff HEAD` + 全部未跟踪文件逐文件 sha256（仅排除 `artifacts/rust-tauri/R06/**` 与 `candidate_digest*.txt`）；`.vscode/settings.json` 客观在口径内（未被 .gitignore 排除），执行者已在报告 §十六如实申报为噪声。审查期间未修改任何产品代码与测试；`cargo build -p lingxi-service` 与测试复跑不改写工作区源文件，digest 输入不受影响（收尾复验见 §7）。

## 1. 任务完整性（审查维度一）

- 5 Steps 逐条：①记忆数据访问/调度（五 store + dream revisions 真实迁移、ticker 缺席如实 503）✅；②按 Agent 隔离 + 材料按来源进上下文——记忆/人格半边成立，**技能半边不成立**（见 F-01）；③Skills 发现/安装审核/启停/同步/技能包/人格文件 ✅（同步=白名单写面+runtime 选择计算；模型侧目录装配缺口归入 F-01）；④hooks 迁入 Rust ports ✅（kernel 纯状态机 + runs.rs 两注入点，无任何 Node 回环路径）；⑤执行仍受 R04 网关 ✅（A12 第二层实证）。
- 3 Deliverables：MemoryService（`rust/crates/lingxi-service/src/memory.rs`）✅；SkillService（`src/skills.rs` + adapters `skill_library.rs`）✅；PI_REPLACEMENT_MATRIX 更新（`r06_t06_update` 块 + PI-01/04/05/09/11 `r06_t06_progress` + extension_hooks 逐 hook `r06_t06_status`）✅。
- A11/A12：均以真实 HTTP/网关/存储层测试覆盖（§3）。
- Depends On（R06-T05）：bootstrap 复用 T05 `prepare_layout`/canonical home 纪律与 T01 `resolve_product_dir` 链（lib.rs:1188-1207）✅。
- 无遗漏编号；`docs/README.md` 索引行、D-12 依赖登记、Cargo.lock 净新增恰 5 条目（ctor 0.8.0/ctor-proc-macro 0.0.7/dtor 0.3.0/dtor-proc-macro 0.0.6/lingxi-test-guard）均落实。

## 2. 生产路径真实性（审查维度二）

- **MemoryService 接线完整**：bootstrap `MemoryService::new`（lib.rs:1194）→ 40+ 条路由真实注册（lib.rs:5684-5860）→ auth `classify_route` T06 纯追加块（auth.rs:775-958，`git diff` 零删除行）→ handler 归属闸先于实体读取（如 `dream_revision_detail_route` 先 `agent_id_from_query`/`dream_store` 再 `read`）→ adapters `resolve_agent_roots`（validate_agent_id 拒 `/`、`\`、`.`、>128、非字母数字）→ `agents/{id}/memory/` 物理隔离。**非只建 store 未接线**。
- **模型工具面真实可达**：`memorytools::register_memory_tools`（lib.rs:1289）仅在真实模型链 + workspace access 的生产分支注册，构造期绑定 `resolve_primary_agent()` 单一 agent；六工具经 R04 网关 prepare→policy→execute 纪律。测试 `a11_memory_tools_are_bound_to_one_agent_at_construction` 经真实网关断言：绑定 B 时检索不到 A 的事实、走私 `agentId` 参数被 schema 拒绝、pin 只落 B 的 tenets.json 且经 PII 脱敏。**不是仅测试构造**。但注册失败为静默跳过，与注释/既有模式矛盾（F-03）。
- **SkillService 生产面**：发现/白名单（含 #397 交集防御）/遮蔽三态/启停/删除 scrub/bundles/外部路径均为真实 HTTP+存储实现（`skills_http_faces…` 逐项断言）。**guard 审查是真实生产实现**：`GatewaySkillGuard` 走 R05 AuxiliaryExecutor 的 `Guard` 辅助槽 + 凭证服务 + 网络面（skills.rs:81-127），非测试 stub；bootstrap 仅在 `wired_real_model_chain` 时接线（lib.rs:1433-1459），槽缺失 → `Unavailable` → fail-closed 同 token 协议（skills.rs:561-585，代码亲读确认；测试缺口见 F-02）。
- **hooks**：runs.rs 两注入点真实——①派发前校验环内 `on_tool_call` Block（runs.rs:2256-2281，位于 args_digest 校验之后、网关 prepare/任何执行器之前，`continue` 即零执行器调用）；②`record_tool_result!` 宏内 `tool_result_guard`（警告前置/L1 32KiB head-tail 截断/失败连击；错误结果豁免截断与扫描，镜像 compaction-guard-ext.ts:211-212）。`extension_hooks` builder 默认 false，宏在 None 时为 outcome 原样 clone——pre-T06 逐字节路径。扩展无任何回调旧 Agent loop 的通道（hooks.rs 为纯 Rust 状态机，无 Node 桥）。
- **上下文入口**：T01 编译器读取 `tenets.json`/`memory.md`/`AGENTS.md`/`identity.md`（context_compiler.rs:683-717），与 T06 写路径产出同一批文件（写后下一次编译即读到，回落链测试覆盖）。记忆/人格内容按 T01 冻结的段注册表进入（无新增高优先级通道）；注入扫描只作用于工具结果（镜像现役）。**技能材料未进入上下文**（F-01）。

## 3. 对抗性验证（审查维度三）

- **A11 正反全过**：HTTP 层 A/B 各写各读、import 不串、B 的 DELETE 不清 A（`a11_memory_read_write_isolated_between_agents`）；tenets/compiled per-agent（含 TENET_STORE_CORRUPTED 响亮）；agentId 缺失 400 / 非法（`..%2FagentA`、`a.b`）400/404 / 未注册 404；存储层物理隔离（A/B 各自 facts.db）+ 路径形态 id 全拒（`../agentA`、`agentA/../../etc`、`a\b`）；子代理视角=构造期绑定 agent 视角（检索不到他 agent 事实 + 无跨 agent 句柄 + schema 层拒绝走私参数）。
- **A12 双层成立**：第一层 7 步全链（requiresRiskConfirmation+token → 无 token `missing_confirmation_token` → 内容篡改 `confirmation_target_changed` → 过期 `invalid_or_expired_confirmation_token` → 新 token+显式接受安装成功且 default-enabled:false → token 单次消费防重放）；安全路径直装同样 default-enabled:false；learn_skills 总闸拒绝。第二层经 R04 网关：workspace 内正对照授权、越 workspace workdir prepare 阶段拒（零派发）、symlink 逃逸按真实目标拒、哨兵文件字节零泄漏。token 协议与现役 install-skill.ts:230-264 逐语义镜像（源码亲验一致）。
- **注入**：`scan_injection` 中英规则 + 零宽/HTML 注释剥离（镜像 injection-scan.ts），只扫非 knowledge_ 前缀的非错误工具结果——与现役同等暴露面，无新增提权通道；记忆/人格内容按 T01 段位进入，无冒充 system 高优先级指令的新路径。
- **urldecode 修复**：逐字节解码，`%XX` 有效 i+=3 / 无效 i+=1 / `+` i+=1 / catch-all i+=1——结构有界（memory.rs:232-269）；3 个回归单测覆盖死循环输入类（纯字面量、`%` 序列、畸形截断）。修复前后对照证据（12s 4.3GB vs 3 次 32MB/0.4s）归档真实。
- **test-guard 范围裁决**：①必要且克制——事故证据链真实（fix_before 12s→4.3GB 截杀；构建波次对照 27×rustc 5.3GB 不收敛 vs -j 8 3.9GB 完成），新代码面仅 1 crate+1 依赖，既有文件改动为每处 +4 行的 use 接线（亲验 diff）；②release/生产零链接成立——归档 nm 扫描 + 审查人独立 nm 复验（release=0）+ fresh `cargo build -p lingxi-service` 后 dev 产物=0（`review_cargo_build_verify.txt`）；③300→600s 证据链成立——三组对照日志真实（默认 300s SIGABRT@300.02s / hooks-off 400s 限 300.46s 通过 / hooks-on 900s 限 300.48s 通过），固有 ~300.5s 为 T05 时代已存在（T05 归档全量日志 300.57s），600s=2 倍富余仍远小于 61 分钟事故；真实失控另由 8GiB 内存线兜住（176GB/61min ≈ 48MB/s → ~2.8 分钟触线），**不会掩盖未来失控**；④guard 12 个单元测试真实且覆盖阈值解析/优先级/采样/线程名/fd 去重/默认安装/幂等；demo A/B 真实触发记录在案。dev 产物在 `cargo test` 后携带守卫符号的特性统一现象未写入文档（OBS-2）。
- **边界**：路径穿越/非法 id/超长（context notes 16KiB 409 + 读侧 marker 截断）/avatar 15MB 白名单均测试锁定；并发写与非法 UTF-8 无专项测试（SQLite 每请求独立连接 + JSON 层拒非法字节 + lossy 解码，结构性处理，见 OBS-3）。

## 4. R05/T01–T05 回归（审查维度四）

审查人复跑（`-j 8`，rust/ 目录，`--locked`）：

| 命令 | 退出码 | 结果 | 证据 |
|---|---|---|---|
| `cargo fmt --all --check` | 0 | 干净 | review_fmt_check.txt |
| `cargo clippy -j 8 --locked --workspace --all-targets -- -D warnings` | 0 | 干净 | review_clippy.txt |
| `cargo test -j 8 -p lingxi-service -p lingxi-adapters --locked` | **0** | **109 测试二进制 / 1458 通过 / 0 失败**（T06 四套件 14+4+7+5 全过） | review_targeted_tests.txt |
| `cargo test -j 8 --workspace --locked --no-fail-fast` | 见 §7 | 见 §7 | review_full_workspace.log |

- auth.rs 分类块纯追加（diff 零删除行）；runs.rs 4 行删除全部为 `record_tool_result!` 宏体替换，hooks=None 时语义逐字节保持，对照腿测试断言无 hooks 时 7 次同参调用全部真实执行且历史无 guard 文本。
- v1–v9 迁移冻结（无 v10；context notes 走运行时目录，D2/R1 双申报）；T01 材料链回落链测试覆盖；T03/T04/T05 遗留义务（T03 R2-F-01~04、T04 F-01/F-02、T05 F-01/F-02）涉及的代码文件不在本候选修改清单内，原样保留。

## 5. 防止虚假完成（审查维度五）

- **62 implemented 叶**：锚点全部非空且具体到 file:line；审查人抽验 12+ 条（persona avatar/identity/agents-md 面、memory delete/import/tenets/pinned/experience/week、memorytools pin/search/context-notes、skills patch/delete/bundles/external-paths），行号与语义逐条相符（±3 行内）。A11/A12/工具面/HTTP 面测试与锚点一一对应，30 个新集成测试在我复跑中全绿，无 #[ignore]。
- **18 share**：抽验供应商 config（→R05）、agent config 写面（T06-owned 键收窄 + 行级 patcher）、孤儿技能半面、主助手消费半面、avatar ETag 半面——半交付锚点真实（ETag 差异未入台账，OBS-1）。
- **21 deferred**：去向分布与报告一致（R07×15 含 1 条「同上」、R09×2、R08/R09×3、R07/R08/R09×1），逐条有 stage 归属与缺席面说明；PI 矩阵 deferred-result/事件广播等顺延与叶图一致。
- **记忆工具单 agent 绑定**：真实生产链（bootstrap 生产分支注册，非仅测试构造，§2）。
- **应做未做**：技能材料入上下文缺口（F-01）——叶图本身也未收录该现役行为，属提取遗漏+未申报缺口，非合法份额/顺延。

## 6. Findings

### F-01（MEDIUM）技能材料未进入模型上下文，且报告声称相反、缺口未申报

- REQUIREMENT_ID: R06-T06 Step 2（"现役知识内容/外装技能材料均按来源和权限进入上下文"）+ 交付完整性申报义务
- FILE_AND_LINE: `rust/crates/lingxi-service/src/context_compiler.rs:735`（`skills: Vec::new()` 恒空）；`rust/crates/lingxi-adapters/src/storage/skill_library.rs:233`（`resolve_runtime_selection` 无编译器消费者，仅 skills.rs:377 的 HTTP 视图与测试调用）；对照现役 `core/session-coordinator.ts:2102-2110`（getSkillsForAgent → skillsResult 快照 → resourceLoader）与 `node_modules/@earendil-works/pi-coding-agent/dist/core/agent-session.js:784-789`（loadedSkills 进 buildSystemPromptOptions）。
- OBSERVED: 候选中已启用技能的目录/SKILL.md 材料永远不会进入模型上下文（kernel `format_skills_for_prompt` 渲染与预算钳制已就绪但服务侧恒空）。用户执行已交付且测试通过的 `PUT /agents/{id}/skills` 启用技能后，模型侧零效果。`docs/rust-tauri/R06/R06-T06_REPORT.md` §五.5 与 `artifacts/.../01_design/design.md` §3 却声称「记忆/人格/技能内容按来源进入上下文」；`R06-T01_REPORT.md` 差异项⑥明文「skill.catalog 生产默认空，SkillService 归 R06-T06」——本任务既未接线也未把该项登记进叶图（101 叶无 skill.catalog 叶）、PI-04 `r06_t06_progress` 顺延清单（deferred-result/事件广播/learn_lesson/show_card/desk/组合导出/autolearn）不含此项。
- EXPECTED: 要么把 SkillService runtime 选择接入 `ContextCompileInput.skills`（与现役默认门控行为对齐），要么在叶图/PI 矩阵/报告如实登记该份额或顺延并修正 §五.5 表述。
- REPRODUCTION: 启用任一技能 → 驱动一次带编译器的 run → 观测 artifact 段：skill.catalog 恒缺；同配置现役（有已启用技能时）prompt 含技能目录。
- ROOT_CAUSE: T06 叶提取以「路由/工具面」为粒度，漏掉「技能目录装配」这一现役行为；T01 移交的悬挂依赖（差异项⑥）未被 T06 盘点接住。
- SAME_ROOT_CAUSE_PATHS: 叶图提取遗漏（同族风险：任何「非路由非工具」的 prompt 装配行为都可能缺叶）；R06-T06_REPORT §五.5、design.md §3 的同句表述。
- IMPACT: 启停/发现功能的模型侧语义不完整（用户可感知）；A11/A12 REQUIRED 验收不受影响；无安全影响（攻击面反而更小）；R06 阶段放行条件「全部叶子功能映射到真实实现」在该行为上未满足且未被申报。
- REQUIRED_FIX: 二选一——(a) 接线：`SkillService` runtime 视图 → `ContextCompileInput.skills`（含现役门控语义），补对照测试；或 (b) 申报：叶图补叶、PI-04 顺延清单补条目、报告 §五.5/design §3 修正，并指明归属 stage。
- REGRESSION_TESTS: (a) 路线——启用技能后编译产物含 skill.catalog 段金样 + 空技能时段省略；(b) 路线——叶图/矩阵 schema 校验。

### F-02（LOW）guard 槽缺席 fail-closed 路径无测试，报告/台账称其被「锁定测试」覆盖

- REQUIREMENT_ID: R06-A12（guard 失败路径 fail-closed 申报，D16）
- FILE_AND_LINE: `rust/crates/lingxi-service/src/skills.rs:561-566`（None → `Unavailable("guard slot is not configured…")`）；`rust/crates/lingxi-service/tests/r06_t06_skills.rs:129-302`（两条 layer1 测试均注入 SuspiciousGuard/SafeGuard，无一覆盖槽缺席）；`artifacts/rust-tauri/R06/T06/07_diff_ledger/diff_ledger.md` D16「锁定测试：a12_layer1_install_review_token_protocol_end_to_end（7 步全链）」。
- OBSERVED: 槽缺席 → Unavailable → requiresRiskConfirmation 的 fail-closed 分支仅有代码实现（审查人亲读确认其汇入与 Suspicious 相同的 `risk_confirmation_result` token 协议，方向正确），无任何测试执行过该分支；guard 执行器错误 → Unavailable 分支同样未测。
- EXPECTED: 安全相关的 fail-closed 分支应有直接测试，或 D16 的「锁定测试」声明收窄为仅 token 协议链。
- REPRODUCTION: `SkillService::new` 后不 `set_guard`，learn_skills 开启下 `install_with_review` 越权技能包——当前无测试执行此序列。
- ROOT_CAUSE: RED 阶段以注入 stub 驱动协议链，缺席分支未被盘点为独立断言点。
- SAME_ROOT_CAUSE_PATHS: 无（独立）。
- IMPACT: 未来重构（如把 None 分支改为放行）不会被现有测试拦截。
- REQUIRED_FIX: 补一条无 guard 安装 → `requiresRiskConfirmation=true` 且不落盘的断言（约 15 行）；或修正 D16/报告 §十一的锁定声明范围。
- REGRESSION_TESTS: 同 REQUIRED_FIX。

### F-03（LOW）memorytools 注册失败静默跳过，与同点注释及 R04 既有响亮模式矛盾

- REQUIREMENT_ID: 项目红线「禁止静默降级」+ R04 网关注册纪律一致性
- FILE_AND_LINE: `rust/crates/lingxi-service/src/memorytools.rs:747`（`if let Ok(receipt) = registry.register(m, budget)` 丢弃 Err）；对照 `rust/crates/lingxi-service/src/lib.rs:1284-1287` 注释「Registration failure is a loud startup error, never a silent skip」与 `src/filetools.rs:2355-2367` 既有 `.expect("the resident read tool registers")` 模式。
- OBSERVED: 六个记忆/技能工具的注册失败被静默忽略；注释声称响亮失败。
- EXPECTED: 与 filetools 相同的 expect/传播，或修正注释。
- REPRODUCTION: 代码审读；实践触发需 registry 重名/预算耗尽（bootstrap 新建 registry 下不可达，故无行为性影响）。
- ROOT_CAUSE: 实现期采用了宽松写法而注释模板沿用了响亮语义。
- SAME_ROOT_CAUSE_PATHS: 无（独立）。
- IMPACT: 当前不可达；但若未来在共享 registry 上复用该函数，工具缺失将无声发生（模型面静默缩窄，恰是 A11 工具面最不想要的失败模式）。
- REQUIRED_FIX: `.expect("…registers")` 化六处注册（或返回 Result 并在 bootstrap `?`）。
- REGRESSION_TESTS: 既有 `a11_memory_tools_are_bound_to_one_agent_at_construction` 已覆盖注册成功路径，无需新增。

### OBS-1（OBS）avatar ETag/304 差异未入差异台账；share 叶锚点错引 D13

- `R06_LEAF_MAP.json` 叶 `F-D06-…-AVATAR-AVATAR-ROLE-AGENT-E4BEE8` 的 t06_anchor 称 ETag/304 缺席「→ 差异台账 D13」，而 D13 实为 config.yaml 行级 patcher；台账全文无 ETag 条目。文档交叉引用失准，属申报完整性小缺口。

### OBS-2（OBS）dev 产物在 `cargo test` 后携带守卫符号且运行期 armed（特性统一），文档未写此翻转

- 审查人实测：`cargo test` 后 `target/debug/lingxi-service` nm 守卫符号 374（feature 统一使 bin 链入 test-guard 边的 kernel rlib）；随后 fresh `cargo build -p lingxi-service` 重建后为 0（`review_cargo_build_verify.txt`）。release 恒 0（独立 nm 复验）。`T06_TEST_GUARD.md` §5 写明「普通 cargo build 与 release 产物不带守卫」与「cargo test 构建的服务子进程 armed」，但未写明 dev bin 会在 cargo test 后**持续** armed（>600s 的长跑 dev 服务会被 abort）。归档证据在其声明口径下真实成立，此为文档边界补充建议。

### OBS-3（OBS）并发写与非法 UTF-8 无专项测试

- 记忆 store 并发依赖 SQLite 每请求独立连接（与现役每请求开 FactStore 同构）；非法 UTF-8 在 HTTP JSON 层被 serde 拒绝、查询值经 `from_utf8_lossy`。结构上处理但无显式测试；如后续 R07 接入后台写（dream ticker），建议补并发写入测试。

## 7. 全量回归与收尾 digest

- `cargo test -j 8 --workspace --locked --no-fail-fast`（审查人复跑，2026-10-10）：**exit 0，1816 通过 / 0 失败**，守卫触发 0（全日志无 LINGXI_TEST_GUARD 报告行），`approval_face_share_cases` 在 600s 默认下正常完成（ok）。比执行者终局轮（1815/1816，唯一失败 r00_management LAN 环境项）更干净——该 LAN 项本轮通过，与其「时通时不通」的既有环境敏感性记录一致，非矛盾。
- 收尾 digest：`bash scripts/rust-tauri/r06_candidate_digest.sh` = `7b4e79110c9dcc2938a21518b8e2c912a77dd2fabf4e74f9ec75ea6871b6d5b9`——**与开工值逐字一致，候选零漂移**（审查只读 + 测试复跑 + 证据落盘 artifacts/ 口径外）。

## 8. 裁决

- REQUIRED 验收 **R06-A11 成立**（存储物理隔离 + HTTP 归属闸 + 非法 id 拒绝 + 工具面构造期单 agent 绑定 + 真实正反测试）；**R06-A12 成立**（审核 token 协议 7 步全链 + fail-closed 方向代码确认 + R04 网关越权拒绝双层实证）。
- Findings：F-01 MEDIUM、F-02 LOW、F-03 LOW、OBS-1/2/3。**无 LOW Blocking 及以上阻断项**：F-01 不属于 A11/A12 验收场景、无安全影响、可由「接线」或「如实申报顺延」二选一收敛，但它推翻了报告 §五.5 的部分表述且暴露叶图提取遗漏，必须在合并/交接前处置（修复或登记），不得以本 PASS 推断该缺口已闭合。
- 四组命令复跑全部通过且优于执行者水位；digest 零漂移；证据链（00-09）亲验无伪造迹象；RED 证据真实；事故修复与 test-guard 基础设施裁决为必要、克制、零生产链接、时限依据充分。

**VERDICT: PASS**（附 MEDIUM F-01 必须在合并前修复或如实申报为顺延；F-02/F-03 建议随手修复）

## 附：审查证据文件清单（本目录）

| 文件 | 内容 |
|---|---|
| `REVIEW.md` | 本报告 |
| `review_fmt_check.txt` / `review_fmt_exit.txt` | fmt 复跑（exit 0） |
| `review_clippy.txt` | clippy 复跑（exit 0） |
| `review_targeted_tests.txt` | 定向复跑 exit 0：1458/1458，T06 四套件 30/30 |
| `review_full_workspace.log` | 全量复跑 exit 0：1816/1816，守卫零触发 |
| `review_full_workspace_exit.txt` | 全量退出码 |
| `review_cargo_build_verify.txt` | fresh `cargo build -p lingxi-service` 后 nm 守卫符号 = 0（OBS-2 依据） |

