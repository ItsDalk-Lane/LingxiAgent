# EX09 旧缓存真实任务独立复跑

PASS（该用例范围）。直接执行当前既有测试bin的具名真实Run用例，exit0，1 passed/0 ignored，0.81s，新随机合成home，显式TMPDIR、未覆盖HOME。命令/源SHA见 `/tmp/r06-ex09-independent-legacy-live-confirmation.json`。

实际检查先旧SQL→明确stale、embedding计数0→显式rebuild新job→真实FTS上海/全角Quartz召回→下一Run读到原文与真实引用→重启不重复调用；原旧收费向量字节保持，原快照逐字节保持。本审阅独立运行原强断言，未修改用例。

另只读原SQLite：3 completed Runs、9 chat+1 embedding usage；当前rebuild新写4 chunks、resumed0、paidResponsesReused0、1实际请求；保留旧3向量+新4向量；4引用分别覆盖原全角词与88..188含上海失落区间；快照326bytes。原文/引用并未因归一化改写。

日志 `/tmp/r06-ex09-independent-legacy-live.log`；raw `/tmp/r06-ex09-independent-legacy-live-sqlite.json` 与 `/tmp/r06-ex09-independent-legacy-live-citation-facts.json`；合成home：/tmp/r06-ex09-independent-live-runtime/tmp/lingxi-r06t07-knowledge-legacy-cache-real-run-29037-1792024572283518000。

只有外部loopback模型协议替身，非真实付费供应商/其它平台/正式包或全R06完成。未改源码/Main/门禁/登记。修前原失败与两fixture诊断保持不动。
