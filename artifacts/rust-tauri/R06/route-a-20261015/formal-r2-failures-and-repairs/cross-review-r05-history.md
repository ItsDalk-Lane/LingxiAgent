# R05 历史替身适配独立交叉复核

结论：STATIC_CROSS_REVIEW_PASS。两测试文件确实只改外部替身如何识别当前提交，并加强实际执行证明，没有修改产品历史、删除原场景或降低资源界限。此次不构建/执行/重跑任何测试，仅只读核对源码、补丁、现有证据；作者最新日志闭环11/11、资源2/2均PASS，属于其动态结果，不冒称本审阅者复跑。

## 当前轮次定位

两个文件的current_turn_messages、last_tool_result、user_text函数正文独立比对完全相同：在完整messages上rposition最后一个user后取借用切片，替身只在该切片读取最新意图和工具结果。不truncate、不写回、不清空产品请求或数据库历史，不新建会话规避同会话第二Run。

C03替身的轮内tool计数也用此范围；C08仅意图归类沿同一个最新user辅助，检查nonce是否跨会话泄漏仍遍历完整body.messages中的所有tool，未用切片掩盖旧历史泄漏。范围只适用于这些固定普通文本请求的外部替身，没有改生产角色/媒体语义。

## 原闭环强要求保留

原11场景名全部保留。独立提取assert/assert_eq/assert_ne宏，原157处变为161处；唯一替换的原断言是C02旧绝对messages[3].content==v1，改成“当前提交内、role=tool且tool_call_id=call_t08_read”的唯一回执.content==同一个v1。这是按真实身份定位，未把本轮新内容逐字相等放松为包含/非空。

每轮四次请求原精确断言、替身固定后才产生并写入随机nonce、同一会话提交两轮、各轮最终正文含本轮nonce、两最终正文不相同、真实重启/历史不丢/零重执行、并发两会话共同callId隔离等原断言均保留。新增当前Run精确三次tool完成；第二轮必须确实仍含第一轮用户和带nonce+EDIT_MARKER的旧tool历史。因此不是删掉真实旧历史来绕开旧替身缺陷。

## 原资源强要求保留

原79处assert宏全部在候选中原样存在，新增后85处；所有const定义逐字相同，包括60取消、60错误、15正常、10长响应、15worker、RSS<400*1024KiB、增长<150*1024KiB、FD<400和增长<=64。

原真实callback 30秒屏障、live时service+child两进程、连接与内存采样、release后仅service/零连接、每样本阈值、日志<=3、closing restart零重执行、真实退出回收、物理清理及UNKNOWN控制均保留。没有串行化两会话并发安排或用静态计数替代物理观察。

新增每个longRun必须实际2main+1tool且0callback；每个workerRun必须2main+1tool+1带parent_tool_call_id的本Run callback，stub请求增量3且callback屏障精确+1。各项从真实SQLite计数并assert之后才写入execution摘要，不能仅靠成功终态/常量JSON假称实际执行。

已读作者完成日志resources-r1.log：2PASS、0FAIL、0过滤，231.55秒。原始资源JSON实际160条cycleResults，恰60取消/60错误/15正常/10long/15worker；10个long和15个worker都有上述execution摘要。此读取不替代重新执行生产，但与加强后的强断言和完整结束日志一致。

## 首次环境失败明确保留

closed-loop-r1.log的c01/c02 failed.provider_error与c03被SIGTERM记录仍存在；commands.json/报告明确漏LINGXI_PRODUCT_DIR导致外置target找不到产品材料，不作为业务负控。本审阅只读首次合成SQLite确认c01/c02均0usage、0model事件，失败在真正模型发送之前，不能拿它证明本次历史bug。

closed-loop-r2只补启动环境Main/lib，同一冻结测试源实际11PASS、0过滤，13.59秒。原正式第二Run四请求变1的业务失败仍在此前formal及独立只读报告中保留，不被这次环境误失取代。

## 冻结范围与限制

net.patch SHA及两文件base/candidate SHA、补丁内存重放、原断言保留、全部资源const一致、同一helper边界、原资源序列与日志SHA记录在confirmation.json。base都与14453cd05的HEAD逐字一致，当前candidate及四个生产前置SHA均匹配作者冻结清单。补丁只有这两测试文件，未动生产或门禁。

本次未运行Cargo/npm/服务/测试；未评定全599正式窗口、跨平台或真实付费供应商。后续应由Root在全批最终源上统一做质量检查与正式验收。本交叉复核没有发现需要撤回这些最小测试适配的阻断问题。
