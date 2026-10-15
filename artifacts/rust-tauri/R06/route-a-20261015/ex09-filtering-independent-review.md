# EX09 独立复核：PASS

核对不可变4文件补丁 `759edf7fa64bbcf08c5d39f92654d18fadb84300dc451dffd64f3a7a442cb9da`，当前Main `4cdbd0f99ab878e3c46c17e47e79c841f007cef1` 四文件与冻结候选逐字节相同。未参与实现，未修改Main、登记、门禁或原断言。

独立原二进制重跑25+12+6=43项全部通过。原22完整前缀和Root12文件保留不变，作者原真实失败日志仍在。源码确认章节/来源先过滤再topN，向量结果读取实际当前chunk，导航与citation受同一冻结来源范围约束。null与省略分别清除/保留配置。

另起真实CLI与模型HTTP替身执行10个实际Run，真实HTTP、原文引用/导航回读、两种空范围、三种越界及schema拒绝、省略/null和FTS/hybrid分支通过。原始供应商请求与SQL费用逐条核对：chat30、embedding4、rerank0，共34条成功费用记录一一对应。4次embedding为2次资料入库+2次授权有效hybrid检索；空范围/错误范围零额外embedding。

rerank未接供应商时明确返回KNOWLEDGE_RERANK_NOT_CONNECTED并保持RRF，未冒充执行rerank。未声称真实供应商、其它平台或Tauri知识界面已验证。额外探针初期配置/解析失败均保留，修正夹具后才记录PASS。

收据：`/private/tmp/r06-ex09-cross-independent/cli-1792026934076591000/result.json`；原始provider请求在同目录；独立检查日志位于`/private/tmp/r06-ex09-cross-independent`。完整结构报告：`/private/tmp/r06-ex09-cross-independent-review.json`。
