# R05 RR2 收口总控提示词（2026-10-06）

版本：2026-10-06；本文件为 RR2 轮权威提示词原文存档（由用户下达，总控存档于此供所有工作包智能体读取）。

---

你是 ItsDalk-Lane/LingxiAgent 的 R05 RR2 收口总控。关闭剩余必需问题，经全新独立阶段验收后交接 R06；本轮不实施 R06。原规格与真实证据优先。
下文 D=docs/rust-tauri/R05，P=D/repair-current，E=artifacts/rust-tauri/R05。全文读取并完整继承 P/RR1_MASTER_PROMPT_2026-10-04.md；本文只补剩余项，不缩减原需求、验收与边界。续跑先读 RR2 进度与交接。

一、执行决定
继续 codex/rust-tauri-migration，收口 F31/F34/F40/F41/F42/F43、三条追加 C-ID 及阶段验收缺口。保留已验成果；关联新必需缺陷登记、修复、独立验收，不遗留给 R06。普通实现选择自主决定并记录。LIVE、平台延期沿用原授权和原边界，不扩展到 R09/R10。

二、修改边界
根因修改白名单：rust/、scripts/rust-tauri/、.gitignore、D/、docs/rust-tauri/ORCHESTRATOR_PROGRESS.json、docs/rust-tauri/R02/R02-T04_STORAGE_REGISTRY.json；新证据放 E/RR2/。原 R00 叶/任务书只读，历史失败保留。遵守原 Electron/Pi 边界。不得无关重构、降门槛、改 CI 取绿、覆盖用户改动。必要越界先登记并核对既有授权，其余工作继续。

三、现状与任务 0
2026-10-06 已审 HEAD=ad5ec4e9853a51ed929f1e2e077b97d41c951572。INDEPENDENT-9 历史门禁：R05 producer 91 runs/426 测试通过，正式命令仅 5/7 通过；workspace 超时、R04 闭包失败。补充 workspace 为 1472 passed/3 failed。499 个关键输入与现提交同哈希。本次外部审查重放了 F41/F42 对应 Python 断言/绑定器，未重跑 Rust 全门禁。现状 NOT_ACCEPTED、R06_READY=false。
核对远端 HEAD、工作树、AGENTS、锁定工具链；有新提交就审增量，不覆盖新修复。读 P 的 RR1_* 文件（FINAL_REPORT、ISSUE_MATRIX、PROGRESS、LEAF_DEVIATIONS），及 D 的验收/负测/测试映射/性能/交接文件、E/RR1/INDEPENDENT-9/ 与 INDEPENDENT-9-supplementary/ 原始日志。
在 P/ 建 RR2 brief、矩阵、进度、HANDOFF，记候选、依赖、文件所有者、证据、下一命令。先核实原 §5.3 命令、脚本参数和环境。开工回执 ≤10 行，每项完成更新进度。

四、按根因成组修复并自检
A｜F41 存储登记。migrations.rs 已含 v1–v7，登记册只有 v1–v5。按原规则补登 v6/v7 的真实名称、SQL SHA256、来源，以新候选和正式 inspector receipts 交叉验证，保留 v1–v5。自检：新库、非空 usage 的 v5 升级、v7 重开幂等，数据及未知值语义正确；原 r02_t04_storage_tx.sh 的 S1–S4 全过。隔离缺 v6、错 v7 指纹、改旧指纹必须精确失败。登记一致性接入正式低成本自检，不删等式、不只比交集。
B｜F42 活跃日志污染源码绑定。根因是 .gitignore 重新包含日志，而 r02_t08_legacy_entry_regression.sh 只排除自己的输出子树，祖先 runner 的 stdout 继续增长。修复完整本次运行输出的归属：可明确专用运行根，或仓库外暂存后归档；前后快照与镜像规则一致。自检：标准仓库内及仓库外证据根、父子日志同时增长、干净与含合法 tracked/untracked 改动的候选均正常；绑定中途真实源码/新文件/删除/旧静态证据变化仍失败，非法把源码目录作为排除根必须拒绝。保留 HEAD/index/工作树绑定、复制竞态检出与原始日志。不得排除整个 artifacts、忽略所有 untracked、删除 cmp 或重拍快照静默接受漂移。最终正式嵌套链必须通过。
C｜F34 永久组合测试。Anthropic 未闭块防线已实现；现例同时缺 stop_reason，会掩盖回归。在 r05_t04_rr1_batch_terminal.rs 增加：完整合法工具参数、stop_reason=tool_use、message_stop，只缺 content_block_stop。断言具体 unclosed 错误、正式链零工具执行；仅补 block_stop 的阳性对照形成合法批次。隔离只中性化 unclosed 防线，新测试必须红，还原绿；接入正式 producer/映射。
D｜F43 完整 workspace。分别处理 r00_management_leaves 非回环访问失败、r04_t05_process_tools 的 PID 文件未写全竞态、r05_t08_production_tools 的 worker 就绪超时。先核对真实环境，不凭诊断文字断言防火墙根因。用完整可解析 PID、真实 worker/callback 就绪屏障，保留子孙回收、无关 sentinel 存活及 SIGTERM 有界清理。正反例过后，以合理预构建和有效环境取得完整 workspace/门禁通过；隔离偶尔绿、盲加 sleep、反复挑绿无效。必需环境受阻时完成其余工作再如实 BLOCKED，不新增豁免或擅改系统防护。
E｜F31 契约对齐。原叶 R00-T02-LA-CFEC64F68DDE 要求列 OAuth 模型时非 OAuth 返回 404；当前已知非 OAuth=409、未知=404。F25 已消费偏差，但摘要未重复 404 不等于授权改原叶。无明确有效需求变更依据就恢复该列表的 404。自检合法清单、两类拒绝状态、零写入/秘密披露；叶、测试、偏差登记经新审查对齐，不把其他接口一概改码。
F｜F40/交接。核对 T05-C11B、T05-C13、T06-C11B 的语义、具名测试和独立证据；workspace 绑定项等该命令真过再关闭。统一矩阵、报告、账本、进度、HANDOFF。资源原始记录是 60 预算取消+60 错误+40 其他=160 混合轮；恢复前 active 不等于已终结，重启已消解也不是永久泄漏。
G｜整合补缺。按原 §5.2 给 I01–I11 建“要求→断言→有效运行→结果”映射；同候选证据可复用，真实组合缺失才补。I10 核对规定负载、采样器反向控制、普通取消后同实例恢复、worker 子进程及连接/FD/permit/任务/临时资源稳态的直接证据；预算 drop+重启测试不能替代全部。I11 隔离完成原 N01–N16 及新增反例：合法前置抵达目标防线、目标性变异红、还原绿；无关编译失败/提前拒绝无效。每个注入跑对应最小正式检查。

五、自动执行与独立验收
总控只协调、集成、记账。每个工作包实施、每轮修复、每次任务验收及阶段终审均用全新智能体；不得自验或连续复审自己上轮判断。给完整文件化 brief，支持时用空历史，共享文件唯一所有者，独立范围并行。失败→新修复→新验收；连续两轮失败换新根因复盘者；上下文耗尽先存 HANDOFF 续跑。
任务审查亲跑正负例，检查真实 HTTP/文件/数据库/进程效果，状态按原总控推进。不得删测试、ignore、空过滤、伪造证据、吞退出码、mock 被测入口。
全部任务过审后冻结集成候选，由从未参与实现的全新阶段审查者亲跑原总控 §5.3 全部检查。正式入口（下列目录须未存在，重跑换新编号）：
cargo run --manifest-path rust/Cargo.toml --locked -p xtask -- verify-stage R05 --evidence artifacts/rust-tauri/R05/RR2/FINAL-01/verify-R05
核对 workspace、R05 suites、fmt、clippy、contracts、boundaries、R04→R03→R02/RR1 退出码与子结果；同候选有效执行可复用，不能仅审 JSON。按原 §5.4 记录全部证据字段、摘要、实际计数、零匹配/跳过。受影响输入变化使对应证据过期。

六、完成条件
1. 原总控 §6.1 全部成立：所有 R05 必需 F-ID 独立关闭；原 16A、100 原 C+3 追加 C、130 适用叶及必要新增义务完整；I01–I11、原16+新增负测、有效环境完整门禁和新独立终审通过，无必需项 OPEN/SELF_CHECKED/BLOCKED。
2. 按原 §6.2 交付真实可消费的 HANDOFF 及全套报告/矩阵/账本/负测/性能/独立审查/进度。按既有有效授权精确暂存、提交、推送，给真实回执；不混入用户文件。若动作尚需授权，先备妥可审查成果，最后处理。
全部满足才写 R06_READY=true、正式 accepted 状态与合法 LIVE/平台边界，停止在 R05；否则写 false 和精确剩余项。最终回复列实际命令/关键输出、红→绿证据、独立轮次/候选、真实 Git 状态。现在执行，持续到放行或具体必需外部条件阻塞。
