# G06-E01 普通自查（R03-FIX-F07-C01..C04）

执行者：EXECUTOR-REPAIR-R03-G06-E01。修复后代码（工作区 8883923a5 + 未提交 G06 修复）。全部命令 `~/.cargo/bin/cargo`（1.98.1）`--locked`，测试数据根为进程临时目录（隔离）。

载体：`rust/crates/lingxi-service/tests/background_steering.rs`（真实 `ServiceState::bootstrap_with_deps` 组合：真实 storage port / 事件服务 / 内核状态机 / 会话 gate / requestId 去重 / 后台注册表 / 分离监督驱动；Provider 替身只记录每轮实际输入并按 (session,pop) 门控泊车，不 mock 任何被测环节）。

统一命令与退出码：

```
cd rust && ~/.cargo/bin/cargo test --locked -p lingxi-service --test background_steering
→ exit 0；test result: ok. 8 passed; 0 failed（normal-selfcheck/normal-run.log）
```

机器证据：`normal-selfcheck/evidence.json`（每用例记录 steer 响应、逐轮 Provider 输入、Run ID、出现次数、容量等）。

---

## R03-FIX-F07-C01｜后台 Accepted 后下一轮收到（只一次）

- 前置：真实后台 drive 第一轮泊车（session busy，`is_busy` 断言）。
- 操作：`steer_for` → `Accepted`；释放第一轮为 Continue；观测第二轮实际输入。
- 观测（evidence `c01`）：turn1 输入 = 原文（无 steering）；turn2 输入 = `原文\n\n[steering]\nSTEER-C01-focus-on-config`；Run 终态 `completed.with_final`；`occurrences == 1`（全部记录轮次中恰一次）；`steering_pending == 0`。
- 判定：**PASS**。
- 命令/退出码：如上 exit 0（`c01_background_accepted_steering_reaches_next_turn_exactly_once ... ok`）。

## R03-FIX-F07-C02｜跨会话和跨运行不串用

- 前置：两个后台会话（alpha/beta）并行，各自第一轮泊车。
- 操作：仅对 alpha steer；分别推进两者；随后在 beta 上开新的不相关后台 Run。
- 观测（evidence `c02`）：alpha turn2 含 `STEER-C02-only-alpha`；beta 全部轮次（含第二个新 Run 的 3、4 轮）均不含该文本；alpha `steering_pending == 0`（被本会话消费完毕）；两 Run 均 completed。
- 判定：**PASS**（要求只到授权目标运行，不进另一会话、不进旁观会话的无关下一 Run）。
- 用例：`c02_steering_stays_in_the_authorized_session_across_parallel_runs ... ok`。

## R03-FIX-F07-C03｜取消和终态边界可解释

两腿，契约均为既有冻结语义（非本轮新造）：

1. **中途 Accepted 必须被本 Run 消费、不残留**（`c03_midflight_...`）：turn1 泊车时 steer → Accepted → 释放 → turn2 输入含 steering；Run 完成后 `steering_pending == 0`；同会话下一个不相关后台任务的轮次（pop3+）不含该文本（无残留串入）。evidence `c03_leak`。
2. **错过消费点保留、不谎称、不触发**（`c03_too_late_...`）：终轮已派发（最后一 drain 已过）后 steer → Accepted → Run 照常 `completed.with_final`；该文本不在本 Run 任何输入中（未假称已用于执行）；`steering_pending == 1`（保留可观测）；`run_count == 1`（steering 未悄悄触发新任务）；下一 Run 首轮输入携带该保留文本（冻结接收方契约）。evidence `c03_retention`。
- 判定：**PASS**（两条用例均 ok）。

## R03-FIX-F07-C04｜容量和前后台一致性

- 前置：`steering_inbox_capacity = 2`（经 `ServiceDeps.session_concurrency` 注入，`validate` 拒 0 的同一配置面）。
- 操作：后台与前台两条腿各自：2 条 Accepted → 第 3 条提交。
- 观测（evidence `c04`）：两腿第 3 条均 `Err(SessionExecuteError::SteeringInboxFull)`（响亮拒绝）；拒绝后 `steering_pending == 2`（队列未被污染，恰为两条已接受）；下一轮输入含两条容量内文本、不含被拒文本；drain 后 pending 归 0；两 Run 均 completed。前后台观测**逐项一致**。（并发竞争容量的对抗腿见 `G06-E01_ADVERSARIAL_SELFCHECK.md` 的 `c04_adversarial_concurrent_steers_respect_the_bound`。）
- 判定：**PASS**（`c04_bounded_inbox_same_contract_foreground_and_background ... ok`）。

---

## 修复前红基线（同用例、未修代码）

隔离 worktree `/tmp/lingxi-r03-g06-redbase`（HEAD=8883923a5，未含修复），同命令 exit 101，**1 passed / 7 failed**：C01 两用例、C02（跨会话）、C03 两用例、C04 两用例（顺序后台腿 + 并发腿）均红（后台下一轮/容量内消费无 steering；详见 `../adversarial-selfcheck/red-baseline-final-file.log`）。`c02_adversarial`（取消边界契约钉）修复前即绿，如实登记。
