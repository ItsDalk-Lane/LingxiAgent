# R00 审计封印 C1 独立审查 R1

**VERDICT: FAIL**

此结论只针对当前未提交的 C1 证据重绑定候选及拟议的 C1→C2 顺序，不改变 R00 阶段 R2 的 PASS，也不代表正式封印已通过。当前候选有两个会影响证据真实性的阻塞项，执行报告另有一处违反总控禁令的回退建议。修复由新的 ZCode 任务承担；本审查没有修改候选、提交、推送或推进审计坐标。

## 1. 坐标、边界及已成立的证据

- 审查时分支为 `codex/rust-tauri-migration`，HEAD、`origin/codex/rust-tauri-migration` 本地引用及在线 `git ls-remote` 均为 `89c24455bbe9da6997a575bc9c731b56ddaddc78`。其父提交为阶段修复候选 `9b2c7f11fc0fd9ece3954794acf90a213bac3c5a`。旧 VERIFIED 仍为 `46f12ab1c5bc00f02a685346a3efc1b473590393`，尚未推进。执行报告字节 SHA-256 为 `572e956504d4d2221db5c50c0cc7c1bb0ac610b9821486516b6608d766bfa84a`。
- 写入本审查报告之前，工作区恰有 **33 个候选路径**：6 个已跟踪文件修改（两份 `COMMAND_RESULTS.json`、两份 `SOURCE_MANIFEST.json`、两份 `.patch.gz`）和 27 个未跟踪文件（22 份 `r00-seal-*.log`、4 份 manifest 快照、1 份 ZCode 执行报告）。22 份本轮日志均未被 Git 忽略；没有候选文件超过 100 MB，最大为 round3 gzip 的 79,434,138 字节。两份 allowlist、产品/测试/审计坐标文件均未进入原 33 文件候选。`git diff --check` 通过。
- 本轮对原 33 文件树独立复算：round2、round3 `SOURCE_MANIFEST.json` 各有 6070 条，路径、字节数及 SHA-256 与 `git ls-files --cached --others --exclude-standard` 按既定排除规则得到的当前树逐项一致；哈希分别为 `d373eb24994b1c4f80d9458566549845fe53bd7215a54d7db317bd03bbe81672`、`f212417a73578f2797e666432f96fb2493e380baef0f60aa1e849d15f049df04`。两份 `COMMAND_RESULTS.json` 每条日志 SHA、每条快照文件名/内容 SHA 均复算一致，本轮日志的 `Tests` 计数与记录一致；各定稿记录的首尾 manifest 哈希相等、漂移标记为 `false`。候选日志无真实用户 home 路径、常见 GitHub token 或私钥头；匹配到的 `sk-…` 来自测试文件名 `task-…` 的中间片段，未见凭证形态。
- 历史 HEAD 全量为 14914 用例中 14895 过、4 失败、15 跳过；四失败是 diff guard、round2 R10-03/R10-04、round3 源码 manifest。C1 定稿日志 `r00-seal-full-r2c.log` 为 14898 过、**仅 diff guard 1 失败**、15 跳过，exit 1，绝非全绿；两份定稿绿色门禁各 22/22，补丁命令记录各 exit 0。审查者另在原 33 文件树只读重跑 round2/round3 关键证据用例，7 通过、8 未选中；`build-sync-matrix.mjs --check` exit 0；独立 diff guard exit 1，符合旧坐标现状。
- C2 的六文件设想与两处现有 allowlist 一致：`.sync-audit/verified-source-sha.txt`、`.sync-audit/build-sync-matrix.mjs`、`.sync-audit/upstream-sync-matrix.json`、`UPSTREAM_SYNC_MATRIX.md`、`PROGRESS.md`、`UPSTREAM_SYNC_AUDIT.md`。候选 C1 包含不在 allowlist 的证据，故 VERIFIED 必须指向**包含完整证据的真实 C1 提交 SHA**，不能指向 `89c24455b` 或 `/tmp` 彩排 SHA。矩阵 JSON/Markdown 应由生成器重建并以 `--check` 核对。现有 `/tmp` C1′→C2′ 彩排的 guard、四文件 25/25、全量 14899/14899 通过，只证明机制可行；`C1′/C2′` 均非正式提交，彩排后 round2 patch 还出现本地重生漂移，不能代替真实 postcommit 复验。

## 2. 阻塞 findings

### F1｜本审查报告加入仓库后，原 C1 manifest 与补丁不再对应完整候选

**证据与原因：**原 33 文件树的两份 manifest 均列 6070 个源码路径；`run-evidence.py` 和证据测试只排除 `artifacts/f1-f12-repair/**` 的非 `.py` 文件、`*.patch` 及 Git 忽略文件。本报告必须写到 `docs/rust-tauri/R00/R00_AUDIT_SEAL_REVIEW_C1_R1.md`，不属于这些排除项；它写入后成为第 6071 个待覆盖路径。原定稿 manifest、快照、同哈希绿色记录及 gzip 补丁是在它不存在时生成的。Git diff guard 只检查已提交差异，不能替这项源码覆盖校验兜底。

**后果与同类范围：**若本报告进入 C1，C1 的两份 manifest 少一项；C2 即使只改六份白名单文件，round2 R10-03/R10-04 与 round3 manifest 仍会在回退 C1 路径上失败。若本报告不进入 C1 而在 C2 后另行提交，它又是不在 allowlist 的新变更，会使封印重新变红。以后每新增或改写一份非排除的仓库审查报告，同类问题都会重现。

**ZCode 修复要求：**先固定本报告及拟纳入 C1 的全部文档最终字节和精确文件清单，然后在隔离副本按原 runner 顺序重新冻结 round2/round3 manifest、对应快照、日志与补丁；以完整新树独立复算，不沿用本报告中的旧哈希。后续独立验收对该冻结树只读检查；验收结论可在总控会话中独立报告。若还要新增一份仓库内 R2 报告，须在**最后一次**冻结前先把其最终字节纳入候选，此后不可再编辑；若验收失败，则如实 FAIL 并重新循环一次，不预填 PASS。报告不嵌入自身或最终 manifest 的哈希，避免文本更新造成自引用循环。不得用扩大排除规则、改测试或扩大审计 allowlist 来掩盖。

### F2｜两份 gzip 补丁的“VERIFIED”没有覆盖冻结 manifest 中的 932 个已跟踪文件

**证据与原因：**审查者用独立临时 Git index、按两份 `create-*-patch.py` 的现行 `stage_current`/`manifest_from_index` 规则复算：两份冻结 manifest 各为 6070 条；两份补丁的临时 index manifest 各只有 **5138 条**，均缺 **932 条**。缺项都在已跟踪的 `artifacts/refactor-2026/**`（719 条）或 `artifacts/rust-tauri/R00/**`（213 条），其中包括 R00 阶段原始日志；没有产品源码或测试文件缺项。脚本先 `git read-tree BASE`，再 `git add -A`；这些相对旧 BASE 新增、当前虽已跟踪却受 `.gitignore` 匹配的文件不会被这个临时 index 加入。round2 最新补丁摘要日志的 `sourceManifestHash` 为 `9f53f280…`，不等于冻结 manifest `d373eb24…`；round3 为 `77b1cef7…`，不等于 `f212417a…`。日志里的 VERIFIED 只比较临时 index 与它自己的重放 index，未与完整冻结 manifest 对盘。

**后果与同类范围：**gzip 文件本身的 SHA、gzip mtime=0、记录中的补丁字节数均复算正确；round2 为 25,402,720 字节、round3 为 79,434,138 字节，均未触及 100 MB 限制。但其重放没有证明能恢复被声明覆盖的完整源码/证据树；报告 §7 的“补丁重放 VERIFIED”应限定为脚本内部较窄结果。任何在 BASE 后加入、后又被忽略规则匹配的已跟踪文件都可能同样漏入临时 index。

**ZCode 修复要求：**在两份补丁生成脚本内以当前已跟踪树为临时 index 起点，保留其中受忽略规则匹配的已跟踪文件，再纳入本轮未忽略的新文件；或采用等效的显式路径加入法。补丁可继续排除输出 gzip 本身以防自嵌套。重放验证必须把补丁重放树按同一排除规则与完整冻结 `SOURCE_MANIFEST.json` 的路径及内容逐项对盘，不能只比较生成 index 与重放 index。修复后重生两份 gzip、日志/快照/manifest，检查文件大小，并在独立隔离副本从各自 BASE 重放核对 6071（或最终实际数）条。此项属于证据生成完整性修复；不触碰产品运行代码、测试断言、两处 allowlist 或 VERIFIED 坐标。

### F3｜执行报告 §10 的未推送回退命令违反总控明令禁用的操作

**证据与原因：**`R00_AUDIT_SEAL_EXECUTION.md` §10 建议在 C1/C2 未推送时执行 `git reset --hard 89c24455b`，并称“回退无损”；同段又建议丢弃工作区候选改动。原总控已明确禁止 `git reset --hard`、`git clean -fd`、重写历史及强推。该命令还会抹掉届时工作区可能新增的用户改动，“无损”断言没有依据。

**后果与同类范围：**即使该命令尚未执行，把它留在将被提交的执行说明中也会向后续操作者提供越权步骤。审查报告与候选证据的清理同样应逐路径、可恢复地处理。

**ZCode 修复要求：**修订 §10：未提交候选先保存明确路径清单与副本，必要时对明确路径使用可恢复的 `git stash push -u -- <paths>`；若 C1/C2 已提交，无论推送与否都以 `git revert` 追加撤销提交，先撤 C2 再撤 C1，并重新核验差异与坐标。不得出现 `reset --hard`、`clean -fd`、amend/rebase 或强推建议。修订后按 F1 重新冻结报告字节。

## 3. 非阻塞但必须如实标注的边界

- 两份 `DELIVERY_MANIFEST.sha256` 的记录仍指向历史交付快照，当前 `COMMAND_RESULTS.json`、`SOURCE_MANIFEST.json` 已与其中记录不符。现有证据测试没有把它们当现行清单读取；只要执行报告明确标成历史、C1 不声称它们是本轮最新交付索引，这一项本身不作为拦截 C1 的新增 blocker。后续若要把它们宣传成现行交付清单，必须重新生成并独立核验。
- `round2` 的 R10-09 测试会重写 `.patch.gz`；C2 彩排全量通过后也留下这一文件的未提交漂移。因此正式提交后全量复验必须在 `/tmp` 隔离副本做，或明确复验前后精确字节差异；不得把测试产生的 patch 漂移混入 C2，也不得把带漂移的工作区称为 clean。
- 审查只核实本地/macOS 证据与在线远端 ref；未运行真实用户数据、供应商、付费接口、Windows/Linux/Intel、正式打包、签名、公证或发布。`/tmp` 彩排 SHA 仅供机制检查。

## 4. 修复后重跑矩阵与交接条件

| 阶段 | 最低检查 | 放行标准 |
|---|---|---|
| 重新冻结前 | 固定 C1 文件清单，包含本审查报告及修订后的执行报告；对照 HEAD 与远端、两处 allowlist | C1 仅证据/报告，所有新日志显式纳入，所有文件小于 100 MB；无产品、测试、坐标改动 |
| 两份证据重建 | 逐项重算 `SOURCE_MANIFEST` 路径、字节、SHA，检查所有 `COMMAND_RESULTS` 日志/快照 SHA 与计数；两份补丁由各自 BASE 在独立 `/tmp` index 重放 | manifest 覆盖最终 C1 候选的完整路径集；补丁重放路径和内容也覆盖同一集合；gzip SHA 与记录一致 |
| C1 候选隔离验收 | 静态门禁、两份补丁重放、round2/round3/矩阵定点，以及全量 `npm test` | 静态与证据门禁绿；全量只允许旧 VERIFIED 导致的 diff guard **1 个失败**，其余 14913 项按实际结果记录，不能预设绝对计数 |
| 真实 C1 提交后 | 核对 `89c24455b..C1` 的每一路径及 C1 完整 SHA；再次只读比对冻结 manifest 与 C1 已提交内容 | 无候选漏提交、无范围外文件；C1 是真正的被验证证据树 |
| 真实 C2 提交后 | 六文件坐标/生成投影一致性、`build-sync-matrix.mjs --check`、独立 diff guard、四文件证据门禁、隔离副本全量 `npm test`、提交差异/远端 ref | VERIFIED 恰为真实 C1 SHA；`C1..C2` 恰六个 allowlist 文件；guard/matrix/25 项证据及全量 0 失败；推送后远端指向真实 C2，再记录终态 |

当前 **FAIL** 不许可提交原 33 文件候选或把 C2 彩排当正式封印。修复、重新独立验收后，C1 提交、ZCode 按真实 C1 SHA 填 C2、总控执行已获授权的提交/推送、postcommit 全量复验仍是强制步骤；任何阶段都不得虚报坐标、扩 allowlist、退役门禁、重写历史或强推。
