# B9 创建通知的生产渲染器补强

聊天作者独立指出原24只证真实WS发出，生产websocket.ts Rust白名单仍丢弃session_created。真实Electron+生产connectWebSocket/handleServerMessage复现 received=true、consumed=false，留在 session-created-dispatch-original-negative-r3/session-created-dispatch-result.json。不是凭静态判断关闭。

生产仅白名单新增一个session_created token；后端原身份检查、完整连接比较和生产locator/list upsert均复用。补丁 session-created-dispatch-incremental.patch SHA256 6eedf08e9d43fc668c93d69ab0f68aab7c521931f49f7d1a117f26045722de07，7文件精确base/candidate见session-created-dispatch-source-sha.json。不要重复应用原24；其余6文件均真实父/fixture新增或追加，未夹带Map改动。

同活动当前CARGO_BIN_EXE新父 current_rust_session_created_reaches_production_renderer_projection 实际1/1 PASS，日志 session-created-dispatch-current-parent.log，raw /private/tmp/lingxi-r06-agent-delete-env/tmp/lingxi-r06-session-created-65881-1792036808517139000。真实网络收到、生产分发更新列表和native locator、窗口可见新SID四项强断言，0模型请求/0usage。原websocket与ws-message-handler两文件88/88通过，日志session-created-dispatch-old-ui.log；test TS通过。

初始r1/r2为夹具将Network.enable放在初始about:blank页面提交前而卡住，已调整观察时序后原r3才是真实产品FAIL；前两份保留仅诊断，不计产品反例。新增父复用唯一活动CLI（含下一Map私有配套），没有新target/拷产物；Root实际Main绑定应按自己的精确候选重编。仍不冒称Tauri/其他平台窗口或完整侧栏创建控件均已验。Main/登记/门禁/原断言未改，未提交推送。
