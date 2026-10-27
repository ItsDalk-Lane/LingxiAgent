# R03 阶段修复工作单 G01 记录（F01 编辑 / F02 部分验证 / F03 总控接管验证）

- 工作单：docs/rust-tauri/R03/dispatches/R03-STAGE_REPAIR_G01_F01_DISPATCH.md（T08 审查 R1 §12 移交三项）
- 基线：77bf3bad19e65335269e3f1e7d1dbdbdaba1e259
- 性质：单写者阶段修复；本文件为总控维护的事实性记录，判定权归 STAGE-REVIEWER-R03。

## 执行链（真实代理）

| 代理 | 身份 | 结果 |
|---|---|---|
| STAGE-REPAIR-R03-G01-F01 | 修复执行（agent 会话，ZCode Agent 工具） | 完成三项编辑后运行验证时 600s 无输出被环境挂起；编辑留存于工作树 |
| STAGE-REPAIR-R03-G01-F02 | 续做验证（全新 agent） | 审读并接受 F01 diff（零修改）；verify-stage R02 跑至最后一条 a16 时同样被挂起；其证据（a07-directed/a14-directed/verify-stage-r02 部分进度）保留于 artifacts/rust-tauri/R03/STAGE-REPAIR-G01-F01/ |
| STAGE-REPAIR-R03-G01-F03 | 总控接管验证（本记录） | 后台运行完整矩阵，证据 artifacts/rust-tauri/R03/STAGE-REPAIR-G01-F03/ |

## 逐项修改（F01 产出，diff 可审）

1. **FINDING-1 文档更正（三份，不改代码）**：
   - docs/rust-tauri/R03/R03-T06_REPORT.md：「父取消传播到子」行的更正——原句删除线保留 + 裁决语义（spawn_linked biased-select 先丢 drive future=监督层立即停止、A06 成立；durable 行由启动扫描两阶段收口 interrupted_needs_attention；child 自身超时路径仍自身四相取消落 cancelled）。
   - docs/rust-tauri/R03/R03_HANDOFF.json：R03-T08-FINDING-1 条目追加 G01-F01 执行记录（产线修复归属不变，不自标关闭）。
   - docs/rust-tauri/R03/R03_REPORT.md：已知缺陷条目追加执行记录注记。
2. **FINDING-2 等价断言**：scripts/rust-tauri/r02_t04_live_fault_evidence.py restart 段 keyEvents 1→2 + 新增 status=interrupted_needs_attention 钉住。等价性：沿 R03-T07 已获审的库内等价断言先例（execute_concurrency.rs 夹具改动 2，(1,1)→(1,2)+no-fake-success 状态断言）；runs 恒 1（无新 run）、状态钉死意味着伪造 completed 或空白行现在都会失败——严格强于原裸计数。
3. **FINDING-3 等价断言**：scripts/rust-tauri/r02_t07_slow_subscriber.sh 并发风暴「全部 200」→「200 或 409 session_busy（retryable）」；409 = R03-T02 已验收的现役 Node 同语义冻结（desktop-session-submit.ts/sessions.ts busy 路径），非降保护。

## 验证矩阵（F03 总控后台运行，真实退出码）

| 命令 | 结果 |
|---|---|
| verify-stage R02（STAGE-REPAIR-G01-F03/verify-stage-r02） | **overall FAIL=仅 a16 一条红**；其余 19/20 全绿：含 a07_live_fault_02_check **PASS**（FINDING-2 修复生效）、a14_slow_subscriber **PASS**（FINDING-3 修复生效）、a07_a08_storage_tx/a09_a10/a11/a12/a13/a15 与 5 个 supplemental 全 PASS；48 叶 17 share PASS + 31 DEFERRED；candidateSourceBinding stable |
| a16_legacy_regression | exit 1——E5a 候选副本全量 npm test 因 2 个 vitest worker fork 崩溃（"Worker forks emitted error"/"Worker exited unexpectedly"，资源竞争类环境干扰）致日志形状不可解析，分类器按设计闭失败。6 个失败测试全部为注册封印族（post-verification-audit-seal + round2/round3-delivery-evidence 的 VERIFIED_SOURCE_SHA 滞后类，与 T08 审查 R1-E5 裁决一致）。已原样重试一次（a16-retry/），结果待补记 |
| verify-stage R03（修复后候选） | 待 a16 重试后运行 |
| workspace --locked / fmt / clippy / check-contracts / check-boundaries | 待补（R03 图命令内含；将随 verify-stage R03 全新证据目录执行） |

## a16 处置结论（三次真实运行）

1. **第一次**（verify-stage R02 内）：npm test 2 个 vitest worker fork 崩溃（"Worker forks emitted error"/"Worker exited unexpectedly"，资源竞争类环境干扰）→ 日志形状不可解析 → 闭失败。6 个失败测试全部为注册封印族。
2. **第二次**（a16-retry/）：总控把重定向日志写进未排除绑定的路径，镜像检查拒跑（脚本自身防篡改机制正确工作）——不计为产品结果。
3. **第三次**（a16-retry-2/，日志在 /tmp、证据在脚本排除子树）：npm test 干净跑完（3 文件/6 测试全为注册封印族；文件级覆盖 PASS；no-new-reds PASS：候选红 ⊆ 基线重放红 ∪ 注册族）。E5 仍闭失败，round3 块分类 `UNRECOGNIZED,seal-coordinate-lag`，与 R02 接受候选 e876171c 的同块逐行对比定位根因：
   - R02 接受时 round3 = `seal-coordinate-lag,uncommitted-source-rejection`（同头部形状，含 wrapper，全部注册）。
   - 本轮 round3 块尾新增 **`patch replay failed: error: patch too large`**：round3 审计测试从固定古老基线 89bc0b64 生成到 HEAD 的增量补丁，随 R02+R03 证据与代码累积超出该审计脚本自身的大小上限。这是**审计重放机制的规模耗尽**（随阶段单调增长，seal 工作流推进重放基线前只会更差），非 R03 产品回归、非旧封印坐标滞后类——按「新出现的失败不能一律套用旧 seal 类别」登记为**独立治理项**，交 STAGE-REVIEWER-R03 裁决。不在本阶段修改审计测试或扩 a16 分类器注册类消红（红线）。

## 最终矩阵状态（G01-F03）

| 项 | 结果 |
|---|---|
| verify-stage R02 | 19/20 绿（修复前 17/20）；唯一红 a16/E5：注册封印族（分类态）+ 新增 round3 patch-too-large 治理项 |
| verify-stage R03（修复后候选，STAGE-REPAIR-G01-F03/verify-stage-r03） | **overall PASS，14/14 命令 exit 0**（含 workspace --locked 全量/fmt/clippy/check-contracts/check-boundaries/A15 矩阵/A16 seed/7 条 R02 定向），16/16 场景，48 叶 17 share PASS + 31 DEFERRED |
| a16 三次运行证据 | verify-stage-r02/a16_legacy_regression/、a16-retry/、a16-retry-2/（+ /tmp/a16-retry-2.*） |

## 剩余问题

- a16/E5 双层红（注册封印族分类态 + round3 patch-too-large 新治理项）→ 阶段审查裁决；seal 坐标推进与重放基线前进属获授权的封印流程，本阶段不伪造、不消红。
- 判定权：本记录不构成 PASS/FAIL 判定，归 STAGE-REVIEWER-R03。
