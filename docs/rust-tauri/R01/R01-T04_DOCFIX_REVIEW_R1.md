# R01-T04 DOCFIX REVIEW R1 — 文档修正独立复验

复验代理：ZCode:R01-T04-docfix-review-r1（全新独立复验；未参与此前执行/验收/修正；只读审查 + 证据复算 + 环境抽查；未修改任何已提交文件与交付物；本报告是唯一新增仓库文件）
日期：2026-09-25｜分支 `codex/rust-tauri-migration`｜基线 HEAD `08b9f075032455be5f57f8fc882bf366b25b4269`（实测 `git rev-parse HEAD` 一致）
被审对象：`docs/rust-tauri/R01/R01-T04_DOCFIX_R1.md` 声称的两项修正（F1 D-09 版本号对齐；F2 「无真实外发」措辞精确化 + GCM 披露）及其新证据 `artifacts/rust-tauri/R01/T04/docfix-r1-gcm/`

**最终判定：PASS**（两项修正均真实落地且事实准确；未发现新的失实）

---

## 1. F1 核对：D-09 版本号与 Cargo.lock

[实算] 独立从 `rust/Cargo.lock` 提取（`awk` 按 `name = "<pkg>"` 取下一行 version），与 D-09 段现行文本逐项比对：

| crate | D-09 文本 | Cargo.lock 实测 | 一致 |
|---|---|---|---|
| serde | 1.0.229 | 1.0.229 | ✓ |
| serde_json | 1.0.151 | 1.0.151 | ✓ |
| libc | 0.2.189 | 0.2.189 | ✓ |
| base64 | 0.23.1 | 0.23.1 | ✓ |
| sha2 | 0.11.0 | 0.11.0 | ✓ |

陈旧的「1.0.228 / 1.0.149」已消除，同段其余三个版本号抽查亦全部一致。旁证：`rust/crates/lingxi-browser-spike/Cargo.toml` 声明的最低版本（0.23.1 / 0.2.189 / 1.0.229 / 1.0.151 / 0.11.0）与 lock 逐项一致。`git diff HEAD -- rust/Cargo.lock` 复算：唯一新增为 lingxi-browser-spike 自身条目（deps: base64/libc/serde/serde_json/sha2），「无新增第三方版本」主张成立。**F1 修正属实、完整。**

## 2. F2 核对：措辞修正与 GCM 披露

[源码] 通读修改后段落并全文检索残留绝对化表述（`无真实外发|无外发|不会外发|没有任何外发|零外发|无外部连接`，覆盖 docs/rust-tauri/R01/*.md 全部 19 个 markdown）：

- `BROWSER_SPIKE_REPORT.md` 方法段（line 6）：blanket「无真实外发」已删除，改为「**测试流量全部指向 loopback**」（限定词为测试流量），并同句明确披露非 proxy 阶段（main/isolation/takeover）Chrome 自带 GCM 后台外联真实发生、无任务数据/凭证，交叉引用 R01-T04_REPORT.md §4 末段与 docfix 证据目录。精确化到位。
- `R01-T04_REPORT.md` §4 末新增「补充披露（docfix R1）」（line 98-114）：事实完整——发生阶段（非 proxy 三阶段直连）、证据（stderr DEPRECATED_ENDPOINT/QUOTA_EXCEEDED + lsof 外部 :443/:5228 ESTABLISHED + 证据目录指针）、性质定性（Chrome 自身遥测/保活，载荷为设备级注册信息）、与 §4 已披露代理事故的区分（同类但不经 proxy.mjs 故不见于其日志）、TUN 出口路径如实记录、缓解评估分「实测有效」（loopback-only proxy.mjs）与「**实测无效**」（既有旗标组 + `--disable-features=PushMessaging`）两列，并明确「不声称任何命令行旗标组合能消除该外联」。对既有结论影响标注为零，与验收 R1 判断一致。
- `R01-T04_REPORT.md` §3 红线自查（line 85）：「测试目标全部 127.0.0.1」句已补 GCM 交叉引用，不再可被读作 blanket 无外发声明。
- `ADR-002-browser-host.md`：全文检索确认无「无真实外发」类事实声明（仅 W13 行引用「红线禁外发」规则），docfix 声称的「核查后未改动」属实（mtime 12:25 早于 docfix 工作窗口 12:49-12:58）。
- 残留检索命中全部为修正语境本身（修正报告/验收报告引用旧措辞、或方法段的否定式说明），无新的绝对化外发表述残留。

**F2 修正属实、完整。**

## 3. docfix-r1-gcm 证据抽查

[实算] `shasum -a 256 -c artifacts/rust-tauri/R01/T04/docfix-r1-gcm/SHA256SUMS.txt`：**12/12 全 OK / 0 FAILED**。

内容一致性逐项核对（复验代理独立读取，非转述）：

- `baseline.stderr.log`：`registration_request.cc:291` DEPRECATED_ENDPOINT（12:50:04，meta 起点 12:50:01 → t≈4s）+ QUOTA_EXCEEDED（12:50:25 → t≈25s），pid 12100 与 `baseline.meta.log` 一致——与修正报告声称逐项吻合。
- `mitigation.stderr.log`：DEPRECATED_ENDPOINT×2 + QUOTA_EXCEEDED×1，pid 12281 与 meta 一致——「`--disable-features=PushMessaging` 实测无效」的声称有证据支撑。
- `endpoints.lsof.log`：6 个采样点（t=10s..60s）均含 `Google Chrome Helper`（pid 12544，endpoints 组 pid 12535 的子进程）到 198.18.x.x `:443`/`:5228`（另有一条 `:80`，报告未列举但不矛盾）的 ESTABLISHED TCP。198.18.0.0/15 为 fake-IP 段——本代理实测本机 `utun1024` 为 UP 且 inet 198.18.0.1、路由表默认经 utun1024，TUN 环境记录属实。
- baseline/mitigation 两组 lsof 全为 `(no chrome sockets)`，与修正报告「未加 `+c 0` 进程名截断未匹配」的注明自洽（endpoints 组日志进程名为未截断的 `Google\x20Chrome\x20Helper` 形式）。
- meta 时长：baseline 82s、mitigation 83s、endpoints 63s，与声称的 80s/80s/60s 采样吻合；时序首尾相接无重叠（12:50:01→12:51:23→12:52:46，12:53:41→12:54:44）。
- 验证脚本 `/tmp/r01t04-docfix/run-gcm-check.sh` 仍在（不入库，符合声明）：通读确认——六个代理变量全部 `env -u` 剥离；baseline 旗标组与 `rust/crates/lingxi-browser-spike/src/launcher.rs`（lines 40-47，[源码] 核对）逐项一致；mitigation 组确为追加 `--disable-features=PushMessaging`；脚本跑后 `pkill` 清理。本代理复核 `pgrep -f 'user-data-dir=/tmp/r01t04'` 为空，无残留进程。

既有证据完整性 [实算]：`shasum -a 256 -c artifacts/rust-tauri/R01/T04/SHA256SUMS.txt` → **51/51 全 OK / 0 FAILED**；清单 mtime 12:28、各证据子目录 mtime 11:51-12:08，均早于 docfix 窗口（12:49 起），既有 51 项证据未被改动。

## 4. 边界确认（零代码改动 / 未触碰禁区）

[实算] `git status --porcelain` + `git diff HEAD --stat`：修改集仍为验收 R1 确认的 4 个文件（DEPENDENCY_DECISIONS.md / DEPENDENCY_RULES.json / rust/Cargo.toml / rust/Cargo.lock，均为执行者既有改动，内容与验收 R1 描述一致——lock 仅新增 spike 条目、workspace members 仅追加 lingxi-browser-spike）；新增路径为 spike crate、tests/migration/r01-t04/、replay 脚本、artifacts 与文档。docfix 自身的增量仅为：D-09 段两个版本号、BROWSER_SPIKE_REPORT.md 方法段、R01-T04_REPORT.md §3/§4、docfix-r1-gcm/ 新目录、DOCFIX 报告本身——**rust/ 代码、desktop/、tests/ 逻辑、scripts/ 零改动**（spike crate 与 tests 目录 mtime 均早于 docfix 窗口，抽查佐证）。

- `docs/rust-tauri/ORCHESTRATOR_PROGRESS.json`：tracked 且 `git diff HEAD` 为空——未触碰。
- `.sync-audit/`：`git status`/`git diff` 无任何命中——未触碰。
- `R01-T04_REVIEW_R1.md`：mtime 12:45 早于 docfix 窗口，既往验收报告原文未动。

## 5. 新发现问题

无。两项观察（均不构成问题，仅如实记录）：

- 修正报告声称「脚本整体 exit=0」未由本代理重跑复现（按任务边界不重跑浏览器 spike）；但 meta 起止时间、pid 与 stderr 内部时间戳三方自洽，无伪造迹象。
- endpoints 组 lsof 另观测到一条 `:80` ESTABLISHED，修正报告只列举 :443/:5228；属列举不全而非失实，不影响结论。

## 6. 最终判定

**PASS**。F1/F2 两项文档修正均真实落地、事实准确、证据可复算、边界干净（零代码改动、禁区未触碰、既有证据零改动），未引入新的失实。R01-T04 的验收 R1 PASS 结论及其附条件至此全部收口。
