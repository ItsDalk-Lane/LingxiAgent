# F1–F12 测试报告

日期：2026-09-07 · 平台：darwin 25.6.0 arm64 · Node 24.16.0 · vitest 4.1.10
基线：fc4a6df7（clean 工作区起步，全部改动为本次未提交变更）

## 1. 定向测试（按 F 分组，先红后绿；红状态记录在各阶段证据文件）

| 组 | 测试文件 | ID 范围 | 结果 |
|---|---|---|---|
| F9/F3 | tests/pinned-tenets-migration.test.ts、tests/pinned-tenets-migration-recovery.test.ts、tests/tenets-source-quota.test.ts、tests/memory-tenets.test.ts | M01–M20 | 全绿 |
| F1/F2 | desktop/src/react/__tests__/components/composer-send.transaction.test.ts、desktop/src/react/__tests__/services/composer-send-coordinator.test.ts（含 P07/P08/P10、X 系列映射） | S01–S15 | 全绿 |
| F4–F6 | tests/system-speech-permission-bridge.test.ts、tests/system-speech-adapter.lifecycle.test.ts、tests/speech-helper-build-contract.test.ts、tests/speech-recognition-service.test.ts | A01–A16 | 全绿 |
| F7 | desktop/src/react/__tests__/components/InputArea.system-dictation.test.tsx（19 用例含 X04） | D01–D12 | 全绿 |
| F8 | tests/reserved-tag-text-preservation.test.ts（真实 TipTap + createChatRoute 实时投影 + buildItemsFromHistory）、tests/reserved-tag-orphan-closer.test.ts、think/mood/chat-route 套件 | T01–T16 | 全绿 |
| F10 | tests/paste-fidelity-serializer.test.ts、desktop/src/react/__tests__/utils/paste-fidelity-editor.test.ts（真实 TipTap Editor） | P01–P12 | 全绿 |
| F11/F12 | tests/speech-parameters.test.ts、tests/speech-response-lifecycle.test.ts | V01–V13 | 全绿 |
| X 组合 | 上表内嵌 + tests/x05-speech-concurrency.test.ts | X01–X16 | 全绿（映射见 evidence/p9-regression-build.md） |

红→绿纪律：每阶段先落反例测试并记录 RED（p5-red-frontend.txt 及各证据文件的“红状态记录”
节），实现后转绿；既有断言的契约升级（5 个转义用例、S13 tripwire、首尾空行断言）均在证据
中写明理由，无删断言/放宽带宽。

## 2. 全量

`npm test`（vitest run 全仓）：**13243 passed / 0 failed / 7 skipped（13250）**，
exit 0，82s。日志：logs/full-vitest-final.txt。
（P6–P8 期间三轮满载跑各出现 1–2 例 pinned-tenets-migration M10 偶发失败，单跑恒绿、
未触碰该模块，按疑似既有 flaky 如实记录；未修改任何断言。）

## 3. 构建与门禁

| 命令 | exit | 结果 |
|---|---|---|
| npm run typecheck（tsc×3） | 0 | 通过 |
| npm run lint | 0 | 0 errors / 9096 warnings（既有基线） |
| npm run build:client | 0 | vite 多入口通过 |
| npm run build:server | 0 | 一次性签名密钥本地验证构建；seed 启动冒烟通过 |
| npm run build:speech-helper | 0 | 通过 |
| npm run pack（干净 dist-speech） | 部分 | 打包/ad-hoc 重签名/资源入包通过；Apple 公证 BLOCKED（凭证）。pack 自身从零重建 dist-speech 两产物（无预生成掩盖） |
| cli-closure-census | 0 | 22/22 |
| persistence-schema-tripwire | 0 | 15/15（无 schema 变化，未 repin） |
| post-verification-audit-seal | 0 | 3/3（VERIFIED_SOURCE_SHA 未推进；候选源码未提交） |

## 4. 原生（Swift / ObjC++ / Electron）

- `swift run lingxi-speech-core-tests`：**16/16**（logs/swift-core-tests.txt）。备注：
  `swift test` exit 1（CLT 无 XCTest，Package 以可执行 runner 为实际测试 target——如实记录）。
- Electron 运行时加载 .node 授权桥（ELECTRON_RUN_AS_NODE）：exports 可枚举、
  rawStatus=0、dispose ok（logs/electron-node-bridge-load.txt）——mock 之外的真实验证。
- 真实 helper fail-closed 冒烟（合成 WAV）：结构化 PERMISSION_REQUIRED、exit 2、
  无弹窗（logs/helper-failclosed-smoke.txt）——真实二进制行为，未授权环境下的预期失败。

## 5. 真实环境验证（与 mock 严格区分）

| 项 | 状态 | 说明 |
|---|---|---|
| 真机首次授权→授权后识别→拒绝→恢复 | **BLOCKED** | 需明确授权的 macOS 验收环境；未执行，不以 mock 冒充 |
| 设备内识别断网验收 | BLOCKED | 同上 |
| 真实云端 TTS（OpenAI/MiniMax/DashScope 短文本、可播放性、用量） | **BLOCKED** | 未提供凭证；未寻找/复用他人凭证 |
| macOS x64 / Windows / Linux | BLOCKED | 无设备；仅本机 arm64 构建验证 |
| Apple 公证 | BLOCKED | 发布凭证缺失 |

真实执行过的部分：Electron .node 加载、Swift helper 真实编译与 fail-closed 行为、
pack 真实打包与 ad-hoc 签名结构（以上均为本机真实工件，非 mock）。
