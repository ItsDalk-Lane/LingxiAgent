# R02-T07｜日志、错误和有界资源 — 执行报告

- 执行者：ZCode:EXECUTOR-R02-T07（一次性执行代理；不负责独立验收，不提交/推送；
  PASS/FAIL 判定归总控另派的独立验收）
- 状态：**READY_FOR_REVIEW**
- 日期：2026-09-26
- 任务书：`Lingxi_Rust_Tauri_Taskbooks_2026-09-23/R02_Rust独立服务、存储与事件基础.md`
  §4 R02-T07（场景 R02-A13「敏感值不出现在日志」、R02-A14「慢订阅者不拖垮服务」，
  均 REQUIRED）
- tested SHA：`7a489cded575f1413ec29ea5ecec565528f16728`（= TASK_BASE_SHA = 远端 HEAD）
  + 本任务未提交改动（§3 全集；工作区另有 `docs/rust-tauri/ORCHESTRATOR_PROGRESS.json`
  一处未提交改动属总控账本维护，本任务未触碰）
- 平台：macOS 27.0 arm64（Darwin 27.0.0）；工具链 rustup 锁定 1.98.1
  （`rust-toolchain.toml`；本机 rustup CLI 1.29.1，锁定 rustc/cargo 1.98.1，已核对）
- 依赖面：`rust/Cargo.lock` **零 diff**（tracing-appender 不在锁内 → 轮转以零依赖
  `std::fs` 实现；零新增 crate、零版本变化）
- 网络与隔离：全部 cargo 命令剥代理 + `CARGO_NET_OFFLINE=true` + `--locked` +
  专属 `CARGO_TARGET_DIR=/tmp/rust-target-r02-t07`；全部证据只用 mktemp 合成 home
  （/tmp）；合成敏感值（`sk-test-…`/`hana_dev_…`/`a13-…`）不含任何真实凭证。

## 1. 范围

任务书四步：①结构化 error code/retryable/causeId，对外错误不暴露 token、本机密钥
路径或原始请求认证头；②日志与 trace 的脱敏及轮转，正文记录遵循现有设置，默认不把
敏感材料导出到证据包；③对连接、DB请求、事件缓存、订阅者、任务注册设置上限，超限
明确 backpressure/rejected；④注入时钟与 ID 生成器供测试使用。
交付：结构化日志/错误；资源上限配置；可注入测试设施。

不在本任务内：T03 REVIEW F01/F02（WS 未掩码帧、设备注册表快照语义，挂 R08/R09）；
Node/Electron 生产入口；任务书、contracts/generated/、.sync-audit/、PROGRESS.md、
ORCHESTRATOR_PROGRESS.json。

## 2. 观察事实（先读再动手，未混写设计）

1. **现役脱敏权威**是 `shared/log-redactor.ts`（经 `lib/log-redactor.ts` 被蛇
   `lib/debug-log.ts` 与 `server/index.ts` 消费）：`[redacted]` 词汇；Bearer/授权头、
   Cookie、secret 赋值（SECRET_KEY_PATTERN）、URL 查询密钥、URL 凭据、已知凭证形态
   （`sk-`/`AKIA`/`gsk_`/`ghp_`/`glpat-`/`xox*`）、40+ 随机 token、`data:` base64、
   用户路径（`/Users/[user]`）。本栈自铸凭证形态为 `hana_dev_`/`hana_ws_` 前缀
   （auth.rs），现役脱敏器不认识 → Rust 镜像必须补上。
2. **现役日志生命周期**（debug-log.ts）：按启动时间戳建文件于 `~/.lingxi/logs/`，
   单文件 5MB 上限（超限截断并停止写入），7 天清理；每行过 redactLogText。
   Rust 侧此前无文件日志（仅 stderr）。
3. **现役错误面**：R02-T03 已有 `EndpointError`（HTTP status + 冻结 `ProtocolError`
   的 `{code,message,retryable,details}`）与 `LINGXI_TRANSPORT_REJECTED` /
   `LINGXI_AUTH_REJECTED` stderr 标记；`details.reason` 已有机读 reason——
   缺 `causeId` 结构化包装与请求级关联 ID。
4. **既有上限**：T03 已有 HTTP body 1MiB、per-peer 速率（240/10s）、WS 连接上限
   （16，acquire 先于 spawn——即任务注册的天然上限）；T04 有界 DB 队列
   （`StoreOptions::queue_capacity=64`，`DbQueue::submit` 为**等待式背压**、
   `try_submit` 才有 QueueFull）；T05 有订阅者信箱（128）、reorder 缓冲（64）、
   页限（500）。**缺**：订阅者总数/单流上限、hub 流注册表上限、错误 causeId、
   请求关联 ID、文件日志与轮转、注入时钟/ID 面、CLI 上限配置面。
5. `tracing-appender` 不在 Cargo.lock（锁内仅 tracing 0.1.44 / tracing-subscriber
   0.3.23）→ 按任务约束走零新增：自实现按尺寸轮转 + 数量裁剪。
6. `Mailbox::recv`（T05）的 `notified()` 首次 poll 才注册 waiter，`create →
   try_recv(空) → await` 之间到达的 `notify_waiters` 在**旧版 tokio 通用语义**
   下存在丢失唤醒窗口（R02-T07 REVIEW_R1 F01 勘误：在锁定 tokio 1.53.1 下该
   窗口经审阅方源码级+确定性差分+压力级三重验证**并不成立**——`Notify` 的
   `notify_waiters_calls` 计数快照会补捕首次 poll 前的唤醒，基线代码无缺陷）。
   本次改动按 tokio 官方 canonical 模式把 `Notified::enable()` 前置，作为
   **前向稳健加固**保留（对更旧 tokio 语义也稳健），不是缺陷级修复（见 §5.2
   与 F01 勘误）。
7. 审计封印测试族开工即红（seal 坐标落后于已授权 R02 提交，R01 同款预存在红）；
   复跑 `npm test -- --run tests/post-verification-audit-seal.test.ts`：
   1 failed | 2 passed（`gates/audit-seal.log`）。如实记录，未使其变绿，
   未动坐标/白名单。

## 3. 修改了什么

新增：
- `rust/crates/lingxi-service/src/redaction.rs`：现役 `shared/log-redactor.ts`
  的 Rust 镜像（17 个单元测试，A13 泄漏反例先行）。
- `rust/crates/lingxi-service/src/logging.rs`：`LogRouter`（脱敏行缓冲 sink，
  stderr + 文件双写）+ `RotatingLogFile`（按尺寸轮转、按数量裁剪、0600/0700、
  序号发现式命名，零新依赖）（6 个单元测试）。
- `rust/crates/lingxi-service/src/inject.rs`：`ServiceClock`（SystemClock/
  ManualClock）+ `RequestIdGen`（Random/Sequential）（4 个单元测试）。
- `rust/crates/lingxi-service/tests/resource_limits.rs`：服务级集成（真实
  router/TCP/runs.db）：错误体 causeId/requestId/无 home 泄漏、注入时钟驱动
  限流的零 sleep 证明、顺序 ID 确定性、订阅者注册表上限显式拒绝 + HTTP 503
  映射（5 个测试）。
- `scripts/rust-tauri/r02_t07_redaction_scan.sh`：R02-A13 二进制级证据脚本。
- `scripts/rust-tauri/r02_t07_slow_subscriber.sh`：R02-A14 二进制级证据脚本。
- `docs/rust-tauri/R02/R02-T07_REPORT.md`（本文件）；
  `artifacts/rust-tauri/R02/T07/`（证据）。

修改：
- `rust/crates/lingxi-service/src/lib.rs`：`EndpointError` 增加
  `cause_id`（`details.causeId`，冻结 `ProtocolError` 结构未动——details 是
  契约扩展点）；`storage_cause_id`（`storage.<variant>` 全变体词表）；
  `RequestId` 扩展 + `error_enrichment` 中间件（最外层：铸 ID、每请求一条
  结构化完成行、错误体重写 `details.requestId` + 经脱敏器重写 `message`
  ——health 探针不记完成行，见 §5.1）；`ServiceDeps` + `bootstrap_with_deps`
  （既有三构造器全部委托）；`ServiceState` 持有 clock/request_ids；transport/
  auth 守卫与 execute/ticket 路径改走注入时钟；标记行经 `redact_line`；
  events_page/WS subscribe 拒绝映射补 SubscriberLimit→503/1013 与 causeId；
  路由层序 enrichment→transport→auth。
- `rust/crates/lingxi-service/src/events.rs`：`EventLimits` 增加
  `max_subscribers_total`(256)/`max_subscribers_per_stream`(32)/
  `max_tracked_streams`(4096)（validate 同步收紧）；`register` 可化（
  `SubscriberCapKind`）→ `SubscribeReject::SubscriberLimit{scope,limit}`；
  `evict_idle_streams`（无订阅者且无 parked 的事件缓存流按序驱逐——基线由
  known-head 对齐无损重建）；detach 路径新增带真实队列统计的 error! 观测行；
  `HubStats`/`hub_stats()`；**`Mailbox::recv` 前向稳健加固（`Notified::enable()`
  前置——tokio 官方 canonical 模式；R02-T07 REVIEW_R1 F01 勘误：锁定 tokio
  1.53.1 下基线并无丢失唤醒缺陷，此改动为加固而非缺陷修复）**；
  新增 4 个 hub 级测试（上限显式拒绝×2、驱逐无损、风暴显式性）。
- `rust/crates/lingxi-service/src/config.rs`：新增 8 个严格解析的 CLI 上限旗标
  （重复/空/0/旗形值全部响亮拒绝）+ 2 个测试。
- `rust/crates/lingxi-service/src/main.rs`：USAGE 与退出码文档同步全部新旗标；
  `init_tracing(LogRouter)` 替换裸 subscriber；轮转日志在**取得单写者锁之后**
  附接（被拒实例不写 home——见 §5.1）；Deps 由 CLI 组装。
- `rust/crates/lingxi-service/src/ws.rs`：`WS_CLOSE_TRY_AGAIN_LATER`(1013)。
- `rust/crates/lingxi-service/src/auth.rs`：`hex_random_public`（request-id 与
  auth 密钥共用同一熵权威）。

未触碰：Node/Electron 生产入口（`git diff 7a489cde -- core/ server/ desktop/
shared/ tests/` 为空）、任务书、contracts/generated/（门禁零漂移）、
.sync-audit/、PROGRESS.md、ORCHESTRATOR_PROGRESS.json、T01–T06 已提交 artifacts
（`git status --porcelain artifacts/rust-tauri/R02/` 滤除 T07 后为空）、
`rust/Cargo.lock`（零 diff）。

## 4. 设计决定

### 4.1 结构化错误模型（任务书步骤 1）

- `causeId` 进 `details`（冻结 `ProtocolError` 结构零改动——契约 §7「扩展进
  details」）。稳定 `domain.cause` 词表：`auth.<denial>`（missing_credential /
  invalid_credential / invalid_ws_ticket…）、`authz.<reason>`、
  `resource.not_found`、`transport.<reason>`（bad_origin / rate_limited /
  body_limit_exceeded / ws_connection_limit）、`request.invalid_message` /
  `request.invalid_limit`、`events.*`（cursor_expired / stream_not_found /
  subscriber_limit / …）、`storage.<variant>`（queue_full / busy / disk_full /
  io / conflict / corrupted / schema_tampered / database_too_new / …）、
  `internal.*`。reason 键全部保留（T03 词表兼容，纯增量）。
- `requestId`（`req-`+16 随机字节 hex）由最外层中间件铸造：请求扩展 →
  拒绝标记行 → 错误体 `details.requestId`——任一诊断可定位回请求（A13
  「关联 ID 保留」锚点）。错误体 `message` 经脱敏器（带 data home）重写：
  结构上保证对外错误不携带本机密钥路径/token/认证头。
- **兼容性**：对既有显式错误路径是包装而非重写——401/403/404 语义与 status
  不变，`reason` 键不变，重试语义（`retryable`）不变，冻结结构零改动
  （`r01-t02-check-generated.sh` 零漂移实证）。

### 4.2 脱敏策略（任务书步骤 2）

- 单一权威 `redaction::redact_text`，应用四层：①每条 tracing 行（LogRouter
  行缓冲 sink，stderr+文件同源）；②`LINGXI_*` 标记行（打印前过 `redact_line`）；
  ③错误响应体 `message`（enrichment 中间件，带 data home 替换）；④stdout
  READY 行不脱敏（机器契约，T01/T02 脚本依赖，且该行无密钥材料）。
- 与现役的**分歧与省略面**（R02-T07 REVIEW_R1 F02 补记，模块文档同步完整
  版）：①`/`与`=`不进本侧 token 字符类——精确边界（对拍实测）：≥40 字符
  且含字面 `/`（或与 `.`/`/`/`-` 相邻，或被 `/`/`=` 切成 <40 段）的长随机
  串**现役捕获、本侧漏网**；不含 `/`/`=`/`.` 相邻的 hex64/base64url 串本侧
  恰恰**被捕获**（原"无 / 无 . 无 = 的 ≥40 字符串会漏网"的表述不准）。本栈
  凭证形态（base64url 无填充/hex、`hana_dev_`/`hana_ws_` 前缀）R02 范围内
  不可达该边界；**R05 接入真实 provider token（标准 base64 可含 `/`）前必须
  复核**。②现役规则未镜像的省略（当前服务不打印 CLI 参数/正文/PII，现实
  影响为零，但须记载）：PII 规则（email/credit-card/CN-ID/SSN）、
  `CLI_SECRET_FLAG_RE`、`CONFIG_SECRET_VALUE_RE`、Windows 用户路径
  （`C:\Users\`）。③对拍实测一致（非分歧）：Host 头内嵌 token 两边都不
  脱敏；`?token=` 双规则 `[redacted]]` 伪影两边逐字一致；非 secret 查询键
  本侧保留 `state=[token]`（现役改写 `?[token]`，本侧更保结构）。④`data:`
  base64、`hana_dev_`/`hana_ws_` 前缀为 Rust 侧补齐。17 个单元测试钉住：
  泄漏反例全绿 + 关联 ID（run_/sess_/req-/seq）全存活 + 普通诊断逐字不变。
- **正文记录遵循现有设置**：服务从不记录请求/消息正文（与现役默认一致——
  正文只存在于脱敏后的业务事件流），且无任何打开正文记录的开关；证据包
  导出即日志/stderr/响应体本身，脱敏在写入点之前完成。

### 4.3 轮转机制（任务书步骤 2）

- `RotatingLogFile`：`{home}/lingxi-service/logs/service-{seq:06}.log`，序号
  目录扫描发现（max+1），按字节预算轮转，按数量裁剪（保最新 max_files 个），
  文件 0600 / 目录 0700（显式 chmod，非 umask 运气）。默认 5MiB×7 个（镜像
  现役 5MB 上限语义，以「轮转保留最近有界窗口」替代「截断后静默丢弃」）。
  零新依赖（tracing-appender 不在锁内——见 §2.5）。
- 附接时机：**取得单写者锁之后**。依据：未持有 home 的进程不得写入 home
  （被拒第二实例零写入 → T02/A03「首实例数据未损坏」的树哈希契约保持）。
  锁前诊断仅 stderr（各证据脚本均捕获 stderr，无信息损失）。附接失败 =
  显式降级（`LINGXI_SERVICE_LOG_WRITE_FAILED` + error!，服务继续
  stderr-only），不静默、不拒启（镜像现役「写日志失败不阻塞业务」）。

### 4.4 上限参数与默认值（任务书步骤 3）

| 上限 | 默认 | CLI 旗标 | 超限行为 |
|---|---|---|---|
| WS 连接（=受管任务注册） | 16 | `--max-ws-connections` | 101 前 503 budget_exceeded（原因 ws_connection_limit）；任务仅在获得槽位后 spawn，注册表结构性有界 |
| DB 请求队列 | 64 | `--db-queue-bound` | **等待式背压**（T04 `submit` 语义：有界排队、不丢不假成功）；队列关闭/错误显式 503 |
| 订阅者信箱（事件缓存/订阅者） | 128 | `--event-subscriber-queue` | 关键事件溢出 → 显式 detach（snapshot_required, slow_consumer）+ error! 统计行；文字增量可计数丢弃 |
| reorder 缓冲（事件缓存） | 64 | `--event-reorder-bound` | 溢出 → 流标记 broken → 全体订阅者显式 detach（publication_gap） |
| 订阅者总数 / 单流 | 256 / 32 | `--max-subscribers` | subscribe 显式拒绝（`SubscriberLimit{scope,limit}`；HTTP 503 budget_exceeded / WS 1013 try_again_later） |
| hub 流注册表（事件缓存） | 4096 | —（防御性常量） | 仅驱逐「无订阅者且无 parked」的流（基线由 durable log 对齐无损重建）；无流可逐则该事件跳过 live 扇出并 error!（durable 权威不变） |
| per-peer HTTP 速率 | 240/10s | `--http-rate-max` | 429 rate_limited（retryable） |
| HTTP body | 1MiB | —（T03） | 413 |
| WS 票据注册表 | 512 | —（T03） | 显式拒绝 |
| 日志轮转 | 5MiB×7 | `--log-max-bytes` / `--log-max-files` | 超限轮转/裁剪（有界积累） |

全部旗标严格解析（重复/空/0/旗形值 → exit 2），USAGE 文档逐条同步。

### 4.5 时钟与 ID 注入面（任务书步骤 4）

- `ServiceClock`（`now_unix_ms`）：`SystemClock`（默认）/`ManualClock`
  （set/advance）。注入点 = `ServiceState`（`ServiceDeps.clock`），覆盖
  served 路径全部时钟读点：速率限流窗口、WS 票据签发/消费、execute 时间戳、
  会话 seed。启动期时间戳（auth bootstrap、实例身份）保持系统时钟（构造于
  ServiceState 之前），文档明示。
- `RequestIdGen`：`RandomRequestIdGen`（/dev/urandom，与 auth 密钥同一熵
  权威）/`SequentialRequestIdGen`（`req-0000000000000001` 递增——ID 序列本身
  即顺序证人）。
- 测试证明（零 sleep）：`manual_clock_drives_rate_limit_reset_without_sleeps`
  在真实 HTTP 路径上填满预算→429→`ManualClock::advance`→复位放行；
  `a13_error_bodies…` 断言请求 ID 逐请求递增（…0001/…0002/…0003）。

## 5. 关键调用链

```
请求：error_enrichment(铸 requestId→扩展) → transport_guard(Origin/Host/限流[注入时钟]
  /标记行[脱敏+request_id]) → auth_guard(票据/令牌[注入时钟]/授权/标记行) → handler
  → execute_for(StoragePort 同事务终态+事件 → publish_committed → EventHub[有界信箱/
  reorder/驱逐]) ；错误 → EndpointError(causeId) → enrichment(错误体 requestId+message 脱敏)
日志：tracing 事件 → LogRouter(LogSink 行缓冲) → redact_text → stderr ⊕ RotatingLogFile
  （锁后附接；超 5MiB 轮转；>7 个裁剪最旧；0600）
subscribe：EventService::subscribe → hub.try_register(总数/单流上限) → durable read →
  release_hold(cut) → live 推送；溢出 → detach_with_signal(snapshot_required)+error! 统计
关闭：SIGTERM → …（T06 原链不变）
```

## 6. 测试列表（层级）

1. **单元（lib 内，红先行）**：`redaction.rs` 17（A13 泄漏反例：Bearer/sk-/hana_dev_/
   hana_ws_/赋值/查询/Cookie/home 路径/用户路径/data:URI/长随机 token；关联 ID 存活；
   普通诊断逐字不变；标记行透传）；`logging.rs` 6（轮转无损、裁剪、重开续序、
   0600/0700、sink 脱敏、配置校验）；`inject.rs` 4（ManualClock、SystemClock 量纲、
   顺序 ID、随机 ID）；`config.rs` +2（8 旗标严格解析、非法值全拒绝）；`events.rs`
   +4（总数/单流上限显式拒绝、驱逐无损、风暴显式性——静默停更即失败）；`lib.rs`
   既有 9 个保持。
2. **集成（真实 router/TCP/runs.db，合成隔离 home）**：`resource_limits.rs` 5——
   错误体 causeId/requestId/无 home 泄漏/凭证不回显、404 cause、注入时钟限流
   零 sleep 复位、订阅者上限显式拒绝+槽位回收、events_page 503 映射。
3. **二进制级（真实进程）**：`r02_t07_redaction_scan.sh`（A13，§7.1）；
   `r02_t07_slow_subscriber.sh`（A14，§7.2）。

全量：`cargo test --workspace --locked --no-fail-fast` = **269 passed / 0 failed
（38 套件）**（T06 基线 231 + 本任务 38）。

## 7. 验收场景逐项结果（REQUIRED）

全部命令剥代理 + `CARGO_NET_OFFLINE=true` + `--locked` +
`CARGO_TARGET_DIR=/tmp/rust-target-r02-t07`；tested SHA `7a489cde…` + 未提交改动；
macOS 27.0 arm64；合成 /tmp home。证据：`artifacts/rust-tauri/R02/T07/`。

### 7.1 R02-A13 敏感值不出现在日志 — PASS（本执行者证据；独立验收归总控）

| # | 覆盖 | 命令 | 退出码 | 结果 | 证据 |
|---|---|---|---|---|---|
| A13-1 | 前置=合成样本（API key `sk-test-…`、OAuth bearer `a13-…`、设备密钥 `hana_dev_…`、查询 token `a13-…`；另含服务实铸 local-token 与 ws-ticket） | `bash scripts/rust-tauri/r02_t07_redaction_scan.sh artifacts/rust-tauri/R02/T07` | **0** | 构建离线锁定通过；全流量真实二进制产生 | `redaction-scan/summary.txt`、`build.log` |
| A13-2 | 正常请求：GET /me（Bearer 真实 local token）、POST execute（**输入体携带预设 API key+设备密钥**）、签发设备凭证+WS 票据 | 同上 | 0 | 200/201 全过 | `p1-me-ok.body`、`p1-execute-ok.body`、`p1-issue-credential.body`、`p1-ticket.body` |
| A13-3 | 错误请求：401（坏 bearer/缺失凭证/坏查询 token）、403（跨主体）、404（未知会话）、WS 拒绝（坏 bearer 升级 401；畸形首帧→invalid_message 帧+close） | 同上 | 0 | 全部按语义拒绝 | `p2-*.body/.headers`、`p2-ws-transcript.txt` |
| A13-4 | storage 错误：外部 SQLite 连接持 WAL 写锁超 busy 超时（真实 SQLITE_BUSY，无注入 mock） | 同上 | 0 | 1×503 `db_busy`（retryable=true），无假成功 | `p3-status-codes.txt`、`p3-burst-*.body`、`p3-lock.log` |
| A13-5 | epoch 拒启诊断：损坏印章 → exit 2 + marker | 同上 | 0 | LINGXI_DATA_EPOCH_* marker 在 stderr；诊断同样入扫描面 | `service-p4.err/.out` |
| A13-6 | **全量扫描**：日志文件+stderr/stdout 捕获+错误响应体+WS 转录（37 文件）对 6 个预设/实铸敏感值逐字 `grep -F` | 同上 | 0 | **6/6 值 0 命中**；扫描脚本可重跑；范围与排除面（4 个契约准许的交付/输入文件）在脚本头与 Python 排除表明示 | `summary.txt` SCAN 段 |
| A13-7 | 关联 ID 保留：requestId↔标记行、会话 ID↔日志、runId↔响应 | 同上 | 0 | 三向关联断言全过 | `summary.txt` CORRELATION 段 |

### 7.2 R02-A14 慢订阅者不拖垮服务 — PASS（本执行者证据；独立验收归总控）

| # | 覆盖 | 命令 | 退出码 | 结果 | 证据 |
|---|---|---|---|---|---|
| A14-1 | 前置=真实 WS 订阅者 A（SO_RCVBUF 4KB，读 subscribed 控制帧后停读）+ 健康订阅者 B（持续读）；操作=真实并发写者（EXECUTES=1000，24 线程并发）+ 确定性顺序量（SEQUENTIAL=1200）直至达到 `--event-subscriber-queue 4` 上限 | `bash scripts/rust-tauri/r02_t07_slow_subscriber.sh artifacts/rust-tauri/R02/T07` | **0** | 全部 2200 execute 200（写者零停滞） | `slow-subscriber/summary.txt`、`ws-probe.log`、`storm-results.json` |
| A14-2 | 明确断开/要求快照（非静默丢关键事件——T05 语义保持） | 同上 | 0 | A 收到显式 `snapshot_required`（reason=slow_consumer，缓冲帧 1437 先行送达）后同连接 resubscribe 成功；服务端 detach 观测行含真实队列统计（queue_capacity=4、dropped_deltas=0、last seq） | `ws-probe.log`、`detach-lines.txt`、`service.err` |
| A14-3 | 其他客户端不受损 | 同上 | 0 | B 合并视图==持久头（3384/3384 seq 连续，live-only 无需重建；以提交的 `storm-results.json` durable_key_events=3384 为准）；`GET /health` 风暴后 200 | `storm-results.json`、`ws-probe.log` |
| A14-4 | 内存有界（可观测证据：真实 RSS 采样，非手填） | 同上 | 0 | 200ms 采样贯穿风暴：first 11,136 KB / peak 17,040 KB / final 17,040 KB（远低于 512MB 断言上限，风暴后零增长；以提交的 `storm-results.json`/`rss-samples.csv` 147 样本为准） | `rss-samples.csv`（147 样本）、`storm-results.json` |
| A14-5 | 负载与队列监控（真实采样） | 同上 | 0 | 服务端 detach 行（error! 级）携带 capacity/dropped_deltas/last_seq 实测值；hub 级 `hub_stats()` 单测覆盖 | `detach-lines.txt`、events.rs 单测 |

### 7.3 门禁与回归（全部亲手执行）

| 门禁 | 命令 | 退出码 | 证据 |
|---|---|---|---|
| fmt | `cargo fmt --all -- --check`（锁定 1.98.1） | 0 | 终端执行（无 diff 输出） |
| clippy | `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0（0 error） | 终端执行 |
| 全量测试 | `cargo test --workspace --locked --no-fail-fast` | 0（**269 passed / 0 failed，38 套件**） | `gates/cargo-test-workspace.log` |
| T01–T06 回归 10 脚本 | smoke/boundary/dual-instance/f01-env-token/path-priority/auth-matrix/storage-tx/events-matrix/backup-restore/recovery-drill（证据定向 `gates/reg-*/`，T01–T06 已提交证据零覆写） | 0 ×10 | `gates/reg-*.log` + `gates/reg-*/` |
| 冻结契约 | `scripts/rust-tauri/r01-t02-check-generated.sh` | 0 | `gates/generated-contracts-check.log` |
| 依赖/所有权 | `python3 -B docs/rust-tauri/R01/r01_t01_check_ownership.py` | 0（RESULT: OK） | `gates/ownership-check.log` |
| 审计封印族 | `npm test -- --run tests/post-verification-audit-seal.test.ts` | 1（**1 failed / 2 passed，预存在红**：seal 坐标落后于已授权 R02 提交；未触碰坐标/白名单） | `gates/audit-seal.log` |
| Cargo.lock | `git diff --numstat rust/Cargo.lock` | — | **空（零 diff，零新增依赖）** |
| Node 红线 | `git diff 7a489cde --name-only -- core/ server/ desktop/ shared/ tests/` | — | 空（零改动） |

## 8. 未验证内容 / 环境限制

1. 跨平台：全部证据仅 macOS arm64。0600/0700 权限、SO_RCVBUF 行为（仅用于
   A14 演练的减速手段，非产品代码）等 unix 设施在 Windows 的等价未做
   （同 T01–T06 平台边界；logging.rs 非 unix 下权限收紧为编译期 no-op 路径
   ——`#[cfg(unix)]`）。
2. A14 的并发写者存在同毫秒 run-id 幂等重放（T04 `record_run_started` 的
   契约语义），事件总数非固定 → 断言以「实测持久头」为基准而非预设数；
   并发量与确定性量分两阶段提供。
3. A14 的 A 订阅者 detach 触发依赖 TCP 管道充满（macOS 环球回环 sndbuf
   autotune ~445KB 实测）；故设置确定性顺序量阶段（1200 executes）。不同
   OS/内核缓冲配置下所需量不同，脚本以「实测持久头 + 显式 detach 断言」
   收口，不依赖固定事件数。
4. 日志轮转的运行期真实轮转在二进制级由单元测试覆盖（`rotation_respects…`）；
   二进制级长跑（>5MiB 单文件）未演练（语义同函数级，默认 5MiB×7）。
5. 审计封印 1 红为预存在坐标问题；本任务未提交，tested SHA 为
   「基线 + 未提交改动」。

## 9. 已知风险

1. **健康探针不记每请求日志行**（设计决定，§5.1）：peer-probe/健康检查不再
   产生 `request handled` 行（否则每次单写者碰撞检查都会改变 probee 的 home
   树——T02/A03 树哈希契约）。错误响应体 enrichment 不受影响；若后续需要
   健康探针审计线，须显式决策并同步 T02 契约。
2. **脱敏器与现役正则的分歧与省略面**（F02 补记后的精确边界，§4.2）：为保住
   T02「安全日志显示有效路径」与 `request_id=` 关联诊断，`/`、`=` 不进本侧
   token 类。真实漏网条件（对拍实测）是"≥40 字符候选被 `/`/`=` 切成 <40 段
   或与 `.`/`/`/`-` 相邻"——hex64/base64url（无 `/`/`=`）实测被捕获；本栈
   不存在该形态密钥，**R05 引入真实 provider token 前必须复核此边界**；
   现役 PII/CLI 旗标/aws-configure/Windows 路径规则未镜像（当前服务不打印
   该类内容）。A13 全量扫描为最终把关。
3. `--http-rate-max` 放开即失去速率保护：A14 演练用 100000 仅限合成环境；
   生产默认 240/10s 不变（USAGE 明示）。
4. hub 流注册表上限（4096）触发「无流可逐」时会跳过该事件的 live 扇出
   （error! + durable 权威兜底 + 客户端快照重建）。R02 规模不可达；R03+ 引入
   在线请求后按负载复核。
5. 日志文件在 home 树内（`{home}/lingxi-service/logs/`）：任何对 home 做全树
   哈希的**未来**脚本需知悉该诊断面（T02 现有脚本不受影响——其实例内写入
   仅发生在自身运行期，其断言窗口内无写入）。已由 dual-instance 回归实证。
6. **`Mailbox::recv` 前向稳健加固（F01 勘误后的定性）**：`Notified::enable()`
   前置属对 T05 交付的**加固**而非缺陷级修复——R02-T07 REVIEW_R1 F01 经
   源码级（tokio 1.53.1 `notify_waiters_calls` 计数补捕）、确定性差分（窗口内
   publish 新旧实现均交付）与压力级（40 轮零停滞）三重验证裁定：锁定工具链下
   基线 `recv()` 不存在所声称的丢失唤醒窗口，原"二进制级 A14 演练复现"的叙述
   系误诊（执行者开发期观测疑似与 reorder 溢出的显式 PublicationGap detach
   混淆）。加固代码语义严格不弱于基线、无新唤醒问题，全部 events 测试与
   events-matrix 回归绿，按审阅要求保留；后续阶段判断 tokio `Notify` 语义时
   以本勘误为准。

## 10. git status --short 全文（报告时点）

```
 M docs/rust-tauri/ORCHESTRATOR_PROGRESS.json
 M rust/crates/lingxi-service/src/auth.rs
 M rust/crates/lingxi-service/src/config.rs
 M rust/crates/lingxi-service/src/events.rs
 M rust/crates/lingxi-service/src/lib.rs
 M rust/crates/lingxi-service/src/main.rs
 M rust/crates/lingxi-service/src/ws.rs
?? artifacts/rust-tauri/R02/T07/
?? rust/crates/lingxi-service/src/inject.rs
?? rust/crates/lingxi-service/src/logging.rs
?? rust/crates/lingxi-service/src/redaction.rs
?? rust/crates/lingxi-service/tests/resource_limits.rs
?? scripts/rust-tauri/r02_t07_redaction_scan.sh
?? scripts/rust-tauri/r02_t07_slow_subscriber.sh
```
（`docs/rust-tauri/R02/R02-T07_REPORT.md` 为本文件，亦为未跟踪新文件；
`ORCHESTRATOR_PROGRESS.json` 的 reviewer_id 笔误修正属总控账本维护，
本任务未触碰。）

## 11. 推荐独立验收重点

1. **A13 复跑 + 动手**：`bash scripts/rust-tauri/r02_t07_redaction_scan.sh
   /tmp/任意目录`——核对 summary 的 SCAN 6/6 clean 与 CORRELATION 三向断言；
   抽查 `p3-burst-*.body`（503 db_busy, retryable=true）、`p2-ws-transcript.txt`；
   脚本头部的扫描范围/排除面声明是否可接受。
2. **A14 复跑 + 动手**：`bash scripts/rust-tauri/r02_t07_slow_subscriber.sh
   /tmp/任意目录`——核对 B 合并视图==持久头、A 的 snapshot_required(reason=
   slow_consumer)+resubscribe、`detach-lines.txt` 的真实队列统计、
   `rss-samples.csv` 采样连续性；对 `storm-results.json` 逐字段核对。
3. **Mailbox 加固定性勘误（T05 交付面）**：审 `events.rs` `Mailbox::recv` 的
   `Notified::enable()` 论证——注意 R02-T07 REVIEW_R1 F01 已裁定基线在锁定
   tokio 1.53.1 下无丢失唤醒缺陷，本改动定性为前向稳健加固（非缺陷修复）；
   复跑 `cargo test -p lingxi-service`（events 18 用例）+
   `r02_t05_events_matrix.sh`；可尝试在旧实现上运行
   `hub_storm_is_explicit_at_every_subscriber`/A14 演练以复核加固的无损性。
4. **causeId/requestId 面**：`cargo test -p lingxi-service --test
   resource_limits`；任一 401 响应体核对 `details.causeId/requestId` 与
   stderr 标记行的 `request_id=` 对得上。
5. **脱敏器分歧裁定**：redaction.rs 模块文档记载的 `/`、`=` 排除与
   `hana_*` 前缀补齐是否符合「镜像现役语义」的验收口径（A13 扫描为最终
   把关；17 个反例/存活测试钉边）。
6. **上限旗标**：`--help` 输出与 config.rs/USAGE 一致性；`--db-queue-bound`
   的等待式背压语义（T04 submit）与「超限明确 backpressure/rejected」的
   对应关系是否按 T04 契约接受。
7. 门禁复跑：fmt/clippy/269 测试/契约/所有权/10 回归脚本；封印 1 红为
   预存在坐标问题。

— 执行者声明：以上命令、退出码、证据路径均来自本机真实执行；未执行项已
如实列入 §8；未 commit/push。
READY_FOR_REVIEW。
