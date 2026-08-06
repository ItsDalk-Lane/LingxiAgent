# PROGRESS — 同步上游 openhanako 0.443.46 + 升版 0.1.21 预览

> 执行者日志。断点续跑先读本文件。每完成一项立刻更新。

## 理解的目标 / 顺序 / 最大风险
- 目标：把上游 openhanako 0.442→0.443.46（94 文件，+6573/−716，旗舰=document-extract 文档抽取模块）同步进本下游分叉 Lingxi，再升版 0.1.21 打 GitHub prerelease。
- 顺序：任务0预检 → 1核心层 → 2 desktop三方合并 → 3依赖/manifest → 4验证 → 5版本 → 6digest+pack → 7发布。
- 最大风险：desktop 35 文件三方合并遗漏 Lingxi 改名定制（hanakoHome→lingxiHome、HanaEngine→LingxiEngine 等）导致 UI 回退；次大风险：直接覆盖 core/lib 层清掉改名定制（已用证据规避，见下「策略偏离」）。

## 任务 0 · 预检（已完成，全绿可动工）
- git：工作区干净，分支 main，HEAD `2780c55`。
- Node：v24.16.0（满足 >=24.12 <25）；npm 11.13.0。
- 上游仓库：`/tmp/openhanako-upstream` 就位，tag `v0.442.0` / `v0.443.46` 均存在。
- 上游 diff：94 文件 +6573/−716。分组：desktop 35 / tests 28 / lib 7 / server 6 / scripts 4 / core 4 / build 2 / 其余各 1。与任务书一致。
- 上游 package.json diff 极小：仅 version + `@firecrawl/anydoc@0.1.2` 一处依赖新增，无其它改动。
- 上游无文件删除（diff-filter=D 为空）。新增文件 20 个（document-extract 3 + 路由 2 + 测试/fixture 15）。

### 判卷指纹（sha256，交付须不变）
- `.github/workflows/build.yml`：`7e7cd144eabcf2d1407d71da35d29efda10cea1cc20fb13d8b05c11c0ab28cdf`
- `.github/workflows/publish-train.yml`：`3a9d7ca0c0004d99cc4f40962e95992246930fd2976915917e4744895c9e4415`
- `AGENTS.md`：`3a7a81d064bfa48f0dc3280958d8b12be772f36ea4f4b804795c66564e302b52`

### 测试基线（开工前，须保持不退步）
- `npm test`：10431 tests, **10419 passed**, 7 skipped, **5 failed**。
- 5 个失败 case 的精确身份（任务4「仍是原来那5个」判据）：
  1. `desktop/src/react/__tests__/components/DeskSection.test.tsx > DeskSection workspace watching > marks the right workspace card with the Jian drawer state for overlay layout`（1）
  2. `tests/persistence-schema-tripwire.test.ts > ... uses real SQLite stores and matches the deterministic committed fingerprint`（指纹漂移，见下）
  3. `tests/persistence-schema-tripwire.test.ts > ... records the digest method and the compiler in the payload it pins`
  4. `tests/persistence-schema-tripwire.test.ts > ... ignores JSDoc drift, including the file headers this guard once taxed`
  5. `tests/persistence-schema-tripwire.test.ts > ... ignores comment-only drift in the sources it hashes`
- 注：另有 34 个测试*文件*级失败（import/transform 失败导致整个文件不计入），属 plugin 系列噪声，vitest 只计 5 个真实 case failed。任务书基线「5 failed」与之吻合。
- persistence-schema-tripwire 根因：已提交指纹 sha256 `168dec1e5ff7d10d2e460a424992ff943c1e0a3ae16575179ee7588a1f6ad047` 与当前源码扫描 `6fdea4a93ab8a797e463479fbf3305facd712fe7f04c5169c0cdb47c043834d0` 不符（committed 过期）。**开工前就存在，与本次上游同步无关**，属任务书基线承认的红。本次不修复（见 BLOCKED）。

## 策略偏离（任务书「直接覆盖」→ 改为「git 三方合并」，更优且合规）
任务1要求 core/lib/server 层「直接对齐到 v0.443.46（覆写）」。实测发现：这些文件本项目含大量 Lingxi 改名定制（engine.ts: hanakoHome→lingxiHome 共 43 处、HanaEngine→LingxiEngine、HanaAgent→LingxiAgent；core/server-identity、server-runtime-context 等数十文件同步改名）。**直接覆盖会清回上游 hanako 命名，静默丢弃本项目核心定制——踩红线「禁止静默降级/不覆盖定制」，违反让步最高项「合得对」。**

证据（git merge-file 三方合并 base=上游v0.442.0 / ours=本项目 / theirs=上游v0.443.46）：
- 任务1 的 20 个源码文件中 **17 个 0 冲突**（上游改动与 Lingxi 改名落点不同行，git 能自动干净合并保留两边）。已验证 engine.ts 合并后 lingxiHome 43 处、LingxiEngine 3 处全保留，0 残留冲突标记。
- 仅 3 处需人工：`server/routes/sessions.ts`(1块)、`build/cli-runtime-closure.json`(1块)、`build/persistence-schema-fingerprint.json`(4块)。
- 故改用 git 三方合并落地核心层；冲突点手工裁决（保留 lingxiHome 改名 + 并入上游逻辑）。这完全符合任务书「建议有更好的路就走并记录」与红线。

## 进度
- [x] 任务 0 预检（全绿）
- [x] 任务 1 核心层同步（18 源码文件 git 三方合并 + sessions.ts 手工 + build json 同步 + 新建 document-extract/路由/测试/fixture；+补 update-config-non-focus.test.ts 配套合并）
- [x] 任务 3 package.json @firecrawl/anydoc@0.1.2 + export-manifest 5 条 + npm install（lock 纯增量 143+/0-）
- [x] 任务 2 desktop 35 文件（2 拷贝 + 31 自动 + main.cjs/api.ts 手工；6 文件定制已核）
- [x] 任务 5 版本号 0.1.2→0.1.21（仅 version 一处）
- [x] 任务 4 验证（typecheck 0 error；npm test 10597 passed / +178 / failed=5=基线；document-extract 反向验证 红→绿）
- [x] 任务 6 release-digest v1+v2 追加 v0.1.21（validate 通过）；npm run pack 卡在本地 codesign（pre-existing xattr，BLOCKED B4）
- [⏸] 任务 7 发布 prerelease —— **停于本地提交，未推 tag**（按死规矩：pack 未 EXIT 0 不推 tag）

## 最终状态（2026-08-06 13:00）
- 已提交：commit `362f815` + `8840d1a`，95 files changed, +7003/−770，工作区干净。
- 代码同步全绿：typecheck 0 error / npm test 10597 passed(+178) / failed=5(=基线) / 反向验证 红→绿 / digest validate 通过 / 判卷三指纹不变。
- **任务7 已执行（用户裁决路径1：直接让 CI 打包）**：
  - `git push origin main` → `2780c55..8840d1a main -> main` ✓
  - `git tag v0.1.21`（指向 8840d1a）+ `git push origin v0.1.21` → `[new tag] v0.1.21 -> v0.1.21` ✓
  - CI build.yml 已触发：run `31074231296`（v0.1.21, in_progress）。本地 pack 的 codesign xattr 卡点不适用于 CI 环境。
  - 注：本地 `npm run pack` 的 EXIT 0 前置未在本地达成（BLOCKED B4，pre-existing 环境问题），用户裁决接受「核心打包链路已验证健康 + CI 打包」替代，直接推 tag。

## 任务 4 验证过程中的关键发现与修复
全量测试暴露了任务1清单的**系统性遗漏**——上游 0.442→0.443 不仅改了 94 个文件的实现，还配套改了**多个已存在的测试文件**（不在「新增」清单里，容易被漏）。逐轮定位并修复：
1. `tests/update-config-non-focus.test.ts`（config-coordinator 删 setMemoryEnabled 的配套）— 三方合并，删旧用例换 persistSessionMeta 用例。
2. `tests/session-route-errors.test.ts`（sessions.ts 分层响应配套，上游新增）— 直接拷贝后 hanakoHome→lingxiHome 改名适配（2 处）。
3. `tests/mcp-runtime.test.ts` + `tests/chat-route-switching.test.ts`（mcp/manager + chat 改动配套）— 三方合并。
4. **系统性扫描**发现共 11 个「上游改过、本项目遗漏」的测试文件，全部三方合并落地（build-server-deps / chat-route-session-identity / cli-closure-census / file-history-watcher / hub-plugin-session-agent-capabilities / mcp-reconnect / mcp-routes / server-startup-diagnostics-contract / session-tool-write / sessions-route / startup-contract）。persistence-schema-tripwire 按基线红不动。
5. `tests/server-startup-diagnostics-contract.test.ts`（main.cjs writeCrashLog 双行配套）— 5 处 HanaAgent/hanakoHome→LingxiAgent/lingxiHome 改名适配（含负向断言 not.toMatch）。
- 结论：合并不仅要对齐实现，还要对齐「上游对既有测试的配套修改」+「直接拷贝的上游测试需做 lingxiHome/LingxiAgent 改名适配」。全部修复后 failed 从 10→7→5，稳定回到基线。

