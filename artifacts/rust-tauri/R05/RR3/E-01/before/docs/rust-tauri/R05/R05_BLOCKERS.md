# R05_BLOCKERS — 缺口、负责人、最迟解除阶段

- 生成：EXECUTOR-R05-T08，2026-10-04。范围：R05 阶段终态仍未解除的缺口（阻塞/延期/环境项）。
- 规则：只有真实阻塞才登记；已关闭的修复轮不在此重复（见 `R05_ACCEPTANCE_LEDGER.json` 的 fix_rounds 与各 REVIEW-T0x）。

## 1. RR-BLK-CREDENTIALS — LIVE 真实供应商验证未授权（延期，非代码缺口）

- 状态：`BLOCKED_NOT_AUTHORIZED`（登记于 `R05_LIVE_VERIFICATION.json`）。
- 内容：真实 provider 登录/请求/工具往返/取消/刷新与跨 provider 媒体实测未获授权（无测试凭证、无目标账号、无预算）。按任务书 §9 允许登记至最迟 **R10**。
- 负责人：用户（提供凭证与预算授权）；实现侧无剩余工作（离线全链已交付并过独立审查）。
- 影响：不影响 offline_gate；`stage_readiness` 只能取 `ACCEPTED_OFFLINE_SCOPE_WITH_REGISTERED_LIVE_DEFERRALS` 形态（在独立审查通过后）。

## 2. R05-ENV-ALF-UNSIGNED-TEST-BINARY — macOS 应用防火墙对未签名测试二进制的 LAN 段拦截（环境项）

- 状态：环境项（T01 已定案，台账 `PROGRESS_LEDGER.json` → environment_items）。R05-T08 窗口内未复现（工作区测试与门禁运行中 r00_management_leaves 全绿）；如最终门禁窗口复现且为唯一失败，按环境项登记，不放宽门禁、不改该测试、不当代码回归修。〔2026-10-06/07 RR2 状态更新见 §7：本轮 D 包独立验收绿窗已过（1476/0），但绿窗依赖当刻二进制实例已授予的 ALF 判定——重链接后的新二进制实例可能需用户再次 Allow，本环境项继续在册。〕
- 负责人：用户（ALF Allow / socketfilterfw / 开发者签名三种解法任一）；执行者无法自解（无免密 sudo）。
- 最迟解除：不设阶段期限——它是运行环境项，不是阶段义务；每次复现按台账登记。

## 3. R05-T05-C12 代理/私有 CA 面 — ~~登记的能力差距（延期到网络加固阶段）~~ 已由 RR1 F14 关闭（2026-10-05）

- 状态：已修复（本节原登记的「NOT_RUN / 延期到网络加固阶段」被 RR1 审查（gate G03）判定为不合法延期并经 F14 修复关闭）。统一网络策略已落地：`rust/crates/lingxi-adapters/src/models/network.rs`（system/manual/direct、NO_PROXY 语法+强制环回 bypass、显式 trustedCaPem 叠加于平台校验之上、配置载体验证与 reload 代次发布），chat 五族/OAuth/辅助/worker callback/operations/资源下载全部消费同一策略（`NetworkPlane`）。受控计数代理+源服务+生成 CA 的 HTTPS 正反对照见 `rust/crates/lingxi-service/tests/r05_t05_network.rs`（mod rr1_f14）。原延期条目保留于此仅作历史记录，不再作为缺口。
- 负责人：无剩余（实现与离线测试已交付；LIVE/真实供应商代理环境仍按 LIVE 规则单独授权，不属本缺口）。
- 最迟解除：已解除；TLS 验证保持默认开启，无降级点（负向 R05-GATE-N12 类保护在 T05 套件与 rr1_f14 负例）。

## 4. 平台验证缺口（继承）

- Windows：进程/沙盒执行面尚非已实现（R04 起登记）；R05 未在 Windows 实测。
- Linux：未真机验证项继承 R02–R04。
- 负责人：具备对应平台的执行窗口；最迟解除：按 ORCHESTRATOR_PROGRESS 的平台义务行（不在 R05 内清零，也不因 R05 宣称支持）。

## 5. 已知非阻塞携带项（来自审查轮，登记在案）

- REV-T07 R01 的 F-04/F-05/F-06（mustFix=false）：`UsageFolder::fold` 注释矛盾（当前调用方不可达）、`decode_operation_usage` 非对象分支的 invalid_detail 未截断/scrub、操作面台账行零测试覆盖。已在 T07 fix_rounds.not_fixed_registered 登记，随阶段携带。
- R05-T01-C01/C12 的范围矩阵再生成脚本（python/node）不是 cargo 测试，未注册为门禁命令——其产物（SCOPE_MATRIX/CALLSITE/PROVIDER 矩阵）在 T01 终树生成并经独立审查核对；R05 门禁经 rust_test_workspace + 生产者覆盖代码面。若后续要把矩阵再生成纳入机器门禁，需注册新命令（登记为可选增强，非缺口）。

## 6. RR1 阶段终审 FAIL 的阶段阻塞（2026-10-06 增补，INDEPENDENT-9）

> 〔2026-10-06/07 状态注记，RR2 收口追加：下列 5 项已全部由 RR2 修复轮关闭（任务级）——F41/F42/F34/F43 独立验收 PASS（A-R2/B-R2/R2/C-R2/D-R2 各 REVIEW），第 5 项 F40+三追加 C 行已收口（RR2_ISSUE_MATRIX.json 10 行全部 INDEPENDENT_PASS）。本节历史原文保留不改写；阶段级结论仍以 FINAL-01 终审为准（READY_FOR_INDEPENDENT_REVIEW，未终审不写 accepted）。〕

- 背景：RR1 候选（`d80737b6cb9186c8a18c0f35923aac00249d45c3` + 未提交工作树）的首次完整 verify-stage R05 由独立阶段审查者亲跑，**overall FAIL（exit 1）**；结论 offline_gate=FAIL / independent_review=FAIL / stage_readiness=NOT_ACCEPTED / R06_READY=false。权威记录：`docs/rust-tauri/R05/repair-current/RR1_ISSUE_MATRIX.json` → `independentStageReview`；机器记录：`artifacts/rust-tauri/R05/RR1/INDEPENDENT-9/verify-R05/verify-stage-result.json`；总收口：`docs/rust-tauri/R05/repair-current/RR1_FINAL_REPORT.md`。逐项（详情、复现/控制证据与修复方向见矩阵 F 条目）：

1. **F41 — R02 存储注册表未随 RR1 v6/v7 迁移补登（阶段硬条件，§6.1(5)）**：`docs/rust-tauri/R02/R02-T04_STORAGE_REGISTRY.json` 仍 5 条（v1–v5），二进制 userVersion/supportedVersion=7；`r02_t04_storage_tx.sh` S4 确定性失败（链内+仓库根 standalone 双复现 exit 1）→ R04→R03→R02 闭包红。负责人：待派修复智能体（矩阵建议 T07 修复轮承接、T08 复验）；修法=按首轮 v5 先例补登真实指纹，不得放宽 S4 断言。
2. **F42 — `d80737b6` 的 `.gitignore` 负向规则 × legacy 门禁镜像绑定（阶段硬条件，§6.1(5)）**：`!artifacts/rust-tauri/**/*.log` 使标准门禁布局下运行自身 live 日志进入 E0 镜像绑定且两次绑定间变化 → `r02_t08_legacy_entry_regression.sh` 确定性 FAIL；控制组（stdout 置仓库外）ALL GREEN exit 0 完成精确归因。负责人：待派修复智能体（矩阵给出三个修复方向，须带变异对照且不削弱漂移检出）。
3. **F43 — rust_test_workspace 门禁通道无有效窗口完整通过记录**：verify-stage 内冷编译耗尽 2400s 超时（timedOut=true，被杀前 41 套件 0 失败）；随后 `--no-fail-fast` 完整枚举 115 套件/1472 通过/3 失败 = r00 ALF 环境项（见 §2，需用户 Allow/签名）+ r04_a09 pid 文件 TOCTOU + f24_sigterm 负载期限（后两者隔离与定向复跑全绿，非产品失败）。负责人：测试侧稳定性修复（待派智能体）+ 用户（ALF 项）。
4. **F34 — Anthropic 流『已知 stop_reason + 未闭 content block』防线无永久测试腿**（§6.1(1) 修复中新增发现未关闭）：行为本体正确（`anthropic_messages.rs` finish 的 unclosed 检查），但现电池腿同时省略 stop_reason，变异验证显示该防线可被移除而不红。负责人：WP-T04-R3（矩阵已写明腿的夹具形状与变异要求）。
5. **F40 + R05-T05-C11B/C13、R05-T06-C11B 三行**：LOW 台账收口项——底层义务已有独立证据、行状态未按证据翻 INDEPENDENT_PASS。负责人：总控收口/下一轮（不得无证据空翻）。

- 上述 5 项全部登记于 `RR1_ISSUE_MATRIX.json`（counts：OPEN=8 = F34/F40/F41/F42/F43 + 三行 C-ID）；修复后须按总控 §3.3 重走新修复轮→新任务验收→新阶段终审，本 FAIL 结论不得被后续文档静默覆盖。

## 7. RR2 环境注记（2026-10-06/07 增补，非新阻塞）

- **R05-ENV-ALF-UNSIGNED-TEST-BINARY（§2 同项的状态更新）**：RR2 轮 D 包绿窗已过——独立验收者亲跑 `cargo test --manifest-path rust/Cargo.toml --locked --workspace` 取得首次完整绿窗 exit 0（115 组 ok/1476 passed/0 failed，`artifacts/rust-tauri/R05/RR2/D-R2/workspace-green-window-REVIEW-r1.log`），其中 r00_management_leaves 1/1 以 170.25s held 后放行形态通过。**边界保留**：该通过依赖当刻二进制实例（590de196dceb15ce，未重链）已存在/被授予的 ALF Allow 判定；ALF 按(路径,cdhash)逐实例拦截且判定不跨 relink（D-R2 REVIEW-r1 同刻全新无判定探针对 192.168.3.5 仍 stalled、Apple 签名 python3 ok 为双向对照）——未来任何重链接产生的新 r00 测试二进制实例可能需要用户再次 Allow（或 `sudo /usr/libexec/ApplicationFirewall/socketfilterfw --add`/临时关防火墙/Developer ID 签名）。仍按环境项处理：不放宽门禁、不改该测试、不当代码回归修。
- **vitest worker 负载抖动（一次，已定性）**：RR2 B 包第 1 轮验收 full 轮 r02_t08 E5 判 UNPARSEABLE——producer summary 分量之和不等于总数（1477≠1478 文件/15053≠15058 测试）且日志尾部记录 `Vitest caught 1 unhandled error / Worker exited unexpectedly`。经 `artifacts/rust-tauri/R05/RR2/B-R2/R2/REVIEW-r2.md` §2 离线复析定性为高负载下 worker 崩溃丢失一个 worker 结果聚合的环境型抖动（相对同候选 s5-full-5 恰少 1 文件/5 测试=典型签名）；门禁解析器按完备性谓词（分量必须求和等于总数）正确 fail-closed，非 F42/B 逻辑缺陷，无需代码变更。同链 s5-full-5 exit 0 为完整链定案证据。
- F44 遗留（登记不阻塞）：R10-09/round3 现场重放的生成器 patch replay 步受 git MAX_APPLY_SIZE 限制（未压缩补丁 ≥3.67GB>1023MiB，`error: patch too large`）——生成器属 `artifacts/f1-f12-repair/` 受审材料未修；该形态已由 B-r2 以分类器形态演化登记处置（登记红不转正式绿，历史核实该失败族从未绿过）。

## 无其他已知阻塞

R05-T08 终树五门禁与 verify-stage R05 的真实结果见 `R05_REPORT.md`；如门禁失败，失败项按实登记，不以本文件预告。〔2026-10-06 已按此承诺登记 §6：RR1 候选正式门禁 FAIL，首轮（c549ff654 候选）的 T08-E01/FINAL-WFR2-1 PASS 记录为历史事实、按 N16 不继承。〕
