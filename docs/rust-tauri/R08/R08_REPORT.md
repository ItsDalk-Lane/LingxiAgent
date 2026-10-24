# R08 最终报告（R08_REPORT.md）

- 阶段：R08 旧 Electron 客户端接入与数据迁移演练
- 基线 git HEAD：`86840d1700397d3dd620a41f2b884df4cddd5cc2`（分支 `codex/rust-tauri-migration`；全部 R08 成果在工作区未提交，工作区为唯一副本）
- 日期：2026-10-23（T01–T06 前段 + 本续接会话 T06 收口/T07/T08）
- 台账：`docs/rust-tauri/R08/PROGRESS.md` 与各 PROGRESS-tXX.md

## 一、结论（如实）

**R08 未全绿放行；达到"实现到位项 READY_FOR_REVIEW + 未竟项如实 FAIL/BLOCKED/PENDING"的诚实关闭态。**

- T01–T06：已完成并经独立验证/评审（T05 评审 PASS；T06 评审 PASS-with-MINORS 且移交项 N1/N2/N3 已落地重绑）。**注意 §三-F 的治理修正**：T04–T06 的迁移绿证据原本只对夹具自造格式成立，T07 的 F-T07-1 修复后链条对真实 Pi 格式亦闭合（夹具路径零扰动，双锚保持）。
- T07：A/B 线束建成并跑出绑定绿证（15 任务：parity=12 / differs=0 / recorded=2 / B-failed=1），两个真修复（F-T07-1 阻断级、F-T07-2）落地；**126 个补充叶全部 required_behavior_pending（其中 9 叶在 evidenceRequired 注记已实测正路径份额图钉，非门禁）**——未完成全产品回归，如实留债（门规 R05 RR1 F25：末位阶段无 later stage，份额拆分不合法，须 full_original_behavior 或 pending）。
- T08：xtask R08 注册（18 命令 × 16 场景 × 126 叶阶段图）；最终门 `verify-stage R08` 运行结果见 §四（117 pending 叶决定 overall 非 PASS——这是诚实结果，不是门故障）。

## 二、交付物清单

| 交付物 | 位置 |
|---|---|
| 迁移命令（migrate，T05 已评审） | `lingxi-service migrate`（本阶段零改动） |
| 回滚/再前滚命令（T06，评审 PASS） | `lingxi-service rollback` / `reforward` |
| 回滚操作手册 | `docs/rust-tauri/R08/T06_ROLLBACK_RUNBOOK.md`（含 manifest 双哈希披露、queued 续投语义、印章条款①、恢复根语义） |
| T06 独立评审 | `T06_REVIEW.md`（PASS-with-MINORS；rev-t06-* 8 组复跑） |
| A/B 线束与探针 | `scripts/rust-tauri/r08_t07_ab_drill.mjs`、`r08_t07_real_format_probe.mjs`、`desktop/src/react/__tests__/r08/r08-t07-kernel-ab.test.ts` |
| UI 差异报告（A13/A14） | `T07_UI_DIFFERENCE_REPORT.md` |
| FEATURE_PARITY_RESULTS 更新 | `FEATURE_PARITY_RESULTS.json`（R08 增量） |
| 阶段图与注册 | `rust/crates/xtask/src/stage_maps/R08.json` + `R08_INPUTS.json` + xtask 注册（main.rs/runner_identity.rs） |
| A15 隔离证据 | `r08_t08_isolation_check.mjs` → a15-isolation.json |
| A16 版本清单 | `r08_t08_version_manifest.mjs` → a16-versions.json |
| 交接 | `R08_HANDOFF.json`、本报告 |

## 三、关键发现与治理修正（诚实账）

- **F-T07-1（阻断级，已修复）**：T04 夹具会话用自造 `entryType` 格式，真实旧栈是 Pi SDK 格式（`{type:"session"}` 头 + `{type:"message",message:{role,…}}`）；修复前真实数据导入全部落 legacy-unknown-block（原字节隔离、对话不可用）。**治理修正：T04–T06 时期"迁移链已验证"的表述对真实数据不成立（修复前）；修复（加性）后经真实格式探针红→绿闭合，夹具证据不受影响（t07-pi-fix-amend-proof-01：435/435/5206 + 双锚保持）**。
- **F-T07-2（已修复）**：客户端边界缺 10 条会话管理写端口旧→Rust 翻译；修复含连接门（无门时旧腿被误译 404，红证据在案）。
- **D-4（未决）**：Rust 侧 rename 标题跨重载回落 + 任务序列会话数变化——post-R08 定位。
- **D-1/D-2/D-3/D-5**：标题投影差异 / 迁移根无 agent 名册（Register 不随迁；补偿=正式入口补建同 id agent，D1 已证）/ 旧栈无模型拒切换（环境前置）。
- **门工具修复**：xtask 候选快照对工作树已删除 tracked 条目（任务前遗留 8 万脏态的 D 项）尝试哈希而失败——修复为跳过（候选=工作树现状）。

## 四、最终门（verify-stage R08）

- 命令：`cargo run --manifest-path rust/Cargo.toml -p xtask -- verify-stage R08 --evidence artifacts/rust-tauri/R08/verify-r08-final-01`
- 阶段图：18 真实命令 / 16 基础场景（R00 镜像）/ 126 补充叶（9 份额绑定 + 117 required_behavior_pending）。
- **预期且如实的结果：overall 非 PASS**——117 个 pending 叶按门规不可判完成（R13-F01：登记≠通过）。最终门结果快照与逐命令回执见证据目录（本报告落笔时门在运行；结果以 `verify-stage-result.json` 为准，若有个别命令红，逐条根因在 PROGRESS-t08.md）。
- 意义：门的非绿是**覆盖缺口的真实表达**，不是门失效；R09+ 逐叶补证据后同门可转绿。

## 五、PPG-1/2/3 终裁（T08 检查单必裁项）

| 项 | 事实（T03 证据） | R08 终裁 |
|---|---|---|
| PPG-1（预览写能力缺失） | 桌面预览链路 workspace_root=None → write/edit 工具不注册；drill-06/08 帧级红证据 | **接受为预览限定 + post-R08 修复**：修复=桌面预览 workspace 策略（产品决策），非本阶段授权范围；如实标注，不隐藏 |
| PPG-2（预览读授权零根） | read 工具已注册但 authorizedFolders 永不生效（零根策略） | 同上：预览限定接受 + post-R08 |
| PPG-3（远端 LAN 端到端不可认证） | loopback token 非 Local 恒拒 + 设备凭证无呈现通道 + ALF 拦截 | 同上：回环退路已裁（T03）；真 LAN 属产品/平台工作 |

## 六、A15/A16 验收

- **A15 试用仍隔离生产**：真实 `~/.lingxi` 未被任何 R08 命令触碰（全部证据回执的 home/work/scratch 均在 /private/tmp）；Rust 预览由环境变量显式门控、默认启动=旧内核（源码事实）；真实 home 只读摘要已存（a15-isolation.json；开工基线未采集如实披露，本摘要为 R09+ 首基线）。
- **A16 对照基线固定**：HEAD/二进制 sha/渲染层树摘要/契约文件摘要/双锚/阶段图 digest 全在 a16-versions.json；R09 不得重改模型协议或数据架构。

## 七、移交 R09+

1. 117 个 pending 叶逐叶补真实证据（同门转绿）；2. D-4 定位修复；3. PPG-1/2/3 产品修复；4. D-1 标题投影裁决；5. T07 D-3 扩腿（旧栈模型替身注入后的含聊天回合全量 A/B）；6. 迁移根 agent 名册补偿步骤入正式切换 runbook（已入 T06 手册语义域）；7. post-R08 卫生清单（#5/#7/#8、manifest 双哈希修复+锚重封、backup 残渣、/api/health 鉴权标注）。
