# R06-T01 上下文编译器与短提示词 — 实施报告

- **TASK_ID**: R06-T01
- **CANDIDATE_SHA**: `c9cca6020e0990016ee0d14af406766370849a7a`（分支 `codex/rust-tauri-migration`，任务基点；本任务改动**未提交**，以工作区形式交付）
- **CANDIDATE_DIGEST**: `6dc9f4c2050faa501bd6360b863f8e81224e3e1e9a750894ffdfdfde9273b183`
  - 口径：`{ git diff; git ls-files --others --exclude-standard | sort | grep -v '^docs/rust-tauri/R06/R06-T01_REPORT.md$' | xargs shasum -a 256; } | shasum -a 256`
  - 即：已跟踪改动的完整 diff + 全部未跟踪文件（除本报告自身）的逐文件 sha256，再取总 sha256。计算时点：全部测试与证据归档完成之后、本报告写入时。
- **执行代理**: EXECUTOR-R06-T01（一次性执行代理，仅本任务）
- **工具链实测**: rustc/cargo **1.93.0**（Homebrew）、rustfmt 1.9.0-stable。任务书写 1.98.1，与实际不符，按任务书"以实际为准"记录于此。
- **锁文件**: `rust/Cargo.lock` sha256 = `259f983e98a4da13eab1b592c2efd7b79579ad6678a08995a4b9e3fca618eff3`，任务前后逐字节一致，未新增任何第三方依赖（含 dev-dependencies）。

---

## 一、已完成的 Steps（任务书五步逐条核对）

| # | 任务书步骤 | 完成情况 |
|---|-----------|---------|
| ① | 从 core/agent.ts 及现役 prompt 测试提取系统约束/人格/工具目录/记忆/技能/资源/会话历史的实际组成，保留顺序与作用域 | 完成。现役装配链（`buildSystemPromptArtifact` preamble + SDK `buildSystemPromptSections` wrapper）逐段提取为 24 段闭合注册表，顺序、可见范围（main/shared）、披露级别（full/digest_only）、装配带（preamble/wrapper）全部保留；现役 token 实测证据存 `artifacts/rust-tauri/R06/T01/incumbent-budget-measurement.raw.txt`。 |
| ② | 建立 ContextArtifact（每段有来源、可见范围、优先级、token 预算、脱敏标记；最终请求直接由 artifact 渲染） | 完成。kernel `ContextArtifact`（`rust/crates/lingxi-kernel/src/context.rs`）：每段携带注册表元数据 + 渲染区间 span + token 实测；最终发往模型的 system 文本 = `artifact.render()`，观测 = 同一 artifact 的 `observation_view()` + service 层 sha256 装饰，无第二套重建。 |
| ③ | 静态提示前缀稳定，动态内容与工具目录按原规则载入，不可把全部工具说明塞回常驻提示词 | 完成。静态带 = agent.roster 及以前 16 段（含 platform.* 固定文案），动态尾 = memory.*/session.time + 4 个 wrapper 段；`static_band` 标记与 cache 分界线在注册表显式声明并被测试锚定。工具目录**不在**系统提示词内（现役亦如此：工具经 R04 网关的工具声明通道下发），skill.catalog 段生产默认空、预算 3000 封顶，未塞回任何工具说明。 |
| ④ | 保留中英文及现役语言策略，子代理上下文按权限裁剪，源材料里的指令不提升为系统指令 | 完成。locale 解析链 = config.yaml → preferences.json → "en"，zh/en golden 逐字节锚定；子代理裁剪丢弃全部 scope=main 段（appearance/subagent-collaboration/roster/memory.*）；用户派生内容（user.md、memory、tenets、AGENTS.md、identity）全部以 digest_only 段材料身份进入，**不作为**平台指令文案，观测视图不携带其正文。 |
| ⑤ | 冻结迁移前后模板差异，只允许修复契约所必需的改动 | 完成。差异清单冻结于本报告 §十，共 18 条，每条注明原因与归属；现役回归测试仅 2 文件 4 处位置断言因 system 槽恒在首位而 +1（语义断言零改动），属契约必需的适配。 |

## 二、已交付的 Deliverables

| 交付物 | 位置 | 说明 |
|--------|------|------|
| **ContextCompiler** | `rust/crates/lingxi-kernel/src/context.rs`（编译器，纯确定性）+ `rust/crates/lingxi-service/src/context_compiler.rs`（材料源 + 会话级冻结缓存 + 观测 digest 装饰） | kernel 侧无 IO、无时钟依赖（时间戳由入参给入），同输入同输出；service 侧负责 $LINGXI_HOME 材料采集与会话冻结。 |
| **ContextArtifact** | `rust/crates/lingxi-kernel/src/context.rs` | `render()`（发往模型的文本）与 `observation_view()`（观测 JSON，schema `lingxi.context-observation.v1`）同源同构；service `CompiledContext` 在构造时对每段算 sha256 并写入观测（顶层 `renderSha256` + 逐段 `sha256`）。 |
| **提示词差异报告** | 本报告 §十 | 18 条迁移差异冻结清单。 |

## 三、实际生产调用链（无平行构造者）

```
main.rs:511-525  组合根
  resolve_product_dir(LINGXI_PRODUCT_DIR?, exe 上溯, <home>/product)
  probe_host_facts(workspace_root, home)            ← uname/$SHELL/平台标签
  FileContextMaterialSource::from_home(home, product_dir, host)
  ContextCompilerService::new(source)  →  ServiceDeps.context_compiler
        │
lib.rs:1263  bootstrap: runs.with_context_compiler(deps.context_compiler)
        │
runs.rs:1072  drive_run：provider 早退（无供应商）之后、turn 循环之前
  compiled_for_run(session_id, agent_id, for_subagent, now_ms)
    ├─ 会话缓存命中 → 复用冻结 artifact（同会话跨 run/turn 不变）
    └─ 未命中 → gather 材料 → kernel 编译 → 缓存（key=(session_id, for_subagent)）
  失败 → RunFinish::Failed{ProviderFailed{code:"context_compile: {err}", retryable:false}}
        → adjudicated_finalize（镜像 no-provider 早退语义）
  成功 → runs.rs:1247  每个 turn：ModelTurnInput.system_prompt = Some(artifact.render())
        → GatewayedProvider → openai-completions 线 messages[0]={role:"system"}
        │
观测读面：GET /lingxi/v1/sessions/{id}/context-observation
  auth.rs:575  classify_route → RoutePolicy::Scope("chat")
  lib.rs:2920  session_context_observation（handler 内 owner 复检，照抄 get_session）
  → observation_for_session（主形态 false 优先）→ 同一 CompiledContext 的观测视图
  无缓存（该会话尚未驱动 run）→ 404
```

唯一编译点 = drive_run 内 `compiled_for_run`；唯一渲染消费点 = turn_input.system_prompt；唯一观测读点 = observation_for_session。Node/Pi 侧零改动（git diff 中无 desktop/、core/ 文件），未新增任何平行 Agent Loop 或隐藏上下文构造者。

## 四、代码修改清单

**新增（生产）**
- `rust/crates/lingxi-kernel/src/context.rs`（约 1000 行）：token 估算（与现役 `estimateTextTokens` 同口径：CJK×1.1、其余 /4、ceil，按码点迭代）、24 段闭合注册表 `SEGMENT_SPECS`、`TOTAL_SYSTEM_TOKEN_BUDGET=25_068`、编译输入/输出类型、`compile()`（逐段预算钳制，超预算=响亮 `ContextCompileError`）、`ContextArtifact::{render, observation_view, preamble_tokens, static_band_tokens}`、子代理裁剪。
- `rust/crates/lingxi-service/src/context_compiler.rs`（约 760 行）：`ContextMaterialError`/`ContextCompileFailure`、`MaterialQuery`、`ContextMaterialSource` trait、`HostFacts`/`platform_note()`/`probe_host_facts`、`resolve_product_dir`、`format_session_started_label`（chrono Local，GMT±H[:MM]）、`FileContextMaterialSource`、`scan_agent_config`/`scan_preferences`（窄扫描，pub）、`CompiledContext`（构造时 sha256 装饰观测）、`ContextCompilerService`（Mutex<HashMap<(session,for_subagent), Arc>> 会话冻结缓存）。

**修改（生产，最小接线）**
- `rust/crates/lingxi-kernel/src/lib.rs`：`pub mod context;`（1 行）。
- `rust/crates/lingxi-service/src/lib.rs`：mod 声明；`ServiceDeps.context_compiler` 字段 + Default None；组合根 `with_context_compiler`（L1263）；`ServiceState` 字段/构造/访问器；`session_context_observation` handler（L2920，鉴权与 owner 复检照抄 get_session）；路由注册（L4001-4002）。
- `rust/crates/lingxi-service/src/main.rs`：组合根构建块（L511-525）。
- `rust/crates/lingxi-service/src/auth.rs`：路由分类表文档行（L41）+ `context-observation` 后缀分类 → `Scope("chat")`（L573-575）。
- `rust/crates/lingxi-service/src/runs.rs`：`context_compiler` 字段（L588-592）+ Debug + 两构造函数 None + builder `with_context_compiler`（L722）；drive_run 编译块（L1072 起）；turn_input `system_prompt: system_prompt.clone()`（L1247）+ R05-T03 注释更新（L1239）。

**新增（测试）**
- `rust/crates/lingxi-kernel/tests/r06_t01_context_compiler.rs`：20 个测试。
- `rust/crates/lingxi-service/tests/r06_t01_context_compiler.rs`：21 个测试（含 5 个组合根测试）。
- `rust/crates/lingxi-service/tests/r06_t01_closed_loop.rs`：1 个真二进制闭环测试（ServiceChild + loopback SSE StubServer）。

**修改（回归适配，契约必需）**
- `rust/crates/lingxi-service/tests/r05_t01_binary_wiring.rs`：c02 消息数 3→4，索引 +1，新增 `messages[0].role=="system"` 断言。
- `rust/crates/lingxi-service/tests/r05_t08_closed_loop.rs`：c01/c02 共 3 处消息计数与索引 +1（均带 R06-T01 注释）。
- 适配原因：openai-completions 线上每 turn 的 messages[0] 现为 system 槽（R06-T01 前该字段恒 None、system 消息不出现）。仅位置敏感断言调整，语义断言（工具调用链、用量、终态、重放）零改动。

## 五、段注册表与预算冻结（R06-A02 的结构性证据）

闭合注册表 24 段，`TOTAL_SYSTEM_TOKEN_BUDGET = 25_068` = 全段预算之和；每段强制预算钳制，任何超段 = 编译响亮失败；新增/重排段 = 修改注册表本身 = golden 与锚点测试变红。

| # | 段 id | 类别 | scope | 披露 | 带 | 静态 | 预算 |
|---|-------|------|-------|------|-----|------|------|
| 1 | platform.intro | platform_instruction | shared | full | preamble | 是 | 15 |
| 2 | platform.environment | platform_instruction | shared | digest_only | preamble | 是 | 200 |
| 3 | user.profile | user_profile | shared | digest_only | preamble | 是 | 400 |
| 4 | persona | persona | shared | digest_only | preamble | 是 | 4000 |
| 5 | agent.appearance | persona | main | digest_only | preamble | 是 | 500 |
| 6 | platform.output-discipline | platform_instruction | shared | full | preamble | 是 | 28 |
| 7 | platform.tool-discipline | platform_instruction | shared | full | preamble | 是 | 373 |
| 8 | platform.session-files | platform_instruction | shared | full | preamble | 是 | 163 |
| 9 | platform.ui-context | platform_instruction | shared | full | preamble | 是 | 62 |
| 10 | platform.subagent-collaboration | platform_instruction | main | full | preamble | 是 | 170 |
| 11 | platform.computer-use | platform_instruction | shared | full | preamble | 是 | 53 |
| 12 | platform.action-discipline | platform_instruction | shared | full | preamble | 是 | 167 |
| 13 | platform.web-tool-priority | platform_instruction | shared | full | preamble | 是 | 56 |
| 14 | platform.learn-skills | platform_instruction | shared | full | preamble | 是 | 110 |
| 15 | platform.skill-usage | platform_instruction | shared | full | preamble | 是 | 141 |
| 16 | agent.roster | agent_roster | main | digest_only | preamble | 是 | 800 |
| — | ── cache 分界线 ── | | | | | | |
| 17 | memory.rules | memory_context | main | digest_only | preamble | 否 | 300 |
| 18 | memory.tenets | memory_context | main | digest_only | preamble | 否 | 6700 |
| 19 | memory.longterm | memory_context | main | digest_only | preamble | 否 | 2100 |
| 20 | session.time | session_instruction | shared | full | preamble | 否 | 110 |
| — | ── wrapper 带 ── | | | | | | |
| 21 | session.append-system | session_instruction | shared | digest_only | wrapper | 否 | 1500 |
| 22 | session.project-context | agents_file | shared | digest_only | wrapper | 否 | 4000 |
| 23 | skill.catalog | skill_instruction | shared | digest_only | wrapper | 否 | 3000 |
| 24 | session.cwd | platform_instruction | shared | digest_only | wrapper | 否 | 120 |

合计：静态前缀带 7 238；动态 preamble 9 210；wrapper 8 620；总计 **25 068**。
冻结实测锚点（kernel golden 逐字节 + 逐 token 锁定）：zh preamble 总 1 747 / 静态带 1 462；en 1 529 / 1 299。现役实测对照证据：`artifacts/rust-tauri/R06/T01/incumbent-budget-measurement.raw.txt`。

## 六、新增测试（42 个，全部通过）

**kernel `r06_t01_context_compiler.rs`（20）**：zh golden 逐字节 + token 锚（1747/1462）、en golden（1529/1299）、逐段 token 对照现役实测、roster 格式对照现役、子代理裁剪（main 段全弃）、wrapper 空段省略但 session.cwd 恒在、预算超标响亮失败、digest_only 段观测无正文、span 切片等于段正文、注册表闭合（24 段/顺序/总预算）、token 估算口径（CJK/混合/空串）、确定性（同输入同输出两次编译）等。

**service `r06_t01_context_compiler.rs`（21）**：材料采集（config 窄扫描 4 键/注释剥离、preferences 门控、tenets 优先级+createdAt 排序、损坏 tenets 响亮 Parse、appearance 摘要、roster 扫描自身在前+id 序+仅含 config.yaml 目录、platform_note 逐字现役形状、cwd 恒空、shell/win32/uname 分支、product_dir 解析优先级、会话标签 GMT 偏移格式）+ 组合根 5 个：
- `sent_system_prompt_is_byte_identical_to_observed_artifact`：RecordingProvider 记录的实际发送 system 文本 == 缓存 artifact.render() == 观测 full 段拼接，三层同源。
- `session_freeze_spans_runs_and_turns`：同一会话跨 run、跨 turn 发送字节完全一致（会话冻结）。
- `subagent_run_sends_trimmed_context_and_observation_prefers_main`：子代理 run 发送裁剪后上下文（无 main 段）；观测端点主形态优先。
- `budget_exceeded_fails_the_run_loudly`：user.md 写 5000 个「档」（超 400 预算）→ Run 终态 Failed/ProviderFailed，code 含 `context_compile`，retryable=false。
- `missing_yuan_fails_the_run_loudly`：persona 材料全缺（无模板兜底）→ 同样响亮失败。

**二进制闭环 `r06_t01_closed_loop.rs`（1）**：`sent_wire_system_bytes_match_the_observation_digests` —— 真实 `lingxi-service` 进程 + loopback SSE 替身；材料在**启动成功后、execute 前**落盘 canonical home（遵守 R02 数据纪元闸）；断言：线上 messages[0].role=="system" 且含 PROFILE/AGENTS/BINARY-TOP_SECRET 材料标记与 sandbox 标签与 `<cwd>` 块；观测 renderSha256 == sha256(线上发送字节)；逐段 sha256 与线上切片一致；full 段 text==render 切片；digest_only 段无 text 且观测 JSON 整体不含 TOP_SECRET；beta（for_subagent 形态无缓存）404；无凭证 401/低权 403；SIGTERM 优雅停机。

## 七、现有回归测试（真实命令与退出码）

| 命令 | 结果 | 证据 |
|------|------|------|
| `cargo test -p lingxi-kernel --test r06_t01_context_compiler` | exit 0，20 passed / 0 failed | `artifacts/rust-tauri/R06/T01/t01-test-runs.raw.txt` |
| `cargo test -p lingxi-service --test r06_t01_context_compiler` | exit 0，21 passed / 0 failed | 同上 |
| `cargo test -p lingxi-service --test r06_t01_closed_loop` | exit 0，1 passed / 0 failed | 同上 |
| `cargo test -p lingxi-service --test r05_t01_binary_wiring --test r05_t08_closed_loop --test r05_t08_production_tools --test r05_t08_resources` | CARGO_EXIT=0，2+11+6+2=21 passed / 0 failed | `artifacts/rust-tauri/R06/T01/r05-binary-regression.raw.txt` |
| `cargo fmt --all -- --check`（rust/） | exit 0 | `artifacts/rust-tauri/R06/T01/fmt-check.raw.txt` |
| `cargo clippy --workspace --all-targets -- -D warnings`（rust/） | exit 0 | `artifacts/rust-tauri/R06/T01/clippy.raw.txt` |
| `cargo test --workspace`（rust/） | WORKSPACE_EXIT=0，118 个套件全 ok，1528 passed / 0 failed | `artifacts/rust-tauri/R06/T01/workspace-test.raw.txt` |
| `shasum -a 256 rust/Cargo.lock` | `259f983e…618eff3`，与任务基点一致 | 本报告头部 |

Node/桌面侧测试：**NOT_RUN**（本任务零改动 desktop/、core/、shared/；`git status` 中无相关路径）。

## 八、每个 Acceptance ID 对应证据

**R06-A01 — 观测和实际请求同源（除明示脱敏外一致，无另一套重建 prompt）**：PASS。
- 结构保证：唯一编译点（runs.rs:1072）、唯一渲染消费点（runs.rs:1247 `system_prompt` 直接取同一 artifact 的 render）、唯一观测读点（observation_for_session 返回同一 `CompiledContext` 的视图）。代码中不存在第二个 prompt 构造函数。
- 进程内三层证据：`sent_system_prompt_is_byte_identical_to_observed_artifact`（发送字节 == artifact.render() == 观测拼接）。
- 真二进制证据：`sent_wire_system_bytes_match_the_observation_digests` —— 线上 messages[0] 字节的 sha256 == 观测 renderSha256；逐段 sha256 与线上切片一致；full 段观测正文 == render 对应切片。
- 明示脱敏：digest_only 段观测只有元数据 + sha256，观测 JSON 不含 TOP_SECRET 材料标记（发送侧含）——脱敏边界被双向断言。

**R06-A02 — 提示词长度不暗增（不超过冻结预算；新增冗余段使测试失败；证据=token 与段来源对照）**：PASS。
- 冻结预算：24 段逐段预算 + 总量 25 068 常量；超预算 = 编译响亮失败（`budget_exceeded_fails_the_run_loudly` 实测 Run 失败）。
- token 与段来源对照：kernel `per_segment_tokens_match_incumbent_measurement` 逐段对照现役实测（证据 `incumbent-budget-measurement.raw.txt`）；zh/en golden 把总 token（1747/1529）与静态带 token（1462/1299）连同渲染字节一起锚死。
- 新增冗余段 = 改注册表 = 注册表闭合测试 + golden 锚点变红；静态带膨胀 = static_band_tokens 锚点变红。

## 九、R00 原始断言对应结果（31 叶逐条）

全部 31 叶隶属 D06 域、分类「保留」、R06 阶段。对 T01 的归类：**31/31 = 后续阶段**（这些叶的服务 API、桌面行为与 UI 行为归 R06 后续任务/R07+；T01 不含其验收责任）。其中 4 叶的**底层读取通道份额**已被 T01 接管并实测（读取侧，不含写入/广播/UI）：

| # | 叶（短名） | R00 补充验收 ID | T01 归类 | 说明 |
|---|-----------|----------------|---------|------|
| 1 | GET-AVATAR-PATH | LA-A02D8C572F5C | 后续阶段 | splash 头像路径，桌面域 |
| 2 | AGENT-CONFIG-GLOBAL-UPDATE | LA-24BC2E961E3E | 后续阶段 | preferences.json 写路径归后续；T01 仅读 userName/locale/learn_skills |
| 3 | AGENT-CONFIG-UPDATE | LA-8C8083F2A5FC | 后续阶段 | config.yaml 写路径归后续 |
| 4 | AGENT-DELETE | LA-517E697740D9 | 后续阶段 | |
| 5 | AGENT-DELETE-WITH-ORPHAN-SKILLS | LA-3516A7326DD9 | 后续阶段 | |
| 6 | AGENT-MEMORY-TOGGLE | LA-0B483D7F6A43 | 后续阶段 | **读取份额已接管**：`scan_agent_config` 读 memory.enabled（默认 true），单测覆盖；disabledSince/ticker/写路径归后续 |
| 7 | AGENT-WORKSPACE-SKILL-POLICY-UPD | LA-23172C8397F2 | 后续阶段 | |
| 8 | AGENTS-MD-PUT | LA-4870805E3A3D | 后续阶段 | 写路径 |
| 9 | AGENTS-MD-READ | LA-07A5CE3637C7 | 后续阶段 | **读取份额已接管**：`resolve_persona_source` 读 agents/{id}/AGENTS.md 并按 agents-templates/{lang} 链回落到 example，组合根与二进制腿实测；HTTP handler/别名归后续 |
| 10 | AVATAR-DELETE | LA-BB4CD1E4CD43 | 后续阶段 | |
| 11 | AVATAR-POST | LA-7F0F3F76E38A | 后续阶段 | |
| 12 | AVATAR-READ | LA-CCDCE84E4378 | 后续阶段 | |
| 13 | CONFIG-READ | LA-BC675C664910 | 后续阶段 | **读取份额已接管**：config.yaml 窄扫描（locale/agent.name/agent.yuan/memory.enabled）单测覆盖；供应商摘要/秘密掩码/GC 归后续 |
| 14 | IDENTITY-PUT | LA-856B28902999 | 后续阶段 | 写路径 |
| 15 | IDENTITY-READ | LA-A9B0BBAD449C | 后续阶段 | **读取份额已接管**：identity.md + identity-templates/{lang} 回落链实测；HTTP handler 归后续 |
| 16 | PUBLIC-AGENTS-MD-READ | LA-3D7A957EF7FF | 后续阶段 | |
| 17 | PUBLIC-AGENTS-MD-PUT | LA-7C699FD2A740 | 后续阶段 | |
| 18 | AGENTS-ORDER-PUT | LA-8D4356B7EC51 | 后续阶段 | T01 roster 扫描为编译自用确定性 id 序，与「保存的展示顺序」无关 |
| 19 | AGENTS-POST | LA-9CC1B13F1787 | 后续阶段 | |
| 20 | AGENTS-PRIMARY-PUT | LA-C0ADBABE5E89 | 后续阶段 | |
| 21 | AGENTS-READ | LA-26AE3688B8DE | 后续阶段 | |
| 22 | AGENTS-SWITCH-POST | LA-8B48FD65CAFB | 后续阶段 | |
| 23 | AVATAR-ROLE-AGENT（恢复默认） | LA-4A56F26EDA32 | 后续阶段 | |
| 24 | AVATAR-ROLE-AGENT（上传） | LA-7570BE1F2527 | 后续阶段 | |
| 25 | AVATAR-ROLE-AGENT（查看） | LA-E4BEE8E6974A | 后续阶段 | |
| 26 | PROVIDER-AGENT-REMOVE | LA-38A989948FDD | 后续阶段 | |
| 27 | PROVIDER-AGENT-SAVE | LA-9E3981A9BFCA | 后续阶段 | |
| 28 | PROVIDER-INLINE-CREDENTIAL-SAVE | LA-464E58DEC9E2 | 后续阶段 | CredentialService 边界未触碰 |
| 29 | UI-SETTINGS-AGENT | LA-206785535A40 | 后续阶段 | 桌面设置 UI 全量 |
| 30 | MOOD-HISTORY | LA-B09CC9536400 | 后续阶段 | |
| 31 | MOOD-LIVE | LA-32E9930CF9D9 | 后续阶段 | |

T01 范围内无「真实实现（整叶）」与「合法份额」之外的第三类；上表 4 叶的读取份额属于 T01 交付物自身的材料通道，不声称完成整叶验收。

## 十、提示词差异报告（迁移前后模板差异冻结清单）

以下为与现役（core/agent.ts + Electron 壳）相比的**全部**有差异点，逐条冻结；除本清单外不允许再有差异。

| # | 差异 | 说明与原因 |
|---|------|-----------|
| 1 | 观测 span 单位为 UTF-8 字节（schema 明示 `spanUnit:"utf8-bytes"`） | 现役 provenance 用 UTF-16 code unit 偏移。Rust 字符串切片天然 UTF-8；ASCII 区间两者数值一致，含 CJK 时不同。观测 schema 版本字 `lingxi.context-observation.v1` 已声明。 |
| 2 | session.time 时区 = chrono Local 系统时区偏移（GMT±H[:MM]） | 现役支持 preferences 内 IANA 命名时区；chrono-tz 不在冻结锁文件内，T01 跟随系统时区。命名时区偏好接入留待后续（需锁文件变更授权）。 |
| 3 | appearance 注入门控简化为「appearance-summary.json 存在且合法即注入」 | 现役另有「模型具备 vision 能力」门控；无头服务在 R05 模型面之下无该能力表可判。 |
| 4 | platform.computer-use 段恒不注入（computer_use_available=false） | 无头服务无桌面引擎；段定义保留在注册表。 |
| 5 | platform.learn-skills 段生产默认不注入 | 门控 = experiments.learn_skills.enabled && allow_github_fetch，后者默认 false，与现役默认行为一致。 |
| 6 | skill.catalog / session.append-system / session.project-context 生产默认空 | SkillService 归 R06-T06；appendSystemPrompt/projectContext 为壳层注入面。kernel 渲染与预算钳制完整并实测（wrapper 空段省略、session.cwd 恒在）。 |
| 7 | 会话冻结为进程内缓存；跨重启后首个 run 重编译 | 现役 coordinator 恢复快照跨重启。差异已冻结；重编译因材料不变而内容相同（session.time 标签除外，其取会话启动时间入参）。 |
| 8 | environment note 的 cwd 恒空串 | **现役逐字移植**（现役即如此；cwd 由 wrapper 段 session.cwd 单独携带）。二进制腿实测 `<cwd>\n{workspace}\n</cwd>` 块在 messages[0] 内。 |
| 9 | sandbox 标签为冻结常量 `read-all_write-scoped_network-on` | 与现役同源常量。 |
| 10 | Windows 的 os release 为空串 | uname FFI 仅 unix；shell=powershell。未实机验证（见 §十二）。 |
| 11 | 材料读取错误策略严于现役 | 现役 safeReadFile 静默置空；T01：NotFound→空段（合法），其余 IO/解析错误→响亮编译失败→Run Failed。符合「禁止静默降级」红线。 |
| 12 | roster 顺序 = 自身在前 + 其余按 id 字典序；model/summary 字段省略 | 现役为运行时注册表顺序（本质不确定）且带 model/summary；Rust 服务无该注册表源，取确定性序并省略字段。 |
| 13 | tenets 排序键 createdAt 缺失时回填空串 | 现役回填 now()（不确定）；T01 取确定性。 |
| 14 | 无 migration-degraded 分支 | 该状态源自 Electron 启动改名失败记录，Rust 服务无此状态源。 |
| 15 | config.yaml 为 4 键窄扫描（YAML 子集：剥 ` #` 注释与成对引号） | 锁文件冻结禁止引入 YAML crate。扫描器只认 `locale:`、`agent.name:`、`agent.yuan:`、`memory.enabled:` 顶格/两空格缩进形态；超出子集按缺省处理（locale→prefs→en；name→"Lingxi"；yuan→模板兜底；memory.enabled→true）。写路径不归 T01。 |
| 16 | user_name 单一来源 preferences.json `userName` | zh 缺省「用户」/en 缺省 "User"；kernel golden 中 user_name≠resolved_user_name 的分叉仅测试构造。 |
| 17 | 每 turn messages[0] 恒为 system 槽 | T01 前 `system_prompt` 恒 None、system 消息不上线。openai-completions 适配器的 Some 分支为现役既有代码路径；位置敏感的 R05 断言已按契约必需适配（§四）。 |
| 18 | 材料目录受 R02 数据纪元闸约束 | 未盖章 home 预置 agents/ 等材料目录 → 拒启（unstamped-home-with-data）。现役用户数据割接须先盖章——R08 割接议题记录于此；测试材料均在启动后落盘。 |

## 十一、R05 接口兼容结果

- **ModelTurnInput.system_prompt**（R05 契约字段，类型 `Option<String>` 未变）：生产值由恒 None 变为恒 `Some(render)`。openai-completions 适配器对该字段的 Some 分支是现役既有代码（render_chat_request），未改动适配器。
- **失败语义**：编译失败镜像既有 no-provider 早退——`RunFinish::Failed{ProviderFailed{code:"context_compile: …", retryable:false}}` + `adjudicated_finalize`，不新增终态变体、不改取消/恢复路径。
- **零改动面**：ModelGateway / ProtocolFamily / DeltaNormalizer / Usage 账本 / quota / R04 统一工具网关 / CredentialService / R02 存储与事件边界 / R03 Run-Attempt 语义，全部未触碰（git diff 可证）。
- **消费者重测**：全部真实二进制套件（binary_wiring 2、closed_loop 11、production_tools 6、resources 2）在接线后全绿（§七），仅位置断言按 §四 适配。

## 十二、未验证事项

- Windows 平台行为（os release 空串、shell=powershell 分支）：NOT_RUN（无 Windows 环境）。
- 真实付费供应商：NOT_RUN（全部 loopback 确定性替身，遵守任务书）。
- `LINGXI_PRODUCT_DIR` 打包布局（`<home>/product` 兜底分支）：NOT_RUN（无真实打包产物；exe 上溯命中仓库 lib/ 的分支已被 4 个二进制回归套件实测）。
- 跨进程重启后的上下文一致性：NOT_RUN（进程内冻结已测；跨重启重编译为已冻结差异 #7）。
- chrono Local 在无 TZ 环境变量/容器中的偏移解析：NOT_RUN。
- 英文 persona 模板链在真二进制腿中未单独走（en 模板选择由 service 单测覆盖，zh 链由二进制腿覆盖）。

## 十三、已知风险

1. 工具链与任务书不符：实际 1.93.0（任务书写 1.98.1）。全部质量门在 1.93.0 下通过；若验收环境用 1.98.1，clippy 可能新增 lint。
2. 会话冻结缓存为进程内 HashMap 无上限：与现役会话快照同语义；会话数受存储行数约束，单 artifact ≤25 068 token 预算封顶，内存有界。
3. config.yaml 窄扫描器是 YAML 子集（差异 #15）：若后续写入侧产出复杂 YAML（多行/锚点），读取侧按缺省处理而非报错——写路径接入时需同步扩扫描器或引入正式解析（需锁文件授权）。
4. 数据纪元闸与材料目录的交互（差异 #18）是 R08 割接必须处理的议题，本任务已按闸纪律执行并记录。
5. appearance 门控简化（差异 #3）在无 vision 能力的模型上会多送一段摘要文本（≤500 token 预算内），不造成功能错误。

---

**STATUS: READY_FOR_INDEPENDENT_REVIEW**
