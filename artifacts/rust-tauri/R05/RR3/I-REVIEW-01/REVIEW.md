# RR3 I-REVIEW-01 / F49 独立验收

**结论：PASS，仅 I/F49 两文件及必要相邻检查。mustFix：无。R06_READY=false，阶段仍 NOT_ACCEPTED。**

验收者 rr3_i_review_01，全新空历史会话，未参加 RR3 实现、修复或前审；派发原件保留在本目录 dispatch，未覆盖。只审不修，不连续复审。2026-10-07，亲跑起点 UTC 02:42:37；各命令的准确开始/结束时间在 commands.jsonl 及永久回归的 commands.json/result.json。主分支 codex/rust-tauri-migration，HEAD=b3ac0e6aeae8d7d530a6ab8657a5fa9ce4849f0b，未提交候选。

此次通过表示：漏恢复确实修好；原候选字节完整保留；异常拒绝、同步前提及来源漂移强制失败没有退化。它不表示默认 N01–N16、完整 R02/R05、workspace 或阶段终审通过。

## 读取与判定依据

全文读取 RR1/RR2 MASTER、RR3_BRIEF/REVIEW_BRIEF、RR3_I_BRIEF/I_REVIEW_BRIEF、根 ISSUE_MATRIX/PROGRESS/HANDOFF、I-01/REPORT、原脚本、当前两个被验文件、B 注入器/自检与独立 41 控驱动。另读原 R05 任务书全文、10-02 专项 §6/7/9/附录 C-D，以及生产 main、verify/run_command、Scope/Git/OS fd 发现器、runner_identity 和 B 权威镜像有关完整函数。

I 原旧红、最终 56、生产同步、N03、失败尝试、命令、输入摘要、原 log 和 manifest 均核查；历史清单 8284 条全部重新计算摘要，读取 652 个原 log/命令或结果文件。具体输入及范围见 read-inputs.json、history-audit.json。实现者的 SELF_CHECKED 只作线索，下列 PASS 均有本轮亲跑支撑。

义务对应 F49、原 N06/N16、I11、F26；相邻 B/F45/N03 与 T08-C13/C14 的身份、执行及范围保护。原 16A、100+3 C、原 16 负测、RR1 §6.1 放行公式未削减。

## 亲跑结果

| 检查 | 实际退出/数量 | 结论与证据 |
|---|---|---|
| 当前 shell 语法 | 0 | syntax 日志 |
| 原真实 snapshot/reset，追加 kernel 后比较 | 1，目标不相等 | old-independent-red；不是编译或前置失败 |
| 同一份 pristine，当前真实 reset | 0，精确相等 | new-independent-restore-same-pristine；未重拍 |
| 最终永久回归 --production-sync | 0；56 检查，0 failed/ignored/filtered | permanent-final；本审查亲跑的新目录 |
| 额外目录/查询/authority/真实僵尸边界 | 按预期拒绝 1、正常/恢复 0 | additional-independent；13 个额外实际命令，含旧红/新绿，不含 B15/B41 |
| B 原永久自检 | 0；15 控通过 | b15-independent |
| B 原独立对抗控，新目录亲跑 | 0；41 控通过 | b41-independent/result.json |
| 原 run_n03/xtask_test/reset/write_results | 外层 0；正常 8 绿→目标 1 红101→恢复 1 绿 | n03-independent；其余15未执行 |
| 审查者额外三次生产 runner | 0/PASS→1/FAIL→0/PASS | sync-independent-audit.json；每次实际1命令、1 checkpoint |
| 最终 N16 原追加语句→原 reset | 0→0；全部注册文件精确恢复 | own-final-N16-append/reset |

永久命令原文：

```bash
python3 scripts/rust-tauri/r05_t08_restore_selfcheck.py --evidence /Users/study_superior/Desktop/Code/LingxiAgent/artifacts/rust-tauri/R05/RR3/I-REVIEW-01/permanent-final --production-sync
```

该命令 UTC 02:42:37.546104—02:42:48.939602，exit0。56 是该回归的检查命令计数，不能换算成 56 个业务场景，也不能计作默认 16 项实跑。

## 旧红、新绿与恢复完备性

独立从 I 开工原件提取未修改的 snapshot_pristine/reset_copy，建立本轮新小副本；先加合法候选脏字节，再执行生产 N06 追加语句。原 reset 后 cmp exit1：

- pristine/合法候选 SHA：1480c0dc2b88510e400a549d54d20fb5d2c72005da0b48a0b54af2a809744b1b。
- 原 reset 后 SHA：48a8f2bae8ee91a46a08bdc21bec82dd9c9c4f1590fbe7090c2fac956e39d523。
- 不重拍、不换 pristine，调用新原函数后恢复为第一个 SHA。

见 old-new-independent.json。原件 SHA=4853e129e2d0cf7dbd6c93c8c158186fad23e6f231a03ff64f140cca61e0042f，与 G 旧 COPY 脚本相同。没有依赖作者写好的 old-red-command 来替代本轮提取与运行。

逐项读全部变异正文，并独立扫描实际写入路径：11 个实际目标、12 个注册文件、无遗漏无重复；额外一项是原有 binary-wiring 测试文件安全超集。见 mutation-scope-independent.json。

| 场景 | 变异文件 |
|---|---|
| N01/N07/N14 | R05 stage map |
| N02/N03 | pins |
| N04 | service credentials、stage suites |
| N05 | 新证据目录内容，未变候选源码 |
| N06/N16 | kernel lib |
| N08 | service lib |
| N09/N11 | tool_render |
| N10 | openai_completions |
| N12 | runs |
| N13 | R04 stage map |
| N15 | adapter credentials |

快照和 reset 消费同一清单；禁止重拍；每项复制后实际 cmp。永久回归实跑原 N06/N16 追加、全部12文件非文本字节变异、两次幂等恢复、tracked dirty kernel 与 untracked 含 NUL/非 UTF-8 文件，恢复后原字节和真实 Git status 相同。不从 HEAD 恢复候选，不删除几行注释敷衍。

永久负控覆盖 cp 失败（含精确 kernel 目标）、cmp exit2、cp 假成功、缺 pristine、snapshot cp/cmp 故障、四个 Git 身份查询故障，拒绝腿均非零，恢复腿均精确相等。本审查另补 mkdir exit23→snapshot 明确拒绝→真实 mkdir/cp/cmp 恢复绿。最后 N16 之后源码确有 reset_copy 再汇总，本轮另实际执行原 N16 追加及终末 reset，全部注册字节相等。

函数提取只证明还原与同步单元边界；上述文件覆盖不是完整16场景执行证据。

## N06 同步与独立来源失败属性

源码顺序为：Scope::new 创建/定义本次输出 → before snapshot → runner 身份校验 → verify 按实际 authority 输出首命令启动行 → run_command 创建命令目录、检查新鲜度、开日志、启动命令。目录存在无法证明 before 已完成，新同步不再依赖目录出现和盲 sleep。

本轮最终永久回归，以及审查者另外三次执行，都用新源码副本、新构建的真实 xtask/main/verify/Scope/Git 和真实 OS fd 发现器。仅新副本 R02 authority 使用 RX 控制 map，让真实子命令在释放文件出现前保持运行；未替换生产 entry、binder、runner 或发现器。此 map 只隔离同步与来源机制，不代表原 R02 业务 gate。

另三次把 runner stdout/stderr 实际放在自身副本中；真实发现器给出精确 FILE 排除，未手传理想 FILE。每次从该 copy authority 首场景首 commandRefs 的真实 argv 推导 expectedSignal；原 wait 函数成功后，审查者再次读取实际日志、记录捕获时刻、查询真实 ps 状态并执行原存活检查，才执行生产 N06 原追加。

关键红腿时序：

- before.atUnixMs=1791341241824（UTC 02:47:21.824）。
- 实际捕获启动输出 UTC 02:47:22.045390，进程存活且非 Z。
- 原追加命令开始 UTC 02:47:22.056857，晚于 wait 成功和实际捕获。
- 唯一变化路径 kernel lib；前后 SHA、原 signal、argv、PID/ps、日志和 result 摘要全部在 sync-independent-audit.json。

| 腿 | 子命令 | runner来源 | checkpoint | stable | overall/exit |
|---|---|---|---:|---|---|
| 正常 | 全部PASS/exit0 | PASS | 1稳定 | true | PASS/0 |
| 运行中追加kernel | 全部PASS/exit0 | PASS | 1不稳定 | false | FAIL/1 |
| 原reset后重跑 | 全部PASS/exit0 | PASS | 1稳定 | true | PASS/0 |

这是真实独立属性证明：**全部命令成功仍因来源不稳 overallFAIL**，不借业务失败、环境失败或单纯 unit 断言证明它。生产 main 的 !stable 强制 FAIL 原逻辑未改。三个回合使用同一 binary/authority，恢复使用同一 pristine。

非法前提均亲验：invalid JSON、空 scenarios、不存在 commandRef、非字符串 argv；根已出现但无启动输出；日志不存在/grep exit2；ps exit2、ps exit1却有状态输出；已退出、曾有真实启动输出后退出；以及通过真实 fork/ps 观测到 Z 的未回收子进程。均拒绝；真实存活和 authority 恢复正常。没有为正常腿自造启动行，没有只等 evidence root，没有盲等注入。

## B/N03 无退化

run_n03 与 write_results 从 I 前原件到当前候选逐字节相同。默认 CASE_SCOPE=ALL、源码16个 record_case 精确覆盖原 N01–N16各一次、N03单项分支仍只登记1项。独立41控覆盖旧7→6、未来42→41、空白/CRLF、缺/重复/冲突及非法计数；汇总空集、错误身份、重复、BAD、逐一缺失16种形状拒绝，完整身份夹具与单N03夹具正常。

这些汇总 fixture 只证明汇总保护，不算真实16实跑。本轮未执行 clone 准备段或共享 NEG_TARGET；从当前原脚本提取 run_n03、xtask_test、reset、write_results，在自己的源码副本与 target 实际执行 Cargo。

实际结果：正常 **8 passed/0 failed/0 ignored/113 filtered**；权威唯一目标 svc:r05_t01_model_plane **24→23、matches=1、mutations=1**；目标 stage_map::map_tests::r05_stage_pin_table_matches_the_registered_suites **0 passed/1 failed/0 ignored/120 filtered，exit101**；原字节恢复后精确单测 **1 passed/0 failed/0 ignored/120 filtered，exit0**。红腿点名目标及 drifted，非编译失败/零匹配。summary scope=N03、cases=1、其余15在 unexecutedCases；正常/恢复输入清单相同，三腿实际测试 binary SHA相同。

证据 n03-independent-result.json、n03-inputs-before.json 和各腿原日志。生产镜像仍比较完整 suite 注册集合及计数；64个 lib pin 每项1测的要求保留，未改 pins 或镜像预期。

## 输入、工具链和摘要

21个局部受保护文件前后 SHA相等，包括 I/B文件、xtask源/图、发现器、锁、工具链、pins、主 .git/HEAD/index；主 HEAD前后同 SHA。见 source-before/after.json。H 并行可能修改自己的 service/smoke，**不签主工作树完整冻结**；本轮实际输入只绑定自身新 copy 的806文件清单、控制 authority 及实际 binary，见 tested-copy-source.json。

| 对象 | SHA256 |
|---|---|
| 当前 negative_gate.sh | a69110b81298705a9161652d28402a5e5a585f945fdfd214ccf071132ecb67dd |
| 当前 restore_selfcheck.py | 0cd24624bf7d6079e3cf7b4e6cf693222026db8261ffc60691bdc77fde0f57a4 |
| 本轮真实生产 xtask | 47adbebcec2fdc1c05c4ca25db8fb80e5119be3541015706be646d7c5a254e05 |
| 本轮N03实际测试 binary | 751afe48cb7e4a7dde76c614dc348413078beec0f8d107afc1888aed6ce1d63f |
| Cargo.lock | 259f983e98a4da13eab1b592c2efd7b79579ad6678a08995a4b9e3fca618eff3 |

工具链真实 rustc/cargo1.98.1、Node v24.16.0/npm11.13.0、Darwin arm64；Rust检查 --locked/offline；自有 target，未读写/构建 G 的共享 NEG_TARGET。没有复制大 .git/target、删除历史或用户数据、修改主源码/currentdocs/主Git/系统、安装依赖、派agent或外发。新夹具内 Git init/add/commit 仅用来构造真实 tracked/untracked 测试，未操作主库。

## G 首轮失败必须保留的边界

亲读 source-drift-default-failure.json、default-console.log、default-command.json、case-results.tsv 和 N06原证；G旧 COPY脚本 hash=4853e129…，确实仍是 I开工原件。I报告“G旧输入未改”仅在 **COPY** 语境成立，不能外推 PRIMARY 实际运行入口。

G默认命令实际读取 PRIMARY negative_gate.sh 时，root同时安排 I改写该 PRIMARY 文件，造成运行中来源漂移、line596/597 command not found、line598解析错误；**defaultExit=2，真实仅15行，N16reuse未执行**。这是root协调失误。旧COPY、已完成前15及两次R02部分原证没有因此被抹去，但不能算16/16，也不能套当前新候选。本包PASS不覆盖、冲销或修复G首轮FAIL。

G旧N06真实20个checkpoint均不稳、唯一kernel变化仍有效；其中真实R02业务失败另行保留，不能把它当 !stable独立强制FAIL的唯一证明。本轮通过用子命令全部绿的真实受控生产运行另补了该独立属性。

I/H另新独立通过后，root须按既有承诺静默冻结全部执行输入，包括 PRIMARY entry/scripts/实际读入docs，再由**全新G-REVIEW-02从头默认16**；其后另新阶段终审按原§5.3。此次不补跑旧G、不套旧证据，不负责耗时默认16或FINAL。

## 失败记录与证据完整性

作者原 selfcheck-02缺R00台账、selfcheck-03字段名读取错误、早期缩进失败均保留；只有最终有效56与本轮亲跑用于通过结论。

审查者自身 additional harness 首次中文 bytes字面量语法错误、finalize harness 首次列表表达式语法错误，均 exit1且未进入检查；初版脚本与失败日志保留，改正记录器后才运行有效检查。history-audit首次要求历史8284条全不变而exit1；实际仅3个后来由派发记录器更新的 dispatch/events.jsonl、request.json、stderr.log变化。修正本次审计断言的范围后，**仍如实保留三条差异**；8281个非dispatch条目完全吻合，不修改旧manifest、不伪称8284全相等。上述均不是被验两文件的产品失败，未算绿。

每个实际运行的 argv/cwd/UTC/exit、实际计数、输入/binary/result/log SHA均在 commands.jsonl、permanent-final/commands.json/result.json及各专项JSON。manifest.json索引本轮自有来源与证据、实际二进制；自动dispatch和派生构建缓存单列排除，未覆盖dispatch。历史摘要差异见history-audit.json。

**包级无mustFix；独立PASS交付后停止写。完整默认16、正式业务前序闭包、workspace、FINAL、LIVE及其他平台本轮NOT RUN。R06_READY=false。**
