# RR3 F46 全新修复与联合C验收简报

先全文读RR3_BRIEF、RR3_REVIEW_BRIEF、RR1/RR2 MASTER与RR3矩阵/进度/交接，继承所有原负载、阈值、独立验收和放行公式。F46是本轮新增必需缺口，不留给R06。

## 已知触发与证据边界

C-01/measurement-01旧测试cargo2/2 exit0，但原始序列最后重启及进程内owner稳态留下4个service日志，超过原--log-max-files3。该旧exit0不得认定资源义务PASS，失败证据保留。C已在原两个同名测试增加post-restart及owner steady≤3断言，未改生产日志代码。

定向真实红：artifacts/rust-tauri/R05/RR3/C-01/f46-directed-command-01/command.json exit1、f46-directed-red-01/result.json；真实lingxi-service四次ready→SIGTERM/reap，日志数[1,2,3,4]，上限3，隔离home事后清理。C仅定位线索：rust/crates/lingxi-service/src/logging.rs RotatingLogFile::open先prune再open_current，而rotate先open再prune。新修复者必须自行取证，不能把线索当根因已验收。不宣称泄漏，不新增资源管理系统。

## 所有权及最小授权范围

全新F46实施者独占rust/crates/lingxi-service/src/logging.rs及本文件内必要回归测试；根因是启动/重新打开日志文件后的保留数量，沿本轮“新增必需缺口登记修好”授权。C的r05_t08_resources.rs及tests/support/仅在C完成交接后可由本轮F46实施者接管必要修错；如需修改先告知总控登记，不能放宽3、改两个原测试身份或160轮、删未知测量拒绝。A/B和文档文件不得写。新证据RR3/F46-01，保存每命令/退出码/候选输入/工具链/二进制/清单摘要及边界，禁止Git写操作。

## 实施与验收要求

最小修复现有日志启动清理，覆盖新目录、已有恰3份、连续重启、实际正常写入/轮转仍可用，以及错误显式返回不静默降级。真实生产二进制定向复证修前红→修后连续至少4次启动数量≤3且清理；不以纯模拟函数代替真实触发。亲跑C两个完整同名资源测试，原160混合轮、100+取消错误、全部进程树/15存活worker、owner61点、known保留/释放正反测量控、最后重启/稳态断言及普通取消恢复引用保持。未执行/UNKNOWN/BLOCKED如实记录。

完成F46 REPORT后总控派从未参与C/F46的全新联合独立审查者，独立重跑上述核心和完整C负载、假FD/TCP零目标红→还原绿、源码/锁/配置/fixtures/binary绑定，联合关闭C/F27与F46才可进入最终冻结。最终全链仍由另一位全新阶段审查者亲跑。
