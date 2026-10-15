# R05 两个旧模型替身的完整历史适配

仅修改 `r05_t08_closed_loop.rs` 和 `r05_t08_resources.rs`。替身保留收到的完整真实历史，只用最后一条用户提交及其后的工具结果决定当前轮下一步。C03 的轮内工具计数和 C08 的当前意图识别沿同一边界；用于检查越界泄漏的全历史扫描仍完整保留。

C02 仍在同一会话实际提交两轮、每轮四次模型请求，随机内容只在替身固定后写入工作区。旧的固定 `messages[3]` 改为当前轮精确 `role=tool`、`tool_call_id=call_t08_read` 的唯一回执，保持新内容逐字相等、各轮最终正文含本轮随机值且两正文不同的原断言，并增加第二轮真实旧用户/旧工具历史仍存在、每 Run 三次真实工具完成检查。

F27 的 60 取消、60 错误、15 正常、10 长响应、15 真实 worker，所有原采样器正负控制、真实 callback 屏障、RSS/FD/TCP/进程树/文件/日志/重启/清理检查与界限均未改变。新增每个长响应 Run 两次主请求/一次工具；每个 worker Run 两次主请求/一次有归属回调/一次工具及精确 callback 屏障次数检查，证据写入既有原始序列的该 Run 条目。

原正式失败依据保留于两份只读报告目录：`/private/tmp/r06-r05-closed-loop-review`、`/tmp/r06-r05-worker-tools-review`。这里没有通过清空历史、换新会话、减少数量、过滤父测试或修改生产代码求绿。

## 验证

- 原 `r05_t08_closed_loop` 全 11 项：PASS，0 过滤，13.59 秒；日志 `closed-loop-r2.log`。
- 原 `r05_t08_resources` 全 2 项：PASS，0 过滤，231.55 秒；日志 `resources-r1.log`，原始完整序列 `f27-resource-series.json`。60 取消全部结算且连接释放，60 错误、15 正常、10 长响应、15 worker 全量完成；每长响应 Run 的 1 工具/2 主请求，以及每 worker Run 的 1 工具/2 主请求/1 有归属回调均通过。最终重启、零重执行及进程/文件清理断言全通过。
- 本次仅对两自有测试执行 rustfmt；两文件 `git diff --check` 通过。

完整命令/环境见 `commands.json`，精确 base/candidate 和生产前置源 SHA 见 `source-sha.json`，仅两文件净补丁见 `net.patch`，原测试名/界限保留记录见 `preservation.json`。

首次自有运行 `closed-loop-r1.log` 漏设 `LINGXI_PRODUCT_DIR`：仓库外 target 无法按可执行文件祖先找到产品 `lib/yuan`，前置上下文编译失败（无模型请求），故终止自有测试及其服务。失败日志和合成目录完整保留；第二次只修正启动环境为 Main/lib，没有为此改源码/断言。该环境失误不冒充原业务负控。

## 范围和限制

F27 使用 persona 同期生产声明修复（真实 write_stdin 与已成功注册的受信配置 worker 纳入现有声明机制），其生产源 SHA 单列；本包没有修改这部分。实际验证为本机 macOS、合成供应商端点和真实服务/工具/worker 进程，不代表 Windows/Linux、真实付费供应商或整轮 R06 通过。未提交、推送、改变门禁/阶段输入/文档或其他测试。没有添加任何共创者署名。
