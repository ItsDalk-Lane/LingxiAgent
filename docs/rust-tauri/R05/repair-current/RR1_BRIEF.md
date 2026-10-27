# R05 RR1 修复基线盘点简报（RR1_BRIEF）

- 生成：基线盘点智能体（全新上下文），2026-10-04。本文只陈述本会话核实过的事实；每条均给出出处。
- 用途：为 R05 RR1 八个修复工作包（总控提示词 `RR1_MASTER_PROMPT_2026-10-04.md` 第四节 F01—F28）提供可信基线。配套文件：`RR1_ISSUE_MATRIX.json`（问题矩阵）、`RR1_PROGRESS.md`（断点进度表）。
- 权威输入：用户 2026-10-04 修复总控提示词（本目录 `RR1_MASTER_PROMPT_2026-10-04.md`，与证据包 `prompt/Lingxi_R05_完整修复与放行验收总控提示词_2026-10-04.txt` 同源）；2026-10-02 专项提示词与 2026-09-23 原始任务书见证据包 `specifications/`（含 `original_taskbooks/` 八份原文件，已确认在包内）。

## 1. Git 基线（本会话实测）

命令：`git rev-parse HEAD`、`git branch --show-current`、`git status --porcelain=v1`、`git log --oneline -3`、`git status -sb`。

- 分支：`codex/rust-tauri-migration`；与 `origin/codex/rust-tauri-migration` 同步（status -sb 无 ahead/behind）。remote `origin = https://github.com/ItsDalk-Lane/LingxiAgent.git`。
- HEAD：`d80737b6cb9186c8a18c0f35923aac00249d45c3`（`refactor(rust-tauri): R05 T01-T08 model gateway, protocol adapters and full closed loop`）。**与证据包冻结被审 HEAD 完全一致，HEAD 未前进。**
- 父提交：`c549ff654508ab951e2cf39cf9d309fc9c6b8656`（R04 RR1 重验收 docs 提交），与证据包 `audit_base` 一致。
- 工作树：无任何已跟踪文件修改；仅两个未跟踪目录：
  - `artifacts/rust-tauri/R05/RR1/`（对抗审查证据包，见 §3）；
  - `docs/rust-tauri/R05/repair-current/`（总控提示词 + 本基线三文件）。
- 最近提交链（`git log --oneline -3`）：`d80737b6c` → `c549ff654`（R04 re-accept）→ `629e15b85`（R04 RR1 G05 gate 注册）。R04 RR1 完整提交链见 §7。
- 台账事实（非阻塞，但不要误读）：`docs/rust-tauri/ORCHESTRATOR_PROGRESS.json` 的 `current_head` 仍为 `0c67b6fc7`（R04 重验收时期缩写），`stages.R05.status=PENDING`、`tasks["R05-T01".."R05-T08"].status` 全为 `PENDING`、`current_task` 停在 "R04 RR1 … RE-ACCEPTED"。即该总台账没有随 R05 实施（d80737b6）更新；RR1 期间判断 R05 状态以 repair-current/ 矩阵与阶段审查为准，收口时（F28）统一回填。

## 2. 工具链事实（本会话实测）

- 仓库工具链锁文件在**仓库根** `rust-toolchain.toml`（不在 `rust/` 下；`find . -name "rust-toolchain*"` 仅命中根目录一份）：`channel = "1.98.1"`，`components = ["rustfmt", "clippy"]`，文件头注明 Homebrew rust 不读该锁。
- 本机 PATH 中 `cargo`/`rustc` = `/opt/homebrew/bin/cargo`、`/opt/homebrew/bin/rustc`，版本 **1.93.0 (Homebrew)**（实测 `/opt/homebrew/bin/cargo --version`）。**禁止使用。**
- 必须使用的 rustup 代理绝对路径：`/Users/study_superior/.cargo/bin/cargo`（及同目录 `rustc`）。在仓库树内以它发起调用，实测解析为锁定的 **1.98.1**：
  - `rustc 1.98.1 (48a229cea 2026-09-01)`，host `aarch64-apple-darwin`；
  - `cargo 1.98.1 (797e8a9bc 2026-08-05)`。
- 证据包原审查环境为 Linux x86_64、同版本 1.98.1（`audit/runtime/environment.json`：rustc/cargo -Vv、`rust/Cargo.lock` SHA256 `259f983e…`、`rust-toolchain.toml` SHA256 `eec34104…`）。本机为 macOS arm64：审查基线 Rust 版本一致（1.98.1），平台差异按总控提示词 §5.3 处理（macOS arm64 上前序证据需在其有效环境复验；F20 macOS say 在本机恰为可实测平台）。
- 强制写法：所有 cargo/rustc 调用以参数列表形式经 `/Users/study_superior/.cargo/bin/cargo` 发起；跨目录（如 /tmp 隔离副本）不确定 cwd 时可加显式 `+1.98.1`。不升级、不降级、不改锁；新增依赖须固定版本并说明必要性。

## 3. 证据包结构与「审计目录 → F-ID」映射

根目录：`artifacts/rust-tauri/R05/RR1/INPUT-adversarial-2026-10-04/`。`MANIFEST.json`（format `lingxi-r05-audit-evidence-v1`）登记 259 个文件（不含 manifest 自身），逐文件 SHA256+字节数；`audit_head=d80737b6…`、`audit_base=c549ff654…`。

目录与文件数（find 实测）：

| 目录 | 文件数 | 内容 |
|---|---|---|
| `prompt/` | 1 | 完整修复总控提示词 txt（与 repair-current 版同源） |
| `specifications/` | 9 | 10-02 专项提示词 + `original_taskbooks/`（01–06 通用约束、R05、R06 任务书原件） |
| `audit/credential_network/` | 5 | report.md（CN-F01–F08）、src/lib.rs（10 个反例）、probes.log、Cargo.toml/lock |
| `audit/protocol/` | 5 | PROTOCOL_AUDIT.md（PF01–PF07）、src/lib.rs（13 测试：1 对照 + 12 反例）、probe-output.log、Cargo.toml/lock |
| `audit/worker_usage/` | 9 | R05_T06_T07_ADVERSARIAL_REVIEW.md（F-WU01–WU06、P1–P8）、tests/、operations_probes.log、usage_probes.log、probes.log（旧首批，不作完整索引）、evidence_manifest.json |
| `audit/closed_loop/` | 32 | R05_CLOSED_LOOP_AUDIT.md（CL-01–CL-06）、probe_binary/process_only/batch.py、binary-result.json、process-only-result.json、batch-result.json、三个 probe 目录（config/events/requests/stderr） |
| `audit/gate/` | 99 | R05_GATE_AUDIT.md（G01–G04 + 信息性/F28）、six_r05_only_leaves.json、cid_rename_probe.json、cid-mirror-{control,mutated,rust-mutated}.log、cid_rename_fixture/（历史日志 fixture + assembler 输出）、non_cid_registered_cases.json、fd_counter_probe.json、fd_counter_raw_lsof.txt、candidate-binding-comparison.json、audit_gate_semantics.py |
| `audit/runtime/` | 98 | runtime-audit-result.json（82 次运行、254 测试、253 通过、1 环境受阻 T08-C12；original_stage_runner_exit=1）、environment.json、r05-suites/（69 份逐套件日志+SHA）、service-pins-fresh/、fresh-service-lib-build.*、rerun-*、pid-namespace-probe.json |

「审计目录 → F-ID」映射（F-ID 定义以总控提示词第四节为准）：

| 审计发现 | 所属目录 | 对应 F-ID |
|---|---|---|
| CN-F01 401 重试新钥匙发旧端点 | credential_network | F02 |
| CN-F02 句柄代次 ABA 复用/复活 | credential_network | F03 |
| CN-F03 无逐模型能力声明载体 | credential_network | F01 |
| CN-F04 OAuth 六叶后端闭环缺失 | credential_network | F04 |
| CN-F05 代理/私有 CA 未实现 | credential_network | F14 |
| CN-F06 egress 域名解析绕过 | credential_network | F15 |
| CN-F07 错误 body 无总预算/大小约束 | credential_network | F16 |
| CN-F08 脱敏截断前缀/协议错误带完整 key | credential_network | F05 |
| PF01 Anthropic thinking 签名 | protocol | F06 |
| PF02 Google 签名丢失/并行分组 | protocol | F07 |
| PF03 OpenAI-compat reasoning 删除 | protocol | F08 |
| PF04 opaque 只绑协议族 | protocol | F09 |
| PF05 整批准入缺失 | protocol | F11 |
| PF06 过程内容判 final / 实时与历史不同源 | protocol | F12、F13 |
| PF07 Responses/Codex 顺序重排 | protocol | F10 |
| F-WU01 附件授权与读取对象不同 | worker_usage | F17 |
| F-WU02 usage 漏账/虚记/父子归属 | worker_usage | F21 |
| F-WU03 usage 宽松转换+invalid_detail 存凭证 | worker_usage | F22 |
| F-WU04 Gemini thoughts 包含关系错误 | worker_usage | F23 |
| F-WU05 媒体任务身份/取消 fence | worker_usage | F18、F19 |
| F-WU06 排队漏预算 + macOS say 绕监督 | worker_usage | F16（排队腿）、F20 |
| CL-01 能力声明（正式二进制复现） | closed_loop | F01 |
| CL-02 think/MOOD 历史 + 仅 reasoning final | closed_loop | F13（反例A）、F12（反例B） |
| CL-03 整批 schema/ID 预验证缺失 | closed_loop | F11 |
| CL-04/CL-05 worker 工具与 exec 未注册 | closed_loop | F24 |
| CL-06 资源验收不足 + FD 谓词失效 | closed_loop | F27 |
| G01 六 OAuth 独占叶误放行 | gate | F25 |
| G02 C-ID 换名可骗过门禁 | gate | F26 |
| G03 代理/CA 属本阶段义务 | gate | F14 |
| G04 资源门禁负载/采样缺陷 | gate | F27 |
| 信息性（report/handoff 与实际提交矛盾） | gate + runtime | F28 |
| 82 次运行台账（253/254 通过、exit 1 保留） | runtime | 背景基线（F27/F28 收口时引用） |

注意（README.txt 明示）：反例数量有跨域重复，不能相加当独立根因数。

## 4. 四条复现命令与 `../../LingxiAgent` 布局前提

命令原文（README.txt 第 36–39 行，逐字）：

```bash
cargo +1.98.1 test --manifest-path audit/protocol/Cargo.toml --locked -- --nocapture --test-threads=1
cargo +1.98.1 test --manifest-path audit/credential_network/Cargo.toml --locked -- --nocapture
cargo +1.98.1 test --manifest-path audit/worker_usage/Cargo.toml --locked --test operations_audit audit_ -- --nocapture
cargo +1.98.1 test --manifest-path audit/worker_usage/Cargo.toml --locked --test usage_audit audit_ -- --nocapture
```

相对路径前提（本会话 grep 三个 Cargo.toml 实证）：三个审计 crate 均以
`path = "../../LingxiAgent/rust/crates/{lingxi-adapters,lingxi-kernel,lingxi-protocol,lingxi-service}"`
引用仓库 crate（`audit/protocol/Cargo.toml:8-11`、`audit/credential_network/Cargo.toml:8-11`、`audit/worker_usage/Cargo.toml:7-10`）。从 `audit/<crate>/` 出发 `../../` 即解压根，因此隔离工作副本必须满足：

```
<隔离工作副本根>/            # 例如 /tmp/r05-rr1-work/   （任意仓库外目录）
├── LingxiAgent/            # 冻结 HEAD 的隔离 checkout：git worktree add … d80737b6 或 clone+checkout
└── audit/                  # 从 artifacts/rust-tauri/R05/RR1/INPUT-adversarial-2026-10-04/audit/ 整体复制
    ├── protocol/  credential_network/  worker_usage/  closed_loop/  gate/  runtime/
```

即「仓库以目录名 `LingxiAgent` 放在与 `audit/` 同级」（README.txt 第 34 行原文要求）。要点：

- 在工作副本根目录执行四条命令；本机把 `cargo` 替换为 rustup 代理绝对路径 `/Users/study_superior/.cargo/bin/cargo`（保留 `+1.98.1` 显式钉版，避免依赖 cwd 的工具链发现）。依赖已就绪后可加 `--offline`（README 第 34 行）。
- 冻结 HEAD（旧红）预期：protocol exit 101（13 测试 = 1 正常对照通过 + 12 正确契约断言失败）；credential_network exit 101（10 failed）；worker_usage 两条分别 3 failed / 5 failed。**这是缺陷证据，不是要求把断言改成当前错误行为**；修复后须按合法前置重跑转绿并迁入仓库正式 gate。
- 修复 F01 后其余旧 probe 会因缺少新能力声明在更早边界拒绝——必须补齐合法能力前置并证明仍命中原测边界（README 第 45 行「重要」段）。
- 三个 Python 正式二进制 probe（`closed_loop/probe_binary.py`、`probe_process_only.py`、`probe_batch.py`）运行前只在工作副本中把 ROOT 改为隔离证据输出目录、BIN 指向候选实际编译出的 `lingxi-service`；原绝对路径不代表本机（README 第 43 行）。
- 原审查用 `source …/audit/runtime/env.sh`；该文件不在证据包内（runtime/ 目录已核对无 env.sh），复跑直接用上面的显式命令即可。

## 5. 既有 R05 文档与 stage_map 入口索引

`docs/rust-tauri/R05/` 现行文件（ls 实测，收口时按 F28 统一更新，不新建空壳替代）：

- 结论/交接：`R05_REPORT.md`、`R05_HANDOFF.json`、`R05_INDEPENDENT_REVIEW.md`、`R05_BASELINE.json`、`R05_BLOCKERS.md`（第 18–23 行的代理/CA「后续网络加固」延期被 G03 判定为不合法延期，F14 修复后须撤）、`R05_LIVE_VERIFICATION.json`、`R05_PERFORMANCE_RESULTS.json`（第 41–47 行无原始采样值，F27 重做）。
- 矩阵/映射：`R05_SCOPE_MATRIX.json`（116KB）、`R05_ACCEPTANCE_LEDGER.json`（4MB，103 C-ID 条目）、`R05_TEST_MAP.json`（103 entries，含 3 个追加 C-ID）、`CREDENTIAL_FLOW_MATRIX.json`、`MODEL_CALLSITE_MATRIX.json`、`MODEL_USAGE_SEMANTICS.md`、`PROTOCOL_WIRE_MATRIX.json`、`PROVIDER_SUPPORT_MATRIX.json`、`WORKER_MODEL_BOUNDARY.md`、`R05_INTERFACE_EVOLUTION.md`（第 354–357 行 N-01 = F10；第 487–504/588–590 行 = F14 相关延期记录）。
- 台账/TSV：`PROGRESS_LEDGER.json`、`r05_stage_cids.tsv`（91 个直接 cid 注册；`R05-T05-C11B` 在第 111–115 行）、`r05_stage_pins.tsv`、`r05_leaf_case_map.tsv`。
- 生成器：`r05_t01_build_scope_matrix.py`（第 31–34、166–178 行 = F25 入口之一）、`r05_t01_build_callsite_matrix.py`、`r05_t01_codemod_next_turn.py`、`r05_t01_extract_provider_matrix.mjs`、`r05_t04_codemod_delta_sink.py`、`r05_t05_generate_compat_goldens.mjs`。
- `repair-current/`：总控提示词 + 本基线三文件。

stage_map 入口：`rust/crates/xtask/src/stage_maps/R05.json`（304KB）：`stage=R05`、`defaultTimeoutSecs=1200`、`commands={rust_fmt, rust_clippy, rust_test_workspace, check_contracts, check_boundaries, r05_stage_suites, r04_regression_gate}`、`scenarios` 18 条（原 16 A-ID 全 REQUIRED）、`supplementalLeafScenarios` 130 条。真实验证器：`rust/crates/xtask/src/verify.rs`、`stage_map.rs`（F25/F26 的修复对象）。阶段套件脚本：`scripts/rust-tauri/r05_t08_stage_suites.sh`、`r05_t08_generate_leaves.py`、`r05_t08_negative_gate.sh`（均存在）。

## 6. 各工作包文件所有权注意事项（并发编辑红线）

总控提示词 §3.2 要求共享文件单一所有者、不同智能体不得同时编辑同一文件。基于 F01—F28 入口清单整理（`rust/crates/` 前缀省略）：

| 工作包 | F-ID | 主要独占文件 | 与其他包共享的文件（冲突点） |
|---|---|---|---|
| WP-T01 | F01、F02 | `lingxi-adapters/src/models/config.rs`、`gateway.rs` | `provider.rs`（T02/T03）、`lingxi-service/src/management.rs`（T02 F02/F04）、`kernel/model_exchange.rs`（T03 F09） |
| WP-T02 | F03、F04、F05 | `lingxi-service/src/credentials/`（mod.rs、store/）、`models/oauth.rs`、`models/credentials.rs` | `management.rs`（T01）、`models/dispatch.rs`（F05 脱敏 vs T05 F14/F16）、`openai_completions.rs` 错误路径（F05 vs T03/T04） |
| WP-T03 | F06–F10 | `anthropic_messages.rs`、`google_generative_ai.rs`、`openai_responses.rs`、`compat.rs` | `openai_completions.rs`（T02 F05/T04 F12）、`kernel/model_exchange.rs`（T01） |
| WP-T04 | F11–F13 | `lingxi-service/src/runs.rs`（批次/终态段）、`lingxi-adapters/src/streaming_norm.rs`、`storage/run_store.rs`（持久化投影） | 四族 parser（T03 F11 腿）、`openai_completions.rs`、`service/lib.rs` 历史路径（T08） |
| WP-T05 | F14–F16 | `models/egress.rs`、`ProviderConfig`/网络策略面 | `dispatch.rs`（T02 F05）、`oauth.rs` 客户端（T02）、`operations.rs` 配额/期限段（T06/T07） |
| WP-T06 | F17–F20 | `resourceaccess.rs`、`models/operations/`（video 等） | `service/operations.rs`（T05 F16/T07 F21；F20 同文件） |
| WP-T07 | F21–F23 | `workermodel.rs`、`models/auxiliary.rs`、`kernel/usage.rs`、`storage/migrations.rs` | `workerrpc.rs`（T08 F24）、`operations.rs`（T06）、`models/usage.rs`/`rerank.rs`/`embedding.rs`（F22 与 T06 边界） |
| WP-T08 | F24–F28 | `service/lib.rs` 组合根、`xtask/{stage_map.rs,verify.rs}`、`scripts/rust-tauri/r05_t08_*`、`r05_t08_closed_loop.rs`、R05 文档收口 | `workerrpc.rs`（T07）、`lib.rs`（T04 历史路径） |

建议次序（总控提示词第 88 行）：先 T01/T02（身份、配置/凭证生命周期）+ T07 所需上下文契约；T03 与 T05 在文件所有权划清后并行；T04 用完整协议结果；T06 用修好的身份/预算/网络；T07 结算与查询；T08 接线、门禁与资源整体验收。F25/F26/F27 的门禁缺口应尽早暴露，不必等全部代码完成。跨包共享文件由总控仲裁单一所有者；必要时隔离 worktree，集成候选上重验。

## 7. R04 交接要点（RR1 模式沿用）

- 目录：`docs/rust-tauri/R04/repair-current/`——`R04_RR1_FIX_ISSUES.json`（5 个 issue：R04-RR1-F01–F05，字段含 workorder/locations/source_fact_confirmed_at/cases/status/normal_selfcheck/adversarial_selfcheck/independent_review + 顶层 repair_baseline/closure_checks/case_results/evidence_roots/commit_receipts/stage_review）、`G0x-E01_REPORT.md` ×5（执行者报告：红→绿记录、逐项自查、不 commit）、`G0x-R1_REVIEW.md` ×4+（独立组审）、`R04_RR1_STAGE_REVIEW.md`（全新阶段终审）。
- 流程模式：根因分组 G01–G05 → 每组执行者先固化旧红再修复（工作区不提交）→ 每组新独立审查者实测 PASS → 全组完成后派从未参与的全新 STAGE-REVIEWER，在 /tmp 独立 git worktree 做「旧树红/候选绿」对照 + 完整 `verify-stage R04` + 负向变异抽查（exit 1 证据）→ STAGE_VERDICT: PASS → 按 Task 精确暂存逐组提交推送。提交链（stage review 亲核）：G01=`1692d2314` → G02=`da15c4bd9` → G03=`614fab1af` → G04=`1285c3bf6` → G05=`629e15b85`，随后 `c549ff654` docs 重验收。
- R04 RR1 证据根：`artifacts/rust-tauri/R04/RR1-G0x-E01/…`、`RR1-STAGE-R1/`（logs/verify-R04/verify-R03/probe-source）。
- 对 R05 的直接约束：`verify-stage R05` 必须消费或实际运行有效 R04 结果（stage map `r04_regression_gate`），内嵌 R03/R02/RR1，子门禁失败外层不得 0；R04 的 RunSupervisor/ToolGateway/进程监督/取消恢复语义是 R05 复用底座（F24 正式接线复用 `register_process_tools`/exectools.rs:1316–1365 的 exec_command，不建第二套权威）。
- 纪律沿用：审查者不修改主树产品/测试/配置（结束时 `git status --porcelain` 仅余自身证据目录）；旧树/候选对照一律在 /tmp worktree；worktreeDirty 如实记录。

## 8. 证据包完整性抽查结果

本会话以 python hashlib 对 22 个文件（覆盖 README、prompt、specifications、六个 audit 目录的六份报告 + 关键源码/日志/数据文件：README.txt；prompt 与 specifications 两个入口文档；credential_network 的 report.md/src/lib.rs/probes.log；protocol 的 PROTOCOL_AUDIT.md/src/lib.rs/probe-output.log；worker_usage 的审查报告/operations_probes.log/usage_probes.log/evidence_manifest.json；closed_loop 的报告/binary-result/process-only-result/batch-result/probe_binary.py；gate 的报告/candidate-binding-comparison.json；runtime 的 runtime-audit-result.json/environment.json）计算 SHA256 与字节数，逐一与 `MANIFEST.json` 对照：**22/22 全部一致（sha256 与 size_bytes 双匹配），0 不符，0 缺失**。另核对文件总数：manifest 259 条 = 实际文件 259（不含 MANIFEST.json 自身）= 声明值 259。脚本结论行：`checked=22 ok=22 mismatch=0 not_in_manifest=0; manifest_entries=259 actual_files_excl_manifest=259 declared=259`。按任务边界只写三个基线文件，未另落抽查清单文件。

## 9. 与总控预期的偏差登记（如实，未自行修复）

1. `docs/rust-tauri/ORCHESTRATOR_PROGRESS.json` 滞后：`current_head=0c67b6fc7`（R04 时期）、R05 stage/tasks 全 PENDING，与实际 HEAD `d80737b6`（R05 T01–T08 已提交）不一致。判定为台账未随 R05 流程回填，不影响 RR1 盘点；收口（F28）时应统一，不在修复工作包内顺手改。
2. 工具链锁文件实际位于仓库根 `rust-toolchain.toml`，而非总控指令所写的 `rust/rust-toolchain.toml`；rustup 代理仍按该锁解析 1.98.1（实测），修复智能体按 §2 的绝对路径调用即可，不要因找不到 `rust/rust-toolchain.toml` 误判锁缺失。
3. 其余核对项无异常：HEAD 未前进、四条复现命令前提文件齐备、六份审计报告与 gate 证据文件（six_r05_only_leaves.json、cid_rename_probe.json、non_cid_registered_cases.json、fd_counter_probe.json 等）均在包内、stage map 与 r05 脚本在仓库内、证据包哈希抽查全数一致。
