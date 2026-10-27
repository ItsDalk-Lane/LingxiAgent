# RR3 A 第2轮全新修复简报

全文继承RR3_BRIEF/RR3_REVIEW_BRIEF及RR1/RR2 MASTER，不缩减F42 A1/A2同包及任何拒绝边界。先读A-01/REPORT、A-REVIEW-01/REVIEW及原始validator-fault-results.json（审查报告收尾时先读已保存原始记录，正式报告随后补读）。

## 独立FAIL与精确所有权

新独立rr3_a_review_01亲跑发现生产shell `validate_run_output_unit`、`validate_declared_run_root` 对git ls-files命令查询失败仍当成空结果，受控exit2/stderr时函数exit0，含tracked-source.rs根被接受；正常查询和还原查询均exit1拒绝。证据RR3/A-REVIEW-01/validator-fault-results.json及6命令日志。发现器本身ps/lsof/git异常拒绝已绿，不能用其结果代替shell边界。当前A=FAIL，不自签PASS。

全新第2轮实施者唯一所有者scripts/rust-tauri/r02_t08_legacy_entry_regression.sh及必要永久回归r02_run_output_regression.py；若需动run_output_sinks.py或xtask文件先登记原因，不无关重做已经正确的A1/A2。根因为归属校验外部命令异常未fail-closed；已有本轮A异常拒绝授权。Git写入/系统过滤/其他包/总控台账禁止。所有注入只能隔离副本。

## 修复与复验

最小补齐两处真实校验命令exit/stderr的显式错误传播；异常不能被替换成空输出/成功，查询到tracked内容仍拒绝，合法新鲜输出仍可用。永久回归需实际生产函数、真实Git/文件系统并在隔离环境仅替代出错外部git命令；目标旧异常红→修后拒绝绿→还原正常对照，记录退出/错误文本，不能手传理想FILE代发现器。

重新亲跑真实OS fd永久反例+三对照/父子孙/内外多sink/sourcecopy/旧tracked及untracked/源码脚本配置map增删改名/links非法根/发现异常，校验共享helper输入是否变化及runner身份。已正确且未受影响xtask A1可按逐项输入相同复用A新独立证据；受改动影响R02 E0s和真实发现器必须重跑，最终全链仍全新FINAL亲跑。阈值、cmp、!stable整体FAIL、排除范围不放宽。

证据仅RR3/A-02/，REPORT含每命令exit/输入工具链binary/loghash/边界及旧失败引用。完成交全新rr3_a_review_02（不能原review_01连续复审）。若第2轮也独立FAIL，交另一全新根因复盘者，不继续同一判断者。保留所有历史失败与未受影响正确修复。
