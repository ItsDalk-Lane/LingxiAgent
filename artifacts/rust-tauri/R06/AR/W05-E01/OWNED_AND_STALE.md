# W05-E01 owned 与组合复验

唯一产品区为本正常 clone。基线 bc2ce51fd005ea75b9949156974e15d2761eb734；接受 F08，未复制 F01/F03/F06/F07 或其他未接受 overlay。Cargo.lock 保持 d4f8c2977adbde14dd42e670205f1054fa7ef1ddceb9f0b13c190c2bfa9d6fd4；无迁移 10、无新依赖。

| 精确 owned 路径 | 最小变化 | 共享消费者/后组合必须重跑 |
|---|---|---|
| rust/crates/lingxi-kernel/src/context.rs | 输入 PartialEq，仅用于真实双采集一致性 | T01 段/预算；F06 合法配置与原 parser 红保留 |
| rust/crates/lingxi-adapters/src/storage/agent_memory.rs | home publication epoch、原 atomic writes 与复合写 guard | T06 persona/memory/Skills config；F06 parser 与写入保存边界 |
| rust/crates/lingxi-adapters/src/storage/run_store.rs | durable actual-call snapshot 查询/原事件事实复用 | F01 身份/reader；F03 DB queue/opjournal/new migration；F08 raw/projection事件/配额原断言 |
| rust/crates/lingxi-service/src/context_compiler.rs | 每 Run fresh material、冻结 Arc、bounded observation index | 全 foreground/background/child；F01/F06/F07 |
| rust/crates/lingxi-service/src/runs.rs | 权威 Session memory/identity、本 Run system、实际 call 当前 tools 与 durable observation | F01 owner/actor/child；F03 cancellation/admission/finalize/queue；F07 tools/generation/history；F08 usage/raw/projection |
| rust/crates/lingxi-service/src/lib.rs | bootstrap/store wiring；原授权 observation route 的精确历史查询/cache stats | F01 scopes/Device/Web/LocalUser；F03 Session authority；F06 material wiring；F07 declarations |
| rust/crates/lingxi-service/src/session_admin.rs | 实际 archive/delete/cleanup 回收索引 | F01 authorization；F03 Session tree/recovery/busy/descendants |
| rust/crates/lingxi-service/src/skills.rs | runtime 同一 fresh preferences、来源与 hidden 不跨采集混用；实际写入口 guard | T06 安装/审核/启停/来源/卸载；F06 config合法解析；F07 tool执行权限/目录 |
| rust/crates/lingxi-service/tests/r06_t01_context_compiler.rs | 原错误 Session 永久冻结断言改为本 Run 冻结与下一 Run 动态 | 保留原所有段、预算、shape、language、wire/observation、输入字节断言 |
| rust/crates/lingxi-service/tests/r06_ar_f05_cache.rs | 实际子代理身份/actor、发布屏障、当前 tools 正控及 OPEN revocation 历史配对负控 | 全部新增具名检查永久保留；负控当前 FAIL，不能移出回归取绿 |

OWNED 之外 tracked 文件零 diff。自己的 evidence scripts/报告不是产品入口；local-only 与两个独立 target 不共享。自己的 .git/info/exclude 四精确目录仅本次自有缓存/合成运行，前后原文/hash/授权与独立 effective source 覆盖另存。tracked .gitignore、门禁、断言、阶段图、注册矩阵未修改。

STALE：所有最终候选绑定以前的当前绿窗口仅为对应旧 source 实测，不能替新 source；Cargo build 窗口的初始 cache SHA 与随后编译产物 SHA 如实分开，不能反填。F01/F03/F06/F07 接受后共享文件组合必须全新 integration + independent Review，含四 C、原 T01/T06全部受影响原用例、R04实时禁用/卸载/代次/配对、F08投影/usage、完整 workspace与正式 R05→R04→R03→R02。其他 Task 均 OPEN，不承接平台受阻 Reviewer、不能申报 R07/Stage 关闭。
