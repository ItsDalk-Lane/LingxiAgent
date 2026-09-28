# R02 A16 / R7-B01 隔离 Git 对照

状态：**PASS（仅此隔离夹具）**。这不是 A16、全量 npm、完整 verify 或 R02 阶段的通过记录。

## 边界与依据

- 只读主仓库：`/Users/study_superior/Desktop/Code/LingxiAgent`；其 HEAD 在本次对照中为 `9b98c679678888b60ff2e32bd67d3da6fc2f5c4f`。只对主仓库执行了 `git rev-parse HEAD`，没有对它提交、切换、清理或回退。
- 本次绑定的 A16 脚本：`scripts/rust-tauri/r02_t08_legacy_entry_regression.sh`，SHA-256 `7b7c4a4dd400d6fdfc672715e5c073ed85df93405e93cbdf05eef779b8ee3c68`。该脚本当前 E0b（约 314–348 行）从 Git 对象库在空工作树中构造历史基线，并要求 HEAD、diff、index、完整 status 均干净；本对照在小仓库中复现同一构造，未调用整个 A16。
- 最终夹具目录：`/var/folders/5r/wq54gtms21gfjgr5v94ynfyh0000gn/T/r02-a16-git-fixture-pe9agblc`。模拟提交、`checkout -f` 和 `git clean -fd` 只发生在此目录的独立仓库/副本。临时目录保留供复核。
- [命令流水](attempt-2/command-trace.jsonl)逐条列出 UTC 开始/结束、工作目录、参数、退出码及各自 stdout/stderr 原始日志与 SHA-256。最终 19 条命令均 exit 0。夹具程序本身于 `2026-09-28T11:30:08.515+00:00` 至 `11:30:08.693+00:00` 执行、exit 0；[结果 JSON](attempt-2/fixture-result.json)保留 SHA、状态和结论。执行程序 SHA-256 `385138aa2a215c6ee4b9545a58ffd6c2b5eb7ec5077a5b1bf14f2c4ddfdbb812`。

## 观察结果

1. 候选先有两次真实模拟提交：基线 `45f62b5bbcec602d480a87d507fb59150ccc424a`，候选 `1b1cb48a872ffc217c4f005c4989d7c05acd0d69`；随后在候选添加未提交的已跟踪修改和未跟踪 `src/new_candidate.rs`。Git status 两条身份分别为 ` M src/tracked.rs` 和 `?? src/new_candidate.rs`，候选两文件 SHA-256 见结果 JSON。
2. 旧的“整体复制脏候选，再 `git checkout -f` 历史提交”确实把已跟踪修改覆盖，但未跟踪文件仍留在旧基线副本，字节摘要前后同为 `c9f99cbdfab033a8b10290f9028d0328a521b2eaf2ba4202fe27c7ffc0c93a10`；旧副本 status 仍有 `?? src/new_candidate.rs`（第 10–11 条命令）。这复现 R4-F02 的基线污染机制。
3. 在**同一个隔离旧副本**上执行 `git clean -fd` 后，日志原文为 `Removing src/new_candidate.rs`，status 为空（第 12–13 条命令）。这说明依赖强制清理确实会删除候选文件，不能用于用户主工作区。
4. 现行方式仅复制该隔离候选仓库的 `.git`，删除副本自己的旧 index，在空工作树用普通 `git checkout --detach` 检出基线。HEAD 等于基线提交，`git diff --quiet BASE` 和 index diff 均 exit 0，完整 status 为空，候选未跟踪文件不存在，历史文件内容与 `git show BASE:path` 相同（第 14–19 条命令）。

## 首次尝试与限制

第一次夹具运行于 `2026-09-28T11:29:42.698+00:00` 开始，程序 exit 1，停止在第 9 条 Git 命令后：夹具读取 `git status --porcelain` 时使用 `.strip()`，错误去掉了表示已修改文件身份的前导空格，内部预期比较失败；原提示为“候选夹具缺少预期的已修改与未跟踪路径”。首次 [命令流水](command-trace.jsonl)、第 1–9 条 stdout/stderr 原件和第一次临时仓库路径保留。第二次在全新证据根与全新隔离仓库改用仅移除结尾换行的 `.rstrip("\\n")`，没有覆盖首次证据。第一次程序级 stdout/stderr 当时未额外重定向为文件；该缺口照实记录，不补造原始文件。

本夹具只检验小仓库的基线构造和污染机制。它没有运行真实 A16、全量 npm、封印族、真实服务或任何 TCP 验收；A16 和 R02 当前候选仍须按阶段合同另行完整验证。
