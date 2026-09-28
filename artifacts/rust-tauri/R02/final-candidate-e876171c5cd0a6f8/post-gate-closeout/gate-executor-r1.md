# R02 最终动态门禁执行报告 — gate-executor-r1

- 执行者：`R02-FINAL-GATE-EXECUTOR-R1`
- 仓库：`/Users/study_superior/Desktop/Code/LingxiAgent`，分支 `codex/rust-tauri-migration`
- 候选：HEAD `cdd213078f6947217000c7ecd1a36ab5ffe2bb01`（未提交，属预期），R02_CANDIDATE_ID `a0af83666f89cc4b`
- 命令日志根：`/tmp/r02-final/gate/`（每命令一个子目录：command.txt / start-utc.txt / end-utc.txt / exit-code.txt / stdout.log / stderr.log）
- 工具链：cargo/rustc 1.98.1；`CARGO_TARGET_DIR=/tmp/rust-target-r02-final`（预热）；全部 `--offline --locked`

## 最终结论（醒目）

**R02 FINAL GATE FAIL: 第一处失败 = 步骤 3 `cargo test --workspace`——`crates/lingxi-service/tests/auth_matrix.rs` 2 个集成测试失败（`a05_unreadable_registry_fails_closed_and_recovers` 期望 401 实得 500；`device_registry_failure_is_not_reported_as_bad_credentials` 期望 200 实得 400 missing `userId`）。后续另有 3 处独立失败（步骤 4 契约漂移、verify-stage overall FAIL、A16 工作树镜像绑定失败）。**

## 候选冻结核对（预检）

- `git rev-parse HEAD` = `cdd213078f6947217000c7ecd1a36ab5ffe2bb01` ✓（符合冻结）
- 分支 = `codex/rust-tauri-migration` ✓
- `git status --short` 恰好 5 个修改文件：`rust/crates/xtask/src/stage_map.rs`、`rust/crates/xtask/src/stage_maps/R02.json`、`rust/crates/xtask/src/verify.rs`、`rust/crates/xtask/src/verify/runner_tests.rs`、`scripts/rust-tauri/r02_t08_legacy_entry_regression.sh` ✓
- 证据根 `artifacts/rust-tauri/R02/final-candidate-a0af83666f89cc4b/` 开工前不存在 ✓（未预创建）

## 命令执行记录

| # | 命令 | UTC 起止 | exit | 结果 |
|---|------|----------|------|------|
| 1 | `cargo fmt --manifest-path rust/Cargo.toml --all -- --check` | 15:08:45Z → 15:08:45Z | 0 | PASS（无输出，无格式差异） |
| 2 | `cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked --offline -- -D warnings` | 15:08:55Z → 15:08:59Z | 0 | PASS（`Finished dev profile in 4.25s`） |
| 3 | `cargo test --manifest-path rust/Cargo.toml --workspace --locked --offline` | 15:09:07Z → 15:10:54Z | 未直接捕获（nohup 启动）；cargo 输出 `error: test failed`（cargo 测试失败 = 101） | **FAIL（第一处失败）** |
| 4 | `cargo run --manifest-path rust/Cargo.toml -p xtask --quiet -- check-contracts` | 15:11:04Z → 15:11:09Z | 1 | **FAIL**（API_COMPAT_MATRIX.json 漂移） |
| 5 | `cargo run --manifest-path rust/Cargo.toml -p xtask --quiet -- check-boundaries` | 15:11:27Z → 15:11:27Z | 0 | PASS（`RESULT: OK (ownership contract + dependency rules + negative battery)`） |
| 6 | `env -u http_proxy -u https_proxy -u HTTP_PROXY -u HTTPS_PROXY -u all_proxy -u ALL_PROXY cargo run --manifest-path rust/Cargo.toml -p xtask --quiet -- verify-stage R02 --evidence /Users/study_superior/Desktop/Code/LingxiAgent/artifacts/rust-tauri/R02/final-candidate-a0af83666f89cc4b` | 15:11:39Z → 15:22:21Z（约 10 分 42 秒，远快于 1–2h 预估：预热 target 生效；期间每 ~55s 轮询记录 progress.log） | 1 | **FAIL**（`overall: "FAIL"`） |

### 步骤 3 失败详情（第一处失败）

`lingxi-service --test auth_matrix`：21 passed, 2 failed（总时长 8.62s；workspace 其余 19 个测试目标全部 `test result: ok`）：

1. `device_registry_failure_is_not_reported_as_bad_credentials`（`auth_matrix.rs:2189`）
   `assertion left == right failed`：left(实得)=400，right(期望)=200；响应体 `{"code":"invalid_message",...,"message":"Failed to deserialize the JSON body into the target type: missing field `userId` at line 1 column 2"}` —— 测试发的请求体缺 `userId` 而服务端现在要求它（或测试请求构造与服务端契约不同步）。
2. `a05_unreadable_registry_fails_closed_and_recovers`（`auth_matrix.rs:771`）
   `invalid registry must deny`：left(实得)=500，right(期望)=401；响应体 `{"code":"internal","reason":"device_registry_failure","message":"credential registry unavailable"}` —— 注册表不可读时服务端按 500 internal 分类，测试期望 fail-closed 401。

注：`crates/lingxi-service/tests/auth_matrix.rs` 不在 5 个修改文件之列（HEAD 版本），失败为候选本体失败。

### 步骤 4 失败详情

`check-contracts` 第 2 项 `API_COMPAT_MATRIX.json (extract --check)` 报 `DRIFT`（第 1 项 lingxi-protocol-gen 56 个生成文件 OK）：

- `contentSha`：disk `2f55e3dd…e3a` ≠ gen `f0e44e6e…9a`
- `desktop/preload.cjs`：disk sha `19fb7dd9…` ≠ gen `a39d002e…`
- `desktop/src/react/types.ts`：disk `7c290e67…` ≠ gen `27262feb…`
- 条目差异（line 5317–5344 区域）：gen 侧含 `preload:getServerConnectionInfo`（disk 侧该位置没有），disk/gen 的 `runEditCommand`/`getAppVersion`/`speechPermissionStatus`/`speechPermissionRequest` 顺序与集合错位。
- 提示重生成命令：`node scripts/rust-tauri/r01-t02-extract-api-surface.mjs`。两份磁盘文件（preload.cjs / types.ts）均不在 5 个修改文件之列——即 HEAD 上已提交的契约矩阵与已提交的 preload/types 源不一致。

## verify-stage 结果 JSON 逐字段核验

文件：`artifacts/rust-tauri/R02/final-candidate-a0af83666f89cc4b/verify-stage-result.json`（170,225 字节）

| 字段 | 结果 JSON 原值 | 门禁期望 | 判定 |
|------|----------------|----------|------|
| `overall` | `"FAIL"` | `"PASS"` | **不符** |
| `candidateSourceBinding.stable` | `true` | true | ✓ |
| `candidateSourceBinding.before.digestSha256` | `37d4f2ce692cf206feed4ca5d5c86efdb79d49f8201134002b0ec10f3f3f3309`（fileCount 10576） | 事前 digest 同值 | ✓ |
| `candidateSourceBinding.after.digestSha256` | 同上（fileCount 10576） | 同值 | ✓ |
| `testedShaAtEnd` | `cdd213078f6947217000c7ecd1a36ab5ffe2bb01` | 同值 | ✓ |
| 20 个 per-command checkpoint | 全部 `stable: true`、digest 同值、`changedPathBytesHex: []` | — | ✓（源未被门禁污染） |
| `runnerSourceBinding.status` | `"PASS"`（before/after compiledSourceFiles 哈希一致） | PASS | ✓ |
| 16 个 scenarios（A01–A16） | 12 PASS；**FAIL：R02-A05、R02-A06、R02-A15、R02-A16** | 全 PASS | **不符** |
| `supplementalLeafCoverage.declaredInStageMap` / `expectedFromR00Ledger` | 34 / 34 | expected 34 | ✓ |
| `supplementalLeafCoverage.pass` | **17** | 25 | **不符** |
| `supplementalLeafCoverage.deferredToR07` | 9（`deferredLeafIds` 列出 9 个） | 9 | ✓ |
| `supplementalLeafCoverage.fail` | **8** | 0 | **不符** |
| `supplementalLeafCoverage.blocked` | 0 | 0 | ✓ |
| `shareSatisfiedLeafIds` | 25 个（= 17 PASS + 8 FAIL 的 r02_share_satisfied 叶） | — | 记录 |
| 每命令 `preExistingEvidence` | 全部空（0） | 空 | ✓ |
| 每命令 `missingEvidence` | 仅 4 个失败命令非空（见下）；其余空 | 空 | **不符（失败命令的连锁缺失）** |
| 每命令 `timedOut` | 全部 `false` | 无超时 | ✓ |
| 9 个 deferred 叶列出且含 `r07Share` 文本 | ✓（见下表） | ✓ | ✓ |

### 8 个 FAIL 叶清单（全部 `basisKind: r02_share_satisfied`，reason 均为“证据命令未通过”链）

| leafId | featureId | 依赖的失败命令 |
|--------|-----------|----------------|
| `R00-T02-LA-D3710D637C19` | F-D20-CLI-CLI-CHAT-D3710D | a05_a06_auth_matrix, a15_full_chain |
| `R00-T02-LA-3B86332B042C` | F-D20-CLI-CLI-CONTINUE-3B8633 | a05_a06_auth_matrix |
| `R00-T02-LA-200D4E5D52C9` | F-D20-CLI-CLI-SESSIONS-200D4E | a05_a06_auth_matrix（+ supplemental_cli_sessions_matrix） |
| `R00-T02-LA-EEC1A2A5CD04` | F-D20-CLI-CLI-STATUS-EEC1A2 | a05_a06_auth_matrix |
| `R00-T02-LA-D1BEE19A95BB` | F-D20-DESKTOP_BEHAVIOR-…-SERVICE-CONNECTION-D1BEE1 | a05_a06_auth_matrix |
| `R00-T02-LA-5816DA563ED8` | F-D20-SEMANTIC_EFFECT-…-WS-TICKET-WEBSOCKET-5816DA | a05_a06_auth_matrix |
| `R00-T02-LA-4BCE8CCFD5DC` | F-D20-SEMANTIC_EFFECT-…-SERVER-IDENTITY--4BCE8C | a05_a06_auth_matrix |
| `R00-T02-LA-D2657E4AB5FF` | F-D20-UI_BEHAVIOR-…-SERVICE-CONNECTIVITY-D2657E | a05_a06_auth_matrix |

reason 原文（代表）：`no legal evidence for the R02 share of this leaf: evidence commands not passing (or not executed): ["a05_a06_auth_matrix"]; original-assertion cases not holding: evidence producer command "a05_a06_auth_matrix" did not pass in this run — the leaf's case evidence cannot be trusted …`

### 4 个失败命令及 missingEvidence

- `a05_a06_auth_matrix`（exitCode 0 但证据缺失）：missing `{EVIDENCE}/A05_A06/ws-matrix.json`、`{EVIDENCE}/A05_A06/leaf-cases.json`、`{EVIDENCE}/A05_A06/sessions-list-matrix.json`
- `a15_full_chain`（exit 1）：missingEvidence 空（脚本自身断言失败）
- `a16_legacy_regression`（exit 1）：missingEvidence 空（脚本自身断言失败）
- `supplemental_cli_sessions_matrix`（exit 1）：missing 6 项（`cli-sessions-cases.json`、`auth/sessions-list-matrix.json`、3 个 stdout/stderr log、`auth/cli-sessions-limit-case.json`）

### 9 个 deferred 叶（deferredLeafIds，均有 r07Share 文本）

`000E6E1301C0`（UI-SETTINGS-ACCESS→R07/R08 UI 动作矩阵）、`2A1C298F62FC`（MOBILE-BOOTSTRA→R07-T09）、`3291CFD5F7E2`（STATIC-MOBILE→R07-T09）、`32FFEC05BAA7`（UI-SETTINGS-SHARING→R07）、`39AD35E1FD71`（THINKING-LEVEL-DEFAULT→R07-T09）、`8BC1A036AFAA`（STATIC-DESKTOP→R07-T09）、`8ED658F9DB9E`（SESSION-THINKING→R07-T09）、`B8A1AD32A8E1`（CLI-HELP→R07-T09）、`F8935B6B0221`（THINKING-LEVEL-SET→R07-T09）。

### verify-stage 内 4 个失败场景的根因线索（从证据目录取证）

1. **A05/A06 + CLI_SESSIONS 级联（8 叶 FAIL 的直接根因）**：生产脚本 `scripts/rust-tauri/r02_t03_auth_matrix.sh` 在 `PASS a05-sessions-list-owner (http=200)` 后崩溃，stderr：`scripts/rust-tauri/r02_t03_auth_matrix.sh: line 180: $4: unbound variable`。精确指针：`record_case()`（定义于 179–184 行，`$4=ok(1/0)`）的部分调用点只传 3 参——如 **496 行 `record_case "a05-sessions-list-owner-contains-own-session" 1 1`**（502、506 行同样 3 参），`set -u` 下 `$4` unbound 致死，脚本后半段（ws-matrix / sessions-list-matrix / leaf-cases.json 组装）未执行。A05_A06 目录仅有 `leaf-cases.ndjson`（流式半成品）；`CLI_SESSIONS` 包装器因此报 `R02 CLI sessions leaf: FAIL (0 cases)`。该脚本不在 5 个修改文件内（HEAD 版本）。
2. **A15**：`r02_t08_full_chain_smoke.sh` boot1 全过（health/me/execute committed），随后 WS 订阅升级被服务端拒绝：`probe1.err` = `FAIL upgrade failed: 400 … causeId":"transport.invalid_ws_upgrade"`，服务日志 `LINGXI_TRANSPORT_REJECTED … /lingxi/v1/ws status=400 reason=missing or invalid Sec-WebSocket-Key`，随后 SIGTERM 干净关停。summary 停在 `PASS boot1-ready`。
3. **A16**：`r02_t08_legacy_entry_regression.sh`（本次修改文件之一）E0 通过（base 201584f2 是候选祖先），随后在隔离副本校验处死亡：`FAIL: candidate copy does not mirror the invoking worktree (tracked+untracked content binding differs)`。E0 绑定 `e0-candidate-binding.tsv` 含 **207 个未跟踪路径，全部位于正在写入的证据根** `artifacts/rust-tauri/R02/final-candidate-a0af83666f89cc4b/` 下（该目录未被 .gitignore，对 git 可见为 `??`）——门禁运行期间证据持续写入，使“调用工作树 vs 隔离副本”的绑定比对天然不稳定（此为线索，非定论）。E1–E5 未执行，`A16/legacy-entry/` 仅有 4 个 e0/summary 文件。

## A15 / A16 抽读结论（按任务要求）

- **A15**（`A15/full-chain/summary.txt`）：**不含**要求的 boot1/关闭/boot2/旧 token 拒绝/新 token/事件保全/无遗留进程端口/ALL GREEN 语义——summary 仅到 `PASS boot1-ready` 即中断，后续阶段全部缺失。
- **A16**（`A16/legacy-entry/`）：**无** e1-default-entry-assertions、e5 相关文件（目录仅 e0-candidate-binding.tsv / e0-candidate-dirty.diff / e0-candidate-dirty.txt / summary.txt）；默认入口断言未执行到；npm 红分类未产生。

## 收尾检查

- `pgrep -fl "lingxi-service|rust-target"`：无匹配（无遗留进程）✓
- `git rev-parse HEAD` = `cdd213078f6947217000c7ecd1a36ab5ffe2bb01`（未变）✓
- `git status --short`：恰好 5 个修改文件 + `?? artifacts/rust-tauri/R02/final-candidate-a0af83666f89cc4b/`（新增证据根，未跟踪）✓
- 证据根清单（`find -type f | sort` 摘要）：68 个目录（含根）、**423 个文件、共 10,975,371 字节**；最大组：CLI_RUST 51、A13 48、A12 40、A01 40、A07_A08 29、A11 26、A04 26、CLI_SESSIONS 19、CLIENT 17、A05_A06 16、A03 15；另有 candidate-source-before/after.json 与 verify-stage-result.json。

## 汇总判定

- 通过：步骤 1（fmt）、步骤 2（clippy）、步骤 5（boundaries）；候选源绑定/运行器绑定/digest/SHA 全部符合冻结口径；无 timedOut；preExistingEvidence 全空；deferred 9 与预期一致。
- 失败（按发生顺序）：
  1. 步骤 3 cargo test：auth_matrix 集成测试 2 败（HEAD 版本测试）。
  2. 步骤 4 check-contracts：API_COMPAT_MATRIX.json 漂移（preload.cjs / types.ts 磁盘哈希与 surface ≠ 再生成）。
  3. 步骤 6 verify-stage：overall FAIL——A05/A06/A15/A16 四场景失败；supplemental 17 pass / 9 deferred / 8 fail / 0 blocked（期望 25/9/0/0）。
- 未重跑、未修复、未提交：首败原样保留。

**R02 FINAL GATE FAIL: 第一失败 = 步骤 3 cargo test（auth_matrix 2 测试：registry 不可读时期望 401 实得 500；device registry 失败用例期望 200 实得 400 missing userId）；根因线索 = r02_t03_auth_matrix.sh record_case 3 参调用致 $4 unbound 崩溃（级联 8 叶 FAIL）、A15 WS 升级 400 invalid_ws_upgrade、A16 证据根被计入 tracked+untracked 绑定致镜像校验失败、API_COMPAT_MATRIX 漂移。**
