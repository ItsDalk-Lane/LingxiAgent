# R06-T03 REPAIR-R1 修复矩阵（fix matrix）

依据：REVIEW-01 九项 finding + 一项观察 + 管理者对 FINDING-05 的裁决
（rewind 必须内容级回滚：检查点真实存内容；无冲突文件真实写回；逐文件
sha256 冲突检测保留；收据如实标注 restored/conflicted/skipped；
externalEffects 恒 not_rolled_back；retry fileRollback 复用同一机制）。

## 同根因归并

- 簇 A（FINDING-01 + 扩展）：「检查点行变更不清子行」——Overwrite 不删旧版本行、
  裁减不删子行（FK 失败）、upsert 与版本记录分两次写事务（非原子）。
- 簇 B（FINDING-05 + FINDING-07 + FINDING-02）：「无内容存储 → 恢复缺席」——
  file_checkpoints 死表激活为内容承载；rewind/retry 共用一套内容级恢复。
- 簇 C（FINDING-03/04/06/08/09 + 观察项）：独立 LOW 项 + fork 撞库 500。

## 矩阵

| Finding | 根因 | 修复（落点） | 回归测试 |
|---|---|---|---|
| F-01 HIGH | Overwrite 不清子行；upsert 与版本记录两次写事务 | adapters `UpsertCheckpoint` 增 files 载荷（sha+内容），单事务内：Overwrite 先删旧 `checkpoint_file_versions`/`file_checkpoints` 再写新行；service `create_checkpoint` 改单次调用；独立 `record_file_version` 入口退役（调用方归零，随改动移除） | adapters：latest 覆盖×同文件（再记录成功）/×不同文件（无残留）；service：HTTP latest 覆盖×filePaths 二次 200、rewind 按新集合判定 |
| F-01 扩展（自查） | 201 裁减 DELETE 不清子行 → FK 失败 | 裁减分支先删两套子行再删检查点行 | adapters：201 窗口且每点带文件版本 → 裁减成功、子行清零 |
| F-05+F-07 | 仅存哈希 → 恢复结构性缺席 | v8 `file_checkpoints` 主键改 `(checkpoint_id,file_path)` 并加 FK（未出库，直接修订 V8_SQL）；检查点写入真实内容字节（encoding utf-8/base64，对照现役 checkpoint-store.ts）；rewind 三档判定：当前==目标→skipped；当前∈该会话历史见证哈希集→restored（真实写回字节）；否则（含被删）→conflicted（不覆盖该文件）；分支照常回移；收据 restored/conflicted/skipped/failed + externalEffects=not_rolled_back；preview 同三档 | kernel 三档判定单测；adapters 内容往返；service：外部改→conflicted+文件保留+分支回移；模型改（见证）→restored 真实字节；未变→skipped；被删→conflicted；偏好关 403 保持 |
| F-02 | retry fileRollback 子句缺席 | `RetryRequestBody` +fileRollback（none/workspace；非法→400 invalid_file_rollback；workspace+偏好关→403 file_rollback_disabled）；service retry 复用同一内容级恢复（锚点=turn_input_message_id 匹配的检查点；无→no_checkpoint 报告且不阻塞分支重置，对照现役 restoreTurn no_checkpoint 与 per-file 失败不阻塞）；`RetryOutcome` +fileRollbackReport；`create_checkpoint` 回填 turn_input_message_id | service：非法值 400；偏好关 403；workspace 真实恢复+报告；无检查点 no_checkpoint 且分支照常重置 |
| F-03 | 主列表无 lifecycle 过滤 | run_store `list_sessions` + `WHERE lifecycle='active'` | service：归档后主列表不含、restore 后复现（HTTP 级） |
| F-04 | fork 复制列裁剪+时间戳改写 | fork SELECT/INSERT 全字段保真（model_call_id/committed_at_unix_ms/entry_type 原样，仅 session/seq/parent 重链，对照 session-manager.js:1113 浅拷贝）；`AppendMessage` +model_call_id、`MessageRow` +model_call_id/committed_at_unix_ms（使保真可观测） | adapters：fork 后逐字段等值断言 |
| F-06 | v8 重建丢 run_id FK | V8_SQL `run_id TEXT REFERENCES runs(run_id)`（可空保 FK）；`append_message` run_id→Option；reset 标记 NULL；`DbUserMessageRecorder` 改 NULL（归属经 `user:{run_id}` 消息 id 保持） | adapters：幽灵 run 插入响亮失败；None 成功 |
| F-08 | 计数无条件 +1 | 仅 UPDATE 影响行数>0 才计（对照现役 sessions.ts:2569-2660 逐子成功才计） | adapters：混合子树计数=真实翻转数 |
| F-09 | RED 证据退出码空缺 | `02_red_tests.txt` 补录真实历史退出码（编译失败=非零）并标注补录时间 | — |
| OBS | fork 撞库→Internal(500) | fork/create_session 事务内预检→`Conflict("session_exists:…")`→409（对照现役 sessions.ts:587 active_session_conflict 409）；同类扫描：create_session 同型修复 | adapters 撞库 Conflict；service HTTP 409 |

## 删除/退役清单（我的改动产生的孤立代码，随修复移除）

- `record_file_version`（adapters 公开入口）：service 改单次组合调用后无生产调用方。
- `SessionTreeError::RewindConflict` 及其 HTTP 映射臂：整批 409 语义被逐文件
  conflicted 取代（设计稿 A06 原本语义）。
- kernel `FileRestoreVerdict` 两档 → 三档（`judge_file_restore` 签名随语义升级）。

## 同类生产入口扫描记录

- 检查点子行清理三入口：upsert Overwrite、裁减、delete_checkpoint、
  delete_archived_session（14 步 purge 已含 file_checkpoints）——前两入口本修复，
  后两入口已正确。
- sessions INSERT 撞库入口：fork_session、create_session（session_admin）、
  seed_session（测试辅助，不经生产路由）——前两入口本修复。
- messages 写入入口：append_message、fork 复制、reset 标记、run_store final
  ——全部随 FK 恢复核对（final 消息 run_id 为真实 run，满足 FK）。

## 修复后回归捕获的追加项（2026-10-09；本矩阵定稿前由全量回归捕获，如实补记）

上文「同类生产入口扫描记录」中「delete_archived_session 已正确」的断言**经实测证伪**
（service 管理面套件 archived delete → 500 FOREIGN KEY constraint failed）。两项追加修复：

| 追加项 | 根因 | 修复（落点） | 回归测试 |
|---|---|---|---|
| F-07 扩展② | 激活 file_checkpoints FK 后，`delete_archived_session` 的删除顺序违反 FK：先删 checkpoints 父行、后删 file_checkpoints 子行 → 永久删除带检查点的归档会话必 500 | session_admin `delete_archived_session`：file_checkpoints 删除上移至 checkpoints 删除之前（14 步 purge 步数不变、顺序修正）；全部 checkpoints 删除点复核：upsert Overwrite / 裁减 / delete_checkpoint / delete_archived_session 四入口均为「子→父」 | service：`management_surface_*` 内 archived delete → 200（修复前 500） |
| F-04×F-06 交互 | fork 副本全字段保真携带源会话 run_id → 恢复 messages.run_id FK 后，源会话永久删除时其 runs 行被子会话副本引用 → FK 拒绝（现役按会话分文件存储，无此跨会话耦合） | fork 副本 run_id 置 NULL（其余字段保真不动）；源 run 可追溯性由消息 id（`user:{run_id}`/`{run_id}-final`）承载。台账记为 D9（对「全字段保真」的唯一有意偏离） | service：`management_surface_*`（fork 子会话存在时永久删除源会话 → 200）；adapters：`fork_preserves_message_fields_verbatim` 断言副本 run_id 为空、其余字段逐相等 |

另：`find_checkpoint_by_turn_input` 排序补 `rowid DESC` 决胜（同毫秒双检查点锚点确定性）。
