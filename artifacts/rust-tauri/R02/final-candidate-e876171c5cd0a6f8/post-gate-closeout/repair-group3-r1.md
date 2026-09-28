# R02 最终收口修复 — Group 3 (R02-REPAIR-GROUP-3-R1)

- 仓库：`/Users/study_superior/Desktop/Code/LingxiAgent`，分支 `codex/rust-tauri-migration`，HEAD `cdd213078f6947217000c7ecd1a36ab5ffe2bb01`
- 唯一改动文件：`scripts/rust-tauri/r02_t03_auth_matrix.sh`（+11/−4 行）
- 验证日期：2026-09-28（全部 UTC 时间戳）
- 解释器事实：脚本由 `/bin/bash` 执行 = GNU bash 3.2.57(1)-release (arm64-apple-darwin26)

## 缺陷 1：record_case 缺第 4 参（ok）

- 复现：`record_case` 以 `"$1" "$2" "$3" "$4"` 调 python3，`set -u` 下少传 `$4` 即 `$4: unbound variable` 崩溃。最小复现脚本（/tmp/g3-trap-test.sh）在崩溃后以 **exit 0** 结束（同时暴露缺陷 2），与最终 Gate 第 1 轮证据一致（`artifacts/rust-tauri/R02/final-candidate-a0af83666f89cc4b/a05_a06_auth_matrix/` 只有 stdout.log/stderr.log，三个声明证据未产出）。
- 全脚本扫描 `record_case` 调用点：共 8 处（189/191/230/317 行均 4 参，正常），仅 496/502/506 三处 3 参 —— 同根因家族仅此三处，无其他。
- 修复（三处均在其 `[ ... ] || fail` 前置守卫之后到达，ok=1）：
  - 496 行：`record_case "a05-sessions-list-owner-contains-own-session" 1 1` → `... 1 1 1`
  - 502 行：`record_case "a05-sessions-list-foreign-excludes-owner-sessions" 0 "$FOREIGN_SESS_HITS"` → `... 0 "$FOREIGN_SESS_HITS" 1`
  - 506 行：`record_case "a05-sessions-list-empty-shape" 1 "$EMPTY_SHAPE_HITS"` → `... 1 "$EMPTY_SHAPE_HITS" 1`
  - expect 值与阶段图图钉一致（owner-contains=1 / foreign-excludes=0 / empty-shape=1）。

## 缺陷 2：EXIT trap 吞退出码（fail-open）

根因测定（bash 3.2.57 实测，`/tmp/g3-trap-test.sh` 与 5 个变体）：

| 失败类别 | 旧 `trap cleanup EXIT` | `trap 'rc=$?; cleanup; exit $rc' EXIT` |
|---|---|---|
| set -e 失败（`false`） | exit 1（保留） | exit 1（保留） |
| 显式 `exit 7` / `fail()` | exit 7 / 1（保留） | exit 7 / 1（保留） |
| **set -u 崩溃（unbound $4）** | **exit 0（fail-open）** | **仍 exit 0**（进入 trap 时 `$?` 已是 0） |

即：本机 bash 3.2.57 上 nounset 崩溃进入 EXIT trap 时 `$?` 已丢失为 0，任务书建议的纯 `rc=$?; cleanup; exit $rc` 形态对本次真实故障类别无效，需等效强化。

修复（不动 cleanup 本体，正常路径清理语义与停止断言不变）：

- 58-59 行区新增 `SCRIPT_COMPLETED=0`（带注释说明）。
- `trap cleanup EXIT` → `trap 'rc=$?; cleanup; if [ "$SCRIPT_COMPLETED" -ne 1 ] && [ "$rc" -eq 0 ]; then rc=1; fi; exit "$rc"' EXIT`
- 末行 `note "== ALL CASES PASSED =="` 之后新增 `SCRIPT_COMPLETED=1`（脚本最后一行）。
- cleanup 残留分支的 `exit 1` 在 trap 内直接生效（实测 variant：原 rc=3 + 残留 → exit 1），不被 `exit "$rc"` 覆盖。

## 验证记录

### 1. 语法
`bash -n scripts/rust-tauri/r02_t03_auth_matrix.sh` → exit 0（2026-09-28T15:35Z 前后）。

### 2. 负向验证（trap 修复，临时副本注入失败，全部在 /tmp，不触仓库）
副本生成：修复后脚本复制到 `/tmp/r02-final/g3-neg/`，仅把 `cd "$(dirname "$0")/../.."` 改为绝对仓库路径，在 trap 行后注入失败：

| 变体 | 注入 | 观测（2026-09-28T15:36:49Z） |
|---|---|---|
| n1_nounset_crash.sh | trap 后调用只传 3 参的函数触发 `$4: unbound variable`（与真实故障同类） | stderr 打印崩溃，**exit=1**（修复前同类复现 exit=0） |
| n2_exit3.sh | trap 后 `exit 3` | **exit=3**（真实码保留） |
| n3_fail.sh | `fail "injected negative N3"` | FAIL 打印，**exit=1** |

原始输出：`/tmp/r02-final/g3-neg-results.txt`；证据目录 `/tmp/r02-final/g3-neg-evidence/{n1_nounset_crash,n2_exit3,n3_fail}`。

### 3. 完整真实运行（真构建 + 真服务 + 真 TCP/WS）
- 命令（阶段图 `a05_a06_auth_matrix` argv 形态，`{EVIDENCE}` → /tmp）：
  ```
  export PATH="$HOME/.cargo/bin:$PATH"; export CARGO_TARGET_DIR=/tmp/rust-target-r02-final
  bash /Users/study_superior/Desktop/Code/LingxiAgent/scripts/rust-tauri/r02_t03_auth_matrix.sh /tmp/r02-final/g3-verify/A05_A06
  ```
- START 2026-09-28T15:37:05Z → END 2026-09-28T15:38:13Z（约 68 s），**FULL_RUN_EXIT=0**，末行 `== ALL CASES PASSED ==`。
- build.log：`Finished dev profile ... in 0.08s`（预热 target dir 生效）；服务真实启动 `addr=127.0.0.1:56233`，合成 home。
- summary.txt：**PASS 行 116，FAIL 行 0**。此前崩溃的三例现在显式 PASS：
  `a05-sessions-list-owner-contains-own-session` / `a05-sessions-list-foreign-excludes-owner-sessions` / `a05-sessions-list-empty-shape`。
- 收尾卫生：服务按 TERM 预算内退出且退出码 0；无 home 残留（`lingxi-r02t03-home-*` 已清理）；无遗留服务进程。

### 4. 三个声明证据文件（真实产出、合法 JSON、案例与图钉一致）
均在 `/tmp/r02-final/g3-verify/A05_A06/`：

| 文件 | 校验 |
|---|---|
| `leaf-cases.json`（9712 B） | 合法 JSON，schema `lingxi.leaf-case-results.v1`，**93 cases / 0 failing**；阶段图钉定的 19 个专属案例（producerCommand=a05_a06_auth_matrix）逐一核对：expect/actual 与图钉一致且 ok=true，无缺失无错配 |
| `sessions-list-matrix.json`（1512 B） | 合法 JSON，leafId `R00-T02-LA-200D4E5D52C9`，**10 cases**（6 个服务端 sessions-list + 4 个 CLI），全部 ok=true；含本轮修复的三例（1/0/1 与图钉一致） |
| `ws-matrix.json`（6881 B） | 合法 JSON，all_ok=true，**44 results / 0 failing**（含真实 30 s ticket-TTL 过期等待） |

抽查命令：`python3 -c "import json;d=json.load(open(...));print(len(d))"`（93 / 10 / 44）。

### 5. 仓库卫生
`git status`：本组唯一改动 `scripts/rust-tauri/r02_t03_auth_matrix.sh`；其余修改文件（`r02_t08_full_chain_probe.py`、`r02_t08_full_chain_smoke.sh`、`r02_t08_legacy_entry_regression.sh`、`API_COMPAT_MATRIX.json`、`rust/crates/lingxi-service/**`、xtask 等）与未跟踪 `artifacts/.../final-candidate-*`、`.g5-verify-tmp/` 均属并行代理，未触碰。本组全部证据只写入 /tmp，未写仓库内 artifacts。未 commit/push；未删任何断言；验收语义未变。

## 结论
根因组 3 两个缺陷修复完成：record_case 三处补第 4 参（同家族无其他短调用点）；EXIT trap 改为「记录 rc → 原样 cleanup → 未完成则强制非零 → 显式 exit rc」，在本机 bash 3.2.57 上对 set -u 崩溃、显式 exit、fail() 三类失败路径均不再吞码。完整真实运行 ALL PASS（116 PASS / 0 FAIL），三个声明证据文件真实产出且与阶段图图钉一致。
