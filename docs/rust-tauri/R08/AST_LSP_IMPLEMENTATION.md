# R07 前置最小修复：AST / LSP

状态：READY_FOR_REVIEW；当前本域候选真实网关验收 4/4 已通过；独立审查未完成。不能据此宣称 R07 或 R08 通过。

## 原要求与责任

- `R00-T02-LA-196D50D8DD6E`：结构匹配而非同文字符串；无匹配空结果且不改工作区；语言/解析失败、越权返回错误。
- `R00-T02-LA-2D896560381E`：位置与源码一致；rename 仅作用于服务器返回范围；不可写时预览不写；服务缺失/崩溃、prepare 拒绝、能力缺失有明确结果；无编辑不改文件。
- 两叶均仍属 R07 交来的原责任；此次用户明确授权其最小修复后继续 R08，不修改原判据。

## 实际链路

正式 Rust ToolRegistry → ToolInvocationGateway → Run 上下文/权限与资源授权 → `coding_ast_lsp`。AST 使用官方 ast-grep CLI 的真实语法树；每次只通过 stdin 传入 Rust ResourceIO 取得的授权 UTF-8 快照。没有文本搜索兜底。保留 `content/files/count`、分页与明确上限；未知语言、模式/解析器错误不冒充无匹配。

LSP 单次 Node 外围仅复用现有纯 JSON-RPC 客户端，不导入旧工具、旧任务内核或 Pi。Rust 枚举、逐文件授权并固定读取版本；临时目录仅放这些快照。语言服务器在该独立根初始化，所有返回 URI 由外围映射为相对快照名，再由 Rust 精确核对。prepareRename 只在服务器明确不支持方法时可略过，显式拒绝不能略过。支持 changes 与 documentChanges；未知文件、文件创建/删除/重命名操作、版本错配、无效/重叠编辑均拒绝。UTF-16 列与 UTF-8 字节转换经过检查，不拆代理对。

Rust 在写前校验全部返回范围、当前写授权及原快照指纹，再通过 ResourceIO 的 `prepare_artifact_write.require_snapshot` / `publish_artifact` 写入，沿用真实文件历史及资源变更收据。`preview:true` 不写。任何目标写授权不可用则整个请求保留预览并声明 `applied:false`。跨文件写入不宣称事务原子性；中途失败返回已完成收据和恢复信息。

单次外围具有真实 Run permit、取消、30 秒限时、输出上限和独立进程组。Some(SandboxPort) 必须成功包装，否则拒绝；None 沿用现有生产外围边界，**没有宣称操作系统级隔离**。不继承服务令牌、供应商密钥或 NODE_OPTIONS。超时/取消发出整组终止并等待回收；正常等待后撤销 PID 所有权，防止退出后的 PID 复用误杀。

## 工具和验证

- 当前 Node `v24.16.0`；真实语言服务 `/usr/bin/clangd`。
- 仅在独立临时工具目录安装官方 `@ast-grep/cli@0.51.0`，未修改项目依赖或全局 HOME。npm 主包 integrity：`sha512-sIZEt4L4iQJRM2QDKQnbltQxddIsxzt9nmguw3/X7eP3cFwBopsZtFRvaEVht/3/6C1xe3Opn/pbsxSFVJDvpA==`。路径 `/private/tmp/lingxi-r08-ast-tools-0510/node_modules/.bin`。
- 外围真实探针已证明：`greet($A)` 仅命中调用，不命中字符串内容；clangd 返回 definition 原始位置和 rename 两处编辑；无诊断确实收到服务器 publishDiagnostics 后才返回空列表。
- 定向真实测试：`r08_admission_ast_lsp`。外部模型仅返回测试用工具请求；Run、存储、授权、ResourceIO、解析器、语言服务器均真实。Rust 工具链固定 `rustup run 1.98.1`。编译/结果将在本记录继续更新。

## 当前边界

- 自动枚举有 10,000 条目和 32MiB 快照总上限，超出明确报错，请求应缩小范围；AST 可指定 files。
- 本候选为单次语言服务；不冒充旧内核常驻进程。仅有当前机器 clangd 实测，其他语言依赖缺失时明确报错并给安装提示。
- 默认遍历保留常见构建/依赖目录跳过和根 .gitignore 模式；尚待独审核验复杂嵌套 ignore 规则等同程度。
- 此文件与定向测试不能替代完整 R07 证据纠偏、独立审查或 R08 逐叶验收。

## 验证流水（保留失败）

1. 首次编译 `--no-run` 退出 101：本模块取消 API 实际为 Option，公共安全模块的权限枚举拼写也不匹配。分别修复，未改变断言。
2. 第二轮/第三轮编译退出 101：公共 PTC 驱动正在接线，先后出现临时模块不存在及 ToolDescription/SessionId 类型不匹配；公共集成负责人修正后继续。
3. 首个 no-run 退出 0，1m40s。随后运行该构建产物 4/4 失败，均为 fixture 重复注册（构建期间组合根接线已变化，产物与待核源码不一致），不是产品行为验收结果。保留失败，改为当前源码重编与明确 fixture 接线。
4. 首个有效完整命令退出 0，1m25s 构建、7.07s 执行：4 passed / 0 failed / 0 ignored。命令：`PATH=/private/tmp/lingxi-r08-ast-tools-0510/node_modules/.bin:$PATH ~/.cargo/bin/rustup run 1.98.1 cargo test --manifest-path rust/Cargo.toml -p lingxi-service --test r08_admission_ast_lsp -- --nocapture --test-threads=1`。
5. 当前继续绑定细化候选：Drop 超时后继续持真实许可等回收；普通 fixture 沿用已注入的实际资源面，只有只读授权场景独立构造共享真实存储的资源面；真实 clangd rename 增加同一行含 emoji 的 UTF-16 范围验证；外部故障替身增加空 WorkspaceEdit 分支。结果待回填。

实际解析二进制：ast-grep `0.51.0` darwin-arm64，SHA-256 `6e4e64393e1cbc67b68f56606982c2834e9ffe771cf10d4811de0f4d15ee3a6d`。语言服务：Apple clangd `21.0.0 (clang-2100.3.34.2)` / arm64-apple-darwin27.0.0。公开包来源为 npm 官方 registry，本地临时安装仅用于本次合成根验收。

具名链路：`r08_ast_structure_pagination_no_match_and_errors_use_real_run`；`r08_lsp_real_clangd_positions_preview_rename_and_rejections`；`r08_lsp_read_grant_rename_returns_preview_without_writing`；`r08_lsp_missing_and_crashed_server_are_explicit_errors`（内部独立进程分别验证服务缺失、崩溃、空编辑和取消清理）。最后一项只替代故障外部 LSP，主成功链使用真实 clangd。

6. 上述细化候选经统一三测试 no-run 退出 0（48.24s）；直接运行 AST/LSP 二进制退出 0，4 passed / 0 failed / 0 ignored，6.46s。本轮四个具名测试已含 emoji 偏移、空编辑外部替身和真实取消进程树。
7. 因 ast-grep 可自动发现工作区配置，额外将其 cwd 收到 Rust 临时根，并显式提供可信的最小 `sgconfig.yml`（`ruleDirs: []`），不加载工作区自定义解析器；真实外围探针退出 0。新追加恶意工作区配置、默认 ignore、count/files 断言是在上一轮链接期间加入；上轮二进制不包含新增断言。需最终统一重编后执行全部测试，不把上一轮 PASS 覆盖新增差异。
8. `node --check scripts/rust-tauri/r08-lsp-worker.mjs` 与限定文件 `git diff --check` 退出 0。尚未执行其他平台、打包或独立审查。

本域待统一复验源码摘要（2026-10-21）：

| 文件 | SHA-256 |
| --- | --- |
| `rust/crates/lingxi-service/src/coding_ast_lsp.rs` | `6d2f5c83c57624477800f133407e84c91082074cdd49a21a1fcc983b52d59188` |
| `rust/crates/lingxi-service/tests/r08_admission_ast_lsp.rs` | `dbbb10e9fe196031b525891e67975fb6eb8b8a0f8e83bd429daf1c4edb4871ab` |
| `scripts/rust-tauri/r08-lsp-worker.mjs` | `13b7a5ff9c6a024d5f59b0fd1193a1262a989c5fe600d45dbe4fcbb87b389c26` |

9. 当前完整本域候选已重编并执行，命令同第4项，退出 0；20.64s 构建、7.33s 执行：4 passed / 0 failed / 0 ignored。包含全部新增恶意 sgconfig、根 ignore、count/files、多 AST 根解析失败断言。`Cannot parse query` 的真正解析器失败经正式 Run 返回 Failed，不伪装空匹配。当前表中三份源码摘要对应此轮。

实现候选分发边界补充：本适配当前寻找 PATH 上的官方 ast-grep，未导入旧 helper 的内置/托管目录及自动下载分发逻辑。缺依赖时明确报错并给环境依赖安装指引；本机验收通过是在临时固定版工具 PATH 下，不代表安装包/其他平台已交付。R08 的正式宿主/产物固定应记录真实依赖位置，不能把本域开发 PATH 当打包证据。

## 2026-10-22 冻结产物交接（仅旧产物复跑）

本域实现、测试和外围三份文件继续冻结，上表摘要未变。上一轮真实完整结果为第9项：4/4、7.33s、退出0；状态仍是 READY_FOR_REVIEW，待独审。该产物不包含后续公共 Run 驱动和 security 操作系统边界修改，不能替代下一轮整体候选复验。

已启动的旧二进制复跑于 2026-10-22 保存到 `artifacts/rust-tauri/R08/ast-lsp-frozen-binary-20261022-01/`：`stdout.log`、`stderr.log` 和 `manifest.json`。结果退出0，4 passed / 0 failed / 0 ignored，测试器报告8.40s；此后不再重复旧产物验证。manifest 包含命令、时间、原始日志摘要、二进制摘要与本域源摘要，并明确限定“只证明旧产物仍通过”。未启动 Cargo。

- AST/LSP 旧二进制：`rust/target/debug/deps/r08_admission_ast_lsp-a93a248840f19715`；mtime `2026-10-21T23:08:03+0800`；SHA-256 `530f9af5ea298f5f7e134718e65ce7b3de5f10b34a663e7144ea5f556ea5a3a5`。
- 同次统一构建留下的 security 旧二进制：`rust/target/debug/deps/r08_admission_security-f2475c2f9d54239a`；mtime `2026-10-21T23:03:25+0800`；SHA-256 `832f01aa39d7c1f062cb66617c07f3d65eedf3d543c3fbf7fe9598f227d39c81`。该旧产物尤其不能证明新增 OS 边界与新测试。

独审应逐项核对原两叶、正式 registry/gateway/Run、Rust 授权与 ResourceIO 的真实读写，再核对实际 worker 和取消进程树；实现者给出的4/4不是独立验收，也不是R07/R08阶段PASS。

## 2026-10-22 续跑：当前候选（cc7f7052）整体复验

deps 中 `r08_admission_ast_lsp-a93a248840f19715` 经统一构建（与 ptc/security/run_code 同轮，00:51 链接）后 SHA-256 为 `389d4b87cb719475247a03e4f2007c6711f2f964ef878a50f1650a97ec3ee546`，即包含后续公共 Run 驱动与 security OS 边界修改的候选产物；冻结旧产物（`530f9af5…`）不复用。本轮带 ast-grep 临时 PATH 实际执行该二进制：退出0，4通过/0失败/0忽略（7.91s），回执 `admission-astlsp-current-01`（candidateDiffSha256=cc7f7052…，sourceBindingsStable）。上一节"不能替代下一轮整体候选复验"的缺口已由本轮补上；仍为实现者本域结果，独审未变。
