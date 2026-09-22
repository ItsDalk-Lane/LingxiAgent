# P08 阶段执行报告｜旧路径退出、全产品回归与发布准备（本轮收官）

## 结论

**PASS_WITH_BLOCKED_ITEMS。** 7/7 任务完成交付物；14 验收场景 12 PASS + 1 带限定 PASS（A07：macOS arm64 --dir 形态，安装器形态与其它平台 BLOCKED 已单列；未计入 pass=12 桶）+ 1 BLOCKED（A06 四平台 CI）+ 0 FAIL——12+1+1=14（P08-FIXR2 精确化：原「12 PASS + 1 BLOCKED」分母悬挂，A07 系带限定 PASS 未入任何桶）；A04/A07 含如实标注的受限层，其协议与套件层全绿。旧路径退出核对、strict 收口、全产品协议级回归（32/32）、本地工程门禁全绿（全量 npm test = F1 已知基线同一组 4 红，未扩大）、本地打包 + 干净安装产物冒烟（7/7）、旧数据升级/故障注入/回退演练（14/14）、独立负向复核（反例 10 组 + 负向探针 4/4）全部真实执行。BLOCKED 项为：四平台候选 SHA CI 证据（ci.yml 仅 PR 触发，无授权开 PR）、F1 封印推进（需候选提交存在 + PROGRESS.md 流程授权）、真供应商/真模型（无凭证授权，继承）、GUI 层/其它平台实机/安装器形态（无环境/授权）。**据此本轮整体为「实现与本地验收完成，四平台远程证据与封印推进待授权」——不写『全部完成』，正式发布准备不得标完成（任务书 §10）。**

## 实际输入

- START = END 候选 = `674b0151f`（P07 提交后 HEAD；本阶段零生产 commit——改动 = 1 个未跟踪残留目录删除（skills2set/lingxi-plugin-creator，git 未跟踪）+ P07 六个 bench 工具 run-id 修复 + P04/P06 三处证据笔误更正 + P08 证据目录未跟踪，等待编排层统一提交）
- 分支 `docs/knowledge-closeout-2026-09-21`（未切换）；Node v24.16.0 / npm 11.13.0 / darwin 27.0 arm64；lockfile `a9735825cea1d018`（P00→P08 全程一致，零依赖变动）
- 研究基线 `8037fae7a`（=origin/main merge-base）；本轮生产改动全集 36 文件（`92c6646c5..674b0151f`，LEGACY_EXIT_LEDGER §0）

## 任务逐项结果

| 任务 | 实现 | 生产入口/源码位置 | 测试/日志 | 旧路径去向 | 状态 |
|---|---|---|---|---|---|
| T01 旧路径退出核对 | UNCHANGED_VERIFIED 复核 + 1 残留删除 | P00 十入口逐一重定位（FINAL_CALLSITE_MATRIX） | P08-T01-* 命令族 + 边界门禁 3 项 exit 0 | L1-L5 已退出/收口；L6-L11 兼容保留（理由+退出条件逐项）；skills2set pycache 残留删除（P08-3） | PASS |
| T02 类型/接口/测试债务收口 | UNCHANGED_VERIFIED + 审计 | tsconfig.core-contracts 6 文件 + CI 接线 | core-contracts 正/负双跑 exit 0；any 增量审计 4 处入账本 | 死别名/恒真条件/taskId 自铸均已在 P01-P03 退出；R10-09 改造评估=不越权，登记 | PASS |
| T03 全产品功能回归 | 新增真实回归工具 | 全量组合 server + witness（E-DESKTOP 同入口） | P08-T03-product-regression-r6：32/32 探针（r1-r5 工具缺陷首败链全留档） | 无新功能/UI 改动（diff 佐证） | PASS（GUI/真账户层 BLOCKED 如实标注） |
| T04 完整工程与平台检查 | 本地全门禁 + CI 取证 | — | typecheck/lint/build:renderer/边界×3/全量 test/open build+smoke 全记录 | F3 网络阻塞解除（open build+smoke 首次真跑成功） | PASS（本地 darwin arm64 一腿）；四平台 BLOCKED |
| T05 打包/安装/升级/回退 | 新增打包与演练工具 | dist/mac-arm64/Lingxi.app + seed 提取树 | pack（公证步按预期缺凭据 exit 1→SKIP_NOTARIZE 复跑 0）；产物冒烟 7/7；升级演练 14/14 | 零版本号/指纹/封印改动；指纹守卫 170 watched OK | PASS（mac arm64 --dir 形态；安装器/他平台 BLOCKED） |
| T06 独立负向复核与范围核对 | 新增反例集 | 真实入口×4 探针 + 十组反例 | P08-T06-negative-probes(-r2)：4/4；INDEPENDENT_REVIEW.md | 排除项未重引入（7 项核对） | PASS |
| T07 最终交付包 | 本报告 + 结果 JSON + 清单 | — | json-validate 10 文件 OK；lint-r2 0 error | P07-F-B 移交项落地（run-id 后缀 + tools/samples 入清单） | PASS |

## 场景逐项结果

见 `ACCEPTANCE_MAP.json`（A01-A14：12 PASS + 1 带限定 PASS（A07，summary pass=12 桶未含）+ 1 BLOCKED + 0 FAIL = 14，P08-FIXR2 精确化分母口径；含命令 ID 与证据路径逐条）。

## 接线和旧路径

见 `FINAL_CALLSITE_MATRIX.json`（十入口当前 SHA 复核 + 第二事实所有者扫描零新增）与 `LEGACY_EXIT_LEDGER.md`（5 退出 + 6 兼容保留 + 3 明确不退出 + 隐式回退扫描零命中）。

## 验证（命令 34 条全留档 command-log.jsonl；首败-重跑链。P08-FIXR1 更正：原记 33 条为计数笔误，实 34 条 = 33 + P08-T07-manifest-check 收尾追加，报告先行）

| 命令族 | exit | 说明 |
|---|---|---|
| P08-T04-full-test | 1（预期） | 4 红 = F1 基线同一组；14797 绿/1 expected fail/15 skipped（与 P07 完全一致）；f1-f12 patch 重写已还原（25fb315f） |
| P08-T03-product-regression → r6 | 1×5 → 0 | r1 工具 sleep 缺陷 / r2 --out 解析缺陷+5 探针形状 / r4-r6 fork target 形状——全部为复核工具自身缺陷，产品行为按其 API 契约正确拒绝；终态 32/32 |
| P08-T05-pack → pack-dir-local | 1 → 0 | 公证步需 Apple 生产凭据（按预期拒绝）；SKIP_NOTARIZE=true（仓库既定本地路径）复跑完成本地测试产物 |
| P08-T07-lint → r2 | 1 → 0 | P08 工具未显式导全局 45 error → 修复后 0 error（negative-probes 重跑 4/4 佐证纯绑定等价） |
| 其余 23 条 | 0 | typecheck/边界/core-contracts 正负/renderer/open build+smoke/升级演练/产物冒烟/负向探针/fingerprint/hashes/cleanup/json-validate/skills 回归 |

## 数据、权限与平台

- 零 schema 变更、零迁移（P05 DATA_COMPATIBILITY 全表有效）；指纹守卫 170 watched 零触碰；pinned-keyset 只追加红线未触碰。
- 全部测试/演练用隔离 LINGXI_HOME + 合成数据 + 本地 witness 供应商；未读写真实用户 HOME/会话/凭证；真实账户零消耗。
- 签名：app=ad-hoc（本地测试件），seed=一次性临时密钥 p08-local-ephemeral（用后即删，不冒充生产签名）。
- 平台：darwin arm64 本地全覆盖；四平台 CI 待触发（清单见 FINAL_ENGINEERING_CHECKS）。

## 差异与限制

1. 四平台候选 SHA CI 证据 BLOCKED（ci.yml 仅 PR 触发；待授权开 PR 后四腿自动跑——注意 npm test 步骤会复现 F1 4 红直到封印推进）。
2. F1 封印推进 BLOCKED（需编排层统一提交产生候选提交后按 PROGRESS.md 执行；本阶段不为绿虚报坐标）。
3. GUI 层（桌面点击/截图/浏览器真机/Electron 窗口）、真供应商/真模型、安装器形态（DMG/NSIS/AppImage）、产物级 OTA：BLOCKED/NOT_ATTEMPTED，原因与解锁条件逐项在册。
4. 继承登记并移交：H1 stream-store trim 热点（P05 锁定面，需显式解锁）、P05 C1 streamId 闸门、R10-09 波动性与补丁重写行为、lib/task-registry.ts 存量 any。
5. 本阶段合法更正三处前序证据笔误（P04 措辞、P06 计数 5→6、P07 tools/samples 清单补 pin）——全部带 P08 注记与清单头注，历史 PASS 证明未改写。

## 回退与下一步

- 回退：本阶段无生产代码改动（除 P07 工具 run-id 修复与残留目录删除）；`git checkout -- artifacts/refactor-2026/P07 tools 六文件 + docs/refactor-2026/P0{4,6}` 三处即完全还原 P08 前状态；dist//dist-server/ 为 gitignore 构建产物可整体删除；临时密钥/worktree/提取树已清理（P08-T07-cleanup）。
- 新增数据保护：全部演练 HOME 用后即删；真实用户数据零触碰。
- 下一步（均需用户明确授权，任务书不代替）：① 编排层统一提交 → ② F1 封印推进（全量门禁绑定候选提交）→ ③ 开 PR 取回四平台 CI 证据 → ④ 按发布列车（release:preflight/build.yml）走正式发布。发布前保持旧可用产物与数据兼容说明（任务书 §9）。

交付后停止。

## 独立验收修复轮（P08-FIXR1，2026-09-22，三项）

来源：P08_ACCEPTANCE_REVIEW.md §4 非阻塞发现 ①–③（验收 PASS 无否决项，本轮仅证据文档最小更正，零生产代码/测试改动）。

**① STRICT_FINAL_SCOPE.json 第 4 处 any 归属笔误（更正）**
- 实测：`const error: any = new Error(message)` 位于 `server/hono-helpers.ts:37`（httpJsonError 内），`git blame -L 37,37 e2392f301` 命中——P01 e2392f301 随该文件新建引入；`lib/task-registry.ts` 全文 635 行且无此行（`wc -l` + grep），初版账本「:788 附近，P02 清理错误上报」系归属笔误。
- 更正：宽松区账本拆两条——lib/task-registry.ts（options ×3，:169/:197/:218，P02 9a19740ba，count 3）+ server/hono-helpers.ts（error ×1，:37，P01 e2392f301，count 1）；scan_method 原口径复算仍 4 命中（分归两条），总数 4 与宽松区定性不动。基线 d5275e568 复核：其余 4 处同名行（activity-hub:74 / deep-memory:33 / session-coordinator:948 / session-manifest/ref:28）均基线既有，非本轮增量——无未入账增量的结论维持。

**② 两处计数笔误（更正）**
- command-log.jsonl：`wc -l` + 逐 command_id 枚举（node 解析全部合法 JSONL）→ 执行轮 stage_id=P08 共 **34** 条（33 + P08-T07-manifest-check 收尾追加，报告先行）。更正：本报告验证节标题 33→34、「其余 22 条」→「其余 23 条」（1+6+2+2 显式 + 23 = 34）；P08_RESULT.json / FINAL_RESULT.json commands 同步。验收轮 P08-ACCEPT 19 条另计，不在该数内。
- P06 清单：`docs/refactor-2026/P06/EVIDENCE_SHA256.txt` = 1 行头注 + **11** 条哈希行（`grep -c '^[0-9a-f]{64}'` = 11）。更正：INDEPENDENT_REVIEW §2「P06 12 条」→ 11 条（12 为含头注总行数）；P08_RESULT / FINAL_RESULT T07 evidence「P06(12)」→「P06(11)」。

**③ 锚点措辞精确化（更正；与验收描述的分歧如实登记）**
- 措辞：FINAL_CALLSITE_MATRIX.json note「锚点行号与 P00 完全一致」与 E-DESKTOP p08_verification「锚点行号与 P00 逐字一致」→ 精确化为「核心链文件（92c6646c5..674b0151f）零改动、行号未移位；深 5 锚点（InputArea.tsx:1814 / chat.ts:2672 / hub/index.ts:213 / desktop-session-submit.ts:385 / session-coordinator.ts:5176）与 P00 原始证据（agent-callsite-raw.md / ENTRYPOINT_MATRIX.md）逐字一致」+ 锚点选取差异登记。
- 分歧登记（以实测为准，不虚报）：验收 §4-③ 称 P00 矩阵「composer-send.ts:577 旧文件名——P00 基线时该文件已名 composer-send-coordinator.ts」**不成立**——`git ls-tree 92c6646c5` 证实 `desktop/src/react/components/input/composer-send.ts`（617 行）与 `desktop/src/react/services/composer-send-coordinator.ts` 基线即两文件并存；composer-send.ts P01-P07 零改动，基线与当前 `:577` 均为 `ws.send(JSON.stringify(prepared.wsMsg));`，与 P00 raw log 逐字一致。故按「同一链路不同段位的锚点选取差异（本表锚租约协调器 :583，P00 锚 ws.send :577）」而非「陈旧锚点重定位」表述；scheduler（P00 定义位 :126/:194/:442 vs 本表 onBeat 位 :163/:168）两者皆真，同为选取差异。措辞过强的更正本身仍执行（初版表述对全部锚点一概而论，E-CRON/composer 段两表锚点确不相同）；P00 历史文档与 P08_ACCEPTANCE_REVIEW.md 按历史证据不改写原则不动。

**记录与清单一致性**：P08_RESULT.json（fix_rounds）与 ACCEPTANCE_MAP.json（fix_rounds）补记本轮条目，前轮事实不改写；EVIDENCE_SHA256.txt（docs + logs 两份）按既有口径刷新——docs 清单本轮改动条目哈希刷新，并补入头注已声称但缺失的 P08_ACCEPTANCE_REVIEW.md 条目（对齐 P06 惯例与自身头注口径）；logs 清单补入 P08-FIXR1-* 日志并刷新 command-log.jsonl 哈希；manifest-check.out 不入清单（P04–P06 同款自指豁免）。

**复核（run-logged，exit code 见 command-log.jsonl P08-FIXR1-*；输出留档 logs/）**：

| 命令 | exit | 结果 |
|---|---|---|
| P08-FIXR1-any-scan | 0 | scan_method 原口径复算 4 命中；hono-helpers.ts:37 / task-registry.ts:169,197,218 sed 实测；task-registry wc -l=635 |
| P08-FIXR1-counts | 0 | command-log 逐 ID 枚举执行轮 34 条；P06 清单 1 头注 + 11 哈希行；P04 70 / P07 logs 147 复点 |
| P08-FIXR1-anchor-evidence | 0 | git ls-tree 92c6646c5 双 composer 文件并存；基线与当前 :577 均 ws.send；scheduler 六个行号均在位；深 5 锚点 raw log 逐字命中 |
| P08-FIXR1-json-validate | 0 | P08 docs 目录 8 个 JSON（含本轮改动 5 个）全部解析合法 |
| 清单终验 `shasum -a 256 -c`（直接重定向，不入 jsonl 以保清单哈希稳定） | 见 P08-FIXR1-manifest-check.out | 逐条 OK 与 exit code 留档该日志 |

未跑全量 npm test 与 npm run pack（约束：耗时与 f1 patch 重写副作用）；f1-f12 patch 前后哈希核对一致（round2 f1 = 25fb315f，与 HEAD 零 diff）。

## 终验收随手更正轮（P08-FIXR2，2026-09-22，微型三项）

来源：P08_RECLOSURE_REVIEW.md §6「本轮新增发现」①–③（终验收 PASS_WITH_SEAL_CONDITIONS，三项文档级观察预批准随手更正；零生产代码/测试改动，不动任何门禁结果、ACCEPTANCE_MAP summary 计数与 A07 自身状态字段）。

**① 悬空引用 ×2（更正）**：`STRICT_FINAL_SCOPE.json` test_merge_mapping「P08 汇总索引见 FINAL_REPORT.md §功能→契约→代码→测试」与 `INDEPENDENT_REVIEW.md` §5「见 FINAL_REPORT.md / FINAL_RESULT.json」——FINAL_REPORT.md 不存在（任务书 T07 建议名，实际职责由本报告承担）。更正：前者改指本报告并如实注记「无独立『功能→契约→代码→测试』索引节，映射散见各阶段 ACCEPTANCE_MAP 系文件」；后者改指 P08_REPORT.md（§结论 + §差异与限制）/ FINAL_RESULT.json。

**② 首败-重跑链计数 6→7（更正）**：command-log.jsonl 实测 status=FAIL 共 **7** 条 = P08-T03-product-regression r1-r5（jsonl line 14-18，工具缺陷首败链）+ P08-T05-pack（line 22，公证凭据）+ P08-T07-lint（line 30，全局纪律）；另 3 条非零退出不属非预期失败（T04-full-test=PASS_WITH_KNOWN_BASELINE 系 F1 基线 4 红；P08-ACCEPT-full-test 与 P08-ACCEPT-ce2d-unknown-key 系负向测试预期非零、status=PASS）。更正：P08_RESULT.json / FINAL_RESULT.json commands 字段「含 6 条首败-重跑链」→ 7 个并补口径注记。

**③ status_note 分母悬挂精确化**：ACCEPTANCE_MAP 14 案例 = 12 PASS + 1 带限定 PASS（A07「macOS arm64 --dir 形态；安装器形态与其它平台 BLOCKED 已单列」，未计入 summary pass=12 桶）+ 1 BLOCKED（A06）。更正：P08_RESULT.json / FINAL_RESULT.json status_note 与 acceptance_cases、本报告 §结论与 §场景逐项结果改为「12 PASS + 1 带限定 PASS（A07）+ 1 BLOCKED + 0 FAIL（12+1+1=14）」等价精确表述；A07 自身状态字段与 ACCEPTANCE_MAP summary 计数不动。

**复核（run-logged，exit code 见 command-log.jsonl P08-FIXR2-*；输出留档 logs/）**：

| 命令 | exit | 结果 |
|---|---|---|
| P08-FIXR2-fail-count | 0 | command-log.jsonl 逐条解析：status=FAIL=7（r1-r5/pack/lint 逐 ID 列出）；另 3 条非零 = 1 基线（PASS_WITH_KNOWN_BASELINE）+ 2 负向预期（status=PASS），口径闭合 |
| P08-FIXR2-json-validate | 0 | P08 docs 目录 8 个 JSON（含本轮改动 3 个：STRICT_FINAL_SCOPE / P08_RESULT / FINAL_RESULT）全部解析合法 |
| 清单终验 `shasum -a 256 -c`（仓库根，两份清单全量，直接重定向至 P08-FIXR2-manifest-check.out，不入 jsonl 以保清单哈希稳定，沿 P08-FIXR1 自指豁免口径） | 见 P08-FIXR2-manifest-check.out | 逐条 OK 与 exit code 留档该日志 |

**记录与清单一致性**：P08_RESULT.json 补 fix_rounds 本轮条目（FINAL_RESULT.json 无 fix_rounds 字段，不加）；command-log.jsonl 追加 P08-FIXR2-* 2 条；EVIDENCE_SHA256.txt（docs + logs 两份）按既有口径刷新——docs 清单本轮改动 5 条目哈希刷新（STRICT_FINAL_SCOPE / INDEPENDENT_REVIEW / P08_REPORT / FINAL_RESULT / P08_RESULT）+ 头注追加；logs 清单补入 P08-FIXR2-fail-count/json-validate 日志并刷新 command-log.jsonl 哈希；P08-FIXR2-manifest-check.out 不入清单（同款自指豁免）。f1 patch 改前改后 sha256 均 25fb315f6c5d68cedd…（与 HEAD 零 diff）。未跑全量 npm test / pack（约束同 FIXR1）。
