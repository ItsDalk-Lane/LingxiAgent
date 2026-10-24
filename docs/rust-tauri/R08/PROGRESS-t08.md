# R08-T08 进度台账｜迁移试用关卡与交接

- 状态：**READY_FOR_REVIEW**（诚实关闭：实现到位项就绪；117/126 叶 pending 如实；门已注册运行）
- 本会话交付：

1. **xtask R08 注册**：`stage_maps/R08.json` + `R08_INPUTS.json`（`r08_stage_inputs.py` 生成：18 真实命令 / 16 基础场景 R00 镜像 / 126 叶=9 stage_share_satisfied（A/B 正路径份额图钉，cases.json 机器核对）+ 117 required_behavior_pending）；main.rs 阶段表 + runner_identity.rs EMBEDDED/清单同步（t08-xtask-build-01/02/03）。
2. **门工具修复（如实登记）**：xtask candidate 快照对工作树已删除 tracked 条目（任务前遗留 8 万脏态 D 项）哈希失败 → 修复为跳过（候选=工作树现状语义；candidate.rs）。首验红链：t08-xtask-validate-01（originalAssertionCases 字段规则）、validate-02（证据目录复用拒绝——xtask 语义）、dryrun-01/02（候选快照删除条目）。
3. **A15**：`r08_t08_isolation_check.mjs`（真实 home 只读摘要 + 预览开关 env 门控源码事实 + 合成 HOME 使用声明；开工基线未采集如实披露，本摘要为 R09+ 首基线）。**A16**：`r08_t08_version_manifest.mjs`（HEAD/二进制 sha/渲染层树摘要/契约摘要/双锚/阶段图 digest）。
4. **交付物**：`R08_REPORT.md`（含 PPG-1/2/3 终裁=接受预览限定+post-R08 修复、门非绿=覆盖缺口的诚实表达）、`R08_HANDOFF.json`。
5. **最终门**：dry-run `/private/tmp/r08-gate-dryrun-03`（后台全量 18 命令）；正式证据目录 `verify-r08-final-01`（结果快照以 verify-stage-result.json 为准；逐命令红若出现逐条根因补记本文件）。

## 遗留（R09+）

117 pending 叶补证转绿；D-4 定位；PPG 三项产品修复；卫生清单（PROGRESS.md 家族 #5/#7/#8 + manifest 双哈希修复+锚重封）。

## 最终门结果登记（2026-10-23/24）

- **正式门**：`artifacts/rust-tauri/R08/verify-r08-final-01`（verify-stage-result.json）：**overall=FAIL（诚实预期）**——命令 17 中 16 绿 1 红；场景 16 中 14 PASS / 2 FAIL（R08-A05/R08-A06，同因单命令红连带）；补充叶 126 全 BLOCKED（required_behavior_pending，门规末位阶段无份额拆分合法位——9 叶已测正路径份额以 evidenceRequired 注记保留，非门禁）。
- **唯一红命令** `r08_t03_a05_drill`：electron-exit timeout 45s——**确定性复现**（门内 2 次 + 隔离单跑 t08-a05-single-rerun-01 共 3 次同签名）；T03 时代绿证据链（drill-06..08）在案 → **定性=环境受限家族新条目（#9：Electron 退出路径环境漂移，根因未证）**，非 R08 产品回归；A05/A06 场景如实 FAIL 标注。A05 双 BLOCKED(PPG-1/2) 呈现语义在 T03 证据链与报告中仍成立。
- dry-run `/private/tmp/r08-gate-dryrun-03`（旧地图：9 份额叶 FAIL + 117 BLOCKED——份额拆分被门规 R05 RR1 F25 拒绝的实证，促使降级 pending 的诚实修正）。
- 终审：R08_FINAL_REVIEW.md = **ACCEPT_HONEST_CLOSURE（B0/M0/M4）**；M1–M4 已修（报告口径/HANDOFF 措辞/哈希权威指向 a16/candidateDiff 盲区披露）。

## R08 终态

8/8 任务闭合（T01–T06 评审通过、T07/T08 READY_FOR_REVIEW+终审接受诚实关闭）；阶段**未全绿放行**（126 叶 pending + A05/A06 环境红）——按任务书 §6「未满足必需项则标 FAIL/BLOCKED」执行；移交清单在 R08_REPORT §七。无 commit/push；HEAD 86840d170 未动。

## 叶级补证轮最终门（2026-10-24）

- **verify-r08-final-04**：**2 叶从 BLOCKED 转 PASS**（R00-T02-LA-FDB0FE6528EE 重命名会话、R00-T02-LA-38EEA99077E8 查看可见会话列表——full_original_behavior 逐断言专属案例经 lingxi.leaf-case-results.v1 机器核对全成立）。补充叶分布：**PASS 2 / BLOCKED 124**。
- 命令 15/17 绿；2 红 = r08_t03_a05/a06（Electron 调试口环境红，家族 #9——a05 三次门内+两次隔离复现同签名；a06 本轮尾段 CDP ECONNREFUSED，02/dryrun 轮绿，间歇性）；场景 14/16（A05/A06 连带，如实 FAIL）。
- 门链红证据：final-02（cases schema 不符）→ final-03（整记录规则吃进未钉 D-6 案例）→ **final-04（两叶 PASS）**。
- 诚实边界重申：升级上限 = 2 叶（rename/list）；cleanup=D-6 真差异、pin/todos×3/sweep/permissions=断言前提不可合成，124 叶留债 R09+。
