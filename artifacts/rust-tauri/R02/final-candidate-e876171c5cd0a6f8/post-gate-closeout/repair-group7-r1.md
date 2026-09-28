# R02 最终收口修复 — 根因组 7（R1）：auth_matrix 两个失败测试

- 子代理：`R02-REPAIR-GROUP-7-R1`
- 仓库：`/Users/study_superior/Desktop/Code/LingxiAgent`，分支 `codex/rust-tauri-migration`，HEAD `cdd213078`
- 唯一改动文件：`rust/crates/lingxi-service/tests/auth_matrix.rs`（未改任何服务端 src；无 commit/push）
- 结论先行：**两个测试均判"改测试"**，服务端语义正确且与机器契约图钉一致。auth_matrix 23/23 全绿，`-p lingxi-service` 全量无回归，fmt/clippy 零告警。

## 0. 复现（修复前）

- 命令：`cargo test --manifest-path rust/Cargo.toml -p lingxi-service --test auth_matrix --offline --locked`
- 结果与 Gate 第 1 轮一致（`/tmp/r02-final/gate/03-cargo-test/stdout.log`）：21 过 2 败
  - `a05_unreadable_registry_fails_closed_and_recovers`：期望 401 + `auth_registry_unavailable`，实际 500 `{"code":"internal","reason":"device_registry_failure"}`
  - `device_registry_failure_is_not_reported_as_bad_credentials`：`POST /lingxi/v1/devices/credentials` body `{}` 期望 200，实际 400 missing field userId
- git 取证：两个测试均在 `cdd213078`（R17–R21 收口提交）中新增/改写，父提交 `cdd213078^` 的 auth_matrix.rs 中不存在（`git show cdd213078^:...auth_matrix.rs` 无 `a05_unreadable_registry` / `device_registry_failure_is_not` 匹配）→ 无历史绿基线，与"R20/R21 新增、从未真实运行"一致。

## 1. 取证

### a. 现行服务端行为与理由（R21 意图）

- `rust/crates/lingxi-service/src/auth.rs:1623-1660` `refresh_registries`：每次设备身份判定前重读两份注册簿；坏 JSON → `AuthSetupError::RegistryInvalid`（"not valid JSON"），文件缺失 → `RegistryInvalid`（"registry missing after service startup"）。
- `rust/crates/lingxi-service/src/auth.rs:1328-1334` `authenticate`：refresh 失败 → `AuthDenial::new("auth_registry_unavailable")`，**该路径不写任何文件**（字节保全、不重建注册簿）。
- `rust/crates/lingxi-service/src/lib.rs:1666-1677` `auth_guard`：把 `auth_registry_unavailable` 映射为 **500**，reason `device_registry_failure`，cause `auth.device_registry_failure`（WS 路径同语义：lib.rs:1587-1612、lib.rs:2196-2210）。
- `rust/crates/lingxi-service/src/management.rs:773-781` web 登录凭证路径：registry 故障 → 500 `device_registry_failure`；其他拒绝才是 403 `invalid_credential`。`management.rs:911-922` web session 复验同映射。
- R21 意图原文（auth.rs:2178-2180 注释）："注册簿读不到时明确报内部错误，不能伪装成凭证撤销" —— 即**registry 故障必须与 bad-credential 拒绝区分**，报 5xx。

### b. 机器门禁图钉（权威；管理矩阵 Gate R1 60 案例 PASS）

- `rust/crates/lingxi-service/tests/r00_management_leaves.rs:519-545`：扣留 devices.json 后 `GET /lingxi/v1/access/summary`、`GET /lingxi/v1/devices` 断言 **status 500**、非假空、状态不变，登记图钉案例 `management-summary-registry-failure-not-empty`、`management-device-list-registry-failure-not-empty`，record 值 `{"status": 500, ...}`。
- `rust/crates/lingxi-service/tests/r00_management_leaves.rs:870-893`：签发时封写入 → **500** 且 body 含 `device_registry_failure`、无 secret、注册簿不变（`management-mobile/desktop-credential-store-failure-no-secret`）。
- 图钉登记：`rust/crates/xtask/src/stage_maps/R02.json`（~:712、~:757，case 名 + `"expect": 1`），消费方 `scripts/rust-tauri/r02_management_leaf_matrix.py:38,63`。管理矩阵 Gate R1 PASS ⇒ 现行服务端"registry 故障=500+device_registry_failure+无副作用"与机器契约一致。
- 签发契约（针对测试 2）：`rust/crates/lingxi-service/src/lib.rs:2025-2075` `issue_device_credential` —— `userId` 必填（缺失 → 400），成功 → **201 CREATED** 返回 `secret`。同文件其他签发用法一律合法 body + 201（如 a05 两测试开头）。

### c. incumbent Node 行为（参考）

- `server/routes/devices.ts:29-30`：registry 读取抛错 → `c.json({ error: err.message }, 500)`。与 Rust 侧 500 语义方向一致（incumbent 无逐 reason 对照，不强求）。

## 2. 判定与修改（均在 tests/auth_matrix.rs）

### 测试 1 `a05_unreadable_registry_fails_closed_and_recovers` —— 改测试

判定依据：服务端 500 `device_registry_failure` 与权威图钉（b 节）逐点一致，incumbent 亦 500；测试的 401 + `auth_registry_unavailable` 是从未运行的臆写。fail-closed 不变量（拒绝、不重建、字节保全、修复后恢复）由服务端保证且全部保留。

- :771-772（坏注册簿 `{invalid json`）：401→**500**，`auth_registry_unavailable`→**`device_registry_failure`**；保留 `assert_eq!(std::fs::read(&path).unwrap(), b"{invalid json")` 字节保全断言；加注释引用图钉案例名。
- :789-790（删除注册簿）：同上 401→500、reason 对齐；保留 `!path.exists()`（不得重建）。
- :818-819（删除 devices.json）：同上。
- 恢复路径（写回 valid → /me 200）不变。

### 测试 2 `device_registry_failure_is_not_reported_as_bad_credentials` —— 改测试（body 笔误修正）

判定依据：`POST {}` → 400 missing userId 是服务端正确行为（lib.rs:2025-2075 userId 必填；同文件所有签发用合法 body 期望 201）。测试名与后续断言（GET /me、web 登录、web session 均 500）表明原意是"合法签发 → 破坏注册簿 → 鉴权操作得到 registry 故障类 5xx 而非 401/403 invalid_credential"。`{}`+200 是转写错误（200 vs 201、空 body vs 必填 userId 双重不符）。

- 签发 body `{}` → `{"userId":"user_registry_probe","scopes":["chat"]}`，断言 200 → **201**。
- 三个 500 断言补充 body 类别断言：`contains("device_registry_failure") && !contains("invalid_credential")`，把测试名语义钉死（与 b 节管理图钉的 body 断言一致）。

同根因扫描：全文件仅 :772/:785/:814 三处 `auth_registry_unavailable`（全在测试 1）与 :2194 的 200（测试 2）依赖旧语义，已全部修复；其余测试无 registry 故障码断言。

## 3. 验证记录（UTC；`CARGO_TARGET_DIR=/tmp/rust-target-r02-final`，`--offline --locked`）

| 时间 | 命令 | 结果 |
|---|---|---|
| 2026-09-28T15:36:00Z | `cargo test --manifest-path rust/Cargo.toml -p lingxi-service --test auth_matrix --offline --locked` | **23 passed; 0 failed**（含两个目标测试），exit 0 |
| 2026-09-28T15:36:17Z | `cargo test --manifest-path rust/Cargo.toml -p lingxi-service --offline --locked` | 15 个测试目标全部 `test result: ok`（lib 182 含 auth/management 单测；auth_matrix 23；r00_management_leaves 1（60 案例矩阵）；r00_static_web_leaves 1；等），exit 0 |
| 2026-09-28T15:39:41Z | `cargo fmt --all -- --check` | exit 0，无 diff |
| 2026-09-28T15:39:54Z | `cargo clippy --manifest-path rust/Cargo.toml -p lingxi-service --all-targets --locked --offline -- -D warnings` | exit 0，零告警 |

- 未改服务端 src，故无需按指令第 4 条单独补跑管理矩阵；全量运行已包含 `r00_management_leaves`（passed，60 案例契约未触碰）。
- 未执行：workspace 级全量（组内分工只要求 lingxi-service 范围）；无网络受限操作。

## 4. 风险与边界

- 只改测试断言与测试输入，未触碰任何产品行为、scripts/、xtask、docs；无提交。
- fail-closed 语义完整保留：拒绝（500 也是拒绝且非伪造成功）、不重建注册簿、坏字节原样保全、修复后恢复访问、registry 故障不冒充 invalid_credential。
