# RR3 A-REVIEW-01：F42 全新独立验收

结论：**FAIL**。A1祖先日志误报、A2父DIR吞旧child JSON的目标回归均已亲证修复；但本包归属校验还有一个必须关闭的异常拒绝缺口，不能把已有正常绿证当成包级PASS。主树A文件未改，注入只在本目录隔离副本；未修改业务、总控台账、Git提交/推送/分支/标签。审查者为全新 `rr3_a_review_01`，未参与A实施/修复，不自行修正此轮发现。

## mustFix：两个校验函数吞掉 Git 查询失败

对象：`scripts/rust-tauri/r02_t08_legacy_entry_regression.sh:599`（`validate_run_output_unit`）及 `:672`（`validate_declared_run_root`）。两处均把 `git ls-files` 命令直接放进 `if [ -n "$(...)" ]`；查询非零且stdout为空时，判断为假，随即 `return 0`。stderr中的错误并不改变返回值。

亲跑隔离根 `validator-fault-repo/artifacts/rust-tauri/R05/declared-run/`，其中 `tracked-source.rs` 已被暂存登记。除目标查询故障外，目录深度/存在性/普通路径均合法，生产函数从隔离快照中原样提取。仅负例以PATH中的git替身令ls-files返回2并输出 `controlled ls-files inspection failure`；其他Git操作执行真实/usr/bin/git，未替换发现器、校验函数或binder。

| 生产函数 | 正常查询（应拒绝） | 目标查询异常（应拒绝） | 恢复真实查询（应拒绝） |
| --- | --- | --- | --- |
| validate_run_output_unit | exit 1 | **exit 0，错误接受** | exit 1 |
| validate_declared_run_root | exit 1 | **exit 0，错误接受** | exit 1 |

这是实际抵达归属校验边界后的错误接受，不是零匹配、编译失败、提前认证拒绝，也不是“异常负例通过”。6次调用的argv、时间、exit、日志SHA在 `commands.json`；原始日志为 `validator-{normal-reject,git-error,query-restored}-<function>.log`；汇总 `validator-fault-results.json`，注入程序 `validator-faults.py` 和 `validator-fault-bin/git`。异常两次明确 `expectedExitCode=1 / exitCode=0`。

修复要求：先成功取得Git查询结果，再判断是否为空；查询失败必须具名非零拒绝。两处都修，不只修共享发现器（发现器自己的异常拒绝已经通过）。永久补充这两处查询故障对照；保持tracked拒绝、合法根通过、真实发现/binder、固定排除集和原有cmp不变。交全新修复者及之后全新审查者，本轮不复审自身判断。

## 实际完成的其余核验

隔离快照包含1563个输入，逐项SHA/字节数在 `input-manifest.json`。HEAD=`b3ac0e6aeae8d7d530a6ab8657a5fa9ce4849f0b`，分支 `codex/rust-tauri-migration`；A归属12个关键输入在审查末仍与主树字节相同（含lock/toolchain/.gitignore）；见 `delivery-inputs.json`。主树同时有F46等并行改动，**没有宣称总体冻结**。所有Cargo使用 `/Users/study_superior/.cargo/bin/cargo`，1.98.1、--locked；rustc1.98.1，macOS arm64，Python3.14.3。输出只本目录；二进制在独占临时target，摘要存 `binary-manifest-before.json` / `scope-binary-identities.json`。

已全文读取RR3 REVIEW/BRIEF、RR1/RR2 MASTER、RR3 ISSUE_MATRIX/PROGRESS/HANDOFF、A-01 REPORT；核对A相关生产源码/差异、A-01有效自检命令与原始真实fd结果、RR2原始R04/R03绑定JSON、RR2终审及B-R2独立报告。原始历史绑定亲读结果另存 `historical-binding-audit.json`：R04 8/8不稳仅父r04 stdout，R03 15/15不稳仅父r03 stdout；历史结论中的“只有ALF”不采为当前结论。

| 检查 | 实际执行与结果 | 证据 |
| --- | --- | --- |
| 生产永久真实OS fd反例及三对照 | 修后4/4绿、ignored0/filtered0；仅子sink、旧tracked、无旧文件对照均绿。提取HEAD旧算法后exit1，精确旧untracked JSON被吞断言红；恢复生产发现器4/4绿 | `discovery-green/`、`discovery-old-red/`、`discovery-restored-green/`及三日志 |
| 生产发现器异常/非法根 | 7非法根、tracked sink全部拒绝；lsof故障拒绝。独立另检ps/lsof/git查询故障各exit1，link根exit1 | 永久results；`extra[-02]/results.json`及stderr |
| 独立父子孙、多sink、内外根、合法脏树 | 6组（内/外×新鲜/旧untracked孙/旧tracked孙），每组6个真实stdout/stderr fd，全部增长稳定；52个内容/新增/删除/重命名/旧JSON变异检出并恢复；36个内容变异源/副本同步且绑定相等 | `extra[-02]/`、`extra_controls.py`、`extra-controls[-02].log` |
| 删除/重命名源副本对称 | 同时删除/改名，双方相等且各自不同于基线；精确恢复双方基线 | `copy-delete-rename-evidence-02/`、`copy-delete-rename-02.log` |
| xtask全套 | exit0，121 passed/0 failed/0 ignored/0 filtered | `xtask-all.log` |
| 仓库内受控四层 | 7/8/15/20 checkpoint，共50，全部stable=true、changedPath空、每条command PASS、overall PASS | `nested-internal/`、`checkpoint-index.json` |
| 仓库外受控四层 | 同50 checkpoint全稳；实际1 passed/0 failed/0 ignored/120 filtered，exit0 | `nested-external/`、`nested-external.log`、索引 |
| A1旧行为目标反证 | 隔离candidate只移除新增精确FILE排除条件（恰1处），真实Scope+verify四层分别7/8/15/20全部不稳且overall FAIL；最深20个命令本身全部PASS，却因20不稳而FAIL，变更路径仅祖先stdout。还原字节重建后50 checkpoint全绿 | `scope-mutation.json`、`scope-ancestor-old-red[-02].log`、`scope-old-red-artifacts-0/`、`scope-restored-nested[-02]/`、`scope-binary-identities.json` |
| 新helper来源身份 | 原编译测试二进制直接运行正常exit0→只改发现器源字节、旧runner exit101点名run_output_sinks.py→还原且不换二进制exit0。没有拿重建后的runner证明旧runner拒绝 | `helper-mutation.json`、`helper-{baseline-02,mismatch-red,restored-green}.log` |
| `!stable`独立强制FAIL保留 | 主main源码原分支仍明确强制overall FAIL，未被删除/改弱。受控旧行为红的最深层命令全PASS但overall FAIL亲证Scope/verify编排边界；**主main这条分支本轮为源码审计，未声称完整正式入口运行** | `delivery-inputs.json`、红checkpoint索引 |

所有生产binder清单来自真实OS发现器，未手传理想FILE。独立父子孙脚本调用原提取discover/binder；不排除整个artifacts或所有untracked。无旧文件时父DIR仅在当前后代全部获证后才产生；旧tracked/untracked孙文件存在时各层不得DIR吸收。原binding/cmp、候选前后摘要、source/copy对称保留。

## 边界、无效过程及下一轮

受控嵌套使用真实生产Scope、verify_stage_with_checkpoint、run_command和真实子进程；RX映射、有界printf是测试负载。**它不是正式R05/R04/R03/R02业务gate**，不生成正式入口的runnerSourceBinding；来源身份另外以旧编译runner真实失配拒绝覆盖。正式全链、workspace、完整N01–N16、Linux/Windows、LIVE本轮未执行，留总控冻结后全新最终审查者，不据此写阶段全绿/R06_READY=true。

自己的准备失败原样说明且不计产品红证：target_controls第一版中文bytes字面量SyntaxError、导入时argv索引错误，均发生在业务调用之前；helper-baseline第一次误选了非测试runner，exit2（日志保留，真实测试runner在helper-baseline-02成功）。copy-delete-rename第一次复制时把前一link负例解引用成目录，初始cmp正确拒绝，尚未到删除/改名；保留日志/夹具，第二版保留链接后目标验证通过。第一轮Scope红未及时抓取二进制SHA，保留过程日志；第二轮先锁定二进制SHA再实跑目标红与恢复绿，见scope-binary-identities。

全部已授权独立检查完成。本轮唯一mustFix是上述两个shell查询异常拒绝，不以正常查询拒绝遮盖它。A/F42保持FAIL、不能包级关闭；总控已收到具体问题，应由全新A修复者精准处理，再安排全新A审查。正式业务全链仍交全新FINAL。
