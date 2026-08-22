# OBSERVABILITY_STORAGE_PROGRESS.md — Phase 7 进度（Durable Model Observatory Storage）

> 本文件是 Phase 7 的跨会话断点文件：新会话先读本文件 +
> MODEL_OBSERVABILITY_STORAGE_AUDIT.md + PAYLOAD_CAPTURE_PROGRESS.md，再动代码。
> 基线：`feature/model-call-observability`；Phase 6 功能树 `7374e0d6`（seal `f7d0fee5`）。

## Current phase

**Phase 7 完成（功能树见 git log；seal 推进见下）。** 开发顺序按任务书 §一百三十一执行：
Step 0（基线 f7d0fee5）→ 1（MODEL_OBSERVABILITY_STORAGE_AUDIT.md 十问）→
2-3（架构决策 + SQLite schema v1 + migration contract）→ 4-6（Trace/Payload Store +
bounded coordinator）→ 7（engine/server wiring）→ 8-9（retention + crash
reconciliation）→ 10-12（Blob Store + externalizer + GC/orphan/missing recovery）→
13（Store Registry ×2 + fingerprint introspector + export-manifest）→ 14-17
（6 测试文件 44 用例）→ 18-23（typecheck ×3 / eslint 0 error / lint:boundary /
scanner / fingerprint compatible repin / targeted + full test）→ 24-25（文档 + seal）。

## DB schema version

`MODEL_OBSERVABILITY_SCHEMA_VERSION = 2`（Phase 8 起；v1→v2 显式单事务 migration
只新增 model_call_usage + conversation index；`PRAGMA user_version` 自管；未知高
版本/迁移失败/损坏 → disabled handle + reasonCode，主程序不受影响；read side
supportedReadVersions=[1,2]，v1 历史库不迁移可读——见 OBSERVABILITY_QUERY_PROGRESS.md）。

## Store paths

```text
{LINGXI_HOME}/model-observability/
├── observability.sqlite (+ -wal / -shm)   # 单 DB 逻辑分表
└── blobs/{shard2}/{blobId}.bin            # blob 外置字节（随机 id，无原文件名）
```

表：observability_meta / traces / model_calls / model_attempts / payload_records /
blob_objects / payload_blob_refs。18 个查询索引（trace/started/model/subsystem/
terminal/attribution_kind/session/agent/task 等，§五十二清单）。
Pragma：WAL / synchronous=NORMAL / busy_timeout=5000 / secure_delete=ON /
auto_vacuum=INCREMENTAL / foreign_keys=OFF（显式容忍 out-of-order，§二十四）。

## Trace projection status

完成。9 类事件全投影（§二十）；attempt shell 幂等 upsert；payload 先到 → partial
call shell（started_at NULL，不虚构）；Startup Reconciliation：崩溃遗留 call 只标
`interrupted_by_restart=1`，terminal_status 保持 NULL（§四十六/四十七）。

## Payload persistence status

完成。Store 只实现 Phase 6 `ModelCallPayloadSink` contract（sanitized detached copy
only）；fail closed（序列化失败/超 1M hard limit → drop + 计数，不保存残缺 JSON）；
provenance（semantic + provider 两类）随 payload JSON 持久化；providerRequestOrdinal
保留且不与 attempt 混淆；captureStatus staged→stored/store_failed 归一是存储态记账
（非第二次业务 redaction）；id 自增 = 稳定排序 tie-break。

## Blob status

- infrastructure FULL：atomic 写（随机 staging→rename，0600）、metadata/ref 事务提交、
  ref-count GC（只删 0-ref，§九十一）、orphan grace 24h 回收（§九十二）、
  missing 标记不 crash（§九十三）、size cap 64MB + queue cap + pendingBlobBytes（§七十三）。
- eligible binary ingestion PARTIAL（诚实，§七十四）：Buffer/TypedArray/ArrayBuffer
  可保存（同步复制）；**Blob 实例 / base64 / dataURL 保持 externalized**（无法同步
  读取字节，不引入 async 进 capture 热路径）。测试与文档均已明确。

## Retention

`ModelObservabilityRetentionPolicy`{traceMaxAgeMs/payloadMaxAgeMs/blobMaxAgeMs/
maxTraceRows?/maxPayloadBytes?/maxBlobBytes?}；safe fallback 集中定义（trace 180d /
payload 30d / blob 30d，§五十五）；删除单位 = 完整 trace（§五十六）；payload 可先过期
（call 标 payload_availability='expired'，metadata 保留）；Usage Ledger 不受影响；
maintenance = startup once + 1h timer（unref，runWithoutModelTrace detach）+ 显式触发。

## Queue health

三 bounded queue（trace 4096 / payload 2048 / blob 256 个 + 64MB bytes）独立容量
（§一百一十 trace 优先）；overflow drop newest + 显式计数 + call 标
payload_availability='dropped'；drop/writeFailures 计数持久化到 observability_meta
并跨 restart 恢复（§四十三）；transaction throw → rollback 后单次 retry → 再失败
诚实 drop 整批（§四十九，防 poison batch livelock）。

## Persistence lifecycle

- 安装：`installModelObservabilityPersistence({ lingxiHome, policy })`；默认
  policy disabled（不建任何文件，生产行为 = Phase 6）。
- 生产 wiring：`new LingxiEngine({ modelObservability })`（engine_construct 阶段安装，
  早于一切模型调用）+ `startServer(root)` 透传（CompositionRoot.modelObservability）；
  engine.dispose() 以 5s bounded timeout flush+close（§四十五）。Phase 8 再接
  UI/settings；**无隐藏环境变量开关**（§八十一）。
- composite observer/sink：安装期间既有 test/debug sink 继续收事件（§八十四）；
  close/uninstall 恢复先前注册对象（§八十五）。
- policy：persistTraceMetadata / persistPayloads / persistBlobs 独立开关；
  persistBlobs ⊆ persistPayloads（§八十三）。

## Tests

新增 6 文件 44 用例：store-schema 8（v1/indexes/migration/未知高版本/损坏/权限/
默认 disabled/blobs⊆payloads）/ trace-projection 5（树/双 attempt/crash/restart/
orphan shell）/ payload-store 7（毒丸落盘字节扫描、正文 roundtrip、provenance、
locator roundtrip、shell、fail-closed、排序）/ blob 7（roundtrip/PARTIAL/GC/orphan/
missing/size cap/queue overflow）/ persistence 9（batch 无 duplicate、write failure、
overflow、trace 优先、graceful flush、composite/uninstall、计数跨 restart、
payload-only expiry、trace 整树删除）/ durable-matrix 8（MC-01/02+03+10/04/05/06
codex 401 双 ordinal/07 opaque/08/09 speech+Volcengine）。
既有观测 27 文件 302 用例回归全绿。

## Guards

typecheck ×3 绿；eslint 0 error；lint:boundary 绿（manifest 收录 7 新模块）；
scan-persistent-stores 61 stores 全绿（新增 model-observability-db/-blobs 两个
descriptor + siteRules）；persistence fingerprint compatible repin
（sha256:f3d6c1f9…，introspector 开真实 store 读 sqlite_master）；
cli-runtime-closure 重算；data-epoch 80 用例绿（affectedByEpochMigration=false
结构性排除 checkpoint）。

## Known storage gaps

1. Blob ingestion PARTIAL：Blob/File/base64/dataURL 不入 Blob Store（见上）。
2. at-rest 无加密（全仓无 keytar/safeStorage）：私有目录 0700 + 文件 0600 +
   explicit opt-in + bounded retention；文件系统权限级保护，非密码学（audit
   §At-Rest Security）。
3. 同用户进程可直接读文件；备份/同步渠道复制明文；SSD wear-leveling 使
   「删除后物理不可恢复」不可承诺（§九十，只承诺 logical deletion +
   secure_delete + blob unlink）。
4. Multi-process：同宅 server 互斥闸防长期并发；WAL+busy_timeout 兜短暂重叠；
   长期双写未做应用级仲裁（当前拓扑唯一写者是 server 进程）。
5. flush 为同步 SQLite batch（≤ queue caps + busy_timeout 5s/语句），未引入
  worker_threads（§三十八：测量证明不可接受才引入）。
6. Query/API/Export 已由 Phase 8 交付（OBSERVABILITY_QUERY_PROGRESS.md）；最终 UI 留 Phase 9。

## Seal

功能提交后 VERIFIED_SOURCE_SHA 按仓库既有机制推进到本轮验证树（单独 audit commit）。
