## S11 最终交付状态

R01–R10 本地实现/联合验证关闭；整体合并/发布验收未满足。阻塞为基线审计封印、notarization 凭证、真机/供应商/跨平台验收。

最终源码摘要：`a342614cece3eba2c27e88561a71653e450114700d87c59e798808b25a8e9bf6`。

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

最终增量补丁为 `patches/89bc0b64-to-r01-r10-source.patch`（相对 round2），大小 `415916` bytes，SHA-256 `24e843e03df335ad9ae945bbd0d3d9f36b92daf6891dba8713f61885809c2637`；已在干净 `89bc0b64` 工作树执行 `git apply --check` 和实际重放并核对源码摘要。此前 S10 数值仅为阶段历史，不作为最终补丁身份。无提交、推送、合并、发布或真实用户数据迁移。

---

历史阶段记录（保留原始先后状态，不覆盖上述最终结论）：

# R01–R10 二轮修复报告（执行中）

适用基线：89bc0b64bf0a9b84ef3532efaa66c23213affb70；源码身份为隔离未提交 worktree，逐文件摘要及每次命令见本目录 JSON。最终状态以本文开头 S11 结论为准；后续阶段记录为历史证据。

| 问题 | implementation | automatedVerification | realEnvironmentVerification | closure |
|---|---|---|---|---|
| R01 | IMPLEMENTED | VERIFIED（合成文件系统） | BLOCKED（home 所有权前提未验证） | BLOCKED |
| R02 | IMPLEMENTED | VERIFIED（合成文件系统与 CLI 拒绝） | BLOCKED（无可复用排他所有权） | BLOCKED |
| R03 | IMPLEMENTED（S3 阶段） | VERIFIED（真实模块/组件阶段验证） | OPEN（不以合成验证冒充真实环境） | LOCAL_CLOSED（整体验收仍 BLOCKED） |
| R04 | IMPLEMENTED（S4 阶段） | VERIFIED（真实模块/组件与 SDK 历史路由） | OPEN（本地受控边界，不代替真实环境） | LOCAL_CLOSED（整体验收仍 BLOCKED） |
| R05 | IMPLEMENTED（S5 阶段） | VERIFIED（真实组件 DOM/复制与历史、scanner 回归） | OPEN（不代替真实桌面及跨平台验收） | LOCAL_CLOSED（整体验收仍 BLOCKED） |
| R06 | IMPLEMENTED（S6 阶段） | VERIFIED（真实 TipTap、serializer 与发送/历史回归） | OPEN（不代替真实桌面及跨平台验收） | LOCAL_CLOSED（整体验收仍 BLOCKED） |
| R07 | IMPLEMENTED（S7 阶段） | VERIFIED（decoder 与真实合成 helper 生命周期/并发） | BLOCKED（原生识别、真实权限及跨平台未验收） | LOCAL_CLOSED（整体验收仍 BLOCKED） |
| R08 | IMPLEMENTED（S8 阶段） | VERIFIED（参数解析与 manager/adapter 受控边界） | OPEN（付费供应商、真实 say 与跨平台未验收） | LOCAL_CLOSED（整体验收仍 BLOCKED） |
| R09 | IMPLEMENTED（S9 阶段） | VERIFIED（真实 TaskStore 与合成文件系统边界） | OPEN（真实权限、跨平台与供应商未验收） | LOCAL_CLOSED（整体验收仍 BLOCKED） |
| R10 | IMPLEMENTED（S10 阶段） | VERIFIED（证据契约及干净基线补丁重放） | BLOCKED（对应真实环境限制仍保留） | LOCAL_CLOSED（整体验收仍 BLOCKED） |

S1/S2 联合阶段出口：s2-exit，10 文件 142 passed / 0 failed / 0 skipped；源码摘要 6f4dbbdc0a6fcbeabd6b52b1bd4a70103e92eb8af8fd169228b1e0c5b59bb7ef。此结果仅证明该时刻源码，后续变更必须重跑受影响检查。

R01：严格 v3 收据（兼容 v2）、全源映射与 active/内容/新增来源/创建时间证明；原始源/目标/旧收据独占备份；归档先规划，COPYFILE_EXCL 后回读再删除原名，支持归档后收据前 SIGKILL 恢复。旧 completed 不重写、不复活。

R02：active 优先去重；批准绑定操作、源摘要与目标快照，逐来源条目映射；独立 recovery operation 收据、同单返回原摘要、完成后不补回删除内容。共享 pins 专用事务执行器。CLI apply 明确 BLOCKED_HOME_OWNERSHIP；合成直接模块测试不代替真实 home 排他证明。

R03（仅 S3 阶段）：队列意图读取最新依赖并按 scope/owner 撤销；开始编辑同步使旧准备租约失效，保存改变正文才增加版本并换逻辑身份，取消保留原失败状态。自动 timer 限当前聊天会话，已取得的 A 租约切页后可完成且不清 B 草稿；立即插入仍受编辑、租约、run 与附件门禁限制。五语种不可编辑提示明确要求先核对发送结果。

阶段验证：`logs/s3-final-delivery.log` 13 文件 120 passed，exit 0；`logs/s3-typecheck-delivery.log` 三段 tsc 通过。受测源码摘要 `e6d582c1d99f9eb8bc67d57ae6f78e83356c96cfdf6c3f56c96735f8492bfe10`。矩阵逐项绑定 R03-01–R03-12 的真实测试标题。S4 正在执行；S3/S4 联合验收及未知投递对账尚不能由这些结果推定通过，R03 closure 保持 OPEN。

无提交、推送、合并、发布、真实用户数据迁移、真实权限或付费调用。未恢复聊天工具栏语音输入，不删除保留后端音频能力。

完整合并或发布验收尚未满足：后续阶段未完成，真实平台及 home 所有权仍有阻塞。S10 已生成精确补丁并完成干净基线重放；S11 全量门禁与最终交付清单仍待完成。

R04（S4 冻结出口）：未知输入保留独立屏障与完整快照；canonical ACK 只证明接收，运行空闲须有权威证据。捕获服务器与会话身份后执行有界分页单飞对账，拒绝陈旧响应及跨分支/版本错误关联；历史和 ACK 合并同一气泡并保留原文附件，失败显示核对诊断且不盲重发。

阶段验证：`logs/s4-exit.log` 7 文件 96 passed / 0 failed / 0 skipped，`logs/s4-exit-typecheck.log` 三段 tsc 通过，均 exit 0；manifest `96f1f8edc3693edafeaddfb31f64aabb8b7555a8827739f5474165af3f7606c4`。R04-01–R04-18 逐项真实标题与限制见矩阵。S5 执行中；R01–R03 上述证据状态保留。该出口不代表全任务闭环、最终重放/全量门禁、真实服务器断网或跨平台验收。

R05（S5 阶段）：StreamingMarkdownContent 直接消费 canonical source，移除 stripTagEscapes，避免显示层再次删除反斜杠；scanner 既有结构化协议与孤儿闭标签边界不改。真实组件覆盖代码、正文、重复渲染、delta 切分、终态和复制，配套历史与 scanner 回归保留。超长文本按 canonical source 转义降级；旧 HTML 缺 source 时不反推或声称恢复既往丢字。

验证：`logs/s5-red.log` 5 failed / 49 passed；`logs/s5-exit-verified.log` 6 文件 105 passed / 0 failed / 0 skipped，exit 0；`logs/s5-typecheck.log` 三段 tsc 通过，exit 0。最终两项 manifest 为 `45132e6c7746daf8d90a7b9b13f4f3140927c35f32460398891ee6116c35bea4`。R05-01–R05-10 的真实标题及 RED/回归区别见矩阵。S6 执行中，真实桌面剪贴板、跨平台及全任务最终门禁不由本地自动验证代替，报告仍非最终完成交付。

R06（S6 阶段）：关闭 StarterKit 默认 trailingNode，删除 serializer 无条件尾空段循环；首中尾空段、hardBreak、tab 和全角空格按用户内容保留，旧无标记草稿 hydration 不猜测删除。R06-05/06 未新增 synthetic 标记或解除机制，因为无来源自动补尾已关闭；真实编辑器覆盖用户创建及编辑尾空段。链接与 badge 身份提取、粘贴、判空、失败重试及 wire/历史边界均回归。

验证：`logs/s6-red.log` 7 failed / 1 passed；`logs/s6-exit.log` 10 文件 77 passed / 0 failed / 0 skipped，exit 0；`logs/s6-typecheck.log` 三段 tsc 通过，exit 0。最终两项 manifest：`1105df54e4123f4bf0131e0e110467a2b214eb2c6eacc6f25ef8e3a793eaec54`。矩阵标明 R06-01–R06-12 的真实标题及条件适用性。S7 执行中，真实桌面剪贴板、跨平台及全任务最终门禁仍未闭环。

R07（S7 阶段）：stdout/stderr 使用各自独立的 StringDecoder，按原始字节计数并在 close 时 flush，避免 UTF-8 跨块破坏中文、emoji 或组合字符。成功输出的 text 必须是字符串，否则明确 INVALID_OUTPUT；合法 U+FFFD 保留。既有输出上限、一次结算、取消/超时及事件循环推进规则保留。

验证：`logs/s7-red.log` 4 failed / 8 passed；`logs/s7-exit.log` 7 文件 39 passed / 0 failed / 0 skipped，exit 0；`logs/s7-typecheck.log` 三段 tsc 通过，exit 0。最终两项 manifest：`2bb59c4adc6950d2e4490b991737e4e3779bf350890555e004c9472052edf7b2`。R07-01–R07-10 绑定 decoder、真实异步合成 helper、既有超时/大输出和 X05 并发测试。S8 执行中，合成 helper 证据不代表原生识别、真实权限、付费调用或跨平台验收；保留后端 ASR/TTS 与原生音频能力，不恢复聊天工具栏语音输入或旧前端听写流程。

R08（S8 阶段）：统一协议参数解析，使实际 wire 参数、TaskStore 和语义观测一致；按协议处理速度限幅、格式缺省及错误类型，DashScope 不记录未发送参数，system say 记录实际整数 WPM 且不伪造默认 voice。保留 speech 域、逻辑 provider 与凭证 lane 的归属及既定模型，不增加供应商请求。

验证：`logs/s8-red.log` 7 failed / 6 passed；`logs/s8-first-green.log` 13/13 为非最终中间证据；`logs/s8-exit.log` 6 文件 73 passed / 0 failed / 0 skipped，exit 0；`logs/s8-typecheck.log` 三段 tsc 通过，exit 0。最终两项 manifest：`e1e345ce172f14510b3537a188e11d230a5fe3c5c4d87bcc72fda3ac343fa05e`。R08-01–R08-10 的对应真实标题见矩阵；S9 执行中，真实付费供应商、系统 say 与跨平台验收不由本地受控边界结果代替，全任务仍未闭环。

R09（S9 阶段 PASS）：新完成与重启恢复复用同一产物校验：realpath 解析 generated 根和目标的符号链接，以 path.relative 校验真实目录包含关系；stat 跟随目标并确认根是目录、产物是普通文件。lstat 只检查链接条目本身，不能代替真实目标和普通文件校验，故不能用于一律拒绝合法根内链接。保留 files[] 文件名合同，多文件全部通过才完成，session 投递与 image/video pending 回归保留。此验证是检查时刻的路径与文件类型判断，不提供打开文件描述符后的身份锁定，不能声称防住检查后并发替换的 TOCTOU；真实权限 EACCES、跨平台及真实供应商验收未由本地合成文件系统结果代替。

S9 RED：`logs/s9-red.log` 为 5 failed / 1 passed；`logs/s9-first-green.log` 为 6/6，通过后另增 generated 根为合法符号链接的子例，不能把首轮绿灯称为最终证据。最终出口 `logs/s9-exit-verified.log` 为 5 文件 / 92 passed；`logs/s9-typecheck-verified.log` 三段 tsc PASS。最终 manifest：`c82aaf2ff0f7cd8a4c87c1c9c2090390a66d61b9939275948f753984f77cb3ca`。保留两次真实类型检查失败：`logs/s9-typecheck.log` 的 TaskStore 联合类型 error 字段收窄错误，以及 `logs/s9-typecheck-final.log` 的测试返回值 error 字段类型错误；均不能改记 PASS。

R09-01–R09-10 的真实标题映射见矩阵，R09-03 包含后来增加的根目录链接子例；R09-11 映射 lifecycle V09 的 session 单次投递，R09-12 映射 V13 的 image/video pending 回归。当前 S10 active，全任务仍未闭环。

S10 环境边界：真实 macOS 真机授权流程 **BLOCKED**；真实 OpenAI/MiniMax/DashScope 凭证与付费调用 **BLOCKED**；Windows、Linux、macOS x64 及其他架构的跨平台验收 **BLOCKED**。旧一轮报告所引但未交付的历史日志为 **UNAVAILABLE**，本轮不会用重跑结果倒推旧日志存在。

R10（S10 阶段）：旧交付文件明确当前适用来源、round2 替代关系与产品变更；证据契约核对真实日志、源码摘要、tracked/untracked 源文件、矩阵标题、历史缺失记录及真实环境限制。

S10 RED `logs/s10-red.log`：4 failed / 4 passed，exit 1；中间 `logs/s10-first-green.log`：8/8，exit 0；`logs/s10-exit.log`：1 failed / 8 passed，exit 1，保留真实失败；最终 `logs/s10-exit-verified.log`：9/9，exit 0。最终 manifest：`a9b34bd5e9fa4fa988cb995ba68422a127447a152eefb6f8b16fb9615a32d796`。

增量补丁：[89bc0b64-to-r01-r10-source.patch](patches/89bc0b64-to-r01-r10-source.patch)，404617 bytes，SHA-256 `116976d9e83c03ca216b413af849d6831998b53d330ebde4de13c8fdc04d6c14`。R10-09 已从干净 `89bc0b64` 重放，源码摘要与受测工作树相等。

R10-01–R10-10 的真实测试均来自 tests/round2-delivery-evidence.test.ts；R10-07/08 共用标题。S10 通过、S11 active，不宣称未执行的 S11 结果或全任务最终完成。
