# RR3 G-REVIEW-02 独立集成负测审查

**结论：BLOCKED_BY_STORAGE / NOT_ACCEPTED，不签 PASS。真正默认入口已执行，但仅 N01 得到有效目标红；N02 在构建阶段耗尽磁盘，未触发目标；N03–N16 未执行。完整 R02/full E5、本轮恢复全绿和最终恢复均未完成。R05 仍 NOT_ACCEPTED，R06_READY=false。**

审查者 `/root/rr3_g_review_02`，全新独立审查，未参加此前实现或审查，未派代理。只写本目录及先行仓外记录；未修生产源码、脚本、现行文档、Git 或系统许可，未提交、推送、发布。历史 PASS 只在下述逐项输入相等范围内引用。旧 G01 的 exit2、15行、中途主脚本被修改及缺 N16 reuse 保持失败，未拼接为16/16。

## 实际对象与命令

全文读取 RR1/RR2 MASTER、RR3 BRIEF/REVIEW/G_R2、最新矩阵/进度/HANDOFF，原10-02专项规格、原通用/架构/功能/交接/验收/风险任务书及R05任务书；R06只读前置与消费契约。读取 A/B/C-F46/D/E/H/I/J 最新完整独立报告、G中断报告及I映射、相关原件。原16A、100+3C、130叶、I01–I11、原16负测与放行公式保持。权威字节见 [authority-files](metadata/authority-files.json)。

主 HEAD=`b3ac0e6aeae8d7d530a6ab8657a5fa9ce4849f0b`，分支 `codex/rust-tauri-migration`，有合法未提交候选。真实默认命令为：

```bash
bash scripts/rust-tauri/r05_t08_negative_gate.sh artifacts/rust-tauri/R05/RR3/G-REVIEW-02/default16-01
```

UTC开始 **2026-10-07 04:24:34.372421**；console最后实际写入 **04:28:34.544201**，后者是文件时间，不冒称精确进程结束时间。使用全新证据目录及生产入口建立的 `/Users/study_superior/r05t08-work/negcopy.HwNE95`。未带 `--case`；显式移除 `R02_LEGACY_REGRESSION_MODE`（父环境原本也为空），默认仍是 full。设 `CARGO_INCREMENTAL=0`、`PYTHONDONTWRITEBYTECODE=1`；未改版本、优化、锁、断言或测试范围。Rust/Cargo1.98.1、Node24.16.0、npm11.13.0、macOS arm64；生产脚本使用绝对Cargo及锁定rustup工具链、locked/offline。真实工具字节在 [tool-identities](metadata/tool-identities.json)，调用环境和版本原输出在 [preflight](metadata/preflight.json)。

**默认 shell 的实际 wait 退出值未成功落盘，记 UNKNOWN。** 其末行来自源码 `fail()` 的 exit1路径，故只能推断1。外层记录器在子进程已结束后保存 `command.json` 时也遇ENOSPC，工具实际返回 **1**；不能用它冒充默认 shell 的独立回执。随后精确终止仍在等结束文件的本轮观察器 PID11853，工具实际返回 **143**。二者分别记录于 [execution-interruption](metadata/execution-interruption.json)。没有补造失落的原始退出值或逐子命令精确开始/结束UTC。

## 本轮实际结果

[run-index](metadata/run-index.json)保存原argv、cwd、退出来源、数量、原日志摘要及未执行项；[原console](metadata/default-console.log)、[原两行结果](default16-01/case-results.tsv)保持原样。

| 项目 | 实际结果 | 判定 |
|---|---|---|
| 完整Git隔离准备 | 38,066 tracked；37,887 CoW、179准确Git blob、0普通复制；68项合法dirty覆盖 | 成功抵达copy-ready，非纯HEAD冒充候选 |
| 完整Node准备 | 64,760项；ws从本轮副本实际解析、vitest实际启动；准备命令均0 | PASS准备；不等于R02业务通过 |
| 正常xtask镜像 | exit0；8过/0败/0忽略/113过滤 | 当前正常对照绿 |
| 正式binary接线 | exit0；2过/0败/0忽略/0过滤 | 当前正常对照绿，实际重编service链 |
| N01 删除A16 | exit101；0过/1败/0忽略/120过滤，明确 `R05 map dropped original scenario R05-A16` | 有效目标红，非编译失败 |
| N02 零匹配 | 外层实际打印并写入case行exit1、BAD、MISSING目标；构建日志含28个 `could not compile` 项 | ENOSPC环境失败；0个producer运行完成，未到零匹配断言，不算有效红 |
| N03–N16 | 无实际case执行 | NOT RUN |

N01之后原reset确实把R05 map恢复到pristine字节，再注入N02 pins；本轮没有执行N01之后的恢复Cargo检查，不能补写“红→恢复执行绿”。N03的入口reset第一次cp失败后终止，N03实际变异/镜像/恢复未运行。最终12文件reset、Node终末verify和 `case-results.json` 汇总均未抵达。

N02原 `build.log` 明确LLVM输出、链接器及依赖文件写入 `No space left on device`，涉及库测试、真实service及其他测试构建目标；28条编译失败汇总不是28个业务断言失败。producer外层 `cat` 也无法写满日志，独立exit-code文件写失败，后续恢复cp失败。所有可观察失败列表保存在run-index，原构建日志在 [build.log](default16-01/n02-zero-match/suites/build.log)。缺失回执见 [missing-run-evidence](metadata/missing-run-evidence.json)。默认目录未发现零字节JSON；不能把正常空stderr当坏证据。记录器后置快照写失败的事实另留，后续同名快照带明确中断后UTC，不伪装原自动收尾。

**本轮N06/N16均未开始，真实完整R02命令数为0，full E5也未执行。** 因而不存在本轮可逐条判为正常业务绿、ALF失败或旧npm失败的R02结果。J02的E0–E4.5 directed通过只保留原范围；旧raw npm候选exit1、3失败文件/6失败测试与baseline0仍红。旧G四轮R02的7项失败及后来的F47/F48/F50修复证据分别保留，不能把旧失败当新运行，也不能把准备成功写成这5项CLI/legacy业务通过。

## 输入、程序和证据身份

主1068项实际源码/脚本/所读文档/契约/根输入，前后数量与逐项内容/模式相同；摘要均 `878533ef3fe5b8fd5e50bdb633a22b8e17451f4d80f8510ab9b7b8058571e69e`。主HEAD/分支/index保持，index为 `e65fd8f9353224483a9b94cc32bca37dde0505f363692af58539d270df9dfc9d`。见 [source-before](metadata/source-before.json)、[source-after](metadata/source-after-default.json)、[post-default](metadata/post-default.json)。不是只靠HEAD证明dirty候选。

中断后又独立逐字重读准备时清单：主38,066个tracked来源无差异；主及副本各64,760项依赖无差异，见 [final-preparation-readback](metadata/final-preparation-readback.json)。这只核清单内实际字节，不代替未运行的生产终末verify，也不声称整台机器或所有新增untracked输出被冻结。Node清单与J02同SHA `a7194de703eabe3832f360f856d613e1ee1e4df7191250726c115ee6bb289408`。

生产pristine12项均与主输入相等。中断副本只有N02的pins残留变异，原 `fdacb2c2…` 对副本 `d4f4661e…`；其他11项相等，包括map。副本**封存为失败现场，不手工恢复、不供后续运行复用**。[copy-state-after-abort](metadata/copy-state-after-abort.json)有完整摘要。`copy-initial-comparison.json`实际在N02期间取得，其名称不表示注入前快照；其中pins差异是当时预期变异，另`.DS_Store`/本地忽略AGENTS不在clone。

编译日志真实指向本轮copy。正常xtask实际二进制观察SHA为 `e8656eed0264b920bb08ed29727cfff78af0a56c5dd3a372f8474de256321b5f`，N01变异重编为 `14e08057231be187f425cce5a02053f54d5ebc5c6f4de958028830724043a480`。接线测试为 `45a5d12964c8db4b3bc7cf7f47df27b7f10064ff9d1665097eac4a102ea41bcb`；实际新service为 `9bd3eb746934486012af9c67b71b53d54ccbd03b3486b4f52009a067f3dfb81f`。观察时点、大小、编译及当前路径状态见 [tested-binary-identities](metadata/tested-binary-identities.json)。哈希是在编译后观察取得，不虚称每项都在进程内抓取。N02链接失败后deps下service路径已不存在，debug主路径仍为该9bd3摘要；不把缓存全部说成完好或已恢复。不复制大二进制，不将本配置下的9bd3对象标成H02的7fa13对象。

动态记录先在 `/private/tmp/rr3-g-review-02-20261007` 保存。默认/观察器及补充只读程序结束后，28份小型记录共5,476,186字节安全归档，逐份复算前后相同，见 [ARCHIVE-INDEX](ARCHIVE-INDEX.json)。原记录器遍历并哈希二进制后才进入下次轮询，观察日志不是精确每2秒采样；不拿空白间隔推断进程消失。所选3098条历史报告/命令/日志原件全部存在，附带旧期望摘要的条目无不匹配；未附期望摘要者只记录本次读取SHA，**不谎称3098项都与旧manifest逐项相等**。见 [reference-raw-audit](metadata/reference-raw-audit.json)。

## 有效复用与I01–I11

逐项要求、具名断言、实际运行、结果及输入依赖见 [I-MAPPING](I-MAPPING.md)。复用结论不是本轮亲跑，不能加进默认16的通过数。

- A1实际17项逐项相同，可复用独立121检查和100个内外受控checkpoint。A02的19项中18项相同，改动的legacy入口由J02真实新验；不谎称19全同。
- I21项中20项相同，唯一negative入口准备段经J改动。J02对原pristine至最终整段、legacy绑定/E0s–E5正文保持性有独立字节证据；当前J02来源1016项仅4个派发前协调文档不同，执行代码相同。可引用I56+13、B15/41、真实N03及生产main/Scope/OS fd的正常0→来源变异1→恢复0机制证据；不当完整R02或默认16。
- F25/F26相关27项实际源码、锁、注册表、R00 ledger和生产者逐项与旧G输入相同；5个注册变异各真实101→恢复0，producer前置1，11份原命令/日志重核。只复用注册/前置机制，见 [f25-f26-reference-audit](metadata/f25-f26-reference-audit.json)，不复用旧producer整套为当前组合。
- H02全部375项输入逐项相同，当前资源160混合、54正式进程树点、61 owner点、15存活worker/45稳态、20项重算、假FD/TCP红→绿、F46六次启动、F47/F48和两个普通取消具名测试可以原范围复用。60预算408后running经重启消解单列，绝不冒充普通取消同实例恢复。

当前I01–I09完整producer组合未新验，旧G的91 runs/427过属于H变更前输入；H新lib/redaction需要当前组合消费。I06额外worker-permission在旧中断及本轮都未取得实跑，不补造通过。I10有当前有效独立复用，I11仍缺完整默认16与恢复。E后续回填/新审及全新FINAL原§5.3仍必要，E14属于完整候选字节输入，后续变更必须明确新坐标或相关输入相等边界。

## 存储阻断、精确收尾与后续条件

开工真实可用1,471,639,552字节，确认没有遗留cargo/rustc/旧负测写者后启动；完整准备本身成功。N02大构建后，连小型报告和shell临时文件也ENOSPC，虽df显示约116MiB仍不能成功创建文件。没有把这些失败当有效红，没有盲目重跑，也没有改断言换取通过。

按brief及root明确单文件授权，只回收本轮失败库测试产生的一个非可执行 `.rcgu.o`：19,546,160字节、SHA `56c7cde2dbc6053d84f7e7b8196eb8e3051cee7e59a499b2a2a56bee4815c130`，出生UTC04:28:25.283037；实际 `lsof` 1且stdout/stderr空，无持有者，原build.log对应 `--test -C extra-filename=-db0defd20b34f871` 失败命令。它不是任何有效运行消费的独立二进制或正式日志。只unlink该精确文件，free121,122,816→140,869,632字节，回执 [cache-single-object-cleanup](metadata/cache-single-object-cleanup.json)。未删除用户数据、不明旧cache、旧证据或其他路径。

UTC04:35:39盘点截点，剩余本轮时间窗内deps直接子文件453项约1.14GB逻辑量，其中354项编译中间物约487.5MB；只列最大20个候选供后续精确核查，见 [owned-cache-boundary](metadata/owned-cache-boundary.json)。出生时间晚可能是覆盖旧同名对象，不能据此批删或推断无历史引用；逻辑量/du也不等于实际可释放空间。完整旧21GB target禁止据此整体删除。本轮copy保留失败现场。总控随后告知另有storage03核定33个完整MH_OBJECT可单独回收；该后续动作由其独立回执承担，不改变本盘点时点、不算本审执行，也不应将盘点候选误认成必须保留的已运行程序原件。

**新增已证产品mustFix：无；新增必需环境阻断：默认N02真实构建空间不足。** 必须先恢复足够实际可写空间并确认旧进程结束，再由总控按原独立角色安排新的默认全16执行，使用新证据目录/新副本，保留本轮失败。若后续真正full R02/E5出现业务失败，再按真实结果分别处理，不能预先豁免或判绿。本审不因容量阻塞推断产品正确，也不为存储问题削减范围。

D必要LAN条件仍BLOCKED；本轮未操作系统许可。原LIVE未授权、Windows/Linux及既有R09/R10边界保持，无新增永久延期。完成本报告和最终摘要后停止全部写者，最终进程检查见 `STOPPED.json`。本报告仅交付已完成的审查和真实阻断，不构成G、R05或R06放行。
