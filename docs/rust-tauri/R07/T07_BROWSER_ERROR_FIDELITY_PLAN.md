# T07 浏览器失败原因保真方案（只读准备）

2026-10-18。候选 t07c 保持冻结，本文不代表已修改或已验证修复；原 adapter 失败保留。后续同二进制十次单例通过不能关闭该失败。本文只准备最小诊断改动，不改变执行策略、2 秒刷新预算、并发方式或原断言。

## 已确认的边界

- t07b 与 t07c 的浏览器实现及本组集成测试共 14 项 manifest 记录相同。
- 原共用 `execute` 的 unwrap 没有调用阶段，现有日志不能恢复具体失败命令。`effect_unknown=true` 也不能缩小到 Evaluate：当前 CDP 已有 terminal 时会直接复制该值，即使当前命令尚未写出。
- 独立真实 Chrome 155 两轮中，alert 打开后 `Page.getNavigationHistory` 在 accept 之前确有回复；`Target.getTargets` 同样返回。原 Evaluate 保持等待，2.2 秒后 accept 才返回 done。`result.json` 中约 1.23 / 0.58 毫秒的 `normal_history_seconds` 属于弹窗前对照，不能用作弹窗期间延迟。另从原始 `trace.json` 时间戳独立计算：弹窗中请求 id 11 的 send 0.423458 → receive 0.424714 秒，差 1.256 毫秒；请求 id 16 的 send 2.641203 → receive 2.642403 秒，差 1.200 毫秒。它们均位于各自 `javascriptDialogOpening` 之后、`handleJavaScriptDialog` 之前；这些是 trace 的观测间隔，并非 Chrome 内部耗时。因此不能据此删除后台历史读取或认定本机存在 modal 必阻塞该命令的问题。
- 仅在已确认属于 popup-dialog 的失败专属 profile 内枚举了 175 项，不跟随符号链接；无 Crashpad/crash/pending/completed/minidump/.dmp 条目。没有转储不能排除 Chrome 异常退出，空 History 也不能证明尚未导航。未搜索用户日志或其他 profile。

独立证据：

- `../../../artifacts/rust-tauri/R07/T07/independent-t07c-browser-close-diagnosis.json`
- `../../../artifacts/rust-tauri/R07/T07/browser-close-probe/result.json`（完整 argv 与 trace 哈希）
- `../../../artifacts/rust-tauri/R07/T07/independent-t07c-popup-profile-crash-metadata.json`

## 最小实施范围（待主代理放行）

### 1. `rust/crates/lingxi-adapters/src/browser/cdp.rs`

新增 `close_with(cause)`，普通主动关闭继续由 `close()` 提供明确的主动关闭原因。读取端真实 EOF 保持 `browser_closed`，但消息明确区分 EOF 和主动关闭；不改变错误代码、重试或关闭条件。

`fail` 在同一 state 锁内确定首次 terminal，将同一首次原因发送给所有 pending，再发布 stop / 唤醒。不能只保存首因、却给 pending 发送后来调用者的 cause；也不能先发布 stop 再保存 terminal，留下 closed 但取不到原因的窗口。可返回 canonical cause，供 Live 记录同一事实。reader 收尾和 Drop 的后续普通 close 不得覆盖它。

`call` 的初始 terminal、获取 writer 时 closed、写出中 closed、pending Err 和通道断开路径，均保留首次终止原因；`effect_unknown` 必须按本次命令的 `written && effect` 设置。初始 terminal 命中尚未写出，故为 false。不要继承其他命令的效果不确定状态。已有取消/超时/写失败后立即关闭的保护保留，但以当前原错误 `close_with`；不要为了日志把半帧保护推迟到 Live。

命令上下文必须在 Cdp 自动终止前保留。只在 Live 收到 Err 后加上下文已太晚：其他 pending 和 terminal 当时已经收到不带具体 method 的错误。推荐在 Cdp 终止前给错误补安全的 method / CDP session 标识；仅这些标识，限长并去控制字符，绝不输出 params、expression、text、value、正文、cookie 或密码。之后原错误代码和消息仍可追溯，不以新 `browser_closed` 包装掉原因。

### 2. `rust/crates/lingxi-adapters/src/browser/runtime.rs`

worker 的 events 读取失败、handle_event 失败、refresh 失败统一：保存原 cause 与阶段（events/handle_event/refresh；事件仅记 method/sequence/session），通过 `close_with` 收口，记录 canonical first cause，立即退出整个 worker。当前 handle_event 错误仅 break 内层 for 后还可能跑同轮 refresh，需一并收口，防止次生错误污染。

Live::call 对失败记录 method/session 和错误代码；不记录 params。authority 检查在现有 CDP 调用之前，仍必须保留其原错误语义。利用现有 tracing 或安全错误上下文，不新增诊断依赖；测试不能依赖未安装的 tracing subscriber 才能看到关键原因。若 worker 发现 CDP 已关闭，应取已有 canonical cause 保存，不能再制造一个新的通用关闭原因。

### 3. `rust/crates/lingxi-adapters/tests/r07_browser_chromium.rs`

共用执行核心在错误时保留 action、tab、实际 workspace 状态；popup-dialog 增加显式诊断上下文（root、阶段、Site.requests）。建议仅为该测试引入可选上下文的 helper：导航、触发 dialog、刷新 snapshot、trusted click、轮询 popup 分别有固定阶段名；请求记录通过本测试 Site 的已有有界内存列表读取。不使用跨测试全局注册表，不改变断言、不捕获错误后继续当作通过。

只在失败分支读取与输出现场。原请求参数或页面正文不进入通用产品日志。该测试的 root 和本地请求属于合成 fixture，可以作为失败记录保留；无需访问用户数据。不得为本改动扩大到 UI、刷新策略、串行测试或其他浏览器重构。

## 可证实的验证目标（由主代理唯一 Cargo 时槽执行）

1. 真实 OS pipe 两条正在等待的命令：peer 实际读到两条 NUL 帧后，用 barrier/channel 通知；分别有副作用与只读。调用 `close_with` 指定原因；两个 pending 与后续未写出调用的 code/message 均保持同一首因，effect_unknown 分别为 true / false / false。再以不同原因 close 和 reader 收尾，首因不得改变，pending 最终为空。
2. 真实 pipe 的已写出命令合作取消：peer 读到完整帧后通知，再设置 cancel；原调用为 cancelled 且 unknown=true，连接关闭后的新调用仍为 cancelled、unknown=false。验证不是依赖 sleep 猜测请求是否写出。
3. 扩展现有 stalled-pipe 超时检查：后续调用保持 browser_timeout 首因、unknown=false；不减弱现有退出时间与 pending 清理断言。
4. reader 真实 EOF 与主动 close 各保留自己的关闭来源；不能把 EOF 或协议错误重写成笼统关闭。
5. 原十项真实 Chromium 集成保持并发、原预算与原断言；popup 诊断 helper 的 stage/安全字段可用已有真实失败回执格式检查。通过只能证明新候选执行结果及错误保真，不能反推 t07c 原失败已定位。

没有证据支持调整刷新预算、删除历史读取或串行规避。若新取证仍未复现，原 t07c 失败继续保留为未定位的真实失败，单独报告后续候选的结果与诊断改动。
