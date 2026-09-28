# R02 最终收口 · 修复组 1（R1）证据报告

子代理：`R02-REPAIR-GROUP-1-R1`（xtask 阶段图与 verify-stage 判定逻辑的阶段所有权恢复）
仓库：`/Users/study_superior/Desktop/Code/LingxiAgent`｜分支 `codex/rust-tauri-migration`｜基线 HEAD `cdd213078f6947217000c7ecd1a36ab5ffe2bb01`（开工时工作区干净）
工具链：rustup 锁定 1.98.1（`PATH="$HOME/.cargo/bin:$PATH"`）；全部 cargo 命令 `--offline --locked`；`CARGO_TARGET_DIR=/tmp/rust-target-r02-final`（预热）。仓库内未新增 artifacts 文件；未 commit、未 push。

## 1. F1 复现（修复前）

命令（2026-09-28 ~14:39Z）：
```
cargo run --manifest-path rust/Cargo.toml -p xtask --quiet --locked --offline -- verify-stage R02 --evidence /tmp/r02-g1-repro/
```
输出（原文）：
```
error: invalid stage map for R02: stage map invalid: supplemental leaf "R00-T02-LA-1B09760C2B1C" references unknown command "supplemental_cli_rust_matrix"
EXIT=2
```
与审计 F1 完全一致：阶段图在 `stage_map.rs` 加载期硬错，verify-stage R02 一次也跑不起来。

## 2. 修复后验证（F1 closed）

命令（2026-09-28T14:55Z 起，evidence 用 `$HOME/r02-g1-fixed`——`/tmp` 是符号链接会被 candidate binding 拒绝）：
```
cargo run --manifest-path rust/Cargo.toml -p xtask --quiet --locked --offline -- verify-stage R02 --evidence "$HOME/r02-g1-fixed/"
```
日志 `/tmp/r02-final/fixed-run.log`（节选，2026-09-28T14:56Z 摘取）：
```
xtask: verify-stage R02 [bash scripts/rust-tauri/r02_t01_service_smoke.sh {EVIDENCE}/A01] > .../r02-g1-fixed/a01_smoke
xtask: verify-stage R02 [a01_smoke] PASS
xtask: verify-stage R02 [bash scripts/rust-tauri/r02_t01_boundary_negative.sh {EVIDENCE}/A02] > .../a02_boundary_negative
xtask: verify-stage R02 [a02_boundary_negative] PASS
xtask: verify-stage R02 [bash scripts/rust-tauri/r02_t02_dual_instance.sh {EVIDENCE}/A03] > .../a03_dual_instance
xtask: verify-stage R02 [a03_dual_instance] PASS
xtask: verify-stage R02 [bash scripts/rust-tauri/r02_t02_path_priority.sh {EVIDENCE}/A04] > .../a04_path_priority
```
- 不再 exit 2：解析通过 → 与真实 R00 两账本（`docs/rust-tauri/R00/ACCEPTANCE_MAP.json` + `FEATURE_STAGE_ACCEPTANCE.json`）的 34 叶 id 集合 + 逐叶 r00* 镜像全字段核对通过 → 叶证据 freshness 通过 → 进入命令执行（a01/a02/a03/a04 依次 PASS 后按计划中止，未跑完 20 命令，符合任务书"跑到第一个命令执行即可"）。
- 这同时是对**真实 R00 账本**的镜像全等核对的实跑证据（r00* 字段未被改动）。
- 中止后检查无残留进程（`ps` 无 r02_*/lingxi-service/xtask），evidence 目录已清理。
- JSON 语法：`python3 -c "import json;json.load(open('rust/crates/xtask/src/stage_maps/R02.json'))"` → OK（exit 0，多轮）。

## 3. 修改文件清单（本代理只改 xtask，4 个文件）

| 文件 | diff 规模 | 内容 |
|---|---|---|
| `rust/crates/xtask/src/stage_maps/R02.json` | +151/−340 | F1：注册 `supplemental_cli_rust_matrix`（argv `["python3","scripts/rust-tauri/r02_cli_rust_matrix.py","{EVIDENCE}/CLI_RUST"]`、timeoutSecs 1800、evidencePaths `["{EVIDENCE}/CLI_RUST/cli-rust-cases.json"]`——与脚本 `Matrix.save()` 实际产物核对一致）；commands 19→20。34 叶重分类（25 `r02_share_satisfied` + 9 `deferred_to_r07`，逐叶与审计 JSON `correctR02Acceptance` 一致）；sessions 叶 `R00-T02-LA-200D4E5D52C9` 门禁 cases 10→6（服务侧），4 个 CLI 渲染案例移入 `deferredR07Cases`（同生产者/同证据文件，仅观察）；9 个递延叶移除门禁 assertionContract/originalAssertionCases/evidencePaths、`evidenceCommandRefs: []`，help/sharing/static×4 带 `earlyEvidenceCommandRefs`（client/static 矩阵照常运行）+ `deferredR07Cases` 记录；thinking×3 的 r02Share 文本按 F4 修正（路由已在 management.rs 存在，取值语义依赖 R05/R06 模型权威）；`supplementalCoverageNote`/`supplementalSemanticLimit` 重写为准确计数与新语义（25 PASS 份额 + 9 DEFERRED_TO_R07、R07 须消费余款、本轮授权撤销 R17 门禁含义）。**r00\* 镜像字段逐值全等**（补丁脚本内置断言 + 实跑 R00 交叉核对双重证明）。 |
| `rust/crates/xtask/src/stage_map.rs` | +527/−（净增） | 新 basisKind `r02_share_satisfied`、`deferred_to_r07`（含 `SUPPLEMENTAL_BASIS_KINDS` 注册）；新结构 `LeafDeferredCase`；`SupplementalLeaf` 新增 `deferred_r07_cases`/`early_evidence_command_refs`；解析校验：递延叶不得有门禁 assertionContract/门禁 evidenceCommandRefs/pass-able evidencePaths、必须含后续阶段（单 R02/RX 叶标 deferred 硬错）、`earlyEvidenceCommandRefs` 仅递延叶可声明且须已注册、`deferredR07Cases` 仅份额/递延叶可声明且案例名唯一、不得与门禁图钉双归属、生产者必须已注册且属于本叶（门禁或 early）命令——F1 类缺陷维持加载期硬错并加测。 |
| `rust/crates/xtask/src/verify.rs` | +426/− | `roll_up_supplemental_leaf` 返回 `LeafRollUp` 结构；`deferred_to_r07` 固定输出 `DEFERRED_TO_R07`（附 r07Share 义务文本 + earlyEvidence 观察，命令未通过只记录不改叶状态）；`r02_share_satisfied` 份额图钉全过即 PASS（附 R07 余款 reason）；**删除/绕过原 675-692「另属后续阶段→BLOCKED」分支对 full_original_behavior 与 r02_share_satisfied 的拦截**（F2）；该保守分支仅保留给旧图 legacy kind（protocol_basis/route_basis_present_static 双阶段）回放防护，auth_primitive_only 的保守 BLOCKED 亦保留（防旧图回放，不出现在现行图）；`observe_deferred_case` 非门禁观察（OBSERVED_HELD/NOT_OBSERVED）；执行顺序纳入 earlyEvidenceCommandRefs；coverage：期望 34 = pass + deferred + fail + blocked、deferred 集合与声明全等（不符即 internal error）、`deferredLeafIds`/`shareSatisfiedLeafIds`/`commandsNotPassing` 输出；**overall = 16 场景全过 ∧ PASS∪DEFERRED 全覆盖 34 叶 ∧ 全部已执行命令通过**（提前实现矩阵失败=命令 FAIL 阻断 overall，不静默变绿）；每叶输出 `deferredToStage=R07`、`deferredR07Cases`、`earlyEvidence`（V7：供 R07 阶段图机器消费）。R14 镜像全等核对、逐案例 actual==expect、evidence freshness、候选绑定机制未动。 |
| `rust/crates/xtask/src/verify/runner_tests.rs` | +514/− | 既有测试适配（roll-up 结构返回、嵌入式图断言重写：25+9 计数、递延叶无门禁绑定、sessions 拆分图钉名单、serve 叶 9 图钉、`supplemental_cli_rust_matrix` 已注册）；新增 6 个语义测试（见 §4）。 |

未改：生产服务代码（lingxi-service/adapters）、`scripts/rust-tauri/*` 生产者脚本、docs/rust-tauri 报告/账本/交接 JSON、`main.rs`/`candidate.rs`。工作区中 `scripts/rust-tauri/r02_t08_legacy_entry_regression.sh` 的改动属于并行代理（A16 组），本代理未触碰。

## 4. 测试与门禁（exit code + UTC 时间）

最终扫描（2026-09-28T15:03:52Z–15:04:29Z，`/tmp/r02-final/final-sweep.txt`、`test-out.txt`、`clippy-out.txt`）：

| 命令 | 结果 | exit |
|---|---|---|
| `python3 -c "import json;json.load(open('rust/crates/xtask/src/stage_maps/R02.json'))"` | R02.json syntax OK | 0 |
| `cargo test --manifest-path rust/Cargo.toml -p xtask --offline --locked` | **87 passed; 0 failed**（原 66 + 新 21） | 0 |
| `cargo fmt --manifest-path rust/Cargo.toml -p xtask -- --check` | clean | 0 |
| `cargo clippy --manifest-path rust/Cargo.toml -p xtask --all-targets --locked --offline -- -D warnings` | Finished，零告警 | 0 |

新增 21 个测试：
- stage_map.rs map_tests（15）：share 叶解析正例/缺契约负例；递延叶带 earlyEvidence+deferredR07Cases 正例；递延叶带 assertionContract/门禁 refs/evidencePaths/单阶段标 deferred 负例；deferredR07Cases 在 legacy kind 声明/生产者未注册/生产者不属于本叶/案例重名/与门禁图钉双归属 负例；earlyEvidenceCommandRefs 非 testdeferred 叶声明/引用未知命令 负例；**F1 回归：assertionContract 生产者未注册 → 加载期硬错（两种触发路径）**。
- verify/runner_tests.rs（6）：**F2 回归**——双阶段 r02_share_satisfied 份额图钉全过 → PASS（含 R07 余款 reason 与递延案例观察）；份额缺案例/案例不成立 → FAIL；legacy protocol 双阶段叶回放仍 BLOCKED；递延叶固定 DEFERRED_TO_R07（earlyEvidence 过/不过两分支）；PASS∪DEFERRED 覆盖 + 全命令通过 → overall PASS（coverage 计数/集合全等/earlyEvidence 命令真实运行）；earlyEvidence 命令失败 → 叶仍 DEFERRED、命令 FAIL、commandsNotPassing 记录、overall FAIL。

## 5. 34 叶新分类计数（终态，与审计 JSON targetClassification 一致）

- **25 叶 `r02_share_satisfied`（R02 份额 PASS 形态）**：管理面 17（093F22C4FF63/747E0ADC941B/4BCE8CCFD5DC/25AB678E7108/F1754C6F755B/F006E094F028/73CDC44696D3/5E4A9CF58BED/229B77A7BA69/B8F8CF9E8AF5/7498024422D7/43A126149416/756217C74101/5E1C3363A070/066B54983F6A/C4E6F27D7873/4C1A9735CF04）+ 协议原语 6（D3710D637C19/3B86332B042C/EEC1A2A5CD04/D1BEE19A95BB/5816DA563ED8/D2657E4AB5FF）+ serve 1（1B09760C2B1C，9 个全服务端图钉）+ sessions 1（200D4E5D52C9，6 门禁 + 4 递延）。
- **9 叶 `deferred_to_r07`（仍 REQUIRED、验收归 R07）**：B8A1AD32A8E1（help）、32FFEC05BAA7（sharing）、8BC1A036AFAA/3291CFD5F7E2（static×2）、2A1C298F62FC（mobile bootstrap）、8ED658F9DB9E/F8935B6B0221/39AD35E1FD71（thinking×3）、000E6E1301C0（UI access）。
- 现行图 legacy kind 计数为 0（protocol_basis/route_basis_present_static/auth_primitive_only/client_only_stage_boundary_conflict/full_original_behavior 全部不再出现；其保守判定逻辑保留在 verify.rs 供旧图回放）。
- 16 基础 REQUIRED 场景不动；命令 19+1=20 全部注册（执行顺序=场景引用 + 叶门禁引用 + 递延叶 earlyEvidence 引用）。

## 6. 与审计建议的偏差与决策记录

- **V1 的"删除 675-692 分支"**：采取"绕过"实现——对 full_original_behavior 与 r02_share_satisfied 不再拦截（满足 F2 修复与授权语义）；对 legacy 局部 kind（protocol_basis/route_basis_present_static）保留双阶段 BLOCKED 作旧图回放防护（与 V6 对 auth_primitive_only 的"保留处理防旧图回放"同策略）。现行 R02 图无任何 legacy kind，实际门禁语义与"删除"等价。
- **overall 增加全命令通过条件**：任务书要求"client/static 矩阵保留注册照常运行、命令失败仍算命令 FAIL——提前实现的已提交代码必须保持健康"。实现为 overall PASS 需 `outcomes 全部 passed`（此前该性质由场景/叶引用隐含；显式化后覆盖只被 earlyEvidenceCommandRefs 引用的命令），失败记入 `supplementalLeafCoverage.commandsNotPassing`，绝不静默变绿。
- 审计 JSON 的递延叶 `correctR02Acceptance.evidenceCommandRefs` 为空：采纳——thinking×3/mobile/UI access 5 叶完全解除命令绑定（a05/a06 通用案例继续由 A05/A06 基础场景自身消费）；help/sharing/static×2 通过 `earlyEvidenceCommandRefs` 保留矩阵运行。
- serve 叶 assertionContract 内错误嵌套的 `originalAssertionCases`（原图缺陷，位于契约对象内而非叶级）随重分类一并移除。
- 无图钉案例名与生产者产物不符的情况（cli-rust 9 例、sessions 6+4 例、client 7 例、static 9 例均与脚本逐一核对存在）。

## 7. 遗留（非本组范围）

- docs/rust-tauri 下 LEDGER/handoff 的计数对齐与最终绑定由总控在 Gate 后统一刷新（本组未动）。
- `scripts/rust-tauri/r02_t08_legacy_entry_regression.sh`（A16）由另一代理负责。
- R02 完整 Gate 的合法全量运行（20 命令跑完）由总控统一执行；本轮只证明 F1 修复后图可加载、可进入执行、单测全绿。
