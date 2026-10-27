# R09 最终门（verify-stage R09）env 清单与期望（r09-host-core 提供）

> 目标：一次 `verify-stage R09 --evidence <新目录>`，由调用进程 env 驱动冻结候选与 hidden/visible；
> 生产者只读消费候选、不重建 renderer/app。本文件是 r09-host-core 负责的 T01/T02/T07（+T03 动作）部分，
> 以及其它生产者的已知 blocked 项（gate 因此不可能 PASS 的如实原因）。

## 0. 候选（v3.2 冻结）

| 项 | 路径 / sha |
|---|---|
| handoff | `artifacts/rust-tauri/R09/dev/t06/candidate-handoff-v3.2/candidate-handoff.json` |
| delivery `.app` | `desktop/src-tauri/target/release/bundle/macos/Lingxi.app`（appTree `2008b71f…`，exe `8e74aa15…`，features `[r09-host]`） |
| delivery release receipt | `desktop/src-tauri/.tauri-build/build-receipt-release.json` |
| **冻结 G1 exe 稳定副本** | `/private/tmp/lingxi-r09-evidence/lingxi-resource-desktop-030607ee`（sha `030607ee…`） |
| G1 debug receipt | `desktop/src-tauri/.tauri-build/build-receipt-debug.json`（sha `513aba98…`） |
| **gate evidence wrapper** | `/private/tmp/lingxi-r09-t07/evidence-app-gate-20261027-000648/Lingxi.app`（delivery 骨架 + 冻结 G1 exe；ad-hoc 签名后主 exe `992f0a7b…`；manifest `artifacts/rust-tauri/R09/dev/t07/gate-wrapper-01/wrapper-manifest.json`） |
| T02 release-evidence hooks `.app` | `/private/tmp/lingxi-r09-t06-release-evidence-target/release/bundle/macos/Lingxi.app`（features `[r09-host,r09-test-hooks]`） |
| renderer（磁盘/extracted） | `b0a20250…`（644 文件；**gate 期间不得重建**） |

## 1. 调用进程 env（导出后执行 verify-stage）

```bash
# 通用
export LINGXI_R09_ALLOW_GUI=1
export LINGXI_R09_T01_VISIBLE=1                     # T01 可见窗口（Electron+Tauri、delivery release）
# T01
export LINGXI_R09_T01_TAURI_APP=/private/tmp/lingxi-r09-t07/evidence-app-gate-20261027-000648/Lingxi.app/Contents/MacOS/lingxi-resource-desktop
export LINGXI_R09_T01_TAURI_RECEIPT=$PWD/desktop/src-tauri/.tauri-build/build-receipt-debug.json
export LINGXI_R09_T01_RELEASE_APP=$PWD/desktop/src-tauri/target/release/bundle/macos/Lingxi.app
export LINGXI_R09_T01_RELEASE_RECEIPT=$PWD/desktop/src-tauri/.tauri-build/build-receipt-release.json
# T02（full suites：debug hidden + release 正/负向走 release-evidence hooks，全程 hidden+sink；不设 HIDDEN_ONLY）
export LINGXI_R09_T02_DEBUG_APP=/private/tmp/lingxi-r09-t07/evidence-app-gate-20261027-000648/Lingxi.app/Contents/MacOS/lingxi-resource-desktop
export LINGXI_R09_T02_DEBUG_RECEIPT=$PWD/desktop/src-tauri/.tauri-build/build-receipt-debug.json
export LINGXI_R09_T02_RELEASE_APP=/private/tmp/lingxi-r09-t06-release-evidence-target/release/bundle/macos/Lingxi.app
export LINGXI_R09_T02_RELEASE_RECEIPT=$PWD/desktop/src-tauri/.tauri-build/build-receipt-release.json
# T07（G1 hidden；G2 delivery 可见）
export LINGXI_R09_T07_APP_BIN=/private/tmp/lingxi-r09-t07/evidence-app-gate-20261027-000648/Lingxi.app/Contents/MacOS/lingxi-resource-desktop
export LINGXI_R09_T07_E2E_RECEIPT=$PWD/desktop/src-tauri/.tauri-build/build-receipt-debug.json
export LINGXI_R09_T07_RELEASE_APP=$PWD/desktop/src-tauri/target/release/bundle/macos/Lingxi.app
export LINGXI_R09_T07_RELEASE_EXE=$PWD/desktop/src-tauri/target/release/bundle/macos/Lingxi.app/Contents/MacOS/lingxi-resource-desktop
export LINGXI_R09_T07_RELEASE_RECEIPT=$PWD/desktop/src-tauri/.tauri-build/build-receipt-release.json
# T03 GUI 动作（visible；manual 目录可选，缺省 3 条 manual 会 blocked）
export LINGXI_R09_GUI_CLEARED=1
export LINGXI_R09_TAURI_APP=/private/tmp/lingxi-r09-t07/evidence-app-gate-20261027-000648/Lingxi.app/Contents/MacOS/lingxi-resource-desktop
export LINGXI_R09_WEBDRIVER_PORT=19530
export LINGXI_R09_MANUAL_EVIDENCE_DIR=<dir>            # 见 §4；不提供则 manual 3 条 blocked
# T08
export LINGXI_R09_T08_HANDOFF=$PWD/artifacts/rust-tauri/R09/dev/t06/candidate-handoff-v3.2/candidate-handoff.json
# 禁止设置：LINGXI_R09_T02_HIDDEN_ONLY / LINGXI_R09_T07_STATIC_ONLY（会让套件只跑子集）
# T06 ACL 当前不在 gate commandRefs；若 T06 加入命令：
#   export LINGXI_R09_T06_RUNTIME_ACL_CLEARED=1
#   export LINGXI_R09_T06_RUNTIME_ACL_HIDDEN_ONLY=1
#   argv 需带 --app /private/tmp/lingxi-r09-evidence/lingxi-resource-desktop-030607ee --wdio-port 19531
```

## 2. 启动次数（gate 全量）

| 套件 | 模式 | app 启动 | 可见窗口 |
|---|---|---|---|
| `r09_t01_react_parity` | visible | Electron×1 + Tauri wrapper×1 | 2 |
| `r09_t01_release_offline` | visible | delivery `.app`×1 | 1 |
| `r09_t02_supervisor_ready` | hidden+sink | debug 1 + release-evidence 4（正/协议注入/摘要注入/fake identity） | 0 |
| `r09_t02_process_ownership` | hidden+sink | debug 3（owned/reused/remote）+ release 2（拒远端/pid-reuse）+ remote fixture 服务 | 0 |
| `r09_t07_native_e2e`（H1） | hidden | gate wrapper×1 | 0 |
| `r09_t07_release_smoke`（G2） | visible | delivery `.app`×1 | 1 |
| `r09_t03_native_matrix` | visible（GUI_CLEARED） | wrapper×1（含托盘/快聊等系统动作，可能有短暂子窗） | 1+瞬态 |
| T04/T05/T06/T08 | 无 Tauri app | T05 用本地 headless Chrome（非应用窗口） | 0 |

- **可见窗口合计 5**（T01×3 + T07 G2×1 + T03×1；T03 托盘/快聊等可能短暂增加）；
- **hidden app 启动 11**（T02×10 + T07 G1×1）；
- 串行执行；各生产者在启动前写 `preflight-ps.json`（T01/T02/T07/T03），结束后断言 ps 0。

## 3. 期望结果（按当前代码）

| 命令 | 期望 | 说明 |
|---|---|---|
| r09_inputs_check / capability_audit / gate_negative / map_integrity | PASS | 静态 |
| r09_t01_react_parity / release_offline | **PASS** | 只读消费 wrapper/delivery；不重建 |
| r09_t02_supervisor_ready / process_ownership | **PASS**（按 H3 同配置，31/19 cases） | 全程 hidden+sink；release 正/负向用 release-evidence（非 delivery；A02 delivery 真实性由 T01 release_offline/G2 覆盖） |
| r09_t03_ipc_negative | PASS | 无 GUI |
| r09_t03_native_matrix | **FAIL(exit2)** | 8 条 blocked_pending（见 §4）+ manual 依赖 |
| r09_t04_permission_matrix / remote_boundary | **FAIL(exit2)** | 10 条登记 blocked（TCC/helper/ASR/截图/signing/linux/双宿主 E2E） |
| r09_t05_browser_pdf / untrusted_preview | PASS（最新 dev 证据 PASS 70/53） | 本地 Chrome headless + Rust 服务，非 GUI app |
| r09_t06_capability_matrix | **FAIL(exit2)** | 21 条 blocked::runtime.*（8 窗口正/负矩阵 + 5 远程内容；需真实 GUI ACL 副作用批次） |
| r09_t06_release_attack_surface | PASS（v3.2 45 cases） | 只读扫描 delivery |
| r09_t07_native_e2e | **PASS**（H1 30 cases） | 只读 gate wrapper |
| r09_t07_release_smoke | **FAIL(exit2)** | `t07::release.file-task` 显式 PENDING（delivery 无 hooks/wdio，无法驱动真实读写 UI）；其余 G2 断言/静态检查 PASS |
| r09_t08_shell_parity / host_completeness | **FAIL(exit2)** | 依赖本门内 T01/T03/T04/T05/T07 release smoke 证据 + visible 批次；当前 candidate-binding 红、A15 pending 59、A16 a16Ok 25/50 |

→ **本门不可能 PASS**（exit 2）。原因全部是已登记 blocked / 待 visible 批次 / T08 依赖门内证据，非候选缺陷。

## 4. blocked 明细（如实，不为过门改绿）

- **T03 native_matrix（8 blocked + 3 manual）**：
  - `blocked_pending_service` 4：onboarding/setup-complete 路由缺失（T03-ONBOARD-01）——`1120B2AA320D::a1/a2`、`6774E79F6714::a1/a2`；
  - `blocked_pending_command` 4：DOCX/XLSX 预览命令未注册（T03-FILE-01）——`37E6C3B9A7B8::a1/a2`、`E84F01456B6C::a1/a2`；
  - `manual` 3（需 `LINGXI_R09_MANUAL_EVIDENCE_DIR/<case>.json` + 截图/录屏路径）：
    `leaf:R00-T02-LA-15DFB1AED8F2::a1-drag-out-single-multi`、
    `leaf:R00-T02-LA-BEC2074CC33C::a1-locale-switch-tray-labels`、
    `leaf:R00-T02-LA-DF4B5168ABB5::a1-notification-delivered-and-click-focus`；
  - schema：`{"schema":"lingxi.r09.manual-observation.v1","case":"<case>","observedAt":"...","operator":"...","observation":"...","evidencePaths":["/abs/...png"]}`。
- **T04**：permission_matrix 7 blocked（TCC 真弹窗/撤销、helper 真实输入、截图执行、ASR 真实识别、打包 Speech TCC、Linux 预览、注册后 host E2E）；remote_boundary 3 blocked（双宿主 E2E、远端设备读取链、任务中 TCC 撤销）。
- **T06**：capability_matrix 21 blocked::runtime.*（每窗口 positive/negative 矩阵 + remote-webview/token/CSP/canary/debug 负向）；release_attack_surface PASS。
- **T07**：release_smoke `file-task` 显式红（A14 真实文件任务需要 delivery hooks/wdio 或可见 UI 自动化，当前未提供）；G1 PASS。
- **T08**：A15/A16 依赖门内证据 + visible 批次；T08 owner（r09-t08-parity）将在证据落盘后重跑，仍会因上述上游红项/visible pending 保持 exit2。
- **可选**：T06 runtime ACL 未含在 stage map commandRefs；若加入需同时加 argv `--app <稳定 G1 exe>` 与 env（§1），否则 A11 运行期 ACL 无证据（1 hidden app 启动，0 可见窗口）。

## 5. 只读/不构建确认

- T01/T02/T07：只读候选 `.app`/exe/receipt + 写 `{EVIDENCE}`；**无 npm build / tauri build / cargo build**；启动前 preflight ps、结束后 post ps；所有 app 用 gate wrapper 或 delivery/release-evidence，不写 `desktop/src-tauri`。
- T03/T04/T05/T06/T08：不构建 desktop crate；T03 native harness 仅 rustc 编译 `src/native` 子集到证据目录；T05 用本地 Chrome headless；T06 只读扫描 delivery；T08 只读证据 + renderer vitest。
- 冻结候选不受 gate 写入；`target/debug` 已恢复为冻结 `030607ee…` + receipt `513aba98…`，且已复制稳定副本，gate/wrapper 只应从稳定副本取。
