# 架构契约补强最终报告

- 基线：`e0afc0dc3b94bc9763d715f981d6f948d1c39c27`（main）。
- 工作区：`LingxiAgent-contract-hardening` detached worktree；未提交、未推送、未发布。
- 环境：macOS arm64，Node v24.16.0，Swift 工具链（原生协调器离线验证）。
- 方法：先把合成反例写成调用真实模块的失败测试（红），再实现最小补强（绿）；替身只用于供应商、网络与操作系统边界。

## 逐项结果

### 1. 输入接受与取消（架构契约）

- 历史线索：round3 C01 已建立 canonical 接受边界与 `rejected_before_acceptance`；本轮缺口在上游证明链。
- 源码证据：基线把「缺少 canonical 关联记录」误读为确定未接受；SDK 预检等待期间的取消被当成接受成功；插入消息未等待 SDK 异步结果。
- 改动：`core/desktop-session-submit.ts`、`lib/pi-sdk/desktop-input-commit.ts`、`core/session-coordinator.ts`、`server/routes/chat.ts`、`lib/session-collab/delivery.ts` 分开记录「尚未移交／已移交待证／已观察写入」；准备期取消发 `input_cancelled_before_acceptance` 既有拒绝回执；插入接口改为等待异步结果的 `Promise<boolean>` 语义，全部调用方等待；移交后的 `session_busy` 异常不再触发跨会话重发。
- 先红证据：`input-red.log`（8 失败）、`input-steer-order-red.log`、`input-runtime-preflight-red.log`（2 失败）、`input-delivery-retry-red.log`。
- 验证：`input-final-tests.log` 12 文件 292 项通过；`input-history-projection.log` 12 项通过；新增 `tests/desktop-input-lifecycle-contract.test.ts`。

### 2. 记忆迁移与恢复（架构契约）

- 源码证据：基线的目标文件写入先于 `target_committed` 收据，收据 rename 失败后用户删除的条目会被旧 prepared 分支重新导入；损坏来源被过滤成空库；恢复批准编号 `restore_1` 与备份路径解析规则不一致。
- 改动：`core/pinned-tenets-migration.ts`、`core/pinned-tenets-recovery.ts`、`core/pinned-tenets-backup-dir.ts` 落地 v4 收据（`prepared → committing → target_committed → sources_archived → completed`）；`committing` 必须先于目标写入落盘；只有 v4 `prepared` 可重放；其余中间态凭完整目标证据收尾，否则持久记录冲突并保留原件；坏来源整体失败；操作编号与路径往返一致并拒绝越界。
- 先红证据：`migration-red.log`（14/18 失败）。
- 验证：`migration-green.log` 10 文件 172 项通过；新增 `tests/pinned-tenets-contract-hardening.test.ts`。
- 明确取舍：`committing` 已落盘而目标未写入时被硬中断，无法区分「未写」与「写后改回」，保守转入冲突等待审阅，不自动续做。

### 3. 媒体任务记录（架构契约）

- 源码证据：基线在 tasks.json 损坏或非数组时静默以空库启动，后续保存会覆盖原件；`destroy()` 取消待保存工作后直接返回。
- 改动：`core/media/task-store.ts` 只有文件不存在才从空库开始，损坏/非法结构/读取失败明确报错并阻止覆盖；`destroy()` 改为 `requireFlush`（先保存最新状态，失败则抛出）；新增 `settleTask`/`beginAttempt`/`markDeliveryHandedOff` 终态与尝试原语；旧记录按 `attempt=1` 兼容读取。
- 验证：`tests/media-task-store.test.ts`（3 个旧断言按新契约改写为「拒绝启动且原件保留」「关闭前保存最新状态」）与 `tests/media-contract-hardening.test.ts` 共 51 项通过。

### 4. 媒体完成、取消与重试（架构契约）

- 源码证据：基线同步与后台完成的产物检查不一致；取消后迟到的查询结果可覆盖终态；提交与查询不区分尝试。
- 改动：`core/media/poller.ts` 重写为代次+尝试模型（运行代次使旧操作失效、同一尝试最多一个在途查询、终态只结算一次、交付需 durable 回执否则保持待办）；`core/media/image-task-runner.ts`/`submit-image.ts`/`download.ts` 接入同一规则；`core/media/universal-media-manager.ts` 的 `stop()` 返回可等待结果并在 `core/engine.ts` 的关闭链中被等待（基线 `this._media?.dispose?.()` 未 await）。
- 先红证据：`media-red.log`、`media-iteration.log`、`media-wide-iteration.log`（21 失败）、`engine-media-shutdown-red.log`（关闭不等待）。
- 验证：`tests/media-poller.test.ts` 改用真实 TaskStore 重写（28 项）、`tests/media-gen-poller-completion-event.test.ts`、`tests/media-session-fork.test.ts`、`tests/engine-lifecycle.test.ts` 全绿；`tests/media-contract-hardening.test.ts` 16 项通过。

### 5. 语音链路（架构契约 + 局部错误）

- 源码证据：适配器网络请求不带调用方取消信号；同一会话文件的两次识别互相覆盖；工具入口提前过滤显式空值导致与 REST 不一致；MiniMax PCM 字节被误标为 MP3；系统 TTS 子进程取消无界；原生协调器在「取消/超时先于挂接到达」时触发 `continuation attached twice` 崩溃。
- 改动：`core/media-adapters/speech.ts` 每次网络请求使用 `ctx.signal`、新增 `runSystemSpeechProcess` 有界回收（SIGTERM 后等待真实退出）、PCM 以 `.pcm` 原始字节交付；`plugins/media/tools/generate-speech.ts` 与 `server/routes/media.ts` 保留显式空值并经由 `core/media/media-parameters.ts` 唯一解析器；`core/speech-recognition-service.ts` 新增同文件识别尝试所有权（新尝试中止旧信号，旧尝试迟到结果返回 `{status:"skipped", reason:"superseded"}` 且不写注册表/不发事件，观测如实各自结算一次）；`RecognitionCoordinator.swift` 保存提前到达的取消/超时，挂接时恰好结算一次；`CoordinatorTestRunner.swift` 以明确就绪屏障替代 `Task.yield()` 猜测。
- 先红证据：`speech-red.log`（9 失败）、`native-red.log`（precondition 崩溃）。
- 验证：`tests/speech-contract-hardening.test.ts` 13 项、`tests/speech-response-lifecycle.test.ts` 8 项（V11 场景改用合法路径架设：终态不得互改，取消经 pending→cancelled 真实路径）及其余 14 个语音相关套件全绿；原生 `swift run lingxi-speech-core-tests` 22/22 通过（含新增「终局先于挂接」两个场景）。

### 6. MCP 连接归属（架构契约）

- 源码证据：基线手动启动/按需启动/自动重连各自为政；旧启动失败无条件删除当前客户端；旧目录刷新覆盖新连接的目录与权限注解；目录刷新不计入使用中，可被空闲停泊。
- 改动：`core/mcp/manager.ts` 三个入口共享连接尝试；客户端实例即连接代次；停止/停用/替换使旧代次失效，旧操作只回收自己；刷新、资源读取、工具调用计入使用中；异步完成后复核实例归属、启用与认证条件。
- 先红证据：`mcp-red.log`（5 失败）、`mcp-connecting-red.log`、`mcp-authorization-red.log`、`mcp-cleanup-ownership-red.log`。
- 验证：`mcp-expanded-green.log` 11 文件 306 项通过；`tests/mcp-operation-ownership.test.ts` 8/8。

### 7. 文件展示与历史（架构契约 + 局部错误）

- 源码证据：历史结果提取丢失文件版本；历史回填把旧版本改写成最新版本；同文件不同版本/独立展示生成相同界面编号；语音结果被历史分类器遗漏。
- 改动：新增 `server/session-file-block.ts` 共用文件字段转换（实时通知与工具结果提取共用，已登记入 `export-manifest.json`）；`server/routes/sessions.ts` 历史回填只更新定位与可用状态、不覆盖记录版本；`desktop/src/react/utils/content-semantics.ts` 展示编号纳入版本与任务身份，重放保持稳定；`server/block-extractors.ts` 补齐语音任务历史识别。
- 先红证据：`presentation-red.log`（5 失败）。
- 验证：`mcp-presentation-green.log` 12 文件 476 项通过；`tests/media-presentation-history.test.ts`。

## 接口与兼容落地核对

- 插入消息等待异步结果；接受前取消使用明确错误码；接受/拒绝/未知三态沿用。
- 收据 v4 + `committing` 已落地；旧 v2/v3 继续读取，旧 prepared 升级映射为 `committing` 而非「可安全重放」。
- 媒体任务带尝试编号，旧记录按初始尝试读取；连接代次为运行时信息；媒体与引擎关闭接口返回可等待结果并接入引擎关闭链。
- 未新增供应商、授权方式或通用状态框架；普通发送与插入、朗读返回与会话交付的业务差异未改变。

## 全量门禁结果

- `npx tsc --noEmit`、`-p tsconfig.node.json`、`-p tsconfig.test.json`：退出码均 0。
- ESLint（全部改动文件）：0 error（既有风格 warning 不变）。
- `npm run build:renderer`：构建成功（chunk 体积警告为既有）。
- `npx vitest run` 全量：13,609 通过 / 4 失败 / 7 跳过（1,350 文件）。
  - 4 个失败在干净基线 `e0afc0dc` 上同样失败（预存红，与本轮改动无关）：
    - `tests/round2-delivery-evidence.test.ts` R10-03、R10-04 与 `tests/round3-delivery-evidence.test.ts`：历史交付 manifest 对照的已验证封印坐标在 v0.1.37 封印推进（4ae02c83/e0afc0dc，仅审计文件）后滞后于树形；按治理规则不为此虚报坐标或扩大白名单，需经授权的封印推进流程处理。
    - `tests/packaged-desktop-cleanup.test.ts`「持续未就绪仍在原有 90 秒期限失败」：基线同形状失败。
- 原生离线验证（修正等待竞态后重新运行）：`swift run lingxi-speech-core-tests` 22/22 通过。
- `git diff --check`：通过（修复前一历史补丁文件被前序会话误再生成，已还原，与本任务无关）。
- 持久化指纹：按兼容声明 repin（`build/persistence-schema-fingerprint.json`，媒体任务 attempt/deliveryState 可选字段、收据 v4 兼容读取）；已核实未触碰真实用户数据目录。

## 未执行与阻塞

- 真实供应商执行、计费停止、系统权限（麦克风/语音识别授权）、其他平台（Windows/Linux）验收：未执行。
- 断电/文件系统持久化顺序：未执行（SIGKILL 只证明进程硬终止路径）。
- 上述 4 个预存红：属既有治理事项，未在本轮处理。
- 本报告绑定基线与本工作区实际改动；不构成新的审计封印。

## 变更规模

54 个跟踪文件修改（+1681/−1172），新增 6 个契约测试文件、1 个共享模块（`server/session-file-block.ts`）、1 篇架构文档（`docs/architecture/async-lifecycle-contracts.md`，已登记 `docs/README.md`）及本记录目录。
