# G02 I01–I11 逐项证据映射

**当前结论：I01–I09完整组合仍待验；I10可按H02当前相等输入复用；I11未完成。G02默认执行仅N01有效目标红，N02环境失败，N03–N16未执行。索引齐全不等于全部PASS，R06_READY=false。**

本表沿RR1 MASTER §5.2、RR2 §四G和原规格，不改变原要求。下列历史运行均是此前独立执行者已结束的原件，经本审读取/核摘要或输入；不是本审亲跑。当前仅正常镜像8、真实binary接线2、N01红1为本轮完成的Rust运行。`旧P`表示 `../G-REVIEW-01/supplements/producer-restored-full/suites/`，其91 runs/427过不整体转签当前H之后候选。逐命令索引仍在 `../G-INTERRUPTION-01/producer-index.json`、`supplement-index.json`；当前本审输入核对见 [reference-input-comparison](metadata/reference-input-comparison.json)。

| I项及必须成立的行为 | 具名断言与实际原运行 | 当前输入依赖和结论 |
|---|---|---|
| I01 配置代次/冻结/迟到凭证隔离 | 旧P `svc_r05_t01_model_plane.log` 24过；`f29_management_reload_during_inflight_401_never_leaks_the_new_key`、`f02_resolve_dispatch_freezes_compat_and_capabilities_with_the_route`、跨provider；credentials38及旧handle/reload迟到写回拒绝 | 当前service lib/redaction较旧P已变，不能仅按model文件未变重签整个组合。本轮接线2绿是新增有限证据；完整配置代次组合未新执行，待当前producer/FINAL |
| I02 OAuth六叶及事务恢复 | 旧P credentials38、oauth_flows21；login/add/remove/list/非OAuth404/status/logout、撤销胜迟到安装、重启事务；13条test级独占叶及磁盘/HTTP/模型计数 | 六叶和独占叶仍必需；F25注册机制27项输入相等支持复用，但注册不代替业务。当前完整六叶组合未新执行 |
| I03 多轮协议原样重放/配对 | 旧P replay33、goldens3、protocol_adapters12；来源/顺序/reasoning/签名；`c05_runtime_nonce_rides_the_next_request_and_follows_changes`；旧N09/N10红，恢复1/2绿 | 本轮正常binary接线2绿，N09/N10未运行。旧局部材料仍可追溯，不把H前整个producer视为当前通过；F26注册反例另按相等输入复用 |
| I04 整批工具边界 | 旧P batch_terminal10、service streaming18；合法write+非法read零副作用、冲突callID拒绝、重复收敛/独立ID/跨轮复用；F34合法stop但unclosed | 需保留真正整批边界，不能以更早认证/能力拒绝替代。本轮未重跑，当前组合待验 |
| I05 规范化消息/唯一终态/重启 | 旧P closed_loop11：`c01`五方一致、`f13`规范化重启、`c05`零重执行、`c07`崩溃诚实；streaming/batch仅reasoning/opaque/mood/截断无假final、`c16`唯一取消结算 | 当前真实service新编译接线成功不覆盖全部终态；H02取消具名原件只覆盖其两项。完整终态组合本轮未执行 |
| I06 正式四工具/worker权限/父子清账 | 旧P production_tools6、worker_model10、usage_ledger15；真实exec子进程、permit1嵌套HTTP、callback授权、未知停止、父子usage、环境无秘密。旧额外 `I06-existing-approval` 11过/0败/0忽略/0过滤、exit0 | 旧 `I06-existing-worker-permission` 没有完成原证，本轮也未运行，仍NOT_OBSERVED/待验。不能照旧草稿补写PASS。当前全部工具/worker组合待当前producer与必要权限补证 |
| I07 网络策略/总预算 | 旧P network21、service timeout9/adapters13、credentials c10及egress exact；proxy/direct/NO_PROXY/私有CA/TLS链和主机名拒绝、DNS pin/redirect、排队/刷新/下载总预算 | 合法LIVE边界不变，不外推真实供应商；H后完整组合未新跑，本轮磁盘失败不提供网络业务结论 |
| I08 媒体/资源/系统语音 | 旧P media_resource17、operations7、system_speech10；canonical授权、父/叶symlink、host/provider job ID、reload不改绑、poll/download取消fence、正式产物SHA；C/F46另媒体取消2过 | 原包证据保留，当前整体需有效组合消费；不能将macOS离线材料外推其他平台。本轮未新增媒体/语音执行 |
| I09 物理调用与usage对账/保密/查询 | 旧P usage_trace9、usage_ledger15、persistence5、strict7/families14；HTTP实际次数、未发不算已发、失败unknown、父子JOIN、owner查询和重启。H02新增当前真实关联/无秘密完整A13、25 redaction、9 logging与5类拒绝 | H02全部375输入相等，F48的当前独立修复证明可复用；旧producer绿不能覆盖后来发现的F48。其余完整调用/usage组合未新执行，不能以6秘密扫描绿代替requestId关联 |
| I10 资源归零/进程树/恢复/未知拒绝 | H02 resources-final exit0，2过/0败/0忽略/0过滤，336.02秒；160混合、54正式binary树点、61 owner点、15存活worker、45稳态；20项原raw复算。装备实际FD0/TCP0各目标101→恢复0；F46六次启动旧[1,2,3,4,4,4]红1→恢复[1,2,3,3,3,3]绿0 | 当前375实际输入逐项相等，H02独立包结果可复用；本轮没有重复资源负载。H02正式service7fa13…、装备07fa…保持原身份，不能套到本轮CARGO_INCREMENTAL=0的9bd3对象。具体取消边界见下 |
| I11 门禁自证/身份/恢复/假绿拒绝 | 本轮正常8+2绿、N01目标101；N02未抵达、N03–16未执行。A1 121与100检查点、A2/J真实fd父子孙/非法根/查询错误；I56+13、B15/41及N03真实红→绿；I生产main/Scope/OS fd正常0→仅stable变异1→恢复0；F25/F26五反例红→绿、producer前置；H资源假零 | A1 17相等，I20/21相等（变化为已由J新验的准备段，原后半段保持）；J执行输入当前相等，4协调文档明确不同；F25/F26相关27相等；H375相等。均是限定机制复用，不补成默认16。G02整体未完成，仍必需全新默认/实际fullR02/E5/终末恢复和新FINAL |

I10的三条链必须分别表达：

1. H02的160轮由60预算408、60错误、15正常、10长响应、15worker组成。60预算段future drop后留running，重启消解且provider零重执行；这说明该预算/重启路径，不能冒充普通取消后同实例恢复，也不等于永久泄漏。
2. 普通取消后同实例恢复由H02亲跑 `subagent_closeout::parent_cancel_closes_children_in_process_repeatedly_beyond_the_cap`（1过/0败/0忽略/7过滤）与 `late_result_fence::r03_a07_late_result_after_cancel_and_next_run_pollutes_nothing`（1过/0败/0忽略/4过滤）分别exit0证明。它们与预算段独立归属；未将旧C/H前程序套成当前。
3. 54点是正式程序实际PPID进程树逐PID RSS/FD/TCP及求和；61点是生产组合根owner观察，15峰值/45稳态permit、waiter、run归零。115点日志≤3、临时文件清理、最终34参与PID退出及服务回收有原raw；两类点不互相冒称。已知FD3→6、TCP2→0、3临时文件持有/释放，死PID/空输出/错身份UNKNOWN拒绝也有H02独立原件。

F25/F26复用以 [27项输入及11命令原证](metadata/f25-f26-reference-audit.json)为准，不是仅看旧PASS：CID改名、缺registry、错误command、独占叶伪share、缺leafcase各目标101→恢复0；生产者缺CID前置1。当前正常镜像8又实际通过，但完整producer在N02构建时已被磁盘阻断。

A/I/J复用的代码边界见 [reference-input-comparison](metadata/reference-input-comparison.json)、`../J-REVIEW-02/preserved-contracts-independent.json`和相关完整REVIEW。A1的17输入未变；A2旧19中legacy脚本已变，J02真实OS fd4组、7非法根、24查询控及原legacy directed新验补其相邻边界。I原恢复/同步正文在J修改准备后保持，真实受控RX是所有子命令绿而来源不稳强制FAIL的独立属性证明，绝不等同完整R02业务。

原D必要LAN环境阻塞、raw npm红、合法directed/E5分类、未授权LIVE及原平台延期保持。本轮真正full R02/full E5未抵达，因此无新业务结果可消费；后续FINAL注册R03七项directed脚本也不能自动补齐该空白。当前执行输入冻结已实测，不代表后续E14回填无需重新绑定。必须先解决真实空间阻断并重新形成完整默认结果，再按原阶段放行公式验收。
