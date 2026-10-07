# RR3 A-02：F42 第二轮实施交接

状态：IMPLEMENTED / SELF_CHECKED，等待全新 A-REVIEW-02 独立验收。本实施者没有自签包级 PASS、accepted 或 R06_READY；首轮 A-REVIEW-01 的 FAIL 与全部历史失败原样保留。

## 输入、范围与结果

全文读取 RR3_A_R2_BRIEF、RR3_BRIEF、RR3_REVIEW_BRIEF、RR1/RR2 MASTER、RR3 台账/进度/交接、A-01 REPORT、A-REVIEW-01 REVIEW；亲读六份查询故障原始日志、validator-fault-results、原始命令记录、A-01 真实 fd 结果与新独立 A1 checkpoint 原证。候选 HEAD 为 b3ac0e6aeae8d7d530a6ab8657a5fa9ce4849f0b，分支 codex/rust-tauri-migration。

唯一生产修改是 `scripts/rust-tauri/r02_t08_legacy_entry_regression.sh` 与永久回归 `scripts/rust-tauri/r02_run_output_regression.py`；没有修改共享发现器、xtask、其他包、总控文档或系统，没有提交、推送或其他主树 Git 写操作。隔离夹具中的 init/add 只用于建立真实索引，没有 commit。

成功标准：两个生产校验不能把 Git 查询故障当成合法空结果；合法新鲜输出仍接受、已登记文件始终拒绝；真实 fd 输出归属、源码变化检出、源副本对称、原 classifier/binder/cmp 与 !stable 强制 FAIL 不削弱。

两处真实 `git --literal-pathspecs -C <repo> ls-files -- <unit>` 均在原路径检查之后显式执行并检查结果：**仅 returncode=0 且 stderr 完全为空视为可靠查询**；非零、不带诊断的非零、非零附带误导 stdout、exit0 附带 stderr 都以 `git ls-files query failed` 具名拒绝，无法启动命令也明确拒绝。成功 stdout 非空仍以 INDEX-TRACKED 拒绝，真正成功的空结果才接受。返回 shell 的拒绝码保持 1。

回归提取实际生产函数，在临时文件系统和真实 Git 索引运行；仅故障行的外部 Git ls-files 用 PATH 代理控制返回值，其他 Git 操作调用真实程序。每个校验分别跑 tracked/fresh 两根，正常→四种故障→恢复，24 条实际命令，全部退出码/时间/stdout/stderr SHA 在各轮 `validator-query/results.json`。正常和恢复行直接使用真实 Git。默认回归包含全部 24 条，不需要单独启用；生产 E0s 已接入默认完整回归。

## 真实红绿与受影响复验

`commands.json` 保存本轮 15 个顶层实际 argv、cwd、开始/结束时间、退出码、预期码与日志 SHA；下列均为本实施者亲跑。没有把零匹配、提前拒绝、编译失败当作目标红证。

| 检查 | 实际结果 | 原始证据 |
| --- | --- | --- |
| 新永久正确断言 + 隔离旧生产校验 | exit1；正常/恢复8条通过，16条故障未满足显式拒绝；含旧受控exit2/stderr接受tracked根 | validator-old-red.log、validator-old-red/validator-query/results.json、old-validator-snapshot/ |
| 修后两生产校验 | exit0，24/24，ignored0/filtered0；tracked正常/恢复各拒绝，fresh正常/恢复各接受，16故障全部具名拒绝 | validator-new-green.log、validator-new-green/validator-query/ |
| 当前真实OS fd永久反例与三对照 | exit0，4/4；旧untracked父子、仅子sink、旧tracked后代、无旧文件均绿；7非法根/tracked sink/lsof故障拒绝 | discovery-new-green.log、discovery-new-green/ |
| A2旧发现器目标反证 | exit1，旧untracked child JSON被父DIR吞掉，变更后绑定未改变；新两校验24条先全部通过，确实抵达发现器/binder边界 | discovery-old-red.log、discovery-old-red/old-untracked-parent-child/、discover-old.py |
| 恢复当前发现器 | exit0，4/4及24/24、7非法根、tracked sink、lsof故障绿 | discovery-restored-green.log、discovery-restored-green/ |
| 独立父子孙/多sink/内外根/合法脏树 | exit0；内/外×fresh/旧untracked孙/旧tracked孙6组，每组6个真实fd；52个源码/脚本/配置/map/新增/删除/改名/旧证据变异检出并恢复；36内容变异源副本对称 | extra-controls.log、extra/results.json、extra/各组真实bindings与sinks |
| 发现器异常与链接根 | ps/lsof/git三故障各exit1，链接根exit1；共10组控制全部满足预期 | extra/各bad-*日志、link-rejection.log |
| 删除/改名源副本对称 | exit0，双方同步删除/改名，相等且都不同基线；恢复双方都等于基线 | copy-delete-rename.log、copy-delete-rename-evidence/ |
| 生产E0s完整片段 | exit0，原62 classifier fixtures、原binding/FILE/root围栏及默认真实fd4+查询24全部绿 | e0s.log、e0s-extracted.sh、e0s/f42-real-fd/ |
| shell/Python语法、限定差异检查 | 各exit0 | shell-syntax.log、python-syntax.log、owned-diff-check.log |
| 工具链/工具身份 | cargo/rustc均1.98.1，Node v24.16.0；Python3.14.3/macOS27.0.1 arm64 | *-version.log、input-manifest.json、binary-manifest.json |

E0s自检脚本由当前生产函数、完整classifier和完整E0s区段提取，仅补运行目录及note/fail打印包装；不是完整 E1–E5 或 npm gate。源副本排除集合始终来自真实发现器；没有手写理想FILE替代生产发现结果、扩大排除范围、改门槛、删cmp、忽略全部untracked或重拍快照接受漂移。

旧校验红证顶层记录的 cwd 为仓库父目录：证据驱动初版取父路径多一层，随后纠正。该次实际执行显式指定隔离快照中的脚本；生产函数/Git根均取临时隔离路径，已抵达24个校验边界并产生具名断言红，未依赖cwd，不影响该目标反证。后续15条清单中的其余命令均使用仓库根cwd。所有失败记录保存，未覆盖挑绿。

## 未受影响 A1 的新独立证据复用

逐项核对 A-REVIEW-01 的原始输入清单：全部 xtask源码/测试/阶段图/Cargo配置，以及 lock、工具链、.gitignore、共享发现器，共17个输入与当前字节完全相同；见 `a1-reuse-input-equality.json`。共享发现器 SHA 为 5080aebb46b7505a884f809be7b09a0ae5c170ca33bae3162b98d17ca64ed061，未变化。两个本轮改动脚本明确不属于该复用集合，全部受影响边界已新跑。

按第二轮 brief 允许，复用新独立 A-REVIEW-01 已亲跑的 xtask121/0/0/0 和仓库内/外各50个checkpoint（7/8/15/20）、旧runner来源失配拒绝与恢复证据。亲读8份原始层结果并逐份核对 SHA、overall PASS、stable=true、无changedPath、全部commands PASS；`a1-reused-checkpoints.json` 保存其索引，共100个稳定checkpoint。Rust runner二进制原始身份存 `a1-binary-manifest.json`，它是引用证据，不冒称本轮新构建/新运行。`!stable`在main中的独立强制FAIL仍未改动。

这些 A1 受控RX映射只证明真实 Scope+verify 编排和输出归属，**不是正式 R05/R04/R03/R02 业务gate**；完整正式链必须由新的 FINAL 审查者亲跑。本轮没有复制大target、没有修改或清理历史用户内容。

## 绑定与限制、下一命令

`input-manifest.json` 对19个A关键输入保存SHA/字节数；`delivery-inputs.json`确认交接时全部相等。`owned-round2.diff`只列相对A-REVIEW-01输入的本轮修改，避免把A-01已正确改动归本轮。`binary-manifest.json`记录实际Python/bash/git/ps/lsof的路径与SHA；`manifest.json`绑定本目录证据。其他包并行修改，未宣称主树整体冻结。

本轮平台仅macOS，未跑Linux/Windows、LIVE、完整workspace/正式stage gate/N01–N16。未把环境FAIL或其他包结果改写成通过。状态维持SELF_CHECKED，请总控交**另一全新 A-REVIEW-02**，不能由本实施者自验或由A-REVIEW-01连续复审。

新独立审查先创建未存在的 A-REVIEW-02 目录，在仓库根亲跑：

```bash
python3 scripts/rust-tauri/r02_run_output_regression.py --evidence artifacts/rust-tauri/R05/RR3/A-REVIEW-02/discovery
```

再按RR3_REVIEW_BRIEF核对新查询正常/故障/恢复、实际发现器的全部反例与对照、A1同输入复用边界；冻结后完整正式业务链交全新FINAL，不用本轮局部绿签阶段放行。
