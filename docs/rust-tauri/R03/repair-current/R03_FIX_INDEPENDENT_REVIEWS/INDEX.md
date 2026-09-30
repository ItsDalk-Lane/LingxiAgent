# R03 修复轮独立审查权威索引（G01-R1 .. G06-R1）

- 建立：2026-09-30，由 G07-E01（EXECUTOR-REPAIR-R03-G07-E01）按派单第 7 项建立。本文件是本轮六份组级独立审查的**唯一权威索引**；本索引自身不是审查，不产生新裁决。
- 规则回执：每位 Reviewer 均为**全新一次性代理**，未参与对应候选的实现、修复或自查；审查期间候选冻结（只读产品/测试/配置/门禁，复测产物只写各自独立证据目录）。逐 F-ID/C-ID 的三层状态以总控账本 `docs/rust-tauri/R03/repair-current/R03_FIX_ISSUES.json` 为准（F01–F07 = CLOSED_BY_INDEPENDENT_REVIEW_R1；F08 = G07 本组，归全新阶段 Reviewer 终审）。
- 阶段终审（F08-C04）**尚未发生**：本索引只覆盖组级审查；`R03_FIX_FINAL_STAGE_REVIEW.md` 须由总控另派的全新 STAGE-REVIEWER 产出（七条主反例独立实测 + 原验收与递延复核），在此之前 R03 保持 REOPENED_PENDING_REPAIR。

## 审查报告一览（全部 VERDICT: PASS）

| 组 | 覆盖 | Reviewer | 报告（权威路径） | 复测证据根 | 审查时候选 | 修复提交（总控推送回执） |
|---|---|---|---|---|---|---|
| G01 | F01（取消树链接/继承）＋F02（父取消收尾、线程/配额、监督回收） | REVIEWER-REPAIR-R03-G01-R1 | `docs/rust-tauri/R03/repair-current/G01-R1_REVIEW.md` | `artifacts/rust-tauri/R03/repair-current/G01-R1/` | HEAD `cd3fb19e6` + 未提交修复工作树 | `520bb75b9` |
| G02 | F03（取消与完成统一竞争裁决） | REVIEWER-REPAIR-R03-G02-R1 | `docs/rust-tauri/R03/repair-current/G02-R1_REVIEW.md` | `artifacts/rust-tauri/R03/repair-current/G02-R1/` | HEAD `520bb75b9` + 未提交工作树 | `ccb09fde6` |
| G03 | F04（无回执工具退出归类 Unknown） | REVIEWER-REPAIR-R03-G03-R1 | `docs/rust-tauri/R03/repair-current/G03-R1_REVIEW.md` | `artifacts/rust-tauri/R03/repair-current/G03-R1/` | HEAD `ccb09fde6` + 未提交工作树 | `198e0da1e` |
| G04 | F05（受理/去重/持久化/后台派发两阶段绑定） | REVIEWER-REPAIR-R03-G04-R1 | `docs/rust-tauri/R03/repair-current/G04-R1_REVIEW.md` | `artifacts/rust-tauri/R03/repair-current/G04-R1/` | HEAD `198e0da1e` + 未提交工作树 | `d56e6883d` |
| G05 | F06（输入载荷保真与显式预算拒绝） | REVIEWER-REPAIR-R03-G05-R1 | `docs/rust-tauri/R03/repair-current/G05-R1_REVIEW.md` | `artifacts/rust-tauri/R03/repair-current/G05-R1/` | HEAD `d56e6883d` + 未提交工作树 | `8883923a5` |
| G06 | F07（后台 steering 接入授权收件箱） | REVIEWER-REPAIR-R03-G06-R1 | `docs/rust-tauri/R03/repair-current/G06-R1_REVIEW.md` | `artifacts/rust-tauri/R03/repair-current/G06-R1/` | HEAD `8883923a5` + 未提交工作树 | `8a6303bcd` |

提交/远程包含回执：`docs/rust-tauri/R03/repair-current/R03_FIX_COMMIT_RECEIPTS.json`（总控登记）。

## 执行者自查与组报告索引（审查的输入侧）

| 组 | 执行报告 | 普通自查 | 对抗性自查 |
|---|---|---|---|
| G01 | `G01-E01_REPORT.md` | `G01-E01_NORMAL_SELFCHECK.md` | `G01-E01_ADVERSARIAL_SELFCHECK.md` |
| G02 | `G02-E01_REPORT.md` | `G02-E01_NORMAL_SELFCHECK.md` | `G02-E01_ADVERSARIAL_SELFCHECK.md` |
| G03 | `G03-E01_REPORT.md` | `G03-E01_NORMAL_SELFCHECK.md` | `G03-E01_ADVERSARIAL_SELFCHECK.md` |
| G04 | `G04-E01_REPORT.md` | `G04-E01_NORMAL_SELFCHECK.md` | `G04-E01_ADVERSARIAL_SELFCHECK.md` |
| G05 | `G05-E01_REPORT.md` | `G05-E01_NORMAL_SELFCHECK.md` | `G05-E01_ADVERSARIAL_SELFCHECK.md` |
| G06 | `G06-E01_REPORT.md` | `G06-E01_NORMAL_SELFCHECK.md` | `G06-E01_ADVERSARIAL_SELFCHECK.md` |
| G07（本组） | `G07-E01_REPORT.md` | 轮次级 `R03_FIX_NORMAL_SELFCHECK.md` §G07 | 轮次级 `R03_FIX_ADVERSARIAL_SELFCHECK.md` §G07 |

G07 组（F08 验收接受缺口）不新增组级代码审查：其改动为门禁/文档/负向测试接入（`R03.json`/生成器/xtask 钉图测试/两个脚本/现行报告），组级三层=本组两层自查+由全新阶段 Reviewer 在 F08-C04 终审中一并复核（门禁负向行为有独立可重跑脚本与 /tmp 隔离副本原始退出码在档）。
