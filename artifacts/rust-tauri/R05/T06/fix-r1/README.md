# R05-T06 fix-r1（REV-T06 R01 · F-01）

- 修复者：R05-T06 第 1 轮独立修复者（独立于实现者与审查者会话）
- 基线：分支 `codex/rust-tauri-migration`，HEAD `c549ff654508ab951e2cf39cf9d309fc9c6b8656`（未提交工作树交付，no_commit_push_authorization=true；本轮未执行任何 git commit/push/stash/checkout/reset/clean）
- 对象 finding：F-01（mustFix，high）——C11B egress 守卫对非十进制 IPv4 拼写与 IPv4-mapped IPv6 按"https 公网主机名"放行，而 reqwest 所用 WHATWG 解析器（url 2.5.8）将其规范化为回环/私网地址后拨号。
- 审查报告：`artifacts/rust-tauri/R05/REVIEW-T06/R01_REVIEW.md`；探针原件：`artifacts/rust-tauri/R05/REVIEW-T06/probe-egress/probe-output.txt`（13/17 BYPASS）。

## 根因与修法（同根因一次修全）

根因：守卫用自有严格 dotted-decimal 解析器认定"字面 IP"，凡不认识的拼写（hex/八进制/短式/单整数/前导零/尾点/百分号编码 host、`::ffff:a.b.c.d`）全部落入主机名分支放行——认定面窄于拨号方（reqwest→url）的 WHATWG 规范化面。

修法（`rust/crates/lingxi-adapters/src/models/egress.rs`，唯一受影响产品文件）：

1. `parse_url`（egress.rs:78-149）：host 分类改用 `reqwest::Url::parse`（即 url 2.5.8，与 `rust/Cargo.lock` 同版；`reqwest::Url` 为 reqwest 0.13.5 的既有 re-export，**零新增依赖、Cargo.lock 不变**）——守卫认定的 host 与 HTTP 客户端实际拨号的 host 由构造保证同源，不可能因拼写分歧。IPv6 host 剥方括号后进入既有 `parse_ipv6`；空 host / 解析失败响亮拒绝。userinfo 改按 WHATWG 解析结果的 username/password 判定（消息不变）。origin 改由 `Url::port()` 生成，默认端口（https:443 / http:80）真隐去——旧实现的"默认端口隐去"仅存在于注释，两侧同为真语义。保留比 WHATWG 更严的前置：原始 authority 为空（如 `https:///a.png`，WHATWG 会跳过多余斜杠拨号 host `a.png`）仍拒绝。
2. `ip_is_guarded` 16 字节臂（egress.rs:267-284）：补 IPv4-mapped `::ffff:0:0/96` 回落到内嵌 v4 判定（双栈 socket 拨号即映射的 v4）；废弃的 IPv4-compatible `::/96`（涵盖 `::`、`::1`）一律拒绝——非合法公网出口目的地。
3. `check_url`（egress.rs:322-…）：分支适配 WHATWG 规范化 host（无方括号、IPv6 含 `:`）；`parse_ipv4`/`parse_ipv6` 保留为规范化形态上的纵深防御。错误消息保留既有关键子串（"guarded literal IP"/"guarded literal IPv6"/"plain http"/"userinfo"），服务级 C03 断言不回归。

同类路径核查：全仓唯一面向拨号的字面 IP/URL-host 判定逻辑就在 `egress.rs`（其余 `is_loopback` 均为已解析 `IpAddr` 的绑定/入站检查，adapters `operations/*` 只引用不抓取）——无需第二处修复。

## 回归测试（新增，egress.rs 测试模块）

- `non_decimal_ipv4_spellings_and_mapped_ipv6_refuse_like_their_normalized_form`（egress.rs:583）：审查探针 13 行 BYPASS 全部转 REFUSE，另加百分号编码 host、`::ffff:0.0.0.0`、`::7f00:1`（v4-compatible）；反向钉死折叠是策略精确而非一刀切——`[::ffff:8.8.8.8]` 与 `0x8.0x8.0x8.0x8`（公网 8.8.8.8）仍放行。
- `the_same_origin_exception_matches_normalized_origins_across_spellings`（egress.rs:627）：同源例外按规范化 origin 匹配（`0x7f.0.0.1`/`2130706433`/`127.1` 拼写的回环桩源仍过），例外之外仍拒；`:443` 隐去语义两端一致。

## 亲跑验证（命令、退出码；全部在 fix-r1 终态树上重跑）

| 检查 | 命令 | 结果 | 日志 |
| --- | --- | --- | --- |
| egress 单测（5 既有 + 2 新增） | `cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-adapters --lib egress` | 7 passed / 0 failed，exit 0 | `logs/fix-r1-adapters-lib-egress.log` |
| adapters C01/C02 方言矩阵 | `cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-adapters --test r05_t06_operations` | 27 passed / 0 failed，exit 0 | `logs/fix-r1-adapters-t06-operations.log` |
| 服务级 C02/C03/C12（含 egress 服务腿） | `cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r05_t06_operations` | 7 passed / 0 failed，exit 0 | `logs/fix-r1-service-t06-operations.log` |
| 审查者探针电池复验 | `cd artifacts/rust-tauri/R05/REVIEW-T06/probe-egress && CARGO_TARGET_DIR=/tmp/r05t06-fix-probe cargo run --quiet --offline` | 17 行全 ok，**bypass rows: 0**（原 13 行 BYPASS 全 REFUSE），exit 0 | `logs/fix-r1-reviewer-probe-rerun.log` |
| fmt 门禁（与 xtask stage_map 同命令） | `cargo fmt --manifest-path rust/Cargo.toml --all -- --check` | exit 0 | `logs/fix-r1-gate-fmt.log` |
| clippy 门禁（与 xtask stage_map 同命令） | `cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings` | exit 0 | `logs/fix-r1-gate-clippy.log` |
| url 2.5.8 行为取证（host_str 含方括号、`///` 容忍、各拼写规范化） | `/tmp/scratch-url` scratch crate（url 2.5.8，offline） | exit 0 | `logs/fix-r1-url-crate-2.5.8-behavior-probe.log` |

未执行：全 workspace 测试与 check-contracts/check-boundaries 两 xtask 门禁未由本轮修复者重跑（定向矩阵已覆盖 F-01 回归面；全量五门禁属终态树复验职责）。审查者目录（REVIEW-T06/）零改动（探针以独立 /tmp target dir 复跑，Cargo.lock mtime 18:53 早于复验时刻 ~19:13，未触碰）。

## 修后文件指纹

- `rust/crates/lingxi-adapters/src/models/egress.rs`：sha256 见 `egress-r1-sha256.txt`。
