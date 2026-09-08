# C01–C03 修复报告（round3）

- 基线：67dee5d2de9d3b9fc75ec5ef5c555e93c65b3ccd（refactor/dismantle-and-voice-features）
- 对应受封印源码候选：7cf986236cb8ac5c2d22cce1e5882554a42cff28
- 环境：darwin arm64 · Node v24.16.0 · npm 11.13.0
- 执行日：2026-09-08（UTC）；全部工作在隔离本地工作区 + 合成数据完成，未提交
- 上轮 R/F 项只做受影响回归；未恢复任何已拆除产品入口；未新增聊天工具栏语音组件。

---

## C01：补齐「明确未接收」的拒绝结算

### 原缺陷
服务端在「接受前」明确拒绝输入（模型切换中、会话忙、媒体校验、无效审阅
Agent、提交层 busy 门禁等）时只回普通 `error` 消息；客户端
`composer-send-coordinator` 没有「服务端接受前拒绝」的结算相位，
`awaiting_ack` 只能等 15s 看门狗转 `delivery_unknown`，而未决记录经
`transport_busy` 持续阻塞同会话后续发送。且同版本重试复用
`clientMessageId`，迟到负回执无从按尝试区分。

### 实际修改
| 层 | 文件 | 内容 |
|---|---|---|
| 提交层 | `core/desktop-session-submit.ts` | `markDesktopInputRejectedBeforeAcceptance`/`isDesktopInputRejectedBeforeAcceptance`（Symbol 标记）；`withInputCorrelation` 增加 canonical 回执观察回调；`submitDesktopSessionMessage` 预检 throw 与外层 catch 在 canonical 回执未触发时标记；`submitDesktopSessionInterjection` 以 receipt 载体包装，steer 前一切失败标记 |
| 路由层 | `server/routes/chat.ts` | `sendInputRejection`（单播 `input_rejected`，身份取自服务端解析的 promptTarget，无 clientMessageId 不投递）+ `inputRejectionInfoForSubmitError`；媒体/忙/模型切换/knowledgeRefs/评审预校验共 16 处站点双发（legacy error 保留 + 类型化拒绝回执）；hub.send 与 interject catch 仅对标记错误发拒绝回执；评审 start 同步抛错本地 catch |
| 评审 | `lib/agent-review/turn-coordinator.ts` | `ReviewTurnInput.onRejected`；`parentSubmitted` 边界；父会话提交前失败/父会话提交被标记拒绝/提交前用户取消 时通知 |
| 客户端 | `desktop/src/react/components/input/composer-send.ts` | `createClientAttemptId`；commit runtime 接受 `attemptId` 并在发送前落入载荷 |
| 客户端 | `desktop/src/react/services/composer-send-coordinator.ts` | 新相位 `rejected_before_acceptance`、`acceptance:'rejected'`、记录级 `activeAttemptId/seenAttemptIds/queueItemIndex/queueItemCreatedAt`；`noteComposerInputRejected` 统一结算入口（五元组匹配、canonical 冲突不倒退、幂等、迟到 HTTP 对账不覆盖）；队列项原位恢复；`retryRejectedUserMessage`/`discardRejectedUserMessage`；默认 deps 工厂 |
| 客户端 | `desktop/src/react/services/ws-message-handler.ts` | `case 'input_rejected'`：形状校验后交 coordinator |
| 客户端 | `desktop/src/react/stores/chat-slice.ts` | `markOptimisticUserMessageFailed` 增 retryable；`removeOptimisticUserMessage`；`restoreQueuedTurnInput`（原位插入 + 幂等） |
| 客户端 | `desktop/src/react/components/chat/UserMessage.tsx` | 拒绝横幅（重试/放弃按钮，复用 delivery_unknown 展示模式） |
| i18n | `desktop/src/locales/{zh,en,ja,ko,zh-TW}.json` | `input.sendNotAccepted/sendRetry/sendDismiss` |

### 修复后行为
- 确定拒绝（not_accepted）与投递未知（delivery_unknown）分离；接受边界以
  canonical 关联回执（session_user_message 带 sourceEntryId）为唯一证据，
  不看文案/ACK/有无输出。
- 已接受后的运行错误（含真实 SDK append 后的失败）绝不产生拒绝回执、不自动
  重发；旧服务端普通 error 继续走 unknown + 显式核对。
- 拒绝结算：取消看门狗与对账覆盖、释放本条租约与屏障、快照（正文/附件/
  引用/知识范围）原样保留、队列项按原位置与身份恢复且后续项不越过失败项。
- 重试同内容同逻辑身份（clientMessageId）+ 新尝试身份（clientAttemptId），
  第一尝试迟到负回执不影响第二尝试。

### 测试
`tests/round3-c01-input-rejection.test.ts` 22 项全绿：路由回执形状 ×6、
提交层标记 ×3（含真实 SDK fixture 的 canonical 边界双向）、评审通知 ×3、
客户端结算 ×10（含「客户端→真实路由→真实 handler→store→coordinator」全链路
模型切换闭环、看门狗前后、迟到尝试、跨会话/服务器/版本、canonical 冲突、
供应商错误不误判、快照保留、A/B 队列顺序、unknown 屏障隔离）。
受影响回归：composer/route/pinned/desktop-session-submit 共 226 项绿。

### 剩余限制
- 供应商错误多数被 SDK 化解为 run 级事件而非提交 Promise 拒绝；「已接受后
  运行失败」的拒绝回执反向断言用提交链后段故障注入验证（真实供应商 5xx 的
  端到端路径未接真实付费模型，按授权限制保留）。
- 多服务器同账号场景仅以 originConnectionKey 隔离验证（合成）。

---

## C02：收据使用平台无关路径，兼容已有 Windows 收据

### 原缺陷
`writeBackups`/`upgradeLegacyReceipt`（migration）与 recovery 的
`backupDir:path.join(...)` 在 Windows 上把 `memory\pinned-migration-backup\…`
直接持久化进收据；而 `isMigrationReceipt` 校验只认 POSIX `memory/…`——
同一份合法收据在任何平台读取即 `MIGRATION_RECEIPT_INVALID`，POSIX 续跑还会
把反斜杠当文件名字符。

### 实际修改
- 新增 `core/pinned-tenets-backup-dir.ts`（迁移/恢复共用的集中合同）：
  `backupDirForReceipt`（构造规范 POSIX 表示）、`parseReceiptBackupDir`
  （结构先行验证：接受 POSIX 与旧 Windows 反斜杠，拒绝盘符/UNC/绝对路径/
  NUL/`.`/`..`/多余层级/混合分隔符）、`receiptBackupDirLocalPath`（本机路径
  合成，非法即明确失败）。
- `core/pinned-tenets-migration.ts`：校验改走 `parseReceiptBackupDir`；两处
  写侧改 `backupDirForReceipt`；`resumeMigration` 在内存中把旧表示转规范
  形式（合法续写持久化为统一路径）；`assertReceiptSourcePlan` 本地访问走
  resolver。
- `core/pinned-tenets-recovery.ts`：新操作写侧 + 续跑规范化 + prepare 的本地
  备份目录解析。
- `export-manifest.json`：`core/pinned-tenets-backup-dir.ts` 列入开放核心
  （open 集合中的 migration.ts 引用它；纯 node:path，无新闭包耦合）。

### 修复后行为
同一份合法收据（POSIX 或旧 Windows 写出）在 macOS/Linux/Windows 读取与
续跑一致：completed/conflict 不重导、不复活用户删除内容、不移动既有备份、
不为换分隔符重跑迁移；批准摘要与 operation 身份不受规范化影响；危险路径
输入整体判无效。

### 测试
`tests/round3-c02-receipt-backup-dir.test.ts` 32 项全绿（写侧规范 ×3、
v3 三状态 × Windows 旧表示 ×3、completed 不复活 ×2、v2 completed/中间态 ×2、
recovery 兼容 ×2、拒绝路径 ×16、本地解析 ×1）；pinned 全家回归 87 项绿。

### 剩余限制
POSIX 环境运行；「Windows 旧表示」经真实迁移/恢复产出收据的数据级重写模拟
（缺陷为数据格式级，表示与续跑语义已完整证明）；真实 Windows 客户端端到端
写入/读取实测保留 BLOCKED。

---

## C03：交付真实日志，并在干净环境验证

### 原缺陷
`tests/round2-delivery-evidence.test.ts` 依赖
`artifacts/f1-f12-repair/round2/logs/` 下 121 个审计日志，但 `.gitignore`
的 `logs/` 与 `*.log` 使其从未入库——干净检出 R10-02/05/S11-X2/R10-10
四项必失败。

### 取证与修复（不编造历史）
- 原始日志完好保存于 LingxiAgent-r01-r10 工作树副本：121/121 的 SHA-256
  与 `COMMAND_RESULTS.json.logSha256` 一致（真实历史，非重造）；全部日志
  不含真实用户路径（泄漏断言通过）。
- 121 个原始日志按原字节复制入主树；`.gitignore` 增加精确例外
  `!artifacts/f1-f12-repair/round2/logs/` 与 `…logs/**`（其余运行日志仍忽略）。
- R10-09 契约按当前树重生成增量补丁（现含 round3 源码，即「新增源码与测试
  进入补丁」）；`DELIVERY_MANIFEST.sha256` 的补丁摘要同步为新值并记录来源
  （459bfbef→a946a461），217/217 哈希一致。
- round2 证据测试 10/10 绿；无任何 `if(!exists)return`/skip/断言删减。

### round3 新证据
`artifacts/f1-f12-repair/round3-c01-c03/`：`run-evidence.py`（与 round2
同构的命令证据 runner，基线 67dee5d2）→ `COMMAND_RESULTS.json` + `logs/`
+ `manifests/`；`SOURCE_MANIFEST.json` 可复算；测试矩阵见
`C01_C02_C03_TEST_MATRIX.json`。

### 剩余限制
- 全部交付物未提交（无提交授权）；「提交后验证」未做，只做候选交付重建验证。
- 真实 Windows/x64 平台与真实供应商验证保留 BLOCKED（见矩阵）。

---

## R/F 状态更新
- R01–R10（round2 已完成项）：本轮仅做受影响回归（226 项绿）+ 证据测试
  10/10；无行为改动。
- F7（语音输入组件）：产品变更范围内维持「不新增语音输入组件、不重建听写
  流程」；后端 ASR/TTS、宿主授权桥、原生音频附件与快捷键全部保留
  （speech/tags/fidelity 回归 12 文件绿）。

---

## 最终验证与候选交付重建（收尾记录）

- 最终冻结源码 manifest：`e1bfe94a0075775ff1cc69683fff6cdcdd81ccdcda7eb14da65367ba7cd4c3ab`
  （`SOURCE_MANIFEST.json` 可从当前树复算；tests/round3-delivery-evidence.test.ts 5/5 绿）。
- 最终门禁（冻结后）：`npm test` 13443 passed / 0 failed / 7 skipped（j15，exit 0、
  无漂移）；`npm run lint` 0 errors（j17）；typecheck ×3 绿（j1）；
  build:client 绿（j7）；build:server 绿（j8，一次性签名密钥，本地验证姿势）。
  j13 出现过一次 vitest worker fork unhandled error（平台抖动），j9/j11/j15 三轮
  全量均无复现，不判为代码问题。
- 增量补丁：`patches/67dee5d2-to-round3-c01-c03.patch`
  （sha256 随冻结重生成，以 COMMAND_RESULTS 最新 create-round3-patch 记录与 DELIVERY_MANIFEST.sha256 为准，
  38,080,707 字节，含源码 + 全部交付证据与 round2/round3 审计日志）；
  temp-index 重放验证 VERIFIED（重放 manifest 与源树逐字节一致）。
- 候选交付重建验证（无提交授权，非「提交后验证」）：独立目录全新 clone
  `67dee5d2` → `git apply` 增量补丁 → `npm ci`（锁文件，1282 包）→
  round3 证据 + round2 证据 + C01/C02 修复测试 4 文件 69/69 绿，
  重建树源码 manifest = `1218e27c…` 与源树一致。首轮重建曾发现 round3 日志
  未随补丁交付（与 C03 同型缺陷），已通过 .gitignore 精确例外修正并复验。
- 未做（按授权/环境）：commit/push/PR/发布；VERIFIED_SOURCE_SHA 未推进；
  真实 Windows 客户端、真实供应商、x64 平台实测。

### 冻结坐标修订（提交前）
首次冻结（1218e27c…）把本地工具目录 `.workbuddy/`（会话工作台状态，非项目源码）
计入未跟踪源集；已将其加入 .gitignore 并重冻结为
`e1bfe94a0075775ff1cc69683fff6cdcdd81ccdcda7eb14da65367ba7cd4c3ab`（j19 typecheck /
j20 补丁重放 VERIFIED / j21 全量）。j21 的 3 个失败均为封印家族「预期红」
（post-verification-audit-seal 与其经 R10-03/R10-04 的同源调用：候选提交已落、
VERIFIED_SOURCE_SHA 尚未推进），13440 项实质测试通过；封印推进提交后转绿。
