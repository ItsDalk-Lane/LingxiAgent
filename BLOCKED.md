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
