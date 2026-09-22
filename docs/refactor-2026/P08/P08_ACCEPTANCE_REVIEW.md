# P08 独立验收报告（P08-ACCEPT）——旧路径退出、全产品回归与发布准备

日期：2026-09-22｜验收者=独立验收子代理（全新上下文，与执行者无共享会话；只读验收，不改生产代码/测试/交付物，仅新增本报告与 logs/P08-ACCEPT-* 证据）｜候选 HEAD `674b0151f`（零生产 commit，改动全在工作区）。

## 结论

**PASS（验收通过；无本轮验收否决项）。** 执行轮自报的关键声称经独立复跑/复算全部成立：零生产代码改动属实；T03 32/32、T05 产物冒烟 7/7 与升级/回退演练 14/14 独立复现；T04 本地门禁（typecheck×3 / lint 0 error / 边界×3 / core-contracts 正负 / open build+smoke）独立复跑全绿；全量 npm test 独立复跑 4 红=F1 已知基线逐名一致（14797 绿与 P07 完全一致）；BLOCKED 5 项均为真实环境/授权受限，未发现以 BLOCKED 掩盖 FAIL 或偷懒未做。执行轮交付物中的三处**非阻塞证据文档瑕疵**见 §4（计数/归属笔误，均不改变任何门禁结果）。

## 1. 逐项独立核验（方法=不信任执行者总结，直接读 diff/源码/原始日志并复跑）

| # | 验收点 | 独立核验方法 | 结果 |
|---|---|---|---|
| 1 | 「零生产代码改动」与 12 个 tracked 修改 | `git diff HEAD` 逐文件审：全部位于 docs/refactor-2026 与 artifacts/refactor-2026；lib/server/core/desktop/hub/shared/tests/package.json/tsconfig 零 diff；diff 外无修改文件 | **属实**。6 个 P07 bench 工具改动=run-id 后缀 + 注释；P04 手接措辞与 P06 计数 5→6 更正均单行最小 diff、带 P08 注记，且以既有记录为据（P06-FIXR1 在 command-log.jsonl 实测 6 条，P04 USAGE_OWNERSHIP §1 实测 8 行表） |
| 2 | skills2set 删除仅 pycache | `git ls-tree HEAD` 证实 lingxi-plugin-creator 目录**零 tracked 文件**（纯未跟踪残留）；cleanup 日志证实删除物=2 个 .pyc；4 个 tracked 技能目录与 21 个 tracked 文件原样；tests 中同名引用均为 makeTmpRoot 合成 fixture；技能测试独立复跑 5 文件 51/51 绿 | **属实**（AGENTS.md「改 skills2set 影响产品行为」红线未被触碰；且产物冒烟证实包内技能同步正常） |
| 3 | P07-F-B run-id 落地 | 读 6 工具 diff（模式一致：默认后缀 + `--out` 保留）+ **bench-memory.mjs 验收者复跑**：输出落 `memory-p07-20260922081301.json`，原 `memory-p07.json` 哈希 11a4ff3d… 前后一致；执行轮自己的 trim 复跑样本（074319）同样在档 | **属实**（两个工具双重验证不覆盖） |
| 4 | 三份前序清单补 pin 不破坏口径 | 仓库根 `shasum -c`：P04 docs 70 条全 OK、P06 docs 11 条全 OK、P07 logs 147 条全 OK（exit 0） | **属实** |
| 5 | FINAL_CALLSITE_MATRIX 锚点 | 11 个锚点行号逐一 `sed -n` 比对当前源码：InputArea:1814/chat.ts:2672/hub:213/desktop-session-submit:385/session-coordinator:5176/engine:4119/gateway:469/bridge-manager:1995/2124/scheduler:163/168/pi-sdk:78/RESIDENT_CORE_TOOL_NAMES 四项——全部命中宣称符号；E-DESKTOP 深链 5 锚点与 P00 agent-callsite-raw.md/ENTRYPOINT_MATRIX.md 逐字一致；核心执行链 8 文件在 92c6646c5..674b0151f 零改动（git diff --stat 空） | **属实**（注：P00 CALLSITE_MATRIX.json 自身有 2 处陈旧锚点——composer-send.ts:577 旧文件名、scheduler :126/:194/:442 定义位——系 P00 文档瑕疵，P08 按当前源码重定位正确，见 §4-③） |
| 6 | LEGACY_EXIT_LEDGER 5 退出/6 保留/3 不退出 | 源码互证抽查：L1（cli/local-server.ts:3-11 仅 import 权威实现）、L2（vitest.config.js 无 @hana 别名）、L3（5 文件 import mintTaskId）、L5（first-run.ts:263 无 SKILL.md 跳过）、L6（knowledge-store research 表族在位无运行时入口）、P03 保留项（prepareAndInvokeForLocalDeveloper 全仓仅定义处，无生产路由注册）；保留项均有成文理由+退出条件 | **属实** |
| 7 | T04 门禁 | typecheck×3、tool-invocation-boundaries(2246 files)、core-contracts(6 files)、dependency-boundaries(2027 files)、lint（0 error/10797 既有 warning）、build:server:open、smoke:server:open（正向 200+负向 exit 1 attributable）全部独立复跑 exit 0 | **全部复现绿**（F3 闭环成立：open build+smoke 在本轮网络下真实成功） |
| 8 | 全量 npm test 4 红=F1 同组 | 验收者独立全量复跑（89.17s）：exit 1，4 failed/14797 passed/1 expected fail/15 skipped，4 红=audit-seal allowlist + round3 manifest + R10-03 + R10-04，与 P07 ACCEPT-R2-full-test.out 逐名逐数一致；**f1 patch 复跑中被重写（25fb315f→23a70044，已知现象）已 `git checkout` 还原为 25fb315f 并复核** | **属实**（无第 5 红） |
| 9 | T03 32/32 | 验收者独立复跑 product-regression.mjs（真实 spawn 全量组合 server + witness）→ 32/32 PASS exit 0 | **复现** |
| 10 | T05 产物冒烟 7/7 | 验收者从 dist seed tar.gz **重新提取**至自建临时树后复跑 packaged-server-smoke.mjs → 7/7 PASS exit 0；asar/seed×4 哈希独立重算与 PACKAGED_ARTIFACT_MATRIX 逐一相符；dist/mac-arm64/Lingxi.app 769M 在位（0.29.4 DMG 为 8 月旧产物非本轮） | **复现** |
| 11 | 升级/回退演练 14/14 + worktree 基线真实性 | 8037fae7a 为真实 git 对象（chore: 审计封印推进 commit）；验收者**自建同名 worktree（8037fae7a+node_modules）自跑演练** → 14/14 PASS exit 0，用后 worktree 已移除（git worktree list 复核仅主树） | **复现** |
| 12 | T06 负向探针 | 独立复跑 negative-probes.mjs → 4/4 PASS exit 0 | **复现** |
| 13 | strict any 增量账本 | 全轮生产 diff `+any` 行复算=4（update/complete/fail options×3 + `const error: any = new Error(message)`×1）；4 处均在宽松区文件、无 gate 逃逸 | 数量属实；**第 4 处文件归属标注错误**（见 §4-①） |
| 14 | BLOCKED 5 项合法性 | ci.yml 实测 `on: pull_request→main` 唯一触发、本分支无开放 PR（gh 实测）、gh 认证可用且历史 run 在列（34551812195 仅历史参考如实标注）；F1 需候选提交（零 commit 约束下不存在）+PROGRESS.md 流程授权；真供应商无凭证（P06 budget_authorization_record.real_model_runs=BLOCKED 在册）；GUI 无授权（P07 同款）；他平台/安装器/OTA 无实机无发布授权（pack 日志实证公证缺 APPLE_APP_SPECIFIC_PASSWORD 按预期 exit 1） | **全部合法**，无掩盖 FAIL |

## 2. 验收者自建反例（≥2 要求，实际 3 组，全部真实执行）

| # | 反例 | 构造 | 结果 |
|---|---|---|---|
| CE-1 | 会话 JSONL **中部**损坏边界（超出本轮 FAULT 的尾部 120 字节截断） | 3 会话各 1 轮真实聊天+operate 设置 → 会话 B JSONL 中部覆写 64 字节非 UTF8 垃圾 + 最旧会话 A 截断至 1/3（半行）→ 同 HOME 再启动当前 HEAD | 6/6 符合预期：就绪 health 200（无半激活/崩溃循环）、3 会话无静默清空、未受损会话历史完整（含真实回复）、设置保持 operate、观测可查。证据：logs/P08-ACCEPT-ce1-*.{mjs,out,json} |
| CE-2 | seed 篡改/坏签名边界 | 走 shared/artifact-core 同一验签原语四例：正确重签（阳性对照）/manifest 内容篡改不重签/归档字节篡改（manifest+sig 有效）/密钥不在 keyset；另跑真实入口 scripts/verify-seed-kit.mjs | 4/4 符合预期：TS 无关；篡改必被签名（signature verification failed）或内容哈希（mismatch=true）拒绝，未知密钥 fail-closed（verify-seed-kit 真实 exit 1）。**产物安全边界非装饰性**。证据：logs/P08-ACCEPT-ce2-*.{mjs,out,json}、P08-ACCEPT-ce2d-unknown-key.out |
| CE-3 | strict 负例独立构造（不借用仓库既有 a05/a06 fixture） | 自建无逃逸 fixture（裸字符串赋给品牌类型 ModelCallId，零 cast）经 extraFiles 注入真实 runCoreContractsTypeCheck | 注入后 TS2322 指向 fixture 且仅 1 诊断、去 fixture 0 诊断——A03「错误必失败」成立。附注：带 `as never` 逃逸 cast 的变体 0 诊断为 TS 合法语义而非检查器缺陷。证据：logs/P08-ACCEPT-ce3-strict-negative.out |

## 3. 未验证边界（如实）

- 四平台候选 SHA CI（A06）：与执行轮同因——ci.yml 仅 PR 触发且无开 PR 授权，验收轮同样不越权触发。
- 真供应商/真模型、GUI 层、他平台实机、安装器形态（DMG/NSIS/AppImage）、产物级 OTA：无凭证/环境/授权，验收轮未验证（协议/套件/产物冒烟层已独立覆盖）。
- 生产签名+公证链：本地 ad-hoc+临时密钥形态已核，生产凭据链不在本机。

## 4. 非阻塞发现（证据文档瑕疵，不改变任何门禁结果；建议随编排层统一提交时一并最小更正）

1. **STRICT_FINAL_SCOPE.json 宽松区账本第 4 处 any 文件归属错误**：`const error: any = new Error(message)` 实际位于 `server/hono-helpers.ts:37`（P01 e2392f301 引入），账本误写为 lib/task-registry.ts「:788 附近」（该文件仅 635 行且无此行）。数量（4 处）与宽松区定性均正确，无未入账增量。最小修复：账本该条文件名/行号更正并带验收注记。
2. **命令计数笔误**：P08_REPORT/P08_RESULT/FINAL_RESULT 写「33 条」command-log，实际 34 条（`wc -l`；执行轮 34=33+P08-T07-manifest-check 收尾追加，报告先行）。同类：INDEPENDENT_REVIEW「P06 12 条」实为 11 条哈希行（12 行含头注）。均不影响日志本身完整性。
3. **「锚点行号与 P00 逐字一致」措辞过强**：对 E-DESKTOP 深 5 锚点成立（与 P00 raw log/ENTRYPOINT_MATRIX 逐字一致，已核）；但 P00 CALLSITE_MATRIX.json 自身两处锚点陈旧（composer-send.ts:577 旧文件名——P00 基线时该文件已名 composer-send-coordinator.ts；scheduler 用定义位 :126/:194/:442 而 P08 用 onBeat 位 :163/:168，两者皆真）。属 P00 期文档瑕疵被 P08 正确重定位，建议未来措辞改为「与 P00 raw 证据一致，CALLSITE_MATRIX.json 两处陈旧锚点已按当前源码重定位」。

## 5. 验收轮操作合规声明

- 全程未修改任何生产代码、测试、执行轮交付物；未 commit/push/PR/tag/发布；未调用真实账户/外发。
- 写入仅限本报告与 artifacts/refactor-2026/P08/logs/P08-ACCEPT-*（92 模板允许的验收轮位置）；两份 EVIDENCE_SHA256.txt 按惯例带验收轮注记刷新。
- 验收复跑副作用处置：全量 npm test 重写 f1 patch 后已 `git checkout` 还原（25fb315f 复核一致）；自建 worktree/临时 seed 提取树/演练 HOME/临时密钥均用后即删（git worktree list 复核）；新增未跟踪样本 memory-p07-20260922081301.json 为 run-id 行为验证副产物，与执行轮 074319 样本同性质，保留为证据。
