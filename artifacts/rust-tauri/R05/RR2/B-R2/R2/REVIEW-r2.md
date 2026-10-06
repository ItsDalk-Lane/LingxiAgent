# R05 RR2 WP-B 第 2 轮独立验收（R2 复核）——REVIEW-r2

- 验收者：B 包第 2 轮独立验收智能体（全新上下文，未参与实施；核验第 1 轮验收者日志并补完代码审计与总判定）。
- 日期：2026-10-04（会话时间 2026-10-07 凌晨）。候选：ad5ec4e9853a51ed929f1e2e077b97d41c951572（分支 codex/rust-tauri-migration，工作树含未提交 RR2 改动，本验收零改动、未 commit/push）。
- 范围：F42（r02_t08 运行输出归属重写）+ F44（maxBuffer）+ B-r2（分类器形态演化）+ B-r3（note 反引号）。
- 第 1 轮验收者完成全部子项检查后被超时终止未写结论；其日志在 `review-r1/`，本轮核验其结论（读日志，不重跑），并补完代码审计。

## 一、第 1 轮子项日志核验（全部成立）

| 子项 | 证据 | 核验结论 |
|---|---|---|
| N1 源码变异→cmp 红 | `review-r1/n1/n1-run.log` | 成立：`gate exit=1` + `FAIL: candidate copy does not mirror the invoking worktree` + `REVIEW-N1 PASS (cmp red on real source mutation)`。变异目标未记录于该日志（偏简），由实施侧 `n1/driver-console.log`（mutation: appended-lib.rs → 同一 FAIL 形态）与 G-NEG F42-N1（credentials.rs → mirror cmp 红）佐证。 |
| illegal rust/ | `review-r1/illegal/rust-stderr.log` | 成立：绑定前精确拒绝，理由行「not a dedicated run-output root strictly inside artifacts/…」+ `FAIL: E0: declared run-output root 'rust' is illegal (F42)`。 |
| illegal artifacts/rust-tauri | `review-r1/illegal/art-stderr.log` | 成立：同一拒绝形状，点名 `artifacts/rust-tauri`（整个证据树被拒）。 |
| F44 seal 形态 | `review-r1/f44/f44-seal-vitest.log` | 成立：52,986 行，`grep -c ENOBUFS` = 0；尾部为完整诊断（违规清单断言 diff + `Test Files 1 failed (1)` / `Tests 1 failed | 2 passed (3)` + Duration），非 ENOBUFS 崩溃形态。测试本身红（工作树未提交 RR2 改动，预期态），诊断完整可分类。 |
| B-r3 复检 | `review-r1/b-r3-recheck/` | 成立：`note-stdout.txt` 1,556 字节、PASS E5-cause-classification 整句完整（含 `error: patch too large` 反引号原貌全文）；`note-stderr.txt` 0 字节，无 command not found。 |
| S1a directed | `review-r1/s1a/gate-stdout.log` | 成立：尾部 `RESULT: R02 legacy entry regression DIRECTED (E0–E4.5) ALL GREEN`（SKIP E5 BY SCOPE: directed mode）；`gate-stderr.log` 0 字节（stderr 为空）。 |
| full 轮 | `review-r1/full/` | FAIL 于 E5 unparseable——离线复析见下节，判定为负载型环境抖动，fail-closed 行为正确，非 F42/B 逻辑缺陷。 |

## 二、full 轮 E5 unparseable 的离线复析（本轮独立完成）

**事实链（全部直接读自日志字节）**：

1. `full/ev/legacy-entry/e5-candidate-npm-test.log` 共 66,944 行 / 6.7 MB，含 6 个 FAIL 块（3 文件：post-verification-audit-seal、round2-delivery-evidence、round3-delivery-evidence）。seal 块独占第 10,754–63,723 行（52,970 行），其中 `+` 行 26,478 行——即台账所称「~2.6 万行+」巨型断言 diff（违规文件清单），与 s5-full-4/F44 观测同源。
2. 日志**确实包含**两行完整 summary（第 66,939/66,940 行，`sed -n l` 逐字节核对无隐藏字符）：
   - ` Test Files  3 failed | 1471 passed | 3 skipped (1478)` → 3+1471+3 = **1477 ≠ 1478**
   - `      Tests  6 failed | 15032 passed | 15 skipped (15058)` → 6+15032+15 = **15053 ≠ 15058**
   两行各缺 1 个文件 / 5 个测试的 passed 计数而总数不变——**汇总行内部不一致**。
3. 同一日志尾部记录了成因：`Vitest caught 1 unhandled error` / `Error: [vitest-pool]: Worker forks emitted error` / `Caused by: Error: Worker exited unexpectedly` / `   Errors  1 error`。
4. 对照运行 `s5-full-5/`（同绑定语义、同候选、仅早 90 分钟、B-r3 note 修复前的脚本）：summary 为 `3 failed | 1472 passed | 3 skipped (1478)`（和恰为 1478）与 `6 failed | 15037 passed | 15 skipped (15058)`（和恰为 15058），Unhandled Error 计数 0，`e5-candidate-summary-counts.txt` verdict=**PARSABLE**、tests_failed=6。full 轮相对 s5-full-5 恰好少 1 个 passed 文件、5 个 passed 测试——一个 worker 的结果从分量聚合中丢失而总数照旧的典型签名。
5. 解析路径（脚本 `parseable_run`，2364–2460 行）：Test Files 行 `parse_summary` 于「components sum ≠ total」处返回 0 → `complete=0` → verdict=UNPARSEABLE → E5 fail-closed。`&&` 短路使 Tests 行解析从未执行，故 counts side-channel 记 `tests_failed=0`（尽管 t_lines=1 且有 6 个 FAIL 块）——`e5-candidate-summary-counts.txt` 里 tests_failed=0 与 fails=6 的「矛盾」是短路径伪影，非额外异常。stderr 的「no vitest summary and no parsable FAIL block」是该 UNPARSEABLE 分支的固定文案（2994 行附近），本次实际触发条件是「存在但不可求和的 summary」；fail-closed 方向正确（见下「观察」）。

**结论**：根因 = vitest worker 崩溃（Unhandled Error: Worker exited unexpectedly）在高负载（巨型断言 diff + delivery 重放套件，Duration 187s/import 1424s/tests 1580s 的大并行）下丢失一个 worker 的结果聚合，使 producer 自己的 summary 分量与总数不一致；解析器按「分量必须求和等于总数」的完备性谓词正确拒绝并 fail-closed。属**负载型环境抖动，非 F42/B-r2/F44 逻辑缺陷**。同链在 s5-full-5 已有 exit 0 完整通过证据（见下节）。

**s5-full-5 完整链证据成立**：`s5-full-5/run-root/stdout.log`——run 21:20:42、绑定工作树（tracked diff sha256=131b031e…、546 paths）、两个排除根 `…/s5-full-5/{ev/legacy-entry,run-root}/**`（6 层 DIR 单元、专用、围栏内）；E0-ancestry/purity、E0s 62 fixtures、E1 全组、E2/E3/E4/E4.5 全 PASS；E5a 候选 exit 1、3 个失败文件全部 file-level coverage 达标；E5b 基线重放 exit 0；E5-no-new-reds / E5-cause-classification / E5-block-coverage / E5-cause-coverage 全 PASS；末行到达脚本成功收尾段（set -euo pipefail 下唯一可达路径）`= R02-A16 legacy-entry regression: GREEN …（candidate raw exit 1, base raw exit 0 — registered, not formal green）`，即门禁 exit 0。其 stderr 仅含 B-r3 修复前的已知 note 噪声（`line 3177: error:: command not found`），该缺陷已由 B-r3 修复并在 r1 复检证实清零。

## 三、代码审计（git diff 对照 HEAD ad5ec4e98；`scripts/rust-tauri/r02_t08_legacy_entry_regression.sh` +759/-48，加 F44 四文件与台账）

### 1. 硬性保留 —— PASS
- `bind_worktree` 核心（`status --porcelain=v1 -z --untracked-files=all`、untracked 内容 sha256、rename 记新路径）未动，仅排除参数从单一 prefix 换为多单元集合，双侧对称排除、每单元一条 `#` header 行。
- 镜像 `cmp -s e0-candidate-binding.tsv candidate-copy-binding.tsv || fail` 原样保留；`e0-candidate-dirty.{txt,diff}` 原始留档保留；复制竞态检出未弱化（s5-full-2 红即其设计行为）。
- 未排除整个 artifacts（见下围栏）；未忽略全部 untracked；未删 cmp；无重拍快照接受漂移（单元集合在首次绑定前一次计算，双方不变传入）。

### 2. DIR/FILE 排除单元围栏 —— PASS
- `discover_run_output_sinks`：进程树（self+祖先，环/深度上限）fd-1/2 文件 sink；/proc 快路径 + `lsof -a -p … -d 1,2 -Fpfn` darwin 回退；任一 pid 均不可解析 → `DISCOVERY-UNAVAILABLE` → 门禁 fail（fail-closed）。
- sink 重定向到 tracked 内容 → `TRACKED-SINK` 行 + 门禁 fail。
- DIR 单元四重围栏：仅 `artifacts/` 内、`len(split("/")) >= 4`（`artifacts`/`artifacts/<area>`/裸 stage 目录不可能成为单元）、`tracked(cand)` 为假、且 `dedicated()` 全目录逐文件归属（符号链接目录即不合格；每个现存文件必须是本 run 的 sink、或位于某个严格更深的 attribution 目录下）；不满足则该 sink 降级 **FILE 单元**（恰好排除该文件，同目录旧证据/兄弟文件保持绑定——N6a 降级路径）。
- 发现调用在子 shell 内重定向（`( discover_run_output_sinks ) > "$SINKS_FILE"`），避免调用点重定向遮蔽门禁 bash 自身 fd-1（前稿实测缺陷③，注释如实留档）。
- E0s fixtures 覆盖：run-output-root 校验 6 例（fresh dir 过；tracked 目录/tracked 路径/repo 根/绝对/父相对拒）、declared 围栏 8 例、多单元 3 例、FILE 单元 2 例。

### 3. R02_A16_RUN_OUTPUT_ROOTS attestation 围栏 —— PASS
- `validate_declared_run_root`：case `artifacts/*/*/*`（bash case 的 `*` 跨 `/`，为深度下限 ≥4 分量）→ `artifacts`、`artifacts/rust-tauri`、裸 stage 目录 `artifacts/rust-tauri/R05`（3 分量）结构性拒绝；`""|.|..|../*|/*|*/` 拒；`[ -d ]` 不存在路径拒；`ls-files` 覆盖 tracked 内容拒；再叠加 `validate_run_output_unit` 双重校验。r1 日志证实 rust/ 与 artifacts/rust-tauri 在真实门禁绑定前精确拒绝；代码+fixtures 证实裸 stage 目录、不存在路径同样拒绝。

### 4. .gitignore 零改动 —— PASS
`git diff HEAD -- .gitignore` 为空、`git status` 干净。

### 5. F44 四文件 —— PASS
- `tests/post-verification-audit-seal.test.ts`：`diffNamesSinceVerified` 的 git diff --name-only 加 `maxBuffer: 64MB`（+8：注释 7+常量 1，options 一行改动）。
- `tests/round2-delivery-evidence.test.ts`：ls-files / ls-tree / node guard 三处各加 maxBuffer（+10/-0）。
- `tests/round3-delivery-evidence.test.ts`：同构三处（+13/-3）。
- `.sync-audit/verify-post-verification-diff.mjs`：git diff execSync 加 maxBuffer（+8/-1）。
- 四文件均为纯选项追加+注释/常量，断言/校验语义零改动；同文件其余调用未动。越白名单在 RR2_PROGRESS.md 轮次日志行级登记，且授权冲突核对属实：seal 测试的 `AUDIT_ALLOWLIST` 明文含 `.sync-audit/verify-post-verification-diff.mjs` 与 `tests/post-verification-audit-seal.test.ts`（自维护设计）；round2/round3 两测试不在 allowlist、如实按越白名单登记并说明既有非审计变更集背景（不新增违规类别）。

### 6. B-r2 分类器形态演化 —— PASS
- 形态识别严格性：ptl 预扫描仅在块首 payload 行匹配 `genwrapper_exact`（generator wrapper）后进行，并从 wrapper 中提取 `gscript`；round2 = 连续 6 行**逐字相等**（Traceback 头；`File "<wrapper>", line 457, in <module>`+其源行；`File "<wrapper>", line 359, in replay_and_verify`+raise 原行；末句 `RuntimeError: patch replay failed: error: patch too large`）；round3 = 裸句**整行相等**；`gscript` 按各自脚本名绑定，跨生产者（裸句在 round2 wrapper 下 / traceback 在 round3 wrapper 下）、node-guard wrapper、变体帧（458≠457）、截断句、其他 git 错误文本全部无法通过相等性检查 → UNRECOGNIZED。9 例 E0s fixtures（2 真实 s5-full-4 形状正例 + 7 负例）与代码一一对应。
- 登记类仍判红不吞退出码：`ptl_start[b] → b_seal=1`（seal-coordinate-lag 登记红）；E5a 明确打印 `candidate npm test raw exit code: 1`，收尾注明 registered/never formal green。
- 「诚实注明」与 R03 链历史一致性：抽查 `artifacts/rust-tauri/R03/STAGE-REPAIR-G01-F02/verify-stage-r02/A16/legacy-entry/`——其 e5-candidate-blocks.txt 含 `patch replay failed: error: patch too large`、其 summary 为 53-fixture 时代（无该形态类）、UNRECOGNIZED 在案，与「R03 起该形状一直诚实 UNRECOGNIZED、各链 fail-closed」的登记表述一致。

### 7. B-r3 —— PASS
- diff 中唯一改动即 3177 行 note 的 `` `error: patch too large` `` → `\`error: patch too large\``（转义反引号，打印文本含反引号原貌）；既有 E0s 段（原 2595 行）的既有转义未动（登记为范围外）。`bash -n` 过；r1 复检 stdout 完整、stderr 0 字节。

## 四、总判定

**F42 / F44 / B-r2 / B-r3 四项全部 PASS；WP-B RR2 轮 2+3 验收通过。**

- 完整链证据：s5-full-5（exit 0，E0–E5 全链，登记红如实记录）；review-r1/full 的 E5 unparseable 经复析为 vitest worker 崩溃致 summary 内部不一致的负载型抖动，门禁 fail-closed 方向正确，无需代码变更。
- 无 mustFix。两条非阻断观察（供后续维护，不属本验收范围）：
  1. E5 UNPARSEABLE 分支的固定文案「no vitest summary and no parsable FAIL block」在「summary 存在但分量不等于总数」时会误述为「无 summary」；fail-closed 行为不受影响，措辞可在后续轮次区分两种触发。
  2. 排除单元的 tracked 校验在绑定前一次执行，理论上存在校验后至绑定前的 TOCTOU 窗口（需并发行为者刻意对运行输出根做 git add）；对称 cmp 仍覆盖双侧绑定，风险极低，登记备查。

—— 第 2 轮独立验收智能体，2026-10-07
