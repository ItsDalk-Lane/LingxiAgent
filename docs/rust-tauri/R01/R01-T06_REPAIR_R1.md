# R01-T06 验收 F1–F4 修复报告（REPAIR_R1）

- 修复身份：ZCode:R01-T06-repair-r1（全新修复代理，未参与此前执行/验收）
- 基线 HEAD：`76bd42c439e180163b150a0058bd6c9e4875c940`；分支 `codex/rust-tauri-migration`；日期 2026-09-25
- 依据：`docs/rust-tauri/R01/R01-T06_REVIEW_R1.md` §11（F1–F4 原文）；未 commit/push；未改任务书目录、`.sync-audit`、`ORCHESTRATOR_PROGRESS.json`（其工作区 diff 为总控派发记录，本代理未触碰）、既往验收报告原文、生产代码
- 新证据根：`artifacts/rust-tauri/R01/T06/repair-r1/`（SHA256SUMS-repair-r1.txt 114 项全量校验通过）

## F1（低，文档失实指控）— 复核结论：指控不成立，矩阵本就正确，未做改动

验收报告称 SHELL_CAPABILITY_MATRIX.json screen 项 "4304/4312 capturePage、2561/2577 getDisplayNearestPoint" 两处行号写反（声称实际 4304/4312=getDisplayNearestPoint、2561/2577=capturePage）。

以真实源码逐行核对（desktop/main.cjs 自基线 HEAD 零改动，`git diff HEAD --stat` 仅 3 个 docs 文件；main.cjs SHA-256=533d468df2b02e95f8f11d5c3f965a3af2afc8e15a38caa9c1e532b5af17a319）：

```
$ awk 'NR==2561||NR==2577||NR==4304||NR==4312{printf "%d: %s\n", NR, $0}' desktop/main.cjs
2561:   const display = screen.getDisplayNearestPoint(screen.getCursorScreenPoint());
2577:   const display = screen.getDisplayNearestPoint(screen.getCursorScreenPoint());
4304:         const img = await wc.capturePage();
4312:         const img = await wc.capturePage()
```

实际 **4304/4312 = capturePage、2561/2577 = getDisplayNearestPoint**，与矩阵现文完全一致。验收方 §7 表格的"实测"列把两者读反了。为迎合误读而"更正"会引入真实错误，故 F1 的正确修复是**不改矩阵、以源码证据否证该指控**。

同类条目顺带全量复核（matrix `electron_current` 全部 main.cjs 行号引用，逐一 awk 核对）：tray 2151 `createTray` / 2164 `new Tray` ✓；shortcut 210-266 globalShortcut 注册/冲突（229/239/240/263/266 命中）✓；notification 1577 `new Notification` ✓；file-dialog 5745/5758 `showOpenDialog`、1776 `showMessageBox` ✓；proxy 296 `setProxy direct` / 305 `setProxy system` ✓；window 2889 `sanitizeWindowState` / 2600 `getAllDisplays` ✓；speech 5449-5460 `speech-permission-status/request` ✓。另 `capturePage` 还有 5167/5177 两处（offscreen 长截图），矩阵未引用，无冲突。**无同类错误。**

## F2（低，文档失实）— 已修复：18 步 → 15 步

根因：PLATFORM_BUILD_MATRIX.json `desktop_prototype_r01_t06` 段凭印象写 "runner-summary.txt 18 步 exit=0"；run_e2e.sh 实际 `record()` 调用 15 处（13 处行首 + relaunch-probe 经 `&&` + runBundle-page-reports 缩进），summary 含 step0/complete 两行共 17 行，无任何口径等于 18；执行报告 §2 自报 15 步一致。

修复：改为 "（runner-summary.txt 15 步 exit=0）"。重跑两轮实测均 15 步（repair-r1 realpath 轮与 symlink 轮 runner-summary.txt `awk` 计数 steps:15）。

## F3（低，文档失实）— 已修复：4 处 "自带/独立 [workspace]" 措辞更正为事实表述

根因：实测 `grep -c '^\[workspace\]'` 两个 spike manifest（app/src-tauri/Cargo.toml、sidecar/Cargo.toml）均为 0，仓库根及 spike 祖先目录均无 Cargo.toml；隔离实效靠"无祖先 workspace 可归入"成立，原"自带 [workspace]"字面失实。

更正为事实表述（统一口径"无显式 [workspace] 段、无祖先 workspace 可归入，隔离实效成立"）：

| 文件 | 位置 |
|---|---|
| docs/rust-tauri/R01/SHELL_CAPABILITY_MATRIX.json | spike.isolation |
| docs/rust-tauri/R01/TAURI_SPIKE_REPORT.md | 头部 Spike 工程行 |
| docs/rust-tauri/R01/R01-T06_REPORT.md | §4 红线合规自查 |
| docs/rust-tauri/R01/DEPENDENCY_DECISIONS.md | D-11 性能/安全/兼容影响段 |

改后 grep 确认 4 处失实措辞零残留（R01-T01_* 与 R01-T06_REVIEW_R1.md 中的 "[workspace]" 字样属其他任务上下文/既往验收原文，不在本任务范围，未动）。JSON 语法复验通过。

## F4（低，runner 门禁弱点）— 已修复：恢复语义断言 + 符号链接路径 realpath 处理

根因：runA-sidecar-crash-recovery 步只记 probe_webdriver.mjs 的退出码，而 probe 的 sidecar-recovery 子命令语义是"捕获到 drill 输出即 exit 0"，不校验 drill 内容；验收 §3.1 实测 /tmp 符号链接路径下 restart 失败（`StartingBinary found current_exe() that contains a symlink`，报告 R2）仍记 exit=0 假绿。

修复（spike/tauri-shell/scripts/run_e2e.sh，两处）：

1. **入口 realpath**：证据目录在 mkdir 后统一 `pwd -P` realpath；若入参经符号链接（如 /tmp/...），stdout 与 runner-summary.txt 均显式记录 `note: evidence dir realpathed ...`，不静默。runner 内 app 二进制从 `$EVID/build/` 运行，EVID realpath 后 current_exe() 不再含符号链接，R2 类失败在符号链接工作目录下被正确处理而非假绿。
2. **恢复语义断言**：crash-recovery 步在 probe 之后用 python 内联断言直接校验 sidecar-recovery.json 语义——`status_after_kill.alive==false`、`restart.restarted==true`、`ping_after_restart` 为真实 pong 且 `seq>=2`（递增）、`final_status.alive==true 且 restarts==1`；JSON 缺失/损坏亦判失败。断言失败时该步 exit 非零（probe exit=0 也救不回来）。步数不变（仍 15），既有负向用例零删除零弱化。

### F4 重跑验证（4 组证据）

| 验证 | 命令 | 结果 | 证据 |
|---|---|---|---|
| realpath 轮全量 | `zsh spike/tauri-shell/scripts/run_e2e.sh <repo>/artifacts/rust-tauri/R01/T06/repair-r1/run-e2e-repair-r1-realpath` | runner exit=0；15/15 步 exit=0；crash-recovery 步 `RECOVERY-ASSERT OK: alive=false -> restarted=true -> pong seq>=2 -> restarts=1` | run-e2e-repair-r1-realpath/ |
| symlink 轮全量 | `zsh spike/tauri-shell/scripts/run_e2e.sh /tmp/t06-repair-r1-symlink/evidence`（/tmp→/private/tmp 符号链接入参） | runner exit=0；stdout+summary 显式记录 realpath note；15/15 步 exit=0；crash-recovery ASSERT OK；relaunch-probe observed exit=0（验收 /tmp 轮此处曾 exit=1）——符号链接工作目录下真实全绿而非假绿 | run-e2e-repair-r1-symlink/（自 /private/tmp 复制入库） |
| 断言独立负向 | 从入库脚本逐字提取断言 heredoc，喂入 F4 原失败形态 JSON（restart="ERR:...symlink"，probe exit=0 模拟） | assert_exit=1；组合 step_exit=1（旧 runner 此处记 exit=0）；正向对照 exit=0；缺失文件/垃圾 JSON 均 exit=1 | crash-recovery-assert-negative-repair-r1.log、assert-block-extracted.txt、bad/good-recovery.json |
| 端到端负向（人为制造 restart 失败） | /tmp  sabotage 副本（仅两处 diff：SPIKE_DIR 硬编码 + kill -9 后破坏 sidecar 二进制）跑全量 | restart 真实失败（`ERR:spawn failed: Permission denied`），probe 仍 exit=0（"drill captured"），**该步记录 exit=1**，summary 其余 14 步不受影响 | run-e2e-repair-r1-crash-recovery-negative/（含 sabotage.diff 与 runner-summary.txt） |

## 附加清理：rust/target/

实测 2.8G（验收时点 1.4G，其后验收复跑继续增长）未跟踪 cargo 构建残留，已删除（仅该目录，未动其他任何文件）；删除时间/原因记录在 repair-r1/rust-target-cleanup.txt。本代理全部 cargo 调用一律 CARGO_TARGET_DIR=/tmp（rust/ workspace 测试用 /tmp/lingxi-t06-rust-target），删除后 `git status` 无 rust/target，`ls rust/target` 不存在。

## 全量重跑清单（命令与退出码）

| 校验 | 命令 | exit |
|---|---|---|
| run_e2e realpath 轮 | 见上表 | 0（15/15） |
| run_e2e symlink 轮 | 见上表 | 0（15/15，realpath note 显式记录） |
| crash-recovery 独立负向 | 见上表 | 断言 1 / 组合步 1（预期失败成立） |
| crash-recovery 端到端负向 | sabotage 副本全量 | 该步 exit=1（预期失败成立） |
| A11 快速复核 | 三份 page-reports.jsonl（test/release/bundle）+ webdriver-evidence | main 10 通+2 ACL 拒；untrusted/remote 各 11 ACL 拒；驱动端 untrusted/remote 拒、main 通（互证） |
| A12 快速复核 | run-release/webdriver-closed.json + binary-strings-probe.txt | tcp+http 双 ECONNREFUSED；release strings wdio/webdriver 0/0（test 40/41） |
| T01 正向 | `python3 docs/rust-tauri/R01/r01_t01_check_ownership.py` | 0 |
| T01 N1–N15 | `… --self-test` | 0 |
| T01 生成器 | `r01_t01_build_ownership.py --check` | 0 |
| T02 roundtrip | `bash scripts/rust-tauri/r01-t02-roundtrip.sh` | 0 |
| T02 handshake | `bash scripts/rust-tauri/r01-t02-handshake.sh` | 0 |
| T02 check-generated | `bash scripts/rust-tauri/r01-t02-check-generated.sh` | 0 |
| cargo test | `cd rust && CARGO_TARGET_DIR=/tmp/lingxi-t06-rust-target env -u <代理> cargo test --workspace --offline` | 0（19 个 test-result ok，0 failed） |

日志均在 `artifacts/rust-tauri/R01/T06/repair-r1/`（reverify-repair-r1/ 为门禁日志；a11-a12-recheck-repair-r1.log 为负向复核提取）。

## 改动文件清单与 SHA-256

```
7cecd37166061042be680dc3118edbc0e6b3d24bde4c2da253d70c484cea6778  spike/tauri-shell/scripts/run_e2e.sh        (F4)
f72f5d79dfe2a0f325b1bc6f0fb55dd1bcf500a03d9114cf42250f8ae4098e3a  docs/rust-tauri/R01/SHELL_CAPABILITY_MATRIX.json (F3；F1 经否证未动行号)
0e47f3259448b8627d00b831fe233d0453b91679e7090aa0d2b00da86eee9231  docs/rust-tauri/R01/PLATFORM_BUILD_MATRIX.json  (F2)
fdca6d24de1fffb7371ea45ecf7ae1e2a3d10b7b8de741f813c535e0ff36015a  docs/rust-tauri/R01/TAURI_SPIKE_REPORT.md        (F3)
88c3a83657f8ea56bdff1885f1b1ef1618d7058941e11cbd2dfbd64e94f509fe  docs/rust-tauri/R01/R01-T06_REPORT.md            (F3)
341b09db23731c748b2099cee1ba1d9d842dbbfaa787c520bb1db97e9d855bf2  docs/rust-tauri/R01/DEPENDENCY_DECISIONS.md      (F3)
```

新增证据：`artifacts/rust-tauri/R01/T06/repair-r1/`（SHA256SUMS-repair-r1.txt 114 项，`shasum -a 256 -c` 全 OK）。本报告为 `docs/rust-tauri/R01/R01-T06_REPAIR_R1.md`。

## 最终 git status --porcelain

```
 M docs/rust-tauri/ORCHESTRATOR_PROGRESS.json   （总控派发记录，本代理未触碰）
 M docs/rust-tauri/R01/DEPENDENCY_DECISIONS.md   （D-11 F3 措辞）
 M docs/rust-tauri/R01/PLATFORM_BUILD_MATRIX.json（F2）
?? artifacts/rust-tauri/R01/T06/                 （原证据 + repair-r1/ 新证据）
?? docs/rust-tauri/R01/R01-T06_REPORT.md         （执行报告，F3 措辞）
?? docs/rust-tauri/R01/R01-T06_REVIEW_R1.md      （验收报告原文，未动）
?? docs/rust-tauri/R01/R01-T06_REPAIR_R1.md      （本报告）
?? docs/rust-tauri/R01/SHELL_CAPABILITY_MATRIX.json（F3 措辞）
?? docs/rust-tauri/R01/TAURI_SPIKE_REPORT.md      （F3 措辞）
?? spike/                                        （run_e2e.sh F4 修复在其中）
```

rust/target 已消失；`.sync-audit` 零 diff；生产目录零改动；未 stage/未提交。

## 结论

F2/F3/F4 按验收原文修复并全量重跑留证；F1 经真实源码逐行否证（验收方误读，矩阵行号本就正确，改动反而会引入错误），同类条目全量复核无同类错误。READY_FOR_REVIEW。
