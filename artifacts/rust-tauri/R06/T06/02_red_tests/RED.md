# R06-T06 RED 证据索引（先红记录；全部为真实命令真实退出码，无伪造）

基线：分支 `codex/rust-tauri-migration`，TASK_BASE_SHA `e89c91a2289c0c7182da9e2ca4b1d31dc56ca5b6`。
工具链：`/Users/study_superior/.cargo/bin/cargo`（1.98.1，rustc 1.98.1，见 `00_toolchain/toolchain.txt`）。

RED 证据分两类（任务允许的两种形态，未伪造任何一种）：

## 1) 实现缺席于 base 提交点（编译失败 / 首跑失败）

前位执行者会话中断，T06 实现以未提交工作区改动存在。把 5 个已跟踪改动文件
（storage/mod.rs、kernel/lib.rs、service/{auth,lib,runs}.rs）`git stash push`
（不含 untracked），即在 base `e89c91a2` 的实现面上运行 T06 测试：

| 文件 | 命令 | 退出码 | 结果 |
|---|---|---|---|
| `red_base_compile_adapters.log` | `cargo test -p lingxi-adapters --locked --test agent_memory_r06t06 --no-run` | 101 | `unresolved import lingxi_adapters::storage::agent_memory / skill_library`（模块在 base 未注册） |
| `red_base_compile_service.log` | `cargo test -p lingxi-service --locked --test r06_t06_hooks --test r06_t06_skills --no-run` | 101 | `unresolved import lingxi_service::{skills,hooks,memory,memorytools}`、`cannot find memory in lingxi_kernel` |
| `red_base_memory_runtime.log` | `cargo test -p lingxi-service --locked --test r06_t06_memory` | 101 | base 无 T06 路由/服务 → 7/7 HTTP 用例首跑全部失败（A11 面在 base 不存在） |

stash 后立即 `git stash pop` 恢复工作区（恢复后 5 modified + untracked 完整）。

## 2) 新增断言对前位实现的首跑失败（真实缺陷，修复前捕获）

续作者补强的测试（r06_t06_skills.rs / r06_t06_hooks.rs）对前位实现首跑，
暴露 3 个真实缺陷；以下日志为**修复前**捕获：

| 文件 | 暴露的缺陷 | 修复 |
|---|---|---|
| `red_first_run_skills.log` | ① `riskAcceptanceRejection` 未浮出（left: Null ≠ "missing_confirmation_token"）——现役 install-skill.ts:417/:529 在 details 里带失败验收码，前位实现丢弃；② memorytools 派发器按短名匹配 `request.target`，而 R05 协议适配器给出的 wire target 是全 id（`tool:first-party:search_memory`）→ 生产链上全部记忆工具必然 "unknown target" 失败 | skills.rs 补 `riskAcceptanceRejection`；memorytools.rs 取 target 末段本地名 |
| `red_first_run_hooks.log` | ③ `tool_result_guard` 对错误结果也做 L1 截断/注入扫描，违反其自身文档契约（compaction-guard-ext.ts:211-212 错误结果原样通过）；④ run 循环把全 id target 传给 name-keyed 的 loop guard（现役扩展语义按工具名，如 "read"）→ read 专用 reread 检测永不触发；⑤ Failed 结果丢弃 loop-guard 失败连击警告（警告必须到达模型） | hooks.rs 错误结果豁免；runs.rs 两处传本地名；runs.rs Failed 分支把警告前置进 error.message |

## GREEN 对应

以上每一项 RED 都有同点位的 GREEN 对应：`05_targeted_tests/` 中修复后
`r06_t06_skills`（5/5）、`r06_t06_hooks`（4/4）全绿；`06_workspace_regression/`
为终局权威回归。

## 3) R1 审查修复轮 RED（REPAIR-R06-T06-R1，审查报告 F-01/F-02/F-03）

| 文件 | 暴露的缺陷 | 修复 |
|---|---|---|
| `repair_R1_F01_F03_red.log` | F-01：bootstrap→run→compile 生产链上已启用技能不进发送文本（`enabled_skill_catalog_reaches_the_production_system_prompt` 断言 `<available_skills>` 失败——`ContextCompileInput.skills` 恒空、`resolve_runtime_selection` 无编译器消费者）；F-03：记忆工具重复注册被静默跳过（`memory_tool_registration_failure_is_loud_not_silent` 期望 panic 实际返回 Ok——`if let Ok(receipt)` 丢弃 Err） | F-01 接线：`SkillMaterialPort` 端口（context_compiler.rs）+ `runtime_skill_catalog`（skills.rs，含 disable-model-invocation 过滤/遮蔽去重）+ bootstrap 绑定（lib.rs）；配套 `resolve_runtime_selection.active_entries`（同名技能不再按名双激活）与 `list_for_agent` 按条目状态配对。F-03：注册改 `.expect` 响亮失败 |
| `repair_R1_F02_red_mutant.log` | F-02：guard 槽缺席 fail-closed 分支零测试覆盖——变异运行（把 `None => Unavailable` 临时改为 `None => Safe`，仅取证用、已复原）：既有 6 用例仍 5 通过（缺口证明），新增 `a12_layer1_guard_slot_absent_fails_closed_to_risk_confirmation` 转红（安装直通 `"ok":true,"safetyReview":true`） | 新增直接断言测试（缺席 → requiresRiskConfirmation + token + 不落盘 + 原因浮出 + 显式接受后放行）；D16「锁定测试」声明收窄为 token 协议链 |

GREEN 对应：`05_targeted_tests/repair_R1_targeted_suites.log`（修复后定向全绿）+
`repair_R1_green_skill_context_skills.log`（修复套件即时绿）；
`06_workspace_regression/` 为修复后全量权威回归。
