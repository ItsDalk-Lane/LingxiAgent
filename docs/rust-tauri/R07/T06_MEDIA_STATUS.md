# R07-T06 媒体生成、语音与视觉桥 — 设计与断点状态

状态：**LOCAL_VERIFIED（候选 t06b，本机隔离环境）**。八项门禁全绿（media-runtime 11/11、
media-admin 9/9、unit 6/6、service 35组777、adapter 16组249、pdf-chromium 7/7、UI 143、lint 绿），
38 叶 + 顺延 7 叶 + A11/A12 全部 PASS_LOCAL_T06；收口结论与逐叶映射以
[T06_REPORT.md](T06_REPORT.md) 与 FEATURE_PARITY_RESULTS.json 为准。
基线：T05 已收口（LOCAL_VERIFIED，候选 t05c）。本文件是 T06 的工作台账；收口后提炼进 T06_REPORT.md。

## 候选史

- **t06a**（作废，回执保留）：444 文件清单，指纹 7be9fb89…0a9a4。四门已跑绿
  （media-runtime 11/11、media-admin 8/8、unit-gates、ui 143/143）后发现视频/语音/ASR
  管理面缺直接 HTTP 覆盖，按 T05 t05b 先例作废重冻。
- **t06b**（现行）：同 444 文件集，指纹 190b0c25…b49bb。相对 t06a：新增视频 HTTP 全链
  测试与 admin 扩展（speech/video CRUD、config PUT、capability 词表、ASR 编辑/移除），
  并修复扩展覆盖揭露的 speech config PUT 405 路由遮蔽缺陷。

## 范围（台账归属 R07-T06 的 38 叶 + T05 顺延 7 叶 + A11/A12）

### 已闭合：模型驱动的媒体运行时链（11/11 集成测试通过）
测试目标 `r07_t05_media`（tests/r07_media/mod.rs，11 项）。修复的两个真实缺陷：

1. **组合根静默替换注入网关/批准门**（lib.rs）：真实模型链接线时（turn_provider 缺席），
   组合根一律自建 registry/ApprovalService/gateway 并覆盖注入依赖，导致注入的批准服务
   永远看不到批准条目——9 项媒体测试全部在 "外部生成须先进入正式批准" 超时。
   修复：注入 tool_gateway+approval_gate 成对存在时采用注入装配（其注册表承接全部正式
   工具登记、批准请求落到宿主批准服务）；只注入其一是响亮启动错误；全未注入保持原
   自建路径（生产不变）。
2. **即梦指纹浮点被拒绝**（media_jimeng.rs）：可执行文件指纹的 mtimeMs 用
   `as_secs_f64()*1000.0` 产生浮点，而工具参数预算校验拒绝一切浮点（TS 安全整数平价），
   导致能力探测 helper 调用永远失败（"即梦纯SDK参数超出限制"），后续 generate 被拒
   （"即梦能力尚未探测"）。修复：mtimeMs 改整数毫秒（u64）。worker 侧契约
   `Number.isFinite(mtimeMs)` 对整数成立，兼容。

回归：service 全量（lib + 33 目标，768 通过 / 0 失败）源码未变回执 dev-t06regress2。

### 已闭合：媒体画廊存储（media_gallery + v23）
- `lingxi-adapters/src/storage/media_gallery.rs`：持久画廊投影（v23 纯追加迁移，
  `supported_version`=23）；媒体 job 认领/状态迁移与画廊同步在**同一 SQLite 写事务**内
  完成（media.rs 的 sync_from_job_txn 钩子），状态映射 pending/done/failed/cancelled，
  task_type 取 kind 原值（image|video|speech）。
- 受影响套件回执：gallery 2/2、session_file_store 13/13、r07_t05_media 11/11（dev-t06t1~t4）。

### 已闭合：媒体管理面（media_admin.rs）
供应商/模型目录读写（providers.manage 门 + 真实校验拒绝不污染持久化）、四类媒体配置
读写（settings.write 门 + security_audit 投影）、ASR 供应商与配置；列表载荷形状与现役
`{providers: MAP, config}` 一致，UI（useMediaSettingsData/ModelEditPanel 等）零改造命中。
扩展覆盖揭露并修复一个真实缺陷：静态路由 `GET /lingxi/v1/media/speech/config` 遮蔽
参数化路由 `PUT /lingxi/v1/media/{kind}/config`，导致现役支持的 speech 配置写入恒 405；
修复为静态路由合并 `get().put()`（新增 put_speech_config 处理器）。

### 已闭合：HTTP 生成/任务/产物/转写面（media_generate.rs，9 路由）
- POST `/lingxi/v1/media/generate`（kind 词汇表：image_generation→image 等；未知 kind 400）、
  POST `/lingxi/v1/media/{image,video,speech}/generate`：实名主体 → MediaService 受理
  （session_busy→409）→ 真实源 Run → 网关 → **受理回执不含任何文件**（A11）。
- GET tasks / tasks/{id} / tasks/batch/{batchId}：画廊投影倒序筛选，404 响亮。
- POST tasks/{id}/retry：仅 failed/cancelled 可重试（其余 409），非生成类 400；
  响应携带**新** taskId，画廊投影同事务改键（attempt/retry_count 连续性保留），旧 id 404。
- GET generated/{filename}：真实字节流（std::fs + spawn_blocking 窗口读），Range→206、
  越界→416、未知→404、嵌入点文件名→400、`%2E%2E` 在路由层拒绝（404）；Content-Type
  按真实产物 MIME。
- POST generated/open/{filename}：注入 opener 端口（生产为系统打开，测试为录制替身）。
- POST asr/transcribe：同步返回转写对象；失败也是 200 + `status:"failed"` +
  `error:"CODE: message"`（诚实失败）；pending/final 两帧 `voice_transcription_update`
  经 app-events 发布 `{sessionId, fileId, transcription}`；provider/model 覆盖不匹配→400。
- 集成目标 `r07_media_admin` 9/9（dev-t06t15）：真实回环 HTTP 服务 + 确定性供应商替身
  （chat catcher 证明媒体面从不触达聊天模型），全部生成/重试经真实批准门放行。
  覆盖扩展（t06b 相对 t06a）：新增视频生成 HTTP 全链测试（agnes-videos 异步提交→轮询→
  真实字节回传→图库 done→产物服务）；admin 测试扩展 speech/video 模型 CRUD、
  PUT speech/video config、通用 `providers?capability=` 四类词表与专属路由逐字节一致、
  ASR 模型编辑/移除。

### 已闭合：UI 接线
- `use-hana-fetch.ts` serviceRoute：`/api/media/*`、`/api/speech-recognition/*` 在
  serverNodeKind=lingxi-service 时重写为 `/lingxi/v1/*`；前缀形似路径（/api/mediax 等）
  不被误吞（负向断言）。
- `app-event-actions.ts` 新增 `voice_transcription_update` 分支：Rust app-event 载荷
  `{sessionId, fileId, transcription}` → `native:{sessionId}` 会话附件按 fileId 就地挂
  transcription（语义对齐现役 applyVoiceTranscriptionUpdate；找不到会话/附件不虚构更新）。
- `rust-automation.ts` 轮询白名单加入 `voice_transcription_update`（此前事件到不了
  handleAppEvent，属本次真实接通）。vitest 143/143（dev-t06ui2）、typecheck×3 绿
  （dev-t06ui3）。

## 偏差登记（与现役 TS 面的语义差异，逐条冻结）

1. **HTTP 生成/重试在 operate 会话也需一次人工批准**：媒体 generate 工具为
   `PermissionKind::Review`，现役裁决语义（approval_service.rs）对 Review 类在一切用户
   会话模式（含 operate）一律 NeedsApproval，read_only 直接 Denied。现役 TS 工作台路由
   直接发起生成。Rust 面移植现役裁决语义（更严，非更松）；r07_media_admin 的每次
   生成/重试都经真实批准门放行。
2. **ASR 读取上限预检 + 整读核验**：transcribe 先按登记大小响亮拒绝超限文件（不整读
   超限文件），落盘内容与登记不一致时由 OperationService::transcribe 按实际字节数再次
   核验，绝不截断冒充（修复了 read_window 对小于上限文件 read_exact EOF 误报
   "session file not found" 的真实缺陷——该缺陷会使一切转写 400）。
3. **retry 响应携带新 taskId**：现役 beginAttempt 原地重试；Rust 面新建 job 并同事务
   改键画廊投影（attempt/retry_count 保留），旧 taskId 404。已在此登记，不冒充原地语义。
4. **语音转写事件双通道各自解析**：Rust app-events 通道以 sessionId 标识；现役 WS 通道
   以 sessionPath 标识。渲染侧两条通道各自解析，互不冒充；原始 `<audio src>` 直连
   `/api/media/generated/{filename}` 为现役已知受限面（file:// 注意事项见码内注释），
   本阶段不重新设计。
5. **`%2E%2E` 穿越段在路由层 404**（不进入处理器），嵌入点文件名在校验器 400——两种
   拒绝都是响亮拒绝，测试分别断言真实层位。
6. **语音 response 投递为同步等待**：与现役一致，路由等待 job 终态（100ms 轮询，
   110s 预算）；Completed→返回真实文件，Failed/Cancelled/Unknown→错误，超时→受理
   回执不含文件（不冒充完成）。

## 验收映射
- A11（提交成功不冒充产物完成）：r07_media 的
  video_uses_real_submit_query_download_and_session_delivery_stages（Generating 阶段
  downloaded==0、文件下载完成后才登记 session file）+ confirmed_supplier_failure 项；
  r07_media_admin 的 http_image_generate_acceptance_then_gallery_done_and_real_bytes_served
  （受理回执零文件、源 Run 终因 `completed.no_final.process_only`、画廊 done 后真实字节
  可服务）。渲染侧 parseRustMediaReceipt 拒绝"completed 但零文件"的伪完成回执。
- A12（取消远端 job 诚实反馈）：
  stopping_a_submitted_job_does_not_claim_remote_cancellation_or_deliver_late_bytes
  （remoteCancel=unsupported、remoteOutcome=unknown、迟到字节不交付、远端生成计数==1）。
- ASR 取消/失败不算成功：r07_media_admin 的
  asr_transcribe_roundtrip_events_and_failure_honesty（失败 200 + CODE 前缀错误透出 +
  事件序列 pending→failed 恰两帧新增）。

## 红线沿用
隔离 home；不 push/PR/发布；外部供应商/CLI/opener 一律确定性替身或已授权本机替身；
不删用例不改预期；失败证据保留（dev-t06t5~t10 失败回执与 dev-t06ui1 失败回执均在
artifacts/rust-tauri/R07/T06/ 未改写）。
