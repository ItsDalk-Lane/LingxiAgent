# P9 证据 — 组合回归、构建与验收

日期：2026-09-07（命令时间戳见下）· 分支：refactor/dismantle-and-voice-features（工作区未提交）
平台：macOS 25.6.0 arm64（darwin）· Node 24.16.0 · 工作树身份 = 本仓当前未提交变更（git status 见 P10 报告）

## P9.1 X01–X16 组合回归映射（每条记录对应 F 编号与测试位置）

| ID | F | 证据（测试文件/用例） | 结果 |
|---|---|---|---|
| X01 | F2 | composer-send-coordinator.test.ts + composer-send.transaction.test.ts S01/S02（串行租约、队首延迟不抢先、独立回合/用量由 usageLedger 断言） | 通过 |
| X02 | F2 | S01（预检失败编辑器/草稿/附件全保留）+ S06（准备期间新输入独立不被覆盖）；队列不跳过由 coordinator awaiting_ack 语义保证 | 通过 |
| X03 | F1/F2 | S07（文字+文件+skill+agent+quote+notebook 组合，失败可重试保留全字段）+ InputArea.pending-identity.test.tsx（新会话一次创建/来源快照） | 通过 |
| X04 | F7+F10+F2 | **新增** InputArea.system-dictation.test.tsx『X04』（听写插入→粘贴代码→流式中发送入队→回合结束按序发出，正文逐字符一致） | 通过 |
| X05 | F6/F7 | **新增** tests/x05-speech-concurrency.test.ts（sleep 假 helper 在途识别 400ms，另一会话 mood/text 事件链 <400ms 完成投递，事件循环不阻塞） | 通过 |
| X06 | F9/F3 | pinned-tenets-migration.test.ts（M01–M20，含新会话创建与批准流程）+ tenets-source-quota.test.ts（双配额不串扰） | 通过 |
| X07 | F9 | pinned-tenets-migration.test.ts M10 三处崩溃注入恢复（completed 之前各阶段）+ 收据不驱动重导入断言 | 通过 |
| X08 | F8 | reserved-tag-text-preservation.test.ts T13（实时/历史同源一致：未知配对保留+孤儿清理+代码保护）+ live-history-reserved-tag-parity.test.ts（2 场景） | 通过 |
| X09 | F10 | composer-send.transaction.test.ts P10（排队→编辑→失败→重试→wire 不 trim）+ P02/P08（首尾空白/全空白+附件） | 通过 |
| X10 | F11/F12 | speech-response-lifecycle.test.ts V07（response 完成终态）+ V08（无会话副作用 spy）+ V12（重启收尾、无供应商请求、无用量新增） | 通过 |
| X11 | （不变项） | InputArea.media-send.test.ts（语音附件原生路径/视觉辅助/视频预检/旧文本路径 8 用例） | 通过 |
| X12 | （不变项） | knowledge 套件 61 个测试文件（rollup/fast-detailed/多笔记本冻结引用等），全量绿 | 通过 |
| X13 | （不变项） | tool-invocation-boundary/errors、builtin 工具、plugin-* 契约套件（v0.1.34 契约执行层），全量绿 | 通过 |
| X14 | F9 | pinned-tenets/tenets-source-quota 套件（配额与来源）；已删除功能未复活：插件市场/邀请码无入口代码，plugin-manager.test.ts 等现行套件不含复活路径（缺席以现行套件+源码结构佐证） | 通过 |
| X15 | F2 | S12（失败重试同一逻辑记录）+ coordinator awaiting_ack 串行；流式点击发送默认入队（InputArea 既有行为，X04 亦覆盖流式中发送路径） | 通过 |
| X16 | F2 | S10（断线无回执/回执超时→delivery_unknown 不自动重发）+ coordinator 回执对账用例 | 通过 |

## P9.2 命令执行记录（exit code / 规模 / 时间戳）

| 命令 | exit | 结果 | 备注 |
|---|---|---|---|
| `npm run typecheck`（tsc×3） | 0 | 通过 | 2026-09-07 22:1x，最后一次在 X04 类型修正后复跑通过 |
| `npm run lint`（eslint .） | 0 | 0 errors / 9096 warnings | warnings 为全仓既有基线（no-explicit-any 等风格项），本任务未新增 error |
| `npm test`（vitest 全量） | 0 | **13243 passed / 0 failed / 7 skipped**（13250 总量） | 含本任务新增 D/T/P/V/X 全部用例；7 skipped 为既有平台/环境跳过项 |
| `npm run build:client` | 0 | vite 多入口构建成功 | |
| `npm run build:server` | 0* | 成功 | *需签名密钥：用 scripts/artifact-keygen.mjs 生成的一次性密钥（/tmp，不入库）+ LINGXI_SIGN_KEYSET 覆盖文件完成本地验证构建；seed 启动冒烟通过、12 Mach-O ad-hoc 签名 |
| `npm run build:speech-helper` | 0 | Swift 构建成功 | P5 内已跑（on-device 要求改造后重编译） |
| `npm run pack`（干净 dist-speech） | 见下 | **构建/打包/重签名通过；公证步骤 BLOCKED** | 详见下节 |

### 干净 pack（无预生成 dist-speech）

1. `mv dist-speech /tmp/...backup` 后运行 pack：pack 链自身执行 build:speech-helper +
   build:speech-permissions，从零重建 `dist-speech/mac-arm64/{lingxi-speech-helper,
   lingxi-speech-permissions.node}` —— 构建入口接线正确，未用预生成产物掩盖。
2. 第二轮（rm -rf dist-speech + 一次性签名密钥）：electron-builder 完成打包，
   `resign-adhoc` 输出 `✓ signed and verified`（含新增的
   `Contents/Resources/speech/macos/lingxi-speech-permissions.node` 1c 签名块）。
3. 最终停在 Apple 公证步骤：`Set APPLE_APP_SPECIFIC_PASSWORD ...` —— 需要发布用
   Apple 凭证，未授权不可用，**BLOCKED（凭证）**；公证前的全部本地结构验证通过：
   - `dist/mac-arm64/Lingxi.app/Contents/Resources/speech/macos/` 含两个 Mach-O arm64 产物；
   - .node 具 ad-hoc 签名标识（codesign -dv 输出 Identifier）。
4. 未删除用户任何产物；dist-speech 由 pack 自身重建，备份留在 /tmp。

### Swift / 原生

| 命令 | exit | 结果 |
|---|---|---|
| `swift run --package-path desktop/native/LingxiSpeechHelper lingxi-speech-core-tests` | 0 | **16/16 coordinator checks passed**（Package.swift 的实际测试 target 是可执行 runner；两次复核通过。备注：后台任务管道下曾出现一次 swift-run 挂起，直接运行二进制与文件输出复跑均即过——管道交互假象，非代码缺陷） |
| `swift test --package-path ...` | 1 | `no tests found`——本机 CLT 工具链无 XCTest/swift-testing，Package.swift 无 XCTest target（P4 已确认并改用可执行 runner，如实记录非绿） |
| Electron 运行时加载 .node（`ELECTRON_RUN_AS_NODE=1 npx electron -e require(...)`） | 0 | exports 可枚举（dispose/getAuthorizationStatus/requestAuthorization）、raw status=0、dispose 正常——**P4 的 A14「Electron 加载」BLOCKED 项就此补验通过** |

### 门禁（P9.5 派生清单核对）

| 门禁 | 结果 |
|---|---|
| tests/cli-closure-census.test.ts | 22/22（本任务无新增动态调用；speech 媒体适配器不在 CLI 闭包） |
| tests/persistence-schema-tripwire.test.ts | 15/15（无 schema 形状变化，无需 repin） |
| tests/post-verification-audit-seal.test.ts | 3/3（VERIFIED_SOURCE_SHA 未推进；封印等待提交授权） |

## P9.3 失败归类

- 全量 vitest 在 P6/P7/P8 阶段共出现 3 次 `pinned-tenets-migration M10` 满载单例失败
  （不同子用例轮换）；每次单跑 25/25 通过，且本任务未触碰迁移模块。按 P9.3 规则
  记录为「疑似既有 flaky（满载时序）」，**最终全量状态以最后一次全绿运行为准
  （13243/0）**，未据此改写任何历史轮次记录。
- `swift test` exit 1（无 XCTest target）为工具链既有限制，非本任务引入；等价验证
  经由 runner target 完成。
- pack 的公证步骤失败为凭证缺失（BLOCKED），exit 链如实记录，未标绿。

## P9.4 真实环境门槛 — BLOCKED 清单（如实，不冒充通过）

| 项 | 状态 |
|---|---|
| macOS arm64 真机：首次授权弹窗→授权后识别→拒绝→恢复 全流程 | BLOCKED（需真机 + 明确授权的验收环境；任务书限定真实权限弹窗只在授权环境执行） |
| macOS x64 交叉构建/运行 | BLOCKED（无 x64 设备；本机仅完成 arm64 构建，交叉构建未执行） |
| 设备内识别断网验收（特定语言） | BLOCKED（同真机限制） |
| 云端 TTS 真实供应商一次短文本（可播放性/声线/用量） | BLOCKED（未提供凭证；未寻找/复用其他个人凭证） |
| Windows/Linux 非 macOS 初始化与文本/云语音回归 | BLOCKED（无对应平台设备） |
| Electron 宿主内 .node 加载 + 授权状态机流转 | 加载已补验通过（上表）；带 TCC 弹窗的授权流转仍 BLOCKED（真机） |

## P9.5 派生清单与封印

- 本任务改动的边界核查：CLI closure（无变化需求）、persistence fingerprint（无 schema
  变化）、audit seal（未推进，候选源码未提交——按任务书保持不动，报告「封印待授权」）。
- 无新增迁移收据/开放边界（export-manifest 未动）；新产物（.node/helper）经
  build 脚本与 CI 工作流接线，不留未登记的开放面。
- 构建产物全部落在既有忽略目录（dist*/release/desktop dist）；源码、构建规则
  （package.json scripts、CI workflow、resign-adhoc）、测试进入交付差异。
