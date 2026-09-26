# R01 阶段报告｜目标契约与高风险替代验证

## 最新总控状态：独立 PASS，正式封印待完成（2026-09-26）

全新 Codex `/root/r01_stage_review_r11` 对完整 R01 的126件冻结候选判 PASS，无阻断项。
报告逐字节归档为 [第11轮阶段独立验收](STAGE_HISTORY/R01_STAGE_REVIEW_R11.md)，
SHA-256 `ffd77f3addd0ce3a988274984101a20411df0e7a84ecb0ff07b029aeb719b126`。
候选指纹 `1917800317b41362c2c330b3f5c096c3c2891d2356481bee0b3803bc780f6be7`。
本 PASS 仅绑定已验收候选；正式 seal/最终 ACCEPTED 尚未完成，R02 未放行。
独立实际全量14943 passed/6 failed/15 skipped、四文件68/74；六红为旧审计坐标/交付证据绑定。
总控须在真实提交后执行补丁 VERIFIED、六审计坐标、74/74、全量0失败和推送核对。
非阻断计数备注 O01 已更正：R10 原对抗24例，加四引号4例、R9 25例、旧例38例共91；当前44风险。
下文失败与修复段落保留历史时点，原报告和已钉住哈希不改写。


## 阶段与结论

**STAGE_PASS_SEAL_PENDING**（执行汇总结论 + R01 阶段修复 R1/R2/R3/R4/R5/R6/R7/R8 更新；阶段放行归总控另派的全新
Codex 阶段独立复验，本报告不含执行者或修复者自评阶段 PASS）。

阶段验收进展（2026-09-25）：T01–T08 任务级独立验收全部 PASS 并已提交推送（T08 独立验收 R1 PASS，
报告 R01-T08_REVIEW_R1.md，SHA-256 `4a0c50ad…`）；阶段级独立验收 R1（/tmp/r01-stage-review-r1.md）
判 **FAIL**，两项 BLOCKING：F01（A15 关卡可被删项绕过）、F02（阶段证据停留在 T08 执行前、审计失败
分类与正式基线相反）。R01 阶段修复 R1（/tmp/r01-stage-repair-r1.md）已交付修复候选：F01 以检查器内
FROZEN_CONTRACT 冻结契约收口（负向 5→17 全绿，正向不变），F02 即本报告/交接/账本/风险/进度坐标的
此次更正；候选待全新 Codex 阶段复验。

阶段验收进展（2026-09-26）：阶段级独立验收 R2（/tmp/r01-stage-review-r2.md，SHA-256 `727e82f0…`）
判 **FAIL**，三项 BLOCKING：F01（递延风险 resolve_by_stage/failure_handling 可被 JSON null/{}/[]
等空值绕过、CLOSED 状态与递延输入矛盾亦放行）、F02（R01-T06 三个 Tauri spike 文件使全仓 lint
61 errors）、F03（封印再生成的双补丁 267MB/577MB 超 GitHub 100MiB 单文件硬限，不可推送）。
R01 阶段修复 R2（/tmp/r01-stage-repair-r2.md）已交付修复候选：F01 严格有意义字符串 + 状态一致性 +
登记完整性校验（自测 29/29、真实 CLI 负向电池 16/16，R1 闭合保持）；F02 按真实运行环境收口 eslint
（61 errors→0，不删原型不放松规则）；F03 双补丁受控分片交付（patch-gzip-shards/v1）+ /tmp 隔离全链
预演（分片 VERIFIED、四文件门禁 74/74、可达集与裸仓实收 0 个 >100MiB 对象、裸仓推送 SHA 一致）；
候选待全新 Codex 阶段复验。

阶段验收进展（2026-09-26）：阶段级独立验收 R3（/tmp/r01-stage-review-r3.md，SHA-256 `df460deb…`）
判 **FAIL**，两项 BLOCKING：F01（递延项 resolve_by_stage 只校验非空字符串，截止阶段改成不存在的
R99 后真实 CLI 仍 exit 0/PASS_WITH_CONDITIONS）、F02（/tmp 预演全量 npm test 14946/2——本机环境性
cli-runtime-closure 漂移经 census「原位重写」用例激活 round2/round3 生成器 tree==HEAD 守卫，全量
绿色无受控流程）。R01 阶段修复 R3（/tmp/r01-stage-repair-r3.md）已交付修复候选：F01 截止阶段机器
校验绑定（契约 1.3-stage-repair-r3：检查器冻结真实阶段索引 R00–R11 与逐项 deadline_stage/
latest_stage，风险登记 15 个递延绑定条目补结构化 resolve_by_stage_id/resolve_latest_stage_id，
结构化坐标×阶段索引×冻结契约×正文一致性四方核验，R99/含糊措辞/过晚/错误归属/越过最迟关卡/倒挂/
文本不一致一律 BLOCKED；自测 44/44、真实 CLI 电池 1 正+22 负、R99 原样反例复测 exit 1/NO-GO）；
F02 根因修复（nft 把本机 /bin/bash 等宿主绝对路径计入追踪使再生成漂移——normalizeNftTraceFiles
类级过滤宿主绝对/越界路径，不改基线、不提交机器特定数据、跨机器收敛于同一基线；census 原位重写
用例 finally 恢复快照防级联；「matches committed」用例新增结构性断言禁止宿主绝对条目）——修复后
同机再生成 8949 文件零漂移、census 单文件 23/23（干净树全绿）、候选态全量 npm test 14949 passed/
6 failed（6 红全部为预期封印前红，2 环境性红消失）、/tmp 封印预演 R3a'' 处四文件门禁 74/74 且
全量 npm test 14949 passed/0 failed/exit 0（预演提交 R3c'=d1cfc7f29/R3s''=d6800d892/
R3a''=500ee91db 均为彩排产物不作正式坐标），候选待全新 Codex 阶段复验。

阶段验收进展（2026-09-26）：阶段级独立验收 R4（/tmp/r01-stage-review-r4.md，SHA-256 `0d6ce683…`，
冻结指纹 `3430e659…` 与 R3 修复自报一致）判 **FAIL**，两项 BLOCKING：F01（伪装阶段标识被截断放行
——检查器旧 `R\d{2}` 正则无边界检查，正文 `R099`/`R0999` 被截成 R09、`XR09`/`R09X` 截出 R09，
仅改 RR-T05-X1 正文的 R099/R09-R099 混合/XR09 反例均 exit 0/PASS_WITH_CONDITIONS，违反 G6「正文
任何 token 均真实存在」承诺）；F02（交接事实失真——RISK_REGISTER 的 RR-ENV-CLOSURE-DRIFT、
RR-AUDIT-SEAL-PREEXISTING 与账本 npm_test_note/description 仍以现在时陈述 R2 的闭包漂移 2 红
待治理，与 R3 修复后同机零漂移、预演全量 14949/0 的实测矛盾）。R01 阶段修复 R4
（/tmp/r01-stage-repair-r4.md）已交付修复候选：F01 完整标识阶段引用校验（契约
1.4-stage-repair-r4：完整形态提取 R+数字全文不截断，逐个核验真实阶段索引，形似但被嵌入更长
标识/带相邻字母数字下划线的伪装出现单独拒绝 risk-stage-disguised，「最迟 X」按完整形态捕获
核验；自测 51/51=44 既有全保留+N44–N50 新负向、真实 CLI 电池 1 正+28 负、R4 评审 v0/v1/v2/v3
原样反例全部 exit 1/NO-GO）；F02 交接事实更新（两条风险与账本改为 R2 历史时态+R3 修复后事实，
剩余条件=正式提交后在真实候选上复验，R2 失败历史保留、不提前称正式封印完成）；候选待全新
Codex 阶段复验。

阶段验收进展（2026-09-26）：阶段级独立验收 R5（/tmp/r01-stage-review-r5.md，SHA-256 `3107c695…`，
冻结指纹 `2e3913b6…` 与 R4 修复自报一致）判 **FAIL**：F01（BLOCKING——R4 完整标识边界类
`[A-Za-z0-9_]` 把点号/连字符当合法 token 边界，仅改临时登记 RR-T05-X1 正文的 `R09.5`/`R09-5`/
`R09．5` 虚构小阶段仍被截成 R09 放行，真实 CLI exit 0/PASS_WITH_CONDITIONS/TRACKED(R09)，而
stage-index 只有 R00–R11）；F02（非独立阻断——R4 修复报告 §3/§7 及 /tmp/r01-repair-r4/
r4-file-hashes.txt 标题多计一件证据：写 STAGE_REPAIR_R4 12 件/候选 18+48=66，实际 11 件/
18+47=65，11 件哈希逐项匹配、无被点名缺失的关键日志，属报告计数错误而非证据缺失）。
R01 阶段修复 R5（/tmp/r01-stage-repair-r5.md）已交付修复候选：F01 把「标识延续字符」边界类
扩展为字母/数字/下划线 + 点号/连字符族（ASCII 与全角点、全角下划线/连字符、en–em dash、间隔号），
正文阶段引用必须为完整合法形态（R+数字且紧邻前后均非延续字符；空白/斜杠/中文标点为合法分隔符），
复合/小阶段/拼接出现按完整复合形态拒绝 risk-stage-disguised，不再截成 R09 放行；「最迟 X」标记
同边界规则；自测 59/59=N1–N50 全保留+N51–N58 新负向、真实 CLI 电池 1 正+36 负、R5 评审原样反例
（三个字面量）与 R1–R4 全套反例复测 18/18 全部 exit 1/NO-GO、正向判定不变（契约
1.5-stage-repair-r5，关卡证据三件重生成）；F02 在本报告/交接/进度/账本写明实际证据范围
（STAGE_REPAIR_R4 11 件、候选 18 修改+47 新增=65 文件）与「无第 12 件计划内证据」的核实结论，
R4 原报告及其 SHA 不改写、历史可追溯；候选待全新 Codex 阶段复验。

阶段验收进展（2026-09-26）：阶段级独立验收 R6（/tmp/r01-stage-review-r6.md，SHA-256 `7c3cc019…`，
冻结指纹 `b14cc659…` 与 R5 修复自报一致）判 **FAIL**：F01（BLOCKING——全角同形阶段文字与合法
ASCII `R09` 混用仍放行：仅改临时登记 RR-T05-X1 正文的 `Ｒ９９（不存在的截止阶段）；R09（仅用于
宿主集成）` 与 `R09（宿主集成）最迟 Ｒ１０（虚构放宽关卡）` 均 exit 0/PASS_WITH_CONDITIONS/
TRACKED(R09)——三条阶段正则只从 ASCII R 开始，全角同形阶段在正文首位或「最迟」位置完全隐身，
人工交接截止与机器判定相矛盾；同族误放还实测包括混合宽度 `Ｒ99`、全角前缀 `ＸＲ０９`、小写
`r99`）。R01 阶段修复 R6（/tmp/r01-stage-repair-r6.md）已交付修复候选：G6c 全角同形失败关闭
（契约 1.6-stage-repair-r6——正文提取前 1:1 等长折叠全角拉丁字母/数字为 ASCII，折叠后与
ASCII 形态走同一套完整标识语法与真实阶段索引/正文首位/最迟一致性核验：`Ｒ９９`→R99 不存在
即拒、「最迟 Ｒ１０」→R10 与结构化最迟 R09 矛盾即拒、`Ｒ０９`→R09 与结构化坐标一致时按
规范化引用通过（首坐标校验不再失真）；结构化坐标不折叠、保持 ASCII 精确权威；伪装检出带
IGNORECASE，小写 r+数字按大小写契约作伪装形态拒绝）；自测 67/67=N1–N58 全保留+N59–N65 新负向
+P2 规范化正向、真实 CLI 电池 2 正+43 负、R6 评审两条原样反例与 R1–R5 全套反例 18/18 复测
全部 exit 1/NO-GO、正向真实数据判定不变（关卡证据三件按 1.6 契约重生成）；候选待全新 Codex
阶段复验。

阶段验收进展（2026-09-26）：阶段级独立验收 R7（/tmp/r01-stage-review-r7.md，SHA-256 `e2bf485c…`，
冻结指纹 `04dea823…` 与 R6 修复自报一致）判 **FAIL**：F01（BLOCKING——重复「最迟」中第二个
相矛盾的期限被忽略：仅改临时登记 RR-T05-X1 正文为 `R09（宿主集成）最迟 R09；最迟 R10（虚构
放宽关卡）`（结构化 resolve_by_stage_id/resolve_latest_stage_id 保持 R09/R09 原样），真实 CLI 仍
exit 0/PASS_WITH_CONDITIONS——检查器对折叠后正文用 `LATEST_MARKER_RE.search()` 只校验第一处
「最迟」，第二处相矛盾的强制期限被完全忽略，风险交接正文与机器接受的最迟期限不一致；单处
「最迟 R10」与顺序交换形态仍被正确拒绝，说明仅首处受检）。R01 阶段修复 R7
（/tmp/r01-stage-repair-r7.md）已交付修复候选：G6d 每一处「最迟 X」逐处核验、失败关闭（契约
1.7-stage-repair-r7——`LATEST_MARKER_RE` 改 finditer 遍历折叠后正文全部「最迟」标记：任一引用
不存在阶段即 risk-stage-unknown、任一与结构化 resolve_latest_stage_id 不符即
risk-stage-text-mismatch；清晰契约：正文每一处「最迟」均为强制期限声明，须全部与结构化最迟
关卡一致方可放行（「最迟 R09；最迟 R09」放行），任一相矛盾即拒；与「最迟」语法无关的正文
正常阶段提及（如「R10（后续平台事项）」）不构成最迟标记、不误伤；结构化坐标仍是判定权威，
不放宽到更晚阶段）；自测 72/72=N1–N65 全保留+N66–N68 新负向+P3 多处一致/P4 上下文提及
两个新正向、真实 CLI 电池 4 正+46 负、R7 评审原样反例与变体 13/13 及 R1–R6 全套反例 25/25
复测全部按预期、正向真实数据判定不变（关卡证据三件按 1.7 契约重生成，RISK_REGISTER.json 本轮
一字节未动）；候选待全新 Codex 阶段复验。

阶段验收进展（2026-09-26）：阶段级独立验收 R8（/tmp/r01-stage-review-r8.md，SHA-256
`8e926451…`，冻结指纹 `1c2bd57d…` 与 R7 修复自报一致，18 修改+77 新增=95 件）判 **FAIL**：
F01（BLOCKING——带常见连接词或标点的「最迟」声明绕过逐处核验：R7 标记语法只识别「最迟」后接
空白再紧跟阶段 ID，仅改临时登记 RR-T05-X1 正文为 `R09（宿主集成）最迟 R09；最迟：R10（虚构
放宽关卡）`（全角冒号）、`…最迟：Ｒ１０…`、`…最迟于 R10…`、单处 `最迟：R10`（结构化
resolve_by_stage_id/resolve_latest_stage_id 保持 R09/R09）时真实 CLI 均 exit 0/
PASS_WITH_CONDITIONS——明确而矛盾的最迟期限被误放，通用阶段扫描只查 R10 在 R00–R11 存在、
不与结构化最迟比较，绕过 R7 逐处核验；R7 原样/顺序交换/全角混用仍正确拒绝，正向与多处一致
不受影响）。R01 阶段修复 R8（/tmp/r01-stage-repair-r8.md）已交付修复候选：G6d 连接形态扩展、
失败关闭（契约 1.8-stage-repair-r8——标记 = 显式期限触发词族「最迟/不迟于/不得迟于/不晚于/
不得晚于」+ 有界连接段（≤16 字符自然书写杂讯：空白/全半角冒号/连接词等；段内不得出现 R/r
与子句终结符——期限声明必须落在同一子句内）+ 完整阶段 ID，逐处核验语义不变：任一标记引用
不存在阶段→risk-stage-unknown、任一≠结构化 resolve_latest_stage_id→risk-stage-text-mismatch；
失败关闭：任一触发词出现而在其子句内解析不出阶段 ID（如「最迟于第三阶段完成」的自然语言
期限）→ 新类别 risk-stage-latest-unresolved，无法机器核验的期限不得替代结构化坐标放行；
结构化坐标仍是判定权威，不放宽到更晚阶段）；自测 83/83=N1–N68 全保留+N69–N77 新负向（含
失败关闭反例）+P5 连接形态一致/P6 否定连接式一致两个新正向、真实 CLI 电池 6 正+54 负
（r01_t08_gate_cli_regression.sh 扩 8 个 R8 变体）、R8 评审原样反例与对照 10/10、R7 评审
原样反例与变体 13/13 及 R1–R6 全套反例 25/25 复测全部按预期、正向真实数据判定不变（exit 0/
PASS_WITH_CONDITIONS/15 递延 TRACKED，关卡证据三件按 1.8 契约重生成，RISK_REGISTER.json
本轮一字节未动）；候选待全新 Codex 阶段复验。

阶段验收进展（2026-09-26）：阶段级独立验收 R9（/tmp/r01-stage-review-r9.md，SHA-256
`b98763a1…`，冻结指纹 `0c70bc53…` 与 R8 修复自报一致，18 修改+87 新增=105 件）判 **FAIL**：
F01（BLOCKING——单个期限声明子句内的「原定→改期」仍只校验首个阶段：R8 标记语法把
触发词绑定到其后首个阶段 ID，仅改临时登记 RR-T05-X1 正文为 `R09（宿主集成）最迟（原定
R09，现改 R10）完成`（改期）、`…最迟由原定的 R09 顺延至 R10 完成`（顺延）、`…最迟在
R09 或 R10 完成`（选择）、`…最迟 R09/R10 完成`（斜杠并列）（结构化坐标保持 R09/R09）
时真实 CLI 均 exit 0/PASS_WITH_CONDITIONS——明确改晚的强制期限被误放，第二阶段被通用
阶段扫描当普通上下文只查存在性；R7/R8 原样反例仍正确拒绝，正向与多处一致不受影响；
评审另非阻断披露触发词被引用为字段名「R10 文档介绍“最迟”字段」可能被误拒为 unresolved
的限制，并更正 R8 报告范围措辞：完整 R7 候选 95=85 主表+10 R7 证据，R8 改 9、实际保留
86=76 主表+10 证据，当前总候选 105 件正确、原 R7/R8 报告不改写）。R01 阶段修复 R9
（/tmp/r01-stage-repair-r9.md）已交付修复候选：G6e 期限子句唯一一致、失败关闭（契约
1.9-stage-repair-r9——每个显式期限触发词治理「从触发词起到子句终结符止」的完整区域，
区域内每个完整阶段引用都必须与结构化 resolve_latest_stage_id 一致，出现任何其他阶段
（改期/顺延/选择「或」/斜杠并列及全角变体）即无法机器判定唯一一致期限，按
risk-stage-text-mismatch 失败关闭拒绝；不枚举改期/顺延/选择语言——中文「或」、拉丁
「or」、半/全角斜杠及其组合由「区域内出现第二阶段」统一覆盖；触发词被成对引号紧包
（“最迟”/「最迟」/『最迟』/"最迟"）时为引用词名而非期限声明，不触发解析义务、不治理
期限子句——闭引号紧邻触发词末端，被包裹内容不可能携带阶段 ID，豁免无法隐藏任何期限，
系 R9 评审披露误拒限制的最小处理；触发词之前的阶段是截止坐标上下文（首坐标校验管辖）、
终结符之后的阶段是普通上下文提及（存在性校验管辖），均不受本规则约束；结构化坐标仍是
判定权威，不放宽到更晚阶段）；自测 92/92=N1–N77 全保留+N78–N84 七个新负向（改期/顺延/
或/斜杠/全角改期/全角斜杠/拉丁 or）+P7 同子句内重复一致/P8 引号词名两个新正向、真实
CLI 电池 8 正+62 负（r01_t08_gate_cli_regression.sh 扩 8 个 R9 变体+2 个正向）、R9 评审
25 变体 25/25 按预期（四误放转 exit 1、quoted-context 误拒转 exit 0、其余正负不变）、
R1–R8 留存 CLI 反例 38/38 退出码与首阻断类别逐项不变、正向真实数据判定不变（exit 0/
PASS_WITH_CONDITIONS/15 递延 TRACKED，关卡证据三件按 1.9 契约重生成，RISK_REGISTER.json
本轮一字节未动）；候选待全新 Codex 阶段复验。

阶段验收进展（2026-09-26）：阶段级独立验收 R10（/tmp/r01-stage-review-r10.md，SHA-256
`c6433031…`，开工候选冻结指纹 `39e9bdd7…` 与 R9 修复自报一致，18 修改+98 新增=116 件）
判 **FAIL**：F01（BLOCKING——新增引号豁免实际跳过整个期限区域，且不考虑括号层次的
终结符会截断完整改期声明：仅改临时登记 RR-T05-X1 正文为 `R09（宿主集成）“最迟”字段：
原定 R09，现改 R10 完成`、`R09（宿主集成）最迟（原定 R09；现改 R10）完成`（结构化坐标
保持 R09/R09）时真实 CLI 均 exit 0/PASS_WITH_CONDITIONS——引号只包词名不证明其后没有
期限书写，括号内分号/换行截断了本应完整的期限声明区域；四种支持引号（“”/「」/『』/
ASCII "）、引号后斜杠/全角斜杠同类误放；评审另独立裁定「最晚于 R10 完成」「R10 前必须
完成」等已披露明确截止不得凭「文档化边界」自动当普通上下文，并查明 R9 修复报告
「任务书明确禁止同义词枚举」的声称在原任务书中无原文依据——原报告不改写，本报告
与现行交接按查明结果更正该声称）。R01 阶段修复 R10（/tmp/r01-stage-repair-r10.md）
已交付修复候选：G6f 期限区域完整性与已披露边界期限拒绝（契约 1.10-stage-repair-r10
——(1) 引号紧包豁免收窄为仅豁免该触发词自身的解析义务（词名引用无须解析出阶段 ID，
R9 披露的词名误拒限制保持修复），其「触发词起到括号感知终结符止」的后续区域仍逐
引用与结构化 resolve_latest_stage_id 一致，词名/字段名引用不得遮蔽同子句内随后的实际
赋值/改期/选择；(2) 区域边界括号感知：未闭合全/半角括号内的终结符不终止区域，未闭合
括号使区域延伸至文本末尾（失败关闭方向）；(3) 「最晚」并入显式期限触发词族（迟/晚
对称的封闭形态族补全，非开放同义词枚举——该方向无任务书禁令，系按 R10 评审裁定
入族逐处核验），「最晚于 R10 完成」exit 1、一致书写「最晚于 R09 完成」exit 0；
(4) 阶段锚定「之前/以前/前」边界期限书写（`R10 前必须完成` 等，封闭词素族紧邻完整
阶段引用）按新类别 risk-stage-boundary-unsupported 失败关闭拒绝——机器契约仅支持
触发词族+完整阶段 ID 的期限语法，不支持 before-stage 语义与结构化坐标的一致性核验；
机器书写/交接契约同步成文：期限表达一律用触发词族+完整阶段 ID（或结构化坐标），四种
支持引号仅用于词名引用）。自测 107/107=N1–N84 全保留+N85–N96 十二个新负向（词名后
改期四引号族/词名后斜杠与全角斜杠/括号内分号与换行/最晚于 R10/R10 前与 R10 之前）+
P9 直角引号词名/P10 最晚于 R09 一致/P11 词名后一致提及三个新正向、真实 CLI 电池
11 正+74 负（r01_t08_gate_cli_regression.sh 扩 12 个 R10 负向+3 个正向；R9 曾记
「文档化边界」正向的两例「最晚于 R10 完成」「R10 前必须完成」按 R10 评审独立裁定
翻转为负向——系加强拒绝，非放宽旧断言）、R10 评审 24 变体+四引号对照 4 例重放
（9 例误放全转 exit 1、5 例正向保持、既有负向保持）、R9 评审 25 变体重放 25/25
（仅评审裁定的两例边界翻转，其余不变）、R1–R8 留存 CLI 反例 38/38 退出码与首阻断
类别逐项不变（合计 91/91）、正向真实数据判定不变（exit 0/PASS_WITH_CONDITIONS/
15 递延 TRACKED，关卡证据三件按 1.10 契约重生成，RISK_REGISTER.json 本轮一字节
未动）；候选待全新 Codex 阶段复验。

## 范围

仅 R01（T01–T08）。基线差异：R00 正式封印 C4（`328cc8bb5a807bdaad520b459907fb1fa4e10dca`）→ 当前
本地/远端 HEAD `363999378482dfed42733a3c4e8ad20ac82fd0fc`（ls-remote 复核一致），全部为 R01
任务/账本提交。获准 ADR：ADR-001（所有权）、ADR-002（浏览器宿主选型）、ADR-003（文档渲染器选型）、
ADR-004（存储切换与旧版本拒写）。**生产目录（desktop/ server/ core/ lib/ shared/）零改动**；
唯一非 docs/artifacts 改动为 T08 按 T05 R2-N1 移交收口的两个负向测试资产注释更正
（tests/migration/r01-t05/，注释级、零语义变化、未被任何哈希清单钉住）。

## 源码

- 起止 SHA：`328cc8bb…`（R01 stage base）→ `36399937…`（当前 HEAD，本地与 origin 一致）。T08 交付
  已提交并推送：任务提交 `358299c1e` + 账本提交 `f783a8e8e`/`363999378`（remote-tracking reflog
  2026-09-25 22:32/22:34 +0800 两次 update by push 实证）。T08 执行前快照（开工基线 `2bbec6d0…` +
  `working_tree_digest=239acceb…`，8686 行可复算）作为历史候选快照保留于 R01_HANDOFF.json。
- 分支：codex/rust-tauri-migration。
- 依赖锁摘要：package-lock.json `e54a16fe…`（R00 起未变）；rust/Cargo.lock `d1d8b4a9…`（280 条目，
  T03 锁 279 + T04 新增 lingxi-browser-spike 自身，无新增第三方版本）；rust-toolchain.toml channel=1.98.1
  `eec34104…`。

## 环境

macOS 27.0 arm64（Mac15,14）；Node v24.16.0；Python 3.14.3；rustup 1.29.1 + rustc/cargo 1.98.1
（锁定）。构建模式：cargo --offline（crates.io 依赖 T03 已 fetch 入锁）；浏览器/PDF 原型用本机 Chrome
+ 限 loopback 吊具；Tauri spike 双产物（app-test/app-release）。外部替身：lingxi-proto-server（loopback
确定性替身）、testsite.mjs（127.0.0.1:18281）、canary（127.0.0.1）；无真实账号、无真实用户数据。
**平台结论仅覆盖 macOS arm64；Windows/Linux/macOS x64 全部 UNVERIFIED，挂 R09/R10 强制关卡。**

## 完成项

- **T01 所有权**：ADR-001 + DEPENDENCY_RULES.json + OWNERSHIP_TARGET.json（736 F-ID/69 store/30 owner/
  11 关键事实逐项归属）+ 机器校验器（正向 O1–O8/D 系列 + N1–N15 负向 + 生成器 --check 零漂移）。
- **T02 协议**：PROTOCOL_SPEC.md（lingxi.wire v1 独立版本轴）+ rust/crates/lingxi-protocol（serde 权威源）+
  生成链（JSON Schema 56 文件 / TS 绑定 / 12 golden / MANIFEST）+ API_COMPAT_MATRIX.json（624 条目：
  427 业务兼容 retain + 197 native_host 映射）+ 握手原型。headSha 漂移缺陷已修复为内容派生戳（b9442d86f）。
- **T03 依赖锁定**：DEPENDENCY_DECISIONS.md（D-01..D-08：rustup 1.98.1 / axum 0.8.9 / reqwest 0.13.5+rustls
  平台根证书 / rusqlite 0.40.2 bundled / tracing / jsonschema 0.57.0 / rmcp 3.4.1）+ PLATFORM_BUILD_MATRIX.json
  + spike_health/spike_tls_probe 真实运行。
- **T04 浏览器原型**：ADR-002 选定受控 Chromium（CDP over pipe）；32 项等价断言 + 隔离 B1–B6 + 接管 D1–D4
  全 VERIFIED；WKWebView/wry 缺口如实记录为落选理由。
- **T05 PDF/办公原型**：ADR-003 选定受控 Chromium printToPDF；19 场景 28 VERIFIED/0 FAILED；A09 中文长文档
  35 项全过；A10 危险资源 7×DENY + 无限脚本 exit=2；repair-r1 CSP connect-src 封堵 WS（负向 7/7）。
- **T06 系统能力原型**：TAURI_SPIKE_REPORT + SHELL_CAPABILITY_MATRIX（14 能力逐项）；15 步 e2e 双产物
  realpath/symlink 两轮全绿 + sabotage 负向；ACL untrusted/remote 11/11 全拒；updater 篡改验签失败。
- **T07 存储切换**：ADR-004（D2 分离根+原子指针）+ DATA_COMPATIBILITY_MATRIX.json（69 存储）+
  ROLLBACK_DESIGN.md；旧二进制拒写变体矩阵实测（含 corrupt 类 fail-open 如实暴露并立项 PROD-DEFECT-1）；
  A14 回滚演练 ROLLBACK-DRILL-PASSED。
- **T08 冻结关卡**：RISK_REGISTER.json（T08 原交付43条、当前44条目，逐条截止阶段+失败处理）+ 关卡检查器（A15）+ 双向覆盖
  检查（A16）+ 隔离核查（机器验证 ISOLATED）+ T06 二进制处置（删除留证）+ /tmp R01 克隆清理（74 项 36GB）+
  本验收账本 + R01_HANDOFF.json。详见 R01-T08_REPORT.md。

## 行为变化

无用户可见变化。全部原型为隔离目录（rust/、spike/、contracts/、tests/migration/r01-*），机器核查证明
生产入口零引用（r01_t08_isolation_check.py：2209 生产文件扫描 0 违规）。本阶段不是新增功能：任务书 §1
明确「原型不得成为默认生产入口」。

## 验收

逐场景命令/预期/实际/退出码/证据指针的完整账本见 **R01_ACCEPTANCE_LEDGER.json**（16/16 REQUIRED）。

| 场景 | 结果 | 关键命令 | 退出码 | 复核 |
|---|---|---|---|---|
| R01-A01 核心不依赖桌面 | PASS | cargo build/test/metadata --offline | 0 | R3 |
| R01-A02 双负责人被检出 | PASS | r01_t01_check_ownership.py --self-test（N1–N15） | 0 | R3 |
| R01-A03 跨语言 round-trip | PASS | r01-t02-roundtrip.sh（12 golden 逐字节） | 0 | R1+fix |
| R01-A04 版本不兼容可诊断 | PASS | r01-t02-handshake.sh（5 场景 400/4409） | 0 | R1+fix |
| R01-A05 锁文件可复现 | PASS | fetch --locked + build --locked --offline | 0 | R1 |
| R01-A06 依赖缺失不掩盖 | PASS | 缺依赖对照实验 exit 101 | 101（预期） | R1 |
| R01-A07 浏览器能力等价 | PASS | T04 四阶段 32 断言 + 旧侧对照 | 0 | R1+docfix |
| R01-A08 跨会话/不可信页隔离 | PASS | B1–B6 + ADV-1..8 对抗 | 0 | R1 |
| R01-A09 中文长文档完整 | PASS | r01-t05-replay.sh a09（35 项全过） | 0 | R2 |
| R01-A10 危险资源/失败可控 | PASS | S5 7×DENY + S6 exit=2 + WS 负向 7/7 | 0/2（预期） | R2 |
| R01-A11 自定义命令受限 | PASS | run_e2e.sh（ACL 11/11 拒） | 0 | R2 |
| R01-A12 测试能力不进生产 | PASS | 双产物探测 + 篡改验签失败 | 0 | R2 |
| R01-A13 旧程序拒写可验证 | PASS | a13 探针组 + 10 变体矩阵 | 0/1（变体预期） | R2 |
| R01-A14 回滚不丢新数据 | PASS | rollback-drill.py step1–7 | 0 | R2 |
| R01-A15 高风险不被演示遮蔽 | PASS | r01_t08_gate_check.py（真实数据 PASS_WITH_CONDITIONS；负向 17/17，含 FROZEN_CONTRACT 后新增的删域/删 user_takeover/删 sha256/删递延等 F01 形态；R2 修复后自测 29/29 + 真实 CLI 负向电池 16/16，新增 null/{}/[] 空值/状态矛盾/重复 id 等形态拒绝；R3 修复后自测 44/44 + 真实 CLI 电池 1 正+22 负——截止阶段绑定 7 变体（R99/含糊/过晚/错误归属/越过最迟关卡/缺结构化坐标/倒挂）+ R99 原样反例 exit 1） | 0 | R1（T08）+阶段修复 R1/R2/R3 加固待阶段复验 |
| R01-A16 目标职责闭合 | PASS | r01_t08_coverage_check.py（COVERAGE-CLOSED + 负向 6/6） | 0 | R1（T08） |

T08 全量重跑门禁（本阶段 HEAD 上真实复跑，日志 artifacts/rust-tauri/R01/T08/gates/）：
T01 校验器正向 exit=0、N1–N15 exit=0、生成器 --check exit=0（736/69 up-to-date）；
T02 roundtrip exit=0、handshake exit=0、check-generated exit=0（56 生成文件+624 条目零漂移）；
cargo test --workspace --offline（rustup 1.98.1，CARGO_TARGET_DIR=/tmp 全新目录）exit=0，45 passed 0 failed。
**重跑新发现 RR-T08-F1**：T02 门禁脚本共享默认 CARGO_TARGET_DIR + 生成器用编译期 CARGO_MANIFEST_DIR，
共享缓存内评审遗留克隆编译的二进制使 --check 比对了遗留克隆树而非本仓库；已用全新 target 复跑证明本仓库
生成树真实零漂移（缺陷限门禁健壮性，归属 R02 硬化；未自行修复前序交付，如实登记并报告总控）。

## 安全与数据

- 权限负向：T04 ADV-4/5/6（file:// 越权/CDP 面）、T06 ACL 11/11、T05 ATK1–ATK8 全部真实拒绝。
- 真实进程：旧发布二进制拒写探针（A13）用现役真实二进制；T06 sidecar 崩溃恢复 sabotage 负向 exit=1。
- 单写者：T01 O8 锁定 rust-service 唯一业务写者（精确串匹配，shadow writer 负向 N15）。
- 迁移/回滚：ADR-004 epoch 闸 + D2 分离根；A14 演练幂等合并 sha256 稳定；协作式闸定性入档（RR-T07-F2）。
- 未伪造授权：TCC 全部只读查询；录音/听写授权正链路、屏幕真实采集如实 UNVERIFIED。

## 完整映射

F-ID→T-ID→A-ID→测试→结果：736 F-ID 的机器双向闭合由 r01_t08_coverage_check.py 输出
（artifacts/rust-tauri/R01/T08/coverage/coverage-report.json）：736/736 有目标 owner（非 worker）与
合法实施阶段；11 关键事实 core/service 单负；14 Pi 能力全部 KERNEL_MIGRATION 无第二 loop 保留项。
叶子功能到阶段/任务的分配在 OWNERSHIP_TARGET.feature_ownership（stage_ids/task 级归后续阶段任务书）。
**未覆盖集合**：无未归属 F-ID；未验证集合 = 跨平台（RR-T06-PLATFORM）、授权态媒体能力
（RR-T06-MIC/SPEECH/SCREEN）、代理矩阵（RR-T06-PROXY）、WebRTC 边界（RR-T05-N2）、T04 原型 Fetch 层
（RR-T05-X1）、shell-local 存储过闸（RR-T07-F3）——全部挂账 R09/R10（代理矩阵最迟 R10）。

## 已知缺陷

当前全部 44 条见 RISK_REGISTER.json（含根因/后果/截止阶段/失败处理/证据指针）。最关键：
- **RR-T07-PROD-DEFECT-1（REGISTERED，归属候选 R02）**：data-epoch-coordinator corrupt-failure fail-open
  + 警告文本失实；R01 不改生产代码；R02 新实现须 fail-closed 并重跑 4 变体。
- **RR-T08-F1（MEDIUM，归属 R02 门禁硬化）**：T02 门禁脚本陈旧二进制错绑（本节验收段已述）。
- **RR-T02-F2/F3（MEDIUM，截止先于 R04/R08）**：TS canonical-json 浮点/>2^53 硬失败+spec 矛盾；
  EventPayload 回退吞畸形已知事件+两信封不变量未编码。
- **RR-T06-R6（HIGH，R09）**：updater 必须 https，dangerousInsecureTransportProtocol 禁进生产。
- **RR-T06-MIC/SPEECH/SCREEN、RR-T06-PLATFORM（HIGH，R09/R10）**：授权态媒体能力与跨平台未验证。

## 未执行/BLOCKED

- 全量 npm test 本轮未重跑（任务规格既定），不把红灯记作通过。审计四文件门禁事实（R01 阶段验收 R1
  F02 + 阶段修复 R1 复证更正，推翻 T08 时「预存四红承继 R00」分类）：**正式 R00 C4 基线（328cc8bb）
  提交后复验 68/68 绿、全量 npm test 14942 passed/0 failed**（/tmp/r00-seal-c4-postcommit.md）；
  **R01 候选 HEAD（36399937）实测 62/68、6 FAIL 为 R01 增量新增**（post-verification-audit-seal 1、
  round2-delivery-evidence 3、round3-delivery-evidence 2；upstream-sync-matrix 7/7 绿）。根因：
  VERIFIED_SOURCE_SHA 仍指 R00 C3（f2b8c687），R01 的 2103 个新增/修改文件不在审计白名单，diff guard
  按其设计拒绝。坐标推进=把适用验证绑定到阶段复验通过的真实候选提交后以纯审计提交推进，需提交授权，
  属总控 seal 工作流（RR-AUDIT-SEAL-PREEXISTING 已重分类 OPEN；阶段修复 R1 已在 /tmp 隔离副本完成
  分步预演，方案见 /tmp/r01-stage-repair-r1.md）；非本阶段实现缺陷，不阻止本阶段放行评审（本阶段无
  生产代码改动）。
  R01 阶段验收 R2 F03 追加事实与 R2 预演（2026-09-26）：封印再生成的两份 .patch.gz（267,221,850 B /
  577,433,322 B）超 GitHub 100MiB 单文件硬限——round2 膨胀因 R01 证据入库（未压缩 495.9MB/5306 项），
  round3 另因 BASE 较旧把 round2 再生成补丁一并打入（未压缩 968.9MB/5719 项）。R01 阶段修复 R2
  交付受控分片（patch-gzip-shards/v1：45MB 片、清单最后落盘为提交点、单/分片互斥回收、MISMATCH
  不改写既有交付；完整性语义不损失——重组 sha256==patchSha256、可 gunzip 重放），/tmp 隔离副本
  全链预演：绿色电池含 lint、分片 VERIFIED（round2 6 片/round3 13 片）、坐标推进后四文件门禁 74/74、
  HEAD 可达集与裸仓实收审计 0 个 >100MiB 对象（最大 97,003,232 B 为历史已推送 round3 补丁）、
  本地裸仓推送 SHA 一致（证据 artifacts/rust-tauri/R01/STAGE_REPAIR_R2/）。预演暴露的环境性
  cli-runtime-closure 漂移（nft 新增 /bin/bash 追踪项，三实验证实与 R1/R2/R01 内容无关）使全量
  npm test 2 红（census 原位写 × 生成器树净守卫的预存竞态被激活），正式封印前需治理，如实登记
  不冒充绿。
  R01 阶段验收 R3 F02 与 R3 修复（2026-09-26）：R3 独立复现上述 2 红并要求受控 0 失败流程。
  R01 阶段修复 R3 根因修复：nft 静态分析解析 lib/sandbox/* 的 /bin/bash 硬编码 spawn 目标并把
  本机该路径文件计入追踪，宿主绝对路径穿过 normalizeSourceGraphPath 进入闭包使本机再生成漂移
  （+14/-3）；修复=normalizeNftTraceFiles 类级过滤宿主绝对/越界路径（committed 基线 8949 项零
  绝对路径零越界路径即既有语义；不改基线、不提交机器特定数据、跨机器收敛同一基线）+ census
  原位重写用例 finally 恢复快照（防未来漂移级联污染工作树）+「matches committed」用例新增结构性
  断言禁止宿主绝对条目（加强非放松）。实测：裸 HEAD 树修复前复现漂移与 R2 归因一致；修复后同机
  再生成 8949 文件零漂移；census 单文件 23/23；候选态全量 npm test 14949 passed/6 failed（6 红
  全部为四文件门禁同集预期封印前红，2 环境性红消失；总数 14964=14963+新增 1 归一化单测）；
  /tmp 封印预演全链（ceremony-r3.sh，证据 STAGE_REPAIR_R3/）：R3c'=d1cfc7f29→R3s''=d6800d892→
  R3a''=500ee91db（REHEARSAL 彩排提交不作正式坐标），四文件门禁 74/74、全量 npm test 14949
  passed/0 failed/exit 0、可达集与裸仓实收 0 个 ≥100MiB（最大 97,003,232 B）、推送 SHA 一致、
  终态零漂移。正式封印仍须由总控绑定真实候选提交执行。
- 跨平台四组真机（BLK-PLATFORM）、真实凭证 LIVE（BLK-CREDENTIALS）、长时稳定性（BLK-LONGRUN-G1）：
  承继 R00，最晚解除 R10；发布授权（BLK-RELEASE-AUTH）R11。均不阻止 R01 放行（阶段放行条件只要求
  实施平台真实原型证据 + 未验项挂 R09/R10 强制关卡——已满足并挂账）。
- check_deps 动态库缺失分支 macOS 实跑（RR-T03-F3）：归 R09/R10，不阻止。

## 回退

本阶段无生产改动，回退 = 删除/停用隔离原型（rust/、spike/、contracts/、tests/migration/r01-*、
scripts/rust-tauri/r01-*、docs/rust-tauri/R01/、artifacts/rust-tauri/R01/）即还原；不涉及用户数据。
原型内回滚设计（A14）已演练：ROLLBACK-DRILL-PASSED，回滚流程不丢切换后新增数据（先导出归档再动指针、
幂等合并），无「直接删新目录」步骤。T06 的 6 个未入库二进制已按「删除无用实验而保留证据」处置
（SHA-256 与已入库清单核验一致后删除，artifacts/rust-tauri/R01/T08/disposition/）。

## 独立审查

T01–T08 各任务独立验收全部 PASS（复核者/轮次/报告 SHA 见 ORCHESTRATOR_PROGRESS.json 与
R01_ACCEPTANCE_LEDGER.json review 字段）；T02 headSha 修复复验 PASS。**T08（A15/A16）独立验收
R1 PASS**（ZCode:R01-T08-review-r1，报告 R01-T08_REVIEW_R1.md，SHA-256
`4a0c50ade4b10d3bb18b8303c78fb0670a42fee86852d4faf380804f9d731fcf`；交付随 `358299c1e` 入库并
推送）。任务级 PASS 不等于阶段放行：**阶段级独立验收 R1（/tmp/r01-stage-review-r1.md，SHA-256
`0ace64a6…`）判 FAIL**——F01：A15 关卡把必需能力全集交给被验输入自报，删 user_takeover/删域/
删 sha256/删递延均放行；F02：阶段证据停留在 T08 执行前、审计失败分类与正式 R00 C4 基线相反。
**R01 阶段修复 R1**（/tmp/r01-stage-repair-r1.md）已修复两项并交付候选：FROZEN_CONTRACT 冻结契约
（独立于被验输入；负向 17/17、正向 PASS_WITH_CONDITIONS 不变、关卡证据重生成）+ 本报告/交接/
账本/风险/进度坐标更正。**阶段级独立验收 R2（/tmp/r01-stage-review-r2.md，SHA-256 `727e82f0…`）
判 FAIL**：F01（同根因遗漏——`str()` 转换使 JSON null/{}/[] 冒充有意义字段、CLOSED 与递延输入
矛盾放行）、F02（T06 三个 spike 文件全仓 lint 61 errors）、F03（封印双补丁超 GitHub 100MiB 硬限
不可推送）。**R01 阶段修复 R2**（/tmp/r01-stage-repair-r2.md）已交付候选：有意义字符串 + 状态
一致性 + 登记完整性校验（自测 29/29、CLI 电池 16/16、R1 闭合保持）；eslint 按真实运行环境收口
（61→0 errors）；双补丁受控分片 + /tmp 全链预演（分片 VERIFIED、门禁 74/74、对象审计 0 超限、
裸仓推送一致）；阶段放行待全新 Codex 阶段复验。**阶段级独立验收 R3**（/tmp/r01-stage-review-r3.md，
SHA-256 `df460deb…`）判 FAIL：F01（截止阶段 R99 仍 exit 0）、F02（全量 2 环境性红无受控 0 失败
流程）；**R01 阶段修复 R3** 已交付候选（G6 截止阶段机器校验绑定 + 闭包漂移根因修复，预演全量
14949/0）。**阶段级独立验收 R4**（/tmp/r01-stage-review-r4.md，SHA-256 `0d6ce683…`）判 FAIL：
F01（伪装阶段标识 R099/XR09 被旧 `R\d{2}` 截断放行）、F02（风险登记/账本 R2 时态与 R3 修复后
事实矛盾）；**R01 阶段修复 R4** 已交付候选（G6b 完整标识边界校验 + 交接事实更新，自测 51/51、
CLI 电池 1 正+28 负、R4 原样反例 exit 1）。**阶段级独立验收 R5**（/tmp/r01-stage-review-r5.md，
SHA-256 `3107c695…`）判 FAIL：F01（R09.5/R09-5/R09．5 虚构小阶段被 R4 边界类截成 R09 放行）、
F02（R4 报告证据计数 12 件/66 实为 11 件/65，11 件哈希均匹配，无证据缺失）；**R01 阶段修复 R5**
已交付候选（标识延续字符边界扩展：点号/连字符族复合形态按完整标识拒绝，自测 59/59、CLI 电池
1 正+36 负、R5 原样反例与 R1–R4 全套反例 18/18 exit 1；F02 计数更正入交接/进度/账本）；阶段放行
待全新 Codex 阶段复验。**阶段级独立验收 R6**（/tmp/r01-stage-review-r6.md，
SHA-256 `7c3cc019…`）判 FAIL：F01（全角同形阶段 Ｒ９９/最迟 Ｒ１０ 与合法 ASCII R09 混用
仍放行，人工交接截止与机器判定相矛盾）；**R01 阶段修复 R6** 已交付候选（G6c 全角同形 1:1
折叠失败关闭 + 大小写契约伪装拒绝，契约 1.6-stage-repair-r6；自测 67/67、CLI 电池 2 正+43 负、
R6 原样反例与 R1–R5 全套反例 18/18 exit 1）；阶段放行待全新 Codex 阶段复验。**阶段级独立验收
R7**（/tmp/r01-stage-review-r7.md，SHA-256 `e2bf485c…`）判 FAIL：F01（正文两处相矛盾「最迟」
期限——「最迟 R09；最迟 R10」（结构化最迟 R09）的第二处被 search() 首匹配实现忽略，真实
CLI exit 0/PASS_WITH_CONDITIONS，交接正文与机器接受期限不一致）；**R01 阶段修复 R7** 已交付
候选（G6d「最迟」标记逐处核验、失败关闭——每一处「最迟」均须与结构化最迟关卡一致，任一
相矛盾即拒，正常上下文提及 R10 不误伤，契约 1.7-stage-repair-r7；自测 72/72、CLI 电池
4 正+46 负、R7 原样反例与变体 13/13、R1–R6 全套反例 25/25 全部按预期）；阶段放行待全新
Codex 阶段复验。**阶段级独立验收 R8**（/tmp/r01-stage-review-r8.md，SHA-256 `8e926451…`）
判 FAIL：F01（带连接词/标点的「最迟」声明绕过逐处核验——「最迟：R10」「最迟：Ｒ１０」
「最迟于 R10」及单处冒号形式在结构化最迟 R09 时均 exit 0/PASS_WITH_CONDITIONS 误放，
R7 标记语法只认「最迟」后接空白，通用阶段扫描不与结构化最迟比较）；**R01 阶段修复 R8**
已交付候选（G6d 连接形态扩展、失败关闭——标记=触发词族（最迟/不迟于/不得迟于/不晚于/
不得晚于）+有界连接段（≤16 字符，段内无 R/r 与子句终结符）+完整阶段 ID，逐处核验；触发词
解析不出阶段 ID→risk-stage-latest-unresolved 失败关闭，契约 1.8-stage-repair-r8；自测
83/83、CLI 电池 6 正+54 负、R8 原样反例与对照 10/10、R7 原样 13/13、R1–R6 全套反例
25/25 全部按预期）；阶段放行待全新 Codex 阶段复验。未验范围：跨平台实机、授权态 TCC
正链路、真实凭证 LIVE 项。

## 下一阶段

- **允许范围**：R02（Rust 独立服务、存储与事件基础）。必须消费输入：lingxi.wire v1 协议 crate 与生成链、
  OWNERSHIP_TARGET/DEPENDENCY_RULES、DEPENDENCY_DECISIONS 锁表、ADR-004 数据规则、RISK_REGISTER
  （尤其 RR-T07-PROD-DEFECT-1 修复归属、RR-T08-F1 门禁硬化、RR-T02-F1..F5 协议修复截止）。
- **依赖顺序与目录约定**：见 R01_HANDOFF.json `allowed_next_scope` 与 §依赖顺序段（本报告不复制事实源）。
- **不允许开始**：正式替壳（R09 之前不得以原型替代生产入口）；跨平台放行结论；生产 CSP/代理/updater
  配置定稿；任何把 UNVERIFIED 项写成已验证的行为。

## 远程/发布

T01–T08 任务提交与账本提交均由总控在对应独立验收 PASS 后提交并推送（remote-tracking reflog 逐次
update by push 实证：T04→`5a8a8e24a` 13:07、T05→`76bd42c43` 15:38、T06→`82870879d` 18:56、
T07→`2bbec6d07` 20:56、T08→`f783a8e8e` 22:32 与 `363999378` 22:34 +0800；ls-remote 复核远端
=`36399937…` 与本地一致）。本执行代理与 R01 阶段修复 R1 均无 commit/push 授权，未发起任何
PR/tag/release；发布授权（BLK-RELEASE-AUTH）未授予。
