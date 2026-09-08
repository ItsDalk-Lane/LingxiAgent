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

## 3. 前端接线（F7）

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
