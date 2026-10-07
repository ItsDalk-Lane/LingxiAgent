# G01 中断后的 I01–I11 证据映射

本表逐项沿RR1 MASTER §5.2及RR2 §四G核对。**所有“历史已结束”均为原G/C等执行者的实际记录，由本次只读核验；不是本归档者亲跑，不给H/I新候选签PASS。** 最新完整集成结论待新G02和FINAL。

历史RR2/G-R2/I-MAPPING逐项具名映射仍保留，但其“I10全部PASS/无F45+”是当时报告，不能消除后来F45/F46/F47/F48/F49或本次依赖前置缺口。原RR2 workspace1476绿窗不替代其后FINAL5/7失败。G旧恢复producer每个原日志SHA及实际结果见[producer-index](producer-index.json)，恢复28命令见[supplement-index](supplement-index.json)。

下表 `P/` 表示旧G `supplements/producer-restored-full/suites/`，非当前源码目录。

| I项 | 要求→具名断言与历史实际证据 | 当前边界/待办 |
|---|---|---|
| I01 配置代次 | P/svc_r05_t01_model_plane.log 24过：f29_management_reload_during_inflight_401_never_leaks_the_new_key、f02_resolve_dispatch_freezes_compat_and_capabilities_with_the_route、跨provider；credentials38与exact旧handle/reload迟到写回拒绝 | 旧恢复producer已结束exit0；H改变共同service lib/redaction，不能仅按model文件未变重签组合。待G02输入归因及FINAL |
| I02 OAuth六叶 | P/credentials38，rr1_f04六叶login/add/remove/list非OAuth404/status/logout、撤销胜迟到安装、重启事务；oauth_flows21；原13条test级独占叶映射，磁盘/模型数/HTTP断言 | 六叶仍必需。旧结果可追溯；当前组合待冻结G02/FINAL，不能用通用share放行 |
| I03 协议重放 | P/adp_r05_t03_rr1_replay.log 33、goldens3、protocol_adapters12；签名/reasoning/来源/顺序，c05_runtime_nonce_rides_the_next_request_and_follows_changes、并行结果映射；N09/N10目标红及恢复1/2绿 | 旧输入有完整恢复证据，未签当前综合候选；F26注册反例红/恢复另有完整回执 |
| I04 整批工具 | P/batch_terminal10及service streaming18；合法write+非法read零副作用、同call冲突、重复收敛/独立ID/跨轮复用；F34合法stop但unclosed具名保留 | 原RR2 C-R2及新旧producer关系清楚；当前待G02/FINAL，不将更早能力拒绝算整批边界 |
| I05 消息终态 | P/closed_loop11：c01五方一致、f13规范化重启、c05零重执行、c07崩溃诚实；streaming/batch仅reasoning/opaque/mood/截断无假final、c16取消唯一结算 | 旧结果结束；H影响真实服务重新链接，新最终行为由FINAL签，不以预算408段冒充普通取消 |
| I06 正式四工具/worker | P/production_tools6、worker_model10、usage_ledger15：真实exec子进程、permit1嵌套HTTP、callback授权/未知停止/父子usage/环境无秘密；额外I06-existing-approval完整11过exit0、02:59:04结束 | 计划中I06-existing-worker-permission无证据目录或exit，UNKNOWN/待G02。不得照未执行write-review草稿写该项补证PASS；原R04历史证据可核输入后使用，不补造本轮运行 |
| I07 网络 | P/network21，proxy/direct/NO_PROXY/私有CA/TLS主机名链拒绝、DNS pin/redirect；service timeout9/adapters13、credentials c10、egress exact：错误体/排队/刷新/下载总预算 | 旧恢复实际全过；合法LIVE边界不变，不能外推真实供应商。当前组合待G02/FINAL |
| I08 媒体文件 | P/media_resource17、operations7、system_speech10：授权canonical/父叶symlink、host/provider job ID、reload不改绑、poll/download取消fence、registered product SHA | 旧结果完整；原C/F46媒体取消2过另有回执。H后有效输入及macOS实际边界需新消费，不外推其他平台 |
| I09 调用对账 | P/usage_trace9、usage_ledger15、persistence5、strict7/families14：HTTP物理计数、未发不算已发、失败unknown、父子JOIN、owner查询/重启/无秘密 | F48已证真实requestId诊断回归，故旧producer绿并未覆盖完整关联义务。H SELF_CHECKED，等H02新签及G02/FINAL，不以秘密扫描绿冲销关联红 |
| I10 资源恢复 | 旧G完整resources2过及raw160轮/54树点/61owner；C-F46-REVIEW-01全新独立资源2/2、337.41s，15 live-worker/15释放/45稳态；假FD/TCP0红→绿；旧日志顺序六轮红→绿；下述普通取消/恢复具名证据 | 旧只有父PID的18点不足已被C新证补齐。H修改lib/redaction后共同service/binary输入变，旧C321中两项不等；H新160自检不是独立签，待H02及新最终资源消费 |
| I11 门禁自证 | 默认15行均目标非零；N16两完整绑定及独立reuse补证；F25/F26五镜像红→绿+producer preflight；A19有效输入/A2 fd反例与三控/全checkpoint；I新56+13额外/B15/B41及真实!stable属性；C采样假零 | **默认整套FAIL，当前待新G02完整默认16**。还原F49包已独立PASS不冲销旧G。新发现默认Node依赖前置遗漏须先修/审，不把5种早退当有效业务红 |

I10必须区分三种证据：

1. 60预算408前仍cancelDanglingActive60/cancelSettled0，连接确实回收；重启后60条running消解且provider零重执行。它不证明普通取消后同实例恢复，也不意味着永久泄漏。
2. 普通取消由C-F46-REVIEW-01亲跑 `subagent_closeout::parent_cancel_closes_children_in_process_repeatedly_beyond_the_cap` 1过/7过滤，`late_result_fence::r03_a07_late_result_after_cancel_and_next_run_pollutes_nothing` 1过/4过滤；各exit0。媒体poll/download两个取消测试2过/15过滤；crash-effect、crash-stream、request-replay、callback-replay各1过且exit0，分别证明崩溃/未知/幂等，无需挪用预算段。
3. 54点是正式binary实际PPID进程树（逐PID RSS/FD/TCP）；61点是生产组合根的owner观察，15峰值/45稳态permit/waiter/run归零。两层不可互相冒称；115点≤3日志和临时文件清理有实际记录。H变更后当前需新独立签。

输入依赖核对：G的 `isolated-copy-reference-equality.json` 将旧copy与A19/B4/C321局部输入对齐；`reference-audit-final.json` 已实际记录当前A19相同、B主脚本不同、C lib/redaction两处不同，故不将旧证复用为当前全树冻结。A1只复用17未变输入及100个受控checkpoint；A2两shell已新独立亲跑。B原主脚本已由I改动，I-REVIEW-01对N03和汇总重新检验，不能把B单N03变成默认全16。E-REVIEW-02的G RUNNING只对其观察时点有效，须按本中断记录消费新状态。

新发现的默认Node依赖前置已由总控登记F50/J/OPEN，当前新实施进行，见registration-readback.json。原raw npm失败及directed/E5合法边界、D必要LAN环境阻塞、原LIVE/平台延期不变。I01–I11索引齐全不等于全部当前PASS；新G02/FINAL完成前R06_READY=false。
