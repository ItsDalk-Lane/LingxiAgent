# RR3 STORAGE-02 精确空间准备结果

本轮缓存回收及保留核验完成，存在下述一项旧观察器自行写入的已解释例外。现有可用空间 **3.790 GiB**，原 6 GiB 准备目标未达到；不据此批准下一轮原默认完整 clone。总控后来明确以 J 可能采用的共享不可变 Git 对象准备为新前提、无需继续追求 6 GiB；本执行者没有实现或验证 J 的变更，不替其签收。

只删除共享 NEG_TARGET/debug/incremental 内 **15746** 个确证本轮 G 已完成命令产生、且无证据保留引用的中间文件。逻辑长度 5230874480 bytes（4.872 GiB）；按唯一 inode 的 st_blocks 合计 4272488448 bytes。没有删目录、binary、deps、源代码、隔离 copy、原日志、序列、输入、历史原件、用户/RR2 缓存，没有 cargo clean、构建、Git 写或系统修改。

## 归属证明与保留规则

G 实际默认命令 default-command.json 为 01:25:56.935082—02:28:48.697982 UTC、exit 2；summary/copy-path 绑定唯一隔离副本 negcopy.hTXzzN。原默认脚本明示 CARGO_TARGET_DIR 为本 NEG_TARGET；run-supplements.py 同样显式设置此 target 和隔离 cwd。各已完成补证 command.json 的真实 argv/cwd/UTC/exit 与编译/Running 原日志保存在 owner-proof.json。旧 G 外层中断/UNKNOWN 不被伪称正常完成。

每个纳入会话都需要：对象内嵌上述唯一 RR3 copy 的 rust 来源路径、同 crate 的真实编译/运行日志，且每个被删对象 birth 和 mtime 同时落在 G 已完成命令区间。旧 hardlink 复用对象的旧生成时间不符合条件而保留；不是仅凭 mtime 新或文件大认定归属。所有 .o 在删除前再读取 Mach-O filetype=MH_OBJECT，拒绝实际可执行文件；其余只允许 query-cache/dep-graph/work-products 中间数据。逐文件 SHA、size、inode、mtime 均在删除前重新核实。

全文扫描 649 份历史 manifest/digest/sha 文件，按绝对/target-relative 路径、完整 crate/session/file 后缀、明确单独名字、不通用 .o 名字及内容 SHA 交叉保留。不同会话下同名 query-cache.bin 等通用名不是同一原件，不能仅 basename 误当引用；实际路径或摘要一旦命中则完整保留。**780 个已确认本轮但有引用的中间对象保留**。详见 reference-scan.json、refined-selection.json、protected-candidates.json。初轮额外保守筛选也保留 candidates-initial-strict.json，未冒充最终策略。

## 活跃与停止

真实 ps/lsof 在准备、删除前和删除过程中反复复核。lsof +D incremental 始终 exit 1 且 stdout/stderr 空，未发现目标打开文件；主 target 的 H 工作可见但不在本范围。没有中断其他工作。

实际删除 UTC：2026-10-07T03:22:51.360150+00:00—2026-10-07T03:23:02.755498+00:00。总控收紧目标的消息到达时，删除循环已结束，正在保留摘要核验。立即 SIGINT 停止本任务脚本（cleanup.log 的堆栈位于 sha 核验），随后只运行 verify_finish.py 完成原件复核，**没有再删除**。cleanup 原退出 130 如实保留；verify_finish 完整扫完后因下述 1 项差异断言失败、退出 1。该失败不改写为零差异通过。

## 空间与完整性边界

准备末尾实际可用 1456541696 bytes（1.357 GiB），最终 4069494784 bytes（3.790 GiB）；观察总体净增加 2612953088 bytes。准备末尾后压缩了本任务新建的大型摘要回执，同时 H 在同盘编译；删除前/后局部内存数值在中断时尚未落盘。因此该净差只表示真实总体空间变化，**不是缓存独占物理释放量**，也不把逻辑长度或 st_blocks 冒称实际释放。

共检查 **531567** 个保留对象，其中 **531566** 个前后摘要一致，零缺失；1 项旧 G 观察日志自行更新，详述如下。整个 NEG_TARGET 的所有保留缓存/二进制均一致；所有被删路径均不存在。覆盖整个 NEG_TARGET 中未删文件/链接（包括实际 binary、历史缓存）、旧 G 全包、G 中断归档全包、STORAGE-01 全包；不冒称冻结了正在并行变动的主 target 或所有主仓库文件。所有源码与 copy 原路径未动。

前摘要 all-before.json.gz、后摘要 preserved-after.jsonl.gz 可直接解压复核；后摘要逐行含 beforeSHA256/sha256/unchanged。deleted-files.jsonl 逐条记录真实删除时间、原大小/摘要、归属命令和最近活跃复核时间。after.json、final-checks.json 记录最终空间与完整性。压缩只针对本任务新回执；旧原件没有被摘要替代。

## 一项真实差异及后续处理

G-REVIEW-01/supplement-live-binary-observations.json 的前 SHA 为 91492b0a…，核验后为 acf6f561…；不是相等。只读排查发现孤儿观察器 PID 37009、PPID 1 自 09:45:51 一直运行，等待从未生成的 extra-complete.json。该程序捕获所有含 NEG_TARGET 的进程参数，连本次 lsof 都写入旧观察列表，造成此差异；没有 Cargo 使用本缓存。原先“G 已全部静止”的假设不成立，不能继续沿用。

总控后来根据精确 argv 只对该孤儿观察器 SIGTERM，TASK0/orphan-observer-stop-20261007.json 留档，停止后 SHA 7cd810b6… 稳定；自然退出码仍未知。本人不改旧 observations、不写 extra-complete、不重写旧 manifest 为相等。

当前列表的前 430 条事件可逐字节还原前测原文件，重算 SHA 完全等于独立前摘要 91492b0a…、长度 5645767 bytes。已将该精确字节副本压缩保存 observer-pre-snapshot-recovered.json.gz，并清楚标为从未变前缀恢复；不是在旧路径伪造原始状态。新追加记录、原观察器脚本、总控停止回执及摘要见 observer-change.json。这解释且保留了原差异，不将 1 项差异从核验结果移除。

本执行者只新增本 STORAGE-02 回执与必要脚本。到此停止写入；下一轮能否启动需由总控结合 J 的实际新准备方式及当前空间决定，不把未达原目标写成 6 GiB 已满足。
