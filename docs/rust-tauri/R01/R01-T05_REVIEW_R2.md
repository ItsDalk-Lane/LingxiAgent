# R01-T05 修复轮独立复验报告（REVIEW R2）

- 任务：R01-T05（HTML/PDF 与办公处理原型）修复轮复验；验收 R01-A09 / R01-A10（REQUIRED）
- 复验代理：ZCode:R01-T05-review-r2（全新独立，未参与执行/R1 验收/修复；只读审查 + /tmp 隔离复跑）
- 基线 HEAD：`5a8a8e24ab9e8e105da5132d16b175221a91d646`；分支 `codex/rust-tauri-migration`
- 被审对象：`R01-T05_REPAIR_R1.md`（ZCode:R01-T05-repair-r1）及其对 R1 发现 F1–F5 的修复
- 平台：macOS 27.0 arm64；Node v24.16.0；Python 3.14.3；Rust 1.98.1；Chrome 153.0.8010.52（实测一致）
- 日期：2026-09-25
- **最终判定：PASS**（F1–F5 全部关闭并经独立复跑/自造对抗样本复证；新发现 1 项 LOW 注释陈旧 + 2 项 INFO，均不推翻修复有效性）

## 0. 基线与工作区核实

`git rev-parse HEAD` = 基线一致。`git status --porcelain` 与修复报告末段清单逐项一致
（M 5 文件 + ?? 11 项；ORCHESTRATOR_PROGRESS.json 为预存总控账本改动，未触碰）。
`git diff HEAD -- desktop/ server/ core/ lib/ shared/ cli/ hub/ plugins/ skills2set/
package*.json rust/Cargo.lock` = **0 行**——生产零改动属实。
本复验全程未修改任何已提交文件与执行/修复交付物；所有复跑产物在 /tmp/r01t05-r2
（含 CARGO_TARGET_DIR=/tmp）；setBlockedURLs 复证用 /tmp/r01t05-r2/rust-sbu 源码副本，
未碰仓库源码。复验前后 `git status --porcelain | md5` 均为
`0ae29a9a28654c45bb9dbb8f6bea1911`——工作区零变化。
修复报告声称的 8 个改动文件 SHA-256 **全部复算一致**（8/8 OK）。

## 1. F1（MEDIUM）WebSocket 边界 —— 判定：关闭（CLOSED）

### 1.1 实现审查（pdf.rs:344-362, 425-437）

- `Page.addScriptToEvaluateOnNewDocument` 注入 meta CSP `connect-src file: data: blob:
  http: https:`——白名单刻意只排除 ws:/wss:；http(s)/file 不放行、照常穿透 Fetch 域
  allowlist 审计层（§4 实测 S5 七条 DENY 逐条保留，声明属实）。
- **fail-closed 属实（源码确证）**：注入调用在导航之前执行，`map_err(...)?` 直接使
  render_job 返回 Err → main 走失败路径（exit 1、零产物、清理照跑）；不存在静默降级分支。
- 注入脚本含文档根未创建时的 MutationObserver 兜底（见 1.2 竞态实测）。

### 1.2 既有负向套件独立复跑

命令：`bash tests/migration/r01-t05/run_repair_r1_negative.sh /tmp/r01t05-r2/negative`
（loopback-only 代理 18482 + canary 18291，全程 env -u 代理变量）。
**退出码 0，7/7 PASS**：ws-negative-exit（exit=0 不误伤渲染）/ ws-negative-canary
（ws://127.0.0.1:18291 握手零命中）/ ws-negative-pagemarker（WS-LOOPBACK-BLOCKED +
WS-REMOTE-BLOCKED）+ F2 四项（见 §2）。两 case 的 cleanup 字段均
leftover_processes 前后为空、profile_removed=true（本复验 run-result.json 实测）。

### 1.3 自造 WS 变体攻击（修复交付物之外新增，全部实测，canary 全程活跃）

统一口径：live canary 127.0.0.1:18291 + loopback-only 代理 + 修复后 spike_pdf
（/tmp/r01t05-r2/cargo-target 构建）；页面标记经 pdf_probe 文本层提取；canary 命中为最终判据。

| 样本 | 攻击向量 | 页面观测 | canary 命中 |
|---|---|---|---|
| ATK1 head 首内联脚本（抢 MutationObserver 微任务竞态） | `new WebSocket` 在文档第一个内联脚本 | WS-BLOCKED（首轮 + 追加 5 连复跑，6/6） | **0**（5 连复跑时 canary 活跃自验） |
| ATK2 `<script type="module">` 内 WS | 模块脚本延迟执行 | WS-BLOCKED | 0 |
| ATK3 srcdoc 子 iframe 内 WS | CSP 继承路径 | WS-BLOCKED | 0 |
| ATK4 `data:text/html` iframe 内 WS | opaque origin 路径 | 标记不可判定（主页面无回传通道） | **0** |
| ATK5 四通道同页（EventSource/fetch/sendBeacon/WS 打 loopback canary） | HTTP 家族 + WS 混合 | ES=BLOCKED FETCH=BLOCKED（Fetch 层）WS=BLOCKED（CSP 层） | 0 |
| ATK6 WebRTC | RTCPeerConnection createOffer | offer 创建成功且 SDP 含 host candidate——**见新发现 N2** | n/a（UDP） |
| ATK7b blob: iframe 内 WS（iframe 自写标记） | blob 继承 CSP | WS-BLOCKED（3/3 复跑） | 0 |
| ATK8 多阶段竞态（head 首脚本 / body 首脚本 / DOMContentLoaded / load / setTimeout0 / rAF / body 尾脚本，七路齐发） | 解析全阶段竞态 | p3-dcl/p4-load/p5-st0/p6-raf 全 BLOCKED（3/3 复跑） | **0** |

合计 9 类变体、竞态类累计 12 次复跑，**canary 零命中、零 WS-OPEN**。wss:// 远端由
repair 既有用例覆盖（WS-REMOTE-BLOCKED，页面 onerror；代理层同时 403，双保险）。
CSP meta 注入对首脚本竞态在 Chrome 153 实测稳健（原理：WS 的 CSP 检查发生在连接
建立步骤而非构造器同步段，注入窗口始终先于网络动作）。

### 1.4 setBlockedURLs 无效的负面证据 —— 独立复证为真

不信任 repair-r1/setblockedurls-ineffective/ 留证，自行复证：复制 rust/ 至
/tmp/r01t05-r2/rust-sbu，将 pdf.rs 的 CSP 注入**替换为** `Network.enable` +
`Network.setBlockedURLs(["ws://*","wss://*"])`（离线构建成功），对仓库原样本
ws-loopback.html 渲染。结果：**canary 实收 `GET /t05-ws-repair-probe` 一次**——
"Network.setBlockedURLs 对 WS 无效"声明独立成立（repair 留证方向一致）。
方法论注意：该运行中页面标记显示 WS-LOOPBACK-BLOCKED（握手到达但升级失败触发
onerror），证明**页面 onerror 标记单独不足以作为封堵证据，canary 零命中才是强判据**——
repair 负向用例的设计（双判据）正确。

**F1 结论**：封堵机制真实有效、fail-closed、不弱化既有边界、负面证据真实；关闭。

## 2. F2（LOW）超时 exit 码契约 —— 判定：关闭（CLOSED）

- 集成回归（本复验 §1.2 同次运行 case2，hang-after-load.html，embedLingxiFonts=false
  使挂死点落在 printToPDF 阶段）：**exit=2**、error="printToPDF: timeout:
  Page.printToPDF id=13"、零伪产物（hang.pdf 不存在）、profile_removed=true、
  leftover 进程前后为空。
- `error_is_timeout()`（spike_pdf.rs:101-103）覆盖 "timed out" 与 "timeout:" 两类文案；
  单测 `timeout_exit_code_classification`（6 断言：load/printToPDF/navigate 超时文案 +
  三类非超时错误）随 `cargo test` 实测通过（§4）。
- load 阶段超时契约未回归：本复验全量 replay 中 a10-infinite-new **exit=2**（S6 样本，
  "loadURL timed out after 5000ms"）、零产物、清理干净。

## 3. F3/F4/F5 勘误核对 —— 判定：全部关闭（CLOSED）

| 项 | 声称 | 本复验实测 | 结论 |
|---|---|---|---|
| F3a compare checks | 35 项 all_pass=true | 本复验 replay a09/compare.json checks=**35**、all_pass=true、0 fail | 属实 |
| F3b SHA256SUMS | 632 项 | 根 SHA256SUMS.txt **632 行**，`shasum -a 256 -c` 全量 **632/632 OK**；spike 报告页首已写 632 | 属实 |
| F4 cargo test | 修复后 45 passed | `cargo test --workspace --offline`：**11(lib)+1(bin)+7+19+7=45 passed, 0 failed**（含新增 bin 单测 timeout_exit_code_classification） | 属实 |
| F5 A09 差异构成 | 提取层伪差，不影响判据 | 本复验 replay 复现：74=74 页、140/140 行、相似度 **0.99910**（87,161/87,148）、网格差 max=7.16 mean=6.43、图片 2=2、三族字体双链嵌入、零空白页——与 R1 独立分析逐项同值 | 属实 |

## 4. 回归与既有负向保持（全部亲自复跑，命令 / 退出码）

统一环境：`env -u 全部代理变量`；浏览器类进程限 loopback（T04 proxy.mjs + canary）；
CARGO_TARGET_DIR=/tmp/r01t05-r2/cargo-target；数据目录 /tmp。

| 校验 | 命令 | 退出码 | 结果 |
|---|---|---|---|
| 全量重放 | `bash scripts/rust-tauri/r01-t05-replay.sh /tmp/r01t05-r2/replay` | **0** | 19 场景退出码全符合预期（a10-infinite-new=2、a10-infinite-old=1，其余 0）；含 generate_samples.py --check（样本零漂移） |
| 矩阵判定 | `python3 tests/migration/r01-t05/analyze_matrix.py /tmp/r01t05-r2/replay` | **0** | **28 VERIFIED / 0 FAILED**，矩阵值（MediaBox/inkBBox/DENY 计数/cleanup）与执行者证据逐项吻合 |
| S5 语义保持 | 复算 replay a10-dangerous/new/run-result.json + fetch-log.jsonl | — | **fetch_denies=7**（/etc/passwd Document+Image+XHR、master.passwd、/etc/hosts、canary.invalid、loopback canary），connect_boundary 字段在位，输出 PDF 无泄漏，profile_removed=true——**修复未弱化既有负向** |
| A09 双链 | replay a09/compare.json | — | 35 项全过（§3）；双链比较仍成立 |
| T01 正向 | `python3 docs/rust-tauri/R01/r01_t01_check_ownership.py` | 0 | — |
| T01 N1–N15 | `… --self-test` | 0 | — |
| T01 生成器 | `… r01_t01_build_ownership.py --check` | 0 | 736 features / 69 stores up-to-date |
| T02 roundtrip | `bash scripts/rust-tauri/r01-t02-roundtrip.sh` | 0 | — |
| T02 handshake | `bash scripts/rust-tauri/r01-t02-handshake.sh` | 0 | — |
| T02 check-generated | `bash scripts/rust-tauri/r01-t02-check-generated.sh` | 0 | — |
| cargo test | `cargo test --workspace --offline` | 0 | 45 passed（§3 F4） |
| cargo fmt | `cargo fmt --all --check` | 0 | — |
| cargo clippy | `cargo clippy --locked --workspace --all-targets --offline -- -D warnings` | 0 | — |
| 证据清单 | `shasum -a 256 -c SHA256SUMS.txt`（根 632）与同命令（repair-r1/ 653） | 0 | 两个清单分别全量 OK（见新发现 N3） |

性能参照（本复验 replay，非阈值口径）：候选 median 1712.9/p95 1728.9ms
（printToPDF 328.0ms，n=16 全 exit0）、旧链 median 987.0/p95 1023ms（n=8 全 exit0）——
与执行者/R1/repair 三轮同量级，比例关系（候选 ≈1.7× 旧链，冷启动归因）不变。
proxy.log REFUSED 293 条（R1 291 / 执行者 286 同量级，外联全部被代理层拒绝）。

## 5. 一致性核对

- **PDF_SPIKE_REPORT §8 第 7/8 条**：WS 缺口影响分析、setBlockedURLs 无效事实、CSP 修复
  与 fail-closed、负向用例 7/7、**R09 门禁（负责人=R09 任务执行者、截止=R09 验收前）**、
  F2 超时契约——逐条与代码/证据事实一致；§3（远端资源读取限制行）与 §5（WS 边界补记）
  同步更正。
- **ADR-003 后续强制门禁节**：WS 边界与超时 exit 码两条门禁均在位，负责人/截止明确
  （缺失即阻塞 R09 通过）。
- **T04 交叉影响登记如实**：spike 报告 §8 第 7 条末段登记「T04 spike_browser 无 Fetch
  拦截层、仅靠代理层，loopback WS 同样不受候选代码约束，留 T08 风险登记」——本复验
  源码核实 `spike_browser.rs` **Fetch.enable 零命中**（grep=0，资源约束确仅靠
  `--proxy-server` 形态），登记内容属实，且按口径未改 T04 交付。
- **任务书符合性**：R01-A09（无缺字/丢行/图片缺失/空白页、语义断言通过）与 R01-A10
  （越权拒绝、超时清理、无伪成功产物）通过条件在修复后全部保持成立；修复未引入
  占位实现或断言放宽（CSP 仅新增 ws:/wss: 排除，DENY 判据、超时判据均未放宽）。

## 6. 新发现问题（均不推翻修复有效性）

### N1（LOW）负向测试资产注释陈旧：封堵机制误注为 Network.setBlockedURLs

- `tests/migration/r01-t05/run_repair_r1_negative.sh` 头注释（case1 说明）与
  `tests/migration/r01-t05/negative/ws-loopback.html` 内注释仍写「必须被
  Network.setBlockedURLs 阻断」——与修复实际机制（CSP connect-src）及修复报告自己的
  结论（setBlockedURLs 实测无效）直接矛盾。仅注释问题：脚本断言逻辑（canary 零命中 +
  页面标记）与机制无关，判定结果不受影响。处置建议：下轮顺手更正注释，无需专轮。

### N2（INFO）WebRTC 位于 Fetch 域与 CSP connect-src 双重边界之外

- 本复验 ATK6 实测：候选链页面 `RTCPeerConnection.createOffer()` 成功且 SDP 含 host
  candidate。WebRTC 不受 CSP connect-src 管辖（规范层面），数据通道走 UDP/DTLS，
  loopback-only HTTP 代理亦不约束 UDP。未完成端到端外带演示（需对端 ICE+DTLS 栈，
  超出本复验成本预算），故不定性为可利用缺口；但 PDF_SPIKE_REPORT §8 第 7 条的
  「WS/连接边界」表述未提及 WebRTC，建议 R09 门禁在落实 WS 等价边界时一并显式决策
  WebRTC（如 launcher 层开关/策略禁用），避免边界清单不完整。

### N3（INFO）证据清单分裂两处，修复报告措辞可误读

- 根 `artifacts/rust-tauri/R01/T05/SHA256SUMS.txt`（632 项）**不覆盖** repair-r1/ 子树；
  repair-r1/SHA256SUMS.txt（653 项）单独覆盖该子树。两个清单本复验分别全量校验 OK，
  但修复报告末段「SHA256SUMS.txt 653 项」指的是后者，且根清单未随修复增量更新——
  后续核验者需同时校验两份。事实层面无缺漏（repair-r1 子树 653 文件全覆盖），记录以校准。

## 7. 范围与限制

- 仅 macOS arm64 + Chrome 153 实测；Windows/Linux 不在覆盖内（已登记 R09/R10 门禁）。
- fail-closed 为源码确证（注入在导航前、错误即 job 失败、零产物），未构造 CDP 层
  注入失败的实证注入故障（无低成本良性手段）。
- ATK4（data: iframe）页面标记不可判定，结论依赖 canary 零命中这一网络层强判据。
- WebRTC（N2）未做端到端外带验证，如实记录为边界完备性事项而非已证实缺口。
- 预存失败（审计封印家族等）与本任务无关，未追修。

## 8. 最终判定

**PASS**。R1 发现 F1–F5 全部关闭且经独立复跑与自造对抗样本复证：F1 的 CSP 封堵在
9 类 WS 变体（含解析全阶段竞态 12 次复跑）下 canary 零命中、不误伤渲染、fail-closed、
不弱化 S5 既有 7 条 Fetch DENY；setBlockedURLs 无效的负面证据经独立复证为真；
F2 双阶段超时 exit=2 契约实测成立；F3/F4/F5 勘误数字与实测逐值一致；全量回归
（replay 19 场景、28 VERIFIED、T01×3、T02×3、cargo test 45、fmt、clippy）全绿；
文档门禁条目（负责人/截止）与 T04 交叉影响登记属实；生产零改动、工作区复验前后
零变化。新发现 N1（LOW 注释陈旧）/N2（INFO WebRTC 边界表述）/N3（INFO 清单措辞）
均为披露项，不阻塞本任务判定，建议并入后续轮次或 R09 门禁清单处理。
