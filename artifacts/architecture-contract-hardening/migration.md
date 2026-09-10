# 迁移与恢复合同补强

## 当前结果

- 工作区：LingxiAgent-contract-hardening；调查与回归基线为 `e0afc0dc3b94bc9763d715f981d6f948d1c39c27`。
- 本文描述未提交改动；没有提交、推送、发布或接触真实用户目录。
- 先新增真实模块回归再修改生产：首次 18 个用例中 14 个失败、4 个通过，见 `migration-red.log`。
- 最终定向验证：10 个文件、172 个测试全部通过，退出码 0，见 `migration-green.log`。本机 Node v24.16.0。
- 最终 `npx tsc --noEmit -p tsconfig.test.json` 因并行修改中的 `tests/session-coordinator.test.ts:1379` 在非 async 函数使用 await 而失败，退出码 2；见 `migration-typecheck.log`。本组没有改动该文件，已交主代理集成处理。
- 本组文件的 `git diff --check` 通过，退出码 0。

## 已确认缺口与改动

### 提交尝试不能被目标原字节替代

基线的目标文件先于 target_committed 收据写入。收据写入失败会被启动扫描记录后继续运行，用户可以正常删除已经导入的条目。若删除后目标恢复为迁移前的相同字节，旧 prepared 分支将其误认作从未提交，下一次启动重新导入。

回归通过真实 migratePinnedMemoryToTenets、removeTenet 与磁盘文件复现：仅对 target_committed 收据的 rename 注入一次 EIO，保持其他存储与业务层真实。另一个恢复分支即使已经是 target_committed，也会因为目标回到旧字节而重新执行。这是跨模块完成语义缺口。

实施：

1. 新收据为 v4，顺序为 prepared → committing → target_committed → sources_archived → completed。
2. committing 收据必须在原子写目标之前成功落盘；失败时根本不尝试目标写入。
3. 只有新 v4 prepared 能证明从未尝试提交，允许按不变快照重放。
4. committing、旧 v2/v3 prepared、target_committed 均只能依靠当前完整目标证明补做结算。缺少证明就持久记录 conflict，保留源与目标，不重写目标。
5. v2 升级时旧 prepared 被映射成 committing；更换格式不能凭空得到“从未提交”的证明。
6. 恢复的 target_committed 独立于目标原哈希判断，不会重新执行。冲突具有粘性；同一批准不能通过反复重试改变结果。
7. 启动仍可继续，但 failed/conflict 收据会产生明确的未完成日志。

兼容性取舍：committing 已落盘而目标尚未写入时发生硬中断，无法安全区分“确实未写”与“写过后被改回”。因此该窗口保留资料并报告冲突，需要重新审阅恢复；不承诺自动续做。旧 prepared 缺少提交证据时同样保守处理。旧 completed 仍然直接返回，不复活用户后续删除的内容。

没有改变 tenets 数据格式，没有增加全局写入框架，也没有改动普通用户编辑路径。

### 来源损坏不能被当成删除语义

基线 parsePinnedJsonItems 会过滤空内容项，使 `[null]`、缺少 content、或有效与坏项混合的来源被部分接受甚至成为空计划，然后归档并标记完成。核对 `04f90d2b^:lib/memory/pinned-memory-store.ts`，旧 readStoreItems → serializeItems 对这些条目原本就会抛错。

现在任一条目按旧规则归一后为空即整体失败，原来源和目标不改，恢复扫描也不会给出伪造的空候选。真正的 items:[]、历史长内容/超量、缺失 ID/时间，以及旧规则允许的非空内容转换保持原行为。

### 操作身份与持久路径往返

恢复批准接受 restore_1，但原 backupDir 解析不接受下划线。现在路径写入与读取支持该合法身份，并校验写入端的路径段；规范 POSIX 与旧 Windows 表示均能往返。混合分隔符、绝对路径、NUL、点段和多层逃逸仍被拒绝。

## 验证边界

新增和既有验证共同覆盖：

- 启动提交收据 EIO 后继续运行、用户删除、再次启动，删除内容不复活。
- 新 prepared 正常续跑；committing 在目标前硬中断变为冲突；目标已提交时只补结算。
- 真实子进程 SIGKILL 位于提交意图落盘后；既有测试还覆盖归档删除原件后的真实 SIGKILL。
- 提交后用户新增保留；全部删除、部分删除、修改正文不重执行或假完成。
- 旧 v2/v3 prepared 的完整目标与缺少目标证据分别处理。
- 完成操作重放、同任务批准改变、后续删除、来源/目标变化、备份失败、计划缺项与重复项。
- 空计划、权威来源选择、坏来源整体失败、严格目标读取、普通配额保持。
- v4 committing 与 Windows 旧路径表示组合、旧 v2 prepared 的保守升级、下划线身份往返、危险路径拒绝。
- CLI dry-run 只读；apply 继续由既有 BLOCKED_HOME_OWNERSHIP 保护拒绝。

关键迁移和恢复模块没有替身；大部分故障用生产故障点抛错，另有真实子进程终止、本机文件权限错误和指定一次收据 rename 失败。所有夹具位于临时合成目录并已清理。

未执行：真实 Windows/Linux、真实用户数据恢复、实际 CLI apply、断电或文件系统持久化顺序测试。本机 SIGKILL 证明进程硬终止路径，不等同于断电耐久性证明。

## 运行命令

```text
npx vitest run tests/pinned-tenets-contract-hardening.test.ts --maxWorkers=1

npx vitest run tests/pinned-tenets-contract-hardening.test.ts tests/pinned-tenets-migration.test.ts tests/pinned-tenets-migration-recovery.test.ts tests/round2-pinned-migration.test.ts tests/round2-pinned-migration-faults.test.ts tests/round2-pinned-recovery.test.ts tests/round2-pinned-recovery-faults.test.ts tests/round3-c02-receipt-backup-dir.test.ts tests/memory-tenets.test.ts tests/tenets-source-quota.test.ts --maxWorkers=1

npx tsc --noEmit -p tsconfig.test.json
```

`migration-first-check.log` 保留实施中第一次全组检查：新回归已通过，仅 3 个旧测试仍断言收据 v3、提交后 prepared、旧 prepared 自动重放。随后按批准的新合同更新这些断言，并增加目标和原件保留验证；没有放宽数据完整性断言。
