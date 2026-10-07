# RR3 G01 中断历史归档与归因

结论：**只读归档完成；G-REVIEW-01 默认执行仍为 FAIL，exit 2，只有 N01–N15 共15行。旧 G 代理外层结束码 UNKNOWN，无最终报告。R05 NOT_ACCEPTED，R06_READY=false。** 本报告不补造旧审查者结论，不签 G02，不运行 Cargo、负测、构建或业务探针。

归档者 `/root/rr3_g_interruption_01`，2026-10-07；只写本新目录。未改旧 request、manifest、events、日志，未触副本、缓存、生产、currentdocs、Git 或系统。全部引用是他人历史实际运行，由本人读取原件并核查完整性，不能称本人亲跑。

## 权威与判定边界

全文读取 RR1/RR2 MASTER、RR3_BRIEF/REVIEW_BRIEF/G_BRIEF/G_INTERRUPTION_BRIEF、最新 HANDOFF/矩阵、TASK0 中断恢复记录、I-REVIEW-01 及 R02-TRIAGE-01 完整报告；读取 RR2/G-R2 I-MAPPING/NEG-GATE、RR3 A/B/C-F46/D/E/I 最新独立报告及 H 实施报告。权威为 RR1 §3.3、§5.2–5.4、§6.1、附录C，RR2 §四G，以及本轮中断 brief；原16A、100+3C、130叶、I01–I11、原16负测完整保留。旧报告的 PASS 不能修改权威或给新输入放行。

[原件摘要](raw-reference-digests.json)记录369份选定原件的实际字节数/SHA256。只读程序完整读取默认每例顶层原日志、全部28份补证回执和日志、91份恢复 producer 原日志、四份真实 R02 结果及全部失败的具体下层原因；没有扫描无关上万重复输出，也未凭旧大 manifest 宣称整库冻结。准确命令和限制见[只读操作记录](READONLY-COMMANDS.md)。

## 中断与已结束子命令

旧 `dispatch/request.json` 仍 RUNNING，518行 events 没有本轮最终完成回执；`dispatch/final.txt`、`REVIEW.md`、`extra-complete.json` 不存在。TASK0 记录旧42187/H3546不存在、ps无对应进程、外层exit null，不能继续盲等旧 RUNNING。events 未闭合的命令是 `observe-supplement-processes.py` 和 `extra-driver.py`，退出均 UNKNOWN。

最后 events 文件时间约02:58:17 UTC，**不等于所有子命令在此时停止**。`I06-existing-approval/command.json` 实际结束02:59:04.341910 UTC，exit0、11 passed/0 failed/0 ignored/0 filtered，日志SHA相等。这个完整子回执应保留。下一条计划中的 `I06-existing-worker-permission` 没有目录、日志或回执，记 **NOT_OBSERVED / UNKNOWN，待 G02**；不能用 `extra-driver.py` 的计划语句当已执行。

`write-review.py` 只是未执行的报告生成草稿，不是验收报告。其中有尚未取得的 worker-permission、final-input-boundary、actual-evidence-audit、MANIFEST 引用及过时A01归因，不消费其 PASS 或完整交付措辞。A01应以已完成 TRIAGE 的 INFO 缺失根因为准，不能说“允许的 instance/log/tmp 脚手架使严格目录检查失败”。

详见[中断索引](interruption-index.json)、[补证完整索引](supplement-index.json)。28份补证均有开始/结束/exit且28/28日志SHA匹配；27份输入前后相等，唯一不等是刻意中途变异的 stable-midbyte-red。`supplements-complete.json` 仅覆盖前12命令、02:56:16截点；它不是最后额外驱动完成回执。

## 默认 N01–N16 的真实边界

默认命令 UTC 01:25:56.935082→02:28:48.697982，cwd主仓库，`bash scripts/rust-tauri/r05_t08_negative_gate.sh artifacts/rust-tauri/R05/RR3/G-REVIEW-01/default16-01`，exit2。默认console SHA=`23c81ff55745906608c09cca28708f6a1b0f68b6fb31dcc98b068bea9e9aa3ed`，本人复算相等。

PRIMARY脚本运行时被并行 I 修复改写，console明确line596/597 command not found、line598 syntax error。原脚本SHA4853e129…→新a69110b8…；COPY仍旧输入不等于PRIMARY冻结。此为总控协调失误；不抹掉失败、不将15行加补证拼为16/16。

| 默认项 | 原exit | 完整原证观察及边界 |
|---|---:|---|
| N01 | 101 | 删除A16，具名镜像0过/1失败/120过滤，点名R05-A16 |
| N02 | 1 | 真实完整producer遇刻意零匹配，gaps点名filter matched 0 tests；非以cargo零测试算通过 |
| N03 | 101 | mutation为唯一24→23、1次；镜像0/1/120过滤，原字节恢复1/0/120过滤exit0 |
| N04 | 1 | 真实handle_refusal断言失败且吞退出码，producer按计数/缺ok行拒绝；补证直接目标101→0 |
| N05 | 1 | 旧证据根not empty明确拒绝 |
| N06 | 1 | 真实R02完整20命令、20检查点均不稳，唯一kernel变化，runner PASS；业务7败另列 |
| N07 | 2 | 删除r04_regression_gate，场景引用unknown command硬拒 |
| N08 | 101 | 正式binary 1过/1败，具名真实链失败；同时lib358/0 exit0，证明unit绿不足 |
| N09 | 101 | runtime nonce具名0过/1败/11过滤 |
| N10 | 101 | binary配对具名1过/1败、0过滤，call_bin_1目标 |
| N11 | 101 | stop-unconfirmed诚实状态0过/1败/118过滤 |
| N12 | 101 | permit=1嵌套链具名1过/1败/8过滤 |
| N13 | 2 | R04递延叶missing basisKind硬拒 |
| N14 | 101 | offline镜像0过/1败/120过滤，live lane目标 |
| N15 | 101 | credential无料断言0过/1败/357过滤 |
| N16 | 未形成case行 | run-a/run-b两个完整R02结果存在，绑定摘要不同；默认reuse腿未运行；整项默认NOT_COMPLETED |

默认正常controls：xtask8 passed/113 filtered exit0；binary2 passed/0 filtered exit0。所有表中Rust结果ignored0。详见[逐项原日志/exit/数量索引](default-index.json)。N07起COPY继承旧reset漏掉的kernel注释，不能说后续与初始候选字节一致；这是F49历史。全部12文件后来精确恢复见旧 `full-restoration.json`，只证明旧副本恢复，不改变默认FAIL。

## 绑定与恢复补证

四份真实业务R02均完整20命令、13 PASS/7 FAIL，runner PASS：N06检查点0/20稳定；N16 run-a/run-b及 restored-real-R02均20/20稳定。N16两份摘要3353a1b4…与2ba5440c…确实不同，旧绑定manifest审计逐条重算的唯一差异为kernel。N16 a/b没有独立shell exit-code文件，本报告保留其完整JSON/每条子命令退出，**不补造外层shell具体退出**。restore-real-R02有完整command回执exit1。

旧根拒收补证02:57:16→02:57:16.917 exit1，日志明确not empty，只归补证，不追记默认N16。旧 G 在RX受控单命令map上使用原生产main/Scope/Git/fd发现器，实际 normal0/PASS→midbyte1/FAIL→restored0/PASS；红腿所有业务子命令PASS但stable=false，独立证明来源不稳强制失败。RX是机制证据，不是完整R02业务通过。还原原map后重建runner exit0。

恢复测试实际：xtask121、binary2、nonce1、诚实1、permit2、无料1全部exit0；完整producer **91 runs/427 passed/0 failed/0 ignored**，27 suites+64 exact，103 C-ID、130 leaves/137 leaf cases。本人读取91份实际日志并复核91/91摘要相等，见[producer索引](producer-index.json)。这是旧隔离输入的完整producer成功，不能替完整verify-stage或H/I新候选。

额外F25/F26：CID改名、缺registry、错误command、独占叶伪share、缺leafcase各目标1失败exit101→恢复1通过exit0；producer preflight目标exit1；最后镜像8 passed exit0。N04额外直接目标1失败101→恢复1通过0。准确argv/UTC/二进制/输入数量/原日志尾均在[28命令索引](supplement-index.json)。

G恢复资源raw160轮、54正式进程树点、61 owner点、15存活worker/45稳态、115点日志≤3，SHA7363dac1…；C/F46新独立原证另有337.41s资源2/2、假FD/TCP0各101→恢复0、六次旧日志顺序红→恢复绿。二者都是H前输入；H改lib/redaction后需H02新签，不能用旧C或G资源重签当前。

## 全部 R02 失败逐条归因

以下7项在N06、N16-a、N16-b、恢复R02四轮均exit1，共28条失败原记录；其余13项每轮真实PASS。完整argv、exit、UTC、timedOut、缺证据字段及逐轮下层原件路径见[r02-and-binding-index](r02-and-binding-index.json)。表中 `{EVIDENCE}` 依次是该索引四个真实业务根；原argv来自原JSON，不是本次新执行。

| 命令key及实际argv | 实际失败、原件位置 | 归因/处理 |
|---|---|---|
| a01_smoke：`bash scripts/rust-tauri/r02_t01_service_smoke.sh {EVIDENCE}/A01` | 顶层stderr拒启或root preservation failed；实际11case中no-root-switch为false | F47检查器INFO前置不足；真实epoch2拒启exit2、原stamp/数据/备用根保持。不是版本迁移失败，不是ALF。H SELF_CHECKED，待H02 |
| a13_redaction_scan：`bash scripts/rust-tauri/r02_t07_redaction_scan.sh {EVIDENCE}/A13` | SCAN完整6secret、246/41读取零命中后，correlation因真实requestId未出现在AUTH marker失败 | F48产品脱敏误吞47字符赋值；H已修待H02。不能撤销base64秘密防线，也不以扫描绿盖关联红 |
| a05_a06_auth_matrix：`bash scripts/rust-tauri/r02_t03_auth_matrix.sh {EVIDENCE}/A05_A06` | `A05_A06/cli-sessions-owner.stderr.log`明确Cannot find package 'ws' imported from隔离cli/client.ts；顶层是CLI owner输出不符 | Rust HTTP认证及列表前置已过；Node CLI导入失败，未抵达CLI列表业务断言。缺依赖环境，非认证产品失败/ALF |
| a16_legacy_regression：`bash scripts/rust-tauri/r02_t08_legacy_entry_regression.sh {EVIDENCE}/A16` | `a16_legacy_regression/stderr.log`明确cp隔离node_modules: No such file or directory | 依赖准备即失败，未到E1–E5；不能套历史E5合法分类写绿 |
| supplemental_client_matrix：`python3 scripts/rust-tauri/r02_client_leaf_matrix.py {EVIDENCE}/CLIENT` | CLIENT三CLI stderr均缺ws；summary.sharingError明确node_modules/.bin/vitest不存在；三case全false | CLI/help/参数与Sharing测试未取得有效业务结果；缺依赖环境，非产品回归已证实 |
| supplemental_cli_rust_matrix：`python3 scripts/rust-tauri/r02_cli_rust_matrix.py {EVIDENCE}/CLI_RUST` | CLI_RUST/serve及其他5份stderr均缺ws；11case只有构建/输入未变2过，其余9false；foreground bindAddr空、Rust PID0 | Rust binary build实际exit0，但CLI导入即失败，后续拒绝/清理不能当有效目标红；无更深CLI行为结论 |
| supplemental_cli_sessions_matrix：`python3 scripts/rust-tauri/r02_cli_sessions_leaf_matrix.py {EVIDENCE}/CLI_SESSIONS` | 内层auth/cli-sessions-owner.stderr缺ws；summary exit1、timedOut=false、无遗留group；报告FAIL(6 cases) | 六项HTTP列表原证不代表CLI通过；CLI依赖早退使所需CLI case未产出。缺依赖环境 |

上述五项缺依赖均有四轮原字节支持；不是仅凭副本现形或错误标题推断。Node实际已启动并给出ERR_MODULE_NOT_FOUND；缺的是本地ws等依赖，不是“没有Node”。不能把这些FAIL全部归ALF，也不能把其早退当N06/N16绑定目标证明。A01/A13旧报告与修复后证据严格分开。

## 必需新增缺口 F50：默认副本依赖前置

本归档首读矩阵只登记两条CLI观察，遗漏另外auth/client/legacy三个同根因命令的明确处理。这是**必需验收环境/默认准备流程缺口**，不是由本次静态归因证明五个产品缺陷，也不是另给ALF豁免。归档交付前总控已据完整原证登记 **F50 / J / OPEN / requiredForR05=true**，新J实施进行；本人只读回读该条并存[登记回读](registration-readback.json)，没有修改总控矩阵或替J签收。

源码证据：当前negative_gate第54–82行每次mktemp新COPY、本地clone，再只overlay `rust scripts docs/rust-tauri`，整份脚本没有node_modules/npm准备。`.gitignore:66`忽略node_modules，`git ls-files node_modules`为0，因此clone及三子树overlay不能把根依赖带入新副本。I/F49修改后仍保留这个准备缺口。

正测真实要求：`r02_t03_auth_matrix.sh:537–560`检查Node后真正执行 `node cli/entry.ts sessions`；`cli/client.ts:1` import ws；`r02_client_leaf_matrix.py:155`直接运行COPY内node_modules/.bin/vitest；`r02_t08_legacy_entry_regression.sh:806–813`明确要求从调用repo隔离CoW复制node_modules给基线；`r02_cli_sessions_leaf_matrix.py`复用auth脚本。package.json登记ws和vitest，锁文件存在。上述源码全文读取并入摘要，查错旧文件名导致的一次rg exit2已在操作记录保留。

最小必要修复建议：默认生产入口在建立候选快照/运行R02前，准备**隔离、不写回原仓库、可验证且与候选package/lock对应**的Node依赖；为ws实际解析和本地vitest可运行建立快速前置，缺失必须明确拒绝而不是耗时跑到五个假业务失败。延用现有依赖复用/隔离方式即可，不建议网络安装或新依赖。必须保留合法dirty输入、F42来源绑定/新鲜度及旧raw npm红，依赖准备不能扩大源码排除范围。

只对某次旧副本手工补node_modules，最多补该次业务对照；**下一次默认mktemp+clone仍会缺失，不能证明默认入口可复跑**。新修复/新独立审查应在全新COPY中证明默认准备和上述五项合法前置生效，再由新G02完整默认16与受影响恢复亲跑。若有效依赖后仍有业务失败，按实际新结果重新分类，不预判一定全绿。

除此之外本次有限读取未确认新的产品根因；已知F47/F48待H02、F49已I独立PASS、默认GFAIL、D环境BLOCKED、完整FINAL未运行，以及I06权限补证未结束均保留。未知不是PASS，也不机械复制庞大旧运行来掩盖缺口。

## 后续与停止条件

I01–I11逐项见[I-MAPPING](I-MAPPING.md)。H02新签、依赖前置新修新审后，冻结全部实际PRIMARY/COPY/source/scripts/所读docs，再新G02从头完整默认16；旧G中断报告不能代替。E后续真实结果回填与新审、最终全新FINAL原§5.3仍必要。

raw npm候选exit1、3失败文件/6失败测试与baseline0保持红；directed-no-seal-family只按既有E0–E4.5/E5合法范围使用，不扩成全npm通过。原LIVE无授权、Windows/Linux及原R09/R10平台后续边界保留，未新增延期。D同路径允许文字不等实际LAN通过，H后实际binary需新身份，仍R06_READY=false。

本次只读归档与归因完成后停止写；未修代码、未安装依赖、未处理系统许可、未替代H02/G02/FINAL。
