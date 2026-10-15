# 聊天后台六叶冻结交付

结论：本次六叶当前 macOS 范围 PASS，可供 Root 独立审阅/整合/正式绑定；不表示 R06 总体验收，不计 81B217A7E581 完成。

- 净补丁 `chat-backend-six.patch` SHA256 `36028a1b91260cc815833f0c02208b6fc2be66b527aefae24b20ba8eb9d9b436`。
- 基线为已提交 `c095f606c`，44 个文件的逐文件 base/candidate SHA 见 `source-sha.json`，冻结源码在 `candidate/`。Main 未修改、未登记、未提交。
- lib/auth/ws 仅聊天必要增量，Root 后台 WIP 的 `subagent_stop_request` 枚举与 handler 需按 hunk 保留。前端 Rust 允许表已保留 `subagent_stop_result`。其余逐 SHA 可合入。Main 已存在的 Office、skills/goal/translate 路由、normalizedFolders 校验、ReviewPanel 与 skills hook 映射均保留，未撤回。
- 临时 focused-create 与资源授权恢复不混入主补丁：`focused-runtime-extension.patch` 仅后续会话包所需两 fresh-choice API、runs 资源根恢复字段/初始化/builder/实际 row 刷新、lib builder。其对应 ResourceAccess 新方法/阻塞 cwd 修复归会话作者下一批。

## 原条款与实际证据

| 原叶 | 原要求 | 实际证据 |
|---|---|---|
| D483E4CD0935 | 经授权进入 Run，流式阶段、工具和最终回复；空内容零提交；非法图片数量/大小/格式类型化拒绝 | 真 WS prompt 经现役 admission/Run/SQLite/ModelGateway；真实 describe→mcp_call read→模型下一轮实际消费文件正文；供应商尚未结束时已收到 LIVE-FIRST。空文本/11 图/SVG/空图/非法 base64 零 Run/消息/调用。受权 fileId 图真实句柄读取，15MiB+1 拒绝，9MiB 真字节入模型。 |
| 7CBF6F4A6760 | accepted；排队提交停止后 UI 立即闲；旧 streamId 不停新流；无任务 already_stopped | 真实文件准备阻塞与取消：停止即 idle、释放读租约后也零迟到 Run。真实窗口另会话占模型唯一槽，当前提交等待配额、停止后当前 UI 立即闲，邻会话仍活跃；释放槽后被取消提交零供应商调用。真旧 ID 返回 stale_stream，原新流仍活跃；空会话 already_stopped。 |
| 81664285F927 | streamId/sinceSeq 补发与 reset/truncated；无流空回执；越权零旧帧 | 真断连期间供应商完成，重连连续补齐精确缺失 seq 与最终尾部；错 ID reset；无缓存无活动会话空 events/runtime false。真实 4200 delta 触发有界缓存截断，尾部 seq 连续/最终闲/持久正文 4200 字符完整。外主体五种控制全部拒绝且零旧帧。 |
| C1FB9C92E813 | 活跃 steer→steered/下一轮；闲时 prompt；删除或越权不得混流 | 真工具回合中 steer 进入实际下一次模型请求；已闲 steer 生成真实用户/回复。真实删除 doomed 后明确 agent_unavailable 拒绝、无新增消息/调用且邻 session 活跃 ID 不变；外主体拒绝。 |
| 92620BF2264F | 授权/媒体校验先于插队打断；真实输入收据和新回复；非法媒体/身份零空消息 | 两真实并行 session，仅目标中断/新收据/新回复，邻 Run 保留。活跃目标在 11 图/不支持 MIME/空图/错误 sessionPath 的 interject 全部 not_accepted、旧 Run 不取消、SQL 与调用不变。跨会话 fileId 图拒绝。 |
| 421EEA1AC8D5 | accepted 先于 succeeded/noop/failed；未知 method/off/无会话/流式拒绝且历史不动 | 默认模型摘要三实际终局，失败调用费用事实保留；全部入口矩阵逐一真 WS。instant_simple 开实验后真实本地 checkpoint，同一支 CAS、原 user/assistant 行保留、零模型/费用新增，重启下一模型确实消费 checkpoint。实验二次读取防提交前撤权。 |

`原要求`完整文字单独冻结于 `original-assertions.json`，未改原断言/Then/门禁。

## 真实运行

最后主补丁候选已真正安装在独立私树：Rust 生产源以 c095 基线重放净补丁，没有借用带 focused 临时模块的旧测试通过。

- `frozen-core-real-2.log`：当前源码真服务 14/14 PASS，11.24s，含真实 Vite + Electron + 当前 ChatPage/InputArea/Tiptap/HTTP/WS 的 27 个具名检查。主控环境仅合成 LINGXI_HOME/TMPDIR；未覆盖 HOME。
- `frozen-core-strict.log`：当前净候选 service lib + 新父 strict clippy `-D warnings` PASS，14.44s，无 warning。
- `frozen-core-types.log`：root TypeScript PASS。
- `frozen-core-js.log`：原 coordinator 33 + 新统一历史投影 6，共39 PASS；无原断言修改。
- `frozen-core-new-rustfmt.log`：新 chat_wire/chat_settings/新父 rustfmt check PASS；不把继承基线全部文件的格式状态虚报为全仓 PASS。
- 相同业务实现的前序受影响回归：`../adapter-model-regression.log` 100 PASS（含新增精确 pair/未配置 pair/跨配置代际与 Longcat）；`../adapter-original-goldens.log` 原 golden/compat 4 PASS，golden 字节未改；`../service-original-regression.log` 原持久5 + 手动压缩10 + 历史内容12 + Slash8 + canonical history7，共42 PASS。净候选剥离未来 focused API 后主14已重跑；这些42/104回归不冒充净基线再跑。

复现主命令：

```sh
LINGXI_HOME=/private/tmp/r06-chat-wire-runtime/home TMPDIR=/private/tmp/r06-chat-wire-runtime/tmp LINGXI_PRODUCT_DIR=/private/tmp/lingxi-r06-chat-wire/lib CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=/private/tmp/r06-ex09-target /Users/study_superior/.cargo/bin/cargo test --locked --manifest-path /private/tmp/lingxi-r06-chat-wire/rust/Cargo.toml -p lingxi-service --test r06_chat_wire -- --test-threads=1
```

实际窗口收据：`/private/tmp/r06-chat-wire-runtime/home/lingxi-r06-ex01-chat-native-60205-1792033597277124000/chat-ui-result.json`（27项）；同目录 `chat-ui.png` 已由本代理实际读取，能看到真实最终文本、输入区和模型保存拒绝提示。截图是生产组件组合窗口，非完整产品启动/Tauri/其它平台。

## 失败与修复未抹去

`../real-wire-3.log` 暴露显式 model pair 丢失 declared capability，修为同一 snapshot 的精确 configured pair 继承；未知/退休 pair 继续无能力。`../native-chat-3.log` 暴露浏览器读不到 ETag，修既有受权 CORS 的 Expose-Headers。后续真实窗口暴露 run_end 缺真实输入 ID、队列 interject 租约门误挡，均保留失败后修生产。`../real-wire-12.log` 大图引用读取超限/ETag时序反例保留；改真实句柄限量读取及按实际稳定水位断言304。`../real-wire-19.log` 的排队停止失败属于测试把所有会话 empty 误当当前会话 idle：失败收据已证明当前按钮恢复 send，而独立 holder 仍运行；已改为生产 sessionScopedListIncludes 判断当前会话，额外断言 holder 未释放、候选零调用。首净包组装过滤遗漏 chat_rx select hunk 被独立实装验证捕获，`frozen-core-real.log` 保留未通过；修净包生成器后当前14真重跑通过。

## 限制与后续

81B 整叶仍未完成：须补完整 AppPages 的侧边/附件/预览装配、区域故障、WS 拒绝/断线的完整 UI 面及其余空态。当前27窗口证明模型 switch/set/思考实际下一 Run、同值零请求、待建默认、失败保留、队列 interject、真正排队停止、mobile/未连零 GET、token 旧连接零提交；不把27当28子要求全部等价。当前夹具会显式报告尚未迁移的 workspace/experiments 辅助请求，不冒充全聊天可用。

大图沿现役 TS 的 20MiB base64 字符上限（原字节15MiB），没有提高 WS 1MiB 帧门禁。Snapshot 通用可信读口最多20MiB；原 read/写入/恢复仍8MiB。新历史存 base64，旧 bytes 格式仍严格可读。统一 UI reader 大响应有32MiB硬拒绝，超大多图历史/全量历史须后续按真实分页/资源引用补足，未在本次6叶宣称解决。

模型动态端到端使用真实 loopback OpenAI-completions 替身；其它协议历史媒体新增分支静态与原 golden 回归，未宣称所有商业供应商/平台动态验证。无正式打包/发布、完整 Electron 启动、Tauri/Windows/Linux 动态验证，也无 R06_ACCEPTED/R07_READY 结论。
