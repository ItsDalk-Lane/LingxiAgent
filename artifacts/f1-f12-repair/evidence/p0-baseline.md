# P0 基线记录（2026-09-07）

## 固定环境

| 项 | 值 |
|---|---|
| HEAD | fc4a6df7714a2a26c423be993147e4772b2883ea（与任务书固定 HEAD 一致） |
| 分支 | refactor/dismantle-and-voice-features |
| 工作树 | 干净（`git status --short` 无输出） |
| 功能源码提交 | 04f90d2b26449d2f3264a5ea2a1a8bd6fb253202 |
| 源码父基线 | 3b40fbe646dc04aa29fa6d21230bfca0c7b17d9f |
| node | v24.16.0（package.json engines: >=24.12.0 <25，符合） |
| npm | 11.13.0 |
| platform/arch | darwin arm64 |
| Electron | 42.8.1（devDependencies） |
| 包管理器 | npm（package-lock.json） |
| 测试 | vitest ^4.1.10，根 vitest.config.ts，testTimeout 60s，setupFiles tests/setup-auto-updater.ts |

git log -3：
```
fc4a6df7 chore(audit): advance VERIFIED_SOURCE_SHA to 04f90d2b
04f90d2b refactor: 九件改造 + 三 bug 修复
3b40fbe6 Merge PR #46 docs/governance-closeout-sep06
```

## 规则文件

- `AGENTS.md` 存在（gitignored 本地规则）已读。
- `CLAUDE.md`：待确认是否存在。
- `README`：存在。
- `docs/README.md` 文档入口存在。
- `.sync-audit/`：verified-source-sha.txt + verify-post-verification-diff.mjs + tests/post-verification-audit-seal.test.ts 构成封印门禁。**本轮无提交授权，VERIFIED_SOURCE_SHA 不推进。**

## 测试/构建命令（以实际 package.json 为准）

- `npm test` = vitest run（排除 .claude/.cache/dist*/dist-server/dist-computer-use/dist-sandbox）
- `npm run typecheck` = tsc x3（root + tsconfig.node + tsconfig.test）
- `npm run lint` = eslint .
- `npm run build:client` = build:main + build:preload + build:renderer + build:splash + build:theme
- `npm run build:server` = scripts/build-server.mjs
- `npm run build:speech-helper` = scripts/build-speech-helper.mjs
- `npm run pack` = build:computer-use-helper + build:client + build:server + verify:seed-kit + electron-builder --dir

## 关键预发现（待 P4 核实）

- **pack / dist / dist:win / dist:linux 均未调用 build:speech-helper**；但 `build.mac.extraResources` 已把 `dist-speech/mac-${arch}/` 映射到 `speech/macos/`。干净 checkout 上 pack 会因产物缺失失败或打包缺 helper——F6 闭环缺口实锤候选。
- `build.mac.extendInfo` 已含 NSSpeechRecognitionUsageDescription 与 NSMicrophoneUsageDescription。
- `build.afterSign` = scripts/notarize.cjs（唯一 afterSign 入口）。

## 既有相关测试文件

- tests/pinned-tenets-migration.test.ts、tests/memory-tenets.test.ts
- tests/reserved-tag-orphan-closer.test.ts、tests/chat-route-reserved-raw-fallback.test.ts、tests/live-history-reserved-tag-parity.test.ts
- tests/paste-fidelity-serializer.test.ts
- tests/speech-recognition-adapters.test.ts、tests/speech-recognition-route.test.ts、tests/speech-recognition-service.test.ts
- tests/media-task-store.test.ts、tests/model-call-payload-speech.test.ts、tests/model-call-speech-observer.test.ts、tests/model-observability-e2e-media-speech.test.ts
- desktop/src/react/__tests__/components/InputArea.media-send.test.tsx、InputArea.paste-and-slash.test.tsx、InputArea.draft-sync.test.tsx 等

## 权限与边界确认

- 无提交/推送/PR/合并/发布授权；VERIFIED_SOURCE_SHA 保持不动。
- 不运行 `npm run install:local`；不触真实 LINGXI_HOME；测试一律临时目录 + 独立 LINGXI_HOME。
- 不运行 `git reset --hard` / `git clean -fd`；不 stash。
- macOS 真机授权/TCC 相关验收：本机是 macOS arm64 真机，但未获授权操作真实 TCC/安装到 Applications；F4 真机授权验收视情况标 BLOCKED 或仅限非破坏性功能验证。

## 工具链（本机实测）

- Swift 6.3.3（/usr/bin/swift，可跑 swift test / 构建 helper）
- Apple clang 21.0.0（可构建 Objective-C++ Node-API 授权桥）
- electron 42.8.1 已装于 node_modules（可做原生模块 Electron 加载验证）
- CLAUDE.md **不存在**（按 P0.1 记为不存在，不补造）

## 持久化/边界门禁（影响 P1/P4 改动的登记义务）

- `build/persistence-schema-fingerprint.json`：57 个被守卫模块。F1–F12 目标文件中 **desktop/main.cjs 与 server/index.ts 在守卫集内**；新增 IPC/原生桥若触及须按 `--classification` 实评后 repin。
- 新增迁移收据存储须登记进持久化清单（scan-persistent-stores / fingerprint registry），不加豁免绕过。
- 其他边界测试：shell-surface-manifest、open-boundary-lint、execution-boundary、tool-invocation-boundary 等存在；改 shell surface（新增 IPC channel）须同步更新对应 manifest 派生件（P9.5）。

## 证据目录

`artifacts/f1-f12-repair/`（evidence/ logs/ patches/）。
