# R06 Agent deletion R3 — 可恢复真实删除净增量

结果：R3 新增 7 个真实 HTTP/磁盘测试通过；原 Dream 17 + Memory 7 通过；原 agent lifecycle 2 通过。严格 Clippy（service lib + R3 新测试、--no-deps、-D warnings）通过。主工作树未修改，无提交/推送，无门禁/原断言/登记变更。

## 补丁与相对基线

- 独立树：`/private/tmp/lingxi-r06-persona-completion`
- 净增量：`/private/tmp/r06-agent-delete-r3-incremental.patch`
- SHA256：`939049013ed797cc84bc0d32756f925b86cb3fb14f751cb68653e23159bf1aed`
- 文件前后 SHA：`/private/tmp/r06-agent-delete-r3-source-sha.json`
- 基线快照：`/private/tmp/r06-agent-delete-r3-base`，相对 Dream 原补丁 f8ec25d04dbd2c7d4b4623bed25790e12b7a9c0e8fe4d6c0c3745c79f220dbff 与 R2 29fac9faca2b09b85f98bc7d9d475845bb5a5996bcb304274afdd7291057eae6 已应用后的内容。
- 对该基线 `git apply --check` 通过。检查当前原始仓库路径发现尚无 dream_service.rs，因此该路径单独检查不能直接应用；需先有 R1/R2 基线，不可当成全量补丁。
- 只修改 agents_admin.rs/auth.rs/lib.rs/memory.rs/dream_service.rs 的必要 hunk；新 agent_deletions.rs 与 r06_agent_delete_completion.rs。原 r06_dream_completion.rs 字节未改。

## 真实提交点与失败实效

原缺口独立最小反例保留：`/private/tmp/r06-dream-delete-partial-experiment.rs` 与 `.log`。只对助手顶层目录设置 uchg，旧 remove_dir_all 返回500，但 facts/longterm 实际已删除。这是数据完整性失败，R2 的全树 flags 测试不曾修复它。

R3 在运行时目录 lingxi-service/agent-deletions 中建立0700私有事务目录，先持久 prepared 收据，再同盘 std::fs::rename 将完整助手目录原子搬为 data。无 copy 回退：搬不动就失败，任何记忆子文件未被清理；旧已付费 Dream 被取消但核心不永久停，新任务等旧 lease 真实释放后可启动。rename成功是逻辑删除提交点，核心永久关闭；之后的 references 修复和 remove_dir_all 只作用于私有暂存。cleanup失败不把可能已半清的 data 搬回公共目录，明确 logicalDeleted=true/cleanupPending=true。

准备、最后助手检查和并发双删使用同一个实际删除门，检查在 Dream 最终发布门内，最后助手拒绝发生在取消前。创建助手也进入删除门，已提交删除的 id 由持久收据保留，禁止旧 paid lease 与同 id 新助手混淆。原孤儿技能清理响应和现役事件顺序保留；references尚未修好时不发虚假的 switched 事件。

收据包含 schema/agent/deletion id、提交/清理/引用状态和时间。文件sync_all，Unix目录sync_all；先提交收据才清理。准备收据仍旧但真实 data 已搬走时，GET通过实际两处目录推导 logicalDeleted=true + journalCheckpointPending=true；重启恢复据同一事实推进。首次收据发布前的空/临时收据目录可安全清理，含 data/未知文件/符号链接的收据缺失目录显式拒绝恢复，不能擅自删或猜结局。

服务 bootstrap 接真实恢复；公开清理重试接口执行同一真实恢复/引用/清理函数。服务目录已接工具文件权限保护。Windows标准API不提供目录sync，因此该平台只声明文件同步和进程重启恢复实现；本轮 macOS 真实故障与重启验证，不宣称断电或其他平台已验证。

## HTTP契约（已与 Settings/UI 作者同步）

DELETE `/lingxi/v1/agents/{id}`：
- 搬不动：500，公共助手/配置/记忆完整，核心有效；旧任务取消/费用保留。
- 搬成功：200，`ok:true, logicalDeleted:true, deletionId, status, cleanupPending, referencesPending, replacementAgentId, skillsDeleted, skillsSkipped`。
- cleanup失败：status=committed，cleanupPending=true + cleanupError；助手不在列表，无公共 data。
- prefs实际写失败：referencesPending=true + referenceError，replacementAgentId=null，不冒充切换完成；已搬走的完整暂存保留待修复。
- 提交checkpoint写失败：journalError明确返回，data不清；原prepared收据和真实目录能在恢复时判断已逻辑删除。

GET `/lingxi/v1/agent-deletions`（settings.read）：`{deletions:[收据]}`，含 aborted（logicalDeleted=false）和完成记录。prepared且已真实搬走时返回推导状态与journalCheckpointPending。收据不披露物理暂存路径、内部schema或候选字段。

POST `/lingxi/v1/agent-deletions/{deletionId}/cleanup`（settings.write）：返回单收据，先恢复真实提交状态，再按当前实际 prefs 修复 A引用，再清暂存。清理失败仍如实 pending；完成为status=completed/cleanupPending=false/referencesPending=false。路径 id 严格验证，chat scope及未授权principal403，编码穿越400。

## 真实验证

`/private/tmp/r06-agent-delete-r3-regression.log`：R3初始5 + 原Dream17 + 原Memory7，共29通过。此时已含准备残留恢复/查询推导生产实现；新增查询证据在终验覆盖。
`/private/tmp/r06-agent-delete-r3-final-tests.log`：最终R3新增7全部通过（21.24s）。原 agent lifecycle2在最终源码另重跑通过，两项通过见 `/private/tmp/r06-agent-delete-r3-lifecycle.log`。
`/private/tmp/r06-agent-delete-r3-clippy.log`：strict Clippy通过。

7项新增证据：
1. 原真实顶层uchg反例：DELETE500时 config/facts/longterm/memory逐字完整；旧付费任务cancelled，解除故障后实际五阶段新Dream成功，共6实际请求/6费用条目。
2. 内层longterm uchg：真实rename后cleanup失败200 logicalDeleted/pending，A不在公共目录，prefs实际切B，旧付费任务结算不重建A；真实整个服务重启保留pending；解除暂存文件flags后cleanup完成，B模型Dream仍成功。
3. 并发删除A+B：恰好一个200/一个400，唯一剩余助手真正模型Dream可用。
4. 真实rename后收据文件uchg：真实checkpoint失败、prepared磁盘收据+完整data，GET如实推导提交；解除flags+服务重启恢复完成，A不重建。
5. 真实权限与穿越拒绝。
6. 真实 preferences uchg：删除逻辑成功、引用修复显式pending，解除故障后实际prefs/current-status读回B，data清理完成。
7. 首次收据发布前实际磁盘残留：服务重启清理仅未发布元数据，公共facts逐字保留，A核心可读，不产生虚假删除收据。

未使用真实供应商；模型替身为生产Gateway调用的真实HTTP供应商形状，并未mock被验收存储/生命周期核心。HOME/TMP/LINGXI_HOME全部合成，cargo指定路径/--locked，target在仓库外，CARGO_INCREMENTAL=0。首次重跑因磁盘满构建失败，Root清理自己的缓存后必要验证通过；不将该次构建失败记为测试通过。

## 准确范围

这是 R3 删除数据完整性和核心生命周期修复，不绑定或修改叶登记。三Dream叶/EX04核心可交主控独立核验；0B483D7F6A43自动ticker暂停/恢复整叶仍OPEN/PARTIAL，实际自动调度尚未实现，不称READY/延期。
