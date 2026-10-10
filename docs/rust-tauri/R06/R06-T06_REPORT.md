# R06-T06 人格、记忆与技能系统 — 实施报告

- TASK_ID: R06-T06
- TASK_BASE_SHA: `e89c91a2289c0c7182da9e2ca4b1d31dc56ca5b6`（分支 `codex/rust-tauri-migration`；工作树为未提交 T06 候选）
- 验收 ID: R06-A11（Agent 记忆不串用：per-agent 归属边界 + 工具视图单 agent 绑定 + 跨 agent 拒绝）、R06-A12（技能审核与执行边界双层有效：审核 token 协议 + 网关越权拒绝）
- 工具链: `/Users/study_superior/.cargo/bin/cargo`（rustup shim → cargo/rustc 1.98.1，仓库根 `rust-toolchain.toml` 锁定）；全程未使用 PATH 上的 homebrew cargo 1.93.0（证据 `artifacts/rust-tauri/R06/T06/00_toolchain/toolchain.txt`）
- 候选 digest：由 `scripts/rust-tauri/r06_candidate_digest.sh` 在全部文件写完后计算，值见 handback，**本报告不内嵌**（报告自身在 digest 范围内）
- 执行分棒：本报告合并四棒记录——①②棒实现 + RED（`02_red_tests/RED.md` 为权威），③棒事故修复 + test-guard + 全量回归，④棒（FIN2）正式证据收尾与一次真实守卫误伤的根因修复

## 一、已完成的 Steps（任务书步骤逐条核对）

1. **记忆存储与路由面**（RED→GREEN）：per-agent 记忆五 store（facts.db SQLite v3 / tenets.json / compiled md 族 / experience / dream revisions，全部按 agent 目录物理隔离）→ `AgentMemoryStore`（adapters）+ `MemoryService`（service）+ HTTP 面（`/lingxi/v1/memories*`、`/lingxi/v1/agents/{id}/tenets|pinned|experience|config*`、`/lingxi/v1/sessions/{id}/context-notes`）；dream 存储面（revisions save/list/read/restore）真实迁移，ticker 调度缺席如实 503（台账 D3）。
2. **技能/人格面**：`SkillLibraryStore`（adapters）+ `SkillService`（发现/白名单/启停/遮Shadow/删除 scrub/bundles）+ `PersonaService`（AGENTS.md/identity/public 别名/avatar 读写）+ `/lingxi/v1/skills*` 与 `/lingxi/v1/agents/{id}/agents-md|ishiki|public-*|identity|avatar*|skills*` 路由。
3. **工具面与 hooks**：`memorytools.rs` 五个模型侧工具（search_memory/pin_memory/unpin_memory/tenet_propose/context_notes）经 bootstrap 真实链注册进 R04 网关，构造时绑定单一 primary agent；kernel `AgentLoopGuard` 纯状态机 + service `hooks.rs`（tool_result L1 截断 + 注入扫描 + 失败连击）经 `runs.rs` 两个注入点接入 agent loop（`with_extension_hooks(true)` 生产接线）。
4. **RED 两形态**（`02_red_tests/`）：base 提交点实现缺席（stash 后编译失败 ×2 + 首跑 7/7 失败）+ 新增断言对前位实现暴露 5 个真实缺陷（①-⑤，`RED.md` 表）；续作盘点再修 ⑥-⑨（台账 F 项）。
5. **事故与第二道防线**：`urldecode()` 无终止循环事故（见 §二）根因修复 + `lingxi-test-guard` 全测试面布防（见 §三）。
6. **回归/格式/静态检查/台账/报告/digest**（FIN2 终局）：定向 `cargo test -j 8 -p lingxi-service -p lingxi-adapters --locked` **exit 0 全绿**；`cargo fmt --all --check` exit 0；`cargo clippy -j 8 --locked --workspace --all-targets -- -D warnings` exit 0；全量 `cargo test -j 8 --workspace --no-fail-fast` 重跑（§九）；台账 D1-D17/R1-R5/F-①-⑨；digest 最后执行。

## 二、事故与根因修复（R06-T06 真实内存失控事件）

- **现象**：2026-10-10，`r06_t06_memory` 测试进程在 ~61 分钟内常驻内存涨到 ~176GB，耗尽 96GB 整机内存，macOS 看门狗强制重启整机（会话内两次拖死）。
- **根因**：`lingxi-service/src/memory.rs` 的 `urldecode()` 循环 catch-all 分支 `b => out.push(b as char)` **不推进游标 `i`**——任何含普通字节（非 `%`/`+`）的输入（如合法查询串 `agentId=agentA`）即无限循环、无限追加字符串。
- **修复**：逐字节解码，每个分支要么推进游标要么终止，结构上有界；附 3 个回归单元测试（合法串、含 `%` 序列、畸形输入均不失控）。修复前实测（外部监控，同一二进制）：12 秒 4.3GB 截杀；修复后连续 3 次：峰值 ~32MB、单次 ~0.4s、7/7 通过（`09_test_guard/fix_before_r06_t06_memory.txt` / `fix_after_r06_t06_memory.txt`）。
- **构建波次教训（同源纪律）**：事故复盘还实测了重建期并行 rustc 的构建内存——默认并行度 20 秒冲到 5.3GB 不收敛（27 个并行 rustc），`-j 8` 同波次峰值 3.9GB 且完成（`09_test_guard/build_wave_unbounded_default_jobs.txt` / `build_wave_bounded_j8.txt`）。此后本任务全部 cargo 命令带 `-j 8`。

## 三、test-guard 基础设施范围申报

- **新 crate**：`rust/crates/lingxi-test-guard/`（788 行 + 12 个单元测试）——`ctor` 预 main 钩子安装 50ms 采样线程，测试进程 RSS（macOS `task_info`/Linux `/proc/self/statm`）与墙钟超限即打印报告（含存活测试线程名）并 `abort()`（SIGABRT）。默认 **8GiB / 600s**（时间默认上市时为 300s，FIN2 收尾发现误伤既有最慢合法用例，见下）；`TEST_MEM_LIMIT_MB`/`TEST_TIME_LIMIT_SEC`（0=关）/`LINGXI_TEST_GUARD=0`（总开关）/`TEST_GUARD_VERBOSE` 可调，非法值响亮回落默认。说明文档 `docs/rust-tauri/R06/T06_TEST_GUARD.md`。
- **依赖**：唯一新增第三方 crate `ctor 0.8.0`（传递 `ctor-proc-macro 0.0.7`/`dtor 0.3.0`/`dtor-proc-macro 0.0.6`）；`libc 0.2.189` 复用锁内版本。Cargo.lock 净新增 5 条目（上述 4 + lingxi-test-guard member）。登记于 `DEPENDENCY_DECISIONS.md` D-12 与 `DEPENDENCY_RULES.json`（新 module 条目 + D5-reverse 成员）。
- **接线范围**：133/133 测试二进制含守卫符号（`coverage_scan.txt` nm 扫描）。接线方式 = kernel `test-guard` feature（仅 dev-dependencies 边开启）+ 不链接 kernel 的二进制一行 `#[cfg(test)] use lingxi_test_guard as _;`（protocol/spike/browser-spike/xtask bins、6 个 spawn-only 集成测试、config_bounds）。
- **release 零链接证据**：`release_zero_overhead.txt`——`cargo build` 全部产物与 `cargo build --release` 的 lingxi-service 等 10 个产物 nm 计数守卫符号 = 0；守卫只存在于测试构建。
- **FIN2 默认时限修正（如实申报）**：收尾验证中守卫以默认 300s 中止既有用例 `r04_t08_tool_matrix::approval_face_share_cases`（SIGABRT @300.02s，内存仅 37MB）。根因不是失控：该用例固有耗时 ~300.5s（T05 时代全量负载下 300.57s；候选 hooks-on 300.48s / hooks-off 300.46s——hooks 增量 ~0.02s 噪声级），300s 默认恰好卡在完成线下。处理：`DEFAULT_TIME_LIMIT_SEC` 300→600（已知最慢合法用例 ~2 倍富余；事故现场失控为 61 分钟，10 分钟触发仍远在其前），同步 crate 文档、`T06_TEST_GUARD.md` §3、D-12；证据 `09_test_guard/guard_time_default_incident.txt` + 三个单跑日志。**注**：第三棒的 `09_test_guard/full_suite_result.txt` 摘要只记录了 PTY flaky 一项失败，漏记了同轮 r04_t08_tool_matrix 的该次守卫中止（被 SIGABRT 的二进制无 test result 行，按行统计会漏计）——FIN2 已在归档与终局重跑中修正口径。
- **诚实边界**：完全不引用被测 crate 的合成测试二进制不会被 rustc 链接守卫（crate 文档写明）；构建期并行 rustc 不在进程守卫范围内（以 `-j 8` 纪律约束）。

## 四、已交付的 Deliverables

- **kernel 纯逻辑** → `rust/crates/lingxi-kernel/src/memory.rs`（2220 行）：tenets 配额（20/200/30）/去重/排序、检索文本构造（CJK 2/3-gram）、FTS query 构造、SKILL.md frontmatter 元数据、技能名净化、风险确认 token 协议（绑定 sourceKey+sha256(content)、TTL 10min、单次消费）、workspace 技能遮蔽三态、注入扫描规则、AgentLoopGuard 状态机、PII 脱敏模式、L1 head/tail 截断语义。
- **adapters 存储** → `storage/agent_memory.rs`（2287 行：五 store + context notes + persona/avatar + config 行级 patcher + dream revisions）+ `storage/skill_library.rs`（1019 行：目录扫描/安装包结构校验/bundles store）。
- **service 面** → `src/memory.rs`（1338 行，MemoryService + HTTP handlers）、`src/skills.rs`（1502 行，SkillService + guard 槽 + handlers）、`src/persona.rs`（333 行）、`src/hooks.rs`（240 行，ExtensionHooks + tool_result_guard）、`src/memorytools.rs`（774 行，五工具执行器 + 注册）。
- **接线**：`lib.rs`（bootstrap 构造三服务 + 工具注册 + guard 槽 + hooks 开关 + 40 余条路由）、`auth.rs`（T06 全面分类块）、`runs.rs`（hooks 两个注入点）。
- **测试**：`r06_t06_memory`（7）/`r06_t06_skills`（6，R1 修复轮 +1：guard 槽缺席 fail-closed 直接断言）/`r06_t06_hooks`（4）/`r06_t06_skill_context`（4，R1 修复轮新增：技能材料进上下文生产链 ×2 + 运行时目录解析回归 + 工具注册响亮失败）/`agent_memory_r06t06`（14）+ auth `route_policy_table` T06 断言族（lib 单测增量）。
- **文档**：`T06_TEST_GUARD.md`（新）、`R06_LEAF_MAP.json` 101 叶标注、`PI_REPLACEMENT_MATRIX.json` T06 份额标注、`docs/README.md` 索引行、D-12 登记。

## 五、实际生产调用链（无平行构造者）

1. **bootstrap（lib.rs）**：storage/runs（v9 冻结不动）→ `MemoryService::new(data_home, runtime_dir)` → `SkillService::new(data_home, product_dir)`（product_dir 复用 T01 `resolve_product_dir` 链）→ `PersonaService::new(memory.agents_root(), product_dir)`（与记忆同 agents 根，文件即 T01 材料链读取的同一批）→ 真实模型链时 `memorytools::register_memory_tools(&registry, gateway, memory, skills, primary_agent, clock, &budget)`（lib.rs:1288-1296，**构造时绑定 `resolve_primary_agent()` 单一 agent，无跨 agent 句柄**）+ `skills_service.set_guard(GatewaySkillGuard::new(AuxiliaryExecutor::new_with_network(...), primary_agent))`（lib.rs:1454-1458，复用同一网关/凭证/网络面；无链时 fail-closed）→ `runs.with_extension_hooks(true)`（lib.rs:1469）。
2. **路由注册（lib.rs:5684-5862）**：记忆面（:5684-5705）→ agents 人格/tenets/experience/config 面（:5707-5747）→ 人格文件面（:5749-5782）→ skills 面（:5809-5855）→ 会话上下文笔记（:5856-5862）。
3. **请求路径**：auth `classify_route`（auth.rs :775-963 T06 块，镜像 route-security.ts scope 表）→ handler（agentId 归属闸先于一切实体读取）→ MemoryService/SkillService/PersonaService → adapters store。
4. **agent loop 注入（runs.rs）**：每 run 新建 `ExtensionHooks`（:1263-1266，agent_start reset 语义）→ 派发前 loop-guard Block（:2263-2281，Block 转 Failed 工具结果、**零执行器调用**）→ `record_tool_result!` 宏内 `tool_result_guard`（:1276-1317，警告前置/L1 截断/失败连击；错误结果豁免截断扫描）。
5. **上下文入口**：T01 FileContextMaterialSource 读 tenets.json/memory.md/identity/AGENTS.md——本任务交付写路径（写后 T01 下一次编译即读到）；**技能材料**（R06-T06 R1 F-01 修复接线，原 §五.5 表述失实已修正）：编译期经 `SkillMaterialPort` 端口由 SkillService 解析运行时技能选择（启停、白名单、工作区边界、用户覆盖顺序；context_compiler.rs:117/:887-897 + skills.rs:932-981 + lib.rs:1461-1475），条目作为**来源材料**进 T01 冻结 `skill.catalog` 段，无任何冒充 system 高优先级指令的通道。

## 六、代码修改清单

**修改（M，41 文件中除 lock 外均为接线/实现所需，逐一盘点）：**
- 实现接线：`lingxi-service/src/{lib.rs(+470 区段：模块声明/ServiceState 字段/bootstrap 接线/路由/accessors), auth.rs(+193+253 分类块与断言), runs.rs(+67 hooks)}`、`lingxi-kernel/src/lib.rs(+11 模块导出+guard 引用)`、`lingxi-adapters/src/storage/mod.rs(+2)`。
- test-guard 接线：`rust/Cargo.toml(+5 member)`、`Cargo.lock(+5 条目)`、8 个 crate 的 Cargo.toml（dev-dependency/feature 边）、protocol 3 bins + lib、spike 6 bins + lib、browser-spike 2 bins + lib、xtask main、service r04_t07_fixture bin、6 个 spawn-only 集成测试与 config_bounds 各 +4 行——全部 `#[cfg(test)]` 门控。
- 文档：`docs/README.md`（+1 索引行）、`PI_REPLACEMENT_MATRIX.json`（T06 份额字段）、`R06_LEAF_MAP.json`（101 叶 t06_status/t06_anchor）、`DEPENDENCY_DECISIONS.md`（D-12）、`DEPENDENCY_RULES.json`（test-guard 条目 + D5-reverse 成员）。

**新增（13 文件，13120 行）：** kernel/memory.rs；adapters storage/{agent_memory,skill_library}.rs；service src/{memory,skills,persona,hooks,memorytools}.rs；lingxi-test-guard crate；测试 4 文件（见 §四）。

**FIN2 收尾修改（如实申报，全部在本任务候选内）：** fmt 重排 17 个 T06 文件（rustfmt 语义保持；共享文件 lib.rs/auth.rs/runs.rs 的重排均落在 T06 新增块内，hunk 亲验）；clippy 修复 17 处（doc 缩进 2、`and_then` 替 map+flatten 6、`&mut [T]` 替 `&mut Vec` 1、format-args `to_string` 1、needless_borrow 1、assert bool 3、slice-from-ref 1、type alias 2、collapsible if 1），其中 6 处 `result_large_err`/`too_many_arguments` 按 fsread.rs/auxiliary.rs 既有先例用带理由的定向 allow；guard 默认时限 300→600（§三）；`DEPENDENCY_RULES.json` 笔误修正（D-10→D-12）。

**未触碰的既有收口义务（原样保留）：** T03 R2-F-01~04、T04 F-01/F-02、T05 F-01/F-02；`.vscode/` 未动、永不入候选；T01-T05 叶与既有路由/迁移链零语义改动（v1-v9 冻结，无 v10——context notes 落运行时目录文件，台账 D2/R1）。

## 七、现役语义锚定与记录在案差异

全部差异逐条锚定现役 file:line 于 `07_diff_ledger/diff_ledger.md`：**D1-D20**（路由前缀现代化；context notes 运行时目录；dream ticker 缺席 503/restore 无 pending journal/prune 无 retain；技能包格式收窄 zip/.skill/github 显式拒绝；skill_content 分支拒绝；tenets 写面 LocalOnly；skill-bundles PATCH 分类；SKILL.md 注册子集解析器；experience stem sha256 后缀；agent config PATCH 收窄 T06-owned 键；config.yaml 行级 patcher；search_memory 语义分支如实 not_configured；health 步骤面收窄；guard 失败路径同 token 协议（R1 收窄锁定口径）；workspace 遮蔽判定源；R1 补记：技能目录装配进上下文接线 D18 / avatar ETag-304 缺席 D19 / HTTP 手动安装 agentId 连带启用未接线 D20）。**R1-R5** 实现细化（v10→运行时目录、dream 拆分、guard 槽生产接线、hooks 单点注入、工具 target 本地名解析）。**F-①-⑨** RED→GREEN 修复留痕；**F-⑩-⑫** 为 R1 审查修复轮（见 §十七）。现役锚点亲验于 base 工作区（RC-2），设计稿 `01_design/design.md` §3 全集。

## 八、新增测试（30 集成 + R1 修复轮 5 = 35 集成 + lib 断言增量 + guard 12 单元，全部通过）

- `r06_t06_memory`（7，真 HTTP）：A11 三件（读写隔离 / tenets+compiled per-agent / 跨 agent 与非法 id 拒绝）+ dream 缺席与 revisions + experience 闸 + 人格回落链与广播 + context notes 往返。
- `r06_t06_skills`（6，真 HTTP）：A11 工具构造期单 agent 绑定 + A12 第一层 token 协议 7 步全链 + A12 第一层 guard 槽缺席 fail-closed（R1 修复轮 F-02）+ A12 第二层网关越权拒绝 + safe 审核直装 default-enabled:false + HTTP 面（白名单/遮蔽/readonly/bundles + R1 补断言：同名用户池副本展示自身状态）。
- `r06_t06_skill_context`（4，R1 修复轮）：技能材料进上下文生产链正反两腿（bootstrap→run→compile + 真实 FileContextMaterialSource，F-01）+ 运行时目录解析回归（白名单序/disable-model-invocation/遮蔽去重/空 config workspace policy）+ 记忆工具注册响亮失败（F-03）。
- `r06_t06_hooks`（4，run 级）：第 7 次同参调用派发前阻断 + 无 hooks 对照腿逐字节执行 + tool_result L1/扫描/警告序 + 注入扫描规则表。
- `agent_memory_r06t06`（14，存储层）：facts/tenets per-agent 物理隔离与跨 id 拒绝、compiled 装配金样与 reset/clear/legacy 迁移、experience 形状、dream restore 安全修订（before=被替换状态 + unchanged 短路）、config patcher 保无关节、PII 金样、persona 回落与 avatar、技能目录校验与 bundles CRUD/scrub。
- auth `route_policy_table`：T06 全部新路径 × 多动词分类断言族（lib 单测增量）。

## 九、现有回归测试（真实命令与退出码）

| 命令（rust/ 目录，`/Users/study_superior/.cargo/bin/cargo`） | 退出码 | 证据 |
|---|---|---|
| `cargo fmt --all --check`（注：cargo fmt 不支持 `-j`，纯源码级不产生构建波次） | 0 | `03_fmt/fmt_check.txt` |
| `cargo clippy -j 8 --locked --workspace --all-targets -- -D warnings` | 0 | `04_clippy/clippy.txt` |
| `cargo test -j 8 -p lingxi-service -p lingxi-adapters --locked` | **0（全绿）** | `05_targeted_tests/targeted_suites.txt`：109 二进制 / 1458 用例 / 1458 通过 / 0 失败 |
| `cargo test -j 8 --workspace --no-fail-fast` | 见下 | `06_workspace_regression/` |

- **全量终局（FIN2 重跑，最终候选字节）**：`cargo test -j 8 --workspace --locked --no-fail-fast` **exit 101 = 恰好 1 个用例级失败**：133 二进制 / 1816 用例 / **1815 通过 / 1 失败**，唯一失败 = `r00_management_positive_and_negative_branches_on_real_service`（LAN 地址入站被防火墙/TUN 间歇拦截的既有环境项；**单跑复核 PASS** 118.84s；同哈希二进制在定向终局轮与 T05 时代全量均通过）。守卫触发 0（approval_face_share_cases 在 600s 默认下正常完成——时限修正生效）。证据 `06_workspace_regression/full_workspace_final.log`（SHA-256 076ae4d3…）+ `full_workspace_result.txt`。
- **前棒全量（/tmp/full-suite3.log 归档）**：133 二进制 / 1804 用例 / 1803 通过 / 1 失败（r04_a10_pty PTY 负载型 flaky，单跑 0.59s 通过，T05 审查已记录同族；与 T06 无关）+ 1 处被摘要漏记的 r04_t08 守卫中止（300s 误伤，§三）；r00 LAN 环境项该轮通过。归档于 `06_workspace_regression/full_suite_prev_run.log`，口径差异逐条写明。
- FIN2 定向重捕前对同一代码的中间轮（post-fmt/pre-clippy-fix）：除上述 r04_t08 守卫中止外全绿（r00 LAN 项 111.13s 通过）。
- 任务起点水位（T05 终局 1749）→ 终局全量（见上）：只增不减；T06 新增 30 集成 + lib 断言增量 + guard 12 单元。

## 十、R06-A11 证据（Agent 记忆不串用）

- **存储层物理隔离**：`a11_facts_are_physically_isolated_per_agent`（A/B 各自 facts.db，A 写入对 B 不可见）；`a11_tenets_are_isolated_per_agent`；`a11_cross_agent_ids_are_rejected`（`[\/.]` 形 id 拒绝）。
- **HTTP 层归属边界**：`a11_memory_read_write_isolated_between_agents`（A/B 各写各读一致；import 到 A 不出现在 B；B 的 DELETE 不清 A）；`a11_tenets_and_compiled_memory_scoped_per_agent`；`a11_cross_agent_and_invalid_ids_are_rejected`（agentId 缺失 400、非法 400、未注册 404；未列出的 memories 变体 fail closed → LocalOnly）。
- **工具面单 agent 绑定**：`a11_memory_tools_are_bound_to_one_agent_at_construction`（构造期绑定 primary agent 的目录，无跨 agent 句柄——镜像现役 `core/agent.ts:1045-1047` 同一 FactStore 语义）。
- **鉴权分类**：auth route_policy_table 记忆面/agents 人格面全族断言（读 settings.read / 写 settings.write / 未注册 LocalOnly）。

## 十一、R06-A12 证据（技能审核与执行边界双层有效）

- **第一层（审核）**：`a12_layer1_install_review_token_protocol_end_to_end` 7 步全链——越权技能首次安装 → guard 审查 `requiresRiskConfirmation` + 一次性 token；`risk_accepted=true` 无 token → `missing_confirmation_token`；token+内容已变 → `confirmation_target_changed`；过期 → `invalid_or_expired_confirmation_token`；带 token 同内容重试 → 安装成功。token 绑定 `sourceKey + sha256(skillContent)`、TTL 10min、单次消费（镜像 `install-skill.ts:230-279`）。
- **第一层（默认禁用）**：`a12_layer1_safe_review_installs_directly_marked_not_default_enabled`（metadata 重写 default-enabled:false）。
- **第二层（执行）**：`a12_layer2_gateway_refuses_out_of_workspace_skill_action`——安装后的技能经 R04 统一网关 `exec_command` 请求越 workspace 路径 → 网关拒绝（授权沙盒不变，技能内容不改变工具边界）。
- **guard 失败路径**：guard 槽缺失/不可用 → fail-closed 同 token 协议（D16，`Unavailable` → requiresRiskConfirmation，绝不静默放行）。**R1 修正**：该分支在交验候选中仅有实现、无直接测试（审查 F-02；D16 原「锁定测试」声明收窄为 token 协议链）——修复轮补 `a12_layer1_guard_slot_absent_fails_closed_to_risk_confirmation` 直接断言（缺席 → requiresRiskConfirmation + token + 不落盘 + 原因浮出 + 显式接受后放行），并以变异运行（None 分支临时改 Safe → 既有套件全绿）证明修复前零覆盖（`02_red_tests/repair_R1_F02_red_mutant.log`）。

## 十二、R00 叶结果（101 叶 = 62 implemented / 18 share / 21 deferred）

`docs/rust-tauri/R06/R06_LEAF_MAP.json` tasks["R06-T06"].leaves 全部 101 叶标注 `t06_status`/`t06_anchor`（T01-T05 叶零触碰；PI_REPLACEMENT_MATRIX.json 同步标注 PI-01/04/05/09/10 份额与 extension_hooks 逐 hook 归属）。deferred 目的地分布：R07×14（dream ticker/自动调度、事件广播、agent 生命周期路由、bridge 等）、R09×2 + R08/R09×3 + R07/R08/R09×1（桌面份额）、其余 1 项 R07。**待裁决项**：无新增 J 项——deferred 叶全部自带 stage 归属（与设计 §6 预分类一致）；对现役的行为差异 D1-D17 与实现细化 R1-R5 已逐条申报待独立审查复核，其中需审查者知悉的三个治理性选择：D2（context notes 存储位置变更）、D8（tenets 写面从现役 STUDIO_OWNER 兜底收窄为 LocalOnly）、guard 默认时限 300→600（§三）。

## 十三、接口兼容结果（R02–R05 / T01–T05）

- 定向终局 **exit 0 全绿（1458/1458）**，含既有 r00-r05 全部套件与 r06_t01-t05 全部套件；终局全量 **1815/1816（唯一失败为 LAN 环境项，单跑复核通过）**，既有套件零回归。
- 全部新增面在 `/lingxi/v1/` 前缀下；既有路由表零改动；auth 既有分类不变（T06 块为纯新增）。
- runs.rs：`extension_hooks` builder 默认 **false**——全部既有测试与 pre-T06 run 路径零变化（r06_t06_hooks 对照腿断言逐字节执行）；仅生产 bootstrap 显式开启。
- 存储链：runs.db v1-v9 冻结未动（无新迁移）；T01 材料链读取的文件即本任务写路径产出的同一批文件（回落链测试覆盖）。
- T03/T04/T05 收口义务项未顺手修（按授权保留原状）。

## 十四、未验证事项

- 真实付费供应商端到端（记忆工具在真实模型流中的行为）未验证——与全仓先例一致，本地 stub 供应商验证协议层；真实供应商验证属发布前口径。
- 桌面 IPC/查看器份额（deferred 叶的 R08/R09 面）未在本任务验证——物理归属 R09。
- dream 自动调度/ticker、事件广播、learn_lesson/show_card 工具、desk 写面、组合导出、autolearn 顺延 R07（deferred 叶）。
- `r04_a10_pty_*` PTY 负载型 flaky 与 `r00_management_leaves` LAN 环境项的本机环境敏感性维持已知记录（非 T06 范围）。
- 守卫对 Linux `/proc/self/statm` 采样路径仅代码审查 + 单元测试（本机为 macOS），跨平台实测属 R10 打包口径。

## 十五、已知风险

- 守卫时间默认 600s 对未来更慢的合法用例仍可能误伤——校准依据是"当前全量最慢合法用例 ~300.5s 的 2 倍"；新增长跑用例应显式设 `TEST_TIME_LIMIT_SEC` 或拆分。
- `approval_face_share_cases` 固有 ~300.5s 是全量回归的时长瓶颈（单二进制占 ~300s）；后续阶段可考虑将其等待循环事件化（R04-T08 范围，本任务未动）。
- D3/D4/D5：dream 自动调度与 restore 崩溃恢复（pending journal）缺席——restore 返回真实结果或错误，但单进程内中断的 restore 无自动回滚；顺延 R07 申报。
- D14：语义检索 `not_configured`——embedding 表已建（schema v3）但检索未接线；若 R07 接线需复核维度口径。
- 6 处 `clippy::result_large_err`/`too_many_arguments` 定向 allow（带理由注释）沿既有先例；若后续统一 Box<Response> 需全仓一起做。

## 十六、候选范围申报

- 候选 = 本报告 §六 全部 M/新增文件 + `artifacts/rust-tauri/R06/T06/`（00-09 全部证据）+ `docs/rust-tauri/R06/{R06-T06_REPORT.md, T06_TEST_GUARD.md, R06_LEAF_MAP.json}` + `docs/rust-tauri/R00/PI_REPLACEMENT_MATRIX.json` + `docs/rust-tauri/R01/{DEPENDENCY_DECISIONS.md, DEPENDENCY_RULES.json}` + `docs/README.md` + `rust/` 全部源/测试/lock 改动。
- **不在候选语义内但客观在 digest 哈希输入里（如实申报）**：`.vscode/settings.json`（本地编辑器噪声，未被 .gitignore 排除，故进入 `git ls-files --others` → digest 输入；任务纪律：永不作为交付内容，审查时忽略）；`/tmp/*.log`（工作区外临时区，不进 digest；关键日志已归档 artifacts，`full_suite_prev_run.log` 为归档副本）。
- digest 由 `scripts/rust-tauri/r06_candidate_digest.sh` 最后运行，值在 handback，本报告不内嵌。

## 十七、R1 独立审查修复轮（REPAIR-R06-T06-R1）

审查：`artifacts/rust-tauri/R06/T06-REVIEW-01/REVIEW.md`（VERDICT: PASS，附 F-01 MEDIUM 合并前必修；R1 审查 digest `7b4e79110c9dcc2938a21518b8e2c912a77dd2fabf4e74f9ec75ea6871b6d5b9`）。本轮按「先 RED 后修复、同根因横向扫清、账本如实回写」执行：

### F-01（MEDIUM）技能材料未进入模型上下文 → 已接线（差异台账 D18）
- **根因**：T06 叶提取以路由/工具面为粒度，漏掉「技能目录装配」这一现役行为（现役把该行为折收在 ENABLE/DISABLE/PUT 三叶的「刷新运行时/prompt」断言里，R00 提取无独立 skill.catalog 叶）；T01 移交的悬挂依赖（T01 差异项⑥「skill.catalog 生产默认空，SkillService 归 R06-T06」）未被盘点接住——`context_compiler.rs` 的 `ContextCompileInput.skills` 恒空，`resolve_runtime_selection` 只有 HTTP 视图消费者。
- **修复**（接线，非申报顺延）：①`context_compiler.rs:117` 新增 `SkillMaterialPort` 端口，`compiled_for_run`（:887-897）在材料采集后经端口解析该 agent 的运行时技能目录——解析失败与材料读取失败同权响亮；②`skills.rs:932-981` `runtime_skill_catalog`：启停白名单 + 现役 `|| []` 空 config 形状 + workspace policy/遮蔽契约 + `disable_model_invocation` 服务侧过滤（pi skills.js:276 在冻结渲染前过滤的等价承担），条目为 name/description/SKILL.md location 的**来源材料**；③`lib.rs:1461-1475` bootstrap 把 SkillService 绑定为生产编译器的技能材料端口。语义槽位镜像现役 session-coordinator.ts:2102-2110 → pi agent-session.js:784-789 → pi skills.js:275-298；条目只走 T01 冻结段注册表（digest_only wrapper 带 + 预算钳制），不新造特权通道。
- **配套真缺陷**（新回归测试在 GREEN 前发现）：`resolve_runtime_selection` 活跃集为纯名字表，按名过滤目录会把同名遮蔽副本双激活 → skill_library.rs:228-305 增加 `active_entries`（解析序活跃条目本体）；`list_for_agent` 状态配对从按名 map 改为按条目位置配对（skills.rs:388-424；同名用户池副本不再吃到 workspace 副本的 shadowed 状态——现役 getRuntimeSkillInfos 逐条映射语义）。
- **RED**：`02_red_tests/repair_R1_F01_F03_red.log`——`enabled_skill_catalog_reaches_the_production_system_prompt` 失败（真实 bootstrap + 真实 FileContextMaterialSource + 真实 run 驱动，发送文本无 `<available_skills>`）。**GREEN**：`05_targeted_tests/repair_R1_green_skill_context_skills.log` + `repair_R1_targeted_suites.log`。**回归测试**：`r06_t06_skill_context`（4 用例，见 §八）。
- **横向扫描**：`ContextCompileInput` 恒空槽仅 skills 属「服务已建、零编译消费」。`append_system_lines`（现役 appendSystemPrompt 组装）与 `project_context_files`（现役 resourceLoader.getAgentsFiles）在候选**无对应已建服务**（deferred-result 归 R07、工作区文件发现无 owner）——维持 T01 差异登记，不新造生产者不伪造来源；`compute_default_enabled`（新 agent 默认白名单）已建但生产消费者（agent 创建面）归 R07，为 helper 先行、消费接线时需复核（非静默缺口）。静默注册横向扫（events/toolgateway/cancel 全部 `.register(` 调用）无第二处静默跳过。
- **失实表述修正**：本报告 §五.5 以修后实态重写（原文声称「技能内容按来源进入上下文由既有编译器承担」在交验候选中不成立）；design.md §2 上下文入口同步改写并加 R1 修订声明；叶图 ENABLE/DISABLE/PUT 三叶 t06_anchor 回填编译消费锚（叶数维持 101）；PI-04 `r06_t06_progress` 补接线锚；台账 D18 记录本差异现役/候选锚点。

### F-02（LOW）guard 槽缺席 fail-closed 分支零测试 → 已补直接断言（D16 口径收窄）
- **根因**：RED 阶段以注入 stub 驱动协议链，缺席分支未被盘点为独立断言点；D16「锁定测试」声明过宽。
- **修复**：新增 `a12_layer1_guard_slot_absent_fails_closed_to_risk_confirmation`（r06_t06_skills.rs）——无 guard 下越权包安装 → `requiresRiskConfirmation=true` + token + 不落盘 + 缺席原因浮出 + 同 token 显式接受后放行（fail-closed ≠ 拒绝服务）。D16 锁定声明收窄为「token 协议链」，缺席分支由新测试锁定。
- **RED 证据**（变异运行，取证后已复原）：把 `None => Unavailable` 临时改为 `None => Safe`，既有套件 5 通过（缺口证明）、新测试转红（安装直通 `"ok":true`）——`02_red_tests/repair_R1_F02_red_mutant.log`。

### F-03（LOW）memorytools 注册失败静默跳过 → 已响亮化
- **根因**：`if let Ok(receipt)` 丢弃 Err，与 lib.rs:1284-1287 注释「Registration failure is a loud startup error, never a silent skip」及 filetools.rs `.expect` 先例矛盾。
- **修复**：`.expect("the resident memory/skill tools register")`（memorytools.rs:753）。
- **RED**：`repair_R1_F01_F03_red.log`——`memory_tool_registration_failure_is_loud_not_silent` 期望 panic 实际 Ok（静默跳过实证）。**回归测试**：同名测试（重复注册必须 panic 且消息点名注册失败）。

### OBS-1（顺手修复）
- 差异台账补 **D19**（avatar ETag/304 缺席）；叶 E4BEE8 的错引「→ D13」修正为 → D19。
- 同类错引清扫：叶 SKILL-INSTALL-AND-ENABLE 的「→ 差异台账 D11」（D11 实为 experience stem 条目）修正为 → 新增 **D20**（HTTP 手动安装 agentId 连带启用未接线，share 半面如实登记）。
- R1 修复导致的 skills.rs/skill_library.rs 行号漂移：受影响 20 条叶 anchor 已按修后实态校正。

### 修复轮验证（rust/ 目录，`/Users/study_superior/.cargo/bin/cargo`，全程 `-j 8`，test-guard armed）

| 命令 | 退出码 | 证据 |
|---|---|---|
| `cargo fmt --all --check` | 0 | `03_fmt/repair_R1_fmt_check.txt` |
| `cargo clippy -j 8 --locked --workspace --all-targets -- -D warnings` | 0 | `04_clippy/repair_R1_clippy.log` |
| `cargo test -j 8 -p lingxi-service -p lingxi-adapters --locked` | 101（唯一失败 = r00_management_leaves LAN 环境项，50/109 二进制后早停；--no-fail-fast 复跑见下） | `05_targeted_tests/repair_R1_targeted_suites.log` |
| 同命令 `--no-fail-fast` 复跑 | 见 `repair_R1_targeted_nonff.log` | 同目录 |
| `cargo test -j 8 --workspace --locked --no-fail-fast` | 见 `06_workspace_regression/repair_R1_full_workspace.log` | 同目录 |

- **r00_management_leaves LAN 环境项如实记录**：修复轮窗口内该用例连续 3 次失败（定向 + 2 次单跑复核），失败形态均为 192.168.3.5（LAN 自地址）入站 TCP 交换停滞（0 bytes read，测试自身诚实报为环境失败）——与 T06 交验轮 §九 记录的「LAN 地址入站被防火墙/TUN 间歇拦截、单跑复核通过」同源；R1 审查人全量轮该用例通过。本轮窗口 TUN/防火墙持续拦截，单跑复核未能复现通过态；该用例与 T06 修复改动无耦合（失败点在 OS 网络层入站，早于任何 T06 面），以全量 `--no-fail-fast` 结果为其在本窗口的最终环境数据点。
- fmt/clippy 修复后全绿；除 LAN 环境项外定向复跑无任何失败（T06 五套件 7+6+4+4+14 全绿；r06_t01 编译器套件 21/21 零回归）。
