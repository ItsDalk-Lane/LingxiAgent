# R01-T06 二次独立复验报告（REVIEW_R2）

- 复验身份：ZCode:R01-T06-review-r2（全新独立复验代理，未参与此前执行/R1 验收/repair-r1 修复）
- 基线 HEAD：`76bd42c439e180163b150a0058bd6c9e4875c940`（核实一致）；分支 `codex/rust-tauri-migration`
- 日期：2026-09-25；机器：macOS 27.0 arm64；Node v24.16.0、Python 3.14.3、rustup+1.98.1
- 复验范围：R1 验收 F1–F4（`R01-T06_REVIEW_R1.md` §11）对 repair-r1（`R01-T06_REPAIR_R1.md`）的关闭状态
- 本复验全程只读仓库，唯一新建仓库文件 = 本报告；全部复跑产物在 `/tmp` 与 `/private/tmp/t06-review-r2/`
- **最终判定：PASS**（F1 裁定为验收方误读、矩阵本就正确，修复方否证成立；F2/F3/F4 修复全部经独立复跑关闭；无新发现问题）

## 1. 工作区与基线核实

`git status --porcelain` 10 行，与预期候选集完全一致：

```
 M docs/rust-tauri/ORCHESTRATOR_PROGRESS.json   （总控账本，本复验未触碰）
 M docs/rust-tauri/R01/DEPENDENCY_DECISIONS.md   （D-11 追加 + F3 措辞，+36 行）
 M docs/rust-tauri/R01/PLATFORM_BUILD_MATRIX.json（T06 段 + F2，+15 行）
?? artifacts/rust-tauri/R01/T06/                 （原证据 57 项 + repair-r1/ 114 项）
?? docs/rust-tauri/R01/R01-T06_REPORT.md / R01-T06_REVIEW_R1.md / R01-T06_REPAIR_R1.md
?? docs/rust-tauri/R01/SHELL_CAPABILITY_MATRIX.json / TAURI_SPIKE_REPORT.md
?? spike/
```

- `rust/target/` **不存在**（2.8G 残留清理属实）；`git status` 无该项。
- `git diff --stat HEAD` 仅上述 3 个 docs 文件；desktop/ server/ core/ lib/ shared/ cli/ hub/ plugins/ skills2set/ package*.json `.sync-audit` 生产树零改动。
- 工作区 `desktop/main.cjs` 与基线 HEAD 逐字节一致（SHA-256 均为 `533d468df2b02e95f8f11d5c3f965a3af2afc8e15a38caa9c1e532b5af17a319`，`git diff` 空）——F1 裁定以 HEAD 内容为准与工作区等效。

## 2. F1 争议裁定（关键项）：验收方误读，矩阵本就正确，修复方否证成立

**三方文本。** 矩阵现文（`SHELL_CAPABILITY_MATRIX.json:83`，本复验亲自 grep）：

```
"electron_current": "desktop/main.cjs:4304/4312 webContents.capturePage（内置浏览器截图），screen.getDisplayNearestPoint 2561/2577"
```

R1 验收 §7 表格与 §11 F1 断言：「实际 4304/4312=`screen.getDisplayNearestPoint`、2561/2577=`webContents.capturePage`，矩阵互换」。修复方 §F1 断言相反。

**行级证据**（本复验亲自执行 `git show HEAD:desktop/main.cjs | awk/sed`，非转述任何一方）：

| 行号 | HEAD 实际内容 | 上下文 |
|---|---|---|
| 2561 | `const display = screen.getDisplayNearestPoint(screen.getCursorScreenPoint());` | `quickChatHeightForMode()` |
| 2577 | `const display = screen.getDisplayNearestPoint(screen.getCursorScreenPoint());` | `defaultQuickChatWindowState()` |
| 4304 | `const img = await wc.capturePage();` | `case "screenshot"`（内置浏览器标签截图） |
| 4312 | `const img = await wc.capturePage();` | `case "thumbnail"` |

4304/4312 的 `wc` 来自 `_withLiveWebContents`（main.cjs:3874）→ `_ensureLiveWebContents(view, sessionPath)` 回调参数，确为 WebContents 实例——矩阵写 "webContents.capturePage" 语义准确。

**裁定：** 实际 4304/4312 = capturePage、2561/2577 = getDisplayNearestPoint，与矩阵现文**逐项一致**；R1 验收方 §7"实测"列把两组行号读反，F1 指控不成立。修复方"不改矩阵、以源码证据否证"的处理正确——若按 R1 建议"更正"反而会引入真实错误。**F1 以"指控不成立"关闭，裁定：验收错、矩阵对。**

**同类错误排查**（本复验独立抽查矩阵 `electron_current` 其余 10 处 main.cjs 行号引用，`git show HEAD:` 逐行核对）：tray 2151 `function createTray()` / 2164 `tray = new Tray(` ✓；notification 1577 `new Notification({` ✓；file-dialog 5745/5758 `dialog.showOpenDialog`、1776 `dialog.showMessageBox` ✓；proxy 296 `setProxy({mode:"direct"})` / 305 `setProxy({mode:"system"})` ✓；window 2889 `sanitizeWindowState(` / 2600 `screen.getAllDisplays()` ✓。全部精确命中，无同类错误。

## 3. F2 关闭判定：已修复，与事实一致

- `PLATFORM_BUILD_MATRIX.json:94` 现文为「（runner-summary.txt 15 步 exit=0）」，18→15 更正生效。
- 事实核对：`run_e2e.sh` 的 `record` 步 = 15（14 个记录点 + relaunch-probe 的 `&&`/`||` 双分支合 1 步）；本复验 realpath/symlink 两轮复跑的 runner-summary.txt 均实测 15 步（`grep -c '^\['` = 15）。
- **F2 关闭。**

## 4. F3 关闭判定：已修复，4 处措辞与事实一致

- 事实：`grep -c '^\[workspace\]'` 两个 spike manifest（`spike/tauri-shell/app/src-tauri/Cargo.toml`、`spike/tauri-shell/sidecar/Cargo.toml`）均 = 0；仓库根、`spike/`、`spike/tauri-shell/` 均无祖先 Cargo.toml（`ls` 三项均不存在）。
- 4 处更正后表述逐一核对（matrix spike.isolation、TAURI_SPIKE_REPORT 头部、R01-T06_REPORT §4、DEPENDENCY_DECISIONS D-11 影响段），统一为「无显式 `[workspace]` 段、无祖先 workspace 可归入，隔离实效成立」的事实口径；旧失实措辞「自带 [workspace]」全仓扫描零残留。
- 三个改动 JSON（SHELL_CAPABILITY_MATRIX / PLATFORM_BUILD_MATRIX / ORCHESTRATOR_PROGRESS）语法复验有效。
- **F3 关闭。**

## 5. F4 关闭复证：修复真实生效，负向不假绿（本复验亲自三轮复跑）

修复内容源码审查（`spike/tauri-shell/scripts/run_e2e.sh`）确认两点：(a) 入口 `mkdir -p "$EVID"` 后 `pwd -P` realpath，入参经符号链接时 stdout 与 runner-summary.txt 均显式记录 `note: evidence dir realpathed ...`，不静默；(b) crash-recovery 步在 probe 之后内联 python 断言直接校验 `sidecar-recovery.json` 语义（`status_after_kill.alive==false`、`restart.restarted==true`、`ping_after_restart` 真实 pong 且 `seq>=2`、`final_status.alive==true 且 restarts==1`，JSON 缺失/损坏亦失败），`RC_STEP_EC` 组合逻辑正确（probe=0 且 assert≠0 时步 exit=assert）。

| 轮次 | 命令（代理变量已 env -u 清除） | runner exit | 结果 |
|---|---|---|---|
| realpath 轮 | `zsh spike/tauri-shell/scripts/run_e2e.sh /private/tmp/t06-review-r2/evidence-realpath` | 0 | **15/15 步 exit=0**；crash-recovery 步日志 `RECOVERY-ASSERT OK: alive=false -> restarted=true -> pong seq>=2 -> restarts=1`（断言真实执行）；relaunch-probe observed |
| symlink 轮 | `zsh spike/tauri-shell/scripts/run_e2e.sh /tmp/t06-review-r2/evidence-symlink`（/tmp→/private/tmp 符号链接入参） | 0 | summary 第 1 行显式记录 `note: evidence dir realpathed '/tmp/t06-review-r2/evidence-symlink' -> '/private/tmp/...' (symlink path; see TAURI_SPIKE_REPORT R2)`；**15/15 步 exit=0**；ASSERT OK；relaunch-probe observed exit=0（R1 /tmp 轮此处曾 exit=1——符号链接工作目录下真实全绿而非假绿） |
| sabotage 负向 | 副本 `/private/tmp/t06-review-r2/spike-sabotage/`（本复验自行注入：kill -9 后 `echo SABOTAGED > $EVID/build/lingxi-t06-sidecar && chmod 644`）跑全量 | 0 | probe 仍输出 `sidecar-recovery drill captured`（probe exit=0，精确复现 F4 原始假绿形态）；断言输出 `RECOVERY-ASSERT FAILED: restart.restarted != true: 'ERR:spawn failed: Permission denied (os error 13)'; ping_after_restart not a real pong: 'ERR:sidecar not running'; final_status alive!=true or restarts!=1...`；**该步记录 exit=1**；其余 14 步 exit=0 不受影响 |

说明：runner 进程本身 exit=0 是记录型编排的设计（门禁语义在 summary 步级 exit 字段），与 repair 报告端到端负向口径一致；关键判定「该步 exit=1 而非假绿」成立。

交叉核对 repair-r1 入库证据（`artifacts/.../repair-r1/`）：symlink 轮 summary realpath note 存在、15 步全 exit=0、ASSERT OK；负向轮 crash-recovery 步 exit=1 且 restart=`ERR:spawn failed: Permission denied`——与本复验独立复跑结果一致，repair 报告声明属实。

**F4 关闭。**

## 6. 独立回归（全部本复验亲自复跑，代理变量已清除）

### 6.1 A11 快速复核（基于本复验 realpath 轮三产物 page-reports.jsonl）

| 产物 | main（tauri://localhost） | untrusted | remote |
|---|---|---|---|
| run-test / run-release / run-bundle（三轮一致） | 10 OK + 2 ACL 拒 | **11 项命令全拒 "not allowed by ACL"** | **11 项全拒** |

- untrusted 逐条核对：shell_public_info / shell_sensitive_probe / shell_secondary_probe / sidecar_status / sidecar_ping / finish_e2e / plugin:dialog\|open / plugin:notification\|is_permission_granted / plugin:clipboard-manager\|read_text / plugin:shell\|execute / shell_exec_arbitrary 全部 `not allowed by ACL`；仅 3 个无害环境探针（tauri_injected / invoke_available / media_enumerate_devices）OK，非敏感命令。
- 驱动端证据（webdriver-evidence.json）：session 200、3 handles（untrusted,remote,main）、untrusted 窗敏感命令 ACL 拒——互证成立。

### 6.2 A12 快速复核

- release 二进制 strings：wdio = 0、webdriver = 0（test 产物 40 / 41）——本复验轮 `binary-strings-probe.txt`。
- `run-release/webdriver-closed.json`：tcp + http 双 ECONNREFUSED。
- 编译门源码确证：`app/src-tauri/Cargo.toml:28` `tauri-plugin-wdio-webdriver = { version = "1", optional = true }` + `:32` `e2e-test = ["dep:tauri-plugin-wdio-webdriver"]`；`src/lib.rs:121-122` `#[cfg(feature = "e2e-test")]` 包裹插件注册——真实编译期门。

### 6.3 T01/T02/cargo 门禁

| 校验 | 命令 | exit | 结果 |
|---|---|---|---|
| T01 正向 | `python3 docs/rust-tauri/R01/r01_t01_check_ownership.py` | 0 | RESULT: OK |
| T01 N1–N15 | `… --self-test` | 0 | PASS-NEG 全列（N11–N15 等抽查在案），RESULT: OK |
| T01 生成器 | `r01_t01_build_ownership.py --check` | 0 | OWNERSHIP_TARGET_UP_TO_DATE features=736 stores=69 |
| T02 roundtrip | `bash scripts/rust-tauri/r01-t02-roundtrip.sh` | 0 | 12 golden byte-identical；tsc 通过 |
| T02 handshake | `bash scripts/rust-tauri/r01-t02-handshake.sh` | 0 | client/server 握手 OK |
| T02 check-generated | `bash scripts/rust-tauri/r01-t02-check-generated.sh` | 0 | API_COMPAT_MATRIX 624 entries drift-free |
| cargo test | `cd rust && CARGO_TARGET_DIR=/tmp/lingxi-t06-r2-rust-target cargo test --workspace --offline` | 0 | **19 个 test-result ok，0 failed** |

### 6.4 证据完整性（本复验执行全量而非抽查）

- 原 `artifacts/rust-tauri/R01/T06/SHA256SUMS.txt`：57 项 `shasum -a 256 -c` 全部 `: OK`。
- `repair-r1/SHA256SUMS-repair-r1.txt`：114 项全部 `: OK`。

## 7. 清理与合规确认

- `rust/target/` 不存在；`git status` 无 rust/target 项；本复验全部 cargo 调用均 `CARGO_TARGET_DIR=/tmp`（未再生成该目录）。
- 生产目录零改动（§1）；`.sync-audit` 零 diff；`ORCHESTRATOR_PROGRESS.json` 保持总控账本原状，本复验未触碰。
- 测试流量全 loopback（runner 证据服务器 127.0.0.1:19274、WebDriver 19275）；联网命令均先清除代理变量。
- 已知预存失败（审计封印等）与本任务无关，未追修。

## 8. 新发现问题

无。

## 9. 最终判定

**PASS**。

依据：F1 经行级源码证据裁定为 R1 验收方误读、矩阵本就正确，修复方否证成立且"不改矩阵"是唯一正确处理；F2（15 步）/F3（4 处 [workspace] 事实表述）更正与事实一致；F4 修复（入口 realpath 显式记录 + 恢复语义内联断言）经本复验 realpath 轮、symlink 轮两轮 15/15 全绿与 sabotage 端到端负向（probe 假绿形态下该步 exit=1）独立复证关闭；A11/A12 快速复核、T01/T02/cargo 门禁、57+114 项证据哈希全部通过；rust/target 清理属实；生产零改动。R01-T06 交付与 repair-r1 修复可关闭。

## 附：本复验关键复跑产物路径（/tmp，易失）

- `/private/tmp/t06-review-r2/evidence-realpath/`（realpath 轮 15/15）
- `/private/tmp/t06-review-r2/evidence-symlink/`（symlink 入参轮 15/15，realpath note 在 summary 第 1 行）
- `/private/tmp/t06-review-r2/evidence-sabotage/`（负向轮：crash-recovery 步 exit=1，其余 14 步 exit=0）
- `/private/tmp/t06-review-r2/spike-sabotage/scripts/run_e2e.sh`（本复验注入的 sabotage 副本，未入仓库）
- `/tmp/t06-review-r2-t01-*.log`、`/tmp/t06-review-r2-t02-*.log`、`/tmp/t06-review-r2-cargo-test.log`（门禁日志）
