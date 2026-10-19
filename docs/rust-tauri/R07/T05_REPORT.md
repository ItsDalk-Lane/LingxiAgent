# R07-T05：Office、PDF、美化和内置插件实施与验证记录

状态：**LOCAL_VERIFIED（候选 t05c，本机隔离环境）**。Office/美化 14 叶与 A09/A10 绑定同一候选通过；7 个媒体/即梦叶的运行时交付归 R07-T06（A11/A12），本轮不领取其通过结论。其它操作系统、真实付费供应商、真实账号与正式安装包未验。

## 范围

仅 R07-T05：Office 文档读取/HTML 导出 PDF、美化（封面生成/应用/预设/状态）、内置插件（office/beautify/media）工具目录与启停。与基线差异：无获准 ADR 之外的架构变更；PDF 渲染复用 T04 已验的受控 Chromium 适配层（ADR002 既定路线），未新引入渲染器。

## 源码

- 候选：`t05c`，[SOURCE_MANIFEST_t05c.json](../../../artifacts/rust-tauri/R07/T05/SOURCE_MANIFEST_t05c.json)，439 份冻结文件（以 T04/r7 为基叠加 T05 全部改动，不再排除 T05 自有源码）。
- 工作树源码指纹：`316fb6da889c6e3605aeb778ba52958094d6f08989fae73628b20cd19a7b7676`。
- 完整源码：[source-t05c.tar.gz](../../../artifacts/rust-tauri/R07/T05/source-t05c.tar.gz)，SHA256 `478c9d33df66b4628951895e8d00179f46adeb130880d16ae1c337c4e4e7b7b8`。
- 基线提交：`0c86675d49442fe0e38259bd3097423fb340eecc`（与 R07 起点一致）；分支 `codex/rust-tauri-migration`；`--locked` 依赖锁未变。
- 被替代候选：t05a（首轮冻结，service 组 1 项时序敏感失败，见失败历史）、t05b（补 3 叶覆盖缺口新增用例后重冻）；两者的回执与日志全部保留，未改写。

## 环境

[LOCAL_ENVIRONMENT.json](../../../artifacts/rust-tauri/R07/T05/LOCAL_ENVIRONMENT.json)：macOS 27.0.1 arm64、Node v24.16.0、cargo 1.98.1、受控 Chromium 155.0.8059.40。全部运行在隔离 `LINGXI_HOME=/private/tmp/lingxi-r07-tests/home`；外部模型/媒体供应商为本地回环确定性替身（SSE 文本 + b64 图片），无真实账号、付费请求或外发。

## 完成项（T05 未闭项逐条收口）

1. **组合根真实编译与接线**：OfficeApi/8 个内置工具（office×3、beautify×5）经真实 ToolInvocationGateway 注册（真实 Run、审批、资源授权、租户身份）；HTTP 路由合并进正式服务；`beautify_generation.rs` 孤儿模块接入组合根（`lib.rs:2610`）。
2. **受控大文件读写调用点迁移**：PDF 产物（50MiB 档）与封面上传走 `create_artifact`/`replace_artifact` 窄产物入口；`sessionfiles.rs` 新增 `register_artifact_blob`——对齐现役 stageFile（plugin-context.ts:121-126）语义：插件/Agent 产物登记不走用户上传 MIME 白名单，调用方先核验字节（%PDF 魔数、%%EOF 尾、sha256、上界）。
3. **ResourceIO 无副作用写入预验接线**：后台封面链在**付费前**以真实 OpContext + typed target + 当前版本完成 `ArtifactWritePreflight` 并持久化冻结事实（`admitted_write`）；应用时 `from_trusted_persistent_facts` 重建预检并 `require_snapshot` 复核——媒体生成期间文档被改写则响亮拒绝，不静默采纳新版本（专项漂移测试证明）。
4. **正式后台 Media Run 完成**：受理（幂等 requestId，重复提交只读原收据）→ 专用 CoverRun（RunOrigin::Beautify 谱系）→ 提示词模型调用（真实 SSE 替身，用量台账恰好一行终态）→ 媒体生成经网关（来源权威校验承认 Beautify 谱系：run/会话/主体逐字一致且冻结写事实已绑定）→ 应用 → 完成收据；提交成功不冒充产物完成（activity.status 区分 running/completed/unknown）。
5. **PDF 渲染收口**：`r07_pdf_chromium` 7/7——真实 Chromium 打印，loopback 金丝雀 TCP 零违例；资产白名单（私网/外链渲染前拒绝）、三个打包字体、分页、失败清理；`office_pdf.rs` 修复协议相对引用（`//host/x`）被 POSIX 双斜杠语义当作绝对路径而绕过外链拒绝的真实缺陷（判定顺序前移）。
6. **公共 UI 路由接线**：`use-hana-fetch.ts` 的 serviceRoute 增加 `/api/desk/beautify/*` 重写（status/cover/apply/preset-apply 四条），lingxi-service 节点命中已迁移面；活动事件沿用既有 ws/app-event 通道。五语言 locale 复用现役键，未新增文案。
7. **Rust 集成门禁**：`r07_office_tools` 14 项（文档 worker 真实读 docx/xlsx/分页 PDF、封面审批与 frontmatter 保留、只读/启停撤销拦截、PDF 资产收集与私网拒绝、HTTP 应用幂等与自报身份拒绝、预设图库真实字节、后台封面两例、远端客户端路径提权拒绝、能力目录/风格指南/候选应用直调）。

**真实缺陷修复（均由测试先暴露，非静态推断）**：
- 封面提示词对同一模型调用双写用量台账（started + completed 冲突）——删除提前写入，保持"每次调用恰好一行终态"不变式；
- 媒体来源权威缺 `RunOrigin::Beautify` 分支，封面链媒体生成被拒（MEDIA_NOT_DISPATCHED）——按持久化封面作业本体补齐正式授权判定；
- `application/pdf` 产物登记被用户上传白名单误拒——新增产物专用登记入口，不放宽上传白名单；
- 会话库迁移链推进到 v21（r07_media_jobs）/v22（r07_beautify_jobs），`session_file_store` 测试旧预期 20 与同文件 registry 断言（tip=22）自相矛盾，按真实顶端修正三处并保留 v7→tip 全链数据存活断言。

## 行为变化

用户可见：美化封面（生成/应用/预设/状态）与 Office 文档读取、HTML 导出 PDF 在 lingxi-service 节点上经 Rust 链交付；文件卡、审批交互、启停开关语义不变。不新增功能：所有面均为现役能力的迁移接线，UI 文案零新增。

## 验收

### A09｜Office 全链交付正确

- 命令：`cargo test -p lingxi-service --test r07_office_tools`（候选回执 [t05c-office](../../../artifacts/rust-tauri/R07/T05/t05c-office.json)，[日志](../../../artifacts/rust-tauri/R07/T05/t05c-office.log)）；`cargo test -p lingxi-adapters --test r07_pdf_chromium -- --include-ignored`（[t05c-pdf-chromium](../../../artifacts/rust-tauri/R07/T05/t05c-pdf-chromium.json)）。
- 预期：样本文档经真实工具链读取；HTML→PDF 经真实受控 Chromium，产物内容/字体/分页达标，登记为会话文件且字节可读回（文件卡链路）。
- 实际：docx/xlsx/分页 PDF 真实读取且输入零改动；PDF 收集已授权资产与三个打包字体并登记真实文件（字节核对一致）；私网资产在任何外部渲染调用前拒绝；取消等待真实隔离页清理确认且零文件登记；Chromium 组 7/7（含 loopback 金丝雀零违例）。退出码均为 0，前后源码指纹不变。

### A10｜恶意 zip 不越权写

- 命令：`cargo test -p lingxi-service --lib office_tools` 等四过滤器（[t05c-unit-gates](../../../artifacts/rust-tauri/R07/T05/t05c-unit-gates.json)）；`r06_agent_settings_live` 角色卡用例 + `r06_original_memory_skill_proofs` 技能包用例（[t05c-a10-imports](../../../artifacts/rust-tauri/R07/T05/t05c-a10-imports.json)，[日志](../../../artifacts/rust-tauri/R07/T05/t05c-a10-imports.log)）。
- 预期：含 `../`、反斜杠、符号链接、重复名、超预算成员的归档全部拒绝；外目录哨兵不变。
- 实际：文档包只读校验（`validate_package`，结构性不展开为宿主文件）对五类危险成员响亮拒绝——符号链接用例以 `external_attributes(0o120777 << 16)` 构造真实链接位（zip crate 的 `unix_permissions` 会掩掉文件类型位，首轮构造无效被测试如实暴露后修正）；角色卡真实 zip 逃逸/链接/膨胀拒绝（落盘前 `inside()` 收容 + unix_mode 链接位双重拦截）；技能包符号链接逃逸拒绝且不创建目录。退出码 0。

## 安全与数据

- 权限负向：只读模式与 live 启停撤销均阻止封面写；远端客户端不能以绝对路径换取 local-owner 权限（403）；自报身份字段被拒绝；封面批准绑定 nonce/真实调用/供应商范围/付费前版本。
- 真实进程：PDF 渲染走真实受控 Chromium 子进程与真实隔离页清理；取消需等待外部清理确认。
- 单写者/迁移：v21/v22 为纯追加迁移（新表+索引），`supported_version`=22 与 registry 断言一致；旧会话数据存活断言保留。
- 密钥：媒体/文本供应商凭据为隔离环境伪值（sk-TEXT-A-SECRET 等），写入隔离 home 的凭据库；无任何真实密钥进入代码或报告。

## 完整映射

F-ID→叶→测试的逐条登记见 [FEATURE_PARITY_RESULTS.json](../FEATURE_PARITY_RESULTS.json)（14 叶 PASS_LOCAL_T05 + A09/A10）。未覆盖集合（7 叶，NOT_RUN）：media get-guide / describe-options / generate-image / generate-video / generate-speech 与 jimeng-cli image/video——其 job 运行时（提交/轮询/取消/下载/交付）属 T06；本轮已证其目录注册、启停与封面链内部 generate-image 真实网关调用，不提前领取运行时通过结论。

## 已知缺陷

1. **`r07_t05_media` 目标 9/9 失败（预期内，T06 所有）**：媒体运行时批准链未实现，全部在"外部生成须先进入正式批准"等待超时。回执 [dev-media-target-r1](../../../artifacts/rust-tauri/R07/T05/dev-media-target-r1.json) 保留；该目标从未在任何候选中通过，不属回归。
2. **`r07_phone_runtime::actual_proactive_31_minutes_obeys_disable_enable_and_restart_slot_receipt` 时序敏感**：t05a-service 失败一次（12 秒投递结清窗口超时，lastError 显示禁用时序交错），同源码在 service-wide-r1、t05b-service、t05c-service 三次通过。原始失败回执 [t05a-service](../../../artifacts/rust-tauri/R07/T05/t05a-service.json) 保留；未改测试、未删证据，记为已知不稳定（疑似高负载下结清窗口不足），后续阶段若复现需根查。
3. PDF 扫描件 OCR、PPTX 读取、Office 编辑为现役明确不支持项（能力目录如实声明），不记为缺陷也不记为已实现。

## 未执行/BLOCKED

- 真实付费供应商往返、真实 jimeng/dreamina CLI：T06 范围，未授权真实外发；
- 非 Unix 系统、其它 OS 打包、正式安装包：未验，不阻止本阶段本机放行；
- R07 最终统一回归与独立最终核验：T12 范围。

## 回退

v21/v22 追加迁移不改动旧表；回退到旧版本二进制将按既定篡改防护拒绝高 user_version 数据库（响亮失败而非静默降级）。本轮未演练真实回退（隔离环境逐次新建 home）；T05 产物均为新表与新文件，无历史数据改写。

## 独立审查

本轮未执行独立审查（无第二名审阅者，不虚构）。审阅留待 R07 最终候选的统一独立核验；本报告全部结论以候选绑定回执（命令/前后指纹/exit/logSha256）为据，可由任何审阅者按 SOURCE_MANIFEST_t05c.json 重算复核。

## 下一阶段

允许开始 T06（媒体生成、语音与视觉桥）：输入为 t05c 候选证据与本报告未覆盖集合；T06 不得引用 T05 回执冒充其 38 叶或 A11/A12 的通过结论。不允许开始：任何远程写、发布、真实供应商调用。

## 远程/发布

未获准、未执行：无 commit/push/PR/tag/发布，无真实外发接口调用，无真实数据迁移。
