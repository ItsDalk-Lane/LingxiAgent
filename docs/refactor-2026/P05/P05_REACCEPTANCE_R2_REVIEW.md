# P05 第二轮复验收报告（FIXR2 后收口复验收子代理，全新上下文）

日期：2026-09-22｜验收对象：P05 阶段整体（执行轮 + 首轮验收 + FIXR1 + 复验收 + FIXR2 终态工作区，HEAD `1f0537b0865c1f6a4a17c28eb66c6d9ebf4948a6` + 未提交改动）｜角色：只读收口复验收（本报告与 `artifacts/refactor-2026/P05/logs/P05-REACC2-*` 为本轮唯一写入；EVIDENCE_SHA256.txt 未再生成，沿复验收先例留作编排层/下一修复轮动作——故本轮新增文件暂不在 111 条清单内，属既有流程的既定状态）。全部结论基于本轮独立复跑与逐点核对，未采信执行者/修复者/前序验收者自报。

## 结论

**阶段终判：PASS（含 FIXR1、FIXR2 修复轮后的阶段整体）。R1 经独立核实真正闭合，本轮零新发现。**

## 1. R1 闭合独立核实（FIXR2 主项）

- **绝对路径零残留（自行多形态 grep）**：`/Users/study_superior`、`from "`/`from '`+绝对、`require("/` 各形态在 P05-ACCEPTANCE-counterexample.test.ts 均 0 命中；`REPO` 常量 0 残留（FIXR2 声明的「import×5 + REPO 常量清退」与实际相符：当前 5 条相对 import 语句 + 头注字面提及 1 行 = grep 行计数 6，与 FIXR2 记录逐点一致）。
- **it/it.fails 精确计数**：`it.fails(` 恰 1 处（C1，line 65）；普通 `it(` 4 处（C2×3 + C3×1）。
- **断言零改动交叉核对**：C1 失败断言在首轮 RED 记录（P05-ACCEPTANCE-counterexample.log:91）与当前文件（:98）均为同一句 `expect(serialized).not.toContain('【迟到旧流尾巴】')`，前置 `toContain('B轮正文')` 亦两侧一致；FIXR2 基线日志（11:48，修复前）红在同一断言、Received 载荷结构相同——三份证据（首轮 RED、FIXR2 基线、当前文件）互证「仅期望方向元语义 it → it.fails」。C1 缺口本身在首轮验收报告有完整登记（源码论证 use-stream-buffer.ts:413-460 无 streamId 闸门、服务端不可达性分析、A03/A04 消费端面归属、最小修复建议），it.fails 注释表述与登记一致，无 undisclosed defect。
- **真实复跑（本轮独立执行）**：仓库根经 npm test 路由（含 --exclude 配置）exit 0，`4 passed | 1 expected fail (5)`；cwd=/tmp + `vitest run --root 仓库` exit 0，同构结果。相对路径解析不依赖执行 cwd 成立。FI 2/2、P04 CE 2/2 亦经本轮复跑确认（P05-REACC2-counterexample-test-*、P05-REACC2-fi-p04-test.out）。

## 2. FIXR2 范围纪律

- `git diff HEAD`：仅执行轮 3 个测试文件 +282/-0，**零删除行**（grep 非头删除行 = 0），起点与终态逐字一致；FIXR2 全部改动落在未跟踪交付目录内。
- mtime 链：FI 文件 11:21（FIXR1 后未再触碰）、P05/P04 counterexample 11:49（FIXR2）、其余文档/日志时间戳与各轮声明吻合。
- P04 counterexample 修复零已提交面影响：`git ls-files artifacts/refactor-2026/P04/logs/` = 0，且该文件命中 `.gitignore:94` `logs/` 规则（被忽略，强于「未跟踪」）。
- **command-log 防篡改**：41 条 × stdout/stderr = **82/82 digest 全 match**（0 MISMATCH 0 MISSING）——含 FIXR2 新增 9 条，全部日志自记录起字节未变。
- FIXR1 节原文「12 处」未被改写；FIXR2 新节以「FI 12（代码 11 + 头注 1）」如实说明口径差异——外观级，处理正确。

## 3. EVIDENCE_SHA256 独立复算

- `shasum -a 256 -c`：**exit 0，111/111 OK，0 NOT-OK**。
- 覆盖核算：两目录实际 114 文件 − 清单 111 = FIXR1/FIXR2 manifest-check.out（两轮均声明排除，自引用约束）+ 清单自身（模板 §7）；**零 MISSING-ON-DISK**。
- ACCEPTANCE_MAP.json / P05_RESULT.json 独立 JSON.parse 合法；P05_RESULT 声明 status=PASS、F1 基线 4 红、fix_rounds=[FIXR1, FIXR2]、blocked_items 仅 P04 继承项——与证据一致。

## 4. 「下次全量回到 4 红」推断依据（未实跑全量，依据可核查）

1. `npx vitest list`（全默认集）确认恰 9 例来自 artifacts/refactor-2026（P04 CE 2 + FI 2 + P05 CE 5），三文件均被收集；
2. 该三文件本轮定向复跑在双 cwd 下全部 file-green（it.fails 计绿）；
3. 历史两次全量（T08 10:58、验收 11:08 启动）终态逐字相同：`4 failed | 14769 passed | 15 skipped (14788)`，且均未收集 P05 CE（落位时间 11:15+ 晚于两次收集；总数相同为证）；4 红 = F1 审计封印族（post-verification-audit-seal ×1、round2-delivery-evidence ×2、round3-delivery-evidence ×1），由已跟踪工作区状态驱动，而该状态自 11:16 全量以来未变（3 文件 diff 一致，mtime 10:35–10:44）。
4. 结论：推断成立，无需实跑。残余限制（如实登记）：从未有过「CE 文件在场」的全量实跑记录；全量模式下测试间干扰属通用风险、非 FIXR2 引入；实跑代价 ≈10 分钟且触发 f1 patch 重写副作用（编排层明令禁止）。

## 5. f1 patch 与收口抽查

- `artifacts/f1-f12-repair/round2/patches/89bc0b64-to-r01-r10-source.patch`：工作副本 sha256 = HEAD blob sha256 = `25fb315f…`，目录 git status 干净——未被触碰。
- `npm run typecheck`（tsc ×3）exit 0；`npm run typecheck:core-contracts` exit 0（6 files）。
- 执行轮 3 测试文件定向复跑 **84/84** exit 0。

## 6. 未验证边界（如实登记）

全量 npm test 未重跑（§4 已给可核查依据）；四平台 CI / open server 冒烟（P08 范畴）；真供应商冒烟（P04-T07-2 继承 BLOCKED）；其他机器实跑可移植性（本机双 cwd 已验证解析与 cwd 无关，无法替代异机/CI）。logs/ 下全部产物（含两个 counterexample 与 command-log）被 `.gitignore:94` 忽略——提交面是否 force-add 仍为编排层待决事项（复验收 §8.3 已登记，本轮无变化）。C1 消费端纵深防御缺口仍开放（已登记、it.fails 承载回归提示，缺口修复时该用例转红属预期信号）。不承诺绝对无缺陷。
