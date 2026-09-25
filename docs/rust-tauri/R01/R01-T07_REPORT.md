# R01-T07 REPORT｜确定存储切换与旧版本拒写策略

- 状态：READY_FOR_REVIEW（执行者结论；PASS/FAIL 判定归独立验收代理）
- 执行代理：ZCode:R01-T07-exec-r1（全新独立执行代理，不参与验收）
- 日期：2026-09-25
- 基线：分支 `codex/rust-tauri-migration`，HEAD `82870879d02b23a03c65fcf6cb9fb03b1425012b`
  （开工 `git rev-parse HEAD` 复核一致；工作区开工时仅有 T06 遗留未跟踪构建目录，
  与 ORCHESTRATOR_PROGRESS.json 均未触碰）

## 1. 交付清单（含 SHA-256）

| 文件 | SHA-256 |
|---|---|
| docs/rust-tauri/R01/ADR-004-storage-cutover.md | 572f9ae17a166dc55454f801eaad9927702bea3cad2056457361a5c6b66a36ea |
| docs/rust-tauri/R01/DATA_COMPATIBILITY_MATRIX.json | 5ce9fb12c9c51a8010299d8fb5d89f098a2ead1feca65cd0d29230553c869422 |
| docs/rust-tauri/R01/ROLLBACK_DESIGN.md | 243ac959256f88c1be38214060885891f84005c9e45f61e44a3a70414d57f504 |
| docs/rust-tauri/R01/r01_t07_build_matrix.py（矩阵生成器） | f4e0d5cd5ce7c7cf0a2e4afef88257399c293a09d60df65ee900306ba1ad2ef6 |
| artifacts/rust-tauri/R01/T07/（90 个证据文件） | 逐文件见 artifacts/rust-tauri/R01/T07/EVIDENCE_SHA256.txt |

注意：交付文件哈希以工作区当前内容为准，独立验收时请重算比对（本报告成文后不再改动
上述四文件之外的交付物；若文档再修订，哈希需重算）。

## 2. 任务书五步执行情况

1. **分域切换策略**：69 存储分五级——switch-to-new-authority 11（运行/消息语义迁入
   Rust 新库）、preserve-format 35（知识/记忆/配置/附件/凭证格式冻结）、regenerable 13、
   shell-local 6、epoch-mechanism 4。逐项见 DATA_COMPATIBILITY_MATRIX.json（机械生成自
   R00 STORES.json + R01 OWNERSHIP_TARGET.json，生成器 r01_t07_build_matrix.py 可复跑）。
2. **旧 JSONL 只读导入**：ADR-004 D1——导入前只读、按会话切换、收据+校验、原始文件
   归档保留、无双向双写。
3. **版本/收据/完整性/停机切换/单写者锁**：ADR-004 D3；旧程序 epoch 拒写**已实测**
   （§3 表）。
4. **旧程序不能识别新 epoch 的替代**：实测结论是当前旧程序**能**识别（印章/过渡日志
   在基线提交前已存在），但闸为协作式——故仍设计并实测了分离数据根 + 原子活动目录
   指针原型（ADR-004 D2 第二层，探针 A7）。
5. **回滚设计**：ROLLBACK_DESIGN.md + 演练脚本真实跑通（ROLLBACK-DRILL-PASSED）。

## 3. R01-A13 旧程序拒写实测（逐场景）

「旧程序」= 当前 HEAD 生产 server/CLI（`node server/main-full.ts` / `node cli/entry.ts`）。
依据：仓库为发布 single source of truth，epoch 机制先于基线提交 d5275e568 存在，HEAD
构建在 epoch 行为上与田间发布逐字节同源。限制：非下载的 DMG；GUI 壳未启动（desktop
侧为源码审计）。launch.js 被有意绕过——`scripts/dev-env.js:11` 强制覆盖 LINGXI_HOME，
直接运行入口并快照验证写入只落在沙盒（01 §8 合规）。全部流量 loopback，代理环境变量
已剥离，无凭证不触发模型调用。

| 场景 | 命令（要点） | 预期 | 实际 | 退出码 |
|---|---|---|---|---|
| A1 epoch-2 v2 印章 | LINGXI_HOME=epoch2-home node server/main-full.ts | 拒写 | `LINGXI_DATA_EPOCH_BLOCKED reason=epoch-downgrade-blocked`；前后快照逐字节一致 | 1 |
| A2 legacy v1 印章 {epoch:2} | 同上 | 拒写 | 同上；零变化 | 1 |
| A3 非法过渡日志 | 同上 | 拒写 | `TRANSITION_INCOMPLETE reason=corrupt-journal`；零变化 | 1 |
| A3b 合法半途过渡日志(toEpoch=2) | 同上 | 拒写 | `TRANSITION_INCOMPLETE reason=incomplete-transition`；零变化 | 1 |
| A4r2 对照 epoch-1 | 同上 | 正常启动 | STARTED=1，loopback 18824 应答，写入只在沙盒 home | 0（SIGTERM 清洁退出） |
| A5r2 epoch-2 + LINGXI_ALLOW_DATA_DOWNGRADE=1 | 同上+覆盖环境变量 | 警告后可写 | 降级警告；**写入 97 个新条目**（逃生口实证） | 0 |
| A6 epoch-2 + 死 server-info.json | 同 A1 | 拒写 | 拒写；但**预闸删除残留锁**（唯一预闸变更，regenerable） | 1 |
| A7ar2 分离根 active→epoch-1 | LINGXI_HOME=active(symlink) | 启动且不触 epoch-2 | STARTED=1；epoch-2 快照逐字节一致 | 0 |
| A7b 原子切 active→epoch-2 | rename 换指针后同 A1 | 拒写 | BLOCKED；epoch-2 零变化 | 1 |
| A7c 指针目标无印章 | 同上 | 如实记录 | `BASELINE_WARNING ambiguous-unstamped-home`，**继续启动并写入** | 0 |
| A8 epoch-2 意图 + 损坏印章 | 同上 | 如实记录 | `BASELINE_WARNING corrupt-stamp`，**fail-open 写入 97 条目** | 0 |
| A9 CLI `data diagnose` 对 epoch-2 | node cli/entry.ts data diagnose | 只读 | 正确读出高位印章；零写入 | 0 |

### epoch 保护覆盖实测表（69 存储）

| 保护状态 | 数量 | 范围 | 证据 |
|---|---|---|---|
| protected-if-stamp-intact | 58 | 全部 server 写入存储（42 authoritative + 13 regenerable + 3 adjacent server-written）：闸先于任何 store 打开 | A1/A2/A3b 零变化 |
| self（闸元数据自身） | 4 | data-epoch-stamp/journal/checkpoints/restore-quarantine | 源码 |
| not-protected-pre-gate | 1 | server-runtime-info（同宅互斥在闸前，死锁自清） | A6 |
| not-gated-by-construction | 6 | desktop 壳写入的 shell-local 存储（desktop/main.cjs 无协调器调用，仅 stderr 标记对话框） | 源码审计（GUI 未启动，如实标注） |

边界（全部实测，非推测）：A5 逃生口、A7c/A8 DATA_EPOCH=1 基线软化 fail-open——
三者构成 ADR-004「分离根为必需而非可选」的直接依据。DATA_EPOCH>1 后闸失败全转硬
（server/index.ts:329 `mustBlock`），但田间旧二进制永远停在 epoch=1 行为，故物理隔离
必须随安装链落地（移交 R09/R11）。

## 4. R01-A14 回滚演练（真实跑通）

脚本（非文档虚构，全部真实执行）：
`artifacts/rust-tauri/R01/T07/scripts/rollback-drill-setup.py`（构造 t0–t3 世界）、
`rollback-drill.py`（七步演练）、`atomic-switch.zsh`（rename 原子指针）、
`run-start-probe.zsh`（旧程序启动探针）。

运行输出（a14/rollback-drill-run.log，exit 0）：

```
step1 PRECHECK ok: active->epoch-2, stamp committed=2
step2 W06-BACKUP ok: integrity_check=ok rows=4 post_cutover_rows=2   (WAL 热态库，wal_checkpoint+Online Backup)
step3 EXPORT ok: archive entries=4                                   (JSONL 导出+附件+收据+sha256 manifest)
step4 RESTORE ok: active -> epoch-1-restored (atomic rename swap)    (独立根恢复，无删新目录步骤)
step5 OLD-BINARY ok: STARTED=1 on restored epoch-1; epoch-2 root byte-identical
step6 ARCHIVE-VERIFY ok: 4 files sha256-verified
step7 REIMPORT-IDEMPOTENT ok: 两次合并逻辑内容 sha256 稳定
ROLLBACK-DRILL-PASSED
```

「无法在旧版继续编辑」清单与保留格式存储的回滚二选一决策点见 ROLLBACK_DESIGN §2/§4。
限制：演练新库为设计级替身（合成 messages.sqlite），R02 真实 Rust 存储落地后需按其
schema 重跑演练；W06 演示用 python sqlite3 backup，生产 Rust(rusqlite) 语义等价性
归 R02 复证。

## 5. 问题 / 阻塞 / 如实登记

1. 探针脚本第一版日志存在 fd 偏移竞争（node 以 `>` 截断打开、tee -a 追加，STARTED 行被
   覆写）——已修复（先截断再全员 O_APPEND）并重跑 a4r2/a5r2/a7ar2；原始 a4/a5/a7a 日志
   保留未删，其 STARTED 行缺失即为该缺陷的在盘痕迹，启动事实由 health.json 与日志内
   `ready: port=` 行佐证。演练首轮因此 DRILL-FAIL 于 step5（该轮日志已被成功重跑覆盖，
   事实经本报告如实登记），修复后全绿。
2. desktop 壳 6 存储的不过闸结论来自源码审计（无头环境未启动 GUI），非探针实测——
   矩阵中逐项标注 `by-construction`。
3. 已知预存失败（4 审计封印 FAIL 等）未追修；未跑全量 npm test（本任务零生产代码改动，
   无受影响面）；未改 `rust/`，T01/T02 门禁无需重跑。
4. 未触碰：任务书目录、.sync-audit、ORCHESTRATOR_PROGRESS.json、T06 遗留未跟踪目录、
   任何真实用户数据（全程 /tmp/lingxi-r01-t07-sandbox 合成数据）。

## 6. 最终工作区状态

见返回总控消息所附 `git status --porcelain`（预期新增：docs/rust-tauri/R01/ 下 4 个
交付文件 + artifacts/rust-tauri/R01/T07/ 证据树；无生产代码改动）。

## 7. 勘误（repair-r1，2026-09-25；按独立验收 R01-T07_REVIEW_R1.md FAIL 修复）

以下勘误不改动上文原始记录，仅作修正与补登；修复全过程与重跑证据见
`R01-T07_REPAIR_R1.md` 与 `artifacts/rust-tauri/R01/T07/repair-r1/`。

1. **F1（Medium）——§3「边界」段与 ADR-004 §2 的闸软化边界陈述错误**。原文称
   软化条件为「无更高 epoch 证据（无可读高位印章、无指向高位 epoch 的过渡）」。
   验收对抗探针 R7/R9/R10 证明该陈述与实测不符：corrupt 类 failure
   （corrupt-stamp/corrupt-journal/corrupt-transition）在
   `core/data-epoch-coordinator.ts:547-550` 丢弃日志 toEpoch，因此**印章损坏/缺失时
   即使存在完好可读、指向 epoch 2 的过渡日志也 fail-open 写入**；无印章时拒写与否
   取决于日志 phase（prepared→incomplete-transition 拒写=R11；barrier_raised→
   corrupt-transition 放行=R10）。fail-open 警告文本 "no higher-epoch evidence was
   found" 在 R7/R10 下与事实相反。repair-r1 以自建样本独立复跑两轮复证全部四个变体
   （R7/R9/R10 各真实写入 55 新条目，R11 拒写零变化，C1 阻断对照零变化）。
   ADR-004 §2/D2/§5 已改为实测边界并补登四变体；§3 上文的探针表与覆盖表本身如实，
   但「边界」段的软化范围表述以此勘误为准。**生产缺陷已立项移交**（候选 R02，条目见
   ADR-004 §5 与 REPAIR_R1 §5）：coordinator corrupt 类 failure 携带可读日志
   from/toEpoch + 修正警告文案。R01 阶段冻结生产行为，不在本阶段改生产代码。
2. **F2（Low）——印章篡改窗口（协作式闸定性）**。验收 ADV-B 实证：手动改印章 2→1
   后旧程序真实写入 96 条目，改回后恢复拒写。ADR-004 D2 与 ROLLBACK_DESIGN §1 已补登
   该定性——软件闸依赖盘上元数据完好，物理分离根因此是必需而非可选；不违反任何
   既有声明。
3. **F3（Low，移交 R09）——desktop 壳 shell-local 存储位于共享数据根内**。6 个
   不过闸的 shell-local 存储路径为共享根相对路径；分离根+指针模型下新旧两壳会在指针
   目标根内争抢同路径文件。已在 ADR-004 §5 与 DATA_COMPATIBILITY_MATRIX.json
   （生成器 r01_t07_build_matrix.py 的 shell-local note）登记，R09 强制项：新壳
   shell-state 落共享根之外或按壳隔离；「指针指向 epoch-2 时禁止旧壳启动」升格为
   安装链验收关卡。
4. 交付物哈希因此次修复全部变化，以 REPAIR_R1 §6 的重算清单为准；上文 §1 的哈希表
   为修复前历史值，保留不删。
