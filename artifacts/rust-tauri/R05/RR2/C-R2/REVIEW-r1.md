# RR2 WP-C（F34）独立验收轮 1 — REVIEW-r1

验收者：RR2 WP-C 独立验收智能体（全新上下文，未参与实施；2026-10-06）。
被验候选：工作树（HEAD `ad5ec4e9853a51ed929f1e2e077b97d41c951572` + 未提交 RR2 改动）。
隔离副本：`/tmp/r05-rr2-c-accept/candidate`（最小集＝rust/Cargo.toml、rust/Cargo.lock、
rust/crates/、根 rust-toolchain.toml；关键两文件 `diff -q` 与主树逐字节一致后才动手）。

## 判定：PASS

## 逐项核验

### 1. 源码审查（主树）

- 新腿 `known_stop_reason_with_unclosed_tool_block_is_loud_and_dispatches_nothing`
  （tests/r05_t04_rr1_batch_terminal.rs:170）：
  - 夹具＝message_start → content_block_start(tool_use id=t1/name=read) →
    input_json_delta(`{"path":"a.txt"}`，完整合法、匹配快照 schema required path) →
    message_delta{stop_reason:"tool_use"}（已知映射终值）→ message_stop，
    **只缺 content_block_stop**（`frames(false)`，阳性对照 `frames(true)` 仅插入该帧）。✓
  - 断言经正式链 `match finish()`：`Ok(parsed) => panic!`（:195，Ok(ToolRequests) 即 panic，
    零工具执行）；`Err` 臂断言 code==InvalidMessage、!retryable、
    `message.contains("content block(s) [0]") && contains("content_block_stop")`
    ——点名具体未闭块索引（生产错误文案 `content block(s) {unclosed:?}`，[0] 为块索引）。✓
  - 阳性对照：恰一请求、provider_call_id=Some("t1")、target="tool:first-party:read"、
    arguments==`{"path":"a.txt"}` 完整、content 为空。✓
- 旧腿不变：`git diff` 全部删除行仅为文件头模块注释 4 行重排（加 F34 措辞），
  9 条旧测试函数体逐字未动（含 `anthropic_open_tool_block_cannot_close_as_a_completed_batch`）。✓
- 生产代码零改动：`anthropic_messages.rs` git diff 为空（unclosed 检查在 finish()
  :1247–1259，检查顺序＝message_stop_seen → unclosed → 缓冲解析的 stop_reason 门）。✓

### 2. 亲跑（主树，`/Users/study_superior/.cargo/bin/cargo`）

| 命令 | 退出码 | 结果 |
|---|---|---|
| `cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-adapters --test r05_t04_rr1_batch_terminal` | 0 | 10 passed / 0 failed（9 旧 + 1 新） |
| 同命令于隔离副本（基线） | 0 | 10 passed / 0 failed |
| `cargo test … -p xtask --bin xtask -- stage_map::map_tests::r05_stage_pin_table_matches_the_registered_suites --exact` | 0 | 1 passed |
| `cargo test … -p xtask --bin xtask -- stage_map::map_tests::r05_stage_cid_table_owns_every_pinned_test_exactly_once --exact` | 0 | 1 passed |
| `cargo clippy --manifest-path rust/Cargo.toml --locked -p lingxi-adapters --tests -- -D warnings` | 0 | 无告警 |

### 3. 隔离变异（仅动 /tmp 副本，两种方式，均与执行者 `&& false` 短路不同）

- 变异 A（整体删除防线：`let unclosed …` + `if !unclosed.is_empty() { return Err(…) }`
  十三行整块替换为注释；diff 单点差异）：
  同套件命令 → **exit 101**，9 passed / 1 failed——**恰新腿 FAILED**
  （panic 于测试 :195 正式断言点，放行形态为 `ToolRequests { … provider_call_id: Some("t1"),
  canonical: {"path":"a.txt"} }`——正是该腿要防的完整工具放行）；**旧腿
  `anthropic_open_tool_block_cannot_close_as_a_completed_batch` 保持绿**（其拒绝被
  缺-stop_reason 防线吸收，复现 F34 事实）。还原（cp 主树原文件，diff -q 一致）→ exit 0，
  10/10 绿。日志：/tmp/r05-rr2-c-accept/{mut1-red.log, mut1-restored-green.log}。
- 变异 B（中性化谓词：`.filter(|index| !self.closed.contains(index))` → `.filter(|_| false)`，
  unclosed 恒空、if 保留、零编译警告）：
  → **exit 101**，9 passed / 1 failed，恰新腿红、旧腿绿；还原 → exit 0，10/10 绿。
  日志：/tmp/r05-rr2-c-accept/{mut2-red.log, mut2-restored-green.log}。

### 4. 映射登记核验

- `r05_stage_pins.tsv`：`pin adp:r05_t04_rr1_batch_terminal 10 r05_t04_rr1_batch_terminal`，
  计数 10 == 套件实测测试数 10（正则枚举 #[test] 亦为 10）。✓
- `r05_stage_cids.tsv`：新腿名按字母序并入 `cid R05-T04-C11 adp:r05_t04_rr1_batch_terminal`
  行；程序化交叉核对：套件全部 10 个测试名在该 TSV 中**各出现恰一次**（无重复、无遗漏），
  另 2 条归 C10。✓
- `rust/crates/xtask/src/stage_map.rs` 镜像期望 9→10（单行 diff）；两镜像测试亲跑 --exact
  各 exit 0，且实现确认**读取仓库真实 TSV**（parse_r05_pin_table/cid_table/required_cid_registry
  经 r05_repo_root() 读 live 文件）——绿结果即登记↔镜像双向一致。✓
- F26 103 集合：`r05_required_cids.tsv` git diff 为空（未触碰，最后修改于 HEAD ad5ec4e98）；
  实测 `reqcid` 行 103（93 `cid` + 10 `command:` 绑定）、103 个不同 C-ID，与镜像测试内嵌
  `assert_eq!(registry.len(), 103)` 双重一致。✓

## 结论

F34 缺口已按 RR2_MASTER_PROMPT §四C 关闭：变异不红=FAIL（两种方式均恰新腿红）、
旧腿被改弱=FAIL（旧腿逐字未动且变异下保持绿）、映射重复/遗漏=FAIL（完备性核对通过）
三条否决线均不触发。无 mustFix。主树被验对象零修改（本验收只写 /tmp 与本文件）。
