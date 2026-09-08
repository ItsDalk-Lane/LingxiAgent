applicableSource: `89bc0b64bf0a9b84ef3532efaa66c23213affb70 worktree`

replacedBy: `round2`

## S11 最终交付状态

R01–R10 本地实现/联合验证关闭；受测源码候选提交后的审计封印状态以根目录 `PROGRESS.md` 和 Git guard 为准。整体发布验收仍受 notarization 凭证、真机/供应商/跨平台验收阻塞。

最终源码摘要：`1f5514ea8ce975cac8b5223fb2e6c6367e25eae4b0276f3296607de285de05ff`。

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

补丁为 `patches/89bc0b64-to-r01-r10-source.patch`（相对 round2）；文档收尾后还会重生成，大小与 SHA-256 以主代理最终 readback 为准。此前 S10 数值仅为阶段历史，不作为最终补丁身份。无提交、推送、合并、发布或真实用户数据迁移。

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

# 系统听写（macOS Speech）验证说明

覆盖 F4（宿主授权）/F5（识别核心）/F6（Node 适配器）/F7（前端接线）。
如实区分：自动化（合成/mock）验证 / 本机真实工件验证 / BLOCKED 未验证。

## 1. 宿主权限链（F4）

- 实现：`desktop/speech-permissions.cjs`（六态状态机 + 单飞请求 + dispose 语义）经
  IPC（speech-permission-status 只读 / speech-permission-request 用户手势）暴露；
  ObjC++ Node-API 桥 `lingxi-speech-permissions.node`（TSFN 一次性回调）。
- **已验证（本机真实）**：.node 以 Electron 42.8.1 运行时加载成功
  （`ELECTRON_RUN_AS_NODE=1 npx electron`）：`exports=dispose,getAuthorizationStatus,
  requestAuthorization`、`rawStatus=0`（not_determined，与当前未授权事实一致）、
  dispose 正常。日志：logs/electron-node-bridge-load.txt。
- **BLOCKED（未验证，不冒充）**：真实 TCC 首次授权弹窗、授权后状态流转、
  用户拒绝与系统设置恢复流程——需明确授权的 macOS 真机验收环境。

## 2. helper（F5）与适配器（F6）

- **已验证（真实构建）**：Swift helper 以 Xcode 工具链真实编译（arm64 Mach-O），
  RecognitionCoordinator 16/16（超时/取消只结算一次、迟到一个不结算、task.cancel
  恰一次、竞态附加即取消）——logs/swift-core-tests.txt（Package 实际测试 target 为
  可执行 runner；`swift test` 因 CLT 无 XCTest 而 exit 1，如实记录）。
- **已验证（真实二进制行为）**：未授权环境下对合成 WAV 转写 → 结构化
  `{"ok":false,"code":"PERMISSION_REQUIRED","protocol":2}`、exit 2、无弹窗
  （logs/helper-failclosed-smoke.txt）——fail-closed 而非静默降级。
- **已验证（自动化/mock）**：Node 适配器生命周期 A06–A12（假 helper：SIGTERM→宽限→
  SIGKILL 有界终止、4MB stdout 上限、'close' 定界、取消/超时不记成功）；X05 证明
  识别在途 400ms 期间另一会话事件链照常投递（事件循环不阻塞）。
- **BLOCKED**：授权成功后的真实识别（文字正确性、耗时）、特定语言的设备内识别
  断网行为（helper 现始终要求 requiresOnDeviceRecognition，不支持时显式
  RECOGNIZER_UNAVAILABLE，不静默走云端——代码语义已定，真机表现待验）。

## 3. 前端接线（F7）：SUPERSEDED_BY_PRODUCT_CHANGE（以下为历史原述）

- **已验证（自动化）**：D01–D12 + X04（jsdom + 真实 ComposerToolbar + mock 后端桥/网络）：
  文本模型可听写、modeAtStart 快照、按钮/快捷键同门禁、类型化解包（无 [object Object]）、
  错误码区分文案、迟到结果不跨会话/连接写入、麦克风停录即释放、无桥显式不可用且
  云端音频路径不受损、听写+粘贴+排队组合。
- 服务端：零配置 system-speech 解析（真实 ProviderRegistry）、api-key 目录模型保持
  不可执行、错误码 `CODE: message` 前缀透出。
- **BLOCKED**：真机「首次授权→文本模型下录音→文字插入→用户发送」全流程（P5 退出条件
  的真实 App 部分——实现保留，不宣称已验收）。

## 4. 构建架构与签名（真实）

| 项 | 结果 |
|---|---|
| build:speech-helper（Swift，本机 arm64） | ✅ 真实构建通过 |
| build:speech-permissions（node-gyp，Electron 42.8.1 头文件，arm64） | ✅ 真实构建通过 |
| CI（.github/workflows/build.yml macOS job） | ✅ 接线完成（两构建步骤 + LINGXI_SPEECH_HELPER_ARCH 矩阵）；线上运行未验证 |
| pack（干净 dist-speech） | ✅ pack 自身从零重建两产物（无预生成掩盖）；app 包内 `Resources/speech/macos/` 含 helper + .node（均 arm64 Mach-O）；resign-adhoc 对 .node 的 1c 签名块执行且 `✓ signed and verified` |
| Apple 公证 | ⛔ BLOCKED（无发布凭证；本地以 scripts/artifact-keygen.mjs 一次性密钥完成验证性签名构建，密钥仅存 /tmp 不入库） |
| macOS x64 | ⛔ BLOCKED（无设备；未执行交叉构建） |

## 5. 真实转写与取消证据状态

- 真实音频→文字的成功转写：**无**（需授权真机）。
- 真实取消（识别中途停止）：自动化验证（A08/A10/X05 同链）；真机手势级取消未执行。
- 本文件不将任何 mock/合成结果表述为真机通过。

</details>
