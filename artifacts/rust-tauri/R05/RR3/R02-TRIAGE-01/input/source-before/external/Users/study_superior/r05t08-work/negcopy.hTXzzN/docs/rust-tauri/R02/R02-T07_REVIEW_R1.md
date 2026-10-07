# R02-T07｜日志、错误和有界资源 — 独立对抗性验收报告（R1）

- 审阅者：REVIEWER-R02-T07-R1（独立验收代理；未参与实现；不信任执行者声明，全部证据亲自重跑/重读）
- 日期：2026-09-26
- 审阅对象：TASK_BASE_SHA `7a489cded575f1413ec29ea5ecec565528f16728` + 未提交工作树
  （排除 `docs/rust-tauri/ORCHESTRATOR_PROGRESS.json`——已核实该文件 diff 仅为总控
  reviewer_id 笔误修正，与本任务候选无关）
- 平台：macOS 27.0 arm64（Darwin 27.0.0）；rustup 1.29.1 + 锁定 rustc/cargo 1.98.1
  （`rust-toolchain.toml`，亲核）；cargo 全程 `--locked` + `CARGO_NET_OFFLINE=true` +
  专属 `CARGO_TARGET_DIR=/tmp/rust-target-r02-t07-review`；网络命令剥代理
- 判定依据：任务书 `R02_…md` §4 R02-T07、task-catalog `R02-T07`、acceptance-catalog
  `R02-A13`/`R02-A14`（均 REQUIRED）、01/02/05 共同必读、T01–T06 报告与既有 REVIEW 契约
- 结论先行：**VERDICT: PASS**（0 BLOCKING / 4 MINOR；Mailbox 修复经独立验证代码正确、
  无新问题，但其"缺陷"定性系误诊，见 F01——报告叙述必须修正，代码可保留）

---

## 1. 验证命令清单及退出码（全部亲跑）

| # | 命令（关键参数：剥代理 + `CARGO_NET_OFFLINE=true` + `--locked` + 专属 target dir） | 退出码 |
|---|---|---|
| V1 | `cargo build --manifest-path rust/Cargo.toml --locked -p lingxi-service` | 0 |
| V2 | `cargo fmt --manifest-path rust/Cargo.toml --all -- --check` | 0 |
| V3 | `cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings` | 0 |
| V4 | `cargo test --manifest-path rust/Cargo.toml --workspace --locked --no-fail-fast` | 0（**38 套件 / 269 passed / 0 failed**，与声称一致） |
| V5 | `bash scripts/rust-tauri/r02_t07_redaction_scan.sh /tmp/r02t07-review-a13` | 0（37 证据文件 × 6 敏感值 0 命中；CORRELATION 三向 PASS） |
| V6 | `bash scripts/rust-tauri/r02_t07_slow_subscriber.sh /tmp/r02t07-review-a14` | 0（B 合并视图 3414/3414；A 显式 `snapshot_required(slow_consumer)` + 同连接 resubscribe；RSS peak 17136 KB / ~146 样本 / health 200） |
| V7 | T01–T06 回归 10 脚本（证据定向 `/tmp/r02t07-review-reg/`）：service_smoke、boundary_negative、dual_instance、f01_env_token_negative、path_priority、auth_matrix、storage_tx、events_matrix（另加 3 轮）、backup_restore、recovery_drill | 全部 0 |
| V8 | `bash scripts/rust-tauri/r01-t02-check-generated.sh` | 0（624 entries，drift-free） |
| V9 | `python3 -B docs/rust-tauri/R01/r01_t01_check_ownership.py` | 0（RESULT: OK） |
| V10 | `npm test -- --run tests/post-verification-audit-seal.test.ts` | 1（**预存在红**：1 failed / 2 passed；失败清单 = `.gitignore` + 已提交的 R01/R02 T01–T06 交付物 + `r02_t06_recovery_drill.sh`，**零 T07 文件**；封印坐标 `ab4f2281` 落后于已授权 R02 提交。未触碰坐标/白名单，如实记录） |
| V11 | `git diff 7a489cded --name-only -- core/ server/ desktop/ shared/ tests/ contracts/ .sync-audit/ PROGRESS.md rust/Cargo.lock` | 空（零改动；Cargo.lock 零 diff，零新增依赖） |
| V12 | `git status --porcelain artifacts/rust-tauri/R02/`（滤除 T07 后） | 空（T01–T06 已提交证据零覆写） |
| V13 | 二进制负向/边界矩阵（见 §4 对抗项） | 全部符合预期 |

测试有效性核验：全量 diff 中**零既有测试删除/弱化**（逐删除行核对：仅 `register`
签名改可化、常量改具名、注释更新；新增测试均为增量）；`resource_limits.rs`/`inject.rs`
零 `sleep`（grep 证实，仅测试名含 "sleep" 字样）；注入时钟真实驱动速率限流（429→
`ManualClock::advance(1000)`→复位，零 sleep，实测路径）。

---

## 2. Acceptance 独立判定

### R02-A13｜敏感值不出现在日志 — **PASS**（独立复跑 + 对抗）

- V5 亲自复跑：exit 0；37 个证据文件对 6 个预设/实铸敏感值（API key、OAuth bearer、
  设备密钥、查询 token、服务实铸 local-token、ws-ticket）逐字扫描 0 命中；
  关联 ID 三向对账 PASS（错误体 `details.requestId` ↔ stderr 标记行 `request_id=` ↔
  日志中的 session id / runId）。
- 全路径覆盖核实：正常（200/201）、401×3（坏 bearer/缺凭证/坏查询 token）、403
  （跨主体）、404、WS 拒绝（升级 401 + 畸形首帧→invalid_message 帧+close）、
  storage 503（外部 SQLite 持 WAL 写锁 → 真实 `db_busy`、`retryable:true`，无 mock）、
  epoch 损坏拒启（exit 2 + marker）。诊断面 = 轮转日志文件 + stderr/stdout 捕获 +
  错误响应体 + WS 转录；扫描排除面（4 个契约准许的请求/交付文件 + summary.txt）
  在脚本头与 Python 排除表明示，属契约准许的一次性秘密交付面，判定可接受。
- 对抗变体（V13，自铸执行者未列形态，全部 /tmp 合成）：
  - Origin 头携带 `?token=<64hex>`（secret 键名）→ 标记行 `[redacted]`，值零残留；
  - Origin 头携带 `?state=<64hex>`（非 secret 键名）→ 长随机规则捕获为 `[token]`，
    `state=` 键名保留（比现役改写 `?[token]` 更保结构——与现役 node 实测对齐）；
  - 伪造 cursor 内嵌 `sess_<64hex>` 流 ID → 400 响应体 message 经脱敏器重写为
    `stream "[token]"`（安全方向牺牲了关联可读性，未泄露）；
  - Host 头走私 token（`evil-<64hex>.example:1`）→ **两边都不脱敏**（Rust 与现役
    node `log-redactor.cjs` 实测同行为，见 F02c——镜像忠实，非回归）；
  - `?token=<36hex>` 触发查询+赋值双规则 → 输出 `[redacted]]` 双括号伪影 ——
    **现役同样存在**（node N6 实测逐字一致），纯外观、零泄露（F02c）。
- 判定：**真实 PASS**。

### R02-A14｜慢订阅者不拖垮服务 — **PASS**（独立复跑 + 对抗）

- V6 亲自复跑：exit 0。真实 WS 订阅者 A（SO_RCVBUF 4KB，读 subscribed 后停读）+
  健康订阅者 B + 1000 并发（24 批）+ 1400 顺序真实 HTTP 写者：
  - 全部 execute 200（写者零停滞）；
  - A 收到显式 `snapshot_required(reason=slow_consumer)`（缓冲帧 1437 先行送达），
    同连接 resubscribe 成功——T05 语义（关键事件不静默丢）保持；
  - B 合并视图 == 持久头（3414/3414 连续，live-only 无需重建）；风暴后 health 200；
  - 服务端 detach 观测行（error! 级）携带真实队列统计
    `queue_capacity=4 dropped_deltas=0 last_enqueued_seq=1437`——非手填；
  - `rss-samples.csv` 为探针进程内 `ps` 真实采样（0.2s 间隔，~146 样本 ≈ 风暴时长
    29s，量纲/间隔与进程生命周期匹配），peak 17136 KB ≪ 512MB 断言上限，风暴后
    零增长。
- 对抗变体（V13）：
  - **双慢订阅者同时**（S1、S2 均 rcvbuf=4096 停读 + 健康者 H + 600 并发 + 1400
    顺序）：S1、S2 **各自**收到显式 `slow_consumer` detach（service.err 两条独立
    detach 行），H 3390/3390 完整，health 200 —— 上限与 detach 路径在多受害并
    存下不互扰；
  - **慢订阅者 + 默认限流叠加**（不放开 `--http-rate-max`）：300 并发请求直击
    默认 240/10s 预算 → 精确 `240×200 + 60×429`，429 体带
    `causeId=transport.rate_limited`，窗口过后恢复 200，health 200 —— 两类上限
    组合无死锁、无静默；
  - **订阅者上限显式拒绝（WS 1013）**：`--max-ws-connections 64 --max-subscribers
    64`（per-stream 默认 32），35 个连接全部保持打开 → 第 33–35 个订阅收控制帧
    后 **close code = 1013**（`WS_CLOSE_TRY_AGAIN_LATER`），前 32 个不受损 ——
    HTTP 503 映射另有单测，WS 侧实测命中；
  - 首轮对抗（事件量 1074 < 回环管道容量）未触发 detach——据此确认 detach 依赖
    "TCP 管道充满"这一前提真实存在（与执行者 §8.3 披露一致），并反向证明该演练
    不能以小流量伪造 PASS。
- 判定：**真实 PASS**。

---

## 3. Mailbox `enable()` 修复独立结论（重点审查项）

**结论：修复代码正确、无新唤醒问题、非行为放宽，可保留；但其声称的"T05 继承缺陷"
在本仓锁定的 tokio 1.53.1 下不成立——属误诊（F01）。**

独立验证过程（scratch 双副本：基线 `7a489cde` vs 工作树，仅 /tmp，未触碰仓库）：

1. **源码级**：tokio 1.53.1 `src/sync/notify.rs` `NotifiedProject::poll_notified`
   的 `State::Init` 分支在首次 poll 时检查
   `get_num_notify_waiters_calls(curr) != *notify_waiters_calls`（future 创建时的
   调用计数快照）——**任何发生在"future 创建之后、首次 poll 之前"的
   `notify_waiters()` 都会在首次 poll 被补捕并立即 Ready**，无需注册 waiter。
   因此基线 `recv()`（`notified()` → `try_recv` → `await`）在该版本语义下：
   push 落在 try_recv 之前 → try_recv 可见；落在 try_recv 与首 poll 之间 → 计数
   补捕；落在了注册之后 → 唤醒。三种时序全覆盖，无丢失唤醒窗口。基线代码的注释
   "Register interest BEFORE checking so a notify racing … cannot be lost" 对
   tokio 1.53.1 而言是正确的（其机制是计数快照而非提前注册）。
2. **实验级（确定性差分）**：在两份 scratch 副本的 `recv()` 内部
   （try_recv 之后、await 之前）插入 `yield_now()`，把理论抢占窗口**确定性实现**，
   并以探针在该窗口内执行 `publish + notify_waiters`（插桩日志证实 publish 确实
   落在窗口内）：旧实现 **交付成功**（计数补捕生效，消费 0ms 收到帧）；新实现
   同样交付。若缺陷如所述存在，旧实现应在此探针下永久沉睡。
3. **压力级**：40 轮 × 8 写者 × 60 事件（多线程 runtime、容量 4096、reorder 上限
   放大以排除 detach 混淆）对两副本各跑：新旧均 0 停滞。注：首轮探针（reorder
   上限取默认 64）在**新旧副本都**出现"消费者停止收帧"，逐一归因后全部是
   reorder 溢出的**显式** `PublicationGap` detach（有日志、有控制帧）——这提示
   执行者在开发期观察到的"静默停更"很可能是同类混淆或中间态代码，其 §2.6 的
   归因未被本次任何实验复现。
4. **修复本身**：`enable()` 先于空检查 = tokio 官方文档的 canonical 模式
   （1.53.1 notify.rs 文档示例与该 `recv` 逐行同构）；spurious wakeup 由循环
   重检处理；enable 后 return 前丢弃 waiter 无副作用（notify_waiters 不存许可）。
   全部 18 个 events 单测 + 3 轮 events_matrix + 269 全量测试绿。**无新问题、
   无断言放宽**；且该写法对更旧 tokio 语义前向稳健，作为加固可以保留。

---

## 4. 对抗变体结果汇总（执行者未列的自铸形态）

| 变体 | 结果 | 定性 |
|---|---|---|
| 双慢订阅者同时 + 风暴 | 两者各自显式 `slow_consumer` detach，健康者/服务无损 | 支持 PASS |
| 慢订阅者 + 默认限流叠加 | 240/10s 精确生效（240×200+60×429，causeId 正确），窗口后恢复 | 支持 PASS |
| WS 订阅者上限（保持 35 连接） | 第 33–35 个 **close=1013**，前 32 完好 | 支持 PASS |
| Origin 携带 secret 键名查询 token | `[redacted]`，零残留 | 支持 PASS |
| Origin 携带非 secret 键名 64hex | `[token]`，键名保留 | 支持 PASS |
| 伪造 cursor 回显 64hex 流 ID | 响应体 message 被脱敏为 `"[token]"` | 支持 PASS |
| Host 头内嵌 token（`evil-<hex>.example`） | 两边都不脱敏（现役 node 实测同） | 镜像忠实（F02c） |
| 标准 base64 含字面 `/`（≥40、两侧段<40） | **Rust 漏网，现役捕获** | 已记载分歧的实证边界（F02a） |
| base64url+`=` padding、纯 hex 64 字符 | 捕获为 `[token]` | 支持 PASS |
| `?token=` 查询+赋值双规则 | `[redacted]]`（现役逐字同） | 外观伪影（F02c） |
| 小流量慢订阅者（未达管道容量） | 无 detach（符合设计前提） | 反向证明演练不可伪造 |

---

## 5. Findings

### F01｜MINOR — "Mailbox 丢失唤醒缺陷"定性系误诊；报告叙述必须修正
- **位置**：`docs/rust-tauri/R02/R02-T07_REPORT.md` §2.6、§3（events.rs 修改说明）、
  §9.6、§11.3；代码 `rust/crates/lingxi-service/src/events.rs` `Mailbox::recv`。
- **证据**：§3 的源码级 + 确定性差分 + 压力级三重独立验证（tokio 1.53.1
  `notify_waiters_calls` 计数补捕；窗口内 publish 两版均交付；40 轮压力零停滞）。
- **问题**：报告把该改动叙述为"二进制级 A14 演练复现的 T05 继承缺陷级修复"，但
  在锁定工具链（tokio 1.53.1，`--locked`）下基线 `recv()` 不存在所声称的丢失唤醒
  窗口；执行者描述的复现（健康订阅者静默停更、无 detach、无日志）在基线代码上
  不可复现，且与本审在默认 reorder 上限下观察到的**显式** PublicationGap detach
  现象高度疑似混淆。
- **为何仍非 BLOCKING**：修复代码是 tokio 官方 canonical 模式、语义严格不弱于
  基线、无新唤醒问题、零断言放宽、全部测试与回归绿；PASS 标准 3 要求的"修复
  成立且无新问题"在代码层面成立。
- **后果（若不修正）**：账本把一个不存在的缺陷记为已修复的继承缺陷，误导后续
  阶段对 tokio `Notify` 语义的判断与同类路径排查。
- **根因**：以通用 tokio 文档警示（旧版本 Notified 首 poll 注册）替代了对锁定
  版本源码语义的核对；开发期一次未归因的观测被顺势归因。
- **同类路径**：报告 §2/§9 中其它"缺陷修复"叙述（无）——本项孤立。
- **修复要求**：修改 `R02-T07_REPORT.md` 相应叙述（改为"前向稳健加固 + 原注释
  机制澄清"，删除"缺陷级修复/已复现"措辞或附上可复现证据）；代码无需变更。
- **必须重跑**：`cargo test -p lingxi-service`（events 用例）+ `r02_t05_events_
  matrix.sh` 一轮（确认纯文档改动后仍绿）。

### F02｜MINOR — 脱敏器与现役的分歧面记载不全，"镜像现役语义"表述过强
- **位置**：`rust/crates/lingxi-service/src/redaction.rs`（模块文档）、
  `R02-T07_REPORT.md` §4.2/§9.2。
- **证据**：Rust 探针（redaction.rs 测试模块内逐规则隔离）+ 现役
  `shared/log-redactor.cjs` node 实测，同输入对拍：
  - (a) **已记载、已实证、方向安全**：含字面 `/` 的标准 base64（≥40 字符、按
    `/`/`=` 切分后两侧段均 <40）在 Rust 长随机规则下漏网，现役替换为 `[token]`。
    本栈自铸凭证（base64url 无填充/hex、`hana_dev_`/`hana_ws_` 前缀）不含 `/`，
    R02 范围内不可达；**R05 接入外部 provider token 前必须复核此边界**。
  - (b) **未记载的省略**：现役 PII 规则（email/credit-card/CN-ID/SSN）、
    `CLI_SECRET_FLAG_RE`（空格分隔旗标值）、`CONFIG_SECRET_VALUE_RE`（aws
    configure）、Windows 用户路径（`C:\Users\`）未镜像。当前 Rust 服务不打印
    CLI 参数/正文/PII，现实影响为零，但"17 个反例钉边 + 镜像"的说法掩盖了这些
    省略。
  - (c) **实测一致（非分歧）**：主机名内嵌 token 两边都不脱敏；`[redacted]]`
    双括号伪影两边逐字一致（赋值规则的值终止集把先前规则的 `]` 当终止符所致，
    纯外观）；非 secret 查询键 Rust 保留 `state=[token]` 而现役改写 `?[token]`
    （Rust 更保结构，安全方向）。
  - 另：§9.2 的漏网必要条件表述不准确——"无 / 无 . 无 = 的 ≥40 字符串"实测恰恰
    被捕获（hex64 → `[token]`）；真实漏网条件是"含 `/`/`=` 分裂为 <40 段"或
    "与 `.`/`/`/`-` 相邻"。
- **后果**：低——A13 扫描针对本栈形态全部成立；但 R05 引入真实 provider 凭证
  流时，(a)/(b) 是现成的泄露面清单，不记载则必被遗忘。
- **修复要求**：redaction.rs 模块文档与报告 §4.2 补记 (b) 项省略与 (a) 项精确
  边界（含本次对拍实证），并把 (a) 列入 R05 交接风险；不必在本任务补实现。
- **必须重跑**：`cargo test -p lingxi-service redaction`（纯文档改动）。

### F03｜MINOR — 报告引用数字与提交证据漂移（多次运行的残留）
- **位置**：`R02-T07_REPORT.md` §7.2/§7.3 vs `artifacts/rust-tauri/R02/T07/`。
- **证据**：报告 A14-3 写 3372/3372、A14-4 写"174 样本 / first 11008 / peak
  16080"，而提交的 `storm-results.json`/`rss-samples.csv`（第二次运行覆盖后）
  为 3384/3384、147 样本、11136/17040；本审复跑为 3414/3414、~146 样本、17136。
  三次均 PASS、结论不变。同类：§7.2 表头与脚本 note 仍写 "400 concurrent
  executes"（实际 `EXECUTES=1000`，断言用变量故证明不受影响）；§7.3 行首
  "9 脚本"与"0 ×10"自相矛盾；`slow-subscriber/summary.txt` 两次运行输出追加
  堆叠未注明。
- **后果**：证据-报告一致性受损（审阅者需自行分辨哪次运行是权威）；不构成
  PASS 伪造（所有断言相对实测持久头收口）。
- **修复要求**：报告数字对齐提交的最终证据文件；脚本 note 文案改用变量或删除
  硬编码数量；summary 重复运行标注轮次。
- **必须重跑**：无需重跑行为，仅文档/文案对齐。

### F04｜MINOR — `--log-max-bytes` 约束文本失实；日志旗标范围违例走降级而非 exit 2
- **位置**：`rust/crates/lingxi-service/src/config.rs`（constraint 字符串）、
  `logging.rs::LogRotationConfig::validate`（下限 64）、`main.rs`（attach 失败=
  显式降级）、USAGE。
- **证据**：`--log-max-bytes 100` 被接受并生效（100 字节预算成功附接轮转），
  而 constraint 文本写 "(>= 65536)"；`--log-max-files 1`、`--log-max-bytes 32`
  解析通过、在 attach 期校验失败 → `LINGXI_SERVICE_LOG_WRITE_FAILED` 标注 +
  stderr-only 继续（exit 0），与其它 7 个上限旗标"解析期 exit 2"路径不一致；
  USAGE 未记载任何下限。其余旗标负向矩阵实测全部正确：0/非整数 → InvalidLimit
  exit 2；重复/旗形/缺值 → 响亮拒绝；`--event-subscriber-queue 1` → bootstrap
  期 exit 2（"must be >= 2"）。
- **后果**：配置错误的响亮程度不一致，constraint 文本误导（宣称的下限从未被
  强制）。属文档/校验一致性缺陷，非安全或资源无界问题（降级有显式标注）。
- **修复要求**：二选一并统一——(i) 解析期强制 `[64, ∞)`/`≥2` 并 exit 2；
  (ii) 修正 constraint 文本与 USAGE，明示"范围违例=显式降级"。
- **必须重跑**：`config.rs` 单测 + `--help`/负向旗标矩阵一轮。

### 记录性 NON-ISSUE（不列编号缺陷）
- 轮转文件数受控实测恰好 = `max_files`（3/3，200 请求压力后）；文件 0600、目录
  0700（stat 实测 100600/40700）；被拒第二实例零写 home 由 dual-instance 回归
  再证；READY 行仅 stdout 一次（T01/T02 机器契约保持，10 个回归脚本全绿背书）；
  错误码为包装非重写（status/reason/retryable 全保留、`ProtocolError` 结构零
  改动、`check-generated` 624 entries 零漂移、`error_details` 保留既有 `reason`
  键）；层序 `error_enrichment → transport_guard → auth_guard` 与 build_router
  逐层核对一致（enrichment 最外层，仅 4xx/5xx 缓冲重写，200 不缓冲）；日志
  file 分支在单写者锁之后附接（epoch 拒启实例持锁属合法写入，与"被拒实例零
  写入"承诺不冲突）；health 探针不记完成行的决策有 T02 树哈希契约依据且已在
  §9.1 披露；无提前 T08（xtask 不存在）、无隐藏删除；scope 与报告 §10 的
  git status 逐项吻合。

---

## 6. PASS 标准逐条核对

1. R02-A13/A14 真实 PASS（亲跑 + 对抗）——✅（§2）。
2. 生产路径真实接通——✅：LogRouter 挂接全局 subscriber（main.rs）、脱敏四层
   应用点（tracing 行/LINGXI_* 标记行/错误体 message/READY 行契约豁免）、8 个
   上限旗标从 CLI→ServiceDeps→bootstrap_with_deps→各限流点全链实测生效
   （429/503/1013/detach/轮转逐一命中），无"仅函数存在"式接线。
3. 无 BLOCKING（Mailbox 修复经独立验证成立且无新问题）——✅（F01 定性为误诊
   但代码正确无害；报告修正为放行条件，非代码阻塞）。
4. 无测试篡改——✅（零删除/零弱化，全量 diff 逐删除行核对）。
5. 无未解释脱敏/资源缺口——✅（仅剩已记载且方向安全的分歧边界，F02 补记）。
6. 回归绿——✅（T01–T06 十脚本 + 契约 + 所有权全绿；审计封印 1 红为预存在
   坐标落后，失败清单零 T07 文件，如实记录、未触碰）。
7. 证据与候选一致——✅（执行者 gates 日志与亲跑结果一致；报告数字漂移见 F03，
   不影响判定）。

## VERDICT: PASS

放行条件（不阻塞本判定，但须在 R02 阶段收口前完成）：F01 报告叙述修正；
F02 分歧补记并入 R05 交接；F03 数字对齐；F04 旗标校验一致性二选一。
本判定不授予 commit/push/发布/封印坐标推进权限；tested SHA 为基线 + 未提交
工作树，最终候选须在授权提交后按 05 协议绑定最终 SHA 重跑关键门禁。

---

## 附：审阅方法与限制

- 所有结论基于本机亲跑命令与亲读源码；scratch 双副本（`git archive 7a489cde`
  与工作树拷贝，均在 /tmp）用于 Mailbox 差分与脱敏规则隔离，探针只写入 /tmp，
  未触碰仓库产品源码/测试/配置。
- 限制：跨平台（Windows 权限/SO_RCVBUF/轮转）未验证（同执行者 §8.1 披露）；
  二进制级 >5MiB 单文件长跑轮转未演练（函数级已钉）；执行者开发期的"复现"
  观测无法事后重建，仅能以基线代码的不可复现性作为结论依据。
