# R05 RR1 WP-T02 R2 执行者报告（F30 修复 + F31 登记，2026-10-04）

执行者：R05-T02-修复-r1（全新上下文修复智能体，未参与 R1 实现）。工具链：`/Users/study_superior/.cargo/bin/cargo`（rustup 1.98.1）。
改动面：`rust/crates/lingxi-service/src/credentials/login.rs`（唯一生产文件）+ `rust/crates/lingxi-service/tests/r05_t02_credentials.rs`（新增 2 测试腿）+ `docs/rust-tauri/R05/repair-current/RR1_LEAF_DEVIATIONS.md`（新登记册）+ 台账两文件。零依赖变更、零管理面/adapter 改动。

## 1. F30 旧行为反例固化（修复前，本候选树上）

- 审查者原探针复跑（断言原文逐字未动，SHA256 与 R1 审查记录一致）：`/tmp/r05t02-f30-fixprobe`（path deps 指向本候选树）
  命令：`cd /tmp/r05t02-f30-fixprobe && /Users/study_superior/.cargo/bin/cargo test --offline`
  结果：**FAILED（exit 101）**——`manual replay outcome: Ok(LoggedIn { persisted: true })`、`token-endpoint exchanges after replay: 2`。
  证据：`old-red-f30-probe-pre-fix.log`。
- 新仓库回归腿（`mod rr1_f04`）先红：`rr1_f04_browser_completion_then_manual_replay_refused_zero_new_exchanges`
  命令：`/Users/study_superior/.cargo/bin/cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r05_t02_credentials rr1_f04_browser_completion_then_manual_replay_refused -- --test-threads=2`
  结果：**FAILED（exit 101）**——浏览器腿完成登录后，同一 (state,code) 手输码重放返回 **200** `{"outcome":{"persisted":true,"status":"loggedIn"}}`（期望 409）。
  证据：`old-red-f30-repo-leg-pre-fix.log`。
- 误吃防护腿基线：`rr1_f04_stale_completion_after_restart_never_eats_the_new_transaction` 修复前即为绿（正对照，钉住修复必须保持的性质，非旧红主张）。

## 2. 根因与修复

根因：浏览器环回 listener 完成路径（oauth_start spawn 任务）直接调 `complete_with_grant`，不做一次性消费；只有手输码路径 `oauth_complete_code` 消费事务。浏览器腿完成安装后 `PendingLogin` 仍留在 logins 表，同一 (state,code) 经手输码面重放可再次交换+安装。

修复（login.rs）：
1. 新增 `consume_pkce_login_matching(provider, principal, state)`——**校验+移除在同一锁持有内原子完成**：PKCE kind、同主体、**同 state**、未过期全部匹配才移除并返回；任何不匹配零写入且事务保留。state 属本次 flow 才能消费 ⇒ 旧 flow 的迟到完成（含并发 restart 换新事务）只会 state mismatch，**不误吃并发新事务**。
2. `oauth_complete_code` 改用该原子消费（消除原先 validate 与 take 两次加锁之间的 TOCTOU——原先并发新 start 可能在窗口内被旧完成误吃；错误消息语义保持不变）。
3. 浏览器 listener 任务在 `await_callback` 成功后、**交换之前**先做 state 匹配消费，成功才 `complete_with_grant`——两条完成路径共用同一消费点，此后任何重放（手输或浏览器）都无可吃事务，零新增交换、零写入。
4. 消费先于交换 ⇒ 交换/安装的失败路径与手输码一致（one-shot，重新 start）。模块文档与 `complete_with_grant` 文档同步更新为两路共同契约。

同类路径全扫描（`take_login`/`complete_with_grant` 全部调用点核对）：完成路径 3 条（手输码、浏览器、device Done）全部先消费；终结路径（device 过期/终态错误、logout、revoke、新 start 替换）保留原 cancel+drop 语义，未改动。

## 3. 修复后自检（全部亲跑）

| 命令（均经 /Users/study_superior/.cargo/bin/cargo） | 退出码 | 结果 |
|---|---|---|
| `cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r05_t02_credentials rr1_f04 -- --test-threads=2` | 0 | 13/13（11 既有 + 2 新增） |
| 审查者探针复跑（同 §1 命令） | 0 | 1/1 绿（`green-f30-probe-post-fix.log`，重放拒 + 交换数保持 1） |
| `cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r05_t02_credentials -- --test-threads=2` | 0 | 38/38（36 既有 + 2 新增；`green-r05t02-credentials-full.log`） |
| `cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service -p lingxi-adapters` | **0** | **1081 通过 / 0 失败**（R1 基线 1079 + 2 新腿；`green-both-crates-full.log`） |
| `cargo fmt --manifest-path rust/Cargo.toml --all --check` | 0 | 无 diff |
| `cargo clippy --manifest-path rust/Cargo.toml --locked -p lingxi-service -p lingxi-adapters --all-targets -- -D warnings` | 0 | 无告警 |

探针 crate `/tmp/r05t02-f30-fixprobe` 为会话内临时目录（复刻审查者隔离探针布局），不进仓库树，验证后已删除；审查者原始探针源码保留在 `WP-T02-R1-INDEPENDENT/f30-probe-src.rs` 供重验。

## 3.1 证据文件（WP-T02-E02/，SHA256）

- old-red-f30-probe-pre-fix.log / old-red-f30-repo-leg-pre-fix.log（修复前双旧红）
- green-f30-probe-post-fix.log（审查者探针转绿）
- green-r05t02-credentials-full.log（38/38）
- green-both-crates-full.log（1081/0，exit 0）
- fmt-check.log / clippy.log（exit 0）

## 4. F31（登记，非行为改动）

新建 `docs/rust-tauri/R05/repair-current/RR1_LEAF_DEVIATIONS.md` 条目 1：LA-CFEC64F68DDE 拒绝边界 404→409 的有意偏差——权威依据（总控 §4 F04 叶文本"非 OAuth 明确拒绝"未规定状态码）、实现位点（management.rs `NotOAuth` 分支）、钉住测试（`rr1_f04_oauth_model_listing_and_non_oauth_rejection`：aux 409+oauth 命名、ghost 404）、理由（404 伪装"provider 不存在"，掩盖已知 provider 存在）、F25 处理指令（语义等价类或携带登记，禁止静默对齐/误杀）。未改任何 R00 叶文本、未放室断言。

## 5. 状态

F30：OPEN → IMPLEMENTED（rounds 1，本报告）；F04：R1 FAIL（F30）→ R2 IMPLEMENTED 待新独立审查；F31：OPEN → IMPLEMENTED（登记完成，F25 吸收归 WP-T08）。重验要求（总控 §3.3）：换新独立审查者复验 rr1_f04 全套 + 2 条新腿 + 本人探针 `f30-probe-src.rs`（已在本树转绿）；F03/F05 未触碰、无须返工。
