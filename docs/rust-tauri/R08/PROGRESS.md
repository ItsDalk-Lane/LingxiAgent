# R08 执行进度

- 当前状态：ADMISSION_PASS_WITH_SCOPE（F01 准入闭合，F02 仍开放）。R08 T01—T08 中消费五工具真实执行面的业务已放行；把 R07 408 叶 PASS 台账整体当可信输入的验收（T01 多工具矩阵、T07 历史 A/B 复用）须等 F02 收口。只执行 R08，不进入 R09。
- 起点：2026-10-21，分支 `codex/rust-tauri-migration`，HEAD `86840d1700397d3dd620a41f2b884df4cddd5cc2`，与用户基线相同；启动前跟踪文件无改动，历史未跟踪 R07 证据原样保留。
- 成功标准：8 任务、16 基础场景、126 补充叶逐条有真实证据；迁移→新增→回滚→再次前滚可恢复且无双写/重复副作用；真实 Electron/React 结果及独立审查通过。
- 已读：本地 AGENTS；Rust/Tauri 00–05、R08 原任务书、91 模板；R01 ADR-004、ROLLBACK_DESIGN；R02 生命周期说明；R07 报告与交接。机器账按实际结构继续展开，不混入旧 P00。
- 准入：R07 gate-11 机器 PASS 的受测实现与当前提交相符；独立准入审查 FAIL。五工具真实登记缺失与原断言错绑必须关闭，依赖业务不得放行。
- 任务0隔离：显式目录/同名副本/符号别名修复、真实启动及全部捕获进程退出已通过局部独审；9行开工回执见 START_RECEIPT.md。业务准入仍失败。
- 工具链：Node v24.16.0；默认 PATH 是 Homebrew Rust1.93.0，正式基线已显式经本机已有 rustup1.98.1实测通过，不改全局环境。
- 并行准备已完成：R00逐叶账、全类别存储复用调查；另由未实施隔离修复的验收者独审实际启动。独立数据集没有并行写者。
- 下一步：先关闭 R07 两组关键准入缺口并独立复验，或取得不弱化原要求的明确责任裁决，之后才按 T01—T08 推进。未运行/未通过项不得写 PASS。


## 2026-10-21 本轮实测与可恢复断点

1. **隔离修复已完成并独审通过（局部）**：开发入口尊重显式副本目录；自定义副本的 Electron 缓存/单实例空间留在各自真实根内；同根路径别名共用单实例。默认目录规则保留。`ISOLATION_INDEPENDENT_REVIEW.md` 结论仅为 `PASS_LOCAL_ISOLATION`。
2. **真实正反验证完成**：最终主代理 `isolation-final-live-01` 与独立 `isolation-review-final-live-01` 均通过；两根由真实 Rust 服务正规采用后经现有启动器启动，服务和全部捕获的 Electron/Rust 后代正常退出，无残留/强制清理。`isolation-wrong-root-01` 原始退出 1，恢复正确条件后绿。原 live-01—04 的失败及清理回执全部保留。
3. **基线已实测**：锁定 rustup 1.98.1 构建 service/xtask 退出 0；xtask 128 项退出 0；桌面完整 client 构建、三组类型检查均退出 0；隔离针对测试 2 文件/20 项退出 0。具名命令/原始退出/源码摘要在 `artifacts/rust-tauri/R08/task0-*-02`、`task0-xtask-tests-01`、`task0-unit-final-01`。这些不是 R08 产品场景通过。
4. **原账准备完成**：8 任务、16 基础场景、126 叶、281 原断言，27 laterShare 已在126内。地图/生成器 `--check` 实跑退出 0（`map-check-01`）；所有实际验收仍 `NOT_STARTED/UNBOUND`，未以旧 R07 PASS 代领。
5. **存储准备完成**：69 旧类逐项核对；V1—V26 真实 Rust 存储与观测/外发去重收据纳入方案；现有局部导入、停写探针、epoch 限制及真实库往返缺口见 `STORAGE_READINESS.md`。未实施迁移/回滚，未将R01设计级库当本阶段证明。
6. **R07 准入独审失败**：候选实现与 gate-11 相符，旧 testedSha 不是拒绝原因。真实登记表缺五工具，且多个原断言错绑旧实现/不同语义测试。结论及可复验包见 `R07_ADMISSION_REVIEW.md` 与 `r07-admission-review-01/`；不重开已接受 R06，不恢复退役浏览器宿主。
7. **依赖部分未执行**：T01—T08 均未整体完成；真实 UI A/B、文件任务、全数据迁移→新增→回滚→前滚、双写/重复副作用负例、R08 xtask 接线及最终入口均待完成。没有伪造正式阶段报告通过或 R09 放行。
8. **安全边界**：本次测试仅用新建合成根，未执行真实用户数据迁移/写入或生产默认切换；未改全局HOME；未提交/推送/发布。未做真实用户目录逐字节前后摘要，不把它说成已验证。

### 恢复顺序

先读本记录、`BLOCKED.md` 和当前差异。用户已授权关闭独审 F01/F02 的最小修复；须完成有效独立复验，再消费受影响 R07 接口，不扩为 R07 整阶段重做。准入解除后，按 T01—T08 原依赖推进；先处理未采用空白根预览拒启的真实流程，再展开客户端/数据演练。最终关卡仍必须从新目录运行原指定 `verify-stage R08`；准备地图检查与128项runner测试均不代替该关卡。

锁定工具链补证：独立验收者以1.98.1在全新合成根重跑同一五工具反例，实际断言失败退出101；321项相关源码/manifest/锁前后摘要一致。原1.93红证据、新1.98.1红证据与复验包已固化到 `r07-admission-review-01/`；不是编译失败，也不因旧testedSha否定当前候选。

## 2026-10-21 续跑：无依赖验收准备

- 上轮属于实际进展：隔离实现/真实正反实测/独审、原账与存储准备已完成。本轮重新核对 HEAD 和工作区：HEAD 仍为用户基线，R07 F01/F02 源码未变、独审 FAIL 尚未解除；不以自动续跑视为扩大修复授权。
- 已向用户集中询问是否把 R07 两组最小前置修复纳入本次范围。依据是本轮明确“只做 R08”与“前置关键阻塞未解除只做无依赖准备”；未收到必要范围答复前不执行相关业务实现。
- 已新增并实际执行 `r08_map_integrity_probe.py`：完整原地图前后两次退出0且明确 NOT_STARTED；8 个隔离损坏输入分别原始退出1（漏基础场景、漏补充叶并伪改数量、漏/弱化原断言、冒用前阶段、空证据伪PASS、漏替代浏览器义务、漏新增Rust存储）。源摘要前后稳定，原账和原地图未被改写。
- 证据：`artifacts/rust-tauri/R08/map-integrity-command-01/receipt.json` 与 `map-integrity-01/result.json`、逐例日志。仅证明准备地图检查器，不证明业务执行、零匹配/关键skip拦截、双写/外发判据或正式R08门禁；后者仍待实现与实际反向验证。
- T02 的当前源码直连/别名、现有 Rust 适配和 Web 缺能力路径正在做只读精确清点；不进行依赖R07的服务抽取，不把静态结果记为 A03/A04 PASS。

## 生效范围补充：用户允许最小前置修复

用户在本会话对范围问题明确答复：**“允许最小修复，再继续 R08”**。因此现在可修复独审 F01（五工具真实执行注册）与 F02（原断言/证据错绑）两组前置问题；修复后仍须独立复验，不自动视为准入通过。不扩展到R06重做、R09、PR/发布/生产迁移。此前“等待范围答复”的记录为历史状态，不再阻止这两组最小修复。当前阶段仍为 IN_PROGRESS_WITH_ADMISSION_FAILURE，F01/F02 在实际闭合前继续有效。

## 前置最小修复的实施断点

- `run_code` 已接真实解释器、授权网关及统一进程监督器：8项真实集成测试通过，包含实际30秒超时；Node异常原始失败保留，修复后定向复验通过。变量/Agent会话隔离、restart、取消孙进程、缺语言、截断与历史读回有实证。当前仅 `READY_FOR_INDEPENDENT_REVIEW_LOCAL`，未独审放行；见 RUN_CODE_IMPLEMENTATION.md。
- F02同因范围已独立重建：363条full_original来源逐条一致，149条进入复核集合（交叉集合非新增叶），另214条未作语义认证；保留40 UI、102 A23、7条件成功、4错端点等边界。只证明范围重建，F02仍OPEN/FAIL；见 ADMISSION_F02_SCOPE_REVIEW.md及原始逐叶JSON。
- `ast_grep/lsp`、`run_tools`、`security_scan` 实现与公共接线正在构建。扫描器初审发现并实际复现Git子根漏扫、只读索引被刷新、外部进程范围及孙进程收束问题；候选正在修复，未标PASS。Rust子调用驱动刚落盘，须验证逐子调用批准/撤权/日志/取消，不能把外围JS探针视为准入证明。
- 这些改动后的公共候选尚未完成全套构建/回归，任务0旧基线不覆盖新增差异。准入解除前不启动R08依赖业务或改写R07历史回执。

## 2026-10-22：先封授权绕过

用户再次明确“继续完成最小修复，先封住工具绕过授权的缺口，独立复验通过后再推进R08”，既有范围授权继续有效。

- 已真实复现PTC外围宿主函数可取process、绕tools.*写合成文件；红证据 `ptc-implementation-01/escape-negative-01.log` 保留。禁止把旧纯运行时的隔离说明当授权边界证明，正在追加系统级只读运行依赖/无网络限制，不改白名单外lib。
- 独审发现run_code在工作流可绕现有终端的写根限制；实现者已补明确拒绝受限工作流run/restart，真实工作流反例测试正在构建，未宣告关闭。
- 安全扫描已补独立快照沙盒、Git当前根限制及no-optional-locks、流式枚举/取消/跳过提示；Mac严格helper真实Git清单探针退出0。仅此外围探针不证明Rust全链；真实网关测试含外部扫描器越界读写、孙进程收束、索引不变，等待当前候选构建后执行。
- AST/LSP本域真实4项曾绿，后续边界/清理修改必须重新绑定候选。三工具测试统一编译以免竞争Cargo，旧构建遇并行源码变化的失败仍保留，不当作当前产物结论。

## 2026-10-22 本域冻结候选验证与恢复点

- run_code工作流边界已补：受限workflow的run/restart明确拒绝，status保留只读。真实writeFolders窄子目录反例经正式workflow/Run/网关执行；新候选9项退出0（33.83秒），包含原四断言、真实30秒、取消孙进程及该反例。实现者本域结果，不等于完整权限或默认组合根独审。
- run_tools严格外围已补：程序运行时即使越出vm，也只能读固定运行依赖、不能读写合成私有目标或访问本机TCP；真实子进程拒写、实际取消回收、独立子审批与有界UI收据有实证。新driver七项功能退出0；默认600秒单项真实耗时600.30秒退出0，合计覆盖全部8项。完整原叶及公共driver仍待独审，不改当前准入FAIL。
- 默认超时首次被测试进程600秒机器保护器先中止，退出134；原红保留。后以仅该命令TEST_TIME_LIMIT_SEC=660重跑，保留8GiB内存保护，未改产品600秒预算或原断言，未使用假钟。此命令余量差异仍需独审核实。
- 证据：RUN_CODE_IMPLEMENTATION.md、PTC_IMPLEMENTATION.md及ptc-implementation-01/final-candidate-01-result.json。列明源码和二进制前后摘要全部一致；早期失败、错误fixture假设及修正过程保留。外部模型替身与固定元数据ManualClock均已写明；真实计时/进程/内核/授权/SQLite未以假实现代替。
- 未完成：独立审查/默认组合根、完整权限/审核/quota/撤权/fence/settlement与all-tools义务、关库重开历史、F02绑定收口，以及完整R08任务和最终入口。没有用本域绿色数量代领R08完成。
- 恢复注意：根主任务在宿主处返回“内容被标记为可能的网络安全风险”并errored。本子任务未绕过该状态继续公共实现；已完成已在运行的授权本域验证并固化记录。继续前先读原独审FAIL、当前差异和新回执，不把此宿主异常当成代码已通过或缺少既有用户授权。

## 2026-10-22 续跑：上会话遗留 ptc_authority 补验完成，五套件统一候选全绿

- 核对断点后确认：上会话中断前遗留 `rust/crates/lingxi-service/tests/r08_admission_ptc_authority.rs`（10-22 03:39 写入，晚于候选冻结 00:51）从未编译执行、文档无记录。本轮首先补上该步骤。
- 编译：`rustup run 1.98.1 cargo test -p lingxi-service --test r08_admission_ptc_authority --no-run` 退出0（23.67s），回执 `admission-authority-build-01`。
- 执行：`rust/target/debug/deps/r08_admission_ptc_authority-23a9c64c6bcaacac --nocapture --test-threads=1` 退出0，5通过/0失败（1.24s），回执 `admission-authority-run-01`。覆盖：批准等待中撤权后拒绝派发（dispatched=false 持久回执、文件未写）；批准等待中禁用工具→`availability is now disabled` 拒绝；schema 换代 1→2 拒绝旧批准；配额=1 下编排父释放配额、两个真实子调用串行完成且 journal 两条 Succeeded；同 requestId 运行中/完成后均返回原 Run、三次事件续读不改文件与子收据。直接闭合授权独审 PTC-A04/A06/A07 的实测缺口。
- AST/LSP 在当前候选上重跑：deps 中 `r08_admission_ast_lsp-a93a248840f19715`（00:51 链接，SHA-256 389d4b87…，区别于冻结旧产物 530f9af5…）即 cc7f7052 候选产物且此前未执行。执行退出0，4通过/0失败（7.91s），回执 `admission-astlsp-current-01`，含 ast-grep 临时 PATH。旧产物复跑不再重复。
- 统一候选结论：五套件（run_code 9、ptc 8、ptc_authority 5、ast_lsp 4、security 4）全部绑定同一 `candidateDiffSha256=cc7f7052…`、sourceBindingsStable=true（636件），各自回执独立保存。这是实现者本域结果。
- 源码机制核对：PTC 每子调用经 `review_and_validate_from_request_pinned`（runs.rs:301）、结果围栏 `fence_verdict`/`audit_late_result`/`CancelledBeforeWrite`（runs.rs:517/2344/2359）、早拒绝经 `refuse_child!` 落 intent+持久回执（runs.rs:188）。PTC-A03/A05/A01 机制在主链；delegation 仍显式 `PTC_DELEGATION_REQUIRES_DIRECT_DRIVER` 拒绝（runs.rs:421）。
- 已知未测边界（交独审判定）：RC-A02 目录一致性样本、phone/cron 自动审核身份矩阵、取消先到与结果围栏竞态的直接样本、delegation 拒绝收据断言、崩溃中点重放。不以本域绿色代领。
- 下一步：候选冻结不变更源码；派独立审查按原断言逐项判定（含五套件独立复跑、660 秒机器保护器余量差异、PTC-A02 显式拒绝是否符合原叶）；独审结论后再定 F02 149 条收口顺序。
- F02 只读调查已完成并写入 [F02_REPAIR_PLAN.md](F02_REPAIR_PLAN.md)：149 条按域分布（D20 35、D21 31、D19 17、D12 14 全不足、D10 13、D22 13、D17 11、其余≤5）；D12 全部为"拒绝/无效输入"绑 A23 的同构缺口，1–2 个真实拒绝测试可覆盖 14 叶；批次0（五工具叶＋learn_lesson＋D18错端点＋D17两条直接重绑现有证据）→A（D18/D04/D17/D16）→B（D12/D22/D19/D10）→C（D20/D21/D13/D03）。关键发现：五工具叶（C88F29…/08185…/196D5…/2D896…/CD5D7…）的真实证据就是本轮准入五套件，F01 成果直接供 F02 重绑；r07_probe_access 既有 `bridge_wechat_qrcode_create_reports_external_failure`、`desk_cron_lists_current_jobs`、`session_permission_default_read_and_write` 等案例可承接多数批次A"拒绝"断言重绑。机械规则"叶内案例不重复"需按 F02 审查结论调整并独审。修复在独立审查运行期间不启动（候选冻结）。

## 2026-10-22 准入独审通过：ADMISSION_PASS_WITH_SCOPE

- 独立审查代理（未参与实现）完成冻结候选 cc7f7052 全面复验，报告 [ADMISSION_CANDIDATE_INDEPENDENT_REVIEW.md](ADMISSION_CANDIDATE_INDEPENDENT_REVIEW.md)＋9 个证据目录 `admission-candidate-review-01/`。候选首尾两次复算未漂移；五套件全部独立复跑（含 600 秒默认超时单项独立复跑 600.25s 退出0）。
- 五个原叶逐断言独立复验全 PASS（run_code 9/9、run_tools 7+1、ast_grep 4/4、lsp 4/4、security_scan 4/4）。**F01 在准入层面闭合（CLOSED_FOR_ADMISSION）**；B00-TOOL-AUTH 三组授权绕过确认封堵。
- 七个边界项裁决：PTC-A02 delegation=OUT_OF_ORIGINAL_SCOPE；RC-A02=GAP_NON_BLOCKING；PTC-A03=MECHANISM_CLOSED_SAMPLE_GAP；PTC-A05=SUFFICIENT_FOR_LEAF；PTC-A07 崩溃中点重放=NOT_ORIGINAL_LEAF_REQUIREMENT（留 R08 恢复义务）；660 秒保护器余量=ACCEPTED；F01=CLOSED_FOR_ADMISSION。
- 登记的 R08 期跟踪缺口（不阻断开工）：RC-A02 切换样本、phone_scoped 专项矩阵、delegation 拒收回执样本、取消精确竞态样本、崩溃中点重放、security 文件上限/引擎失败直接样本、run_code 无会话分支独立样本。仅 macOS 实测。
- 放行边界：F02（149 条错绑）仍 OPEN；T01 多工具矩阵与 T07 历史证据复用须等 F02 收口。下一步按 [F02_REPAIR_PLAN.md](F02_REPAIR_PLAN.md) 批次0 执行重绑（候选解冻，首个变更后准入五套件需在新候选复跑重新绑定），随后启动 R08 T01。

## 2026-10-22 F02 批次0 执行完成

- 准入放行后立即执行批次0（详见 [F02_REPAIR_PLAN.md](F02_REPAIR_PLAN.md) 执行记录）：8 叶重绑（五工具叶→r08_admission_* 真实套件、learn_lesson→r06_skills_completion 真实测试、D17 两条断言1 去 A23 改绑真实探针）；跨断言案例复用规则在 apply 脚本与 xtask stage_map.rs 同步放宽（单断言组内仍禁重复、pinned 全指派），xtask 128 绿。
- 8 套件经注册生产者命令真实串行执行全绿（`f02-batch0-suites-02`，含 600 秒默认超时单项）；24/24 批次0 pinned 案例 leaf-cases 验证 actual=1；绑定回执 `f02-batch0-binding-check-01` 候选 diff d7fc8d13。
- 真实红证据保留：并行模式下 cancelling_* 断言环境错配失败（suites-01，exit 101）→ 修复生产者串行化（RUST_TEST_THREADS=1）后绿；首轮 xtask 单测夹具错误（xtask-tests-01）→ 修正断言夹具后绿。
- D18 错端点两条未重绑：现有测试不证拒绝分支，部分重绑即造假，移入批次A 补测。
- 准入五套件已在新候选上全绿复跑（suites-02 内含全部五个），维持准入绑定。
- 未完成：批次0 域级独审、批次A—C、R08 T01—T08。

## 2026-10-22 批次0 域审 FAIL→修复→第二轮全绿

- 批次0 域级独审（`F02_BATCH0_DOMAIN_REVIEW.md`）结论 FAIL：6/8 叶过（3 叶带 NOTE）；**D17 两叶断言1超claim**——探针对非法 permissionMode 只断言 400 未回读状态，且 D5514B 只测默认方向 true（默认值 bridge_routes.rs:204）。判据变化（复用规则）、证据链、D18 未重绑决定均 PASS。
- 按域审修复方向实施：探针 `bridge_settings_receipt_and_rich_streaming_toggles_persist` 重写为两开关双向写入回读＋400 后经真实端点读回核对状态不变；lsp 叶 A3 补列错位 prepare/LSP_WRITE_UNAVAILABLE 两案例；run_code 叶 A3 补列含 RUN_CODE_INVALID_LANGUAGE/无会话样本的 `real_gateway_status_remains_read_only…`；D17 rationale 如实更新。绑定表重注入退出0。
- 全部 8 套件在新证据根 `f02-batch0-suites-03` 重新串行执行**全绿**（含修改后探针 9.06s 与 ptc 真实 600 秒单项）；leaf-cases 25/25 pinned 案例 actual=1；绑定回执 `f02-batch0-binding-check-02` 候选 812f053c。
- 聚焦复验独审（前轮问题1—5逐项处置）已在后台运行，产出 `F02_BATCH0_DOMAIN_REVIEW_ROUND2.md`；通过后批次0 收口，进入批次A。

## 2026-10-22 批次0 收口：复验 PASS_WITH_NOTES

- 聚焦复验（`F02_BATCH0_DOMAIN_REVIEW_ROUND2.md`）结论 **PASS_WITH_NOTES**：批次0 整体 8/8 叶通过，前轮 FAIL 撤销。前轮问题1—5逐项核实修复成立（400 后真实读回且读回载体不污染所核字段——bridge_routes.rs 先校验后写、400 路径零写入；rich=false 非默认方向双向回读；receipt 双向；run_code A3 补列含 INVALID_LANGUAGE/无会话样本；lsp A3 补列两分支案例）。
- 证据链复核全部成立：suites-03 8/8 绿、日志哈希相符、ptc 真实 602.39 秒、25/25 pinned 案例绿、R07.json 与绑定表逐叶全等、绑定回执候选 812f053c 与现场重算逐字节一致。
- 三条新 NOTE（不阻断）：套件内串行但跨套件曾并发（不同父 PID 不影响证据）；拒后 receipt=true 单看不可区分保持/重置（由 rich=false 承担可区分证据，与源码互证）；D17 两叶同案例跨组复用属已放宽规则。
- security_scan 文件上限/引擎失败直接样本（前轮 NOTE 6）未处置，维持披露，归入批次B/R08 跟踪。
- **批次0 闭合**。下一步：批次A（D18 错端点补测、D04/D16/D17 剩余叶）与 R08 T01 并行推进（T01 实现面已获准入放行，其多工具矩阵验收仍等 F02 全收口）。

## 2026-10-22 批次A 开工：D18 错端点两条补测并重绑完成

- 新增两个真实拒绝分支测试（`r07_automation_flow.rs`）：`apply_suggestion_rejections_are_explicit_and_persist_nothing`（未知建议/改目标Agent/不支持字段/跨会话身份→显式可见错误；未应用过期（TTL 30 分钟）→automation_suggestion_expired；全程 list_jobs 为空）；`slash_apply_missing_cross_session_and_repeat_never_duplicate_jobs`（找不到建议/跨会话 Studio 身份→错误可见不产任务；重复 /apply 幂等仅 1 任务）。套件 13/13 通过（`f02-batchA-d18-tests-01`，32.96s）。
- `r07_automation_flow` 注册进 R07 地图；两条 D18 错端点叶（095E0402EEEC/5F394AA4C9A3）按原断言重绑（成功链沿用既有真实测试＋拒绝链绑新案例）；leaf-cases 4/4 pinned 绿；xtask 128 绿（`f02-batchA-xtask-tests-01`）。
- 批次A 剩余：D17 七条 bridge 叶拒绝断言、D04 两条会话权限叶＋条件成功组、D16 三条。域审随批次完成后统一做。
- R08 T01 范围调查代理后台运行中。

## 2026-10-22 T01 范围调查完成：关键缺口定位

- 只读调查代理产出 [T01_SCOPING.md](T01_SCOPING.md)：API_COMPAT_MATRIX 634 项（HTTP 423/WS 2/preload 100/PlatformApi 109，无逐项实现状态字段）；Rust 现役路由 410 条全部在 /lingxi/v1/* 下、/api/* 兼容路由为 0（协议转换唯一发生在 renderer 适配层）；旧客户端经 use-hana-fetch serviceRoute→rust-* 适配链＋server-connection 硬闸。
- **最高优先缺口**：`/api/confirm/{id}`（4 个生产组件在用，ask-user/computer-app/mcp 审批提示同路径）Rust 无对应路由——批准链在 Rust 连接下必断，A01 阻塞项，需补 Rust 实现且兼容层只转换、Rust 唯一裁决。
- 纯接线缺口（Rust 路由已就位仅缺 renderer 映射）：knowledge 17:1:1、file-history 4 条、session-projects 9 处、sessions/search、history-overview/conversation-map（后两个需路径改写）。
- Rust 无对应物待实现或显式能力错误：`/api/commands`、terminal WS 消息；mcp-approval-actions 应收敛到 mcp-api 原生分支。
- WS resume 机制完备但缺"带副作用工具断线重连不二次执行"端到端证明（A02）；EndpointError 与旧错误形状统一、列表域游标对账待做（A01/A02 配套）。
- 推荐开工顺序见 T01_SCOPING §6：三方绑定表（纯核对）→ knowledge 接线（首个实现步）→ confirm 路由（A01 阻塞）→ 契约统一 → A01/A02 证据。

## 2026-10-22 T01 §6.1＋§6.2 完成：三方绑定表落地＋knowledge 域接线闭合

- **§6.1 三方绑定表**：新生成器 `scripts/rust-tauri/r08_t01_api_binding.mjs`（映射判定经 TypeScript 转译后调用真实 serviceRoute 求值＋四条自检锚点，未改产品源码；Rust 路由=生产源码 .route 注册扫描 413 条；消费者=desktop/src 字面量形状匹配＋双分支启发式如实标注）。产出 `T01_API_BINDING_TABLE.json`，`--check` 复核模式正反向实测（稳定绿／篡改即退出1）。修复了两处生成器缺陷（枚举大小写笔误致转译失效——被自检锚点抓住；形状键精确匹配改为形状匹配，media 17 项 `{kind}` 参数路由归位）。
- 首轮全量分类（接线前）：MAPPED 285 / UNMAPPED_REFERENCED_DIRECT 65（真断链）/ DUAL_BRANCH_ONLY 37（启发式）/ NOT_CONSUMED 36 / WS 1+1 / 宿主桥 209。DIRECT 缺口分布与范围调查吻合（knowledge 16、session-projects 9、sessions 12、file-history 4、confirm 1…），另新发现 preferences 6、checkpoints 2、env-deps 2 等。
- **§6.2 knowledge 接线**：新增 `desktop/src/react/services/rust-knowledge.ts`（/api/knowledge/* ↔ /lingxi/v1/knowledge/* 逐条同构前缀转换，语义裁决全在 Rust）并接入 serviceRoute 链。验证三层：单测 62/62（含 17 条 knowledge 映射＋不误吞前缀相近路径＋Node 连接保持原路径）；real-service 测试 1/1 通过（真实 Rust 子进程：空表→创建→改名→读回→ingestion jobs=[]，首轮 ingestion DTO 断言错误红证据保留于 t01-knowledge-wiring-01，修正后绿于 -02）；`npm run typecheck` 三组退出0。
- 接线后绑定表复算：knowledge 18/18 全部 MAPPED_ROUTE_EXISTS（总 MAPPED 303，DIRECT 49）。生成器锚点随接线现实更新（knowledge→已映射锚点；confirm 保持"未映射"锚点继续看守 A01 缺口）。
- 安全钩子两次拦截 spawn(env) 写法：已改为固定字面量仓库构建产物路径＋env 仅作等值门槛，不执行任意来源程序。
- 证据：`t01-knowledge-wiring-02`、`t01-knowledge-typecheck-01`；绑定表 `T01_API_BINDING_TABLE.json`（含 --check）。下一步：confirm 路由 Rust 实现（A01 阻塞，DIRECT 首项）→ file-history/session-projects 接线。

## 2026-10-22 T01 confirm 裁决：动态链更正，不建平行确认裁决

- 静态范围调查把 `/api/confirm/{id}` 判为 A01 最高优先阻塞。本轮动态链核查更正：旧 confirmStore 流在 Rust 侧**无生产者**（confirmation 块类型 Rust 源码零命中；history_content 的 confirmation card 属 session-collab 投递，走已接线 apply/reject）；四组件仅 legacy 事件/历史可达。替代链全部已接线：工具批准=approvals 面板（含 MCP 免问）、ask-user=interactions 面板、定时建议=apply_suggestion（批次A 实测）、learn_lesson=R06 token。
- 裁决落地：不实现 `/lingxi/v1/confirm`（避免第二确认裁决/平行空接口）；use-hana-fetch.test.ts 新增专项断言旧路径显式拒绝（63/63 绿）；绑定表生成器新增 ADJUDICATIONS 机制，confirm 项带完整裁决注记（含矩阵概念标签更正——判据变化待独审）；T01_SCOPING §4(b)1 已附动态链更正。T07 A/B 需验证批准 UX 可见等价性。
- 下一步：file-history（4 条）、session-projects（9 处）等剩余 DIRECT 接线。

## 2026-10-22 T01 接线推进：file-history＋session-projects 闭合

- **file-history（4 条）**：`/api/file-history/*` 前缀接入 serviceRoute 内联块（files/versions/snapshot/restore 与 Rust filehistory 路由逐条同构）；专项单测含不误吞前缀；绑定表 +4 MAPPED。
- **session-projects（9+1 条）**：前缀接入后 9 条转 MAPPED_ROUTE_EXISTS；**session-assignment 为真实 Rust 缺失路由**（前端 POST {sessionPath, projectId} 分配会话到项目，Rust projects 模块无对应——绑定表唯一 MAPPED_ROUTE_MISSING，排入 Rust 实现队列）。
- 汇总：hook 单测 75/75；`npm run typecheck` 退出0（t01-wiring-typecheck-02）；绑定表 MAPPED 316／DIRECT 36／--check 稳定。
- T01 累计：knowledge 18＋file-history 4＋session-projects 9＝31 项新接线；confirm 裁决闭合。剩余 DIRECT 36（sessions 12、preferences 6、fs 4=Web 平台域、desk 3、checkpoints 2、env-deps 2、upload 1、html-preview 1、index-direct 4、oauth 4→DUAL 分支）与 Rust 缺失路由 session-assignment。

## 2026-10-22 T01 session-assignment Rust 路由补齐（唯一 MAPPED_ROUTE_MISSING 清零）

- 实现 `POST /lingxi/v1/session-projects/session-assignment`（projects.rs `set_session_assignment_route`＋`validate_assignment_project`）：旧契约 body {sessionPath, projectId}；会话定位经 session_locator（native: 前缀或旧 JSONL 路径）；项目校验同旧 engine 语义（null=清除、auto id 豁免、未知→"project not found"）；写入走 R06 v12 `set_session_project` 同一服务方法（与既有 `PUT /sessions/{id}/project` 同存储同裁决，非平行机制）；未知会话/缺 sessionPath/归档会话显式 400。认证沿目录面既有策略（POST→settings.write），无 auth.rs 改动。
- 真实服务测试 `session_assignment_assigns_clears_and_rejects_unknown`（r06_session_project_catalog 套件 2/2 绿）：分配→studio-list 读回归属→null 清除读回→未知项目/未知会话/缺参/无凭证全部显式拒绝。首轮因 sessions/new 需模型面而红（model plane unavailable，t01-session-assignment-01 保留）；改用显式 sessionId 的 `POST /lingxi/v1/sessions`（201、无需模型面）后绿（-02）。
- 前端：session-assignment 补进映射单测（76/76）；绑定表复算 Rust 路由 413→414、**MAPPED_ROUTE_MISSING 1→0**、MAPPED 317、--check 稳定；typecheck 退出0（t01-assignment-typecheck-01）。
- T01 兼容面现状：MAPPED 317／DIRECT 36／DUAL 37／NOT_CONSUMED 33／WS 2。剩余 DIRECT（sessions 12、preferences 6、fs 4=Web 平台域、desk 3、checkpoints 2、env-deps 2、upload 1、html-preview 1、index-direct 4 等）继续逐域接线或裁决。

## 2026-10-22 T01 直连缺口批次接线：+13 项（MAPPED 330）

- 逐项核对 DIRECT 36 与 Rust 路由形状后分三类：纯前缀接线（11 项）/形状改写待逐项（sessions summary·memory·authorized-folders·content·prompt-snapshot·history-overview·messages 等 ≈9）/无对应待实现或裁决（appearance×2、workspace-ui-state×2、env-deps×2、upload-blob、checkpoints DELETE、fs×4=Web 平台域）。
- 本批接线：desk 技能三件（workbench desk 组）、preview/html（browser）、checkpoints 前缀放宽（user-edit 通、DELETE :id 如实暴露为 MAPPED_ROUTE_MISSING——真实缺失路由）、preferences/session-permission-default（settings 列表）、sessions/{search,find,new-detached} 与 session-thinking-level（内联精确项，不吞会话域其余旧路径——专项负例断言）。
- 单测 87/87（新增 12 映射断言＋不误吞负例）；typecheck 退出0（t01-direct-batch-typecheck-01）；绑定表 MAPPED 330／DIRECT 23／MISSING 1（checkpoints DELETE）／--check 稳定。
- T01 累计新接线 45 项（knowledge 18＋file-history 4＋session-projects 10＋本批 13）＋session-assignment Rust 路由＋confirm 裁决。剩余：DIRECT 23 与 checkpoints DELETE 实现或裁决。

## 2026-10-22 T01 sessions 域形状改写完成：协议翻译层＋8 项转 MAPPED（338）

- 新增 lingxiFetch 层 `translateLegacySessionCall`（use-hana-fetch.ts，已导出）：旧会话域 query 寻址读端口（summary/memory/prompt-snapshot/history-overview/authorized-folders/content/:id）与写端口（memory、authorized-folders PATCH）→ `/lingxi/v1/sessions/{id}/` 子路由。纯协议转换：native:<id> 定位（prompt-snapshot 沿用其 sessionId query）、写端口剥离旧 body.path（Rust deny_unknown_fields）、旧 JSONL 路径不猜映射触达即显式拒绝；权限与数据裁决全在 Rust。
- 单测 90/90（读端口改写＋参数保留＋写端口 body 剥离＋JSONL 负例；首轮 PATCH 早期返回缺陷被测试抓住后修正）；**real-service 端到端绿**（`r08-session-legacy-compat.test.ts`×真实子进程：建会话→memory 翻转往返→authorized-folders add/remove 往返→summary 端口；首轮漏 authorized-folders GET 的红保留于 t01-session-compat-01，补读族后绿 -02）。
- 绑定表生成器接入翻译层（真实求值导出函数＋代表性 query/body 形状），8 项 sessions 叶转 MAPPED_ROUTE_EXISTS（mappedVia=legacySessionTranslation 可追溯）；MAPPED 338／DIRECT 15／--check 稳定；typecheck 退出0。
- 剩余 DIRECT 15：sessions/messages（TraceDetailOverlay，需切 canonical history 客户端）、appearance×2、workspace-ui-state×2、env-deps×2、upload-blob（InputArea 直连，chatFetch 已有正确映射）、fs×4（Web 平台域→T02）、confirm（已裁决）＋checkpoints DELETE（MAPPED_ROUTE_MISSING，缺 Rust 路由）。

## 2026-10-22 T01 收尾批次：observabilityRequest 接翻译层＋三份逐项裁决

- `observabilityRequest`（model-observability-actions.ts）接入同一 `translateLegacySessionCall`：TraceDetailOverlay 的 prompt-snapshot 经真实包装器走 Rust 子路由（此前裸路径必被硬闸拒）；相关测试 95/95 绿（hook 90＋observability 5），rust cargo check 0 error，typecheck 退出0（t01-wrap-typecheck-01）。
- 绑定表新增三条逐项裁决注记：**upload-blob 为消费者扫描口径误报**（唯一调用点实际经 chatFetch 原生分支已映射到 /sessions/{id}/attachments/blob，不经 lingxiFetch）；**sessions/messages 延后**（Rust canonical history 为游标/ETag 协议且响应形状不同，URL 翻译必错，需消费者切 rust-chat-history 客户端，排入 A01 观测域适配）；confirm 裁决维持。
- T01 兼容面最终盘点（本轮止）：**MAPPED 338／DIRECT 15（其中 upload-blob 误报＋confirm 已裁决＋fs×4=T02 域＝6 项有归属，真实待办 9 项：messages、appearance×2、workspace-ui-state×2、env-deps×2、session-permission-mode×2）／MAPPED_ROUTE_MISSING 1（checkpoints DELETE）**。待办分类：session-permission-mode＝Rust 路由缺失（同 session-assignment 型）；appearance/workspace-ui-state＝Rust preferences 路由缺失；env-deps＝旧 Node 系统依赖检查语义，Rust 侧无对应需裁决。
- T01 累计：新接线 53 项（含翻译层 8）＋2 条 Rust 路由（session-assignment 已实现、permission-mode 待实现）＋3 份裁决；A01/A02 端到端证据（多工具事件/历史一致＋断线重连不二次执行）为下一关键交付。

## 2026-10-22 T01 session-permission-mode 接线闭合（DIRECT 待办 −2，真实 Rust 路由补齐）

- **Rust 路由**：`/lingxi/v1/session-permission-mode` GET/POST（userprefs.rs `session_permission_mode_get_route`/`_post_route`，management.rs:604 注册，auth.rs GET/HEAD→Scope("chat")、POST→LocalOnly）。语义对齐 engine 侧 session-permission-mode.ts：GET 返回 `{mode, accessMode, defaultMode}`（焦点会话显式值优先，否则偏好默认）；POST 四分支——currentSessionOnly→当前会话、pendingNewSession→偏好默认、sessionPath（仅 `native:` 定位，旧 JSONL 路径不猜映射显式 409）→会话级写、默认→偏好默认；会话级 "auto" 显式 409；非法 mode 显式 400；无活动会话/未知会话显式 409，不静默落默认。
- **前端**：serviceRoute 内联块新增 `/api/session-permission-mode` 精确项（不误吞——`/api/session-permission-modex` 负例断言）；hook 单测新增 GET 项＋4 组真实消费者 POST body 形状（字节级保留断言），95/95 绿（t01-permission-mode-unit-01）；typecheck 退出0（t01-permission-mode-typecheck-01/-02）。
- **真实服务证明双层**：Rust HTTP 套件 `session_permission_mode_defaults_session_write_and_rejections`（r06_session_project_catalog 套件 3/3 绿，t01-permission-mode-catalog-suite-01）——默认域往返、pendingNewSession、会话级写入后经 sessions/switch 聚焦读回、currentSessionOnly 命中焦点会话、默认写不改会话显式值、400/409×4/无凭证 401；TS real-service `r08-session-permission-mode-real-service.test.ts`（真实子进程＋lingxiFetch 全链，1/1 绿，t01-permission-mode-real-service-01）——同一组分支经前端映射缝端到端命中。
- **auth 面**：auth_matrix 套件 25/25 绿（t01-permission-mode-auth-matrix-02；首轮直接 cargo test 因无隔离环境全数 bootstrap panic，红证据 -01 保留，改经生产者运行）。
- **绑定表**：session-permission-mode GET/POST 双项转 MAPPED_ROUTE_EXISTS（MAPPED 338→340，真实 DIRECT 待办 9→7：messages、appearance×2、workspace-ui-state×2、env-deps×2）；生成与 --check 均绿（t01-permission-mode-binding-regen-01/-check-01）。
- 过程更正：交接记录称 userprefs.rs 内含双单元测试，实际从未落盘（0 命中，t01-permission-mode-rust-unit-01 显示 650 filtered/0 run）；分支覆盖改由上述 Rust HTTP 套件与 TS real-service 双层真实证明承担，覆盖强于原计划。
- 剩余 T01 待办不变：messages（消费者切 rust-chat-history 客户端）、appearance×2 与 workspace-ui-state×2（Rust preferences 路由缺失）、env-deps×2（需裁决）、checkpoints DELETE（Rust 路由缺失）；其后 A01/A02 端到端 WS 证据。

## 2026-10-22 T01 checkpoints DELETE Rust 路由补齐（MAPPED_ROUTE_MISSING 清零）

- 实现 `DELETE /lingxi/v1/checkpoints/{checkpoint_id}`（checkpoint_store.rs `CheckpointStore::remove`＋`delete_route`，lib.rs 注册）：现役 `/api/checkpoints/:id`（server/routes/checkpoints.ts:46-54 → engine.removeCheckpoint → checkpoint-store.ts remove）端口。端口决策显式标注于模块头：缺失/重复删除按现役可观察契约幂等 `{ok:true}`（删除终态已满足）；现役 `remove` 的 `catch{}` 全吞属静默降级不复刻——真实 IO 失败（如同名目录）响亮 500。鉴权零改动：auth.rs 既有 `/lingxi/v1/checkpoints*` 前缀分支 → LocalOnly（现役 STUDIO_OWNER 兜底的最严格档），含本 DELETE 形状。
- 真实服务测试 `actual_http_global_archive_delete_is_idempotent_and_loud_on_io_failure`（r07_t08_interface_preferences 套件 5/5 绿，t01-checkpoints-delete-suite-01）：建存档→删除→落盘消失→列表读回空；重复删除与未知 id 幂等 {ok:true}；同名目录 IO 失败 500；非 loopback 凭证 403 且不删文件；无凭证 401。编译检查绿（t01-checkpoints-delete-check-01）。
- 绑定表：checkpoints DELETE 转 MAPPED_ROUTE_EXISTS，**MAPPED_ROUTE_MISSING 1→0**，MAPPED 341／DIRECT 13（t01-checkpoints-delete-binding-01，生成＋--check 双绿）；auth_matrix 回归 25/25 绿（t01-checkpoints-delete-auth-01）。
- 前端零改动（映射此前已就绪并带单测负例）；该旧路由无前端消费者（绑定表 consumers 仅 user-edit），TS real-service 缝证据不适用。
- T01 兼容面盘点：MAPPED 341／DUAL 37／NOT_CONSUMED 32／WS 2／HOST_BRIDGE 209／MISSING 0。真实待办收敛为 7 项：sessions/messages（消费者切 rust-chat-history）、appearance×2 与 workspace-ui-state×2（Rust preferences 路由缺失）、env-deps×2（需裁决）；其后 A01/A02 端到端 WS 证据。

## 2026-10-22 T01 appearance 偏好端口闭合（DIRECT 13→11）

- Rust `GET/PUT /lingxi/v1/preferences/appearance`（userprefs.rs appearance_get/put_route，lib.rs 注册）：GET 规范化读取（normalizeAppearance 端口：仅 theme 非空白 trim 串 + serif/paperTexture/leavesOverlay 布尔）；PUT 合并写入（wrapper `appearance` 或裸 body 两形），按旧 emitAppearanceEvents 口径仅对变化字段发布 theme-changed/font-changed/paper-texture-changed/leaves-overlay-changed（payload serif 用 serif 键、后两者用 enabled 键）。端口决策：旧 PUT catch-all→400 改为损坏/IO 响亮 500（本模块族既有约定），体校验 400。
- 鉴权（auth.rs 显式臂）：GET/HEAD settings.read、PUT settings.write、其余动词 LocalOnly（现役 /api/* STUDIO_OWNER 兜底的最严承担）。前端 use-hana-fetch inline 块加 `/api/preferences/appearance` 映射 + 负例。
- 证据：r07_t08_interface_preferences 套件 6/6 绿（新 actual_http_appearance_preferences_normalize_merge_publish_and_scope：规范化丢弃/trim、合并保留、事件序列精确断言含幂等写零事件、scope 403、重启持久化、坏体 400 不落盘；t01-appearance-suite-01）；hook 单测 97/97（t01-appearance-unit-02；-01 因编辑缺闭括号红、已修保留）；typecheck 3 组绿（t01-appearance-typecheck-01）；auth_matrix 25/25（t01-appearance-auth-01）；绑定表 MAPPED 343／DIRECT 11（t01-appearance-binding-01）。
- 剩余真实待办 5 项：sessions/messages、workspace-ui-state×2、env-deps×2。

## 2026-10-22 T01 sessions/messages 轨迹叙事通道闭合（DIRECT 11→10）

- 根因：TraceDetailOverlay.fetchSessionMessages 旧 GET /api/sessions/messages 在 Rust 拓扑下撞 rustObservabilityRoute 硬门禁恒抛 → 每条轨迹的会话叙事静默丢失（parity 洞）；文件内 0 个 /lingxi/v1/ 字面量 → 绑定表 DIRECT。
- 实现（desktop/src/react/services/rust-chat-history.ts 追加，native 双分支文件）：纯导出 `projectRustHistoryForTrace(items)` 把 canonical HistoryItem[] 投影为 trace-conversation-model.parseSessionMessages 读取的旧最小形状 + `readTraceSessionMessages(sessionId, signal)`（Rust 分支 readRustChatHistory {all:true} → 投影；Node 分支沿旧 URL 原样）。TraceDetailOverlay.fetchSessionMessages 切换调用，保留 30s 超时与 catch→null→calls-only 显式降级。
- 端口决策如实标注于模块头：工具 args 只取 argsSummary（canonical 不存完整参数串，完整载荷属观测调用侧）；工具 startedAt/endedAt 省略（canonical 只有 firstSeq，解析器允许缺省）；modelCallRef 只填 modelCallId（traceId/parentCallId 置 null，modelCallId 是精确 join 键）；turnStatus 走现役外部词表 completed/failed/aborted（canonical cancelled→aborted、interrupted_needs_attention→failed 如实标错）；无 final 的 run 正文取 final_answer/commentary/unresolved 段、推理取 reasoning 段、时间戳诚实置 null；reset_marker/legacy_raw 跳过。
- 证据：新单测 r08-trace-session-messages.test.ts 9/9 绿（user legacyFields 透传、完整 turn 投影含生产解析器 parseSessionMessages 真实兼容性断言、取消 run aborted/null 时间戳、interrupted→failed、多 turn 顺序、跳过未知 kind/响亮失败、Node/Rust 双分支、非 2xx 响亮失败；t01-trace-history-unit-04；-01/-02/-03 红分别为 mock 缺 subscribe、缺 revision/ETag、类型联合收窄，已修保留）；typecheck 3 组绿（t01-trace-history-typecheck-03；-01/-02 红为 strictNullChecks:false 下判别联合收窄失效，改用显式 Exclude，已修保留）。
- 绑定表：http:GET:/api/sessions/messages 转 UNMAPPED_REFERENCED_DUAL_BRANCH_ONLY（消费者收拢到 rust-chat-history.ts/session-actions.ts 两个 native 双分支文件），**DIRECT 11→10**，MAPPED 343／MISSING 0（t01-trace-history-binding-01 生成＋t01-trace-history-binding-check-01 --check 双绿）。
- 剩余真实待办 4 项：workspace-ui-state×2（teammate）、env-deps×2（teammate）；其后 A01/A02 端到端 WS 证据。

## 2026-10-22 T01 workspace-ui-state×2 闭合（teammate 合入，DIRECT 10→8）

- 新建 `rust/crates/lingxi-service/src/workspace_ui_state.rs`（730 行）逐字移植现役
  `server/http/routes/workspace-ui-state.ts` + shared/workspace-ui-state/preview-reading-position/
  workspace-history/workspace-persistence-gc 全部规范化与 GC 语义（路径/surface/相对路径去逃逸、
  openTabs/previewTabs/readingPosition 全链、record.updatedAt=max(surface)、>50 裁剪、
  GET 缺失目录 GC 仅裁剪时落盘、PUT 不 GC、JS 数值与 UTF-16 截断语义）。lib.rs 注册
  `/lingxi/v1/preferences/workspace-ui-state`（GET+PUT）；auth.rs 逐字对齐 route-security.ts
  GET/HEAD files.read、PUT files.write、其余 LocalOnly；use-hana-fetch 前缀映射 query/body
  逐字节透传。端口决策与偏差逐条登记于 PROGRESS-workspace-ui-state.md。
- 证据（全绿）：编译检查 t01-ws-ui-check-01；套件 t01-ws-ui-suite-01；hook 单测
  t01-ws-ui-unit-01；typecheck t01-ws-ui-typecheck-01；auth_matrix 25/25
  t01-ws-ui-auth-01/-02。
- 本条目为 Lead 合入登记；实现与原始证据由 teammate ws-ui-state 完成，明细见其进度文件。

## 2026-10-22 T01 env-deps×2 闭合（teammate 合入，DIRECT 8→6）

- 新建 `rust/crates/lingxi-service/src/env_deps.rs`（约 760 行）：13 项注册表逐字、探测原语
  （spawn+双读线程+try_wait 轮询+到期 kill，3s/64KB）、受管目录（data_home/runtime/pi-sdk/bin）
  + PATH 两级探测、信号扫描、单槽缓存、两手写路由（GET env-deps；POST refresh；refresh 的
  GET 维持 404 与现役一致）。lib.rs 注册路由；auth.rs 两路径 LocalOnly（现役落 STUDIO_OWNER
  兜底，候选无该会话概念取最严——刻意更严，登记为决策 1）；use-hana-fetch 映射
  `/api/system/env-deps(?:/refresh)?` query 逐字节保留。六条偏差（LocalOnly、findBundledBin
  恒 None、data_home 根、顺序探测、超时自实现、note 不产出、真实时钟）全部登记于
  PROGRESS-env-deps.md。
- 集成套件 `rust/crates/lingxi-service/tests/r08_t01_env_deps.rs` 单测试六相位全绿。
- 证据（全绿）：t01-env-deps-{check,suite,unit,typecheck,auth}-01；auth_matrix 25/25
  （本条 auth 绿与此前「auth_matrix exit 101 环境性失败」记录相左，以本证据为准——
  经生产者 r07_stage_inputs 隔离环境运行可稳定复现 25/25；早前 101 是直接 cargo test
  无隔离环境的 bootstrap panic，两者不矛盾，早前记录属运行方式错误而非产品失败）。
- 本条目为 Lead 合入登记；实现与原始证据由 teammate env-deps 完成。

## 2026-10-22 A01/A02 端到端 WS 证据闭合（READY_FOR_REVIEW）

- **A01「旧客户端不用猜新状态」+ A02「重连不重新执行」**：新套件
  `rust/crates/lingxi-service/tests/r08_t01_a01_a02_ws.rs`（约 950 行），真实服务 +
  确定性替身 Provider，完整跑 read→write→edit→exec_command 四工具链会话。
  - A01：直播 WS 事件（tool_start 命名目标 `tool:first-party:*`、id 成对、turnId=runId、
    seq 连续）与 canonical 历史（toolCallId 唯一×4、run 关联 state:known、firstSeq u64 串、
    RunTerminal 诚实契约——有 final 的 run 零 terminal 项）**同源同 id**（跨通道断言
    `history_tool_ids == start_ids`）；SQL 侧 key_events 表 started==4/completed==4、
    runs.status=='completed'（canonical 工具收执计数，不用恒 0 的 messages role='tool'）。
  - A02：断线重连全游标语义按 chat_wire.rs:1408-1438 钉死——流尖游标空回放；
    回退三格恰好尾三帧（segment_end/run_end/status）；中流游标回放含工具卡片帧但
    **副作用四路计数全程不变**（provider 请求 5、key_events 4/4、exec marker 1、文件内容），
    证明回放是只读重绘；过期游标（≥nextSeq）与未知流均为 reset:true + 全量回放
    （现行契约，非空集）；重复 resume 逐字节幂等。重启后 resume 诚实钉死
    events:[]/nextSeq:1/reset:false（回放缓冲在内存，恢复走历史重建，T01 已覆盖）。
  - **golden 绑定**：`tests/fixtures/r08_a01_ws_golden.json` 由 golden-write 运行生成
    （21 事件/7 历史项/文件副作用/provider 计数全录，不稳定字段占位化），compare 模式
    逐字节比对——任何协议形状漂移立即红。
- 红证据保留：a01-a02-suite-01（mcp_call 包装被网关拒绝，转直连工具名）、-02（帧名/字段/
  SQL 表假设错误）、-03（RunTerminal 只在无 final 时发出，断言反转）、-04（过期游标实际
  返回 reset+全量回放而非空集）。全部按源码真相修正断言，无一弱化。
- 绿证据：golden-write a01-a02-suite-05/-06（-06 为占位符补全后定稿）；compare
  a01-a02-suite-07 2/2 绿（0.47s）；typecheck a01-a02-typecheck-01 三组绿。
- **TS UI 投影验收**：真实捕获 `a01-history-raw.json` 原样落
  `desktop/src/react/__tests__/services/fixtures/r08-a01-history.json`（复制证据
  a01-ui-fixture-02；-01 红为 fixtures 目录不存在）；新测试
  `r08-a01-ui-projection.test.ts` 5/5 绿（a01-ui-projection-02；-01 红为持久历史不含
  argsSummary 的假设错误——产品真相：历史仅存 argsDigest，前端不许编造摘要，已钉死缺席并
  在夹具层断言 digest 形状）：1 条用户消息逐字、1 个助手回合 completed 且非
  missing_final_answer、4 张工具卡片命名目标/成功/顺序全部来自投影、最终回复
  A01-FINAL-ANSWER 进答案区、零伪造 turn_status。
- T01 剩余真实待办 6 项 DIRECT：confirm、fs/read、fs/read-base64、fs/docx-html、
  fs/xlsx-html、upload-blob。

## 2026-10-22 工具链修复：r08_run.py 候选 diff 排除 artifacts（磁盘写满事故）

- 事故：`artifacts/` 含 git 跟踪文件，历史清理删除的 ~11GB 二进制补丁分卷在
  `git diff --binary HEAD` 中逐份内嵌旧字节——25 份 candidate.diff 各 11GB 共 275GB，
  Data 卷写满（t01-binding-check-a01a02-01 红：Errno 28，保留）。
- 修复：r08_run.py 候选 diff 改为 `git diff --binary HEAD -- . ':(exclude)artifacts/**'`——
  候选 diff 只绑定源码候选；证据输出与历史档案不属于受测源码（源码绑定本由
  sourceBindings 逐文件 sha256 承担）。本脚本自身在 sourceBindings 清单内，改动后所有
  新回执自动绑定新版本。
- 存量处置：25 份巨型 candidate.diff 原址 gzip -9 为 candidate.diff.gz（字节无损）；R08 证据
  目录 275GB→66GB，Data 卷可用 411GiB。抽验 a01-a02-suite-06 与 t01-env-deps-suite-01：
  gunzip 后 sha256 与回执 candidateDiffSha256 一致（可独立复验）；小体积 diff 不动。
- 绑定表复算：生成绿（t01-binding-regen-a01a02-01，旧脚本运行、其 candidate.diff 属上述
  巨型组已一并 gzip）**MAPPED 347／DUAL 38／NOT_CONSUMED 32／DIRECT 6／WS 2／
  HOST_BRIDGE 209／MISSING 0**；--check 复跑绿（t01-binding-check-a01a02-02，新脚本首个
  回执）。
- 连带维护：`cargo clean` 释放 rust/target 126.3GiB（纯构建产物，可复现，非证据；
  后续首次 cargo 命令需全量重建，属预期成本）。

## 2026-10-22 事故与恢复：TraceDetailOverlay.tsx 误删（t02-core 批量回滚越界）

- 事故：t02-core 在二分定位其 vitest 红时用 `git checkout -- desktop/src/react/settings/` 回滚，
  误删 Lead 未提交的 T01 sessions/messages 改造（TraceDetailOverlay.fetchSessionMessages →
  readTraceSessionMessages 投影）。其余产物无损：rust-chat-history.ts 187 行追加（服务/）、
  r08-trace-session-messages.test.ts（未跟踪文件）均不受影响。
- 恢复（证据链字节级）：从事故前最后一份含该文件的候选 diff
  `t01-binding-check-a01a02-02/candidate.diff` 抽取该文件 patch 并 `git apply`
  （19+/9-），恢复后 r08-trace-session-messages 9/9 绿（t01-trace-history-recover-01）、
  typecheck 3 组绿（t01-trace-history-recover-02）。文件 sha256=54933714…。本事故再次证明
  候选 diff 作为未提交工作唯一冗余的价值。
- 纪律升级（全体队友已通告）：禁止目录级 git checkout/restore/clean；只允许对本人会话
  亲手改动且经 git status 逐一核对的精确文件清单回滚；清单外异常一律停手报 Lead。
- 附带定性：t02-core 的 t02-batch-b-vitest-01/02 红（TraceDetailOverlayRendering 3 例）系
  Lead 该工作消费方测试当时待适配的中间态，非其收拢引起；已要求其重跑该批确认恢复后仍绿。

## 2026-10-22 R08-A04 闭合（READY_FOR_REVIEW）：无 final 会话卡片展开不退化

- 交付（执行者 t02-a04，全部新增零产品改动）：rust 套件
  `rust/crates/lingxi-service/tests/r08_a04_no_final_ws.rs` + golden
  `tests/fixtures/r08_a04_ws_golden.json`（确定性替身三幕剧本：成功读→失败读→空 final；
  断言 WS 帧/副作用反证/持久历史含诚实 run_terminal
  (completed + completed.no_final.tool_partial_failure)/跨通道同源/供应商回灌账本）；
  桌面夹具 `__tests__/services/fixtures/r08-a04-history.json`（服务捕获原样拷贝）；
  组件测试 `__tests__/components/r08-a04-card-expansion.test.tsx`（3 用例：真实
  projectRustHistoryItems → 真实 ChatTranscript/ProcessFoldBlock/ToolGroupBlock/ThinkingBlock，
  展开汇总区→逐卡点击失败/成功工具卡与思考卡→往返收折）。
- 绿证据：a04-suite-03（golden 写入+捕获）、a04-suite-04（字节比对）、a04-ui-03（3/3）、
  a04-typecheck-01、a04-ui-regression-01（相邻回归 10/10）。红证据保留并记根因：
  a04-suite-01/02、a04-ui-01/02（断言对齐实测/环境，无产品缺陷）。详见
  PROGRESS-a04.md 偏差日志。

## 2026-10-22 T01 补债：TraceDetailOverlayRendering 消费方测试适配 Rust 拓扑（Lead）

- 背景：T01 sessions/messages 改造把 fetchSessionMessages 切到 readTraceSessionMessages
  （Rust 拓扑读 canonical 历史），但既有组件测试 TraceDetailOverlayRendering 的桩只覆盖
  旧 /api/sessions/messages 且只设置 settings store——该套件 3 例中 2 例红（落入 calls-only
  显式降级）。此为 Lead 当时的验收盲区（只跑了新单测+typecheck），现补齐。
- 修复（测试侧适配已验证的产品真相，未改产品码）：该测试改走真实 Rust 拓扑——
  useStore 设 serverNodeKind:'lingxi-service'，桩新增
  /lingxi/v1/sessions/s-render/history 路由返回 canonical 夹具（etag=revision='1:1'），
  夹具投影后与旧 messages 夹具同构（同用户消息/助手正文/read_file 成功工具调用）。
- 证据：t01-trace-overlay-rendering-01（3/3 绿）、-02（typecheck 3 组绿）。

## 2026-10-22 绑定表 --check 红→绿说明（并发编辑瞬态，非漂移）

- t01-binding-check-03（红，exit 1）：t02-core 正在迁移消费者文件的并发窗口内
  复算，消费者清单瞬时不一致。非真实漂移。
- 处置：立即复算生成并与旧表逐字段比对——除 generatedAt 外逐字节相同
  （bindings/inputs/summary 全等）；t02-core 编辑落盘后 t01-binding-check-04 复跑绿。
- 纪律：绑定表 --check 必须在消费者迁移冻结窗口内运行；T02 收尾时以 t02-core
  最终复跑为准。

## 2026-10-22 既有红登记与 T02 批次D范围裁决（Lead）

- 既有红×2（t02-core 批次C 全量 413 文件暴露，非 T02 回归）：
  composer-send.transaction.test.ts「Rust wire 只有握手时拒绝旧聊天帧」与
  R02RustWebSocket.test.ts「stays offline until handshake」。Lead 独立核实：
  两文件工作树=HEAD 未改动；`rust_core_unavailable` 全仓仅存在于测试文件自身，
  产品源码无该符号——R02 期 TDD 先行红，基线即红。归入 T01 在途债台账，
  T05 切换安全审查时裁决（实现握手拒绝 or 显式改测试），红证据保留于
  t02-batch-c-vitest-04。
- T02 批次D（window.platform 直连迁移，约 65 文件）裁决为**要做**：别名仅是
  兼容过渡，边界扫描必须把白名单外 window.platform 读取计为违规，否则
  A03「Electron-free 明确解释」与 T08 独立审查不可信。执行：TS-AST codemod
  限 MemberExpression 根、按目录分批、每批 typecheck+vitest 绿进下一批；
  白名单=host/*、platform.js、shared/theme.ts、mobile-platform.ts，
  __tests__ 赋值桩经收编路径不算违规。

## 2026-10-22 T03 预研（Lead）：docs/rust-tauri/R08/T03_SCOPING.md

- 结论：main.cjs 已有 R07 期 Rust 桌面模式（env 门 LINGXI_DESKTOP_SERVER_RUNTIME、
  READY 握手、实例/token 校验、失败清理、serverNodeKind 分流），T03 缺口集中在
  预览可见性、远端模式（loopback 校验之外）、冻结策略矩阵、进程树证据、A05/A06 夹具。
- lingxi-service debug 二进制后台构建已启动（job bash-b282ea52ff3c4d64），供 T03 消费。

## 2026-10-22 R08-T02 闭合（READY_FOR_REVIEW）：DesktopHost/ServiceClient 边界落地

- 执行者 t02-core。批次 A-E 全部收口，详见 PROGRESS-t02-core.md / T02_DESIGN.md §6.5。
- Lead 独立核验：t02-batch-e-typecheck-01(exit 0)、t02-batch-e-scanner-02
  (boundary PASS, productionViolations 0)、t02-batch-e-check-01 + t02-final-check-01
  (exit 0) 收据逐一复核；vitest 全量 399 文件 3637 过 / 2 红 = 已登记的 HEAD 既有红
  （Lead T01 债，扫码器 leadT01ScopeTestFiles 显式隔离）。变更范围审计与申报一致，
  唯一越范围文件 viewer-window-entry.tsx 已书面报备且属设计目标 5 必需。
- 边界规则：白名单=host/**、platform.js、theme.ts、mobile-platform.ts + 3 份带理由
  单文件；生产违例 0 为阻断项；测试赋值桩 151 收编、测试直连读取 193 处登记为债
  （task-4，T08 终检前闭合）。
- 遗留：A03 组件测试已随批次交付；193 测试读取与 T01 两红不计入 T02 阻断。

## 2026-10-22 T03 设计评审通过（附修订）

- T03_DESIGN.md 批准动工。裁决：About badge 不做；A06 离线退路可但须显式偏差+Lead复核；
  远端 dev-only 批准。
- Lead 评审抓出设计缺口：远端 host 传播链断裂——server-connection.ts:228-229 硬编码
  127.0.0.1、get-server-connection-info 无 host 字段，远端模式渲染层会连回环。
  §3.2 须补全链传播（对齐既有 isLoopbackHost/lan 助手与 browser: 手动连接形态），
  批次 B 验收含 remote host 出现在最终 baseUrl/wsUrl 的单测断言。task-3 写范围相应扩充。

## 2026-10-22 T03 批次 D 重大发现：PPG-1（预览平价缺口 #1）

- t02-core 高质量 BLOCKED 上报：rust 桌面预览链路（真实启动路径）模型工具面无写文件能力。
  Lead 逐环复核属实：main.rs:455 workspace_root 唯一赋值点（strict --config only）；
  main.cjs:1437 桌面 spawn 仅传 --home；ensure_default_workspace_route 只建目录不回灌；
  lib.rs:1666 写工具仅 workspace_access=Some 时注册。drill-06 stub journal 实证
  request.tools 恰 14 个无 write，写任务 outcome=failed.provider_error（红证据保留）。
- Lead 裁决：A05 写腿 = 正式验收发现 PPG-1（工具面断言 + 红证据 + 范式差异成文：
  旧 Node=/api/fs/* 任意路径 HTTP，Rust=workspace 约束），A05 原语义诚实标 BLOCKED；
  A05 读腿（已注册 read 工具真实文件任务）补齐端到端绿证，与 BLOCKED 并列不互掩。
  PPG-1 修复（桌面预览 workspace 策略）移交 T04/T05 裁决，不在 T03 现场发明。

## 2026-10-22 T03 批次 D：A05 双 BLOCKED（PPG-1 + PPG-2），drill-08 证据固化

- PPG-2（读侧孪生）发现并经 Lead 逐环复核：read 工具预览模式「已注册已声明」但
  结构性不可用——lib.rs:1498-1508 零根策略 + 2042-2047 session roots 仅随
  workspace_access 注入（预览=None）→ authorizedFolders 永不生效；sessions/new cwd
  孔道 503、resourceio 需 workspace plane、grant_root 零生产调用方。总根因同 PPG-1
  （无 --config → 无 workspace），失效机制不同（写=工具不注册；读=注册但授权零根）。
- Lead 裁决：A05 双 BLOCKED 呈现（读腿 PPG-2 / 写腿 PPG-1），红证据 drill-06/07/08
  全部保留；PPG-1+PPG-2 登记为 T04/T05 MUST-ADJUDICATE 并列入 T08 终检查清单
  （R08 收口前修复或成文接受为预览限定）。
- 批次 D 正向实绩（drill-08 绿项）：写腿帧级红证据升级为硬断言全过（assistant_run_end
  reason=failed.provider_error + run_failed 帧 + 零 assistant_segment）；14 工具面
  无 write/edit 断言过；S1/S2/S3 node-kernel=0 硬断言过；freeze row1（实窗
  visibilityState=hidden + /health 200）与 row3（Cmd+Q electron+rust 双退出收敛）
  实机验证。drill-06「不发 run-end 帧」记录更正为「旧桩行为差异，服务实际发终结帧」。
- A05 双 BLOCKED 不阻断 T03 其余部分；A06（batch E）继续。

## 2026-10-22 T03 批次 E：A06 双缺口裁决（PPG-3 登记 + 回环退路批准）

- 缺口一（环境）：macOS ALF 对 adhoc 签名 debug 二进制静默丢弃 en0 入站——P0a/b/c
  三帧实证（lingxi-service 0.0.0.0 绑定下 127.0.0.1→200/LAN→超时；python3 对照同
  LAN IP→200 = 非服务缺陷）。不碰系统安全设置。
- 缺口二（产品，PPG-3）：远端桌面真实 LAN 端到端不可认证——auth.rs:2558-2566 非
  Local 恒拒 loopback token；rust-local-service.cjs:369-372 的 32hex 正则拒
  "hana_dev_" 设备凭证形状（无呈现通道）；签发面 /devices/credentials 为 local_only。
  batch-B remote 单测覆盖形状校验但从未 e2e，A06 首次端到端。登记 PPG-3，
  T04/T05/T08 MUST-ADJUDICATE。
- Lead 裁决：接受回环退路（服务 127.0.0.1:0，A/B remote 分支 + local-token.json），
  A06 核心零接触/双客户端/连续性语义不变；**补防降级硬断言：A/B 两端
  rustConnectionMode==='remote' 必须经 GSCI/诊断取证，落回 local 分支即红**。
  auth-probe-04 绿（env-blocked-lan-source-chain-stands）；01/02/03 红保留。

## 2026-10-23 T03 批次 F 收尾：口径校正 + 两项新登记

- t03-batchf-typecheck-01 绿。t03-batchf-vitest-01（bare 全仓 1545 文件）22 文件 123 红，
  t02-core 逐一定性：T03 邻域切片全绿 + HEAD 基线 2 红同名同数 + 20 文件归因非 T03 域。
  Lead 复核发现口径错位：T02 时代基线为 scoped（desktop/src/react/__tests__ 399 文件），
  bare 全量含 staged-harness 套件裸跑必红项。闭合门禁 = 同口径 scoped 复跑
  （t03-batchf-vitest-scoped-01，期望恰 2 红），补齐后关 task-3。
- 新登记 ①：migration/network-guard-negative 隔离复测仍红 = migration 域真红，
  T04 开工前必查。新登记 ②：persistence-schema-tripwire/store-registry/startup-receipt
  裸跑红（dangling knowledge-indexes 规则），task-5 用 staged 环境复验定性。

## 2026-10-23 T03 闭合（READY_FOR_REVIEW）

- 批次 A-F 全部闭合。交付：预览可见性（标题后缀「· Rust 预览[/（远端）]」+ 崩溃诊断
  两行）；远端模式（dev-only：packaged 抛、cleartext 显式门、URL 先于令牌校验、
  结构性零接触）；host 全链传播（GSCI/preload/server-connection，远端走手动连接
  归一化）；冻结矩阵 6×4 全格代码核对 + 批次 D/E 实测回填；进程树记录器（POSIX/win32
  CIM，截断形态修正）。
- A05 = 读腿 BLOCKED(PPG-2) + 写腿 BLOCKED(PPG-1)，drill-06/07/08 红证据全保留；
  写腿帧级硬断言绿（run_end failed.provider_error + run_failed + 零 assistant_segment）；
  node-kernel=0 三点硬断言绿；freeze row1/row3 实窗验证绿。
- A06 绿（t03-a06-drill-05，34/34）：回环-remote 双客户端、GSCI 三重 remote oracle +
  标题第四重、退 B 零接触（服务 pid/instanceId 连续 + A 任务完成）、harness 回收。
  偏差：退出机制 SIGTERM 优雅兜底（键盘路径双同名实例前台竞态，红迹 drill-01..04 保留）；
  真 LAN 待 PPG-3。
- PPG-1/PPG-2/PPG-3 登记 T04/T05 MUST-ADJUDICATE + T08 终检查清单。
- 闭合门禁：t03-batchf-typecheck-01 绿；t03-batchf-vitest-scoped-01 同 T02 口径
  恰 2 基线红同名同数（3639 过/25 跳/2 红）；全量 bare 清单 123 红定性表入
  PROGRESS-t03.md 批次 F 节（意外收获：两项已登记待办）。
- 批次 E 零产品改动经 Lead mtime 独立核实。T03 全部证据链齐，进入待独立评审队列。

## 2026-10-23 基线红家族 #2 登记：persistence 三件套（10 测试）

- ws-ui-state staged 复验（r07r-persistence-trio-01，exit 1）：persistence-schema-tripwire 4 /
  store-registry 5 / startup-receipt 1，根因同一：scan-persistent-stores.mjs 规则
  knowledge-indexes → lib/knowledge/vector-search-backend-factory.ts 声明写 token
  [mkdir/remove-path/rename/database-open]，但该文件自 R06 时代提交 6f5c4b01d 起只做
  构造注入（写动作在被注入的 ann-index-store/usearch-vector-backend 内）→ dangling rule。
- Lead 复核：三涉域路径全部 git-clean = HEAD 提交树自带漂移，非 R08 引入。
- 裁决：R08 不修（旧 Node 持久化治理域，修复需单独授权）；登记为基线红家族 #2，
  去向=用户/后 R08 裁决。T04 注记：knowledge-indexes 在 69 类清单内，夹具计数对
  实际库文件取证，不以扫描器位点清单为准。
- 治理教训：R06 bulk checkpoint 引入规则漂移未同步扫描规则。
- task-5 口径更正：TASK_SUITES 实为 51 套件 + R07_INPUTS --check + A23 behavior_matrix
  = 53 命令；台账如实标注。试点 r07_scheduler / r07_t08_interface_preferences 绿，管道通过。

## 2026-10-23 task-5 验收：R07 套件重绑落定（53/53 全绿定稿）

- 53 命令当前候选重绑完成：51 套件全绿（387/0）+ inputs-check 绿；a23-matrix 初红
  = env-deps 两路由未登记 r07_behavior_matrix.py PREFIX_TASK（R08-T01 叶产物，T01 表
  已 MAPPED）。Lead 授权单行登记 `"/lingxi/v1/system/env-deps": "R08-T01(env-deps)"`
  （插入 settings 与 user-profile 之间，git diff=1 insertion，已核验全部
  /lingxi/v1/system 路由仅此 2 条、无前缀误捕），r07r-a23-matrix-03 复跑转绿
  （exit 0，416 routes / 0 unmapped）；-01/-02 红证据保留 superseded。**53/53 全绿定稿**。
- 候选分段裁决（接受）：串行期间 task-4 并行暂存 desktop 测试文件致候选 6 段；ws-ui-state
  逐字节核验 55 份 candidate.diff 的 rust/+scripts/rust-tauri/ 部分恒定（17 段
  16905aa29383）= 被测产物零扰动，分段与服务被测面零交集；T08 终检在最终树单候选
  全量复验兜底，不要求静默窗口重跑。
- 旧 cc7f7052 绑定回执声明 superseded（保留不删）。

## 2026-10-23 task-4 闭合（READY_FOR_REVIEW）

- 193 → 30（163 迁移经宿主边界 + 23 显式豁免 + 7 Lead T01 域）。豁免类：B=桩卸载 delete×12
  （resetDesktopHostForTests 设计上无权删桥本体，与白名单赋值对称）/ C=桥身份保存恢复
  脚手架×8（getDesktopHost 惰性安装副作用不能捕获安装前身份）/ D=宿主边界自测×3。
  逐条理由在 PROGRESS-t04.md 豁免登记节。Lead 抽验三类理由成立。
- 门禁：t04-closure-scanner-check-01 exit 0；t04-closure-vitest-scoped-01 恰 2 基线红
  同名同数（3639/25/2 与 T03 收口基线全等）；t04-closure-typecheck-01 exit 0。
- 11 批逐批绿回执 t04-batch01..11；零产品源码/零 rust/ 触碰；红线全守。

## 2026-10-23 T04 设计批准 + network-guard-negative 定性（基线红家族 #3）

- T04_DESIGN.md 批准：离线 CLI（lingxi-service legacy-import --source --target [--resume]）
  不开常驻端点；V27 三表簿记只追加；69 类切片（11 实转/35 登记/13 重建收据/10 out-of-scope）；
  legacy_unknown 六条边界；A08 双轮崩溃模型。绑定条件四条（V1 形状不变+B2 受影响套件复跑、
  夹具逐字节确定性、无 --resume 重跑 fail-closed 硬断言、家族编号更正）已下达。
- network-guard-negative 定性（t04i-netguard-probe-01..04）：Node 24.16 bootstrap 期
  Agent.createConnection 与 net.createConnection 解耦，用户态猴子补丁整体够不到
  http.Agent 外连面；双层补丁均未触达，真实外连静默悬挂 = 60s vitest 超时形态。
  7 测试仅绕过类②失守。**基线红家族 #3**（R00/migration 域，修复授权归该域，R08 不修）。
  家族编号：#1=vitest 2 基线红；#2=persistence 三件套 10 红；#3=network-guard-negative。
- A07/A08 零外发证明改用结构性论证（合成 HOME+只读源+importer 无网络调用面），
  不依赖已失效守卫。探针③诚实注记（exit 前或发出一次真实 DNS 查询，无 HTTP 载荷）在账。

## 2026-10-23 T04 B1–B3 收口 + 基线红家族 #4

- B1（V27/扫描/manifest/checksum/收据骨架/夹具生成器 v1）、B2（切片 A：session_jsonl+
  sessionfiles V2 扩展纯增量、受影响 R07 两套件窗口内复跑双绿）、B3（切片 B/C：trace v1..v7
  薄壳复用、cron 同链、V28 运行态登记 8 类、stamp=1 epoch 候选、插件/knowledge 负路径如实）
  逐批复验通过。夹具演进 v1→v2 在账；条件②确定性对 v2 重证闭合（40395742…5832df）。
- **基线红家族 #4**：adapters browser::cdp a_stalled_pipe_write（stash 至 HEAD 复跑同红、
  browser/ 域未被 R08 触碰；非本次引入，修复授权不在 R08）。
- 裁决：plugins/knowledge 正路径不造夹具 v3，移交 T07 真实数据演练覆盖（T05_SCOPING 在账）。
- T05_SCOPING.md（Lead 预置）已落 docs/rust-tauri/R08/：协调器五硬要求 + A09 五断点 +
  A10 受控小卷取证 + PPG/基线红复核清单。

## 2026-10-23 T04 验收通过（task-6 关闭）→ T05 开工

- B4 终验：Round A stop/resume（15/15+27/27 vs 基线）、Round B kill -9 深层捕获
  （75done/1in_progress/359planned → 435/435 恢复、issues 5207=5206+1 kill-recovery）、
  条件③ e2e（raw-exit=2 fail-closed 钉住）、收据终审（resumedFromRunId 链 + kill-recovery）。
  四条绑定条件全闭合。missing-run-boundary 触发不对称=真实缺口已修（诊断集对称）。
- 归一化比对口径沉淀：run-bound 列归一化 + identity 哈希全等 + 主键无重复 + issues
  多重集相等 + 行数单调 + 源清单字节不变 —— T06/T07 沿用。
- task-7（T05 迁移协调器/切换状态机/A09/A10）派 t02-core，设计先行。
  T05_SCOPING.md 为 Lead 预置硬要求。

## 2026-10-23 T05 设计批准（P1–P8 全裁决）

- T05_DESIGN.md（16 节）批准：10 相 journal 状态机（相位=唯一权威）、三遍 manifest 静止点、
  copy-set→scratch→Online Backup 源零改动、epoch 采用执行器复用既有拒启分支作牙齿、
  pointer=atomic JSON（拒 symlink）、--self-check 受限启动、单写者三层证据链。
- P1 裁决：epoch 真升 2，客户端显式双章接受 {1,2} 对账派 **t02-a04**（保 ws-ui-state 独立评审）；
  硬排序：t02-core 切 R08_CUTOVER 常量晚于 P1 绿灯。R02RustWebSocket.test.ts 禁触保留。
- P2 libc statvfs / P3 动词体提取+零行为对拍 / P4 拒 symlink / P5 self-check 面 /
  P6 源库绝不直开 / P7 全集 69 缺类 refuse / P8 真升 2+独立常量+爆破半径红→绿销账 —— 全批。

## 2026-10-23 P1 客户端纪元对账完成（t02-a04）

- desktop/src/react/services/websocket.ts +34/-3：RUST_ACCEPTED_DATA_EPOCHS=[1,2] 显式枚举
  替换严格相等；rustWireHelloDataEpochDirection() 方向可区分（too_old/too_new/非整数通用）；
  4409+wsHandshakeFailed 不变；shared/contract-versions.{ts,json} 未动。
- 证据链：p1-baseline-01(1红/5绿) → p1-epoch-01(13/13) → p1-r02-regression-01(1红/5绿
  根因逐行一致) → p1-adjacent-01(15/15) → p1-typecheck-01(0)。R02RustWebSocket.test.ts 未触。
- brief 路径偏差（lib/→services/）如实在账。**R08_CUTOVER 常量切换排序约束解除。**

## 2026-10-23 T05 Phase A 验收 + 基线红家族 #5

- P3 动词体提取零行为对拍闭合：重构后 bulk 435/435、issues 5206、runs.db 逻辑 dump
  摘要双侧相等（8e8d661695d75947）、20 断言 PASS（t05-refactor-parity-01/-04）。
- **基线红家族 #5**：service lib 3 红（owned_media MCP 只读目录 / github real-driver /
  concurrent_atomic_receipts）——pristine HEAD worktree 同签名复跑、文件相对 HEAD 零改动、
  Phase A 只动 bin 目标；非 R08 引入。家族台账：#1 vitest 2 红 / #2 persistence 10 红 /
  #3 network-guard-negative / #4 browser::cdp / #5 service lib 3 红。

## 2026-10-23 T04 账目残留清零（登记表同步）

- #6 候选裁决执行：非 kept-red，系 T04 V27/V28 未登记的账目缺口。按登记表既有 parser
  路径+指纹先例同步至 tip=28，r06_ar_f14_migration_receipts 红转绿（测试零改动），
  r02_registry_consistency.py 门 PASS（28 entries 三方指纹一致）。
  证据 t05-registry-sync-01/-03/-04。登记表 generated_by 追加 R08-T04 收口注记。

## 2026-10-23 T05 Phase C/D 验收 + 家族 #5 改判

- Phase C（epoch 采用执行器）：28/28 + 回归红集=#5 签名；写后回读当场抓真实缺陷
  （prepared 相位误带 checkpoint）；启动门拒启文案诚实化（点名 coordinator-only）追认。
- Phase D（--self-check 面）：703/703 + 7/7 全绿（t05-selfcheck-02）；8 项结构性禁启 +
  业务面 503 self_check_surface_closed 抽样钉死；SELF_CHECK_LISTEN k=v 偏差追认。
- **基线红家族 #5 改判「环境受限-已定位」**：macOS TMPDIR /var→/private/var 符号链接 +
  受限 ReadDirectory → mcp_settings read_home=None；真实 TMPDIR 下 trio 3/3 绿。非代码红，
  R08 不修，移交 post-R08 卫生清单；service_health 4 红同族归并。

## 2026-10-23 T05 Phase E 验收 + 家族 #7 + 夹具 v3 授权

- Phase E（migrate 动词编排）绿：t05-migrate-e2e-04 24 断言（评审 N1 更正：此前记 17 系
  低估，实际 24；停相/八相 history/import+adopt
  书签/指针读回/旧根逐字节零写/user_version=28/无残渣/裁决书相位门/双守卫 exit 2）。
- 裁决①夹具保真：授权生成器正式修复——knowledge.db 改真实 SQLite（对 store-registry
  声明）、mystery-unmapped.bin 移出基座改派生变体、锚点 v2→v3 演进在账、parity pin 同步、
  确定性重证；e2e 垫片为过渡、T07 前必须消除。
- **基线红家族 #7**：r00_management_leaves LAN 绑定（本机防火墙/TUN 拦未签名二进制，
  panic 文案自证 + R07 间歇史）= 环境受限-间歇，post-R08 卫生清单。
- fmt 漂移披露接受（语义中性、终 diff 单独标注）。

## 2026-10-23 T05 常量切换推进 + 家族 #8 + P9 挂起待取证

- R08_CUTOVER 常量切换 Rust 侧绿（protocol 44/44、lib 703/703）；R02 四链对齐按 P8
  爆破半径执行（红留档+绿闭合）；cargo fetch 披露接受。
- **基线红家族 #8**：R02 cli-matrix 脚本腐化 4 红（裸 INSERT 5 列 vs 20 列 schema 及级联），
  预存腐化、T05 未触、R08 不修，post-R08 卫生清单。
- P9（selfcheck 计数分叉 vs ensure_session_seed +2）挂起：grep 证实 sess_local_alpha/beta
  仅 Rust 侧存在，Node 无同 id 落库 → 真实迁移根首启同样分叉，「真实数据 delta=0」
  假设证伪。决定性取证：旧 Node 栈合成会话的展示形态与条件（内存合并/空库限定/API 注入），
  连同 R02-T03 平价原始语义——取证到位前不选 A/B/C/D。

## 2026-10-23 P9 裁决：A-变体（空表才播种）

- 取证结论：旧栈从不展示合成会话（server/shared/desktop 零命中）；种子系 R02-T03 内存
  脚手架道具，R02-T04 ensure_session_seed 落库时注释自声明「fresh database」但实现无条件
  INSERT OR IGNORE —— 实现违背自身文档，对迁移根构成超额 +2。
- 裁决：空表守卫恢复自声明语义（既有安装 no-op、全新安装行为不变、迁移根不再 +2）；
  self-check 计数不变量零容差原样成立；B/C/D 拒绝理由在账。
- **T07 清单追加**：真实迁移根首启后会话计数 == 导入收据计数（无种子增量断言）。
- smoke 链闭合（17/17）；logreg TMPDIR=/tmp 符号链接红 = 家族 #5 同族（本机结构性必现，
  T08 终门同墙预期内）。

## 2026-10-23 P9 修正：α1（守卫变体）替代函数内守卫

- 爆破半径扫描（停手条件正确触发）：ensure_session_seed 37 调用点，
  r07_bridge_notice 同库连播两次铁证 → 函数内守卫破消费契约。
- 裁决修订：新增 ensure_session_seed_if_empty（空表守卫），lib.rs:1335 唯一生产调用点
  换用变体，原函数零改动、37 调用点零爆破；「调用点不动」修订为「编排语义不变仅换变体」。
  红→绿链：t05-d2-01-full counts diverge 红 → α1 后 D2-full 绿。

## 2026-10-23 T05 CLOSED READY_FOR_REVIEW；独立评审 + T06 启动

- T05 验收：D2 四腿绿（d2-07/08/09 + full）、A09 30 组全绿、A10 353MiB PASS（64MiB 拒迁
  红留档）、夹具 v3 定版垫片移除、α1 种子守卫 4/4、爆炸面 703+7+3 全绿、写域审计干净。
  task-7 completed。**T05 CLOSED READY_FOR_REVIEW**。
- 独立评审启动：ws-ui-state（与 t02-core/t02-a04 均无交集），产出 T05_REVIEW.md，
  证据前缀 rev-t05-*，问题分级 BLOCKER/MAJOR/MINOR，评审人不修问题。
- #7 定性重申：已裁决=环境受限-间歇（t02-core 收官报中的「候选」系消息交叉）。
- T07 清单钉子已录：真实迁移根首启后会话计数 == 导入收据计数（无种子增量）。

## 2026-10-23 T05 评审 PASS + T06 设计批准

- **T05 独立评审 PASS**（ws-ui-state，BLOCKER 0 / MAJOR 0 / MINOR 4；rev-t05-* 8 组独立
  复跑全绿；T05_REVIEW.md + PROGRESS-review.md）。N1 断言数已更正（17→24）；N2 入 T08
  卫生清单；N3 范围外；N4 转 T06 保持。T05 正式 CLOSED（评审通过）。
- **T06_DESIGN 批准**：J1–J8 全采纳（J1 旧根准备相唯一事务写入+原字节先归档；J2 默认不回写；
  J3 冻结身份集差集；J4 模态 A 默认；J5 新动词 rollback/reforward；J6 失败回指新根；
  J7 scaffold-seed 导出；J8 印章 v2{1,1,1} Node 同构）。附三钉：N4 保持 / 哨兵禁 queued /
  D1 新增走正式服务入口。演习矩阵 D1/D2/D3 批准。task-8 in_progress（t02-core）。

## 2026-10-23 T06 进行中 + manifest 双哈希发现裁决

- **发现**：T04 manifest.rs file_sha256 双重哈希（sha256(sha256(content))），全消费方同口径
  自洽从未炸响=证据保真度瑕疵非功能 bug；已锁进 T05 封存锚（80a8d2c2 对拍口径）。
- 裁决：R08 内不修（修=锚变=封存链与 rev-t05 评审收据失效，评审后动被评审码红线）；
  T06 增量包用真·sha256（外部可验）与内部约定显式隔离批准；runbook 披露义务升级必须。
- **post-R08 治理项登记**：manifest.rs 双哈希修复 + T05 锚重封（与 #5/#7/#8 同列卫生清单）。

## 2026-10-23 T06 里程碑② + J9 裁决

- 编排器建成：cutover_rollback.rs ~2175 行 6 单测、双动词注册、migrate 零改动、build 零警告、
  bin 13/13、adapters 176/176、Cargo.lock 零新增。三偏差追认（backup_new_root 独立实现/
  /api/health 实测端点/journal 按动词分离）。
- **探针白名单改证据派生**：控制探针（旧栈在未回滚副本冷启动）先行实测写集，
  D1 白名单=控制实测∪准备相；回滚相内冻结/探针相起活跃，口径分离。
- **J9 批准**：准备相写入集扩展 {印章重写, 过渡日志归档, 夹具同构修复集}——封闭两项
  （execution-leases.json schemaVersion:1 / a1 config.yaml 引生产 first-run 模板），
  J1 纪律全适用，修复集外新阻断=停手上报。fixture_gen 不动保锚。

## 2026-10-23 J9 细化①批准 + ⑤a 裁决

- J9①细化：execution-leases.json → 生产空注册表形状（createEmptyExecutionLeaseRegistry
  逐字段）；夹具 {id,holder} 行是注册类覆盖样本非有效 lease，「原样保留」不可行。
- **⑤a 批准**：knowledge.db 合成表 ALTER RENAME fixture_synthetic_*（行零丢失字节在库内），
  迁移链自建生产空表=首启同态；⑤b/⑤c 拒。钉：原库先归档/事务内手术+integrity_check/
  prepareWrites 与 readable-scope 如实披露/T07 对照清单录三例同族噪声（epoch:7、leases、
  knowledge.db——真实旧根皆不会有，T07 见任一=数据异常停手）。

## 2026-10-23 控制探针转绿 + 两裁决

- t06-control-probe-05 绿（added=55 modified=4 removed=0 markersClean）；J9+⑤a 全链生效。
- **① data-epoch 条款修订批准**：四 epoch 字段逐值冻结断言不变；lastVersion/updatedAt
  provenance 盖戳允许+证据列名+runbook 披露。
- **② models-store.json 增补 Register 类批准**（三方 SDK 运行时存储，生产同写；不修则 T07
  真实旧根必撞 Unclassified 拒）。四条件：STORAGE_READINESS 对拍定性 / registry JSON 同步+
  一致性门 / 修改后夹具锚不变确定性双跑重证（t06-registry-amend-*）/ 定性「T05 评审后
  注册表修正」披露入最终 diff 评审包。
- 事实更正：/api/health 就绪相需鉴权；Rust 探针两缺陷+终止纪律修复批准。

## 2026-10-23 T06 实现续接会话：§4b–§4h 全链闭合（D1/D2/D3 全绿）

- 续接者按交接书推进（上一轮 t02-core 断点后）：§4a 探针三修正确认在位；**§4b
  models-store.json 增补**（classes.rs provider-state + 注册表注记 + 一致性门 PASS +
  双腿双跑重证：v3 锚 80a8d2c2 保持、大规模源精确复现 435/5206/40f42dc1）；
  **§4c 证据派生白名单**（控制实测 18 类∪准备相 2 类 + wal/shm 兄弟泛化 + 裁决①
  印章条款 + 控制写集钉死测试）；**§4d J9+⑤a 实装进准备相**（归档-复验-journal-
  prepareWrites 全链 + 幂等 + 封闭集披露）。
- **§4e 替身**（固定端口环回 HTTP 平台计数/收据 + openai 兼容确定性模型端点 +
  协议 worker 注入）；**§4f 演习矩阵全绿**：D1 38/38（最终二进制绑定复跑）、
  D2 27/27（三注入腿）、D3 26/26（恢复式再前滚 + secondImportZeroGrowth）。
- **七个真缺陷修复**（红→绿链全保留，根因在 PROGRESS-t06 §4f 节）：lineage
  时间戳泄入确定性门 / payload 比对扁平名残留 / READ_ONLY 连接污染封缄源
  （P6 副本纪律落地）/ 空表 arity 误拒 / schema_migrations 簿记漏列 /
  automation_* 族回灌语义缺失（「导入零写入」结论证伪）/ session_files 族
  根作用域路径冲突（恢复根行胜 + pathScopedPreserved 披露）。
- **§4g runbook 交付**（含 manifest 双哈希内部约定披露、queued 续投语义、
  印章条款①、恢复根附件子集方向语义）。
- 最终基线：build 0 警告 / svc-bin 16/16 / adapters 177/177；红线全守
  （无 commit/push、合成 HOME、真实 ~/.lingxi 未触碰、migrate 本体零改动）。
- T06 待独立评审（评审人不得为实现者）；T07/T08 未启动。

## 2026-10-23 续接会话②：T06 评审关闭 + T07 完成 + T08 注册与诚实关闭

- **T06**：独立评审 PASS-with-MINORS（B0/M0/N5；rev-t06-* 8 组全绿）→ CLOSED；N1 台账更正、N2/N3 移交落地（死块删除 + 旧根 READ_ONLY 对称套 P6）并重绑（t06-n2n3-*：三演习全绿）。
- **T07**：A/B 线束（同 React 构建源 × 真实 Pi 格式会话数据 × 双内核 × jsdom UI 绑定层 15 任务：终版 12 平价/0 语义差异/2 recorded/1 B-failed）+ 真实格式探针；**两个真修复**：F-T07-1（阻断级：T04 夹具自造 entryType 格式 vs 真实 Pi SDK 格式——修复前真实数据导入全落 legacy-unknown-block；治理修正：T04–T06 绿证据原本只对自造格式成立，修复后对真实格式闭合且夹具证据不受影响）与 F-T07-2（客户端 10 条会话写端口翻译 + 连接门）。交付 T07_UI_DIFFERENCE_REPORT / FEATURE_PARITY_RESULTS / PROGRESS-t07。
- **T08**：xtask R08 注册（18 命令/16 场景/126 叶=9 份额图钉+117 pending）+ 门工具修复（候选快照容忍工作树删除条目）+ A15/A16 助手 + R08_REPORT（PPG-1/2/3 终裁=预览限定接受+post-R08）+ R08_HANDOFF；最终门 dry-run 运行中，正式证据 verify-r08-final-01。
- **R08 终态（如实）**：实现到位项 READY_FOR_REVIEW；117/126 叶 pending 留债 → 门 overall 非 PASS 是覆盖缺口的诚实表达；未 commit/push，工作区为唯一副本。
