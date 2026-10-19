# R07-T06：媒体生成、语音与视觉桥实施与验证记录

状态：**LOCAL_VERIFIED（候选 t06b，本机隔离环境）**。38 叶 + T05 顺延 7 叶与 A11/A12 绑定同一候选通过。其它操作系统、真实付费供应商、真实 jimeng/dreamina CLI、真实账号与正式安装包未验。

## 范围

仅 R07-T06：媒体生成/任务/产物/转写 HTTP 面、媒体管理面（供应商/模型/四类配置）、媒体画廊存储投影、媒体工具运行时（generate-image/video/speech/asr、get-guide、describe-options、jimeng-cli image/video）、渲染侧接线（serviceRoute 重写 + voice_transcription_update 事件链）。与基线差异：无获准 ADR 之外的架构变更；媒体供应商协议复用现役 metadata 目录与网关路由解析。

## 源码

- 候选：`t06b`，[SOURCE_MANIFEST_t06b.json](../../../artifacts/rust-tauri/R07/T06/SOURCE_MANIFEST_t06b.json)，444 份冻结文件（以 T05/t05c 为基叠加 T06 全部改动；文件集规则与指纹方法学显式记录在清单 `fingerprintMethodology` 字段，可独立重算）。
- 工作树源码指纹：`190b0c25f184eaacc23c53a02bafb54845f25dc05d8b7fa5c86550a7705b49bb`。
- 完整源码：[source-t06b.tar.gz](../../../artifacts/rust-tauri/R07/T06/source-t06b.tar.gz)，SHA256 `76e3e42abf8a8e0b307bb88d7b7171b09a3cd213e027705fc67e97652886e580`。
- 基线提交：`0c86675d49442fe0e38259bd3097423fb340eecc`；分支 `codex/rust-tauri-migration`；`--locked` 依赖锁未变。
- 被替代候选：t06a（四门已跑绿后发现视频/语音/ASR 管理面缺直接 HTTP 覆盖，按 T05 t05b 先例作废重冻）；t06a 的回执与日志全部保留，未改写。

## 环境

隔离 `LINGXI_HOME=/private/tmp/lingxi-r07-tests/home`、`CARGO_TARGET_DIR=/private/tmp/lingxi-r07-build`；外部模型/媒体/ASR 供应商与 jimeng CLI 全部为本地回环确定性替身（SSE 文本、b64 图片、合成 MP4/MP3、受监督合成 CLI），无真实账号、付费请求或外发；系统打开为录制替身。macOS arm64，Node 24 系，cargo `--locked`。

## 完成项（T06 缺口逐条收口）

1. **媒体运行时链**（`r07_t05_media` 11/11）：批准→独立真实 Run→提交/轮询/下载/交付阶段分离→自有会话文件登记；重复请求复用 Run；供应商确认失败保持 failed；重启后查询已知远端不重复提交；插件停用取消真实后台并阻断交付；不可信参数不得挑选凭据/路径/丢弃类型化覆盖。
2. **媒体画廊存储**（media_gallery.rs + v23 纯追加迁移）：job 认领/状态迁移与画廊投影在**同一 SQLite 写事务**内同步；状态映射 pending/done/failed/cancelled。
3. **媒体管理面**（media_admin.rs）：供应商/模型 CRUD（providers.manage 门 + 真实校验拒绝不污染持久化）、四类媒体配置读写（settings.write 门 + security_audit 投影）、ASR 面；列表载荷与现役 `{providers, config}` 形状一致，UI 零改造命中。
4. **HTTP 生成/任务/产物/转写面**（media_generate.rs，9 路由）：统一 generate（kind 词汇表）+ 三类专用 generate、tasks 列表/单读/批次、retry（新 taskId + 同事务改键）、generated 字节服务（Range 206/416/404/400）、generated/open（注入 opener）、asr/transcribe（同步返回 + pending/final 双帧事件 + 失败 CODE 透出）。
5. **UI 接线**：serviceRoute 增加 `/api/media/*`、`/api/speech-recognition/*` 重写（前缀形似路径负向断言）；`voice_transcription_update` 加入轮询白名单并新增 app-event-actions 分支（按 fileId 就地挂转写，未知会话/附件不虚构更新）；vitest 7 套件 143/143。
6. **视觉桥**：本阶段无独立视觉桥新面；图像引用的生成链（image2image/image2video 模式声明校验）经媒体运行时与 describe-options 覆盖。

**真实缺陷修复（均由测试先暴露）**：
- 组合根在真实模型链接线时静默覆盖注入的网关/批准门（9 项媒体测试全部超时暴露）——注入成对存在时采用注入装配，只注入其一响亮启动错误；
- 即梦能力探测指纹 mtimeMs 浮点被参数预算校验拒绝（"即梦能力尚未探测"连锁）——整数化；
- **静态路由 `GET /lingxi/v1/media/speech/config` 遮蔽参数化 `PUT /lingxi/v1/media/{kind}/config` 致 speech 配置写入恒 405**（t06b 扩展覆盖揭露）——新增 put_speech_config 处理器并合并路由；
- ASR `read_window` 对小于上限文件 read_exact EOF 误报 "session file not found"（会使一切转写 400）——按实际字节数核验；
- `rust-conversation-events-http` 既存 fixture 缺 `/sessions/source/media/jobs` 替身路由（dev-t06ui1 暴露的预存缺口）——补真实路由形状并钉住断言。

## 行为变化

用户可见：媒体生成（图/视频/语音）、媒体任务台/重试、生成产物打开、ASR 转写、媒体设置面在 lingxi-service 节点上经 Rust 链交付。语义偏差 6 条逐条登记于 [T06_MEDIA_STATUS.md](T06_MEDIA_STATUS.md)（operate 会话也需批准、ASR 预检+复核、retry 新 taskId、转写事件双通道各自解析、`%2E%2E` 路由层 404、语音 response 同步等待），均不更松于现役。

## 验收

### A11｜提交成功不冒充产物完成

- 命令：`cargo test -p lingxi-service --test r07_t05_media` / `--test r07_media_admin`（候选回执 [t06b-media-runtime](../../../artifacts/rust-tauri/R07/T06/t06b-media-runtime.json)、[t06b-media-admin](../../../artifacts/rust-tauri/R07/T06/t06b-media-admin.json)）；`npx vitest run .../rust-media-http.test.tsx`（[t06b-ui](../../../artifacts/rust-tauri/R07/T06/t06b-ui.json)）。
- 预期：受理回执不含文件；下载完成前不登记会话文件；供应商失败不冒充成功；渲染侧拒绝伪完成回执。
- 实际：受理回执零文件（图/视频/语音三链）；Generating 阶段 downloaded==0；源 Run 以 `completed.no_final.process_only` 结清；供应商确认失败保持 failed 并结清真实执行 Run；vitest 断言 completed 零文件回执被拒收。退出码均为 0，前后清单指纹不变。

### A12｜取消远端 job 诚实反馈

- 命令：同上两候选回执 + UI 套件。
- 预期：取消只停本地等待与交付；不声称远端已取消；迟到字节不交付。
- 实际：`remoteCancel=unsupported`、`remoteOutcome=unknown`，远端生成计数==1（不重复提交），取消后到达的字节不入库不交付；ASR 取消/失败 200+CODE 透出且事件恰为 pending→failed 两帧；渲染侧保留已交付文件并区分远端不支持取消与未知。退出码 0。

## 安全与数据

- 权限负向：read_only 会话生成 403 且零派发；ask 模式并发准入阻塞等待真实裁决；管理面 providers.manage/settings.write 分门把守；越权主体响亮拒绝。
- 凭据：全部供应商凭据为隔离环境伪值，写入隔离 home；任务/列表响应不泄漏凭据（断言无 credential 字段透出）；视频链携带真实 Bearer 头命中替身校验。
- 路径安全：generated 文件名永不参与路径拼接（嵌入点 400、`%2E%2E` 路由层 404）。
- 迁移：v23 纯追加（media_gallery 表+索引），`supported_version`=23 与 registry 一致；画廊同步与 job 状态同事务，无跨事务漂移。

## 完整映射

F-ID→叶→测试逐条登记见 [FEATURE_PARITY_RESULTS.json](../FEATURE_PARITY_RESULTS.json)：38 叶 + T05 顺延 7 叶（jimeng-cli-image/video、generate-image/video/speech、get-guide、describe-options）全部 `PASS_LOCAL_T06`，A11/A12 `PASS_LOCAL_T06`，均绑定候选 t06b 指纹。

## 已知缺陷

1. **`r07_phone_runtime::actual_proactive_31_minutes_obeys_disable_enable_and_restart_slot_receipt` 时序敏感复发**：t06b-service 首跑失败一次（禁用/启用交错窗口：lastError="Agent phone is disabled"，第二主动槽 Run 未在 12s 结清窗口内启动），与 T05 t05a-service 登记为同一签名。根查结论：phone/conversation 路径 30 份文件与 t05c **逐字节一致**（清单哈希比对），非 T06 回归；机制定位在"全局开关写入→wake→后台内部 tick 与测试手动 tick 的交错窗口"，生产周期 tick 会在下一拍自愈错过的槽位，测试的单次手动 tick 无重试。同源码孤立重跑 15/15（dev-t06phone1）与 t06b-service-r2 全量 777/777 通过。首跑回执 [t06b-service](../../../artifacts/rust-tauri/R07/T06/t06b-service.json) 保留未改写。该用例属 T03 叶族，建议 T12 统一回归前做一次专项根因（把交错窗口收敛为确定性）。
2. **t06b-adapter 首跑为编排错误**（误列不存在目标 `r07_media_store`，cargo 拒绝并列出可用目标），非代码失败；t06b-adapter-r2 以修正清单（16 目标）249/249 通过。首跑回执保留。
3. 即梦 CLI 的真实安装/登录/付费生成、真实供应商协议往返：未授权真实外发，全部确定性替身，不记为已验。

## 未执行/BLOCKED

- 真实付费供应商（图片/视频/语音/ASR）往返、真实 jimeng/dreamina CLI：未授权；
- 非 macOS 系统、正式安装包、真实 opener 系统打开：未验（opener 为录制替身）；
- R07 最终统一回归与独立最终核验：T12 范围。

## 回退

v23 为纯追加迁移，不改动旧表；回退到旧二进制按既定篡改防护拒绝高 user_version（响亮失败）。画廊投影可由 job 表重建（同事务同步钩子保证一致起点）。本轮未演练真实回退（隔离环境逐次新建 home）。

## 独立审查

本轮未执行独立审查（无第二名审阅者，不虚构）。全部结论以候选绑定回执（命令/前后清单指纹/exit/logSha256）为据，可由任何审阅者按 SOURCE_MANIFEST_t06b.json 重算复核（方法学字段已显式记录）。

## 下一阶段

允许开始 T07（工作台/文件历史/工作流，A13/A14）：输入为 t06b 候选证据与本报告。不允许开始：任何远程写、发布、真实供应商调用。

## 远程/发布

未获准、未执行：无 commit/push/PR/tag/发布，无真实外发接口调用，无真实数据迁移。
