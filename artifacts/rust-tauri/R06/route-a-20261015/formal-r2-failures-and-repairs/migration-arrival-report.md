# 两项测试修复交付

结论：仅负责的两文件已修复，当前 Main 实际 Cargo 定向验证 22/22 PASS。原正式 98472 的失败保持原记录；本结果不是全量正式验收通过。HEAD 未改变，仍为 14453cd05b8aeec7951f27c4dbd021ad2a21d486，未提交、未推送。

## 修改范围

- `rust/crates/lingxi-adapters/tests/session_file_store.rs`：当前支持版本和 tip 名称固定为 14 / session_proactive_delegation_at_creation，历史 v9 和 v13 名称独立保留；v7→当前的精确应用链固定 [8,9,10,11,12,13,14]。旧 title、lifecycle、四个 v9 表原断言保留。真实旧行升级后 proactive_delegation=0，PRAGMA 核 INTEGER/NOT NULL/DEFAULT0。真实 receipts 的 version/name/fingerprint 对独立 R02 登记全 14 项、旧七项五字段完全不变，新 receipt 时间与作者准确。第二次 apply_all 不产生新迁移、不改变收据、schema_version、user_version 或完整旧会话行。
- 新增约束回归：真实 SQLite 写入 0/1 均读回；2/-1 以 SQLITE_CONSTRAINT_CHECK 拒绝，NULL 以 SQLITE_CONSTRAINT_NOTNULL 拒绝。非法更新同时尝试改 title，验证整次写入回滚，合法值和标题均未被污染。不是只匹配 SQL 文本或只改版本数字。
- `rust/crates/lingxi-service/tests/background_steering.rs`：采用已审 ArrivalInbox + pending 队列，先提取对应事件，交错其他会话/轮次事件保留；原八测试只替换八处 channel 工厂。新增一个预先固定乱序的回归，验证 beta1/alpha2 在等 alpha1 后仍可按准确 run 取回，取过的不能重复返回，超时取消等待后新到达仍可读取。等待均有显式上限，没有通过串行化业务场景规避并发。

## 实际验证

`session_file_store.log`：原 12 + 新 1，共 13 PASS，0 FAIL，测试正文 0.15 秒；`background_steering.log`：原 8 + 新 1，共 9 PASS，0 FAIL，测试正文 0.73 秒。两个 Cargo 命令和退出码在各自 run.json，正式 Cargo --locked，CARGO_INCREMENTAL=0，RUST_TEST_THREADS=1，Node24 置于 PATH；每组使用不同合成 LINGXI_HOME/TMPDIR，复用现有仓库外 native-write-clean target，未新增 target。

`arrival-negative-control.json` 及两个独立源码/编译与运行日志：从当前被验收文件直接提取 helper 与同一新增回归；对照为当前完整 buffered helper，1 PASS/exit0；负控仅将 await_arrival 恢复为旧“读到不匹配就丢弃”的算法（保留相同接口），同一回归稳定在等待 beta1 时超时，1 FAIL/exit101。此负控按预期失败是证明测试能识别旧缺陷，未将它当成普通测试 PASS。独立 rustc 复用真实 Tokio 已有依赖，未 mock 被测通知 helper；完整原八业务场景另由上述 Cargo 当前 Main 真跑。

`preservation.json`：原八业务场景自首个原 test 到文件末尾，在反向替换恰好八处工厂名后，与修改前逐字相同，原 gates、并行安排、业务断言完全保留；原十个 CRUD 测试尾部逐字相同。两文件 rustfmt --check 和 git diff --check 均 exit0，只格式化这两个文件。原正式 600 秒保护未改动、未延长；本次未重跑原正式入口。

## 边界与风险

本次未修改生产迁移、迁移登记、验收规则或任何其他仓库文件。其他代理的并行修改未触碰；这两测试的局部通过不代表全仓库、组合生产修改、Windows 或 GUI 已验收。未执行全 599 预检或正式流程，根代理负责汇总当前组合后续门禁。此处 pending 属测试通知收件箱，不是生产 steering 收件箱；已有业务边界与容量检查未放宽。固定 tip=14 将来新增迁移时仍会主动失败，不能自动跟随被测 registry 自证。

交付包含两文件净差异 `two-tests.patch`、修改前原文、当前源 hash、实际命令/日志、负控/对照完整源码及其 diff。详见 RESULT.json。
