# ROLLBACK_DESIGN｜切换后回滚不丢新数据（R01-T07 / R01-A14）

- 状态：READY_FOR_REVIEW（执行者结论；独立验收归总控另派代理）
- 日期：2026-09-25
- 修订：2026-09-25 ZCode:R01-T07-repair-r1 按验收 REVIEW_R1 F2 补登印章篡改窗口的
  协作式闸定性（§1 第 5 条），修复详情见 `R01-T07_REPAIR_R1.md`
- 演练证据：`artifacts/rust-tauri/R01/T07/a14/`（`rollback-drill-run.log`：ROLLBACK-DRILL-PASSED；
  `rollback-drill-summary.json`；探针日志 `a13/a14-restored-epoch1-starts.log`）
- 演练脚本（真实跑通，非文档虚构）：`artifacts/rust-tauri/R01/T07/scripts/rollback-drill-setup.py`、
  `rollback-drill.py`、`atomic-switch.zsh`、`run-start-probe.zsh`

## 1. 原则

1. 恢复旧备份只恢复旧时点，**不称无损回滚**（02 §8）。
2. 回滚流程中**不存在「直接删新目录」步骤**；新数据先导出归档，再动指针。
3. 新数据导出归档后，在再次前滚时**幂等合并**（按主键去重，重复导入不产生重复行——
   演练 step7 已实测：两次合并后逻辑内容 sha256 稳定）。
4. 备份一致性：SQLite 一律走 Online Backup API（W06），WAL 先 `wal_checkpoint(TRUNCATE)`
   再备份（W07）；多库与文件以「停写 → checkpoint → 逐库 backup → 文件快照 → 清单哈希」
   为共同静止点；禁止复制活跃 `.db` 单文件（01 §8）。
5. **闸是协作式的（验收 REVIEW_R1 F2 / ADV-B 实证，repair-r1 补登）**：epoch 软件闸
   依赖盘上印章/日志完好且未被改动——手动把印章 2→1 后旧程序在篡改窗口内真实写入
   96 条目，改回 2 后恢复拒写。含义：任何依赖「旧程序不会写高位目录」的环节（含本
   回滚设计 step5 的「旧程序启动验证」与切指针窗口）都必须以物理分离根 + 停写确认为
   前提，不能把软件闸当作防篡改屏障；此定性支持 ADR-004 D2「分离根必需」的结论，
   不与任何既有声明冲突。

## 2. 时间线与数据驻留

```
t0  epoch-1 旧世界（JSONL 会话 + 附件文件 + 知识/记忆/配置原格式）
t1  切换前备份 B0（停写一致拷贝，含印章）
t2  切换：epoch-2 新根（新运行/消息库 + 保留格式各库 + 切换收据），active → epoch-2
t3  切换后新增：新会话/消息、新附件、新配置编辑（只写 epoch-2 根）
t4  回滚决定
```

回滚后各数据的驻留与旧版可用性：

| 内容 | 回滚后驻留 | 旧版能否继续编辑 |
|---|---|---|
| t0 之前的一切 | 旧备份 B0（恢复后生效） | 能（原格式原内容） |
| t3 新增会话/消息 | 回滚归档（JSONL 导出）+ epoch-2 根原样保留 | **不能**（新权威库内容，旧版无读写器） |
| t3 新增附件/文件资源 | 回滚归档（逐文件 + sha256）+ epoch-2 根 | **不能在旧版 UI 引用**；文件本体可人工取回 |
| t3 对保留格式存储的编辑（偏好/记忆/知识） | epoch-2 根（同格式文件） | **能**（格式冻结保证旧版可读）；但注意恢复 B0 会盖回旧时点——见 §4 检查表第 6 项 |
| t3 新增配置键（旧版不认识的字段） | epoch-2 根 | 旧版忽略/不识别；不得声称保留其行为 |
| epoch 印章/过渡元数据 | 各根自带 | 不适用（协调器管） |

## 3. 回滚流程（演练已逐步跑通）

前置确认：所有新旧进程停止（`server-info.json` 无存活写者；desktop 已退出）。

1. **预检**：确认 `active` 指向 epoch-2、印章 `committedDataEpoch=2`、无写者。
   （演练 step1）
2. **新库一致性备份**：对 epoch-2 根内每个 SQLite 执行 `wal_checkpoint(TRUNCATE)` +
   Online Backup API 到归档；`integrity_check=ok` 才继续。（演练 step2，W06/W07）
3. **导出归档**：新权威消息 → 确定性 JSONL 导出（含 post_cutover 标记）；附件逐文件
   拷贝；切换收据副本；生成 `manifest.json`（每文件 sha256 + 类别）。
   归档路径形如 `{数据根}/rollback-archive/{timestamp}/`。（演练 step3）
4. **恢复旧备份到独立根**：`cp -a B0 → {数据根}/epoch-1-restored/`（**不是**覆盖
   epoch-2，也不是删新目录）；原子切换 `active → epoch-1-restored`
   （临时 symlink + rename）。（演练 step4）
5. **旧程序启动验证**：旧 server 在恢复根上正常启动（实测 STARTED=1，loopback 应答），
   且 epoch-2 根逐字节不变（演练内哈希比对）。（演练 step5）
6. **归档校验**：按 manifest 重算 sha256 全绿。（演练 step6）
7. **再次前滚的准备**：归档可幂等重新导入新库（INSERT OR IGNORE 按主键；
   演练 step7 两次合并逻辑内容哈希一致）。

## 4. 操作检查表（草案，供 R08/R11 演进为正式 runbook）

- [ ] 1. 确认无写者进程（新旧 server、desktop、CLI 后台任务全部停止）
- [ ] 2. 确认 `active` 指针当前目标与预期一致；印章可读且committed
- [ ] 3. 对 epoch-2 全部 SQLite 执行 WAL checkpoint + Online Backup；integrity_check 全 ok
- [ ] 4. 导出消息/会话为 JSONL；拷贝附件；复制切换收据；生成 sha256 manifest
- [ ] 5. 归档写入完成并校验后，才允许进入恢复步骤（顺序不可颠倒）
- [ ] 6. **保留格式存储的二选一决策点**：若回滚目的是「旧版继续用最新偏好/记忆/知识」，
      则从 epoch-2 根按矩阵 `preserve-format` 清单选择性合并这些文件到恢复根
      （同格式，旧版可读）；若目的是「回到切换时点状态」，则纯恢复 B0。两者互斥，
      必须在 runbook 中显式勾选，禁止默认混用
- [ ] 7. 恢复 B0 到 `epoch-1-restored/`（独立根）；原子切指针
- [ ] 8. 启动旧版验证：健康应答 + 抽样旧会话可读
- [ ] 9. 验证 epoch-2 根与归档哈希未变
- [ ] 10. 向用户明示「无法在旧版继续编辑的内容」清单（§2 表）
- [ ] 11. 再次前滚时：重跑切换工具链，归档幂等合并回新库，重复导入不产生重复

## 5. 演练实测记录（A14 证据）

- 世界构造：epoch-1（s-001 会话 JSONL + 旧附件）→ B0 备份 → epoch-2（WAL 模式
  messages.sqlite 含 2 条导入历史 + 2 条切换后新消息 + 新附件 + 切换收据；WAL 故意保持
  热态以证明备份一致性不依赖冷文件）。
- 结果：`ROLLBACK-DRILL-PASSED`——7 步全绿；post_cutover 2 条消息与 1 个新附件全部
  进入归档且 sha256 校验通过；旧程序在恢复根启动成功；epoch-2 根全程零变化；
  再导入幂等。退出码 0。
- 限制（如实登记）：演练新库为**设计级替身**（合成 messages.sqlite），非 R02 真实
  Rust 存储——真实库实现后本演练脚本需按其 schema 重跑（验收场景 ID 归 R08 迁移演练）；
  演练用 python sqlite3 的 backup API 演示 W06，生产实现语言为 Rust（rusqlite backup）
  时语义等价性需在 R02 复证。
