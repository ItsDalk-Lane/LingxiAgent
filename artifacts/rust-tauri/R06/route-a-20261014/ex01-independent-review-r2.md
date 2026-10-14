# EX-01 第二轮全新独立复验：服务核心限定通过

结论：此前最终保存竞态的材料、工具两条反例已在实际候选独立复验消除，实际PUT设置变更与晚期取消同样拒绝提交并保全原历史。审阅中又发现真实PUT未参与材料栅栏；已保留反例，主控补了真实持久化入口的发布guard后，按同样单行修改独立候选复验通过。可关闭下文列出的EX01服务核心条件，不可将slash/WS/UI完整叶或R06翻绿。

## 候选、边界和证据坐标

独立候选 `/tmp/lingxi-ex01-independent-review`，HEAD `2c8f8ddd0b75b4de61ebd047a0246a7f88f6d486`，依次应用原 `/tmp/lingxi-ex01.patch`、增量 `/tmp/lingxi-ex01-late-commit.patch`。增量SHA256 `a28393fb0b80586b05fd8b8f06d4ef47cdf9b933037f4f67765a846485afaa61`；原补丁SHA256 `422320189356e299d3a6ecb1ce705be3ab14e0efb4d9068f59d6a3806380b63c`。

之后仅按主控明确要求，在独立候选 `agent_config_readback.rs:94` 增加 `let _publication = am::ContextMaterialMutation::for_path(path);`。该文件最终SHA256 `728a35b188bf74727c5646e2c363da7b5e36f304a66e358676876712eb2d5706`，与当前主树逐字一致。其余主控memory详情/拒绝凭据检查重排等总集成变更未应用，不替代主控总候选验收。没有修改主树、作者树、.vscode、.sync-audit、门禁或原断言，未提交/推送。

完整生产源码/测试/日志SHA256及结果在 `/tmp/r06-ex01-independent-review-r2.json`；独立实验稳定副本 `/tmp/r06-ex01-independent-review-r2-tests.rs`。原第一轮报告和失败日志继续保留。

全部cargo使用 `/Users/study_superior/.cargo/bin/cargo`，版本1.98.1，`--locked`；LINGXI_HOME `/tmp/lingxi-ex01-review-env/home`、TMPDIR `/tmp/lingxi-ex01-review-env/tmp`、仓库外独占target `/Users/study_superior/.cache/lingxi-r06-builds/ex01-review-target`。二进制closed-loop的产品素材显式用独立候选lib。HTTP、认证、driver、Gateway、辅助请求、上下文编译、SQLite、历史、取消域均为生产实现；仅模型远端替身。没有mock核心。

## 真正最终保存窗口的四腿独立实验

沿用第一轮审查者自己的反例与原409/原历史保全断言，不引用作者93PASS代替实验。额外新增实际PUT设置腿。每腿都必须观察该会话真实 `auxiliary.summarize` 付费记录已落盘，再由外部SQLite连接取得BEGIN IMMEDIATE，锁内确认manual档案COUNT=0，然后生产数据库队列查询100ms超时，证明写者正在等锁。只有观察到这些条件才注入变化；未捕获窗口必失败。

- 材料：外部真实助手配置姓名修改。409 `compaction_commit_refused`，原messages/head/revision/runs快照逐字不变。
- 工具：实际ToolRegistry.set_availability禁用edit。409，同上。
- 设置：实际HTTP PUT config reserveTokens=17001，并从生产settings_for确认已消费。409，同上。
- 取消：同一生产cancel_manual获得成功；释放外部锁后409 `compaction_commit_refused`，原历史逐字保全。

四腿均在第0个会话抓到真实窗口。每腿另验证manual档案=0、无compaction_succeeded、accepted真实action恰一条、付费记录保留且run/attempt为空、cause_ref等于accepted action。日志 `/tmp/ex01-review-r2-independent-final-confirm.log`。测试位置：材料1064起、工具1109起、晚期取消1155起、实际PUT设置1201起（以稳定副本具体函数名定位）。

提交获胜的独立正控：真实compact成功提交，外部独立SQLite读者必须看到持久manual档案1条，随后生产取消域返回false、实际HTTP cancel409；原compact返回200，已提交历史不变，下一真实Run请求含摘要。测试 `independent_actual_commit_wins_then_cancel_is_refused`。这证明真实落盘已赢后不会再报告取消成功；没有人为暂停CAS获胜至SQL COMMIT完成之间的极窄区间，该区间仅源码核对。

## 审阅新增发现与修复验证：真实PUT必须参加发布栅栏

仅增量候选中 `agent_config_readback.rs:93` 的write_secret_file没有ContextMaterialMutation，真实PUT路径 `save_agent_config_merge:2167 → write_secret_file` 可以穿过材料最终保存栅栏。独立测试真实持有 `context_material_epoch(home).with_stable_commit`，期间发实际HTTP PUT配置：返回200，磁盘reserveTokens已变18001，publication revision仍0。修前两次复现，日志 `/tmp/ex01-review-r2-actual-put-fence.log` 与 `...-confirm.log`。这是实际锁/真实入口的参与检查，不冒称另一个已经发生的SQL档案失效端到端实验；它证明最终读至COMMIT的保护锁无法约束该生产写者。

主控补write_secret_file的发布guard后，独立树逐字加入同一行，再复验同一真实HTTP入口：持最终保存栅栏250ms期间PUT不完成、磁盘字节不变、epoch仍0；释放后PUT才200、磁盘reserve18001、epoch4。现役PUT会显式保存及runtime refresh再保存两次（2541–2547），每次guard进入/退出各+1，所以最终精确4。新增审查者的世代观测最初误按一次写期待2，亲读这两次保存后修正为精确4；没有放宽原产品断言或作者86+7断言。最终日志 `/tmp/ex01-review-r2-independent-final-confirm.log`，9/9。

## 最终裁决与锁序源码核对

- `session_tree.rs:320`仅manual走with_write_txn_guarded，普通append仍用原with_write_txn。新事务 `run_store.rs:1481` 先BEGIN IMMEDIATE及暂存消息/头/引用，再执行guard包裹真实SQL COMMIT；guard拒绝或COMMIT错误均走ROLLBACK。
- final context callback `history_content.rs:1707` 重编译并比较render、materialSha256/materialRevision、真实compaction配置；`1736`持原catalogGeneration的实际目录锁跨过该检查与COMMIT。
- 实际顺序是DB写锁 → toolcatalog锁 → materials保存保留位 →同步双采集/设置读→取消CAS→SQL COMMIT。材料保留位只CAS0进入，已有写者时立即拒绝，不在DB写锁中等活动资料写者。目录回调内不再次访问目录；材料/技能采集走同步文件读取，未发现回调内异步DB等待或目录重入。
- `agent_memory.rs:58`保存保留位不抹掉嵌套/并行写者；`for_home:93`新的发布者等待保留位释放。修复后真实PUT write_secret_file、既有atomic_write/记忆/技能发布参与同一home epoch。两次真实PUT磁盘写均参加。
- `model_history.rs:72`的ManualSaveGuard在材料/目录栅栏内检查取消并CAS0→2，紧接真实COMMIT。取消CAS0→1时提交拒绝；已2时不能取消成功。若COMMIT失败，仍明确失败并ROLLBACK，不因CAS2冒称保存成功。
- Run/manual来源合同未放宽：manual_source要求独立action且禁止sourceRun/attempt；manual_replacement仍核对压前digest/原始保留后缀；Run源 `model_history.rs:529`仍要求真实输入锚，旧工具请求/opaque/never-dispatched journal校验维持。晚期增量只添加提交守护，没有改读取/合法fork来源校验。

纯锁补充：实际嵌套资料写者全部退出才允许保存、新资料发布不能穿过材料栅栏、真实工具目录set_availability不能穿过目录锁，审查者重跑2+1全部通过；这些是锁语义测试，与上面的真实SQL窗口证据分开记。

## 实际运行结果

最终单行PUT修复后的service回归：r06_ex01_manual_compaction10、r06_ar_f01_history8、r06_session_history_content12、r06_t02_closed_loop2、r06_t02_compaction26、r06_t03_session_tree14，合72/72。日志 `/tmp/ex01-review-r2-service-regression-final.log`。

Adapters session_tree_r06t03 18/18、context_material_commit_r06ex01 2/2；kernel final_commit_generation_lock_blocks_actual_catalog_mutation1/1，日志 `/tmp/ex01-review-r2-adapter-fences.log`、`/tmp/ex01-review-r2-kernel-fence.log`。本候选原86+作者新增7共93场景由审查者实际重跑通过，另审查者新增9实验最终全部通过。service/adapters/kernel三库Clippy --lib -D warnings独立通过（/tmp/ex01-review-r2-clippy.log），git diff --check通过；未独立重跑全仓fmt。数字不对应全78义务，也不等于阶段验收。

复现命令统一前缀：

```sh
env LINGXI_HOME=/tmp/lingxi-ex01-review-env/home TMPDIR=/tmp/lingxi-ex01-review-env/tmp CARGO_TARGET_DIR=/Users/study_superior/.cache/lingxi-r06-builds/ex01-review-target /Users/study_superior/.cargo/bin/cargo test --manifest-path rust/Cargo.toml --locked
```

审查者实验后接 `-p lingxi-service --test r06_ex01_independent_review independent_ -- --nocapture`。Service后接 `-p lingxi-service --test r06_ex01_manual_compaction --test r06_ar_f01_history --test r06_session_history_content --test r06_t02_closed_loop --test r06_t02_compaction --test r06_t03_session_tree`，并在env再加 `LINGXI_PRODUCT_DIR=/tmp/lingxi-ex01-independent-review/lib`。Adapters后接 `-p lingxi-adapters --test session_tree_r06t03 --test context_material_commit_r06ex01`；kernel后接 `-p lingxi-kernel --lib final_commit_generation_lock_blocks_actual_catalog_mutation`。

## 可以关闭与继续保留的条件

本候选服务核心可关闭：真实手动/fresh持久提交、下一Run/重启/源删除fork仍消费真实摘要、enabled/keepRecent/reserve实际消费、custom关注线上字节、合法manual来源与严格Run锚、同head不同revision CAS、材料/工具/设置真实并发最终裁决、取消赢家与真实落盘赢家互斥、供应商/权限/实际存储失败原史保全、禁自动预算不足不发送、旧档案兼容边界不被放松，以及此前P2最终保存竞态。D62A81服务份额的成功与拒绝可绑定此限定结果。

继续未完成：slash的先回复/接管目标与fresh接管拒绝，完整WS accepted→succeeded/noop/failed、实验关闭条件，旧UI/客户端接线。不可把0FE8BE、5A6E0D、421EEA等整叶仅凭核心结果翻READY。未验证真实付费供应商、Windows/Linux、断电/磁盘满、正式发布或全R06门禁；不签R06_ACCEPTED/R07_READY。

外部编辑器不参与服务内发布栅栏，按最终同步双采集结果作线性化点；未声称跨进程文件与数据库物理原子性。完整主树的memory/native/persona总集成仍由主控在真实最终候选验证。
