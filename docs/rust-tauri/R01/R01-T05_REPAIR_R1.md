# R01-T05 修复报告（REPAIR R1）

- 任务：R01-T05 修复轮；依据 `R01-T05_REVIEW_R1.md` 发现 F1–F5，逐发现修复，不扩大范围
- 修复代理：ZCode:R01-T05-repair-r1（全新独立，未参与执行/验收；不 commit/push）
- 基线 HEAD：`5a8a8e24ab9e8e105da5132d16b175221a91d646`；分支 `codex/rust-tauri-migration`
- 平台：macOS 27.0 arm64；Rust 1.98.1（rust-toolchain.toml 锁定）；Chrome 153.0.8010.52；Node v24.16.0
- 日期：2026-09-25
- 证据根：`artifacts/rust-tauri/R01/T05/repair-r1/`（replay/ negative/ gates/ setblockedurls-ineffective/）
- 环境约束：全程 `env -u 全部代理变量`；浏览器类进程限 loopback（T04 proxy.mjs 127.0.0.1:18482
  + canary 127.0.0.1:18291）；`CARGO_TARGET_DIR=/tmp/r01t05-repair/cargo-target`；数据目录 /tmp

## F1（MEDIUM）WebSocket 不经 Fetch 域拦截 —— 已修（原型级即时封堵，低成本可验证路径成立）

- **根因**：spike_pdf 的拦截仅 `Fetch.enable({patterns:[{urlPattern:"*"}]})`；Chrome 153 的
  Fetch 域不对 WS 握手触发 `requestPaused`，`decide_fetch` 从未被咨询；`--proxy-bypass-list=<-loopback>`
  形态下 loopback 目标走 loopback-only 代理（该代理放行 loopback），ws://127.0.0.1:* 端到端连通。
- **方案评估（按验收要求先评估再实现）**：
  1. `Network.setBlockedURLs(["ws://*","wss://*"])`——**实测无效**：注入后 ws://127.0.0.1:18291
     握手仍真实到达 canary（留证 `setblockedurls-ineffective/ws-canary.log` 1 次 GET
     `/t05-ws-repair-probe` + 对应 run-result.json）。该负面结论对 R09 选型有用，已写入 spike 报告 §8。
  2. 注入 CSP（`Page.addScriptToEvaluateOnNewDocument`，任何页面脚本执行前注入 meta
     `connect-src file: data: blob: http: https:`）——**实测有效**。白名单刻意只排除
     ws:/wss:：http(s)/file 连接不放行、照常穿透 Fetch 审计层，S5 的 7 条 DENY 日志语义
     与修复前逐条一致（repair replay 实测 fetch_denies=7 不变），不弱化既有负向证据。
  3. 代理层封堵不可行：T04/T05 吊具代理对 loopback 本就放行（canary 测试语义依赖），
     收紧代理会把「候选代码的边界」混淆为「测试吊具的边界」，不采用。
- **修复**：`rust/crates/lingxi-browser-spike/src/pdf.rs` render_job 在 Fetch.enable 后注入上述
  CSP（fail-closed：CDP 调用失败即整个 job 失败，不静默降级）；spike-result.json 新增
  `connect_boundary` 字段记录边界机制。
- **负向用例（新增，既有用例零删除零弱化）**：
  `tests/migration/r01-t05/negative/ws-loopback.html`（ws://loopback canary + wss://远端双通道）、
  `negative/hang-after-load.html`（F2 用）、驱动 `run_repair_r1_negative.sh`。
  实测 7/7 PASS（`repair-r1/negative/negative-verdicts.txt`）：
  ws-negative-exit PASS（exit=0，封堵不误伤渲染）/ ws-negative-canary PASS（canary 对
  ws://127.0.0.1:18291 握手**零命中**）/ ws-negative-pagemarker PASS（页面 JS 观测
  WS-LOOPBACK-BLOCKED + WS-REMOTE-BLOCKED）。
- **文档补记**：`PDF_SPIKE_REPORT.md` §8 第 7 条（缺口、影响分析、setBlockedURLs 无效事实、
  修复与 R09 门禁：负责人=R09 任务执行者、截止=R09 验收前）+ §3 远端资源读取限制行 + §5 补记；
  `ADR-003` 后续强制门禁节新增 WS 边界与超时契约两条。
- **交叉影响（T04）**：T04 浏览器 spike（`spike_browser.rs`）无 Fetch 拦截层、资源约束仅靠
  代理层；loopback WS 在其原型路径同样不受候选代码约束，属同一机制族缺口。按验收口径
  不改 T04 交付，登记留 T08 风险登记（已写入 spike 报告 §8 第 7 条末段）。

## F2（LOW）printToPDF 阶段超时 exit=1 ≠ 文档承诺的 2 —— 已修（修代码，二者一致 + 双回归）

- **根因**：`spike_pdf.rs` 以 `e.contains("timed out")` 判定超时；CDP 超时错误文案为
  "timeout: …"（`CdpError::Timeout` 的 Display），printToPDF 阶段超时串为
  "printToPDF: timeout: Page.printToPDF id=N"，漏匹配 → exit=1。
- **修复**：新增 `error_is_timeout()` 覆盖两类文案（"timed out" 与 "timeout:"），选修代码
  而非改文档（exit=2 契约是任务书级承诺，向上对齐）。
- **回归**：① bin 级单元测试 `timeout_exit_code_classification`（6 断言：load 文案 /
  printToPDF 文案 / navigate 文案 / 三类非超时错误），随 `cargo test` 通过；
  ② 集成回归 `run_repair_r1_negative.sh` case2（hang-after-load.html，复刻验收 ATK5 形态，
  embedLingxiFonts=false 使挂死点落在 printToPDF 阶段）：实测 **exit=2**、
  error="printToPDF: timeout: Page.printToPDF id=13"、零伪产物、profile_removed=true
  （`repair-r1/negative/print-timeout/`）。

## F3（LOW）计数笔误两处 —— 已更正为实测值

- `R01-T05_REPORT.md` §2「all_pass=true（41 项）」→ **35 项**（repair replay 复算
  compare.json checks=35，all_pass=true 属实）。
- `PDF_SPIKE_REPORT.md` 页首「623 项」→ **632 项**。

## F4/F5（INFO）—— 已在 R01-T05_REPORT.md §7 以勘误段收口（不回改历史结论，参照 R00 先例）

- F4：「37 passed: 11+7+19」少计 lingxi-protocol 7 个，实测 44 passed；勘误校准
  （本修复新增 bin 单测 1 个，当前实为 45 passed）。
- F5：A09 的 0.0009 文本层缺口主因 = 旧链 Chromium 142 文本运行顺序导致的数字串提取
  转置（提取层伪差，位图层无对应差异）+ SVG 数学文本化差异 + 𪚥 旧链缺字 + 2px 位图
  舍入差；不影响 A09 判据。

## 重跑命令与退出码（全部实测，日志在 repair-r1/gates/ 等目录）

| 项 | 命令 | 退出码 | 结果 |
|---|---|---|---|
| replay 全量 | `bash scripts/rust-tauri/r01-t05-replay.sh artifacts/rust-tauri/R01/T05/repair-r1/replay` | 0 | 19 场景退出码全符合预期（a10-infinite-new=2、a10-infinite-old=1，其余 0） |
| 矩阵判定 | `python3 tests/migration/r01-t05/analyze_matrix.py …/repair-r1/replay` | 0 | **28 VERIFIED / 0 FAILED**（analyze-matrix-repair-r1.log） |
| S5 证据语义保持 | 复算 replay a10-dangerous/new/run-result.json | — | fetch_denies=**7**（与修复前逐条一致）、profile_removed=true、connect_boundary 字段在位 |
| A09 复算 | replay a09/compare.json | — | checks=35、all_pass=true |
| A10 负向（WS+print 超时） | `bash tests/migration/r01-t05/run_repair_r1_negative.sh artifacts/rust-tauri/R01/T05/repair-r1/negative` | 0 | 7/7 PASS（negative-verdicts.txt） |
| T01 正向 | `python3 docs/rust-tauri/R01/r01_t01_check_ownership.py` | 0 | t01-check-positive-repair-r1.log |
| T01 N1–N15 | `… --self-test` | 0 | t01-self-test-N1-N15-repair-r1.log |
| T01 生成器 | `… r01_t01_build_ownership.py --check` | 0 | 736 features / 69 stores up-to-date |
| T02 roundtrip | `bash scripts/rust-tauri/r01-t02-roundtrip.sh` | 0 | t02-roundtrip-repair-r1.log |
| T02 handshake | `bash scripts/rust-tauri/r01-t02-handshake.sh` | 0 | t02-handshake-repair-r1.log |
| T02 check-generated | `bash scripts/rust-tauri/r01-t02-check-generated.sh` | 0 | t02-check-generated-repair-r1.log |
| cargo test | `cargo test --workspace --offline`（CARGO_TARGET_DIR=/tmp） | 0 | **45 passed**（11+1bin+7+19+7；含新增 F2 回归） |
| cargo fmt | `cargo fmt --all --check` | 0 | cargo-fmt-repair-r1.log |
| cargo clippy | `cargo clippy --locked --workspace --all-targets --offline -- -D warnings` | 0 | cargo-clippy-repair-r1.log |
| 样本确定性 | `generate_samples.py --check`（replay 第 0 步内含） | 0 | 样本零漂移（repair replay 留证） |

性能参照（repair replay，仅口径记录非阈值）：候选 median 1707.9/p95 1720.4ms
（printToPDF 331.0ms），旧链 994.5/1026ms——与执行者/验收两轮同量级，比例关系不变。

## 改动文件清单 + SHA-256

| 文件 | 改动 | SHA-256 |
|---|---|---|
| `rust/crates/lingxi-browser-spike/src/pdf.rs` | F1：CSP 注入（render_job step 3） | 701ed875e8c9ed518d65635d43d95889c64f3463979e8f59e8566cdbce7f0ede |
| `rust/crates/lingxi-browser-spike/src/bin/spike_pdf.rs` | F2：error_is_timeout + 单测；connect_boundary 字段 | d47f1a1d5747403fedf8c30ebdedace1398c66b88673506c86e0346b9a0120f7 |
| `tests/migration/r01-t05/run_repair_r1_negative.sh` | 新增：F1/F2 负向驱动 | 752440d717877642e53f9fc35fd6d1df3b43f5858982c43a26fcd98d2eebded3 |
| `tests/migration/r01-t05/negative/ws-loopback.html` | 新增：WS 负向样本 | 0d14b381a8b9c9a72af8dfbdcaafae21f7264ed4a95a378c8f054a55d79fc3fa |
| `tests/migration/r01-t05/negative/hang-after-load.html` | 新增：print 阶段超时样本 | f27efe238ed5243a9929d90e731822e22a7b532b8113390fc7e34c43c7f241ed |
| `docs/rust-tauri/R01/PDF_SPIKE_REPORT.md` | F3 计数 + F1/F2 补记（§3/§5/§8） | 87d8d55fe9979cd20d901a1676d457e882d0fad9186b568f179eb831872b0c57 |
| `docs/rust-tauri/R01/ADR-003-document-renderer.md` | F1/F2 门禁节补记 + 候选 C 行 | 0e69b99491bc08a57db2b127f423b04e9018694306149c4719e5f95dd3544c4b |
| `docs/rust-tauri/R01/R01-T05_REPORT.md` | F3 计数 + §7 勘误段（F4/F5） | 20f83f421a84e38c1eef2ceca01697cc3e70c1f85f2b8628e629e9990a1df4c6 |
| `artifacts/rust-tauri/R01/T05/repair-r1/` | 新增证据树（replay/ negative/ gates/ setblockedurls-ineffective/） | 逐文件见下条 |

（本报告自身哈希由验收方现算。）

## 红线与边界自查

- 未 commit/push；未改任务书目录、`.sync-audit`、`ORCHESTRATOR_PROGRESS.json`、
  既往验收报告原文（`R01-T05_REVIEW_R1.md` 零改动）；生产目录零改动
  （`git diff HEAD -- desktop/ server/ core/ lib/ shared/ cli/ hub/ plugins/ skills2set/
  package*.json rust/Cargo.lock` 为空，实测 0 行）。
- 改动仅限本任务交付物（spike crate 原型 + T05 测试资产 + T05 文档），与验收处置要求一致。
- 既有负向用例零删除零弱化：S5 七通道 Fetch 层 DENY 证据逐条保留（收窄 CSP 特意绕行
  http(s)/file，仅收口 ws:/wss:）；新增用例只做加法。
- 全程 loopback-only 代理 + canary（127.0.0.1）；代理自验 200(loopback)/403(example.com)
  留证 repair-r1/replay/proxy-selftest.txt；proxy.log 287×REFUSED。
- `artifacts/rust-tauri/R01/T05/repair-r1/` 下 `*.log` 命中根 .gitignore，提交时需
  `git add -f`（沿用既有先例）。

## 最终 git status --porcelain（修复后，本文件成文时复算）

```
 M docs/rust-tauri/ORCHESTRATOR_PROGRESS.json   （预先存在的总控账本，本修复未触碰）
 M docs/rust-tauri/R01/DEPENDENCY_DECISIONS.md  （T05 执行轮 D-10 追加，本修复未触碰）
 M docs/rust-tauri/R01/DEPENDENCY_RULES.json    （T05 执行轮职责补注，本修复未触碰）
 M rust/crates/lingxi-browser-spike/Cargo.toml  （T05 执行轮 description，本修复未触碰）
 M rust/crates/lingxi-browser-spike/src/lib.rs  （T05 执行轮 pub mod pdf;，本修复未触碰）
?? artifacts/rust-tauri/R01/T05/                （含本修复新增 repair-r1/ 证据树 + SHA256SUMS.txt 653 项）
?? docs/rust-tauri/R01/ADR-003-document-renderer.md        （本修复补记 F1/F2 门禁）
?? docs/rust-tauri/R01/PDF_SPIKE_REPORT.md                 （本修复更正计数 + §8 第 7/8 条）
?? docs/rust-tauri/R01/R01-T05_REPAIR_R1.md                （本报告）
?? docs/rust-tauri/R01/R01-T05_REPORT.md                   （本修复更正计数 + §7 勘误段）
?? docs/rust-tauri/R01/R01-T05_REVIEW_R1.md                （验收原文，零改动，diff 实测 0 行）
?? rust/crates/lingxi-browser-spike/src/bin/spike_pdf.rs   （本修复 F2 + connect_boundary）
?? rust/crates/lingxi-browser-spike/src/pdf.rs             （本修复 F1 CSP 注入）
?? scripts/rust-tauri/r01-t05-replay.sh                    （T05 执行轮交付，本修复未触碰）
?? tests/migration/r01-t05/                 （本修复新增 negative/ 两样本 + run_repair_r1_negative.sh）
```
