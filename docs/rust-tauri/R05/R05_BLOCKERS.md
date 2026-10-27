# R05_BLOCKERS — 缺口、负责人、最迟解除阶段

> **RR3 E-04 生成截点（2026-10-08）：stage_readiness=ACCEPTED_OFFLINE_SCOPE_WITH_REGISTERED_LIVE_DEFERRALS / R06_READY=true / offline_gate=PASS / independent_review=PASS。** 依据 RR3/FINAL-04 全新独立终审亲跑：§5.3 六条命令全部真实 exit=0，verify-stage R05 三层（R05/R04/R03）overall=PASS、stable=true、checkpoint 全稳、runner 全 PASS、testedSha=b3ac0e6a+真实工作树，失败清单为空；F42–F54 全部独立 CLOSED；r00 两新对象 cf9bce2f…/d57ea731… LAN 6 次实测通过且 ALF 放行（无证据需要用户操作）。LIVE=BLOCKED_NOT_AUTHORIZED（原许可最迟 R10）、Linux x86_64 继承未复验/Windows 未验证（R09/R10）原边界不变；raw npm 历史 candidate红保持登记不写全绿。Git 至今零暂存/零提交/零推送（FINAL-04 亲核），本 E04 不预写提交回执。本 E04 仅 SELF_CHECKED，待全新 E-REVIEW-05；现行范围见[R05_REPORT §13](R05_REPORT.md#rr3-current)，此前各轮原文（含 §12 E-03 截点）均保留为历史。

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

## 8. RR3 E-02历史阻断截点（2026-10-07）

stage_readiness=NOT_ACCEPTED／R06_READY=false。A/F42、B/F45、C/F27及F46均新独立PASS、包级CLOSED，无包级mustFix；不列为待验阻断。正式全链stable与全部checkpoint仍待新FINAL，历史RR2 FINAL的R04 8/8、R03 15/15来源漂移FAIL不覆盖。当前阻断为：D-REVIEW-01定位/精确准备PASS但必需r00自然101、非回环BLOCKED；E-REVIEW-01独立FAIL后E-02仅修后SELF_CHECKED，待另一新独立验收；G默认16 RUNNING无完整结论；RR3 FINAL NOT RUN。C旧4日志超3及F46旧红永久保留，已由C-F46-REVIEW-01联合独立关闭。不得把包级PASS推成阶段accepted。

§5旧invalid usage缺口归属为历史，后由RR1 F21/F22完成持久记账及诊断安全，不再把必需功能挪R06；WORKER生产trace已接Ledger，usage当前v7。I10普通同实例取消恢复与预算408重启消解分别引用现行报告§11.2。raw npm登记红、合法directed/E5、LIVE/平台原许可保持；新义务不得扩大延期。D具体真实身份、限制、未执行准备及当前跨层失败见[R05_REPORT §11](R05_REPORT.md#rr3-current)。

## 9. RR3 E-03 当前必需阻断（2026-10-07；已由§10取代，历史截点）

**NOT_ACCEPTED / R06_READY=false。** A/B/C-F46/H/I/J包级独立CLOSED，原E两mustFix也已独立关闭；仍有以下实际必需事项，不写“只剩ALF”：

- G02 N02真实构建ENOSPC，未达负测目标；STORAGE03精确回收后约265MB，完整构建空间未获证。保持构建停止，先恢复足够空间。
- 当前默认N01–N16仅N01有效，N02无效、N03–N16及终末恢复未跑；full R02/full E5未执行。I01–I09当前组合和I06额外worker-permission待验，I11未完成；I10仅按H02 375输入相等复用。
- D历史r00真实101、0过1败和LAN超时未解除；旧9f7489对象不代表未来FINAL，不据其旧准备要求用户改系统。未来确切对象需重新核验。
- E03本轮回填SELF_CHECKED待另全新E审；RR3 FINAL从未执行，原§5.3、全部来源/checkpoint/依赖要求仍须新独立审亲跑。最近真实正式RR2/FINAL历史FAIL保留。
- 实际交付新枚举/秘密与引用核验/精确Git收据尚未完成，本截点未提交推送；PREP02旧清单不能当最终名单，部分历史原件仅本地。

证据与具体下一步骤见[R05_REPORT §12](R05_REPORT.md#rr3-current)、[G02](../../../artifacts/rust-tauri/R05/RR3/G-REVIEW-02/REVIEW.md)及[STORAGE03](../../../artifacts/rust-tauri/R05/RR3/STORAGE-03/REPORT.md)。raw npm历史红、directed/E5原合法范围、LIVE未授权与Windows/Linux原平台义务完整保留；没有新延期豁免。


## 10. RR3 E-04 当前缺口（2026-10-08）

**stage_readiness=ACCEPTED_OFFLINE_SCOPE_WITH_REGISTERED_LIVE_DEFERRALS / R06_READY=true（[RR3/FINAL-04](../../../artifacts/rust-tauri/R05/RR3/FINAL-04/STAGE_REVIEW.md)）。RR3 无未关闭的必需缺口**：F42–F54 全部独立 CLOSED（§9 及更早各节的历史阻断均为截点事实，保留不改写）；G-REVIEW-03 默认 16+full R02/E5 包级 PASS（G02 空间阻断经总控 cargo clean 解除，阻断与解除均按时间线保留）；FINAL-01/02/03 历史 FAIL 已由 F51/F52/F53/F54 修复关闭。当前仅存以下登记项（原边界，无新增豁免）：

- **RR-BLK-CREDENTIALS（§1，延期）**：LIVE 真实供应商验证未授权，最迟 R10；负责人=用户（凭证/预算）。不影响已接受的离线范围。
- **平台验证缺口（§4，继承）**：Linux x86_64 继承原登记未复验、Windows 未验证（R09/R10）；macOS arm64 本轮全部真实执行。不因离线放行清零。
- **R05-ENV-R00（原 §2 同族，观察属性）**：按二进制实例偶发——历史 9f748902… 曾被 ALF 拦；FINAL-01/02/03 43d95970… 与 FINAL-04 cf9bce2f…/d57ea731…（CDHash 364514be…）连续放行，**本轮无证据需要用户防火墙操作，也不能写成永久解除**；未来重链接实例若再被拦按台账逐实例登记。
- **观察项（非阻断）**：I06 额外 worker-permission NOT_OBSERVED（沿 G-INTERRUPTION→G-REVIEW-03 结论如实保留，不补造）；raw npm 历史 candidate 登记红保持 registered-not-formal-green。
- **流程收口（非产品阻断）**：E-REVIEW-05 全新独立文档审查 → DELIVERY-FINAL-02/DELIVERY-REVIEW-01 → 总控按既有授权精确 Git 提交/推送并归档真实回执（截至本截点零暂存/零提交/零推送，FINAL-04 亲核，不预写）。

除此以外无其他已知阻塞。raw npm 红、directed/E5 原许可范围、LIVE/平台延期的完整边界见 [R05_REPORT §13](R05_REPORT.md#rr3-current)；负测状态见 [R05_NEGATIVE_GATE_REPORT](R05_NEGATIVE_GATE_REPORT.md) 的 RR3 E-04 节。
