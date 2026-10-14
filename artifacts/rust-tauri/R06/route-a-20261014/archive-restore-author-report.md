# 31CDA283A11A归档恢复：独立新边界交付

本补丁补齐真实JSONL+sidecar恢复，独立于EX01等旧补丁。主树只读，无提交/推送，无门禁、原断言、白名单、登记或台账变更；不代表全部78项或阶段验收完成。

## 真实来源与范围

亲读现役TS restore (:2813)、archive (:373)、delete (:515)、header-only修复(:584)、manifest locator(:686)，以及session-file-registry sidecar命名与移动函数：sidecar确为`${sessionPath}.files.json`。亲读Rust session_admin/storage/session_admin、实际HTTP归属闸与sessionfiles/import-legacy；原Rust只有真实DB生命周期恢复，sidecar导入仅导附件行，原DB没有JSONL locator。

独立树 `/tmp/lingxi-archive-restore-codex`，HEAD `2c8f8ddd0b75b4de61ebd047a0246a7f88f6d486`，起步只读复制主控当前全部Rust文件作为一次基线。原始SHA清单 `/tmp/archive-restore-base/source-snapshot.json`；本交付只输出8个指定源/测试文件的净hunks，加报告/证明，不包含主控已有memory/persona/native/EX改动。后来为Clippy仅同步主控已有agent_memory的chunks_exact→as_chunks等价基础修正，明确排除于本补丁。

## 权威与实际行为

组合根绑定真实canonical home；请求不能改变home或直接登记任意路径。sessionPath/path须绝对、在真实home/agents/{agent}/sessions/{archived}边界、合法JSONL名且无目录/文件链接；先读取type=session,id，再查真实DB row和agent，之后仍走现有认证/真实owner闸。sessionId单独调用则只在真实DB agent的可信目录扫描现有header，多个候选明确冲突。HTTP已经授权的owner/agent/已解析path随后带入prepared和最终事务CAS，不把gate之后变化的owner当成新授权。

实际匹配到旧文件时，将JSONL与可选sidecar按原名移回sessions并更新真实生命周期、持久locator，返回restoredPath和sessionId。成功路径在会话操作权释放前捕获，避免后续归档改变响应路径。纯DB会话保持原DB流程，不制造JSONL/sidecar路径；建立locator后若源缺失，不降级为DB-only成功。

目标有真实message、坏JSON、standalone sidecar、目录/链接等拒绝覆盖。旧bug留下的header-only目标先备份，包括旧sidecar，成功才清理；孤立新header id按TS旧bug可修复，但能对应另外真实DB会话的空header拒绝，保护其他实际会话。sidecar原字节、未知字段及已有sf_关联不改写，不擅自重导或登记假文件身份。

archive、archive_children、delete、cleanup、workspace-disposal、sweep共用存储层物理配套：归档移动回archived，恢复后二次归档不留下活跃悬挂文件；删除先同目录隔离再真实DB删除/清扫。后代写集在最终事务核对谱系，变化拒绝整批。旧DB-only/计数/后代skip语义由原回归保留。

## 失败与恢复

复用已有session_operations记录真实locator收据和schema=1 prepared物理计划，不新增表、不改旧迁移指纹。准备计划先持久化，文件摘要和文件身份固定；之后单写者BEGIN IMMEDIATE内校验身份/来源并执行实际DB业务和双文件动作，locator与生命周期同最终COMMIT。不能把文件系统和SQLite说成物理单一原子操作；跨域中断由先行持久journal调和。

Unix逐层O_NOFOLLOW目录句柄访问，macOS RENAME_EXCL/Linux RENAME_NOREPLACE原子不覆盖；不是检查目标后用普通覆盖rename。移动/DB提交失败实际倒序撤回已尝试动作，SQL回滚；已确认撤回则保存失败收据并解除prepared阻断。撤回无法证明身份时保全文件和journal，显式要求恢复。已提交后的物理清扫失败明确返回“commit succeeded; cleanup/recovery pending”，不谎称零副作用；prepared继续阻止执行，重启按真实最终生命周期回退未提交移动或收尾已提交清扫。启动恢复在HTTP/Run准入之前，不自动重做不确定用户操作。

## 新边界真实验证

14个新测试全部位于r06_archive_jsonl_restore：

1. path/header/真实DB归属恢复双文件、原字节和未知字段保持、返回路径；重启后二次archive→restore→archive→delete；DB-only无假路径，已建立locator缺源404。
2. 目标message/standalone sidecar冲突保全；同id及孤立新id header-only旧bug修复；真实其他DB会话header拒绝。
3. 实际设备owner、home/agent/path/header/重复候选/缺源/非regular/文件和祖先链接拒绝。
4. 实际SQLite生命周期写拒绝：任何文件移动前拒绝并保全。
5–8. 必须见到prepared已持久化后取得独立SQLite BEGIN IMMEDIATE，phase仍archived且两源在原位置；生产队列探针超时证明最终写者真等待。此时注入sidecar目标目录碰撞（实际第二份rename失败、第一份真实退回）、源外部编辑、owner变化或真正chmod写权限失败，分别409/500，与实际副作用一致，原文件/用户修改/外部sentinel保留。至多12个新会话捕获窗口；未抓到真实窗口必FAIL，不以未到点PASS替代证据。
9. 真二进制完成JSONL rename、sidecar还未移动、DB对独立读者仍archived、prepared=1时SIGKILL；实际重启退回JSONL、保持sidecar、解除prepared，再次真HTTP恢复成功。使用真实较大sidecar形成自然I/O观察窗口，没有生产测试hook或fake存储。
10. 实际locator INSERT触发器在两份文件都rename后拒绝DB写：两份文件和DB全部退回，后续真实恢复成功。
11. 真实设备先GET确认本来有权，prepared前真实写者被外部锁卡住（journal仍0）后改owner；409、无文件移动、无prepared残留。不会把auth gate后的新owner当授权。
12. 实际准入忙拒绝；两个真HTTP恢复并发只有一个200，另一409，只留下一个真实物理提交/locator。
13. 实际周期cleanup删除旧文件对，不留活跃或归档悬挂文件。
14. 真已提交后清扫期间chmod失败：500明确说明commit succeeded，lifecycle实际active、正确文件对已恢复、旧sidecar备份与prepared保留；重启实际清扫备份、保全新文件并解除阻断。

不替身验收核心，无fake Run/user输入，无远端模型调用；HTTP、认证、实际数据安全闸、SQLite、准入和文件操作均生产实现。测试全部使用合成HOME/TMP与自建服务子进程/文件。

## 结果与复跑

最新新套件14/14：/tmp/archive-final.log。相关旧服务F01 8、history_content12、session_tree14；adapter migration_idempotency2、migration_receipts3、session_file_store12、session_tree18，合计原69项PASS，日志 /tmp/archive-final-regression.log 与 /tmp/archive-adapters.log。总83场景PASS，ignored=0。Clippy service/adapters --lib -D warnings、fmt与diff检查PASS。

把同一目标消息冲突测试和设备gate后owner变化测试切回精确起步基线、强制重编后，旧生产代码均实际200、按原409断言FAIL（两个exit101）。负控日志 /tmp/archive-negative-header_only_old_bug.log、/tmp/archive-negative-device_owner_changed.log；修复源码随后恢复。没有放宽断言或靠重跑蒙绿。

cargo test/clippy/check使用指定绝对cargo、--locked、仓库外target；最终运行HOME/LINGXI_HOME=/tmp/lingxi-archive-env/home，TMPDIR=/tmp/lingxi-archive-env/tmp，target=/tmp/lingxi-ex01-target，CARGO_HOME/RUSTUP_HOME仅复用已安装依赖/工具链。原始运行目录不提交。

未实跑Windows/Linux、断电/磁盘满、正式打包或全R06门禁；Windows实现不覆盖MoveFileExW及逐层链接检查，未声称目录句柄保障与Unix相同。不执行R08存量迁移/正式切换，不翻R06_ACCEPTED/R07_READY；主控须整合并独立复核。
