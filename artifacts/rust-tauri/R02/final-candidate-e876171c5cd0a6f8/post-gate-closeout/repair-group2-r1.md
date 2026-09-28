# R02-REPAIR-GROUP-2-R1 修复报告 — A16 门禁脚本语义修正

- 代理：`R02-REPAIR-GROUP-2-R1`（只负责根因组 2：A16 门禁脚本语义修正）
- 仓库：`/Users/study_superior/Desktop/Code/LingxiAgent`，分支 `codex/rust-tauri-migration`，基线 HEAD `cdd213078f6947217000c7ecd1a36ab5ffe2bb01`
- 日期：2026-09-28，工作区间约 14:38Z–14:54Z（本地 UTC+8 22:38–22:54）
- 唯一改动文件：`scripts/rust-tauri/r02_t08_legacy_entry_regression.sh`（1964 行 → 2133 行；`git diff --stat`：217 insertions / 48 deletions）
- 未动：rust/xtask（并行代理正在修改的 `rust/crates/xtask/src/stage_map.rs`、`rust/crates/xtask/src/verify.rs` 保持其原样）、生产服务代码、docs 账本、E0/E0s/E2/E3/E4/E5(3) 机制、BASE_SHA 默认值、历史注释与负向 fixture。未 commit、未 push。
- 语义依据：`/tmp/r02-final/a16-audit-r1.md`（A16-AUDITOR-R1，必读已完成）。A16 正确语义 = 默认入口未切换 + 无新增回归，而非零 diff / 零 Rust 引用。

---

## 1. 修改点清单（7 个 diff hunk，逐块定位）

| # | 位置（新行号） | 内容 |
|---|---|---|
| 1 | `@@ -10,6 +10,26 @@`（头注释区，R1 块之后） | 新增「R02 FINAL CLOSEOUT (repair-group2, 2026-09-28)」历史注释块：说明 E1 语义修正依据（授权接线后 53 文件 diff + main.cjs 守卫引用导致旧零 diff/零引用判定永久翻红）、E5 白名单泛化、E4.5 新增；明确保留 R1–R11 修复史，仅更新现行行为描述 bullet |
| 2 | `@@ -108,21 +128,39 @@`（头注释 E1/E4.5/E5 bullet） | E1 bullet 改为 default-entry-not-switched 断言集描述；新增 E4.5 bullet；E5 bullet 的 check(2) 描述改为「baseline replay reds ∪ REGISTERED pre-existing families」 |
| 3 | `@@ -188,8 +226,10 @@` | 「REGRESSION gate, not a seal」的 PASS 含义改为「默认入口未切换（E1c+E1d 结构断言）+ 相对基线回放∪已登记族无新增失败」 |
| 4 | `@@ -1617,43 +1657,146 @@`（E1 段整体重写，核心） | 见下文 1.1 |
| 5 | `@@ -1670,6 +1813,17 @@`（E4 之后） | 新增 E4.5：候选副本内 `npm run build:renderer`，失败即 FAIL，日志 `e4-5-build-renderer.log`（命名风格与 E2/E3/E4 一致） |
| 6 | `@@ -1786,24 +1940,39 @@`（E5(2)） | 见下文 1.2；同 hunk 内 NEW_REDS note 文案同步（"family members"→"baseline-reds ∪ registered families"）；E5(3) 逐块原因分类与 fail-closed 语义完全未动（diff 中仅上下文行） |
| 7 | `@@ -1961,4 +2130,4 @@`（尾部 GREEN 文案） | "production surface unchanged" → "default entry NOT switched … (E1c entry chain + E1d 六断言)；authorized wiring range archived in e1-surface-diff-files.txt；no new regressions vs baseline (candidate reds ⊆ baseline-replay reds ∪ registered pre-existing families)" |

### 1.1 E1 段重写（原 1620–1656 → 新 1657–1800）

- **E1a 降级为 RECORD**：`git diff --name-only BASE -- core/ server/ desktop/ shared/ tests/ package.json package-lock.json` 仍计算，写入 `e1-surface-diff-files.txt`（空清单时追加 empty 说明行），只记 `RECORD E1a-surface-diff` note，不再 FAIL。
- **E1b 降级为 RECORD**：binding∩surface 写入 `e1-binding-surface-intersect.txt`（同样空清单+说明），`RECORD E1b-binding-surface-intersect` note；正则合并为单前缀分支并补齐 `package.json/package-lock.json` 前缀形式（审计指出的记录完整性缺口）。
- **E1c 保留**：`package.json` `main == desktop/bootstrap.cjs`（原断言原样）。
- **E1d 替换为新断言集**（全部在 `$CAND_COPY` 内执行，逐项证据落盘 `e1-default-entry-assertions.txt`，任一失败即 `fail`；grep 断言捕获 stderr 且钉死 grep 退出码——文件缺失=exit 2=FAIL，fail-closed，不会因文件消失而空过）：
  - d-1 运行时选择器默认值：`node -e "…rustDesktopEnabled({})!==false||…'node'…!==false||…'rust'…!==true → exit 1"`（导出名以源码核实：`rust-local-service.cjs:319` module.exports 含 `rustDesktopEnabled`）。
  - d-2 无默认注入：`LINGXI_DESKTOP_SERVER_RUNTIME` 不得出现在 `package.json`、`scripts/launch.js`、`desktop/bootstrap.cjs`、`scripts/notarize.cjs`（覆盖 scripts 与 build/electron-builder 段所在的 package.json 全文）。
  - d-3 分支守卫：`desktop/main.cjs` 含 `if (rustDesktopEnabled())`（grep -F）且 Node server 主体仍在（`server-info.json` 处理，grep -F）。
  - d-4 bootstrap 加载链：`desktop/bootstrap.cjs` 以 `app.isPackaged` 选择并 require `main(.bundle).cjs`（两个结构 grep：`app\.isPackaged.*main\.bundle\.cjs.*main\.cjs` 与 `require(app.isPackaged`）。
  - d-5 CLI 默认：`cli/args.ts` 含 `runtime: "node"`（grep -F）。
  - d-6 数据根不双写：`desktop/main.cjs` 含 `RUST_DESKTOP_NODE_SERVER_INFO_PRESENT` 且 `rust/crates/lingxi-service/src/paths.rs` 含 `const RUNTIME_DIR_NAME: &str = "lingxi-service"`（同时钉住标识符与布局值，见 §3 负向验证 N5）。

### 1.2 E5(2) 白名单语义（原"红必须 ⊆ seal 三件套"）

- 新增显式登记变量 `REGISTERED_PREEXISTING_FAMILY="$SEAL_FAMILY"`（当前登记 = seal 三件套；注释注明来源：冻结 `VERIFIED_SOURCE_SHA` 早于授权 R01/R02 提交，属 PROGRESS.md 封印流程的坐标滞后，非 R02 行为回归），落盘 `e5-registered-preexisting-family.txt`。
- 判定改为：`sort -u base-failed ∪ registered > e5-allowed-reds.txt`；`NEW_OUTSIDE=$(comm -23 candidate-failed allowed)` 非空即 FAIL。族外新红仍 FAIL（反回归牙齿保留）；R7-F01 整行精确语义由 comm 的整行比较天然保留。
- `BASE_NON_FAMILY` 检查原样保留（基线回放红必须仍在登记族内）。
- E5(3)（逐块 seal-coordinate-lag / uncommitted-source-rejection / UNRECOGNIZED 分类、fail-closed）与 E5(3b–5) 登记/覆盖/一致性检查：零改动。

---

## 2. 验证结果

### 2.1 语法

| 命令 | 结果 |
|---|---|
| `bash -n scripts/rust-tauri/r02_t08_legacy_entry_regression.sh` | exit 0（编辑后首查与最终复查均 0） |
| 从成品脚本 sed 提取 E1 块（新 1660–1800 行）`bash -n` | exit 0 |

### 2.2 E1 断言集成验证（正例）— 14:50:08Z

方法：把成品脚本的 E1 块逐字节提取（sed 行区间），连同 gate 自己的 `bind_worktree`，在 harness（`/tmp/r02-final/work/harness-e1.sh`）中以 `CAND_COPY=主仓库`（E1 全部只读）执行。输出（exit 0）：

```
RECORD E1a-surface-diff (…: 53 file(s) — archived in e1-surface-diff-files.txt, not gated)
RECORD E1b-binding-surface-intersect (…: 0 path(s) — …not gated)
PASS E1c-package-main (package.json main = desktop/bootstrap.cjs)
PASS E1d-1-runtime-default (rustDesktopEnabled: unset/node → false, explicit rust → true)
PASS E1d-2-no-default-injection (…absent from package.json, scripts/launch.js, desktop/bootstrap.cjs, scripts/notarize.cjs)
PASS E1d-3-branch-guard (…'if (rustDesktopEnabled())' at main.cjs:1438; server-info.json body intact)
PASS E1d-4-bootstrap-load-chain (…app.isPackaged selects and requires main.bundle.cjs / main.cjs)
PASS E1d-5-cli-default-node (cli/args.ts default runtime = "node")
PASS E1d-6-no-double-write-guards (main.cjs RUST_DESKTOP_NODE_SERVER_INFO_PRESENT + rust paths.rs RUNTIME_DIR_NAME layout both present)
HARNESS-E1-EXIT=0
```

关键点：E1a 精确记录审计实测的 **53 文件** diff 而不再 FAIL——即修复后的 gate 在授权接线后的 HEAD 上 E1 静态可绿。证据文件内容核实：`e1-surface-diff-files.txt` 53 行（desktop/main.cjs、locales…）；`e1-default-entry-assertions.txt` 含各 grep 退出码与命中（d-2 exit 1 空命中、d-3 guard 命中 1438 行等）。

（d-1 的裸命令在主仓库也单独跑过：`node -e …` → `assert-ok`, exit 0。）

### 2.3 负向验证（E1d 必须能检出违约）— 14:50:48Z–14:51:56Z

方法：`git clone --shared`（对象共享、只读）HEAD 到 /tmp 临时副本（10576 个 tracked 文件，node_modules 后补 CoW 拷贝），逐项单点变异后跑同一 harness，期望 exit 1 + 对应 FAIL 消息；每例之后 `git checkout --` 还原。

| # | 变异 | 结果 |
|---|---|---|
| N1（任务书要求项） | `rust-local-service.cjs:187` 默认 `'node'`→`'rust'` | exit 1，`FAIL: E1d-1: desktop runtime selector no longer defaults to node` ✔（E1b 同时把该未提交 surface 改动记为 1 path——记录语义正确） |
| N2 | `scripts/launch.js` 追加 `process.env.LINGXI_DESKTOP_SERVER_RUNTIME ||= "rust"` | `FAIL: E1d-2: …injects LINGXI_DESKTOP_SERVER_RUNTIME…` ✔ |
| N3 | 删除 `desktop/main.cjs:1438` `if (rustDesktopEnabled()) {` 行 | exit 1，`FAIL: E1d-3: …lost the rustDesktopEnabled() startServer guard…` ✔ |
| N4 | `cli/args.ts:18` `runtime: "node"`→`"rust"` | exit 1，`FAIL: E1d-5: cli/args.ts no longer defaults runtime to "node"` ✔ |
| N5 | `paths.rs:36` `RUNTIME_DIR_NAME` 值改为 `"moved-runtime"` | 首版（只 grep 标识符）未检出 → **已把 d-6 收紧为 grep 常量+值整串**；重跑 exit 1，`FAIL: E1d-6: …RUNTIME_DIR_NAME…` ✔（收紧后正例复跑仍全绿） |
| N6 | 删除 `desktop/main.cjs:1333` `RUST_DESKTOP_NODE_SERVER_INFO_PRESENT` 行 | exit 1，`FAIL: E1d-6: …no-double-write guards missing` ✔ |

负向验证全部在 /tmp 临时副本进行，未触碰仓库；临时克隆已删除。

### 2.4 E4.5 实跑 — 14:53:03Z

在带 node_modules 的同一临时副本内 `npm run build:renderer`：**exit 0**（`✓ built in 6.81s`，chunk-size 警告非错误）。输出目录 `desktop/dist-renderer/` 为 gitignored，且 gate 在候选副本内执行不污染调用方工作树。日志：`/tmp/r02-final/work/e4-5-build-renderer.log`。

### 2.5 E5(2) 新语义 fixture 验证 — 14:53:25Z

以与脚本逐字相同的 `sort -u`/`comm -23` 管道跑 4 个场景（`/tmp/r02-final/work/e5fix/`）：

| 场景 | 期望 | 结果 |
|---|---|---|
| A 候选红=seal 三件套，基线红=round2+round3 | 通过（NEW_OUTSIDE 空） | ✔ PASS |
| B 候选红含 `tests/unrelated.test.ts` | 族外新红被拦 | ✔ 检出（反回归牙齿保留） |
| C 候选红= `…seal.test.ts.regression.test.ts`（子串撞名） | 整行比较必须拦 | ✔ 检出（R7-F01 语义保留） |
| D 登记族扩展（+`tests/known-flaky.test.ts`）后该族成员无基线红也通过 | 登记账本机制生效 | ✔ PASS |

### 2.6 未跑项（如实说明）

- **全量 A16 gate**（E0 副本构造 + E2–E5 全程，含双端 npm test 回放）未执行——任务书明确那是最终 Gate 的事；本修复的验证覆盖到被改动的每一块逻辑（E1 集成正/负例、E4.5 实跑、E5(2) 管道 fixture）。
- E0s 分类器 fixtures 未动也未重跑（E5 分类机器零改动；bash -n 覆盖语法）。

---

## 3. 结论

- 唯一修改 `scripts/rust-tauri/r02_t08_legacy_entry_regression.sh`：E1a/E1b 由"零 diff 即过/有 diff 即死"降级为授权接线范围的证据记录；E1d 换成 6 项"默认入口未切换"结构断言（fail-closed）；新增 E4.5 build:renderer；E5(2) 泛化为"基线红 ∪ 显式登记既有失败族"（当前登记=seal 三件套，注释注明封印坐标滞后来源）；头注释追加本轮修正依据；尾部 GREEN 文案同步。BASE_SHA 默认值、E0/E0s/E2/E3/E4/E5(3) 机制、历史注释、负向 fixture 均未动。
- `bash -n` exit 0；E1 新断言在当前 HEAD（干净树上与工作树等价的 tracked 内容）全部通过；6 项负向变异全部被检出；E4.5 实跑 exit 0；E5(2) 四场景行为符合新语义且族外新红/撞名仍 FAIL。
- 修复后的 gate 不再有"授权接线后静态必 FAIL"的假红，同时族外新红、UNRECOGNIZED、分类矛盾的 fail-closed 牙齿完整保留。

## 4. 工件索引（/tmp）

- 本报告：`/tmp/r02-final/repair-group2-r1.md`
- harness 与提取块：`/tmp/r02-final/work/{harness-e1.sh,e1-block.sh,bind_worktree.fn.sh}`
- 正例证据：`/tmp/r02-final/work/e1test-pos{,2}/`（summary、e1-surface-diff-files.txt、e1-binding-surface-intersect.txt、e1-default-entry-assertions.txt）
- 负例输出：`/tmp/r02-final/work/e1test-neg{1,2,3,4,5b,6}/` 与 `neg{1,3,4,5b,6}.out`
- E4.5 日志：`/tmp/r02-final/work/e4-5-build-renderer.log`
- E5(2) fixtures：`/tmp/r02-final/work/e5fix/`
