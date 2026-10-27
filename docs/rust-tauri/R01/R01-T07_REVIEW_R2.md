# R01-T07 REVIEW R2｜独立复验报告（对 REVIEW_R1 FAIL 的 repair-r1 修复）

- 复验代理：ZCode:R01-T07-review-r2（全新独立复验，未参与此前执行/R1 验收/修复；只读审查 +
  独立复跑，临时验证全部在 `/tmp/lingxi-r01t07-review-r2`，仓库内唯一新增文件=本报告）
- 日期：2026-09-25
- 基线：分支 `codex/rust-tauri-migration`，HEAD `82870879d02b23a03c65fcf6cb9fb03b1425012b`
- **最终判定：PASS**（F1/F2/F3 全部关闭；核心机制以自建样本独立复跑逐项复证；既往证据未弱化；
  零生产改动；真实用户数据零接触）

## 1. 工作区与基线核实

- `git rev-parse HEAD` = 82870879d…012b（与任务书基线一致）。
- `git status --porcelain` 14 行全部 `??`：T06 遗留未跟踪构建目录 6 个（不属候选，仅记录，
  未删除）+ T07 交付物（ADR-004 / DATA_COMPATIBILITY_MATRIX.json / ROLLBACK_DESIGN.md /
  R01-T07_REPORT.md / R01-T07_REVIEW_R1.md / R01-T07_REPAIR_R1.md / r01_t07_build_matrix.py /
  artifacts/rust-tauri/R01/T07/）。无 tracked 文件改动；`git diff HEAD` 为空（零生产改动，
  含 `core/data-epoch-coordinator.ts`、`server/index.ts`）。
- 交付物 SHA-256 复算与 REPAIR_R1 §6 表逐项一致：ADR-004=f30d1998…c774c9、
  ROLLBACK_DESIGN=96a102b7…49b472f、REPORT=b11743bd…9239ac、build_matrix.py=
  de6e41b7…c918af、DATA_COMPATIBILITY_MATRIX.json=458bb2da…acbbb1。
- 真实家目录零接触：复跑全程约 12 分钟窗口内 `find ~/.lingxi -mmin -45 -type f` = 0 文件
  （基线 56629 文件未动）。
- 环境：Node v24.16.0、Python 3.14.3；所有探针 `env -u` 剥离代理变量；健康检查仅 loopback。

## 2. F1 关闭判定（关键项）：自建样本独立复跑 + ADR-004 §2 逐句核对

### 2.1 样本构造（独立于 repair-r1，不复制其样本）

样本构造器 `/tmp/.../scripts/build-samples.cjs`：合法印章/日志一律经**生产** schema 构造器
`shared/data-epoch.cjs` 的 `createDataEpochStamp`/`createDataEpochJournal` 生成（transitionId/
版本号与 repair-r1 样本不同），落盘后**再经生产 reader**（`readDataEpochStamp`/
`readDataEpochJournal`）校验，输出：合法样本全部 `status=ok`（phase/toEpoch 符合设计），
损坏样本（合法载荷半截断的 torn write）全部 `status=corrupt`。构造依据因此是生产校验逻辑
本身，非人工猜测。

### 2.2 探针实测（旧程序=当前 HEAD `node server/main-full.ts` 直接入口；LINGXI_HOME 指向
合成目录；每变体前后 listing+SHA-256 双快照）

| 变体 | 样本（生产 reader 校验态） | 闸标记 | STARTED | 退出码 | FS diff | 新写入 |
|---|---|---|---|---|---|---|
| c1 | 完好 epoch-2 v2 印章 | `LINGXI_DATA_EPOCH_BLOCKED reason=epoch-downgrade-blocked` | 0 | 1 | IDENTICAL | 0 |
| r7 | torn 印章 + 合法 barrier_raised 日志(to=2) | `BASELINE_WARNING reason=corrupt-stamp` | 1 | 0 | CHANGED | 55 |
| r9 | torn 日志 + 无印章 | `BASELINE_WARNING reason=corrupt-journal` | 1 | 0 | CHANGED | 55 |
| r10 | 合法 barrier_raised 日志(to=2) + 无印章 | `BASELINE_WARNING reason=corrupt-transition` | 1 | 0 | CHANGED | 55 |
| r11 | 合法 prepared 日志(to=2) + 无印章 | `..._TRANSITION_INCOMPLETE reason=incomplete-transition` | 0 | 1 | IDENTICAL | 0 |
| half2 | 屏障印章(min=2,committed=1) + 合法 barrier_raised 日志 | `..._TRANSITION_INCOMPLETE reason=incomplete-transition` | 0 | 1 | IDENTICAL | 0 |
| r8 | 屏障印章(min=2,committed=1)、无日志 | `..._TRANSITION_INCOMPLETE reason=inconsistent-transition-state` | 0 | 1 | IDENTICAL | 0 |
| ctrl | epoch-1 印章对照 | 无标记 | 1 | 0 | CHANGED | 56（全落合成 home，`server-node.json` 等回读证明 LINGXI_HOME 实际生效） |
| escape-on | epoch-2 印章 + `LINGXI_ALLOW_DATA_DOWNGRADE=1` | 降级警告后启动 | 1 | 0 | CHANGED | 55 |
| escape-off | epoch-2 印章、不设逃生口 | `..._BLOCKED reason=epoch-downgrade-blocked` | 0 | 1 | IDENTICAL | 0 |

与 repair-r1 声明逐项一致（C1=1/R7=0/R9=0/R10=0/R11=1；fail-open 各写入 55 新条目——
独立构造的样本写入数与 repair-r1 完全相同）。警告文本失实复证：r7/r10 日志均含
「…could not be trusted (…); **no higher-epoch evidence was found**, so ordinary startup
will continue.」而同目录 `data-epoch-transition.json` 即为生产 reader 可读的高位证据。

### 2.3 ADR-004 §2 修正后边界陈述逐句核对（声明 ≤ 实测）

- 「mustBlock = DATA_EPOCH>1 || 存在完好可读的高位印章 || epochResult.toEpoch > DATA_EPOCH
  （server/index.ts:320-329）」——源码逐行核实（hasHigherStamp/hasHigherTransition/
  mustBlock 在 325-329 行）。✔
- 「corrupt 类 failure 在 core/data-epoch-coordinator.ts:547-550 丢弃日志 from/toEpoch」——
  源码 549 行 `return failure(homeDir, maintenance.reason, maintenance.detail)` 确无
  from/toEpoch；incomplete 类（572-578 行区域）携带。✔
- 「印章损坏/缺失时有可读高位过渡日志也 fail-open（R7/R9/R10）」——本代理 r7/r9/r10
  实测复证。✔
- 「无印章时拒写取决于日志 phase：prepared→incomplete 拒写（R11）；barrier_raised→
  corrupt-transition 放行（R10）」——r10/r11 实测复证；checkpoint_complete 分支经
  `transitionConsistency`（coordinator 282-310）源码核实与 prepared 同类。✔
- 「警告文本与事实相反」——本代理日志逐字复证。✔
- 「DATA_EPOCH>1 后所有闸失败重新转硬」——源码级陈述（mustBlock 首析取项），如实标注
  为公式而非探针实测。✔
- 旧错误表述「无指向高位 epoch 的过渡」已不在 ADR-004 中（grep 核实；REPORT §7 勘误段
  中的出现是对旧文的显式引用并声明作废，勘误体例正确）。✔
- 未发现任何残留「声明>实测」表述；D2 第一层覆盖声明（完好高位印章 A1/A2 或
  incomplete 类高位过渡 A3b/R11 才拒写）与本代理 c1/r11/half2 实测一致。

### 2.4 生产缺陷立项条目完整性

ADR-004 §5 与 REPAIR_R1 §5 的立项条目含：根因行号（coordinator 547-550、server
325-329/331-334，均经本代理核实准确）、警告文本失实、影响评估、修复方向、归属候选
R02（Rust 存储阶段）、R01 不改生产的理由（阶段冻结生产行为、需独立授权+专项回归+
发布评估）。完整。✔

## 3. A13 核心复跑

见 §2.2 表：高位印章拒写（c1：exit 1 + FS 逐字节一致）；半途过渡拒写（half2：屏障印章+
合法 barrier_raised 日志 exit 1 零变化；r8：屏障印章无日志 exit 1 零变化）；对照启动
（ctrl：STARTED=1 exit 0，写入只落合成 home）；逃生口默认关闭（escape-off 不设变量即
exit 1 拒写；escape-on 显式 `LINGXI_ALLOW_DATA_DOWNGRADE=1` 才警告后启动）。全部满足
A13 通过条件，证据=退出码+前后快照（/tmp 留档）。

## 4. A14 演练复跑

将执行者脚本（rollback-drill-setup.py / rollback-drill.py / atomic-switch.zsh /
run-start-probe.zsh）拷至 /tmp 并仅 patch 证据输出路径（不触碰仓库证据树）。世界构造后
由**复验方注入**执行者世界之外的第 5 条 post-cutover 消息 m-103（`wal_autocheckpoint=0`，
-wal 保持 8272 字节热态）。结果：

```
step1 PRECHECK ok / step2 W06-BACKUP ok: integrity_check=ok rows=5 post_cutover_rows=3
step3 EXPORT ok: archive entries=4 / step4 RESTORE ok (atomic rename swap)
step5 OLD-BINARY ok: STARTED=1 on restored epoch-1; epoch-2 root byte-identical
step6 ARCHIVE-VERIFY ok: 4 files sha256-verified / step7 REIMPORT-IDEMPOTENT ok
ROLLBACK-DRILL-PASSED  (drill exit=0)
```

m-103 真实进入备份与 JSONL 导出（Online Backup + wal_checkpoint 静止点有效）；全程无
「删新目录」步骤（脚本仅 rmtree 自建的恢复目标根与 reimport 靶库）；恢复根独立、旧程序
在其上启动、epoch-2 根逐字节不变、再导入两次逻辑内容哈希稳定。检查表（ROLLBACK_DESIGN
§4）核心步骤逐项对应演练 step1-7。满足 A14 通过条件。

## 5. F2/F3 关闭核对

- F2（协作式闸定性）：ADR-004 D2 缺口清单第 3 条与 ROLLBACK_DESIGN §1 第 5 条均已登记
  印章篡改窗口实证（2→1 篡改写入 96 条目、改回恢复拒写），定性「闸是协作式的、物理分离
  必需」入档且与 D2 结论一致。✔
- F3（挂 R09）：ADR-004 §5「R09 新增强制项」条目真实存在（负责人 R09、新壳 shell-state
  落共享根之外或按壳隔离、「指针指向 epoch-2 时禁止旧壳启动」升格为安装链验收关卡）；
  矩阵 6 个 shell-local 存储（desktop-diagnostics/desktop-gpu-startup-state/
  desktop-update-channel/desktop-win32-install-acl-heal-state/
  desktop-window-version-state/signed-artifacts）的 epoch_protection note 均含
  「RISK (REVIEW_R1 F3, owner R09)」风险段（计数核实=6）。✔

## 6. 矩阵一致性

- 与 R00 STORES.json 双向差集为空：69/69 全覆盖，无伪造 ID；counts 与声明一致
  （策略 11/35/13/6/4；保护 58/4/1/6）。✔
- note 修正抽查 3 个：session-jsonl 与 agent-memory（protected-if-stamp-intact）新表述
  「corrupt 类丢弃 toEpoch → 印章损坏/缺失时有可读高位日志也 fail-open；无印章时拒写
  取决于 phase」与本代理实测一致；desktop-window-version-state（shell-local）R09 风险段
  与 F3 定性一致。note 计数核实：58 个含 MEASURED BOUNDARY、6 个含 R09 风险段，与
  repair-r1「64 个 store 仅 note 变化」的变更范围声明相容。✔
- 生成器确定性：拷贝生成器至 /tmp、仅 patch 输出路径后连跑两次 exit 0，双跑 diff 为空，
  且与已交付矩阵**逐字节一致**（SHA-256 同为 458bb2da…acbbb1）。✔

## 7. 既往证据未弱化

- 原 EVIDENCE_SHA256.txt（90 文件）`shasum -c` **全量** 90/90 OK（非抽查）。
- repair-r1/EVIDENCE_SHA256.txt（89 文件）**全量** 89/89 OK。
- repair-r1 两轮探针日志抽查：round1/round2 五变体 STARTED/EXIT_CODE 逐项一致，且与本
  代理独立复跑结果一致；REVIEW_R1 原文未改动（哈希留存）。

## 8. 门禁与工作区纪律

- T01：`python3 docs/rust-tauri/R01/r01_t01_check_ownership.py` → RESULT: OK（exit 0）。
- T02：`zsh scripts/rust-tauri/r01-t02-check-generated.sh` → 56 生成文件无漂移 +
  API_COMPAT_MATRIX 624 条一致，全绿（exit 0）。
- 复跑后 `git diff HEAD` 仍为空、`git status --porcelain` 与基线一致（除本报告外无新增）。
- 真实家目录零接触（§1）。已知预存失败与本任务无关，未重跑全量 npm test/typecheck
  （本次候选改动为 Markdown/JSON/Python 生成器与证据，无 TS/生产行为受影响面）。

## 9. 新发现问题

无阻断性新发现。一条佐证性观察（不构成新发现、不要求修复）：复验中曾构造「完好 epoch-1
（sourceSteady）印章 + 合法 barrier_raised 日志」组合，实测 corrupt-transition fail-open——
与修正后 ADR §2 的**机理级**陈述（corrupt 类 failure 恒不携带 toEpoch、mustBlock 第二条
只看**高位**印章）精确相容，且源码核实该组合在真实过渡中不可达（coordinator 先写屏障印章
后写 barrier_raised 日志，崩溃窗口落在 checkpoint_complete+屏障印章这一可拒写形态），
仅可由篡改/损坏人为造成，已由 D2 第二层覆盖。

## 10. 结论

- F1 关闭：ADR-004 §2 边界陈述已改为实测边界，与本代理独立复跑逐句一致；R7/R9/R10/R11
  +C1 变体登记在档且复现；生产缺陷立项条目完整移交 R02。
- F2/F3 关闭：协作式闸定性入档；R09 条目在 ADR §5 与矩阵中真实存在。
- A13/A14 核心复跑全绿；矩阵 69 全覆盖、note 与实测一致、生成器双跑逐字节确定；
  既往证据 90/90 + 89/89 全量哈希通过；T01/T02 门禁复跑通过；零生产改动；
  真实用户数据零接触。
- **最终判定：PASS**。
