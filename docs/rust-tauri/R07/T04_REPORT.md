# R07-T04：受控浏览器实施与验证记录

状态：**r7 自动门禁与窗口可见性 LOCAL_VERIFIED；A07 已在解锁后的 T05 接入期依赖快照补验通过。** 本报告保留全部 18 叶和 A07/A08，结论只绑定 r7 归档。源码已解冻供 T05 继续实现，后续工作区变化不能沿用本候选的通过结果。R07 总进度与功能账本由主代理另行维护。

## 已得到的结果

浏览器沿 ADR002 已选的独立受控 Chromium 与 CDP pipe 实施。正式服务绑定会话、所有者、授权代次与执行节点；模型工具仍经过真实网关及审核，不向网页开放桌面原生能力。浏览器控制面板对应同一实际工作区，Rust 分支不回落旧 Node browser-host 或另一个本机网页宿主。PDF 后续复用这一适配层，PDF 完整验收属于 T05，本轮未计为完成。

实际检查覆盖导航、后退/前进/重载、新标签、同页快照、真实 PNG、下载登记与确认、冷恢复、用户接管时的操作阻止、Cookie 策略、设置失败与回滚 Unknown、关闭/撤权/删除、启动失败和清理未确认。冷恢复只登记网址，明确恢复才打开；已确认急停关闭的旧网址不在重启后复活。新的人工急停推进授权代次，旧插话与旧审批不能解除后来的急停；稳定请求重放只读取原收据。

截图、下载和网页全文均进入正式会话文件链。没有工作区时仍可将自己产生的文件物化，再用正式 read 读取；只授予真实主体、会话及精确版本的单个文件，不开放整个缓存目录。图片读取沿现有真实模型凭据、视觉调用与用量记录。跨会话文件请求会正常归档，再由真实资源执行器拒绝；不再因为模型参数中包含外会话 fileId 而让任务卡在运行中。

搜索保留四个付费供应商、免费入口、三个浏览器入口和 auto 的现役选择规则。付费凭据只有同供应商实际验证成功才原子保存，精确掩码不串供应商；免费/浏览器/auto 的纯配置检查明确标注未验证外网。正文读取沿 NetworkPlane、公共 DNS/IP 校验、固定目标、有界正文与重定向限制。**配置代理时若无法证明公共目标 IP 固定，web_fetch 明确拒绝，不能声称已支持任意代理隧道，也不静默直连；固定供应商 API 仍沿真实网络平面。** 自定义 CA 未能可信安装到 Chromium 时同样明确拒绝。

桌面入口只使用当前连接私有句柄与明确 native 会话，不接受 renderer 指定任意服务地址。初次连接、重连和游标过期重新读取真实状态；连接或助手切换后，迟到结果不能操作旧窗口。跨会话卡片同时核对连接和节点。Cookie 策略变更关闭当前受控页面，保留网址及既有登录资料，明确重新打开才采用新策略；可能丢失未提交表单，五种语言均提示。

## 绑定的候选与复核范围

- 候选：[SOURCE_MANIFEST_r7.json](../../../artifacts/rust-tauri/R07/T04/SOURCE_MANIFEST_r7.json)，372 份冻结文件。
- 源码指纹：`612f1bdd45a4af24ec84e9956c3cd233fdec21d3883ac41bc7d26db1d5a6fc55`。
- 完整源码：[source-r7.tar.gz](../../../artifacts/rust-tauri/R07/T04/source-r7.tar.gz)，SHA256 `63b2aa50c18c16302386ecc62611de9c5e69e6ba0ee91524305eb409ca11dbbb`。
- [自动计划](../../../artifacts/rust-tauri/R07/T04/r7_GATE_PLAN.json)、[Node 计划](../../../artifacts/rust-tauri/R07/T04/r7_NODE_GATE_PLAN.json) 与下列 19 份实际回执逐参数核对；exit、前后指纹、sourceUnchanged、日志摘要均一致。回执含真实命令、开始/结束时间、环境与退出结果，不以计划文件代替运行。
- [独立审阅 r7](../../../artifacts/rust-tauri/R07/T04/INDEPENDENT_REVIEW_r7.md) 已重算 372 份源码及全部归档条目，并核对全部 19 份原始日志；独立源码审阅范围为其他实现者编写的 Chromium 启动/清理修复。审阅者自己编写的服务装配与 fixtures 只核运行回执，不当作独立代码审阅覆盖。主代理另作交叉复核。
- [本机环境](../../../artifacts/rust-tauri/R07/T04/LOCAL_ENVIRONMENT.json)：macOS arm64 隔离目录、真实服务/认证/数据库/文件/网关/执行链，外部平台和供应商响应使用本地替身；原生浏览器组使用真实本机 Chromium。没有真实账号、付费请求、真实消息外发、提交、推送或发布。

## 18 叶逐项结果

以下 LOCAL_VERIFIED 限于本机真实链及对应自动验证；不能扩写为其他系统、真实远端机器或人工接管已通过。flows、adapter、UI 等名称对应下一节同一 r7 的实际命令。A07 仍是独立必需项。

| 叶子 | 实际行为 | 本机状态与证据入口 |
| --- | --- | --- |
| `R00-T02-LA-A67D342381C0` | 指定会话标签集合、活动标签与服务快照同步 | LOCAL_VERIFIED：flows / adapter / UI；Chromium target/工作区；真实 HTTP AppEvent 与初始快照 UI 回归 |
| `R00-T02-LA-A09B78985A14` | 急停关闭并撤销授权，下一条真实用户输入才重新授予 | LOCAL_VERIFIED：flows / adapter / 故障组；持久命令/代次、工具与 HTTP 撤销回归；Unknown 不重复派发 |
| `R00-T02-LA-8A45B19A18B4` | 活动会话导航更新真实网址和可用状态 | LOCAL_VERIFIED：adapter / UI；Chromium Back/Forward/Reload/新标签；不能只断言发出了 CDP 命令 |
| `R00-T02-LA-13049521EA0C` | 关闭控制面板与浏览器会话回收分别处理 | LOCAL_VERIFIED：flows / adapter / UI / visibility；Electron IPC 与窗口生命周期；归档/永久删除 HTTP 回归 |
| `R00-T02-LA-6E13E5DF05EB` | 指定会话标签、允许 URL 新开标签及空态 | LOCAL_VERIFIED：flows / UI / browser-contract；Rust 链接入口、可信 IPC、Viewer 空工作区测试 |
| `R00-T02-LA-7D212CBD9EEF` | 外部验证成功才保存 provider/key，失败不保存 | LOCAL_VERIFIED：web（flows）/ UI；`/search/verify`、凭据命名空间、供应商 HTTP 替身；配置级检查明确标注 |
| `R00-T02-LA-1809C8E13EA3` | 读取设置和运行状态不修改设置 | LOCAL_VERIFIED：flows / UI；`/preferences/browser` GET、真实磁盘与 Port fixture |
| `R00-T02-LA-03A9E92909C6` | Cookie/站点清理实际确认后成功，失败明确报错 | LOCAL_VERIFIED：flows / adapter / UI；`/preferences/browser/clear-cookies` 与 Chromium 站点数据回归 |
| `R00-T02-LA-ED5FD5B469B8` | 保存偏好并返回实际新状态 | LOCAL_VERIFIED：flows / adapter / UI；部分 patch 保留未知旧字段；外部部分失败/回滚 Unknown HTTP 回归 |
| `R00-T02-LA-367052089D11` | 返回运行、可恢复及不可用状态 | LOCAL_VERIFIED：flows / adapter / UI；`/browser/session-states`、`/browser/workspaces`，启动不隐式恢复 |
| `R00-T02-LA-593534A4F2AB` | 返回现役浏览器会话目录，保留冷恢复网址 | LOCAL_VERIFIED：flows / adapter；`/browser/sessions` 冷重启 HTTP 断言，读取目录零外部 execute |
| `R00-T02-LA-178C8CBA29C2` | 关闭浏览器并广播真实状态 | LOCAL_VERIFIED：flows / adapter / UI；正式 close-session、browser_status/browser_update 共同消费者 |
| `R00-T02-LA-F1A5597E3E78` | 关闭并撤销该会话授权 | LOCAL_VERIFIED：flows / adapter / 故障组；重放同 requestId 只读原收据；Unknown 零重复外部操作 |
| `R00-T02-LA-DCD85A04A7E9` | 显式恢复选定会话，失败明确空态/错误 | LOCAL_VERIFIED：flows / UI；open-session、可信会话句柄、连接/助手切换迟到丢弃 |
| `R00-T02-LA-A30E4BDF2FA2` | 按会话隔离执行浏览器动作，返回快照、图片、状态和记录 | LOCAL_VERIFIED：flows / adapter / 故障组；A07另列；真实 Run/网关/收据、Chromium 动作、PNG SessionFile 与内容 HTTP |
| `R00-T02-LA-FD7A5A76B8EA` | web_fetch 正文、最终网址或提取摘要可引用 | LOCAL_VERIFIED：web（flows）；正式 NetworkPlane/公共地址边界、真实 Run 与本地 HTTP fixture |
| `R00-T02-LA-D1B56A275C3A` | web_search 标题、链接、摘要及供应商可引用 | LOCAL_VERIFIED：web（flows）；付费/免费/浏览器/auto 选择和实际供应商 HTTP fixture |
| `R00-T02-LA-8BD3BCC6D725` | 设置页面逐动作反映实际状态，失败不伪装空数据 | LOCAL_VERIFIED：UI / browser-contract / types；BrowserTab/Search 配置、跨连接迟到、清理失败和全量状态重连回归 |

## 19 项自动门禁的实际回执

| 检查 | 实际结果（均 exit 0） | 命令、版本与原始证据 |
| --- | --- | --- |
| `browser-flows` | routes 10、browser 13、web 8；31 项通过 | [回执](../../../artifacts/rust-tauri/R07/T04/r7-browser-flows.json) · [原始日志](../../../artifacts/rust-tauri/R07/T04/r7-browser-flows.log) |
| `service` | 32 组、727 项通过；其中 lib 476、gateway 15、MCP/worker 19、协作 9 | [回执](../../../artifacts/rust-tauri/R07/T04/r7-service.json) · [原始日志](../../../artifacts/rust-tauri/R07/T04/r7-service.log) |
| `adapter` | 15 组、241 项通过；其中 lib 133、Chromium 10；9 ignored 另列 | [回执](../../../artifacts/rust-tauri/R07/T04/r7-adapter.json) · [原始日志](../../../artifacts/rust-tauri/R07/T04/r7-adapter.log) |
| `native-startup-faults` | 显式 --ignored：7 通过、0 失败、0 跳过 | [回执](../../../artifacts/rust-tauri/R07/T04/r7-native-startup-faults.json) · [原始日志](../../../artifacts/rust-tauri/R07/T04/r7-native-startup-faults.log) |
| `kernel` | 126 项通过 | [回执](../../../artifacts/rust-tauri/R07/T04/r7-kernel.json) · [原始日志](../../../artifacts/rust-tauri/R07/T04/r7-kernel.log) |
| `protocol` | 36 项及验证器 1 项通过 | [回执](../../../artifacts/rust-tauri/R07/T04/r7-protocol.json) · [原始日志](../../../artifacts/rust-tauri/R07/T04/r7-protocol.log) |
| `generated` | 146 份生成文件一致，无漂移 | [回执](../../../artifacts/rust-tauri/R07/T04/r7-generated.json) · [原始日志](../../../artifacts/rust-tauri/R07/T04/r7-generated.log) |
| `rust-roundtrip` | 66 份跨语言样本逐字节一致 | [回执](../../../artifacts/rust-tauri/R07/T04/r7-rust-roundtrip.json) · [原始日志](../../../artifacts/rust-tauri/R07/T04/r7-rust-roundtrip.log) |
| `clippy` | workspace/all-targets，-D warnings，exit 0 | [回执](../../../artifacts/rust-tauri/R07/T04/r7-clippy.json) · [原始日志](../../../artifacts/rust-tauri/R07/T04/r7-clippy.log) |
| `format` | 全 workspace 格式检查 exit 0 | [回执](../../../artifacts/rust-tauri/R07/T04/r7-format.json) · [原始日志](../../../artifacts/rust-tauri/R07/T04/r7-format.log) |
| `ui` | 26 文件、293 项通过 | [回执](../../../artifacts/rust-tauri/R07/T04/r7-ui.json) · [原始日志](../../../artifacts/rust-tauri/R07/T04/r7-ui.log) |
| `typecheck` | root、node、test 三套类型检查 exit 0 | [回执](../../../artifacts/rust-tauri/R07/T04/r7-typecheck.json) · [原始日志](../../../artifacts/rust-tauri/R07/T04/r7-typecheck.log) |
| `ts-roundtrip` | 66 样本解码、schema 验证、重编码一致 | [回执](../../../artifacts/rust-tauri/R07/T04/r7-ts-roundtrip.json) · [原始日志](../../../artifacts/rust-tauri/R07/T04/r7-ts-roundtrip.log) |
| `browser-contract` | 1 文件、4 项通过；使用项目 Vitest | [回执](../../../artifacts/rust-tauri/R07/T04/r7-browser-contract.json) · [原始日志](../../../artifacts/rust-tauri/R07/T04/r7-browser-contract.log) |
| `contract-types` | 契约类型检查 exit 0 | [回执](../../../artifacts/rust-tauri/R07/T04/r7-contract-types.json) · [原始日志](../../../artifacts/rust-tauri/R07/T04/r7-contract-types.log) |
| `ownership` | 所有权、依赖规则及反例检查 exit 0 | [回执](../../../artifacts/rust-tauri/R07/T04/r7-ownership.json) · [原始日志](../../../artifacts/rust-tauri/R07/T04/r7-ownership.log) |
| `dependency-boundaries` | 2145 份源码依赖边界检查 exit 0 | [回执](../../../artifacts/rust-tauri/R07/T04/r7-dependency-boundaries.json) · [原始日志](../../../artifacts/rust-tauri/R07/T04/r7-dependency-boundaries.log) |
| `main-build` | Electron main 生产 bundle exit 0 | [回执](../../../artifacts/rust-tauri/R07/T04/r7-main-build.json) · [原始日志](../../../artifacts/rust-tauri/R07/T04/r7-main-build.log) |
| `preload-build` | Electron preload 生产 bundle exit 0 | [回执](../../../artifacts/rust-tauri/R07/T04/r7-preload-build.json) · [原始日志](../../../artifacts/rust-tauri/R07/T04/r7-preload-build.log) |

这些是各命令的执行数量，不是去重后的功能数量，也不代表 408 个细分场景已全部验收。adapter lib 的 9 ignored 中，7 项启动故障已通过独立命令实际运行；另两项原生检查见下节，不把 ignored 算通过。

七项启动故障中，1 项证明 CDP 准备失败发生在任何 spawn 之前；6 项使用真实 Chromium/Child。Unknown 由仅测试构建的故障钩子注入，实际 pid 存活及后续 Child.try_wait 回收由真实进程核实，不能夸称已经复现操作系统真正拒绝终止进程。

## A07/A08 与额外原生检查

**A07：ENV_BLOCKED，原始运行仍为 FAILED / exit 101。** [r7-native-handoff 回执](../../../artifacts/rust-tauri/R07/T04/r7-native-handoff.json)与[原始日志](../../../artifacts/rust-tauri/R07/T04/r7-native-handoff.log)记录精确 `--ignored --exact` 检查，实际 0 通过、1 失败、0 跳过。Mac 处于锁屏状态，主代理的 UI 工具不能自动解锁；未输入测试表单，也未创建 continue 文件。等待实际 120 秒后超时，不能因环境原因将原始失败改写为通过。命令传入 180 秒，但源码上限为 120 秒，按实际时间记录。

该次前后指纹不变；已保留[阻塞原因及原始文件摘要](../../../artifacts/rust-tauri/R07/T04/native-handoff-r7-blocked/EVIDENCE_STATUS.json)、[cleanup.json](../../../artifacts/rust-tauri/R07/T04/native-handoff-r7-blocked/cleanup.json) 与仅本测试页面帧。清理 `confirmed=true`，帧不能证明成功人工输入。旧 [native-handoff-r1](../../../artifacts/rust-tauri/R07/T04/native-handoff-r1/MANIFEST.json) 是先前版本证据，不替代 r7。解锁后，必须在实际最终候选重新完成“Agent 填表→同一可见 target 原生手改→交还读取最新内容”，核对输入来源、target/session、录像与清理；A07 仍是最终 R07 必需项。

**窗口可见性：LOCAL_VERIFIED，额外 1 项通过。** [r7-native-visibility 回执](../../../artifacts/rust-tauri/R07/T04/r7-native-visibility.json)与[原始日志](../../../artifacts/rust-tauri/R07/T04/r7-native-visibility.log)为精确 `--ignored --exact` 命令，1 通过、0 失败、0 跳过、exit 0；同 r7 前后指纹与日志摘要匹配。按日志目录归档的 [result.json](../../../artifacts/rust-tauri/R07/T04/native-visibility-r7/result.json)、[cleanup.json](../../../artifacts/rust-tauri/R07/T04/native-visibility-r7/cleanup.json) 和 [MANIFEST.json](../../../artifacts/rust-tauri/R07/T04/native-visibility-r7/MANIFEST.json)核实原 target 在 idle 回收后保留、第六工作区被拒绝、仍有 5 个窗口，最终清理确认。此检查用真实 Browser.setWindowBounds 模拟窗口管理层外部恢复，并刻意不更新宿主缓存；它不等同鼠标点击 Dock 的人工恢复，也不补足 A07。

**A08：本地节点/连接拒绝边界 LOCAL_VERIFIED，真实另一台机器未验证。** UI/IPC、真实 HTTP 权限和 Chromium scope 检查覆盖 executionNode、连接切换、chat-only 拒绝及明确授权路径。固定节点与可信连接句柄使远端错误范围不进入本机执行，但不能用这些本机检查冒充另一台真实服务器或手机的完整验证；相关外部环境继续移交。

## 失败历史与修复交接

原始候选、归档与日志均保留，修复后的结果只记在新候选，不改旧退出结果。候选汇总见 [CANDIDATE_STATUS.json](../../../artifacts/rust-tauri/R07/T04/CANDIDATE_STATUS.json)，开发原始记录见 [DEVELOPMENT_LOGS.json](../../../artifacts/rust-tauri/R07/T04/DEVELOPMENT_LOGS.json)。

| 候选 | 实际失败或未执行 | 后续处理 |
| --- | --- | --- |
| r1 | lib 471 通过、5 失败；routes 10 通过、browser 10/12；web 8 通过。 | 显式晋升目录初始化、正式说明调用、fork 引用分类、WaitingApproval 插话边界修复；不借旧绿色数量代替新结果。 |
| r2 | 已保存源码但未运行门禁，发现新急停与旧插话竞争。 | 标为被替代且未运行，修复新人工急停事务递增代次后进入 r3。 |
| r3 | routes 10、browser 12/13；新时序检查在真实会话许可释放前提交下一任务 Busy；另一次接口测试使用错误 Node runner 失败。 | 等真实 SessionSupervisor/SessionAdmission 释放，不增加 submit 重试；接口使用项目 Vitest。 |
| r4 | browser 31、UI 293 等通过，但旧 gateway 2/13；新增 read 的默认目录影响已有宿主声明。 | 收窄目录装配，保留原权限和防循环断言。 |
| r5 | phone_tools 未同步权限校验新参数，编译失败；Rust 测试未运行。 | 同步真实调用方后另立 r6，不把编译失败记成测试通过。 |
| r6 | Github、MCP/worker、资源工具和协作四个目标未通过；MCP 一项因计数永不到达而无限等，主代理对精确 PID 发送 TERM，原始终止留档。 | 可信注入工具面不再被隐式装配侵入；Github fixture 显式正式目录；资源工具原 HTTP500 断言依赖模型参数归档旧缺陷，改为核真实失败收据、无内容泄露、无外会话文件/引用及原文件不变；不回退已修归档缺陷。 |
| r6 IR-01 | [独立报告](../../../artifacts/rust-tauri/R07/T04/INDEPENDENT_REVIEW_r6.md)发现启动半成功且停止 Unknown 时丢失 Child/Live owner。 | r7 将可失败准备前移、先登记 Live、Unknown 保留 owner/目录；七项真实故障回归通过，IR-01 本机关闭。 |
| r7 人工接管 | 原生输入等待超时，exit101，锁屏 ENV_BLOCKED。 | 原始失败和确认清理保留；解锁后在新实际候选重验，不借旧版录像关闭。 |

设置部分失败、PNG 类型、删除资料、跨连接卡片、首次快照、延迟导航与 Cookie 等早期开发失败亦保留于原始日志。T03 五类 AppEvent 的 Rust HTTP 白名单补齐，记为本轮共享链修复，不改 T03 冻结证据；R06 已有失败记录不重写、不重开整轮。

## 未完成边界与恢复条件

A07 的 r7 失败记录不改写。用户解锁后，主代理在后续实际依赖快照完成原生接管补验，证据如下；最终 R07 仍需绑定同一完整候选重跑全部适用检查。其他操作系统、真实远端机器、正式安装包、真实登录与真实供应商网络均未验证；非 Unix 浏览器端口明确不可用。T05 PDF 和后续任务、完整 R07 最终门禁及独立最终核验不由本报告提前通过。


## 解锁后的 A07 补验（后续候选，保留上方 r7 失败）

用户明确回复已解锁后，新建 `a07-t05-local`，只冻结该原生 adapter 单元目标实际涉及的 adapter/kernel/protocol/test-guard 源码及全部 Cargo manifests/lock，共116文件。依赖指纹 `898c5e0ba785aa0d2b09fcffb2f0e37ad9ba972b5ce2323d13194e040af8ea8d` 前后相同；它是 T05 接入期真实快照，不冒称 r7，也不是完整 R07 最终候选。

[源码清单与归档摘要](../../../artifacts/rust-tauri/R07/T04/SOURCE_MANIFEST_a07-t05-local.json)、[精确命令/退出回执](../../../artifacts/rust-tauri/R07/T04/a07-t05-local-native-handoff.json)、[原始日志](../../../artifacts/rust-tauri/R07/T04/a07-t05-local-native-handoff.log)记录 1通过、0失败、0跳过，exit0。实际 Cua 原生 Chrome 窗口核对唯一测试标题与 URL，在输入栏执行 click/super+a/typeText；AX 读回由 `agent-initial` 改为 `human-latest`。之后真实同一 target Snapshot 与 Release 后再次 Snapshot 均读到最新值，不是用脚本改 DOM。

[原生输入观察](../../../artifacts/rust-tauri/R07/T04/native-handoff-a07-t05-local/native-input-observation.json)、[实际结果](../../../artifacts/rust-tauri/R07/T04/native-handoff-a07-t05-local/result.json)、[清理回执](../../../artifacts/rust-tauri/R07/T04/native-handoff-a07-t05-local/cleanup.json)与[44张真实本target录像帧及GIF摘要](../../../artifacts/rust-tauri/R07/T04/native-handoff-a07-t05-local/MANIFEST.json)互相对应。只录本次创建的测试页面，`cleanup.confirmed=true`。本补验解决锁屏造成的 A07 操作阻碍；不删除 r7 exit101，不借用旧版本录像，也不提前通过完整 R07 回归。
