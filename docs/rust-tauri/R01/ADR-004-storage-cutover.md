# ADR-004｜存储切换与旧版本拒写（R01-T07）

- 状态：READY_FOR_REVIEW（执行者结论；独立验收归总控另派代理）
- 日期：2026-09-25
- 执行基线：分支 `codex/rust-tauri-migration`，HEAD `82870879d02b23a03c65fcf6cb9fb03b1425012b`
- 作者：ZCode:R01-T07-exec-r1
- 修订：2026-09-25 ZCode:R01-T07-repair-r1 按独立验收 `R01-T07_REVIEW_R1.md`（FAIL）修复
  F1/F2/F3——§2 闸软化边界陈述改为实测边界并补登 R7/R9/R10/R11 变体、D2/§5 补登印章篡改
  窗口实证与 shell-local 共享根争抢风险（移交 R09）、§5 新增生产缺陷立项条目。
  修复过程与重跑证据见 `R01-T07_REPAIR_R1.md`、`artifacts/rust-tauri/R01/T07/repair-r1/`
- 配套机器产物：`DATA_COMPATIBILITY_MATRIX.json`（69 存储逐项，生成器 `r01_t07_build_matrix.py`）、`ROLLBACK_DESIGN.md`
- 实测证据：`artifacts/rust-tauri/R01/T07/`（探针 A1–A9 日志、文件系统前后快照、回滚演练输出）

## 1. 问题

任务书 02 §8 要求：新内核运行/规范化消息存储由 Rust 拥有；知识、记忆、配置、附件先保留原格式；
不能同时保留两个消息权威；单写者不能只靠新程序自觉——必须用旧版本实际理解的 epoch 拒写，
或可验证的分离数据根；恢复旧备份不等于无损回滚。本 ADR 在写任何新存储代码之前冻结这些决策。

## 2. 基线事实（源码复核 + 实测）

现役 epoch 机制（全部在当前 HEAD 源码中逐行复核）：

- `shared/contract-versions.json`：`DATA_EPOCH=1`；`shared/data-epoch.cjs` 定义印章
  `data-epoch.json`（schema v2：`epoch`/`minimumReaderEpoch`/`committedDataEpoch`，兼容 legacy v1
  `{epoch}`）与过渡日志 `data-epoch-transition.json`（七阶段 prepared→…→committed）。
- `core/data-epoch-coordinator.ts`：`coordinateDataEpochStartup` 实现「低位内核拒绝打开高位
  epoch 目录」（`epoch-downgrade-blocked`）、半途迁移拒绝（`incomplete-transition`）、损坏元数据
  拒绝；升级走 checkpoint+journal 事务。
- `server/index.ts:294-355`：闸位于同宅互斥（`server-info.json`）之后、任何 store 打开与端口
  绑定之前；`LINGXI_DATA_EPOCH_BLOCKED` / `LINGXI_DATA_EPOCH_TRANSITION_INCOMPLETE` 机读标记
  打到 stderr，`desktop/main.cjs` 识别后渲染专属双语拒启对话框。
- **闸在 DATA_EPOCH=1 基线下的软化**（server/index.ts:313-335 注释为有意设计；以下边界为
  repair-r1 按源码+实测修正后的**实测边界**，替换本段此前的错误陈述）：闸失败时
  `mustBlock = DATA_EPOCH>1 || 存在完好可读的高位印章 || epochResult.toEpoch > DATA_EPOCH`
  （server/index.ts:320-329）。第三条仅在 coordinator 返回的 failure **携带** `toEpoch`
  时成立——实际只有 incomplete 类 failure（`incomplete-transition`）携带；**corrupt 类
  failure（`corrupt-stamp`/`corrupt-journal`/`corrupt-transition`）在
  `core/data-epoch-coordinator.ts:547-550` 丢弃日志的 from/toEpoch**，因此恒不触发第三条。
  实测推论（repair-r1 探针 R7–R11 复证验收 REVIEW_R1 F1）：
  - 印章损坏或缺失时，即使目录内存在**完好可读、明确指向 epoch 2 的过渡日志**，旧程序
    同样仅警告 `LINGXI_DATA_EPOCH_BASELINE_WARNING` 并 fail-open 写入（R7/R9/R10）；
  - 无印章时拒写与否**取决于日志 phase**：`prepared`/`checkpoint_complete` 日志被判为
    `incomplete-transition`（携带 toEpoch）→ 拒写（R11）；`barrier_raised` 日志与缺失印章
    状态矛盾 → 被判为 `corrupt-transition`（不携带 toEpoch）→ 放行（R10）；
  - fail-open 分支的警告文本固定声称 "no higher-epoch evidence was found"——在 R7/R10
    场景下目录里**就有**可读高位过渡日志，该文本与事实相反（生产缺陷，立项见 §5）。
  `DATA_EPOCH>1` 后所有闸失败重新转硬。
- 逃生口：`LINGXI_ALLOW_DATA_DOWNGRADE=1` / `hana serve --allow-data-downgrade` 显式覆盖降级。

实测（旧程序=当前 HEAD 生产 server/CLI；依据见 §6；全部隔离于 `/tmp` 合成目录）：

| 探针 | 场景 | 结果 | 证据 |
|---|---|---|---|
| A1 | epoch-2 v2 印章目录 | exit 1，`LINGXI_DATA_EPOCH_BLOCKED reason=epoch-downgrade-blocked`，**文件系统零变化** | a13/a1-*.log + snapshots/a1-* |
| A2 | legacy v1 印章 `{epoch:2}` | 同上拒写，零变化 | a13/a2-* |
| A3/A3b | 指向 epoch-2 的半途过渡日志（含合法 journal） | exit 1，`..._TRANSITION_INCOMPLETE`，零变化 | a13/a3*, a3b-* |
| A4 | 对照：epoch-1 目录 | 正常启动（loopback 18814 应答），写入只落在沙盒 home（证明 LINGXI_HOME 实际生效值） | a13/a4-* |
| A5 | epoch-2 + `LINGXI_ALLOW_DATA_DOWNGRADE=1` | **警告后启动并写入 97 个新条目** | a13/a5-* |
| A6 | epoch-2 + 残留死 `server-info.json` | 拒写，但**预闸删除了残留锁文件**（闸前唯一的文件系统变更） | snapshots/a6-* diff |
| A7a/b | 分离根 + 原子指针：`active→epoch-1` 旧程序正常写 epoch-1 且 epoch-2 逐字节不变；rename 原子切换 `active→epoch-2` 后旧程序拒写且 epoch-2 仍不变 | 机制级原型通过 | a13/a7* + snapshots/a7* |
| A7c | 指针目标无印章（symlink 且未盖章） | `ambiguous-unstamped-home` 仅警告、继续启动并写入 | a13/a7c-* |
| A8 | epoch-2 意图 + **损坏印章** | `corrupt-stamp` 仅警告、继续启动并写入 97 条目（fail-open 边界） | a13/a8-* |
| A9 | `cli data diagnose` 对 epoch-2 home | exit 0，正确读出高位印章，零写入（维护面可读新目录） | a13/a9-* |
| R7 | **损坏印章 + 合法高位过渡日志**（barrier_raised，toEpoch=2） | exit 0 fail-open，`BASELINE_WARNING reason=corrupt-stamp`，写入 55 新条目；警告文本谎称无高位证据 | repair-r1/logs+snapshots/round1/round2-r7-* |
| R9 | **损坏日志 + 无印章** | exit 0 fail-open，`reason=corrupt-journal`，写入 55 新条目 | repair-r1 round1/round2-r9-* |
| R10 | **合法 barrier_raised 日志（toEpoch=2) + 无印章** | exit 0 fail-open，`reason=corrupt-transition`，写入 55 新条目；警告文本谎称无高位证据 | repair-r1 round1/round2-r10-* |
| R11 | 合法 prepared 日志（toEpoch=2) + 无印章 | exit 1 拒写，`..._TRANSITION_INCOMPLETE reason=incomplete-transition`，文件系统零变化（对照：无印章时拒写取决于日志 phase） | repair-r1 round1/round2-r11-* |

（R7–R11 由验收 REVIEW_R1 首次发现、repair-r1 以自建样本独立复跑两轮留证；C1 阻断对照
——完好 epoch-2 v2 印章 exit 1 零变化——同目录 round1/round2-c1-*。每轮 5 变体退出码与
文件系统 diff 摘要见 `R01-T07_REPAIR_R1.md` §3。）

## 3. 决策

### D1 切换策略：按域分级，不强制统一数据库

69 个注册存储分五级（逐项见 `DATA_COMPATIBILITY_MATRIX.json`）：

| 策略 | 数量 | 范围 |
|---|---|---|
| switch-to-new-authority | 11 | 运行/消息语义：session-jsonl、session-sidecars、session-manifest-sqlite、conversation-map-layout、workflow-state、subagent-state、deferred-result-state、loop-state、plugin-task-registry、terminal-session-state、execution-leases → 权威迁入 Rust 新运行/消息库 |
| preserve-format | 35 | 知识（knowledge-*）、记忆（agent-memory、agent-facts-sqlite）、配置（user-preferences、provider-state、mcp-config…）、附件/文件资源（session-files、knowledge-source-snapshots…）、凭证（local-user-auth、device-access-registries、web-session-registry、security-grants/key-material）等：**格式冻结**，新内核读写同一磁盘格式；格式要演进即触发 epoch bump |
| regenerable | 13 | 可重建缓存：丢弃重建，永不迁移 |
| shell-local | 6 | 桌面壳自态（窗口/GPU/更新通道/签名产物等）：不迁移、不共享、非权威 |
| epoch-mechanism | 4 | 印章/日志/检查点/恢复隔离区：由 epoch 协调器自身携带 |

消息权威唯一性：旧 JSONL 在导入前为**只读来源**；导入按会话（或整目录批次）进行，
转换写入新隔离库，校验通过后该会话的新权威切换到新库并写切换收据；原始文件保留归档
（重命名为归档形态/保留原路径均可，但旧世界不得再被当作权威写入）；**不做长期双向双写**。
旧未知字段、缺失关联、签名块分别归档保留，不猜测成新运行事实（02 §8）。

### D2 单写者保证：双层，不依赖旧程序认识新锁文件

1. **第一层（软件闸）**：现役 epoch 闸。实测结论（repair-r1 修正）：只要**高位印章
   完好可读**（A1/A2 零变化），或高位过渡日志完好可读**且被协调器判为
   `incomplete-transition`**（A3b/R11 零变化），当前发布的旧程序在任何 store 打开前
   拒写退出，desktop 壳另有 stderr 标记对话框。覆盖 69 存储中的 58 个 server 写入存储
   （42 authoritative + 13 regenerable + 3 adjacent server-written）。注意反向边界：
   印章损坏/缺失时高位过渡日志**不**保证拒写（R7/R9/R10 fail-open，见 §2 实测边界）。
2. **第二层（物理隔离，必需而非可选）**：分离数据根 + 原子活动目录指针。
   实测证明第一层存在三个如实记录的缺口，任何一个都足以要求第二层：
   - `LINGXI_ALLOW_DATA_DOWNGRADE=1` 显式覆盖后旧程序真实写入高位目录（A5，97 条目）；
   - DATA_EPOCH=1 基线软化 fail-open 的实际面比初版登记更大：不仅是「印章损坏/缺失且无
     高位证据」（A8/A7c），**印章损坏/缺失时即使存在完好可读的高位过渡日志同样
     fail-open**——corrupt 类 failure 丢弃日志 toEpoch（R7 损坏印章+合法日志、R9 损坏
     日志+无印章、R10 合法 barrier_raised 日志+无印章，均真实写入 55 条目）；唯一拒写
     例外是日志 phase 使 failure 落入 incomplete 类（R11 prepared 日志拒写零变化）；
   - 印章篡改窗口（验收 ADV-B 实证）：手动把印章 2→1 后旧程序真实写入 96 条目，改回 2
     后恢复拒写——闸是**协作式**的（依赖盘上元数据完好且未被篡改），这是 D2 把物理
     分离定为必需而非可选的直接实证之一；
   - 同宅互斥在 epoch 闸之前：残留死 `server-info.json` 会被预闸删除（A6，唯一预闸变更，
     属 regenerable runtime_state）；
   - desktop 壳写入的 6 个 shell-local 存储从构造上不过闸（desktop/main.cjs 不调用协调器，
     仅事后识别 server stderr 标记）——且其路径在**共享数据根内**，新旧两壳会在指针
     目标根内争抢同路径文件（移交 R09，见 §5）。
   机制：`{数据根}/epoch-1/`、`{数据根}/epoch-2/` 物理分目录，`active` 符号链接用
   「建临时 symlink + rename 覆盖」原子切换（A7 原型脚本 `scripts/atomic-switch.zsh`，
   已实测：旧程序经指针只在 epoch-1 内写；切到 epoch-2 即拒写且零接触）。
   **约束（实测 A7c）**：指针目标必须先盖章再启用——未盖章的 symlink 目标在当前基线下
   只警告不拒绝。旧世界为未盖章遗留目录时，切换工具链必须先完成 `adopted-legacy` 盖章
   （或显式盖章维护命令）再引入指针。
3. 锁文件纪律：新内核可以在自己的 epoch-2 根内使用任何新锁；**任何安全性论证不得
   依赖旧程序认识这些锁**（06 风险行「旧二进制绕开新锁」）。

### D3 数据版本、收据、完整性、停机切换

- 数据版本：DATA_EPOCH 语义版本（整数，单调）+ 印章 `lastVersion` 记录最后写入程序版本；
  存储级格式契约以 `shared/persistence/store-registry.ts` 指纹为约束（现役
  `check-persistence-schema-fingerprint` 门禁）。
- 迁移收据：复用现役 checkpoint+journal 体系（`data-epoch-checkpoints/{transitionId}`、
  七阶段 journal、checkpointProvider.verify）；会话级导入另有导入收据
  （演练中的 `cutover-receipt.json` 为形态样板）。
- 完整性检查：导入后校验（行数/哈希/抽样回读）通过才切换权威；备份用 SQLite Online
  Backup API + WAL checkpoint 静止点（W06/W07），多库与文件以「停写 → checkpoint →
  逐库 backup → 文件快照 → 整体清单哈希」为共同静止点，**禁止复制活跃 `.db` 单文件**。
- 停机切换：切换窗口内旧 server 与 desktop 全部停止（`server-info.json` 同宅互斥 +
  进程检查确认无写者），再原子切指针；切换后首轮启动验证印章与抽样数据。

### D4 回滚

见 `ROLLBACK_DESIGN.md`。原则：回滚前先导出归档新数据（演练 A14 已跑通：
W06 备份 → 导出 JSONL+附件+sha256 清单 → 旧备份恢复到**独立**根 → 原子切指针 →
旧程序启动验证 → 归档校验 → 幂等再导入）；全程无「直接删新目录」步骤；
恢复旧备份只恢复旧时点，不称无损。

## 4. 被拒候选

| 候选 | 否决理由 |
|---|---|
| 只靠 epoch 闸（不要分离根） | A5/A7c/A8 实测证明闸是协作式而非物理隔离；任务书明确「不能只靠新程序自觉」的镜像——也不能只靠旧程序自觉 |
| 只靠新锁文件/互斥 | 旧程序不认识新锁（06 风险行）；A6 还显示旧程序有自己的预闸写路径 |
| 全量统一进一个 SQLite | 违反 02 §8「先保护原格式」；无谓扩大迁移面 |
| 双向双写过渡 | 02 §8/§10 明确禁止双权威；同步失败即静默分叉 |
| 回滚=删除新目录恢复备份 | 丢切换后新数据（A14 红线） |

## 5. 后果与移交

- R02 实现新运行/消息库时按矩阵 `switch-to-new-authority` 11 项执行只读导入+收据+校验+切换；
  `preserve-format` 35 项由 Rust 侧实现同格式读写器，格式演进必须走 epoch bump。
- 安装/更新链（R09/R11）：新旧版本安装器必须把「分离根 + 指针」作为数据布局前提；
  旧版本自动更新不得在指针指向 epoch-2 时静默启动旧二进制（软件闸兜底，物理隔离为主）。
  **R09 新增强制项（验收 REVIEW_R1 F3，repair-r1 登记）**：desktop 壳 6 个 shell-local
  存储（crash.log、window-state、update-channel、GPU 态等）路径在共享数据根内且不过闸，
  分离根+指针模型下新旧两壳会在指针目标根内**争抢同一路径**的「各自本地」状态——
  R09 必须保证新壳 shell-state 落在共享根之外或按壳隔离路径，并将「指针指向 epoch-2
  时禁止旧壳启动」从方向性表述升格为安装链验收关卡。
- **生产缺陷立项（验收 REVIEW_R1 F1③，移交候选：R02 Rust 存储阶段）**：
  `core/data-epoch-coordinator.ts:547-550` 的 corrupt 类 failure（corrupt-stamp/
  corrupt-journal/corrupt-transition）丢弃了可读日志的 from/toEpoch，导致
  `server/index.ts:325-329` 的 `hasHigherTransition` 恒 false、mustBlock 误判，
  且 fail-open 警告文本 "no higher-epoch evidence was found" 在存在可读高位过渡日志时
  与事实相反（R7/R10 实测）。修复方向：corrupt 类 failure 在日志可读时携带
  from/toEpoch（或至少在 server 侧补读日志再判 mustBlock），并修正警告文案。
  **R01 不改生产代码的理由**：R01 阶段冻结生产行为（任务边界为文档/矩阵/证据），该缺陷
  修复触及运行期闸语义，需独立授权、专项回归（data-epoch 测试组）与发布评估，故立项
  移交而非在本阶段顺手修改。缺陷已由 D2 第二层（物理分离根）覆盖，不阻塞 R01 结论。
- 已知边界（如实登记）：desktop 壳 6 存储不过闸（构造性，且共享根争抢——上条 R09
  强制项）；`server-runtime-info` 预闸自清（A6）；DATA_EPOCH=1 下损坏元数据 fail-open
  （A8），且 corrupt 类 failure 时**有可读高位过渡日志也 fail-open**（R7/R9/R10，
  R11 为拒写对照）——四者均由 D2 第二层覆盖。
- 本任务未改 `rust/` 与任何生产代码，T01/T02 门禁无需重跑（无受影响面）。

## 6. 「旧程序」选择依据与限制

- 依据：本仓库是发布与自动更新的 single source of truth（AGENTS.md），HEAD=82870879 即
  当前发布线代码；epoch 机制（印章 schema v2 + 协调器 + server 闸 + desktop 对话框）在
  基线提交 `d5275e568` 之前已存在，现役所有发布均含此闸；因此 HEAD 构建的 server/CLI
  在 epoch 行为上与田间旧二进制逐字节同源。
- 限制：未使用下载的发布 DMG（安装器层行为未复测）；Electron GUI 壳未在无头环境启动，
  desktop 侧结论为源码审计（`desktop/main.cjs` 无协调器调用、仅 stderr 标记识别）而非探针实测。
