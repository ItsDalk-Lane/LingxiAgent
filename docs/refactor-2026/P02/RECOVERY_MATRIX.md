# RECOVERY_MATRIX — 崩溃窗口与最小持久化证据（P02-T05）

版本：1.0｜证据基线：HEAD `c3cd52859` + 本阶段改动。
结论先行：**本阶段零持久化结构改动**（无需 data epoch/迁移），只补齐真实进程退出重启/磁盘故障注入证据；各事实的"恢复承诺"语义如下表。

## 1. 写入位置与崩溃窗口（接受输入 → 外部动作 → 结果 → 终态）

| 事实 | 写入者（唯一） | 落盘时机 | 崩溃窗口语义 | 恢复扫描 |
|---|---|---|---|---|
| 后台 task 登记/终态 | TaskRegistry `_persist`（atomicWriteSync → `.ephemeral/plugin-tasks.json`） | 每次 register/update/complete/fail/abort 即写 | register 已落盘 = 接受已持久化；终态写失败只丢可见性（见 §2 语义） | `_loadPersisted`：活跃→recovering、attempt/父关联保留（A09/A10） |
| 延迟交付（媒体/子代理结果） | DeferredResultStore（`_save` 1s 延迟批量 / `flushSync` 显式回执） | dirty 标记 + atomicWriteSync | **durable 接受才承诺可恢复**：`deferred:register/resolve` 带 durable:true 时返回落盘回执；失败 → `{ok:false, durable:false}`，dirty 保留可原样重试（A11） | `_load` 恢复 pending；deliveryState 由 media TaskStore 配合 |
| 媒体生成状态 | media TaskStore（plugin-data/image-gen/tasks.json，attempt/generation 栅栏） | settle/attempt 变更即写 | submit 中断且无 provider 回执 → settle `failed: "generation interrupted during submission; provider acceptance is unknown and generation was not retried"`（**不自动重发**） | poller `_recover`：pending→重挂轮询或如上判定；terminal+delivery pending→重投递（durable 回执门） |
| 会话 JSONL | Pi SessionManager（追加式） | 每条 entry 即写 | 已写条目保留；中断回合由 interrupted-turn-marker 合成 user 消息标注 | 分支投影按 parentId 链重建 |
| 会话清单 sess_ | SessionManifestStore | 变更即写 | — | 启动 recovery（health 暴露 resolveSessionMetadataRecoveryStatusForHealth） |
| 观测（mc_/ma_/mt_） | observability.sqlite v7 | 事务写 | 不阻塞主流程 | 只读 |

## 2. 接受协议的响应语义（不悄悄把内存接收称持久接收）

- **承诺可恢复的接受**：仅 DeferredResultStore 的 durable 路径（flushSync 布尔回执；失败显式 `ok:false, durable:false, error:"deferred result persistence failed"`）。媒体 poller 拿不到 durable 回执即 throw "media result handoff is not durable" 并保持 deliveryPending 重试。
- **不承诺持久的接受**：TaskRegistry `_persist` 失败只 warn（best-effort visibility，媒体代码注释已声明该分工）——它的条目不承担交付承诺，重启后丢失等价于可见性回归，不产生重复副作用。
- **TaskRegistry 落盘的接受**（register 成功且 atomicWriteSync 完成）：重启后任务在、状态 recovering、attempt 保留——这是 A09 验证的"已确认输入不丢"。

## 3. 重放分类（读 / 幂等写 / 供应商幂等 / 不可安全重放）

| 类别 | 操作 | 依据 |
|---|---|---|
| 可安全重试读 | read/grep/history 回读、poller 查询供应商任务状态 | 只读；A10（同 attempt 续执行收口） |
| 本地确定幂等写 | 会话 JSONL 追加（parentId 链）、deferred flushSync 重试（同状态覆盖写）、TaskRegistry 终态重写（first-write-wins + attempt 栅栏） | A11 修复磁盘后原样重试成功 |
| 供应商支持幂等键 | 无本地虚构：adapterTaskId 复用查询（media poller 按 provider 任务 id 轮询，不重复 submit） | poller 逻辑；**不假装本地幂等键能让不支持它的远端去重** |
| 不可安全重放 | 媒体 submit（外部生成请求）、子代理执行、外发消息 | submit 中断→failed+unknown 理由（§1）；不自动重发 |

## 4. 重启扫描边界（只处理本运行时拥有的数据与未闭合记录）

- TaskRegistry `_loadPersisted`：只读自有 plugin-tasks.json；活跃→recovering（不运行、不重发），终态原样。
- poller `_recover`：只扫描本 store pending/未投递终态；`submitting` 且无 adapterTaskId 无 files → failed(unknown)，**不重试**。
- 同宅互斥闸（server/index.ts:273-291）：活跃 foreign server → `process.exit(1)`；dead 残锁自清。**两进程共用相同 HOME 写库被结构性阻止**（A15 第二 writer 拒绝实证）。
- 会话清单恢复有独立 health 状态位（session metadata recovery status），不与任务恢复混用。

## 5. 本阶段证据

| 场景 | 测试 | 关键断言 |
|---|---|---|
| A09 写后结果未存崩溃 | tests/p02-recovery-restart.test.ts（真实子进程 exit 70 + 同文件重启） | 副作用计数重启后仍 1（不自动重复写）；recovering+attempt 保留；可人工收口 |
| A10 只读恢复 | 同上 | 父关联保留、无伪造 result；同 attempt 合法续执行 |
| A11 持久化失败 | 同上（目录置为普通文件注入） | durable 回执 ok:false；内存状态可见未落盘（persistPath 不存在）；修复后原样重试 durable:true |
| A15 停服与重启 | tests/server-composition-boundary.test.ts Part 5（真实 server 进程三段循环） | 待执行记录启动不崩；第二 writer exit 1；SIGTERM 后端口 ECONNREFUSED（无孤儿 listener）；重启后记录原样、健康可用 |

## 6. 任务完成检查对照

进程实际退出重启后不丢已确认输入（A09：register 落盘记录重启仍在）、不重复未知副作用（A09 副作用计数、§3 不可重放类不自动重发）——非仅内存 throw 模拟（A09/A15 均为真实进程退出）。
