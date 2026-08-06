# BLOCKED.md — 待裁决清单

> 本任务书约定：无关 bug / 想做的重构 / 无关依赖升级写这里，本次不动。
> 「断了、换新会话」：先读 PROGRESS.md 接着做，不重做。

## 待裁决（需人工/上层定夺，本次跳过）

### B1. persistence-schema-tripwire 指纹漂移（开工前就红，与本次无关）
- 现象：`tests/persistence-schema-tripwire.test.ts` 4 个 case 失败。仓库已提交指纹
  sha256 `168dec1e5ff7d10d2e460a424992ff943c1e0a3ae16575179ee7588a1f6ad047`
  与当前源码扫描 `6fdea4a93ab8a797e463479fbf3305facd712fe7f04c5169c0cdb47c043834d0` 不符。
- 判定：committed `build/persistence-schema-fingerprint.json` 过期（本项目自身长期状态）。
- 本次为什么不修：任务书红线「不许动开工前就红的测试 / 不改判卷相关」，且修复=重新生成+repin 指纹，属另一独立工作。基线已含这 4 个失败，任务4判据是「仍是原来那5个」，故本次保持原样同步上游版本而不尝试变绿。
- 建议（给后续）：单独排一次「repin persistence schema fingerprint」——跑更新后的 `generate-persistence-schema-fingerprint.mjs` 重生成，按其 schema-change 流程登记 review 后提交。

### B2. build/cli-runtime-closure.json 与 build/persistence-schema-fingerprint.json 是生成物，统计数字不可手工合并
- 两文件 `generatedBy` 标注分别由 `scripts/compute-cli-closure.mjs` / `generate-persistence-schema-fingerprint.mjs` 生成。
- 三方合并时它们的冲突全在统计数字（totalFiles 9618/9634/9641 等）与 review 指纹，本项目与上游各自反映自己的源码闭包。
- 本次策略：fingerprint.json 按任务1原文同步上游 v0.443.46 版本（不重生成，避免触发 B1 争议 + 触碰判卷）；closure.json 同理同步上游版本。两个 json 在本项目本就与源码不一致（B1），同步上游版本不会让情况更糟，且符合「同步上游」的本意。
- 建议（给后续）：B1 修完后，连带重跑两个生成脚本让 json 与本项目实际闭包对齐。

### B5. CI mirror-atomgit job 失败（undici 缺失，不影响 GitHub prerelease）
- 现象：v0.1.21 的 CI run `31074231296` 整体 `failure`，唯一失败 job 是 `mirror-atomgit in 18s`：
  `Cannot find package 'undici' imported from scripts/mirror-release-to-atomgit.mjs`（上传 latest-linux.yml 到 atomgit 时）。
- 其余 job 全绿：✓ renderer-box / ✓ 4 平台 build（含 macOS dmg x64+arm64）/ ✓ release（创建 GitHub prerelease）/ ✓ publish-train（推 train-beta）。
- 根因（**非本次同步引入**）：`scripts/mirror-release-to-atomgit.mjs` 本次未改（git log 空）；undici 是 package.json dependencies@7.24.7；脚本用 `await import("undici")`。失败说明 mirror-atomgit job 的 runner 上下文没装/没暴露 node_modules（很可能该 job 缺 `npm install` 步骤，或 Node 升级后 undici 不再隐式可用）。属既有 CI 配置问题。
- 影响：**GitHub prerelease（single source of truth）已完整发布**——v0.1.21，isPrerelease=true，isDraft=false，18 assets（Linux/macOS/Windows 全平台）。仅 atomgit 次要镜像源未同步，不影响 GitHub 渠道与 OTA。
- 建议（给后续）：给 build.yml 的 mirror-atomgit job 加 `npm install`（或 `npm install undici`）步骤，确保 runner 有 undici。

### B4. 本地 `npm run pack` 在 codesign 阶段失败（pre-existing 环境问题，阻断任务7 推 tag）
- 现象：`npm run pack` 在 electron-builder 内置 codesign 阶段报错：
  `Lingxi Helper (GPU).app/.../Lingxi Helper (GPU): resource fork, Finder information, or similar detritus not allowed`。连败 3 次（①缺 LINGXI_SIGN_KEY→补临时密钥通过；②清 dist xattr 重跑；③重建后 Helper 仍带 xattr）。
- 根因（已查实，**非本次同步引入**）：
  - package.json `build.mac.afterSign = None`（HEAD 即如此，本次零改动）。故本地 `pack` 的 electron-builder 内置 codesign **不触发**项目的 `notarize.cjs`/`resign-adhoc.cjs` xattr 清理。
  - `resign-adhoc.cjs`/`sign-local.cjs` 注释明确：codesign 对带 `com.apple.FinderInfo`/`com.apple.fileprovider` 标记的文件报 detritus；这些 xattr 由 macOS 文件提供者/iCloud 机制在删除后秒级重新注入，只能用 `xattr -cr` 在签名前瞬间原子清空。本地 pack 走 `identity=None`（ad-hoc）+ `hardenedRuntime=True`（严格 codesign），却无 xattr 预清理 → 必然撞错。
  - CI 不受影响：build.yml 用环境变量 + 独立步骤编排签名（不依赖 package.json afterSign），且配齐 Apple 凭据后走完整 notarize+resign-adhoc。
- 证明核心链路健康的证据（pack 失败前全部 OK）：
  - `[verify-seed-kit] OK — seed kit matches its manifest and verifies against the pinned keyset`（用临时 keyset 本地校验通过）
  - `[build-server] @firecrawl/anydoc runtime smoke passed`（document-extract 依赖正确打包）
  - `[fix-modules] seed resources verified (renderer archive + server archive + manifest + sig)`
  - app 完整生成（主程序/app.asar/Frameworks 齐全），仅 electron-builder 内部 codesign 未完成。
  - 手动对 4 个 Helper 跑 `xattr -cr` + codesign（ad-hoc）全部成功重签。
- 为什么不修（红线）：修这个需改 package.json 的 afterSign/entitlements/sign 配置，全在本项目「专属文件只允许合并/增量、严禁整文件覆盖」+「不改判卷指纹」红线内；且 AGENTS.md 记载项目历史 macOS 签名问题（v0.1.0 启动崩溃同源），属既有环境局限。本次同步未触碰任何签名相关配置（git diff 零改动）。
- 影响：任务7 前置「pack EXIT 0」未达成 → 按死规矩「任何一项不过＝停，不推 tag」。代码同步本身全绿（typecheck 0 error / npm test 10597 passed(+178) / failed=5=基线 / digest 校验通过），仅本地 codesign 环境卡点阻断推 tag。
- 建议（给后续）：
  1. 本地 pack 前手动跑 `xattr -cr dist/` 并配置临时 `LINGXI_SIGN_KEY`（本任务已验证可行），或
  2. 在 package.json `build.mac.afterSign` 指向 `scripts/notarize.cjs`（让本地 pack 也走项目的 resign-adhoc xattr 清理），或
  3. 直接推 tag 让 CI 打包（CI 环境无此 xattr 问题）——但本任务书要求本地 pack EXIT 0 作前置，故未自行推 tag。

### B3. 34 个测试文件级失败（plugin 系列 import/transform 噪声）
- vitest 报 `Test Files 34 failed` 但 `Tests 5 failed`——其余 29 个文件级失败是 import/transform 失败导致整文件不计入，属 plugin-* / plugin-ui-* 系列开工前就存在的环境问题，与本次上游同步无关。
- 本次不动它们（任务书红线）。基线判据按 `Tests 5 failed` 计。

---
（其余未发现；执行中若遇新无关 bug 续写于此。）
