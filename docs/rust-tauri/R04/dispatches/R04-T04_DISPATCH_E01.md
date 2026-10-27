# R04-T04 派单｜EXECUTOR-R04-T04-E01

派单时间：2026-09-30。派单人：R04 总控编排器。
TASK_BASE_SHA：c72e0fa02（R04-T03 已独立 PASS 并推送确认）。

---

你是一次性执行代理，只负责 R04-T04：原生读写改文件与资源权限。
基线：c72e0fa02；分支：codex/rust-tauri-migration；工作区：/Users/study_superior/Desktop/Code/LingxiAgent。

完整读取 R04 总控提示词、原阶段书、本 Task 全部 Steps/Deliverables、
R04-A07 与 R04-A08、T01/T02/T03 报告（前置）、R03 交接、真实源码与测试。
不得依赖前一个代理的聊天记忆。

必读文件：
- /Users/study_superior/Downloads/Lingxi_R04_自动执行总控提示词_2026-09-30.md（§4.1、§7.T04）
- 任务书 R04-T04 节 + 共同规范 01/02/05
- docs/rust-tauri/R04/R04-T01_REPORT.md、R04-T02_REPORT.md、R04-T03_REPORT.md、reviews/R04-T03_REVIEW_R1.md（OBS-1：别名解析腿建议本 Task 补真实腿；OBS-4 登记）
- docs/rust-tauri/R04/R04_SCOPE_MATRIX.json
- 源码：T01 toolcatalog.rs（注册/schema/EffectiveArguments）、T02 toolgateway.rs（prepare/execute_prepared/策略端口）、T03 approval_service.rs（资源范围在批准记录中的字段）、runs.rs 接线
- 现役语义参照：lib/sandbox/path-guard.ts、lib/sandbox/file-tool-guards.ts、lib/sandbox/file-freshness.ts、lib/sandbox/ast-edit-tool.ts、lib/sandbox/edit-error-hints.ts、现役 read/write/edit 工具实现（搜索 lib/tools/ 内真实实现与测试）——读取范围/分页/编码/二进制处理、write/edit 匹配与冲突语义按现役对照迁移

任务要点（原 Task 全文仍为准）：
1. 对照现役 schema 和行为，迁移 read 的读取范围/分页/编码/二进制处理与结果截断语义、write、edit 的匹配与冲突语义（不默认静默覆盖）。保持真实输入完整，执行器不得根据 args_summary 猜正文（T01 已建立完整参数契约——read/write/edit 经真实有效参数执行）。
2. 建立 ResourceRef/ResourceAccess：路径、工作区和主体授权连到同一资源规则；canonicalization 覆盖目标不存在时的受控父目录、相对路径、符号链接、Windows junction/盘符/UNC（本机 macOS 实测符号链接与相对路径，Windows 形态按 cfg/测试登记）。验证真实目标，不只检查字符串前缀。
3. 写入和替换采用平台可行的原子/可恢复方式（临时文件+原子替换），核验版本或内容摘要，防并发修改丢失；检查到使用的间隙用受控句柄/目录相对操作等可证明机制约束 TOCTOU（不能仅 canonicalize 一次就声称无竞态）。
4. 写入失败、磁盘满、权限拒绝和中断不得登记成功产物。保留 checkpoint/rewind 需要的明确修改记录接口（完整产品集成归 R06/R07）。文件确有部分副作用时如实记录。
5. 网关集成：read/write/edit 作为真实注册工具经 T02 网关执行（prepare→策略/批准（T03 面）→execute）；资源范围进入批准记录（A05 语义对文件路径成立）。补 T03 OBS-1 的真实别名解析腿。

验收（两场景都必须真实执行，真实文件系统隔离测试根）：
- R04-A07 并发编辑不覆盖用户修改：工具基于版本1准备编辑，用户改成版本2后继续；返回冲突且版本2不丢；重读/重试遵守原合同，不自动覆盖。
- R04-A08 符号链接不能逃逸授权目录：测试工作区链接指向授权测试根中的外部受限哨兵，读/写/改按真实目标权限判定，越权无副作用。
- 追加对抗检查：相似路径前缀、上级目录切换、空格/中文/Unicode 文件名、CRLF、长文本尾部、edit 重复匹配、临时文件清理（原子替换无残留）；非法输入应有合法对照（不能靠拒绝全部路径过关）。

测试边界：真实文件系统操作必须实际执行（tempdir 隔离测试根+授权根内外哨兵）；替身只替代外部系统。
验证命令（/Users/study_superior/.cargo/bin/cargo）：fmt/clippy/test workspace --locked（r00 防火墙环境项隔离核证）/check-contracts/check-boundaries/R03 十套件+A15+A16/T01-T03 套件回归。

禁止修改需求让实现通过、删断言、吞错误、静默降级或伪造证据。禁止 commit/push。
报告 docs/rust-tauri/R04/R04-T04_REPORT.md；证据 artifacts/rust-tauri/R04/T04-E01/；更新 R04_TEST_MAP.json T04 条目。
自检全过只返回 READY_FOR_REVIEW。结束本次代理。
