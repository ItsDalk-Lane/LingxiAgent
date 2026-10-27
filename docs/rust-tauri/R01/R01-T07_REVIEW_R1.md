# R01-T07 REVIEW R1｜独立对抗性验收报告

- 验收代理：ZCode:R01-T07-review-r1（未参与执行；只读审查 + 独立复跑，临时验证全部在 /tmp 隔离目录）
- 日期：2026-09-25
- 基线：分支 `codex/rust-tauri-migration`，HEAD `82870879d02b23a03c65fcf6cb9fb03b1425012b`
- **最终判定：FAIL**（证据真实、核心机制复跑全绿；但交付物 ADR-004 对 epoch 闸软化边界的陈述
  与实际行为不符，对抗探针发现 3 个未登记 fail-open 变体——见 F1。修复范围窄、同根因，见 §6）

## 1. 候选清单与复算哈希

| 交付物 | 报告声明 SHA-256 | 复算 | 结论 |
|---|---|---|---|
| docs/rust-tauri/R01/ADR-004-storage-cutover.md | 572f9ae1…b66a36ea | 一致 | OK |
| docs/rust-tauri/R01/DATA_COMPATIBILITY_MATRIX.json | 5ce9fb12…c869422 | 一致 | OK |
| docs/rust-tauri/R01/ROLLBACK_DESIGN.md | 243ac959…d57f504 | 一致 | OK |
| docs/rust-tauri/R01/r01_t07_build_matrix.py | f4e0d5cd…1ad2ef6 | 一致 | OK |
| artifacts/rust-tauri/R01/T07/（90 文件） | EVIDENCE_SHA256.txt | `shasum -c` 90/90 OK | OK |

工作区核实：`git status --porcelain` 与任务书预期完全一致（T06 遗留未跟踪构建目录 + T07 交付物，
无其他）；`git diff HEAD` 为空（零生产代码改动）；`git diff HEAD -- rust` 为空。
真实家目录零接触：复跑全程前/后对 `~/.lingxi`（56629 文件）做全量 SHA-256 快照，diff 为 0。

## 2. 独立复跑（自建样本与探针，全部 /tmp/lingxi-r01t07-review 隔离）

方法：自写探针框架（snap.zsh/probe-block.zsh/probe-start.zsh），旧程序=当前 HEAD
`node server/main-full.ts` / `node cli/entry.ts`（直接入口，LINGXI_HOME 指向合成目录；
代理环境变量全部 `env -u` 剥离；健康检查仅 loopback）。每个拒写场景做文件系统前/后
listing+SHA-256 双快照对比。

### 2.1 A13 拒写矩阵（退出码均为真实观测）

| 探针 | 场景（自建样本） | 结果 | FS 快照 | 结论 |
|---|---|---|---|---|
| R1 | epoch-2 v2 印章 | exit 1 `LINGXI_DATA_EPOCH_BLOCKED reason=epoch-downgrade-blocked` | 逐字节一致 | 复证 A1 |
| R2 | legacy v1 印章 `{epoch:2}` | 同上 exit 1 | 逐字节一致 | 复证 A2 |
| R3 | 合法半途过渡日志(barrier_raised,1→2)+屏障印章 | exit 1 `..._TRANSITION_INCOMPLETE reason=incomplete-transition` | 逐字节一致 | 复证 A3b |
| R4 | 对照 epoch-1 印章目录 | STARTED=1 exit 0，写入 96 条目全落沙盒 | 预期内变化 | 复证 A4；LINGXI_HOME 实际生效值经 server-info 落点验证 |
| R5 | epoch-2 + `LINGXI_ALLOW_DATA_DOWNGRADE=1` | STARTED=1，双语降级警告，写入 96 条目；印章本体未被改写 | 预期内变化 | 复证 A5（逃生口真实且默认关闭——不设环境变量即 R1 拒写） |
| R6 | 仅损坏印章 | exit 0，`BASELINE_WARNING reason=corrupt-stamp` 后写入 97 条目 | fail-open | 复证 A8 |
| R7 | **损坏印章 + 合法高位过渡日志(toEpoch=2)** | **exit 0 fail-open 写入 57 条目**；警告文本谎称 "no higher-epoch evidence was found" | 被写入 | **新发现，见 F1** |
| R8 | 印章 min=2/committed=1 且无日志 | exit 1 `reason=inconsistent-transition-state`（经 hasHigherStamp 转硬） | 逐字节一致 | 新变体，行为正确 |
| R9 | **损坏日志 + 无印章** | **exit 0 fail-open** `reason=corrupt-journal` | 被写入 | **新发现，见 F1**（执行者 A3 能拒写仅因其 home 同时含完好高位印章） |
| R10 | **合法 barrier_raised 日志(toEpoch=2) + 无印章** | **exit 0 fail-open** `reason=corrupt-transition` | 被写入 | **新发现，见 F1** |
| R11 | 合法 prepared 日志(toEpoch=2) + 无印章 | exit 1 `reason=incomplete-transition` | 逐字节一致 | 对照：无印章时拒写与否取决于日志 phase |

### 2.2 A13 对抗

| 对抗 | 操作 | 结果 | 结论 |
|---|---|---|---|
| ADV-A1 | `cli data restore forged-id --confirm-token …` 对 epoch-2 home | exit 1「No checkpoint found」，FS 逐字节一致 | 唯一 CLI 写路径无检查点即拒；确认令牌不可跳过（源码+实测） |
| ADV-A2 | `cli data diagnose` 对 epoch-2 home | exit 0，正确读出 minimumReaderEpoch=2，FS 逐字节一致 | 复证 A9（只读维护面） |
| ADV-B | 篡改印章 2→1 → 旧程序写入 96 条目 → 改回 2 → 再启动 | 篡改窗口内真实写入（双写污染成立）；改回后 exit 1 拒写且零变化 | 闸为协作式的直接实证，支持 ADR D2「分离根必需」结论 |
| ADV-C | 逃生口默认值 | 不设 `LINGXI_ALLOW_DATA_DOWNGRADE` 即拒写（R1）；全仓 grep 仅 `cli/server-runner.ts` 在显式 `--allow-data-downgrade` 时注入该变量并打印警告 | 逃生口不会默认可用，声明属实 |
| A7 双向 | 自建 root/{epoch-1,epoch-2} + active symlink，自建 rename 原子切换 | active→epoch-1：旧程序启动、写入只落 epoch-1、epoch-2 逐字节不变；切换 active→epoch-2：exit 1 拒写、两根均零变化 | 独立复证 A7a/b |

### 2.3 A14 回滚演练复跑（加强版）

将执行者脚本拷至 /tmp 并仅改写证据输出路径（不触碰仓库证据树），世界构造后**额外加强**：
保持一个有活写连接的 WAL 热态库（wal_autocheckpoint=0，-wal 文件 8272 字节），并由验收方
注入执行者世界之外的第 5 行 post-cutover 消息 m-103。

```
step1 PRECHECK ok / step2 W06-BACKUP ok: integrity_check=ok rows=5 post_cutover_rows=3
step3 EXPORT ok: archive entries=4 / step4 RESTORE ok (atomic rename swap)
step5 OLD-BINARY ok: STARTED=1 on restored epoch-1; epoch-2 root byte-identical
step6 ARCHIVE-VERIFY ok: 4 files sha256-verified / step7 REIMPORT-IDEMPOTENT ok
ROLLBACK-DRILL-PASSED  (exit 0)
```

- m-103（热 WAL 中未 checkpoint 的行）真实进入备份与 JSONL 导出——证明 Online Backup +
  wal_checkpoint 静止点有效，非裸拷 .db。
- 归档含消息导出/附件/切换收据/manifest，sha256 全绿；恢复根为独立 epoch-1-restored，
  旧程序在其上启动且旧时点 JSONL 可读；全程无「删新目录」步骤；再导入两次逻辑内容哈希稳定。
- 限制同执行者登记：新库为设计级替身，R02 真实 Rust 存储落地后需按 schema 重跑。

## 3. 源码核对（源码确证）

- 闸位置：server/index.ts 同宅互斥（274-291）→ epoch 事务闸（294-355）→ 端口绑定（407+）
  → ensureFirstRun（417）→ engine.init（445，FactStore/会话/记忆等全部 store 在此之后打开）
  → server-info.json 写入（1277）。「闸先于任何 store 打开」成立；抽查 fact-store
  （core/agent.ts:393 经 engine.init）、session 存储（engine 内）、server-runtime-info
  （写于 1277）三个打开路径均在闸后。
- A6 预闸自清复证：快照 diff 显示唯一预闸变更为残留 server-info.json 删除（regenerable）。
- desktop 壳无协调器调用：desktop/main.cjs 中仅两处注释提及，仅识别 server stderr 标记
  （2262-2264）渲染对话框。6 个 shell-local 存储路径在共享数据根内（crash.log、
  user/window-state.json、update-channel.json 等）——见 F3。
- `DATA_EPOCH=1`（shared/contract-versions.json）；mustBlock 逻辑在 server/index.ts:320-329。

## 4. DATA_COMPATIBILITY_MATRIX 核对

- 与 R00 STORES.json 双向差集为空：69/69 全覆盖，无伪造 ID；计数与声明一致
  （策略 11/35/13/6/4；保护 58/4/1/6）。
- 生成器确定性：patch 输出路径后双跑 diff 为空，且与已交付矩阵**逐字节一致**。
- 抽查 5 存储（session-jsonl/server-runtime-info/agent-memory/desktop-window-version-state/
  knowledge 系）分类与读写方与 R00 登记及源码现实一致。

## 5. 披露核实与门禁抽查

- fd 竞争缺陷：原始 a4/a5/a7a 日志在盘且均无 STARTED 行（覆写痕迹），修复版 a4r2/a5r2/a7ar2
  各有 1 行；run-start-probe.zsh 现行「先截断再全员 O_APPEND」与报告 §5.1 描述一致。如实。
- §5.1 覆盖登记：首轮演练日志被成功重跑覆盖一事已在报告登记；无法复核被覆盖内容，
  但登记行为本身存在且与在盘证据无矛盾。
- T01/T02：`git diff HEAD -- rust` 为空；抽查复跑 `r01_t01_check_ownership.py` → RESULT: OK；
  `r01-t02-check-generated.sh` → 56 生成文件无漂移 + API_COMPAT_MATRIX 624 条一致，全绿。
- 验收代理自身事故登记：首轮探针误传相对路径 LINGXI_HOME，旧程序在仓库 `sandbox/` 下
  启动（顺带实证了 provably-new → stamped-new 路径）；已立即删除并经 git status 核实
  工作区恢复原状，未触碰任何交付物与真实数据。

## 6. 发现问题

### F1（Medium，判定依据）——ADR-004 的闸软化边界陈述与实际行为不符；3 个 fail-open 变体未登记

- 最小重现（R10）：构造目录仅含合法过渡日志
  `{transitionId,fromEpoch:1,toEpoch:2,phase:"barrier_raised",migrationIds:["m"],affectedStoreIds:["session-jsonl"],recoveryModes:{m:"resume-idempotent"},checkpointId+receipt,时间戳,lastVersion}`，
  **不放印章**；`LINGXI_HOME=<dir> node server/main-full.ts` → 打印
  `LINGXI_DATA_EPOCH_BASELINE_WARNING reason=corrupt-transition` 后正常启动并写入。
  R7（损坏印章+同一日志）与 R9（损坏日志+无印章）同型。
- 根因：`core/data-epoch-coordinator.ts:548-550` 对 maintenance-corrupt（含 corrupt-stamp/
  corrupt-journal/corrupt-transition）返回 failure 时**不携带日志的 from/toEpoch**；
  `server/index.ts:325-329` 的 `hasHigherTransition = epochResult.toEpoch > DATA_EPOCH`
  因此恒为 false，mustBlock 退化为只看完好高位印章。结果是：只要印章损坏或缺失，
  即使目录里存在**完好可读、明确指向 epoch 2 的过渡日志**，旧程序也 fail-open 写入；
  且无印章时拒写与否取决于日志 phase（prepared/checkpoint_complete 拒=R11，
  barrier_raised 放行=R10）。警告文本「no higher-epoch evidence was found」在 R7/R10 下
  与事实相反。
- 与交付物的冲突：ADR-004 §2 称「当闸失败但无更高 epoch 证据（无可读高位印章、
  **无指向高位 epoch 的过渡**）时仅警告继续」——R7/R10 证明「有可读高位过渡」同样
  fail-open；报告 §3 边界行（「无更高 epoch 证据」括注）同误。该陈述同时是对
  server/index.ts 注释设计意图的转写，即生产代码在此角落也未实现其自述意图。
- 影响评估：行为**显式而非静默**（stderr 警告+机读标记），「未知不能算通过」仅以
  「显式分类后放行并写入」形式满足；ADR D2 已将物理分离定为必需，战略结论不被推翻，
  反而被加强。但 ADR 的全部价值在于精确冻结保护边界，而对抗探针数分钟内即抵达
  该错误角落；运行期警告文本在此情形下误导操作者。
- 同根因完整修复范围（窄）：
  1. 修正 ADR-004 §2 软化边界陈述为实测边界（仅「完好可读高位印章」或「incomplete 类
     failure 且带 toEpoch>1」才转硬；印章损坏/缺失时高位过渡日志**不**阻止 fail-open），
     并在 ADR/矩阵/报告中登记 R7/R9/R10/R11 变体及探针证据；
  2. 对生产缺陷立项（超出本任务零生产改动范围，需另行授权）：coordinator 的 corrupt 类
     failure 携带可读日志的 from/toEpoch，使 server mustBlock 与警告文本反映真实证据；
     或至少修正警告文案。该缺陷移交对应生产修复任务，不由本验收直接改生产代码；
  3. 修复后重跑本报告 §2.1 全组探针。

### F2（Low）——印章篡改窗口（ADV-B）

手动把印章改为 epoch 1 后旧程序真实写入 96 条目，改回后恢复拒写。闸为协作式的直接实证。
ADR 已据此将分离根定为必需，未违反任何声明；作为 D2 必要性的补充实证登记。

### F3（Low，移交观察）——desktop 壳 shell-local 存储位于共享数据根

6 个不过闸存储路径均在数据根内。分离根+指针模型下，若旧 desktop 壳在 active→epoch-2
期间被启动，其 shell-local 写入（窗口态/GPU 态/更新通道/crash.log 等）不受任何闸约束，
且新旧两壳会在同一路径上争抢「各自本地」状态（矩阵称 shell-local 各壳自持，但路径是
共享根相对的）。执行者已如实标注 by-construction 与 GUI 未实测；建议 R09 明确新壳
shell-state 必须落在共享根之外或按壳隔离路径，并将「指针指向 epoch-2 时禁止旧壳启动」
纳入安装链强制项（ADR §5 已有方向，需升格为验收关卡）。

## 7. 结论

- 执行者核心声明全部独立复证：58 存储「印章完好即闸前拒写」（R1/R2/R3/R8/R11 零变化）、
  逃生口真实且默认关闭（R5/ADV-C）、CLI 维护面只读/拒绝（ADV-A1/A2）、分离根双向零接触
  （A7 复跑）、回滚演练 7 步全绿（加强版复跑）、矩阵 69 全覆盖且生成器确定、披露如实、
  零生产改动、T01/T02 门禁抽查通过、真实用户数据零接触。
- 但 F1 表明交付物 ADR-004 对其最核心的机制边界（何时拒写、何时放行）作了与实测不符的
  陈述，且有 3 个同族 fail-open 变体未登记。按「未知不能算通过」与交付物准确性要求，
  判定 **FAIL**。修复范围同根因且窄（§6 F1 三项），修复后本验收的复跑资产
  （/tmp/lingxi-r01t07-review 探针与样本构造方法）可直接用于复验。
