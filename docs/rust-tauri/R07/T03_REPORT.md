# R07-T03：频道、DM 与会话协作实现记录

状态：**LOCAL_VERIFIED：T03 本地实现、独立复核修复和适用门禁已通过**。本报告记录22项本轮直接或实质共享责任的证据与边界，不修改进度或功能账本，不代表 R07 整阶段完成。其他系统、真实供应商网络、正式包和完整联合故障注入的限制仍保留。

## 结果与范围

频道、Agent 间 DM、phone 后台处理和会话协作已接入正式 Rust 服务。频道文件、书签、双侧 DM 历史保留现役格式和未知字段；后台消息沿真实来源、父任务和原因运行。重复请求、重启、成员退出、关闭频道和权限收窄均使用持久事实核验。模型提出的跨会话发送或创建只产生确认卡；确认后目标忙碌会持久排队，只有真实运行被受理才显示首条消息已接收。

phone 使用正式后台运行链，只有显式 `channel_reply` 才写公开对话；`channel_pass`、普通最终回答和处理过程保留为活动记录。DM 同一因果链最多三轮，两个方向共享预算，并保留方向冷却和处理租约；频道使用冻结的有限预算。成员撤销、启停和权限变化在实际执行及发布前复核，协作排队也不能绕过原 phone 来源的权限。

本轮在 macOS 隔离目录、真实服务、认证、数据库、文件、会话、工具网关和运行管理器上验证，模型供应商响应使用确定性替身。页面测试使用 HTTP 替身，不能独立证明服务实现。没有连接真实外部账号、付费调用、提交、推送或发布。非 Unix 的频道固定目录文件操作明确返回不支持；没有用普通路径读写静默降级。其他操作系统、正式安装包和真实模型供应商网络未验证。T04—T12 仍按后续任务推进，本稿不把先前 T01/T02 的已验成果重新计数。

## 最终候选与证据索引

永久证据位于 [artifacts/rust-tauri/R07/T03](/Users/study_superior/Desktop/Code/LingxiAgent/artifacts/rust-tauri/R07/T03)。`SOURCE_MANIFEST_FINAL.json`、`SOURCE_MANIFEST_R2.json`与`source-snapshot-r2.tar.gz`保存最终 r2 源码；`VALIDATION_RUNS.json`保存精确命令、退出码与源码归属；`CAPTURED_LOGS.json`保存指定原日志及校验值；`INDEPENDENT_REVIEW.json`保存独立复核摘要和限制。下表日志 basename 均在该目录，原临时路径仅作执行来源。

基准提交为 `0c86675d49442fe0e38259bd3097423fb340eecc`，本轮未提交。最终清单于 `2026-10-15T21:04:07.325938+00:00` 冻结292个变更或新增源码、测试、生成物和登记文件，源码指纹：

`c450888b209b37ec02e9c19a6a1c71722551477866c3e056546b572bf3688fb4`

压缩归档 SHA256 实读复算为 `9520a5a812a109b144341f35c4f66b6dfc9a13a19ff33632fd3b279f3af67fdb`，292个归档文件均逐一符合清单 SHA256。六条冻结门禁和三条跨语言协议命令全部退出0，并绑定同一 r2 源码指纹。主代理又在同一 r2 源码上重跑页面155项、三套tsconfig和归属检查，三条精确命令全部退出0；最终VALIDATION_RUNS共21条回执，CAPTURED_LOGS收69项原日志/清单。早期UI、类型及归属结果作为历史保留，最终结论直接引用同一r2候选上的复跑。

旧 r1 指纹 `64bfbd4e684a9d5b86b3545895aa1c54f139e93cad862a9d5e2be2b70a35a61a` 已被 r2 替代。r1 服务625项通过，但适配器升级断言及Clippy失败，不能作为最终整组通过。旧日志与指纹清单保留；未对缺少正文归档的 r1 声称完成逐字 diff。**后续T04等阶段编辑不改变本报告结论，结论绑定此处冻结归档和实际命令回执。**报告不属于源码执行范围。

子集、旧轮次和失败命令中的局部通过不重复相加。

| 标识 | 最终实际结果 | 永久日志 basename/回执 |
| --- | --- | --- |
| E01 | 最终适配器95/95、12组：迁移3、活动6、自动化12、建议10、Bridge效果8/投影1/通知渲染1/存储14、频道文件10、phone存储9、协作存储8、会话文件13；退出0 | `lingxi-r07-t03-frozen-r2-adapter.log`；`VALIDATION_RUNS.json` |
| E02 | E07内T03集成31/31：工具7、phone运行15、会话协作9，均为最终候选实际运行 | `lingxi-r07-t03-frozen-r2-service.log` |
| E03 | E07内既有 R06 会话历史内容12/12，包括真实跨会话 list/read/summary/transcript 与草稿语义，未用fake Delivery代替；早期针对性1项另保留但不累计 | 同上 `r06_session_history_content`；旧 `lingxi-r07-t03-collab-r06-r1.log` |
| E04 | 最终页面12组155/155，退出0：请求错误、请求归属、频道动作、确认卡、历史重建、消息事件、输入焦点、滚动等；HTTP为替身 | `lingxi-r07-t03-frozen-r2-ui.log`；`lingxi-r07-t03-ui-frozen-r2-runs.json` |
| E05 | `npm run typecheck` 三套 tsconfig 全部通过，精确命令与退出0已记录 | `lingxi-r07-t03-frozen-r2-typecheck.log`；同上回执 |
| E06 | 归属检查通过，精确命令与退出0已记录 | `lingxi-r07-t03-frozen-r2-ownership.log`；同上回执 |
| E07 | 完整冻结服务627/627、23组：全lib472及其余集成155，含T01/T02配套与R06回归；退出0，不把全部627计作T03新增用例 | `lingxi-r07-t03-frozen-r2-service.log`；`VALIDATION_RUNS.json` |
| E08 | 协议36单元+1验证器检查、132生成文件无漂移；59样本TS→Rust逐字稳定往返与协议tsc，全部退出0 | `lingxi-r07-t03-frozen-r2-protocol.log`、`lingxi-r07-t03-frozen-r2-generated.log`、`lingxi-r07-t03-final-ts-roundtrip.log`、`lingxi-r07-t03-final-rust-roundtrip.log`、`lingxi-r07-t03-final-protocol-types.log`；`VALIDATION_RUNS.json` |
| E09 | 全workspace/all-targets Clippy `-D warnings`及全Rust格式检查通过；空格式日志由精确退出0回执确认，不凭无输出推定 | `lingxi-r07-t03-frozen-r2-clippy.log`、`lingxi-r07-t03-frozen-r2-format.log`；`VALIDATION_RUNS.json` |
| E10 | E02 phone第14项：真实HTTP覆盖频道创建/详情/list未读/书签/成员双侧书签、同名与request冲突、改用户名重放、DM两联系人排序/缺省主助手/双侧历史、两类设置与GET mode、reset可见边界/raw export、开关保留偏好、删除后二次404 | E07内 `actual_http_channel_management_and_dm_read_surfaces_preserve_real_state` |
| E11 | E02 phone第15项：两Agent真实频道递归回复共享四轮根预算，预算耗尽后与重启后均零新Run/零模型调用 | E07内 `actual_channel_recursive_replies_share_one_guard_budget_and_restart_does_not_reopen_it` |

E02 的具体目标和关键断言如下。外部模型端口被替换，真实 Run、数据库、网关、文件和历史均未用内存假成功代替。

| 目标 | 关键实际检查 |
| --- | --- |
| `r07_conversation_tools` 7项 | 真实审批前后成员撤销；真实 Run/代次/尝试/owner；频道 create/post 的真实来源和发言者；DM 双侧历史只写一次；成员范围与同名频道歧义；继承只读的网关闸 |
| `r07_phone_runtime` 15项 | 实际成员移除取消暂停模型；只读拒写；只有显式回复公开；pass 不公开普通 final；重复请求与重启不重做；双向 DM 三轮及冷却；停止标记变体；同秒 reset 边界；旧 Markdown 发现与外部追加因果；后台会话30分钟复用/轮换；31分钟提醒开关/重启；真实 Run 已完成而活动未落盘的恢复；E10真实HTTP全表；E11频道循环四轮重启 |
| `r07_session_collab` 9项 | 工具只产卡；忙目标持久排队到真实因果 Run；并发创建同一目标及重启不重发；reject/过期/只读/伪造来源；未知创建不重发；phone 成员撤销/关闭频道/写权限收窄后排队零新 Run；合法 Completed 来源仍可排队交付；ticker 不把 claim 当已运行 |

## 22 个直接或实质共享叶子的对应

当前 `R07_LEAF_MAP.json` 给 T03/T07 同时映射64叶。以下22叶是 T03 本轮直接实现或实质承担的份额。22叶均有本机适用证据；不由一个共享通过推定所有组合或其他系统通过。主代理负责账本认定，表内明确实际证据层次与持续限制。

| 叶子 | 实际入口与行为 | 已见证据 | 仍需保留的证据边界 |
| --- | --- | --- | --- |
| `R00-T02-LA-DD087FDD3244` | `DELETE /lingxi/v1/channels/{id}` 删除正式频道文件；不存在404 | E10 真实DELETE200、GET404、二次DELETE404，且正式频道文件列表不再包含该ID | 未执行真人桌面/其他系统删除操作 |
| `R00-T02-LA-83F82782EBE0` | 删除成员、更新书签并撤销其 phone 范围 | E01 撤权后旧 prepared append 拒绝；E02 `actual_member_removal_cancels_waiting_models_before_any_publication`、协作排队撤成员 | 真实模型供应商与其他系统未验 |
| `R00-T02-LA-85E238ED0A35` | `GET /channels` 返回真实摘要、未读数、用户书签 | E10 真实list核messageCount=1/newMessageCount=1，读标后归零且bookmark等于真实时间；E01独立user目录 | 其他系统未验；不把UI替身单独当服务证据 |
| `R00-T02-LA-D6260D4602D4` | `GET /channels/{id}` 返回消息、成员和元数据 | E10 GET实际消息/发言者及成员变化；E01旧格式、正文与未知字段保留；E02成员工具范围 | 非Unix文件入口明确不支持 |
| `R00-T02-LA-2D0E80691667` | `GET /conversations/{id}/agent-activities` 返回实际活动与 ticker | E02 `phone_ticker_distinguishes_claims_actual_runs_and_the_persisted_reminder_slot`，区分 claim、真实根/后代运行、排队、下一提醒 | DM 无主动提醒；忙碌/排队下一时间为 null，不能编造计数或时间 |
| `R00-T02-LA-4E5926A9497C` | `GET .../agent-phone-settings` 读频道或 DM 投影设置 | E10 两类GET/POST设置roundtrip，频道及DM未知字段均保留；E02实际提醒/只读运行 | 所有可选模型覆盖字段排列未穷举 |
| `R00-T02-LA-58E05103F577` | `GET .../agent-phone-tool-mode` 读现行模式 | E10 频道与DM独立GET tool-mode均精确回读write；E02实际只读网关/模式收窄 | 只读不仅展示值；旧运行撤权另有真实零派发/零新Run证明 |
| `R00-T02-LA-64A517AF731B` | `GET /conversations/{id}/export` 返回 Markdown 下载附件 | E10 实际HTTP频道和DM原始正文导出/下载头/私有缓存，reset后raw仍保留历史；E07防头注入单元；E04页面消费 | 未执行真人桌面文件保存或其他系统下载 |
| `R00-T02-LA-65D8750AA9E8` | `POST /channels` 写频道，返回 id/name/members；身份冲突409 | E10 POST精确id/name/members、同动作重放稳定、同request变更payload409、友好同名不同ID、人数不足400；E02真实工具创建 | 现役以频道ID为身份；友好同名合法，不新增名字唯一约束 |
| `R00-T02-LA-A3FF205AEA4E` | `POST /channels/{id}/members` 同时更新成员和成员书签 | E10 真实add/remove同时核频道成员和对应Agent书签；剩不足两位400且原成员未变；E01原文保留 | 成员撤销时等待任务的真实竞态另见A05 |
| `R00-T02-LA-9C2C9DDD5C20` | `POST /channels/{id}/messages` 追加消息，持久来源后异步提及交付，返回时间戳 | E10 HTTP timestamp/用户名及改名后同request沿首次事实；E02正式后台交付、真实journal来源、重启不重做 | 普通final不公开；不以模型回答冒充已发布消息 |
| `R00-T02-LA-21462479CB14` | `POST /channels/{id}/read` 写用户 bookmark，影响未读 | E10 POST read后GET未读归零且独立用户bookmark等于真实消息时间；E01不混淆同名Agent | 其他系统未验 |
| `R00-T02-LA-AB6659451E92` | `POST /channels/toggle` 保存 enabled 并启动/停止正式宿主发现 | E02 31分钟提醒启停/重启、关闭频道后协作排队零新 Run；宿主有监督恢复/停止 | 没有启动旧 Node Agent loop；正式包生命周期另待后续验证 |
| `R00-T02-LA-6663A77204AC` | `POST .../agent-phone-settings` 写 DM 投影或频道元数据并刷新提醒 | E10 频道和DM真实设置roundtrip及未知字段保留；E02提醒启停/持久游标和实际完成时间 | DM没有主动定时器；旧活动只作兼容 |
| `R00-T02-LA-4B76B864DF95` | `POST .../agent-phone-tool-mode` 保存模式，撤销已收窄权限并刷新提醒 | E02 `narrowing_phone_mode_blocks_queued_collaboration_before_any_target_run_is_created`、只读 Phone 拒绝实际写执行 | 只读不仅是展示值；来源/根冻结模式、绑定模式和当前模式一起核验 |
| `R00-T02-LA-0C94E56AF61D` | `GET /dm` 按真实 owningAgent 读取，按最近活动排序，隐藏 reset 前历史 | E10 两联系人实际GET排序、latest正文、ownerAgentId主助手a而非当前c；E02reset后可见历史 | 其他系统未验，不借用T02 Bridge排序证据 |
| `R00-T02-LA-963D7804F78C` | `GET /dm/{peer}` 按可见时间边界读双侧历史，缺失404 | E10 缺失DM与无效Agent均404，双侧真实文件读取同一正文；E02同秒reset不丢新动作 | 其他系统未验 |
| `R00-T02-LA-021F8ADF489B` | `POST /dm/{peer}/reset` 保留原始字节/历史，更新可见边界并取消旧 phone 范围 | E01 持久 epoch/reset 原子绑定；E02 `actual_same_second_dm_reset_hides_old_history_without_replaying_or_losing_new_action` | 不删除旧历史，不将旧消息重新派发；非 Unix 文件行为未支持 |
| `R00-T02-LA-0FB9CBC86E41` | `POST /session-collab/apply` 确认真实草稿；必要时正式创建目标；持久排队与真实受理分开 | E01协作8项；E02协作9项：真实创建/接受Run、并发/重启、来源撤权；E03跨会话读取；E04卡片/历史 | partial与decisionPersisted失败有真实存储事务及UI分层覆盖，未执行完整服务联合故障注入；不虚称该整链已验 |
| `R00-T02-LA-E76F081F6E5B` | `POST /session-collab/reject` 记录拒绝，不建会话或发送 | E01 rejection 真实来源/状态校验；E02 拒绝/过期/伪造来源；E04 拒绝失败仍保留卡 | 不信任 body 自报 sourceSession；拒绝不能重新打开已开始的草稿 |
| `R00-T02-LA-361603791D9E` | 正式 `channel` list/read/create/post 工具；`dm` 及 phone reply/pass 接同一真实服务 | E02 工具7项：审批/只读/成员撤销/实际来源/双侧历史/歧义名 | 私密可见性不赋予文件写权限；继承只读在真实网关阻止副作用 |
| `R00-T02-LA-51E2C5A963ED` | Channels 页面使用实际请求、范围和错误；协作卡读持久决策 | E04 12组155项含动作/切页/草稿queued/unknown/partial重试/事件与历史重建 | 页面为 HTTP 替身；无完整真人桌面、非 macOS 或正式包 E2E |

## A05、A06 与其余基础场景

| 场景 | 本轮证据与边界 |
| --- | --- |
| A05 成员退出后不能继续收私密内容 | E02 成员移除时真实模型暂停，恢复后 Run 实际取消且无公开写入；真实工具等待审批时撤成员，之后发布被拒；E01 撤权不能恢复旧文件 prepared 操作。协作已确认排队后撤成员、关闭频道或 mode 收窄，E02 均核零新增目标 Run。真实已发生来源、任务、交付和决策均有持久记录，不把“没启动工具”当作权限证明。E07最终门禁及独立复核已通过，本结论限本机隔离环境。 |
| A06 循环委派有界停止 | E01 同一冻结根预算并发只消费一次、后代共用根；方向冷却10秒、处理中租约5分钟、DM总计三轮持久。E02 真实双向 DM 到三轮受控停止、停止标记变体及冷却后新用户动作；不制造无界 Run。E11又以真实频道两Agent四轮共享根预算证明耗尽及重启不重开；上限64的存储防护另有测试，未声称逐个预算数值均作端到端穷举。 |
| A01—A04 | 属 T01/T02，各自报告和冻结证据继续有效；本稿不重复完成认定。 |
| A07—A24 | 属后续浏览器、文档/插件、媒体、工作台、配置、入口、观测、开发和整阶段闭环，本轮未据此认定完成。 |

## 64 叶中另外 42 叶的交接

清单对 T03/T07 的广泛共同映射不等于本轮应重做全部已验能力。以下42叶仍按 T07/相应既有责任核验；其中先前 T01/R06 已有证据的活动或资源能力保持原归属，本稿不重新打开，也不直接把其账本改成未完成。最终逐叶继承、补测和状态由主代理处理。

下列标识统一省略前缀 `R00-T02-LA-`，与现行叶清单逐项对应，总计42个。

| 范围 | 数量与标识 | 后续交接 |
| --- | --- | --- |
| 文件树监听 | 1：`CECACC73463E` | T07 的实际 native root/文件树监听；没有 root 不宣称已监听 |
| Desk 活动、文件、jian、日志 | 18：`01631175277C`、`F6F578E62D72`、`B8313FF711C7`、`6A5E3FEEA80D`、`23824E784D71`、`5530FFB14481`、`8192BCF388CF`、`F6CEE2A4A3CE`、`4AC692C4C7C8`、`770B9A0E7420`、`2AB1A11D81C8`、`DAE825843EAE`、`E4CC01D8499E`、`3F0215CE6CAC`、`3EDEDFD7E60D`、`8C68B3F74F1F`、`034CA4E35DBF`、`2396CADB0FE6` | 继承已有活动/升格证据；T07 核工作台处置、逐项结果、回收及挂载范围，不将 phone 活动 ticker 当全部 Desk 完成 |
| 日记 | 2：`67D6791650C6`、`6DBCFD556C3B` | 按既有日记/工作目录职责衔接，不以频道 Markdown 写入代替日记逻辑日期行为 |
| 移动工作台文件 | 10：`CC0271AAE0BD`、`BD5448131631`、`5F675621186C`、`46F9CB129DD1`、`9F44400169B1`、`361D16F25214`、`D85EC29D698A`、`1BDB6B867DBA`、`EE30E2A3381C`、`BEAB7F136A6F` | 保留 files.read/files.write、挂载/子目录、写租约及每项结果，T07/T09 对齐正式移动入口 |
| Goal 偏好 | 2：`1F41AFB4207D`、`2CB7D4AE4478` | T07 目标预算/偏好；不能与 phone 因果传播预算混为一个状态 |
| Studio 工作区挂载 | 4：`52A0034B50CF`、`ED2647610C7B`、`A08713994B5B`、`932719987220` | 沿既有 ResourceService 与本地主人/远端路径边界，不重新实现一套挂载权限 |
| loop/goal/workflow | 4：`125BFE1EE56C`、`E09C077F0100`、`1DFC5E1E130A`、`C1BA7A7C2737` | T07 正式循环、目标、工作流生命周期/用量/节点复用；有限 phone 传播不替代这些工具 |
| 工作设置页面 | 1：`E3A8057D7156` | T07 实际逐动作页面投影，不能用 Channels 页面检查代替 |

## 外围执行器清单与接口交接

T03 沿既有 Rust 结构完成，未新增外部库、独立 Node Agent loop 或新的用户聊天作为后台中转。模型供应商仍走既有 R05 模型端口；正式服务、文件和运行接受点均在 Rust。

| 组件 | 本轮真实职责 | 后续必须保留的边界 |
| --- | --- | --- |
| `ConversationApi` / `conversation_routes` | 认证的频道、DM、设置、活动和导出；只从真实会话/Agent 派生身份 | HTTP 未知身份字段拒绝；来源权限不从消息正文和保存路径恢复 |
| `ConversationFiles` | 现役 Markdown、双侧 DM、书签、原子写入前后像、逐项收据 | Unix 固定目录句柄/无跟随；未知字段和失败保留；非 Unix 明确拒绝 |
| `ConversationHost` / phone 正式运行入口 | 有监督恢复、发现、排队、运行和停止；真实父子/cause；有限根预算 | 不将 claimed 当执行；重启未知不重发；DM 无主动定时器；实际终态时间计算提醒 |
| `conversations` 存储 / discovery / status | 文件请求收据、来源 journal、phone根链/投递/方向租约、绑定epoch、游标、真实状态证明 | 共用数据库写通道；旧epoch、同因果重复及无真实Run的假绑定拒绝 |
| 既有 `SessionCollabTools` 与新 `session_collab` | 复用现有 list/read/send/create 端口，持久草稿、确认、真实创建/队列/受理 | 工具不直接跨会话发送；sourceSession/run/toolCall来自真实ctx；没有 ServiceState 强引用环 |
| `conversations_collab` / 历史投影 | 冻结编辑、创建目标预留、唯一交付租约、真实接受回执、独立决策/跨页overlay | pending/applying/applied/rejected/unknown 与 delivery queued/started/accepted/failed/unknown 分开；partial仅同目标重试；未知禁止盲重发 |
| 正式工具网关 / phone Run authority | 审批、动态只读、实时成员与启停模式；SessionCollab/Subagent 祖先范围 | 已受理后代也不丢 phone 范围；公开频道材料不等于电脑写权限 |
| React 原生 HTTP / 现有消息事件 | 保持每次请求真实范围、稳定requestId、错误保留、确认卡恢复和活动显示 | 不把网络失败写成空数据，不把 queued/unknown 写成已送达；不新建旁路消息通道 |

主要复用接口：

- `session_collab::production(&ServiceState)` 构造窄依赖工具；`register` 仅登记现有 session 工具。主组合根存 `OnceLock<Arc<SessionCollabTools>>`，不保存循环 ServiceState 引用。
- `session_collab::{tick,recover,shutdown}(&ServiceState)` 由统一 ConversationHost 调度，不另起无人监督循环。
- `SessionStore::execute_collaboration_for(...)` 从真实已确认 Started 草稿核 frozen body、source session/run/toolCall、target 与模式，使用 `RunOrigin::SessionCollab`、`parent_run_id=sourceRun`、`cause_id=suggestionId`、`source_message_id=None`。输入文本由共享 `collaboration_input(realSourceAgent, frozenBody)` 生成，后台档案按真实因果逐字校验。
- `conversation_status::snapshot(db, conversation, actor)` 只认正式 phone 根及真实因果后代；`last_completion(db, conversation, members: Vec<String>)` 只取当前频道成员的非取消终态真实根 Run 时间，API 与运行发现共同使用，不能改成恢复时钟。
- `collab_decisions_for_session(db, sid)` 返回跨历史页的最新每草稿决策；`decisionPersisted` 只有最新决策与当前真实交付事实一致才为 true。历史模型输入跳过该 UI 决策记录，前端确认卡使用 `frozenApply` 恢复已确认编辑。

本轮 V19 名为 `r07_conversation_phone_collab_receipts`。当前登记 SHA256 为 `ad7496e374aeae2f6faeebaf46ee35e29013af39e95dfcc1c8303de5d477f20e`，增加文件操作/目标/来源/派发、phone根链/投递/方向/游标/epoch/绑定，以及协作草稿/决策等正式表；V1—18 保留。E01包含实际迁移/升级、重启与独立登记验证，全部通过，不仅凭登记值认定。

后续阶段优先沿现有 RunSupervisor、SessionAdmin、BackgroundDriveRegistry、ToolGateway 和 ResourceService 扩展具体能力；文件/浏览器/媒体执行器不应借用频道成员身份扩大本地权限。T07 处理上述42叶的独立生命周期与资源能力，T09 核移动/LAN入口，正式包与跨平台验证按后续阶段安排，本报告不代替这些交付。

## 失败与修正记录

以下旧日志保留原结果。编译失败不算测试已执行，失败命令中的部分通过也不改写成整命令成功。

| 原始日志 | 当时实际失败 | 已有后续证据/待核事项 |
| --- | --- | --- |
| `check-r1`—`check-r5`、`collab-store-r1` | 新接口接线、错误类型、RunId/整数/生命周期、fixture类型等编译失败 | 最终E01/E07/E08全部通过，保留旧失败来源 |
| `collab-service-r1` | 0/4；早期fixture/阻塞请求错误，另暴露决策记录被当作模型历史的真实缺口 | 正式决策类型分类和跳过后修复；后续r5/r6为4/4，E02为9/9 |
| `collab-service-r2`、`collab-service-r4` | 各2/4；fixture旧列名及后台输入未归档/前缀因果校验不一致，实际目标Run失败 | 共享输入helper、真实Phone/Collab来源证明及已有branch追加修复；E02真实模型输入和后续历史检查通过 |
| `collab-service-create-r3` | Root区域缺少模式类型限定，编译失败 | 修正后后续服务目标运行；未把该轮记为测试失败用例数 |
| `tools-runtime-r1` | 工具7/7；phone当时4/6，真实因果重复发现导致输入数不符 | source operation持久绑定/发现去重修复；E02 phone15/15 |
| `phone-collab-r5` | PhoneBinding 不支持测试直接比较的编译错误，另有未用方法警告 | 改逐字段真实断言/清理；下一轮执行，不屏蔽门禁 |
| `phone-collab-r6` | 工具7通过；phone11/13。其一旧fixture预期小写`user`，实际配置默认`User`；其二活动写入延迟覆盖真实终态时间，31分钟尚未到 | 默认名断言按现役配置修正；真实DB终态优先，并以生产settlement clock记录实际完成。E02 phone15/15；新崩溃窗口在失败r6内已局部通过，未把r6整命令称通过 |
| UI `ui-r1`、`ui-r2` | r1为45通过/5失败：partial卡、频道目录/消息投影和DM设置owner；r2为100通过/2失败：DM事件方向及切换服务后迟到目录覆盖 | r3—r7对应子集103/105/106/106/107通过；E04最终12组155项、E05三配置检查通过；旧子集不累计 |
| `root-eslint-r1`、`ui-lint-r6` | 日志含未用局部函数及 `no-explicit-any` 警告；没有把“0 errors”写成无警告通过 | Rust最终E09禁止警告通过；TS旧lint警告仍保留，不将三套tsc通过冒称TS无lint警告 |
| `frozen-r1-adapter` | 旧Bridge V17升级回归7/8，`after.len`仍预期18而实际迁移已为19；命令失败，后续目标未由该命令证明 | 改为正式MIGRATIONS长度断言，最终E01全部95/95；没有修改旧迁移使断言迁就旧值 |
| `frozen-r1-clippy`、`clippy-r2` | 测试/函数布局、缺else布局、过大的Err返回、Busy单元variant误用结构pattern、可折叠if导致静态门禁失败 | 保留逻辑并Box缩小错误返回、修正布局/模式和冗余分支；未放宽lint，最终E09全workspace/all-targets禁止警告通过 |

独立复核揭示并修复的两个实际跨层问题需要保留：一是已确认协作排队时没有目标 Run，原 phone 成员撤销/关闭频道未必能通过取消已有任务收口；二是写模式降为只读后，旧 session 的冻结模式仍可能让协作先受理再被运行闸取消。现在 apply、排队 claim 和实际 execute 前重核真实 phone 祖先、根 Run、当前成员/启停/模式与正式绑定；E02 三项撤权回归均要求**没有创建目标 Run**，不是先 Accepted 再 Cancelled。

提醒时间另有真实崩溃边界：Run 已完成而 Host 活动未写入时，只读活动文件会丢失正确计时基础。现在从已证明的真实根 Run 终态时间优先计算，取消不作为完成提醒基准，恢复时刻不能替代真实完成时刻；E02 的15项 phone 中有专门重启窗口测试。

## 独立复核与交付边界

永久目录内 `INDEPENDENT_REVIEW.json` 记录审阅者 `/root/r07_t01_review` 的 `PASS_LOCAL_REVIEW`。审阅者只读核对292文件的hash/bytes、压缩包摘要、最终精确回执和原日志，确认成员/全局撤权后协作排队、归档phone会话阻塞、done标记变体、只读降权、实际完成后未写活动即重启等问题已关闭；未独立重跑Cargo或GUI。其结论明确保留partial/decisionPersisted失败的分层覆盖边界、真实供应商/其他OS/正式包及T04—T12未验，不把其审阅说成额外测试执行。

主代理已将指定源码清单/归档、原日志和精确命令回执归入永久目录，没有复制隔离测试主目录、数据库、合成凭据或整个临时目录。`lingxi-r07-t03-evidence-manifest-draft.json`仅为早期日志观测索引，最终依据为 `SOURCE_MANIFEST_FINAL.json`、`VALIDATION_RUNS.json`和`CAPTURED_LOGS.json`，不能将该旧草稿索引当作最终执行清单。

本报告收口没有运行Cargo，没有修改冻结源码/测试、T01/T02报告、R07_PROGRESS或功能结果账本。22叶本机份额与A05/A06可由主代理按冻结证据认定；42叶交接不被批量标完成。其他账号、其他操作系统、非Unix文件操作和正式包的限制持续保留；后续阶段编辑不倒改本轮冻结证据。
