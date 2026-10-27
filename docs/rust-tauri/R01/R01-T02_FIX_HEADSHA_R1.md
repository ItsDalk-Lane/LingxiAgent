# R01-T02 FIX_HEADSHA_R1 — 生成器 headSha 缺陷修复报告

- 修复代理：ZCode:R01-T02-fix-headsha-r1（全新代理，未参与此前任何执行/验收；不 commit/push）
- 日期：2026-09-25｜分支 codex/rust-tauri-migration｜基线 HEAD `58789bc11def0c75fb41d92847e8afae4bf0a6fe`
- 缺陷来源：`docs/rust-tauri/R01/R01-T03_REVIEW_R1.md` F1（Medium-Low）+ F2（Info）
- 结论：**READY_FOR_REVIEW**（当前 HEAD 与"任意后续提交"两态 check-generated 均 exit 0，防漂移能力未下降）

## 1. 根因

**F1**：`scripts/rust-tauri/r01-t02-extract-api-surface.mjs`（原 367-369 行）把
`git rev-parse HEAD` 这一移动坐标嵌入**已提交的**生成物 `API_COMPAT_MATRIX.json`
（`generatedFrom.headSha`）。该值只在"生成后、提交前"的窗口内与盘一致；任何后续提交
（包括记录 T02 PASS 的账本提交本身）都使 `--check` 全文 diff 变红。"重新生成 → 无 diff"
契约在任何静止已提交 HEAD 上结构性不可满足。复现（修复前实测）：
`bash scripts/rust-tauri/r01-t02-check-generated.sh` exit 1，完整 diff 仅第 6 行
headSha（盘 a98d2487… vs 当前 58789bc1…），624 条目与 sourceDigests 逐项一致。

**F2**：`r01-t02-check-generated.sh` 调裸 `cargo`，结果依赖 PATH 顺序。本机 PATH 中
Homebrew cargo 1.93.0（/opt/homebrew/bin）优先于 rustup 代理，[1/2] 会跑在非锁定
工具链上；rust-toolchain.toml 的锁定仅对经 rustup 的调用生效。

## 2. 修复（采用验收建议方向 ②：内容派生戳；F2 显式锁定工具链）

1. **extract-api-surface.mjs**：
   - 删除 `git rev-parse HEAD` 调用及 `execSync` 导入；生成器不再接触 git。
   - 新增内容派生戳 `contentSha = sha256(JSON.stringify({fullInventory, surfaces, summary, sourceDigests}))`
     —— 为被扫描源码内容的纯函数。`generatedFrom.headSha` 字段替换为
     `generatedFrom.contentSha`，`note` 同步写明语义。
   - `--check` 仍为**全文逐字节比较，无字段豁免**（未采用方向 ①，防漂移能力不下降）。
2. **r01-t02-check-generated.sh**：从 rust-toolchain.toml 解析 channel（sed），经
   `rustup run <channel> cargo …` 显式调用锁定工具链；rustup 不在 PATH 时回退标准安装位
   `$HOME/.cargo/bin/rustup`，仍不落到裸 cargo；解析失败/无 rustup 显式报错 exit 1。
   启动行打印实际生效的 rustc 版本（实测 `rustc 1.98.1 (48a229cea 2026-09-01)`）。
3. **API_COMPAT_MATRIX.json** 再生：与现提交版 diff 仅 6-7 两行（headSha→contentSha
   及 note），624 条目、sourceDigests、policy、inventory 逐字节一致。
4. **PROTOCOL_SPEC.md §10**：新增"矩阵戳记与门禁语义"段，写明 contentSha 定义、
   不嵌入移动坐标、无字段豁免、门禁经 rustup 锁定工具链。

## 3. 重跑证据（逐命令退出码；日志在 artifacts/rust-tauri/R01/T02/，fix-headsha 前缀）

| 命令 | 退出码 | 日志 |
|---|---|---|
| `bash scripts/rust-tauri/r01-t02-check-generated.sh`（当前 HEAD 58789bc1） | 0 | fix-headsha-check-generated.log |
| `bash scripts/rust-tauri/r01-t02-roundtrip.sh`（A03） | 0 | fix-headsha-roundtrip.log |
| `bash scripts/rust-tauri/r01-t02-handshake.sh /tmp/fix-headsha-handshake`（A04） | 0 | fix-headsha-handshake.log |
| `rustup run 1.98.1 cargo test --workspace --offline`（/tmp CARGO_TARGET_DIR） | 0（kernel 7 + protocol 19 + spike 7，0 failed） | fix-headsha-cargo-test.log |
| `python3 -B docs/rust-tauri/R01/r01_t01_check_ownership.py` | 0 | fix-headsha-t01-positive.log |
| 同上 `--self-test`（N1–N15 全拒） | 0 | fix-headsha-t01-selftest.log |
| `python3 -B docs/rust-tauri/R01/r01_t01_build_ownership.py --check` | 0 | fix-headsha-t01-generator-check.log |

退出码台账：fix-headsha-EXIT_CODES.txt；环境：fix-headsha-ENVIRONMENT.txt。

## 4. "任意后续提交后仍 exit 0" 实证（/tmp 克隆副本）

`/tmp/r01t02-fix-headsha-future` = 本分支 `git clone --local` + 应用本修复工作区文件：

| 步骤 | clone HEAD | check-generated 退出码 | 日志 |
|---|---|---|---|
| 提交修复（fix commit） | 11a693e8b8c4e3ead34790b3989c5d0a8b442803 | 0 | fix-headsha-future-commit-a.log |
| +1 个空提交 | abe8014870cb840af796f9bcc42a7ae656d3455b | 0 | fix-headsha-future-commit-b.log |
| +2 个空提交 | （abe80148 之上） | 0 | fix-headsha-future-commit-c.log |

矩阵不再含任何 git 坐标，HEAD 前进不再引起漂移。

## 5. 防漂移能力未下降（负向实证，同 clone）

| 测试 | 操作 | 退出码 |
|---|---|---|
| neg1 | 矩阵条目改 1 字节（disposition retain→retaiX）→ `extract --check` | 1（DRIFT，逐行指出） |
| neg2 | 被扫描源 server/routes/auth.ts 追加一行 → `extract --check` | 1（sourceDigests 不一致） |
| 对照 | 复原后 `--check` | 0 |

证据合并于 fix-headsha-negative-proofs.log。

## 6. 范围与一致性声明

- 改动仅限：extract-api-surface.mjs、check-generated.sh、再生的 API_COMPAT_MATRIX.json、
  PROTOCOL_SPEC.md §10 语义段、本报告、fix-headsha 证据。任务书目录、.sync-audit、
  ORCHESTRATOR_PROGRESS.json、生产代码、既往报告原文零改动。
- 矩阵 624 条目无语义变化：再生 diff 仅 `generatedFrom` 的戳记字段两行。
- 未提交、未推送；工作区改动留待总控授权处置。
- 环境限制：仅 macOS arm64 本机验证；握手原型 loopback。

## 7. SHA-256

见 artifacts/rust-tauri/R01/T02/fix-headsha-SHASUMS.txt（本报告及全部 fix-headsha 证据 +
两个改动脚本 + 再生矩阵）。
