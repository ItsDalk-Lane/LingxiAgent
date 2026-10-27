# RR3 G 全新集成负测验收简报

全文继承RR3_BRIEF、RR3_REVIEW_BRIEF、RR1/RR2 MASTER，读RR3各包实施/独立审查报告和历史RR2/G-R2/NEG-GATE-RR2.md、I-MAPPING.md。只R05，不R06，不重做原28项。你只独立验收，不能修生产对象；FAIL给具体mustFix后交全新修复者。

## 冻结与入口

总控派发时应确认A/B/C/F46生产输入停止修改。共享负测reset/汇总已修改、A影响N05/N06/N16，因此必须亲跑当前生产负测默认完整N01–N16，不能拿单N03或旧RR2 16/16替代。

真实入口：`bash scripts/rust-tauri/r05_t08_negative_gate.sh <全新证据目录>`。主脚本隔离副本/变异为被测生产流程，不能用手写理想FILE/简化脚本替代；所有额外注入同样只你的隔离副本。用绝对/Users/study_superior/.cargo/bin路径/1.98.1、--locked，主树源码/权威表与总控文档禁止写、Git禁止写。总控会说明证据是在仓库内新RR3/G-REVIEW-01还是先外置后归档；证据不得和正式FINAL同时写主树。

## 成功标准

默认实际身份恰N01–N16全部，目标错误原因点名、合法前置已过、实际退出非零，正常控与恢复绿。零匹配/编译失败/非目标认证失败不算有效红；受影响的恢复必须实际目标测试绿和字节还原证明。N03动态唯一权威old/new恰一次/目标镜像红/恢复1测绿，其他15不能假标通过。核对缺/重复/非法锚点拒绝，summary空/少项/重复身份拒绝（B新41独立控可在同输入证明后复用，必要变化重跑）。

保留N06中改源码stable=false整体FAIL、N16两次绑定摘要实际不同/旧root拒收，不能依赖环境R02整体FAIL当绑定证明。恢复生产文件逐项hash相等，改过生产输入的旧二进制须按runnerSourceBinding规则重构建，不能新源/旧runner。A真实fd永久反例、三对照、完整checkpoint和C测量假零负控及F46日志重启新增反例由对应新独立包报告覆盖，逐项映射有效输入/hash/平台/边界，不重复把受控编排当正式业务门禁。

报告RR3/G-REVIEW-01/REVIEW.md、完整真实command/exit/counts/target原因/UTC/toolchain/inputs/binary/loghash/manifest，列全失败。I01–I11完整映射以既有有效证据为底，I10必须使用本轮C/F46新独立补证与普通取消具名恢复；历史资源只有父PID不能声称全部进程。raw npm红及合法directed/E5边界原样保留。G通过不是阶段通过，最终仍新FINAL §5.3全链；任何必需外部环境阻断仍R06_READY=false。

总控静态线索（不是独立判断）：pristine列表含kernel/src/lib.rs，但reset_copy列表未含该文件；N06/N16均在隔离副本append注释。请独立核实所有实际变异文件的恢复/各例输入，不能把残留变异当原候选或只靠summary绿。若自有隔离副本精确恢复与目标对照可完整满足既有负测义务，限定补证即可；若影响默认生产负测属性有效性，给具体mustFix交新修复者。不要以本线索自动扩大产品或把无害注释宣称功能缺陷。
