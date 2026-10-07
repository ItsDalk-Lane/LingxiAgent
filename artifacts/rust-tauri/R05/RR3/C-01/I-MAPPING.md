# RR3 C-01 I10 映射（实施补证，待全新独立验收）

本文件提供 RR3 I10 证据解释，纠正 RR2/G-R2/I-MAPPING.md 的过度结论；历史文件不覆盖。**状态：部分补证自检完成，F46 必需项 OPEN，I10 不得标 PASS。**I01–I09/I11 的整体验收属于总控与阶段审查，本包不重签它们。

| I10 义务 | 直接断言与证据 | 边界 |
|---|---|---|
| 原 100+ 取消/错误与混合负载 | `f27_sustained_cancel_error_worker_load_stays_bounded` 保留 60 预算取消+60 错误+15 正常+10×512KiB 长响应+15 worker=160，双会话；本轮 `measurement-01/f27-resource-series.json` 的 load、cycleResults、series | 取消看门狗产生408、真实 parked provider连接逐轮回收；不能据此把恢复前 running 行说成已终结 |
| 普通取消后同实例恢复 | `subagent_closeout::parent_cancel_closes_children_in_process_repeatedly_beyond_the_cap` 与 `late_result_fence::r03_a07_late_result_after_cancel_and_next_run_pollutes_nothing`；有效历史运行 `RR2/D-R2/workspace-green-window-REVIEW-r1.log` 分别1922、1207行明确ok，本轮复验另见 commands | 既有实现和证据已存在；预算drop+重启不是普通取消的替代证据，也不重报“缺实现” |
| FD/RSS 测量器 | 同名controls测试：活helper ≥3FD且RSS>0，已知额外3FD恰3→6，kill+reap后UNKNOWN，假PID拒绝；本轮日志原始controls JSON | 不存在/命令失败/空记录不填0；负控分别见negative-fd-zero-run与negative-tcp-zero-run |
| 连接测量器 | `lsof -p PID -F fpPTn`校验PID/exit，以PTCP+TST=ESTABLISHED记录计数；已知保留连接两端2→释放后0 | LISTEN不计为ESTABLISHED；TCP两端在测试装备同进程，正式服务另单列 |
| 进程树/存活worker | 真实ps PID/PPID递归发现所有后代，仅排除本次ps采样命令自己的精确PID；15轮真实aux HTTP屏障后worker-live必须service+worker=2，释放后worker-released必须1 | 原FINAL-01仅worker-final采父，不能证明存活worker RSS/FD；新原始序列逐PID记录RSS/FD/TCP。无浏览器/WebView参与本服务负载 |
| 正式二进制峰值与稳态 | series保存原基线、取消live/settled、错误、正常、15组worker live/released、重启；serviceTree完整总RSS/FD仍受原400MiB/400FD上限 | 未声称发布优化性能收益、2小时/1000任务比较、其他平台通过 |
| 实際任务/permit/等待队列 | ownerResourceSeries：严格磁盘配置→原Credential/Gateway→原生产组合根→真实worker及aux HTTP；每组worker阻塞后启动第二真实任务，使activeSessionRuns/liveRunIds=2、modelPermits/toolPermits/modelWaiters=1；取消排队兄弟并join两个driver后，连续3×100ms窗口全部0 | 直接读取现有QuotaManager/SessionSupervisor/CancelRegistry/BackgroundDriveRegistry；这是进程内生产集成补证，与正式二进制series分列，不冒称二进制内部已开放观测接口；仅统计实际负载owner，不声称枚举所有Tokio后台线程 |
| 临时文件/清理 | files保存home/workspace递归完整路径与长度；controls已知3个17B .tmp文件→空，缺目录报UNKNOWN；最终cleanup在真实进程exit/reap、stub退出及三个隔离根删除成功后才写true | SQLite历史/日志/输入文件为明确保留的持久事实，记录完整清单，不把它们叫泄漏；媒体产物另外复用下面具名测试 |
| 媒体临时产物 | `r05_t06_rr1_media_resource::rr1_f19_cancel_during_download_leaves_no_registered_product`、`rr1_f19_cancel_during_inflight_poll_fences_download_and_completion`；system-speech已有真实平台产物/permit/child清理测试 | 正式聊天/worker负载不生成媒体；不拿删除整个测试目录代替产品取消中的产物回收 |
| R03/R04恢复、未知与幂等 | FINAL-01资源记录60条dangling active重启全量消解+零provider重执行；`c06_crash_after_tool_effect_no_blind_redo`、`c07_crash_midstream_honest_terminal`、`c04_request_id_replay_and_conflict`、`c07_a_replayed_cb_id_is_answered_from_the_receipt_cache`在RR2绿窗中有效运行 | RR3本包没有改相关生产代码；保留原ALF与绑定失败边界，阶段完整PASS由新独立终审亲跑决定 |

资源原始序列必须跟实际命令的exit与输入摘要一起消费。仅有测试通过文案不能外推没有被采样的资源，也不能宣称存在泄漏或证明未知负载绝不泄漏。

## F46 新失败及验收状态

完整清单发现最终重启后持续存在4个service日志文件，超过原上限3；不是暂态，也不能以删除home掩盖。measurement-01的旧断言只在worker-final看3，因此cargo exit0不能当资源整项PASS。f46-directed-red-01四次真实二进制重启记录1→2→3→4（exit1，准确点名F46）。最终测试新增post-restart和owner released-steady同一≤3断言；当前生产logging未修，本包不独立签收。

普通取消本轮精确复验：ordinary-cancel-subagent-01（1 passed、7 filtered、0 ignored、exit0）与ordinary-cancel-late-result-01（1 passed、4 filtered、0 ignored、exit0）。这些具名有效运行解决引用缺口，不把普通取消实现重新报作缺失。
