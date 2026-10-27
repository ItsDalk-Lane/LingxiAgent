# R01-T04 DOCFIX R1 — 独立验收 R1 两项低严重度文档修正

- 修正代理：ZCode:R01-T04-docfix-r1（未参与此前执行/验收；仅改文档 + 新增 docfix 证据，不改代码逻辑，未 commit/push）
- 日期：2026-09-25｜分支 `codex/rust-tauri-migration`｜基线 HEAD `08b9f075032455be5f57f8fc882bf366b25b4269`
- 依据：`docs/rust-tauri/R01/R01-T04_REVIEW_R1.md` §8 发现 F1/F2（最终判定 PASS，附两项文档修正要求）

## 修正点 1（F1）：D-09 版本号与 Cargo.lock 对齐

- **文件**：`docs/rust-tauri/R01/DEPENDENCY_DECISIONS.md` D-09 段（仅此段）。
- **改动**：「serde 1.0.228、serde_json 1.0.149」→「serde **1.0.229**、serde_json **1.0.151**」。
- **依据**（本代理独立核对，非转述）：
  ```
  $ grep -A1 '^name = "serde"$' rust/Cargo.lock        → version = "1.0.229"
  $ grep -A1 '^name = "serde_json"$' rust/Cargo.lock   → version = "1.0.151"
  ```
- **同段其他数字复核**：libc 0.2.189、base64 0.23.1、sha2 0.11.0 均与 Cargo.lock 一致，无需改动。D-09 实质主张（复用已锁定版本、无新增第三方版本）不受影响。

## 修正点 2（F2）：「无真实外发」措辞精确化 + GCM 后台外联补披露

### 文档改动

1. `docs/rust-tauri/R01/BROWSER_SPIKE_REPORT.md` 方法段：删除 blanket 表述「无真实外发」，改为「测试流量全部指向 loopback」，并明确指出非 proxy 阶段 Chrome 自带 GCM 后台外联真实发生（无任务数据/凭证），附披露与证据交叉引用。
2. `docs/rust-tauri/R01/R01-T04_REPORT.md` §4 末：新增「补充披露（docfix R1）」段，含事实、性质与已披露事故的区分、缓解措施评估（实测有效/无效分列）、对既有结论影响（零）。
3. `docs/rust-tauri/R01/R01-T04_REPORT.md` §3 红线自查：「测试全部 127.0.0.1」句补交叉引用，避免被读作 blanket 无外发声明。
4. `docs/rust-tauri/R01/ADR-002-browser-host.md` 核查后**未改动**：全文无「无真实外发」类表述（仅 W13 行引用「红线禁外发」规则，非事实声明）。

### 实测验证（本代理执行，证据落盘）

目的：独立复现 F2 所述 GCM 外联，并实测一个候选缓解旗标。脚本 `/tmp/r01t04-docfix/run-gcm-check.sh`（不入库），全部命令剥离六个代理变量；数据目录 `/tmp/r01t04-docfix/`；跑完 `pkill` 清理，`pgrep -f "user-data-dir=/tmp/r01t04"` 复核为空。

- **baseline 组**：`/Applications/Google Chrome.app/Contents/MacOS/Google Chrome` + 与 `rust/crates/lingxi-browser-spike/src/launcher.rs` 默认旗标组完全一致的参数（`--no-first-run --no-default-browser-check --disable-session-crashed-bubble --hide-crash-restore-bubble --disable-background-networking --disable-component-update --disable-sync --metrics-recording-only`，全新 profile，`about:blank`），运行 80s。
  - 结果：stderr 出现 `google_apis/gcm/engine/registration_request.cc:291 Registration response error message: DEPRECATED_ENDPOINT`（t≈4s）与 `QUOTA_EXCEEDED`（t≈25s）。**GCM 外联复现**——收到服务端响应码即证明完成真实往返；已有 `--disable-background-networking` 旗标组对 GCM 不完全有效（与验收 R1 观察一致）。
- **mitigation 组**：同上 + `--disable-features=PushMessaging`，80s。
  - 结果：stderr 同样出现 DEPRECATED_ENDPOINT×2 与 QUOTA_EXCEEDED。**该候选旗标实测无效**，故文档不声称任何命令行旗标组合可消除该外联。
- **endpoints 组**（补充取证，60s）：`lsof +c 0 -nP -iTCP` 每 10s 采样，观测到 Chrome Helper 进程持有到外部地址 `:443` 与 `:5228`（GCM/mtalk 常用端口）的 ESTABLISHED TCP 连接（前两组 lsof 因未加 `+c 0` 致进程名截断未匹配，已注明，不影响 stderr 证据）。
- **环境如实记录**：本机存在活跃 TUN 代理（utun1024，fake-IP 198.18.0.0/15；shell 代理环境变量虽指向已死的 127.0.0.1:7890，TUN 层仍生效），外联经该路径转发并收到 Google 侧响应。出口路径是机器环境特性；「浏览器自发外联、越出 loopback 边界、收到 Google 响应」的事实与路径无关。
- **退出码**：脚本整体 exit=0（两 case 正常启动/采样/清理）。

### 证据

`artifacts/rust-tauri/R01/T04/docfix-r1-gcm/`（docfix 标记目录，未触碰既有 51 项 `SHA256SUMS.txt` 清单，目录内自带独立 `SHA256SUMS.txt`）：

| 文件 | 内容 |
|------|------|
| `baseline.stderr.log` / `baseline.stdout.log` | baseline 组 Chrome 输出（GCM DEPRECATED_ENDPOINT/QUOTA_EXCEEDED） |
| `mitigation.stderr.log` / `mitigation.stdout.log` | mitigation 组 Chrome 输出（同现 GCM 错误 → 旗标无效） |
| `endpoints.stderr.log` / `endpoints.stdout.log` | 补充取证组 Chrome 输出 |
| `{baseline,mitigation,endpoints}.lsof.log` | TCP 采样（endpoints 组含外部 :443/:5228 ESTABLISHED） |
| `{baseline,mitigation,endpoints}.meta.log` | 起止时间与 pid |
| `SHA256SUMS.txt` | 上述文件哈希清单 |

已验证有效的缓解（沿用现有证据，无需重跑）：对全部阶段套用修补后的 loopback-only `proxy.mjs`（`chromium-proxy/proxy-server.log` 中 Chrome 对 Google 的后台请求全部 REFUSED）。未验证项（host 级防火墙/DNS 阻断、其他 disable-features 组合）已在文档中如实标注。

## 边界自查

- 未改任何代码（rust/、desktop/、tests/、scripts/ 等零改动）；未 commit/push。
- 未触碰任务书目录、`.sync-audit/`、`ORCHESTRATOR_PROGRESS.json`、既往验收报告原文（`R01-T04_REVIEW_R1.md` 未动）。
- 未修改既有证据清单 `artifacts/rust-tauri/R01/T04/SHA256SUMS.txt`；新增证据全部在独立 docfix 子目录。
