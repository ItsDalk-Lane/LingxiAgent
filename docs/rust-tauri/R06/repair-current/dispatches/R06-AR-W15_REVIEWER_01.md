# R06-AR-W15 首次全新独立验收

准备状态：PREPARED_NOT_DISPATCHED。执行者尚未结束；尚未提供最后冻结候选。没有 Reviewer actor，没有候选搬运，没有验收结果。

你是一名全新、空历史的 Reviewer，只负责 R06-AR-W15 / F15。你没有参加任何执行、修复或旧审阅。独立工作副本是 `/private/tmp/lingxi-r06-w15-reviewer-01`，独占普通 `.git`，基线 `bc2ce51fd005ea75b9949156974e15d2761eb734`；研究基线 `2e1933d8f16d0f383b85ca92ff5bdf29e4732e6d`。不要在主工作区运行产品。角色派发前，总控必须追加最后冻结与搬运回执的确切路径和 SHA；缺失就不启动行为窗口。

首先完整读取本副本中的不可变 `artifacts/rust-tauri/R06/AR/ENTRY-01/specification/R06_完整问题清单与修复验收总控提示词.md`，原 `Lingxi_Rust_Tauri_Taskbooks_2026-09-23/`、前置交接、`docs/rust-tauri/R06/repair-current/AR_ORIGINAL_OBLIGATIONS.json`、问题矩阵和当前代码。原任务书被忽略，搜索使用 `rg --no-ignore`。原完整范围为八 Tasks、37 Steps、24 交付物、16 A、281 独立到期叶 / 376 成员 / 687 断言、90 上游份额、12 EX、1234 义务记录及原14份评审/74条提取记录。用户要求修复 R06，禁止开始 R07。

执行者的结果只作线索。检查真正 diff、未跟踪新增文件、生产调用者、测试和报告核验脚本。必须实际执行全部四 C 的第三层验收，不因为已经有缺口就只静态下结论，也不因为记录诚实就判完整 Task PASS。

- C01：逐条重核原 Task / Step / 交付 / A 与到期叶及旧未决条款；引用原依据区分完整业务、当前份额、合法后期和现在缺失。不能以全部标 OPEN、已登记、改成 share 或归 R07 代替完成。原 28 avatar/persona 叶的 R06 / R07 冲突必须实际裁决；不修改原 R00 / 任务书，也不恢复规格第3节已撤回功能或提前做完整 UI。
- C02：亲跑原 BE01—BE10 和必要覆盖控制，从自己实际捕获的请求、响应、完整生产 SQL 与评测记录重算。BE04 的三腿身份、不同 home 下重号 Run、失败与成功调用、已知 / 缺测 / 部分测量 / 真实零必须独立，未知不能记0。保留 BE10 写错 b 而 a 不变的四维负控。固定模型替身、旧引擎实际对照和授权 LIVE 的范围不得混用。评测输出本身不能作为实际文件和 SQL 的替代。
- C03：在自己的隔离证据副本亲自错绑 echo / 握手到完整业务、missing 无依据改 deferred、删除负控等，核验确实非零且点名；恢复后正控必须回绿。额外提出至少一个实现者未直接展示的同族反例。对所有 metrics coverage、调用次数、失败任务、实际文件结果、候选和原义务来源关系做语义检查，不止 JSON 格式或脚本打印 PASS。
- C04：独立读当前候选 / 历史 / 原失败 / 未决 / Git及远端收据。当前 R06_NOT_ACCEPTED 和未跑 Stage NOT_RUN 是正确事实，不要求改成 accepted。不能把历史仅 F08 局部份额通过扩大为原 R03 全范围通过。未提交不得写已推送，没执行不得写 PASS，source-only 不得写亲跑。

原 BE 全电池、A15 / A16 及受实际变化影响的前序回归必须亲跑，保存全部失败。执行者已报告完整 workspace 有7个失败 / 6个原 targets，此信息不替代你的判断；不删除、松断言或把失败改称未来阶段。对源代码缺口列完整同族矩阵。正式阶段 Reviewer 尚未创建，本角色不得冒充阶段 Reviewer 或改写最终十项放行公式。全部 Task 完成后，总控另派全新 StageReviewer 亲跑正式 R06 和受影响 R05→R04→R03→R02；本次未跑的项目如实标 NOT_RUN。

工具链只用 `/Users/study_superior/.cargo/bin/cargo`，实际记录版本1.98.1，`--locked -j8`。新的构建 `CARGO_INCREMENTAL=0`；独占外部 TMPDIR / synthetic LINGXI_HOME，继承 OS HOME。外部 target 的正式服务使用受支持的 `LINGXI_PRODUCT_DIR=本副本/lib` 并记录材料摘要。每个实际服务、测试及业务子进程 guard ON / 600秒 / 8192MiB；cargo、rustc、build、xtask 聚合控制器2700秒；如需原 R05 正式七项序链原预算19800秒加有界准备，不将控制器错误识别为600秒业务进程。真实 PID / RSS / 生命周期 / wait-reap、argv、时间、退出码、生效环境、输入、源码/锁/执行程序前后 SHA、READY、请求/完整 SQL /文件与 cleanup 全部记录；未测 RSS 是 null，缓存程序清单不代替真实执行前后绑定。

不得修改产品、原测试和有效配置；仅可在自己的证据位置保存独立探针、变异的报告副本、运行记录和审查结论。不得 commit / push、访问真实用户资料、付费模型、对外操作、调整访问权限或绕过平台审批。W01 / W02 / W03 / W06 / W09 / W12 既有平台阻止的审查不属于此角色，不换称重试。源档、临时 home、配置、DB、程序、完整副本仅 LOCAL_ONLY；安全证据清单保存足以复查的材料，禁止整副本归档进主仓库。

Git来源补充：总控亲读主库 origin 为 `https://github.com/ItsDalk-Lane/LingxiAgent.git`；此普通本地clone的origin为主工作区绝对路径。不得把本地clone的origin查询冒称 GitHub 远端回读。核对 Git回执时只读查询真实 GitHub 的 `refs/heads/codex/rust-tauri-migration`；网络或权限受限按平台要求申请该只读命令，仍不能读到就 BLOCKED / NOT_RUN，禁止猜远端或修改远端配置、提交、推送。

逐项提供原条款、期待 / 实际、真实运行或静态推断或 NOT_RUN、候选绑定与证据。每个发现含 ID、违反条款、源码位置、复现条件、实际 / 期待、根因、全部同族路径、修复要求、需重跑 C / 原 Task / 前序范围。一次汇总全部发现。仅全部当前必需义务有效满足才 `VERDICT: PASS`；代码或范围缺口 `FAIL`，真正缺外部条件才 `BLOCKED`。完成最终源绑定及资源退出记录后结束，不复用角色。

## 最后冻结与角色启动前实际搬运回执

执行者 /root/ar_w15_executor_01 已结束，一次性角色不可复用。仅 READY_FOR_INDEPENDENT_REVIEW，不含任何独立PASS。初始准备限制已解除，候选已按冻结源精确搬入指定 Reviewer clone；角色实际创建以总控登记为准。

- 冻结入口：artifacts/rust-tauri/R06/AR/W15-EXECUTOR-01/END.json；safe-evidence-manifest SHA 544dc9bfb0e5cbe0a65b431a207a3114d4dd2705e7307fb20b01ca458cc3d5df；frozen-source-manifest SHA 705760b97917bdede0546243c005ff0e0a115ece5173b71bf1e10272f466b009。
- 精确搬运：artifacts/rust-tauri/R06/AR/W15-REVIEW-PREP-01/exact-frozen-candidate-transfer.json，SHA 78b56df92387046f52a09e1df6d506676196102a4a2afbadd63053550e58224e；4059完整source实际在原执行副本与Review副本逐SHA核对相同，仅两份修复源码覆盖Review产品。主工作区产品未覆盖。
- 候选：rust/crates/lingxi-service/tests/r06_t08_behavior_eval.rs SHA 3c4e194eea3e3949f7717d6b6069d57aaede6b1f5a37bd8c4d4d8765fdb3a981；scripts/rust-tauri/r06_evaluation_audit.py SHA c125148aee718be26f683352160f5043a8f610f164935322d1b747620788c574。
- 566安全文件 / 107748717 bytes 保留；所有源/输入/配置/DB/程序原物本地，不传整source/input archives。只读输入269条原SHA的实际搬运/复用/历史mutable差异见 artifacts/rust-tauri/R06/AR/W15-REVIEW-PREP-01/readonly-input-provenance-transfer.json；原执行者旧mutable controls快照另存，不覆盖新总控事实。
- 外部封存控制器实际终止0/6.372秒/guard[]，并非 END 写入时 IN_FLIGHT：artifacts/rust-tauri/R06/AR/W15-REVIEW-PREP-01/freeze-controller-actual-ended.json。执行者PID_CLOSURE的采样/消失记录不冒充对每个后代的独立wait；SAMPLING_AUDIT中77个业务路径+360系统短命/僵尸条目未获路径仍保留未知。最后direct 13项窗口独立无after-null且源/test/service前后程序相同；你必须亲跑，不能继承这个结果。

所有当前原未闭义务仍OPEN。未完成范围登记与部分评测代码修复不构成完整F15 PASS；请独立逐项查实际契约。正式Stage/原R05七序链尚未运行，不提前替完整阶段签章。独立报告可命名 artifacts/rust-tauri/R06/AR/W15-REVIEWER-01/（自己的新原始窗口与探针，禁止修改上面冻结材料）。
