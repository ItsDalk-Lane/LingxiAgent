# R02 最终收口 — 证据绑定审计 R1（R02-EVIDENCE-AUDITOR-R1）

- 审计对象：`/Users/study_superior/Desktop/Code/LingxiAgent`，分支 `codex/rust-tauri-migration`
- 本轮 `R02_FINAL_REPAIR_BASE_SHA` = HEAD `cdd213078f6947217000c7ecd1a36ab5ffe2bb01`（单提交 `feat(rust-tauri): R02 修复轮 R17–R21 收口`，2026-09-28 21:50 +0800，直接叠在 `9b98c679678888b60ff2e32bd67d3da6fc2f5c4f` 上），工作区干净（`git status --porcelain` 0 路径）。
- 本审计只读取证，未修改任何仓库文件、未执行任何动态门禁（无 cargo/npm/xtask 实跑）。
- 结论速览：**全部现行 JSON 导航字段与 artifacts 证据均绑定到提交前旧树，没有任何一份已提交证据是在 cdd213078 树上产生的**；且当前 HEAD 的 stage map 存在一个会让 verify-stage R02 必然 exit 2 的注册缺口（§1.5），最终修复提交落地后须以全新证据根重跑全部证据（§4）。

---

## 1. 陈旧绑定清单（文件 / 字段 / 当前值 / 应绑定值）

### 1.1 docs/rust-tauri/R02/R02_HANDOFF.json

| 字段 | 当前值 | 审计判定 | 应绑定值 |
|---|---|---|---|
| `source_sha` | `9b98c679678888b60ff2e32bd67d3da6fc2f5c4f` | 陈旧（R2–R14 轮均以"同一 HEAD 9b98c679 + 未提交工作树"为候选；该工作树已在 cdd213078 全部提交） | 最终修复提交的 HEAD SHA |
| `working_tree_digest` | `79b44094b01456d5708ccfe49032b47f19591995c881bc124da5559bdc9df8b5` | 陈旧且自我声明为"R8 终值口径，不能用于当前候选"（`working_tree_digest_scope` 明示 R9–R14 未重算） | 新候选的 candidate source digest（见 §2；或在干净提交树上等价于 HEAD 树 digest） |
| `current_candidate_binding.head` | `9b98c679…` | 陈旧 | 最终修复提交 HEAD |
| `current_candidate_binding` 指向的清单 | `artifacts/rust-tauri/R02/audit-r16/prechange-*-corrected.txt`（R16 开工前 58 路径/9716 文件快照） | 陈旧：R16 之后候选又经历 R17–R21 大量改动（提交 stat：983 文件、+145994/-1838） | 新一轮 prechange/终态全量 SHA-256 清单（写入新证据根或新审计目录，不覆盖 audit-r16） |
| `status` / 各 `stage_review_r1..r14`、`repair_r1..r14_candidate` | 滚动历史记录，止于 repair_r14 | 历史条目按其"历史滚动记录"定位保留，不算错误；但缺 R15 评审、R16–R21 修复轮条目（R15 只在 ORCH 有） | 最终交接刷新时补 R15–R21 链与最终候选条目；历史字节不回改 |
| `dependency_locks` | `rust/Cargo.lock b95eb840b48c07e3`、`package-lock.json e54a16fe14f15b47`、`rust-toolchain.toml eec3410410647a7d` | Cargo.lock 前缀陈旧（cdd213078 树实值 `90111c4b5988531d…`，提交含 326 行 Cargo.lock 变更）；package-lock 与 toolchain 恰仍一致（实值 `e54a16fe14f15b47…`（64 位截断形式恰同）/`eec3410410647a7d…`） | 以 cdd213078（或最终提交）实算为准重登记 |

### 1.2 docs/rust-tauri/R02/R02_ACCEPTANCE_LEDGER.json

| 字段 | 当前值 | 审计判定 | 应绑定值 |
|---|---|---|---|
| `tested_sha` | `5741989165fe7e04c9a58a9d35c7747d3599d274` | 最陈旧（T08 执行时点，早于 9b98c679 两个提交）；`tested_sha_note` 自认"+本任务未提交工作树" | 最终候选的 testedSha（verify-stage 结果 JSON 的 `testedSha`） |
| 16 个 `scenarios[].tested_sha` | 全部 `5741989165fe7e04c9a58a9d35c7747d3599d274 + 未提交 T08 改动` | 同上，执行者旧候选历史 | 新一轮逐场景 tested_sha |
| `scenarios[].evidence` / `evidence_root` | 全部指向 `artifacts/rust-tauri/R02/T08/verify-stage/…` | 旧树证据（T08 时点），`current_candidate_status.historical_results_note` 已声明"不能代替当前工作区验证结果" | 指向新证据根 `artifacts/rust-tauri/R02/final-candidate-<candidate-id>/` 下同路径文件（§4） |
| `executor_run.result_json` | `artifacts/rust-tauri/R02/T08/verify-stage/verify-stage-result.json`（旧 resultVersion v1、无 candidateSourceBinding） | 旧树证据 | 新 verify-stage-result.json |
| `stage_review_and_repair`（30 键） | 止于 `stage_review_r15` + `root_direct_static_fix_after_r15` | R16–R21 轮无条目 | 最终交接补记 |

### 1.3 docs/rust-tauri/ORCHESTRATOR_PROGRESS.json（R02 条目）

| 字段 | 当前值 | 审计判定 | 应绑定值 |
|---|---|---|---|
| 顶层 `current_head` | `9b98c679678888b60ff2e32bd67d3da6fc2f5c4f` | 陈旧（真实 HEAD 为 cdd213078）；`current_head_note` 还写"工作区仍有未提交候选"——提交后失实 | cdd213078 或最终修复提交 |
| `stages.R02.status` | `R18_FAIL__A16_CONTRACT_CONFLICT__DYNAMIC_BLOCKED` | R18 时点判定，未反映 R19–R21 与提交 | 按最终 Gate 实测刷新 |
| `stages.R02.current_controller_position` | "2026-09-28 R18：…当前候选真运行 BLOCKED…" | R18 时点 | 最终收口刷新 |
| `stages.R02.stage_review_round` / `stage_verdict` | `15` / `FAIL` | R15 评审时点 | 最终轮次 |
| `stage_review_r15` + `root_direct_fix_after_r15` | 静态修正未验证（`STATIC_CHANGE_ONLY_UNVERIFIED`） | R15 后根代理直接修复的登记；其后的 R16–R21 工作无条目 | 补记 |
| `blockers`（6 条 R02-R18-*） | R18 时点问题表 | 部分（如 R02-R18-WINDOWS-CONSUMERS）在 R19–R21 中继续处理，状态未更新 | 最终收口按实测重评 |

### 1.4 artifacts/rust-tauri/R02/ 下 r17–r21 证据目录与 HEAD 树的关系（实测）

对 HEAD 树逐文件重算 SHA-256 并与各证据登记清单对比（均不一致 → 证据产于提交前旧树）：

| 证据目录 | 自称绑定 | 与 cdd213078 树实测 |
|---|---|---|
| `audit-r18/candidate-final-full-sha256.txt`（10496 文件，12:10 UTC，清单 SHA `6967b614…`） | "当前 10496 文件完整摘要"（R02_FULL_SCOPE_HANDOFF_R18.md §文件清单） | **51 文件内容不同 + 99 个新文件缺失**（提交在 13:50 UTC；final-noport-2 批次、a16-e1-static、docs 终稿均在快照之后）→ 该清单不代表最终树 |
| `audit-r18/final-noport-2/source-before/after-sha256.txt`（2803 文件，12:39–12:42 UTC，批内 before==after 成立） | 无端口批次源码绑定 | **35 文件与 HEAD 树不同**（cli/desktop/xtask/main.rs/windows_acl/Cargo.lock 等在批次后又改） |
| `management-r21/{start,end}-source-sha256.txt`（6 文件，09:49 UTC） | "start 与 end 完全一致" | 批内一致成立，但 6 个哈希**全部与 HEAD 树不同**（auth.rs `8a281cbc…` vs 树 `5d9408af…` 等） |
| `management-r20`（first/final 两批日志） | 前轮 | 同为旧树（早于 r21） |
| `a15-root-r17/full-chain/summary.txt`（2026-09-28T14:17+0800 = 06:17 UTC，ALL GREEN 真端口运行） | 无 HEAD 绑定字段（只有运行时戳与产物） | 06:17 UTC 早于最终多轮修改 → 旧树产物；其副本目录 `R02/full-chain/`（提交内重复路径）同源 |
| `audit-r18/a16-e1-static-20260928.json`（13:17 UTC） | `"head": "9b98c679…"` | 字段即陈旧（head 仍写 9b98c679）；其 E1 冲突面 53 文件与当前 HEAD 一致方向的判定仍有效 |
| `r17-leaf-gate`、`audit-r17-*`、`desktop-*-r17`、`tls-r17`、`windows-acl-r18`、`r18-*` 等 | 各自 run 目录 | 均为提交前工作树产物（时点全部 < 13:50 UTC） |

**结论：artifacts 下没有任何一份已提交证据绑定到 cdd213078 的树字节。** 这些目录属已提交历史：本轮不删除、不覆盖、不改写字节；最终 Gate 证据全部写入新目录（§3）。

### 1.5 附带发现（阻断级，须最终修复提交处理）

**当前 HEAD 的 stage map 注册缺口：verify-stage R02 现在必然 exit 2。**
`rust/crates/xtask/src/stage_maps/R02.json` 的补充叶 `R00-T02-LA-1B09760C2B1C`（前台服务进程叶）`evidenceCommandRefs` 引用 `supplemental_cli_rust_matrix`，但 `commands`（19 个 key）中没有该命令；`rust/crates/xtask/src/stage_map.rs:466-477` 对叶引用未知命令硬失败（`supplemental leaf … references unknown command`），`parse_stage_map` Err → `cmd_verify_stage` exit 2，任何 verify-stage R02 运行在命令执行前即中止。脚本 `scripts/rust-tauri/r02_cli_rust_matrix.py` 已存在（提交内 +546 行；r17 取证中的调用形态为 `python3 r02_cli_rust_matrix.py <evidence-dir>`，产物 `cli-rust-cases.json`，叶契约路径 `{EVIDENCE}/CLI_RUST/cli-rust-cases.json`）。最终修复提交需补注册该命令 spec（argv 仿 `['python3','scripts/rust-tauri/r02_cli_rust_matrix.py','{EVIDENCE}/CLI_RUST']` + evidencePaths），随后 R02_CANDIDATE_ID 以修复后 HEAD 重新计算。除此之外 commands/references 交叉核对：19 命令全部被引用、其余引用全部可解析。

### 1.6 /tmp 旧报告现状（§4 前置确认）

- `/tmp/r02-stage-review-r1..r15.md`（15 份）与 `/tmp/r02-stage-repair-r1..r14.md`（14 份）**仍存在**（本轮实测，非已丢）；无 `*-r15` 修复报告、无 `r16..r21` 的 review/repair 报告文件。R16–R21 只有 artifacts 证据目录与提交信息，无独立 /tmp 报告。
- 处置：这些 /tmp 文件不必依赖（随时可能被系统清理）；审计引用以仓库内已提交副本/哈希为准。本轮不写、不改任何 `/tmp/r02-stage-*` 旧文件；新产物只进 `/tmp/r02-final/` 与新证据根。

---

## 2. R02_CANDIDATE_ID 方案

### 2.1 xtask 现有机制已覆盖绑定面（直接采用其 digest 形式）

`rust/crates/xtask/src/candidate.rs`（R17–R21 新增，`main.rs cmd_verify_stage` 编排）已实现比任务书要求更宽的绑定：

- **文件集**：`git ls-files --cached --others --exclude-standard -z`（= 全部 Git 跟踪 + 非忽略新增文件，按原始路径字节排序去重），仅排除本次调用的 `--evidence` 输出子树（`Scope::new`，拒绝 evidence=repo root、拒绝路径含 `..`/symlink 组件）。任务书点名的 `rust/Cargo.lock`、`package-lock.json`、`rust-toolchain.toml`、`stage_maps/R02.json`、`scripts/rust-tauri/r02_*.sh/py` 全部是跟踪文件，其 SHA-256 均已纳入 digest——无需单独再钉。
- **逐文件绑定**：O_NOFOLLOW/开放句柄身份复证（Unix）/reparse 拒绝（Windows），内容 SHA-256 + mode（Unix `mode & 0o7777` 十进制字符串）。
- **digest 算法**：`SHA-256( "lingxi-candidate-source-v1\0" ‖ 逐条目[路径hex, "file", sha256, mode]，每值前缀 8 字节大端长度 )`，schema `lingxi-candidate-source-v1`。
- **时点绑定**：门禁前 1 次 + 每条注册命令后 1 次（含失败/超时分支）+ 结束 1 次；全部 digest 相等、`runner_identity::check` 前后 PASS（防旧 xtask 二进制：main/verify/stage_map/candidate/runner_identity 源码、R02.json、两个 Cargo.toml、rust-toolchain.toml 的编译期内嵌字节须与磁盘逐字节一致 + 源码清单全等）、`git rev-parse HEAD` 结束时不变，才 `stable=true`；否则 `overall` 强制 FAIL（`candidateSourceBinding.reason` 说明）。
- **结果 JSON 字段**（`verify-stage-result.json`）：
  - `testedSha` / `worktreeDirty`（顶层，来自 `git rev-parse HEAD` / `git status --porcelain`）
  - `candidateSourceBinding.before.digestSha256` / `after.digestSha256` / `checkpointAfterEveryCommand[].digestSha256` / `stable` / `finalChangedPathBytesHex` / `testedShaAtEnd` / `excluded`（schema `lingxi-candidate-source-binding-v1`）
  - `candidate-source-before.json` / `candidate-source-after.json`（逐文件 manifest，含各文件 sha256 与 manifest 自身 SHA-256）
  - `runnerSourceBinding`（schema `lingxi-xtask-runner-source-v1`）
  - 单命令级：`evidencePaths` / `missingEvidence` / `preExistingEvidence`。

### 2.2 R02_CANDIDATE_ID 定义与生成命令

**定义**（本轮新增的短标识，仅用于证据根命名与人工核对；权威绑定仍是上面的全量 digest）：

```
R02_CANDIDATE_ID = SHA-256( "R02-CANDIDATE-V1\n" + HEAD_SHA + "\n" + candidateSourceDigest + "\n" ) 的前 16 个十六进制字符
```

其中 `candidateSourceDigest` = §2.1 的 xtask 口径 digest（evidence 根尚不存在时计算，与运行时 xtask 排除空/不存在 evidence 子树后的值一致）。

**一条可复现命令**（在仓库根执行；依赖 git + python3，无网络、无构建）：

```bash
python3 - <<'PY'
import subprocess, hashlib, os
h = hashlib.sha256(); h.update(b"lingxi-candidate-source-v1\0")
out = subprocess.run(["git","ls-files","--cached","--others","--exclude-standard","-z","--"],
                     capture_output=True, check=True).stdout
for raw in sorted(set(p for p in out.split(b"\0") if p)):
    data = open(raw, "rb").read()
    sha = hashlib.sha256(data).hexdigest()
    mode = str(os.stat(raw).st_mode & 0o7777)
    for v in (raw.hex(), "file", sha, mode):
        vb = v.encode(); h.update(len(vb).to_bytes(8,"big")); h.update(vb)
head = subprocess.run(["git","rev-parse","HEAD"],capture_output=True,check=True).stdout.decode().strip()
digest = h.hexdigest()
cid = hashlib.sha256(f"R02-CANDIDATE-V1\n{head}\n{digest}\n".encode()).hexdigest()[:16]
print("HEAD=", head); print("candidateSourceDigest=", digest); print("R02_CANDIDATE_ID=", cid)
PY
```

**对当前 HEAD（cdd213078，干净树）的实测值**（最终修复提交落地后须重算）：

- HEAD = `cdd213078f6947217000c7ecd1a36ab5ffe2bb01`
- fileCount = 10576
- candidateSourceDigest = `7095bd80656de8bf55379565cdc9406de2dc031590ba2f6ed14ed41d2ba13a40`（两次运行及带 evidence 前缀排除运行结果一致——确定性验证通过）
- **R02_CANDIDATE_ID = `4447fe17fbab4107`**

组件参考哈希（均已包含在 digest 内，列出便于人工核对）：`rust/Cargo.lock 90111c4b5988531d7a891339b6ee5df8afc9d3ec655e4929747da07d5811bb49`；`package-lock.json e54a16fe14f15b4797069106392040924a5dd616c69a73bd025729090505ac8b`；`rust-toolchain.toml eec3410410647a7d39f25371e73dda3ecfd5e7c10359090fff11e83053e11c37`；`stage_maps/R02.json 60d2afe9f1595a8860fc5b4a8a47164839adde40e27ebd86773eae6f80f374c1`。

**强制交叉核验**：python 口径只是便于事前命名；正式 Gate 完成后必须断言
`result.candidateSourceBinding.before.digestSha256 == 事前 candidateSourceDigest` 且 `result.candidateSourceBinding.stable == true` 且 `result.testedShaAtEnd == HEAD`，不等即候选漂移或实现漂移，按 FAIL 处理（xtask 侧自身也会强制 FAIL）。

### 2.3 失效规则（验收过程中哪些文件改变会使 candidate 失效）

以下任一发生 → candidate digest 变化 → xtask 逐命令 checkpoint 捕获差异 → `stable=false` → `overall=FAIL`：

1. 任何 **Git 跟踪文件**的字节/删除（含全部 `rust/**`、`desktop/**`、`cli/**`、`server/**`、`scripts/**`、`docs/**`、`shared/**`、`tests/**`、`package.json`、`package-lock.json`、`rust/Cargo.lock`、`rust-toolchain.toml`、`stage_maps/R02.json`、`scripts/rust-tauri/r02_*`）；
2. 任何**新增非忽略文件**出现在工作树（`--others --exclude-standard` 收录）；`.gitignore` 覆盖的构建/缓存产物（dist、target、node_modules 等）不使候选失效，也不属候选源；
3. 跟踪文件的**权限位变化**（Unix mode & 0o7777 参与 digest）；
4. **HEAD 移动**（`testedShaAtEnd != testedSha`，如门禁期间有人 commit/amend/rebase）；
5. 唯一豁免：本次 `--evidence` 指定的输出子树自身（`final-candidate-<id>/`）内的写入不计入。
6. 附加失效（非 digest 但同判 FAIL）：xtask 运行器源码/阶段图与编译期内嵌字节不一致（`runnerSourceBinding` FAIL）。

已知限制（R18-N02 登记，xtask 注释亦自认）：离散快照（前/每命令后/结束）检测不到"单条命令内部改后又恢复"的瞬态；最终封存仍需外部全量清单 + 原始日志共同钉住（新证据根中的 candidate-source-before/after manifest 即是）。

---

## 3. fresh evidence root 方案兼容性确认

目标目录：`artifacts/rust-tauri/R02/final-candidate-<candidate-id>/`（如 `final-candidate-4447fe17fbab4107/`，最终修复提交后重算 id 换名）。

**兼容性逐点确认（全部基于源码）：**

1. **必须事前为空或不存在**：双重拒绝——`main.rs:348-367`（非空 → exit 1，保留旧证据不覆盖）与 `verify.rs:1479-1489`（verify 内部同样拒绝非空 evidence root）。不存在则由 verify `create_dir_all` 自建。**操作规程：不要预创建、不要预放 README/占位文件**（任何非空内容都会被拒）。
2. **逐命令 freshness**：每条命令的 `command_dir = evidence_root/<command-key>` 必须不存在（`verify.rs run_command_with_after_dir`）；每条声明 `evidencePaths` 文件事前必须不存在，存在即 `preExistingEvidence` 非空 → 该命令 FAIL（不 spawn）。
3. **叶证据事前不存在**（R14-F01）：全部 34 个补充叶的 `evidencePaths` + `assertionContract.evidencePath` 在任何命令前必须不存在（`verify.rs:1492-1509`）。
4. **{EVIDENCE} 模板**：`verify.rs substitute()` 以 `--evidence` 绝对路径替换 `{EVIDENCE}`（`{REPO_ROOT}` 另有占位）；`resolve_evidence_path` 对 `{EVIDENCE}/…` 条目 join evidence root。目录名只含 `[a-z0-9-]`，与模板、路径拼接、`Scope` 排除逻辑完全兼容；`Scope::new` 的限制仅为：不得是 repo root、不得含 `..`、不得跨 symlink——命名满足。
5. **候选 digest 无自引用**：该子树被 Scope 排除，写入证据不改变候选 digest（与 §2.2 事前计算一致性等价成立的前提）。
6. **结果落位**：`verify-stage-result.json`、`candidate-source-before/after.json` 写在该根下；xtask 退出码 = overall PASS ? 0 : 1（gate 中止 2）。
7. **历史证据保留边界确认**：artifacts 下 70+ 个旧目录（T01–T08、audit-r16/r17/r18、a15-root-r17、management-r17/r18/r19/r20/r21、r17-leaf-gate 等）属已提交历史，本轮不删除、不覆盖、不重命名；新证据只写新目录。/tmp 旧报告见 §1.6，不依赖、不改动。

**前置阻断再强调**：在 §1.5 的 stage map 缺口修复并提交前，verify-stage R02 会 exit 2（gateAbort），fresh evidence root 里只会得到失败结果 JSON——先落最终修复提交、重算 candidate-id，再开 Gate。

---

## 4. 需要重新生成的证据文件全集（新证据根下）

来源：`rust/crates/xtask/src/stage_maps/R02.json` `commands[].evidencePaths` + `supplementalLeafScenarios[].evidencePaths` + `assertionContract.evidencePath`（{EVIDENCE} = `artifacts/rust-tauri/R02/final-candidate-<id>/`）。**A01–A16 逐项 evidence 文件清单：**

| 场景/命令 | 证据文件（相对 {EVIDENCE}） |
|---|---|
| A01 `a01_smoke` | `A01/a01-health-check.log`、`A01/a01-shutdown.log`、`A01/a01-deptree-evidence.log`、`A01/a01-leaf-cases.json` |
| A02 `a02_boundary_negative` | `A02/a02-summary.log`、`A02/a02-residue-check.log` |
| A03 `a03_dual_instance` | `A03/a03-summary.txt` |
| A04 `a04_path_priority` | `A04/a04-summary.txt` |
| A05/A06 `a05_a06_auth_matrix` | `A05_A06/summary.txt`、`A05_A06/ws-matrix.json`、`A05_A06/leaf-cases.json`、`A05_A06/sessions-list-matrix.json` |
| A07/A08 `a07_a08_storage_tx` | `A07_A08/s1-counts-after-stop.json`、`A07_A08/s4-hashes-after.txt` |
| A07 活故障 `a07_live_fault_01_test` / `a07_live_fault_02_check` | `A07_LIVE_FAULT/case.json`、`A07_LIVE_FAULT/checked.json` |
| A09/A10 `a09_a10_events_matrix` | `A09_A10/probe-matrix.jsonl`、`A09_A10/key-events-dump.json` |
| A11 `a11_backup_restore` | `A11/backup-restore/summary.txt` |
| A12 `a12_recovery_drill` | `A12/recovery-drill/summary.txt` |
| A13 `a13_redaction_scan` | `A13/redaction-scan/summary.txt`、`A13/redaction-scan/inventory.txt` |
| A14 `a14_slow_subscriber` | `A14/slow-subscriber/storm-results.json` |
| A15 `a15_full_chain` | `A15/full-chain/summary.txt` |
| A16 `a16_legacy_regression` | `A16/legacy-entry/summary.txt`（脚本另产 E0–E5 全套 `e0-*`/`e1-*`/`e5-*` 文件于同目录，虽未全部入 evidencePaths，但属 summary 佐证，随目录封存） |
| 补充叶（R13/R14 起 34 叶消费） | `STATIC_WEB/{summary.json,static-web-cases.json,cargo.stdout.log,cargo.stderr.log}`；`MANAGEMENT/{summary.json,management-cases.json,web-login-cases.json,tls-web-login-cases.json,tls-cargo.stdout.log,tls-cargo.stderr.log,security-audit-sources.json,security-audit.jsonl,cargo.stdout.log,cargo.stderr.log}`；`CLIENT/{leaf-cases.json,summary.json,sharing-cases.json,sharing-vitest.stdout.log,sharing-vitest.stderr.log,cli-help-exit0-no-start/stdout.log,cli-help-exit0-no-start/stderr.log,cli-unknown-arg-exit1-no-start/stdout.log,cli-unknown-arg-exit1-no-start/stderr.log,cli-serve-unknown-arg-exit1-no-start/stdout.log,cli-serve-unknown-arg-exit1-no-start/stderr.log}`；`CLI_SESSIONS/{summary.json,cli-sessions-cases.json,producer.stdout.log,producer.stderr.log,auth/sessions-list-matrix.json,auth/cli-sessions-owner.stdout.log,auth/cli-sessions-foreign.stdout.log,auth/cli-sessions-unauthorized.stderr.log,auth/cli-sessions-limit-case.json}`；`CLI_RUST/cli-rust-cases.json`（**依赖 §1.5 补注册的命令**） |
| 运行器自产（不必手动生成） | `verify-stage-result.json`、`candidate-source-before.json`、`candidate-source-after.json`、各命令 `<key>/stdout.log`/`stderr.log` |

16 基础场景 → 命令映射：A01→a01_smoke；A02→a02；A03→a03；A04→a04；A05/A06→a05_a06；A07→{a07_a08, a07_live_fault_01, a07_live_fault_02}；A08→a07_a08；A09/A10→a09_a10；A11→a11；A12→a12；A13→a13；A14→a14；A15→a15；A16→a16。共 19 注册命令（补注册 CLI_RUST 后 20）全部会按"场景引用序 + 叶补充引用序"执行。

---

## 5. A16 / npm 基线冻结（最终 Gate 重跑注意点，只读审计，不改脚本）

脚本：`scripts/rust-tauri/r02_t08_legacy_entry_regression.sh`（1964 行，R1/R4/R5/R6/R7/R8/R9/R10/R11 多轮重写后形态）。现状要点：

1. **基线 SHA**：默认 `BASE_SHA=201584f2917a7fd96d6ea603bdeddbd420082cfe`（可用 `R02_A16_BASE_SHA` 覆盖）。该基线是"纯历史 git 基线"：R4-F02 教训后改为全新构造（自有 CoW `.git` + 陈旧 index 重初始化 + 空 worktree 上非强制 `checkout --detach` + 隔离 node_modules，无 `checkout -f`/`clean -fd`/commit/reset），E0 纯度断言：HEAD==BASE_SHA、index clean、tracked 内容与提交一致、无 residue、无禁用 git 操作。
2. **候选绑定**：`bind_worktree` 全量绑定（tracked 修改 + untracked，`e0-candidate-binding.tsv`）；候选侧 CoW 镜像必须与主仓逐字节一致。**最终 Gate 时工作树应为干净（候选已提交）**：脚本走 "candidate worktree clean: this run binds commit $CANDIDATE_SHA exactly" 分支——此时若出现 `uncommitted-source-rejection` cause 且 CANDIDATE_DIRTY!=1 会 fail-closed（矛盾态）。
3. **E1 与客户端接入的合同冲突仍在**（blocker R02-R18-A16-E1）：当前 HEAD 实测 `git diff 201584f -- core/ server/ desktop/ shared/ tests/ package.json package-lock.json` = **53 文件非空** → `E1-diff-empty` FAIL；`desktop/main.cjs` 有 **10 行**命中 `lingxi-service|rust-target|/rust/` 模式 → `E1-no-rust-in-launch` FAIL；`package.json main == desktop/bootstrap.cjs` 该子项通过。A16 在原合同下静态必然 FAIL——最终 Gate 不得改门禁求绿；需用户对"旧模式行为保持 + 逐入口旧测试 + 显式区分新模式"的合同问题给出决定（R02_FULL_SCOPE_HANDOFF_R18.md §阻碍 2 已提出，尚无答复）。
4. **E5 cause 分类（重跑要点）**：分类是内容驱动、端点锚定的三类已知 + fail-closed——`seal-coordinate-lag`（仅接受真实生产者的完整句形，句后多余内容同时记 UNRECOGNIZED）、`uncommitted-source-rejection`（仅候选脏树合法；基线端出现=基线污染 FAIL）、`UNRECOGNIZED`（任何未知/截断/零载荷/包装错误，两端一律 FAIL）；无 "registered unreadable" 类。完成性判定 `parseable_run`：exit0 必须带完整双行摘要、分量求和一致、`fails<files_failed`/`fails<tests_failed`/空身份等均 CONTRADICTORY/UNPARSEABLE；另有文件级覆盖下界（distinct FAIL-header 文件数 ≥ summary files_failed）与 shell/awk 归一化一致性断言（R10-F01）、逐块精确 TAB 首字段匹配（R11-F02/F03）。
5. **npm 基线冻结含义**：E5b 在纯 201584f 基线上重放三文件封印族（`tests/post-verification-audit-seal.test.ts`、`round2/round3-delivery-evidence`），历史实测（R4-F02-C1 起）为 exit 0 全绿；候选端全量 `npm test` 的封印族红按 `seal-coordinate-lag` 登记（坐标 ab4f2281/201584f 落后于已授权 R01/R02 提交，`RR-SEAL-COORDINATE-LAG` 归属总控封印流程，不得为变绿扩大白名单）。**注意：候选端 npm 原始 exit 1 属如实登记的"registered red"，不是正式绿**；非封印族文件红（non-family）任何一端都 FAIL。
6. **npm 首败/复跑纪律**：首败日志原样保留，复跑须新目录（R3-O01/R4-O01 ustar 瞬态教训）；单文件复跑通过不能改写首败记录。全量 npm test 至今未在当前候选合法执行（blocker R02-R18-DYNAMIC），最终 Gate 需平台真实放行后原命令运行并留首败与复跑。

---

## 6. 给总控的处置建议（不构成授权）

1. 最终修复提交必须先补 `supplemental_cli_rust_matrix` 命令注册（§1.5），否则 Gate 无法启动；提交后以 §2.2 命令重算 R02_CANDIDATE_ID 并命名 `final-candidate-<id>/`。
2. Gate 完成断言：`overall=PASS`、`candidateSourceBinding.stable=true` 且 `before.digestSha256` == 事前 digest、`runnerSourceBinding.status=PASS`、`testedShaAtEnd==HEAD`、`worktreeDirty=false`（建议干净树跑）、16 场景 + 34 叶 roll-up 全 PASS/BLOCKED 如实、`preExistingEvidence`/`missingEvidence` 全空。
3. 交接刷新（HANDOFF source_sha/working_tree_digest/current_candidate_binding、LEDGER tested_sha/16 场景 evidence 指针、ORCH current_head/R02 条目）在 Gate 证据落地后一并绑定新值；旧值按历史保留不回改。
4. A16 E1 合同冲突与 14 叶产品/证据缺口（R02-R18-LEAF-COVERAGE 等 6 blockers）不因新证据根自动消失，须按 R18 交接的恢复条件处置。

— R02-EVIDENCE-AUDITOR-R1，2026-09-28。只读取证；唯一写入为 /tmp/r02-final/evidence-audit-r1.md。
