# R01-T06 系统能力与桌面安全原型 — 执行报告（READY_FOR_REVIEW）

- 执行者：ZCode:R01-T06-exec-r1｜基线 HEAD `76bd42c439e180163b150a0058bd6c9e4875c940`｜分支 `codex/rust-tauri-migration`｜日期 2026-09-25
- 交付物：`docs/rust-tauri/R01/SHELL_CAPABILITY_MATRIX.json`、`docs/rust-tauri/R01/TAURI_SPIKE_REPORT.md`、桌面 E2E 原型 `spike/tauri-shell/`（可重放 `scripts/run_e2e.sh`）、证据 `artifacts/rust-tauri/R01/T06/`（SHA256SUMS.txt 见 §6）、本报告
- 平台结论：**macOS 27.0 arm64 单机实测 VERIFIED；Windows / Linux / macOS x64 全部 UNVERIFIED**，owner=R09/R10 平台验收负责人（总控指派），verify_by_stage=R09 最迟 R10。无任何 OS 授权伪造（TCC 全部只读查询；录音/听写授权态正链路如实留 UNVERIFIED-interactive）

## 1. 五步任务执行对照

| 步骤 | 结果 | 证据 |
|---|---|---|
| 1 现役能力清点 | DONE | SHELL_CAPABILITY_MATRIX.json `capabilities[].electron_current`（desktop/main.cjs、auto-updater.cjs、speech-permissions.cjs、login-item-settings.cjs、github-release-check.cjs 行级定位） |
| 2 隔离原型逐项实测 | DONE | TAURI_SPIKE_REPORT §2；run-test/host-report.ndjson（13 类探针真实调用全记录） |
| 3 capabilities 授权面 + 负向 | DONE（A11 成立） | TAURI_SPIKE_REPORT §3；页面自报 + WebDriver 驱动端互证：untrusted/remote 全拒（各 11/11），main 授权命令全通、plugin:shell\|execute 仍拒 |
| 4 sidecar 生命周期 | DONE | 8 场景矩阵（握手/版本拒绝/garbage 拒绝/崩溃恢复/有序退出/SIGKILL 无孤儿/受限参数 64/EOF 自退），TAURI_SPIKE_REPORT §4 |
| 5 W03 测试路径 + A12 双产物 | DONE（A12 成立） | tauri-driver macOS 不可用（如实记录）；wdio-webdriver 1.4.0 实测 session/3 windows/execute；release 产物端口 ECONNREFUSED + strings 计数 0 |

## 2. 关键场景流水（命令/预期/实测/退出码）

runner-summary.txt 全 15 步 exit=0。核心场景：

| 场景 | 命令/操作 | 预期 | 实测 | 退出码 |
|---|---|---|---|---|
| sidecar 构建+集成测试 | `cargo build --release` + `cargo test --test sidecar_handshake`（SIDECAR_BIN 真实二进制） | 8 测试全过 | 8 passed/0 failed | 0 |
| app 双产物构建 | `cargo build --release --features e2e-test` / 无 feature | 均 exit 0 | 均成功 | 0/0 |
| Run A 页面电池 | 三窗（main/untrusted/remote）自动跑探针并 POST 证据 | 3 报告 | `{"reports":["main","remote","untrusted"]}` | 0 |
| Run A WebDriver | `probe_webdriver.mjs session-test 19275` | 3 windows + 驱动端负向 | `session-test OK: windows=main,untrusted,remote`；untrusted/remote 驱动端 invoke 敏感命令被拒 | 0 |
| 全局快捷键 | `post_shortcut`（合成 CGEvent Cmd+Alt+Shift+9） | handler 触发 | host-report `shortcut.fired` | 0 |
| 文件对话框 | 宿主 callback pick_file + `post_cmd_period`（合成 Cmd+period postToPid，cancel: 等价物） | 面板弹出并可取消 | `dialog.open picked=null`（取消兑现） | 0 |
| 更新器正向 | updater.check+download（loopback 夹具，有效 minisign 签名） | 发现+验签下载，不 install | update_found 0.2.0，downloaded 406B，installed=false | 0 |
| 更新器负向 | latest-tampered.json（合法签名+字节篡改包 url） | 验签失败 | `download: The signature verification failed` | 0 |
| sidecar 崩溃恢复 | `kill -9 <sidecar pid>` → WebDriver sidecar-recovery | dead→restart→ping | alive=false → restart ok → pong seq=2 → restarts=1 | 0 |
| 有序退出 | WebDriver `finish`（invoke finish_e2e） | exit 0 + sidecar code 0 | exit=0；bye=true orderly=true | 0 |
| Run B A12 | release 产物 `closed-check` | 端口关闭 | ECONNREFUSED（tcp+http） | 0 |
| Run B 二进制探测 | `strings` 双产物 | release 无 wdio/webdriver | test 40/41，release 0/0 | 0 |
| Run B SIGKILL 孤儿 | `kill -9` 宿主 | sidecar EOF 自退 | no sidecar process after host SIGKILL | 0 |
| relaunch | `tauri::process::restart` | 第二实例观测 | second_instance_observed ok | 0 |
| 打包 | `npx tauri build --bundles app`（TAURI_SIGNING_PRIVATE_KEY 从 /tmp 注入） | .app + 签名更新产物 | .app + .app.tar.gz + .sig | 0 |
| 打包冒烟 | realpath 目录运行 .app 副本 | 3 报告 + sidecar/updater 正常 | 3 报告；handshake_ok；updater 验签通过 | 0 |

## 3. 实测发现（R09 直接输入，详见 TAURI_SPIKE_REPORT §7）

1. **R1（重要缺陷）**：tauri-plugin-dialog 回调兑现依赖 cancel: 语义——blocking_pick_file 外部取消后永不返回；callback API 下合成 Escape 也走 performClose: 不触发回调；只有 Cmd+period（cancel: 等价物）postToPid 确定性兑现。生产禁用阻塞变体，自动化取消用 Cmd+.。
2. **R2**：macOS /tmp→/private/tmp 符号链接触发 sidecar/updater current_exe() 拒绝；运行目录须 realpath。
3. **R3**：app 命令自动生成权限无前缀（`allow-<command>`），capability 写错形式会被拒。
4. **R4**：tauri-driver 无 macOS 支持；macOS E2E 走 wdio-webdriver feature 门控。
5. **R5**：WKWebView 媒体语义——getDisplayMedia 需用户手势；TCC 未授权时 getUserMedia 直接拒绝不弹窗。
6. **R6**：release updater 强制 https；spike 的 `dangerousInsecureTransportProtocol` 仅限 loopback 夹具，禁止进生产。
7. **R7/R8**：未打包二进制的通知/自启动归属终端上下文；CSP 需显式放行证据 loopback（仅 spike）。

## 4. 红线合规自查

- 未 commit/push/PR/tag/release；未改任务书目录、`.sync-audit`、`ORCHESTRATOR_PROGRESS.json`（其工作区 diff 为总控派发本任务时写入的 task_base_sha 记录，非本 agent 改动）。
- 生产目录零改动：desktop/ server/ core/ lib/ shared/ cli/ hub/ plugins/ skills2set/ package*.json 均未触碰（§6 git status 取证）。`spike/tauri-shell/app/package.json` 是 spike 自有清单（仅 devDependency @tauri-apps/cli 2.11.5），不属生产 package*.json。
- 禁占位/假证据/跳过断言：全部结论有对应证据文件；录音/听写授权态、屏幕采集、代理矩阵如实 UNVERIFIED。
- 无真实用户数据/真实账号/付费 API/真实外发：全部流量限 loopback（127.0.0.1:19274/19275）；runner 联网命令（cargo/npx）前缀 env -u 剥离死代理；crates.io 访问仅构建期依赖解析。
- 系统交互最小化：通知 1 次真实 show；自启动 enable→disable 往返后恢复原状；剪贴板写后恢复用户原内容；updater 只 download 不 install，未触发真实系统更新；TCC 全只读查询，零授权弹窗、零伪造。
- updater 测试私钥在 /tmp/lingxi-t06-keys/（不入库、不进报告）；pubkey 入 spike 配置（一次性测试公钥）。
- CARGO_TARGET_DIR=/tmp 隔离（sidecar/app 各自独立 target 目录）；rust/ workspace 未被改动（spike 为独立 manifest——无显式 [workspace] 段、无祖先 workspace 可归入，DEP-07 不受影响）。

## 5. 改源码后强制校验（rust/ 零改动；DEPENDENCY_RULES.json 零改动）

本任务未触碰 rust/ 与 DEPENDENCY_RULES.json（spike 隔离决策记录于 D-11：登记为 module_registry exists 会触发校验器"exists 但缺席 workspace"误报，故不登记，理由在案）。仍按惯例重跑全套校验器作无回归证据（reverify/ 目录）：

| 校验 | 退出码 | 证据 |
|---|---|---|
| `r01_t01_check_ownership.py`（正向） | 0 | reverify/t01-check-positive.log |
| 同 `--self-test`（N1–N15） | 0 | reverify/t01-self-test-N1-N15.log |
| `r01_t01_build_ownership.py --check` | 0 | reverify/t01-build-check.log |
| `r01-t02-roundtrip.sh` / `handshake` / `check-generated` | 0 / 0 / 0 | reverify/r01-t02-*.log（roundtrip/handshake 为 bash 脚本，首次用 zsh 直跑 exit=127 系调用方式错误，bash 重跑通过；非产品代码问题） |
| `cargo test --workspace --offline` | 0 | reverify/cargo-test-workspace.log |
| `cargo fmt --all --check` / `cargo clippy --locked --workspace --all-targets --offline -- -D warnings` | 0 / 0 | reverify/cargo-fmt.log / cargo-clippy.log |

## 6. 证据与最终状态

- 证据根 `artifacts/rust-tauri/R01/T06/`：runner-summary.txt（15 步 expected/actual/exit 全 0）、build/（双产物+sidecar 二进制与 SHA256）、sidecar-it/、updates/（夹具与签名）、run-test/（host-report.ndjson、page-reports.jsonl、webdriver-evidence.json、sidecar-recovery.json、ps-tree.txt、dialog-dismiss.log）、run-release/（A12 探测、篡改验签失败、SIGKILL 孤儿检查）、run-relaunch/、run-bundle/、bundle/、reverify/（9 个校验器日志）、SHA256SUMS.txt（57 项，含三个二进制）。
- 已知预存失败不追修：4 项审计封印 FAIL 等（本任务未触碰 `.sync-audit` 与封印坐标）。
- `git status --porcelain` 终态（§下方「最终 git status」）：改动仅限 `docs/rust-tauri/R01/{DEPENDENCY_DECISIONS.md,PLATFORM_BUILD_MATRIX.json}`（M）+ 新增 `docs/rust-tauri/R01/{SHELL_CAPABILITY_MATRIX.json,TAURI_SPIKE_REPORT.md,R01-T06_REPORT.md}`、`spike/`、`artifacts/rust-tauri/R01/T06/`、`rust/target/`（cargo test 构建产物，未跟踪）。ORCHESTRATOR_PROGRESS.json 的 M 为总控派发记录（见 §4）。
