# R03 对抗性修复派单｜G07-E01（执行代理｜最终整体验收）

派单时间：2026-09-30。派单人：R03 修复总控编排器。派单性质：一次性执行代理（EXECUTOR-REPAIR-R03-G07-E01）。

## 0. 你是谁、只做什么

你是一次性执行代理，只处理 **G07 = F08**（验收接受缺口：新增反例进入正式验收，纠正旧接受裁决）及最终组合验收。基线 `FIX_BASE_SHA=cd3fb19e651f763afc6c75cb3163064fb54ca3fe`，当前候选 `CANDIDATE=8a6303bcd477cda891d64fddd6e96fa35017f7ef`（含已通过独立审查的 G01–G06 修复——不得回退或破坏）。工作区 `/Users/study_superior/Desktop/Code/LingxiAgent`，分支 `codex/rust-tauri-migration`。

**你没有 commit/push 权限，不得自称阶段 PASS。** 阶段最终裁决归总控另派的全新阶段 Reviewer（F08-C04）；你负责把验收面准备到可复核状态。总控账本文件为未提交更新——不要改动 R03_FIX_ISSUES.json/R03_FIX_COMMIT_RECEIPTS.json 中 G01–G06 已登记内容（你只追加 G07 自己的登记字段需要的素材进你的报告，账本更新归总控）。

## 1. 必读

1. `Lingxi_Rust_Tauri_Taskbooks_2026-09-23/Lingxi_R03_对抗性审查_问题清单与修复总控提示词_2026-09-30.md`（F08 节全文 + 总控规程 §10 整体验收/§12 交付清单）
2. `Lingxi_Rust_Tauri_Taskbooks_2026-09-23/Lingxi_R03_修复验收清单_2026-09-30.json`（F08 的 4 个 case：R03-FIX-F08-C01..C04）
3. `Lingxi_Rust_Tauri_Taskbooks_2026-09-23/R03_运行状态机、并发、取消与恢复.md`（A01–A16）+ `05_验收与性能协议.md` §3（xtask verify-stage 契约：空集合/缺场景/过滤 0/缺证据/陈旧证据必须失败）
4. 现行门禁资产：`rust/crates/xtask/src/stage_maps/R03.json`、`rust/crates/xtask/src/verify.rs`/`stage_map.rs`、生成器 `scripts/rust-tauri/r03_t08_generate_stage_map.py`（48 叶镜像自 R00 双账本——**不得破坏既有 16 场景/48 叶结构**）；`docs/rust-tauri/R03/R03_ACCEPTANCE_LEDGER.json`
5. 本轮全部产物：`docs/rust-tauri/R03/repair-current/`（G01–G06 各 E01 报告+R1 审查、账本、回执）与 `artifacts/rust-tauri/R03/repair-current/`
6. 现行报告：`docs/rust-tauri/R03/R03_REPORT.md`（REOPENED 标注节）、`R03_HANDOFF.json`、`R03_FINAL_STAGE_REVIEW_R1.md`（理解裁决2 为何被替代）

## 2. 你的 4 个 C-ID 与任务

**F08（P1-验收）**：旧接受容许未完成取消收口（裁决2 把"普通父取消后 child active/busy 直到重启"解释为 MINOR 递延）；须用新增反例重建验收。

- R03-FIX-F08-C01 逐问题的红绿回归可追溯：建立 F-ID→测试→源码→前后结果（红基线）→三层审查的完整矩阵；真实反例与修复结果一一对应；禁止为制造红灯破坏无关代码。
- R03-FIX-F08-C02 漏项和空测试不能通过：本轮场景索引与实际测试生产者映射后，删除一个映射/使用匹配 0 项过滤器/缺证据文件 → 校验**非零退出并准确点名缺口**，不给总 PASS（门禁负向测试）。
- R03-FIX-F08-C03 候选与三层证据绑定：candidate/runner 绑定、独立证据根、差异记录；修后自查与审查之间故意改变测试副本中的执行输入 → 原 PASS 标 STALE 或拒用；元数据与执行输入明确区分。
- R03-FIX-F08-C04 重新接受范围正确且可交接：为全新阶段 Reviewer 准备完整材料（报告/递延表/Git 状态），正式签名/真实供应商/完整 R07 UI 不作为本次新增前置。

### 具体工作

1. **反例接入门禁**：把 F01–F07 的 7 个新测试套件（cancel_link_inheritance、subagent_closeout、cancel_terminal_race、tool_receipt_unknown、admission_dedup_consistency、admission_dedup_adversarial、input_payload_fidelity、input_budget_refusal、background_steering——9 个）接入 `verify-stage R03`。方式：扩展现有 R03.json 阶段图（新增 basisKind 或 commands，沿用现有机制；不新造第二套门禁平台；不破坏 16 场景/48 叶/7 条 R02 定向链既有注册）。映射须为真实测试生产者（命令过滤匹配数非 0）。
2. **门禁负向测试（C02）**：为新增映射验证负向行为——在隔离副本（/tmp）上删除一条映射/用匹配 0 的过滤器/缺证据文件，验证 verify-stage 非零退出并点名缺口（不修改生产门禁代码来"演示"；负向测试落为可重跑脚本/测试进 repo）。
3. **完整门禁运行（全新证据根 `artifacts/rust-tauri/R03/repair-current/G07-E01/verify-stage-r03/`）**，真实退出码：
   - `cargo fmt --manifest-path rust/Cargo.toml --all -- --check`
   - `cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings`
   - `cargo test --manifest-path rust/Cargo.toml --workspace --locked`（期望 72 suites/704/0 或更高）
   - `cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- check-contracts`
   - `cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- check-boundaries`
   - `cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- verify-stage R03 --evidence artifacts/rust-tauri/R03/repair-current/G07-E01/verify-stage-r03`
   - 原 16 场景 + 48 叶（17 份额+31 递延）+ 7 条 R02 定向链必须全部保持。
4. **受影响 R02 回归**：认证/作用域、单写者与事务、事件快照续读、备份和损坏库、shutdown、A15 真实重启链（r02_t04/r02_t08 等，按 R03 图既有定向命令），真实退出码；全量 verify-stage R02 若有红须逐条归因（旧 FINDING-2/3 已修复、a16/E5 封印族与 patch-too-large 为既有治理递延——不得套用旧类别掩盖新失败）。
5. **C03 证据绑定验证**：verify-stage 的 candidateSourceBinding（testedSha、worktree digest、每命令后 checkpoint）在证据根中真实体现；做一次受控演示（在 /tmp 副本改一个测试输入→重跑→验证 STALE/拒用行为）并记录。
6. **现行报告更正**（保留历史原文，不伪造）：
   - `R03_REPORT.md`：REOPENED 节追加本轮 G01–G06 修复结果与门禁重验；明确替代裁决2 依据；不删除 2026-09-29 STAGE PASS 原文。
   - `R03_HANDOFF.json`：known_gaps FINDING-1 标注已由本轮 G01 产线修复关闭（引用 520bb75b9）；status 保持 REOPENED_PENDING_REPAIR（阶段重开→待新阶段 Reviewer；总控在阶段 PASS 后改）。interfaces 若受 G02–G06 语义影响（CancelRunOutcome::TooLate、InputTooLarge、AdmissionInFlight、RequestIdBoundToEarlierRun、后台 steering）须如实更新交接描述。
   - `R03_ACCEPTANCE_LEDGER.json`：追加本轮 repair 场景/命令登记（不动原 16 场景条目）。
7. **轮次交付物**（放 `docs/rust-tauri/R03/repair-current/`）：`R03_FIX_NORMAL_SELFCHECK.md`（轮次级汇总：逐 F-ID 汇总各 G 组普通自查与证据索引）、`R03_FIX_ADVERSARIAL_SELFCHECK.md`（轮次级对抗汇总）、`R03_FIX_ACCEPTANCE_RESULTS.json`（本轮门禁实际结果：命令/退出码/testedSha/证据路径）、`R03_FIX_HANDOFF.md`（接口、剩余后续义务、边界；引用真实提交，不写自身 SHA）、`R03_FIX_INDEPENDENT_REVIEWS/INDEX.md`（指向 G01-R1..G06-R1 六份审查报告的权威索引）。
8. **F-ID→测试矩阵**（C01）：完整表格落 `R03_FIX_ACCEPTANCE_RESULTS.json`——每个 F-ID/C-ID 对应测试文件/用例名/红基线证据路径/绿证据路径/三层审查状态（从总控账本读取 G01–G06 已登记状态，不重复执行已完成的审查）。

## 3. 红线

1. 保留历史 PASS/FAIL 原文与原证据；禁止删旧失败或把旧报告改成从未出错。
2. 普通取消测试必须验证当前进程终态/线程/配额（G01/G02 已建立的语义不得在门禁化时被稀释为重启语义）；真正崩溃的 startup recovery 测试保留。
3. R04+ 既有递延项保持原要求和归属；不借修复扩大阶段；正式签名/真实供应商/完整 R07 UI/Tauri 发布不是本轮前置。
4. 审计治理残留（seal 坐标族、round3 patch-too-large）单列不伪造已解决；新失败不得一概归入旧 seal 类别。
5. 缺场景、空集合、过滤 0 测试、旧证据、exit 0 但无内容、源码漂移必须使门禁失败。
6. 不使用 git add -A；不 commit/push；不改 G01–G06 已登记账本内容。

## 4. 环境与产出

- 一律 `~/.cargo/bin/cargo`（锁定 1.98.1；PATH 中 Homebrew cargo 1.93.0 禁用）；全部 `--locked`；隔离 /tmp 数据根；无网络外发。
- 证据根：`artifacts/rust-tauri/R03/repair-current/G07-E01/`；报告 `docs/rust-tauri/R03/repair-current/G07-E01_REPORT.md`。
- 回归底线：workspace ≥ 72 suites/704 passed/0 failed；fmt/clippy 零输出零告警；Cargo.lock 不变；G01–G06 全部套件绿。

## 5. 返回格式

结论（READY_FOR_REVIEW / FAIL / BLOCKED）、改动/新增文件清单、verify-stage R03 结果（overall+命令数+场景/叶计数+testedSha）、R02 回归结果（含任何红的逐条归因）、C01–C04 各自完成情况与证据路径、对审查结论的反证（如有）。
