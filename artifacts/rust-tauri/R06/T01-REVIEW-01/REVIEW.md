# R06-T01 独立对抗性审查报告

- **REVIEWER_ID**: REVIEWER-R06-T01-R1（一次性独立审查代理，未参与实现）
- **TASK_ID**: R06-T01（上下文编译器与短提示词）
- **CANDIDATE_SHA**: `c9cca6020e0990016ee0d14af406766370849a7a` + 未提交工作树（7 修改 + 7 新增路径）
- **审查时点工具链**: rustup 锁定 1.98.1（`/Users/study_superior/.cargo/bin/cargo`，经 `rust-toolchain.toml` 生效）
- **证据目录**: `artifacts/rust-tauri/R06/T01-REVIEW-01/`（全部为本审查者亲跑原始输出）

---

## 一、总控疑点裁定

### 疑点 A：工具链矛盾 —— 成立（见 F-02）

执行者报告自报 Homebrew cargo 1.93.0，并声称「任务书以实际为准」。经核实：

- `rust/rust-toolchain.toml` 冻结 channel = 1.98.1；该冻结只对 rustup 代理（`~/.cargo/bin/cargo`）生效，Homebrew 工具链不读此文件（仓库内注释明确警告这一点）。
- 执行者全部质量门证据（`artifacts/rust-tauri/R06/T01/*.raw.txt`）均产自 1.93.0，**按冻结要求不可直接采信**。
- 本审查者用锁定 1.98.1 对同一候选**独立重跑全部质量门**：fmt、clippy `--locked -D warnings`（含 touch 强制重 lint）、42 个新测试、R05 回归 21 个、workspace 全量 118 套件 1528 通过 / 0 失败，真实退出码全 0（证据见 §五）。**实质结论由审查者证据独立成立**，执行者证据仅作旁证。

### 疑点 B：R05 测试改动是否只增不减 —— 排除

- 两文件 diff 逐行经复核：仅新增 `messages[0].role=="system"` 断言、位置索引 +1、消息计数 3→4 / 1→2 / 5→6 / 7→8；语义断言（工具调用链、用量、终态、重放、鉴权）零删改。
- 反向对照实验（本审查者）：取 HEAD 版未适配的 `r05_t01_binary_wiring.rs` 对候选代码运行 → `c02` 红在 `left: 4, right: 3`（消息数断言），即 system 槽上线是真实线上行为变化，适配属契约必需而非趁机放水。证据：`r05-head-test-against-candidate.raw.txt`。
- 适配后全套 R05 真实二进制套件（2+11+6+2=21）在锁定工具链下全绿。证据：`r05-regression-reviewer.raw.txt`。

---

## 二、Findings

### F-01：总量预算常量未被强制，且对应测试名不副实

- **FINDING_ID**: R06T01-REV-F01
- **SEVERITY**: LOW
- **REQUIREMENT_ID**: R06-A02
- **FILE_AND_LINE**: `rust/crates/lingxi-kernel/src/context.rs:313`（`TOTAL_SYSTEM_TOKEN_BUDGET` 定义，生产代码零引用）；`rust/crates/lingxi-kernel/tests/r06_t01_context_compiler.rs:517`（`total_budget_constant_matches_spec_sum`）
- **OBSERVED_BEHAVIOR**: 常量仅被该测试以字面量 `assert_eq!(TOTAL_SYSTEM_TOKEN_BUDGET, 25_068)` 断言——测试名宣称「匹配段预算之和」，实际不做求和；`compile()` 也无总量检查。探针树注入第 25 段（预算 64）后该测试仍绿。
- **EXPECTED_BEHAVIOR**: 报告 §五与 acceptance-map 宣称「总量 25 068 上限」；应有真实求和断言或 compile 总量钳制。
- **REPRODUCTION**: 探针树注入常驻段 → `registry_is_frozen_at_24_segments` + 双 golden 红、本测试绿。证据：`neg-injection-segment.raw.txt`。
- **ROOT_CAUSE**: 总量约束停留在文档层，防护实际由「每段预算钳制 + 注册表冻结 + golden 锚」承担。
- **SAME_ROOT_CAUSE_PATHS**: 报告 §五「总量 25 068」表述；`acceptance-map.json` R06-A02「总量上限」表述。
- **IMPACT**: **无实际暗增通道**——每个渲染段都过自身预算钳制，故总量 ≤ 各段预算之和 = 25 068 由构造恒成立；注入攻击实测被另外 3 个测试捕获。影响限于：测试名误导后来者；若有人只改常量不改段表会误以为有防护。
- **REQUIRED_FIX**: 把测试改为对 `SEGMENT_SPECS` 真实求和（或在 `compile()` 加总量响亮失败），并修正两处文档措辞。
- **REGRESSION_TESTS**: 求和版测试 + 注入段即红（本审查者探针可直接转正）。

### F-02：执行者质量门证据全部产自非锁定工具链（1.93.0）

- **FINDING_ID**: R06T01-REV-F02
- **SEVERITY**: MEDIUM（流程/证据归属缺陷；实质结论已由审查者锁定工具链证据独立成立）
- **REQUIREMENT_ID**: 01 通用约束（工具链冻结）/ 05 验收协议（证据可信性）
- **FILE_AND_LINE**: `docs/rust-tauri/R06/R06-T01_REPORT.md:9`（自报 1.93.0 并称「以实际为准」）；`rust/rust-toolchain.toml`（channel 1.98.1）
- **OBSERVED_BEHAVIOR**: fmt/clippy/测试证据均产自 Homebrew 1.93.0；「以实际为准」是对冻结机制的误读。
- **EXPECTED_BEHAVIOR**: 质量门在冻结 1.98.1 下执行并留证。
- **REPRODUCTION**: 报告头部自述 + 证据文件无工具链版本记录。
- **ROOT_CAUSE**: 执行环境 PATH 中 Homebrew 优先于 rustup 代理，且未察觉冻结语义。
- **SAME_ROOT_CAUSE_PATHS**: `artifacts/rust-tauri/R06/T01/` 下全部 *.raw.txt 证据文件。
- **IMPACT**: 执行者证据按规则不可采信；本审查者已用 1.98.1 全量重跑补位（§五），验收实质不受影响。若验收规程要求「执行者自证」，则该批证据需重发。
- **REQUIRED_FIX**: 用锁定工具链重跑并替换证据文件；报告工具链行改为实测+锁定一致。
- **REGRESSION_TESTS**: 证据文件首行记录 `cargo --version` 输出（本审查者证据已照此办理）。

### F-03：候选摘要不可复现

- **FINDING_ID**: R06T01-REV-F03
- **SEVERITY**: LOW
- **REQUIREMENT_ID**: 05 验收协议（候选钉定）
- **FILE_AND_LINE**: `docs/rust-tauri/R06/R06-T01_REPORT.md:5`（声明 `6dc9f4c2…`）
- **OBSERVED_BEHAVIOR**: 排除审查者证据目录后重算 = `9a6e0541…`（两次独立重算一致），≠ 声明值。mtime 证据链：全部源码/测试文件 ≤ 17:58:46，证据归档 ≤ 18:20:53，报告 18:21:45，而 `docs/rust-tauri/R06/R06_PROGRESS.json` 于 **18:23:56**（报告写入后约 2 分钟）被再改。
- **EXPECTED_BEHAVIOR**: 声明摘要应钉住被审候选。
- **REPRODUCTION**: 按报告口径重算（命令存 `candidate-digest-recompute.raw.txt`）。
- **ROOT_CAUSE**: 摘要计算后进度台账又被更新，摘要在交付前已过期。
- **SAME_ROOT_CAUSE_PATHS**: 无其他文件（mtime 全量排查）。
- **IMPACT**: 摘要失去钉定功能；但 mtime 链证明**代码与测试内容在摘要计算后零改动**，被审对象实质未漂移。审查者全部验证均直接作用于当前真实工作树，审查有效性不依赖该摘要。
- **REQUIRED_FIX**: 进度台账纳入摘要排除清单或最后写；重发摘要。
- **REGRESSION_TESTS**: 不适用（流程项）。

### F-04：acceptance-map.json 引用的测试名陈旧（4 处）

- **FINDING_ID**: R06T01-REV-F04
- **SEVERITY**: LOW
- **REQUIREMENT_ID**: 05 验收协议（证据可追溯）
- **FILE_AND_LINE**: `artifacts/rust-tauri/R06/T01/acceptance-map.json`（mtime 16:33，早于测试定稿 17:52）
- **OBSERVED_BEHAVIOR**: `sent_request_is_byte_identical_to_observed_artifact`（实际 `sent_system_prompt_is_byte_identical_to_observed_artifact`）；`provenance_view_carries_no_digest_only_content`（实际 `observation_view_carries_no_digest_only_content`）；`segment_registry_is_frozen`（实际 `registry_is_frozen_at_24_segments`）；`golden_preamble_zh/en_byte_equal`（实际为两个独立测试）。
- **EXPECTED_BEHAVIOR**: 验收映射中的测试名应可直接索引到真实测试。
- **REPRODUCTION**: 名称逐个对拍（脚本对拍记录于本文件 §六）。
- **ROOT_CAUSE**: 映射写于测试定稿前，定稿改名后未回填。
- **SAME_ROOT_CAUSE_PATHS**: 无。
- **IMPACT**: 所映射测试**全部真实存在且通过**，无验收空洞；仅可追溯性受损。
- **REQUIRED_FIX**: 回填 4 处真实测试名。
- **REGRESSION_TESTS**: 不适用。

### F-05：执行者 clippy 证据未使用 --locked

- **FINDING_ID**: R06T01-REV-F05
- **SEVERITY**: LOW
- **REQUIREMENT_ID**: 05 验收协议（锁文件纪律）
- **FILE_AND_LINE**: `artifacts/rust-tauri/R06/T01/clippy.raw.txt`；报告 §七 clippy 行
- **OBSERVED_BEHAVIOR**: 执行者命令为 `cargo clippy --workspace --all-targets -- -D warnings`，无 `--locked`。
- **EXPECTED_BEHAVIOR**: 验收硬性项要求 `clippy --locked`。
- **REPRODUCTION**: 证据文件仅有 2 行输出，无命令回显与退出码。
- **ROOT_CAUSE**: 证据采集不严。
- **SAME_ROOT_CAUSE_PATHS**: fmt-check.raw.txt 同样无命令回显（仅一行 `fmt exit=0`）。
- **IMPACT**: 本审查者已用 `--locked`（含 touch 强制重 lint 新代码）重跑通过，实质无影响。证据：`fmt-clippy-reviewer.raw.txt`。
- **REQUIRED_FIX**: 证据文件须含命令行、工具链版本与退出码（本审查者证据格式可直接作模板）。
- **REGRESSION_TESTS**: 不适用。

### F-06：观测端点跨主体 403 无仓库内提交测试

- **FINDING_ID**: R06T01-REV-F06
- **SEVERITY**: LOW
- **REQUIREMENT_ID**: 02 契约 §7（鉴权）/ 任务书对抗清单（权限越界）
- **FILE_AND_LINE**: `rust/crates/lingxi-service/src/lib.rs:2920`（handler 内 owner 复检）；测试缺口：`r06_t01_closed_loop.rs` 仅覆盖无凭证 401/403（L448），`r06_t01_context_compiler.rs` 无跨主体用例
- **OBSERVED_BEHAVIOR**: 「持合法凭证的主体 B 访问主体 A 会话的观测」这一越界面无任何提交测试。
- **EXPECTED_BEHAVIOR**: 权限越界属任务书明示对抗项，应有入库回归。
- **REPRODUCTION**: 本审查者黑盒探针对真实二进制实测：B 的凭证 → `403 authz.cross_principal_access`（正确拒绝）。证据：`reviewer-e2e-probe.raw.txt`。
- **ROOT_CAUSE**: 执行者把鉴权复用（照抄 get_session）视为既有覆盖，未为新端点补跨主体用例。
- **SAME_ROOT_CAUSE_PATHS**: 无（handler 实现本身正确）。
- **IMPACT**: 行为已验证正确，缺口仅在回归保护；未来重构 handler 可能静默失守。
- **REQUIRED_FIX**: 闭环测试补一例跨主体 403（探针逻辑可直接移植）。
- **REGRESSION_TESTS**: 即本审查者探针的 cross-principal-403 用例。

---

## 三、生产路径真实性（亲验）

完整链路已逐段读码并由**真实二进制端到端跑通**（非替身内部状态）：

```
main.rs:511-525 组合根 → ServiceDeps.context_compiler → lib.rs:1263 接线
→ runs.rs:1072（provider 早退后、turn 循环前唯一编译点）
→ runs.rs:1247 每 turn system_prompt = Some(同一 artifact 的 render)
→ openai-completions 适配器现役 Some 分支 → 线上 messages[0]={role:"system"}
观测读面：auth.rs:575 → Scope("chat") → lib.rs:2920 owner 复检 → 同一 CompiledContext 观测视图
```

独立验证（审查者黑盒探针，真实 `lingxi-service` 进程 + loopback SSE 替身，15/15 过）：

- 线上 messages[0].role=="system" 且携带真实材料标记；
- 观测 `renderSha256` == 线上发送字节的 sha256；逐段 sha256 == 线上字节切片（UTF-8 字节偏移口径）；
- full 段观测正文 == 线上切片；digest_only 段观测无正文；观测 JSON 不含 TOP_SECRET 标记；
- 无凭证 401 / 错误动词 405 / 未知会话 404 / HEAD 200 / 跨主体 403 / SIGTERM 优雅停机。

证据：`reviewer-e2e-probe.raw.txt`。代码面确认无第二套 prompt 构造（全仓 `system_prompt` 赋值点唯一），协议适配器五个 Some 分支为现役既有代码、本候选零改动（`git diff` 对 adapters/protocol 为空）。

## 四、对抗性验证矩阵（审查者亲构造）

| 对抗项 | 方法 | 结果 |
|--------|------|------|
| 权限越界 | 黑盒：无凭证/低权/跨主体/未知会话/错误动词 | 401/401-403/403 cross_principal/404/405 全部正确 |
| 不可信注入 | 材料含 `{{userName}}` 标记 → 不被替换；伪装系统指令写进 user.md → 保持 digest_only 数据身份且观测不含 | 过 |
| 超长材料 | user.md 超 400 预算 → run 响亮失败 code 含 context_compile；边界二分：恰好通过/超 1 token 即败且点名 user.profile | 过 |
| 空材料 | 材料全缺 → MissingYuan 响亮失败；空 wrapper 段省略、session.cwd 恒在 | 过 |
| 新增冗余段 | 探针树注入第 25 常驻段 → 注册表冻结 + zh/en golden 3 测试变红 | 过（同时暴露 F-01） |
| 并发 | 8 会话并发编译 digest 互不相同不串扰；同会话 4 线程并发结果逐字节一致；同会话主/子形态并存互不覆盖且观测主形态优先 | 过 |
| 取消 | 代码走查：编译为 turn 循环前同步原子段，失败镜像 no-provider 早退（adjudicated_finalize），无半成品状态 | 过 |
| zh/en golden | 锁定工具链下逐字节锚（1747/1462、1529/1299）复跑通过 | 过 |
| 子代理裁剪 | 组合根测试 + 并发探针双路验证：main scope 段全弃 | 过 |
| 配置代次 | locale 链 config.yaml → preferences.json → en；窄扫描器对非规范 YAML 回落缺省（差异 #15 已登记） | 过 |
| HEAD 版 R05 负对照 | 未适配测试对候选代码 = 红在消息数断言（证明适配契约必需） | 过 |

证据：`kernel-adversarial-probes.raw.txt`、`neg-injection-segment.raw.txt`、`concurrency-probe.raw.txt`、`reviewer-e2e-probe.raw.txt`、`r05-head-test-against-candidate.raw.txt`。

## 五、审查者自己的验证（硬性项，全部亲跑，锁定工具链 1.98.1）

| 项 | 真实结果 | 证据 |
|----|---------|------|
| 工具链版本 | cargo/rustc 1.98.1（rustup 代理） | fmt-clippy-reviewer.raw.txt 首行 |
| `cargo fmt --all -- --check` | exit 0 | 同上 |
| `cargo clippy --locked --workspace --all-targets -- -D warnings` | exit 0（另做 touch 强制重 lint 新源文件，仍 0） | 同上 |
| 新测试 42 个（kernel 20 + service 21 + 闭环 1） | 全过 | new-tests-reviewer.raw.txt |
| R05 回归 21 个（binary_wiring 2 + closed_loop 11 + production_tools 6 + resources 2） | 全过，R05_REGRESSION_EXIT=0 | r05-regression-reviewer.raw.txt |
| `cargo test --locked --workspace` 全量 | **118 套件 ok，1528 passed / 0 failed，cargo 真实退出码 0**（直接重定向，非管道尾码） | workspace-test-reviewer.raw.txt |
| 候选摘要重算 | 9a6e0541… ≠ 声明 6dc9f4c2…（根因见 F-03） | candidate-digest-recompute.raw.txt |
| 执行者原始日志复核 | workspace-test/clippy/fmt-check/incumbent-budget-measurement 均读原文；测量值与 golden 锚点算术一致（如 platform.intro 12 CJK×1.1 + 6 ASCII/4 = ceil 14.7 = 15） | 本报告 §四 |
| 现役对照 | `tests/agent-system-prompt-equivalence.test.ts`（5 passed）现役 golden 锁定输出；`core/session-coordinator.ts:2061` 会话快照冻结语义与候选一致 | 现役测试亲跑 |

token 估算口径逐项核对：CJK 区间 7 段边界与现役 `lib/llm/estimate-text-tokens.ts` 完全一致，按码点迭代、ceil、CJK×1.1、其余 /4 —— 两侧源码对读一致。

## 六、31 叶归类抽查（16/31，含指定 #6/#9/#13/#15）

- 结构核实：31 叶与 `R06_LEAF_MAP.json` 的 tasks['R06-T01'] 逐叶一致（序号、补充验收 ID 全对），且同批叶同属 R06-T06 清单，「后续阶段」归类有结构支撑。
- 条款级抽查 16 叶：#1/#2/#3/#6/#7/#8/#9/#13/#15/#18/#19/#21/#22/#28/#29/#30/#31。断言原文均为桌面管理面/写路径/UI 行为（如 preferences 写路径、AGENTS.md PUT、助手排序 PUT、凭证转存、设置页、MOOD 展示），无一落在 T01 的编译器职责内，「后续阶段」归类成立。
- 4 个读取份额叶条款级核实：#6 `scan_agent_config` 读 memory.enabled 有单测；#9 `resolve_persona_source` agents/{id}/AGENTS.md → 语言模板 → example 回落链有组合根+二进制腿实测；#13 config.yaml 4 键窄扫描有单测；#15 identity.md + 模板回落链实测。报告未把读取份额夸大为整叶完成，表述诚实。
- 附带发现（非 finding）：F-04 所列测试名陈旧属证据卫生，不构成归类错误。

## 七、裁决

**VERDICT: PASS**

理由：任务书全部硬性验收点（A01 同源、A02 不暗增、R05 兼容、生产路径真实、对抗面健壮）由**本审查者在锁定工具链 1.98.1 下独立取证成立**，不依赖执行者证据。六个 findings 无一能推翻「本 Task 已完成」的实质声明：F-01 无可利用的暗增通道（构造性上界恒成立，注入攻击实测被三道独立闸捕获）；F-02 属证据归属缺陷且已被审查者证据补位；F-03/F-04/F-05 为证据与流程卫生；F-06 为回归保护缺口而非行为缺陷。

**验收后必修项（不阻断本次验收，进入 R06 台账跟踪）**：
1. F-01：总量测试改为真实求和（或 compile 加总量钳制），修正报告与 acceptance-map 措辞；
2. F-02/F-05：用锁定工具链重发质量门证据（可直接引用本审查者证据文件）；
3. F-03：摘要口径排除进度台账或最后计算，重发候选摘要；
4. F-04：回填 4 处真实测试名；
5. F-06：补观测端点跨主体 403 入库测试。

---

**证据清单**（`artifacts/rust-tauri/R06/T01-REVIEW-01/`，均为审查者亲跑原始输出；探针内凭证为一次性合成数据，对应临时 home 已销毁）：

- `reviewer-e2e-probe.raw.txt` — 黑盒端到端 15/15
- `kernel-adversarial-probes.raw.txt` — kernel 对抗探针 5/5
- `concurrency-probe.raw.txt` — 并发探针 1/1
- `neg-injection-segment.raw.txt` — 冗余段注入 3 红 + 回退后 20 绿
- `r05-head-test-against-candidate.raw.txt` — HEAD 版 R05 测试负对照
- `r05-regression-reviewer.raw.txt` — R05 回归 21/21
- `new-tests-reviewer.raw.txt` — 新测试 42/42
- `fmt-clippy-reviewer.raw.txt` — fmt/clippy --locked（含强制重 lint）
- `workspace-test-reviewer.raw.txt` — workspace 全量 118/1528/0
- `candidate-digest-recompute.raw.txt` — 候选摘要重算与根因 mtime 链
