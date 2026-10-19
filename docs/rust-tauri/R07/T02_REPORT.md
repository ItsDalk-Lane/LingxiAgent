# R07-T02：本地实现与验证记录

状态：**LOCAL_VERIFIED：T02 本地实现、独立复核修复和适用门禁已通过**。真实平台账号、其他系统及正式安装包不在本次通过范围内；非 Unix 手动本地文件发送明确不支持。这不是 R07 整阶段完成报告，T03—T12 尚未完成。进度与功能账本由主代理另行更新。

## 结果与验证边界

五个平台已接入受控 SDK worker 和正式 Rust 入口。身份、会话、真实 Run、附件登记、命令、回发和送达回执使用持久记录；重复消息不会重新运行，已经开始但无法确认的外发保留 Unknown。主人清除、配置代次变化和权限收窄均可撤销正在运行的授权。普通访客不加载私人记忆、偏好或工具权限。`/rc` 只授权一个真实桌面会话，`/exitrc` 撤销该范围；二者使用命令回执，不制造 Run。

本机测试运行正式服务、认证、数据库、运行链、SDK worker、平台适配器和页面逻辑。模型与平台服务器响应使用确定性替身。`r07_bridge_sdk_platforms` 只替换外部飞书 SDK、WebSocket 和 HTTP 响应，实际运行生产 SDK worker 与平台工厂；`r07_bridge_native_flow` 使用实际 Telegram 适配链验证输入提示、草稿、最终回执和文档正文。早期 `r07_bridge_flow` 的外部 worker 协议替身只证明 Rust 宿主链，不能单独替代生产 SDK 链或真实账号。

没有连接真实平台账号、付费调用、真实外发、提交、推送或发布。其他操作系统、正式安装包、真实网络限流与供应商故障恢复尚未验证。任务书明确要求每个平台经授权完成真实账号的一收一回和附件验证；本报告不把本机替身通过写成该项完成。手动本地文件发送目前使用 Unix 固定目录句柄读取；非 Unix 明确拒绝，Windows 对等能力仍未验证。真实账号、其他系统和正式包的后续验证边界保留到 R10 相应工作，不以本地通过替代。

独立复核确认的短暂网络错误被当成致命错误、子委派撤权及父任务完成后漏回收问题已修复。最终冻结候选上服务 523 项、适配器 40 项、协议 36 单元与 1 验证器检查全部通过；生产 SDK 五平台陌生主体强行申请 exec/write 仍零派发，父任务完成后暂停子任务也实际取消并清理。页面与 SDK 14 组 233 项、三套类型检查、全工作区 Clippy 禁止警告、格式、归属/依赖检查及两个 SDK 打包入口检查通过。重复消息、重启、排序和 stop 后 enabled=false 均有最终执行证据。

复核者已撤回“生产飞书独立文字等待 Unknown 阻断答案”的 P2 判断：飞书普通流式使用 `edit_message`，其能力构造器默认 `receiptMode=fold_into_stream`，等待消息和内容共用同一条平台消息；原判断遗漏了该默认值，不能算作现役缺陷。折叠预览 Unknown 后禁止另发新最终消息是所需保护。显式 `independent` 的通用分支已兼容且有 SDK 回归，这项增强也不倒推成生产飞书原有漏洞。

## 证据索引

永久证据位于 [artifacts/rust-tauri/R07/T02](/Users/study_superior/Desktop/Code/LingxiAgent/artifacts/rust-tauri/R07/T02)。`SOURCE_MANIFEST_FINAL.json`与`source-snapshot.tar.gz`保存最终源码；`VALIDATION_RUNS.json`保存精确命令、退出码和源码归属；`CAPTURED_LOGS.json`保存原日志/样本及SHA256；`INDEPENDENT_REVIEW.json`保存独立复核摘要、误判纠正和限制。下表basename均在该目录。原始失败、挂起和旧候选保留；同一测试的子集和跨轮次通过不重复累加，不能将失败命令中的局部通过称为整组通过。

| 标识 | 实际范围与结果 | 日志 basename |
| --- | --- | --- |
| E01 | 最终服务 523/523：全 lib 467、自动化权限配套回归15、T02集成41（diagnostic2、flow8、guest context2、native4、notice3、permission2、production SDK11、session routes4、worker5）；退出码0 | `lingxi-r07-t02-service-frozen-r3.log` |
| E02 | V18 最终适配器40/40：migration receipts3、effect store8、message projection1、notice render1、bridge store14、session file store13 | `lingxi-r07-t02-frozen-r3-adapter.log` |
| E03 | V18 新库/V7/V8升级、重启收据及 inspector 核对3/3；是E02子集，旧V17候选23项另行保留 | `lingxi-r07-t02-frozen-r3-adapter.log` |
| E04 | E01内 Telegram 生产 SDK 原生链4/4：富草稿/最终收据分离、等待和富文本开关、stop后enabled=false/取消/清理、文档正文进入真实模型输入、实际响应经前端消费器 | `lingxi-r07-t02-service-frozen-r3.log` 的 `r07_bridge_native_flow` |
| E05 | E01内 production SDK11/11：四平台入站/清洗/回执/重启，飞书折叠Unknown不另发最终，钉钉短断线恢复，以及五平台陌生主体exec/write零派发/公开材料隔离/重启零重做 | `lingxi-r07-t02-service-frozen-r3.log` 的 `r07_bridge_sdk_platforms` |
| E06 | SDK worker与生成契约定向3组72/72；覆盖五平台回执、上下文、附件、效果、诊断、进程退出、预算和递增重放。最终相关TS组见E07 | `lingxi-r07-t02-sdk-connection-contract-r2.log` |
| E07 | 最终页面/SDK/相关回归14组233/233；UI HTTP使用替身，正式服务验证见E01/E04/E05；主代理确认其后相关TS源码未变 | `lingxi-r07-t02-ui-sdk-final.log` |
| E08 | E02内消息投影1/1、通知位置渲染1/1；包括两联系人真实lastActive排序、canonical身份与全部投递状态 | `lingxi-r07-t02-frozen-r3-adapter.log` |
| E09 | 最终协议36单元+1验证器检查、132生成文件无漂移、59样本Rust/TS往返；精确命令回执均exit0并绑定最终源码指纹 | `lingxi-r07-t02-frozen-r3-protocol-tests.log`、`lingxi-r07-t02-frozen-r3-protocol-generated.log`、`lingxi-r07-t02-frozen-r3-protocol-roundtrip.log`；`VALIDATION_RUNS.json` |
| E10 | E01全lib内：host-only身份、凭据别名/空值/遮罩/未知字段、媒体预算、bind前重启，以及模型暂停时停用/清主人/operate→read_only真实取消与零写入/执行 | `lingxi-r07-t02-service-frozen-r3.log` |
| E11 | 既有平台适配器定向回归 3 组 57/57 | `lingxi-r07-t02-effects-adapters-regression-r2.log` |
| E12 | 最终三套TypeScript检查通过；三条tsc及exit0由主代理执行回执确认，其后相关TS源码未变 | `lingxi-r07-t02-typecheck-final.log` |
| E13 | E02内V18会话文件/升级/独立登记核对13/13；保留此前两处旧版本断言失败 | `lingxi-r07-t02-frozen-r3-adapter.log` |
| E14 | E01全lib内子任务撤权4/4：operate正对照到达两个真实执行器；父模型暂停时停用/权限收窄阻止子任务；父Completed后仍暂停的子任务取消清理，父终态不重写 | `lingxi-r07-t02-service-frozen-r3.log` |
| E15 | SDK worker 扩展为 64/64，新增独立文字等待 Unknown 不重发且不阻断最终回复、钉钉临时断线保留正式重连 | `lingxi-r07-t02-sdk-connection-independent-r1.log` |
| E16 | worker/diagnostic两个实际bundle入口smoke均exit0；归属、单一写入者、模块/依赖及负例检查全通过，ownership另在最终冻结候选复跑通过 | `lingxi-r07-t02-sdk-bundle-final.log`、`lingxi-r07-t02-sdk-bundle-smoke-final.log`、`lingxi-r07-t02-ownership-final.log`、`lingxi-r07-t02-frozen-r3-ownership.log` |
| E17 | 最终全工作区all-targets Clippy `-D warnings`与全工作区fmt均exit0，精确命令回执绑定最终候选 | `lingxi-r07-t02-frozen-r3-clippy.log`、`lingxi-r07-t02-frozen-r3-format.log`；`VALIDATION_RUNS.json` |

空stdout日志不能单独推断通过：最终fmt/Clippy以六条精确命令的exit0为依据，类型检查以主代理保存的执行回执为依据。此前仅有输出或未完成的日志仍保留原分类，不替换为最终成功。

## 27 项 T02 责任逐叶映射

来源是 `R07_LEAF_MAP.json` 中 `task_ids` 含 `R07-T02` 的27条补充叶。以下对应已通过的本机证据；平台行均指本机协议链，不认定真实账号验收，其他边界逐项列明。

| 叶 ID | 要求 | 可核对的实际测试证据 | 未覆盖或限制 |
| --- | --- | --- | --- |
| `R00-T02-LA-2441694DE286` | 钉钉接入、会话绑定与清洗回复 | E05 `actual_dingtalk_sdk_factory_roundtrip_and_restart_are_idempotent`；E06“钉钉已有 legacy_app 的规范凭据可直接运行”“发送目标沿回调上下文”；E01 五平台真实 Run | 企业/legacy 凭据真实账号、真实附件未验；未声明钉钉所有媒体类型可用，未开放媒体明确失败 |
| `R00-T02-LA-C32ABFA73E21` | 飞书接入、会话绑定与清洗回复 | E05 `actual_feishu_sdk_factory_roundtrip_and_restart_are_idempotent`；E06 飞书消息资源/线程上下文、下载、CardKit 创建更新关闭；E11 既有适配器回归 | 真实 app/region、账号权限与媒体往返未验；本机正常链不等于所有 region 组合通过 |
| `R00-T02-LA-45432DEEC90A` | QQ 接入、会话绑定与清洗回复 | E05 `actual_qq_sdk_factory_roundtrip_and_restart_are_idempotent`；E06“QQ 频道私信保留 guildId，只向正式私信端点发送”；E01 五平台真实 Run | 真实 app/群/频道账号未验；QQ群文档发送明确不支持；r1 正文夹具断言失败保留 |
| `R00-T02-LA-0909C113F294` | Telegram 接入、会话绑定与清洗回复 | E04 原生链 4 项；E06 Telegram 真实消息/话题与分片附件、平台返回编号；E01 五平台真实 Run | 真实 bot 账号一收一回及附件未验；真实网络、话题/群权限组合未验 |
| `R00-T02-LA-2D2B3DAD17B6` | 微信接入、会话绑定与清洗回复 | E05 `actual_wechat_sdk_factory_roundtrip_and_restart_are_idempotent`；E06 contextToken、uint64 原编号、缺编号 Unknown；E01 未收到回执重启不重发 | 真实登录账号及媒体往返未验；不能将任意私聊推导成主人 |
| `R00-T02-LA-C3A723B0BAB9` | 指定助手最近消息 | E08 `messages_share_canonical_identity_and_preserve_inbound_and_all_delivery_states`；E05 实际 GET `/bridge/messages` 及敏感字段不泄漏 | 已改为持久权威投影；未见大量历史下分页/保留窗口的专门压力证据 |
| `R00-T02-LA-2424DA9AE3A5` | 指定助手会话及最近活跃排序 | E08最终projection核两联系人实际lastActive排序；E04实际GET sessions的chatId/displayName/isOwner/lastActive经前端消费；E05四平台昵称投影；E07刷新失败保留列表 | 大量联系人压力场景和其他系统未验；不以单联系人测试替代排序证据 |
| `R00-T02-LA-F465ECC87EE3` | 详情可见正文与媒体计数 | E01 routes `history_uses_canonical_final_and_reset_obeys_live_lease_then_starts_new_context` 核最终正文、空文媒体 mediaCount=1、无令牌；E07 详情读取原生历史 | 专门图片多张计数组合、各平台所有富媒体形态未逐项覆盖 |
| `R00-T02-LA-5CB57741F1BB` | 状态真实且读取不启动连接 | E01 五平台 status configured 与秘密遮罩；E05 实际 status；E07 状态失败保留旧值、200 error 不变空状态 | 尚无“仅多次 GET status，启动次数始终零”的独立计数断言；实际 error/initializing 每种投影未全部枚举 |
| `R00-T02-LA-21BCAF26AB04` | 凭据归一、持久且响应不泄密 | E01 配置保存启用重启和 status 不含合成 token/secret/context；E05 公开 status/sessions/messages 不含私密字段；E10 五项别名/空值清除/遮罩/未知字段检查；E07 credentials/snapshot 相关组 | 各平台所有旧别名→重启的 HTTP 组合未逐项列证；真实秘密未使用 |
| `R00-T02-LA-29FD2CE83BB4` | enabled=true 启动或刷新连接 | E01 `all_five_platforms_use_real_runs_and_stable_reply_receipts` 各平台实际 POST 配置后收到消息；E04/E05 生产 worker 启用链 | 每个平台“运行中编辑配置再刷新”的生产 SDK 组合未全部独立覆盖 |
| `R00-T02-LA-C16CB9FD3738` | enabled=false 停止连接 | E02 配置代次撤销旧 claim；E04 `disabling_connector_cancels_actual_run_and_cleans_the_original_native_draft`；E06 五平台停用回收连接 | E04 的 HTTP 停用走 `/stop`；直接 config enabled=false 与运行中各平台组合尚未全部端到端列证 |
| `R00-T02-LA-34B7475C9BAD` | 设置主人并反映到状态 | E05 四平台 actual status.owner 与 inbox Owner；E04 Telegram isOwner；E02 五平台 host-derived role；E07 setOwner 状态消费 | 真实账号 owner ID 未验；不能依据消息自报 owner/role 授权 |
| `R00-T02-LA-809F2DF0CB86` | 清主人后旧身份失效 | E01 `clearing_owner_changes_new_messages_to_guest_without_replaying_old_owner_task` 核旧私人历史不进 Guest；E02 `authority_role_changes_isolate_context_and_unproven_legacy_context_is_never_reused`；E10 撤权取消 | 五平台生产 SDK 逐平台运行中清主人组合未全部端到端跑；共享正式存储/运行检查已覆盖 |
| `R00-T02-LA-312590978841` | 真实文件发送并返回 fileId | E01 routes 手动发送真实文件/HTTP 重试同事实、20MiB 以上安全读取、QQ群文档拒绝；flow `manual_file_delivery_uses_actual_managed_bytes_and_retry_keeps_one_send`；E02 manual action 与 Started 再核权限；E07 文件菜单稳定 requestId | 非 Unix 读取明确不可用；真实平台附件投递未验；无 requestId 的旧请求不宣称跨独立请求幂等 |
| `R00-T02-LA-88749DE980FF` | reset 清引用、保留联系人元数据并换上下文 | E02 `conversation_reset_is_atomic_keeps_metadata_and_pins_replayed_inbox_to_original_epoch`；E01 routes 忙时409、重置后epoch2/详情空、不造额外 Run；E07 重置失败保留历史 | 真实账号 reset 后附件往返未验；保留原历史和旧 inbox 事实，不重写既有任务 |
| `R00-T02-LA-F4C513BAD52E` | 当前及 legacy 权限映射 | E10 `legacy_and_current_modes_share_one_strict_result`、`permission_narrowed_while_model_parked_cancels_before_write_or_exec_dispatch`；E01 permission2；E05五平台生产SDK陌生者exec/write真实网关零派发；E07三选项与加载状态 | 模型不能自报身份/role提权；真实供应商账号未验，五平台安全矩阵见A04 |
| `R00-T02-LA-09D567E99524` | receiptEnabled 保存实际值 | E04 设置 true 有真实 typing，false 无 typing；E07 `saves the bridge receipt toggle through global bridge settings`；E02 V18 independent typing Unknown 保守规则；E15 通用显式独立文字等待 Unknown 不阻断最终回复；E05 飞书折叠 Unknown 禁新最终气泡通过 | 飞书生产 edit_message 默认折叠，先前“生产独立等待缺陷”判断已撤回；真实供应商效果未验 |
| `R00-T02-LA-D5514BBDBF88` | richStreamingEnabled 保存实际值 | E04 true 调用富草稿/富最终，false 只普通草稿/最终；E07 rich toggle；E10 actual scope/支持模式选择；E06 飞书 CardKit 与原生最终 Unknown 不兜底重复 | 真实供应商富文本效果未验；不支持的平台/群范围明确不启用，不以替身通过宣称平台都有原生流式 |
| `R00-T02-LA-678DA7A676FF` | stop 停连接且 enabled=false | E04 最新 r3 实际 POST `/bridge/stop` 后 GET status.enabled=false、真实 Run Cancelled、原草稿 cleanup、零最终发送；E01 immediate stop 有界退出；E06 连接回收 | 五平台 HTTP stop 全组合未逐平台列证 |
| `R00-T02-LA-2F5ED5999431` | 单次凭据验证不启动轮询 | E06 五平台“验证调用真实供应商入口，不启动接收循环”，核无 socket/getupdates/gateway、错误去敏；E01 diagnostic 关闭竞态；E15/E05 钉钉临时断线 SDK 与完整链恢复 | 平台响应被替换，真实 token 失效/限流/供应商错误码仍待账号验证 |
| `R00-T02-LA-ECA3EAB38C12` | 微信二维码服务 | E06“二维码和确认状态复用现役登录函数”核二维码编号/PNG；E07 微信保存→主人→启用及连接切换丢弃旧凭据 | 真实二维码登录未执行；完整生产 HTTP 诊断输出与 UI 的二维码像素未作真人扫描验证 |
| `R00-T02-LA-57B2329825A0` | qrcodeId 实际状态 | E06 scanned/confirmed 与实际返回字段；E01 `normal_qrcode_long_poll_can_exceed_twenty_seconds_and_is_reaped`；E07 绑定失败不启用与旧连接隔离 | 实际微信登录服务响应被替换；真实过期/取消/网络断线的全部状态未逐项外部验证 |
| `R00-T02-LA-A9936962073B` | /exitrc 撤待选/接管及横幅 | E01 `actual_owner_can_take_over_only_selected_desktop_session_and_exit_restores_bridge`；E02 durable commands 无假 Run、重启恢复；E09 闭合单会话范围 | 待选未选择即退出与每个平台桌面横幅的 UI 全组合未独立逐项列证 |
| `R00-T02-LA-A220BFDCD4BC` | /rc 同助手桌面候选、选择接管与通知 | E01 同一真实 flow 核选定桌面、权限范围和退出；E02 `explicit_owner_takeover_is_one_inbox_scope_and_never_relaxes_ordinary_bridge_binding`；E09 canonical scope | 多候选最近排序、所有数字选择错误、跨平台横幅未独立全组合覆盖；非 owner/group 不据此获取桌面权限 |
| `R00-T02-LA-7AD1B00D8158` | Bridge 面板逐动作实际投影 | E07 R07BridgeRustUI：联系人失败保留、详情 canonical history、reset 失败保留、文件网络重试同 requestId，Queued/Unknown 不弹成功；E04 实际服务响应经前端消费器 | 尚无完整真人桌面 E2E/其他系统操作；不能把 HTTP 替身 UI 检查称真实平台收发 |
| `R00-T02-LA-DF1F77F2D776` | Bridge 设置逐动作实际投影 | E07最终14组233项内R07BridgeRustUI、credentials/snapshot/permission：原生路由、业务失败、连接/令牌切换、主人、二维码顺序、遮罩、开关；E01/E04/E05正式服务对应入口 | 真人二维码与真实凭据未执行；UI使用HTTP替身的边界保留，不将其单独作为真实账号证据 |

## 24 个基础场景的归属与本轮范围

| 基础场景 | 归属 | 本轮证据与状态 |
| --- | --- | --- |
| A01 重启不重复提醒、A02 夏令时与重叠 | T01 | 使用 T01_REPORT 的已冻结证据，本稿不重复计数，也不重开 R06 |
| A03 消息重投只创建一个任务 | T02 | 最终冻结复跑通过。E01五平台真实Run计数；E04 Telegram生产SDK重启零新模型/零二次发送；E05其余四平台重投后inbox=1、Run=1、发送=1，五平台访客安全组也实际重投并重启；E02原子claim、bind前崩溃恢复与Unknown不重发 |
| A04 陌生主体不能借 Bridge 控制电脑 | T02 | 最终五平台production SDK各自1项安全回归通过：真实可用exec/write已登记，陌生者入站形成真实Bridge Run，模型强行申请工具，实际网关拒绝，两条journal均dispatched=0，执行器计数0/标记文件不存在；只加载公开资料，回复一次且有平台编号，重启重投不重做。E10/E14另核运行中撤权与父Completed子任务清理 |
| A05/A06 | T03 频道与协作 | 本轮未执行；只读准备不算完成 |
| A07/A08 | T04 浏览器宿主 | 本轮未执行 |
| A09/A10 | T05 Office/PDF/插件 | 本轮未执行 |
| A11/A12 | T06 媒体、语音、视觉 | 本轮未执行 |
| A13/A14 | T07 工作台/文件/工作流 | 本轮未执行 |
| A15/A16 | T08 配置与导入导出 | 本轮未执行 |
| A17/A18 | T09 CLI/移动/LAN | 本轮未执行 |
| A19/A20 | T10 观测 | 本轮未执行 |
| A21/A22 | T11 开发与扩展生命周期 | 本轮未执行 |
| A23/A24 | T12 全功能闭环与残留 | 尚未执行整阶段矩阵；本轮不调用旧 Node Agent loop，但这不等于 T12 已完成 |

A04的五个实际测试均在E01最终日志中单独出现：`actual_telegram_sdk_guest_command_cannot_dispatch_registered_exec_or_write`、`actual_feishu_sdk_guest_command_cannot_dispatch_registered_exec_or_write`、`actual_dingtalk_sdk_guest_command_cannot_dispatch_registered_exec_or_write`、`actual_qq_sdk_guest_command_cannot_dispatch_registered_exec_or_write`、`actual_wechat_sdk_guest_command_cannot_dispatch_registered_exec_or_write`。不是从共享网关样例推定平台批量通过，也不是靠未注册工具或模型自觉不调用来避免副作用。

## 迁移与候选指纹

基准提交为 `0c86675d49442fe0e38259bd3097423fb340eecc`，本轮未提交。永久目录内`SOURCE_MANIFEST_FINAL.json`于 `2026-10-15T19:41:24.510372+00:00` 冻结247个变更/新增源码、测试、生成物和登记文件；未变源码绑定上述HEAD，报告和证据不在源码范围内。源码指纹：

`de949dedc106172aab3365fb50fcc7155bd53122afb428f3cb9d32b7a9a418b9`

永久目录内`source-snapshot.tar.gz`的SHA256已实读复算为 `c259e90b52850e08825da755596a0faef67126aac1d5e8f119658d1135b6efaf`，247个归档文件也逐一符合清单。协议/适配器/Clippy/格式的六条冻结门禁回执均包含精确命令、起止时间、exit0和同一源码指纹，执行前后检查源码无漂移。`VALIDATION_RUNS.json`另记录服务523项、最终UI和类型检查命令：服务在该冻结候选执行，UI/类型检查先于仅Rust的lint修正完成，消费的TS输入未变，未虚称当时整工作区与最终候选完全相同。**后续T03编辑工作区不改变这份T02快照，T02结论绑定本节冻结归档与日志，不绑定以后变化的工作区。**

旧a16候选（`lingxi-r07-t02-final-source.json`）与40154前缀候选（`lingxi-r07-t02-frozen-source-r2.json`）均已过时，保留作为失败/修正过程来源，不用于最终认定。a16旧归档SHA为 `8b9e5161dfcb15d32e441992d8bea9b28832f1295f294e6af7acc9dca22f4098`。

下列是实际迁移 SQL 的登记指纹。冻结前的多轮日志仍分别代表当时的候选，不能全部事后绑定到最终源码摘要；只有明确在冻结候选上执行的最终日志才具备该口径。

| 版本 | 名称 | SHA256 | 证据口径 |
| --- | --- | --- | --- |
| V15 | `r07_automation_receipts` | `0d18730f33d5aafb4fb302ed969025d2e71cba0cd5fd3ed63c5d7d597d89ec37` | T01 冻结字节保留，不重写 |
| V16 | `r07_bridge_inbox_outbox` | `e4b910009a7a1c21599b43c918fb53900ba7ee58bda3ae6093ae1a6bccf27869` | E02/E03最终V18整链已核对；早期缺候选表失败原文保留 |
| V17 | `r07_bridge_effect_receipts` | `3acf38748a3921ff99ef809ad95aecd56e57a39dd83b5484af1f91ef6edc0773` | SQL保留已应用字节；`effect-store-r1`的23项仅代表旧V17候选，不替代E02最终40项 |
| V18 | `r07_bridge_independent_receipt` | `ff916d2332e1e35b9fcfad0a8bed46973639b645ed15e58c7e9b3dffeb2e31f4` | E02最终effect8/store14/session13/projection1/render1/migration3均通过，独立登记、inspector及旧收据保留验证通过 |

V18 为旧列默认 false，保留 V17 已存在回执；新增测试实际打开 V17 候选库再升级，核旧收据保留。正式登记在 Rust MIGRATIONS 与 R02 STORAGE_REGISTRY；没有修改共享 Node registry 的无关指纹。

## 失败、挂起与后续通过均保留

独立审阅者`/root/r07_t01_review`在`INDEPENDENT_REVIEW.json`中确认：审阅的本地范围内没有未关闭、具有实际证据的P1/P2；检查了生产SDK安全矩阵和真实平台/原生替身链。审阅结论形成时全服务最终结果尚在等待，随后由主代理独立核实523项通过；不把主代理后验结论倒写成审阅者当时已见。该摘要也保留飞书误判撤回、真实账号/其他系统/后续任务未验的限制。

清单保留所有已发现原始日志；这里只解释可核对的主要失败，不删除或改写失败文件。

| 原始日志 | 当轮实际结果/原因 | 后续证据或当前状态 |
| --- | --- | --- |
| `worker-first`、`storage-first`、`storage-r2`、`service-first`、`service-r2`、`service-r4`、`service-r5` | SHA 格式、Bridge 枚举匹配、代次参数、类型/可见性、错误格式、引用比较、claim 返回字段等编译失败 | E01/E02 后续对应目标通过；不能把首次编译失败计为测试执行 |
| `service-r3` | flow 0/2，未达到 Started/Confirmed，15 秒断言失败 | 宿主会话绑定后续修正，E01 完整 flow8 通过 |
| `service-r6`、`lingxi-r07-t02-hang-readonly.sample.txt` | 一项通过后重启测试挂起，SIGTERM 中止；启动恢复期间 stop=true，订阅者错过初始状态 | r7 3 项 flow 与 E01 flow8 通过，增加立即停止有界退出；原命令不算通过 |
| `storage-expanded-r1` | migration3 通过，store9/10，配置撤销后 pending 数量断言错误 | `storage-expanded-r2` 3+10 通过，E02 store14 通过 |
| `session-routes-r1` | routes0/3，`UnsupportedMedia(text/plain)` 暴露通用文档注册缺口 | 实现受控 Bridge 文档字节登记，E01 routes4 和 E04 文档正文链通过；没有改成图片回避问题 |
| `service-r8` | projection/render 各1通过，flow6/8；缺少 commands 候选表与通知回执超时 | 后续r9为26项通过，最终E01为523项通过；保留r8候选来源限制 |
| `contract-tests-r1` | 历史源码快照被误收集、缺 validator；当前契约另有字段断言失败 | r2明确目标6通过，后续E06定向72及E07最终233通过；失败原文保留 |
| `sdk-effects-r1` | SDK44/47；能力顺序、CardKit 真实参数外形、诊断进程断言失败 | r2为47通过，后续E06定向72及E07最终233通过 |
| `effects-adapters-regression-r1` | 56/57，旧飞书状态断言没有包含缺少实际 messageId 的明确标记 | E11 57/57，仍不允许缺真实编号冒充送达 |
| `sdk-lifecycle-contract-r4` | 67/68，新增派发字段后旧闭合契约负断言需同步 | r5/r6 68/68，字段仍严格闭合 |
| `host-units-r1`、`host-units-r2` | 首轮 ToolSchemaDocument 可见性编译失败；次轮17/19，旧测试存储 mode=auto 不合法 | 后续r3为19/19；最终E10所在lib467项全过，含权限收窄实际零派发回归 |
| `adapter-v18-r1`、`adapter-v18-r2` | r1 effect8、store14通过，但 session file12/13，实际18对旧预期15失败；r2 另一处旧预期14失败；两轮整命令均失败 | E13 修复两处旧版本断言后13/13；未把 r1/r2 改成通过 |
| `sdk-platforms-r1` | 四平台3/4，QQ夹具错读 body.content 占位值而漏了真实 markdown.content | r2 4/4；只修外部响应夹具读取实际用户可见正文且拒绝缺正文，未改生产发送 |
| `child-revoke-r1` | 新增子任务撤权检查首轮未执行测试，宿主 ConnectionStatus 错误参数 Option<String>/Option<&str> 编译失败 | E14 修正后 r2 真实3/3、父 Completed 清理追加后 r3 真实4/4；没有以 E10 之前的通过替代新回归 |
| `fmt-r3`、`fmt-final` | 输出Rust格式差异 | 修正后最终E17格式exit0；原差异日志保留 |
| `service-final` | 首轮全lib为466/467；旧 `group_signal_is_skipped_when_the_kernel_identity_disagrees` 使用dummy PID1234碰到本机已有进程组，系统拒绝信号EPERM | 测试改当前PID与不可能匹配的PGID0确保identityMismatch，生产逻辑未改；旧候选过时，最终E01的lib467及总523全过 |
| `clippy-final`及r2—r6 | 依次发现大枚举字段、测试模块次序、临时分支格式、字符串借用、测试MutexGuard跨await、多余clone/trim等问题 | 以Box收敛内存字段，serde wire/生成契约不变；明确guard作用域、移动测试模块、清理多余操作。最终全workspace/all-targets `-D warnings`通过；不屏蔽警告或放宽门禁 |

早期worker bundle smoke的`exitCode=1/bridge_sdk_worker_failed`仍保留，不单独称完整worker打包运行成功；最终E16两个实际bundle入口均exit0。所有stdout为空、执行中截断、守卫终止的旧日志保持原分类，不从“没有错误文字”推断成功。

## 交付边界

主代理已将白名单原始日志、挂起诊断样本、源码清单/压缩归档及独立复核摘要归入永久证据目录。没有复制测试数据主目录、SQLite数据、合成凭据文件、环境变量、SDK缓存、Cargo target或整个`/private/tmp`。旧失败与新成功分别保留，日志校验值及精确执行回执可防止混用轮次。

本报告由证据整理者只读核对并更新，没有运行Cargo、修改生产代码或改动R07_PROGRESS/FEATURE_PARITY_RESULTS。T02的27叶本机份额可由主代理按逐叶证据记为`PASS_LOCAL_T02`；A03/A04只认定本机矩阵。真实账号、非Unix支持缺口、其他系统与正式包边界仍保留，T03—T12不由本报告认定完成。
