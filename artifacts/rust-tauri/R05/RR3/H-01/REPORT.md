# RR3 H-01 / F47、F48 实施与自检

实施者 rr3_h_impl_01；本轮只 R05，未参与旧实施/验收/定位。不派代理，不外发，不操作 Git 写入或系统许可。
状态：**SELF_CHECKED**；F47/F48同包实现及本轮自检已完成。独立验收 PENDING，不能代最终阶段结论。

## 实施前登记

- F47（别名 RR3-R02-OBS-A01）：唯一所有者本实施者。修改 `scripts/rust-tauri/r02_t01_service_smoke.sh` 固定实际 INFO 日志前置。新增永久回归 `scripts/rust-tauri/r02_t01_log_level_regression.py`，实际驱动完整正式脚本的 warn/info 两次，并从17项原case JSON核完整三来源断言；不从PASS字符串判成功。不改epoch/迁移，不删实物保持、拒启、配置来源等现有断言。
- F48（别名 RR3-R02-OBS-A13）：唯一所有者本实施者。修改 `rust/crates/lingxi-service/src/redaction.rs` 增加仅内部可信诊断调用使用的请求编号格式化包装及内联永久正反回归。根因不是秘密扫描错误，而是裸 `request_id=` 和正式随机36字符编号连成47字符，落入原≥40规则。选择在宿主字段格式化时加引号边界，仍整行执行全部原脱敏；不增加req前缀或长token白名单。
- 必要邻接预登记 `rust/crates/lingxi-service/src/lib.rs`：仅宿主RequestId诊断的AUTH/TRANSPORT/WS与request handled调用使用上述包装。未知值不补造编号、Debug转义防字段注入。`logging.rs` 预登记可用但目前无需改，F46 open→prune、轮转及原3上限保持原样。
- 隔离注入只在本包源码副本，不复制target/.git，不使用G的negcopy.hTXzzN或NEG_TARGET。正式构建复用主树现有缓存，Cargo绝对rustup入口1.98.1 --locked。
- 服务库/编号诊断改变使C资源及F46运行证据需重跑；G旧copy输入不代表当前候选，G恢复control与FINAL由总控另排新验收。D旧9f对象重链接后只为历史，不继承其hash。

## 读取与证据

全文读RR1/RR2 MASTER、RR3 BRIEF/REVIEW/H brief、当前矩阵/进度/HANDOFF、triage REPORT和registration-proposal。沿原证读取A01/A13原JSON、拒启日志、前后快照/sha、响应/WS、retention文件、相关源码与历史；本包read-audit登记实际读取字节。独立定位已完成，实施前先登记。
原规格16A、100+3C、130叶、I01–I11、N01–N16与原放行公式不改。保留F31/F34/F41/F43/F44和A/B/C/F46已有成果，历史FAIL全部保留。

## 已实现的最小改动

仅4个生产/永久回归文件归本包：

1. `r02_t01_service_smoke.sh` 增加 INFO 前置；其余全部原断言逐字保留。
2. 新 `r02_t01_log_level_regression.py` 从外层warn/info实际驱动原脚本，核精确17个case及三来源选根、完整原文件/备用根证据，不读PASS字符串作结论。
3. `redaction.rs` 新内部 `DiagnosticRequestId` 格式化包装：仅在宿主字段边界加引号并转义；后续仍整行执行全部原脱敏规则。没有改任何秘密匹配函数、40阈值、字符类、前缀或secret-key列表。新增3个永久内联回归，包括正式随机36字符ID、伪编号秘密/拼接/URL/worker、未知编号/转义注入、个人路径保护。
4. `lib.rs` 只改宿主RequestId的AUTH/TRANSPORT/WS与request handled/错误buffer诊断调用。来源是最外层服务生成并放入扩展的RequestId，不消费客户传入的X-Request-Id或业务载荷requestId。真实编号仍随机36字符，原未加格式引号的赋值仍47字符；格式输出增加2个边界引号，未缩短或改造编号生成。

`logging.rs` 没有生产修改，SHA仍 `495608195d8c7702707b710781a4fb1be836ad6cf5904fef8558d2c1337ca301`。原open→prune、轮转及3文件上限不变。epoch、迁移、RequestId生成、锁、R00叶、配置来源、模型/媒体/权限/usage等均未修改。旧秘密扫描和关联检查的永久正式入口仍是原A13脚本；新增内联回归补上原短编号测试漏掉的正式形状，不替换正式服务检查。

## 本轮原红与修后绿

所有vector/cwd/UTC/退出/原始输出/输入前后/实际binary SHA在`commands/<记录>/command.json`与相邻原日志；Cargo绝对入口1.98.1、--locked、offline，主树缓存仅复用，不复制target/.git。下述是实施者自检，不是独立PASS。

| 记录 | 实际结果 |
|---|---|
| baseline-a01-warn | 原完整shell exit1，11项中1项失败；真实拒启正确、实物保持，INFO选根缺失导致no-root-switch误报 |
| baseline-a13 | 原正式shell exit1；6种preset/minted秘密246次读取/41目标均无命中，但真实401的36字符编号在AUTH marker缺失，correlation失败 |
| regression-old-red | 旧裸字段格式的3个新永久测试全部目标红，exit101；0 passed/3 failed/0 ignored/358 filtered；不是编译错误或空过滤 |
| a01-both-levels | 修后正式脚本外层warn/info各17/0，三来源全部真实拒启2、明确epoch2错误、原文件及备用根不变、无DB/token/READY |
| a13-green | 原正式shell完整exit0，包含SCAN及CORRELATION；完整retention/inventory生成 |
| live-correlation | 正式main+真实HTTP的5个请求：AUTH坏/缺认证、WS坏query、合法认证但坏WS升级、坏Origin；401/401/401/400/403均抵达原拒绝边界，真实响应ID逐字匹配对应AUTH/TRANSPORT marker和request handled文件日志；均为不同36字符随机ID，伪造头未被采用，伪编号合成认证秘密不出现在日志 |
| live-correlation-final-command | 上述5个请求在最终3c2c主树对象重新亲跑，全部真实marker/handled逐字对应、伪头未采用、秘密无泄露，TERM exit0/reap/home删除 |
| a01-final-both-levels | 最终主树程序重新亲跑warn/info各17/0，全部原三来源断言成立 |
| a13-final | 最终主树程序原正式shell完整exit0，6种秘密246/41读取无命中，correlation、session/run、retention/inventory完整 |
| redaction-final | 25 passed/0 failed/0 ignored/336 filtered；全部原22项及新3项 |
| logging-final | 原9项全部通过，0 failed/0 ignored/352 filtered |
| fmt-check / clippy-service | 两者exit0；clippy覆盖service lib及原resources测试，--locked -D warnings |

shell场景的ignored/filtered不适用；不把工具版本、--list或脚本驱动的exit0算成Rust测试数。详细原case JSON、P1–P4响应/WS/日志及所有失败均保留，不将P3的其余409隐藏成“全200”。

## 隔离目标红→字节还原绿

- F47 guard：仅副本较新epoch阻止分支的退出码2改为0；真实到达guard、仍无READY且实物保持，原`a01-newer-data-epoch-refused`精确失败，shell exit1。
- F47 wrong root：仅副本在遇到既有stamp时将实际选根切到环境fallback；真实READY显示备用根且真实存储初始化。原正式shell拒绝该行为，具名epoch-refused失败、exit1，并按原有界trap回收。不是只改诊断字符串。
- 两者还原后`f47-restored-green-2`正式shell完整17/0、exit0。副本实际4个关键文件SHA与主树逐项相等。
- F48：仅副本`DiagnosticRequestId`删除新引号/转义防线，所有原秘密规则不动；原正式A13的6秘密扫描仍246/41无命中，随后真实认证ID关联精确失败、exit1。字节还原并实际重编译后的`f48-restored-green`完整SCAN+CORRELATION+inventory绿、exit0。
- F46：未复用旧binary反例。以本轮完整service源码副本只把启动恢复成prune→open，锁定Cargo实编译，6次真实READY→TERM→reap，日志数 `[1,2,3,4,4,4]`，目标exit1；字节还原/重编译后 `[1,2,3,3,3,3]`、exit0，全部回收/home删除。
- FD/TCP：分别用本轮两个已编译资源装备的精确字节副本亲跑normal0→假FD零101→假TCP零101→移除PATH替身恢复0；具名目标各1测试/1 filtered，失败点明确，所有自有控制进程组清理后为空。系统lsof/主生产采样器不改。最终装备SHA `50e8fbdf2cb63adeb6614635ff4a9ca8bde421c678f26c09e72b9ecff996c397`，未给它套旧`323456…`或首次H装备`9aa07b…`的hash。

副本只有源码和锁/必要脚本，不含target/.git；所有构建记录均明示隔离cwd、实际副本变异及binary hash。没有访问G的negcopy.hTXzzN或NEG_TARGET，也没有动其证据。

### 保留的无效准备与缓存限制

`f47-restored-green`首轮实际exit1永久保留：副本字节已还原，但copy2保留旧mtime使Cargo误复用wrong-root变异程序（build仅0.13s，实际READY仍fallback）。它不算有效还原绿。改为写入新mtime且核真实重编译后，以新编号`f47-restored-green-2`取得17/0。

隔离构建后首个`final-root-build`虽Cargo exit0/fresh=true，但物理对象仍副本编译产物，未冒称主树重编译。只刷新本包所有者lib文件mtime（字节不变），以`final-root-build-2`强制主树实际编译，保存Cargo JSON/源入口/物理hash。最终主树对象为`3c2c30954478adc179b6b1073d70405a9025722463a132be6ee36c8727ea4865`；旧D的9f、本轮首次H的096f以及副本e1dd均按各自对象记录，不外推为最终对象。

## 资源复验与受影响证据

首次修后`resources-green`2/2、0 ignored/0 filtered、336.03s；原160=60预算取消+60错误+15正常+10长响应+15worker完整，54 binary点/61 owner点/15存活worker/45稳态全部20项实物分析成立，最后重启3日志、真实轮转和最后清理成立。预算408仍60 active、重启后消解且零重执行，不冒称普通取消的同实例恢复，也不宣称数据损坏/永久泄漏。

该次广义输入前后**false**原样保留：另一个所有者运行期间改了`r05_t08_negative_gate.sh`并新增`r05_t08_restore_selfcheck.py`。逐项核对372个实际Cargo/Rust/contracts/toolchain执行输入相等；这两脚本不在resources命令执行链，详见`binding-scope-analysis.json`。没有偷删变化路径或把整个候选说成冻结。

首次对象096f的资源结果只属于其自身。最终主树对象3c2c重新完整`resources-final`，**2 passed/0 failed/0 ignored/0 filtered，335.11s、exit0**；372个Rust/contracts/toolchain输入和扩展实际记录的整个选定输入集前后均相等，广义摘要`d44d6f5509b454afc2e8e1d46632efd6855ed781a74b15eae2447f0d8717e296`。最终equipment、config/fixture/worker/PID与SHA已在运行中实际绑定，不复用旧hash。

最终原序列及独立于测试布尔的逐点复算见`commands/resources-final/f27-resource-series.json`、`RESOURCE_ANALYSIS-resources-final.json`；20项全部true，实际160/54binary/61owner/15存活worker/45稳态不变。service RSS27264–50384KiB、FD15–19；完整树RSS27264–52960KiB、FD15–22；owner实际峰值、排队者、permit、任务及稳态逐项核零，日志115个资源点最多3份，最后重启3份、序号前进/实际轮转、无临时文件残留。所有参与PID退出回读、service exit0/reap、stub停止及home/workspace/config删除均成立。全部原绝对/增长门槛、UNKNOWN拒绝和取消/恢复语义保留。

最终A01两级、A13、5类真实关联、资源运行及root build的物理程序hash均为上述3c2c；以实际对象一致性而非同名证明。最终FD/TCP负控用50e8新装备重新亲跑，不继承旧装备的命令结果。资源装备每次采样失败仍UNKNOWN，未填假0。`FINAL_SOURCE_BINDING.json`、`COMMAND_INDEX.json`与`manifest.json`提供实际全量摘要及每条有效/目标红/无效准备分类。

生产变更输入是redaction/lib；A01检查器与新永久回归也改变测试输入。因此旧C/F46的完整资源、旧G恢复control/绑定与FINAL全链不能靠原报告冒充最新验收。H本轮正反及资源自检仅供新独立H验收者消费；总控另排全新H审查、受影响C/G补验、新文档回填及新FINAL实际C覆盖。D环境允许对象也须按真正FINAL产物重新核验。本包不解决或豁免D的LAN、G/I其他缺口。

## 边界与下一棒

仅macOS arm64、本包离线/合成材料、真实回环服务与原资源负载。未跑全workspace/完整§5.3终审、完整默认N01–N16、R04前序全链、收费LIVE、Linux/Windows或正式打包；这些均不被本包局部绿外推。未提交/推送/改系统/修改E现行文档或总控台账，没有独立签收。

完成后停止写生产源码/本包证据，交全新H验收者；其须亲跑原warn/info 17/0、完整A13与五类真实关联、隔离目标红/精确还原、最终对象完整160/54/61/15及清理。只能推进本包SELF_CHECKED，原R05放行公式与R06_READY=false仍由总控按全新独立结果处理。
