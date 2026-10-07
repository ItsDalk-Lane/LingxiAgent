**独立 REVIEW：PASS（仅 RR3 C/F27 与 F46 联合包）**

日志保留数量修复有效，规定的完整负载与资源回收证据齐备。本轮没有发现需要交新修复者处理的阻断项，`mustFix=[]`。本结论允许总控消费 C/F27/I10 本包义务及 F46 的独立关闭证据；不重签其他整合义务，也不宣布 R05 阶段 PASS 或 R06_READY。

**身份与独立性**

验收者 rr3_cf46_review_01，线程 `01a113da-d9e4-7582-a187-f1c2c674a6a7`。本会话由已安装 Codex CLI 的新 `exec --ephemeral` 空历史启动，未参与 C/F46 实施、修复或前轮判断，未再派代理。[真实派发记录](dispatch-request-at-review.json)记录此前 root/branch collaboration 派发均因 thread limit 失败；它们没有创建本验收线程。本报告不虚称 collaboration 子线程。

会话系统身份为 GPT-6 based Codex；[模型设置只读回读](model-settings-readback.json)是 `gpt-6.1-sol`、reasoning `high`，派发无模型覆盖。设置文件不足以独立证明后端实际服务的精确模型版本，本会话未获该额外证明，因而仅按上述可核事实报告。工具实际回读见 [environment](environment.json)：macOS 27.0.1 arm64，rustc/cargo 1.98.1，绝对入口 `/Users/study_superior/.cargo/bin`。

**继承规格与实际输入**

全文读取联合 brief、F46/review/RR3 brief、RR1/RR2 MASTER、RR3 进度/交接/矩阵、C REPORT/I-MAPPING 与 raw、F46 REPORT 与 raw，并沿继承链读原八份任务书、10-02 完整执行验收提示词、性能登记与相关 RR2 历史审查记录。原 16A、100+3C、130 适用叶、I01–I11、N01–N16 及 §6.1/6.2 的阶段规格继续有效；本包限定亲验没有降低它们。历史全阶段失败不因本包 PASS 翻绿，完整阶段另由全新阶段验收者执行。

HEAD `b3ac0e6aeae8d7d530a6ab8657a5fa9ce4849f0b`，分支 `codex/rust-tauri-migration`；工作区已有所有者未提交改动，HEAD单独不能代表被测输入。每次正式运行保存405项实际文件前后快照，当次均相等，运行快照摘要 `a9027ff2e3fdeadecfb6554b315afaaf05eabc4237db9d8f5a7a654cf5f56514`。收尾广义快照出现其他所有者更新性能结果汇总，旧/新摘要完整存于 [FINAL_SOURCE_BINDING](FINAL_SOURCE_BINDING.json)的snapshotChanges，**广义快照最终不相等，原F46扩展322项清单也有这1项变化**。该结果文档不参与执行，正式阈值登记与源码常量均未变；其余321项实际执行输入逐项回读与F46相等，actualHashes/执行摘要单列。三份TSV另由实际SHA及HEAD字节一致核对，原405项快照未包含它们，不伪称包含。快照包含必要源码、测试/support、Cargo清单/锁、工具链、contracts、性能登记和附带只读文件，不含target、xtask与其他所有者的编排产物。其他包写入实际记录，不被误当成本包漂移，也不据此声称总体树冻结。

| 被实际使用的对象 | SHA256 |
|---|---|
| logging.rs | `495608195d8c7702707b710781a4fb1be836ad6cf5904fef8558d2c1337ca301` |
| 原资源测试文件 | `23b03f56bef900fc0563b3348596e61437995f7c05cc25a5173455f11a23e33f` |
| 资源采样 support | `1f6601122e5d7841f3975ec68ea8c60adb653fc939ca4e65cc3da7565c269274` |
| Cargo.lock | `259f983e98a4da13eab1b592c2efd7b79579ad6678a08995a4b9e3fca618eff3` |
| 正式 lingxi-service | `b180d47abeeecef4e6d677b9501fde0828cd4061f74276d8e5e2a2a9afb33d81` |
| 实际 worker：r04_t07_fixture | `0b9e45e0c4e6530bbf02959f7d7f23704e7a9d9c82cd590dabbd899ae55cc794` |
| 本次资源测试装备 | `323456600dc961ee6e4d20b60cf7cf9964526bfcd4153090c7b4e89c0bf2e3a1` |

[运行中绑定](resources-01/runtime-binding.json)在真实服务存活时回读 PID/PPID、argv、正式二进制、磁盘配置完整摘要、脱敏配置内容、真实 worker 与两个输入文件摘要。真实 worker 是上表对象，不按其他报告中可选路径猜测。动态 home/config/端口只用于本包自己的隔离夹具。

原阈值在运行前已有正式登记与 C 的 preregistration，且最早 input-before 已绑定其字节：[阈值依据](thresholds-before.json)。本文件的整理时间晚于开跑，不伪装成新的事前登记。RSS 400MiB、FD 400、增长 150MiB/64FD、日志最多3，均未放宽。[SOURCE_AUDIT](SOURCE_AUDIT.json)与 [只读源码差异](source.diff)核对原两个测试身份、所有负载常量、UNKNOWN 拒绝和新增重启/稳态日志断言；没有 ignore 或替换生产入口。

**F46 真实触发、隔离负控与恢复**

[正式二进制连续六次启动](binary-six/result.json)：每次 READY→SIGTERM→实际 wait/reap，exit0，日志数量 **1→2→3→3→3→3**；全新目录与连续重启均通过。[已有恰三份](binary-three/result.json)：下一次启动仍3份，旧2/3文件字节不变。

[显式错误](binary-error/result.json)：自己的日志夹具中把待裁剪旧日志做成目录，真实 binary 输出 `LINGXI_SERVICE_LOG_WRITE_FAILED` 和 `continuing stderr-only`，服务仍 READY、exit0、实际回收。该错误集合有4个同名条目，其中一项是目录，裁剪失败；本项仅证明错误显式可见，**不记为日志数量上限通过**。正常情况下上限由六次重启与完整负载证明，错误返回由 logging 回归补充。

严格复制本包源码到自己的 [isolated](isolated-source-manifest.json)，不复制 target 树。重建完整生产 `lib.rs` 与 `main.rs`，复用由已锁定 Cargo 构建的依赖成品，以 Cargo fingerprint 精确选择并记录24项直接依赖成品 SHA。实际 `rustc 1.98.1` 命令、两次编译 exit0、UTC、输出摘要全部保存；没有手写替代日志函数，也没有主树注入。所有本轮 Cargo 编译/测试调用都有 `--locked`；此处直接 rustc 重建不支持该 Cargo 参数，也不进行依赖解析或锁文件变更，使用锁文件及既有依赖成品绑定证明边界。仅省略调试符号，三阶段编译方式一致。

| 隔离链 | 真实生产副本运行结果 | 命令退出 |
|---|---|---|
| [正常重建](isolated-normal-build/command.json)→[运行](isolated-normal/result.json) | 六次 **1,2,3,3,3,3**，PASS | 编译0/0，探针0 |
| [唯一变异](mutation.json)：仅 open 改回 prune→open_current；[重建](isolated-old-build/command.json)→[运行](isolated-old/result.json) | 六次均到 READY、服务exit0；**1,2,3,4,4,4**，准确触发 F46 | 编译0/0，探针**1** |
| [字节还原](restoration.json)→[重建](isolated-restored-build/command.json)→[运行](isolated-restored/result.json) | 六次 **1,2,3,3,3,3**，PASS | 编译0/0，探针0 |

正常/恢复的副本二进制 SHA 都是 `7d9190d27d1b6a019abfbe97049d6855a3f225494124fee43ef8988e032ae78b`；旧顺序为 `f4814d808b83ab97865135d1db94204fcf1fffc09c3df0f368fc3195b6155c15`。恢复源码逐字等于冻结源码与主树，真实红发生于日志数量边界，非编译/认证早退。每次进程与自己的临时 home 均完成清理，旧红记录保留。

**C/F27/I10 的完整资源证据**

[完整两个原同名测试](resources-01/command.json)实际 **2 passed、0 failed、0 ignored、0 filtered，exit0，337.41s**。UTC 开始 `2026-10-07T00:56:36.193396+00:00`，记录结束 `2026-10-07T01:02:54.485657+00:00`；记录区间含绑定等开销，测试耗时取真实 summary。

[完整原始序列](resources-01/f27-resource-series.json)保存全部160逐轮、54正式二进制进程树点、61生产组合根进程内 owner 点。本人独立回算 [RESOURCE_ANALYSIS](RESOURCE_ANALYSIS.json)，20项检查全成立，逐点和逐PID未裁剪。

| 义务 | 本轮实际观测与结论 |
|---|---|
| 原混合与100+取消错误 | 两会话，60预算408+60错误+15正常+10×512KiB长响应+15worker=160，未删负载 |
| 正式进程树 | 54点：baseline1、cancel-live6、cancel6、error6、ok3、worker-live15、worker-released15、worker-final1、post-restart1；实际PPID发现树，每PID RSS/FD/TCP全部求和相等 |
| 15组真实存活worker | 每组峰值service+worker=2、实际保留TCP≥2；释放后树=1、ESTABLISHED=0；不以结束后只采父进程代替峰值 |
| 正式service占用 | RSS 27408–49184KiB、FD15–19；整树RSS27408–51760KiB、FD15–22；绝对值及从基线的增长均在原限内 |
| owner单独证据 | 61点=baseline1+15peak+45steady。15峰值真实两个run，modelPermit1/toolPermit1/modelWaiter1；每组join后3个100ms稳态窗口，run/permit/waiter及登记ID全部0/空，树=1、TCP=0 |
| owner进程树实际占用 | RSS30640–35792KiB、FD15–23、TCP0–2；逐PID求和及边界复核通过。观测现有QuotaManager/SessionSupervisor/CancelRegistry/BackgroundDriveRegistry，不创建第二资源管理系统 |
| 日志真实写入/轮转 | 所有115个资源点最多3份；最终重启3份、全部owner稳态3份。最终000004/000005/000006各65290/31986/1518B；序号前进且真实累计写入>64KiB，轮转继续工作 |
| 临时文件/持久文件 | 完整home/workspace清单与长度保留；worker释放及owner稳态无.tmp；SQLite历史、身份、输入和日志明确属于持久数据，不称泄漏 |
| 退出与清理 | 最终服务exit0并reap，stub停止、home/workspace/config删除；[真实ps回读](pid-readback.log)全部34个参与PID均已不存在 |

60预算408逐轮真实 provider 连接回收（dropped60），但重启前仍 `cancelDanglingActive=60/cancelSettled=0`。原断言亲验重启恢复时将全部60条running消解，恢复过程 provider 零重执行；这不被冒称为普通取消后同实例恢复，也不称永久泄漏。普通取消另由下列两个具名测试亲验。

**测量器正常→两种目标红→恢复绿**

完整controls真实观测FD **3→6**；kill+reap之后记UNKNOWN而非0。已知TCP双端 **2→0**、三个17B .tmp **保留→空**；dead/bogus PID、空/错身份、缺目录均诚实拒绝。

[隔离测量器结果](sampler-negative-isolated/result.json)是本人重新执行：复制资源测试装备逐字相等，只副本进程 PATH 接受自有lsof替身，系统lsof与主源码不改。目标始终 `f27_sampler_controls_detect_growth_release_and_failure --exact`。

| 对照 | 实际退出与目标结果 |
|---|---|
| normal | exit0，1 passed，1 filtered |
| fake FD zero | **exit101**，具名目标FAILED，`fds: invalid live-process sample` |
| fake TCP zero | **exit101**，具名目标FAILED，已知连接两端断言 left0/right2 |
| restored（移除PATH替身） | exit0，1 passed，1 filtered |

负控自己的进程组残留sleep已回收，实际回读组内无成员；所有原始stderr/stdout和替身文本保留。非零是本次刻意变异被检测的有效证据，不当作正常生产命令通过。

**取消、恢复及零重执行的具名复验**

以下均本人亲跑，实际exit0、0 ignored、非零匹配；[COMMAND_INDEX](COMMAND_INDEX.json)保存完整argv、UTC、实际counts、输入与日志摘要。

| 证据 | 真实测试与counts |
|---|---|
| [ordinary-subagent](ordinary-subagent/command.json) | `subagent_closeout::parent_cancel_closes_children_in_process_repeatedly_beyond_the_cap`，--exact；1 passed/7 filtered |
| [ordinary-late](ordinary-late/command.json) | `late_result_fence::r03_a07_late_result_after_cancel_and_next_run_pollutes_nothing`，--exact；1 passed/4 filtered |
| [media-cancel](media-cancel/command.json) | download/poll两个原具名取消测试；2 passed/15 filtered |
| [crash-effect](crash-effect/command.json) | `c06_crash_after_tool_effect_no_blind_redo`；1 passed/10 filtered |
| [crash-stream](crash-stream/command.json) | `c07_crash_midstream_honest_terminal`；1 passed/10 filtered |
| [request-replay](request-replay/command.json) | `c04_request_id_replay_and_conflict`；1 passed/10 filtered |
| [callback-replay](callback-replay/command.json) | `c07_a_replayed_cb_id_is_answered_from_the_receipt_cache`；1 passed/9 filtered |

媒体与恢复关联项直接亲验，未依赖不同候选的旧绿窗替代新输入。未重跑与此服务负载无关的system-speech全套或真实供应商调用，不借历史引用外推这些项目的新独立PASS。

**logging九回归与64 exact关系**

[logging-nine](logging-nine/command.json)实际9 passed、0 failed、0 ignored、349 filtered，exit0；不是零匹配绿。新目录、已有日志、连续重启、写入/轮转、错误返回等九个现有回归全部执行。

[PIN_AUDIT](PIN_AUDIT.json)逐行核权威TSV、CID与生产者：原27集成suite保留、resources固定2；64个lib pin仍各计数1、唯一CID归属、正式生产者使用--exact。分别亲跑service/kernel/adapters --list（实际358/87/119）后，64个名称逐个恰匹配一项。pins/cids与HEAD字节相同，没有logging全库总数pin。**本项是64关系核对，未声称逐个执行64项**；--list也未记作测试通过。F46旧错误筛选0 passed/121 filtered明确INVALID，不消费为有效绿；其随后真实镜像2/2仅保留历史。

**历史真实失败、执行限制与交接**

[HISTORICAL_AUDIT](HISTORICAL_AUDIT.json)逐项全文字节读取并验证C的53项与F46的106项历史manifest，均相等。C旧measurement cargo2/2 exit0，但raw最终重启与owner稳态日志4份，完整资源义务仍 **FAIL**；C定向红与F46修前红不覆盖、不改写。RR2旧I10过度归属按C新映射纠正；旧“只剩ALF”不被继承为当前完整阶段结论。本轮新的独立raw与目标红→恢复绿支持本包PASS。

本轮只写指定证据目录及自己的隔离夹具，未修复生产源码、未更新总控文档、未执行Git写操作、未修改系统工具/权限/模型设置。按照brief复用已有构建目标，未复制target树、未清理用户/历史内容。所有Cargo测试使用--locked，临时输出指向自己的证据目录。工具输出/目录读取出现的非测试错误不被计入行为PASS；真实负控非零和历史失败按原值保留。

本次平台仅macOS arm64、debug正式服务与本包负载，不外推Linux/Windows、正式打包、LIVE供应商、全Tokio后台线程或其他所有者的阶段门禁。format/clippy及阶段镜像历史记录已核对应绑定；本轮没有把它们说成重新执行，也未执行§5.3全阶段链。`R06_READY`保持未放行，本报告不修改该总控字段。下一位全新阶段审查者必须消费本包冻结摘要，并另行完成完整正式阶段要求。

最终交付：C/F27/I10本包资源补证 **PASS**，F46 **PASS**，联合独立验收 **PASS**，mustFix为空。完整证据见本目录 [MANIFEST](MANIFEST.json)；逐命令退出/UTC/counts详见 [COMMAND_INDEX](COMMAND_INDEX.json)，每条原始输出保持可回读。
