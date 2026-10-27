# R01-T06 独立对抗性验收报告（REVIEW_R1）

- 验收身份：ZCode:R01-T06-review-r1（未参与执行；只读审查 + /tmp 隔离复跑）
- 基线 HEAD：`76bd42c439e180163b150a0058bd6c9e4875c940`（核实一致）；分支 `codex/rust-tauri-migration`
- 日期：2026-09-25；机器：macOS 27.0 arm64（同机型环境）
- 被审交付物：`spike/tauri-shell/`、`docs/rust-tauri/R01/{SHELL_CAPABILITY_MATRIX.json,TAURI_SPIKE_REPORT.md,R01-T06_REPORT.md}`、`DEPENDENCY_DECISIONS.md`（D-11）、`PLATFORM_BUILD_MATRIX.json`、`artifacts/rust-tauri/R01/T06/`
- 本验收所有复跑证据在 `/tmp/t06-review-r1/`（`/tmp` 轮）与 `/private/tmp/t06-review-r1/evidence2/`（realpath 轮），未写入仓库
- **最终判定：PASS**（附 3 项低严重度文档失实 F1–F3 与 1 项 runner 门禁弱点 F4，不改变任何 VERIFIED 结论，建议修复但不阻塞）

## 1. 候选清单与工作区核实

`git status --porcelain` 终态与任务书预期完全一致：

| 路径 | 状态 | 本验收复算 SHA-256 |
|---|---|---|
| docs/rust-tauri/R01/SHELL_CAPABILITY_MATRIX.json | ?? | 8a072aa983dbd7af322fbaf5c05a5098ce8664ef71453e8ea3375ea47a5169c5 |
| docs/rust-tauri/R01/TAURI_SPIKE_REPORT.md | ?? | 2cef2170385ec640f19d65e4d6391ead39b9d22ab1b17744d4365db4f433df6c |
| docs/rust-tauri/R01/R01-T06_REPORT.md | ?? | e1888c5949bfd4a7d156fc7e491eab2dbee8bd85b3520f34c89dcfabea08718e |
| docs/rust-tauri/R01/DEPENDENCY_DECISIONS.md | M（+36 行，仅追加 D-11） | 416a3cd23bff6e16c6fce29404d1b109e39dafe9eaf1198b95ed565a9ba06ff5 |
| docs/rust-tauri/R01/PLATFORM_BUILD_MATRIX.json | M（+15 行 desktop_prototype_r01_t06） | 23487dc901af9403408df67ff9907b7f95fef345e9782c5a5eee71de53274cac |
| docs/rust-tauri/ORCHESTRATOR_PROGRESS.json | M（仅 task_base_sha 派发记录，非本任务候选，未触碰） | — |
| spike/tauri-shell/ | ??（15M，含 app/node_modules 的 @tauri-apps/cli） | 关键文件哈希见 §7 |
| artifacts/rust-tauri/R01/T06/ | ??（SHA256SUMS.txt 57 项） | 见 §2 |
| rust/target/ | ?? 未跟踪构建残留（1.4G），**未 stage**（staging 区为空），属可清理残留；本验收未删除 | — |

生产目录零改动确认：`git diff --stat HEAD` 仅 3 个 docs 文件；desktop/ server/ core/ lib/ shared/ cli/ hub/ plugins/ skills2set/ package*.json 均无 diff。`DEPENDENCY_RULES.json` 无 diff。任务书目录无 diff。`.sync-audit` 无 diff。

## 2. 证据完整性：SHA256SUMS 57 项全量校验

```
cd artifacts/rust-tauri/R01/T06 && shasum -a 256 -c SHA256SUMS.txt
# 57 项全部 ": OK"，无一行 FAILED，exit=0
```

（任务要求抽查，本验收执行了全量 57/57，含三个二进制与全部日志。）

## 3. 独立复跑：run_e2e.sh（两轮）

### 3.1 第一轮：证据目录在 /tmp（符号链接路径）——意外成为 R2 的独立复现

`zsh spike/tauri-shell/scripts/run_e2e.sh /tmp/t06-review-r1/evidence`，runner 进程 exit=0，但逐条核对 actual 字段发现两步**语义失败**：

| 步骤 | 记录 exit | 实际语义 |
|---|---|---|
| runA-sidecar-crash-recovery | 0 | restart 失败：`ERR:sidecar() failed: StartingBinary found current_exe() that contains a symlink`（/tmp→/private/tmp 符号链接，即报告 R2）；probe 脚本只判"捕获到 drill 输出"故 exit=0 |
| relaunch-probe | **1** | `NOT observed`——第二实例未出现；同一 symlink 路径根因 |

**这一轮独立复现了发现 R2**（符号链接路径下 current_exe() 被拒，且波及 relaunch），R2 如实。

### 3.2 第二轮：realpath 证据目录 —— 15/15 全绿

`zsh spike/tauri-shell/scripts/run_e2e.sh /private/tmp/t06-review-r1/evidence2`，runner exit=0，逐条核对：

| 步骤 | exit | 语义核对（actual 字段实质内容） |
|---|---|---|
| sidecar-build | 0 | 构建成功 |
| sidecar-integration-tests | 0 | `8 passed; 0 failed`（真实二进制驱动） |
| app-test-build / app-release-build | 0/0 | 双产物均成功 |
| runA-page-reports | 0 | 3 报告（main,remote,untrusted） |
| runA-webdriver-session | 0 | 3 窗会话 + 驱动端负向互证（见 §4） |
| runA-sidecar-crash-recovery | 0 | kill -9 → alive=false → restart ok → **pong seq=2** → restarts=1（真实恢复） |
| runA-app-exit / runA-sidecar-cleanup | 0 | exit=0；bye=true orderly=true；无孤儿 |
| runB-webdriver-closed | 0 | tcp+http 双 ECONNREFUSED |
| runB-page-reports | 0 | release 产物 3 报告、同判拒绝矩阵 |
| runB-sigkill-orphan | 0 | 宿主 SIGKILL 后无 sidecar 孤儿 |
| relaunch-probe | 0 | second_instance_observed |
| tauri-build-bundle | 0 | .app + .app.tar.gz + .sig 真实签名产出 |
| runBundle-page-reports | 0 | 打包 .app（realpath 目录运行）3 报告；sidecar 随包握手 OK；updater 打包内正向验签通过 |

## 4. A11 对抗（独立驱动，非执行者脚本路径）

### 4.1 复跑证据语义核对（realpath 轮 run-test/run-release/run-bundle 三份 page-reports.jsonl）

- main（tauri://localhost）：10 OK；`plugin:shell|execute` 与未知命令 `shell_exec_arbitrary` 均 "not allowed by ACL"。
- untrusted（打包页，不在任何 capability）：11 项命令全拒 "not allowed by ACL"。
- remote（http://127.0.0.1:19274 远程源等价）：11 项全拒。
- 三份产物（test/release/bundle）矩阵一致。
- WebDriver 驱动端（独立进程，非页面自报）在 untrusted/remote invoke `shell_sensitive_probe`：两窗均 "not allowed by ACL"；main 同调用 OK——互证成立（本验收 realpath 轮 webdriver-evidence.json）。

### 4.2 自造越权调用（自写 WebDriver 驱动脚本 /tmp/t06-review-r1/adversarial_a11.mjs，端口 19375，独立 scratch）

从每个窗口尝试 5 种攻击向量：`__TAURI_INTERNALS__.invoke` 底层通道（绕过 `__TAURI__.core` 封装）、`core.invoke` 敏感命令、`plugin:shell|execute` 任意 shell（touch /tmp/t06-review-r1/PWNED）、`shell_secondary_probe`（检验 capability 并集不扩权到未授权窗）、手工构造 `window.ipc.postMessage` 原始 IPC 报文。

| 窗口 | internals 敏感命令 | core 敏感命令 | shell\|execute | secondary_probe | 原始 IPC |
|---|---|---|---|---|---|
| untrusted | 拒(ACL) | 拒(ACL) | 拒(ACL) | 拒(ACL) | 静默丢弃 |
| remote（证据服务器在线的真实 http 源） | 拒(ACL) | 拒(ACL) | 拒(ACL) | 拒(ACL) | 静默丢弃 |
| main | OK | OK | **拒(ACL)** | OK（第二 capability 并集生效） | 静默丢弃 |

- 副作用核验：独立 scratch 中 `sensitive-probe-executed.marker` 仅由 main 窗调用产生；`/tmp/t06-review-r1/PWNED` 不存在——拒绝无静默副作用。
- 错误可诊断：全部拒绝返回结构化 "Command X not allowed by ACL"。
- **结论：授权面由 capability 配置决定（窗口 label 绑定），非前端隐藏；底层 IPC 通道同样被 ACL 拦截。A11 独立成立。**

## 5. A12 对抗（release 产物三重探测）

对 realpath 轮 release 产物（本验收独立启动，TAURI_WEBDRIVER_PORT=19375）：

| 探测 | 命令 | 结果 | exit |
|---|---|---|---|
| WebDriver 端口（runner 口） | `probe_webdriver.mjs closed-check 19275` | tcp+http 均 ECONNREFUSED | 0 |
| 本验收自定义端口 | `closed-check 19375` | ECONNREFUSED | 0 |
| 插件默认端口 | `closed-check 4445` | ECONNREFUSED | 0 |
| 全量监听套接字 | `lsof -nP -iTCP -sTCP:LISTEN -p <pid>` | release 进程**无任何 LISTEN** | — |
| 二进制 strings（本验收独立计数） | `strings app-release \| grep -ci wdio/webdriver/tauri_plugin_wdio` | **0 / 0 / 0**（test 产物 40 / 41 / 7） | — |
| release 页面电池 | 同 A11 | main 10 OK + 2 ACL 拒；untrusted/remote 各 11 ACL 拒 | — |

编译门真实性（源码确证）：`Cargo.toml` 中 `tauri-plugin-wdio-webdriver = { version = "1", optional = true }` + `e2e-test = ["dep:tauri-plugin-wdio-webdriver"]`；`lib.rs:121-122` `#[cfg(feature = "e2e-test")]` 包裹插件注册。optional 依赖无 feature 即不进编译单元，属真实编译期门而非运行时开关。**A12 独立成立。**

## 6. sidecar 安全（源码确证 + 真实二进制独立探测）

- 源码（spike/tauri-shell/sidecar/src/main.rs）：固定协议、仅接受 `--report-version X`（测试钩子），其他参数 exit 64；stdin EOF 自退 exit 0；无 shell、无任意命令执行面。
- 独立探测真实二进制（/tmp/lingxi-t06-target-sidecar/release/lingxi-t06-sidecar）：
  - `--evil` / 单参数 / 三参数 → exit 64 ×3；`</dev/null`（EOF）→ exit 0；
  - `--report-version 9.9.9` → hello 报 9.9.9（宿主侧 `validate_hello` 纯函数拒绝，集成测试断言）；garbage hello 拒绝（8 测试含）；
  - ping→pong(seq 对齐) / 未知命令→`unknown_command` / shutdown→bye+exit 0，逐行实测。
- 宿主侧（src/sidecar.rs）：5s 握手超时、失败即 kill（不静默降级）、3s 命令超时、MAX_RESTARTS=1 预算、Terminated drain pending。前端仅可经 sidecar_status/ping/restart 窄命令接触（A11 已证未授权窗连这些都调不到）。
- 崩溃恢复在本验收 realpath 轮真实复跑通过（kill -9 → pong seq=2 → restarts=1）。
- 说明：任务书/执行者"受限参数 64"指 **exit code 64（EX_USAGE）**，非"64 个参数上限"——语义核实一致。

## 7. 能力矩阵抽查（10 项）

**现役 Electron 定位（desktop/main.cjs 行号，独立 grep/sed 核对）：**

| 能力 | 矩阵声明 | 实测 | 结论 |
|---|---|---|---|
| tray | 2151 createTray / 2164 new Tray | 精确命中 | ✓ |
| shortcut | 210-266 globalShortcut 多组注册 | 命中（229/239/240/263-266） | ✓ |
| notification | 1577 new Notification | 精确命中 | ✓ |
| file-dialog | 5745/5758 showOpenDialog、1776 showMessageBox | 精确命中 | ✓ |
| proxy | 296-305 session.setProxy direct/manual(fixed_servers)/system | 精确命中 | ✓ |
| screen | "4304/4312 capturePage、2561/2577 getDisplayNearestPoint" | **两处行号写反**：实际 4304/4312=getDisplayNearestPoint、2561/2577=capturePage | ✗ 见 F1 |

其余引用文件（auto-updater.cjs、speech-permissions.cjs、login-item-settings.cjs、github-release-check.cjs）均存在；窗口（2889 sanitizeWindowState、2600 getAllDisplays）、speech（5449-5460）行号命中。

**Tauri 侧 VERIFIED 语义抽查（本验收 realpath 轮 host-report.ndjson，非"调用没报错"层面）：**

1. 全局快捷键：`shortcut.register registered=true` + runner 合成 CGEvent 后 `shortcut.fired`（handler 真实触发）——✓ 语义成立。
2. 更新器：正向 `update_found=0.2.0 downloaded=408B installed=false`；负向篡改包 `download: The signature verification failed`（本验收轮与执行者原始证据一致复现）；打包 .app 内复测通过——✓ 验签强制生效，不是编码错误。
3. 登录自启动：`enable→is_enabled=true→disable→false` 真实往返（test 轮与 bundle 轮各一次）——✓。

另核对：clipboard roundtrip_ok+restored、notification Granted+shown、tray.created、window.ops 全 Ok、TCC 快照全 not_determined/granted 只读值，均与报告一致。录音/代理等 UNVERIFIED 标注如实（本验收未伪造授权）。

## 8. R1–R8 发现复核

| # | 复核方式 | 结论 |
|---|---|---|
| R1a blocking_pick_file 外部取消永不返回 | 源码注释与报告一致；本验收未单独搭阻塞变体装置复触（标：执行者会话观测，机制与 AppKit 语义一致，无反证） | 部分独立核实 |
| R1b 合成 Escape 走 performClose: 不触发回调 | **本验收独立复现**：app-test 弹 NSOpenPanel 后 post_escape → 4s 内 host-report 无 dialog.open 事件且 CGWindowList 上面板已消失；对照 runner 路径 Cmd+. postToPid 则 dialog.open picked=null 确定性兑现（两轮复跑均现） | ✓ 独立成立 |
| R2 /tmp 符号链接 current_exe 拒绝 | **本验收意外独立复现**（§3.1：/tmp 证据目录轮 sidecar restart 与 relaunch 双双因 symlink 失败；realpath 轮全绿） | ✓ 独立成立 |
| R3 无前缀 allow-<command> | capabilities/main.json 写法与 tauri-build 生成权限一致，A11 实测通过 | ✓ |
| R4 tauri-driver 无 macOS | 与上游事实一致；wdio-webdriver 路径实测可用（3 窗会话） | ✓ |
| R5 WKWebView 媒体约束 | page-reports 中 getUserMedia 拒、getDisplayMedia "must be called from a user gesture handler" | ✓ |
| R6 dangerousInsecureTransportProtocol | 全仓 grep：仅出现于 spike/tauri-shell/app/src-tauri/tauri.conf.json（endpoints 为 loopback http）与三份文档的"禁进生产"标注；生产树零命中 | ✓ 范围正确 |
| R7/R8 | 与证据一致（通知 Granted 单次 show；CSP connect-src 显式放行 19274） | ✓ |

## 9. D-11 依赖锁定一致性

- spike Cargo.lock 全量解析：**603 crates**，与 D-11/报告一致。
- 13 项锁定版本逐项比对 Cargo.lock：tauri 2.11.6 / tauri-build 2.6.3 / wry 0.55.1 / tao 0.35.3 / shell 2.3.6 / notification 2.4.0 / dialog 2.7.3 / clipboard-manager 2.3.3 / global-shortcut 2.3.2 / autostart 2.5.1 / updater 2.12.0 / process 2.3.1 / wdio-webdriver 1.4.0 / serde 1.0.229 / serde_json 1.0.151 / tokio 1.53.1——**全部一致**。
- workspace 边界：rust/Cargo.toml members 不含 spike；仓库根及 spike 祖先目录均无 Cargo.toml，spike 各 manifest 自成隔离根，DEP-07/D5-reverse 不受影响（T01 正向 PASS 佐证，§10）。
- 偏差（见 F3）：报告称 "app/src-tauri/Cargo.toml 自带 [workspace]"，实测两个 spike manifest **均无显式 `[workspace]` 段**；隔离实效成立（无祖先 workspace 可归入），但字面描述失实。

## 10. T01/T02 门禁独立回归（全部亲自复跑）

| 校验 | 命令 | exit | 证据（/tmp/t06-review-r1/reverify/） |
|---|---|---|---|
| T01 正向 | `python3 docs/rust-tauri/R01/r01_t01_check_ownership.py` | 0 | t01-positive.log（RESULT: OK） |
| T01 N1–N15 | `… --self-test` | 0 | t01-selftest.log（PASS-NEG N14/N15 等全列） |
| T01 生成器 | `r01_t01_build_ownership.py --check` | 0 | OWNERSHIP_TARGET_UP_TO_DATE features=736 stores=69 |
| T02 roundtrip | `bash scripts/rust-tauri/r01-t02-roundtrip.sh` | 0 | r01-t02-roundtrip.log |
| T02 handshake | `bash scripts/rust-tauri/r01-t02-handshake.sh` | 0 | client/server exit 0/0 |
| T02 check-generated | `bash scripts/rust-tauri/r01-t02-check-generated.sh` | 0 | drift-free |
| cargo test | `cd rust && cargo test --workspace --offline` | 0 | 19 个 test-result ok，0 failed（含 11/19/7/7/1 实测试） |
| cargo fmt / clippy | `--all --check` / `--locked --workspace --all-targets --offline -D warnings` | 0 / 0 | — |

## 11. 发现问题（验收侧新增）

| # | 严重度 | 问题 | 最小重现/根因 | 建议 |
|---|---|---|---|---|
| F1 | 低（文档失实） | SHELL_CAPABILITY_MATRIX.json screen 项行号写反 | main.cjs 实际 4304/4312=`screen.getDisplayNearestPoint`、2561/2577=`webContents.capturePage`；矩阵互换 | 下次触碰该文件时修正 |
| F2 | 低（文档失实） | PLATFORM_BUILD_MATRIX.json 称 "runner-summary.txt 18 步 exit=0" | 实际 record() 步数=15（执行者自报告亦写 15）；summary 文件含 step0/complete 两行共 17 行，无论哪种口径都不是 18 | 同上修正 |
| F3 | 低（文档失实） | "spike manifest 自带 [workspace]" 不实 | 两个 spike Cargo.toml 均无显式 `[workspace]` 段（grep 计数 0）；隔离靠"无祖先 Cargo.toml"成立 | 修正表述为"独立 manifest，无祖先 workspace 可归入" |
| F4 | 低（runner 门禁弱点，已在报告 R2 中间接暴露） | run_e2e.sh 的 runA-sidecar-crash-recovery 步仅记录 probe 脚本退出码，不校验恢复语义 | 本验收 /tmp 轮 restart 失败仍记 exit=0（actual 字段可见失败，未掩盖）；relaunch 步有语义判定（exit=1 正确暴露）故不对称 | R09 接手时给该步加 `pong` 断言门禁 |

F1–F3 均为引用/计数级失实，不改变任何 VERIFIED 结论的事实基础（相关能力均经本验收独立实测）；F4 是复放工具弱点而非产品/原型缺陷。均不构成阻塞。

## 12. 红线与环境合规核验

- 本验收全程只读仓库（唯一新建文件=本报告）；复跑产物与 scratch 全部在 /tmp、/private/tmp；未 stage/未提交任何内容。
- 执行者红线声明抽查属实：updater 只 download 未 install（host-report installed=false）；测试私钥在 /tmp/lingxi-t06-keys/ 未入库（git status 无）；流量全 loopback；TCC 只读（本验收两轮复跑同样零授权弹窗）；已知预存失败（审计封印 FAIL 等）与本任务无关、未追修——复核确认 `.sync-audit` 无 diff。
- rust/target/（1.4G）为 cargo 构建残留，未跟踪未 stage；不属候选；建议后续清理，本验收未动。

## 13. 最终判定

**PASS**。

依据：任务书 §4 R01-T06 五步全部有真实交付与独立复跑证据；A11（越权拒绝、capability 决定授权面、含底层 IPC 通道对抗）与 A12（release 无监听/无字符串/无可达入口、编译期门真实）经本验收独立对抗复跑成立；sidecar 8 场景真实复跑通过；D-11 锁表与 Cargo.lock 逐项一致；T01/T02 回归全绿；57 项证据哈希全对；生产零改动。F1–F4 低严重度项建议修复但不改变判定。

## 附：spike 关键文件 SHA-256（复算）

```
e974037cae3ce92e4df0b2dee9c26fc32c8af7ef3658d70b11d2d7f1edaf1596  spike/tauri-shell/app/src-tauri/Cargo.toml
af9520f5edf8df9d2c8daf5e19325bff4f44864c623c795beb2729fa91553483  spike/tauri-shell/app/src-tauri/Cargo.lock
06f3597e55f7a4955aab93b456d529d149b7422c9153f705e191631681829595  spike/tauri-shell/app/src-tauri/tauri.conf.json
a2d76a6048d8543ce0edb8cecb93616b7a417466a84b0613e6db26162174258b  spike/tauri-shell/app/src-tauri/capabilities/main.json
5614f24b5053fb3bdd76e9d53b2c8d55de4c4130f4ff8ea11cabda038dcb5b53  spike/tauri-shell/app/src-tauri/capabilities/main-secondary.json
796eefe4b717d979bca8b1e6c1616573435c313d1c51934f7ab68d9ecee9fb3f  spike/tauri-shell/scripts/run_e2e.sh
```
