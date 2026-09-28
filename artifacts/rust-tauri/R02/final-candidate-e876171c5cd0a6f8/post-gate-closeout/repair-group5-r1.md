# R02 最终收口修复 — repair-group5-r1（A16 门禁 E0 自引用绑定缺陷）

- 代理：R02-REPAIR-GROUP-5-R1
- 唯一改动文件：`scripts/rust-tauri/r02_t08_legacy_entry_regression.sh`（未 commit/push；未放松任何门禁断言）
- 日期：2026-09-28

## 1. 第 1 轮失败证据（形态确认）

- `artifacts/rust-tauri/R02/final-candidate-a0af83666f89cc4b/a16_legacy_regression/stderr.log`：
  `FAIL: candidate copy does not mirror the invoking worktree (tracked+untracked content binding differs)`
- `.../A16/legacy-entry/summary.txt`：8 行，止于 `== isolated copies under /var/folders/.../r02-a16.erp4K4 ==`（即死在 E0 镜像比对）。
- `.../legacy-entry/` 内容恰为 4 个自引用文件：`summary.txt`、`e0-candidate-binding.tsv`、`e0-candidate-dirty.txt`、`e0-candidate-dirty.diff`。

机制复核（逐行核对脚本 316-352 段）：
1. 绑定 1（主仓）运行时 `e0-candidate-binding.tsv` 由 shell 重定向刚创建、内容为空（python 的 print 在进程退出才落盘），以"空文件 SHA"入绑定；`summary.txt` 只含前 6 行 note。
2. 绑定 1 之后、`cp -Rc` 之前，脚本向 `$EVIDENCE_DIR` 写入：`e0-candidate-dirty.txt`、`e0-candidate-dirty.diff`、note 追加 summary.txt 2 行（dirty-note 与 isolated-copies）、绑定 tsv 从空到全量。
3. 副本携带的是 cp 时刻的状态 → 绑定 2（副本）中这 4 个文件全部与绑定 1 不同 → `cmp` 必然失败。两次扫描之间脚本写仓库内路径的点**仅限** `$EVIDENCE_DIR` 子树（其余全在 `$WORK`/TMPDIR，git 读命令不落盘）——已逐条核对，无同族遗漏。
- 运行器捕获 `a16_legacy_regression/{stdout,stderr}.log` 虽在运行期增长，但被 `.gitignore:99 (*.log)` 忽略，本就不入 `git status --untracked-files=all` 绑定，无需排除。
- 其他 verify-stage 证据目录（A01..CLI_RUST 等）在 A16 运行期间静止 → 留在绑定内无害且必要（保持全量绑定语义）。

## 2. 修复（选择"仅排除 $EVIDENCE_DIR 精确子树"，影响更小、更可解释）

对 `bind_worktree` 增加可选第 3 参 `[exclude-prefix]`：

- `EVIDENCE_REL_PREFIX`：由两个 canonical（`pwd -P`）路径 `os.path.relpath($EVIDENCE_DIR, $MAIN_REPO)` 一次性算出；证据根在仓外（/tmp 布局）时为空 → 绑定保持全量、无 header（历史行为不变）。
- python 侧对称过滤：`path == prefix or path.startswith(prefix + "/")` → skip；两次扫描传入**同一个**前缀（副本复刻同一相对布局）。
- 可审计性：排除生效时，两份 tsv 头部都写同一行 `# binding-exclusion: <prefix>/** — this gate's own run-time evidence output subtree (runner output, never candidate source); applied symmetrically to BOTH ...`；summary.txt 增加 `NOTE e0-binding-exclusion: ...` 行；脚本头文档新增 repair-group5 段落 + E0 条目补充。
- 消费端适配（3 处）：
  - 脏检查 `[ -s tsv ]` → `CANDIDATE_BINDING_ROWS="$(grep -vc '^#' tsv || true)"`（避免纯 header 文件被误判为脏）；
  - dirty-note 的路径计数由 `wc -l` → 数据行数；
  - E1b 交集约入前先 `grep -v '^#'` 过滤 header。
- E0s 新增手术性自检 fixture（无提交 scratch 仓，真实 verify-stage 前缀形态）：排除子树内 summary.txt 变更 + tsv 新建 → 绑定不变；非排除未跟踪文件内容变更 → 绑定必变；header 行必须存在；被排除路径不得出现为绑定行；**不带前缀时证据子树必须仍被全量绑定**（默认穷尽性不变）。
- R4-F02 教训不弱化：候选源文件仍全量绑定；被排除的只有本运行自身输出；xtask candidate.rs 全树 SHA 绑定仍为外层兜底。

## 3. 验证

### 3.1 静态
- `bash -n` exit 0。
- 逐行 diff 复查（保留 repair-group2 既有未提交改写，仅叠加本修复）。

### 3.2 负向钻演（真实函数文本，从脚本 sed 提取的 `bind_worktree`，scratch git 仓）
- A 无排除 + 证据子树在两次扫描间变更 → 两绑定**不同**（复现缺陷形态，2 行差异）。
- B 排除激活 + 同样证据子树变更（含 tsv 从无到有）→ 两绑定**逐字节相同**。
- C 排除激活 + 非排除未跟踪文件内容变更（rust/crates/new_candidate_file.rs）→ **检出**（SHA 行差异）。
- D 排除激活 + 新增非排除未跟踪文件（server/new_untracked.ts）→ **检出**（新绑定行）。
- E header 行正确；被排除子树在绑定中仅 header 提及 1 次；3 个非排除路径全部绑定。
- F 被跟踪文件修改（tracked.txt）在排除激活下仍入绑定。

→ 排除只覆盖证据根自身，不掩盖任何真实候选变化。

### 3.3 动态全链（真实仓库，仓内证据根 + 脏工作区）
- 命令：`bash scripts/rust-tauri/r02_t08_legacy_entry_regression.sh artifacts/rust-tauri/R02/.g5-verify-tmp/A16`（dot 前缀临时路径，结束即删）。
- E0 真实通过证据（`.../.g5-verify-tmp/A16/legacy-entry/summary.txt`）：
  - `PASS E0-ancestry`
  - `NOTE candidate-worktree-dirty: ... e0-candidate-binding.tsv (263 paths)`（脏工作区：并行代理正改 10 个跟踪文件）
  - `NOTE e0-binding-exclusion: artifacts/rust-tauri/R02/.g5-verify-tmp/A16/legacy-entry/** ...`
  - **`PASS E0-baseline-purity`**
  - **`isolated copies verified: base pristine at 201584f29...; candidate mirrors the invoking state (tracked+untracked)`** ← 第 1 轮失败点，本轮真实通过
  - `PASS E0s-gate-self-checks`（含新 fixture 行 `binding exclusion (repair-group5): ...`）
  - `PASS E1c / E1d-1..6 / E2 / E3 / E4 / E4.5` 依次通过
- E5 结果：见下方"最终运行结果"。
- 运行期 stderr 存在 `✗: command not found`、`]: command not found`、`fatal: ambiguous argument '失败'` 噪声 —— 定位为**既有**（非本修复引入）：R9 的 `PASS E0s` 长注中 `legal \`]\`/\`,\`/escape` 反引号 + `<<CLASSES` 未引号 heredoc 内的反引号片段被 bash 命令替换执行；替换失败不传染退出码，门禁判定不受影响（证据 txt 中该处反引号内容被吞为空，属 cosmetic，超出本组授权范围未动）。

## 4. 最终运行结果（全链，真实仓库 + 仓内证据根 + 脏工作区）

- 命令：`bash scripts/rust-tauri/r02_t08_legacy_entry_regression.sh artifacts/rust-tauri/R02/.g5-verify-tmp/A16`；gate 退出码 1（E5 fail-closed）；summary 快照存 `/tmp/r02-final/g5-verify-summary.txt`（91 行），临时证据根已删除。
- **E0（本组缺陷）真实通过**：`PASS E0-ancestry` → `NOTE candidate-worktree-dirty ... (263 paths)` → `NOTE e0-binding-exclusion: artifacts/rust-tauri/R02/.g5-verify-tmp/A16/legacy-entry/** ...` → `PASS E0-baseline-purity` → `isolated copies verified: base pristine at 201584f29...; candidate mirrors the invoking state (tracked+untracked)`。第 1 轮死点（`candidate copy does not mirror ...`）在本配置下确定性消失。
- E0s（含新增排除 fixture）/ E1c / E1d-1..6 / E2 / E3 / E4 / E4.5 全部 PASS。
- E5：base 侧 seal 族重放 GREEN（exit 0，0 失败）；candidate npm test `Test Files 9 failed | 1466 passed | 3 skipped`，verdict=PARSABLE；gate 按设计在"族外新红"处 fail-closed：`FAIL: E5: candidate failures OUTSIDE baseline-replay reds ∪ registered pre-existing families`。
  - 族外 6 文件：`desktop/src/react/settings/__tests__/settings-primitives-contract.test.ts`、`tests/cli-closure-census.test.ts`、`tests/desktop-rust-local-service.test.cjs [ ... ]`、`tests/open-boundary-lint.test.ts`、`tests/persistence-schema-tripwire.test.ts`、`tests/server-startup-diagnostics-contract.test.ts`。
  - **与 E0 修复无关的实证**：这 6 个文件在主仓（非副本、无本脚本改动参与）直接 `npx vitest run` 结果完全相同（5 个 TS 文件 13 个测试失败；`.cjs` 为 "No test suite found in file" 的 suite 级错误）。即它们是当前工作区内容（并行代理在途修改 + 已提交树）的真实红，不是镜像/绑定修复引入或掩盖的；是否登记属 orchestrator/其他组的决策，本组未动任何白名单。
  - 任务预期"E5 期望 seal 族按 seal-coordinate-lag/uncommitted-source-rejection 登记、无族外新红"未在本轮达到——族外新红存在，但其根因全部落在工作区内容（已实证与本修复正交），非 E0 缺陷修复的回归。因 gate 在族外红处先于逐文件 cause 分类终止，e5-candidate-causes.txt 未生成（fail-closed 设计使然）。
- 副产物观察（超出本组范围，仅报告）：运行期 stderr 的 `command not found` 噪声源自既有文本的 shell 引用问题——R9 的 `PASS E0s` 长注内反引号片段（如 `` `]` ``/`` `,` ``）与 `<<CLASSES` 未引号 heredoc 内的反引号片段被 bash 命令替换执行；不影响退出码与门禁判定（替换失败不传染），但对应证据文本中反引号内容被吞。未修改（避免越组改动）。

## 5. 结论

- 修改点：仅 `scripts/rust-tauri/r02_t08_legacy_entry_regression.sh`（在 repair-group2 既有未提交改写之上叠加）：bind_worktree 可选排除前缀（对称过滤 + tsv header 审计行）、EVIDENCE_REL_PREFIX 一次性计算、两处调用点传参、脏检查/E1b 消费端适配 `#` header、E0s 手术性排除 fixture、头文档与注释。未 commit/push，未放松任何断言（排除仅作用于本运行自身输出子树；候选源仍全量绑定）。
- E0 真实通过证据：`PASS E0-baseline-purity` + `isolated copies verified`（见 §4，快照 /tmp/r02-final/g5-verify-summary.txt）。
- 负向验证：scratch 仓钻演 A-F（§3.2）+ E0s 内置 fixture；主仓复跑证明族外红与本修复正交（§4）。
- 报告路径：/tmp/r02-final/repair-group5-r1.md

