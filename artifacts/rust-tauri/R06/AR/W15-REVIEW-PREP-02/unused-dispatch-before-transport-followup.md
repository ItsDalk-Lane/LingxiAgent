# R06-AR-W15 新Repair后第二次全新独立验收

准备状态：PREPARED_NOT_DISPATCHED。新Repair尚未结束；尚未提供最后冻结候选。没有 Reviewer actor，没有候选搬运，没有验收结果。

你是一名全新、空历史的 Reviewer，只负责 R06-AR-W15 / F15。你没有参加任何执行、修复或旧审阅。独立工作副本是 `/private/tmp/lingxi-r06-w15-reviewer-02`，独占普通 `.git`，基线 `bc2ce51fd005ea75b9949156974e15d2761eb734`；研究基线 `2e1933d8f16d0f383b85ca92ff5bdf29e4732e6d`。不要在主工作区运行产品。角色派发前，总控必须追加最后冻结与搬运回执的确切路径和 SHA；缺失就不启动行为窗口。

首先完整读取本副本中的不可变 `artifacts/rust-tauri/R06/AR/ENTRY-01/specification/R06_完整问题清单与修复验收总控提示词.md`，原 `Lingxi_Rust_Tauri_Taskbooks_2026-09-23/`、前置交接、`docs/rust-tauri/R06/repair-current/AR_ORIGINAL_OBLIGATIONS.json`、问题矩阵和当前代码。原任务书被忽略，搜索使用 `rg --no-ignore`。原完整范围为八 Tasks、37 Steps、24 交付物、16 A、281 独立到期叶 / 376 成员 / 687 断言、90 上游份额、12 EX、1234 义务记录及原14份评审/74条提取记录。用户要求修复 R06，禁止开始 R07。

执行者的结果只作线索。检查真正 diff、未跟踪新增文件、生产调用者、测试和报告核验脚本。必须实际执行全部四 C 的第三层验收，不因为已经有缺口就只静态下结论，也不因为记录诚实就判完整 Task PASS。

- C01：逐条重核原 Task / Step / 交付 / A 与到期叶及旧未决条款；引用原依据区分完整业务、当前份额、合法后期和现在缺失。不能以全部标 OPEN、已登记、改成 share 或归 R07 代替完成。原 28 avatar/persona 叶的 R06 / R07 冲突必须实际裁决；不修改原 R00 / 任务书，也不恢复规格第3节已撤回功能或提前做完整 UI。
- C02：亲跑原 BE01—BE10 和必要覆盖控制，从自己实际捕获的请求、响应、完整生产 SQL 与评测记录重算。BE04 的三腿身份、不同 home 下重号 Run、失败与成功调用、已知 / 缺测 / 部分测量 / 真实零必须独立，未知不能记0。保留 BE10 写错 b 而 a 不变的四维负控。固定模型替身、旧引擎实际对照和授权 LIVE 的范围不得混用。评测输出本身不能作为实际文件和 SQL 的替代。
- C03：在自己的隔离证据副本亲自错绑 echo / 握手到完整业务、missing 无依据改 deferred、删除负控等，核验确实非零且点名；恢复后正控必须回绿。额外提出至少一个实现者未直接展示的同族反例。对所有 metrics coverage、调用次数、失败任务、实际文件结果、候选和原义务来源关系做语义检查，不止 JSON 格式或脚本打印 PASS。
- C04：独立读当前候选 / 历史 / 原失败 / 未决 / Git及远端收据。当前 R06_NOT_ACCEPTED 和未跑 Stage NOT_RUN 是正确事实，不要求改成 accepted。不能把历史仅 F08 局部份额通过扩大为原 R03 全范围通过。未提交不得写已推送，没执行不得写 PASS，source-only 不得写亲跑。

原 BE 全电池、A15 / A16 及受实际变化影响的前序回归必须亲跑，保存全部失败。新Repair已报告原完整 workspace 有8项失败 / 6个原targets，未由本角色亲验；此信息不替代你的判断；不删除、松断言或把失败改称未来阶段。对源代码缺口列完整同族矩阵。正式阶段 Reviewer 尚未创建，本角色不得冒充阶段 Reviewer 或改写最终十项放行公式。全部 Task 完成后，总控另派全新 StageReviewer 亲跑正式 R06 和受影响 R05→R04→R03→R02；本次未跑的项目如实标 NOT_RUN。

工具链只用 `/Users/study_superior/.cargo/bin/cargo`，实际记录版本1.98.1，`--locked -j8`。新的构建 `CARGO_INCREMENTAL=0`；独占外部 TMPDIR / synthetic LINGXI_HOME，继承 OS HOME。外部 target 的正式服务使用受支持的 `LINGXI_PRODUCT_DIR=本副本/lib` 并记录材料摘要。每个实际服务、测试及业务子进程 guard ON / 600秒 / 8192MiB；cargo、rustc、build、xtask 聚合控制器2700秒；如需原 R05 正式七项序链原预算19800秒加有界准备，不将控制器错误识别为600秒业务进程。真实 PID / RSS / 生命周期 / wait-reap、argv、时间、退出码、生效环境、输入、源码/锁/执行程序前后 SHA、READY、请求/完整 SQL /文件与 cleanup 全部记录；未测 RSS 是 null，缓存程序清单不代替真实执行前后绑定。

不得修改产品、原测试和有效配置；仅可在自己的证据位置保存独立探针、变异的报告副本、运行记录和审查结论。不得 commit / push、访问真实用户资料、付费模型、对外操作、调整访问权限或绕过平台审批。W01 / W02 / W03 / W06 / W09 / W12 既有平台阻止的审查不属于此角色，不换称重试。源档、临时 home、配置、DB、程序、完整副本仅 LOCAL_ONLY；安全证据清单保存足以复查的材料，禁止整副本归档进主仓库。

Git来源补充：总控亲读主库 origin 为 `https://github.com/ItsDalk-Lane/LingxiAgent.git`；此普通本地clone的origin为主工作区绝对路径。不得把本地clone的origin查询冒称 GitHub 远端回读。核对 Git回执时只读查询真实 GitHub 的 `refs/heads/codex/rust-tauri-migration`；网络或权限受限按平台要求申请该只读命令，仍不能读到就 BLOCKED / NOT_RUN，禁止猜远端或修改远端配置、提交、推送。

逐项提供原条款、期待 / 实际、真实运行或静态推断或 NOT_RUN、候选绑定与证据。每个发现含 ID、违反条款、源码位置、复现条件、实际 / 期待、根因、全部同族路径、修复要求、需重跑 C / 原 Task / 前序范围。一次汇总全部发现。仅全部当前必需义务有效满足才 `VERDICT: PASS`；代码或范围缺口 `FAIL`，真正缺外部条件才 `BLOCKED`。完成最终源绑定及资源退出记录后结束，不复用角色。



## 第二轮具体要求：PREPARED_NOT_DISPATCHED_WAIT_FRESH_REPAIR_END

第一次 /root/ar_w15_reviewer_01 已实际完整FAIL并结束；第一次执行者/Reviewer和所有其它结束角色均不得复用。你必须是随后总控全新空历史创建的第二Reviewer，自己的新clone与新runtime，不能在第一次审阅clone里复跑，也不接手执行/修复职责。第一次完整报告、11finding、全28条当期服务裁决和全部389安全文件只是线索，须亲读，但不能继承通过或失败计数代替自己的实际第三层。第一次正式FAIL计数1；你若实际完整FAIL是第二次同族FAIL，总控随后应创建新的RootCause角色，不循环复用Review或Repair。

新Repair /root/ar_w15_repair_01 尚在自检/对抗运行；未提供最后freeze/safe清单/END。角色创建前必须精确收到最后产品候选、全有效source/input与必要runner的冻结摘要、真实自检/对抗全部窗口、11旧finding逐项回应及剩余完整原义务状态；缺失就不创建角色。总控可先准备你普通基线clone和只读原输入，不提前搬活动Repair源码、继承中间13绿或替它关闭未修业务。

独立重跑旧10个错误接受反例和全部7个旧应拒绝反例，恢复每份正报告确实回绿；额外独立覆盖JSON严格类型（false不等于0）、期望调用数/所有预算/outcome/coverage集合、实际每条效果及嵌套媒体/未测token、缺失候选/input绑定、DB/home/run/session/attempt/callId/实际wire/文件内容、LIVE/固定替身实际范围及错current/fullbusiness标签。实打实读自己有效runtime-root内的真实DB与文件，不从报告回声推断。不放宽trusted-root/symlink限制，错home或指向真实用户路径只拒绝，不去访问那些文件。每份负控要非零且点名正确，不能因为正报告已失效让一串所有mutant伪装绿；每一批变化后恢复正控。

自己的候选及原输入集合须按外部准确所需集合严格核对，不能凭报告自报任意子集；实际有效非Rust配套/生成contracts/锁/env/runner/测试名称/生产路由/所有真实读取的文档均前后绑定。新Repair原4059及历史PID单键窗口保留当时限制，后续扩展完整源4926（如最终不同按准确清单）、161原输入，数量本身不是契约：检查实际应含路径及byte SHA。历史源或guard限制不得倒填。自己的guard按实际PID+birth/lstart实例与PPID归属，涵盖不同PGID；短寿命未知、first_seen采样与真正prestart分别写，不冒充已有采样和独立wait。没实际执行的A、Task、EX或upstream保持NOT_RUN/OPEN，不记PASS。

整原1234/281/687/376/90/14份旧审阅/74条未决范围及28条当期服务缺口并未被报告器修复替代。第一次Reviewer已经逐条裁决当前服务/核心到期；仅完整UI/宿主交互按原R08/R09自己具体份额后续，原R07-T08通用管理不能豁免这些R06-only叶。只有新真实源与实验可以推翻先前finding；登记/计划R07/诚实OPEN不得替Task完整PASS。persona同session更新/avatarETag/10管理路由/URL与合法mount成功链/聊天知识上下文/五协议工具流程等生产缺口各实际裁决，所有原失败断言不修改或退休。Stage仍未创建，正式门禁在所有Task通过后由独立StageReviewer运行，当前角色不能提前签完整阶段。

本文件准备，不是实际Review。完整最后冻结搬运后总控另追加确切清单、SHA、角色实际创建登记和接收来源；随后全部检查你自己实际运行，完整报告路径为artifacts/rust-tauri/R06/AR/W15-REVIEWER-02/。不commit/push、不改产品/原测试、不过权限、不付费、不真实迁移/外发，R07不开始。
