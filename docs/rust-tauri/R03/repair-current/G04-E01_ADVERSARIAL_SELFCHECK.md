# R03 修复轮 G04-E01 对抗性自查（F05）

- 执行代理：EXECUTOR-REPAIR-R03-G04-E01。日期：2026-09-30。
- 证据根：`artifacts/rust-tauri/R03/repair-current/G04-E01/adversarial-selfcheck/`（`green-admission_dedup_adversarial.log`＝修复后全绿 5/5；`red-baseline-admission_dedup_consistency.log`＝隔离 worktree 旧代码红基线）。
- 对抗套件：`rust/crates/lingxi-service/tests/admission_dedup_adversarial.rs`（5 测试，真实组合根；存储装饰器只注入受控停驻/故障，全部写委托真实库；Provider/Tool 替身只产生外部响应与受控外部副作用）。
- 结论：**5/5 对抗变体未推翻修复**；每个攻击窗口给出"窗口→观测→是否推翻→命令/退出码→证据"。

## 红基线复核（攻击是否真实存在）

隔离 git worktree（HEAD=198e0da1e，未含修复）运行与普通自查同一套 5 用例：**1 passed（C04 钉）/ 4 failed**，失败信息逐条对应审查的 source_facts：

| 用例 | 旧代码观测（红） | 审查条目 |
|---|---|---|
| C01 | 重试 `Ok(replayed=true)` 返回 `run_000001a0dcba6c00_000001`，该 id 无 durable 行 | "admit 成功记 K→R，spawn 拒绝…当前代码可直接 Replay R 而不执行" |
| C02 | 起始事务失败后同 key 重试返回无行 run | "前台随后才 record_run_started…失败后没有与受理状态匹配的回滚" |
| C03 | 预留/提交窗口内的并发同 key 拿到半提交假 replay | "不暴露半提交假结果"被违反 |
| C04 | 通过（旧代码不删绑定——修复红线钉） | — |
| C05 | 重启后同 key 被静默当全新请求执行（外部 +1、新 run） | "不能把已有事实静默忽略后盲重做" |

## 逐 C-ID 对抗变体

### C01｜容量检查与 spawn 之间注入失败（不只测最早拒绝）— 未推翻

- 攻击窗口：后台注册表容量检查（1024，未触及、通过）与真实 `TaskSupervisor::spawn_detached` 之间——受监督任务容量（4）先被真实 parked 任务占满，拒绝发生在 spawn 侧（`adv_c01` fillers + `supervised_task_cap=4`）。
- 观测：首次 = `BackgroundRegistryFull{cap:4}`（loud，runs 0、外部 0）；释放后**同 key 改内容**重试 = 全新受理（被撤销的预留不残留 digest 冲突），run 行真实存在（running）→ 取消收束；**无关 key** `adv-c1-other` 完整受理并结算（撤销未破坏注册表/容量记账）。
- 是否推翻：否——重试真实启动（可核实 run 行），无幽灵。
- 命令/退出码：`cargo test -p lingxi-service --locked --test admission_dedup_adversarial adv_c01 -- --test-threads=1` → exit 0。
- 证据：`green-admission_dedup_adversarial.log`。

### C02｜受理窗口内响应丢失 + 取消交错（Unverified 懒解决走真实存储）— 未推翻

- 攻击窗口：首个提交停在 `record_run_started` 内部（预留已记、durable 未写）；同窗口注入（a）取消请求（b）请求 future 被 abort（响应丢失，无 verdict）。
- 观测：(a) `cancel_run_for(parked_run_id)` = 诚实的 `NotFound`（无任何可取消事实）；(b) abort 后 runs 仍 0（绑定 Drop 留 Unverified，**未删除**）；同 key 重试在**真实存储**上懒解决：`load_run` = None → 安全撤销 → 全新受理（`!replayed`、行存在、`run_id ≠` 消失的预留 id）→ 结算 completed；结算后同 key 重试 = 幂等 replay 同一运行；总量始终 1。
- 是否推翻：否——未知结果保留绑定、可证明缺席才撤销，链路在真实库上成立。
- 命令/退出码：`… adv_c02 …` → exit 0。证据：同上。

### C03｜跨主体/会话相同 key 隔离对照（受理窗口期间）— 未推翻

- 攻击窗口：alpha 会话的同 key 受理停在预留/提交之间时，同 key 并发打到（a）alpha 同主体（b）另一会话 beta 同主体（c）另一主体（同用户 device）。
- 观测：(a) = `AdmissionInFlight`（显式可重试）；(b) = beta 自己的命名空间全新受理（并发期间即成功——隔离不因窗口而失效）；同会话内 (c) 在窗口期间被 **per-session busy gate** 串行化（R03-T02 冻结语义，两主体同会话仍一忙皆拒——如实观测，不规避）；窗口释放后 alpha 首个提交真实受理、owner 同 key 重试 replay 真实运行、device 在 alpha 的提交是其命名空间的全新受理（`run_id` 互异，非 replay 非冲突）。
- 是否推翻：否——命名空间隔离在窗口期间与结束后都成立。
- 命令/退出码：`… adv_c03 …` → exit 0。证据：同上。

### C04｜journal 提交前后丢响应（外部动作之前/之后两态）— 未推翻

- 攻击窗口：变体一（外部动作**之前**丢响应，即 journal intent 之前/started 之前的形态）：run 行已 durable（绑定已承诺）但工具停在 0 许可闸门上时 abort 请求 future；变体二（外部动作**之后**）：`r03_f05_c04` 变体 A（journal started 已提交、abort）与变体 B（receipt 已提交、响应丢弃）。
- 观测（变体一）：同 key 重试 = `replayed=true` 指向**真实 dangling-active 行**（status=running，可查询），runs 保持 1、外部保持 0——不是幽灵、不重复受理；随后 `cancel_run_for` 对该行返回诚实的 `DanglingActive`（无活驱动，不伪造终态）。（变体二见普通自查 C04：外部计数不增、返回既有结果。）
- 是否推翻：否——两态都返回可核实的真实受理，无一例"遇错清绑定后重复执行"。
- 命令/退出码：`… adv_c04 …` → exit 0。证据：同上。

### C05｜重启后新旧 key 交错 + 未知副作用形态 — 未推翻

- 攻击窗口：进程 A 的运行停在外部系统响应上（外部 +1 已发生、journal started、run 仍 active——未知副作用崩溃形态）→ 关闭存储（进程死亡）→ 进程 B 同数据根重启。
- 观测：B 的 bootstrap 恢复扫描把该运行诚实收束为 `interrupted_needs_attention`（`RecoverySettlement::Written`，无假成功）；同 key `adv-c5` = `RequestIdBoundToEarlierRun` 指名该 run（runs 保持 1、外部保持 1——未知外部操作未被当全新请求重做）；**新 key** `adv-c5-new` 正常受理并真实结算。基套件 `r03_f05_c05` 另覆盖"先完成再重启 + 同 key 改内容"两态（均为显式契约，不盲重做）。
- 是否推翻：否——按明确契约拒绝（并给出可查询的 run id），不声称 exactly-once。
- 命令/退出码：`… adv_c05 …` → exit 0。证据：同上。

## 结构性论证（为何 Replay 不再可能返回不存在的 Run）

Committed 状态**只能**由三类 durable 事实进入：(1) `drive_run` 在 `record_run_started` 返回 Ok 后立即 `commit_durable()`；(2) Unverified 懒解决/前台补偿在 `load_run` 返回 Some 后提升；(3) 后台任务失败补偿同 (2)。run 行无任何 DELETE 路径（全仓 grep 无 `DELETE FROM runs`），故 Committed ⇒ 行存在是结构不变量，Replay 不需要逐次重读。已知未派发失败（spawn 拒绝）经 `release_not_started` 撤销；未知执行经 `mark_unverified`/Drop 保留，由下一次同 key 提交在真实存储上解决。

## 环境限制与如实披露

1. **后台任务内起始写失败**（spawn 成功后、detached 任务内 `record_run_started` IO 失败）无法经真实链确定性注入：`execute_background_for` 的驱动直接使用 `Arc<RunDatabase>`，无端口接缝（为后台断连语义所有意为之）。覆盖方式：与前台 C02 完全相同的补偿代码形状（同一组 `commit_durable/release_not_started/mark_unverified` 调用）、Drop→Unverified 回退的单元级钉（`dropped_owner_without_verdict_is_kept_and_resolved_lazily`）、以及 Unverified 懒解决在真实存储上的端到端验证（adv_c02）。与 G01/G02/G03 报告的同类披露一致。
2. InFlight 窗口内的同 key 并发从"立即假 replay"改为"显式可重试拒绝"是有意语义收紧（红线 1/2 的直接推论）；A12 断连重连（durable start 之后）replay 语义不变（`background_disconnect_recovery` 3/0 绿）。
3. 跨重启同 key 从"静默全新执行"改为"显式拒绝并指名既有运行"是 C05 要求的契约变化；新 key 与无 id 路径行为不变（`request_dedup` 4/0、exit_race_rejections 3/0 绿）。
