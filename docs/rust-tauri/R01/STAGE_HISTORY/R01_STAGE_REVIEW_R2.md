# R01 阶段独立验收 R2

- 审查者：全新 Codex 子代理 `/root/r01_stage_review_r2`；只读验收，未参与 R01 执行、T 验收、R1 阶段验收或修复。
- 日期：2026-09-26；分支 `codex/rust-tauri-migration`。
- 阶段基线：R00 正式 C4 `328cc8bb5a807bdaad520b459907fb1fa4e10dca`。
- 候选：本地及远端 HEAD 均为 `363999378482dfed42733a3c4e8ad20ac82fd0fc`，其上 9 个已跟踪文件修改、`artifacts/rust-tauri/R01/STAGE_REPAIR_R1/` 下 16 个未跟踪证据文件；未提交。
- 冻结指纹：将 `git diff --binary` 与按路径排序的 16 个新文件 SHA-256 清单串接后取 SHA-256，开始与结束均为 `66e84f0bb0cfd9ef4c4eb9a51a61a2e92649b2113b3ec6b89cbf1460cfe5383e`。修复报告列出的 25 个逐文件 SHA-256 全部实算吻合，`git diff --check` 通过；主仓未因本轮测试漂移。
- R1 评审报告 SHA-256 `0ace64a61fdcff4aa6d15b9eaa890b55ca91b30c3fed2d62ddc77653ec2dc020`，修复报告 SHA-256 `2843c62c867cdbfe0e7aa0f3d4c1f3214359eb5160f9358a942573c40d4959b2`，均与交接一致。
- **STAGE VERDICT: FAIL。**

## 范围和已核实的事实

独立读取用户原始总控要求、共同任务书、R01 阶段规格、`stage-index.json`、`task-catalog.json`、`acceptance-catalog.json` 中 R01 的 8 个任务和 16 个 REQUIRED 场景，以及 R00 HANDOFF、R01 八项任务执行/最终独立验收、阶段报告、交接、验收账本、风险登记和总控进度。比对 R00 C4→当前 HEAD 的 2103 个路径及候选 25 文件；生产目录 `desktop/ server/ core/ lib/ shared/ cli/ hub/ plugins/ skills2set/` 与 `package*.json` 均无改动。实际运行原型隔离检查扫描 2209 个生产文件，零原型引用，结果 `ISOLATED`。

8 个任务在进度账本中均为 DONE，执行者与最终独立验收者 ID 不同，8 份最终验收报告 SHA-256 均实算 MATCH，8 个任务提交均为当前 HEAD 祖先；远端目前仍为 HEAD `363999378…`。T04–T07 的 push 记录已补齐，T08 `358299c1…`、两笔账本提交和远端坐标一致。R01 HANDOFF 中 17 个交付文件哈希全部 MATCH，`source_sha=2bbec6d…` 已明确为 T08 执行前历史快照，另有 `accepted_stage_candidate` 标明当前已推送候选。16 项账本各有 PASS、命令/退出码、证据和复核记录，所列证据文件全部存在。R00 正式 C4 68/68 绿、R01 候选 62/68（6 红）的归因现已按真实基线改正，未再伪称 R00 正式基线四红。F02 的原记录失真已关闭。

R1 的 F01 主要绕过已关闭：加固版关卡在真实输入上判 `PASS_WITH_CONDITIONS`，17/17 自测通过；删整个域、删 `user_takeover`、删哈希、删递延项、改名、重复、证据改指并重算哈希等自测都判 NO-GO。冻结契约独立于被验输入，证据路径和哈希均核验。但对**风险登记值类型**的同根因闭合仍有新的遗漏，见 F01。

## F01（BLOCKING）递延风险的截止阶段和失败处理可用 JSON 空值绕过

- 位置：`docs/rust-tauri/R01/r01_t08_gate_check.py`，`evaluate()` 对递延风险的 `resolve_by_stage` / `failure_handling` 检查。
- 最小复现：只复制 `RISK_REGISTER.json` 到 `/tmp`，将已冻结绑定的 `RR-T05-X1.resolve_by_stage` 改为 JSON `null`，其他数据不动；执行 `python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py --register /tmp/r01-review-r2-null-risk.json --out /tmp/r01-review-r2-null-risk-report.json`。**退出码 0**，总体仍为 `PASS_WITH_CONDITIONS`，`browser_host=COMPLETE`，递延 15 项，`blocking_reasons=[]`。本轮还以调用公开 `evaluate()` 的只读变体实测：`failure_handling=null`、`resolve_by_stage={}`、`failure_handling=[]` 均同样放行；`RR-T05-X1.status=CLOSED` 与输入的递延 `UNVERIFIED` 矛盾时也放行。
- 根因：`str(r.get("resolve_by_stage", "")).strip()` 把 `None` 转成非空的 `"None"`，把 `{}`/`[]` 转成非空字符串；另一字段使用同一写法。修复自测 N16 只把值改成空字符串，因此漏掉合法 JSON 中最直接的“字段存在但无值”形态。递延项虽然与风险 ID 精确绑定，风险条目本身仍能失去有意义的截止阶段/失败处理，门禁却宣称“15 项已全部挂账”。
- 违反要求：R01-T08 §4 要求每个高风险缺口有截止阶段与失败处理；R01 Stage Gate 允许其他平台/授权态延期的前提是 R09/R10 强制关卡确实可执行；R01-A15 禁止用表面证据遮蔽高风险缺口。该反例直接破坏延期放行条件。
- 修复要求：两个字段必须是非空、非空白的字符串，不能以 `str()` 转换非字符串值；校验风险状态与输入的递延状态不矛盾，并检查风险 ID 重复的同类输入。把 `null`、对象、数组、空白字符串及互相矛盾的状态加入负向回归；以实际 CLI 而非只调用内部函数证实 NO-GO，再重跑真实输入正向、A15 全组、A16 及受影响风险交接。

## F02（BLOCKING：阶段回归）R01 新增原型使全仓 lint 保持红灯

- 独立执行 `npm run lint`，退出码 **1**，`61 errors / 10898 warnings`；原始日志 `/tmp/r01-review-r2-lint.log`。
- 61 错分布：`spike/tauri-shell/app/frontend/probe.js` 25（其中一项 `no-unused-expressions`），`spike/tauri-shell/scripts/evidence_server.mjs` 11，`spike/tauri-shell/scripts/probe_webdriver.mjs` 25。三文件由 R01-T06 引入，并在 R01 隔离原型范围内；报告已如实披露这一红灯，但未修复。`05_验收与性能协议.md` §2 将 `npm run lint` 列为现有验证命令，R01 §5 要求相关基线回归。R00 C3 封印绿色电池也包含 lint；R1 封印预演明确剔除了这一项，因此预演绿色不能代替正式绿色。
- 修复要求：按三文件实际运行环境修复 lint 错误，不删除原型或放宽全仓规则取绿；重跑 `npm run lint`、T06 原型相关检查及阶段受影响回归。原有 10898 条 warning 不以数量冒充失败。

## F03（BLOCKING：正式收口与远端交付）现有双补丁封印无法按合同推送

- 独立对 `/tmp/r01-repair-r1/iso` 中预演**实际文件**执行 `stat`：round2 的 `.patch.gz` 为 **267,221,850 B**，round3 为 **577,433,322 B**；修复证据中的 `patchBytes` 与实物一致。当前仓库中对应已跟踪文件仅 29,581,472 B / 89,457,199 B，未配置 Git LFS 过滤器。预演 R1c' 还缺本轮 16 个新增证据文件，正式重冻结只会改变字节及尺寸，预演哈希/SHA 不能直接作为正式坐标。
- [GitHub 官方单文件限制](https://docs.github.com/en/repositories/working-with-files/managing-large-files/about-large-files-on-github)明确普通 Git 文件超过 100 MiB 会被阻止；两份预演产物都远超限制。现有 round2/round3 交付机制若照预演重生成并提交，将无法把封印提交推到当前 GitHub 远端。未实际发起 push，结论基于实物尺寸与平台规则。
- `PROGRESS.md` 的 Seal 工作流要求先把适用验证绑定真实候选源码提交，再推进六处审计坐标，完整复验并在纯审计提交后复核差异门禁。候选态六红本身是坐标未推进的预期结果，**不单独推翻 R01 功能 Gate**；但用户明确要求正式提交后验证、提交并推送 R01 收口，才能进入 R02。修复报告的 `/tmp` 预演 68/68 仅证明方法可能有效，不是正式提交 SHA 的封印通过，也未解决远端单文件限制。
- 修复要求：给双补丁交付建立可审阅、可复放且 GitHub 可推送的方案（例如受控分片并更新相应清单/重放校验；具体形式由新的修复代理取证选择），保留所有校验语义，不扩审计白名单、不退役测试或虚报坐标。绑定实际 R01 候选提交，完成正式证据重冻结、六处坐标、四文件门禁与全量 `npm test`、工作树/交付字节零漂移核对；推送前验证每个 Git 对象满足远端限制，推送后核对远端 SHA。F02 的 lint 红也必须在正式绿色电池前解决。

## 本轮复验矩阵

| 项 | 本轮结果与证据 |
|---|---|
| R01-A01/A02 所有权、无桌面依赖、双 owner 负向 | `r01_t01_check_ownership.py --self-test` exit 0，含 O/D 正向与 N1–N15 负向；任务最终 R3 报告哈希 MATCH；workspace 构建与 metadata 证据仍在。 |
| R01-A03/A04 跨语言和版本握手 | 全新 `/tmp/r01-review-r2-target` 下实际重跑：12 golden Rust→TS→Rust 逐字节一致、TS 类型检查通过；HTTP/WS 五场景包含新版/旧版显式拒绝，均 exit 0。 |
| R01-A05/A06 依赖锁与缺依赖诊断 | T03 执行/独立 R1 报告、两项日志和锁文件证据存在；本轮 workspace `cargo test --locked --offline` exit 0，19 test binary、45 passed/0 failed。未重做缺依赖破坏性探针。 |
| R01-A07/A08 浏览器与隔离 | T04 最终 R1 报告哈希 MATCH，操作/隔离/接管证据存在；关卡对 `user_takeover` 的删项和 FAILED 变体能拒绝。未重启本机 Chrome 原型。 |
| R01-A09/A10 PDF 与危险资源 | T05 最终 R2 报告哈希 MATCH，中文长文档、资源拒绝、超时及 WS 负向证据存在；冻结关卡对 PDF 五项证据哈希实算通过。未重做完整本机 PDF 渲染。 |
| R01-A11/A12 Tauri 权限与 release 区分 | T06 最终 R2 报告哈希 MATCH，双产物/ACL 证据存在；原型未进入生产入口；全仓 lint 反而发现 F02。未重建真实桌面产物。 |
| R01-A13/A14 旧程序拒写/回滚 | T07 最终 R2 报告哈希 MATCH，旧二进制变体、D2 分离根与回滚演练证据存在；已知 corrupt fail-open 明示挂 R02/R09。未访问真实用户数据。 |
| R01-A15 高风险关卡 | 原始真实输入 exit 0 `PASS_WITH_CONDITIONS`、17/17 自测；旧五类删项绕过均被拒；新风险登记空值/非字符串反例仍 exit 0，F01 阻塞。 |
| R01-A16 覆盖闭合 | 实跑正向 `COVERAGE-CLOSED`、736 F-ID/69 store/14 Pi，6/6 自测含 5 负向，exit 0。 |
| 跨 T 协议生成和隔离 | `r01-t02-check-generated.sh` exit 0：56 生成文件/624 兼容项零漂移；`r01_t08_isolation_check.py` exit 0：2209 生产文件扫描零违规。 |
| 审计与交付 | `verify-post-verification-diff.mjs` exit 1（当前 HEAD 超 C3 坐标，按设计）；`upstream-sync-matrix.test.ts` 7/7 绿。候选四文件 62/68、6 红由 R1 与修复报告原始日志一致，但本轮没有在主仓运行会重写交付 gzip 的完整四文件集合；预演 68/68 不计正式通过。`npm run lint` 61 错；完整 `npm test` 未运行。 |

## 结论、未覆盖项和提交判断

R01 的协议/所有权/存储设计、高风险原型和实施平台证据有真实交付；16 项任务级账本与任务级独立验收链完整，F02 的阶段记录修复可信。然而 A15 递延风险检查仍可被空值绕过，且 R01 引入的 lint 错误使相关基线回归失败。**STAGE VERDICT: FAIL。**当前候选不得作为已验收的 R01 阶段收口提交并推进 R02。

形式上，本轮 25 文件本身均小于 GitHub 单文件限制，`git diff --check` 绿；但阶段 FAIL，**不准据本报告提交或推送为最终阶段成果**。正式封印更不具备可推送性：双 gzip 产物超过远端限制，真实提交后的 68/68、全量 `npm test`、零漂移和远端核对尚未发生。下一轮应交全新的 ZCode 阶段修复任务处理 F01/F02/F03，随后由另一全新 Codex 子代理从完整 R01 阶段重新独立验收；本代理不修候选。

未覆盖：Windows/Linux/macOS x64 真机、授权态 TCC 正链路、真实供应商、正式打包与真实数据迁移（按任务书留 R09/R10/R11，并非本轮伪 PASS）；本轮未重做 T04/T05/T06 需本机 UI/进程的完整原型实验，也未跑全量 `npm test`，其已有任务级报告只能作为当时输入的证据，不能代替 F01 修复后和正式封印后的复验。
