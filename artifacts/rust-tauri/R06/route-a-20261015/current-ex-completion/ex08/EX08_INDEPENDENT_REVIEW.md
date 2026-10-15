# EX08 独立复核

结论：本轮未发现需要阻断的生产缺陷。按用户明确授权保留七天核对；原永久delete_source、冻结409和权限拒绝没有放宽。发现1处原负控夹具与生产收紧不兼容：它通过add_membership造跨studio脏行，在进入HTTP负控前被正确拒绝。已交精确仅夹具补丁，改隔离SQLite直接INSERT，全部原HTTP拒绝/零副作用断言原样保留；Root已确认应用。

本轮不是静态签字：独立私树实际执行 Root8生命周期+25完整原知识父+3独立探针，共36 PASS。最终日志：`ex08-combined-lifecycle-final.log`、`ex08-combined-service-final.log`、`ex08-combined-probes-final.log`。初次15用例因私树缺未修改产品模板失败，补base只读材料后重跑；另一1处就是上述夹具问题。最早并发探针误要求另一连接必须阻塞，但SQLite实际及时busy拒绝；修正探针只要求租约中绝不能提交引用并保留原始日志，无生产修改。

精确被测源码逐SHA见 `ex08-final-tested-source-sha.json`；最初Root4源+原父冻结见 `source-snapshot-frozen.json`。随后EX07已合两知识源及真实parser/office/worker/Cargo/lock必要前置另有 `ex07-compatibility-prerequisites.json`；lib只加knowledge_processors声明，未复制Root中途其他变更。全部使用现存缓存，--offline --locked，无安装/依赖新增。

## 核对与动态证据

- 事务/竞态：retained delete、prune、with_artifact_retention、purge_orphan_if_unreferenced持事实库Immediate事务；查询使用时间与vector清理共SQLite写事务。独立第二真实连接在GC检查后、删除前尝试挂靠，实际拒绝且不提交新引用，GC后source/snapshot清除。源码 `adapters/tests/ex08_independent_review.rs`。
- 权限/归属：原25父实际HTTP覆盖跨studio/not-member/missing/只读、永久删除及冻结409零副作用；Root8还验证foreign maintenance无动作、共享来源null永久/最大保留日生效、重新挂靠清timer、scope/job/citation引用保护。
- 原文/缓存：Root8实际存储验证旧向量到期、当前variant和原文逐bytes保留、真实search续期/旧扫描不能删除，重启一致；v1 usage附加迁移不改原vectors、future user_version拒绝且db bytes不变。未结清付费统计是合成持久fixture，真实生产GC+checkpoint字节比较通过；没有调用付费供应商。
- 七天/EX07：独立 `service/tests/ex08_processing_retention_review.rs` 真实CSV→当前worker子进程→processing ready/structural产物；七天前1ms snapshot与processing txt逐bytes保留；重启后满七天实际删除processing FK行/semantic+txt目录/snapshot，用户原CSV逐bytes保留。最终收据 `/var/folders/5r/wq54gtms21gfjgr5v94ynfyh0000gn/T/ex08-processing-independent_80742d449db079e7584a7bae/ex08-processing-proof.json`。
- GC文件边界：Root8实际artifact父目录symlink拒绝、原文和外部文件不动；独立probe将已打开knowledge根rename并在原路径放替代同名snapshot，还在原产物放指外部payload的末端链接。实际仅从已打开原根删除，替代根/外部payload逐bytes不变。
- 不可达状态：原完整父 `original_ingestion_listing_is_studio_scoped_and_reports_unreachable_sources` 实际通过，服务明确on_demand_stat/watching:false，不冒充常驻watcher。

## 交付与范围

- `ex08-legacy-fixture-only.patch` 只有故意造脏行夹具最小hunk，不覆盖Root测试改名。
- `ex08-independent-probes.patch` 只新增上述两个可复现父测试文件，原始源码也留私树；Root可按需取独立证明。
- macOS实际执行；Windows/Linux文件清理实现未运行，NOT_RUN。本轮不宣称完整打包或常驻watcher/cron全验。没有修改Main、正式原断言、登记或门禁。
