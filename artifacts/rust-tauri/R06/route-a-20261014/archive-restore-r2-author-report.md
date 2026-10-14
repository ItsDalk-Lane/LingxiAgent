# 31CDA283A11A 归档恢复 R2：活跃批次日志隔离

## 结论与净增量

R1真实P1已修复，仍交主控同候选独立复验后裁决。本补丁只相对于已交付R1 `/tmp/lingxi-archive-jsonl-restore.patch`（SHA256 `bba1cd9ccaae944c6309fde77f9069a9d8d93ef2f25f6281e39120fb40b43f1b`），生产仅变更legacy_session_files.rs，测试仅在原测试末尾追加3项，原14完整文本字节相同。没有修改主树、登记、门禁、白名单、台账，没有提交推送。验证仍在仓库外原候选树；依赖主控已确认的agent_memory as_chunks等价修正，其不在此增量。

## 根因与实际修复

R1 prepare和finish都全量recover_conn，能把另一会话仍活跃、DB仍before的prepared当遗留日志完成。不同会话准入锁不能保护彼此，后续JSONL先移动、sidecar未移动时中断就失去日志保护。不是SQLite事务不能回滚的问题，而是先前另一次操作已经删除恢复资格。

R2只在bootstrap接受HTTP/Run前全量恢复。运行prepare不恢复任何其他日志；同会话存在prepared就明确409拒绝重入，保留原批收尾或重启恢复。finish仅核验并收尾本批plan，要求真实DB为after；运行时从不根据before猜遗留和撤销其他活跃操作。最终BEGIN IMMEDIATE事务触文件之前，核验operation_id/session_id/owner_kind/owner_subject/operation_kind/state=prepared/file_effects/完整snapshot_json/原input_digest全部一致；失去恢复凭据时不执行物理动作。失败收尾也先核验自己的原日志，外部损坏/改写不会被静默标completed。恢复公共函数只有组合根bootstrap一个调用点。

## 原独立反例修前负控

完整 `/tmp/r06-archive-independent-review-tests.rs` 原样复制为临时集成测试；仅回置生产legacy_session_files.rs到R1精确字节，其他R1文件不变。运行原函数independent_concurrent_sessions_keep_prepared_journal_until_actual_commit，首次再次真实捕获并FAIL(exit101)：

```
INDEPENDENT_CONCURRENT_JOURNAL id=independent-b state=completed phase=archived receipt={"lifecycle":"archived","recovered":true,...}
INDEPENDENT_RESTART_BACK=false source_jsonl=false active_jsonl=true source_sidecar=true active_sidecar=false
```

日志 `/tmp/archive-r2-original-negative.log`。原反例源码函数逐字保存在本目录archive-restore-r2-independent-original.rs.txt。原函数在没有捕获损坏completed时必panic“不能签PASS”，因此它是修前负控而非修后正例；没有把“没捕获故障”当修后通过。

## 修后真正并发与中断证据

新增independent_concurrent_actual_sigkill_retains_prepared_and_recovers沿用原真实二进制/两个真实HTTP恢复/独立SQLite与文件观察/真实SIGKILL/同资料根重启。捕获要求两个真实transition日志已经存在、JSONL已active且归档源消失、sidecar仍archived、active sidecar不存在、DB仍archived；观察窗口接受prepared或completed，从而不会以不出现旧损坏为PASS。达点才SIGKILL自己启动的合成进程。随后必须真实重启退回被中断JSONL，并核验state=prepared且未提前产生receipt；两个会话原JSONL与16MiB sidecar全部原字节保全，active文件不存在，prepared清零。

最终同候选日志 `/tmp/archive-r2-service-final.log`：

```
INDEPENDENT_CONCURRENT_JOURNAL id=independent-a state=prepared phase=archived receipt= responses=(0,connectionClosed)/(0,connectionClosed)
INDEPENDENT_RESTART_BACK=true source_jsonl=true active_jsonl=false source_sidecar=true active_sidecar=false
```

新增two_different_sessions_concurrently_commit_their_own_pairs_and_journals：真实两个不同会话同时恢复均200，各自双文件原字节、实际DB active和各自completed日志，prepared0。

新增actual_final_writer_refuses_completed_bad_digest_and_replaced_snapshot_journals：真实prepared落盘后外部BEGIN IMMEDIATE取得写锁，实际队列超时证明最终writer确在等待；分别把日志提前completed、坏digest、替换snapshot且重算正确digest。三腿均500且明确prepared journal missing/changed；无文件动作、DB仍archived，损坏prepared不被失败收尾抹去。只修复测试自己注入的日志变更后真实重启，恢复清零并真实再次restore200。没有生产hook、假日志或假Run。

## 验证结果

- 最终恢复套件17/17 PASS，原14字节不变、原强断言保留；包括单会话SIGKILL、第二文件真实rename拒绝回退、实际数据库locator写失败回退、late owner/source/EACCES与提交后真实cleanup EACCES500/重启收尾。
- 旧service F01 8、history_content12、session_tree14，共34/34 PASS；与恢复套件同命令日志 `/tmp/archive-r2-service-final.log`。
- Adapter migration_idempotency2、migration_receipts3、session_file_store12、session_tree18，共35/35 PASS；日志 `/tmp/archive-r2-adapters-final.log`。
- 总86/86 PASS，ignored0。service/adapters Clippy --lib -D warnings PASS `/tmp/archive-r2-clippy.log`；cargo fmt --check PASS。
- 运行原指定cargo绝对路径，cargo1.98.1、--locked；HOME/LINGXI_HOME=/tmp/lingxi-archive-env/home，TMPDIR=/tmp/lingxi-archive-env/tmp，CARGO_TARGET_DIR=/tmp/lingxi-ex01-target，实际产品素材为独立树lib。原始合成数据/运行日志不提交。
- 一次回归命令误写不存在的test target r06_t02_history_content，未执行测试，随后改为真实r06_session_history_content全套PASS；不计产品失败或通过。

## 限制

本次为macOS真实进程中断、SQLite等待与实际I/O故障验证；Windows/Linux、真实断电/磁盘满、正式打包和全R06门禁未运行。未签R06_ACCEPTED/R07_READY，未扩展R08正式JSONL迁移。损坏日志保留明确阻断，运行时不自动猜测和清理，需要修复可信资料或重启按日志处理；未知内容仍显式失败保全。
