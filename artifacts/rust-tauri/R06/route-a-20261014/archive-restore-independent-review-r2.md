# 31CDA283A11A R2 独立复核：PASS（限定本叶服务实现）

此前P1已在同一真实两会话并发/两文件移动中断窗口独立复验修复。运行中不会全量恢复另一活跃会话，最终移动前的原始prepared凭据校验也通过实际三项改写负控。本叶归档恢复的当前服务实现可按此限定证据关闭；不是全部78项、R06_ACCEPTED或R07_READY。

## 候选与精确绑定

复用独立树 `/tmp/lingxi-archive-independent-review`，HEAD `d73729a2821a51e39e27038c0c42052a1f2f737e`，原R1净补丁之后干净应用 `/tmp/lingxi-archive-jsonl-restore-r2.patch`，SHA256 `913822dd74763074309199fa05cc17f09edcb03826eb3f8697b509bbb0659e52`。R1 SHA256 `bba1cd9ccaae944c6309fde77f9069a9d8d93ef2f25f6281e39120fb40b43f1b`。

R2生产只改 `legacy_session_files.rs`，SHA256 `0bc36c6d3bdd41170cfac03aa0e227d4c4045122e3cd8372f1b09219d9ffe508`，与作者交付坐标一致；测试原14的38852字节前缀SHA256 `12189bbb4418b4e7847dae830831f08ab58ed024e51ef2d7447c10bb32bc749b` 独立确认逐字未改。所有候选源码/实验/日志哈希在 `/tmp/r06-archive-independent-review-r2.json`。

保留R1真实反例与原失败断言：`/tmp/r06-archive-independent-review-tests.rs`、`/tmp/archive-review-independent-concurrent.log`。修后另建审查者实验 `/tmp/r06-archive-independent-review-r2-tests.rs`，不覆盖原反例，不把“没观察到旧坏状态”当PASS。未改main、作者树、原17断言、登记、门禁、.sync-audit或.vscode，未提交推送。

## 修后真正同窗口独立实验

审查者独立变体 `independent_r2_concurrent_sessions_preserve_real_journal_and_restart_pairs`（独立树新测试文件1016起）：真二进制创建两个真实会话independent-a/b，真归档，真实JSONL和各16MiB未知字段sidecar；两个真HTTP restore并发。独立SQLite与文件观察者必须看到两个transition日志均已持久化、被中断会话DB仍archived、JSONL已移入active且原源不存在、sidecar仍archived且active sidecar不存在，才SIGKILL本实验启动的服务。

观察窗口接受prepared/completed，随后强断言必须prepared、receipt为空，因此绝非用“未出现损坏completed”计通过。真实重启后，两会话原JSONL与sidecar逐字节一致地回到archived、active两文件均不存在、DB均archived、两会话prepared计数均0。模型/存储/恢复/认证/文件移动没有mock。

实际日志 `/tmp/archive-review-r2-independent-sigkill.log`：

```text
INDEPENDENT_CONCURRENT_JOURNAL id=independent-b state=prepared phase=archived receipt= responses=(0,connectionClosed)/(0,connectionClosed)
INDEPENDENT_RESTART_BACK=true source_jsonl=true active_jsonl=false source_sidecar=true active_sidecar=false
1 passed; 0 failed
```

额外双会话完整字节与DB断言实际执行通过。R1在同窗口是completed/recovered:true、重启留下分裂文件对；R2保持prepared并恢复两会话，前次P1可以关闭。

## 活跃隔离与原prepared权威

亲读R2调用链：`legacy_session_files.rs:576–581`prepare不再调用recover_conn，同会话未收尾prepared明确409。`694–711`finish只遍历自身plans、核原日志、调用recover_plan(...,false)，不能按before生命周期撤销其他活跃批次。`905`全量recover_conn只有recover_pending `930`调用；整个服务的公共恢复调用点只有bootstrap `lib.rs:1148`，接受HTTP/Run前执行。

最终BEGIN IMMEDIATE内 `637`先verify_prepared，再verify_row/verify_plan及DB业务，然后才触文件；`851`逐项核operation_id/session_id/owner_kind=session_owner/owner_subject/operation_kind/state=prepared/file_effects/完整snapshot_json/原input_digest。失败收尾 `685`也核原日志，不把外部改写静默抹成completed；finish `710`再次要求原日志。

实际受影响套件中新负控 `actual_final_writer_refuses_completed_bad_digest_and_replaced_snapshot_journals` 已独立执行：真HTTP prepared落盘后，外部SQLite BEGIN IMMEDIATE和队列超时确认最终写者等待；分别提前completed、坏digest、替换snapshot且重算正确digest。三项均500明确prepared journal missing/changed；两文件没有移动、DB仍archived、改坏的日志不被失败收尾抹去。只还原测试自己外部改写的记录后，实际重启清零并再次restore200。正确digest不能掩盖snapshot替换。

原HTTP owner/agent/path的ExpectedSource仍到prepare和最终row校验，文件摘要/身份与不覆盖rename、其他真实DB空头保护、locator与DB生命周期提交、错误保全均沿R1原合同。没有放松路径、来源或冲突检查。

## 实际验证范围

- 审查者自有同窗口两会话真实SIGKILL/重启：1/1，日志 `/tmp/archive-review-r2-independent-sigkill.log`。
- R2受影响恢复套件：17/17，0 ignored，日志 `/tmp/archive-review-r2-affected-suite.log`。包含原14原样场景、新两会话并发各自200、新同窗口prepared崩溃恢复、新完整journal三项负控；单会话崩溃、实际第二件rename/DBlocator写失败、late owner/source/权限、已提交cleanup权限500/重启恢复等再次通过。
- 旧69场景在R1独立树已由审查者实跑69/69，日志 `/tmp/archive-review-service-regression.log` 与 `/tmp/archive-review-adapter-regression-final.log`。本轮按照主控磁盘限制及“无新理由不重复全大回归”要求，不把作者86PASS当成自己的当前86，也未重复与新变更无关的旧大套件。当前R2直接证据是17+自有1。
- git diff --check通过；本轮未独立重跑Clippy/fmt，全R06门禁未执行。

全部命令使用 `/Users/study_superior/.cargo/bin/cargo --locked`，`CARGO_INCREMENTAL=0`，原仓库外独占target复用且未复制；LINGXI_HOME `/tmp/lingxi-archive-review-env/home`、TMPDIR `/tmp/lingxi-archive-review-env/tmp`、LINGXI_PRODUCT_DIR为独立树lib。磁盘观察有10GiB余量，没有清理源码/原证据或其他人的cache。

复现审查者同窗口实验：

```sh
env CARGO_INCREMENTAL=0 LINGXI_HOME=/tmp/lingxi-archive-review-env/home TMPDIR=/tmp/lingxi-archive-review-env/tmp LINGXI_PRODUCT_DIR=/tmp/lingxi-archive-independent-review/lib CARGO_TARGET_DIR=/Users/study_superior/.cache/lingxi-r06-builds/archive-review-target /Users/study_superior/.cargo/bin/cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r06_archive_jsonl_independent_r2 independent_r2 -- --nocapture
```

受影响17套件同环境，把测试参数换为 `--test r06_archive_jsonl_restore -- --nocapture`。

## 限制与可关闭范围

可绑定本叶“真实归档JSONL+sidecar经路径/冲突校验移回active并更新生命周期、返回真实路径/错误”和“失败状态与实际副作用一致”，包括前次P1跨会话日志保护与真重启恢复；仅针对已存在且对应真实DB行的旧文件，不执行R08正式存量迁移或切换。

实际macOS SIGKILL不等于断电；Windows/Linux、磁盘满、正式打包、全R06阶段门禁仍未验证。主控应将准确源码哈希绑定到最终整合候选；不签R06_ACCEPTED/R07_READY，也不因本叶通过改其他未完成叶。
