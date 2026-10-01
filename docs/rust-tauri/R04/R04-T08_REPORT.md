# R04-T08 报告｜工具结果、产物与全矩阵验收

执行代理：EXECUTOR-R04-T08-**E02**（一次性续做代理）。日期：2026-09-30。
分支 `codex/rust-tauri-migration`，基线 `ded3467bf`（R04-T07 已独立 PASS 并推送确认）。
状态：**READY_FOR_REVIEW**（两层自查完成；不自行判独立 PASS）。

> **E01 中断与 E02 续做的如实声明**：前一位执行代理 EXECUTOR-R04-T08-E01 在实现中途因宿主
> 用量上限中断，未写报告/交接/导航更新，验证完成状态未知。E02 启动时工作区含 E01 的未提交
> 进展（`xtask/src/{stage_map.rs,runner_identity.rs,main.rs}` 改动 + `stage_maps/R04.json`（4439 行）
> + `lingxi-service/src/artifactverify.rs`（全新）+ `toolgateway.rs`/`workerrpc.rs`/`filetools.rs`/
> `lib.rs`/`bin/r04_t07_fixture.rs` 改动 + `tests/r04_t08_tool_matrix.rs`（全新 2417 行）+
> T04/T07 测试改动 + `scripts/rust-tauri/r04_t08_{generate_stage_map.py,matrix.sh}` +
> `R04_ACCEPTANCE_LEDGER.json` + `artifacts/rust-tauri/R04/T08-E01/` 的 t07f1 三份日志）。
> E02 独立审计了上述全部落盘代码（不依赖 E01 聊天记忆），结论：**实现方向与主体完整正确、
> 无占位/半成品逻辑，但 E01 从未跑通验证**——`cargo fmt --check` 失败（stage_map.rs R04 钉测
> 未格式化）、`cargo clippy -D warnings` 失败（stage_map.rs:2368 `needless_borrow`）、验收账本
> R04-A03 的 evidence 字段残留一段 Python 表达式文本、矩阵测试
> `preauthorization-single-session-scoped` 案例的「跨会话」半边只有 `let _ = other_session_ctx;`
> 占位（断言未执行）、一处死代码 transmute；主门禁实跑又暴露两处阶段图作者错误（三个
> 「零洞」案例 expect 误钉=1、标准四命令未被场景引用而被 runner 跳过）。E02 按根因修复/
> 补齐（§2.3/§2.6/§5.1），补跑全部验证（§5），补全部交付文档与导航。中断历史与 E01 原有
> 成果（含 t07f1 复现/修复日志）均如实保留。

---

## 1. 目标与结论

将前七项接成真实可用的工具链，并证明 R05 接入时不再依靠假的工具结果：

- **统一外部语义与内部收据**：Success/Failed/Cancelled/Unknown 四态在真实 run 链上与 journal
  收据一致（`semantics-*` 四案例图钉）：Success=dispatched+Succeeded 且 wire 事件携带真实内容；
  Failed（拒派发，如越权写）=!dispatched+目标零副作用；Unknown（worker 超期限）=dispatched+
  Unknown 恰一条、不盲重试；Cancelled（用户 cancel）=无伪造成功收据。同一 read_only 上下文
  read 派发而 write 拒——逐调用独立授权（`gateway-each-call-independent-permission`）。
- **产物核验（A15 机制层）**：新增 `artifactverify.rs`——(1) claimed-artifact 核验（worker
  `claimed_files` →grant 内+存在+**常规文件**+工具声明的**内容契约**（min_bytes/UTF-8/JSON，
  读取 64KiB 有界），失败=响亮 `worker_claimed_artifact_invalid`，不铸造引用）；(2) 网关登记
  审计（`toolgateway.rs` dispatch 后、SUCCESS 落账前对每个 `file://` 引用核验存在+常规文件，
  失败转显式 Failed `gateway_artifact_verification_failed`，消息如实声明「已派发/可能有副作用/
  未登记为有效交付」）。远程 URI 永不强制走本地文件审计（按其类型验证——T07 桥已保证远程
  资源只映射文本描述）。
- **全矩阵**：7 个真实工具家族（read/write/edit/exec_command/write_stdin/MCP/worker）×
  权限上下文（operate/ask/read_only×用户会话 + 子代理 ReadOnly 档/Operate-of-ask 档）× 调用
  路线（直接两阶段 vs 完整 run 链）× 生命周期（disable/uninstall/generation bump/钉旧代次）
  的机器核验矩阵；未迁移的 11 个工具形态以 `Availability::Future` 诚实注册（可发现、零派发、
  不伪装 available）；不适用组合给出理由（子代理 write 衰减由驱动授权步承担——T02/T03 套件
  钉住，矩阵断言策略面一致性）。
- **正式 Gate**：`rust/crates/xtask/src/stage_maps/R04.json`（19 场景=16 A-ID+SUP01/SUP03/
  SUP05；124 个 R00 补充叶=55 stage_share_satisfied（份额叶全部带 assertionContract，逐案例
  图钉 56 个矩阵案例）+69 deferred_to_later_stage（不绑门禁命令，验收归 R06/R07/R08/R09））；
  真实生成器 `r04_t08_generate_stage_map.py`（逐字段镜像 R00 双账本，幂等复现）；STAGE_MAPS+
  runner_identity 注册（stale binary 检测）；生产者 `r04_t08_matrix.sh`（真链跑全矩阵并产出
  `lingxi.leaf-case-results.v1` 证据文件）；`verify-stage R04 --evidence <fresh>` 真实退出 0
  （含内嵌完整 verify-stage R03 回归）；9 例门禁负向测试全部非零退出+缺口点名。
- **遗留关闭**：T07-R1-F1（unique_dir 微秒碰撞→进程内计数器后缀；复现 3/8 失败→修复后
  15/15 绿，日志在案）；T04-R1-OBS-1（edit 重复计数空间→按现役 edit-diff.js 对齐为「恒在
  fuzzy 归一空间计数」并加回归，NFKC 折叠仍是登记的 T04 §8.6 差距）；T07-R1-O4（worker
  read_only 面补钉）。T05-R1-OBS-2 按冻结面纪律登记携带（不属本 Task 修复范围）。

结论：R04-A15/A16 及全部矩阵/对抗项真实执行通过（§4）；fmt/clippy/workspace 全量（887
通过 0 失败，含 r00 环境项本轮通过）/check-contracts/check-boundaries(+self-test)/完整
verify-stage R04（7 命令全 0、19/19 场景、55+69 叶、内嵌完整 verify-stage R03 回归
17/17）/9 例负向门禁全部在案（§5 退出码，含三轮主门禁尝试的完整历史）。

## 2. 实现摘要（关键文件:行，E01 原有 vs E02 补齐）

### 2.1 产物核验模块（E01 原有主体）

`rust/crates/lingxi-service/src/artifactverify.rs`（全新，369 行含 4 单测；E01 落盘、E02 审计
确认无半成品）：

- `ArtifactRejection`（L47，稳定码：artifact_missing/artifact_not_a_regular_file/
  artifact_out_of_scope/artifact_content_condition_failed）+`ClaimedFileContract{min_bytes,
  format: Any|Utf8Text|Json}`（L114）——min_bytes=0 允许空文件（诚实产物可以是空的；只有
  **承诺**非空/结构化的工具才声明更严契约）。
- `path_within_scopes`（L125）：组件级 `Path::starts_with`+canonicalize 回退（符号链接按真实
  目标判定）——把 T07 worker 执行器的判定收敛为唯一权威。
- `verify_claimed_artifact`（L137，layer 1）：grant 内→metadata 存在→`is_file()`（目录/fifo
  不是文件交付物）→大小下限→格式契约（`read_bounded` L195：64KiB 上限——内容契约永远不让
  不可信声明扩大宿主读取）。
- `audit_delivered_file`（L221，layer 2）+`local_path_of_uri`（L211，仅 `file://` 前缀——
  远端资源按其类型验证，绝不假装本地文件）。
- 4 条内嵌单测：目录声称非文件交付物/越界与缺失/有界内容契约（含空文件合法对照、非法
  UTF-8 只违反 Utf8 契约不违反 Any）/URI 提取与审计形状。

### 2.2 网关登记审计（E01 原有主体）

`rust/crates/lingxi-service/src/toolgateway.rs`：

- `execute_prepared` 派发后（L1267 起，E01 落盘）：`audit_success_artifacts(result)` 对 SUCCESS
  的每个 `file://` 资源引用执行 layer 2 核验；失败→`ToolOutcome::Failed`，错误码
  `gateway_artifact_verification_failed`（details.code 同名），消息强制三段事实：「execution
  WAS dispatched」「side effects may exist」「NOT registered as a valid deliverable」——已执行
  不冒称已交付（「请求已发送」不冒称「文件已生成」的反向同理）。
- `audit_success_artifacts`（L1326）：只做网关可自行推导的普适事实（存在+常规文件，登记时
  判定）；scope/契约条件归各自边界所有（worker grant、文件工具资源抽取器），不重复裁决。

### 2.3 worker claimed-files 收紧（E01 原有主体 + E02 审计确认）

`rust/crates/lingxi-service/src/workerrpc.rs`：

- `WorkerFailure::ClaimedArtifactInvalid{path, code, detail}`（L358，E01 新增稳定码
  `worker_claimed_artifact_invalid`，映射 InvalidMessage/不可重试）。
- `WorkerToolSpec.claimed_file_contract`（L577）：工具声明的产物内容契约（None=基础契约）。
- 执行器 claimed_files 处理（L1060 起重构，E01）：原「grant 内+exists」两查改为
  `verify_claimed_artifact` 一处权威——grant 内/存在/**常规文件**/内容契约全过才铸造本地
  ResourceRef；否则按拒绝类型映射 ClaimedUnauthorizedPath 或 ClaimedArtifactInvalid，**worker
  已运行（副作用可能存在）但无任何登记为已交付**。
- `bin/r04_t07_fixture.rs`（E01 新增 3 模式，L141-176）：`claim_missing`（worker 删除自己的
  grant 内交付物再声称）/`claim_dir`（替换为目录再声称）/`claim_bad_json`（写非法 JSON 再
  声称）——A15 对抗腿的外部双。

### 2.4 T04 OBS-1 修复（E01 原有）

`rust/crates/lingxi-service/src/filetools.rs` `count_occurrences`（L855 起）：计数**恒在 fuzzy
归一空间**（两侧都过 `normalize_for_fuzzy`），对齐现役 edit-diff.js `countOccurrences`
（L177-180 无论哪层匹配都归一化两侧）——`it's`+`it’s` 混合文件对 oldText `it's` 报
「Found 2 occurrences」，不再静默走 exact 层只换 ASCII 份。NFKC 折叠差异保持登记（T04
§8.6，未迁移不伪装）。回归 `tests/r04_t04_file_tools.rs::
edit_duplicate_counting_runs_in_the_fuzzy_space_like_the_incumbent`（E01，L1713 起：OBS-1 本例
+两个合法对照）。

### 2.5 T07 F1 修复（E01 原有）

`tests/r04_t07_mcp_and_workers.rs` `unique_dir`（L73 起）：进程内 `AtomicUsize` 计数器后缀
（T04/T05/T06 约定）。复现证据：`t07f1_reproduce_before_fix.log`（8 轮 3 失败，
`table invocation_journal already exists` @r04_t07_mcp_and_workers.rs:587）+
`t07f1_panic_signature.txt`；修复后 `t07f1_after_fix_15runs.txt`（PASS=15 FAIL=0 of 15）。

### 2.6 R04 stage map 与钉测（E01 原有主体，E02 修 fmt/clippy）

- `scripts/rust-tauri/r04_t08_generate_stage_map.py`（562 行）：124 R00 叶从 R00 双账本
  （ACCEPTANCE_MAP+FEATURE_STAGE_ACCEPTANCE）逐字段镜像（xtask 运行前交叉核对每个字段）；
  55 share/69 deferred 分类表可复现——E02 实测**幂等**（diff 零字节）。
- `xtask/src/stage_maps/R04.json`（4439 行）：7 命令（rust_fmt/rust_clippy/
  rust_test_workspace/check_contracts/check_boundaries/**r04_tool_matrix**/**r03_regression_gate**）
  +19 场景+124 叶；份额叶 55 个全部带 assertionContract（56 案例图钉，actual==expect 机器
  核验）；r03_regression_gate 在本图内对最终候选完整重跑 verify-stage R03（SUP-05）。
- `xtask/src/main.rs`（STAGE_MAPS 注册 R04）/`runner_identity.rs`（embedded+SOURCE_INVENTORY
  追加 R04.json——stale binary 检测覆盖新图）。
- `xtask/src/stage_map.rs` R04 钉测（L2101 起，E01 主体；**E02 修**：rustfmt 重排 +
  L2368 `needless_borrow` clippy 违规——`contains(&known)`→`contains(known)`；**E02 补两项
  防护**）：
  `r04_production_map_keeps_the_sixteen_a_scenarios_verbatim`（16 A-ID 的 commandRefs 冻结——
  **E02 将 R04-A15 的 refs 扩为 6**：+rust_fmt/rust_clippy/check_contracts/check_boundaries，
  因为未被任何场景引用的命令会被 runner 静默跳过——R03-A15 同款绑定模式）、
  `r04_production_map_registers_the_supplemental_duty_scenarios`（SUP01/03/05）、
  `r04_production_map_registers_the_matrix_and_regression_producers`（r04_tool_matrix argv/
  证据路径/r03_regression_gate 存在性/生产者脚本存在性）、`r04_production_map_keeps_the_
  124_leaf_split`（124 叶=55+69；份额叶必带契约；每个图钉案例都在 56 案例真实集合内；非
  场景证据案例外**每个已知案例必须被至少一个份额叶图钉**——死证据=缺口）、**E02 新增
  `r04_production_map_pins_the_real_case_expectations`**（56 案例的 expect 值镜像自生产者
  record_case 调用：主门禁首轮实跑暴露 E01 生成器 share 表把三个「零洞」案例钉成
  expect=1——名字级钉测抓不到值错，本测试把它移到 `cargo test -p xtask` 即红并点名案例）。

### 2.7 全矩阵测试与生产者（E01 原有主体，E02 补一处断言）

`tests/r04_t08_tool_matrix.rs`（2442 行，10 个 `#[tokio::test(flavor="multi_thread")]`——含 E02 补的
preauthorization 跨会话探针；R1 审查 N2 校正）+
`scripts/rust-tauri/r04_t08_matrix.sh`（109 行生产者：offline 锁定构建→`--test-threads=2`
真跑→grep 绿→python 组装 leaf-cases.json/matrix-counts.json（**零案例片段=响亮失败**——
0 匹配过滤器的负向锚点））。

**E02 补齐**（`approval_face_share_cases` 预授权段）：
- 删除 E01 残留的死代码 `unsafe { std::mem::transmute::<&Harness, &Harness> }`（L1633 原位置，
  无语义作用）。
- `preauthorization-single-session-scoped` 案例补上被 E01 省略的**跨会话**断言：另一会话
  （`sess_other`）以同 target+canonical digest 在 ask 档 prepare→必须 `NeedsApproval`
  （看不到 `sess_local_alpha` 的 grant，也不消耗它——单次使用后续仍可用）。经真实
  `ToolPolicyPort::adjudicate` 消费路径（approval_service.rs L648 `grant.session_id == session_id`
  判定为生产判定面）。原 `let _ = other_session_ctx;` 占位移除。

## 3. 修改范围与调用链

全部改动（相对基线 ded3467bf，未提交）：

| 文件 | 性质 | 内容 |
|---|---|---|
| `lingxi-service/src/artifactverify.rs` | 新增（E01） | 产物核验双层权威（§2.1） |
| `lingxi-service/src/lib.rs` | 修改（E01） | `pub mod artifactverify;` |
| `lingxi-service/src/toolgateway.rs` | 修改（E01） | 登记审计接入 execute_prepared（§2.2） |
| `lingxi-service/src/workerrpc.rs` | 修改（E01） | ClaimedArtifactInvalid+contract+收紧（§2.3） |
| `lingxi-service/src/filetools.rs` | 修改（E01） | OBS-1 计数空间（§2.4） |
| `lingxi-service/src/bin/r04_t07_fixture.rs` | 修改（E01） | A15 对抗三模式 |
| `lingxi-service/tests/r04_t08_tool_matrix.rs` | 新增（E01+E02） | 全矩阵+A15/A16+语义（§2.7） |
| `lingxi-service/tests/r04_t04_file_tools.rs` | 修改（E01） | OBS-1 回归 |
| `lingxi-service/tests/r04_t07_mcp_and_workers.rs` | 修改（E01） | F1 修复+read_only 面补钉+contract 字段 |
| `xtask/src/{main,runner_identity,stage_map}.rs` | 修改（E01，E02 修 fmt/clippy） | R04 图注册+钉测 |
| `xtask/src/stage_maps/R04.json` | 新增（E01） | 19 场景+124 叶 |
| `scripts/rust-tauri/r04_t08_generate_stage_map.py` | 新增（E01） | 真实生成器（幂等） |
| `scripts/rust-tauri/r04_t08_matrix.sh` | 新增（E01） | 矩阵生产者 |
| `scripts/rust-tauri/r04_t08_gate_negative_tests.sh` | 新增（**E02**） | 9 例门禁负向 |
| `docs/rust-tauri/R04/R04_ACCEPTANCE_LEDGER.json` | 新增（E01，**E02 修 A03 指针**） | 16 A-ID+义务→证据 |
| `docs/rust-tauri/R04/R04_{T08_,}REPORT.md`、`R04_HANDOFF.json` | 新增（**E02**） | 报告与交接 |
| TEST_MAP/PLATFORM_CAPABILITIES/ORCHESTRATOR_PROGRESS | 修改（**E02**） | T08 条目收尾 |

调用链（A15 网关审计位）：`execute_prepared → dispatch_executor（真实执行器）→
audit_success_artifacts（file:// 引用逐个 audit_delivered_file）→ 落账`；失败路径不回滚
已发生副作用，只拒绝**登记**。生产默认入口零改动：`ServiceDeps` 的工具面组装仍是显式
opt-in（bootstrap 不自动注册工具族——与 T04/T05/T06/T07 同口径，A16 保护语义）。

## 4. 验收结果（逐 A-ID/义务）

### 4.1 基础场景

| ID | 场景 | 实际执行（真实链） | 结果 |
|---|---|---|---|
| R04-A15 | 空产物不算成功 | `a15_empty_and_invalid_products_never_register`：真实 worker 子进程链——claim_ok 对照（grant 内存在→ResourceRef 铸造+审计存活+size 正确）；claim_outside（越权→`worker_claimed_unauthorized_path`，受限哨兵字节不变）；claim_missing（worker 删自己交付物→`worker_claimed_artifact_invalid`+`artifact_missing`+「nothing is registered as delivered」）；claim_dir（目录替换→`artifact_not_a_regular_file`——修复前 `exists()`-only 检查会放行）；claim_bad_json（JSON 契约→`artifact_content_condition_failed`+「not valid JSON」）。`a15_gateway_registration_audit_converts_fake_refs`：网关层——真实交付对照通过；幽灵文件引用→`gateway_artifact_verification_failed`+三段事实消息；目录引用同拒。单测 4 条（artifactverify）。图钉 8 案例全 actual==expect | PASS |
| R04-A16 | 禁用状态覆盖所有路线 | `a16_disabled_state_covers_every_route`：先真执行留 Succeeded+dispatched 历史收据→缓存描述+双 prepared 句柄（本体+别名）→disable→6 路线：fresh prepare（TargetNotCallable/StaleCatalog）/缓存句柄（TargetChanged）/别名 ByName（拒）/缓存别名句柄（TargetChanged）/完整 run 链（journal Failed+dispatched=false）/钉旧代次（StaleCatalog）——**holes=0**；历史收据 disable 后保留（a16-history-preserved=1，无删历史掩盖）；uninstall 腿（resolve 拒+缓存句柄拒，holes=0）；generation bump 腿（schema 语义变化→缓存句柄 TargetChanged，holes=0） | PASS |

### 4.2 矩阵与语义（Task 要点 1/3）

| 项 | 实际执行 | 结果 |
|---|---|---|
| 家族×权限×入口 | `matrix_tools_x_permission_x_entry_consistency`：7 家族（family-count=7 图钉）× operate（全 Allowed）/ask（Read Allowed、其余 NeedsApproval）/read_only（Read Allowed、其余 Denied）/子代理 ReadOnly 档/子代理 Operate-of-ask 档（Read Allowed、其余 Denied=TOOL_APPROVAL_UNAVAILABLE）——35 格一致（permission-consistency=1）；SUP-01 结构化拒词汇逐字断言（code+deny_on_prompt+allowHumanApproval+the action was not run）；直接路线 vs 完整 run 链同判定同收据（route-consistency=1） | PASS |
| 未迁移能力诚实 | `matrix_future_tools_honest_in_catalog`：11 形态 Future——search 命中+describe 诚实+prepare 零派发 TargetNotCallable（11 案例逐个图钉） | PASS |
| 四态语义+收据 | `outcome_semantics_and_receipts_unified`：见 §1（4 案例+独立授权案例） | PASS |
| 真实文件链 | `native_tool_share_cases_on_the_real_chain`：read/write/edit 真实字节核对+edit 冲突保 v2（FILE_STALE_SINCE_READ）+exec_command 真进程 | PASS |
| PTY/终端 | `terminal_family_share_cases`：tty Running 句柄/tail 游标只交付新输出/第二 marker 不含旧输出/foreign 会话 write_stdin 拒/close 后迟到写诚实「not running」/取消后 live_handles 清空 | PASS |
| MCP 面 | `mcp_mechanism_face_share_cases`：真实握手注册/identity/命名空间 search/快照含权限契约/T03 面三档（operate=allowed/ask=needs-approval/read_only=denied）/完整链真实内容 | PASS |
| 批准面 | `approval_face_share_cases`：三模式可设可读/ASK park→批准恰一次/重复点击 AlreadySettled/REJECT 零派发（dispatched_count=0）/预授权跨会话不可见+单次消费（E02 补跨会话腿） | PASS |

### 4.3 遗留与义务

| 项 | 处置 | 结果 |
|---|---|---|
| T07-R1-F1 | unique_dir 计数器后缀；复现 3/8→15/15 绿（日志三份在 artifacts） | CLOSED |
| T04-R1-OBS-1 | 修复对齐（fuzzy 空间计数）+回归三腿；NFKC 差距保持登记 | CLOSED（选择修复而非裁定的依据：现役 edit-diff.js 计数语义明确可对齐，且 OBS-1 差异在混合变体文件上产生真实的用户可见分歧） |
| T07-R1-O4 | worker read_only 面断言补钉（ACTION_BLOCKED_BY_READ_ONLY） | CLOSED |
| T05-R1-OBS-2 | 登记携带（R04_HANDOFF.deferred_registrations[t05-obs2-registryfull-tail]）——T05 源码不在本 Task 修复范围，非阻塞 | REGISTERED |
| SUP-01 | sup01 案例图钉+T03 套件全量（rust_test_workspace 内） | HELD→PINNED |
| SUP-02 | 网关零授权逻辑/零存储不变；全矩阵经同一判定链 | HELD |
| SUP-03 | F1/F2 已于 T01/T02 关闭；F3/F4 携带；check-contracts 零漂移 | HELD |
| SUP-04 | 11 future 形态逐叶图钉（含 laterShare 指向） | PINNED |
| SUP-05 | r03_regression_gate 命令在 R04 图内完整重跑 verify-stage R03 | PINNED |

## 5. 验证（真实命令与退出码）

环境：macOS darwin 27.0.0 arm64；rustup 1.98.1（`/Users/study_superior/.cargo/bin/cargo`）。
仓库根执行；原始输出归档 `artifacts/rust-tauri/R04/T08-E01/gates/`。**保留首次失败**：

| # | 命令 | 退出码 | 说明 |
|---|---|---|---|
| 1a | `cargo fmt --all -- --check`（E01 落盘首跑） | **1** | stage_map.rs R04 钉测未格式化（E01 中断于验证前） |
| 1 | `cargo fmt --all` 后 `--check` | 0 | `gates/rust_fmt_check.log` |
| 2a | `cargo clippy --workspace --all-targets --locked -- -D warnings`（首跑） | **101** | stage_map.rs:2368 `needless_borrow`（E01 遗留） |
| 2 | 同上（E02 修复后） | 0 | `gates/rust_clippy.log` |
| 3 | `cargo test --workspace --locked` | 0 | **887 通过/0 失败**（82 套件全 ok；含 r00_management_leaves 本轮通过——环境窗口良好；含完整 r04_t08_tool_matrix 套件）。注：该轮编译于 E02 补 preauth 断言之前；当前版本套件另直跑 **10/10 exit 0**（`gates/r04_t08_matrix_edited_suite.log`，300.87s），且最终门禁 attempt3 的 rust_test_workspace 即当前树（exit 0） |
| 4 | `cargo run -p xtask -- check-contracts` | 0 | 零漂移 |
| 5 | `cargo run -p xtask -- check-boundaries` | 0 | O1-O8+D1-D5+B1 |
| 6 | `cargo run -p xtask -- check-boundaries -- --self-test` | 0 | N1-N17 |
| 7 | `python3 scripts/rust-tauri/r04_t08_generate_stage_map.py` + diff | 0 | 生成器幂等（R04.json 零字节差异） |
| 8 | `bash scripts/rust-tauri/r04_t08_matrix.sh <fresh>`（经门禁命令 9 执行） | 0 | 全矩阵绿+56 案例组装（见 9） |
| 9a | `cargo run -p xtask -- verify-stage R04`（attempt1，E01 原图） | **1** | 7 命令全 0 但 **3 个份额叶 FAIL**：E01 生成器把 matrix-lifecycle-{disable,uninstall,generation}-holes 三案例误钉 expect=1（零洞语义=0；叶 R00-T02-LA-FAE7503D0D0F/2D194C1684BC/F4DA2AFCB72B）——门禁按设计抓到。归档 `verify-R04-attempt1/` + `gates/verify-stage-R04-attempt1.log` |
| 9b | 同上（attempt2，expect 修复后） | 0 | 3 命令全绿 19/19 场景——但审计发现 fmt/clippy/contracts/boundaries 四命令未被任何场景引用而被 runner 跳过（声明≠执行）。归档 `verify-R04-attempt2/` |
| 9 | `cargo run -p xtask -- verify-stage R04 --evidence artifacts/rust-tauri/R04/T08-E01/verify-R04`（attempt3，最终图：三 expect 修复+A15 绑标准四命令+新值钉测） | **0** | **7/7 命令全 0**（rust_test_workspace/r04_tool_matrix/rust_fmt/rust_clippy/check_contracts/check_boundaries/r03_regression_gate），**19/19 场景 PASS**，55 份额叶 PASS+69 递延+0 fail，candidateSourceBinding stable=true（2296s）；`gates/verify-stage-R04.log` + `verify-R04/verify-stage-result.json`；嵌套完整 verify-stage R03（17/17 场景、17 份额叶 PASS、15 命令全 0）于 `verify-R04/R03_REGRESSION/` |
| 10 | `bash scripts/rust-tauri/r04_t08_gate_negative_tests.sh`（/tmp 隔离副本：git worktree@HEAD+未提交门禁文件；cargo target 在 `$HOME/.cache`） | 0 | **9 用例全部非零退出+缺口点名**：未知阶段(2)/空集(2)/删叶映射(1)/删命令映射(2)/删 SUP 场景(101)/0 匹配过滤器(1，「no case fragments were produced」)/缺失证据文件(1，missingEvidence 点名路径)/陈旧证据根(1)/候选中途漂移(1，stable=false+「Candidate file bytes or HEAD changed」)；`negative-tests/case-results.json`（allRefused=true）。**Caveat 如实登记**：电池跑于 expect 修复+A15 绑定前的图（9 个负向形状的检测逻辑均不依赖这三处图内容——未知阶段/空集/加载前拒绝是 runner 行为；n2/n3/c03b 的缺口点名文本各自独立于叶 expect 值）；最终图的正向验证=attempt3 全绿。电池中途被宿主杀过一次 wrapper（n3 后），c03b 以相同隔离方法独立重跑并合并（`negative-tests/c03b_rerun_driver.sh`）；full-gate 用例在 worktree 副本带一个已知环境红（嵌套 R03 的 r02_legacy_regression 假设 .git 为目录，worktree 副本的 .git 是文件——主仓库门禁不受影响，attempt3 已证） |
| 10a | 负向脚本首两轮的现场记录（非门禁缺陷） | — | (a) `--evidence` 路径穿过 `/tmp`→`/private/tmp` 符号链接被候选绑定**拒绝**（门禁自身 fail-closed 正确触发）——修复=证据根改仓库内绝对路径；(b) 隔离副本构建目标先置于 `/tmp`：T06 A12 的「受限哨兵」按冻结契约本就可写 `/private/tmp`（tests/r04_t06_sandbox.rs L124-133 注释明示 CARGO_TARGET_TMPDIR 必须避开），目标目录在 /tmp 使哨兵落在契约可写区→A12 环境性变红——修复=NEG_TARGET 移至 `$HOME/.cache/lingxi-r04-t08-neg-target`。两轮现场日志未保留（worktree 已清理），结论以脚本注释与本行登记为准 |

R03 十套件+A15+A16：完整 `verify-stage R03` 在命令 9 的 r03_regression_gate 内部于最终候选
重跑（嵌套证据 `verify-R04/R03_REGRESSION/`），非仅单脚本。

### 5.1 E01 中断事实与主门禁三轮尝试（保留）

E01 落盘时验证零执行：无 gates/ 目录、无 verify-stage 结果、报告/交接/导航全部缺失。
t07f1 三份日志是 E01 唯一验证产物（复现 3/8→15/15）。E02 审计+实跑发现并修复的 E01 遗留
共 7 处（两类）：
- 工程遗留 5 处：fmt 未过、clippy `needless_borrow`、账本 A03 evidence 字段 Python 表达式
  残留、preauth 跨会话断言占位（`let _ = other_session_ctx;`）、死代码 transmute。
- 阶段图作者错误 2 处（主门禁实跑暴露）：(a) 生成器 share 表把三个「零洞」案例误钉
  expect=1（attempt1 FAIL，三份额叶红）；(b) 标准四命令（fmt/clippy/check-contracts/
  check-boundaries）声明但未被任何场景引用→runner 跳过（attempt2 只有 3 命令执行）。
  修复均在生成器根因层，并补 `r04_production_map_pins_the_real_case_expectations` 值级
  钉测+A15 绑定（R03-A15 模式）。三轮主门禁日志全部归档（attempt1/attempt2/最终）。

另：E01 遗留一个未清理的 git worktree `/private/tmp/r04t08-neg`（ded3467bf detached +
E01 未提交改动的副本，2026-10-01 06:59），E02 未删除（保留其中断现场事实）；总控提交前
可自行裁决清理（`git worktree remove --force`）。

## 6. 对抗性自查（十反例族）

| 反例族 | 尝试推翻 | 结果 |
|---|---|---|
| 工具身份 | 同名跨来源/别名路由在 disable 后仍可达？（A16 别名 ByName+缓存别名句柄）；钉旧代次请求 | 全拒，holes=0 |
| 参数 | 摘要 A 载荷 B（T02 套件）；预授权跨会话/跨 digest 复用（本 Task 补跨会话腿：sess_other 看不到 grant） | 拒 |
| 授权 | 伪 principal/跨会话句柄（T02）；SUP-01 ask 子代理写 | 结构化拒 |
| 时序 | 批准后 disable（T03）；批准重复消费（AlreadySettled）；REJECT 后迟到（零派发）；取消竞争（cancel_run 与 sleep 300 真进程） | 全部确定结果 |
| 文件 | edit 冲突保 v2；越权写零副作用（semantics-failed 腿：../restricted 无文件落地）；目录/幽灵/坏 JSON 产物 | 拒且事实准确 |
| 进程 | 孙进程清理归 T05 套件；本 Task 轻量复钉（取消后 live_handles 清空）；close 后迟到写「not running」 | 通过 |
| MCP/worker | fixture 对抗模式（自毁交付物三态）；worker 越界声明；永续分页（T07）；断回执 Unknown（semantics-unknown） | 全拒/诚实 Unknown |
| 恢复 | Unknown 不盲重试（journal 恰一条）；重启语义归 R03 回归 | 通过 |
| 结果 | success 无文件（A15 双层）；「请求已发送」≠「文件已生成」（三段事实消息） | 拒登记 |
| 证据 | 门禁 9 负向（含删映射/0 过滤器/陈旧根/漂移）；份额叶死案例检测（钉测断言非场景案例必被叶图钉）；**主门禁三轮自审：attempt1 的 FAIL=门禁抓到 E01 的 expect 误钉（证明叶契约核验真实生效）；attempt2 审计发现声明≠执行（未引用命令被跳过）→A15 绑定修复**——执行者把「自己写的图跑绿」当线索而非结论，逐轮深挖至根因 | 全部 fail-closed |

## 7. 未验证项与风险

- **平台**：全部动态验证在 macos-arm64 本机；linux 源码级/windows 拒绝形态见
  PLATFORM_CAPABILITIES（T06 冻结，本 Task t08_closure 节登记再消费方式），真机归 R09/R10。
- **r00 环境项**：`r00_management_leaves` 的 LAN 自连腿受 macOS 应用防火墙间歇影响（历史登记）；
  本轮 workspace 全量与门禁内嵌 workspace 均通过（环境窗口良好）。若后续复现，按 T07 报告
  §5.1 口径隔离核证、不销项。
- **npm/TS 侧未运行**：本 Task 未触碰任何 TS/Node 源（§3 改动清单），按派单「如触碰」条件
  不适用；旧 Pi 工具工厂与生产默认入口零改动。
- T05-R1-OBS-2 携带（§4.3）；NFKC 折叠差异为登记的 T04 §8.6 递延（R06）。

## 8. 回退

未提交任何 commit（按派单禁止）。回退=丢弃工作区未提交改动即可（git restore/clean 由总控
在授权后执行）；无数据迁移、无生产入口切换、无外部副作用残留（测试全部使用
`std::env::temp_dir()` 隔离目录与合成 fixture 子进程）。

## 9. 交付物索引

- 本报告；`R04_REPORT.md`（91 模板阶段报告）；`R04_HANDOFF.json`（R05 交接）；
  `R04_ACCEPTANCE_LEDGER.json`；`R04_TEST_MAP.json`/`R04_PLATFORM_CAPABILITIES.json`/
  `ORCHESTRATOR_PROGRESS.json` T08 条目。
- 证据：`artifacts/rust-tauri/R04/T08-E01/`（gates/ 逐命令日志+退出码；t07f1 三份 E01 日志；
  verify-R04/ 完整门禁证据树含嵌套 R03_REGRESSION；negative-tests/ 9 用例）。
- 下一步：全新独立 Task Reviewer 验收 R04-T08；通过后总控 commit/push，再进入阶段组合验收
  （阶段 Reviewer 另派）。
