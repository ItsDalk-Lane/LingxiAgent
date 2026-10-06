# RR2 WP-A（F41）独立验收记录 r1 — REVIEW-r1

- 审查者：ZCode:RR2-A-REVIEWER-r1（全新上下文，未参与实施；2026-10-06）
- 候选：codex/rust-tauri-migration @ ad5ec4e98 工作树（未提交 RR2 改动）
- 结论：**F41 verdict = PASS**（无 mustFix；2 条非阻塞观察）
- 亲跑证据根：/tmp/rr2a-review/（本审查专用，主树零接触；唯一仓库写入=本文件）

## 1. 指纹独立重算（审查者自有提取代码，非复用 inspector/consistency 解析器）

从 rust/crates/lingxi-adapters/src/storage/migrations.rs 按 `r#"`…`"#;` 定位原文提取 SQL 体并 sha256：

| v | 我的 sha256(SQL) | 登记册值 | 真实二进制 receipts==compiledIn（我自建 fresh 库） |
|---|---|---|---|
| 1 | 479b0321494269fca85d9f973b01a8f9d1aa57dc08fc3cf8fddbafeace461bb8 | 同 | 同 |
| 2 | 64d7edfdab74e13e623a9d8d5f2381dad4803f545fa126d8eda8cc02c3dc889c | 同 | 同 |
| 3 | 3bd5388f090ab0ae0e137d91d59a8e48f82f409bbd0de657abf29b897a4252c1 | 同 | 同 |
| 4 | 371d415462b7e8d698693f50a7b9c9f493fbb4e653ee30e5d3e8930f77e161b5 | 同 | 同 |
| 5 | 16dce1c9d5106b74ef00df7348591f5fad11bf8a7bc0244effba804dfde7606d | 同 | 同 |
| 6 | cd875eae835e6a5ecdee8480b7c7c223eea5a180eaf8ee3c2b43416a7deadb86 | 同 | 同 |
| 7 | b3189ef9c77630918436df07291bccd8f5a6a55f21109153ff73841cef58e3fb | 同 | 同 |

- v6/v7 名称与 migrations.rs V6_NAME/V7_NAME 逐字一致；registered_by 来源描述与实际 SQL 内容相符（v6=5 ALTER+3 索引；v7=整表重建保行+索引原样重建）。
- v1–v5 逐字未动：程序化对比 working tree 与 HEAD 的 migrations 数组，前 5 条 byte-identical，diff 仅 +v6/v7 两条 + generated_by 溯源延长。
- migrations.rs `git diff` 为空（零改动）。

## 2. r02_registry_consistency.py 语义审查

- 等式而非交集/子集：条数不等单独报错；版本并集逐版点名（代码有/登记缺、登记有/代码无均 FAIL）；名称、指纹逐一精确比较。
- 解析 fail-closed：MIGRATIONS 切片缺失/解析空/引用悬空 const/重复 const 均 SystemExit 非 0；只有被 MIGRATIONS 切片引用的 const 计数（散落 const 不得静默通过）。
- 未发现放水路径。观察①（非阻塞）：MIGRATIONS_ENTRY_RE 依赖条目格式，若未来 rustfmt 改写成不匹配形状会少解析条目——但需同时截断登记册才会假绿，且 S4 二进制级等式独立兜底。

## 3. 亲跑命令与退出码（全部本人执行）

| # | 命令 | exit |
|---|---|---|
| 1 | env …CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=/tmp/rr2a-review/rust-target /Users/study_superior/.cargo/bin/cargo build --manifest-path rust/Cargo.toml --locked -p lingxi-service -p lingxi-adapters | 0 |
| 2 | rust-target/debug/lingxi-service --home <fresh> --bind 127.0.0.1:0（日志在 home 外）；inspector …migrations | 0/0 |
| 3 | v7 重开×3（service+inspector），三轮 migrations JSON `cmp` 字节级相同且等于首开 | 0 |
| 4 | 自建 v5 库（V1..V5 SQL 逐字+真实 receipts+2 行非空 usage，审查者自己的行值）→ 真实 service 升级 | 0 |
| 5 | PATH=$HOME/.cargo/bin:$PATH CARGO_TARGET_DIR=/tmp/rr2a-review/rust-target bash scripts/rust-tauri/r02_t04_storage_tx.sh /tmp/rr2a-review/gate1 | **0**（S0+S1–S4 ALL GREEN，见 gate1/summary.txt） |
| 6 | bash /tmp/rr2a-review/review-mutations.sh（隔离副本三变异+恢复，S0/S4eq/realgate 三门禁） | **0**（3/3 精确失败+恢复绿） |
| 7 | bash -n r02_t04_storage_tx.sh；python3 -m py_compile r02_registry_consistency.py；cargo fmt --all -- --check | 0/0/0 |

- v5→v7 数据核验：两行全部原列逐字保留；v6 新列 outcome='unknown'、started/settled/parent_tool_call_id/emitted_tool_calls NULL；升级前 NOT NULL 拒绝 NULL、升级后接受且读回 NULL≠0（rev-one=7、rev-two=2 观测值原样）。
- 变异细节（S4eq=从真实脚本 awk 提取的 28 行断言块逐字喂数，配我自己的 fresh 库 inspector 输出）：
  - 删 v6：S0 exit1（点名 v6 缺失+7≠6）；S4eq exit1 复现 F41 签名 "userVersion 7 / supportedVersion 7 disagree with the registry's 6 migrations"；真实完整脚本在隔离副本 exit 1（S0 前置快速失败，零构建）。
  - 篡改 v7 指纹：三门禁各 exit1，点名 v7 真实值 vs 篡改值。
  - 篡改旧 v2 指纹：三门禁各 exit1，点名 v2（旧条目受保护）。
  - 恢复后 S0/S4eq 双绿。
- S4 等式逐字保留：r02_t04_storage_tx.sh 的 diff 为单一纯插入 hunk（+15 行 S0，位于构建之前），S1–S4 无一行改动。

## 4. 观察（非阻塞，无需修复）

1. 执行者自有 mutations/run.sh 的 check() 仅以 S0 退出码判例（S4eq 退出码只记录不强制）；本审查用自己的三门禁强制复核，结论不受影响。
2. 首次尝试把 service 日志放进 --home 内被 data-epoch 闸拒绝（unstamped-home-with-data，exit 2）——防御行为正确，非缺陷。

## 5. 红线核对

- 未修改任何被验对象（主树唯一写入=本文件；误产生的 scripts/rust-tauri/__pycache__ 已当场删除并核对 git status）。
- 无 git commit/push/branch/tag。cargo 全程绝对路径 /Users/study_superior/.cargo/bin/cargo。
