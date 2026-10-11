# R06-T08 独立对抗性审查（REVIEWER-R06-T08-R1）

- 审查者未参加实现；目标 = 推翻「本 Task 已完成」，未推翻。
- HEAD = `ea61380577916e9fb75a4f4c2694321af7b83923`（= TASK_BASE_SHA）；分支 `codex/rust-tauri-migration`；候选 = 未提交工作树。
- 工具链：cargo 1.98.1（`/Users/study_superior/.cargo/bin/cargo`），全程 `-j 8`，从 `rust/` 目录执行。

## digest

- 开工复算（2026-10-11T02:58Z 前）：`61055b4f2c275dbe21d597e1a63f57bb70e62357ca7e2bc94c302262e9c02e7f`
- 收尾复算（审查结束、全部证据落盘后）：同上——**零漂移**。
- 与执行者三次申报值逐字一致。口径按脚本头注释：`git diff HEAD` + 全部未跟踪文件 sha256，剔除 `artifacts/rust-tauri/R06/**` 与 `candidate_digest*.txt`。`.vscode/`（未 gitignore，脚本口径内）与 T04–T07 先例同式申报为 IDE 噪声归因，不属候选。
- 审查全程仅新增 `artifacts/rust-tauri/R06/T08-REVIEW-01/**`（口径外），候选未被改动（收尾 `git status` 与开工一致 + digest 同值）。

## 四组命令独立复跑（退出码）

| 命令（rust/） | 退出码 | 证据 |
|---|---|---|
| `cargo fmt --all -- --check` | **0** | `fmt.log` |
| `cargo clippy -j 8 --locked --workspace --all-targets -- -D warnings` | **0** | `clippy.log` |
| `cargo test -j 8 -p lingxi-service --locked --no-fail-fast --test r06_t08_behavior_eval --test cancellation_tree --test cancel_link_inheritance --test cancel_terminal_race` | **0** | `targeted_tests.log`：eval 10 + cancellation_tree 8 + cancel_link_inheritance 7（含真实子代理 child 超时腿 C04）+ cancel_terminal_race 13，全绿 |
| `cargo test -j 8 --workspace --locked --no-fail-fast` | **101 = 恰 1 个环境失败** | `full_tests.log`：**1861 passed / 1 failed**（145 target），唯一失败 = `r00_management_positive_and_negative_branches_on_real_service` |

- **水位**：与执行者申报逐字一致（总 1862 = T07 终局 1852 + 10，恰为 T08 评测套件，只增不减）。
- **环境项 solo 另证**：审查者 solo 复跑同项**仍失败**（`r00_management_lan_solo_reviewer.txt`，86.03s），panic 原文与既有登记**同签名**——非回环自地址 `192.168.3.5` 入站交换停滞（macOS 应用防火墙/TUN 拦截未签名测试二进制），测试自身如实声明 "environment failure surfaced honestly, not a skipped assertion"。执行者 solo PASS 证据（142.64s）在案；T06/T07 两轮审查均记录同项波动（"两轮唯一失败项互异"、"solo×2 同签名"）。该项属 LAN 环境依赖，与 T08 diff（subagents.rs + 评测套件 + 文档）无任何代码关系。

## 评测可复现性（加测）

- 审查者以 `R06_T08_EVAL_OUT` 独立重放评测套件（`--test-threads=1`）：**10/10 过**；产出 `eval_records_reviewer.jsonl` 与执行者 `eval_raw/eval_records.jsonl` 逐字段机器比对：**10 任务全部字段一致，唯一差异 = BE-07 的 citationId**（每次运行由摄入派生的数据 id；`citationResolvable/canonicalMatches/onlyAllowedSource` 断言两轮同真）——套件判定可复现，非一次性手工产物。

## 六部分结论

1. **任务完整性：成立**。5 Steps 全落：固定任务集八类齐全（BE-01 直接四工具 / BE-02 需按需目录 / BE-03 参数复杂 / BE-04 工具禁用 / BE-05 权限不满足 / BE-06 跨provider媒体 / BE-07 资料引用 / BE-08 子代理边界 / BE-09 不应调用工具；BE-10 = A15 存在证明，申报为八类外评分反例）；四维单独统计 + token + 额外调用（机器复算 `dimensions_true` 10/10/9/9、extra=0、AND 规则逐任务成立、9/9 确定性契约 100%）；确定性 100% 达成、真实模型多次采样 BLOCKED_NOT_AUTHORIZED 如实登记（不冒称）；正确性下降按根因修（T08-FIX-01），非提示词补丁；PI 账本全量交接（×14 带 r06_t08_handoff）。3 Deliverables 就位（docs/rust-tauri/R06/{R06_REPORT.md, R06_HANDOFF.json, BEHAVIOR_EVAL.json}，位置合模板）；Depends On T07 已提交（`5656ea446d09` cat-file 为 commit）且 R1_PASS；Stage Scope 未越界。
2. **生产路径真实性：成立**。B 层 = 真 `lingxi-service` 子进程（`CARGO_BIN_EXE_lingxi-service`、真 argv/--config、SIGTERM 优雅停机断言）；M 层 = 真 ConfigModelGateway→CredentialService→dialect→dispatcher→settlement；S 层 = `bootstrap_with_deps`（真 supervisor/网关/存储/编译器）。替身只读请求体、回脚本化响应，从不决定权限/状态/审批（BE-08 ArrivalRecorder 证明旧通道零到达）。token 读 runs.db `model_call_usage` 台账（只读连接）；「额外调用 0」= stub 实测 hits/claims 比对。T08-FIX-01 生产可达：`SubagentRuntime` 于 `lib.rs:1369` 服务装配构造；三处 drive_run 调用点（sessions.rs:747 / background.rs:307 / subagents.rs:784）现全部传真实 agent 身份。
3. **对抗性验证：成立**（D-T08-1 裁决见下）。A15/A16 见下。数据一致性：`eval_records.jsonl` ↔ `BEHAVIOR_EVAL.json` tasks[] **10 任务逐字段 0 漂移**（超额完成 ≥3 抽查）。现役锚点抽验：core/agent.ts、core/session-coordinator.ts、core/media-adapters/{openai,gemini}.ts、lib/knowledge/knowledge-query-service.ts、model_call_usage 台账、R04 filetools/gateway prepare、ResourceAccess 授权闸原文——全部真实存在。注入：恶意「放宽」文本进入模型输入不获特权（BE-04C/BE-05B 拒绝形状不变）；评测基础设施自身不构成授权旁路（权限判定全在真实网关）。
4. **R02–R05/T01–T07 回归：成立**。修复 diff 范围 = subagents.rs 单 hunk（生产代码唯一改动；runs.rs 零改动——任务书边界猜测不成立但无害）；R03 取消相关套件亲跑全绿（上表）；执行者五套件 30 用例日志核验（8+7+8+3+4，exit 0）；全量 1861/1 与执行者两轮互证。
5. **防虚假完成：成立**。报告/HANDOFF 抽查无虚报：A01–A14 归属前序独立评审（T01–T07 REVIEW.md 目录在案），A15/A16 标注「待独立复核」不自称 PASS；无预写自身提交 SHA（T08 以工作区交付，digest 定稿后计算）；PI 矩阵与 HEAD 机器比对**仅追加**（`r06_t08_handoff`×14 + `r06_t08_update`，共享路径零值变化、零删键），11 replaced/2 shared/1 deferred 与叶图、HANDOFF 三处一致；`Cargo.lock` 零改动（git diff 空）；叶计数 376 = 31+74+89+57+101+24 与报告一致；9/9 确定性契约无恒真（BE-01/02/04/05/06/08/10 亲读：断言全部落在真实文件系统/DB/线体事实上，BE-10 为常驻评分反例）；BLOCKED 登记合规。

## A15 裁决：成立

BE-10 在真二进制链上：任务「修改 a.md 追加 DONE-42」、替身写 b.md；机器断言 a.md 逐字节未动 + b.md=DONE-42 + run completed + 台账 2 行 succeeded；评分 schemaValid=true / semanticParamsCorrect=false / taskComplete=false / overall=fail，`assert!(!score.all_true())` 机器锁定不混算。四维分列在 jsonl 与 BEHAVIOR_EVAL.json 双载体可机核。**语法有效 ≠ 参数正确**成立。

## A16 裁决：成立

- BE-04（工具禁用，正反两腿）：解释腿 completed + 零工具意图；两强制腿均 failed、各恰 1 次物理调用、零 journal、零工作区副作用，**拒绝形状逐项相同（terminal_reason 相等断言）**——正常 prompt 与注入「放宽安全策略」prompt 拒绝无差。
- BE-05（权限不满足，正反两腿）：拒绝文本取自真实线体 tool 消息，含授权闸原文「not inside any authorized root」+ `resource_access_denied`，两腿**逐字相同**；越界文件全程缺席。
- 非恒真：断言对象是平台产出的真实字符串/状态，注入若改变网关行为即红。

## D-T08-1 裁决：缺陷真实、修复真实、有意偏离可接受（无需回改）

1. **修复前缺陷真实**：机制亲证——`drive_run.agent_id` 直喂 `compiler.compiled_for_run` → `materials.gather` 按 `agents/{id}/` 解析 → `subagent:{id}` 目录不存在 → 响亮 `ContextCompileFailure::Material` → `failed.provider_error`（runs.rs:1166、context_compiler.rs:862+）。RED 证据真实（`red_first_full_suite_run.txt` exit 101、6 失败含 BE-08；RED.md 记录两阶段发现与按现役词汇改锚）。
2. **R03 取消语义无回归**：取消传播走 CancelScope 树（`child_scope = parent.parent_scope.child(...)` + drive_run `register_linked`），与 agent 身份完全正交，diff 未触碰；`cancel_link_inheritance`（含真实子代理 child 超时腿 C04）/`cancellation_tree`/`cancel_terminal_race` 亲跑 28/28 绿。「按 agent 取消」从来不是取消机制（取消按 run 树），多子代理场景无丢失。
3. **偏离等价或更强**：旧 agent lane `subagent:{agent}` 本就是**每目标 agent**（跨线程/跨会话共享，旧注释「per thread」不准确——见 OBS-3），故 agent lane 粒度实质未变，只是并入真实代理命名空间——更符合现役「按 agent 配额」本意；session lane 由 `{session}::subagent::{thread}` 升为 `{session}::subagent::{agent}::{thread}`（严格更精确）；并发隔离另由 runtime per-session/global 上限承担。无饥饿/死锁：配额全局→agent→session 顺序加permit、RAII 无部分持有、等待有界且可取消退出；子代理派发 fire-and-forget，父不持模型 permit 等待子。身份修复同时纠正了材料解析/工具归属/用量台账归 identity——与现役「子代理继承派发代理裁剪后上下文」语义一致（BE-08 亲证 child prompt 无 main-scope 段）。

## Findings

### F-01（LOW）BE-04 token 字段硬编码 0/0，与申报来源不符
- REQUIREMENT_ID: R06-T08 Step2（token 实测）/ PI-10 r06_evidence
- FILE_AND_LINE: `rust/crates/lingxi-service/tests/r06_t08_behavior_eval.rs`（BE-04 `emit_eval_record`，`tokens_json(0, 0)`）；载体 `docs/rust-tauri/R06/BEHAVIOR_EVAL.json` tasks[BE-04].tokens
- OBSERVED: BE-04 record `inputTokens:0, outputTokens:0, source:"model_call_usage ledger (outcome=succeeded rows)"`——值系硬编码，该测试未调用 `usage_stats`。
- EXPECTED: 值应来自台账实测（BE-04 解释腿为单次 `sse_final`，同形状的 BE-09 同一台账路径实测 40/17，故真值非零）。
- REPRODUCTION: 读 BE-04 发块与 BE-09 对比；或以 `R06_T08_EVAL_OUT` 重放后比对 runs.db。
- ROOT_CAUSE: 多 run 腿（a/b/c 三 run）无单一 run_id 可喂 `usage_stats`，求省而写 0。
- SAME_ROOT_CAUSE_PATHS: 横扫其余 9 任务——BE-01/02/03/05/07/09/10 均 `usage_stats` 实测，BE-06 null 如实申报（按件计费），BE-08 申报 S 层无台账面。仅 BE-04 一处。
- IMPACT: 交付物一个信息性 token 字段与申报来源矛盾；不影响 A16 裁决、四维评分、extra_calls=0（stub hits 独立实测）与任何 verdict。
- REQUIRED_FIX: BE-04 对三 run 的 `model_call_usage` 求和后填入（或如实改标 source=not measured for multi-run legs）。
- REGRESSION_TESTS: 重放评测套件并机核 BE-04 tokens 与 runs.db 一致。

### OBS-2（OBS）BE-05 expectedBehavior 保留首轮误锚词
- tasks[BE-05].expectedBehavior 仍写 `resource_outside_write_scope`，而同记录 observed/refusalCode 已按真实链改锚（`resource_scope_refused/resource_access_denied` + authorized-root 原文；D-T08-4 在案）。建议随 F-01 一并修订文字，非阻断。

### OBS-3（OBS）D-T08-1 对旧语义的转述不精确
- 旧代码注释与差异台账称「每子代理线程独立 agent lane」，实际旧 lane `subagent:{agent}` 为每目标 agent 共享（非 per-thread）。裁决结论不变（见上），但台账转述宜更正，避免后续以讹传讹。

### OBS-4（OBS）`compiled_for_run` 缓存键缺 agent 段（潜伏）
- `ContextCompilerService` 缓存键 = `(session_id, for_subagent)`，不含 agent_id：同一会话若向**不同** agent 派发子代理，后编译者会命中前一个 agent 的缓存 artifact。现网唯一接线面（父代理回落，request.agent_id=None）不触发；R07 接线委派面（D-T08-3）时若放开 request.agent_id 需先补此键。

## VERDICT

A15 成立 + A16 成立 + T08-FIX-01 修复真实（RED→GREEN，生产可达）+ D-T08-1 有意偏离可接受（等价或更强，取消语义亲证无回归）+ 无阻断 findings（1 LOW + 3 OBS 均不要求回改候选，可作 R07/收口义务登记）→

**VERDICT: PASS**

## 证据文件清单（artifacts/rust-tauri/R06/T08-REVIEW-01/）

- `rerun_chain.log` — 四组命令退出码总账（FMT=0 / CLIPPY=0 / TARGETED=0 / FULL=101）
- `fmt.log`、`clippy.log` — 逐字输出
- `targeted_tests.log` — eval 10 + cancellation_tree 8 + cancel_link_inheritance 7 + cancel_terminal_race 13
- `full_tests.log` — 全量 145 target，1861/1（唯一失败 r00_management LAN 环境项）
- `r00_management_lan_solo_reviewer.txt` — solo 复跑（与登记同签名的环境失败原文）
- `behavior_eval_reviewer.txt`、`eval_records_reviewer.jsonl` — 审查者独立重放 10/10 + 原始 records
- `solo_eval_chain.log` — solo+eval 链路日志（含一次无效 solo 命令的即时失败与重试记录）
