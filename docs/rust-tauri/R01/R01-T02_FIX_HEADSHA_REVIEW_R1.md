# R01-T02 FIX_HEADSHA_REVIEW_R1 — 修复独立复验报告

- 复验代理：ZCode:R01-T02-fix-headsha-review-r1（全新独立代理，未参与此前任何执行/验收/修复）
- 日期：2026-09-25｜分支 codex/rust-tauri-migration｜基线 HEAD `58789bc11def0c75fb41d92847e8afae4bf0a6fe`
- 复验对象：`docs/rust-tauri/R01/R01-T02_FIX_HEADSHA_R1.md`（修复 T03 验收发现 F1/F2）
- 工作方式：只读审查 + 独立复跑；临时验证全部在 `/tmp/r01t02-fixheadsha-review/` 与
  `/tmp/r01t02-review-clone/`（git clone --local 副本）；未修改仓库任何已提交文件与交付物
- 最终判定：**PASS**（F1、F2 均关闭；防漂移能力未下降；回归全绿；生产零改动）

## 0. 环境（实测）

macOS 27.0（Build 26A428）arm64；Node v24.16.0；Python 3.14.3；
rustup 1.29.1（`$HOME/.cargo/bin/rustup`，**不在 PATH**）；锁定工具链
`rustc 1.98.1 (48a229cea 2026-09-01)`；裸 PATH 上 `cargo` 解析为 Homebrew
**cargo 1.93.0**（/opt/homebrew/bin）——即 F2 描述的 PATH 陷阱在本复验环境真实存在。

## 1. 复跑清单与退出码（全部为本代理独立执行，日志在 /tmp/r01t02-fixheadsha-review/）

| # | 命令 | 退出码 | 日志 |
|---|---|---|---|
| 1 | `bash scripts/rust-tauri/r01-t02-check-generated.sh`（当前工作区） | 0 | rev-check-generated.log |
| 2 | /tmp 克隆 + 应用修复并提交（2bec9795b）→ check-generated | 0 | rev-clone-fixcommit.log |
| 3 | 克隆 +1 空提交（3716c55ab）→ check-generated | 0 | rev-clone-empty1.log |
| 4 | 克隆 +2 空提交（aeb448ebf）→ check-generated | 0 | rev-clone-empty2.log |
| 5 | 负向：矩阵 `retain`→`retaiX` → check-generated | **1**（DRIFT，逐行指出差异） | rev-neg-matrix.log |
| 6 | 负向：`server/routes/auth.ts` 追加一行 → check-generated | **1**（contentSha 与 sourceDigests 双双不一致） | rev-neg-source.log |
| 7 | 负向复原后 → check-generated | 0 | rev-neg-restored.log |
| 8 | `bash scripts/rust-tauri/r01-t02-roundtrip.sh`（A03） | 0（12 golden 样本字节稳定） | rev-roundtrip.log |
| 9 | `bash scripts/rust-tauri/r01-t02-handshake.sh /tmp/rev-handshake`（A04） | 0 | rev-handshake.log |
| 10 | `rustup run 1.98.1 cargo test --workspace --offline`（CARGO_TARGET_DIR=/tmp/lingxi-r01t02-review-target） | 0（33 passed / 0 failed：19+7+7，与修复报告 kernel 7 + protocol 19 + spike 7 一致） | rev-cargo-test.log |
| 11 | `python3 -B docs/rust-tauri/R01/r01_t01_check_ownership.py`（正向） | 0 | rev-t01-positive.log |
| 12 | 同上 `--self-test`（N1–N15 全拒） | 0 | rev-t01-selftest.log |
| 13 | `python3 -B docs/rust-tauri/R01/r01_t01_build_ownership.py --check` | 0（OWNERSHIP_TARGET_UP_TO_DATE features=736 stores=69） | rev-t01-gen-check.log |

## 2. F1 关闭判定：**关闭**

- 生成器 diff 审查：`git rev-parse HEAD` 调用与 `execSync` 导入已删除；全文 grep
  无 `Date`/`random`/`process.env`/`git` 等移动坐标残留（仅注释与 note 文本提及 git）。
- contentSha 纯函数性实证：
  - 生成器植入探针（/tmp 副本，不改仓库）dump 哈希输入，两次运行均得
    `2f55e3dd4130bdba5aa66cfae9ccb5a6c833112a8d506ed0a4c3fdeb00fe0e3a`，与矩阵存储值
    逐字符一致——确定性纯函数。
  - 哈希输入 `{fullInventory, surfaces, summary, sourceDigests}` 全部派生自被扫描源文件
    （mounts/factoryModules/preload.channels/路由与桥接提取结果/逐文件 sha256）。
- 「任意后续提交免疫」实证：/tmp 克隆上修复提交及 +1/+2 空提交三态 check-generated
  均 exit 0（矩阵不再含任何 git 坐标，HEAD 前进不引起漂移）。
- `--check` 仍为全文逐字节比较：源码确认为 `disk === out` 字符串全等，无字段豁免；
  负向测试 5/6 均 exit 1，复原后 exit 0——防漂移能力未下降。
- 624 条目不漂移：工作区矩阵对 HEAD 的 diff 仅 `generatedFrom` 戳记 2 行
  （headSha→contentSha 及 note）；`--check` 输出 `624 entries` 与 summary.total=624 一致。

## 3. F2 关闭判定：**关闭**

- check-generated.sh 从 `rust-toolchain.toml` 解析 channel（实测得 `1.98.1`），经
  `rustup run "$TOOLCHAIN" cargo …` 显式调用；解析失败/无 rustup 均显式报错 exit 1，
  不落到裸 cargo（无静默降级，符合项目红线）。
- 本复验环境中 rustup **不在** bash PATH，脚本走了 `$HOME/.cargo/bin/rustup` 回退分支
  并成功——回退路径得到真实执行验证；启动行实测打印
  `== toolchain: rustup run 1.98.1 (rustc 1.98.1 (48a229cea 2026-09-01))`，而裸 PATH
  cargo 为 Homebrew 1.93.0，证明门禁确实运行在锁定工具链上。

## 4. 回归与范围核对

- T02 roundtrip / handshake：exit 0（见表 #8/#9）。
- `cargo test --workspace --offline`（rustup run 1.98.1，/tmp target）：exit 0，
  33 passed / 0 failed。
- T01 校验器正向、`--self-test`（N1–N15 全拒）、生成器 `--check`：均 exit 0。
- 生产零改动：`git diff HEAD -- desktop server core lib shared cli hub plugins
  skills2set package.json package-lock.json` 为空。
- 工作区差异严格限于修复声明范围：`API_COMPAT_MATRIX.json`、`PROTOCOL_SPEC.md`、
  `r01-t02-check-generated.sh`、`r01-t02-extract-api-surface.mjs` 四个修改文件 +
  修复报告与 fix-headsha 证据（未跟踪）。`ORCHESTRATOR_PROGRESS.json` 无改动。

## 5. 新发现问题

- **F-info-1（Info，不阻塞）**：contentSha 的哈希输入 `fullInventory` 含完整 mounts 与
  factoryModules，而矩阵落盘的 `inventory` 字段是缩减视图
  （`{mountTableSize, preloadIpcChannels}`），故 contentSha 无法仅由矩阵 JSON 重算，
  必须重跑提取器验证。note 中 `fullInventory` 命名与矩阵 `inventory` 字段字面相近，
  初读易误解为可由矩阵自校验（本代理首次重算即因此不符，经探针 dump 定位）。
  不影响门禁正确性（`--check` 本就是全文逐字节重生成比较），建议后续阶段在
  PROTOCOL_SPEC §10 补一句「fullInventory 为提取器内部全量清单，非矩阵 inventory 字段」。

## 6. 结论

修复报告的全部关键声称（contentSha 纯函数、--check 无豁免、rustup 显式锁定、
未来提交免疫、负向可检出、回归绿、范围受限）均经本代理独立复跑证实。
**F1 关闭、F2 关闭，最终判定 PASS。**
