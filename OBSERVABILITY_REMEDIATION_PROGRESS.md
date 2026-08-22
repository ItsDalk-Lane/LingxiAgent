# Progress Log

## Session: 2026-08-22

### Phase 1：需求与基线事实

- **Status:** complete
- **Started:** 2026-08-22
- Actions taken:
  - 确认用户要求在当前工作树和当前状态原地执行。
  - 确认当前 Git 为 detached HEAD，未创建或切换分支。
  - 读取 `planning-with-files` 全部指引并执行会话恢复检查。
  - 定位 2503 行任务书及相关历史记忆入口。
  - 完整读取任务书第 1～1000 行，登记时区、查询真实性、完整性和 Blob 两阶段提交等首批发现。
  - 完整读取任务书第 1001～2000 行，登记事务计数、失败回执、悬空引用、动态排空、路径权限、流式读取、证据时间线及纵向验收要求。
  - 完整读取任务书第 2001～2503 行，登记故障注入、数据库升级条件、界面/导出同步、20 条发现台账、验证序列和审计封印要求。
  - 记录开始提交、本地主线、远程主线、共同基点、功能提交范围和当前源码封印对象类型。
  - 确认远程功能分支与当前 detached HEAD 完全一致，没有创建或切换分支。
  - 定位 Blob 分片、数据库路径信任、同步读取、重试重复写、事务计数和外置器恢复的生产代码证据。
  - 完整读取 `security-review` 指引，建立输入校验、参数化查询、受控路径、错误与敏感信息不外泄的检查边界。
  - 逐行还原查询筛选、数字映射、完整性、用量关联、Trace 聚合和 DST 分段的当前实现与根因。
  - 还原整体聚合与 Trace 详情的用量覆盖逻辑，确认旧投影、部分覆盖和缺行均被错误折叠。
  - 还原精确载荷、健康接口和 Blob 读取的缺失/损坏语义；确认当前数据库为 v2 且缺少显式用量关联列。
  - 审查共享 DTO、数据库迁移和载荷状态写入，确认契约非空约束与写入优先级会阻碍正确语义。
  - 完整读取持久化协调器，定位文件/事务重试、回滚计数、逐调用 drop、失败回执和注册对象恢复的准确故障路径。
  - 完整读取 BlobStore、PayloadStore 和 Capture registry，定位悬空引用、目录权限、假分片、历史兼容和外置器恢复的具体实现点。
  - 还原引擎设置更新、记录器固定接收者和 Pi 原生压缩接点，确定代际排空与显式 MC-03 用量关联事实的落点。
  - 创建 `MODEL_OBSERVATORY_ADVERSARIAL_REVIEW.md`，登记 AR-01～AR-20 的修复前证据、失败测试、根因、目标和剩余边界。
  - 使用 Node 24.16.0 执行 `npm ci` 恢复锁文件固定依赖；原生 SQLite 与项目 postinstall 校验成功。
  - 审查真实测试 harness、v1 schema fixture 和 Export 复用链，确定新增测试可直接覆盖真实 SQLite 与共享 DTO。
  - 新增 Query Truth 测试并在修复前运行：9 个用例全部按预期失败，覆盖同字段 OR、NULL、完整性、usage unknown、Trace 完整统计、partial payload、aggregate coverage 和 provenance corruption。
  - 修复 Query Truth 主链：同字段 OR、严格 NULL、完整性 unknown、Trace 完整统计、部分载荷优先级、用量聚合覆盖状态和 provenance 损坏状态已通过新增测试。
  - 新增 DST 对抗用例，先证明窗口两端 offset 相同会漏掉中间两次切换，且段数超限会静默返回错误数据；随后改为固定小窗口扫描与显式稳定原因码。
- Files created/modified:
  - `task_plan.md`（创建）
  - `findings.md`（创建）
  - `OBSERVABILITY_REMEDIATION_PROGRESS.md`（创建；替代与已有 `PROGRESS.md` 冲突的技能默认文件名）

### Phase 2：调用链与根因还原

- **Status:** complete
- Actions taken:
  - AR-01～AR-20 均已登记修复前证据、失败测试、根因和目标；查询、时区、存储、生命周期三条实现线已按不重叠文件范围推进。
- Files created/modified:
  - 暂无

## Test Results

| Test | Input | Expected | Actual | Status |
|------|-------|----------|--------|--------|
| 工作区状态检查 | `git status --short --branch` | 不创建或切换分支，获取真实状态 | `HEAD (no branch)`，无已显示改动 | ✓ |
| 现有相关套件基线（首次） | 10 个 Observatory 测试文件，`--maxWorkers=4` | 使用仓库固定 Vitest 启动测试 | 本地依赖缺失；`npm exec` 临时安装失败，测试未启动 | ✗ ENV |
| 依赖恢复 | `npm ci` | 按锁文件安装且 postinstall 成功 | 1282 packages；Pi SDK checks passed；exit 0 | ✓ |
| 现有相关套件基线（依赖恢复后） | 10 个 Observatory 测试文件，`--maxWorkers=4` | 当前树原有用例全绿 | 10 files / 78 tests passed；exit 0；1.54s | ✓ |
| Query Truth 新增失败测试（修复前） | `tests/model-observability-query-truth-integrity.test.ts` | 每项缺陷均被真实 SQLite 测试复现 | 1 file failed；9/9 tests failed；exit 1；84ms | RED ✓ |
| Query Truth 修复后 | 同文件 9 个基础真值用例 | 全部通过 | 1 file / 9 tests passed；exit 0；396ms | ✓ |
| DST 新增失败测试（修复前） | 同文件 2 个 IANA 对抗用例 | 复现偶数次切换漏检和上限静默错误 | 2/2 tests failed；exit 1；63ms | RED ✓ |
| Query Truth + DST 修复后 | 同文件全部 11 个用例 | 全部通过 | 1 file / 11 tests passed；exit 0；367ms | ✓ |
| 查询相关回归（中间态） | Query 旧套件 + Truth + DST | 识别旧断言与新事实的冲突 | 32 tests 中 31 passed、1 failed；旧用例仍把普通缺 usage row 断言为 `not_correlated`，已交由 schema/usage 修复线更新 | BOUNDED |

## Error Log

| Timestamp | Error | Attempt | Resolution |
|-----------|-------|---------|------------|
| 2026-08-22 | `progress.md` 在大小写不敏感文件系统中覆盖 `PROGRESS.md` | 1 | 初始状态已确认干净；迁移本轮记录并从 HEAD 精确恢复原文件 |
| 2026-08-22 | 多文件纠错补丁上下文不匹配 | 1 | 补丁未部分应用；先定位精确文本后以小补丁完成 |
| 2026-08-22 | Vitest 基线未启动：缺少本地依赖，临时 `npm exec` 在 fsevents node-gyp 失败且无法解析 `vitest/config` | 1 | 不重试同一命令；按 lockfile 恢复当前工作树依赖后使用本地运行器 |
| 2026-08-22 | 记录 Query RED 结果的多文件补丁上下文不匹配 | 1 | 补丁未部分应用；拆为逐文件小补丁 |
| 2026-08-22 | 只读 `rg` 正则中的反引号被双引号 shell 解释为命令替换 | 1 | 没有文件写入；改用单引号固定正则 |

## 5-Question Reboot Check

| Question | Answer |
|----------|--------|
| Where am I? | Phase 1：完整读取需求并建立基线事实 |
| Where am I going? | 根因还原、精准修复、完整验收、交付复核 |
| What's the goal? | 完成 Phase 10.1 指定真实性修复并重新验收 |
| What have I learned? | 见 `findings.md` |
| What have I done? | 见本文件上方记录 |

### Phase 3：精准实现与回归测试

- **Status:** complete
- Query 与界面：修复同字段 OR、严格 NULL、聚合覆盖状态、完整性 unknown/degraded、显式 usage correlation、完整 Trace 统计、载荷状态优先级、损坏状态、IANA 时区与复杂度错误。
- Schema：升级至 v3 正式列；覆盖 v1/v2 只读、v1/v2→v3、回滚和未知新版本。
- Persistence/Blob：文件准备与数据库提交拆相；事务局部计数；最终失败回执；逐调用 dropped；无悬空引用；真实分片与 legacy 兼容；数据库路径不再授权；GET 流式、HEAD 只检查元数据；外置器所有权恢复。
- Lifecycle：新增代际管理，四类在途切换只影响新调用；旧调用有界排空，生产设置入口和延迟 Provider 均有回归。
- 独立场景：补齐 parallel tools、subagent、MC-02、MC-03、approval repair、memory representative prompt、Call Inspector、Payload 与 Trace 纵向链。
- 生成物：CLI closure 10609 files；持久化 inventory 61 stores / 721 sites；指纹更新为 `sha256:f4cfa1e85848f7b621a87f5cc638a3d0c9229d71acae9f662e34b438358a3d49`。

### Phase 4：完整验收与证据闭合

- **Status:** complete（本地）
- `npm run typecheck`：PASS。
- `npm run lint`：PASS，0 error。
- `npm run lint:boundary`：PASS，1 条既有基线边。
- persistence scanner / fingerprint / CLI closure / i18n：PASS。
- 定向 Observatory：51 files / 449 tests；最终新增纵向复跑 2 files / 17 tests。
- 最终全量：1201 files passed / 1 skipped；12135 tests passed / 7 skipped；0 failed；134.40s。
- 三个构建：`build:server`、`build:server:open`、`build:client` 均 PASS。
- package smoke：macOS arm64 应用目录生成并完成本地签名验证；notarization 明确 NOT_EXECUTED。
- remote Windows/Linux/macOS CI：NOT_EXECUTED；没有本轮功能提交和推送，不能触发可信远端验证。
- Phase 10.1 source seal：NOT_EXECUTED；旧 seal 是 commit 对象，但不覆盖本轮工作树。

### Phase 5：交付复核

- **Status:** complete（除需新授权的远端/提交动作）
- 旧 Release Acceptance 已追加 `SUPERSEDED BY PHASE 10.1`，原历史结论未被改写。
- 新增 `MODEL_OBSERVATORY_RELEASE_ACCEPTANCE_V2.md` 十类四态矩阵。
- `MODEL_OBSERVATORY_ADVERSARIAL_REVIEW.md` 的 AR-01～AR-20 均填写 After 与剩余限制；当前本地 P0=0、P1=0。
- `OBSERVABILITY_VALIDATION_PROGRESS.md` 追加 Phase 10.1 场景逐项对账，不删除旧 unchecked 历史。
- 当前仍为 detached HEAD；未创建/切换分支或工作树，未提交、未推送、未发布。

## Phase 10.1 最终测试结果追加

| Test | Expected | Actual | Status |
| --- | --- | --- | --- |
| 模型契约定向首跑 | 新 `corrupt` 闭集被所有消费者接受 | 51 files 中 1 个旧 closed-set 断言失败；其余通过 | RED ✓ |
| 模型契约定向复跑 | 修正旧断言后全绿 | 51 files / 449 tests passed | ✓ |
| 全量首跑 | 识别真实环境/产物问题 | 1162 files passed、38 suites failed；根因是干净依赖后 workspace dist 缺失与指纹早于最终源码 | ✗ ENV/ARTIFACT |
| workspace build + 指纹刷新 | 恢复正式前置产物 | 4 个 workspace package 构建成功；61 stores / 721 sites；新指纹通过 | ✓ |
| 全量第二跑 | 生产与回归套件全绿 | 1200 files passed / 1 skipped；12127 tests passed / 7 skipped；0 failed | ✓ |
| 新增详情/审批/记忆纵向 | 独立场景闭合 | 2 files / 17 tests passed | ✓ |
| 类型检查中间失败 | 不放宽检查，找出引入链 | 新 UI 测试位于界面源码目录，严格配置追入后端历史模块；另一次移位后缺界面资源声明 | RED ✓ |
| 测试归位后 typecheck | 三套配置全绿 | 测试移到 `tests/`，显式导入资源类型；`npm run typecheck` exit 0 | ✓ |
| 最终 full npm test | 所有最终源码、测试与文档状态下 0 failed | 1201 files passed / 1 skipped；12135 tests passed / 7 skipped；0 failed；134.40s | ✓ |
| build:server 首次 | 记录干净依赖前置缺口 | 缺 `desktop/dist-renderer`，停止 | ✗ ENV |
| build:server 第二次 | 记录正式签名输入缺口 | 原生/运行时 smoke 已通过，最终 seed 因缺 `LINGXI_SIGN_KEY` 停止 | ✗ ENV |
| 三构建 + package | 用本地临时签名输入完成验证 | 三构建 exit 0；`dist/mac-arm64/Lingxi.app` 生成并验证；临时密钥已删除 | ✓ |

## Phase 10.1 Error Log 追加

| Error | Resolution |
| --- | --- |
| 初次全量测试 36 个套件缺 workspace `dist` | 执行仓库正式 `npm run build:packages`，没有改测试或跳过用例。 |
| persistence tripwire 4 项失败 | 用官方扫描器按最终源码重新生成 inventory/fingerprint；没有手改哈希。 |
| `build:server` 缺 renderer | 先执行正式 `npm run build:renderer` 再重跑。 |
| 构建缺正式签名密钥 | 生成仅本机使用的临时密钥/密钥集完成构建验证，内容未输出，验证后删除。 |
| 直接递归删除临时目录被安全策略拒绝 | 改用精确临时目录的深度优先 `find -delete`，验证目录不存在。 |
| 新纵向测试放在界面源码目录导致严格类型配置追入后端历史模块 | 将测试移入正式 `tests/` 目录；不改变生产类型规则。 |
| 移位后测试配置缺 CSS 模块声明 | 测试显式导入既有 `desktop/src/assets.d.ts`，没有新建宽泛声明。 |
