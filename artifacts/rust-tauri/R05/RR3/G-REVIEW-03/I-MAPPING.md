# G-REVIEW-03 I01–I11 逐项证据映射

**当前结论：I11 门禁自证由本轮 default16-03 完整亲跑关闭（16/16 fail-closed 点名+controls 绿+恢复绿+真实 exit0）；I10 按 H02 375 项输入逐字节相等原范围复用；I01–I09 的完整 verify-stage R05 正常组合仍待新 FINAL 亲跑（本轮 producer 运行均为注入态 fail-closed 证据）。索引齐全不等于全部阶段 PASS，R06_READY=false。**

依据 RR1 MASTER §5.2、RR2 §四G 与原规格；输入相等性证明见 [reuse-input-equality](metadata/reuse-input-equality.json) 与 [j02-source-final-equality](metadata/j02-source-final-equality.json)。本轮亲跑＝default16-03（有效轮）及 default16-01 的 15 项与 run-a full R02（辅助证据）。`旧P`＝`../G-REVIEW-01/supplements/producer-restored-full/suites/`（91 runs/427 过，H 变更前输入）。

| I 项 | 要求（具名断言） | 本轮实际运行 | 结果 | 输入依赖/复用边界 |
|---|---|---|---|---|
| I01 配置代次/迟到凭证隔离 | reload 与在途 401 不混材料；旧 handle 失效；能力/compat 冻结 | 旧P `svc_r05_t01_model_plane` 24 过（`f29_management_reload_during_inflight_401_never_leaks_the_new_key` 等）；本轮未重跑该套件 | 复用旧P（机制），当前完整组合待 FINAL | service lib/redaction 较旧P 已变（H/F48）；本轮 control-binwiring 2 绿是新增有限证据；不能按 model 文件未变重签整个组合 |
| I02 OAuth 六叶/事务恢复 | 六叶各自真实状态/模型数/磁盘/HTTP 对账；撤销胜迟到安装 | 旧P credentials38/oauth21；F25 注册机制 27 项输入相等（G02 已核，本轮执行代码零差异再确认） | 注册/前置机制复用；业务组合待 FINAL | 注册不代替业务；当前完整六叶组合未新执行 |
| I03 协议原样重放/配对 | 签名/reasoning/nonce 来源位置顺序保持 | 本轮 N09（固定 done → c05_runtime_nonce 红 101）与 N10（callId 错配 → call_bin_1 红 101）为注入态目标红；旧P replay33 | 注入态红本轮亲跑；正常重放组合待 FINAL | N09/N10 证明防线存在且点名，非正常路径通过证据 |
| I04 整批工具边界 | 合法+非法批次零副作用；冲突 ID 拒绝 | 旧P batch_terminal10/streaming18（含 F34 合法 stop 仅缺 block_stop 的永久腿） | 复用旧P；当前组合待 FINAL | 本轮未重跑该套件 |
| I05 唯一终态/重启一致 | 仅真 final 提交；重启读取一致 | 旧P closed_loop11（c01 五方一致/f13 规范化重启/c05 零重执行/c07 崩溃诚实）；H02 两普通取消具名测试 | 复用旧P+H02（H02 375 输入全等） | 完整终态组合本轮未新执行 |
| I06 四工具/worker 嵌套链 | permit=1 真实子进程 HTTP 链、父子 usage、取消回收 | 本轮 N12（删 drop(model_permit) → c06 嵌套链红 101）为注入态目标红；旧P production_tools6/worker_model10/usage_ledger15 | 注入态红亲跑；正常组合待 FINAL；I06-worker-permission 仍 NOT_OBSERVED（沿 G-INTERRUPTION 结论，不补造） | 旧额外 approval 11 过保留引用 |
| I07 网络策略/总预算 | proxy/direct/NO_PROXY/CA/TLS/DNS/redirect/总预算 | 旧P network21/timeout9/adapters13/credentials c10 | 复用旧P；本轮磁盘/网络环境未提供新业务结论 | LIVE 边界不变 |
| I08 媒体/资源/系统语音 | canonical 授权、host/provider ID、取消 fence | 旧P media_resource17/operations7/system_speech10；C-F46/H02 媒体取消 2 过 | 复用（H02 375 全等覆盖 C 资源线） | macOS 离线材料不外推其他平台 |
| I09 调用对账/usage/保密 | 物理尝试/父子 JOIN/unknown 不变 0/查询隔离 | 旧P usage_trace9/usage_ledger15/persistence5/strict7/families14；H02 当前 A13 完整（F48 修复后）246/41 零泄漏+correlation | H02 部分当前复用（375 全等）；其余组合待 FINAL | 6 秘密扫描绿不代 requestId 关联，后者 H02 已新验 |
| I10 资源归零/进程树/恢复/未知拒绝 | 规定负载+采样器正反控+恢复 | H02 resources-final：160 混合轮（60 预算 408+60 错误+15 正常+10 长响应+15 worker）、54 正式树点、61 owner 点、15 存活 worker/45 稳态、假 FD/TCP 零各 101→恢复 0、F46 六轮 [1,2,3,4,4,4]→[1,2,3,3,3,3] | 复用 H02（本轮 375/375 输入逐字节相等亲核） | 三链分列：①60 预算 408 future-drop 后 running 经重启消解、provider 零重执行（预算/重启路径）；②普通取消同实例恢复＝`subagent_closeout::parent_cancel_closes_children_in_process_repeatedly_beyond_the_cap` 与 `late_result_fence::r03_a07_late_result_after_cancel_and_next_run_pollutes_nothing` 两具名测试；③54/61 两类点不互冒。NEG_TARGET 本轮 CARGO_INCREMENTAL=0 冷重建，不套 H02 的 7fa13a 二进制身份 |
| I11 门禁自证/身份/恢复/假绿拒绝 | 原 16 负测目标红+点名+恢复绿；需求 ID 与原规格一致 | **本轮 default16-03 完整亲跑**：16/16 fail-closed 且逐项点名（表见 REVIEW.md §3）；N03 唯一权威 24→23 恰一次+恢复 1/1 绿；N06 绑定先于变异（sync=0）+stable=false overall FAIL；N16 两次不同绑定 a3ec…/dce6…+旧根拒收；Node verify PASS 64,765；最终 12 文件逐字节恢复（独立复算全等）；真实 shell exit=0 落盘。辅证：default16-01 的 N01–N15 同判、default16-02 缓存污染被 control 即时拒绝（fail-closed 又一生效样本） | **本轮关闭（G 包级）** | A1 121/100 checkpoint、I 56+13、B 15/41、F25/F26 27 项、J 1016 项复用边界见 REVIEW §5；G01 FAIL/G02 BLOCKED 永久保留不冲销 |

**full R02/full E5 消费说明**：本轮 N06+N16 三次完整 verify-stage R02 与 N02/N04 完整 producer 均为**注入态或绑定演示态**的运行——它们证明门禁在每种注入下 fail-closed 且点名（I11 义务），其中 N16 run-b 的 R02 20/20 命令业务 PASS、run-a 19/20（a16 E5 环境 worker 崩溃 fail-closed）为业务面的真实新证据；F47/F48/F50 修复后的五项原红命令（a01/a13/a05_a06/三 CLI supplemental）多轮全 PASS。**FINAL 注册的正常态完整 verify-stage R05/R04/R03/R02 闭包仍须新 FINAL 亲跑**，本表不代签。

**保留边界**：raw npm seal trio 登记红保持红（registered, never formal green）；D 的 ALF LAN 阻断 BLOCKED（default16-01 的 management 偶发拦截为其新增佐证，default16-03 未触发）；LIVE 未授权、Windows/Linux 及 R09/R10 平台延期沿原口径。
