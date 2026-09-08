applicableSource: `89bc0b64bf0a9b84ef3532efaa66c23213affb70 worktree`

replacedBy: `round2`

## S11 最终交付状态

R01–R10 本地实现/联合验证关闭；受测源码候选提交后的审计封印状态以根目录 `PROGRESS.md` 和 Git guard 为准。整体发布验收仍受 notarization 凭证、真机/供应商/跨平台验收阻塞。

最终源码摘要：`c418b3ef2fc475e40ed779f21b8e087b120834e9b673baf2e67c3758138d30d8`。

| 验证 | 真实结果 | 日志（round2/logs） |
|---|---|---|
| X2 联合冻结 | 25 文件 / 321 passed | s11-joint-freeze.log |
| typecheck | PASS | s11-typecheck-final.log |
| lint | exit 0，9203 warnings | s11-lint-final.log |
| 原样 npm test | 13383 passed / 1 failed / 7 skipped，exit 1 | s11-full-test-final.log |
| 干净 89bc0b64 同环境 audit seal | 2 passed / 1 failed | s11-audit-seal-baseline.log |
| 排除基线 seal 的补充全量 | 13381 passed / 7 skipped，exit 0；不替代原样门禁 | s11-full-test-excluding-baseline-seal.log |
| client 构建 | PASS | s11-build-client.log |
| server 构建 | 初次缺 key 失败；临时合成 key 重跑 PASS，key 已删除 | s11-build-server.log / s11-build-server-synthetic-signing.log |
| speech helper / permissions 构建 | PASS | s11-build-speech-helper.log / s11-build-speech-permissions.log |
| pack | 从无 dist-speech 开始，ad-hoc 签名及验证成功；最终 exit 1 | s11-pack-synthetic-signing.log |

原样全量唯一失败为 post-verification audit seal。verified SHA `04f90d2b` 到基线 HEAD `89bc0b64` 已有 113 个已提交差异；干净基线复现同一失败，不能修改白名单或宣称完整门禁通过。pack 最终因缺 `APPLE_APP_SPECIFIC_PASSWORD` / `APPLE_ID_PASSWORD` 失败，notarization 为 BLOCKED，不能把签名阶段成功写成 pack 全流程通过。

真实 macOS 权限/转写、真实供应商、Windows/Linux/其他架构均 BLOCKED；本地构建与合成数据不能替代这些验收。聊天工具栏上没有增加语音输入的组件是用户要求的。后端 system-speech 模型解析、上传转写、错误码、原生音频附件、当前快捷键、ASR/TTS/朗读与宿主授权桥保留，旧前端听写不恢复。真实用户恢复仍受 BLOCKED_HOME_OWNERSHIP 及单独数据授权限制。

最终增量补丁为 `patches/89bc0b64-to-r01-r10-source.patch`（相对 round2），大小 `426195` bytes，SHA-256 `459bfbef9e58102ae51be798694802d4b76fcbdf35ea6f1ecd3c53d91696a1a7`；已在干净 `89bc0b64` 工作树执行 `git apply --check` 和实际重放并核对源码摘要。用户已授权把源码候选和审计封印提交、推送到任务书指定分支；未合并 main、发布或迁移真实用户数据。

---

以下为恢复说明或历史交付背景；当前结果以上方 S11 和 round2 记录为准。

> applicableSource: `89bc0b64bf0a9b84ef3532efaa66c23213affb70 worktree`
> replacedBy: `round2`
> 状态：旧交付记录已被 round2 取代；下方历史内容不构成当前验收。

聊天工具栏上没有增加语音输入的组件是用户要求的。旧 F7 前端听写、D01–D12、X04 状态为 **SUPERSEDED_BY_PRODUCT_CHANGE**，不称当前通过，不恢复旧前端听写流程。

明确保留：后端 system-speech 模型解析、上传转写、错误码、原生音频附件、当前快捷键、TTS/朗读、宿主授权桥。当前快捷键按现行产品行为保留，不以旧听写快捷键合同复活废弃流程。

现行入口：[round2 修复报告](round2/R01_R10_REPAIR_REPORT.md)、[逐项测试矩阵](round2/R01_R10_TEST_MATRIX.json)、[真实执行记录](round2/COMMAND_RESULTS.json)、[阶段断点](round2/PROGRESS.md)。不在旧交付文件推定最终 S11 结果、补丁 hash 或当前 manifest。

旧日志 `logs/full-vitest-final.txt`、`logs/swift-core-tests.txt`、`logs/electron-node-bridge-load.txt`、`logs/helper-failclosed-smoke.txt` 当前均 **UNAVAILABLE**。旧测试数字、旧 PASS、旧本机工件或签名声明仅为历史文档原述，不能冒充 round2 当前通过。真实系统权限/识别、付费供应商凭证、目标跨平台验收仍按适用条件 **BLOCKED**；本机合成自动验证不等于真实权限或识别验收。真实用户恢复 apply 仍受 `BLOCKED_HOME_OWNERSHIP` 与单独数据授权约束。

<details>
<summary>历史正文：仅供追溯，不是当前功能、操作要求或验收结论</summary>

# F1–F12 测试报告

日期：2026-09-07 · 平台：darwin 25.6.0 arm64 · Node 24.16.0 · vitest 4.1.10
基线：fc4a6df7（clean 工作区起步，全部改动为本次未提交变更）

## 1. 定向测试（按 F 分组，先红后绿；红状态记录在各阶段证据文件）

| 组 | 测试文件 | ID 范围 | 结果 |
|---|---|---|---|
| F9/F3 | tests/pinned-tenets-migration.test.ts、tests/pinned-tenets-migration-recovery.test.ts、tests/tenets-source-quota.test.ts、tests/memory-tenets.test.ts | M01–M20 | 全绿 |
| F1/F2 | desktop/src/react/__tests__/components/composer-send.transaction.test.ts、desktop/src/react/__tests__/services/composer-send-coordinator.test.ts（含 P07/P08/P10、X 系列映射） | S01–S15 | 全绿 |
| F4–F6 | tests/system-speech-permission-bridge.test.ts、tests/system-speech-adapter.lifecycle.test.ts、tests/speech-helper-build-contract.test.ts、tests/speech-recognition-service.test.ts | A01–A16 | 全绿 |
| F7 | desktop/src/react/__tests__/components/InputArea.system-dictation.test.tsx（19 用例含 X04） | D01–D12 | SUPERSEDED_BY_PRODUCT_CHANGE（旧结果不适用） |
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

</details>
