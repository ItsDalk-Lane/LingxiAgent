# R05 RR3 总控与工作包简报（2026-10-07）

## 权威与成功标准

所有实施与审查者必须全文读取 RR1_MASTER_PROMPT_2026-10-04.md、RR2_MASTER_PROMPT_2026-10-06.md，继承原规格、16A、100+3 C、130适用叶、I01–I11、N01–N16、四层证据、独立验收及原§6.1/6.2。本轮只收口 R05，不实施 R06。原任务书/R00叶只读，历史失败保留。

候选 HEAD=b3ac0e6aeae8d7d530a6ab8657a5fa9ce4849f0b，分支 codex/rust-tauri-migration；远端同 SHA、开工工作树干净。保留 F31/F34/F41、F43 两处就绪问题及 F44 正确修复，不重做原28项。最新正式门禁 5/7、FAIL；“只剩ALF”结论有遗漏，不能继承为当前结论。

根因修改白名单：rust/、scripts/rust-tauri/、docs/rust-tauri/R05/、docs/rust-tauri/ORCHESTRATOR_PROGRESS.json。新证据 artifacts/rust-tauri/R05/RR3/<包>-<轮>/。必要邻接修改先向总控登记根因/所有者/授权。禁止无关重构、覆盖用户改动、弱化契约、扩大产品范围、吞异常、空过滤、ignore、伪造计数、替换生产入口。注入只限隔离副本。LIVE/平台延期沿原边界。禁止子代理 commit/push/branch/tag。

## 执行与证据纪律

总控仅协调、集成、记账。每包实施、每轮修复、每次验收、最终阶段审查使用全新智能体，空历史+此文件。禁止自验/连续复审自己的上一判断；失败→新修复→新验收，连续两轮失败另换根因复盘者。共享文件唯一所有者。

每项真实保存命令、退出码、测试名/实际数/ignored/filtered、时间、候选/工作树绑定、工具链/lock/schema/config/fixture/binary hash、日志摘要、替身边界及目标失败原因。测量失败记 UNKNOWN/BLOCKED，不填0。写各包 REPORT.md，不直接写总控矩阵。收尾向总控返回实现/证据/限制/下一命令。

原始规格在 artifacts/rust-tauri/R05/RR1/INPUT-adversarial-2026-10-04/specifications/（original_taskbooks 八份及10-02专项），只读；锁定工具链文件实际为仓库根 rust-toolchain.toml，cargo/rustc必须用 /Users/study_superior/.cargo/bin 的rustup代理1.98.1，Node v24.16.0/npm11.13.0。

必读 RR2_BRIEF/ISSUE_MATRIX/PROGRESS/HANDOFF；当前 R05 报告/映射/负测/性能/交接；RR2/FINAL-01/STAGE_REVIEW.md及verify-r05-2各层JSON；RR2/B-R2/R2/REVIEW-r2.md；RR2/D-R2/REVIEW-r1.md及probe；RR2/G-R2/I-MAPPING.md与NEG-GATE-RR2.md。历史报告原样保留并标历史。

## 文件所有权及工作包

A（F42两方向同包）：独占 rust/crates/xtask/src/candidate.rs、main.rs、verify.rs及必要xtask编排/测试；scripts/rust-tauri/r02_t08_legacy_entry_regression.sh及新增其归属回归脚本。A1：FINAL-01 R04_REGRESSION stable=false、8/8 checkpoint不稳，仅外层 r04_regression_gate/stdout.log变化；嵌套R03 stable=false、15/15不稳，仅父r03_regression_gate/stdout.log变化。main !stable独立强制FAIL。必须修R05→R04→R03→R02全链，绑定前确定完整有依据本次输出集，源/副本对称；可外置日志后归档，不得仅修R02/忽略stable=false。A2：真实发现器discover_run_output_sinks/dedicated任意深sink误证目录。永久反例run001父stdout、child/stdout、child/old-evidence.json（旧非忽略untracked、无声明根）：子FILE父DIR导致旧JSON被吞，变更binder仍相同。逐层证明目录整体归属，否则精确FILE；子目录未整体获证，祖先不得吸收。不得手传理想FILE冒充发现器修好。

A自检：当前忽略规则新鲜父/子/孙日志、多sink、仓库内/外证据根、合法脏树，增长各层稳定；旧tracked/untracked后代、源码/脚本/配置/stage map变化、新增/删除/重命名、符号链接/非法根均检出或拒绝。仅子sink/旧文件tracked/无旧文件三组对照；有效平台真实OS fd发现器与标准嵌套布局，保存前后绑定、全部checkpoint。不得排除整个artifacts或所有untracked、删cmp、重拍快照接受漂移。

B（N03可复跑）：独占 scripts/rust-tauri/r05_t08_negative_gate.sh及必要注入自检。主树7→6过期，当前24，RR2 G仅隔离24→23且16/16历史有效。从权威表解析唯一目标/计数，变异恰一次记录old/new，零/重复匹配直接失败，不能硬编码24。正常镜像绿→真实降计数目标镜像红点名→还原绿；缺/重复锚点不得假有效。历史负测保留，受改动影响证据重验。

C（F27/I10补证）：独占 rust/crates/lingxi-service/tests/r05_t08_resources.rs及限定新增资源测试/采样器与RR3/C-*证据，其他共享挂点需协调。FINAL-01已有160混合轮/18点/60连接回收/60running重启消解；普通取消恢复有 subagent_closeout::parent_cancel_closes_children_in_process_repeatedly_beyond_the_cap 与 late_result_fence::r03_a07_late_result_after_cancel_and_next_run_pollutes_nothing，修映射引用，不重报缺实现。全部实际进程/资源稳态义务仍缺：原序列仅service PID RSS/FD，worker结束才采父。逐项映射有效采样/负载/峰值/稳态/清理，充分复用，缺的限定补齐（存活worker、任务/permit、连接、临时文件）；已知保留/释放资源正反控制测量器。保持原100+取消/错误和混合负载、事前阈值/原始序列。不宣称泄漏、不造资源系统。

D（r00环境）：只读生产代码与系统状态，最小探针可放RR3/D-*，若确需测试改动先登记。非回环入站超时、历史差分支持ALF诊断；同文件名不证身份或授权。核对本轮二进制绝对路径/hash、监听地址/端口、过滤状态，最小探针定位；按已有授权准备该确切二进制允许入站操作，不全局关闭防火墙/跳LAN。需用户系统操作时先完成其余工作，最后说明精确对象/动作/证据。合理预构建/受监督运行，保留超时/被杀。

E（文档）：最终独占R05主报告/矩阵/HANDOFF/进度/回执/ORCHESTRATOR_PROGRESS。纠正“只剩ALF”，列全绑定失败、环境、补证；旧尚未终审标历史；更新FINAL路径/候选/时间/各层结果，修I10归属；保留历史FAIL、合法directed/E5，raw npm红不得全绿。总控矩阵仅总控写。

## 最终冻结与终审

各包独立PASS后冻结，全新阶段审查者亲跑原§5.3全部：fmt、clippy workspace all-targets locked -D warnings、cargo test workspace locked、xtask check-contracts、check-boundaries、前序R04及正式R05入口。最终目录必须新建，重跑换编号：

`/Users/study_superior/.cargo/bin/cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- verify-stage R05 --evidence artifacts/rust-tauri/R05/RR3/FINAL-01/verify-R05`

递归检查各xtask层commands/overall PASS、candidateSourceBinding.stable=true、全部checkpoint稳定、runnerSourceBinding.status=PASS、摘要/依赖引用有效无缺；列全部失败，不只首workspace。N01–N16及新增反例目标红→还原绿；未受影响同候选证据可复用，受影响必须重验。

只有原§6.1全部成立、新独立终审PASS且无必需OPEN/SELF_CHECKED/BLOCKED才能accepted/R06_READY=true；仅原许可LIVE/平台延期。按§6.2接口/版本/数据语义、绑定与错误/取消/未知交接。既有提交推送授权有文件回执依据，精确提交推送由总控核对后做，不强推/不混用户文件。纯文档回执证明生产输入相等不伪造tested SHA。环境必需阻断时false并列全项；成功停R05。
