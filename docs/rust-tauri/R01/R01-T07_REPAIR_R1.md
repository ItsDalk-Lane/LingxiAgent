# R01-T07 REPAIR R1｜按独立验收 REVIEW_R1（FAIL）修复报告

- 修复代理：ZCode:R01-T07-repair-r1（全新修复代理，未参与此前执行/验收）
- 日期：2026-09-25
- 基线：分支 `codex/rust-tauri-migration`，HEAD `82870879d02b23a03c65fcf6cb9fb03b1425012b`
- 输入：`R01-T07_REVIEW_R1.md`（FAIL，发现 F1/F2/F3）、ADR-004、DATA_COMPATIBILITY_MATRIX.json、
  R01-T07_REPORT.md、ROLLBACK_DESIGN.md
- 范围纪律：只改 R01 文档/矩阵/证据；**零生产代码改动**（`git diff HEAD` 为空，含
  `core/data-epoch-coordinator.ts`、`server/index.ts` 等全部生产文件）；不 commit/push；
  不触碰任务书目录、.sync-audit、ORCHESTRATOR_PROGRESS.json、既往验收报告原文；
  既有证据与负向用例全部保留未删未改（原 a13/a14/snapshots/EVIDENCE_SHA256.txt 逐字节不动）。
- 数据纪律：全部探针 LINGXI_HOME 指向 `/tmp/lingxi-r01t07-repair-r1/homes/` 合成目录；
  代理环境变量以 `env -u` 剥离；仅 loopback 健康探测；两轮探针窗口内真实 `~/.lingxi`
  被修改文件数 = 0（`find ~/.lingxi -mmin -N -type f` 实测）。

## 1. F1 根因（源码确证 + 自建样本复测）

验收判定依据：ADR-004 §2 称闸软化条件为「无更高 epoch 证据（无可读高位印章、**无指向
高位 epoch 的过渡**）」，但验收构造的 R7/R9/R10 三变体在**存在可读高位过渡日志**时
同样 fail-open。根因链（本代理逐行复核，与验收一致）：

1. `server/index.ts:325-329`：`mustBlock = DATA_EPOCH > 1 || hasHigherStamp ||
   hasHigherTransition`，其中 `hasHigherTransition` 取 `epochResult.toEpoch > DATA_EPOCH`。
2. `core/data-epoch-coordinator.ts:547-550`：`inspectDataEpochMaintenance` 返回
   corrupt 类（corrupt-stamp/corrupt-journal/corrupt-transition）时，
   `return failure(homeDir, maintenance.reason, maintenance.detail)` —— **不携带**
   日志的 from/toEpoch。incomplete 类（`incomplete-transition`）则携带 toEpoch
   （同文件 572-578 行区域）。
3. 推论：corrupt 类 failure 下 `hasHigherTransition` 恒 false；无完好高位印章时
   mustBlock=false → fail-open。`barrier_raised` 日志与缺失印章状态矛盾
   （`transitionConsistency` 判 invalid → corrupt-transition，coordinator 290-307 行），
   故 R10 落 corrupt 类放行；`prepared` 日志与缺失印章不矛盾 → incomplete-transition
   携带 toEpoch=2 → R11 拒写。**无印章时拒写取决于日志 phase**，与验收观察一致。
4. fail-open 警告文本固定输出 "no higher-epoch evidence was found"
   （server/index.ts:331-334），在 R7/R10 场景下目录内即有可读高位过渡日志——
   文本与事实相反。

本代理**独立重跑**（非抄验收日志）：样本由 `repair-r1/scripts/build-samples.cjs` 构造，
合法印章/日志一律经生产 schema 构造器 `shared/data-epoch.cjs` 的 createDataEpochStamp /
createDataEpochJournal 生成并经生产 reader 校验通过（校验输出见 §7）；损坏样本为对应
合法载荷的截断（torn write）。探针 `probe-variant.zsh` 复用 T07 方法论（截断一次后全员
O_APPEND，不重演 fd 竞争缺陷），每变体前后双快照（listing+SHA-256）。

两轮实测（round1=修复前留证，round2=文档修正后复跑确认陈述与实测一致；两轮结果逐
项一致）：

| 变体 | 样本 | 闸标记 | STARTED | 退出码 | FS diff | 新写入文件 |
|---|---|---|---|---|---|---|
| C1 对照 | 完好 epoch-2 v2 印章 | `LINGXI_DATA_EPOCH_BLOCKED reason=epoch-downgrade-blocked` | 0 | 1 | IDENTICAL | 0 |
| R7 | 截断损坏印章 + 合法 barrier_raised 日志(toEpoch=2) | `BASELINE_WARNING reason=corrupt-stamp` | 1 | 0 | CHANGED | 55 |
| R9 | 截断损坏日志 + 无印章 | `BASELINE_WARNING reason=corrupt-journal` | 1 | 0 | CHANGED | 55 |
| R10 | 合法 barrier_raised 日志(toEpoch=2) + 无印章 | `BASELINE_WARNING reason=corrupt-transition` | 1 | 0 | CHANGED | 55 |
| R11 | 合法 prepared 日志(toEpoch=2) + 无印章 | `..._TRANSITION_INCOMPLETE reason=incomplete-transition` | 0 | 1 | IDENTICAL | 0 |

警告文本失实复证（round1/round2 R7、R10 日志均含）：「…could not be trusted (…);
**no higher-epoch evidence was found**, so ordinary startup will continue.」——同目录
`data-epoch-transition.json` 即为可读高位证据。

## 2. 修复内容（逐发现）

### F1（Medium）→ 已修复（文档边界修正 + 变体登记 + 缺陷立项）

1. **ADR-004 §2 软化边界陈述改写为实测边界**：mustBlock 实际公式、corrupt 类 failure
   丢弃 toEpoch 的机理、「印章损坏/缺失时有可读高位过渡日志也 fail-open」、无印章时
   拒写取决于日志 phase、警告文本失实——全部如实写明；§2 探针表新增 R7/R9/R10/R11
   四行（含证据路径与写入条目数）；D2 第一层结论与缺口清单同步修正。
2. **R7/R9/R10/R11 + C1 对照实测证据**：本代理自建样本独立复跑两轮（见 §1 表），
   证据树 `artifacts/rust-tauri/R01/T07/repair-r1/`（样本副本、日志、前后快照、
   fs-diff 摘要、逐文件 SHA-256 清单）。
3. **生产缺陷立项**（见 §5）：写入 ADR-004 §5 与本报告，归属候选 R02；R01 阶段
   冻结生产行为，不改生产代码。
4. **修正后复跑**：round2 全组 5 变体结果与 round1 逐项一致，修正后的 ADR 陈述
   与实测行为一致。
5. 同步修正：DATA_COMPATIBILITY_MATRIX.json 的 `protected-if-stamp-intact` note
   原文含同一错误边界（"higher-epoch transition journal -> exit 1"），经生成器
   修正后重生成（见 §4）；R01-T07_REPORT.md 新增 §7 勘误段（原文保留不删）。

### F2（Low）→ 已修复（如实补登）

ADR-004 D2 缺口清单与 ROLLBACK_DESIGN §1 新增第 5 条：印章篡改窗口（验收 ADV-B）
实证闸为**协作式**——手动改印章 2→1 后旧程序真实写入 96 条目、改回后恢复拒写；
该定性支持 D2「分离根必需」结论，不与任何既有声明冲突。

### F3（Low，移交 R09）→ 已修复（登记 + 挂负责人）

ADR-004 §5 新增 R09 强制项、DATA_COMPATIBILITY_MATRIX.json 6 个 shell-local 存储的
epoch_protection note 追加风险段：6 个 shell-local 路径在共享数据根内且不过闸，
新旧两壳会在指针目标根内争抢同路径文件；R09 负责——新壳 shell-state 须落共享根之外
或按壳隔离路径，「指针指向 epoch-2 时禁止旧壳启动」升格为安装链验收关卡。

## 3. 矩阵修复与确定性证据

- 生成器 `r01_t07_build_matrix.py` 仅改两处 note 文本与 generated_by/source_inputs
  元数据；重生成后矩阵变更范围（`repair-r1/logs/matrix-change-scope.txt`）：
  69 存储中 64 个仅 `epoch_protection` note 变化（58 protected-if-stamp-intact=F1
  边界修正 + 6 shell-local=F3 风险段），**其余字段 0 变化**；counts 逐键一致；
  策略/回滚/分类全部不变。
- 双跑确定性：生成器连跑两次，输出逐字节一致
  （`repair-r1/logs/matrix-determinism-diff.txt`，0 行，diff exit 0）；
  run1/run2 日志 exit 均为 0。

## 4. 复跑命令与退出码

```bash
# 样本构造（exit 0；生产 reader 校验：R7/R10 合法日志 barrier_raised->2，
# R11 prepared->2，损坏样本按预期判 corrupt）
node artifacts/rust-tauri/R01/T07/repair-r1/scripts/build-samples.cjs        # exit 0
# 探针两轮（每轮 5 变体；变体退出码见 §1 表，脚本自身 exit 0）
zsh artifacts/rust-tauri/R01/T07/repair-r1/scripts/run-all.zsh round1        # exit 0
zsh artifacts/rust-tauri/R01/T07/repair-r1/scripts/run-all.zsh round2        # exit 0
# 矩阵重生成 + 双跑确定性
python3 docs/rust-tauri/R01/r01_t07_build_matrix.py                          # exit 0 (x2)
diff <run1输出> docs/rust-tauri/R01/DATA_COMPATIBILITY_MATRIX.json           # exit 0（逐字节一致）
```

## 5. 生产缺陷立项条目（移交，不在 R01 修复）

- **标题**：data-epoch 闸 corrupt 类 failure 丢弃过渡日志 toEpoch，致高位过渡证据
  下 fail-open 且警告文本失实。
- **位置**：`core/data-epoch-coordinator.ts:547-550`（corrupt 类 failure 不携带
  from/toEpoch）；`server/index.ts:320-329`（mustBlock 的 hasHigherTransition 因此恒
  false）；`server/index.ts:331-334`（警告文本固定声称无高位证据）。
- **重现**：本报告 §1 的 R7/R9/R10 探针（repair-r1 证据树两轮留证，样本构造器与
  探针脚本可复跑）。
- **影响**：DATA_EPOCH=1 基线下，印章损坏/缺失时即使存在可读高位过渡日志，旧程序
  仍 fail-open 写入（显式警告而非静默）；运行期警告文本误导操作者。战略层面已被
  ADR-004 D2 第二层（物理分离根）覆盖，不推翻 R01 结论。
- **修复方向**：corrupt 类 failure 在日志可读时携带 from/toEpoch（或 server 侧在
  corrupt failure 下补读日志再判 mustBlock），使「有可读高位过渡即拒写」与代码注释
  自述意图一致；同步修正警告文案为如实分类。
- **归属候选**：R02（Rust 存储阶段）——该阶段落地真实存储迁移，理应一并修复闸语义
  并回归 `tests/data-epoch*.test.*` 全组；若总控认为应提前，可单独立项。
- **R01 不改的理由**：R01 阶段任务边界为文档/矩阵/证据，冻结生产行为；此修复触及
  运行期闸语义与文案，需独立授权、专项回归与发布评估，不属于本修复任务范围。

## 6. 交付物清单与 SHA-256（修复后重算）

| 文件 | SHA-256 |
|---|---|
| docs/rust-tauri/R01/ADR-004-storage-cutover.md | f30d1998c606c37d98aedae92c38b94a336d979bfa4e5167359dd313e8c774c9 |
| docs/rust-tauri/R01/DATA_COMPATIBILITY_MATRIX.json | 458bb2da36e2f14f4100b985e6ceb5480fc1b5f947a441aced41077331acbbb1 |
| docs/rust-tauri/R01/ROLLBACK_DESIGN.md | 96a102b75b6b2a37fd465048c3082e57f9c6e54d349bfb234fc492eef49b472f |
| docs/rust-tauri/R01/R01-T07_REPORT.md | b11743bdbcc20991ebdbd28a4562780f3cff5be6cdc35583713877382d9239ac |
| docs/rust-tauri/R01/r01_t07_build_matrix.py | de6e41b781726fde33224d03b31ed5fe3d958309356c5fff3541113aa8c918af |
| docs/rust-tauri/R01/R01-T07_REPAIR_R1.md（本文件） | 见返回总控消息（成文后哈希随附） |
| artifacts/rust-tauri/R01/T07/repair-r1/（新增证据树） | 逐文件见 `repair-r1/EVIDENCE_SHA256.txt` |

修复前交付物哈希（R01-T07_REPORT.md §1 表）为历史值，保留不删；既往证据树
`artifacts/rust-tauri/R01/T07/` 下 repair-r1 以外的全部文件（含原 EVIDENCE_SHA256.txt
覆盖的 90 个文件）本次未增删改。

## 7. 工作区与边界声明

- `git diff HEAD` 为空：零生产代码、零既有跟踪文件改动（所有 T07 交付物本即为未跟踪
  新文件，本次在其上修订）。
- 未触碰：任务书目录、.sync-audit、ORCHESTRATOR_PROGRESS.json、R01-T07_REVIEW_R1.md
  原文、T06 遗留目录、既往证据与负向用例。
- 未跑全量 npm test / typecheck：本次改动为 Markdown/JSON/文档生成器（python）与
  证据脚本，无 TypeScript/生产行为受影响面；矩阵经生成器双跑确定性验证。
- 结论：**READY_FOR_REVIEW**。
