# R02-T08｜独立服务交付与门禁 — 执行报告

- 执行者：ZCode:EXECUTOR-R02-T08（一次性执行代理；不负责独立验收，不提交/推送；
  PASS/FAIL 判定归总控另派的独立验收）
- 状态：**READY_FOR_REVIEW**
- 日期：2026-09-26/27
- 任务书：`Lingxi_Rust_Tauri_Taskbooks_2026-09-23/R02_Rust独立服务、存储与事件基础.md`
  §4 R02-T08（场景 R02-A15「真实二进制完成全链」、R02-A16「不影响旧入口」，均 REQUIRED）；
  共同必读 01–06 已完整读取（05 §3 xtask 接口契约为本任务的核心规格）
- tested SHA：`5741989165fe7e04c9a58a9d35c7747d3599d274`（= TASK_BASE_SHA = 远端 HEAD）
  + 本任务未提交改动（§9 全集；工作树 digest `f6058f26…`，见 §9）
- 平台：macOS 27.0 arm64（Darwin 27.0.0）；rustup CLI 1.29.1 + 锁定 rustc/cargo
  1.98.1（`rust-toolchain.toml`，开工核对）
- 隔离：全部 cargo 命令剥代理 + `CARGO_NET_OFFLINE=true` + `--locked` +
  专属 `CARGO_TARGET_DIR=/tmp/rust-target-r02-t08`；全部测试用 mktemp 合成
  /tmp home；无真实数据、无真实供应商

## 1. 范围

任务书五步：①健康/版本/存储/认证/事件测试串成真实二进制集成测试；②按 05 §3
实现 xtask check-contracts/check-boundaries/verify-stage 及 --help（空集合/
未知阶段/缺证据非零退出，禁止仅统计手填 PASS）；③fmt + clippy/test --locked +
schema/边界真实命令 + 构建日志归档；④检查生产默认入口未替换 + 最小服务启动
说明与关闭失败诊断手册；⑤把 R00 的 stores/entrypoints/acceptance map 更新为
实际实现路径交接 R03。

授权的移交修正（本次一并收口，逐项可对质）：
T07-R1 F01/F02/F03/F04、T02-R1 F02、T04-R1 F01/F02、T06-R2 F05（惯例采纳）、
RR-T08-F1（R01 门禁脚本硬化，RISK_REGISTER 明确归属 R02）、RR-T02-FINFO1
（PROTOCOL_SPEC §10 补说明）。

不在本任务内：Node/Electron 生产入口改动（零改动，A16 本意）、任务书、
.sync-audit/、PROGRESS.md、ORCHESTRATOR_PROGRESS.json、T01–T07 已提交
artifacts（`git status --porcelain artifacts/rust-tauri/R02/` 滤除 T08 后为空）。

## 2. 观察事实（先读再动手）

1. **R00 三份 map 是封存交付**：STORES/ENTRYPOINTS/ACCEPTANCE_MAP 的哈希钉在
   R00_HANDOFF.json（R00 阶段修复 R1 复核 0 失配），R00 之后无人原地改写。
   原地改写会破坏封印链——这是设计决定 §4.1 的输入。
2. **RR-T08-F1 机理**（RISK_REGISTER 原文 + 复现证实）：三个 R01 门禁脚本默认
   共享 `CARGO_TARGET_DIR=/tmp/lingxi-r01t02-target`；lingxi-protocol-gen 用
   编译期 `env!(CARGO_MANIFEST_DIR)` 解析仓库根；cargo 会跨检出复用内容指纹
   相同的构建产物。本次在 /tmp 克隆树上**实测复现**（§6.3）。
3. **xtask 在 DEPENDENCY_RULES.json 注册为 `status=planned, establish_stage=R02`**；
   D5 反向闭包要求 workspace 成员必须是注册模块——建 crate 必须同步
   planned→exists（T01/T04 同款 deliberate update）。
4. **serde_json 在锁内**（多 crate 已用）；clap 不在。xtask 三个固定子命令
   不需要 CLI 框架——手写参数解析，零新增第三方包（Cargo.lock 仅 +7 行成员块）。
5. **T07 报告的 A14 数字与提交证据漂移**（REVIEW_R1 F03）：报告 3372/174 样本/
   11008/16080 vs 提交证据 `storm-results.json` 3384、`rss-samples.csv` 147 样本、
   first 11136 / peak 17040；`slow-subscriber/summary.txt` 两次运行输出堆叠无
   轮次标注；脚本 note 硬编码 "400 executes"（实际 EXECUTES=1000）。
6. **FINFO1**：contentSha 哈希输入是提取器运行期内部全量清单（含完整 mounts 与
   preloadIpcChannels），无法仅由矩阵重算——PROTOCOL_SPEC §10 无此说明。
7. **审计封印测试族开工即红**（seal 坐标落后于已授权 R01/R02 提交，R01 同款
   预存在红）：本次全量 `npm test` 中 `post-verification-audit-seal` /
   `round2-delivery-evidence` / `round3-delivery-evidence` 三族失败，其失败
   清单**全部为已提交的 R01/R02 交付文件，0 个 T08 文件**（反掩码检查）。
   如实记录，未使其变绿，未动坐标/白名单。

## 3. 设计决定

### 3.1 R00 map 的"更新为实际实现路径"＝冻结原件 + 覆盖层（ADR 记录）

原地改写 R00 三份 map 会破坏 R00_HANDOFF 的哈希封印链（观察事实 1），且
R00 原件描述的是旧栈现役事实（A16 证明其仍然成立）。故采用**覆盖层**：
`docs/rust-tauri/R02/R02_IMPLEMENTATION_MAP.json`——钉住三份原件的 SHA-256，
叠加登记 R01/R02 新栈已实现并真实接线的 stores/entrypoints/acceptance 映射
（runs.db、instance 身份、epoch 印章、local-token、服务日志、
`lingxi-service`/`lingxi-storage-inspect` 两个新入口、xtask 阶段图），未列出
者仍归旧栈。R03 以"R00 原件 + 覆盖层"并读为输入。这满足任务书"更新为实际
实现路径，交接 R03"的语义而不损封印链。

### 3.2 xtask 映射与执行模型（05 §3 契约的落法）

- 阶段图 = `rust/crates/xtask/src/stage_maps/<STAGE>.json`（编译期
  `include_str!` 内嵌，内容随检出走，无路径绑定）。schema：`schemaVersion`
  /`resultVersion`/`stage`/`defaultTimeoutSecs`/`commands{}`/`scenarios[]`。
- **命令必须是真实命令**：R02 图把 16 场景绑到 14 个真实证据脚本（T01–T07
  的 12 个既有二进制级脚本 + 本次新增 A15/A16 脚本）；多条场景可共享同一
  命令（A05/A06、A07/A08、A09/A10——同一证据链，去重执行）。
- **判定只来自真实退出码**：执行器 spawn 登记命令、stdin 关闭、stdout/stderr
  逐命令落盘；scenario PASS ⟺ 其引用命令 exit 0 且未超时且**声明的证据
  文件存在**。不读任何 JSON 里的 PASS 字符串。
- **硬失败面**：未知阶段（exit 2，列出已注册阶段）；图非法/场景空集/
  命令空集/场景无命令/证据声明缺失（exit 2）；命令失败/超时/缺证据
  （exit 1，结果 JSON 留档真实退出码）。结果 JSON 含
  resultVersion/stage/testedSha(git rev-parse)/worktreeDirty/platform/
  toolchainChannel/逐命令 exitCode+durationMs+missingEvidence/逐场景
  status/overall。
- **RR-T08-F1**：xtask 运行期从 CWD 解析仓库根并拒绝与编译期根不一致
  （`bound_repo_root()`，fail-closed 不回退）。
- 超时：逐命令 `timeoutSecs`（默认 1200s），超时 kill + FAIL。
- 零新增依赖：serde_json 复用锁内版本；参数解析手写。

### 3.3 RR-T08-F1 硬化方案（双保险，RISK_REGISTER 两个建议方向都采纳）

1. **gen/verify 运行期根绑定**：`lingxi-protocol` 新增 `devgate::bound_repo_root()`
   ——运行期从 CWD 向上找 `rust/Cargo.toml` 定位检出根，与编译期根
   canonicalize 比较，不一致即 exit 2 拒绝（不静默回退）。两个门禁二进制
   （protocol-gen / protocol-verify）的默认输出/读取路径改用绑定根。
2. **三脚本默认 target 按检出路径派生**：
   `/tmp/lingxi-r01t02-target-$(printf '%s' "$PWD" | shasum -a 256 | cut -c1-16)`
   ——两个检出永不共享产物；显式传入的 `CARGO_TARGET_DIR` 仍然尊重（此时由
   第 1 条运行期绑定兜底）。
   负向证据（§6.3）：修复前在带漂移的克隆树上跑门禁→绿（错绑）；修复后同
   场景→拒；跨检出直接运行绑定二进制→exit 2 拒绝。

### 3.4 移交修正的取舍

- **T07-F04 选 (i) 解析期执法**（任务指示"范围违例行为与其余 7 旗标统一"）：
  `--log-max-bytes` 下限 64、`--log-max-files` 下限 2 在 parse_cli 强制
  （`parse_limit_value_min`），违例=InvalidLimit（exit 2 路径）；constraint
  文本与 USAGE 同步改为真实下限并明示"解析期启动错误"；attach 期 validate
  保留为纵深防御。
- **T06-R2 F05（证据原始性惯例）**：采纳"门禁/修复证据统一存档原始命令输出"——
  本任务所有门禁证据均为原始日志（含命令行与退出码），并把该惯例写进
  R02_HANDOFF 交接约定。
- **T07-F01**：报告叙述修正（加固非缺陷修复），代码不动——审阅方明确
  "代码无需变更"。

## 4. 修改了什么

新增：
- `rust/crates/xtask/`：`Cargo.toml`、`src/main.rs`（CLI + 根绑定 + 两个
  gate 子命令）、`src/stage_map.rs`（图解析/校验 + 8 个负向单测）、
  `src/verify.rs` + `src/verify/runner_tests.rs`（真实执行器 + 6 个
  正/负向单测，含超时与缺证据）、`src/stage_maps/R02.json`（16 场景→
  14 真实命令）。
- `rust/crates/lingxi-protocol/src/devgate.rs`：RR-T08-F1 运行期根绑定
  （含成功路径单测；错绑拒绝需跨检出二进制，由二进制级负向证据覆盖）。
- `scripts/rust-tauri/r02_t08_full_chain_smoke.sh` +
  `r02_t08_full_chain_probe.py`：A15 全链冒烟（构建→启动→认证负/正→写→
  WS 订阅+活事件→HTTP 读回→优雅关闭→重启→旧 token 401/新 token 200/
  数据读回→再关闭→遗留进程/端口检查）。
- `scripts/rust-tauri/r02_t08_legacy_entry_regression.sh`：A16 旧入口回归
  （E1 入口未变三证据 / E2 typecheck / E3 core-contracts / E4 两边界门禁 /
  E5 全量 npm test + 封印族分类 + 反掩码检查）。
- `docs/rust-tauri/R02/SERVICE_START_AND_SHUTDOWN.md`：最小服务启动说明 +
  关闭失败诊断手册（退出码表、5 类关闭失败处置）。
- `docs/rust-tauri/R02/R02_IMPLEMENTATION_MAP.json`：R00 map 覆盖层（§3.1）。
- `docs/rust-tauri/R02/r02_t08_worktree_digest.py`：工作树 digest 计算器
  （R01 方案，R02 口径；-z 防 git quotepath 转义）。
- `docs/rust-tauri/R02/R02-T08_REPORT.md`（本文件）、`R02_REPORT.md`、
  `R02_HANDOFF.json`、`R02_ACCEPTANCE_LEDGER.json`、
  `artifacts/rust-tauri/R02/T08/`（证据）。

修改（最小闭合）：
- `rust/crates/lingxi-protocol/src/bin/lingxi-protocol-gen.rs`：main 开头
  经 devgate 绑定根（默认输出目录改用绑定根）。
- `rust/crates/lingxi-protocol/src/bin/lingxi-protocol-verify.rs`：同上
  （默认 golden 目录改用绑定根）。
- `rust/crates/lingxi-protocol/src/lib.rs`：`pub mod devgate;`（一行）。
- `scripts/rust-tauri/r01-t02-check-generated.sh` / `r01-t02-roundtrip.sh` /
  `r01-t02-handshake.sh`：默认 target 目录按检出路径派生（RR-T08-F1）。
- `rust/crates/lingxi-service/src/config.rs`：F04——两日志旗标解析期下限
  强制 + constraint 文本修正 + 新单测 `log_flag_range_violations_are_parse_time_errors`。
- `rust/crates/lingxi-service/src/main.rs`：F04 USAGE 下限/语义明示；
  T04-F02——模块 docstring 补 exit 5、日志字符串长空格残留清理。
- `rust/crates/lingxi-service/src/redaction.rs`：T07-F02——模块文档补记
  分歧精确边界/现役未镜像规则清单/实测一致项（纯文档）。
- `rust/crates/lingxi-service/src/paths.rs`：T02-F02——"normalize to 0700"
  （可放宽更严位）替代"只收紧"失配表述（纯文档）。
- `rust/crates/lingxi-adapters/tests/storage_transactions.rs`：T04-F01——
  文件头故障注入描述改为实测机制 RLIMIT_FSIZE，uchg 定性更正（纯文档）。
- `scripts/rust-tauri/r02_t07_slow_subscriber.sh`：T07-F03——summary 逐轮
  标注、note 去硬编码数量（数值权威=storm-results.json）。
- `docs/rust-tauri/R02/R02-T07_REPORT.md`：T07-F01（§2.6/§3/§9.6/§11.3
  Mailbox 定性改为前向稳健加固）、F02（§4.2/§9.2 分歧精确边界补记）、
  F03（A14-1/3/4 数字对齐提交证据；§7.3 9→10 脚本勘误）。
- `docs/rust-tauri/R01/PROTOCOL_SPEC.md`：FINFO1——§10 补 contentSha 复算
  限制说明。
- `docs/rust-tauri/R01/DEPENDENCY_RULES.json`：xtask planned→exists
  （deliberate update，+responsibility/established_by；diff 共 3 行改动，
  无整文件重排）。
- `rust/Cargo.toml`：成员 + xtask（含 scope 注释更新）；`rust/Cargo.lock`：
  +7 行（xtask 成员块 + serde_json 依赖引用，零新包、零版本变化）。

## 5. 关键调用链

```
xtask verify-stage R02 --evidence <dir>
  └─ bound_repo_root()            # RR-T08-F1：CWD 检出根 == 编译期根，否则 exit 2
  └─ parse_stage_map(include_str!("stage_maps/R02.json"))
  └─ git rev-parse HEAD / status --porcelain   # 真实 SHA + dirty 位
  └─ 逐命令（首引用序去重）：
       Command::new(argv).current_dir(repo_root)
       ├─ stdout/stderr → <dir>/<command_key>/{stdout,stderr}.log
       ├─ 100ms 轮询 wait/try_wait；超时 kill → timedOut=true
       └─ 证据检查：evidencePaths（{EVIDENCE}/{REPO_ROOT} 替换）must exist
  └─ scenario PASS = 全部引用命令 (exit 0 ∧ ¬timeout ∧ evidence 完整)
  └─ verify-stage-result.json（真实退出码/平台/SHA/时长/overall）+ 退出码
A15 全链：bash 编排（rustup 锁定构建→spawn 二进制→READY 行解析）
  └─ python 探针（真 HTTP + 真 RFC6455 客户端）
       boot: health→401 负→local-token /me→execute→WS subscribe→live 事件→
             HTTP 读回→future_cursor 显式拒绝
       close: SIGTERM→exit 0→pgrep 空→curl 拒
       restart: 新 token 200（旧 401）→session/events 读回→health→close→检查
A16 旧入口：git diff(base)=空 → package.json main → launch 路径 0 rust 引用
  → npm run typecheck → typecheck:core-contracts → 2×boundary gates
  → npm test（失败族⊆预存在封印族 ∧ 失败清单 0 T08 文件）
F1 门禁：check-generated.sh → 派生 target → protocol-gen --check（运行期根绑定）
```

## 6. 验收场景逐项结果（REQUIRED）

全部真实执行；tested SHA `574198916…` + 未提交改动；macOS 27.0 arm64。
总执行器 = `xtask verify-stage R02`（结果 JSON：`verify-stage/verify-stage-result.json`，
overall **PASS**，16/16）。

### 6.1 R02-A15 真实二进制完成全链 — PASS

| # | 覆盖 | 证据 |
|---|---|---|
| A15-1 | 干净隔离目录（mktemp /tmp home）；构建离线锁定 | `verify-stage/A15/full-chain/build.log` |
| A15-2 | 启动→READY→health 200→无凭证 401→local-token /me 200 | `full-chain/summary.txt`（boot1 段） |
| A15-3 | 写数据：execute 两次提交（runId 回显） | `summary.txt` execute-write-* |
| A15-4 | 订阅：subscribed.snapshotSeq 边界 + 订阅中写入的活事件 seq 越界送达 + HTTP 读回一致 + 伪造 future cursor 显式拒绝 | `summary.txt`、`probe1.pass-lines` |
| A15-5 | 关闭：SIGTERM→exit 0→无遗留子进程（pgrep）→端口关闭（curl 拒） | `summary.txt` close1 段 |
| A15-6 | 重启读取：同 home 重启→旧 token 401/新 token 200→session runCount=2→events head 不回退→health 200 | `summary.txt` boot2 段、`pre-restart-head.json` |
| A15-7 | 二次关闭 + instance 记录清理 + 终态遗留检查 | `summary.txt` close2 段 |

### 6.2 R02-A16 不影响旧入口 — PASS

| # | 覆盖 | 退出码 | 证据 |
|---|---|---|---|
| A16-1 | git diff(base..工作树) 对 core/ server/ desktop/ shared/ tests/ package.json package-lock.json = 空 | 0 | `verify-stage/A16/legacy-entry/e1-diff-node-surface.txt`（空） |
| A16-2 | package.json main=desktop/bootstrap.cjs；launch 路径 0 rust 引用 | 0 | `e1-launch-rust-refs.txt`（空）、summary |
| A16-3 | npm run typecheck / typecheck:core-contracts | 0 | `e2-typecheck.log`、`e3-core-contracts.log` |
| A16-4 | check:dependency-boundaries / check:tool-invocation-boundaries | 0 | `e4-*.log` |
| A16-5 | 全量 npm test：失败族 ⊆ 预存在封印族（3 文件/6 用例）；反掩码=失败清单 0 个 T08/new-stack 文件；1463 test files passed / 3 failed | 1（分类后收敛） | `e5-npm-test.log`、`e5-failed-test-files.txt`、`e5-seal-family.txt`、`summary.txt` |

封印族红定性：`VERIFIED_SOURCE_SHA`（审计坐标）落后于已授权 R01/R02 提交，
失败清单全部是**已提交**的 R01/R02 交付文件——R01-T08 与 R02-T07 验收时
已观察同类红；与本任务无关，未触碰坐标/白名单，未使其变绿。

### 6.3 xtask 负向 + RR-T08-F1 负向逐项（全部真实执行）

| # | 负向 | 结果 | 证据 |
|---|---|---|---|
| N1 | `verify-stage R09`（未知阶段） | exit 2 + 诊断（列出已注册阶段） | `gates/xtask-unknown-stage-exit2.txt` |
| N2 | 未知子命令 / 缺子命令 | exit 2 | `gates/xtask-unknown-subcommand-exit2.txt` |
| N3 | 空场景集合 | 解析拒绝（"must not be EMPTY"） | `cargo test -p xtask`（empty_scenario_set…） |
| N4 | 场景无命令 / 引用未知命令 / 证据声明缺失 / 重复 id / 坏 schemaVersion / 占位值 / 坏 JSON | 解析拒绝（8 单测） | `cargo test -p xtask`（16/16） |
| N5 | 命令失败真实退出码传播（exit 3/7） | FAIL + 结果 JSON 记录真实码 | runner_tests（fail / rollup） |
| N6 | 退出 0 但缺证据 | FAIL（missingEvidence 非空） | runner_tests（missingevid）+ **run1 实战**：阶段图声明错→A11/A12/A16 FAIL，`gates/verify-stage-run1-missing-evidence-FAIL.json` |
| N7 | 超时 | kill + timedOut=true + FAIL | runner_tests（timeout） |
| N8 | `--help` | exit 0 全文 | `gates/xtask-help.txt` |
| F1-1 | 修复前：带漂移克隆树跑旧门禁（默认共享 target） | **exit 0 绿**——实际校验主仓树（日志打印主仓路径） | `f1-hardening/prefix-run-in-drift-clone.log` |
| F1-2 | 修复前：显式传入被干净克隆污染的共享 target | **exit 0 绿**——校验污染源克隆树（跨检出产物复用证实） | `f1-hardening/prefix-run-in-drift-clone-explicit-target.log` |
| F1-3 | 正确绑定下同漂移树 | exit 1 检出漂移（证明漂移真实、F1-1/F1-2 是掩蔽） | `f1-hardening/correctly-bound-detects-drift.log` |
| F1-4 | 修复后：同一漂移克隆树 + 派生 target | exit 1 拒绝（drift detected） | `f1-hardening/postfix-run-in-drift-clone-default-derived-target.log` |
| F1-5 | 修复后：显式 poisoned target | exit 1 拒绝 | `f1-hardening/postfix-run-in-drift-clone-explicit-poisoned-target.log` |
| F1-6 | 修复后：绑定二进制跨检出运行 | exit 2 + repo-root mismatch 诊断 | `f1-hardening/postfix-binding-mismatch-refusal.log` |
| F1-7 | 硬化后三脚本在本仓复跑 | check-generated exit 0（`gates/xtask-check-contracts.log` 内含）；roundtrip/handshake 见 §7 | 本节 + §7 |

### 6.4 门禁与回归（全部亲手执行，原始输出归档）

| 门禁 | 命令 | 退出码 | 证据 |
|---|---|---|---|
| fmt | `cargo fmt --all -- --check`（锁定 1.98.1） | 0 | `gates/cargo-fmt-check.log` |
| clippy | `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 | `gates/cargo-clippy.log` |
| 全量测试 | `cargo test --workspace --locked --offline --no-fail-fast` | 0（**287 passed / 0 failed，39 测试目标**；T07 基线 269 + devgate 1 + config 1 + xtask 16） | `gates/cargo-test-workspace.log` |
| xtask 三命令 | check-contracts / check-boundaries / verify-stage R02 | 0 / 0 / 0 | `gates/xtask-*.log`、`verify-stage/verify-stage-result.json` |
| T01–T07 回归 | 12 个既有二进制级脚本经 verify-stage 全量复跑（证据定向 `verify-stage/A0x/`，T01–T07 已提交证据零 diff） | 全 0 | `verify-stage/` 各命令目录 summary |
| 冻结契约 | r01-t02-check-generated.sh（xtask 内） | 0（56 文件 624 条目零漂移） | `gates/xtask-check-contracts.log` |
| 所有权/依赖 | r01_t01_check_ownership.py（xtask 内；D5 反向闭包含 xtask） | 0（RESULT: OK + 负向电池） | `gates/xtask-check-boundaries.log` |
| Cargo.lock | `git diff --numstat rust/Cargo.lock` | — | **+7（成员块），零新增包/版本** |
| Node 红线 | `git diff base -- core/ server/ desktop/ shared/ tests/ package.json` | — | 空 |
| 审计封印族 | npm test 内含 | 1（预存在红，见 6.2） | `verify-stage/A16/legacy-entry/e5-npm-test.log` |

## 7. 交接修正逐项对质表

| 移交项 | 修复内容 | 验证 |
|---|---|---|
| T07-R1 F01 | R02-T07_REPORT §2.6/§3/§9.6/§11.3 叙述改为"前向稳健加固（tokio 1.53.1 计数补捕）"，删除"缺陷级修复/已复现"措辞；代码零改动 | 复跑 events 用例 + events-matrix（经 verify-stage A09/A10 PASS） |
| T07-R1 F02 | redaction.rs 模块文档 + 报告 §4.2/§9.2：精确漏网边界（`/`/`=` 分段与相邻）、现役未镜像规则清单（PII/CLI 旗标/aws-configure/Windows 路径）、实测一致项对拍结论；R05 交接风险已入 R02_HANDOFF unresolved_items | `cargo test -p lingxi-service redaction`（全量套件内 287 绿）；纯文档改动 |
| T07-R1 F03 | 报告 A14-3 3384/3384、A14-4 147 样本/11136/17040 对齐提交证据；§7.2 表头 EXECUTES=1000+SEQUENTIAL=1200；§7.3 "9 脚本/0×10"矛盾修正为 10；脚本 note 逐轮标注 + 去硬编码 | 脚本复跑（A14 shakedown + verify-stage 内 PASS） |
| T07-R1 F04 | 解析期强制下限（bytes≥64、files≥2）exit 2，与其余 7 旗标统一；constraint 文本 "(>= 65536)"→"(>= 64)"；USAGE 明示 | 新单测 + 全量测试绿；USAGE/`--help` 一致 |
| T02-R1 F02 | paths.rs 文档"只收紧"→"normalize to 0700（更严位加回 owner-write）"（模块头 + 函数 doc） | 纯文档；layout 测试保持绿 |
| T04-R1 F01 | storage_transactions.rs 头注释：uchg 定性更正为实测机制 RLIMIT_FSIZE（EFBIG），指向 disk_full_fault.rs | 纯文档；disk_full_fault 用例保持绿 |
| T04-R1 F02 | main.rs docstring 补 exit 5；日志字符串长空格残留清理（grep 复核零残留） | 纯文档/字符串；`--help` 与退出码表一致 |
| T06-R2 F05 | 采纳"证据=原始命令输出"惯例：本任务全部门禁证据为原始日志；惯例写入 R02_HANDOFF 交接约定 | 本报告 §6 全部证据可复核 |
| RR-T08-F1 | gen/verify 运行期根绑定 + 三脚本派生 target（§3.3） | §6.3 F1-1..F1-7 |
| RR-T02-FINFO1 | PROTOCOL_SPEC §10 补 contentSha 复算限制说明（提取器内部全量清单；复算必须重跑提取器） | 纯文档；check-generated 门禁保持绿 |

## 8. 未验证内容 / 环境限制

1. 跨平台：全部证据仅 macOS arm64（同 T01–T07 平台边界）。R09 按平台矩阵登记
   Tauri 侧命令；本任务 xtask 的 verify-stage 机制本身平台中立（std::process）。
2. R03 及以后阶段的 verify-stage 图尚未存在——`verify-stage R03` 现在是 exit 2
   （这是契约行为：R03 建图后自动可用）；本任务只注册 R02。
3. A16 的旧入口验证为**源码级 + 测试回归**级：未启动真实 Electron 窗口
   （需要 GUI 会话，超出本机隔离约束）；npm test 1463 个测试文件包含旧栈
   行为回归，typecheck 覆盖构建面。真实打包/安装验证按任务书归 R09/R10。
4. xtask 超时 kill 用 child.kill()（SIGKILL）：被杀脚本是 bash 时其 EXIT trap
   会先运行并清理子进程；"杀整进程组"需 libc/pre_exec（未引入，依赖纪律）。
   本任务脚本均有 trap 清理，风险登记为低。
5. 工作树 digest（`f6058f26…`）是**报告时点**值；本报告与 HANDOFF 自身
   不入 digest（自引用规避），最终候选以授权提交后重算为准。

## 9. git status --short 全文（报告时点）

见 R02_REPORT.md §10（同题小节，同一次快照）。

## 10. 推荐独立验收重点

1. **重跑 xtask 全链**：`cargo run -p xtask -- verify-stage R02 --evidence /tmp/任意`
   ——核对结果 JSON 的真实退出码/SHA/平台与逐命令日志；对任一场景抽一条
   命令从入口重跑。
2. **F1 对抗复核**：按 §6.3 F1-1..F1-6 在自建 /tmp 克隆上重现"修复前错绑绿、
   修复后拒绝"；检查 devgate 绑定不可被 CWD 伪造绕过。
3. **A15 动手**：`bash scripts/rust-tauri/r02_t08_full_chain_smoke.sh /tmp/任意`
   ——重点核对重启后旧 token 401、事件 head 不回退、pgrep/lsof 遗留检查。
4. **A16 反掩码**：自行 grep `e5-npm-test.log` 确认封印族失败清单 0 个 T08
   文件；在 base SHA 上复核三族红为预存在。
5. **F04 行为**：`lingxi-service --log-max-bytes 63 …` 应 exit 2（不再"接受后
   降级"）；`--log-max-bytes 64` 应正常启动。
6. **阶段图映射完备性**：对照 acceptance-catalog R02-A01..A16 与
   stage_maps/R02.json 的 commandRefs，确认没有场景绑到空命令或占位。

— 执行者声明：以上命令、退出码、证据路径均来自本机真实执行；未执行项已
如实列入 §8；未 commit/push。
READY_FOR_REVIEW。
