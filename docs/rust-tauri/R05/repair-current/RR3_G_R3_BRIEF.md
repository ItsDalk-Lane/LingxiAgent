# RR3 G-REVIEW-03 全新冻结候选完整负测独立验收

仅总控明确派发后执行。你是全新空历史独立审查者，未参加此前任何实施或审查，不修生产代码、不自行复审失败判断、不派代理。全文读取 RR1_MASTER_PROMPT_2026-10-04.md、RR2_MASTER_PROMPT_2026-10-06.md、RR3_BRIEF.md、RR3_REVIEW_BRIEF.md、RR3_G_R2_BRIEF.md（G02 蓝本，其低磁盘约束已被本轮空间事实取代）、最新 RR3_ISSUE_MATRIX.json / RR3_PROGRESS.md / RR3_HANDOFF.md 和本文件；完整继承原规格、负测、§5.2/5.4/6.1，不实施 R06。

## 实际前置与冻结（本轮派发事实）

- A/F42（A-REVIEW-02）、B/F45（B-REVIEW-01）、C-F46（C-F46-REVIEW-01）、H/F47+F48（H-REVIEW-02）、I/F49（I-REVIEW-01）、J/F50（J-REVIEW-02）全部包级独立 PASS 且作者停写；E03 文档轮已经 E-REVIEW-04 独立审查（结论见总控派发消息，勿预写）。
- G-REVIEW-01 默认 FAIL（root 并行改写 PRIMARY 入口的协调失误）永久保留；G-REVIEW-02 真实 BLOCKED_BY_STORAGE：N01 有效（目标红 exit101）、N02 预构建 ENOSPC 未抵达（无效注入）、N03–N16/fullR02/fullE5/最终恢复未跑、默认 shell 实际 exit 未落盘（UNKNOWN）、记录器 exit1/观察器 exit143。两轮均不冲销、不可拼成当前候选结论。
- 磁盘阻断已由总控解除：cargo clean 主树（280.7GiB）+部分 RR2 tmp scratch 回收，Data 卷可用约 585Gi（回执 artifacts/rust-tauri/R05/RR3/TASK0/cargo-clean-receipt.txt）；NEG_TARGET=~/.cache/lingxi-r05-neg-target(约21G 暖缓存) 保留可复用。开工先亲核 df，仍不足时如实 BLOCKED 不盲跑。
- 本轮期间所有源码、脚本、被读入文档、总控台账停止写入；root 在你全命令与实际子进程停止前不写仓内任何文件。你的 CLI/过程元数据如需外置，放 /private/tmp 新目录，结束后归档并核摘要。
- 主分支 codex/rust-tauri-migration；候选=HEAD b3ac0e6aeae8d7d530a6ab8657a5fa9ce4849f0b+全部未提交 RR3 工作树改动（你要逐项记录 dirty 清单与摘要，不能仅以 HEAD 代替）。根 rust-toolchain.toml 锁 1.98.1，绝对 /Users/study_superior/.cargo/bin/cargo；Node v24.16.0/npm11.13.0、macOS arm64。Git 主树只读；原任务书/R00 叶只读。

## 必须亲跑当前默认十六项

真实生产入口：
`bash scripts/rust-tauri/r05_t08_negative_gate.sh artifacts/rust-tauri/R05/RR3/G-REVIEW-03/default16-01`

只允许全新目录；不能用 --case N03 代替默认全量、不能修改实际入口或换简化检查。主脚本创建的真实 Git/文件系统隔离副本与实际变异必须使用；额外注入只在自己的隔离副本。明确清除 R02_LEGACY_REGRESSION_MODE，使用原默认 full，不把 J 子进程 directed 环境带入本轮。

N01–N16 实际唯一身份完整、每项准确目标错误/实际非零/合法前置抵达；正常与恢复检查实际绿；默认 shell 真实 exit 必须落盘保存（G02 的 UNKNOWN 缺口）。N03 唯一权威计数 old/new 恰一次、镜像点名红、精确恢复绿；N06 初始绑定完成且首命令信号先于变异、运行中变异检出；N16 真实两次不同绑定旧 root 拒收、完整末尾 reset 消费同一 12 文件表逐项 cmp 恢复。编译失败/更早认证拒绝/环境其他错误不算有效目标红。

完整默认 R02（含 E5 全量）与最终恢复/还原核验：四组 R02 检查在 F47/F48/F49/F50 修复后应真实通过；若仍有业务失败逐条区分目标绑定失败与环境/准备失败，不把 R02 整体红冒称绿也不抹掉正确绑定结论；新产品/检查器必需缺陷交具体 mustFix 给总控。

## 复用与有限补证

逐项消费 A-REVIEW-02（真实 OS fd 发现器/父子孙 sink/旧后代/非法根/查询异常/源副本对称/121+checkpoint）、I-REVIEW-01（56+13、B15/41、N03、三次目标红还原绿）、B-REVIEW-01（41 控）、C-F46-REVIEW-01 与 H-REVIEW-02（当前资源对象 7fa13a…、160 轮/54+61 点/15 存活 worker、FD/TCP 假零红恢复绿、F46 六轮红绿）。复用必须以当前相关输入逐项相等证明；实际受影响才补验。据 RR2/G-R2/I-MAPPING.md、G01 映射与最新证据给 I01–I11 逐项"要求→断言→实际运行→结果→输入依赖"，I10 用当前 H375 输入相等的资源证据+两具名普通取消恢复测试+60 预算 408 重启消解单列。

## 边界与产物

- 磁盘充足仍不得复制主 target 进副本、不删用户/历史证据；NEG_TARGET 复用需每次变异/恢复后核实际重新编译来源，保旧 mtime 旧二进制复用不得充作还原绿；可设 CARGO_INCREMENTAL=0 但记录实际环境与二进制摘要。
- 交付 artifacts/rust-tauri/R05/RR3/G-REVIEW-03/REVIEW.md、I-MAPPING.md、真实 commands/exit/UTC/counts/ignored/filtered/目标原因、源码与二进制/日志摘要、无缺失引用清单；明确当前默认全 16 结果、额外补证、可复用边界、全部失败与 mustFix。
- G 通过仅包级，不等于阶段终审；最终仍须新 FINAL 亲跑 §5.3。完成停止写入，不做 Git 提交或系统权限操作。

## 派发时附加事实（总控，E-REVIEW-04 完成后）

- E-REVIEW-04 已 PASS 无 mustFix（E-REVIEW-04/REVIEW.md），E03 文档轮独立关闭；除你外当前无任何仓内写入者，root 在你停止前保持静默。
- 环境修正：你的会话 HOME 可能为 /var/root。开工先 `echo $HOME`；若非 /Users/study_superior，先 `export HOME=/Users/study_superior`（使脚本 NEG_TARGET=$HOME/.cache/lingxi-r05-neg-target 解析到既有 21G 暖缓存），并记录该设置前后的解析路径。cargo 一律绝对路径 /Users/study_superior/.cargo/bin/cargo。
- 磁盘当前约 585Gi 可用；开工与每个大步骤前核 df，仍按回执 TASK0/cargo-clean-receipt.txt 口径记录。
