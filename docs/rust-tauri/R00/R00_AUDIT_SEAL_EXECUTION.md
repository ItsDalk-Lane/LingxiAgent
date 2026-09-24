# R00 审计封印收口执行报告（ZCode）

- 任务：R00 审计封印收口执行（ZCode）→ 经 C1 独立审查 R1（FAIL）后由「R00 审计封印 C1 修复 R1（ZCode）」修订
- 执行日期：2026-09-24
- 执行者：ZCode 审计封印收口执行代理（主会话直接执行，未委派任何子代理）
- 分支：`codex/rust-tauri-migration`
- **终态声明：`READY_FOR_INDEPENDENT_SEAL_REVIEW`。本报告不宣布封印 PASS、不替代独立验收、不启动 R01；提交、推送与坐标推进由总控在独立验收通过后按 `PROGRESS.md` seal 工作流执行。后续独立验收在仓库外出具结论，不再新增待收录的仓库文件。**

## 1. 任务边界与纪律

本任务只准备 seal 候选，完成即结束。本轮（C1 修复 R1）已遵守的硬边界：

- 未 commit / push / PR / tag / release；未新建分支或 worktree；未委派代理。
- 未接触真实用户数据、真实供应商、付费 API；无任何真实外发（含 `git fetch`，本机代理端口不可达，详见 §8）。
- 未扩大 `.sync-audit/verify-post-verification-diff.mjs` 与 `tests/post-verification-audit-seal.test.ts` 两份 allowlist（两处本轮零改动）；未删除/退役任何门禁；未放宽任何断言；未手改生成投影；未以 skip 充当 PASS；未虚报审计坐标。
- 未更改生产应用代码；未触碰 R01 文件；未修改 R00 阶段既有验收结论、账本、HANDOFF 与任何历史评审报告字节；审查报告 `R00_AUDIT_SEAL_REVIEW_C1_R1.md` 原字节保持不变。
- 两份证据生成脚本（`create-delivery-patch.py` / `create-round3-patch.py`）的修复属于审查 F2 明确要求的证据完整性修复，见 §5.0；此外无任何脚本/测试改动。
- 未执行 `git reset --hard`、`git clean -fd`、amend/rebase、强推，也未建议任何人执行。
- 有写副作用的验证（runner 序列会重写 manifest/日志/补丁，全量 `npm test` 会经 R10-09 重写 round2 `patch.gz`）只在 `/tmp` 隔离副本执行；原仓仅回拷精确候选文件。
- 旧候选（33 路径）在改动前已逐文件备份到 `/tmp/r00-c1fix-r1/backup/`（34 个文件，含审查报告），可恢复；被取代的旧记录/日志/快照字节不以任何形式进入新候选。

## 2. 坐标与基线核对（本轮实测）

| 项 | 值 | 核对方式 |
|---|---|---|
| 当前 HEAD | `89c24455bbe9da6997a575bc9c731b56ddaddc78`（`docs(rust-tauri): record R00 stage acceptance`） | `git rev-parse HEAD`（本轮复算，与任务书一致） |
| R00 修复提交 | `9b2c7f11fc0fd9ece3954794acf90a213bac3c5a`（HEAD 的 parent） | 上一轮 `git cat-file -t` / `git log`，本轮未变动 |
| 本地 remote-tracking ref | `origin/codex/rust-tauri-migration` == HEAD | `git rev-parse`（在线 fetch 受网络限制未执行，见 §8） |
| 工作区状态（本报告交付时） | 恰含 §5.1 清单的 22 个候选路径（8 个已跟踪修改 + 14 个未跟踪），无其他漂移 | `git status --porcelain` |
| 旧审计坐标 | `46f12ab1c5bc00f02a685346a3efc1b473590393`（v0.1.43 收官树） | 本轮复算六处引用一致：`.sync-audit/verified-source-sha.txt`、`build-sync-matrix.mjs` 常量、`upstream-sync-matrix.json`、`UPSTREAM_SYNC_MATRIX.md`、`UPSTREAM_SYNC_AUDIT.md`、`PROGRESS.md` |
| C1 独立审查 R1 报告 | SHA-256 `02070406a6b670a22fbb8f0f0eddb5506d115bd2a9011e5c0b50c3d2cb4102b5`，与任务书指定值一致；结论 FAIL，三项修复要求见 §5.0 | `shasum -a 256`（本轮复算） |
| R00 阶段复验 R2 报告 | SHA-256 `4bf1363f027295e187f2c4d52c85071e255309aba94248495e6413ceee420bec`，与任务书指定值一致 | 上一轮 `shasum -a 256` |
| 依赖锁 | `package-lock.json` SHA-256 `e54a16fe14f15b4797069106392040924a5dd616c69a73bd025729090505ac8b`，与 R2 记录一致 | `shasum -a 256`（本轮复算） |
| 生产运行面零差异 | `git diff --name-only 46f12ab1..HEAD -- desktop/ server/ core/ lib/ shared/ cli/ hub/ plugins/ skills2set/ package.json package-lock.json` = 0 文件 | 上一轮复算，与 R2 §1 结论一致；本轮 `.sync-audit/`、`tests/`、`desktop/`、`server/`、`core/` 对 HEAD 零差异（脚本修复未动这些目录外的既有断言与产品面） |

`46f12ab1..HEAD` 共 753 个文件变化，分组：artifacts 569（R00 阶段证据为主）、docs 118、tests 28（R00 新增 migration 夹具/回放测试）、scripts 13（均为新增 `scripts/rust-tauri/r00-*` 测量工具）、任务书出库 17（`60dbe0384`）、`.sync-audit` 3 + `PROGRESS.md` + `UPSTREAM_SYNC_*` 2 + `.gitignore` + `eslint.config.js`（仅向 lint 范围追加 `docs/**/*.{js,mjs}` 工具脚本条目）。封印坐标链：`46f12ab1c`（证据重绑定）→ `c5dc940b9`（坐标推进）→ … → `60dbe0384`（任务书出库）→ R00 各任务提交 → `9b2c7f11` → `89c24455b`。

环境：macOS 27.0（Darwin arm64）；Node v24.16.0；npm 11.13.0；Python 3.14.3；vitest 4.1.10；git 2.53.0。

## 3. 四项旧 FAIL 逐项现状

复现（隔离副本 A = HEAD 原样物理克隆，上一轮 `/tmp/r00-audit-seal-exec/copyA`，HOME/TMPDIR/LINGXI_HOME 均指向 `/tmp`）：

- `npx vitest run tests/post-verification-audit-seal.test.ts tests/upstream-sync-matrix.test.ts tests/round2-delivery-evidence.test.ts tests/round3-delivery-evidence.test.ts` → **exit 1，4 failed / 21 passed（25 用例）**，日志 SHA-256 `1ffb749148c5751093e67905ca9b6fa379cdf03b3582e59ff031aceca74b23ab`。
- `node .sync-audit/verify-post-verification-diff.mjs` → **exit 1**（非白名单清单自 `.gitignore` 与 R00 增量起），日志 SHA-256 `6b1fa973db8ff037ce032d1d800b99071d533ad679ed34fbc629b1e1efaa3374`。

失败集合与 R00_REPORT §7、阶段修复 R1、阶段复验 R2 记录完全一致：

| # | 失败用例 | 根因（同一坐标家族） | 本候选设计下的转绿路径 | 现状 |
|---|---|---|---|---|
| 1 | `post-verification-audit-seal` ｢changes since VERIFIED_SOURCE_SHA are audit-only｣ | 旧坐标 `46f12ab1` 之后存在非白名单的 R00 阶段增量 | C1（证据重绑定）+ C2（坐标推进至 C1）两提交落地后，`git diff C1..C2` 仅 6 个白名单文件 → 转绿 | **仍 FAIL**（结构性：坐标未推进前必红，见 §4） |
| 2 | round2 `R10-03`（源码 manifest 可复算 + 绿色门禁） | 上项失败使 `manifestSourceRef` 回退路径中断；且冻结 manifest 仍绑定 v0.1.43 旧树 | C1 候选树上重建 manifest 后经 current-tree 路径转绿；C2 后经 `sourceRef=C1` 提交内容比对路径保持绿 | 候选工作区内已按 §5 重建（结果以 §7.2 记录为准） |
| 3 | round2 `R10-04`（manifest 覆盖与排除规则） | 同上 | 同上 | 同上 |
| 4 | round3 ｢源码 manifest 可复算，且覆盖本轮新增源码与测试｣ | 同上 | 同上 | 同上 |

未把任何一项历史 FAIL 改写为 PASS；未删测、未改预期、未扩白名单。

## 4. Seal 顺序与范围分析（候选/提交顺序）

**为什么必须拆成两个提交、且 VERIFIED_SOURCE_SHA 终值只能是 C1 而不是 89c24455b：**

1. 证据文件（`artifacts/f1-f12-repair/**`）**不在**审计 allowlist 内，且历史定式亦不允许入列（v0.1.39 条目原话：证据文件作为坐标之后的独立提交会被 diff guard 判为非审计改动）。round2/round3 manifest 当前绑定 v0.1.43 旧树，与 R00 收官树逐文件不同（753 文件差异），R10-03/R10-04/round3 三项仅靠推进坐标无法转绿，**证据重冻结不可避免**。
2. diff guard 要求 `VERIFIED..HEAD` 之间只允许白名单变化。若把证据重冻结放在 89c24455b 之后、坐标仍指 89c24455b，则该区间含证据文件 → guard 必红。两条门禁（guard 与 R10-03 家族）同时绿的唯一结构是：**证据先行提交 C1，坐标推进提交 C2 仅含白名单文件并令 `VERIFIED=C1`**。这与 v0.1.43 收官定式 `46f12ab1c`（证据重绑定）→ `c5dc940b9`（坐标推进）逐一同构。
3. 对任务书「把 VERIFIED_SOURCE_SHA 真实绑定到 89c24455b」的实现方式：**C1 的 parent 即 89c24455b，且 C1 相对 89c24455b 零生产代码差异**（仅证据重绑定产物、修复后的证据生成脚本与两份报告，见 §5）；验证证据（§7）绑定的源码树与 89c24455b 的生产树逐字节一致。R2 §5 的收口要求（「先把未提交修复固定为真实候选提交……再更新审计坐标/生成投影/完整复验」）已由总控前两提交（9b2c7f11、89c24455b）完成，本任务接续其坐标部分。
4. 备选顺序「先推坐标、后刷证据」（2026-09-22 refactor-2026 条目使用过的过渡窗定式）会在窗口内留下证据门禁红；本任务采用证据先行定式，窗口内仅 guard 单点结构性红（见 §7.2），红得更小且语义更直接。
5. C1/C2 均不得改写已推送历史（89c24455b 已与远端一致），禁止 amend/rebase。

## 5. C1 候选（证据重绑定 + 两份报告 + 修复后证据脚本；本任务已备好、未提交）

### 5.0 C1 独立审查 R1（FAIL）三项修复

审查报告 `docs/rust-tauri/R00/R00_AUDIT_SEAL_REVIEW_C1_R1.md`（SHA-256 `02070406a6b670a22fbb8f0f0eddb5506d115bd2a9011e5c0b50c3d2cb4102b5`）对旧 33 文件候选给出 FAIL 与三项阻塞 findings，本轮逐一修复：

- **F1（冻结树不完整）**：审查报告与修订后的本报告以**最终字节**纳入冻结候选，随后 round2/round3 的 manifest、`manifests/` 快照、`COMMAND_RESULTS.json` 记录、`logs/` 日志与两份 gzip 补丁在 `/tmp` 隔离副本按 §5.2 序列全部重冻结；候选路径从 6070 增至 6071（新增即审查报告）。本报告不嵌入自身或最终 manifest/补丁/快照/本轮日志的哈希，避免自引用循环；全部证据哈希以 `COMMAND_RESULTS.json` 记录为准（R10-02 与 round3 记录校验逐条可复算）。旧候选的 17+5 条 `r00-seal-*` 记录、22 份日志、4 份快照绑定的是旧树（补丁经旧脚本仅覆盖 5138 条），本轮整体取代：`COMMAND_RESULTS.json` 先回退到 HEAD 记录（round2 178 条 / round3 93 条）再追加本轮新记录；旧字节备份于 `/tmp/r00-c1fix-r1/backup/`。
- **F2（补丁重放未覆盖完整冻结 manifest）**：两份补丁生成脚本的临时 index 改为**以真实 index 的全部已跟踪条目逐项回放为起点**（`git ls-files -s -z` 经 `git update-index -z --index-info` 重建，该回放与真实 index 逐条一致已实测），再由 `git add -A` 纳入未忽略新文件；旧实现从 BASE `read-tree` 起步，相对 BASE 新增、已跟踪却匹配 `.gitignore` 的 932 个证据文件（`artifacts/refactor-2026/**` 719 条、`artifacts/rust-tauri/R00/**` 213 条）会被当作未跟踪忽略文件跳过。重放验证在原有「生成 index ↔ 重放 index」比较之外，**新增与完整冻结 `SOURCE_MANIFEST.json` 的逐项对盘**（路径 + 字节数 + SHA-256，任一不一致即非零退出），摘要新增 `frozenSourceManifestHash`、`frozenManifestEntries`、`replayedMatchesFrozenManifest` 三个字段。脚本字节 SHA-256（旧 → 新）：`create-delivery-patch.py` `37ad85a01573ed75d27ed6fa67c02ea7cb81e6b2578d0c21dd7535518016c6b9` → `e1a9b87057c753bfb21248a722cd87485a245502739acb1a370b984ac7200dc7`；`create-round3-patch.py` `2a3075004b48f2c1c885ce8c85bd35f744ce0a613255a5e388ffbf29aa648400` → `ec583eb2c0c33deb44cc1b2851f01225f8bfd1f521867d95825039fa466f95f8`。两份 `run-evidence.py` 零改动（round2 SHA-256 `576ad9902037f3dded9c1291c9b48ad594daa7f0f8d32ef4c70151256de45358`，round3 `a32aac6ca675889dd8b4729669470ac6127ce0730b120b9b421644b1fea1c453`）。
- **F3（回退方案违规）**：§10 重写——删除 `git reset --hard` 建议与「回退无损」误述，改为明确路径的可恢复保存与逐路径恢复，已提交时一律 `git revert` 追加撤销；不得出现 reset --hard / clean -fd / amend / rebase / 强推建议。

本轮不变项：生产运行代码零改动；测试断言零改动；两份审计 allowlist 零改动；审计坐标零改动（VERIFIED 仍为 `46f12ab1…`，待 C2）；R01 文件零改动；审查报告原字节零改动。

### 5.1 文件清单（相对仓库根，共 22 个路径）

**已跟踪修改（8）：**

1. `artifacts/f1-f12-repair/round2/create-delivery-patch.py`（F2 修复）
2. `artifacts/f1-f12-repair/round3-c01-c03/create-round3-patch.py`（F2 修复）
3. `artifacts/f1-f12-repair/round2/SOURCE_MANIFEST.json`（run-evidence 按最终 C1 树重冻结，6071 条）
4. `artifacts/f1-f12-repair/round2/COMMAND_RESULTS.json`（HEAD 178 条 + 本轮 `r00-seal-r1-*` 8 条）
5. `artifacts/f1-f12-repair/round2/patches/89bc0b64-to-r01-r10-source.patch.gz`（修复后脚本确定性重生成，gzip mtime=0）
6. `artifacts/f1-f12-repair/round3-c01-c03/SOURCE_MANIFEST.json`（同上，6071 条；与 round2 仅 `sourceIdentity.base` 不同）
7. `artifacts/f1-f12-repair/round3-c01-c03/COMMAND_RESULTS.json`（HEAD 93 条 + 本轮 `r00-seal-r1-*` 2 条）
8. `artifacts/f1-f12-repair/round3-c01-c03/patches/67dee5d2-to-round3-c01-c03.patch.gz`（同上重生成）

**新增未跟踪（14）：**

9. `docs/rust-tauri/R00/R00_AUDIT_SEAL_EXECUTION.md`（本报告，本轮修订）
10. `docs/rust-tauri/R00/R00_AUDIT_SEAL_REVIEW_C1_R1.md`（审查报告，字节冻结、本轮零改动）
11. `artifacts/f1-f12-repair/round2/manifests/<round2新哈希>.json`（runner 落盘的同内容快照）
12. `artifacts/f1-f12-repair/round3-c01-c03/manifests/<round3新哈希>.json`（同上）
13-20. `artifacts/f1-f12-repair/round2/logs/r00-seal-r1-{typecheck,contracts,boundary,lint,build-renderer,patch-r2,full,green}.log`（8 份 runner 脱敏日志：仓库根→`<WORKTREE>`，home→`<USER_HOME>`）
21-22. `artifacts/f1-f12-repair/round3-c01-c03/logs/r00-seal-r1-{patch-r3,green-r3}.log`（2 份）

**C1 不包含**：`.sync-audit/*`、`PROGRESS.md`、`UPSTREAM_SYNC_*`（属于 C2）；旧候选的 22 份 `r00-seal-*` 日志、4 份旧快照与被取代的 `COMMAND_RESULTS.json` 增补段（superseded，备份于 `/tmp`，不入库）。

### 5.2 生成命令序列（`/tmp` 隔离副本执行，顺序固定；run-evidence 每次调用先重冻结 manifest 再执行命令）

```bash
# 0) 前置：副本树 = HEAD + §5.1 全部输入最终字节（两份报告 + 两份修复脚本）；
#    两份 COMMAND_RESULTS.json 已回退到 HEAD 记录；旧 r00-seal-* 日志/快照已移出副本。
#    全程 HOME/TMPDIR/LINGXI_HOME 指向 /tmp 隔离路径；命令之间不得编辑任何被 manifest 覆盖的文件。
# 1) round2 静态门禁五条（首个调用即完成 round2 manifest 重冻结）
python3 artifacts/f1-f12-repair/round2/run-evidence.py r00-seal-r1-typecheck npm run typecheck
python3 artifacts/f1-f12-repair/round2/run-evidence.py r00-seal-r1-contracts npm run typecheck:core-contracts
python3 artifacts/f1-f12-repair/round2/run-evidence.py r00-seal-r1-boundary npm run lint:boundary
python3 artifacts/f1-f12-repair/round2/run-evidence.py r00-seal-r1-lint npm run lint
python3 artifacts/f1-f12-repair/round2/run-evidence.py r00-seal-r1-build-renderer npm run build:renderer
# 2) round3 补丁重放（完成 round3 manifest 重冻结；须为 round3 末条 patch 记录）
python3 artifacts/f1-f12-repair/round3-c01-c03/run-evidence.py r00-seal-r1-patch-r3 python3 artifacts/f1-f12-repair/round3-c01-c03/create-round3-patch.py
# 3) round2 补丁重放（同时建立 round2 同哈希绿记录）
python3 artifacts/f1-f12-repair/round2/run-evidence.py r00-seal-r1-patch-r2 python3 artifacts/f1-f12-repair/round2/create-delivery-patch.py
# 4) 全量（两份 manifest 均已冻结、同哈希绿记录均已存在）
python3 artifacts/f1-f12-repair/round2/run-evidence.py r00-seal-r1-full npm test
# 5) 双绿门禁（trio 不含 guard 文件）
python3 artifacts/f1-f12-repair/round2/run-evidence.py r00-seal-r1-green npx vitest run tests/round2-delivery-evidence.test.ts tests/round3-delivery-evidence.test.ts tests/upstream-sync-matrix.test.ts
python3 artifacts/f1-f12-repair/round3-c01-c03/run-evidence.py r00-seal-r1-green-r3 npx vitest run tests/round2-delivery-evidence.test.ts tests/round3-delivery-evidence.test.ts tests/upstream-sync-matrix.test.ts
```

约束：round3 末条 `create-round3-patch.py` 记录即本轮重放（round3 测试取末条）；补丁记录先于绿门禁与全量落地；标签一律使用本轮新族 `r00-seal-r1-*`（不复用旧候选标签；历史已有「标签复用覆盖日志」事故先例）；全部命令之间不得编辑任何被跟踪文件，否则 manifest 哈希漂移、绿色门禁记录失效。

建议提交信息（与 v0.1.43 定式同构）：`chore: R00 阶段证据随封印终态重绑定——门禁绑定 89c24455b 阶段收官树`。

## 6. C2 候选（坐标推进；模板，SHA 待 C1 提交后填入）

**文件清单（恰 6 个，全部在审计 allowlist 内，与 `c5dc940b9` 同集）：**

1. `.sync-audit/verified-source-sha.txt` ← C1 完整 SHA（单文件单行）
2. `.sync-audit/build-sync-matrix.mjs` 第 43 行 `VERIFIED_SOURCE_SHA` 常量 ← C1 完整 SHA
3. `.sync-audit/upstream-sync-matrix.json` ← `node .sync-audit/build-sync-matrix.mjs` 重生成（仅 `coordinates.VERIFIED_SOURCE_SHA` 变化）
4. `UPSTREAM_SYNC_MATRIX.md` ← 同一生成器重生成（仅坐标行变化；`Source-JSON-SHA256` 投影哈希只覆盖 summary+records，应保持 `8bf6c07a873c2b25c51db12ae36addb89cbfe33f23747fcdd3d32b4c7878dbc9` 不变）
5. `PROGRESS.md` ← 追加 §6.2 条目
6. `UPSTREAM_SYNC_AUDIT.md` ← 第 18 行坐标行按 §6.3 更新

**6.1 命令序列（总控在 C1 提交后执行）：**

```bash
C1=$(git rev-parse HEAD)   # C1 已落地、工作区 clean
printf '%s\n' "$C1" > .sync-audit/verified-source-sha.txt
# 编辑 .sync-audit/build-sync-matrix.mjs 第 43 行常量为 "$C1"（唯一允许的手改，与历史定式一致）
node .sync-audit/build-sync-matrix.mjs          # 重生成 JSON + MD（禁止手改产物）
# 按 6.2/6.3 模板更新 PROGRESS.md 与 UPSTREAM_SYNC_AUDIT.md，把 <C1_SHA> 替换为 "$C1"
node .sync-audit/build-sync-matrix.mjs --check  # 期望 exit 0
git add .sync-audit/verified-source-sha.txt .sync-audit/build-sync-matrix.mjs \
        .sync-audit/upstream-sync-matrix.json UPSTREAM_SYNC_MATRIX.md \
        PROGRESS.md UPSTREAM_SYNC_AUDIT.md
git commit -m "chore: 审计封印推进——VERIFIED_SOURCE_SHA → ${C1:0:9}（R00 阶段收官树）"
```

**6.2 PROGRESS.md 条目模板（追加至文件末尾；`<…>` 由总控按实际记录填入）：**

```
## 2026-09-24 R00 阶段收官树审计封印推进（rust-tauri 迁移 R00 收口，已获授权的提交流程）

- 坐标 `46f12ab1c5bc00f02a685346a3efc1b473590393` → `<C1_SHA>`（分支 codex/rust-tauri-migration：`89c24455b` R00 阶段验收记录 + `<C1短SHA>` R00 证据重绑定与封印执行报告提交；阶段独立复验 R2 PASS 后按 PROGRESS.md seal 工作流收口）。
- 验证绑定（证据提交内 `r00-seal-r1-*` 记录）：typecheck×3 绿；core-contracts 绿；lint:boundary 绿；eslint 0 error；build:renderer 绿；全量 npm test <总控填入实测> 唯一预期红 = post-verification diff guard ｢audit-only｣（坐标推进的结构性前置，本提交就位后转绿）；round2/round3 绿色门禁 manifest `<r2哈希前12位>`/`<r3哈希前12位>` 双绿、执行期无漂移；round2/round3 补丁重放 VERIFIED 且与完整冻结 manifest 逐项对盘一致（各 6071 条）。
- build-sync-matrix 常量随动，矩阵与 markdown 投影重生成（133 paths，projection sha256 `8bf6c07a…` 不变——投影哈希只覆盖 summary+records）。

### 2026-09-24 R00 收官树坐标后终态复验

- 坐标 `<C1_SHA>` 就位后，全量 `npm test`（含 post-verification-audit-seal / upstream-sync-matrix / round2 / round3 全部自指门禁）终态复验：<总控填入实测数字>，0 失败为通过标准。
```

**6.3 UPSTREAM_SYNC_AUDIT.md 第 18 行行模板：**

```
| VERIFIED_SOURCE_SHA | <C1_SHA> | 2026-09-24 R00 阶段收官树（89c24455b R00 阶段验收记录 + <C1短SHA> 证据重绑定/封印执行报告；验证证据见 PROGRESS.md 最新条目） |
```

## 7. 本轮验证证据

### 7.1 HEAD 现状复核（上一轮隔离副本 B，树 == 89c24455b 原样）

| 命令 | 结果 | 日志 SHA-256 |
|---|---|---|
| `npm run typecheck`（tsc ×3） | exit 0 | 合并日志 `9446aa31704433850b01d5ca213f9cbba8b404dd55d107b17dc8d600991e2526` |
| `npm run typecheck:core-contracts` | exit 0 | 同上 |
| `npm run lint:boundary` | exit 0 | 同上 |
| `npm run lint`（eslint .） | exit 0（警告为既有存量） | 同上 |
| `npm run build:renderer` | exit 0（vite ✓ built） | 同上 |
| `npm test`（全量） | **exit 1；1469 文件（1463 通过 / 3 失败 / 3 跳过）；14914 用例（14895 通过 / 4 失败 / 15 跳过）；90.64s** | `cabccdc1ef98eb8e2a05b153a0c925d29324a7fb2b41f5a23806f0f4cd412ee6` |

全量 4 失败逐项为：post-verification-audit-seal ｢audit-only｣、round2 R10-03、round2 R10-04、round3 ｢源码 manifest 可复算｣——与 §3 定点复现、T08 原仓候选记录（4 FAIL / 14895 PASS）完全一致，无新增、无环境类失败。原始日志存于上一轮 `/tmp/r00-audit-seal-exec/logs/`（不入库、不含真实用户路径）。本轮对 HEAD 未重复全量（树未变）；本轮证据全部绑定 §5 候选树。

### 7.2 C1 候选树证据（本轮隔离副本 `/tmp/r00-c1fix-r1/copyF`，树 == §5.1 最终字节）

判定标准：静态五条全部 exit 0；两条补丁记录 exit 0 且日志 JSON 含 `"result": "VERIFIED"`、`sourceManifestHash == replayedSourceManifestHash == frozenSourceManifestHash`、`frozenManifestEntries` 等于最终 manifest 条目数（6071）、`replayedMatchesFrozenManifest == true`；全量 exit 1 且**恰好 1 个失败用例**（post-verification guard ｢audit-only｣，坐标未推进的结构性前置红；R10-03/R10-04/round3 在 C1 树经 current-tree 路径转绿）；双绿门禁 exit 0 且全记录 `sourceManifestHash == endSourceManifestHash`、`sourceChangedDuringCommand == false`。出现任何其他确定性失败即候选无效，须回到 §4 重新分析；既有平台抖动家族（speech SIGTERM 回收竞态等）按惯例单跑甄别、如实登记并存保留记录。

**本轮实际执行的 exit 码、计数与全部哈希以两份 `COMMAND_RESULTS.json` 末批 `r00-seal-r1-*` 记录为准**（每条含日志 SHA-256、首尾 manifest 哈希与漂移标记，R10-02/round3 记录校验逐条可复算）；本报告不嵌入最终 manifest/补丁/日志哈希以避免自引用循环（F1）。

方法彩排（报告 v2 字节树 + 修复后脚本，先于定稿轮）：两份补丁脚本直接运行均 exit 0 VERIFIED，`frozenManifestEntries=6071`，`replayedMatchesFrozenManifest=true`；gzip 体积 round2 约 29.6 MB、round3 约 86.8 MB，均低于 GitHub 单文件 100 MB 硬限（定稿轮精确字节数与 SHA-256 以补丁记录为准）。独立对盘方式：审查要求的「从各自 BASE 独立重放并与完整冻结 manifest 逐项对盘」已由脚本内置校验执行，并由本任务在隔离副本以独立脚本复核（逐路径、字节数、SHA-256 全等），结果见本任务终报。

### 7.3 C2' 预演（上一轮隔离副本 D，仅对象库层面的临时提交对象；本轮未重做）

上一轮方法：在副本内用 `git write-tree` / `git commit-tree` 生成临时提交对象 C1'（parent=89c24455b，内容=旧 C1 候选）与 C2'（parent=C1'，内容=§6 六文件，其中坐标填 C1'），再将副本分支指针移至 C2' 复跑门禁：guard exit 0、`build-sync-matrix.mjs --check` exit 0、封印四文件 25/25 绿、全量 0 失败。**该预演只在 `/tmp` 副本的对象库中产生对象，不改本仓任何 ref；预演 SHA 与总控实际提交 SHA 必然不同，仅验证机制与文件集自洽。** 本轮未重做 C2'：C2 的文件集与生成器路径未变（脚本修复只影响证据生成，不触碰六白名单文件与矩阵生成器），正式结论以总控 postcommit 复验（§9）为准。

## 8. 局限与未执行项

- 平台：仅 macOS 27.0 arm64 实测；Windows/Linux/Intel 未运行；真实 Windows 安装/NSIS 交互沿用既有 Known limitation。
- 未执行：`npm run pack`、正式签名、公证、发布、真实供应商 live、真实用户数据迁移、长时（2h/1000 任务）测试。
- 网络受限：`git fetch` 因本机代理端口不可达未执行；远端一致性以任务书声明 + 本地 remote-tracking ref（== HEAD）为准，postcommit 推送前由总控在线复核。
- 结构性预期红：C1 时点全量含且仅含 post-verification guard ｢audit-only｣1 红，C2 提交后转绿（§7.3 机制预演 + §9 postcommit 复验双重确认）；在此之前的任何树态下不得声称全量 PASS。
- R10-09 运行时派生：全量运行会按当时树确定性再生成 round2 `patch.gz`；同一树字节一致、无漂移；C2 之后该文件在工作区的本地漂移属既有机制（v0.1.42 条目已登记），不随审计提交。
- `DELIVERY_MANIFEST.sha256`（round2/round3）为历史交付快照，HEAD 上已不与现行 `COMMAND_RESULTS.json`/`SOURCE_MANIFEST.json` 逐字节匹配（v0.1.43 证据重绑定未同步它），无任何测试引用；本轮沿用先例不更新。C1 不声称它们是本轮最新交付索引。
- 隔离副本环境差异：R1 曾观测副本特有抖动（ustar 临时目录 ENOTEMPTY、worker fork 崩溃）；上一轮副本 C 首轮出现 1 次 speech SIGTERM 计时抖动（单跑复绿）；若本轮或后续复验出现，按既有惯例单跑甄别、如实登记。
- 审计封印不覆盖未提交/未跟踪文件（AGENTS.md 既有说明）；本报告交付时工作区恰含 §5.1 清单的候选改动，无其他漂移。
- 旧候选证据（旧 33 文件树及其记录）已被本轮取代，不作为任何现行结论的依据；其字节仅存于 `/tmp/r00-c1fix-r1/backup/` 供追溯。

## 9. Postcommit 必须执行的检验（总控 / 独立验收代理）

**C1 提交后、C2 之前：**

1. `git log -1 --format=%H` 确认 C1；`git diff --stat 89c24455b..C1` 应仅含 §5.1 清单的 22 个路径（8 修改 + 14 新增），生产运行面零差异；两份修复脚本的提交字节应等于 §5.0 记录的新 SHA-256。
2. 只读复算 manifest 与树一致（注意 R10-09 所在文件会确定性再生成 round2 `patch.gz`：同一树字节一致、无漂移）。
3. 独立验收代理按本报告 §3/§7 复核证据链（R10-02 逐记录日志哈希、`manifests/` 快照、两份补丁摘要中 `frozenManifestEntries=6071` 且 `replayedMatchesFrozenManifest=true`）；并按审查 §4 矩阵自行从各自 BASE 独立重放对盘。

**C2 提交后：**

4. `node .sync-audit/verify-post-verification-diff.mjs` → exit 0（diff C1..C2 恰 6 个白名单文件）。
5. `node .sync-audit/build-sync-matrix.mjs --check` → exit 0。
6. `npx vitest run tests/post-verification-audit-seal.test.ts tests/upstream-sync-matrix.test.ts tests/round2-delivery-evidence.test.ts tests/round3-delivery-evidence.test.ts` → 25/25 绿。
7. `npm test` 全量终态复验 → **0 失败为通过标准**；既有平台抖动家族（ustar ENOTEMPTY、worker fork、speech SIGTERM 回收竞态等）按惯例单跑甄别登记。
8. `git diff --check`；`git status --porcelain` 除 round2 `patch.gz` 可能的运行时再生漂移外应为空。
9. 远端推送后核对远端 ref == C2；封印终态以推送后的真实提交 SHA 为准再登记。

## 10. 回退方案

- **未提交的候选改动**：先把候选精确路径清单与逐文件副本保存到仓库外明确路径（本轮实际备份：`/tmp/r00-c1fix-r1/backup/`，含被取代的旧候选字节）；确需把工作区恢复到候选前状态时，只对清单内已跟踪路径执行 `git restore -- <paths>`、对清单内未跟踪路径逐个 `rm`，或使用可恢复的 `git stash push -u -- <paths>`（stash 条目可随时 `git stash pop` / `git stash apply` 找回，删除条目前先核对备份在位）。不执行也不建议 `git reset --hard`、`git clean -fd` 或任何批量不可逆命令；回退是否造成损失取决于候选文件是否已按前款保存，不作「无损」断言。
- **C1/C2 已提交（无论是否推送）**：以 `git revert` 追加撤销提交，先撤 C2 再撤 C1，保持历史只追加、不重写；坐标文件随 C2 的 revert 回到 `46f12ab1`，证据文件字节随 C1 的 revert 回滚，原提交仍保留在 git 对象库；完成后重新核对 `git diff` 结果与六处坐标引用一致。禁止 amend/rebase/强推。
- **C2 后门禁仍红**：禁止以扩白名单/退役门禁/改断言压平；按 seal 工作流如实登记失败，定位后以新的证据重绑定提交推进。
- **候选被判无效**：按本条第一款处理（备份 → 逐路径恢复），工作区回到 89c24455b 候选前状态，无任何已提交影响。

## 11. 纪律对照声明

| 禁令 | 执行情况 |
|---|---|
| 不虚报坐标 | VERIFIED 终值 = C1 的论证见 §4；报告不以任何方式把 89c24455b 写成已验证坐标 |
| 不扩大两份 allowlist | 两处 allowlist 本轮零改动（`git diff` 可证） |
| 不删除/退役门禁、不放宽断言、不以 skip 当 PASS | 三个门禁文件零改动；全部判定以 exit code 与真实通过数为准 |
| 不手改生成投影 | 矩阵 JSON/MD 由 `build-sync-matrix.mjs` 产出；C2 复验含 `--check` |
| 不用「仅改指针」求绿 | 证据重绑定先行、坐标推进殿后，R10-03 家族经 manifest 重建真实转绿 |
| 不把历史测试说成当前提交后实跑 | 本轮实跑均落 `r00-seal-r1-*` 记录；§7.1/§7.3 为上一轮记录并注明出处 |
| 不改生产代码 / 测试断言 / R01 文件 | 候选改动全集见 §5.1，生产运行面零差异（§2 复核） |
| 不用排除规则/测试改动掩盖覆盖缺口 | F2 经脚本修复真实纳入 932 个已跟踪忽略证据文件并与完整 manifest 逐项对盘；排除规则与测试零改动 |
| 审查报告字节保持不变 | `R00_AUDIT_SEAL_REVIEW_C1_R1.md` SHA-256 恒为 `02070406…4102b5` |
| 保留现有候选与用户改动 | 旧候选 34 文件逐字节备份于 `/tmp/r00-c1fix-r1/backup/`；任务开始时工作区除旧候选外无其他改动，无他人改动被覆盖 |
