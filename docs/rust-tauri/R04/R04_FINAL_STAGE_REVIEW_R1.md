# R04 最终阶段审查报告（R04_FINAL_STAGE_REVIEW_R1）

```text
STAGE_VERDICT: PASS
```

- 审查者：**STAGE-REVIEWER-R04-R01**（2026-09-30/10-01，一次性全新阶段审查代理；未参与本阶段任何执行、修复、Task 审查或总控编排）。
- 候选：`R04_FINAL_CANDIDATE = 0c67b6fc749ac5c21d0514e3076b0ed2058cd7c4`（`refactor(rust-tauri): R04-T08 tool results, artifact verification and full matrix gate`）。
- 起点：`R04_START_SHA = bf6450bcd722668188d1a3091681bf285875ef11`。分支 `codex/rust-tauri-migration`；审查期间 `git status` 干净、`git worktree list` 无残留（T08 报告 §5.1 提及的 `/private/tmp/r04t08-neg` 已清理）。
- 远程回执：`git rev-parse origin/codex/rust-tauri-migration` = `0c67b6fc7…`（= 本地 HEAD；`git branch -r --contains 0c67b6fc7` 含 origin 分支）。
- 本审查者的独立证据根：`/private/tmp/r04-stage-review/`（verify-R04 全新门禁证据树 + 自写探针 crate `/private/tmp/r04-stage-review/probe`）。未修改任何产品、测试、配置、阶段图或原始证据；本报告是唯一写入仓库的文件。

## 0. 结论摘要

R04 八个 Task 在候选 `0c67b6fc7` 上可接受：

1. **完整 R04 Gate 由本审查者在全新证据根独立重跑，exit 0、overall PASS**——7/7 命令全 0（fmt / clippy / workspace 888 通过 0 失败 / check-contracts / check-boundaries(含 B1+N16/N17) / r04_tool_matrix 56 案例全 actual==expect / 内嵌完整 verify-stage R03），19/19 场景 PASS，124 补充叶 = 55 份额 PASS + 69 递延，`candidateSourceBinding.stable=true`、`testedShaAtEnd=0c67b6fc7…`（与本地 HEAD、远程三方一致）。
2. **内嵌 R03 Gate 同轮独立重跑 overall PASS**：15/15 命令 exit 0（十修复套件、A15/A16、七条 R02 定向链 r02_auth_matrix / storage_tx / events_matrix / backup_restore / recovery_drill / full_chain / legacy_regression 全 PASS），17/17 场景、48 叶=17 PASS+31 递延，binding stable 于同一 SHA。
3. **六组组合链由本审查者自写探针真实复跑，6/6 通过**（§3）；测试替身仅产生外部响应（StepsProvider 扮演 R05 模型、fixture 子进程与自写 python stdio MCP server 扮演外部系统），网关/策略/批准/注册表/journal/监督器全部为真实实现。
4. **R03 RR2 接受链核对成立**：`R03_HANDOFF.json.reaccepted_after_rr2_fixed_repair` → 候选 `f4d4b1666` + `R03_RR2_STAGE_REVIEW.md` STAGE_VERDICT: PASS + `RR2-F05-GATE/verify-stage-r03/verify-stage-result.json`（overall PASS、stable、testedShaAtEnd=f4d4b1666、17/17）；`f4d4b1666` 经 `git merge-base --is-ancestor` 验证为 `bf6450bcd` 祖先。
5. **生产默认入口未暗切**：`ServiceDeps::default()` 的 `tool_gateway/tool_executor/turn_provider/approval_gate` 全部 `None`（rust/crates/lingxi-service/src/lib.rs:574-600）；Rust 工具族仅显式 opt-in 组装；bf6450bcd..0c67b6fc7 中 `desktop/`、`core/`、`lib/`、`package.json` 零改动（非 rust 非 R04 文档/证据的改动仅 7 个文件，见 §4.1）。
6. **后续义务未删除或提前伪完成**：R04.json 69 递延叶逐一保留（全部含晚于 R04 的执行阶段、无门禁绑定）；PLATFORM_CAPABILITIES 的 Windows 真机 R09/R10、Linux 未真机登记如实；GOV-01/GOV-02、SUP-03 F3/F4 按登记携带。

无阻塞 finding。非阻塞观察与携带项见 §6。

## 1. 逐 Task / A-ID / 补充义务 / 平台状态

### 1.1 Task → 提交 → 审查映射（全部独立确认）

| Task | 提交 | 独立审查 | 结论 |
|---|---|---|---|
| R04-T01 目录与参数契约 | `fd5fc2c2c` | R1 **FAIL**（R04-T01-R1-F01：无 `items` 的数组长度约束被静默跳过——违反自身 fail-closed schema 契约）→ REPAIR_R1 → R2 **PASS** | 通过 |
| R04-T02 唯一执行网关 | `1a1602c33`+补 `39168bfb0`（origin 字段遗漏补提交） | R1 **FAIL**（R04-T02-R1-F01：网关接线下 read-only 子代理对 Read 类也整体拒绝，破坏显式 read 衰减）→ REPAIR_R1 → R2 **PASS** | 通过 |
| R04-T03 批准/撤销/只读 | `c72e0fa02` | R1 **PASS** | 通过 |
| R04-T04 原生文件工具 | `80b4edbf5` | R1 **PASS**（OBS-1 edit 计数空间差异→T08 修复关闭） | 通过 |
| R04-T05 命令/PTY/进程树 | `84e6499fe` | R1 **PASS**（OBS-2 RegistryFull 孤儿路径登记携带） | 通过 |
| R04-T06 跨平台沙盒 | `26251dc92` | R1 **PASS**（0 阻塞 + 2 LOW） | 通过 |
| R04-T07 MCP+worker | `ded3467bf` | R1 **PASS**（F1 测试隔离 flake→T08 修复关闭；O4 read_only 面→T08 补钉） | 通过 |
| R04-T08 结果/产物/全矩阵 | `0c67b6fc7` | R1 **PASS**（E01 中断事实+E02 七处修复经审查者核实） | 通过 |

两次 FAIL 均被真实修复并换新 Reviewer 复验（reviews/ 与 repairs/ 目录齐全）；未发现"自己修自己验"。

### 1.2 十六个 A-ID（本审查者的独立证据来源）

| A-ID | 本审查者的独立证据 | 结论 |
|---|---|---|
| A01 目录与执行同源 | 全新门禁 rust_test_workspace 内 r04_t01 套件 + r04_tool_matrix `future-tool-shape-*`/身份格；探针 chain1 用 registry resolve（名字→权威 target id）真路由 | PASS |
| A02 陈旧目录拒绝 | 门禁 `matrix-lifecycle-generation-refusals`/disable/uninstall holes=0；探针 chain2(d) 自验：prepare 后 update 代次 → `TargetChanged`、零派发（gen.txt 不存在） | PASS |
| A03 各入口权限不变 | 门禁 `matrix-permission-consistency`/`route-consistency`（35 格）；探针 chain1：同 target 直接两阶段 vs 完整 run 链 vs worker 路线，operate 执行/read_only 拒绝、副作用文件与 journal 一致 | PASS |
| A04 伪造凭证失败 | 探针 chain2(b)：args_digest 用 A 的摘要配 B 载荷 → `DigestMismatch`、零副作用（forged.txt 不存在）；T02 套件（100 轮双花竞争等）在门禁 workspace 内重跑 | PASS |
| A05 批准后改参不能执行 | 探针 chain2(c)：批准绑定精确调用（批准 file-A 后 file-B 停在 pending、拒绝后零执行；B 文件不存在）；批准面 `args_summary` 不泄漏路径值（实测断言） | PASS |
| A06 等待期禁用工具 | 门禁 a16 生命周期族（disable/uninstall/generation holes=0）+ T03 套件；A02 同根因链 | PASS |
| A07 并发编辑不覆盖 | 探针 chain4(a)：真实 read v1 → 用户改 v2 → 旧 oldText 编辑 → journal Failed、磁盘仍为用户 v2、无工具写入泄漏 | PASS |
| A08 符号链接不逃逸 | 探针 chain4(b)：ws 内 symlink → 受限哨兵，write/read 经网关均按真实目标拒（哨兵字节不变）；合法对照通过 | PASS |
| A09 孙进程清理哨兵存活 | 探针 chain5：真实 3 层树（bash 子+孙进程写 PID 文件）→ `cancel_run` → `kill(pid,0)` 级 reap 确认子/孙均消失、无关哨兵 sleep 存活、运行进入 cancelled 终态、同进程再执行 exec_command 成功 | PASS |
| A10 PTY 不退化 | 探针 chain5：真实 pty + bash——`echo` 往返、`stty size` 默认 24 80 → `pty_resize(100,40)` 后 40 100、外来会话 write_stdin 被 `WRITE_STDIN_NOT_OWNED` 拒、exit 后句柄退场 | PASS |
| A11 缺沙盒不裸跑 | 门禁 workspace 内 r04_t06 套件（helper 缺失/换版/不支持后端响亮拒绝）；T06 真机探针同套重跑 | PASS |
| A12 隔离实际成立 | 本审查者全新 workspace 输出亲见 `r04_a12_filesystem_write_isolation_holds_with_real_sentinels` / `r04_a12_network_isolation_against_the_registered_loopback_service` / `r04_a12_environment_whitelist_holds_through_the_sandbox_wrapper` / `r04_a12_sandboxed_process_tree_still_honors_the_cancellation_chain` 全 ok（macos-arm64 真机） | PASS |
| A13 MCP 重连不重复 | 探针 chain6(b)：**自写** python stdio MCP server（非复用仓内 fixture）——count_up 副作用落盘后进程退出丢回执 → `Unknown`（含 never blindly retried 语义）；`refresh_mcp_server` 真重连（connects≥2）计数仍 1；`get_count` 核验 count=1 | PASS |
| A14 worker 不绕权 | 探针 chain1(d)+chain6(a)：worker 经同一网关同一策略面（read_only 拒/operate 行）；hang worker 期限 → journal `Unknown`+dispatched=true 恰一条；T07 套件（凭证/越界/白名单）在门禁 workspace 重跑 | PASS |
| A15 空产物不算成功 | 探针 chain4(c)(d)：自写 Ghost 双（success+幽灵 file:// 引用）→ `gateway_artifact_verification_failed` + 三段事实消息（WAS dispatched / side effects may exist / NOT registered as a valid deliverable）；真实交付对照通过；门禁 a15 八案例 | PASS |
| A16 禁用覆盖所有路线 | 门禁 a16 六路线 holes=0 + history-preserved + uninstall/generation 腿 | PASS |

### 1.3 补充义务

| 义务 | 状态 | 本审查者证据 |
|---|---|---|
| SUP-01（R03 r04_obligation_t06_o1，ask 档子代理审批差距） | **CLOSED** | 探针 chain3 四腿全真实运行：(1) 真实子 run（delegation 链）ask 父+省略 access → 子 run journal `Failed+!dispatched+TOOL_APPROVAL_UNAVAILABLE+deny_on_prompt`、文件零落地；(2) read_only 父+显式 write → 零逃逸；(3) operate 对照 → 子写真实执行；(4) 网关策略面直验（ask 档子代理写=结构化拒、read=Allowed、用户 read_only 写=ACTION_BLOCKED_BY_READ_ONLY） |
| SUP-02（唯一授权面/单一 journal 写者） | HELD | 码级亲读：`toolgateway.rs` 网关零授权逻辑零存储端口，驱动在 journal authorized 步应用 RunGrant（runs.rs:1259 起）；全矩阵经同一链 |
| SUP-03（RR-T02 F1..F4） | F1/F2 CLOSED（T01/T02），F3/F4 携带 R08 | check-contracts 在我的门禁轮零漂移；canonical-json.mjs 注释如实记载 UTF-16 键序裁定 |
| SUP-04（R03 laterShare 目录面） | PINNED | 11 个未迁移形态 `Availability::Future` 可发现零派发（门禁 future-tool-shape-* 11 案例 + 探针未见伪装 available） |
| SUP-05（R03 回归硬保护） | PINNED | r03_regression_gate 在我的全新门禁内完整重跑（见 §2.1） |

### 1.4 平台状态（PLATFORM_CAPABILITIES 冻结矩阵 vs 实测一致）

| 平台 | 声明 | 本审查者核对 |
|---|---|---|
| macos-arm64 | 真机全部验证（seatbelt 沙盒 10/10 探针、真进程/PTY、A09/A10） | 一致——r04_t06_sandbox 与 r04_a12_* 探针在我的全新 workspace 输出中逐一亲见 ok |
| macos-x64 | 同源码、无真机，真机验证归 R09/R10，不冒称已验证 | 一致（递延登记在册） |
| windows-x64 | 后端=Unsupported fail-closed（`EXEC_SANDBOX_REFUSED`/sandbox_policy_unsupported，绝不裸跑）；既有递延 R03-WINDOWS-R09-R10 保留 | 一致（不把拒绝形态冒充隔离能力） |
| linux-x64 | bwrap 源码级适配，未真机，capabilities 明示 NOT machine-verified | 一致（如实未验证，不冒称通过） |

三层分离（源码/交叉构建、真机基础隔离、最终安装包）在矩阵中明确；安装/签名/公证归 R10/R11。

## 2. 本审查者实际执行的命令与退出码

环境：macOS darwin 27.0.0 arm64；`/Users/study_superior/.cargo/bin/cargo`（rustup 1.98.1，rust-toolchain.toml 生效）；全部 `--locked`/`--offline`；仓库根 `/Users/study_superior/Desktop/Code/LingxiAgent`；候选期间零工作树改动。

### 2.1 完整 R04 Gate 独立重跑（全新证据根）

```bash
mkdir -p /private/tmp/r04-stage-review/verify-R04
cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- \
  verify-stage R04 --evidence /private/tmp/r04-stage-review/verify-R04
# → exit 0（overall PASS）
```

- 首次尝试用 `/tmp/r04-stage-review/verify-R04`（穿 /tmp→/private/tmp 符号链接）被候选绑定**响亮拒绝**（exit 1：`cannot define candidate binding: --evidence path crosses symlink /tmp; refusing…`）——门禁 fail-closed 自身的一次真实正例，记录在案。
- 结果（`verify-stage-result.json`）：overall **PASS**；7/7 命令 exit 0——`rust_test_workspace`（**82 套件 ok、888 通过、0 失败**，从 stdout 逐行复算）、`rust_clippy`（-D warnings）、`rust_fmt`、`check_contracts`、`check_boundaries`（O1-O8+D1-D5+B1）、`r04_tool_matrix`（56 案例全 actual==expect，`allCasesOk=true`）、`r03_regression_gate`；19/19 场景 PASS；124 叶=55 PASS+69 DEFERRED_TO_LATER_STAGE；`candidateSourceBinding.stable=true`、`testedShaAtEnd=0c67b6fc749ac5c21d0514e3076b0ed2058cd7c4`。
- 内嵌 R03 Gate（同轮、同候选）：overall **PASS**、15/15 命令 exit 0（rust_test_workspace / fmt / clippy / contracts / boundaries / repair_suites（十套件）/ a15_combo_and_leaves / a16_seed_mechanism / **r02_auth_matrix / r02_storage_tx / r02_events_matrix / r02_backup_restore / r02_recovery_drill / r02_full_chain / r02_legacy_regression**）、17/17 场景、48 叶=17 PASS+31 递延、binding stable 于 `0c67b6fc7`。R02 高风险链（认证矩阵、事务、事件快照/续传、备份恢复、恢复演练、全链、旧库回归）全部在我的重跑中通过。

### 2.2 六组组合链探针（自写 crate，真实执行）

```bash
# 探针 crate：/private/tmp/r04-stage-review/probe（path 依赖指向冻结候选的
# lingxi-kernel / lingxi-protocol / lingxi-service；serde/tokio/serde_json
# 锁内版本钉死；--offline）
cd /private/tmp/r04-stage-review/probe
CARGO_TARGET_DIR=/private/tmp/r04-stage-review/probe-target cargo build --offline --tests   # exit 0
CARGO_TARGET_DIR=/private/tmp/r04-stage-review/probe-target cargo test --offline --test chains
# → exit 0；6 passed; 0 failed（chain1..chain6 全绿）
```

逐链真实内容（探针代码在 `/private/tmp/r04-stage-review/probe/tests/chains.rs`，约 1500 行自写）：

1. **chain1 路线一致性**：`write` 与 worker 工具同 target 经 直接两阶段 / 完整 run 链（真实 admission→journal→授权→网关）→ read_only 两路线均拒（`ACTION_BLOCKED_BY_READ_ONLY`、journal `!dispatched`、文件零落地）、operate 两路线均真执行（磁盘字节=完整参数、journal dispatched）；worker 结果（真实子进程输出）经运行器入账。
2. **chain2 完整参数/批准/撤销**：Unicode+CRLF+500 词长尾全文写读一致（非摘要执行）；伪造摘要（A 摘要配 B 载荷）→ `DigestMismatch` 零副作用；ask 档批准绑定精确调用（批准 A → A 落盘且恰一次；B 停留 pending，拒绝后 B 永不执行）；批准面摘要不泄漏路径值；prepare 后代次更新 → `TargetChanged` 零派发；prepared 句柄单次消费（重放 `PreparedHandleConsumed`、文件恰写一次）。
3. **chain3 SUP-01/模式对照（真实子 run）**：ask 父+省略 access 的子代理写 = journal `TOOL_APPROVAL_UNAVAILABLE`（含 deny_on_prompt、!dispatched、文件零落地）；read 类放行；read_only 父零逃逸；operate 对照子写真执行；网关策略面直验同判定。
4. **chain4 文件**：真实读改读+用户并发改 → 陈旧编辑拒（`ReceiptOutcome::Failed`）且用户 v2 完整保留；symlink 逃逸（write/read 均按真实目标拒、哨兵字节不变）+ 合法对照；幽灵产物（自写外部双 success+不存在文件引用）→ `gateway_artifact_verification_failed` 三段事实消息；真实交付对照通过同一审计。
5. **chain5 进程/PTY**：真实三代树（PID 文件可证）→ `cancel_run` Fired → 子/孙均 `kill(pid,0)` 级消失、无关哨兵存活、运行达 cancelled 终态、**同进程** supervisor 立即可复用；真实 PTY：echo 往返、resize（24 80→40 100 实测生效）、外来会话 `WRITE_STDIN_NOT_OWNED`、exit 后句柄退场。
6. **chain6 Unknown/不盲重发/R03 复测**：hang worker（真实子进程）期限 → journal `Unknown`+dispatched=true 恰一条不重试；**自写** stdio MCP server 副作用后丢回执 → `Unknown`（never blindly retried）→ `refresh_mcp_server` 真重连（connects≥2）计数仍 1 → `get_count` 核验 count=1；R03 受理去重复测——填充空白 id（`"  req-…-6x  "`）受理执行后：同 id 异内容 → `DuplicateRequestConflict`；同 id 同内容（原样与 canonical 两种拼写）→ 同一 run 的幂等 Replay、run 数不增、外部副作用文件仍为第一次内容（零重放）。

探针过程中的三次"失败"实为被审对象正确行为，作为正例记录：(a) 同 id 异内容被显式冲突拒；(b) 同 id 同内容是幂等 Replay 而非盲重跑；(c) 外来会话写终端是完成态 Failed（`WRITE_STDIN_NOT_OWNED`）而非网关拒绝——均为现役契约的正确形态，探针按契约修正后全绿。

### 2.3 其余核对命令（摘要）

```bash
git status / git log --oneline bf6450bcd..0c67b6fc7      # 干净；9 提交（8 Task + T02 补）
git diff --stat bf6450bcd..0c67b6fc7 -- rust/             # 58 文件 +37,542/−563
git diff --name-only bf6450bcd..0c67b6fc7 | grep -v …     # 非 rust 非 R04 文档/证据仅 7 文件
git diff bf6450bcd..0c67b6fc7 -- rust/crates/lingxi-service/tests/  # 断言增删盘点：−1/+1260（唯一删除为 digest→outcome 等价适配）
git diff bf6450bcd..0c67b6fc7 -- rust/Cargo.lock          # 仅 +2 依赖边（serde_json→kernel、rmcp→service），零新版本
git merge-base --is-ancestor f4d4b166… bf6450bcd          # true（RR2 候选为 R04 起点祖先）
git rev-parse origin/codex/rust-tauri-migration           # = 0c67b6fc7（远程包含）
git worktree list                                         # 仅主工作树，无残留
python3（结构核验 R04.json 124 叶=55+69、递延叶零门禁绑定且全部晚于 R04；
        RR2 门禁 JSON overall PASS/stable/17-17；scope 矩阵/账本/交接结构）
```

## 3. 候选绑定与差异范围

- **candidateSourceBinding**：本审查者全新门禁轮 `stable=true`、`testedShaAtEnd=0c67b6fc7…`；逐命令 checkpoint 摘要一致；与 T08 轮证据（artifacts/rust-tauri/R04/T08-E01/verify-R04）同一候选。
- **差异范围**（bf6450bcd..0c67b6fc7）：rust 生产/测试 58 文件 + xtask R04 图（4443 行，生成器 `r04_t08_generate_stage_map.py` 幂等）+ 3 个 r04 脚本 + R04 文档/账本/证据。非 rust 非 R04 文档/证据的改动仅 7 文件：`ORCHESTRATOR_PROGRESS.json`（导航）、`R01/PROTOCOL_SPEC.md` §9（UTF-16 键序裁定）、`R01/r01_t01_check_ownership.py`（**新增** B1 受保护执行器边界 + N16/N17 负向，亲读为收紧而非放宽）、`tests/migration/r01-t02/canonical-json.mjs`（注释对齐 F1 裁定）。
- **R03 既有测试改动**：全部为 4.1 契约演进的 API 适配（`ToolOutcome::Success{content_digest}`→`success_text`、`ToolRequest` 结构体→`from_effective_arguments`）；唯一删除的断言行（`assert_eq!(digest, recorded)`→`assert_eq!(outcome_text, recorded)`）语义保留；新增断言 1260 行。未见删测/弱化/过滤 0。
- **Cargo.lock**：仅两条依赖边新增，无任何新包版本进入锁（rmcp 3.4.1/tokio process feature 均为锁内复用，与 T07 报告主张一致）。

## 4. 专项核对（阶段派单"另必做"逐项）

| 项 | 结论 | 证据要点 |
|---|---|---|
| 所有承诺可用 Rust 工具真接入同一网关 | 成立 | read/write/edit/exec_command/write_stdin（T04/T05 注册处方）+ MCP（`register_mcp_server` 动态注册）+ worker（`register_worker_tool`）+ subagent 委派族（同一网关 prepare，runs.rs:1662-1744 一致性注释与实现亲读）；探针六链全部经真实网关；B1 边界检查（`dispatch_executor`/`execute_prepared` 白名单+理由强制）在我的 check_boundaries 轮通过；无旁路回旧 Node/Pi（diff 零触碰旧栈） |
| R05 交接非摘要-only | 成立 | `ToolSuccess` 携真实 ContentBlock/resource_refs/截断/运行状态（ports.rs:1021-1038）；探针 chain2 从网关读回真实全文、chain1(d) worker 真实结果入账、chain5 PTY Running 句柄+新输出游标；`ToolRequest::from_effective_arguments` 全参数→可信 digest；HANDOFF 的 registration_recipe 与 harness 参考真实可编译（探针即按其组装） |
| 生产默认未暗切 | 成立 | §0.5；`bootstrap_with_instance_identity` 仅消费 deps 注入；组装面（`register_*`）生产默认不调用（workerrpc.rs:1130 注释"production default does NOT call"亲核） |
| 递延不丢失、R05+ 不倒灌 | 成立 | 69 递延叶原样（每叶含 R00 原文断言与晚于 R04 的阶段）；explicit_non_goals 七条在 SCOPE_MATRIX；R04.json 份额叶全部带 assertionContract；未发现 R05+ 功能倒灌（无供应商协议/凭证/上下文迁移代码入 diff） |
| 此前修复保留（R03 回归清单） | 成立 | 内嵌 R03 门禁全绿：取消树/同进程清理（cancellation_tree、cancel_terminal_race 13/13 域）、统一终态裁决（late_result_fence）、Unknown 不盲重试（tool_receipt_unknown+探针 chain6）、受理去重/补偿（admission_dedup_* 5/5+5/5 + RR2 canonicalization 7/7，均在 888 内）、requestId 规范化（探针 chain6 复测三种形态）、完整输入保真（input_payload_fidelity）、后台 steering（background_steering）、父子权限衰减（subagent_permission_inheritance + 探针 chain3） |
| R02 高风险回归 | 成立 | 七条定向链在我的 R03 门禁轮全 PASS（§2.1） |
| 平台必需安全验证有真证据 | 成立 | §1.4；macos-arm64 真机探针在我的 workspace 轮亲见 ok；缺失项按登记递延而非编译/截图替代 |
| 治理递延 | 携带 | GOV-01/GOV-02 原分类沿用，未套旧类别消新红 |

## 5. Finding

**无阻塞 finding。** 非阻塞观察（不构成放行障碍，供后续阶段处理）：

| # | 观察 | 评估 |
|---|---|---|
| O-1 | T05-R1-OBS-2：`PreparedRegistryFull` 拒绝路径可能留下已 spawn 未监督子进程（R04-T05 审查 R1 登记，T08 按冻结面纪律不修） | 已登记于 R04_HANDOFF.deferred_registrations[t05-obs2-registryfull-tail]，归属 R05+/阶段审查裁定；本审查者确认登记在册且网关 cap 逻辑（清理过期项后再拒）如实现测正常。属罕见路径（cap 压力+spawn 后注册失败窗口），建议 R05 消费面接线时补一个永久回归 |
| O-2 | edit 重复计数的 NFKC 折叠差异（T04 §8.6） | 已登记 R06；fuzzy 空间计数对齐已在 T08 关闭（OBS-1） |
| O-3 | `r00_management_leaves` LAN 自连腿受 macOS 应用防火墙间歇影响（R04-ENV-FIREWALL-R00-MGMT） | 环境项非产品缺陷；本轮（本审查者全新门禁）环境窗口良好全绿，处置口径在 T07 报告 §5.1 |
| O-4 | xtask fmt/clippy 的 evidencePaths 指向 stdout.log（实际输出多在 stderr.log） | R03 RR2 审查已登记的通用小瑕，非本轮引入；命令真实执行且退出码与日志自洽 |
| O-5 | verify-stage 证据根穿 /tmp 符号链接被拒（本审查者首跑亲历） | 门禁 fail-closed 的正确行为；使用 `/private/tmp` 解析路径即可，非缺陷 |

诚实性核对：门禁声明数字与我从 stdout 逐行复算一致（82 套件/888/0）；矩阵 56 案例 actual==expect；RR2/T08 两份历史证据与我的重跑一致；报告与账本无虚报形状；E01 中断与三轮门禁尝试历史如实保留（attempt1 FAIL=门禁抓到 E01 expect 误钉、attempt2 声明≠执行被审计发现——执行者自曝且根因修复，是好信号非瑕疵）。

## 6. 后续递延与当前范围限制（如实）

**递延（登记在册、不阻塞 R04）**：69 补充叶（R06/R07/R08/R09 验收，REQUIRED 保留）；R04-SUP-03 F3/F4（R08 事件面消费点）；T05-OBS-2（R05+）；NFKC 差异（R06）；Windows 真机+正式签名/安装（R09/R10，含 R03-WINDOWS-R09-R10）；macos-x64 真机（R09/R10）；Linux 沙盒真机/容器验证（后续具备环境后按同套探针补验）；GOV-01/GOV-02（治理分类沿用）；R04-ENV-FIREWALL（环境观察项）。

**本审查的范围限制**：
- 全部动态验证在本机 macOS darwin 27.0.0 arm64；未验证其他平台真机（与平台矩阵口径一致，不作为缺陷）。
- 真实模型供应商/凭证未接入（R05 范围；Provider 双仅产生外部响应——符合 05 契约第 1 层边界）。
- 探针 chain6 的 R03 去重为单进程腿（Replay/冲突/绑定拒绝三形态）；跨重启绑定腿由内嵌 R03 门禁的 `request_id_canonicalization` 套件（7/7，我的重跑内）覆盖。
- 未执行安装包/签名/公证（R10/R11）；未触碰真实用户数据；无真实外发。

## 7. 裁决

- 逐 Task 独立 PASS 链完整（含两轮 FAIL→修复→复验的真实历史）；16 A-ID 与 5 项补充义务在本审查者独立重跑与自写探针下全部成立；组合层未发现任何后续 Task 破坏前序 schema/身份/授权/收据/取消边界的回归。
- 完整 R04 Gate（含内嵌完整 R03 Gate 与七条 R02 定向链）在全新证据根独立重跑 exit 0/overall PASS，候选绑定 stable 于 `0c67b6fc7`，远程包含已核。
- 生产默认未暗切；后续义务未删除、未提前伪完成；平台能力声明与实测一致、缺失如实递延。
- **R04 在候选 `0c67b6fc7` 上接受（PASS）。** 按 §14 由总控收口：更新 ORCHESTRATOR_PROGRESS/R04_REPORT 状态、HANDOFF 接受记录（引用本报告与 `/private/tmp/r04-stage-review/verify-R04/` 门禁证据），不伪造坐标、不新增自引用提交。本次止于 R04，不启动 R05。
