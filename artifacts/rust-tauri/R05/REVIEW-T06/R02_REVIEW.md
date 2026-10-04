# R05-T06 独立审查报告（REV-T06 R02 · fix-r1 复验）

- 审查对象：R05-T06 fix-r1 对 R01 发现 F-01（mustFix, high）的修复复验 + 高风险链抽验
- 审查轮次：R02（第 2 轮独立验收；审查者未参与实现与修复）
- 基线：分支 `codex/rust-tauri-migration`，HEAD `c549ff654508ab951e2cf39cf9d309fc9c6b8656`；R05-T01—T06 未提交工作树交付（no_commit_push_authorization=true）
- 审查日期：2026-10-03；上轮报告：`artifacts/rust-tauri/R05/REVIEW-T06/R01_REVIEW.md`（CONDITIONAL_GO）
- 证据目录：`artifacts/rust-tauri/R05/REVIEW-T06/`（R02 新增 `logs/r02-*.log`、`probe-egress-r02/`；R01 证据原件未改动）
- 只读边界遵守声明：本轮未修改任何产品代码/测试/文档/台账；新增文件仅在本目录。R02 探针为独立 crate（`probe-egress-r02/`，target dir 在 `/tmp`），path-dep 当前工作树真实 `lingxi-adapters`。

## 总体判定：PASS（离线范围；F-01 已按根因关闭）

F-01 的修复是**根因级**而非表面补丁：守卫不再用自有解析器认定"字面 IP"，`parse_url`（egress.rs:111）改以 `reqwest::Url::parse`（即拨号方使用的同一 WHATWG 解析器，url 2.5.8 与 `rust/Cargo.lock` 同版）分类 host——守卫判定面与拨号面**由构造同源**，拼写分歧这一类根因（而非 13 行个案）被消除；`ip_is_guarded` 16 字节臂（egress.rs:266-287）补 `::ffff:0:0/96` 映射回落并拒绝废弃的 `::/96`。本审查者的 R02 扩展探针（**34 行 = R01 原 17 行 + 17 行 fixer 未钉的新拼写**，含单整数 `0`/`1`、hex 短式 `0x7f.1`、八进制整数 `017700000001`、大写 `HTTPS://0X7F...`、映射+端口、逐组写全的映射私网、v4-compatible、ULA、组播、两类 userinfo 陷阱）对生产守卫亲跑：**bypass rows: 0**，且策略精确性未过度收紧（公网 v6 `[2606:4700::1111]`、映射公网 `[::ffff:8.8.8.8]`、数字前缀域名仍放行——DNS 范畴维持已声明缺口，未一刀切）。回归矩阵独立复跑全绿（adapters lib 全量 102 = 原 100 + 2 个新对抗单测、adapters T06 27、service T06 7（含 C03 egress 服务腿）、worker model 9、workerrpc units 6，均 exit 0）。上轮遗留 F-02/F-03 维持建议级、按修复声明的归属延后；LIVE 维持 `BLOCKED_NOT_AUTHORIZED`。

## 复核范围声明（三类证据分明）

- **亲自运行**：R02 扩展探针（34 行，对生产 `EgressGuard::check_url` + url 2.5.8 双侧对照）；五组定向测试复跑 + `cargo fmt --all -- --check`；修后 `egress.rs`/`Cargo.lock`/`Cargo.toml` 哈希核验；T06 manifest 63 文件全量复算；台账 `fix_rounds` 结构读取。
- **源码推断（亲读，未单独动态复现）**：修后 `egress.rs` 全文（parse_url/ip_is_guarded/check_url/新增 2 单测）、`transport.rs:150-205`（同类路径核查——入站 Host 头分类，非拨号面）、`dispatch.rs`/`credentials.rs` 的 origin 面 grep（凭证无独立 origin 比较，仅按 route 建 plan 时应用，redirect 不跟随）；fix-r1 的 7 份日志与 README 内容核对。
- **未验证**：全 workspace 测试与 check-contracts/check-boundaries/clippy 未由本审查者重跑——编排者本轮声明已在当前（修后）树上亲跑五门禁全 exit=0（本 prompt 所列）；修复者另在 fix-r1 亲跑 fmt/clippy exit 0（`T06/fix-r1/logs/`，本审查者仅重跑了 fmt）。真实供应商 LIVE（未授权）；Windows/Linux 平台。

## F-01 关闭验证（逐条对照 R01 的修复要求）

R01 修复要求 vs 实际（每条有本审查者证据）：

| R01 要求 | 修后事实 | 本审查者证据 |
| --- | --- | --- |
| 守卫按 WHATWG host 语义识别全部字面 IP 形态（或改用 url crate 解析 host） | 采纳更强方案：`reqwest::Url::parse` 分类 host（egress.rs:111-116），与拨号方同解析器同版本；hex/八进制/短式/单整数/前导零/尾点/百分号编码 host、畸形端口/括号、zone id 在解析层统一拒绝或规范化 | 探针 34 行亲跑；源码亲读 |
| `ip_is_guarded` 补 `::ffff:0:0/96` 回落 | egress.rs:271-276 折叠到内嵌 v4 判定；且超出要求地拒绝了废弃 `::/96`（涵盖 `::`、`::1`，egress.rs:280-282） | 探针映射 6 行全 REFUSE；`[::0.0.0.1]`/`[::7f00:1]` REFUSE |
| 13 行 BYPASS 全转 REFUSE | 13 行全 REFUSE（探针 R01 电池复跑 + fixer 复跑日志双证） | `probe-egress-r02/r02-probe-output.txt` 前 17 行 |
| 既有 5 单测与 C03 服务级腿保持绿 | adapters lib 全量 102 passed（含 5 既有 + 2 新）；service T06 7 passed（C03 egress 腿在内） | `logs/r02-adapters-lib-full.log`、`logs/r02-service-t06-operations.log` |
| 同源例外/默认端口隐去不回归 | 例外改按**规范化 origin** 匹配（拼写的回环桩源仍过、例外之外仍拒，`the_same_origin_exception_matches_normalized_origins_across_spellings` 亲跑绿）；`:443` 隐去由 `Url::port()` 两端真实语义承载（egress.rs:140-143） | 亲跑 + 源码亲读；规范化匹配不越出已配置 origin（同 host:port 才匹配），无放宽 |
| 同类路径一并处理 | 全仓拨号面的 URL-host/字面 IP 判定仅 `egress.rs` 一处：`transport.rs:150 host_is_loopback` 为入站 Host 头分类（fail-closed，不产生外发）；`dispatch.rs:329`/`embedding.rs:422` 的 `split_once("://")` 为已配置 endpoint 的路径拼装；`lib.rs`/`management.rs` 的 `is_loopback` 均作用于已解析 `IpAddr`（绑定/入站） | grep 亲跑 + `transport.rs:145-205` 亲读 |
| 零新增依赖 | `rust/Cargo.lock`（259f983e…）与 `lingxi-adapters/Cargo.toml`（e9b4b24e…）哈希与 T06 manifest 原值一致，未动；`reqwest::Url` 为 0.13.5 既有 re-export | 亲自 shasum |
| 仅一个产品文件受影响 | 修后 `egress.rs` = b52d1534…（与 fix-r1 声明逐字一致）；T06 manifest 63 个 tracked 文件全部仍匹配（R02 复算 63/63）→ tracked 树自 T06 终树未再变动，唯一变化产品文件即 untracked 的 `egress.rs` | `logs/r02-manifest-check.log`；亲自 shasum |

**修复质量要点（根因 vs 表面）**：新单测把 R01 探针 13 行 + 百分号编码 + `::ffff:0.0.0.0` + `::7f00:1` 全部固化为常驻回归，且反向钉死折叠是策略精确（公网映射/hex 公网仍放行）——不是"把所有可疑形态一刀切拒绝"的过度修复；同源例外语义随规范化升级而**变严不变宽**（匹配面=规范化 origin 本身）。zone id 在 url 解析层即拒（探针行 13：双侧 parse-error，守卫 REFUSE）。

## 高风险链抽验（R01 标记项的回归确认）

- worker async 桥四链复跑：`r05_t06_worker_model` 9 passed（C04 真实网关/trace、C05 单线程延迟回调、C06 正负腿、C07 重放、C08 超限零外发、C09 双拒、C10 挂起 Unknown、C11 零密钥）——`logs/r02-service-t06-worker-model.log`，exit 0。
- `workerrpc` 预算/回收单测 6 passed——`logs/r02-workerrpc-units.log`，exit 0。
- adapters C01 方言矩阵 27 passed——`logs/r02-adapters-t06-operations.log`，exit 0。
- fmt 门禁亲跑 exit 0——`logs/r02-gate-fmt.log`。

## 发现清单（R02）

- **无新增必修项。**
- **F-04（建议，low，并入 F-02 的终态树重冻结仪式）**：`R05_ACCEPTANCE_LEDGER.json` 的 `R05-T06-C11B`/`R05-T05-C11B` 条目 observed 仍写"5/5 绿"、evidence 仍指向修前 `T06/test-adapters-egress-lib.log`；修复后事实是 7/7（fix-r1 logs）+ 探针 bypass 0。`PROGRESS_LEDGER` 的 fix_rounds 已如实登记修复与复跑，故这不是行为失真，而是验收台账交叉引用未随 fix 刷新——与 F-02（manifest 未含 untracked 交付文件）同属终态树重冻结时需一并完成的登记动作：重冻结 manifest + 刷新 C11B 两条 evidence/observed 绑定修后执行。R01 的 F-03（say argv voice 前缀防御纵深）维持登记，归属不变。
- 备注（非发现）：`egress.rs:572` 测试注释仍引用已不存在的 `origin_of` 符号名——纯注释陈旧，行为无影响，下次触碰该文件时顺手更正即可。

## 登记

- **registeredBlockers**：LIVE 供应商验证 `BLOCKED_NOT_AUTHORIZED`（RR-BLK-CREDENTIALS，最迟 R10）——任务书允许的延期，不计 FAIL；Windows/Linux 平台延期维持任务书 §9 状态。
- 五门禁中的 workspace-test / clippy / check-contracts / check-boundaries：编排者已在当前（修后）树亲跑全 exit=0（本轮任务书所列）；fmt 本审查者另亲跑 exit 0。R05-ENV-ALF-UNSIGNED-TEST-BINARY：本轮定向复跑（loopback-only）未触发，按台账环境项处理。
- 终态树重冻结义务（F-02 + F-04）：T08 verify-stage 前统一完成并重新绑定证据。
- 本轮 R02 判定为**修复验证轮**结论：F-01 关闭、无回归、无新增必修——R01 的 CONDITIONAL_GO 条件已满足，T06 在离线范围内转 PASS。
