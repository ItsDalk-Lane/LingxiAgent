# R02｜Rust 独立服务、存储与事件基础 — 阶段报告

> **现行状态（2026-09-28 最终收口）**：**R02 ACCEPTED**（独立最终阶段验收
> VERDICT: PASS，见 [R02_FINAL_STAGE_REVIEW_R1.md](R02_FINAL_STAGE_REVIEW_R1.md)）。
> 最终候选 `R02_CANDIDATE_ID = e876171c5cd0a6f8`（HEAD cdd213078 + 授权修复
> 工作树，candidate source digest `96d5aa25…fb846`，11409 文件），完整门禁
> 第 6 轮全绿（fmt/clippy/全测试集/check-contracts/check-boundaries/
> verify-stage overall PASS：16/16 基础场景 + 34 叶 = 25 R02 份额 PASS +
> 9 DEFERRED_TO_R07 + 0 FAIL/BLOCKED、20/20 命令证据干净、候选绑定全程
> stable），证据根 `artifacts/rust-tauri/R02/final-candidate-e876171c5cd0a6f8/`。
> 过程性 Gate 失败轮（a0af83666f89cc4b / a29e5adbb404ad70 / fc9971898df7a9e8）
> 按历史原样保留。生产默认入口仍为 Node/Electron（A16 新语义：默认未切换 +
> 受影响回归无新增失败）。**历史候选失败；最终候选已在当前 SHA 上重新验证
> 通过**；正式审计封印推进（seal 三件套坐标滞后族）留待封印工作流，不属
> 本阶段技术门禁。下方 §1–§30 与各历史轮记录原样保留；[R18 全范围交接]
> (R02_FULL_SCOPE_HANDOFF_R18.md) 转为历史问题表，其未闭合项的处置见 §31。

按 91 交付模板填写。本文是**执行者口径的阶段候选报告**：阶段状态
READY_FOR_REVIEW；最终 PASS/ACCEPTED 归总控另派的独立验收代理。

## 1. 阶段与结论

**READY_FOR_REVIEW**（T01–T07 各任务经独立对抗验收 PASS(R1/R2/R3)；
T08 为本阶段收口任务，执行者自评 READY_FOR_REVIEW，待独立验收）。

16/16 基础验收场景由收口执行者以真实命令全量复跑（xtask verify-stage R02，
overall PASS），状态与证据指针见 `docs/rust-tauri/R02/R02_ACCEPTANCE_LEDGER.json`。

## 2. 范围

- 本阶段授权：R02 任务书 8 项任务（T01 workspace/组合根 → T08 交付与门禁）。
- 与基线差异：TASK_BASE（=T07 收口提交 574198916…）之后仅 T08 的未提交工作树
  （§10 全集）；获准变更 = 任务书 R02-T08 五步 + RISK_REGISTER 明确归属 R02 的
  RR-T08-F1 硬化 + 移交修正清单（T07 F01–F04 / T02 F02 / T04 F01/F02 /
  T06-R2 F05 惯例 / FINFO1），逐项对质表见 R02-T08_REPORT §7。
- 不包含：真实供应商连接、真实高权限工具、生产默认入口替换、新 UI、
  数据迁移（归 R08）。

## 3. 源码

- 起始 SHA：`5741989165fe7e04c9a58a9d35c7747d3599d274`（= 分支
  codex/rust-tauri-migration 远端 HEAD；工作区开工时干净）。
- 终点：同 SHA + T08 未提交工作树（17 个跟踪文件修改 + 11 个未跟踪新文件/
  目录；工作树 digest `94f95e7a53dac99f5cfacea2e6eca3043da6ff20afb261cf9c68b7388ce32332`，
  9707 条目，明细 `artifacts/rust-tauri/R02/T08/working-tree-digest.txt`，
  计算器 `docs/rust-tauri/R02/r02_t08_worktree_digest.py`）。
- 依赖锁摘要：`rust/Cargo.lock` 5c43390c894421e6（T08 仅 +7 行 xtask 成员块，
  零新增第三方包/版本）；`rust-toolchain.toml` eec3410410647a7d（1.98.1）；
  `package-lock.json` e54a16fe14f15b47（未动）。

## 4. 环境

- macOS 27.0.0 arm64；rustup 1.29.1 + 锁定 rustc/cargo 1.98.1（组件
  rustfmt/clippy）；cargo 全程 `--locked` + 离线 + 专属
  `CARGO_TARGET_DIR=/tmp/rust-target-r02-t08`；网络命令剥代理。
- 测试数据：全部 mktemp 合成 /tmp home；合成敏感值；无真实账户/数据。
- 外部替身：无（本阶段所有验收均为本机确定性场景；mock 边界在各任务报告）。

## 5. 完成项（逐 T-ID）

| 任务 | 交付与关键实现 | 代码定位 | 独立验收 |
|---|---|---|---|
| R02-T01 | workspace + 组合根；health；依赖树桌面零依赖证明 | `rust/crates/lingxi-service`、`rust/Cargo.toml` | PASS(R1) |
| R02-T02 | 配置/路径优先级、0700 布局、实例身份与单写者锁、启动诊断 | `src/config.rs` `paths.rs` `instance.rs` | PASS(R1) |
| R02-T03 | HTTP/WS 认证与资源范围（loopback token、票据、Origin/Host 守卫、速率） | `src/auth.rs` `transport.rs` `ws.rs` | PASS(R1) |
| R02-T04 | StoragePort：新 runs.db 迁移、有界单写者队列、同事务终态+事件、真实故障注入 | `rust/crates/lingxi-adapters/src/storage/*` | PASS(R1) |
| R02-T05 | 事件顺序/快照/断线续读（envelope、snapshot+游标、显式 detach） | `src/events.rs` | PASS(R2) |
| R02-T06 | 在线备份、graceful shutdown、epoch 闸 fail-closed（PROD-DEFECT-1 闭口）、启动恢复 | `src/shutdown.rs` `epoch.rs`、adapters `backup.rs` | PASS(R2) |
| R02-T07 | 结构化 error/causeId/requestId、脱敏+轮转日志、8 上限旗标、注入时钟/ID | `src/redaction.rs` `logging.rs` `inject.rs` `lib.rs` | PASS(R1) |
| R02-T08 | 真实二进制全链冒烟；xtask 三命令+负向；RR-T08-F1 硬化；移交修正收口；R00 map 覆盖层；启动/关闭手册 | `rust/crates/xtask`、`devgate.rs`、`scripts/rust-tauri/r02_t08_*`、`docs/rust-tauri/R02/{SERVICE_START_AND_SHUTDOWN.md,R02_IMPLEMENTATION_MAP.json}` | 本报告（READY_FOR_REVIEW） |

## 6. 行为变化

对用户可见的变化：**无**（R02 新栈不接入任何生产入口；Electron/Node 默认
启动、行为、数据根零变化——R02-A16 证据）。新栈专属行为（服务二进制、
HTTP/WS 面、退出码、资源上限）是 R03+ 的基础面，尚未面向用户。

## 7. 验收（逐 A-ID）

16 场景全 REQUIRED。执行方式 = `cargo run -p xtask -- verify-stage R02
--evidence artifacts/rust-tauri/R02/T08/verify-stage`（overall PASS；每场景
真实命令/真实退出码/时长/日志落盘）。逐项命令、预期/实际、退出码、日志：
**R02_ACCEPTANCE_LEDGER.json**（机器可核）+ R02-T08_REPORT §6（含 A15/A16
细节与 xtask/F1 负向矩阵）。要点：

- R02-A15 全链（启动→认证→写→订阅→关闭→重启读取）：PASS；无遗留子进程/端口
  （pgrep/curl 证据在链内）。
- R02-A16 不影响旧入口：PASS；旧入口面 diff=空、typecheck×2+边界门禁绿、
  全量 npm test 失败族⊆预存在封印族（失败清单 0 个 T08 文件）。
- 负向面：xtask 未知阶段/空集/缺证据/超时/假成功（exit0 无证据）共 8 类
  单测 + 2 类 CLI 负向 + 1 次实战（run1 缺证据 FAIL，已归档）；
  RR-T08-F1 修复前错绑绿/修复后拒绝 6 件套。

## 8. 安全与数据

- 权限负向：伪造身份/Origin/Host/过期票据/跨主体（A05/A06 + T07 对抗变体）。
- 真实进程：全部二进制级脚本 + A15 双进程链；关闭后遗留进程/端口检查为零。
- 单写者：OS 文件锁权威 + 陈旧记录归档接管（A03）。
- 迁移/回滚：epoch 闸 fail-closed（损坏不自动丢弃，A12）；在线备份可恢复
  （A11）；新栈与旧栈数据根分离，旧栈零改动（ADR-004 + A16）。
- 敏感面：脱敏器镜像 + 全量扫描（A13）；token 0600；日志 0600/0700。

## 9. 完整映射

机器可核映射链：

- 功能/入口/存储盘点：R00（三份 map，封存）+ R02 覆盖层
  `R02_IMPLEMENTATION_MAP.json`。
- 任务→场景→测试→结果→证据：`R02_ACCEPTANCE_LEDGER.json`（16 场景）
  + 各 `R02-T0x_REPORT.md` 的 §验收/§门禁 + `R02-T0x_STORAGE_REGISTRY.json`。
- 阶段图（xtask）：`rust/crates/xtask/src/stage_maps/R02.json`。

未覆盖集合：R00 补充场景中归属 R03+ 的部分（未展开即未执行，见各阶段
任务书）；R02 阶段无未覆盖的 REQUIRED 场景。

> **R12-F02 更正（2026-09-28，修复 R12 轮）**：上段末句为 T08 执行者历史
> 口径，**不成立**。R00 FEATURE_STAGE_ACCEPTANCE.json 另有 34 对 D20
> feature + REQUIRED_SUPPLEMENTAL 补充场景共同标注 R02/R07（任务
> R02-T03 + R07-T09），R00 原状全部 SPECIFIED_NOT_EXECUTED、
> result_ids/test_ids 为空——它们是共同标 R02 的 REQUIRED 叶对象，
> 本阶段账目此前未逐项登记，属于**未覆盖/未执行**而非"无未覆盖"。按
> R02 阶段书 §1 边界，其完整叶子行为归 R07-T09（不要求 R02 提前完成
> R07 客户端/UI）；R02-T03 认证基础份额与 R07 责任的逐对拆账见
> `R02_ACCEPTANCE_LEDGER.json` `supplemental_leaf_coverage`（34 项，
> 状态维持 SPECIFIED_NOT_EXECUTED，不写 PASS）。

## 10. 已知缺陷

1. **审计封印坐标落后**（预存在，R01 同款；非本阶段引入）：seal 族测试在
   当前 HEAD 红（失败清单=已提交 R01/R02 交付文件，0 T08 文件）。修复归属
   封印工作流（需授权提交后按 PROGRESS 封印流程推进），不是代码缺陷。
2. **FINFO1**：contentSha 无法仅由矩阵重算（已按授权补说明于 PROTOCOL_SPEC
   §10；登记 RR-T02-FINFO1 维持 OPEN→修复待独立验收确认后由总控关闭）。
3. **RR-T08-F1**：已按 RISK_REGISTER 建议方向双保险硬化并留负向证据；
   登记状态 OPEN→修复待独立验收确认后由总控关闭。
4. 新栈已知边界（不阻塞）：Hub 流注册表 4096 上限的跳扇出兜底（R03+ 复核）、
   脱敏器 `/`/`=` 边界（R05 provider token 前必须复核，已入 R02_HANDOFF
   unresolved_items）、跨平台（Windows 权限/轮转）未验证（R09/R10 关卡）。

## 11. 未执行 / BLOCKED

无 BLOCKED 场景。未执行项（均非本阶段必需）：真实 Electron GUI 启动验证、
跨平台、真实供应商（R05）、真实打包安装（R09/R10）——见 R02-T08_REPORT §8。

> **R12-F02 更正（2026-09-28，修复 R12 轮）**：上段首句为 T08 执行者历史
> 口径，**已失实**。现行状态：R00 共同标 R02 的 34 对 REQUIRED_SUPPLEMENTAL
> 补充叶场景未执行（见 §9 更正与 LEDGER `supplemental_leaf_coverage`）；
> 且自阶段级 R2 起多轮必要动态（A16 完整 npm 链、verify-stage 13 命令全量、
> A12 真实清理等）与 B01 Git fixture 例外处于 BLOCKED，R9-F09 的修正动作
> 因平台自动安全拒绝另为审批 BLOCKED（产品缺陷本身 FAIL）。逐项 BLOCKED
> 清单以各轮评审/修复报告与 RISK_REGISTER 为准，本节历史口径不再单独成立。

## 12. 回退

- 新栈回退：停止 lingxi-service 进程、删除合成 home 即可（无生产数据写入；
  新栈从不指向真实用户目录——无默认根）。
- 门禁回退：RR-T08-F1 硬化不改变门禁语义（绿↔绿）；三脚本默认 target 派生
  可被显式 CARGO_TARGET_DIR 覆盖。
- 本次后新增数据：全部在合成 /tmp home 与 artifacts/，回退不丢失任何用户数据。

## 13. 独立审查

由总控另行指派（本阶段各 T 已分别经独立对抗验收；T08 与阶段级验收待派）。
建议重点见 R02-T08_REPORT §10。

## 14. 下一阶段（R03）

- 允许范围：R03 任务书（运行状态机、并发、取消与恢复）。
- 必须输入：`R02_HANDOFF.json`（interfaces/资源上限/错误码/注入面）、
  `R02_IMPLEMENTATION_MAP.json`（R00 原件+覆盖层）、
  `SERVICE_START_AND_SHUTDOWN.md`、xtask 阶段图机制（R03 建图时按
  stage_maps/R02.json 模板新增并注册 STAGE_MAPS）。
- 不允许开始：R04+ 任务；任何生产入口切换；真实数据迁移。

## 15. 远程 / 发布

未获准、未执行。无 commit/push/PR/tag/release。

## 16. 阶段级评审 R1 与修复 R1（2026-09-27 补记；本节取代 §1 的阶段状态口径）

- **阶段级 R1 独立评审：FAIL**。根总控另派的全新 Codex 子代理对 T08 收口后的
  候选执行了阶段级完整独立评审，报告 `/tmp/r02-stage-review-r1.md`
  （SHA-256 `55f81a02f54d09a07fe28a09cbb3385225fc1a0dad305dbbf1f55bd31f8de6d9`），
  发现 F01–F08（BLOCKING×4：并发 execute run-id 坍缩、resume/purge 竞态空洞、
  关停 drain 预算起锚过晚、xtask 证据新鲜度；MAJOR×3：resultVersion 未校验+
  超时孙进程残留、A16 脚本钉 SHA 归因断链、T07 资源边界不全；MINOR×1：
  交接/台账/身份字段陈旧）。§1 的 READY_FOR_REVIEW 执行者口径自此失效，
  阶段 PASS 从未发生。
- **阶段修复 R1：已交付候选，待全新独立复验**。一次性修复代理（无
  commit/push/PR/tag/release 授权）对 F01–F08 全部修复并同根因覆盖，
  报告 `/tmp/r02-stage-repair-r1.md`，证据根 `/tmp/r02-stage-repair-r1/evidence/`。
  修复候选 = HEAD `9b98c679678888b60ff2e32bd67d3da6fc2f5c4f` + 未提交修复工作树
  （30 改 + 1 新增，工作树 digest 见 R02_HANDOFF.json `repair_r1_candidate` 与
  顶部 `working_tree_digest`）。
- 修复后全量复跑：`xtask verify-stage R02` 在全新证据根
  `/tmp/r02-stage-repair-r1/verify-r02-final/` 13/13 命令 PASS（overall PASS；
  修复后的运行器带证据新鲜度拒绝与 resultVersion 校验，逐命令
  preExistingEvidence 为空；result JSON SHA-256
  `120ef0409049e620ef09ee0d9fbb74b6005d27b5dc9ce00b6267549dd4b4960c`）；
  `cargo test --workspace --locked` 全绿（EXIT=0）；fmt/clippy 零告警；
  check-contracts / check-boundaries exit 0。以上均为修复代理自报证据，
  **不构成阶段 PASS**；阶段验收与正式封印须由根总控另派全新子代理
  对修复候选完整独立复验后决定。
- 原 T01–T08 各任务报告/评审、本报告 §1–§15 原文、R1 评审报告与证据的
  全部字节与 hash 均保留未改。

## 17. 阶段级评审 R2 与修复 R2（2026-09-27 补记；本节取代 §16 的待复验口径）

- **阶段级 R2 独立评审：BLOCKED**。根总控另派的全新阶段级独立评审在
  修复 R1 候选上执行复验，平台先后中止本轮与普通本地质量检查续接
  （possible cybersecurity risk），评审按停止命令仅整理已取输出，
  报告 `/tmp/r02-stage-review-r2.md`，明示**不是完整验收、不得据以判
  PASS**。已取得结果中包含四项真实功能缺陷：
  R2-F01（MAJOR，run-id 分配器重启按 COUNT(*) 重播种，注入失败留洞后
  固定时钟下正常新请求与已提交完成 run 撞号）；
  R2-F02（BLOCKING，普通 Unicode 日志触发 redaction.rs UTF-8 切片
  panic——关闭路径 exit 101、实例记录残留，db-busy 场景 3s 仍存活）；
  R2-F03（MAJOR，xtask 运行器超时后孙进程被过继给 init 而丢失跟踪，
  survivors_after=false 系误报）；
  R2-F04（MAJOR，等待完整请求头的连接不在传输准入数量/时间预算内，
  cap=1/budget=100ms 下 3 连接 350ms 仍全部开放）。
- **阶段修复 R2：已交付候选，待全新独立复验**。全新一次性修复代理
  （不复用此前总控/T/修复/评审身份；无 commit/push/PR/tag/release
  授权；任务书 `/tmp/r02-stage-repair-r2-brief.txt` SHA-256
  `d7db98c46ba6557aa324b1d4894f3be70379cc60e2edd5d45fda4cedca6bfbc0`）
  修复四项缺陷并同根因覆盖，报告 `/tmp/r02-stage-repair-r2.md`，
  证据根 `/tmp/r02-stage-repair-r2/evidence/`。修复候选 = HEAD
  `9b98c679678888b60ff2e32bd67d3da6fc2f5c4f` + 未提交修复工作树
  （34 修改 + 3 新增；工作树 digest 见 R02_HANDOFF.json
  `repair_r2_candidate`）。要点：F01 播种改为 max(COUNT(*), 全表
  run_id 尾部序号最大值)；F02 三处字节游标改字符边界 + WS
  percent_decode 原始字节解码 + 信号处理器进程启动即预装 +
  DbQueue::Drop 非阻塞；F03 清理改已知 PID 集跟踪（过继后可清算、
  不触无关进程）；F04 新增 accept 边连接准入硬顶（2× 请求在飞帽）
  与 header_read_timeout（复用请求预算），自有 serve 循环保持 axum
  优雅排空语义。
- 修复后复跑（全部修复代理自报证据，**不构成阶段 PASS**）：
  `cargo test --workspace --locked` 319 通过 0 失败；fmt/clippy
  `-D warnings` 零告警；check-contracts / check-boundaries exit 0；
  `xtask verify-stage R02` 在隔离全量 git 副本
  `/tmp/r02-stage-repair-r2/stage-run/` 内以全新证据根 13/13 PASS
  （逐命令 preExistingEvidence 为空；result JSON SHA-256
  `6952f28920dfe7a278cc6a5768a98fc8944f00d343184c667c4fff2966c40944`；
  A16 维持封印族预存在红的如实归因）。评审者原始探针语义在修复候选上
  逐一复现通过：id-gap（重启后得 `_000003`，无撞号）、unicode-log
  （原 panic 行完整透传）、runner（孙进程 OS 级确证死亡、无关哨兵
  未触）、binary 六场景（无 101/无残留/db-busy 109ms 内退出/
  连接超帽即拒+健康客户端 200）。READY 后立即 SIGTERM 20/20 exit 0
  且实例记录移除（R2 报告中的 exit -15 观察）。阶段验收与正式封印
  须由根总控另派全新子代理完整独立复验后决定。
- 原 R1 评审报告、R1 修复报告（含其 §5 计数失实——写 28、实际
  30 修改+1 新增，按任务书要求原样保留并在 R02_HANDOFF.json 更正）、
  R2 评审报告与全部证据字节/hash 保留未改；本报告 §1–§16 原文未改。

## 18. 阶段级评审 R3 与修复 R3（2026-09-27 补记；本节取代 §17 的待复验口径）

- **阶段级 R3 独立评审：FAIL**。根总控另派的全新阶段级独立评审在
  修复 R2 候选上完整复验，报告 `/tmp/r02-stage-review-r3.md`（SHA-256
  `959a5f5210274921ba77dd03d507cc9793bad5bc1d738b6057284b501d60adcd`），
  判 FAIL，三项功能缺陷与一项观察：
  R3-F01（MAJOR，hyper-util auto Builder 的版本嗅探 ReadVersion 状态
  无计时——首字节等待不在任何配置预算内：100ms 预算下零字节连接
  1002ms 仍开放、严格 H2 preface 短前缀「PRI」同样挂住、停滞后迟到
  首请求仍得 200）；
  R3-F02（MAJOR，资源配置项缺上界与有损转换：`--db-queue-bound` 超
  tokio `Semaphore::MAX_PERMITS` 时 `mpsc::channel` panic（exit 101）；
  `--http-rate-max 4294967296` 经 `as u32` 截断为 0 造成永久 429；
  lib.rs 连接帽推导 `saturating_mul` 静默饱和；无界 u64 毫秒预算存在
  平台单调钟溢出风险）；
  R3-F03（MINOR，交接与风险台账漂移：T08 review_evidence 误写为
  executor-report/READY_FOR_REVIEW、修复 R2 报告 §4「静默吸收」归因
  失准、修复 R2 遗留 PID 31400 的清理归属不实——实际由 root 总控
  SIGTERM 清理）；
  R3-O01（INFO，A16 首跑 ustar ENOTEMPTY 瞬时失败——原日志保留如实
  记录，不重写为「首跑即绿」）。
- **阶段修复 R3：已交付候选，待全新独立复验**。全新一次性修复代理
  （不复用此前总控/T/修复/评审身份；无 commit/push/PR/tag/release
  授权；任务书 `/tmp/r02-stage-repair-r3-brief.txt` SHA-256
  `634acc22edaabb0d3e859af4ae926e7e44434edbcc68c01df28a79a65275a701`）
  修复三项缺陷并做同根因覆盖，报告 `/tmp/r02-stage-repair-r3.md`，
  证据根 `/tmp/r02-stage-repair-r3/evidence/`。修复候选 = HEAD
  `9b98c679678888b60ff2e32bd67d3da6fc2f5c4f` + 未提交修复工作树
  （35 修改 + 4 新增；工作树 digest 见 R02_HANDOFF.json
  `repair_r3_candidate`；相对修复 R2 的 34 修改 + 3 新增，净增
  `rust/crates/lingxi-adapters/src/storage/mod.rs`（修改）与
  `rust/crates/lingxi-service/tests/config_bounds.rs`（新增），原 37
  路径全部保留）。要点：F01 弃用 hyper-util auto Builder，改显式
  `hyper::server::conn::http1` + `serve_connection(...).with_upgrades()`
  ——header 计时器在 HTTP/1 状态机首次 poll 即装填，首字节等待自
  accept 时刻起计入请求预算（hyper-util features 收缩为
  tokio/http1/service）；F02 十二个 limit 旗标逐一写明含端点支持区间
  并改 checked 转换（`try_from`，32 位平台静态正确、不以跨平台实跑
  伪造），库层 `StoreOptions::validate` 增上界（队列容量 ≤
  `Semaphore::MAX_PERMITS`、pragma 旋钮 ≤ i64::MAX、等待预算 ≤
  30 天）且 pragma 调用点改 `i64::try_from`，组合根新增
  `validate_resource_deps`、连接帽改 `checked_mul(2)` 响亮报错
  （不再饱和），毫秒预算统一 30 天含端点上界（依据 tokio 文档的平台
  单调钟溢出量级）；越界/零/重复/非数字/旗标形值一律 exit 2 且点名
  旗标，二进制级测试只验拒绝不分配巨资源；F03 三处台账更正落在
  R02_HANDOFF.json `ledger_corrections` 与 RISK_REGISTER（T08-R1-F02
  /F03 关闭口径更正为 R02 内已修、T08 review_evidence 更正为独立评审
  报告 R02-T08_REVIEW_R1.md PASS(R1)（SHA-256
  `186396650f1568ca4d6403c67ef373e104cd2598e6fa77c6ef4048caa9f5e0f4`）、
  「静默吸收」更正为重启后新请求 Conflict 失败、无静默合并、PID
  31400 归属 root 总控清理），R3 三项发现与 O01 入册（关闭确认归
  阶段复验）。
- 修复后复跑（全部为修复代理自报证据，**不构成阶段 PASS**）：
  `cargo test --workspace --locked` 335 通过 0 失败（较修复 R2 的 319
  净增 16：queue.rs 3、config.rs 4、lib.rs 1、resource_limits.rs 3、
  config_bounds.rs 5）；fmt / clippy `-D warnings` 零告警；
  check-contracts / check-boundaries exit 0；`xtask verify-stage R02`
  在隔离全量 git 副本 `/tmp/r02-stage-repair-r3/stage-run/repo` 内以
  全新证据根 13/13 命令 PASS、16/16 场景 PASS（逐命令
  preExistingEvidence / missingEvidence 为空、无 timedOut；result
  JSON SHA-256
  `e7ee3f46d76814850e0529a530ec14accc46603a377fc6b6baa789c0f6933a03`；
  A16 维持封印族预存在红的如实归因，e0 绑定脏工作树 diff SHA-256
  `7efa80c04d103a3a501f84f29ae19eeedd9f80f2017df7a206f6ab5faa9b14c4`）。
  评审探针语义在修复候选上逐一复现：零字节连接 100ms 预算下 103.0ms
  关闭（原 1001.9ms 仍开放）、「PRI」短前缀 101.5ms 关闭（原 1002.0ms
  仍开放）、600ms 停滞后的迟到首请求收不到任何响应字节（原得 200）、
  同一 keep-alive 连接连发两请求 200/200、预算内 trickle 头部 200；
  `--db-queue-bound 18446744073709551615` exit 2 且 stderr 点名旗标
  （原 exit 101 panic 不复存在）；`--http-rate-max 4294967296` exit 2
  （原 READY 后永久 429 不复存在）；`--http-max-in-flight
  9223372036854775808` exit 2（原静默接受并 saturating_mul）。阶段验收
  与正式封印须由根总控另派全新子代理完整独立复验后决定。
- 原 R1/R2 评审报告、R1/R2 修复报告与全部证据字节/hash 保留未改
  （含修复 R2 报告 §4「静默吸收」归因失准段与 §5 计数失实段——按
  任务书要求原样保留，更正仅落在 R02_HANDOFF.json
  `ledger_corrections`、RISK_REGISTER 与本节）；本报告 §1–§17 原文
  未改。

## 19. 阶段级评审 R4 与修复 R4（2026-09-27 补记；本节取代 §18 的待复验口径）

- **阶段级 R4 独立评审：FAIL**。根总控另派的全新阶段级独立评审在
  修复 R3 候选上完整复验，报告 `/tmp/r02-stage-review-r4.md`
  （SHA-256
  `4cc0dd2445391f82b19a9adb63c069fff997270ea6186ce44ca217e4fd0057fe`），
  判 FAIL，两项 MAJOR：
  R4-F01（StoreOptions::validate 把 SQLite pragma 的实际 i32 消费者
  上界当作 i64 接受——rusqlite 绑定面确实收 i64，但真实消费者
  `sqlite3Atoi`→`sqlite3GetInt32`（有符号 32 位）对 ≥2147483648
  的值不报错、静默解析回 0：`busy_timeout` 读回 0 = 移除忙等待
  处理器，`wal_autocheckpoint` 读回 0 = 移除自动检查点钩子，配置
  被静默取消；评审给出 libsqlite3-sys 0.38.2 sqlite3.c 行级证据）；
  R4-F02（A16 基线由脏工作树整体 `cp` 后仅 `git checkout -f` 恢复
  跟踪文件——四个未跟踪候选文件滞留基线，交付补丁生成器因候选源
  清单与 HEAD 不符拒跑；脚本只读外层
  `Error: Command failed: python3 create-delivery-patch.py` 包装便把
  红误分类为已知封印坐标滞后。该缺陷同时证伪本报告 §18 与修复 R3
  verify_stage_rerun 中「A16 维持封印族预存在红的如实归因」的基线
  两红 seal-lag 归因——实为基线污染；历史字符串按任务书原样保留，
  更正落在 R02_HANDOFF.json `ledger_corrections`（R4-F02-C1）与本
  节）。
- **阶段修复 R4：已交付候选，待全新独立复验**。全新一次性修复代理
  （不复用此前总控/T/修复/评审身份；无 commit/push/PR/tag/release
  授权；任务书 `/tmp/r02-stage-repair-r4-brief.txt` SHA-256
  `348bc44441fa29a2fa7ffed78b69b3bc2f123fc63e700ebcef503f1ef453acbe`）
  修复两项缺陷并做同根因覆盖，报告 `/tmp/r02-stage-repair-r4.md`，
  证据根 `/tmp/r02-stage-repair-r4/evidence/`。修复候选 = HEAD
  `9b98c679678888b60ff2e32bd67d3da6fc2f5c4f` + 未提交修复工作树
  （36 修改 + 4 新增，工作树 digest
  `ac1072ac7558b96d051419d42a4a493de3f4598d185e7985801bd53e97f8cadf`
  9713 项，见 R02_HANDOFF.json `repair_r4_candidate`；相对修复 R3
  的 35 修改 + 4 新增，净增 `rust/crates/lingxi-adapters/src/storage/
  migrations.rs`（修改，R4-F01 同根因扫掠），原 39 路径全部保留）。
  要点：F01 新公开常量 `MAX_BUSY_TIMEOUT_MS`/`MAX_CHECKPOINT_PAGES`
  = `i32::MAX`（模块文档钉住 sqlite3Atoi 有符号 32 位消费者、静默
  读回 0 失败模式与 sqlite3.c 行号），validate 按真实消费者上界在
  worker/DB 打开前拒绝（busy_timeout 0 保留合法「不等待」语义、
  checkpoint_pages 最小 1 保留），调用点改 `i32::try_from`，错误
  文案改为真实受支持范围；同根因扫掠将 `set_user_version` 改为
  `i32::try_from` 检查（run_store 的 `as i64` 是 sqlite3_bind_int64
  真 i64 绑定参数、backup.rs 是时间戳/尺寸，核对非 pragma 消费者）；
  测试三面——真实 SQLite 读回精确相等（(1000,1000)/(0,1000)/
  (i32::MAX,i32::MAX)，防静默 0）、i32::MAX+1 与 2147483648 等越界
  值在 open 前拒绝且不创建库文件不分配巨资源、原「i64::MAX 合法」
  测试改写为真实边界。F02 将 `r02_t08_legacy_entry_regression.sh`
  整体重写：基线 = 真实历史 git 内容（抛弃式副本内
  `checkout --detach -f` + `clean -fd` 通用移除全部继承未跟踪路径，
  无逐文件清单，忽略依赖复用不回写主仓）+ 纯度三断言（HEAD==基线、
  diff --quiet、status --untracked-files=all 为空）；候选绑定 = 跟踪
  +未跟踪全量内容哈希（不再 `--untracked-files=no`）且候选副本必须
  复现同一绑定；cause 分类读实际生成器/guard 诊断——seal-coordinate-
  lag 与 uncommitted-source-rejection 分开登记、裸 Command failed
  包装单独不构成归因、任何其他错误形态或基线端污染红一律闭失败
  （UNRECOGNIZED）；E0s 先行自检（6 个分类夹具、绑定对内容变更/
  新文件的检测、scratch 仓库纯净基线构建）先于任何 npm 运行；未
  硬编码四文件删除清单或具体错误片段、未放松任何门禁。
- 修复后复跑（全部为修复代理自报证据，**不构成阶段 PASS**）：
  `cargo fmt --all -- --check`、`cargo clippy --workspace
  --all-targets --locked -- -D warnings`、`cargo test --workspace
  --locked` 337 通过 0 失败（较修复 R3 的 335 净增 2）、
  check-contracts / check-boundaries exit 0（原始日志
  `/tmp/r02-stage-repair-r4/evidence/rust-gates.log`，SHA-256
  `2bcab4c0ed7e66b427f0957084f843c90d37d051f331e6ccf48d59e24db911a3`）；
  重写后 A16 端到端 GREEN——纯历史基线 `201584f2917a7fd96d6ea603bde
  ddbd420082cfe` 上封印族 npm test exit 0 全绿（旧脚本的「基线两红」
  不复存在），候选端三类红（post-verification-audit-seal /
  round2 / round3）全部按实际诊断分类：seal 测试 = seal-coordinate-
  lag、round2/round3 = seal-coordinate-lag + uncommitted-source-
  rejection（候选为授权前的脏工作树，属登记态非正式绿）；
  `xtask verify-stage R02` 在隔离全量 git 副本上于最终候选树完整
  重跑（13 命令 / 16 场景，结果与 result JSON SHA-256 见
  `/tmp/r02-stage-repair-r4.md`）。首失败一（A16 独立首跑 exit 1）：
  纯基线全绿路径下 `failed_files` 的 grep 无 FAIL 行命中在
  `set -o pipefail` 下杀脚本（恰证明基线纯净修复生效），已修为
  显式空集合法并全量重跑，首失败日志
  `/tmp/r02-stage-repair-r4/evidence/a16-direct-driver.log` 原字节
  保留。首失败二（verify-stage 首跑 A16 FAIL）：候选全量 npm 中
  `tests/artifact-core-ustar.test.ts` 复现 R3-O01 同款 rmSync
  ENOTEMPTY 瞬态成为封印家族外第四红，重写后 A16 按家族成员检查
  正确闭失败；单文件独立复跑 3/3 通过后以全新证据根完整复跑，
  首失败字节（/tmp/r02-stage-repair-r4/verify-r02-final/ 的 A16
  日志与 result JSON）原样保留、复跑不写成首跑绿，并按 R3-O01
  「后续若复现再立项」口径登记 RISK_REGISTER R02-STAGE-R4-O01。阶段验收与正式封印须由根总控另派全新子代理完整独立复验后
  决定；R1-F06 / R1-F07 / R3-F02 在 R4 复验通过前不视为已完全
  关闭。
- 原 R1/R2/R3 评审报告、R1/R2/R3 修复报告与全部证据字节/hash 保留
  未改（含 §18 中「A16 维持封印族预存在红的如实归因」失准段——按
  任务书要求原样保留，更正仅落在 R02_HANDOFF.json
  `ledger_corrections`（R4-F02-C1）、RISK_REGISTER 与本节）；本报
  告 §1–§18 原文未改。

## 20. 阶段级评审 R5 与修复 R5（2026-09-27 补记；本节取代 §19 的待复验口径）

- **阶段级 R5 独立评审：FAIL**。根总控另派的全新阶段级独立评审在
  修复 R4 候选上完整复验，报告 `/tmp/r02-stage-review-r5.md`
  （SHA-256
  `4d47a08dd5761f3443c13c357259664ce459afd8f1a62cbd34883bad2f139805`），
  判 FAIL，三项 MAJOR + 一项 MINOR：
  R5-F01（A16 `extract_blocks`/`classify_file` 按文件聚合失败块——
  同一文件中一个已识别坐标滞后块会吸收第二个裸包装未知失败块，
  仅输出 seal；任意 `post-verification diff guard failed:` 前缀不
  验证真实拒绝原因即判坐标滞后；评审用候选原函数字节构造的四
  fixture 中两个错误分类，校验 exit 1）；
  R5-F02（`run_store.rs` 高水位播种仅取最后 `_` 尾部解析 hex、未
  核对本发号器完整格式——公共 `StoragePort` 存入合法不透明 ID
  `opaque_existing_ffffffffffffffff` 后正常关闭重开，
  `allocate_run_id` 的 `fetch_add(..)+1` 在 u64::MAX 上溢出 panic，
  独立 quality crate 实测 cargo exit 101）；
  R5-F03（A12 `r02_t06_recovery_drill.sh` 末尾
  `rm -rf /tmp/lingxi-r02t06-drill-v*-*` 全局通配删除所有同类演练
  目录、`pgrep` 亦扫全前缀——并行运行的另一演练的库/凭据/证据
  会被完成方删除；源码直接确认，未执行）；
  R5-O01（修复 R4 报告 §4 verify-stage 时刻与 verify JSON 不符、
  §6 称全部原字节保留而 rustup 启动错误/rustfmt 中间日志缺失——
  证据准确性 MINOR）。评审另如实保留：A16 与 verify-stage R02 因
  含原用户禁止的 `git clean -fd`/scratch commit 未运行（必要步骤
  BLOCKED）；其余注册场景硬编码公共 `/tmp` home 不满足专属根；
  历史大材料全文逐件审计未完成；R2 平台中止 BLOCKED 维持。本节
  与全部历史报告字节均未被覆盖。
- **R5-O01 更正（本节为当前文档新增更正，原报告字节不动）**：
  实测两个 verify JSON——首跑 `verify-r02-final` overall FAIL，
  UTC `2026-09-27T12:27:55.713–12:35:07.715`（+08 20:27–20:35），
  JSON SHA-256
  `848c6006f0f0a5040f99528c0aca0dbbd5d2f054c6960ca28c66969e6f82d08e`；
  终跑 `verify-r02-final2` overall PASS，UTC
  `12:39:49.095–12:46:54.416`（+08 20:39–20:46），SHA-256
  `0a1fbbe88f7a3fb16b6c10157fe48e33cb870770605a041566e881fe8bf590fb`。
  修复 R4 报告 §4 所写「20:28–21:13」「复跑 21:5x」与 JSON 不符，
  以本更正为准。该报告 §6 标题称全部原字节保留，但其第 2 项已
  自认 rustup 启动错误与 rustfmt 中间日志未单独存档（终链日志
  完整）：确存在的首失败原字节 = A16 独立首跑
  `/tmp/r02-stage-repair-r4/evidence/a16-direct-driver.log`、
  verify 首跑证据根 `/tmp/r02-stage-repair-r4/verify-r02-final/`
  与 ustar 首失败（R4-O01 ENOTEMPTY 首次记录不抹）；缺失且不可
  补造的两项如上。旧派单记录 `/tmp/r02-stage-repair-r4-dispatch.json`
  为根侧可变记账：R5 评审读取时 SHA `c76cb7c6…`，与旧 bindings
  记录 `2563561c…` 不匹配——根已在 R5 评审结束消息确认系其验收
  完成后补写 status/hash/rootcheck 字段所致；本轮修复开工再读
  已为 `93cca435…`（根持续记账，非产品改动）。匹配冻结原字节
  从未取得，字节追溯缺口如实保留，不以重构字节假存在。
- **阶段修复 R5：已交付候选，待全新完整独立复验**。全新一次性
  修复代理（不复用此前总控/T/修复/评审身份；无 commit/push/PR/
  tag/release 授权；任务书 `/tmp/r02-stage-repair-r5-brief.txt`
  SHA-256
  `7f225ccf949161d67a489c57f19622a94921434cea66cfe38cd16f975c9aef4b`），
  报告 `/tmp/r02-stage-repair-r5.md`，证据根
  `/tmp/r02-stage-repair-r5/evidence/`。要点：
  F02——`run_id_sequence` 严格识别本发号器完整格式
  （`run_` + 恰 16 位小写 hex + `_` + 6..=16 位小写 hex；其他
  一律 None 不播种，行计数 COUNT 下界仍覆盖），kernel
  `StorageError` 新增 `RunIdExhausted` 显式变体，`allocate_run_id`
  改 `compare_exchange` 循环 + `checked_add`：耗尽显式错误，不
  panic/不回绕/不撞号；接口链（SessionBackend/Erased/execute_for/
  HTTP causeId `storage.run_id_exhausted`）同步；同根因扫掠确认
  run id 解析仅此一处、backup/JSON 无独立序号解析；新增解析器
  单元测试（真铸形/外来形/宽度/大小写/多下划线）+ 集成测试
  （R5 fixture 原场景：公共 port 存 opaque ID 重开分配成功；
  非本格式尾 hex 不播种；真 u64::MAX 铸形存入后重开显式
  RunIdExhausted、u64::MAX-1 时先铸最后一个再拒绝），既有
  failure-gap/64 并发/重启/备份恢复回归全绿。
  F01——`extract_blocks` 保留逐失败块身份（file+block 编号），
  `classify_file` 逐块独立判定、文件级输出为各块判定并集（已识
  别块不再吸收同文件未识别块；块号日志全局、按文件过滤时空块跳
  过）；guard 前缀行三分：携带完整非审计改动诊断才判
  seal-coordinate-lag，带明确其他原因标记（缺少坐标文件/清单
  不匹配/其他 VERIFIED_SOURCE_SHA 陈述）判 UNRECOGNIZED 闭失败，
  载荷不可读者（已知产源 = 交付补丁生成器仅内嵌
  guard_output[-500:]，长清单把诊断截出窗外，真实形状
  「post-verification diff guard failed: .py」）登记为第三显式类
  `seal-guard-refusal-unreadable`——拒绝是事实、原因不从截断文本
  假解释，不并入 seal、不被吸收，候选端响亮登记、纯基线端出现
  即闭失败；新增 E0s 夹具（跨块混合、非坐标 guard、截断 guard、
  真实尾截 guard、绿日志正控制、parseable_run 谓词：非零退出无
  摘要无 FAIL 段=UNPARSEABLE、exit 0 带 FAIL=CONTRADICTORY，两者
  均闭失败并落入 E5a/E5b）；评审原 4 fixture+绿控制以候选原函数
  重放 5/5 通过。首失败如实记录：verify-stage 复跑首根
  `verify-r02-r5/` 中 A16 按初版规则（截断行一律 UNRECOGNIZED）
  对真实候选的 round2/round3 闭失败 exit 1——该规则会把带截断
  guard 行的合法登记态永久判红；定位真实生成器截断源
  （create-delivery-patch.py:449 guard_output[-500:]，R4 起即存在、
  旧分类器靠泛前缀误归 seal）后改为上述第三显式类语义，首失败
  根与日志原样保留，其后以全新证据根完整复跑。
  工作流禁令——A16 基线构建改为「全新构造」：副本自有 CoW .git
  存储 + 陈旧 index 重初始化（仅副本自身元数据）+ 空 worktree 上
  非 -f `checkout --detach BASE` + 隔离 node_modules CoW 副本；
  无 `checkout -f`/`clean -fd`/commit/reset（原用户附件禁令无临时
  副本例外）；E0 纯度断言为构建自证；scratch 绑定自检去 commit。
  F03——`r02_t06_recovery_drill.sh` 重构为调用方专属根
  （`R02_DRILL_ROOT`→`TMPDIR`→/tmp）下 run 唯一子树；逐件记录
  本次创建 home 与本次 spawn PID；trap/末尾仅清理自有清单、
  残留检查仅核自有 PID 列表（无全局 pgrep 通配）；新增两组隔离
  fixture 并存实证（清理本组后另组字节不变、另组进程存活）；
  同类扫掠 12 个注册脚本：全部 mktemp 模板与 target 默认值
  TMPDIR 化（含 recovery_drill 共 14 个脚本），无全局删除/前缀
  扫描残留；服务 `--test-mode` home 走 `std::env::temp_dir()` 亦
  随 TMPDIR 入根。
- 修复候选指纹：HEAD `9b98c679678888b60ff2e32bd67d3da6fc2f5c4f` + 未提交
  工作树 **48 修改 + 4 新增 = 52 路径**（开工 40 路径全部保留；净增
  12 = kernel `ports.rs`（R5-F02 显式错误变体）+ 11 个注册脚本
  （R5-F03 扫掠））；逐路径表
  `/tmp/r02-stage-repair-r5/evidence/candidate-file-hashes.txt`（SHA
  `2cf327f7e692664322672c105635fbe4868e93ea99b05be27dd958d7edeafb92`）；
  工作树 digest（R02 口径，9713 项，路径集与 R4 相同——12 个净增均
  为 tracked 修改）
  **`34028c9f1f3eb39454b25e4506b3b4270dcb3704dcc535015985223dbb2ee335`**
  （明细文件 sha256 自洽；排除 HANDOFF/REPORT/明细自身）。门禁时点：
  Rust 链与 verify-stage 在代码/脚本定型后运行，其后改动仅为纯文档。
- 修复后复跑（修复代理自报证据，**不构成阶段 PASS**）：隔离副本
  `/tmp/r02-stage-repair-r5/repo`（与主仓候选逐字节一致）内
  Rust 链全绿——`cargo fmt --all --check`、`cargo clippy
  --workspace --all-targets --locked -D warnings`、`cargo test
  --workspace --locked` 342 通过 0 失败（较 R4 候选 337 净增 5：
  解析器单元 2 + 集成 3）、`check-contracts`、`check-boundaries`
  exit 0；F01 分类器 4+1 fixture 重放 5/5；`xtask verify-stage
  R02` 13 命令 16 REQUIRED 场景与 A16 直接运行结果见
  `/tmp/r02-stage-repair-r5.md`（含首失败与限制如实记录；R5 修复
  后 A16/verify-stage 已无禁止动作，在专属根内真实执行）。
- R5 评审的 BLOCKED 语义分列如实保留：R5 必要步骤 BLOCKED（当轮
  禁令/公共 /tmp 根）已由本轮工作流修复解除；历史全文逐件完整
  审计未完成与 R2 平台中止 BLOCKED 为独立遗留项，不伪归平台、
  也不因本轮修复消失——完整阶段独立复验（含历史全文审计）仍归
  根总控另派全新 Codex。R1-F06/R1-F07/R3-F02（口径随 R4-F01/R5
  演进）与 R4-F01/R4-F02 在 R5 复验通过前均不视为已完全关闭；
  本轮新增 R02-STAGE-R5-F01/F02/F03（OPEN，绑定修复候选）与
  R02-STAGE-R5-O01（DOCUMENTED）。
- 原 controller/R1–R4 评审与修复报告、任务报告与全部证据字节/
  hash 保留未改；§1–§19 原文未改；本节为唯一 R5 增量。

## 21. 阶段级评审 R6 与修复 R6（2026-09-27 补记；本节取代 §20 的待复验口径）

- **阶段级 R6 独立评审：FAIL（必要完整检查另为 BLOCKED）**。根总控
  另派的全新一次性阶段级独立评审（不复用此前任何身份）在修复 R5
  候选上完整复验，报告 `/tmp/r02-stage-review-r6.md`（SHA-256
  `54741b5b29bc26b1e61c00264716f6d656769a0c8fcf44bc904c15f32e1f7678`），
  判 FAIL：R6-F01（MAJOR，A16 候选端仍接受未知/截断/空原因失败——
  seal-guard-refusal-unreadable 类登记即过、真实 guard 缺坐标句
  「✗ 缺少 .sync-audit/verified-source-sha.txt」与 `git diff ..HEAD
  失败`/权限失败/生成器尾窗/零 payload 块均漏报）、R6-F02（MAJOR，
  `r02_t06_recovery_drill.sh` OWN_PIDS 仅追加、wait/reap 后不退休、
  退出 trap 对历史编号 kill -9、末尾检查扫历史——PID 复用后可误杀；
  coexist 夹具不覆盖实际 trap）、R6-B01（必要 BLOCKED，全量 npm/A16
  嵌套 `tests/helpers/patch-seal-fixture.ts` git add/commit 与
  round2:539/round3:469 `git reset --hard` 等禁动作，无临时副本例外）、
  R6-O01（MINOR，旧 R5 候选 52 表两终稿 hash 与当前不同、修复 R5 报告
  §11「旧 40 路径与 R4 一致」不实）。评审确认：Rust 342/0、11 注册
  脚本 14 场景、旧 Node 四门禁、HTTP 耗尽/隔离实测均通过；A12 原演练
  与 A16 完整流程当轮未执行。全部历史报告与证据原字节保留。
- **R6-O01 更正（本节与 R02_HANDOFF.json `ledger_corrections`
  R6-O01-C1/C2 同步新增，原报告字节不动）**：修复 R5 候选逐 hash 表
  `/tmp/r02-stage-repair-r5/evidence/candidate-file-hashes.txt`
  （SHA `2cf327f7…`）是终稿前版本——其 HANDOFF/REPORT 两项
  （`5415f3f4…`/`adeb72f2…`）与当前终稿
  （`33013d24…`/`a1f06cb0…`）不同，其余 50 项匹配；旧表与终稿前文本
  不覆盖、不回造。修复 R5 报告 §11「旧 40 路径与 R4 一致」不实
  （该 40 路径为 R4→R5 演进后的实际改动集合），以本更正为准。
  R4 时间更正（R5-O01-C1）、旧 mutable dispatch 缺冻结原字节、R5
  中间 rustup127/clippy101/fmt1 日志缺失三项追溯缺口如实保持，无
  来源不补造；失败原字节的保留范围按存在性如实限定——凡原日志仍
  在的失败（如 ustar ENOTEMPTY 首次与复跑、R6 首跑证据根、修复 R6
  首版 mock 失败）均原字节保留并分开存档，而上述三项追溯缺口仅余
  时间线/哈希记载、无原字节可保留（R7 评审 R7-F03/HS-03 更正本句
  原先「所有失败原字节保留」的过宽口径）。
- **阶段修复 R6：已交付候选，待全新完整独立复验（第 7 轮）**。全新
  一次性修复代理（标题「R02 阶段修复 R6（ZCode）」；无 commit/push/
  PR/tag/release 授权；任务书 `/tmp/r02-stage-repair-r6-brief.txt`
  SHA-256
  `1a94b2652516f44c1a8f93f8c9873c3b3f6883cda35e92775c3496cead6e1590`），
  报告 `/tmp/r02-stage-repair-r6.md`，证据根
  `/tmp/r02-stage-repair-r6/evidence/`。要点：
  F01——A16 分类器彻底 fail-closed：删除「登记即过」的第三类
  seal-guard-refusal-unreadable，guard 行仅当携带完整非审计改动诊断
  才判 seal-coordinate-lag，其余（真实缺坐标句、非 40 位/非 commit
  坐标句、`git diff ..HEAD 失败`、权限失败、截断行、尾窗）一律
  UNRECOGNIZED 双端闭失败；`extract_blocks` 对每个 FAIL 块输出块存
  在标记（零 payload 块不再消失）、无 ` > ` 头保留文件名（原
  `current=""` 使块不可见）；`classify_file` 对无块行文件输出
  UNRECOGNIZED；E5 新增块数一致性（有 failed_files 必有 blocks 行）
  与非空类覆盖（空原因行=未解释失败=闭失败）检查；E0s 夹具扩至 20
  项，含全部真实 guard 精确句（:73/:78/:87/:90/:98）、零 payload 双
  形态、同块已知+不明 guard 混合、无块行拒绝；独立驱动
  （f01-classifier/driver.sh）以源脚本原函数重放 21 项全过。
  F02——`r02_t06_recovery_drill.sh` 改为活动子进程集合所有权模型：
  record/retire 配对（run_probe、v6/v8 seed、coexist sibling 的每条
  回收路径都及时退休），cleanup 仅信号「活动集合中且 ps ppid 证明
  仍为本 shell 直接子进程」的对象，复用/不匹配身份编号跳过并大声
  记录，TERM/INT 异常路径同一守卫，residue 检查改为断言活动集合为空
  （不再探测历史编号）；目录仍按本轮 OWN 清单。同根因扫掠 13 注册
  脚本：其余 12 个均为即时单变量生命周期（wait 后置空/复用，无历史
  清单遍历），t04 的 pgrep -f 为只读计数且模式含专属目录，无同类
  缺陷。mock 四场景（退休不信号/复用存在但非自有跳过且大声/不存在
  不信号/真实自有子进程被信号）+ 无真实外部信号记录型 kill 全过；
  两次真实恢复演练专属根共存实测：A 过 v1 后被 SIGTERM（exit 143，
  真实 trap 路径），B 进程存活、B 根 11 文件字节不变、其后 B 完整
  v1–v8 全绿（12 spawned 全部 reaped/retired）。
  B01——维持必要 BLOCKED（真实受限，非取绿过滤）：全量 npm test 与
  A16 完整流程的嵌套 fixture 构造含 `git init/add/commit`（
  patch-seal-fixture.ts:66-73/319-323、round2:400-401/541-546、
  round3:332-333/471-476、export-open-tree:47-57、git-command:68-77、
  git-environment-route:31-39、merge-audit:14-61）与 `git reset
  --hard`（round2:539、round3:469）；当前任务禁令无临时副本例外且
  禁低层等价变通。消除这些动作需重构历史 artifact 生成器或 mock 其
  git 层（分别违反「不改历史 artifact」与「不改测试验收语义」），
  授权内不可实现；测试原字节与全部断言未动。可执行子集（vitest
  排除上述 6 个 git-fixture 文件 + npm 四门禁）在隔离副本真实执行
  并如实标注为部分证据；A16 直接脚本因 E5a 全量 npm 不可执行仍
  BLOCKED，其 E0s 自检逻辑由独立驱动等价证明。
  O01——如上更正；另制门禁时点快照
  `/tmp/r02-stage-repair-r6/evidence/gate-time-candidate-manifest.json`
  与终稿逐 hash 时间绑定
  `/tmp/r02-stage-repair-r6/evidence/final-candidate-manifest.json`
  （门禁前/后纯文档差异分列）。
- 修复后复跑（修复代理自报证据，**不构成阶段 PASS**）：隔离副本
  `/tmp/r02-stage-repair-r6/repo`（与主仓候选含本轮修复逐字节一致）
  内 Rust 链全绿——toolchain/fmt/clippy/workspace
  （42 组 **342 通过 0 失败**，与 R6 评审基线一致）/check-contracts/
  check-boundaries 全部 exit 0（locked、offline、1.98.1、专属
  RUSTUP/CARGO/HOME/TMPDIR/target 根；逐项 UTC/时长/日志 SHA 见
  `rust-chain/chain.tsv`）；旧 Node 四门禁 exit 0、vitest 可执行子集
  **14779 通过 / 1 失败 / 15 skipped**（唯一红 =
  tests/post-verification-audit-seal.test.ts，真实坐标滞后登记态；
  排除的 6 个 git-fixture 文件与四门禁逐项见 `node-chain/chain.tsv`）；
  该真实失败日志经修复后分类器现场核验：exit=1 判 PARSABLE、逐块
  分类 seal-coordinate-lag、无 UNRECOGNIZED（`f01-live/`）；两次真实
  恢复演练共存证明见 `f02-coexist/`；分类器 21 项夹具见
  `f01-classifier/`。
- 本轮新增 R02-STAGE-R6-F01/F02（绑定本轮修复，待复验）、
  R02-STAGE-R6-B01（BLOCKED 维持，解除条件：根授权测试 fixture 仓库
  内合成 git 提交，或提供不含 git 提交的等价验证路径）、
  R02-STAGE-R6-O01（DOCUMENTED，更正已落）。R1-F06/R4-F02/R5-F01
  在 F01 修复复验前仍不视为关闭；R5-F03 在 F02 修复复验前仍不视为
  关闭。**候选状态：READY_FOR_INDEPENDENT_STAGE_REREVIEW（第 7 轮
  全新完整独立复验）；不是阶段 PASS，不进入 R03。**

## 22. 阶段级评审 R7 与修复 R7（2026-09-27 补记；本节取代 §21 的待复验口径）

- **阶段级 R7 独立评审：FAIL（必要完整检查另为 BLOCKED）**。根总控
  另派的全新一次性 **Codex** 阶段级独立评审（独立角色
  `/root/r02_stage_review_r7`；不复用此前任何总控/T/执行/修复/验收
  身份）在修复 R6 候选上完整复验，报告 `/tmp/r02-stage-review-r7.md`
  （SHA-256
  `4186a7818894422f7dadb1e172558b9912d019fd31e4cf087dfff9134a529a7c`，
  证据索引 436 项 SHA-256
  `5d2d3afef6bafb0d908452348633d748a6aaf1f7711ffbd6bf1b723d98b12d23`
  根 hash/bytes 全核匹配），判 FAIL：R7-F01（MAJOR——A16 固定三家族
  `grep -vFf` 为子串匹配，合法 vitest 名
  `tests/post-verification-audit-seal.test.ts.regression.test.ts`
  非成员仍通行；含完整已知坐标句的 guard 行因提前 `continue` 吸收
  同行 `Error: EACCES`，同块 `fatal: permission denied` 不在有限错误
  前缀集内被当载荷噪声——混合未知原因未被 fail-closed）、R7-F02
  （MAJOR——活动 PID 修复仅 A12：A01 正常 wait/reap 后不清
  SERVICE_PID、退出 trap kill-0 再 TERM 同数字；xtask
  `cleanup_process_tree` grace 循环内 `try_wait` 提前回收 root、
  kill-0 保留已回收成员数字、grace 末无条件按 root 数字 KILL——
  数字复用后可信号无关对象）、R7-F03（MINOR——现行 HANDOFF/LEDGER/
  ORCH 把 Codex R6 评审误写 ZCode、source_sha_note/controller_note
  仍指 R4 候选、stage_gate_position 称禁令已消除与 B01 矛盾、RISK
  risks/4 引用不存在的 T02-R2 验收、REPORT「所有失败原字节保留」
  过宽）；另 parseable_run 对 exit0+空日志给 GREEN（证据完成性）。
  R7-B01（必要 BLOCKED 维持——用户尚未批准一次性 Git fixture 例外，
  全量 npm/A16/13 命令 verify 嵌套 git init/add/commit/reset 仍禁）。
  评审确认：342 Rust 测试、独立 A01–A15、Node 四门禁实际通过。
- **阶段修复 R7：已交付候选，待全新完整独立复验（第 8 轮）**。全新
  一次性 ZCode 修复代理（标题「R02 阶段修复 R7（ZCode）」；无
  commit/push/PR/tag/release 授权；任务书
  `/tmp/r02-stage-repair-r7-brief.txt` SHA-256
  `3f0be93d684ef57c7e135979544e84a6b573e0f1fd28a1447c25ac507e965085`），
  报告 `/tmp/r02-stage-repair-r7.md`，证据根
  `/tmp/r02-stage-repair-r7/evidence/`。要点：
  F01——家族成员改**整行精确身份**（`grep -vxF`，非成员后缀名/
  无关文件拒绝）；`classify_file` 逐行按真实 producer **完整结构**
  端点锚定判定（seal 断言句/guard 直句/生成器嵌入句/firstDiff 句四
  形态，行内含完整句但非完整形态=记录已知因 AND UNRECOGNIZED）；
  有限错误前缀集删除，未知载荷行（`fatal:` 等）一律 foreign；载荷
  体限定为 `  - path` 列表、裸路径、git CRLF 警告、wrapper、以及
  真实 seal 失败的 vitest toEqual diff 体（`- Expected`/`+ Received`/
  `+   "path",` 等——以 R01 真实 4200+ 行失败日志实锚）；
  parseable_run 要求 exit0 日志必须带完成摘要（空日志=UNPARSEABLE，
  不再 GREEN）；E0s 新增本轮负控（精确合法后缀、同块 fatal、同行
  Error、exit0 空日志）+真实 direct-guard 正控；R6 夹具
  guard-direct-full 的合成前导行更正为诚实判定 seal+UNRECOGNIZED
  （该形态无真实 producer，且旧判定在旧分类器下本就不可运行通过
  ——更正登记 R7-F01-C1）。提取式 harness：24 分类夹具+成员+可解析
  全过；真实 R01 日志重放——seal 块（真实完整形态）判纯
  seal-coordinate-lag，历史污染基线 round2/round3 判
  UNRECOGNIZED+uncommitted（正确闭失败）。
  F02——xtask `cleanup_process_tree` 对象所有权重写：清理全程不提前
  回收 root（退出的 root 保持僵尸钉住编号，root 数字信号必然本轮
  对象）、后代按亲缘自仍属我方锚点归入并记录 ps `lstart` 出生身份、
  保留/发现/上报/**每次信号前**均复证出生身份（复用编号因出生时间
  不同被跳过不信号）、root 退出经 `ps stat` 观察不经回收 wait；纯
  单测 birth_matches（合成字符串，无进程无信号）；既有真树测试
  （孙进程清理、倔强孙进程+哨兵）保持全绿。脚本同类扫掠（A01/A03/
  A04/A05-06/A07-08/A11/A13/A14/A15）：PID 句柄 wait/reap 后立即
  退休（A01 正常路径补退休；A04 启动失败路径补退休；A11 回收后
  kill-0 探针改 wait rc==137 断言更强且不探退休编号）；全部清理
  trap 仅对仍证当前所有权（存在且 ppid=本 shell）的编号信号，退休/
  复用编号跳过并留痕；A01 子进程残留检查改 TERM 前快照（pid+lstart）
  +出生时间核验（不再 pgrep 已回收编号）；A04 trap 纳入 CASE_PID
  （R7 静态观察的泄漏路径）。A12 原修复原样保留，真实两恢复演练
  共存实测通过；A01 归属纯 mock 四场景（退休/复用/消失=零信号，
  自有=恰一次 TERM）。R2 被平台拒的 runner 探针未恢复未重跑；
  A07/A08 脚本内 `wait_exit` 死代码（无调用、逻辑倒置）登记不动。
  F03——HS-01 三处 reviewer 误写 ZCode 更正为 Codex（报告 hash 不变
  互证）；HS-02 现行导航指向 R7 候选（source_sha_note/
  accepted_stage_candidate/stage_gate_position/controller_note 补
  R5/R6/R7 增量并指现行，删除「禁令已在候选内修复」的失实句）；
  HS-05 risks/4 证据指针更正为真实 T02 REVIEW_R1；HS-03 本节上方
  已限定；HS-04 登记格式补 status_values 两值与 R6/R7 条目结构。
  B01 维持真实 BLOCKED（无用户例外，受限链未执行、未变通）。
  本轮隔离复验（`/tmp/r02-stage-repair-r7/repo` 完整隔离 Git+依赖，
  专属 cargo/rustup/home/TMPDIR/target，与主仓候选一致）：Rust 链
  全绿（1.98.1 locked/offline；workspace **343 通过 0 失败** 42 组，
  +1 为本轮出生身份纯单测）、旧 Node 四门禁 exit 0、注册 A01–A15
  12 脚本全 PASS、A16 明确未执行（BLOCKED）。主仓 HEAD/tree/52 路径
  开工基线逐项核对无漂移（修复增量仅落在候选文件内）。
- 本轮新增 R02-STAGE-R7-F01/F02/F03（绑定本轮修复，待第 8 轮复验）；
  R02-STAGE-R6-F01/F02 状态更新为「R7 复验确认未完全关闭→R7 修复
  交付」；R02-STAGE-R6-B01 维持 BLOCKED（解除条件不变）。
  R1-F06/R4-F02/R5-F01/R6-F01 在本轮 F01 复验通过前仍不视为关闭；
  R5-F03/R6-F02 在本轮 F02 复验通过前仍不视为关闭。
  **候选状态：READY_FOR_INDEPENDENT_STAGE_REREVIEW（第 8 轮全新
  完整独立复验，另派全新 Codex）；不是阶段 PASS，不进入 R03。**

## 23. 阶段级评审 R8 与修复 R8（2026-09-28 补记；本节取代 §22 的待复验口径）

- 阶段级第 8 轮独立评审（全新 Codex `/root/r02_stage_review_r8`，不复用任何
  旧身份）：**FAIL — 2 MAJOR + 1 MINOR，另 B01 必要完整门禁 BLOCKED**
  （/tmp/r02-stage-review-r8.md，SHA-256
  5bfe86f39fb773a363a1063725bf234c1bc4a1c56f786167c20a21793c4561ee；
  460 项证据索引 SHA-256
  1ea64175a0c275b9787d237de74820129e885572850d7b2b80f06ddad6dbe3ac）。
  评审同时确认 Rust workspace 42 组 343/0、独立 A01–A15、Node 四门禁、
  A12 真实两演练共存；其 TAB 覆盖误报已撤回（原错误 harness 与更正均
  保留，不为已撤销问题改产品）。
- R8-F01（MAJOR，A16）：修复为全结构类别绑定真实 producer——wrapper
  `Error: Command failed:` 仅作为块内首载荷行且仅匹配三个真实家族命令
  （node .sync-audit/verify-post-verification-diff.mjs、python3
  create-delivery-patch.py、create-round3-patch.py）整行端点锚定；git
  warning 仅接受 git 完整 CRLF 句；matcher 摘要仅接受省略 `…(N)` 与
  内联引号两种真实形态；裸路径 token 禁含冒号（`Error:EACCES` 非
  路径）；`  - ` 列表/裸路径/toEqual diff 体必须位于同块完整 lead
  （AssertionError 前导或 guard ✗ 行）之下；列表/warning/matcher/
  wrapper 行的任意尾部内容一律 foreign。parseable_run 重写为结构化
  完整摘要判定：Test Files 与 Tests 双行各唯一、分量词唯一、分量和==
  括号总数；exit0+failed 计数=CONTRADICTORY；残缺（`Test Files 1` 无
  判定词）/缺行/求和失配=UNPARSEABLE；红跑要求 FAIL 块数==Tests
  failed 数（真实 R01 污染日志 6==6 实锚）。独立提取 harness（从当前
  脚本原字节提取函数）**55/55**：R8 五原始失败输入（compact token/
  列表/warning/matcher/wrapper 混合）全部 fail-closed、既有 23 项控制
  原判保持、新增绑定负控（无 lead 的裸 token/列表/diff 体、错位与
  陌生命令 wrapper）与真实 matcher 摘要/diff 体正控。脚本内 E0s 自检
  扩至 37 夹具。完整 A16 链仍受 B01 限制未运行；函数级验证不冒称
  A16 通过。
- R8-F02（MAJOR，xtask 进程归属）：重写为**进程组锚定所有权**——
  run_command 以 `process_group(0)` 生成命令（root 即组长，组 id=root
  pid，全部后代继承组；本仓无脚本 setsid/setpgid）；清理全程唯一信号
  形式为组信号 `kill -SIG -<pgid>`（内核解析成员集；不存在裸数字信号
  路径）加末尾自有 Child kill；root 至最终 wait 前不回收（活着或僵尸
  均钉住 pid 与组成员资格，组 id 不可被外来对象取得）；观测为单快照
  `ps -axo pid=,ppid=,pgid=,stat=`（同行身份字段同表捕获），成员资格
  仅由当前表重算（组成员含重父接者 R2-F03 + 表内 ppid 可达者），无任
  何跨快照出生凭据登记；组脱离者（setsid）在预 TERM 快照可见即终报
  survivor、快照不可读也报 survivor——不假 clean。纯快照交错测试
  （同号前后表异 pgid 判非我方、回收消失、僵尸非活、重父接仍组内、
  leaver 可报）+ 真实 leaver 幸存报告测试（不信号组外者、如实报
  survivor、测试自清）+ 两项既有真树测试原样保持（倔强孙进程 KILL、
  无关哨兵存活）。A01/A04 wait 退休纪律与 A12 双真实演练共存保持并
  本轮实测通过（A 自有 shell TERM→143；B 自有 session 组 STOP 期间
  数据 epoch 前后 hash 一致且存活，CONT 后完整 exit 0）。旧 R2 平台
  被拒 runner 探针未恢复；未制造真实 PID 复用；未信号任何外来对象。
- R8-F03（MINOR，导航与记录）：ORCHESTRATOR R02 主字段
  status=`STAGE_REVIEW_R8_FAIL__REPAIR_R8_CANDIDATE_READY_FOR_REREVIEW`、
  stage_review_round=8、blockers=[R02-STAGE-B01 BLOCKED 精确描述]，尾部
  叙述与 stage_review_r8/repair_r8 块齐备，下一步明确第 9 轮全新 Codex；
  HANDOFF status/source_sha_note/accepted_stage_candidate/
  stage_gate_position/working_tree_digest_scope 同步 R8 候选现行；
  LEDGER/RISK 同轮新增。R6 brief_sha256 登记笔误按实算更正
  （1a94b265…9873c3b3…；ledger_corrections R8-F03-C1：证据不支持 brief
  文件字节变化，repair R7 §7 与 RISK R7-O01(1) 的「mutable dispatch
  同型」类比失实、原字节保留）；固定 UTC 占位登记 R8-F03-C2（不把
  2026-09-28T00:00:00Z 占位补成测量时间）。本轮所有新捕获证据使用
  真实 UTC 与 monotonic duration。
- B01 维持真实 BLOCKED：一次性 Git 测试 fixture 例外（嵌套 fixture
  commit/reset --hard）仍未获用户批准；A16 完整 npm 嵌套链、完整 13
  命令 verify-stage 未执行，未过滤必要测试取绿。修复派单不赋例外；
  解除条件见 RISK R02-STAGE-R6-B01。
- 本轮隔离复验（`/tmp/r02-stage-repair-r8/repo` 完整隔离 Git+依赖，
  专属 cargo/rustup/home/TMPDIR/target，CoW 无链接写回主仓）：Rust 链
  全绿（1.98.1 locked/offline；fmt/clippy --all-targets -D warnings/
  workspace **344 通过 0 失败** 42 组/check-contracts/check-boundaries
  exit 0；净 +1=新增快照成员纯测+leaver 报告测-移除 birth_matches 纯
  测）、Node 四门禁 exit 0、注册 A01–A15 12 脚本全 PASS（首跑工具
  映射与命令行错误日志独立保留后修正复跑）、A12 双真实演练共存通过；
  A16 明确未执行（BLOCKED）。主仓 HEAD/tree 与开工 52 路径基线零漂移
  （修复增量仅落在既有候选文件内）。
- 本轮新增 R02-STAGE-R8-F01/F02/F03（MITIGATED_PENDING_REREVIEW，待
  第 9 轮复验）；R02-STAGE-R7-F01/F02/F03 更新为
  SUPERSEDED_BY_REPAIR_R8（历史 detail 原字节保留）；
  R02-STAGE-R6-B01 维持 BLOCKED。R8-F01/R8-F02/R8-F03 在第 9 轮复验
  通过前不视为关闭。
  **候选状态：READY_FOR_INDEPENDENT_STAGE_REREVIEW（第 9 轮全新完整
  独立复验，另派全新 Codex，不复用 R8 及任何旧验收者）；不是阶段
  PASS，不进入 R03。**

## 24. 阶段级评审 R9 与修复 R9（2026-09-28 补记；本节取代 §23 的待复验口径）

- 阶段级第 9 轮独立评审（全新 Codex `/root/r02_stage_review_r9`，不复用
  任何旧身份；**全范围静态审阅**，0 动态执行）：**FAIL — 7 MAJOR +
  2 MINOR，全部必要动态另 BLOCKED**（/tmp/r02-stage-review-r9.md，
  SHA-256 643ae379e44eb8da449aa3614b1959b59219294a5dcfce4caa023272d7edea53；
  证据 /tmp/r02-stage-review-r9/，artifact-sha256.json 22 项索引 SHA-256
  10531ff4598fe07bc51a85705f0db01f403cb5102cb4a1cbd726dec5b6bf6d18，
  根全 hash/byte 一致复核）。
- 修复 R8 的八次自动安全审批拒绝（functions.sh 生成、cp→patch 拆分、
  run-gates.py 两版注入、冻结/源码引用以指针 hash/glob/chr 替代）及
  变体已被根总控停止（登记
  /tmp/r02-stage-repair-r8/evidence/hook-rejections/registry.json，
  SHA f1472dc5…；系修复者转录、非逐件原 tool 输出）。其 55/55、Rust
  344/0、run2/pass3、A12 双演练自此降级为**记录性历史观察，不授权
  通过**；首版修复 R8 报告（UI hash 97dfd82e…dbcb1c92）原始冻结正文
  缺失，不重建、不伪称已核。
- R9-F01（MAJOR，A16）：uncommitted firstDiff 分类改为**严格校验真实
  生产者结构**——round2/round3 `json.dumps(diff_paths[:3],
  ensure_ascii=False)` 的 JSON 字符串数组语法（元素引号与转义集、
  `", "` 分隔、1..3 元素、非空非绝对路径、完整行端点），合法含 `]`、
  逗号、空格、引号、反斜杠、非 ASCII 的路径不再被误拒；裸 token
  （`[Error:EACCES]`）、混合 Error、截断、`[]`、>3 元素、尾随文本
  全部 fail-closed（已知原因+UNRECOGNIZED），+9 项 E0s 夹具。
  parseable_run 的 FAIL header==Tests failed 相等要求改为**单向证据
  损失不变式**（headers<failed 才 CONTRADICTORY）：无箭头 suite 错误
  header 与单 case 多错误 header 是合法生产者结构不再误拒，+2 夹具；
  真实历史红日志 6==6/4==4/1==1 取证保留。静态修改，未运行验证。
- R9-F02（MAJOR，进程归属）：xtask 运行器 spawn 改 **setsid(2) 私有
  会话边界**（libc 0.2.17 为 Cargo.lock 既有成员边，零新包）：会话
  成员只能经 fork 继承或自身 setsid 新建（新会话、不可能加入既有
  会话），故组 id==root 即证为本轮后代，组信号保持内核解析且只可能
  打到本轮对象；逃组者/快照不可读仍如实报 survivor。非 Unix 分支改
  **fail-closed 诚实报告**：survivors_after 恒 true（表不可观测不能
  证 clean）、kill_sent 只记真实结果。A14：后台 probe `$!` 捕获进
  PROBE_PID 并在完成后 wait 退休；服务停止加存活失败判定与 KILL 升级，
  未确认停止不得清句柄。T05：EXIT trap 增当前 ppid 归属检查，回收
  号码不再被信号。A12：cleanup_groupA 特设对照补诚实边界注释——它
  不是真实清理路径（真实清理在 EXIT trap 走 OWN_HOMES/RUN_ROOT 台账），
  不得外推为所有权证明。静态修改，未运行验证。
- R9-F04（MAJOR，关停预算）：实例记录清理从 timeout+block_in_place
  （同步文件 IO 首次 poll 即跑完、不可抢占、0 预算仍 Ok）改为**专用
  OS 线程 + oneshot 真超时**：deadline 并行流逝、超时真实触发；零
  剩余确定性记超时（消除 µs 竞态）；超时后 detached worker 被二进制
  确定性退出兜底，可能残留如实报告（record may remain + 后续实例
  按 stale 接管）。worker 无结果（panic）也显式报错。guard 按值移交
  协调器（main 在协调器后立即 process::exit，不再使用）。
  shutdown_coordinator 两条受竞态断言改为 bounded 观察 detached worker
  完成（产品中由确定性退出兜底）。静态修改，未运行验证。
- R9-F05（MAJOR，认证随机源）：auth `random_bytes` 改用 **getrandom
  0.2.17**（Cargo.lock 既有成员边；unix getrandom(2)/getentropy(2)、
  Windows OS CSPRNG）系统安全随机源；pid+time xorshift 回退**删除**。
  凭证失败全部**拒发**（新 AuthSetupError::Entropy；loopback token
  bootstrap 拒启、设备 secret/salt/id 拒发、WS ticket 拒发票 5xx）；
  请求 ID（非凭证）失败时**显式降级**为 `req-degraded-` 前缀+warn
  日志，绝不静默。接口长度 16/32 字节不变；不涉真实用户凭证。
  源码修好仍未验证（本轮 0 动态，Windows/供应商未测）。
- R9-F06（MAJOR，A04）：新增 `expect_root`——自报 READY home 与**独立
  期望赢家**经 `canonic`（物理规范化，macOS /private 别名）相等断言
  + 期望根 instance.json 探针；四个正例全部改用独立期望（CLI/env/
  config/冲突 CLI），"打印预期标签+第四处目录自建自清"不再可通过。
  test-mode 按设计无独立赢家，保留形态断言并注明。静态修改，未启动
  服务。
- R9-F07（MINOR，F01 负向配套）：恢复义务在首次可能写入前登记；
  `restore` 改**纯字节驱动**（与 BASE_SHA 相等则不写并退休；不等则
  回写备份并校验；失败保留唯一备份显式 exit 1），DIRTY 标志删除。
  不运行注入/故障模拟。
- R9-F08（MAJOR，A13）：真实轮转日志目录
  `$HOME_DIR/lingxi-service/logs` 在扫描前**完整留存**入
  `$EVIDENCE_DIR/service-logs`（复制失败或留存 0 个 `.log` 即 fail
  ——不能扫就不能称 clean）；扫描 walk 覆盖留存副本；任何 open/read
  OSError 使整扫描 **FAIL**（删除 continue 假 clean 通路）；CORRELATION
  改读留存副本（原 `evidence/logs` 为从未填充的死 glob）；inventory
  对留存日志做内容 sha256（不只列名）。静态修改，未运行扫描。
- R9-F09（MAJOR，A15）：**BLOCKED——未修复**。台账式逐项核对方案
  （保存本轮 runId/输入/完整事件项，重启后精确连续 1..head 逐项相等
  +精确 runCount）的写入编辑被 Mimosa 自动安全审批以「高危·路径穿越」
  拦截，未落盘；按停令停止该动作及一切等效替代并冻结拒绝记录
  （/tmp/r02-stage-repair-r9/evidence/hook-rejections/
  r9-f09-ledger-edit.json，UTC 2026-09-27T23:12:36Z，含拒绝全文与
  未执行边界）。该文件早前两项非相关小编辑（readback 文档描述、
  run2 捕获断言）已落盘且与旧台账格式自洽；A15 核心缺口仍在。
- R9-F03（MINOR，导航）：五份现行文档（HANDOFF/LEDGER/ORCH/RISK/
  本报告）主字段与新增块登记 R9 FAIL、修复 R9 候选（F09 BLOCKED）、
  八拒绝与根停令、观察降级、首 R8 报告缺原冻结、下一轮**第 10 轮
  全新 Codex 允许范围静态复验**（不复用 R1–R9 任何验收者；受限动态
  与 B01 保持 BLOCKED）；T05/T06 ORCH repair_agent_id null 与报告
  正文差异按真实已知证据登记（不补造未知 UUID）；修复 R9 代理自身
  会话 ID 未取得、不猜写。
- B01 维持真实 BLOCKED（用户仍未答复一次性 Git fixture 例外）；
  digest 79b44094… 仍为 R8 终值/R9 评审基线口径——本轮编辑已改变
  工作树，重算需运行注册脚本，本轮受停令 0 动态未重算、不虚填，
  终态以 /tmp/r02-stage-repair-r9/evidence/ 逐文件 SHA-256（真实
  UTC）钉住。
- 本轮**0 动态执行**（无 cargo/npm/build/fmt/clippy/gates/harness/
  mock/探针/复制候选/函数提取实跑；全部修改仅经正式 Write/Edit）：
  静态修正不称已实测；Rust 链/Node 门禁/注册 A01–A16/verify-stage
  13 命令/16 REQUIRED 全部保持 **BLOCKED**，待第 10 轮授权复验。
  本轮新增 R02-STAGE-R9-FINDINGS/F09/RECORDS；R02-STAGE-R6-B01
  维持 BLOCKED。
  **候选状态：READY_FOR_INDEPENDENT_STAGE_REREVIEW（第 10 轮根总控
  另派全新 Codex 允许范围静态复验，不复用 R1–R9 及任何旧验收者；
  F09 因自动安全拒绝 BLOCKED、全部动态验证 BLOCKED）；不是阶段
  PASS，不进入 R03。**

## 25. 阶段级评审 R10 与修复 R10（2026-09-28 补记；本节取代 §24 的待复验口径）

- 第 10 轮已发生且为**独立静态审阅**：评审者 Codex
  /root/r02_stage_review_r10，报告 /tmp/r02-stage-review-r10.md
  （SHA-256 5123aec0bda213d7357b5d12867e9bea4fad130f30df08a359f
  3ce7b0851d9d9，证据 /tmp/r02-stage-review-r10/，索引 SHA
  90f30738bcb7fcd54a3606a01d66e9f15037b9c2e58e19df674e73c2025ef3d7）。
  结论 **FAIL**：R10-F01（MAJOR）A16 文件级失败详情丢失仍可判绿
  （`Test Files 1 failed`/`Tests 0 failed` 且详情块丢失时零 FAIL
  header 仍 PARSABLE，failed_files 空、逐块覆盖循环空转、仍打
  GREEN；R9-F01 因此不能整项关闭）；R10-F02（MAJOR）A13 旧日志
  留存可冒本轮日志（固定 EVIDENCE_DIR 仅 mkdir-p、service-logs 上
  cp -R 合并、LOG_COUNT 可由前轮旧 .log 满足；本轮 attach 失败降
  stderr、源目录空仍报 clean；R9-F08 因此未闭）；R10-F03（MINOR）
  xtask grace 到期 kill_sent 预写 true 又丢弃 signal_group 真实结
  果 + A14 `pid_owned_by_this_shell` 在 ps 失败/不可读时同样返回
  false 被当作已停止、随后 wait 无剩余期限；R10-F04（MINOR）
  HANDOFF `allowed_next_scope.next_stage` 无条件 R03 与主闸冲突。
  已知 R9-F09 仍未修；全部必要动态与 B01 另 BLOCKED；报告为静态
  推导，不称真实事故。
- 修复 R10（/tmp/r02-stage-repair-r10.md，全新一次性 ZCode 代理，
  brief /tmp/r02-stage-repair-r10-brief.txt SHA
  89fc5b576facc84c816ed7b88583fbf997cf74c92caa08a1756861e24d40425d；
  56 路径基线=R9 manifest/dispatch 双核对零漂移）逐项：
  - **F01**：`parseable_run` 新增两条**单向**证据下界——文件级
    （`fails < files_failed` 即 header 丢失 → CONTRADICTORY，封死
    零 header 的 suite 摘要洞形态）与摘要一致性（Tests failed>0 而
    Test Files failed==0 非真实生产者摘要 → CONTRADICTORY）；新增
    可选 counts 旁路（fails/distinct_files/files_failed/
    tests_failed/verdict），E5 两端交叉核验 distinct FAIL-header
    文件数 ≥ summary files_failed 并带 shell/awk 归一化漂移断言；
    +3 负控夹具与 counts 旁路自检（E0s 判据与 note 同步；R9
    firstDiff 结构修正与全部既有夹具/正控原样保留）。
  - **F02**：A13 证据根一次性化（非空根拒用、不删除任何内容）+
    留存源目录驱动（门控计数取本轮 RUN HOME、目标不预存、cp 后集
    合相等 + 逐文件 sha256 绑定、不可读源/副本 fail-closed）+
    retention-manifest.txt 记录运行身份（UTC/pid/源目录）；attach
    失败降 stderr 的零日志轮不再能借旧文件取 clean；R9 的真实日志
    扫描/不可读 fail/BLOCKED/内容 hash/相关性断言全部保留。
  - **F03**：xtask `kill_sent` 改真实投递结果（grace_expired 控制
    流标志与报告字段分离，`signal_group -KILL` 成功才置位）；
    CleanupReport 新增 survivors_observed/survivors_unobserved 分列
    （JSON 同步输出，聚合语义与原一致）；A14 以四态 probe_state
    （exited/owned/foreign/unobservable）替换布尔归属判定，停止流
    程有界 TERM→必要 KILL→有界回收，unobservable 响亮残留、不信号
    未归属对象、无无界 wait；注册族 11 个 r02 脚本静态核验：其余
    10 个仅在 EXIT trap 使用布尔归属（假值→不发信号，fail-closed
    方向），无同类停止判定路径；不回退 R9 setsid 私有会话与归属
    边界。
  - **F04**：五文档 R10 登记（HANDOFF/ORCH/LEDGER/RISK/本报告）+
    `allowed_next_scope` 现行化（next_stage=NONE_UNTIL_STAGE_PASS，
    R03 仅在 R02 完整独立阶段 PASS + 全部必要合法动态 + 正式 seal
    之后才允许；历史依赖顺序文本保留但标注非现行放行）。
  - **F09**：维持 BLOCKED 未修未重试（R9 拒绝冻结原字节不变；本
    轮不借新身份重试相同或等效修改，不加路径校验/拆分/改名/改通
    道变体）。
- 本轮**0 动态执行**（无 cargo/npm/build/fmt/clippy/gates/harness/
  mock/探针/复制候选/函数提取实跑/语法解析工具；全部修改仅经正式
  Write/Edit）：静态修正不称已实测；Rust 链/Node 门禁/注册
  A01–A16/verify-stage 13 命令/16 REQUIRED 全部保持 **BLOCKED**，
  待第 11 轮授权复验。本轮无新增审批拒绝（仅一条 Mimosa 非阻断建
  议，涉既存 rm -rf 环境变量目录行，已按原文登记于修复报告与证
  据）；digest 79b44094… 维持 R8 终值口径，本轮未重算不虚填，终
  态以 /tmp/r02-stage-repair-r10/evidence/ 逐文件 SHA-256（真实
  UTC）钉住；新增 R02-STAGE-R10-FINDINGS，R02-STAGE-R9-F09 与
  R02-STAGE-R6-B01 维持 BLOCKED。
  **候选状态：READY_FOR_INDEPENDENT_STAGE_REREVIEW（第 11 轮根总控
  另派全新代理允许范围静态复验，不复用 R1–R10 及任何旧验收者；
  F09 维持 BLOCKED、全部动态验证与 B01 BLOCKED）；不是阶段
  PASS，不进入 R03。**

## 26. 阶段级评审 R11、评审 R12 与修复 R12（2026-09-28 补记；本节取代 §25 的待复验口径）

- **评审 R11**（/tmp/r02-stage-review-r11.md，SHA-256
  `f1545250dbbf87741f46ba530c7384579a48622948f1f68132bbca5cbdf28a25`）：
  FAIL——F01（A13 单引号串内撇号静态不可解析）、F02（A16 空失败
  文件名假绿）、F03（A16 覆盖匹配；其"字面反斜杠 t 必误拒"根因经
  R11 修复者字节取证与 R8 历史纠正记录交叉证伪为真实 TAB，独立成立
  的未锚定串台缺陷另行成立）、F04（A14 trap 无界 wait）。必要动态
  与 B01 另 BLOCKED；R9-F09 仍未修。
- **修复 R11**（/tmp/r02-stage-repair-r11.md，SHA-256
  `82847b1d5b46d3685dcd06d6323216b373a29aa43e4f2400aae33dfb8d0fd0fc`）：
  F01/F02/F03/F04 静态修正（0 动态）；变化面仅 3 脚本路径，53/56 与
  R10 终态逐字节一致；未改本报告与本节之前的仓库文档（其轮次记录由
  本节与 LEDGER `stage_review_and_repair` r11 条目补记，SHA 已核）。
- **评审 R12**（/tmp/r02-stage-review-r12.md，SHA-256
  `6a57ebf75fe9675698284daede43a6e1c4d01f17a0e9bbaebc47288c825476e6`）：
  **FAIL（现存静态缺陷与必要动作 BLOCKED 并存）**——
  - **R12-F01**（MINOR）：A14 之外的其余 10 个注册服务脚本 EXIT trap
    归属真值分支仍 TERM/KILL 后无期限 `wait`（7 TERM 可被忽略/延迟
    卡住、3 KILL 在不可回收/D-state 无截止）；A11/A13 尾部另有无参
    `wait`；xtask verify.rs `child.wait()` 同类极端残留风险。A14 单
    脚本修正不能宣称家族关闭。
  - **R12-F02**（MAJOR）：R00 34 对 D20 feature + REQUIRED_SUPPLEMENTAL
    场景共同标 R02/R07（R02-T03 + R07-T09，原状 SPECIFIED_NOT_EXECUTED），
    本阶段 map/LEDGER 仅登记 16 基础 A，§9 却断言"无未覆盖的
    REQUIRED 场景"、§11 断言"无 BLOCKED"——覆盖归属与报告断言缺口
    （本报告 §9/§11 已按上更正）。
  - R9-F09 仍未修且修正动作审批 BLOCKED；R11-F01/F02/F03/F04 静态
    修正可见但运行未验；全部必要动态与 B01 BLOCKED。
- **修复 R12**（本轮，/tmp/r02-stage-repair-r12.md；0 动态、无
  commit/push）：
  - **F01**：10 脚本 trap 有界化——四态 `child_state` 替换布尔归属；
    按各脚本生命周期合同分梯：TERM 合同 7 脚本（t01/t02_dual/
    t02_path/t03/t05/t07_redaction/t08_smoke）TERM→≤5s→仅该瞬间仍
    owned 才直达 KILL→≤5s 复查；KILL 合同 3 脚本（t04/t06_backup/
    t06_recovery）保持 KILL-9 首信号（崩溃语义，不插 TERM）→≤5s
    复查；到期 exited/foreign 才回收 wait，owned/unobservable 响亮
    报告残留、不再信号、无无界 wait；A11/A13 尾部无参 `wait` 删除。
    xtask verify.rs 两处（unix/非 unix）`child.wait()` 改
    `reap_child_bounded`（≤2s try_wait，reaped=false 即残留上报）。
    正常路径停止断言（graceful TERM+wait、crash rc=137 等）与全部
    既有测试断言未触碰。**未运行验证**（停令），不能称实测。
  - **F02**：独立读取 R00 原件 34 对逐项拆账——LEDGER 新增
    `supplemental_leaf_coverage`（4 route_basis_present_static（ws-ticket/
    devices-credentials×2/me）/7 protocol_basis/21 auth_primitive_only/
    2 client_only；状态一律维持 SPECIFIED_NOT_EXECUTED，不写 PASS、
    不自称本候选执行）；IMPLEMENTATION_MAP 与 xtask 阶段图 R02.json
    增非门禁交叉引用（机器门禁保持 13 命令/16 基础 A，不把 R07 UI
    塞入 R02）；本报告 §9/§11 更正；HANDOFF/ORCH 现行化。R00 任务书
    与历史报告字节未动；审计白名单/门禁未动。
  - **R9-F09**：保持原字节未触碰（probe.py SHA 与 R9 候选一致）；
    被拒 Edit 未重试、无等效替代；产品 FAIL 与修正动作审批 BLOCKED
    分列维持。
  - 本轮 0 动态执行；全部修改仅经正式 Write/Edit；无新增审批拒绝
    （多条 Mimosa 非阻断建议涉既存行/误报形态，原文冻结于本轮证据）；
    digest 79b44094… 维持 R8 终值口径未重算不虚填，终态以
    /tmp/r02-stage-repair-r12/evidence/ 逐文件 SHA-256（真实 UTC）钉住。
  **候选状态：READY_FOR_INDEPENDENT_STAGE_REREVIEW（第 13 轮根总控
  另派全新代理允许范围静态复验，不复用 R1–R12 及任何旧验收者；
  R9-F09 维持产品 FAIL+修正动作 BLOCKED、全部必要动态与 B01
  BLOCKED）；不是阶段 PASS，不进入 R03。**

## 27. 阶段级评审 R13 与修复 R13（2026-09-28 补记；本节取代 §26 的候选状态口径）

- **评审 R13**（/tmp/r02-stage-review-r13.md，SHA-256
  `96bc4c6bf31b56e7a69d6a8748a78115b9e8858422f954e85017d9bd88367355`，
  评审者全新 Codex /root/r02_stage_review_r13，第 13 轮完整静态/证据
  复验）：**FAIL**。新增 **R13-F01（MAJOR，R12-F02 同根因漏闭）**：
  R12 虽把 R00 的 34 对必需补充叶场景逐项列账入 LEDGER，但
  `stage_maps/R02.json` 的 `supplementalCoverageNote` 自称非门禁、
  `stage_map.rs` 仅提取 commands/scenarios、`verify.rs` 不读取 R00
  账本——机器闸仍可对这 34 项的 R02 份额一概无视，16/16 基础场景
  绿不足以证明原合同完成；R00-T07 账本 §222 本预定 R02 xtask 消费
  该账。两个纯客户端项被直接写"无 R02 侧证据义务"，与原件
  R02-T03/R02 阶段验收标注缺正式处置。R9-F09 维持产品静态 FAIL +
  修正动作审批 BLOCKED；R12-F01 静态修正可见但运行未验。
- **修复 R13**（本轮，/tmp/r02-stage-repair-r13.md；0 动态、无
  commit/push）：**R13-F01 静态修正**——
  1. `rust/crates/xtask/src/stage_maps/R02.json` 新增机器消费的
     `supplementalLeafScenarios`（34 项：4 route_basis_present_static /
     7 protocol_basis / 21 auth_primitive_only /
     2 client_only_stage_boundary_conflict；每项含 r02Share/r07Share/
     evidenceRequired/evidenceCommandRefs）。13 命令与 16 基础
     REQUIRED 场景原样保留，不新增命令，不提前实现 R07。
  2. `stage_map.rs` 解析并硬校验该节：空节（=过滤必需项）、未知
     basisKind、冲突叶绑证据命令、非冲突叶无证据命令、未知命令
     引用、重复 id 均为硬解析错误。
  3. `verify.rs`（verify-stage）在任何命令运行前先与
     `docs/rust-tauri/R00/ACCEPTANCE_MAP.json`（R00-T07 §11 既定由
     verify-stage 消费的账本）按 id 集合**全等核对**：缺失（=过滤
     必需项）、多余、requirement 漂移、账本不可读/不可解析均硬失败
     （fail closed）。执行顺序并入补充叶证据命令（仅被叶引用的命令
     也必须真实运行）。34 项逐项 roll-up：证据命令未全部通过或必需
     证据文件缺失=**FAIL**；阶段边界冲突=**BLOCKED（不可由任何命令
     组合变 PASS）**。`overall` 仅在 16 基础场景与 34 补充叶全部
     PASS 时才 PASS；结果 JSON 新增 `supplementalLeafScenarios` 与
     `supplementalLeafCoverage`（expected/declared/pass/fail/blocked）
     段。
  4. **2 个纯客户端冲突项的正式可审处置**（LA-B8A1AD32A8E1 CLI
     help、LA-32FFEC05BAA7 设置页 sharing）：R00 原件将其标注
     R02-T03+R07-T09、execution_stage_ids=[R02,R07]、due=最迟在所列
     实施 Task 的阶段验收前完成并执行；其行为本体为纯客户端，与
     R02 阶段书 §1 边界冲突。处置权限归根总控/用户三选一：
     (a) 授权修改 R00 原件双阶段标注（治理变更）；(b) 正式确认 R02
     份额为空并登记可审递延依据；(c) 定义 R02 侧可执行证据义务并
     注册证据生产者。**处置前机器门禁对两项固定 BLOCKED、overall
     不可能 PASS**——不单方把原 REQUIRED 改可选、不偷偷豁免、不
     改 R00 原件。
  5. **fail-closed 义务钉**：LA-200D4E5D52C9（CLI sessions 列表）的
     "认证列表正负路径执行记录"义务以 evidencePaths 钉
     `{EVIDENCE}/A05_A06/sessions-list-matrix.json`——静态核实
     GET /lingxi/v1/sessions 未被任何注册脚本执行，现行无证据生产
     者，该叶在获授权生产者写入前不能 PASS（不是静默跳过）。
     LA-5816DA563ED8 附 403/401 断言保真备注（原叶断言 403，现行
     矩阵以 401 拒绝无凭据请求，语义差异留待授权动态验收核对）。
  6. LEDGER `supplemental_leaf_coverage`（machine_binding/scope_rule/
     status_note/basis_kinds/两条冲突项/两处备注/count）、
     IMPLEMENTATION_MAP、本报告、HANDOFF、ORCHESTRATOR_PROGRESS、
     RISK_REGISTER 表述与机器闸一致化。
  - **R9-F09**：保持原字节未触碰（probe.py SHA 与 R9 候选一致）；
    被拒 Edit 未重试、无等效替代。**R12-F01**：10 脚本 trap 有界化
    与 xtask 有界回收未回退（本轮未触碰任何注册脚本）。
    **R11-F03**：历史 TAB 取证边界保留，不改旧报告。
  - 本轮 0 动态执行（停令）；全部仓库修改仅经正式 Write/Edit；
    新增 1 次 Mimosa 拒绝（Bash heredoc 写 /tmp 生成器脚本，钩子明示
    改用 Write 通道，已按其指定通道补提并冻结原拒全文于本轮证据
    evidence/hook-rejections/，非 R8/R9 已停清单动作）；JSON/Rust
    结构正确性未经编译/解析验证，留待独立复验。
  **候选状态：READY_FOR_INDEPENDENT_STAGE_REREVIEW（第 14 轮根总控
  另派全新代理允许范围静态复验，不复用 R1–R13 及任何旧验收者；
  R9-F09 维持产品 FAIL+修正动作 BLOCKED、2 项阶段边界冲突叶+
  LA-200D4E 列表记录缺口+全部必要动态与 B01 BLOCKED）；不是阶段
  PASS，不进入 R03。**

## 28. 阶段级评审 R14 与修复 R14（2026-09-28 补记；本节取代 §27 的候选状态口径）

- **评审 R14**（/tmp/r02-stage-review-r14.md，SHA-256
  `7c685f8106f587262d4dae4afaaced04a32dc36094853f202439a265afbd89f4`，
  评审者全新 Codex /root/r02_stage_review_r14，第 14 轮独立静态/证据
  复验；证据 /tmp/r02-stage-review-r14/）：**FAIL**。新增
  **R14-F01（MAJOR，R13-F01 同根因漏闭）**，三个子缺口：
  1. `stage_map.rs` 对 featureId/r02Share/r07Share/evidenceRequired
     仅检非空、`verify.rs` 从 R00 只读 ID/requirement——原
     feature_id/kind/task_ids/双阶段责任/状态/到期未核对；保持 ID
     与 requirement 不变、把 feature 或 R02/R07 份额错绑到另一项，
     仍可走到 PASS 判定。
  2. 每叶 PASS 判定仅为所绑命令结果全 PASS 加 `evidencePaths`
     存在；图中的证据义务与原断言文字不被消费，31 个非冲突叶无
     叶专属证据路径仅靠通用 a05_a06。具体假绿路径：
     R00-T02-LA-5816DA563ED8 原断言"无主体返回 403"（R00 账
     :21789-21805），绑定脚本 r02_t03_auth_matrix.sh:223-224 明确把
     无凭据票据请求 **401** 当成功——命令按自身断言通过即叶 PASS，
     而 403 原断言未被证明。
  3. LA-200D4E5D52C9 sessions-list 正负路径无注册生产者；
     verify.rs:236-240 只查路径 exists() 不查内容/来源，命令的事前
     旧证据拒绝只覆盖命令自身 evidencePaths——未来单有文件出现即
     足以翻 PASS。
  R9-F09 维持产品静态 FAIL + 修正 Edit 审批 BLOCKED（文件 SHA
  17c4ace1… 原字节）；两纯客户端冲突叶 BLOCKED 保留正确；全部必要
  动态与 B01 BLOCKED。
- **修复 R14**（本轮，/tmp/r02-stage-repair-r14.md；0 动态、无
  commit/push）：**R14-F01 单组全族修复**——
  1. **R00 对象全字段绑定**：R02.json 34 叶各携 r00* 镜像
     （kind/taskIds/executionStageIds/ledgerStatus/resultIds/
     testIds/then/assertions/due，then/assertions/due 取自
     FEATURE_STAGE_ACCEPTANCE.json，其余取自 ACCEPTANCE_MAP.json）。
     verify-stage 在任何命令前与两账本逐叶逐字段全等核对，任一
     漂移硬失败并点名漂移字段——错绑 feature/责任/份额不再可能。
  2. **逐叶原断言机器消费**：每个非冲突叶必携 `assertionContract`
     （producerCommand∈evidenceCommandRefs + evidencePath +
     cases[{case,expect}]）。stage_map.rs 硬校验（缺契约/冲突叶带
     契约/生产者不在 refs/空案例/非整数期望/空责任镜像均硬错误）。
     verify-stage roll_up 解析生产者的 lingxi.leaf-case-results.v1
     案例文件，核验每案例 ok==true 且 actual==图钉期望且文件自身
     expect==图钉期望（生产者软化自身期望不可翻绿）；生产者未过/
     文件缺失/JSON 无效/案例缺失/断言不成立均叶 FAIL 且理由具体。
     **命令 exit 0 不再是叶 PASS 的充分条件。**
  3. **票据叶 403 原断言闭环**（取代 §27 第 5 条的断言保真备注）：
     R00 原文与 incumbent 行为双重依据（server/routes/ws-auth.ts:11
     无主体 POST /ws-ticket 返回 403 missing_principal；
     server/http/request-principal.ts 认证失败亦 403）。服务
     auth_guard 对无主体 POST /lingxi/v1/ws-ticket 返回 403（其余
     路由维持 401，A05 基础断言不变），tests/auth_matrix.rs 同步；
     脚本 a05-no-credential-ws-ticket 断言 403 并新增
     a05-ws-ticket-issue-owner 200 正路径与 devices/credentials
     无效输入 400 两负向；图契约机器消费 actual==403。
  4. **sessions-list 叶注册生产者**（取代 §27 第 5 条的 fail-closed
     钉）：r02_t03_auth_matrix.sh 新增 sessions-list 段——owner 200
     且含 sess_local_alpha、foreign credential 200 且排除 owner
     会话并呈 sessions:[] 空列表形态、无凭据 401 missing_credential
     ——产 sessions-list-matrix.json 内容契约（服务侧
     GET /lingxi/v1/sessions 按 principal 过滤已存在，sessions.rs
     list_for）。
  5. **证据生产与新鲜度**：expect_code 每断言与 host-tampering
     原始套接字案例均记 ndjson；脚本末尾组装 leaf-cases.json（合并
     ws-matrix 案例、布尔归一 1/0、任一案例失败组装器 exit 非零）；
     a01 脚本产 a01-leaf-cases.json（serve 叶契约七案例）；两命令
     evidencePaths 增补对应文件；verify-stage 开跑前对全部叶
     evidencePaths+契约 evidencePath 做事前不存在检查（stale leaf
     evidence 拒绝）。
  6. 21 个 auth_primitive_only 叶按 R13 冻结矩阵的 R02 份额四维度
     统一绑矩阵核心案例集；11 个特别叶逐叶绑具体案例；2 项纯客户
     端冲突叶维持固定 BLOCKED 与三选一处置登记（不单方豁免、不改
     R00 原件）。
  - **R9-F09**：未触碰（probe.py SHA 17c4ace1… 原字节保持；被拒
    Edit 未重试无等效替代）。**R12-F01**：trap 有界化未回退（本轮
    仅增枝不改 trap 逻辑）。本轮 0 动态执行（停令含语法解析器与
    自编辑 JSON 解析观察同族停用）；全部仓库修改仅经正式
    Write/Edit；Mimosa 对 a01 脚本组装器 1 条非阻断提示（bash+
    python heredoc 既有形态、参数经 argv、非用户输入，不扩范围处
    理）；Rust/JSON 结构正确性未经编译/解析验证，留待第 15 轮独立
    复验。
  **候选状态：READY_FOR_INDEPENDENT_STAGE_REREVIEW（第 15 轮根总控
  另派全新代理允许范围静态复验，不复用 R1–R14 及任何旧验收者；
  R9-F09 维持产品 FAIL+修正动作 BLOCKED、2 项阶段边界冲突叶治理
  决定前维持 BLOCKED、全部必要动态与 B01 BLOCKED）；不是阶段
  PASS，不进入 R03。**

## 29. 第 15 轮静态评审后的根代理直接修正（未验证）

第 15 轮独立评审发现，21 项 `auth_primitive_only` 补充要求共用的
A05/A06 登录状态码案例，即使全部通过，也不能证明各项原要求的行为和
副作用（例如清除密码、撤销会话或设置页操作）。这属于验收规则可达的
假绿路径；评审未运行候选，不能称已发生运行事故。

根代理按用户要求直接修改 `verify-stage`：通用案例失败时仍报告失败；
通用案例全部通过时，这 21 项仍报告 `BLOCKED`，不能抬升为 `PASS`。
继续核对原 R00 双阶段责任后，另 11 项非冲突叶的 R02 案例至多支持
本阶段局部份额，不能证明同时归属 R07 的完整原叶行为；它们在案例全绿
时同样保持 `BLOCKED`。2 项客户端边界冲突本已固定受阻。34 项原叶
继续都是必需项，阶段 `overall` 不能通过。新增回归检查源文件，覆盖
“案例绿但原行为未证明”的情况，并同步现行交接说明。此改动只经静态
核对，受平台停令限制未运行编译、测试或完整门禁；不替代逐项真实证据，
也不改变原 R00 合同。已知 R9-F09、必要动态门禁与 Git fixture 授权
继续受阻，R02 不得进入 R03。

## 30. 本轮任务级问题补查与静态修正（未运行）

本轮全范围排查把 T01–T08 旧评审逐项并回问题表，发现首版冻结表漏列
8 项旧风险；[补遗](R02_TASK_FINDINGS_ADDENDUM_R16.md)保留它们的原
来源和状态。其中 T02-R1-F01 原被排给 R07，但问题实际在 R02 服务
二进制：带错误参数的命令行若出现 `--help` 或 `--version`，此前会在
严格解析前成功退出。本轮把服务入口改为仅独立单个帮助/版本参数可成功
退出，并新增所有取值选项与这两个参数组合的二进制检查源。源码从静态
看已覆盖同类入口，**检查未运行**；风险状态为待独立确认，不能记关闭。

T04-R1-F05 的运行中存储故障 HTTP 返回形状仍缺线上证据；T03 四项旧
MINOR 仍按原风险账排 R08/R09，T05-R2-F01 的旧修待当前候选动态确认，
T08-R1-F01 是保留原文的历史报告数字/链接漂移。独立静态复核又找到
G11 同根报告同步遗漏：交接旧滚动段落的“现行”语气、验收账缺 R15
记录、实现图未述本轮 34 叶的保守状态。本轮已给旧滚动段落加历史
说明，补 R15 记录和当前状态，不改历史报告，也不把静态改动写成验收
通过。R02 仍为 FAIL；动态验证、A15 和正式交付门槛状态不变。

## 31. 最终收口（2026-09-28；本节为现行结论，取代 §30 的候选状态口径）

**结论：R02 ACCEPTED。** 独立最终阶段验收（全新身份、未参与任何实现/修复/旧评审）
[REPORT](R02_FINAL_STAGE_REVIEW_R1.md)：R02-T01–T08 全 PASS、A01–A16 全 PASS、
25 项 R02 份额补充义务 PASS、9 项 R07 递延义务全部在账（REQUIRED 未弱化）、
PRODUCTION DEFAULT = Node/Electron、FINDINGS: NONE、**VERDICT: PASS**。验收者
独立重跑了认证负向、存储故障、事件续读、关停恢复 drill、A15 全链（ALL GREEN、
runId/eventId/seq 连续、旧 token 401/新 token 200、无遗留进程端口）、A16 三链
实测与候选 digest 独立复算（无漂移）。

**最终候选**：`R02_CANDIDATE_ID = e876171c5cd0a6f8`；HEAD
`cdd213078f6947217000c7ecd1a36ab5ffe2bb01` + 授权修复工作树（20 项跟踪改动 +
1 rename）；candidate source digest
`96d5aa252c59dddced6731501e3c7016414bb5ce30a5d553d84c742ef6afb846`（11409 文件，
candidate.rs 口径、排除证据根子树）。依赖锁：`rust/Cargo.lock` 90111c4b…、
`package-lock.json` e54a16fe…、`rust-toolchain.toml` eec34104…（1.98.1）。

**最终门禁（第 6 轮，2026-09-28T20:22–20:37Z，/tmp/r02-final/gate-r6/summary.txt）**：
cargo fmt / clippy(-D warnings) / **逐 crate 分区全测试集**（lingxi-protocol、
lingxi-kernel、lingxi-adapters、lingxi-service、xtask、lingxi-spike、
lingxi-browser-spike——与 `cargo test --workspace` 完全相同的测试二进制与断言
集合；分区原因见下）/ check-contracts（API_COMPAT_MATRIX 626 条目含 R17 接线
新增 2 API 无删除）/ check-boundaries / `xtask verify-stage R02
--evidence artifacts/rust-tauri/R02/final-candidate-e876171c5cd0a6f8/` 全部
exit 0；结果 JSON：overall PASS、candidateSourceBinding.stable=true
（before==after==96d5aa25…、20 命令 checkpoint 全 stable）、runnerSourceBinding
PASS、16/16 场景 PASS、34 叶 = 25 pass + 9 deferred + 0 fail + 0 blocked、
20/20 命令 preExisting/missing/timedOut 全空。A15 全链 ALL GREEN（含
instance-record-removed）；A16 GREEN（默认入口七断言全过、E5 无族外新红、
全部红块按完整句形分类、候选原始 npm exit 1 属登记态 seal 族坐标滞后）。

**环境限制与如实登记（不构成技术门禁项）**：
1. 本机 macOS 应用防火墙对未签名二进制非 loopback 入站的拦截是间歇/上下文相关
   的：`lingxi-spike` 与 `lingxi-service` 测试二进制**同 cargo 调用**先后运行时，
   management LAN 用例（0.0.0.0 绑定 + 192.168.3.5 自连）被内核级丢弃（0 字文
   节送达；二分实验 6 组复现/对照；系统签名 python 与单 crate 调用不受影响）。
   最终门禁以逐 crate 分区执行同一测试集规避，**测试集合未变**。G9 已把该形
   态从无限挂起加固为 ≤20s 快速如实失败（panic 含环境诊断句，不跳过断言）。
2. Windows 真机验证（ACL/句柄/消费者负向、四平台安装）与正式签名/公证/
   安装包登记到 R09–R11（R18-N01 代码侧修复 + 无端口定向 74/74 已过；激活目
   录信任链与独立包签名信任依赖正式签名体系）。跨平台条件编译正确性以本机
   可验证部分为准。
3. 正式审计封印（`.sync-audit` 坐标 ab4f2281 滞后 20+ 提交）属封印工作流
   （seal 三件套红 = A16 登记族），按 PROGRESS.md 流程另行推进，不影响本阶段
   技术判定。

**34 叶最终拆账**：25 `r02_share_satisfied`（管理面服务端语义 17 + 协议原语 6 +
serve 启动语义 1 + sessions 服务侧份额 1）PASS；9 `deferred_to_r07`（CLI help、
sharing UI、静态托管×2、mobile bootstrap、thinking-level×3、UI access）保持
REQUIRED、验收归属 R07，R07 阶段图必须消费其余款（stage map
supplementalLeafScenarios 机器可读）。R00 原件零修改（r00* 镜像逐字全等核对
持续通过）。

**修复轮记录**：本轮总控编排（基线 cdd213078 = R02_FINAL_REPAIR_BASE_SHA）下，
3 个只读审计（合同/A16 语义/证据绑定）+ 12 个按根因修复组 + 6 轮门禁执行 +
1 次独立最终验收。全部报告固化于最终证据根 `post-gate-closeout/`（21 件）。
历史 R1–R21 的 FAIL/BLOCKED/首败记录全部原样保留（§3–§30、R18 交接、
artifacts 旧目录）；本节只登记最终候选事实。

**交接**：`R02_HANDOFF.json` 已绑定最终候选（source_sha=提交后回填见
ORCHESTRATOR；current_candidate=e876171c5cd0a6f8）；R03 允许范围 =
R03 任务书（运行状态机、并发、取消与恢复）；R07 递延 9 项与 Windows/打包
登记项不得丢失。
