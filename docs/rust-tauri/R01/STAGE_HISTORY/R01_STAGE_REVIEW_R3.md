# R01 阶段独立验收 R3

- 审查者：全新 Codex 子代理 `/root/r01_stage_review_r3`；只读验收，未参与 R01 的执行、T 验收、R1/R2 阶段验收或修复。
- 基线：R00 C4 `328cc8bb5a807bdaad520b459907fb1fa4e10dca`。
- 候选：`codex/rust-tauri-migration` 的本地及远端 HEAD 均为 `363999378482dfed42733a3c4e8ad20ac82fd0fc`；工作树为 R1+R2 修复候选，16 个已跟踪修改、26 个新文件。
- 冻结指纹：`git diff --binary` 串接按路径排序的新文件 SHA-256 清单，开始和结束均为 `beee5117ed3bd0382efa3ac6ac43d353cc37befa56c12ecb6e29e60169808038`。`git diff --check` 通过；主仓未因本轮验证漂移。R2 评审、修复报告 SHA-256 分别为 `727e82f0323fd067b33f49a7c09a44dc494cc58ee5c0697aa5ca490f12f71e82`、`5cddb8c74366d366d2da87803f5bb68566e6e48fdb0c6bcd6d773e7d6b73fc06`，与交接一致。
- **STAGE VERDICT: FAIL。**

## 独立核对的阶段范围

已读取原始任务书的共同约束、R01 目标/8 项任务/16 项 REQUIRED 场景及 stage-index/task-catalog/acceptance-catalog，核对 R00 C4 基线与交接、R01 阶段报告/交接/风险/验收账本/总控进度、T01–T08 的最终独立报告、R1/R2 旧阶段评审与修复报告及实际源码差异。T01–T08 的执行者和最终独立验收者均不同；8 份最终验收报告 SHA-256 与进度账本相符，8 笔任务提交均为当前 HEAD 祖先，推送状态已登记 CONFIRMED。16 项账本 ID 唯一且均为任务级 PASS；A06 故意删除依赖的负向测试退出码 101 是预期拒绝，不能误读为验收失败。HANDOFF 所列 18 个文件哈希均实算匹配（另一个 `note` 是说明字段）；当前阶段状态仍为待复验，未伪称阶段已通过。R00 C4 的 68/68 与 R01 未封印候选的六红已正确分开记账。R00→R01 当前已提交差异 2103 路径；生产入口未接入 R01 原型，隔离检查扫描 2209 个生产文件，零原型引用。

## F01（BLOCKING）：不存在的截止阶段仍被当成有效递延挂账

R01-T08 §4 明确要求**每个高风险缺口设截止阶段与失败处理**，R01 Stage Gate 对跨平台/授权态缺口的放行依赖后续 R09/R10 强制关卡。修复后的 `r01_t08_gate_check.py` 已拒绝字段空值、对象、数组、数字、空白和部分占位值，却只要求 `resolve_by_stage` 是非空字符串，未校验它指向存在的 R02–R11 阶段或冻结的最迟关卡。

独立真实 CLI 复现：只复制 `RISK_REGISTER.json` 到 `/tmp/r01-r3-risk-invalid-deadline.json`，将与 `browser_host.fetch_interception_layer` 冻结绑定的 `RR-T05-X1.resolve_by_stage` 改为 `R99（不存在的阶段）`，其余数据不变；运行 `python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py --register /tmp/r01-r3-risk-invalid-deadline.json --out /tmp/r01-r3-risk-invalid-deadline-report.json`。结果 **exit 0 / PASS_WITH_CONDITIONS**，该项显示 `TRACKED(RR-T05-X1 -> R99（不存在的阶段）)`，总体宣称“15 个递延项已全部挂账（截止阶段+失败处理）”。R99 不存在于任务书 stage-index，也没有可执行的后续验收关卡。此输入使一个重要浏览器边界风险失去实际截止期，却仍允许下一阶段。

根因是 `_meaningful_str()` 只能识别字段类型与少数完整占位词，不能验证阶段坐标。修复需将递延项可接受的截止阶段绑定真实阶段索引及适用最迟阶段（可从结构化字段或明确冻结契约验证），拒绝 R99、纯“以后”、越过应有最迟关卡等反例；保持现有 29 项进程内与 16 项真实 CLI 回归，并在真实 CLI 增加该类负向。不得仅靠正文出现 `R09` 字样冒充有意义的截止期。由新 ZCode 阶段修复任务处理，再由另一全新 Codex 子代理完整复验。

## F02（正式封印/推进阻断）：全量测试仍未获得受控绿色结果

R2 修复报告明确披露 `/tmp` 预演纯审计树的 `npm test` 为 **14946 passed / 2 failed / 15 skipped**。本轮独立检查完整日志 `/tmp/r01-repair-r2/logs/npm-test-rerun.log`：两红均为 round2/round3 的现场补丁重放，失败守卫精确指出 `firstDiff=["build/cli-runtime-closure.json"]`；并在该隔离树复算 `git diff`，确认现机重新生成的闭包多 `/bin/bash` 一项（14 行新增、3 行删除，8949→8950 文件）。单独重跑 `tests/cli-closure-census.test.ts` 是 22/22 绿，**但其运行前隔离树的闭包文件已经漂移**，所以它的绿不能证明原封印提交的干净树在当前环境可得到全量绿色。

R2 报告的三个历史树对照支持“环境变化而非 R01 新增源码引起”的归因；本轮未推翻该归因。可归因不等于已通过：现有全量测试在同一树/环境下仍是 exit 1，缺可验证的正式受控 0 失败流程。任务书 `05_验收与性能协议.md` 列出 `npm test` 为相关基线回归，现行 `PROGRESS.md` Seal 流程及 R00 C4 提交后复验均以全量 0 失败为最终证据。不能把 74/74 的四文件预演或干净外观替代完整 `npm test`。正式收口前需要在无漂移环境实跑完整绿色，或由有权总控对闭包生成环境/冻结数据作可审阅治理，重新绑定实际提交并跑全量 0 失败；不得删掉 census 测试、放松树净守卫或把两红记绿。本项与 F01 独立阻止“已完成正式封印/可进入 R02”的宣告。

## 已通过的独立复验矩阵

| 范围 | R3 实测或证据核对 |
|---|---|
| R01-A01/A02 所有权与双 owner/桌面依赖负向 | `r01_t01_check_ownership.py --self-test` exit 0，N1–N15 拒绝；最终 T01 R3 报告哈希匹配。 |
| R01-A03 跨语言 | `r01-t02-roundtrip.sh` 独立 target：12 golden Rust→TS→Rust 字节一致，tsc 通过，exit 0。 |
| R01-A04 版本边界 | `r01-t02-handshake.sh` 独立 target：HTTP/WS 5 场景，过新/过旧明确拒绝，exit 0。 |
| R01-A05/A06 锁依赖与缺依赖 | T03 最终独立报告及证据仍在；本轮未重做破坏性缺库探针。R2 轮独立 Rust workspace 45/45 证据可核。 |
| R01-A07/A08 浏览器 | T04 最终独立报告/接管与隔离证据在；本轮 A15 真实 CLI 对 `user_takeover` 删项/失败负向仍拒绝；未重启本机浏览器原型。 |
| R01-A09/A10 PDF | T05 最终独立报告及长文档/危险资源/超时/WS 证据在；门禁证据哈希实算通过；未重渲染 PDF。 |
| R01-A11/A12 Tauri | T06 最终独立报告与测试/release 区分证据在；`npm run lint` 实跑 exit 0、0 errors/10896 warnings；eslint 仅为原型真实 Node/browser 环境声明，`probe.js` 一处等价改写；未重建桌面产物。 |
| R01-A13/A14 旧版本拒写/回滚 | T07 最终独立报告及旧二进制/分离根/回滚演练证据在；未接触真实用户数据。 |
| R01-A15 高风险关卡 | 正向 `PASS_WITH_CONDITIONS`；自测 29/29、真实 CLI 1 正+15 负均 exit 0；R1/R2 旧反例已拒；新增 R99 反例仍误放，F01。 |
| R01-A16 覆盖 | `r01_t08_coverage_check.py` 正向 `COVERAGE-CLOSED`，736 F-ID/69 store/14 Pi 项；自测 6/6，exit 0。 |
| 跨 T 生成与隔离 | T02 `--check` 56 文件/624 兼容接口零漂移；隔离扫描 2209 生产文件、零引用；`npm run typecheck` 三段通过。 |
| R2 F03 分片交付 | 6 项 `/tmp` 合成夹具测试本轮实跑 6/6。R2 预演树实物：round2 6 片、267310615 B，round3 13 片、577588418 B；本轮逐片尺寸/哈希与重组 SHA-256 复算全匹配，单体不共存。其 HEAD 可达 14579 个 blob，最大 97003232 B，0 个 ≥100 MiB；裸仓推送日志显示接收 SHA 一致。实物与日志支持 GitHub 单文件尺寸可行性，但正式 GitHub push 尚未发生。 |
| 正式封印 | 预演四文件 74/74，预演 SHA 仅为彩排坐标；当前主仓六处审计坐标尚未推进，正式四文件门禁/全量 `npm test`/远端推送未通过。本轮未在主仓运行会改写交付 gzip 的脚本。 |

## 结论与下一步

R2 的风险登记空值绕过、T06 lint 错误、超限双补丁这三项修复有真实正负测试和隔离预演支持；未见通过删除测试、扩大审计白名单或修改生产入口取绿。**但 F01 使高风险延期可落到不存在的阶段，违反 R01-T08 的可执行截止期要求；F02 使正式封印及 R02 入口尚无全量绿色证据。STAGE VERDICT: FAIL。**当前候选不可标阶段 PASS，不得据本报告提交推送 R01 最终收口或进入 R02。阶段修复继续交全新 ZCode 任务；下一轮由另一全新 Codex 子代理只读复验。正式提交/封印必须绑定真正候选 SHA，完成后复验四文件、全量 `npm test`、交付字节/工作区零漂移和远端 SHA，不能使用本次预演 SHA 代替。

未覆盖：Windows/Linux/macOS x64、授权态系统能力、真实供应商、正式打包/签名、真实用户数据迁移及 GitHub 正式 push；按任务书后续关卡处理，本轮不冒充通过。主仓未修改、未提交、未推送。
