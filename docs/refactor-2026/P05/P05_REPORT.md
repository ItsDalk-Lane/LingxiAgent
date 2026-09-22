# P05 阶段执行报告

## 结论

**PASS（本地验证口径）**。消息语义、历史恢复、资源与数据兼容的现役实现经逐项核实
为正确（8 项任务中 8 项 UNCHANGED_VERIFIED 生产面），本阶段以契约文档收口语义权威、
以 8 个新增测试锚点补齐此前缺失的恢复/资源/负向证明场景；零生产代码改动、零 schema
变更、零迁移。全量测试 14769 绿 / 4 红 = F1 已知基线（同 P04，未扩大、未新增失败项）。
真供应商冒烟 BLOCKED 为 P04 继承项（无凭证/预算授权），不属本阶段验收必需场景。

## 实际输入

- START_SHA = END_SHA = `1f0537b08`（工作区改动，无阶段内提交；提交由编排层统一执行）
- 分支 `docs/knowledge-closeout-2026-09-21`（未切换）；与 origin 同步起点
- Node v24.16.0 / npm 11.13.0 / darwin arm64；lockfile sha256
  `a9735825cea1d018c2a42ae04f875368a003c386ca68fc86503bc8acbcd79d3c`（与 P04 交接一致，未变）
- 进入条件核对：P04 PASS（含 T07-2 单项 BLOCKED 登记）；P02 终态契约、P04 规范化事件、
  P00 冻结的旧历史/分页/资源契约均在手（P04/NEXT_STAGE_HANDOFF + P00 三图）。
  基线漂移：无（HEAD 即 P04 产物）。

## 任务逐项结果

| 任务 | 实施动作 | 生产入口/源码 | 测试与日志 | 旧路径去向 | 状态 |
|---|---|---|---|---|---|
| P05-T01 语义权威确认 | UNCHANGED_VERIFIED + 文档收口 | MESSAGE_SEMANTICS.md §1–§6（normalizer/解析器清点/Run 结局连接/unresolved 策略）+ SEMANTIC_FIXTURES.json（11 类 fixtures→测试映射） | 语义fixtures 全部指向本轮执行过的既有测试 | 无第三套解析器；无旧路径需退出 | PASS |
| P05-T02 规范化事件收口 | UNCHANGED_VERIFIED + 消费者审计 | CONSUMER_MIGRATION.md（批输出唯一消费点；桌面/面板/上游三类消费者逐一分类；text_delta 兼容契约与退出条件） | 消费者等价断言（parity 套件，P05-T07-parity-and-fi） | 旧 text_delta 保留为兼容 adapter（远程客户端/面板精简视图产品契约），退出条件登记 P08 | PASS |
| P05-T03 可解释重连 | UNCHANGED_VERIFIED + 5 新测试 | RECONNECT_CONTRACT.md（协议零新增；语义规则表：seq 流内比较/幂等/reset/truncated/流结束恢复/水位门槛） | 新增：chat-route-switching A03/A04/A05/A06（真实路由×真实 store）；stream-resume 跨流 seq 例；回归 store/protocol/dedupe 全绿 | 无旧协议字段废弃 | PASS |
| P05-T04 跨页 Run 与有界读取 | UNCHANGED_VERIFIED + 实测 | HISTORY_PARITY.md（游标=display 序号读取位置；场景→测试映射；phase B 实测热页 fullFileReadCalls=0、jsonlParseCount 恒 103 与规模无关） | 16+117 例回归全绿；bench phase B 全断言过 | all=1 仅显式请求，无全量加载逃避 | PASS |
| P05-T05 资源身份与安全预览 | UNCHANGED_VERIFIED + 1 新测试 | RESOURCE_HISTORY_CONTRACT.md（三类引用身份模型；预览≠全文唯一副本；逐块授权；文件状态语义表） | 新增 A11 真实磁盘对照例；资源面 9 文件回归全绿 | PDF/媒体链未触碰（现状正确） | PASS |
| P05-T06 数据兼容 | 无变更证明 | DATA_COMPATIBILITY.md（8 类数据版本表；**无 schema 变更、无迁移**；A14/A15 机制证据；指纹守卫 170 watched 零触碰；knowledge research 残留决策=保留只读兼容） | data-epoch×4 + persistence×2 = 109 例全绿 | 不删兼容读；退役留授权（P08/用户决策） | PASS |
| P05-T07 路由→store 等价 | UNCHANGED_VERIFIED + 负向证明 | LIVE_HISTORY_REPLAY_REPORT.md（等价矩阵：首屏/翻页/断线/重启/导出 × 内容类型） | FI-1（commentary→final 篡改被 parity 捕获）、FI-2（游标误用被页欠填/页数学捕获）全绿；与 parity 套件同进程隔离验证 | 生产函数零 mock | PASS |
| P05-T08 清理与验收 | 审计+门禁+交接 | 无可删旧输出（全部消费者在用，CONSUMER_MIGRATION §5）；P05-3 双身份契约收口 SESSION_IDENTITY_CONTRACT.md | 门禁全绿（下节）；协议/数据版本矩阵在 HANDOFF §2/§7 | 双写检查：无双写事实源（T02 §4 只读对照仅测试内） | PASS |

## 场景逐项结果

P05-A01…A16 全部 PASS，逐场景测试文件/用例/命令/证据见
**ACCEPTANCE_MAP.json**（含 fixture sha256 与生产入口）。

## 接线和旧路径

- 新增测试全部经真实生产入口：createChatRoute WS handlers（resume_stream 真实分发）、
  真实 session-stream-store（不 mock）、真实 projectFullHistoryPage/projector、
  真实 ResourceAccess 授权链外层（A11 在 deferred 解析层）、真实 file-history/资源面。
- 新增消费者：无（无新模块、无只在测试使用的新生产代码）。
- 旧输出去向：WS text_delta 保留（兼容 adapter，退出条件成文）；canonical/legacy
  双链在主聊天有明确优先级不双计；无第二事实源。
- 无隐式回退新增；历史读取降级链（目录→legacy）行为未动。

## 验证（命令、exit code 与原始日志：artifacts/refactor-2026/P05/logs/command-log.jsonl，27 条）

| 命令 | exit | 结果 |
|---|---|---|
| P05-T00-node/npm/lockfile | 0/0/0 | 环境与 P04 交接一致 |
| P05-T03-ws-resume-route（→r2） | 1→0 | 首跑 A05 断言口径错（truncated 语义），修正后 4/4 绿；两次都留档 |
| P05-T03-client-cross-stream（→r3） | 1→1→0 | async/等待修正后绿；过程留档 |
| P05-T03-store-regression | 0 | store/protocol/gate/dedupe 回归全绿 |
| P05-T04-history-parity | 0 | S16 两文件 16/16 绿 |
| P05-T04-directory-suite | 0 | 12 文件 117 绿/4 跳过 |
| P05-T04-bench-phase-b | 0 | 热页工作量断言全过（样本归档 samples/history-bench-b/） |
| P05-T05-a11-tool-history | 0 | 含新增 A11 例全绿 |
| P05-T05-resource-suite | 0 | 9 文件资源面全绿 |
| P05-T06-data-epoch-suite | 0 | 109/109 绿 |
| P05-T06-fingerprint-check | 0 | 170 watched 零触碰 |
| P05-T07-fault-injection（→r4） | 1→1→1→0 | 三次迭代修正（mock hoisting/回退/巧合断言）后 2/2 绿，全部留档 |
| P05-T07-parity-and-fi | 0 | parity 套件 + FI 同进程隔离验证 |
| P05-T08-typecheck | 0 | typecheck×3 绿 |
| P05-T08-gates（→r3） | 1→1→0 | 两次脚本名试错（check:core-contracts→typecheck:core-contracts；compute-cli-closure 非 npm script）留档后全绿：core-contracts / dependency / tool-invocation / cli-closure / lint:boundary / lint(eslint 0 error) |
| P05-T08-export-obs-bridge | 0 | 导出/观测/Bridge 附件 7 文件全绿，无字段丢失 |
| P05-T08-full-suite | 1（F1 基线） | **14769 绿 / 4 红 / 15 跳过**；4 红 = post-verification-audit-seal（VERIFIED_SOURCE_SHA 后非审计改动，含分支既有提交）+ round3 manifest/R10-03/R10-04，与 P04 基线完全同一组，未扩大 |

失败重跑链完整保留（5 组 retry）；无未运行冒充通过的条目。

## 数据、权限与平台

- 数据：零 schema 变更、零迁移、零迁移演练需求（机制回归即演练，DATA_COMPATIBILITY §2）；
  指纹无 repin。
- 权限：未触碰任何权限面；A12 逐块授权语义回归为原状。
- 平台：仅 darwin arm64 本地；四平台 CI/open server/打包验证为 F3/P08 范畴（未执行，非本阶段必需）。

## 差异与限制

- **生产行为差异：无**（零生产文件改动；diff 仅 3 个既有测试文件 +282 行）。
- 未完成/受环境限制：真供应商冒烟（P04-T07-2 继承 BLOCKED）；四平台 CI 证据（F3，
  P08）；均未虚报。
- 现有非关键问题登记：
  1. P04/NEXT_STAGE_HANDOFF §2「五个计量观察点」措辞与 §6 不一致——本阶段未合法触碰
     该文件，按"不为此扩大范围"留待下次合法触碰者（HANDOFF §5）。
  2. ChannelsPanel/SubagentSessionPreview 仍消费 legacy text_delta（有意精简视图），
     若未来要显示过程文本需迁 canonical（CONSUMER_MIGRATION §5，范围外建议）。
  3. benchmark summary-b.md 头部 HEAD/分支取自夹具 environment.json（历史时点），
     非本次执行坐标——已在其文档注明，脚本行为未改（避免范围外改动）。
- 范围外建议：无新增。

## 回退与下一阶段

- 回退步骤：`git checkout -- tests/chat-route-switching.test.ts
  desktop/src/react/__tests__/services/stream-resume.test.ts
  tests/tool-presentation-history.test.ts` 并删除未跟踪的 docs/refactor-2026/P05/、
  artifacts/refactor-2026/P05/。零生产/数据影响，无需数据回退动作。
- 新增数据：无。用户数据零触碰（全部合成 fixture + 临时目录）。
- 交接：NEXT_STAGE_HANDOFF.md（P06 唯一允许修改范围 = 上下文/提示词/人格/记忆/资料
  边界/评测；语义裁决面、恢复协议、分页投影、资源授权面已锁定）。

## 独立验收修复轮（P05-FIXR1，2026-09-22，两项）

来源 = 独立验收 [P05_ACCEPTANCE_REVIEW.md](P05_ACCEPTANCE_REVIEW.md) §6 非阻塞发现 1/2。
零生产代码、零既有测试断言改动；改动面 = FI 测试探针文件的可移植性 + 记录文档。

**问题 #1：P05-FAULT-INJECTION.test.ts 机器绝对路径 + 头注归属矛盾**

- 路径修复：`/Users/<user>/…/LingxiAgent/` 绝对前缀全部替换为相对本文件的仓库相对路径
  `../../../../`（vi.mock×2、importOriginal 类型导入×2、import×8，共 12 处；写法与
  tests/ 同类探针一致，vi.mock 相对路径按测试文件解析，与本仓库 tests/ 内既有惯例同款）。
  修复后文件内 `/Users/` 命中 0 处。
- 头注矛盾裁决（以事实为准）：该文件**确实在默认测试集**——npm test 的 --exclude 不含
  `artifacts/`，vitest 默认 include 覆盖之，P05-T08-full-suite.out:4164 留有真实执行记录
  （2 tests passed）。故头注「不进入默认测试集」为错误一侧，已改记「已并入默认测试集」；
  NEXT_STAGE_HANDOFF §3「已并入默认 vitest 集」表述正确，未改动。
- 可移植性验证（run-logged，exit code 入 command-log.jsonl）：
  - `P05-FIXR1-fi-test-repo-root`：仓库根 `npx vitest run <FI 文件>` → exit 0，2/2 绿；
  - `P05-FIXR1-fi-test-alt-cwd`：进程 cwd=/tmp、`vitest run --root <仓库>` → exit 0，2/2 绿
    （相对导入按测试文件位置解析，与执行 cwd 无关；本机无法替代其他机器/CI 实跑，
    该限制如实登记）。
- 旁证：该文件不在 tsc（root include 仅 desktop/src，test include 仅 tests/）与 eslint
  （全局 ignores 含 `**/logs/**`）覆盖范围，vitest 为其唯一验证面，无门禁面需要复跑。

**问题 #2：ACCEPTANCE_MAP.json A14 测试名归属错位**

- 事实核实：`requires a checkpoint receipt after the prepared phase` 实际位于
  tests/data-epoch.test.ts:109；tests/data-epoch-coordinator.test.ts 无此用例（grep 零命中），
  仅 `runs checkpoint, barrier, migration, validation, and final commit in exact durable order`
  （:252）属 coordinator。两文件均在 P05-T06-data-epoch-suite（exit 0，109/109）执行。
- 修复：该测试名从 A14 的 test_names 移入 supporting 的 tests/data-epoch.test.ts 条目
  （附行号与修正说明）；test_path 与 fixture_sha256（coordinator 文件哈希）不变；测试本身
  零改动。其余 15 个场景条目零触碰。

**记录与清单一致性**

- P05_REPORT.md（本节）、P05_RESULT.json（fix_rounds）、ACCEPTANCE_MAP.json（fix_rounds）：
  补记本轮条目，前轮已记录事实未改写。
- EVIDENCE_SHA256.txt 按既有口径更新：本轮改动条目（FI 测试文件、ACCEPTANCE_MAP.json、
  P05_REPORT.md、P05_RESULT.json、command-log.jsonl）哈希刷新；验收轮产物
  （P05_ACCEPTANCE_REVIEW.md + P05-ACCEPTANCE-* 5 件）与本轮 P05-FIXR1-* 日志补入清单
  （manifest-check.out 按设计不入清单，P04-FIXR1 同款；验收报告 §7 已注明该重生成属
  提交前动作，本轮完成）。路径列同步规范化为全仓相对路径——P02–P04 清单同口径；初版
  docs 条目缺 `docs/refactor-2026/P05/` 前缀，单一 cwd 无法执行 `shasum -c` 全量校验，
  规范化后哈希值与「哈希↔文件」绑定关系不变（仅路径文本补前缀）。
- `P05-FIXR1-json-validate`（exit 0）：ACCEPTANCE_MAP.json / P05_RESULT.json 解析合法。
- `P05-FIXR1-attribution-grep`（exit 0）：FI 文件 `Users/` 残留计数 = 0；A14 测试名实际
  位于 tests/data-epoch.test.ts:109（coordinator 文件零命中），coordinator 实有 A14 用例
  为 :252 的 durable order 例。
- 清单终验 `shasum -a 256 -c`：exit 0，逐条 OK，命令与输出留档
  artifacts/refactor-2026/P05/logs/P05-FIXR1-manifest-check.out（直接重定向，不入 jsonl
  以保清单哈希稳定——P04-FIXR1 同款处理）。
- 验收 §6 发现 3（f1 patch 重写副作用）：本轮未运行全量 npm test（定向运行不触发），
  终态已核对 `artifacts/f1-f12-repair/round2/patches/89bc0b64-to-r01-r10-source.patch`
  未被触碰（git status 无该文件改动）。
- 验收 §6 发现外的 C1 纵深防御缺口：按本轮范围声明，仅登记不处理（归属后续 hardening）。

## 独立验收修复轮二（P05-FIXR2，2026-09-22，一项）

来源 = 复验收 [P05_REACCEPTANCE_REVIEW.md](P05_REACCEPTANCE_REVIEW.md) §7 新发现 R1
（提交/收尾前必须处理）。零生产代码、零既有测试断言改动；改动面 = 首轮验收反例
证据文件的可移植性与预期方向元语义 + 同类文件 + 记录文档。

**R1：首轮验收反例 P05-ACCEPTANCE-counterexample.test.ts 落入 vitest 默认集且恒红 + 6 处机器绝对路径**

- 基线独立复现（`P05-FIXR2-counterexample-baseline`，exit 1）：1 failed | 4 passed，
  C1（跨流迟到事件）红在 `expect(serialized).not.toContain('【迟到旧流尾巴】')`——与
  首轮 RED 证据（P05-ACCEPTANCE-counterexample.log）同一断言，未重跑全量即污染下一次
  npm test 基线信号（4 红 → 5 红）。
- 修复（复验收 §7 最小修复要求选项 (a)，与 FI 文件修复先例同口径）：
  - 6 处机器绝对路径（import×5 + 未使用的 REPO 常量）全部替换/删除，import 改相对
    本文件的仓库相对路径 `../../../../`（与修复后的 FI 文件及 tests/ 同类探针一致）；
  - C1 用例 `it` → `it.fails` 并加注释：演示已登记的纵深防御缺口（use-stream-buffer.ts
    无 streamId 闸门、当前服务端不可达），缺口修复后本用例转红作为回归提示；
    **断言内容零改动**，改的仅是期望方向的元语义（基线 .err 与首轮 RED 日志的失败点
    均为同一污染断言，可交叉核对）；其余 4 例保持普通 `it` 且全绿；
  - 头注「不进入默认测试集」为错误一侧（同 FIXR1 问题 #1 的头注矛盾），改记
    「已并入默认测试集」——npm test --exclude 不含 artifacts/，vitest 默认 include 覆盖。
  - 未给 npm test 增加 `artifacts/**` exclude（复验收明确要求勿做——那会推翻
    FI/HANDOFF §3 已对齐的默认集语义）。
- 同类文件同口径处置（枚举 `find artifacts/refactor-2026 -name "*.test.ts"` 共 3 个，
  复验收预告「应只有 P05 两个」，实多 1 个）：
  - `artifacts/refactor-2026/P04/logs/P04-ACCEPTANCE-counterexample.test.ts`（P04 首轮
    验收交付物）第 4 行含同款机器绝对导入 1 处，同口径改 `../../../../core/model-operation-client.ts`。
    该文件 2 例断言本就全绿（P05 全量套件 11:16 真实执行记录 full-suite.log:3892），
    仅路径可移植化，无 it.fails 需求；P04 EVIDENCE_SHA256.txt 不引用该文件（零失配）。
  - P05-FAULT-INJECTION.test.ts：零改动（grep 复核 `/Users/` 与绝对导入形式 0 命中）。
- 可移植性复核（`P05-FIXR2-portability-grep`，exit 0）：三个文件 `/Users/` 残留 0、
  `from "`/`from '`+绝对、`require("/` 形式 0 命中；相对前缀行计数 = CE 6（import×5 +
  头注字面提及 1）、FI 12（代码 11 + 头注 1，与复验收 §8.1 口径一致）、P04 CE 1。
- 定向复跑（run-logged，exit code 入 command-log.jsonl；本轮未跑全量 npm test，理由同
  FIXR1——f1 patch 重写副作用，终态核对补丁未触碰）：
  - `P05-FIXR2-counterexample-test-repo-root`：exit 0，4 passed | 1 expected fail（5）；
  - `P05-FIXR2-counterexample-test-alt-cwd`：exit 0（cwd=/tmp + --root=仓库），同上；
  - `P05-FIXR2-fi-test-repo-root` / `P05-FIXR2-fi-test-alt-cwd`：exit 0，2/2 绿（旁证，零改动复核）；
  - `P05-FIXR2-p04-counterexample-test-repo-root` / `P05-FIXR2-p04-counterexample-test-alt-cwd`：exit 0，2/2 绿。
  本机无法替代其他机器/CI 实跑（相对路径解析不依赖执行 cwd 已验证），该限制如实登记。

**记录与清单一致性（P05-FIXR2）**

- P05_REPORT.md（本节）、P05_RESULT.json（fix_rounds）、ACCEPTANCE_MAP.json（fix_rounds）：
  补记本轮条目，前轮已记录事实未改写。
- `P05-FIXR2-json-validate`（exit 0）：ACCEPTANCE_MAP.json / P05_RESULT.json 解析合法。
- EVIDENCE_SHA256.txt 按既有口径刷新：本轮改动条目（counterexample 测试文件、
  command-log.jsonl、P05_REPORT.md、P05_RESULT.json、ACCEPTANCE_MAP.json）哈希刷新；
  复验收轮产物（P05_REACCEPTANCE_REVIEW.md + P05-REACCEPTANCE-commands.log——复验收
  报告 §9 声明留作提交前动作，FIXR1 先例同款由修复轮完成）与本轮 P05-FIXR2-* 日志
  补入清单（manifest-check.out 仍按设计不入清单，P04-FIXR1/P05-FIXR1 同款）。
- 清单终验 `shasum -a 256 -c`：exit 0，逐条 OK，命令与输出留档
  artifacts/refactor-2026/P05/logs/P05-FIXR2-manifest-check.out（直接重定向，不入 jsonl
  以保清单哈希稳定——P04-FIXR1/P05-FIXR1 同款处理）。
- 修复后本工作区全量 npm test 预期回到 F1 基线 4 红（R1 的第 5 红消失）；本轮按约束
  未实跑全量，该预期基于 vitest list 成员资格与本轮定向复跑结果推断，如实登记。

