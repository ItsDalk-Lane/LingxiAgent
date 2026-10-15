# Slash 五叶独立复核：PASS_CURRENT_MAC_HOST_SCOPE

冻结patch SHA256 `199a92df2f0548f3771bc47070fa439203853132b1fd217a367a5c61ac8e6ef8`，14份Main源与作者冻结清单全部逐SHA匹配。独立源确认 `source-confirmation.json`。我完整读取五叶原10断言与统一执行器、Bridge归属、原子历史清除、WS与当前输入区接线。

## 独立真实验证

- 当前Main现存testbin原8全部由我实际执行PASS（12.02秒），日志 `current-main-original8.log`；不是引用作者通过记录。外部target无新编译。另在冻结作者源8项独立执行也PASS（12.00秒），日志 `frozen-original8.log`。
- 其中真实Electron父实际启动当前生产InputArea/Tiptap、生产WS发送器及结果处理，通过真实Rust网络、供应商HTTP替身和持久SQL，子8个严格检查均通过：握手、真实键入stop、compact统一入口与先行进度、unknown可见错误、fresh/reset菜单及结果、切换连接旧socket零命令。真实帧/页面收据位置见review.json。我未把业务mock或作者截图当自己执行。
- 自行编写额外真实CLI/HTTP/SQL探针 `body-target-probe-r2.py`，日志同名.log，3项额外检查全部PASS，原始证据 `own-boundaries-r2/`。候选binary SHA `f12c29a5c2ad07f37b88c0075a322e459ea58b0c2155824fe4fd9c12256e734b`，开始结束一致：
  1. 三种请求体冒认desktop/source/target字段均真实4xx拒绝，数据库四表全行不变、零模型请求；实际已接管Bridge的reset仍明确拒绝。
  2. 三个真实provider等待Run同时运行；Bridge stop只令接管A cancelled，B和Bridge仍running；真实解除接管再stop只令Bridge cancelled，B继续完成。取消者晚到响应没有新增assistant消息。
  3. 重启前已接管B，重启后Bridge身份保留但attachment清空；reset只删Bridge自身，B消息/Run/事件逐行不变，六条原费用记录逐行保留。

## 原条款逐项结论

|原叶|原要求与实际证据|结论|
|---|---|---|
|0FE8BED686E2|WS先给正在压缩且供应商仍等待、此时源历史不变；完成给token；实际下游失败明确回传且费用存在；接管仅改变desktop|当前host PASS|
|2FAA749DA790|HTTP/WS同一dispatch并经当前InputArea显示slash_result；unknown、Agent不匹配/实际目录缺失、未配置系统、跨主体拒绝|当前host PASS|
|3FCB259FF69C|实际SQL清当前messages/anchors/checkpoints/正文事件；提示清除；重启下一Run无旧正文；接管/未知/busy拒绝、旁会话和费用保留|当前host PASS|
|4EA88B5073E4|实际活跃Run取消及晚到响应拒绝；idle明确已停止；自有接管解除边界证明不误停其他会话|当前host PASS|
|5A6E0D526903|真实名字/身份/记忆修改后fresh档案和重启下一请求读最新材料及摘要/工具；接管拒绝；真实下游失败原历史不变、费用记录|当前host PASS|

未发现本候选范围内新的可证实缺陷。首个自有CLI夹具在epoch stamp之前放agents，真实安全闸正确拒绝启动；原探针/日志保留。改用全新空home启动并通过真实API创建agent后运行检查，没有删除拒绝现场或绕过安全闸。

范围：本机macOS Rust服务、真实HTTP/WS/SQLite和当前生产输入区；供应商仅真实loopback HTTP替身，不声称实际商业付款、完整应用启动、其他平台/Tauri资源窗口/外部Bridge平台或全部聊天已经完成。无源修改/新target/登记/门禁/提交推送。全部显式独立LINGXI_HOME/TMPDIR与当前Main/lib产品资料，未覆盖HOME。不是R06整体验收或阶段READY。
