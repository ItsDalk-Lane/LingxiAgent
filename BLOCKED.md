# BLOCKED.md — pi SDK 0.80.3→0.83.0 升级

> 本任务书约定：拿不准的写这里；收不回的操作停下写这里；无关 bug/想做的不动。
> 「断了、换新会话」：先读 PROGRESS.md 接着做，不重做。

## 当前状态：无（全部验收项达标）

升级牵出的 6 个新失败（cli-closure×3、open-boundary×2、compaction-guard-ext GLM×1）已全部转绿：
- cli-closure / open-boundary：用户二次裁决放宽白名单，把 `scripts/compute-cli-closure.mjs`（DYNAMIC_CALL_ALLOWLIST 追加 2 条）+ `build/cli-runtime-closure.json` + `build/open-boundary-baseline.json`（重生成）+ `export-manifest.json`（补 3 个新路径）按「升级必经产物重生成」（与 persistence 指纹同处理）落地。详见 PROGRESS.md「白名单放宽记录」。
- compaction-guard-ext GLM：tests/ 白名单内，glmHistory 改 canonical 数组 content（pi-coding-agent 0.83.0 convertToLlm 行为变化），保持测试意图。

最终 `npm test`：**Tests 1 failed | 10608 passed | 7 skipped (10616)**——failed=1 = 仅 DeskSection（基线允许），达 failed≤1。

## 遗留风险（非阻塞，写进交付说明）
- **真实 OAuth 登录不做人工验证**（无法自动化）：loginOAuthProvider 的旧 OAuthLoginCallbacks→0.83.0 AuthInteraction 适配（onAuth↔notify auth_url、onDeviceCode↔notify device_code、onPrompt/onSelect/onManualCodeInput↔prompt、signal）已用 pi-sdk-oauth-login-adapter 测试验证 selector 契约（browser 选项在 I/O 前触达），但端到端真实 OAuth 登录未人工跑通。

---
（执行中未遇到需上层裁决的收不回操作或拿不准项；原 6 个白名单受限失败已由用户裁决放宽后全部转绿。）

---

# ========== upstream-0.444.1 + pi SDK 0.84.1 迁移任务（2026-08-08）==========

> TASK_ID=upstream-0.444.1_pi-0.84.1。本 section 仅供本任务；上方旧 section 是历史记录。

## 当前状态：无 blocker（STATUS: COMPLETE）

本任务未遇到 Phase 23 定义的真正 blocker。所有验收项达标（详见 PROGRESS.md P20 验收矩阵）。最终 `npm test`（v24.16.0，sqlite rebuilt）：**10978 passed | 0 failed | 7 skipped**，exit 0。

执行中遇到的"看似 blocker、实为正常迁移工作"的项（均已解决，不属 blocker）：
- Lingxi 与 upstream 无共同 git 祖先 → 不能用 git merge，改逐文件 `git merge-file` 3-way（Phase 5）。
- pi 0.84.1 `AuthStorage` 删了 has/remove → model-manager auth cleanup 适配（required migration，非 blocker）。
- pi 0.84.1 OAuth provider `refreshToken` 加 signal 参数 → xai-oauth 适配（required migration）。
- 衍生产物 stale → 用仓库自带 generator 重生成（Phase 12，非 blocker）。
- `~/.local/bin/node`(v22) PATH 抢占 nvm → 显式 `export PATH=...v24.16.0/bin` 解决（环境，非 blocker）。

## 遗留风险（非阻塞）
- **真实 OAuth 登录 / 真实 Provider 网络调用未做人工端到端验证**（无法自动化）：refreshToken 的 signal 接入、AuthStorage has/remove→read/delete 适配、stream-guard 契约保持均用单元测试验证，但真实 OAuth refresh / 真实模型流式未人工跑通。
- **better-sqlite3 native binary 已为 v24.16.0 (NODE_MODULE_VERSION 137) 重建**；若 CI/其他开发者默认 node 不同，需各自 `npm rebuild better-sqlite3`。这是环境前提，非本任务产物。

---

# ========== 对抗性审计收口（adversarial closeout，2026-08-08）==========

> TASK: adversarial closeout of upstream-0.444.1 + pi 0.84.1 迁移。本 section 记录
> 收口阶段（ISSUE 1-4）的状态。详见 PROGRESS.md P22。

## STATUS: COMPLETE（本收口阶段）
## BLOCKERS: none

收口阶段解决了审计指出的 4 个问题，均不构成 merge blocker：

1. **ollama open-boundary 裁决（ISSUE 1）**：纠正 P13 的逻辑矛盾分类。
   证据（dispatcher import + sibling 同型 + manifest 历史漏列 + 无 closed 语义）证明
   ollama.ts 是 open provider-compat 架构的合法成员，真实分类为
   "open-set omission"。manifest addition 保留；仅修正文档分类。boundary lint 17/17 ✓。

2. **START_HEAD Node24 baseline 重建（ISSUE 2）**：原 P1 baseline 经 `nvm exec` 被
   `~/.local/bin/node`(v22) PATH 抢占，证据链失效。用 disposable worktree + 真
   process.execPath-verified v24.16.0 重建：13 test-level failures / 6 files。
   screenshot.test.ts 在真 Node24 **不失败**（12/12），原 P1.1 清单为 v22 污染 →
   INVALIDATED AND REPLACED。

3. **当前 HEAD Node24 重新验证（ISSUE 3）**：typecheck 0 error；targeted SDK/boundary/
   guardrail/screenshot 全绿；full suite **10979 passed | 0 failed | 7 skipped**，
   exit 0（+1 pass 为新增 pi-agent-core 回归测试；7 skipped = baseline win32 platform）。

4. **GitHub CI 证据（ISSUE 4）**：见下方 CI section。

## CI section

仓库 `.github/workflows/ci.yml` 的触发条件：`on.push.branches=[main]` +
`on.pull_request.branches=[main]`。push 到 `chore/upstream-0.444.1-pi-0.84.1` 不触发
CI（Situation B）——CI 只在 pull_request → main 时运行。为获得第二环境证据，按
任务书 Section 16 情况 B 创建 **Draft PR**（base=main, head=chore/upstream-0.444.1-pi-0.84.1），
仅用于触发 CI，**不 merge、不开 auto-merge**。CI 完成后结果记录于本文件 + 最终验收矩阵。

## 遗留风险（非阻塞，收口阶段未变）
- 真实 OAuth 登录 / 真实 Provider 网络调用未做人工端到端验证（同迁移任务遗留）。
- better-sqlite3 native binary 为 v24.16.0 重建，CI/其他开发者默认 node 不同时需各自 rebuild。
