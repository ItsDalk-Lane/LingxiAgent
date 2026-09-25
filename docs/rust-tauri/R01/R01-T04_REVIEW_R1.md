# R01-T04 REVIEW R1 — 浏览器宿主替换原型（独立对抗性验收）

验收代理：ZCode:R01-T04-review-r1（未参与执行；只读审查 + /tmp 隔离复跑；未修改任何已提交文件、生产代码与执行者交付物；本报告是唯一新增仓库文件）
日期：2026-09-25｜分支 codex/rust-tauri-migration｜基线 HEAD `08b9f075032455be5f57f8fc882bf366b25b4269`（实测 `git rev-parse HEAD` 一致）
被审交付：执行者工作区差异 + `docs/rust-tauri/R01/{R01-T04_REPORT.md,BROWSER_SPIKE_REPORT.md,ADR-002-browser-host.md}` + `artifacts/rust-tauri/R01/T04/` + `rust/crates/lingxi-browser-spike/` + `tests/migration/r01-t04/`

**最终判定：PASS**（附两项低严重度修正要求 F1/F2，均为文档准确性修正，不推翻任何能力证据，见 §8）

---

## 1. 候选清单与复算哈希

`git status --porcelain` + `git diff --stat HEAD` 实测与声明的候选集一致：4 个修改文件（DEPENDENCY_DECISIONS.md / DEPENDENCY_RULES.json / rust/Cargo.toml / rust/Cargo.lock）+ 7 个新增路径（spike crate、tests/migration/r01-t04/、replay 脚本、三文档、artifacts）。任务书目录、`.sync-audit/`、`ORCHESTRATOR_PROGRESS.json` 均未出现在 diff 中（账本当前无未提交修改，本验收未触碰）。

哈希复算（验收代理独立执行，非转述）：

| 项 | 方法 | 结果 |
|---|---|---|
| 生产 SNAPSHOT_SCRIPT | 独立 Python 从 desktop/main.cjs 提取（marker `const SNAPSHOT_SCRIPT = \`` → 终止行 `})()\`;`） | lines 3288-3453，sha256=`387b2e10ee8e39abfbbf21b78f29c4046b519598e8f92d9a66f345d4c6f80e21`，与执行者三宿主证据逐字符一致 |
| 证据包完整性 | `shasum -a 256 -c artifacts/rust-tauri/R01/T04/SHA256SUMS.txt` | **51 项全 OK / 0 FAILED** |
| wry 0.57.0 crate | 重新下载 static.crates.io（剥离六个代理变量） | sha256=`a819957a01b3119af85e638a38d242af76dbc87d130dca67bfd0441072e21ff0`，与执行者取证一致 |
| Cargo.lock 增长 | `git diff HEAD` + 包计数 | 279→280 条目，唯一新增为 lingxi-browser-spike 自身；base64 0.23.1 / libc 0.2.189 / serde 1.0.229 / serde_json 1.0.151 / sha2 0.11.0 全复用锁定版本，「无新增第三方版本」声明属实 |

## 2. 逐场景独立复跑

证据分层：**[实跑]** = 本代理亲自执行（命令/退出码实测）；**[源码]** = 源码审查确证；**[环境]** = 环境限制。

复跑环境：测试站/代理为执行者未修改的源码（我只读使用），数据目录 `/tmp/r01t04-review/profiles*`，证据输出 `/tmp/r01t04-review/evidence/`，`CARGO_TARGET_DIR=/tmp/r01t04-review/cargo-target`，全部命令剥离六个代理变量。复跑后浏览器进程与 18281/18282 端口已清理（pgrep/lsof 复核为空）。

### Chromium main 阶段（A1-A14，20 步）— PASS [实跑]

```
/tmp/r01t04-review/cargo-target/debug/spike_browser run --site http://127.0.0.1:18281 \
  --repo <repo> --evidence /tmp/r01t04-review/evidence/chromium-main \
  --profile-root /tmp/r01t04-review/profiles --phases main
```
退出码 **0**，pass=21 fail=0（20 步 + M0 证据源）。关键断言独立成立：上传服务端回显 `sha256=9396d463…3093154` 与执行者证据逐字符一致（payload 确定性）；整页截图 1280x11878（PNG 头维度解析，非仅字节数）；A5 计数器 ==2；A6 echo 含中文标题+choice=c；A13 挂起后 textarea 值 `suspend-marker-保留` 保留；A14 冷重启后 `LOGGED_IN demo`；A1b `nc -z 127.0.0.1 9222` exit=1（无 TCP 调试口）。A10 弹窗复现了执行者披露的首击停滞+重试成功（`urlNavigated=false lastUrl="about:blank" [retry ok]`），F1 发现属实且重试路径真实工作。

### Chromium isolation 阶段（A08，B1-B6）— PASS [实跑]

退出码 **0**，7/7 VERIFIED。B2 双 context 回读 `local=ALPHA_A` vs `BRAVO_B` 无交叉；B3 cookie jar 按 browserContextId 查询各自独立；B4 探针页 `hana/process/require` 全 undefined、`cdpHttpJson=BLOCKED:TypeError`、`cdpWs=BLOCKED:error`；B6 dispose 后查询被 `-32602 Failed to find browser context` 拒绝。

### Chromium proxy 阶段（C1/C2）— PASS [实跑]

退出码 **0**。C1 死代理 `net::ERR_PROXY_CONNECTION_FAILED`；C2 活代理导航成功。**本代理自己的代理实例日志**（非执行者证据）独立重现了修补后行为：Chrome 后台对 `clients2.google.com`（HTTP GET）、`www.google.com`/`accounts.google.com`/`update.googleapis.com`/`content-autofill.googleapis.com`/`www.gstatic.com`（CONNECT）全部 `REFUSED`，loopback 的 `/form`、`/favicon.ico` 正常放行。这既验证了补丁有效性，也独立证实了泄漏向量的真实性（Chrome 确实会把此类后台流量打进代理）。

### Chromium takeover 阶段（D1-D4）— PASS [实跑]

退出码 **0**，5/5 VERIFIED。CGWindowList 原生枚举 `onscreen=1 layer=0 1280x860`；osascript System Events 独立 OS 通道键入 `HUMAN-TAKEOVER` 落进页面输入框且 CDP 读回；`screencapture -x` 全屏截图 1,934,696 bytes。[环境] 该阶段会在物理屏幕上弹出真实窗口并注入真实键击，属 macOS 本机验收的固有特性。

### WKWebView spike（W0-W16）— PASS [实跑]

`swiftc -O` 独立编译执行者源码后运行：退出码 **0**，**15 VERIFIED / 0 FAILED / 3 UNVERIFIED**，与执行者声明逐项一致。三项 UNVERIFIED 全部复现：W6（JS files 赋值 len=0，无 setFileInputFiles 等价物）、W13（`deadProxyNav=true err=none`——WK 对 loopback 隐式绕过代理，负向验证在红线约束下不可完成）、W16（`appActive=false keyWin=false`——未签名 CLI 不可激活，OS 键入不可达）。UNVERIFIED 标注诚实，未冒充 VERIFIED。

### Electron 42.8.1 旧侧对照（E0-E16 + P1/P2）— PASS [实跑]

main 模式退出码 **0**：17 VERIFIED / 1 UNVERIFIED（E6 上传，实测 `Cannot set properties of null`——生产契约缺口 C16 的如实记录）/ 0 FAILED；E11 cookie 剥离、E12 partition 隔离、E15（defaultSession 代理生效而浏览器 partition 未配置，C14 缺口实证）均复现。login-write/login-verify 两进程接力退出码均 **0**：P1 写入 → P2 全新进程同 user-data 读回 `LOGGED_IN demo`，跨进程登录持久化成立。

## 3. A08 对抗性测试（验收代理自造，非执行者用例）[实跑]

执行者用例之外，本代理编写独立 Rust 对抗驱动（`/tmp/r01t04-review/adv/`，path 依赖 spike crate 的公开 API，源码不入库），7 项全部 PASS：

| # | 攻击/验证 | 结果 |
|---|---|---|
| ADV-1 | 第三个全新 BrowserContext（攻击者会话）读 /storage-read | `local=null cookie=`——看不到 A 的 ADV_ALPHA 与 B 的 ADV_BRAVO |
| ADV-2 | 默认 context（无 browserContextId）读 /storage-read | 同样为空——命名 context 数据不向默认会话泄漏 |
| ADV-3 | 宿主侧查第三 context 的 t04 cookie jar | 0 条 |
| ADV-4 | 不可信页 `fetch('file:///etc/passwd')` | `BLOCKED:TypeError` |
| ADV-5 | 不可信页 iframe 嵌 `file:///etc/passwd` 并读 contentDocument | iframe 根本不加载（`TIMEOUT(no-load)`），读不到内容 |
| ADV-6 | 页面侧探测 127.0.0.1:9222/9229 CDP 端点 | 双端口均 BLOCKED（pipe 模式无 TCP 面） |
| ADV-7 | 伪造 browserContextId（32×'F'）宿主侧查 cookie | CDP `-32602 Failed to find browser context` 拒绝 |
| ADV-8（信息项） | 页面 `window.open('file:///etc/passwd')` | 弹窗拦截器返回 null（顺手观测，非判定依据） |

结论：A08 的「会话数据不串用、不可信页拿不到原生权限」在超出执行者用例的第三会话/默认会话/file:// 越权/伪造 ID 四个额外维度上独立成立。

## 4. 泄漏事故定性（§4 披露核实）

- **预修补日志核实** [源码]：`chromium-proxy/proxy-server-prepatch-LEAK-EVIDENCE.log` 内容与披露逐条相符——EADDRINUSE 崩溃头（与披露的第二起事故一致）+ **1 条明文 HTTP GET `clients2.google.com/time/1/current`** + **7 条 CONNECT**（www.google.com×2、content-autofill.googleapis.com×2、www.gstatic.com、update.googleapis.com、accounts.google.com）。数量与目标清单无出入。
- **泄漏内容定性**：明文 GET 是 Chrome 时间校准探针（cup2key/cup2hreq 为 Chrome 标准参数）；CONNECT 为盲隧道，代理只见目标主机名不见 TLS 内容。**无任务数据、无测试 payload、无账号/凭证外发**——披露的影响评估属实。
- **修补核实** [源码+实跑]：现行 `tests/migration/r01-t04/proxy.mjs` HTTP 与 CONNECT 双路径对非 `127.0.0.1`/`localhost` 目标一律 403/断连并落日志。本代理实测：经代理 `curl http://example.com/` → **403**；`curl https://example.com/`（CONNECT）→ 连接被毁（curl 000）；`curl http://[::1]:18281/healthz` → **403**（IPv6 loopback 也被保守拒绝，无绕过面）；loopback 正常 200。
- **修补后证据链**：`chromium-proxy/proxy-server.log` 显示重跑时 Chrome 对 Google 的全部后台请求均 REFUSED，loopback 放行；C1/C2 退出码 0。EADDRINUSE→run2 跑在旧代理上的过程披露与日志结构自洽。
- **对验收的影响**：事故属测试基础设施外发，已修补、复验、双向留证（预修补日志未销毁，符合如实披露红线）；最终采信的 proxy 阶段证据全部产自修补后代理。**不推翻验收**。遗留 hygiene 观察见 F2。

## 5. 能力矩阵抽查（现役契约锚点 + 语义级断言）[源码]

从 §1 矩阵独立抽 7 项核对 desktop/main.cjs 行级定位，全部属实：

| 抽查 | 声明锚点 | 本代理核实 |
|---|---|---|
| C1 分区命名 `persist:hana-browser-<sha256[:32]>` | 3465-3472 | 实测 3468-3470 `_browserPartitionName`，sha256→hex→slice(0,32) 语义一致 |
| C9 弹窗 deny+转新 tab | 3805-3809 | 实测 3805-3809 `setWindowOpenHandler` → `_openUrlInNewBrowserTab` + `action:"deny"` |
| C11 挂起不销毁 | 4106-4123 | 实测 `case "suspend"` 走 `_detachActiveBrowserView`（detach 非 destroy） |
| C12 cookie 剥离 | 3721-3748 | 实测 `onBeforeSendHeaders`/`onHeadersReceived` 删 Cookie/Set-Cookie |
| C14 代理只管 defaultSession | 291-308 | 实测 `applyDesktopNetworkProxy` 仅 `session.defaultSession.setProxy`，缺口声明属实 |
| C15/C16 下载/上传缺口 | grep 零命中 | 本代理独立 grep `will-download\|setFileInputFiles`：desktop/main.cjs、lib/browser/browser-manager.ts、lib/tools/browser-tool.ts 全 0 命中 |
| C17 不可信页 sandbox | 3984-3988 | 实测 WebContentsView `nodeIntegration:false, sandbox:true` 无 preload |

「CDP 接口存在≠功能齐全」核查（抽 2 项 VERIFIED 看断言深度）：**A8 上传**断言服务端回显的 sha256 等于本地 payload 哈希且 size/filename 三元组全匹配（值级，非「调用没报错」）；**A14 冷重启**断言浏览器进程关闭重启后 `/account` 仍渲染 `LOGGED_IN demo`（跨进程状态级）。另核 A5（计数器==2）、A6（echo 内容含中文与 choice）、B2（交叉值负向断言 `!contains`）均为语义级。合格。

## 6. ADR-002 合理性

- **候选 B 采纳有证据支撑**：四阶段 32 项断言本代理全部独立复跑通过（§2），无一项依赖转述。
- **WKWebView 拒绝理由成立**：W6/W13/W16 三项缺口本代理实测复现（§2 WKWebView 节）；其中 W13/W16 如实标 UNVERIFIED 而非虚构 VERIFIED，符合「不偷偷删功能/不假阳性」要求。
- **wry「API 面取证」如实**：报告明说未建 app、未实测，仅以 crate 源码取证替代（任务书 02 §9 允许「取证不支持」路径）。本代理重新下载 wry 0.57.0（sha256 与执行者证据一致）独立 grep 复核：截图/快照公共 API 零命中；`with_download_completed_handler` 文档明写 macOS 路径参数 always empty（lib.rs:1377-1381）；代理需 macOS 14+ 且 `mac-proxy` feature、Android/iOS 不支持（lib.rs:763-767）；WebContext 仅进程级 `data_directory`；file input 仅原生 runOpenPanel（wkwebview ui delegate:100）。五条取证全部属实，「wry 覆盖严格小于裸 WKWebView」的推导成立。
- **退出条件与后续门禁**（Windows/Linux UNVERIFIED → R09/R10、生产缺口 C14/C15/C16 进 R09 清单）明确写入 ADR，未把单平台实测夸大为跨平台结论。
- 快照算法等价性经 sha256 三宿主交叉核对（本代理独立重算一致），SNAPSHOT_SCRIPT 为运行时只读提取生产文件，无副本漂移风险。

## 7. T01/T02 门禁回归 [实跑]

| 校验 | 退出码 |
|---|---|
| `r01_t01_check_ownership.py`（正向） | **0** |
| 同 `--self-test`（N1-N15，实测 15 个负向用例全 PASS） | **0** |
| `r01_t01_build_ownership.py --check` | **0** |
| `r01-t02-roundtrip.sh` | **0** |
| `r01-t02-handshake.sh` | **0** |
| `r01-t02-check-generated.sh` | **0** |
| `cargo test --workspace --offline`（/tmp 隔离 target） | **0**（spike 7/7，kernel 7/7，protocol 19/19，其余 0 用例套件全绿） |
| `cargo fmt --all --check` | **0** |
| `cargo clippy --locked --workspace --all-targets --offline -- -D warnings` | **0** |

DEPENDENCY_RULES.json 的 `git diff HEAD` 语义级比对（JSON 解析后逐项 diff）：唯一实质变化 = 新增 lingxi-browser-spike 模块条目（kind=prototype）+ DEP-07 `applies_to_modules` 追加该 crate——属诚实收紧而非放水；其余差异为缩进重排。T01 校验器正向通过即机器证明 DEP-07 对新 crate 生效。已知预存失败（全量 npm test 审计封印 4 项 FAIL）与本任务无关，未追修符合任务书指示。

## 8. 发现问题

**F1（低，文档准确性）**：`DEPENDENCY_DECISIONS.md` D-09 引用的版本号「serde 1.0.228、serde_json 1.0.149」与锁定文件实际值（serde **1.0.229**、serde_json **1.0.151**）不符（spike Cargo.toml 声明的最低版本与 lock 一致，D-09 系引用陈旧）。实质主张「复用已锁定版本、无新增第三方版本」经本代理 lock diff 核实为真，仅引用数字陈旧。
- 最小重现：`grep -A1 '^name = "serde"$' rust/Cargo.lock` vs D-09 文本。
- 修复范围：D-09 两处版本号改为 1.0.229/1.0.151。不涉及任何证据或代码。

**F2（低，报告措辞 + 测试 hygiene）**：`BROWSER_SPIKE_REPORT.md` 方法段声称「无真实外发」，但非 proxy 阶段（main/isolation/takeover）浏览器直连运行，Chrome 后台 GCM 注册请求会真实到达 Google——本代理复跑 main 阶段 stderr 实测 `Registration response error message: DEPRECATED_ENDPOINT`（收到响应即证明完成了一次真实外联），执行者原始运行同二进制同参数必有同样行为，且该阶段未采信代理、无外发日志留存。性质与 §4 已披露事故同类（Chrome 自带遥测，无任务数据/凭证），但未被披露且与「无真实外发」的 blanket 声明矛盾。launcher 已有 `--disable-background-networking` 等缓解，但对 GCM 不完全有效。
- 修复范围：R01-T04_REPORT.md §4 或 BROWSER_SPIKE_REPORT.md §5 补一段如实说明（非 proxy 阶段存在 Chrome 自带后台外联尝试，内容与已披露事故同类）；可选加固：后续复跑脚本对全部阶段启用 loopback-only 代理或补充 `--disable-features`/host 级断网。不影响任何能力断言（全部断言针对 loopback 测试站内容），不需要重验。

两项发现均为文档修正级，不触及证据有效性、隔离结论与选型结论，故不构成 FAIL。

## 9. 最终判定

**PASS**。

- A07（等价性）：受控 Chromium 四阶段 32 项断言 + Electron 旧侧 17V/1U + P1/P2 全部由本代理独立复跑通过；生产契约矩阵抽查 7/7 属实；断言为语义级。
- A08（隔离）：执行者 B1-B6 复跑通过 + 本代理自造 8 项对抗检查（第三会话/默认会话/file:// 越权/伪造 context ID/CDP 面探测）全部通过。
- ADR-002 选型被独立证据支撑；wry 取证经独立下载复核；WKWebView 拒绝理由实测复现。
- 泄漏事故：披露准确、修补真实生效（本代理实测 REFUSED 含 IPv6 边界）、无任务数据外发、证据双向保留；不阻塞验收。
- T01/T02 门禁与 cargo 测试/fmt/clippy 全绿；DEPENDENCY_RULES 收紧诚实；生产目录零改动（`git diff HEAD -- desktop server core lib shared cli hub plugins skills2set package*.json` 实测为空）。
- 修正要求：F1/F2（§8）建议由执行者以文档修订收口，不阻塞本任务判定。
- Windows/Linux 维持 UNVERIFIED，R09/R10 强制门禁不变。
