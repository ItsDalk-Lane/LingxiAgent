# Slash 五叶净增量：作者真实验证通过，待主控独立审阅及绑定

净补丁 slash-completion.patch，14 文件，SHA256 `199a92df2f0548f3771bc47070fa439203853132b1fd217a367a5c61ac8e6ef8`。
显式文件窗口 base `18ddb5a33ef3cf022e6ae7eeb282860efa0428e5f83fe5ddd616cb3c8cc7560f` → candidate `a5799f678b47589e5bd0f156b96ce6db099bf6b2254dba4865cc0d8debf36b39`，详 slash-source-sha256.json；这不是全 R06 正式 sourceDigest。ROOT_BASELINE_HEAD.txt / ROOT_BASELINE_SHA256.json 记录当前 Main 前置：Settings R2、Native R5 与 EX09 适配器。复制必要 Rust 源前置，未复制 artifacts/旧 target；净增量不重复任何前置。

未改 Main、原78叶断言、Stage规则、登记、门禁或白名单；未提交/推送。git apply --check 对保存的明确基线通过。冻结前后14文件 SHA逐条一致。

## 实际实现与入口

`slash.rs::dispatch` 是 HTTP、WS、Bridge 唯一执行器。POST `/lingxi/v1/sessions/{id}/slash` 接 `{text,agentId?}`，返回按实际产生顺序的 `{results:[slash_result...]}`；WS `{type:"slash",sessionId,text,agentId?}` 真实逐条发出进度/终态。HTTP是汇总响应，未声称流式HTTP先行返回。

已有真实 SQLite 会话由 POST `/sessions/{id}/bridge` 的 `{channelKey}` 登记为 Bridge。归属和 Agent 都取服务端会话资料，不信任命令自报来源；登记在私有、有界、拒绝损坏/链接资料的文件保存。POST `/sessions/{bridge}/bridge/attachment` 的 `{desktopSessionId}` 只允许同 owner、同 Agent、活动的桌面会话，一个桌面目标只允许一个 Bridge；DELETE 同路径解除接管。接管只在本次宿主内存，重启清空；Bridge身份跨重启保留。这是真实授权宿主入口，未声称 Telegram/飞书等外部平台已验证或已完成 R07。

- compact/fresh 直接复用原已验手动压缩实现，唯一生产改动是其 crate 可见性。原来源校验、未知历史保全、材料/工具/配置最终 COMMIT 守卫和费用链未绕过。
- stop 按实际目标会话的活跃 Run 调现役 cancellation supervisor。无活跃流明确已停止；dangling/too-late 等拒绝不冒称成功。接管只取消桌面目标。
- reset 经同会话 admission + SQLite 原子事务：复查 owner/lifecycle/无活跃 Run/无待决操作后，真删当前 messages、正文 key_events、anchors、checkpoints/旧操作收据；保持单调事件序号和分支版本，再发真实已清除事件。保留会话身份、真实运行/费用审计、独立 Mood 诊断与外部文件；其他会话原始字节不动。不是仅回移分支头隐藏历史。
- Native 当前输入框菜单的 compact、stop、fresh-compact、reset 经现役发送器发送 typed WS slash；Rust只桥接明确已实现的 slash_result 到原消息处理器/原 InputArea。完整连接改变时旧socket不能代新服务发命令/显示收据。其他正文 wire/普通聊天尚未迁移提示继续保留。
- reset 真实成功收据带可选 `historyCleared:true`；当前连接/会话校验后清页面消息缓存并推进读版本，旧在途读取不能把旧历史重新写回页面。旧消息缺字段和 Node 正常流程保持兼容。

## 五叶原完整条款与真实证据

|原叶|成功及拒绝/隔离证据|
|---|---|
|0FE8BED686E2|真实 Bridge WS /compact 先收到“正在压缩”，供应商仍在等待时 SQLite 历史逐字未变，释放模型才收到 tokens 并实际提交摘要；仅被接管 desktop 历史改变，Bridge自身/旁会话不变；供应商真实400→明确压缩失败、原历史不变且失败费用记录存在。|
|2FAA749DA790|HTTP/WS 共用同一个 production dispatch；未知命令真实 slash_result error；Agent请求不匹配、实际Agent目录被搬走、系统真实无模型/编译链、跨主体实际凭证都拒绝且零额外模型调用/原资料保全。当前Electron真实InputArea经原handler显示未知错误和成功收据。|
|3FCB259FF69C|实际 /reset 后 ModelHistory空，messages/anchors/checkpoints为0，正文仅单调新clear事件；所有真实费用记录逐条相同，旁会话原始消息/分支/Run快照不变，重启后下一真Run请求不含旧用户正文；接管拒绝、busy拒绝、未知404不动其他源；当前输入框实际显示“历史已清除”。|
|4EA88B5073E4|三个真实供应商等待中的活跃 Run：Bridge已接管desktop，/stop仅把desktop落为cancelled，Bridge自身与旁会话仍running并随后正常completed；晚到供应商响应无assistant消息落入已取消目标。空闲明确“已停止（当前无活动回复）”。|
|5A6E0D526903|真实修改当前名字、身份、compiled memory，/fresh-compact 后持久档案的render含三项最新材料；重启后下一实际Run供应商请求含最新memory及摘要，工具声明逐项来自保存快照；真实模型失败明确fresh失败并保留原历史/记录失败费用；接管拒绝且零模型请求。|

接管由真实 HTTP 注册/接入而非测试直接改内存。核心 Service、鉴权、SessionStore、SQLite、模型网关、压缩服务、供应商HTTP传输和现役React组件无业务mock；只有外部模型是合成 loopback HTTP替身。

## 原失败与修后证据

1. `before-negative.log`：原生产 POST slash 真实404，新增强断言先失败；没有伪造404。
2. 连续原生输入真实 stop/compact/unknown/fresh/reset 的 sent/received 表明都发对且 reset 已收到真实成功；旧 InputArea 每条notice各自留下3秒timer，旧timer清掉新reset提示，严格可见提示断言失败。保留 `evidence/old-notice-timer-failure.json/.txt` 和 ui-third/ui-fourth日志。修复仅保留最新notice单个timer并卸载清理，最终实际8check全过；没有放宽原断言。
3. 新fixture最初在空窗口创建前启用CDP导致 target关闭；已改为真实 loadURL后attach，并用当前 Electron 可执行文件realpath。另最初错误地在用户history中找模型摘要，触发保护性失败：原contract应保留用户原始history而模型档案不暴露；现在实际父检查 ModelHistory摘要，子真实GET仍检验用户原始正文和opaque档案不暴露。这是校正观察面，没有改变任何生产历史投影/旧断言。

## 固定候选最终结果

`frozen-combined-final.log`：原 EX01 10 + 原 chat 6 + 新 Slash 8 = 24 PASS（无失败/忽略）；新8含1真实Electron父，子8个严格检查：wire握手、实际Tiptap stop、compact共享派发、先行进度、unknown可见错误、fresh菜单/可见结果、reset菜单/可见收据、改连接旧socket零命令。`evidence/slash-ui-result.json` 保存真实所有WS帧/DOM与HTTP历史；`slash-ui-model-observed` 为父从真实存储读到的模型档案，raw合成home见 evidence-roots.json。

`frontend-extended-regression.json`：105 既有前端检查全部PASS，包括原slash/WS/消息handler/InputArea提示栈；原断言未改。三TS配置通过：types-frozen-root/test/node.log。strictClippy `clippy-frozen-final.log` PASS；fmt-last.log 成功。全部 Rust 仅指定 cargo1.98.1绝对路径、--locked、CARGO_INCREMENTAL=0、单一仓库外target和合成LINGXI_HOME/TMPDIR。

这里只是作者验证，不登记整叶READY，主控仍需独立审阅/同候选整合绑定。macOS Electron+Rust执行通过；Windows/Linux/Tauri原生资源窗口/正式打包/真实供应商/外部Bridge平台未执行。完整原生普通聊天、/rc数字菜单、/new、插件命令等不在本五叶交付中，不借本补丁声称已完成。
