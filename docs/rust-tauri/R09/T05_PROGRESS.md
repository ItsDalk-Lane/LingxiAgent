# R09-T05 浏览器、PDF 与媒体展示最终接线 — 实施进度

- 任务：R09-T05（验收 R09-A09 / R09-A10，均 REQUIRED），分支 `codex/rust-tauri-migration`，基点 HEAD `aac8cb9fe6bb`。
- 状态：**PARTIAL_GREEN — 本机可执行面 PASS；Tauri 产品宿主/安装面 BLOCKED（host-core T01/T02 尚未形成可测安装）**。
- 本文件只记录真实执行结果；未执行/环境阻塞项逐条登记，不冒充通过。不提交、不 push、不发布、不迁移真实数据。
- 证据根（未跟踪工作区产物，非候选实现）：`artifacts/rust-tauri/R09/dev/t05/`。
  - A09 逐断言扩展最终套件：`01-a09-per-assertion-final2/`（PASS，caseCount=70 / cases.json=69；
    36 条逐断言 `leaf:*` case，15 叶可绑定 + FILE-PREVIEW 部分 + 3 叶 BLOCKED）
  - A09 前一版最终套件：`01-a09-browser-pdf-final/`（PASS，71 case，服务级证据保留）
  - A10 最终套件：`02-a10-preview-final/`（PASS，53 case）
  - 旧链冻结基线：`old-chain-baseline/`（manifest + SHA256SUMS）
  - A09 早期红/重跑记录：`01-a09-base/`、`01-a09-browser-pdf/`、`01-a09-browser-pdf-02/`
  - A10 早期红/迭代记录：`02-a10-preview/`、`02-a10-preview-02/`、`02-a10-preview-03/`、`02-a10-preview-04/`

## 1. 结论摘要

1. **两个生产者套件已真实执行并通过**（经 `scripts/rust-tauri/r09_suites.py`，非 mock、非旧日志）：
   - `r09_t05_browser_pdf`：**PASS exit=0**，`result.json.caseCount=71`（`cases.json` 70 case + `suite::all-cases-hold`）。
   - `r09_t05_untrusted_preview`：**PASS exit=0**，`caseCount=53`（`cases.json` 52 + `suite::all-cases-hold`）。
2. **A09 真实能力面**：本机 Chrome 155.0.8059.85 跑通 R07 浏览器 10/10、PDF 7/7（`--test-threads=1`）；Electron 42.8.1 旧链在独立对照环境重生成 S1 基线（74 页、994832 B、与 R01 冻结 old probe 逐页 16×16 网格 |Δ|=0.0、文本 sha256 相同）；Rust 链产物与旧链基线 `compare_outputs.py` **35/35 all_pass**：74 页、140/140 表格行、文本相似度 0.99910、视觉网格 max 7.15（阈值 20）、三族字体嵌入、无空白页。
3. **逐原断言扩展（本轮裁决后）**：新增真实 Chromium 测试 target `r09_t05_assertions`（8/8）
   与 2 个真实服务测试；`r09_t05_browser_pdf` 最终套件 **PASS exit=0，caseCount=70
   （cases.json 69）**，其中 **36 条 `leaf:<id>::a<N>-<slug>` 逐断言 case 全部真实执行通过**；
   15 叶逐断言可绑定、`4173A0` 仅服务侧部分（整叶 pending T03/T06）、3 叶仍 BLOCKED（§6/§8）。
4. **A10 真实安全面**：真实 `lingxi-service`（隔离 home/临时端口/真实 local-token）+ 真实 Chrome 验证：Tauri Origin 精确匹配 3 放行/6 伪造 403；恶意 HTML 预览脚本确实执行但 `__TAURI__/__TAURI_INTERNALS__` 不可见、父/顶层 DOM/storage 全部 SecurityError、fetch 全 TypeError、beacon/img/ws loopback canary **0 命中**、wrapper Tauri canary **0 调用**、服务 bearer token 零出现、preview token 越权 401/404；伪造祖先帧被 CSP `frame-ancestors` 真实拒绝；SVG 经 `<img>` 脚本不执行；正常 HTML/SVG 预览保留。
5. **15/19 个 T05 叶逐原断言可绑定**（A09 套件）；1 叶（文件预览）仅服务侧部分、整叶 pending；
   3 叶需 Tauri 产品宿主/窗口/UI，保持 BLOCKED 未绑定（见 §6/§8）。三平台缺真机 → `BLOCKED_PLATFORM`。
6. **宿主桥接口已交付并被 host-core 合并注册**：`desktop/src-tauri/src/browser_viewer.rs`（5 命令/state/事件/安全边界/单测），host-core 已加 `pub mod`/`.manage`/`invoke_handler`/`build.rs AppManifest`/`capabilities/browser-viewer.json`/main 能力（本轮未改这些文件）。
7. **自动化边界修复（用户可见系统弹窗）**：所有测试/诊断 Chrome 加 `--use-mock-keychain`，进程组整组清理并逐次断言无残留；产品 launcher 默认不变，只在测试环境变量 `LINGXI_BROWSER_TEST_NO_KEYCHAIN=1` 下注入 mock keychain（见 §4.7）。

## 2. 本轮交付物（写入范围）

| 文件 | 用途 | 状态 |
|---|---|---|
| `scripts/rust-tauri/r09_t05.py`（新） | A09/A10 双套件生产者：真实 Chrome/Electron 参考链/真实服务/真实浏览器 harness + `Producer` 回执/cases | 完成 |
| `tests/migration/r09-t05/fixtures/malicious.html`（新） | A10 恶意 HTML 预览样本（脚本/IPC/跨源 DOM/网络外发/父窗口 token 请求） | 完成 |
| `tests/migration/r09-t05/fixtures/benign.html`（新） | A10 正常 HTML 预览对照 | 完成 |
| `tests/migration/r09-t05/fixtures/malicious.svg`（新） | A10 恶意 SVG（CDATA 包裹脚本/onload/foreignObject/file:// 与 canary 外链） | 完成 |
| `tests/migration/r09-t05/fixtures/benign.svg`（新） | A10 正常 SVG 对照 | 完成 |
| `tests/migration/r09-t05/fixtures/wrapper.html`（新） | 真实 Chrome 祖先帧模拟页（注入 Tauri canary、sandbox iframe、`<img>` 模式） | 完成 |
| `rust/crates/lingxi-adapters/tests/r09_t05_assertions.rs`（新，本轮扩展） | 逐原断言真实 Chromium 测试（TABS/NAVIGATION/BROWSER-TOOL 共 8 测试） | 8/8 PASS（`--test-threads=1`） |
| `rust/crates/lingxi-service/tests/r07_browser_routes.rs`（追加 1 测试） | 偏好/会话投影只读、无效输入显式、普通关闭真实 Stop 不撤权 | 11/11 PASS |
| `rust/crates/lingxi-service/tests/r07_web_tools.rs`（追加 1 测试 + 1 fixture 分支） | WEB-FETCH 阻断页/非法 URL/重定向上限显式失败且不写工作区 | 9/9 PASS |
| `desktop/src-tauri/src/browser_viewer.rs`（新，**已由 host-core 注册**） | Tauri browser-viewer 宿主桥接口草案：state/5 命令/事件/安全边界/单测 | 完成，编译校验见 §7 |
| `rust/crates/lingxi-adapters/src/browser/launcher.rs`（改） | test-only `LINGXI_BROWSER_TEST_NO_KEYCHAIN=1` → `--use-mock-keychain`；生产默认不变 | 完成 |
| `artifacts/rust-tauri/R01/T05/a09/{new,old}/probe/*`（恢复） | 从 git `15fa0cee3^` 恢复 R01 冻结基线（**未跟踪兼容性恢复，非 R09 候选实现**） | 完成，provenance 落盘 |
| `docs/rust-tauri/R09/T05_PROGRESS.md`（新，本文件） | 进度/证据/红绿/映射/阻塞/接口 | 完成 |

**未修改**（按写作用域）：`desktop/src-tauri/src/lib.rs`、`Cargo.toml`、`tauri*.json`、`build.rs`、`capabilities/**`、`package.json`/锁文件、`rust/crates/xtask/**`、`stage_maps/R09*.json`、`scripts/rust-tauri/r09_{suites,producer,stage_inputs}.py`、`docs/rust-tauri/R09/{PROGRESS.md,BLOCKED.md,IMPLEMENTATION_PLAN.md}`。未提交、未 push、未发布。

## 3. A09 证据（移除 Electron 后浏览器/PDF 能力）

### 3.1 环境（`01-a09-browser-pdf-final/environment.json`）

| 项 | 值 |
|---|---|
| 平台 | macOS arm64（本机唯一；macOS x64/Windows/Linux = `BLOCKED_PLATFORM`） |
| Chrome（实测受控浏览器） | `/Applications/Google Chrome.app/Contents/MacOS/Google Chrome`，`Google Chrome 155.0.8059.85`，sha256 `6e156f22…3bd1c9d` |
| Rust | `rustup run 1.98.1 cargo`（工作区锁定） |
| Node / Python | v24.16.0 / 3.14.3 |
| Electron（**仅旧链对照环境**） | v42.8.1（`node_modules/.bin/electron`），不进测试安装 |
| 隔离 | 每次套件 `LINGXI_HOME`/`TMPDIR` 在 `/private/tmp/lingxi-r09-t05/runtime-*`；`isolation::real-home-untouched` 前后摘要断言 PASS |

### 3.2 旧链基线重生成与冻结（A09-08）

- 独立对照环境：Electron 旧链 `desktop/src/office-pdf-helper.cjs` 经 `tests/migration/r01-t05/run_old_chain.mjs` + 隔离 harness 运行；隔离 HOME/TMPDIR/`R01_T05_OLD_TMP`，仅 loopback 预留代理，无真实网络。
- S1 中文长文档（74 页/140 行表格/三族字体）：`exit 0`，`s1-long-zh.pdf` = **994832 B**（与 R01 归档 old PDF 同字节数）；PDFKit probe：**74 页**。
- 与 R01 冻结历史交叉核对（`old-chain-baseline/manifest.json`、`crossCheckVsR01FrozenHistory`）：
  - R01 冻结 old probe 来自 git `15fa0cee3^`（git blob sha1 `76db4518c2…`）。
  - 页数相等 74=74；逐页 16×16 luma 网格 **max |Δ|=0.0**；文本长度 91198=91198；**文本 sha256 相同**（`30715233…`）→ 重生成基线可信。
- 冻结目录：`artifacts/rust-tauri/R09/dev/t05/old-chain-baseline/`（`manifest.json` + `SHA256SUMS` + `job-s1.json` + PDF + probe）。
- R01 冻结基线兼容性恢复（Approved）：来源 commit `15fa0cee3^`；4 文件 git blob sha1/sha256 记录在 `r01-baseline-provenance.json`：
  - `new/probe/probe.json` git blob `69744cb4c6…`、`new/probe/text.txt` `12bab09e5f…`、`old/probe/probe.json` `76db4518c2…`、`old/probe/text.txt` `afa556f924…`。
  - 标注“**兼容性恢复的未跟踪工作区文件，非 R09 候选实现**；R07 判据/历史证据不变”。（R07 质量门禁因此可原样复跑，不改变判据。）

### 3.3 Rust 链 PDF 真实质量门禁

命令：`cargo test -p lingxi-adapters --test r07_pdf_chromium -- --include-ignored --test-threads=1`（真实 Chrome）；**7/7 PASS**（`r07-pdf-chromium-tests.json`）。逐项 case：

| case | 证明 |
|---|---|
| `a09::pdf-chromium-7-green` | 7/7 串行真实执行 |
| `a09::pdf-s1-quality-rendered-real-chromium` | S1 文本/表格/字体/图片/分页/JS 开关真实渲染 |
| `a09::pdf-network-canary-zero-hit` | Fetch allowlist：iframe/img/file:///worker/WS/fetch/beacon 到 loopback canary 零命中 |
| `a09::pdf-timeout-cancel-cleanup` | 总超时/取消后清理确认（租约/进程/临时目录） |
| `a09::pdf-broken-assets-explicit-failure` | 坏字体/未冻结样式显式失败（不静默回退） |
| `a09::pdf-print-isolates-logged-in-workspace` | 打印不切换/清空已登录工作区 |
| `a09::pdf-lease-scope-and-quota` | 双租约 scope/配额/取消 future |
| `a09::pdf-static-utf8-cleanup-confirmed` | 一次性进程/私有目录清理确认后才返回 PDF |

> 运行参数边界：默认并行（cargo 默认线程数）下 7 个测试并发超过 `pdf.rs::MAX_LEASES=2` 进程级上限 → `PDF_CAPACITY`；并行红日志保留于 `01-a09-base/r07_pdf_chromium.log`。`--test-threads=1` 是**运行参数**而非产品缺陷；README/套件已固定串行。

### 3.4 旧链 vs Rust 链 A09 产物比对（`a09-compare.json`，35/35 PASS）

| 判据 | 结果 |
|---|---|
| 页数 | old=74 new=74（相等） |
| 异常空白页 | old=[] new=[] |
| 表格行 | 140/140 双链 |
| 文本归一化相似度 | **0.99910**（≥0.99） |
| 16×16 视觉网格 | **max 7.15 / mean 6.45**（阈值 20，R01 归档 7.16） |
| 三族字体 | EBGaramond/NotoSerifSC/JetBrainsMono 双链嵌入 |
| 图片 XObject | 双链存在（R01 判据保留） |

### 3.5 浏览器与服务回归（真实执行）

| 命令 | 结果 |
|---|---|
| `cargo test -p lingxi-adapters --test r07_browser_chromium -- --test-threads=1` | **10/10 PASS**（真实 Chrome；串行） |
| `cargo test -p lingxi-adapters --test r07_browser_store` | 3/3 PASS |
| `cargo test -p lingxi-adapters --test r09_t05_assertions -- --test-threads=1`（新） | **8/8 PASS**（真实 Chromium 逐断言） |
| `cargo test -p lingxi-service --test r07_browser_routes` | **11/11 PASS**（含追加逐断言测试） |
| `cargo test -p lingxi-service --test r07_browser_tools` | 13/13 PASS |
| `cargo test -p lingxi-service --test r07_web_tools` | **9/9 PASS**（含追加逐断言测试） |
| `cargo test -p lingxi-service --test r07_office_tools` | 15/15 PASS |
| `cargo test -p lingxi-service --test r06_t05_preview` | 5/5 PASS |

### 3.6 真实 Chromium 测试抖动记录（不删除）

- 并行默认线程首跑：`r07_browser_chromium` 9/10，`independent_persistent_profiles_auxiliary_locale_and_site_data_cleanup` 在 cookie 策略切换后 Resume 阶段 `browser_navigation [net::ERR_SOCKET_NOT_CONNECTED]`（与 R07 保留失败记录同阶段）。原始红日志：`01-a09-base/r07_browser_chromium.log`。
- 串行隔离重跑：10/10 PASS（`01-a09-base/r07_browser_chromium_serial.log`）。
- 生产者在 `01-a09-browser-pdf` 首跑出现另一次真实 Chrome 抖动：`Page.getNavigationHistory (CDP -32000: Not attached to an active page)`（`same_real_target_input_snapshot_generations_and_empty_workspace`），原始日志 `01-a09-browser-pdf/r07-browser-chromium-attempt1.log`。
- 处置：生产者对真实 Chrome 集成测试实现**至多一次**有界重试，每次都保留原始日志；最终套件最终首跑即绿（`01-a09-browser-pdf-final/r07-browser-chromium-tests.json` attempts=1、exit 0、10/10）。这是对 R07 已登记真实 Chromium 抖动的工程处置，不弱化断言、不覆盖红历史。

## 4. A10 证据（恶意预览内容无宿主权限）

### 4.1 套件回执（`02-a10-preview-final/result.json`）

- **status=PASS exit=0，caseCount=53**；`isolation::real-home-untouched` PASS；服务 SIGTERM 退出码 0；8 次真实 Chrome 运行 `chrome::process-tree-cleaned::*` 全部 PASS（ps 零残留）。
- 环境：真实 `lingxi-service` 由 `cargo build` 产物启动（`--home` 隔离、`--bind 127.0.0.1:0`、真实 local-token 只存宿主内存，不写入证据）；真实 Chrome 155.0.8059.85（`--headless=new --use-mock-keychain`，host-resolver-rules 映射 evil.example→127.0.0.1）。

### 4.2 Tauri Origin 精确匹配（真实 HTTP，`origin-matrix.json`）

| Origin | 结果 |
|---|---|
| `tauri://localhost` / `http://tauri.localhost` / `https://tauri.localhost` | **200**（精确放行） |
| `tauri://evil` / `tauri://localhost.evil.example` / `http://evil.tauri.localhost` / `http://tauri.localhost.evil.example` / `http://tauri.localhost:8080` / `http://127.0.0.1.evil.example` | **403 bad_origin** |

预览读取/创建面同样受该 Origin 闸：伪造 Origin 的 preview GET → 403；`tauri://localhost` 正常 200。

### 4.3 恶意 HTML 预览：真实 Chrome 沙箱 iframe（postMessage 自证）

`malicious-report.json`（祖先帧 `127.0.0.1:*`，CSP 显式允许来源）：

| 攻击面 | 实测 |
|---|---|
| 脚本是否运行 | **运行**（同步 `postMessage({kind:"r09-a10-preview-alive"})` 自证，父 wrapper 记录） |
| `window.__TAURI__` / `__TAURI_INTERNALS__` | `undefined` |
| `parent.__TAURI_INTERNALS__.invoke` / `parent.__TAURI__.core.invoke` / `parent.hana.fs.readFile` | 全部 `SecurityError` |
| `parent.document` / `parent.location` / `parent.localStorage` / `top.document` / `top.localStorage` | 全部 `SecurityError` |
| fetch 服务 API / fs / 相对路径 / canary token（4 条） | 全部 `rejected: TypeError`（CSP `connect-src 'none'`） |
| beacon / Image / WebSocket / popup | 发起即被阻断（canary 0 命中；popup null） |
| wrapper Tauri canary 调用 | **0** |
| 父窗口 token 请求 | wrapper 记录 `tokenRequest` 并拒绝 |
| 恶意页可见凭据 | 仅自身 43B preview token；服务 bearer token 零出现 |

### 4.4 frame-ancestors 真实浏览器行为

- 允许祖先：`http://127.0.0.1:*`、`http://localhost:*`（CSP 显式 `:*`）→ 预览 iframe 正常加载、脚本执行。
- 拒绝祖先：`http://evil.tauri.localhost:PORT`、`http://evil.example:PORT`（host-resolver 映射）→ iframe 未加载；Chrome CONSOLE 明确 `Framing 'http://127.0.0.1:…/' violates the following Content Security Policy directive: "frame-ancestors …" The request has been blocked.`
- CSP 文本断言：`frame-ancestors 'self' file: http://127.0.0.1:* http://localhost:* tauri: http://tauri.localhost https://tauri.localhost`（scheme-source `tauri:` + 两个精确 host-source；无 `tauri://*`/`*.tauri.localhost` 通配）。
- **CSP 端口语义观测（如实登记）**：host-source `http://tauri.localhost`（无端口）按 CSP 规范只匹配默认端口，而真实 Tauri Windows/Linux origin 即无端口形态；本机 harness 使用非特权高位端口，故 `tauri.localhost:PORT` 祖先被 Chrome 规范拒绝（case `a10::csp-host-source-no-port-matches-default-port-only`，属预期观测，不是失败）。Tauri 真机 webview 帧嵌入回归归 T06/T07。

### 4.5 SVG / 正常预览对照 / token 越权

- 恶意 SVG 经 `<img>`（MediaViewer 语义）：SVG 可渲染、内联/foreignObject/onload 脚本**无执行 marker**、`file://` 与 canary 外链 loopback **0 命中**。
- 正常 SVG `<img>` 正常加载；正常 HTML 预览 iframe 正常加载并回传 `BENIGN-MARKER-2026`。
- preview token：调用 `/lingxi/v1/browser/workspaces` → 401；读取其他 preview id → 404；无 Bearer 读取 `/lingxi/v1/fs/read` → 401。服务 bearer token 在预览字节/DOM/URL 中**零出现**。

### 4.6 回归

- `r06_t05_preview` 5/5 PASS（CSP/token/素材域/50MB/HTTP 面）。
- `r07_pdf_chromium::html_network_frames_and_workers_never_reach_loopback_canary` 1/1 PASS（PDF 资源门禁仍为 `connect-src 'none'` 强边界）。

### 4.7 自动化环境边界（用户可见系统弹窗修复）

- 事件：自动化/无钥匙串访问会话中启动真实 Chrome，macOS 弹出 “找不到用于储存‘Chrome’的钥匙串” 系统窗口（Chrome Safe Storage 行为，非 Lingxi 产品弹窗）。
- 处置：所有**测试/诊断** Chrome 启动加 `--use-mock-keychain`；改用 Popen `start_new_session=True`，退出后 `killpg(SIGKILL)` 整组清理并逐次断言 `chrome::process-tree-cleaned::<label>`；本轮 A10 8 个 Chrome 运行全部零残留（`ps` 复核 0 进程）。
- **产品面不改变**：`launcher.rs` 仅在测试环境变量 `LINGXI_BROWSER_TEST_NO_KEYCHAIN=1`（producer 对全部 cargo 测试设置）时追加 `--use-mock-keychain`；生产/真实用户会话不设置，cookie 加密语义保持真实默认。
- 遗留：用户若仍看到历史弹窗点“取消”即可；自动化不再复现。

## 5. 红 → 绿历史（全部原始证据保留，不覆盖）

| 证据目录 | 状态 | 原因/处置 |
|---|---|---|
| `01-a09-base/` | 部分红 | 默认并行： pdf `PDF_CAPACITY`（MAX_LEASES=2 并发上限）、browser 9/10（cookie-resume 抖动）；→ 串行重跑 7/7、10/10 绿；红日志保留 |
| `01-a09-browser-pdf/` | FAIL | 真实 Chrome flake `Not attached to an active page` 致 browser 测试 1 项红；→ 实现至多一次有界重试并保留 attempt1 红日志 |
| `02-a10-preview/` | FAIL（6 failed） | CSP 断言按字面找 `tauri://localhost`、iframe 计时、SVG XML 非法、tauri.localhost 非默认端口语义；→ 逐项修正 |
| `02-a10-preview-02/` | FAIL（5 failed） | 沙箱跨源 iframe 的 setTimeout 在 headless dump-dom 前不保证运行 → 改用 postMessage + 微任务自证 |
| `02-a10-preview-03/` | FAIL（2 failed） | 完整报告依赖定时器 → 微任务链 + 立即 alive 探针 |
| `02-a10-preview-04/` | PASS（53） | 真实证据；保留 |
| `01-a09-browser-pdf-02/` | PASS（54） | 真实证据；保留 |
| `01-a09-browser-pdf-final/` | PASS（71） | **A09 最终**（含旧链 manifest/SHA256SUMS 与新增 leaf case） |
| `02-a10-preview-final/` | PASS（本文件 §4） | **A10 最终** |
| （本地迭代，未留目录） | FAIL ×2 | 逐断言扩展期 producer 的旧链基线复用 helper 两处缺陷（manifest schema 兼容 / job-s1.json 未写）→ 修正后全新目录重跑；属本地 harness 迭代，不是门禁证据，如实登记 |
| `01-a09-per-assertion-final/` | PASS（70） | 逐断言扩展首跑；coverage 记账把 FILE-PREVIEW 计入 verified 的 bug，已被 final2 取代（证据保留） |
| `01-a09-per-assertion-final2/` | PASS（70 / cases.json 69） | **逐断言最终**：36 条 `leaf:*` case；15 叶可绑定 + 1 叶部分 + 3 叶 BLOCKED；复用 hash 校验冻结旧链基线（未启动 Electron） |

## 6. 19 叶逐原断言 → case 最终映射（供 Lead 唯一登记 FULL_BINDINGS）

- 命名：`leaf:<leafId>::a<N>-<slug>`（N = 原断言序号，从 1 起）；同一真实测试可作为
  多条断言的证据来源，但每个 case 记录该断言专属 detail 与其依赖的真实执行测试。
- 最终套件：`r09_t05_browser_pdf`（**PASS exit=0，caseCount=70，cases.json=69**），
  证据 `artifacts/rust-tauri/R09/dev/t05/01-a09-per-assertion-final2/`
  （逐断言清单 `leaf-assertion-cases.json`；36 条 `leaf:*` case 全部真实执行通过）。
- 新增真实执行面：`rust/crates/lingxi-adapters/tests/r09_t05_assertions.rs`（真实
  Chromium 8 测试）＋ `r07_browser_routes.rs`/`r07_web_tools.rs` 追加 2 个真实服务测试；
  均由同一 producer 运行、同一 cases.json 绑定。
- 覆盖：15 叶逐断言可绑定（下表逐行）；`R00-T02-LA-4173A08A51CC` 仅服务侧断言
  有 case、**整叶 pending T03/T06 不登记**；3 个 viewer/设置页叶 BLOCKED。
- 平台：以下只绑定 **macos-arm64**；macOS x64/Windows x64/Linux x64 = `BLOCKED_PLATFORM`。

| 叶 ID | feature | 断言 | case 名 | 真实执行测试（证据） | 结果 |
|---|---|---|---|---|---|
| `R00-T02-LA-A67D342381C0` | BROWSER-TABS | A1 | `leaf:R00-T02-LA-A67D342381C0::a1-tabs-and-active-sync-server-snapshot` | `browser_tabs_a1_tab_set_and_active_follow_real_target_changes` | 真实执行 PASS：真实 Chromium target 集合/活动标签逐步同步；服务端快照跟随变化（viewer DOM 投影属 T06） |
| `R00-T02-LA-A67D342381C0` | BROWSER-TABS | A2 | `leaf:R00-T02-LA-A67D342381C0::a2-invalid-tab-and-close-failure-explicit` | `browser_tabs_a2_invalid_tab_id_and_close_failure_are_explicit_and_state_unchanged` | 真实执行 PASS：协议非法 tabId→invalid_browser_command 不执行；未知 tab→browser_stale_tab；关闭失败如实报错且状态不变（不得显示已关闭） |
| `R00-T02-LA-A09B78985A14` | BROWSER-EMERGENCY-STOP | A1 | `leaf:R00-T02-LA-A09B78985A14::a1-emergency-stop-revoke-until-new-user-run` | `actual_http_command_replay_changed_input_and_emergency_receipt_never_repeat_external_actions`、`new_actual_user_submission_releases_emergency_stop_with_a_new_epoch` | 真实执行 PASS：close-session revoke=true 关闭并撤销授权；旧请求 403；新用户提交产生新 epoch 才释放 |
| `R00-T02-LA-A09B78985A14` | BROWSER-EMERGENCY-STOP | A2 | `leaf:R00-T02-LA-A09B78985A14::a2-no-legacy-local-instance-and-close-failure-visible` | `unknown_external_revoke_receipt_never_repeats_the_stop_or_reopens_content` | 真实执行 PASS：Tauri 无 Electron 旧实例（close_browser_viewer 本地销毁结构断言，T06 运行时待回归）；关闭失败可见：409 effectUnknown + BROWSER_CLEANUP_UNCONFIRMED 状态投影，不隐藏窗口冒充成功 |
| `R00-T02-LA-8A45B19A18B4` | BROWSER-NAVIGATION | A1 | `leaf:R00-T02-LA-8A45B19A18B4::a1-real-back-forward-reload-url-title-flags` | `browser_navigation_a1_back_forward_reload_update_url_title_and_flags` | 真实执行 PASS：真实后退/前进/刷新更新 URL/标题/canGoBack/canGoForward 与服务端快照 |
| `R00-T02-LA-8A45B19A18B4` | BROWSER-NAVIGATION | A2 | `leaf:R00-T02-LA-8A45B19A18B4::a2-no-active-view-and-illegal-urls-explicit` | `browser_navigation_a2_no_active_view_and_illegal_urls_fail_explicitly` | 真实执行 PASS：无活动 view 的 Back/Forward/Reload/Snapshot/Screenshot/Scroll 显式 browser_empty_workspace；javascript:/file:/data:/ftp:/无 scheme/userinfo 地址 browser_url_forbidden 且不改地址 |
| `R00-T02-LA-7D212CBD9EEF` | SEARCH-VERIFY | A1 | `leaf:R00-T02-LA-7D212CBD9EEF::a1-verify-success-persists-only-after-real-success` | `verification_real_http_masks_same_provider_and_only_persists_after_success` | 真实执行 PASS：真实外部验证成功后保存 provider/key；失败时不保存（before 快照逐字段相等） |
| `R00-T02-LA-7D212CBD9EEF` | SEARCH-VERIFY | A2 | `leaf:R00-T02-LA-7D212CBD9EEF::a2-verify-reject-no-widened-persistence-visible-error` | `verification_real_http_masks_same_provider_and_only_persists_after_success`、`provider_auth_redirect_and_rate_limit_keep_real_error_diagnostics` | 真实执行 PASS：拒绝/无效输入无越权持久化（跨供应商掩码不发请求、密钥不回显）；错误对用户可见（ok=false + 真实 errorCode/诊断） |
| `R00-T02-LA-1809C8E13EA3` | BROWSER-PREFERENCES-READ | A1 | `leaf:R00-T02-LA-1809C8E13EA3::a1-read-prefs-and-running-state-no-write` | `preferences_and_session_projection_are_read_only_and_close_without_revoke_is_real` | 真实执行 PASS：GET /preferences/browser 返回设置；GET /session-states 返回运行中状态；磁盘与 BrowserManager 设置前后不变（不写） |
| `R00-T02-LA-1809C8E13EA3` | BROWSER-PREFERENCES-READ | A2 | `leaf:R00-T02-LA-1809C8E13EA3::a2-put-apply-no-widened-write-or-disclosure` | `partial_preferences_preserve_unknown_fields_and_external_cookie_failure_is_not_success`、`chat_device_reads_state_but_only_explicit_remote_admin_controls_the_actual_service_node` | 真实执行 PASS：PUT 后 apply 失败不扩大写入（未知字段 400、设置/磁盘不变）；非本机 owner 读全局偏好 403（不扩大披露范围） |
| `R00-T02-LA-03A9E92909C6` | BROWSER-CLEAR-SITE-DATA | A1 | `leaf:R00-T02-LA-03A9E92909C6::a1-clear-site-data-success-and-failure-receipt` | `independent_persistent_profiles_auxiliary_locale_and_site_data_cleanup`、`partial_preferences_preserve_unknown_fields_and_external_cookie_failure_is_not_success` | 真实执行 PASS：真实 Chromium 站点数据清理后 cookie 消失（clear_site_data 后 Resume/Eval 为空）；服务 clear-cookies 成功 ok=true、失败 BROWSER_CLEAR_FAILED 非 2xx |
| `R00-T02-LA-03A9E92909C6` | BROWSER-CLEAR-SITE-DATA | A2 | `leaf:R00-T02-LA-03A9E92909C6::a2-clear-state-matches-actual-side-effect` | `partial_preferences_preserve_unknown_fields_and_external_cookie_failure_is_not_success` | 真实执行 PASS：失败/拒绝时返回状态与实际外部副作用一致（clear_calls 真实计数、命令零派发） |
| `R00-T02-LA-ED5FD5B469B8` | BROWSER-PREFERENCES-WRITE | A1 | `leaf:R00-T02-LA-ED5FD5B469B8::a1-update-prefs-save-and-return-new-state` | `partial_preferences_preserve_unknown_fields_and_external_cookie_failure_is_not_success` | 真实执行 PASS：PUT 保存未知字段保留并将新状态落盘/返回 BrowserManager 实际设置 |
| `R00-T02-LA-ED5FD5B469B8` | BROWSER-PREFERENCES-WRITE | A2 | `leaf:R00-T02-LA-ED5FD5B469B8::a2-apply-failure-400-readback-no-auto-rollback` | `partial_external_configuration_is_rolled_back_and_unconfirmed_rollback_is_explicit_unknown` | 真实执行 PASS：应用失败非 2xx 返回 ok=false；外部部分失败时回滚到实际旧策略；回滚未确认返回 409 effectUnknown（不假设自动回滚） |
| `R00-T02-LA-367052089D11` | BROWSER-SESSION-STATES | A1 | `leaf:R00-T02-LA-367052089D11::a1-states-running-resumable-unavailable-projection` | `restart_restores_cold_metadata_and_confirmed_receipt_without_automatically_opening_a_page`、`preferences_and_session_projection_are_read_only_and_close_without_revoke_is_real` | 真实执行 PASS：session-states 返回 running/executionNode/url；冷重启后 running=false、resumable=true |
| `R00-T02-LA-367052089D11` | BROWSER-SESSION-STATES | A2 | `leaf:R00-T02-LA-367052089D11::a2-states-invalid-unauthorized-downstream-explicit` | `chat_device_reads_state_but_only_explicit_remote_admin_controls_the_actual_service_node`、`preferences_and_session_projection_are_read_only_and_close_without_revoke_is_real` | 真实执行 PASS：权限不足（chat-only 设备控制被 403、全局偏好 403）与无效输入显式错误，不呈现成功 |
| `R00-T02-LA-593534A4F2AB` | BROWSER-SESSIONS | A1 | `leaf:R00-T02-LA-593534A4F2AB::a1-sessions-list-cold-url-retained` | `restart_restores_cold_metadata_and_confirmed_receipt_without_automatically_opening_a_page`、`preferences_and_session_projection_are_read_only_and_close_without_revoke_is_real` | 真实执行 PASS：GET /browser/sessions 返回冷恢复会话目录与旧网址（读取目录不启动页面） |
| `R00-T02-LA-593534A4F2AB` | BROWSER-SESSIONS | A2 | `leaf:R00-T02-LA-593534A4F2AB::a2-sessions-invalid-unauthorized-explicit` | `chat_device_reads_state_but_only_explicit_remote_admin_controls_the_actual_service_node`、`preferences_and_session_projection_are_read_only_and_close_without_revoke_is_real` | 真实执行 PASS：目录读取的权限/错误边界显式（非 owner 权限被拒、无效会话定位 4xx，不呈现成功） |
| `R00-T02-LA-178C8CBA29C2` | BROWSER-CLOSE-SESSION | A1 | `leaf:R00-T02-LA-178C8CBA29C2::a1-close-session-real-stop-and-state-broadcast` | `preferences_and_session_projection_are_read_only_and_close_without_revoke_is_real` | 真实执行 PASS：普通关闭走真实 Stop、响应 sessions 投影 running=false、重放不重复外部动作 |
| `R00-T02-LA-178C8CBA29C2` | BROWSER-CLOSE-SESSION | A2 | `leaf:R00-T02-LA-178C8CBA29C2::a2-close-invalid-unauthorized-downstream-explicit` | `chat_device_reads_state_but_only_explicit_remote_admin_controls_the_actual_service_node`、`unknown_external_revoke_receipt_never_repeats_the_stop_or_reopens_content` | 真实执行 PASS：权限不足 403、无效输入 4xx、下游效果未知 409 effectUnknown，均显式不呈现成功 |
| `R00-T02-LA-F1A5597E3E78` | BROWSER-CLOSE-SESSION-REVOKE | A1 | `leaf:R00-T02-LA-F1A5597E3E78::a1-close-and-revoke-authority-real` | `actual_http_command_replay_changed_input_and_emergency_receipt_never_repeat_external_actions`、`new_actual_user_submission_releases_emergency_stop_with_a_new_epoch` | 真实执行 PASS：close+revoke 真实关闭并撤销授权（authority.revoked/epoch 递增），后续命令 403 |
| `R00-T02-LA-F1A5597E3E78` | BROWSER-CLOSE-SESSION-REVOKE | A2 | `leaf:R00-T02-LA-F1A5597E3E78::a2-revoke-invalid-unknown-explicit` | `unknown_external_revoke_receipt_never_repeats_the_stop_or_reopens_content` | 真实执行 PASS：撤权下游返回未知时 409 effectUnknown 且只撤一次；重放不重复外部动作 |
| `R00-T02-LA-DCD85A04A7E9` | BROWSER-OPEN-SESSION | A1 | `leaf:R00-T02-LA-DCD85A04A7E9::a1-open-session-explicit-resume-cold-metadata` | `restart_restores_cold_metadata_and_confirmed_receipt_without_automatically_opening_a_page` | 真实执行 PASS：open-session 尝试恢复冷元数据并返回 resume 状态；恢复失败可空态；viewer 显示该会话属 T06，非本 case 通过前提 |
| `R00-T02-LA-DCD85A04A7E9` | BROWSER-OPEN-SESSION | A2 | `leaf:R00-T02-LA-DCD85A04A7E9::a2-open-invalid-unauthorized-downstream-explicit` | `archive_and_permanent_delete_reclaim_only_the_actual_session_browser_before_success`、`chat_device_reads_state_but_only_explicit_remote_admin_controls_the_actual_service_node` | 真实执行 PASS：无效/已删除会话打开失败非 2xx；chat-only 设备打开 403；不呈现成功 |
| `R00-T02-LA-A30E4BDF2FA2` | BROWSER-TOOL | A1 | `leaf:R00-T02-LA-A30E4BDF2FA2::a1-navigation-title-and-snapshot-match` | `browser_tool_a1_title_and_snapshot_match_real_navigation`、`same_real_target_input_snapshot_generations_and_empty_workspace` | 真实执行 PASS：导航后标题/快照正文与真实页面匹配（含表单元素进入快照） |
| `R00-T02-LA-A30E4BDF2FA2` | BROWSER-TOOL | A2 | `leaf:R00-T02-LA-A30E4BDF2FA2::a2-stale-ref-explicit-failure` | `browser_tool_a2_stale_snapshot_reference_fails_explicitly`、`page_cannot_move_or_forge_a_snapshot_reference_to_another_element` | 真实执行 PASS：页面更换后旧 ref click/type 显式 browser_stale_snapshot，伪造 ref 同样拒绝 |
| `R00-T02-LA-A30E4BDF2FA2` | BROWSER-TOOL | A3 | `leaf:R00-T02-LA-A30E4BDF2FA2::a3-session-a-state-not-leaked-to-b` | `browser_tool_a3_session_state_and_cookies_never_leak_between_sessions`、`password_values_never_enter_normal_or_compact_snapshots_and_stay_in_the_page`、`screenshot_owned_file_materializes_into_exact_read_pixels_without_granting_other_sessions` | 真实执行 PASS：独立 Chromium 进程/profile：A/B 标签与 URL 不同、快照互不含对方内容、Cookie 不继承；截图文件只授予本会话 |
| `R00-T02-LA-A30E4BDF2FA2` | BROWSER-TOOL | A4 | `leaf:R00-T02-LA-A30E4BDF2FA2::a4-no-session-blocked-or-browser-error-explicit-no-cross-session` | `browser_tool_a4_browser_errors_are_explicit_and_never_touch_another_session`、`unconfirmed_http_emergency_keeps_last_page_and_live_process_explicitly_unknown` | 真实执行 PASS：不可达页面 browser_navigation 显式失败（effect_unknown=false）；无会话 browser_not_running；另一会话状态逐字段不变，不误操作 |
| `R00-T02-LA-FD7A5A76B8EA` | WEB_FETCH | A1 | `leaf:R00-T02-LA-FD7A5A76B8EA::a1-fetch-body-and-final-url-session-file` | `public_fetch_keeps_five_hop_guard_and_registers_readable_full_session_file` | 真实执行 PASS：正文/最终 URL 对应；截断标记与完整会话文件可读回；未授权文件不越权 |
| `R00-T02-LA-FD7A5A76B8EA` | WEB_FETCH | A2 | `leaf:R00-T02-LA-FD7A5A76B8EA::a2-blocked-page-not-normal-content` | `web_fetch_blocked_page_invalid_urls_and_redirect_failures_are_explicit_without_writes` | 真实执行 PASS：403 captcha 阻断页返回 WEB_FETCH_HTTP_403 显式失败，不当正文/摘要成功返回 |
| `R00-T02-LA-FD7A5A76B8EA` | WEB_FETCH | A3 | `leaf:R00-T02-LA-FD7A5A76B8EA::a3-invalid-url-redirect-network-error-no-workspace-write` | `web_fetch_blocked_page_invalid_urls_and_redirect_failures_are_explicit_without_writes`、`public_fetch_proxy_pin_limitation_is_explicit_without_sending_any_request` | 真实执行 PASS：javascript:/file:/非法 URL 与重定向上限显式错误；失败不登记会话文件、不写工作区 |
| `R00-T02-LA-D1B56A275C3A` | WEB_SEARCH | A1 | `leaf:R00-T02-LA-D1B56A275C3A::a1-search-real-links-and-provider` | `all_five_http_providers_use_real_gateway_and_distinct_authentication`、`auto_records_each_real_http_attempt_then_all_three_browser_page_diagnostics` | 真实执行 PASS：结果含真实链接与 provider/source_type；五种 HTTP 供应商真实网关与独立鉴权 |
| `R00-T02-LA-D1B56A275C3A` | WEB_SEARCH | A2 | `leaf:R00-T02-LA-D1B56A275C3A::a2-no-config-not-fake-success` | `auto_all_failed_is_explicit_and_fixed_provider_never_silently_falls_back` | 真实执行 PASS：无可用配置时 diagnostics=all_failed 且内容显式失败，不伪造搜索命中 |
| `R00-T02-LA-D1B56A275C3A` | WEB_SEARCH | A3 | `leaf:R00-T02-LA-D1B56A275C3A::a3-missing-key-rate-limit-failure-explicit` | `provider_auth_redirect_and_rate_limit_keep_real_error_diagnostics`、`auto_all_failed_is_explicit_and_fixed_provider_never_silently_falls_back` | 真实执行 PASS：缺 key/限流/鉴权重定向显式错误码（WEB_SEARCH_RATE_LIMITED 等）与真实诊断，不假成功 |
| `R00-T02-LA-4173A08A51CC` | FILE-PREVIEW | — | **BLOCKED**：部分需要 Tauri 文件预览宿主（D8 asset protocol/自定义协议，T03/T06）；服务侧预览由 A10 套件覆盖 | 阻塞 |
| `R00-T02-LA-13049521EA0C` | CLOSE-BROWSER-VIEWER | — | **BLOCKED**：需要 Tauri browser-viewer 窗口生命周期（T01 host + T05 注册合并） | 阻塞 |
| `R00-T02-LA-6E13E5DF05EB` | OPEN-BROWSER-VIEWER | — | **BLOCKED**：需要 Tauri browser-viewer 打开/空态窗口（T01 host + T05 注册合并） | 阻塞 |
| `R00-T02-LA-8BD3BCC6D725` | SETTINGS-BROWSER-UI | — | **BLOCKED**：需要 Tauri 设置页 UI（T03/T06 DesktopHost-Tauri） | 阻塞 |


## 7. Tauri 宿主接口（`browser_viewer.rs`）

> 文件由本任务交付；host-core 已在其写作用域内完成注册（`lib.rs:1,10-12,552,604-608`、`build.rs:45-49`、`capabilities/browser-viewer.json`、`capabilities/main.json`）。本节记录最终契约，供 T06 四份集合核对。

- 文件：`desktop/src-tauri/src/browser_viewer.rs`（Rust 侧 snake_case；camelCase↔命令名映射归 DesktopHost-Tauri 层）。
- State：`BrowserViewerState { connection: Mutex<Option<HostConnection>>, viewer_target: Mutex<Option<BrowserViewerTarget>> }`（`Default`）；`HostConnection` 不实现 Serialize/Debug，服务 token 只存宿主内存。
- 事件：`browser:connection-changed`（payload 仅 `{active: bool, reason?}`，无 token）。
- 常量：`VIEWER_LABEL="browser-viewer"`、`MAIN_LABEL="main"`、`VIEWER_URL="browser-viewer.html?mode=native"`（真实 vite entry `desktop/src/browser-viewer.html`；`BrowserViewerApp` 只认 `mode==='native'`）、`VIEWER_ACTIONS`（12 项白名单）、`INVOKE_COMMANDS`（T06 四集合比对用）。
- 命令：
  - `set_browser_connection(window, app, state, connection: Option<BrowserConnectionInput>) -> Result<Option<String>, String>` — main only；返回 CSPRNG 32B hex connectionKey；身份变化清空 target + 销毁旧 viewer + 发事件。
  - `get_browser_viewer_context(app, window, state) -> Result<Option<BrowserViewerContext>, String>` — 仅 `browser-viewer` 返回 `Some`；其它窗口 `None`。
  - `open_browser_viewer(window, app, state, target) -> Result<BrowserViewerOpenReply, String>` — main only；校验 connectionKey + native sessionPath；destroy 旧 viewer → 建 label=`browser-viewer`（1100×760/min 620×420，`browser-viewer.html?mode=native`）。
  - `rust_browser_request(window, state, target, action, options) -> Result<Value, String>` — main 或当前 viewer；viewer 的 sessionPath/connectionKey 必须等于本窗口 target；action 白名单；宿主 reqwest 30s + `redirect::Policy::none()` + Bearer；await 后再次校验连接未变（防迟到）。
  - `close_browser_viewer(window, app, state) -> Result<(), String>` — viewer 关自己 / main 关 viewer；**绝不**发 stop/revoke。
- 安全裁决（Lead 已批准）：sessionPath **只接受 `native:<session-id>`**（服务 `session_locator` 契约），显式拒绝文件系统路径形态（renderer 无任意文件面；未来 legacy 绝对路径须先 canonicalize + `LINGXI_HOME` 会话域前缀 + 存在性 + 非 symlink 再另立 ADR）；connectionKey 用 `libc::getentropy` 32B，非 unix 平台显式 `browser_entropy_unavailable`（不降级伪随机；Windows/Linux CSPRNG 接线列 T06/BLOCKED_PLATFORM 决策）。
- 单测（模块内）：native session/URL/action/tabId 校验、connection identity 不含 token、VIEWER_URL 真实入口、viewer context 逐字段 shape（connectionKey/sessionPath/executionNode/serviceLabel/serviceUrl，无 token）；`rustfmt --check` 通过。
- **编译/测试验证（host-core 注册后）**：`cargo check --manifest-path desktop/src-tauri/Cargo.toml --features r09-host` exit 0（`03-host-bridge/tauri-host-cargo-check.log`）；`cargo test --manifest-path desktop/src-tauri/Cargo.toml --lib --features r09-host browser_viewer:: -- --test-threads=1` **7/7 PASS**（`03-host-bridge/browser_viewer-tests.log`）。
- capability 名建议（已由 host-core 落地）：main → `allow-set-browser-connection`/`allow-open-browser-viewer`/`allow-rust-browser-request`/`allow-close-browser-viewer`；viewer → `allow-get-browser-viewer-context`/`allow-rust-browser-request`/`allow-close-browser-viewer`（无文件/shell/任意 HTTP）。

## 8. 未完成 / 阻塞与解除条件

| 项 | 状态 | 解除条件 |
|---|---|---|
| A09 “测试安装不含 Electron”探针（安装树文件/进程/lsof） | **BLOCKED（T01/T02）** | host-core 产出可运行 Tauri 产品宿主 + 隔离安装树；用本生产者同口径探针（零 Electron 文件/进程/旧 helper 调用）复跑 |
| Tauri browser-viewer 窗口叶（130495/6E13E5） | **BLOCKED（T01/T02/T06）** | Tauri 全量 renderer（`browser-viewer.html` entry）+ host 注册 + wdio E2E；viewer 关闭不得误记急停/打开窗口不建空标签等断言真机复跑 |
| 设置页浏览器 UI 叶（8BD3BC） | **BLOCKED（T03/T06）** | DesktopHost-Tauri 设置页 + 能力矩阵 |
| 文件预览整叶（4173A0） | **部分 BLOCKED（T03/T06）** | Tauri 文件 URL/媒体/自定义协议 + 只读窗口 + 资源变更重读；服务侧读取/预览安全已由本套件覆盖 |
| A10 Tauri 真机 webview 嵌入 / Tauri IPC canary | **BLOCKED（T06/T07）** | Tauri 构建 + `e2e-test` wdio 注入 canary 命令；本轮只以真实 Chrome 验证等价 CSP/sandbox/Origin 语义 |
| macOS x64 / Windows x64 / Linux x64 | **BLOCKED_PLATFORM** | 提供真机/受控 VM（见 `docs/rust-tauri/R09/BLOCKED.md`） |
| Chromium 分发/签名/公证/R10 发布决策 | OPEN（R10） | 随包 vs 系统 Chrome 方案定稿；本轮冻结实测路径/版本/sha256（`environment.json`） |
| OQ-16 office 工具模型可发现性 | OPEN（产品面复核） | A09 证据为服务链/适配器真实执行 + `r07_office_tools` 真实网关；未证明真实模型自动发现 `office_html-to-pdf`（Deferred），需另行产品面验证 |
| preferCSSPageSize=false 角例 KNOWN-DIFF | 继承 R01 登记 | 默认 `preferCSSPageSize=true` 不受影响；如需对齐旧链可注入 `@page{size:auto}`（本任务未实施） |
| R07 保留失败（真实弹窗/cookie resume 等） | 原记录保留 | 不删除/不改写；本轮串行重跑与历史一致 |

## 9. 复现命令

```bash
REPO=/Users/study_superior/Desktop/Code/LingxiAgent; cd "$REPO"
# 最终 A09 逐断言套件（真实 Chrome + 真实服务；T01 renderer digest 窗口内用
# LINGXI_T05_REUSE_OLD_CHAIN_BASELINE 复用 hash 校验冻结旧链基线，不启动 Electron；
# 门禁正式运行不设置该 env 时仍会真实重生成旧链基线）
LINGXI_T05_REUSE_OLD_CHAIN_BASELINE=$PWD/artifacts/rust-tauri/R09/dev/t05/old-chain-baseline \
LINGXI_R09_RUNTIME=/private/tmp/lingxi-r09-t05/runtime-a09-final \
  python3 -B scripts/rust-tauri/r09_suites.py --suite r09_t05_browser_pdf \
  --evidence artifacts/rust-tauri/R09/dev/t05/01-a09-per-assertion-final2
# 前一版最终 A09 套件（真实 Chrome + Electron 仅作旧链对照 + 真实服务测试）
LINGXI_R09_RUNTIME=/private/tmp/lingxi-r09-t05/runtime-a09-final \
  python3 -B scripts/rust-tauri/r09_suites.py --suite r09_t05_browser_pdf \
  --evidence artifacts/rust-tauri/R09/dev/t05/01-a09-browser-pdf-final
# 最终 A10 套件（真实服务 + 真实 Chrome + loopback canary）
LINGXI_R09_RUNTIME=/private/tmp/lingxi-r09-t05/runtime-a10-final \
  python3 -B scripts/rust-tauri/r09_suites.py --suite r09_t05_untrusted_preview \
  --evidence artifacts/rust-tauri/R09/dev/t05/02-a10-preview-final
# 单独跑真实 Chrome 组（串行；PDF 上限 MAX_LEASES=2）
LINGXI_TEST_CHROMIUM="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
  ~/.cargo/bin/rustup run 1.98.1 cargo test --manifest-path rust/Cargo.toml \
  -p lingxi-adapters --test r07_browser_chromium -- --test-threads=1
LINGXI_TEST_CHROMIUM="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
  LINGXI_PDF_EVIDENCE_DIR=/private/tmp/lingxi-pdf-evidence \
  ~/.cargo/bin/rustup run 1.98.1 cargo test --manifest-path rust/Cargo.toml \
  -p lingxi-adapters --test r07_pdf_chromium -- --include-ignored --test-threads=1
```

## 10. 隔离 / 数据安全 / 权限声明

- 所有测试：`LINGXI_HOME`/`TMPDIR` 指向 `/private/tmp/lingxi-r09-t05/*`；生产者内建真实 `~/.lingxi` 前后摘要断言（`isolation::real-home-untouched` PASS）。
- 旧链 Electron 只在独立对照环境运行（隔离 HOME/TMPDIR/`R01_T05_OLD_TMP`、loopback 预留代理），**不进** Tauri 测试安装，不作为产品依赖。
- 真实 Chrome 均 headless/隔离 profile/mock keychain；进程组整组回收并逐次断言无残留。
- 真实服务使用临时端口 + 隔离 home + per-start local-token；token 不写入证据/不返回 viewer。
- 无真实模型/付费供应商调用、无真实用户数据迁移、无 commit/push/PR/tag/发布；证据均为仓库内未跟踪产物。
- 本轮逐断言扩展期间**未重建 `desktop/dist-renderer`、未启动 Electron 壳**（遵守 T01 renderer
  digest 稳定窗口）：旧链基线以 `LINGXI_T05_REUSE_OLD_CHAIN_BASELINE` 复用并逐文件 SHA-256
  对照冻结 manifest；门禁正式运行未设置该 env 时仍会真实重生成旧链基线。
