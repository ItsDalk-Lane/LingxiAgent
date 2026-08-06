# PROGRESS — pi SDK 0.80.3 → 0.83.0 例行跟进升级

> 执行者日志。断点续跑先读本文件。每完成一项立刻更新。

## 理解的目标 / 顺序 / 最大风险
- 目标：把仓库 pi SDK（@earendil-works 的 pi-agent-core / pi-ai / pi-coding-agent）三包从 0.80.3 真实装到 0.83.0，类型检查/测试/服务端冒烟不劣于基线。例行跟进，不加新功能。
- 顺序：任务0 基线核对 → 任务1 升版+哨兵 → 任务2 生产码迁移 → 任务3 测试+指纹 → 任务4 总验收。
- 最大风险：0.83.0 把 AuthStorage 上的 OAuth/login/getOAuthProviders 全移到 ModelRuntime/Models，4 个消费方（oauth-force-refresh / auth.ts / providers.ts / pi-sdk index）受牵连；其次 6 处 createAgentSession options 从 authStorage+modelRegistry 改收 modelRuntime。
- 让步顺序：不回归 ＞ 升级到位 ＞ 改动最小。死规矩不许违反。

## 基线核对（任务0，2026-08-06 全部亲测）
- `npm run typecheck`（三段 tsc）exit 0 ✓
- pi 六文件：`npx vitest run tests/pi-sdk-oauth-login-adapter.test.ts tests/pi-sdk-image-resize.test.ts tests/pi-sdk-create-session-adapter.test.ts tests/pi-sdk-search-tools.test.ts tests/model-manager-auth-storage.test.ts tests/oauth-force-refresh.test.ts` → **46 passed (6 files)** ✓
- 全量 `npx vitest run`：**Tests 10616 | 10604 passed | 5 failed | 7 skipped**（5 failed = 4 tripwire 指纹过期 + 1 DeskSection；34 文件级失败为 plugin 噪声）。与任务书基线一致 ✓
- 已装版本：0.80.3 0.80.3 0.83.0 0.83.0（pi-coding-agent / pi-ai / pi-agent-core）

## 0.83.0 契约（拆 npm 包逐条验证，非猜测）
- **AuthStorage / FileAuthStorageBackend**：不再从包根导出，仍在 `dist/core/auth-storage.js`（深路径）。`AuthStorage.create(authPath)` 保留；`FileAuthStorageBackend` 构造收 authPath。**AuthStorage 上 login / getOAuthProviders / get 方法全删**（只剩 create/fromStorage/inMemory + read/modify/delete/list/reload）。read 改 async。
- **createAgentSession(options)**：仍从包根导出（dist/core/sdk.js）；options 不再收 authStorage/modelRegistry，改收可选 `modelRuntime`（不传则内部 `await ModelRuntime.create({authPath, modelsPath})`）。
- **ModelRuntime**：从包根导出（dist/core/model-runtime.js）。`static async create({credentials, authPath, modelsPath, ...})`。credentials 可传 AuthStorage 实例（内部包 RuntimeCredentials，调 store.read/list/modify/delete，AuthStorage 全有 → auth.json 同一把锁语义保住）。
- **ModelRegistry**：从包根导出，但变成 ModelRuntime 的同步兼容 facade：`constructor(runtime: ModelRuntime)`，无静态 create。registerProvider/unregisterProvider 委托 runtime。
- **OAuth 新范式**：provider 注册表移到 pi-ai `Models`。`ModelRuntime.getProvider(id)` 返回 **composed provider**（dist/core/provider-composer.js composeModelProvider 产物），其 `.auth.oauth` 有 `.login(interaction)/.refresh(cred)/.toAuth(cred)`。extension 注册的 provider（Hana 走 registerProvider）oauth 走 `adaptOAuth` 包装。
- **pi-ai compat/extension-oauth-types**：旧 `OAuthLoginCallbacks` 形状（onAuth/onDeviceCode/onPrompt/onSelect/onManualCodeInput/signal）作为 compat 类型保留；composed extension provider 的 oauth.login 仍吃这套回调形状。
- **ModelRuntime.login(providerId, type, interaction)**：委托 `this.models.login`，interaction 是新形状（prompt/notify/signal）。
- **prepareCompaction** 深路径 `dist/core/compaction/compaction.js` 还在 ✓；pi-ai /compat（getModel/getModels/completeSimple）、StringEnum、pi-agent-core runAgentLoop 都还在 ✓。
- **CURRENT_SESSION_VERSION** 两版均 = 3，无数据迁移 ✓。
- **models.json compat**：0.80.7 起 `compat.sendSessionIdHeader` 删，换 `compat.sessionAffinityFormat`（"openai-nosession"=不发 session 头，其余照发）。provider-cache-affinity.ts:28 读旧字段须迁，双向语义保持。

## 迁移设计（最小桥接，对齐任务书）
1. `lib/pi-sdk/index.ts`：AuthStorage/FileAuthStorageBackend 改深路径相对引；新增 `createModelRuntime({credentials, authPath, modelsPath})` 门面；`createModelRegistry` 内部先建 ModelRuntime 再包 ModelRegistry；`loginOAuthProvider` 改收 modelRuntime（旧 OAuthLoginCallbacks 形状适配到 ModelRuntime.login 的 interaction）；新增 getOAuthProviders / force-refresh 门面供 routes 用。
2. `core/model-manager.ts`：init() 建 ModelRuntime（credentials=现有 AuthStorage）+ 包 ModelRegistry；新增 `modelRuntime` getter 暴露给消费方。
3. 6 处 createAgentSession（session-coordinator 2149/7913 + bridge 1243/1722 + agent-executor 281/510 + session-coordinator createSessionContext 7502）：authStorage+modelRegistry → modelRuntime。
4. `core/oauth-force-refresh.ts`：getOAuthProviders() → modelRuntime.getProvider(id)；provider.refreshToken/getApiKey → provider.auth.oauth.refresh + 写回 backend 同一把锁。
5. `server/routes/auth.ts` + `providers.ts`：getOAuthProviders/login 走 modelRuntime 门面。
6. `lib/llm/provider-cache-affinity.ts`：sendSessionIdHeader → sessionAffinityFormat。

## 进度
- [x] 任务0 基线核对（全绿）
- [x] 任务1 升版 + 哨兵反向验证（红→绿证据见下）
- [x] 任务2 生产码迁移（tsc.node exit0，0 个 as any/@ts-ignore/@ts-expect-error）
- [x] 任务3 测试 + 指纹（tsc.test exit0 / pi 六文件 46 绿 / tripwire 15 绿）
- [x] 任务4 总验收（**failed=1=仅 DeskSection 基线，达 ≤1**；全部指标达标）

## 收尾（用户追加要求，2026-08-06）
- **model-sync 视频测试「expected 1 got 16」根因修复**：
  - 复现命令：`ANTHROPIC_AUTH_TOKEN=x ANTHROPIC_BASE_URL=http://127.0.0.1 npx vitest run tests/model-sync.test.ts -t video` → 必现 `expected [ …(16) ] to have a length of 1 but got 16`（不设这俩 env 不复现，所以前一轮没撞到）。
  - 根因（已定位）：0.83.0 的 ModelRuntime.getAvailable()/getAvailableSnapshot() 经 runAvailabilityRefresh→checkAuth 把「环境凭据可用」的内置 provider（如设了 ANTHROPIC_AUTH_TOKEN 的 anthropic）算进 configuredProviders/availability；0.80.3 的 ModelRegistry.create 不算。桥接层 createModelRuntime 透传了新语义，model-manager.refreshAvailable 与 model-sync 测试都依赖旧语义，于是内置目录（16 个）漏进可用集合。`available` 从 1（仅 dashscope）变成 17（dashscope + 16 个 env-auth 内置），经 model-manager 的 projection 过滤后生产侧表现不明显，但直接调 modelRuntime.getAvailable() 的测试当场撞红。
  - 修法（恢复 0.80.3 语义，不改断言/不删 env 凑绿）：在 `lib/pi-sdk/index.ts` 的 createModelRuntime 外加 `withLegacyAvailabilityScoping(modelRuntime)`——包装 getAvailable / getAvailableSnapshot，只保留 provider 命中「显式配置集合」的模型；显式配置集合 = models.json config provider ∪ extensionProviders ∪ nativeExtensionProviders。仅靠 env 凭据激活的内置 provider 不在该集合，被滤掉；registerProvider 注册的 OAuth provider（xai-oauth/openai-codex）仍在 extension 集合，登录后照常可用（model-manager-auth-storage 29/29 验证）。
  - 验收：带/不带 env 都绿——`ANTHROPIC_AUTH_TOKEN=x … -t video` 5/5、`-t video` 5/5；model-manager-auth-storage 29/29；全量 npm test（带 env 跑）failed=1（仅 DeskSection 基线）。
- **草稿文件清理**：删除 build/ 下 22 个 `.cli-closure-nft-scratch-nft-{server,cli}-bundle *.mjs`（macOS 复制残留，非 compute-cli-closure.mjs 生成——脚本用确定名 + finally 清理，跑完不残留，已验证 cli-closure 测试跑后不重生）+ lib/extensions/ 下 2 个 `compaction-guard-ext {2,3}.ts`（带空格的副本）+ `export-manifest 2.json`。

## 白名单放宽记录（用户二次裁决授权，2026-08-06）
完成校验器判定 failed≤1 未达成（原 failed=7），用户裁决「放宽白名单修绿」，把以下文件按「升级必经的产物重生成」（与 persistence 指纹同处理）临时纳入可改：
- `scripts/compute-cli-closure.mjs`：DYNAMIC_CALL_ALLOWLIST 追加 2 条（pi-coding-agent 0.83.0 resolve-config-value.js 的 spawnSync(shell)+execSync(command) 命令解析 fallback）。
- `build/cli-runtime-closure.json` + `build/open-boundary-baseline.json`：重生成（node scripts/compute-cli-closure.mjs）。
- `export-manifest.json`：白名单补 3 个 pi-sdk 0.83.0 牵出的新路径——`lib/pi-sdk/auth-facade.ts`（新增桥接文件）、`node_modules/@earendil-works/pi-coding-agent/dist/core/auth-storage.js`（深路径引）+ 其 SDK 内部依赖 `dist/core/resolve-config-value.js`、`dist/utils/shell.js`（auth-storage→resolve-config-value→shell→config 依赖链，config 已在 manifest）。
- `tests/compaction-guard-ext.test.ts`：GLM case 的 glmHistory 改 canonical 数组 content（pi-coding-agent 0.83.0 convertToLlm 不再归一 wire 形态，serializeConversation 要求数组 content），保持「GLM tool_call 无 reasoning_content → cache-recovery」测试意图。

## 最终验收输出（任务4，全部贴实际命令输出）
- **三包版本查询**：`node -p "..."` → `0.83.0 0.83.0 0.83.0` ✓
- **哨兵**：`node scripts/patch-pi-sdk.cjs` → `[verify-pi-sdk] all checks passed` exit 0 ✓（反向验证红→绿证据见上）
- **typecheck 三段**：`npm run typecheck` → **exit 0**（tsc + tsc.node + tsc.test 三段全绿）✓
- **pi 六文件**：`npx vitest run <6 files>` → Test Files 6 passed (6) / Tests 46 passed (46) ✓
- **tripwire**：`npx vitest run tests/persistence-schema-tripwire.test.ts` → Tests 15 passed (15) ✓
- **cli-closure + open-boundary**（重生成后）：`Tests 38 passed (38)` ✓
- **compaction-guard-ext**（GLM case 改契约后）：`Tests 53 passed (53)` ✓
- **全量 npm test**：`Tests 1 failed | 10608 passed | 7 skipped (10616)` —— **failed=1（仅 DeskSection 基线，达 ≤1）**；总数 10616（=基线，≥10616）；skipped 7（≤7）。
- **冒烟链**：`npm run build:server:open` exit 0；`npm run smoke:server:open` → positive smoke PASSED: GET /api/server/identity -> 200 / negative smoke PASSED: exit code=1 / all smoke checks passed ✓（正负双过）

## 达标情况（全部达标）
- 约束指标：三包 0.83.0 0.83.0 0.83.0 ✓；typecheck 三段 exit0 ✓；哨兵通过 + 反向验证红→绿证据齐 ✓。
- 结果指标：npm test **failed=1（仅 DeskSection 基线）≤1** ✓、总数 10616 ≥10616 ✓、skipped 7 ≤7 ✓；冒烟正负双过 ✓。

## 任务1 哨兵反向验证证据
- 三包版本查询输出：`0.83.0 0.83.0 0.83.0`
- 白名单临时改回 0.80.3 跑哨兵（RED）：`[verify-pi-sdk] SDK version 0.83.0 is not verified. Verified versions: 0.80.3` exit 1
- 改回 0.83.0（GREEN）：`[verify-pi-sdk] all checks passed` exit 0

## 任务2 迁移落地清单（实际改的）
- `lib/pi-sdk/index.ts`：AuthStorage/FileAuthStorageBackend 改深路径引；新增 createModelRuntime / 改 createModelRegistry 为 async 返 {modelRuntime,modelRegistry}；loginOAuthProvider 改收 facade/modelRuntime + 旧 OAuthLoginCallbacks→AuthInteraction 适配（onAuth↔notify auth_url 等）；导出 SdkAuthFacade/LegacyOAuthProvider。
- `lib/pi-sdk/auth-facade.ts`（新）：SdkAuthFacade 包 AuthStorage+ModelRuntime，桥接 getOAuthProviders/getApiKey/get/has/remove/logout/setRuntimeApiKey/removeRuntimeApiKey/reload 旧形状。
- `core/model-manager.ts`：init() 改 async，建 ModelRuntime+ModelRegistry+SdkAuthFacade；新增 modelRuntime getter；_removeApiKeyProviderAuthEntries 改 async（await 删除，防 ENOENT）；syncAndRefresh await _modelRegistry.refresh()；refreshAvailable 走 _modelRuntime.getAvailable()（兜底 _modelRegistry）；resolveProviderCredentialsFresh 的 forceRefresh 传 modelRuntime。
- `core/oauth-force-refresh.ts`：getOAuthProviders→modelRuntime.getProvider；provider.refreshToken/getApiKey→provider.auth.oauth.refresh + toAuth；签名 authStorage→modelRuntime。
- 6 处 createAgentSession（session-coordinator×3 + bridge×2 + agent-executor×2 + createSessionContext）：authStorage+modelRegistry → modelRuntime。
- streamFn→streamFunction（SDK Agent 属性改名，0.83.0）：session-coordinator×多处 + session-compactor + lib/pi-sdk/stream-guard.ts。
- `lib/llm/provider-cache-affinity.ts`：sendSessionIdHeader→sessionAffinityFormat（双向语义保持）。
- `server/routes/auth.ts`：logout 路由 await engine.authStorage.logout。
- `shared/provider-model-validation.ts`：THINKING_LEVEL_MAP_KEYS 加 "max"（与 Hana VALID_THINKING_LEVELS 对齐；0.83.0 目录有模型用 max 键）。
- `core/engine.ts`：await this._models.init()；新增 modelRuntime getter。

## 任务3 测试改写清单
- `tests/oauth-force-refresh.test.ts`：深路径引 AuthStorage 等；用真实 ModelRuntime（内置 openai-codex）；storedCred 读 backend.value 真理源。
- `tests/pi-sdk-oauth-login-adapter.test.ts`：建 ModelRuntime+SdkAuthFacade；selector 契约改 0.83.0。
- `tests/model-manager-auth-storage.test.ts`：init() 全改 await（21 处）+ 2 处 toThrow→rejects.toThrow + 2 callback 加 async + Grok logout 加 await。
- `tests/model-sync.test.ts`：createModelRegistry 用新 {modelRuntime,modelRegistry} 契约；refreshAvailable 兜底；opencode-go deepseek-v4-flash thinkingLevelMap 去 xhigh（0.83.0 目录漂移）。
- `tests/persistence-schema-tripwire.test.ts`：版本期望 0.80.3→0.83.0（仅字符串，断言逻辑未动）。
- `build/persistence-schema-fingerprint.json`：重生成（classification=compatible，review 如实写 pi SDK 0.80.3→0.83.0 升级，CURRENT_SESSION_VERSION=3 不变）。

## 类型报错牵出清单外文件（每改一行记原因）
（无清单外 core/server/hub 文件被改；所有改动落在白名单内。）

## 遗留风险
- 真实 OAuth 登录不做人工验证（无法自动化）——作遗留风险，交付说明里写。

## 发布 v0.1.22（2026-08-06）
- 用户追加要求：把 pi SDK 0.83.0 升级成果以 **v0.1.22 预览版**发布（GitHub prerelease，不动 stable 通道）。
- 升版：package.json / package-lock.json version → 0.1.22；electron-builder artifactName 用 `${version}` 自动跟随。
- digest：release-digest.v1.json 重写为 v0.1.22 条目（3 items：pi SDK 0.83.0 升级 / server 指针误判修复 / 关于页上游版本单一真相源），`node scripts/generate-release-digest.mjs --append-history` 追加进 v2（head 0.1.22，4 entries），v1/v2 `validate-release-digest.mjs --tag v0.1.22` 双向通过 ✓。
- 流程沿用 v0.1.21：提交 → push main → tag v0.1.22 → push tag → CI build.yml 自动出 prerelease（release job 显式 `--prerelease` + 校验 v1 digest 与 tag 一致）。
