# RR2-F05-R1 — REVIEWER-R03-RR2-F05-R01 independent adversarial review evidence

审查者：REVIEWER-R03-RR2-F05-R01（全新、未参与实现）。审查对象：R03-RR2-F05-01
定点修复候选（HEAD `c96f7cc635cf18fa83a81b0e28f33d4fed8baf9c`，未提交改动 =
`git diff HEAD`（10 文件）+ 3 个未跟踪路径）。审查日期 2026-09-30。

裁决与逐项核对表见
`docs/rust-tauri/R03/repair-current/R03_RR2_F05_R1_REVIEW.md`。

## 候选冻结证明（candidate-fingerprint.txt）

- 审查前 `git diff HEAD | shasum -a 256` =
  `7f72a789fbe50ccdeb544c9ed99d4ce1dcba6eb894e88d3173e3a6fa5f21939f`
- 审查后同一命令输出**完全相同**（候选未漂移）。
- 未跟踪文件 sha256（前后一致）：新测试
  `e2d5bb35…d3d14e`；执行者报告 `d9e084c1…c022af`。
- 审查期间本审查者未修改任何产品代码、既有测试、脚本或执行者报告；
  本证据根与审查报告是仅有的新增产物。

## logs/（全部为真实 cargo 运行的原始输出，工具链 rustup 1.98.1，
`CARGO_NET_OFFLINE=true`，独立 `CARGO_TARGET_DIR`，命令自仓库根或临时树根）

| 文件 | 命令（要点） | 结果 |
|---|---|---|
| `01-new-suite-request-id-canonicalization.txt` | `cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test request_id_canonicalization -- --test-threads=4` | **7 passed / 0 failed / 0 ignored，exit 0**，7 个用例名逐一列出 |
| `02-new-suite-list.txt` | 同上以 `--list` 收尾 | **`7 tests, 0 benchmarks`**（与 pin=7 一致） |
| `03-existing-admission-dedup-consistency.txt` | `--test admission_dedup_consistency -- --test-threads=4` | **5 passed / 0 failed，exit 0** |
| `04-existing-admission-dedup-adversarial.txt` | `--test admission_dedup_adversarial -- --test-threads=4` | **5 passed / 0 failed，exit 0** |
| `05-xtask-pin-tests.txt` | `cargo test --manifest-path rust/Cargo.toml --locked -p xtask` | **100 passed / 0 failed，exit 0**，含 `r03_repair_producer_pin_table_matches_the_registered_suites` |
| `06-reviewer-retest-FIXED-candidate.txt` | 审查者自写 `rv1_independent_retest`（3 用例）在**修复候选的临时完整拷贝树**（rsync 自当前工作区 rust/，排除 target；测试文件加入拷贝树，从未进入仓库 rust/ 树） | **3 passed / 0 failed，exit 0** |
| `07-reviewer-retest-BASELINE-c96f7cc6.txt` | **同一份**审查者测试文件在 `git worktree`（detached @ c96f7cc6，产品代码零改动基线） | **3 FAILED，exit 101**——失败全部是「重试被全新受理 = 盲重执行」的缺陷行为，独立复现 RR2 报告指控 |
| `08-kernel-canonical-unit-tests.txt` | `cargo test -p lingxi-kernel -- request_cause_id_prefix canonical` | **2 passed / 0 failed，exit 0** |
| `09-workspace-full.txt` | `cargo test --manifest-path rust/Cargo.toml --locked --workspace` | **73 个 ok 结果行 / 合计 718 passed / 0 FAILED / exit 0**（与执行者 E5 声明一致的独立复算） |

## retest-src/（审查者自写复测源码，非执行者套件）

`rv1_independent_retest.rs` — 3 用例，刻意写成**同一文件可在基线与修复版两侧
编译**（修复后的错误变体用运行时 Debug 字符串匹配，不做编译期引用），因此
「修复侧通过 + 基线侧失败」构成对本缺陷的成对反证：

1. `rv1_a_padded_id_raw_and_canonical_retries_bind_the_one_logical_request`
   —— " req-42 " 前台真实受理 → 受控外部计数 +1 → `storage().close()` +
   drop + 同数据根新 boot → 原样 " req-42 " 与规范化 "req-42" 两种重试都必须
   是 `RequestIdBoundToEarlierRun` 指向**同一**旧 run；run 不重复创建、计数
   不增、拒绝回显 canonical 形态、落库锚点为 `request:req-42`。
2. `rv1_b_colliding_legacy_rows_refuse_as_explicit_ambiguity_never_a_pick`
   —— 经真实存储 API 写入两条归一到同一逻辑 key 的旧格式 cause_id
   （`"request: req-42 "` 与 `"request:\treq-42\t"`）→ 重启后三种形态重试都
   必须是 `RequestIdBoundAmbiguous` 并列出**全部两个** run（不得退化为单 run
   挑选、不得全新执行）；计数 0；两条冻结行逐字不变。
3. `rv1_c_same_logical_id_never_crosses_session_or_principal_namespaces`
   —— 同一逻辑 id " iso-9 " 在 owner 会话 alpha / owner 会话 beta / 同用户
   device 主体三个命名空间各自全新受理；重启后**各命名空间**的重试拒绝各自
   指向**自己的** run，绝不串用他命名空间绑定；计数不增。

基线侧（07 日志）三用例的失败点全部是
`FRESHLY accepted (… run_count 增长 …) — the silent blind re-execution`，
即 RR2 §3.1 源码链推出的行为，用审查者自己的测试在真实服务链上坐实。

## 环境与清理

- 基线侧用 `git worktree add --detach … c96f7cc6`（仓库工作区零影响），
  复测后 `git worktree remove --force` + `git worktree prune`；
  修复侧临时树与两个临时 `CARGO_TARGET_DIR` 均已删除。
  `git status` 中 rust/ 相关条目与审查开始时完全一致
  （`rv1_independent_retest.rs` 从未进入仓库 rust/ 树）。
