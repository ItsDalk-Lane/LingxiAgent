# RR2 WP-A（F41）实施证据 — A-R2

- 候选：codex/rust-tauri-migration @ ad5ec4e9853a51ed929f1e2e077b97d41c951572（工作树含本 WP 改动 + 并行 WP-C 的无关改动，均未提交）
- 工具链：rustup 1.98.1（绝对路径 /Users/study_superior/.cargo/bin/cargo；rust-toolchain.toml channel=1.98.1）
- 执行者：ZCode:RR2-WP-A（2026-10-06）
- 红线遵守：migrations.rs 零改动（git diff 为空）；S4 等式断言逐字保留；无 git commit/push。

## 变更（本 WP 所有权内）

1. docs/rust-tauri/R02/R02-T04_STORAGE_REGISTRY.json：补登 v6/v7（真实名称、sha256(SQL) 指纹、来源 F21/F38），v1–v5 条目逐字保留（diff 仅新增 13 行 + generated_by 溯源延长）。
2. scripts/rust-tauri/r02_registry_consistency.py（新）：等式自检——migrations.rs MIGRATIONS 与登记册在 条数+版本+名称+sha256(SQL)指纹 上精确相等（非交集；单向缺失、双向多余、指纹漂移均点名失败）。
3. scripts/rust-tauri/r02_t04_storage_tx.sh：新增 S0 步（零构建成本的登记一致性预检），S1–S4 原样保留。

## 指纹三方交叉验证（源码 SQL → sha256 == 二进制 receipts == 登记册）

- v6 model_call_usage_rr1_f21 = cd875eae835e6a5ecdee8480b7c7c223eea5a180eaf8ee3c2b43416a7deadb86
- v7 model_call_usage_rr1_f38_attempts_nullable = b3189ef9c77630918436df07291bccd8f5a6a55f21109153ff73841cef58e3fb
- 来源：真实构建的 lingxi-service 在全新 home 创建 runs.db（open 时应用迁移），lingxi-storage-inspect `migrations` 输出的 receipts 与 compiledIn 在 (version,name,fingerprint) 上精确相等；v1–v5 receipts 与登记册旧值逐字一致（旧指纹未被改动）。指纹登记值逐字取自该二进制输出，非手抄。

## 命令与退出码（全部真实执行）

| # | 命令 | exit |
|---|---|---|
| 1 | env … CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=$TMPDIR/rust-target-r02-t04 /Users/study_superior/.cargo/bin/cargo build --manifest-path rust/Cargo.toml --locked -p lingxi-service -p lingxi-adapters | 0 |
| 2 | $TARGET/debug/lingxi-service --home <fresh> --bind 127.0.0.1:0（建库）；…/lingxi-storage-inspect <db> migrations | 0 / 0 |
| 3 | python3 scripts/rust-tauri/r02_registry_consistency.py | 0 |
| 4 | bash artifacts/rust-tauri/R05/RR2/A-R2/upgrade-v5-to-v7/run.sh | 0 |
| 5 | PATH=$HOME/.cargo/bin:$PATH bash scripts/rust-tauri/r02_t04_storage_tx.sh artifacts/rust-tauri/R05/RR2/A-R2/r02-storage-tx | 0 |
| 6 | /Users/study_superior/.cargo/bin/cargo fmt --manifest-path rust/Cargo.toml --all -- --check | 0 |
| 7 | bash artifacts/rust-tauri/R05/RR2/A-R2/mutations/run.sh（隔离副本三变异+恢复） | 0 |
| 8 | bash -n scripts/rust-tauri/r02_t04_storage_tx.sh；python3 -m py_compile scripts/rust-tauri/r02_registry_consistency.py | 0 / 0 |

## 自检结论

1. 新建库从零到 v7：PASS（fresh-migrations.json userVersion=supportedVersion=7，7 receipts==compiledIn）。
2. v5（非空 usage 2 行）升级 v7：PASS（两行全部列逐字保留；v6 新列 outcome='unknown'、时间戳/血缘列 NULL；升级前 NOT NULL 拒绝 NULL、升级后接受且读回为 NULL 非 0）。
3. v7 重开幂等：PASS（S4 三轮 restart 的 migrations JSON 逐字节相同，行数稳定，inspector 只读哈希不变）。
4. r02_t04_storage_tx.sh S0+S1–S4：ALL GREEN（exit 0）。
5. cargo fmt --check：PASS（本 WP 未改 Rust 文件）。

## 反例（隔离 /tmp 副本，主树零接触）

| 变异 | S0 等式自检 | S4 等价断言（真实 inspector 输出喂入） |
|---|---|---|
| 删登记册 v6 条目 | exit 1，点名 v6 缺失 + 条数 7≠6 | exit 1（逐字复现 F41 签名 "userVersion 7 / supportedVersion 7 disagree with the registry's 6 migrations"） |
| 篡改 v7 指纹 | exit 1，点名 v7 指纹不匹配（真实值 vs 篡改值） | exit 1，compiledIn v7 指纹断言 |
| 篡改旧指纹 v2（v1–v5 类） | exit 1，点名 v2 指纹不匹配 | exit 1，receipt v2 指纹断言 |
| 恢复后 | exit 0 | exit 0 |

## 证据文件

- fingerprint-extraction/（build.log、fresh-migrations.json、fresh-service.out/.err）
- upgrade-v5-to-v7/（run.sh 可复跑、run.log、usage-before/after-upgrade.json、schema-v5.sql、migrations-after-upgrade.json、unknown-semantics.json）
- r02-storage-tx/（正式门禁全套输出）与 ../r02-storage-tx-stdout.log
- mutations/（run.sh、run.log、registry-pristine.json、各例 s0/s4 日志）
- fmt-check.log
