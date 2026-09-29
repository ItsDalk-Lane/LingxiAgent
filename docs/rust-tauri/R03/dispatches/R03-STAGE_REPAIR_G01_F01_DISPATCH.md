# DISPATCH: STAGE-REPAIR-R03-G01-F01（阶段修复子代理，一次性，单写者）

- WORK_ORDER: R03 阶段收口义务（T08 审查 R1 裁决移交清单）
- BASE_SHA: 77bf3bad19e65335269e3f1e7d1dbdbdaba1e259
- BRANCH: codex/rust-tauri-migration
- WORKSPACE: /Users/study_superior/Desktop/Code/LingxiAgent
- REPORT_PATH: docs/rust-tauri/R03/repairs/R03_STAGE_REPAIR_G01_F01.md
- EVIDENCE_ROOT: artifacts/rust-tauri/R03/STAGE-REPAIR-G01-F01/
- 派发时间: 2026-09-29
- 性质：单写者工作单，只做下列三项；不得 commit/push；不得自标 CLOSED/PASS。

## 工作单项（全部来自 T08 REVIEW_R1 §12 移交清单，先读原文）

1. **FINDING-1 文档更正**（三份阶段文档，不改代码）：docs/rust-tauri/R03/R03-T06_REPORT.md、R03_REPORT.md、R03_HANDOFF.json 中「父取消路径 subagent child run durable 行就地收口」类表述，更正为审查裁决语义：spawn_linked biased-select 先丢 drive future（监督层子任务立即停止，A06 成立）；durable 行由下一进程启动扫描按 T07 两阶段闭环诚实收口为 interrupted_needs_attention（重启闭环已实测）。更正处以变更说明标注，不删除原陈述的上下文。
2. **FINDING-2 等价断言**：R02 期资产 a07 检查器的 keyEvents 精确计数断言（(1,1)）相对 R03 生命周期过期——按 T07 已获审更新的库内等价断言 (1,1)→(1,2) 模式，修 R02 图引用的检查器/脚本资产（保持原公开不变量：auth 路径保护不降）。必须列出：原断言→新断言→为何等价（未降低保护）。
3. **FINDING-3 等价断言**：a14 风暴 200-only 断言改为 200-or-409（409 session_busy=现役同语义冻结，R03-T02 已验收）；同样列出等价性论证。

## 完成判定与必跑矩阵（T08 REVIEW_R1 §12 原文为准）

- 全量 verify-stage R02 --evidence <全新目录>：预期 19/20 绿、仅剩 E5 封印族红（审计坐标滞后=治理项，不得消红、不得伪造）。
- verify-stage R03 --evidence <全新目录>：exit 0（受影响命令重跑全绿）。
- workspace --locked 全量 + fmt/clippy/check-contracts/check-boundaries。
- 受 a07/a14 断言变化影响的定向测试。

## 红线

- 不修改 R03 阶段已验收测试断言语义（除上述两项经裁决的 R02 期资产等价断言）；不改阶段图 scenarios/leaves 集合；不动迁移指纹；不引入新依赖；不触碰 npm/桌面栈。
- 文档更正保持原文可追溯（diff 可审）。
- 禁止静默降级；失败如实报告。

## 交付物

修复报告（逐项 finding→根因→修改→验证 映射）、证据目录（命令+退出码+日志摘要）、候选变更清单（供总控冻结与阶段 Reviewer 复核）。
返回 READY_FOR_REVIEW 或如实失败/阻塞说明；返回后结束。
