# 默认组合根独立实际复验

**本组结论：PASS_LOCAL_DEFAULT_COMPOSITION_ONLY。整体授权准入仍未关闭。** 五个新目标已在真实默认工具面可见、可准备；run_code、security_scan、run_tools 已从真实默认模型入口执行，并通过真实关库重开读回。该结论不代领 AST/LSP 实际执行、完整权限/撤权矩阵、所有原叶或 R08 完成。

独立审查者：`admission_evidence_review`，未实现这些业务工具。执行日期：2026-10-22（Asia/Shanghai）。主集成冻结候选并释放 Cargo 后执行，未修改产品实现；基点仍为 `86840d1700397d3dd620a41f2b884df4cddd5cc2`，实际候选由逐文件摘要绑定。

## 实际入口及替身边界

独立宿主复用 [原R07独立反例宿主](../../../artifacts/rust-tauri/R08/r07-admission-review-01/rustup-1.98.1/tests/admission.rs) 的真实 `ConfigModelGateway`、`CredentialService`、`ContextCompilerService` 和 `ServiceState` 装配。**没有注入 TurnProvider、ToolRegistry、ToolGateway、ToolExecutor、ProcessSupervisor，没有手工调用 registrar。** 唯一替身是 loopback OpenAI SSE 协议供应商。

五目标先从默认 registry resolve，再经真实 gateway prepare；这部分是准备检查，不执行准备句柄。业务则从实际 HTTP 会话执行入口创建真实 Run，通过外部模型 HTTP 协议发出 `mcp_describe_tool` / `mcp_call`，消费正式说明和真实权限、内核、存储及进程链。测试不读取真实用户资料，不改全局 HOME；每轮独立合成 home/tmp，启动和停止真实 Rust 服务实例。

原要求沿 [run_code/run_tools原叶及授权独审](ADMISSION_AUTHORITY_INDEPENDENT_REVIEW.md)、[R00原账](../R00/FEATURE_STAGE_ACCEPTANCE.json) 和 [R07准入结论](R07_ADMISSION_REVIEW.md)，没有修改原断言或判据。

## 两轮结果与来源绑定

| 轮次 | 实际结果 | 来源边界 |
|---|---|---|
| [run-01回执](../../../artifacts/rust-tauri/R08/default-composition-independent-01/run-01/receipt.json) | Cargo退出0；1测试通过、0失败/忽略/过滤，业务0.82秒，总104.198秒（含编译） | 327件Rust四库/清单锁/worker前后摘要一致；第一轮原样保留 |
| [run-02回执](../../../artifacts/rust-tauri/R08/default-composition-independent-01/run-02/receipt.json) | Cargo退出0；同一业务测试通过，业务0.87秒，总6.511秒 | 补入纯TS规则/PTC运行时后334件输入前后摘要一致；实际rustc1.98.1、Node24.16.0、Python3.14.3已记录 |

实际命令入口：

```text
python3 artifacts/rust-tauri/R08/default-composition-independent-01/run_review.py --run --round run-02
```

runner 使用 `/Users/study_superior/.cargo/bin/rustup run 1.98.1 cargo test --offline --locked` 和独立临时 Cargo 包；精确 manifest 路径、参数、临时根、环境、耗时与退出码均在每轮 `command.json` / `receipt.json`。未占用生产默认根。大型二进制的既有 unwind 链接警告保留，未吞退出码。

首轮业务没有失败；补第二轮有两个明确原因：补齐纯TS输入来源摘要，以及修复独立协议替身潜在的历史计划选择不稳定。原替身按场景名匹配，会话历史含旧场景名时可能挑中已耗尽的计划；现只选非空计划。首轮实际逐场景完成且断言通过，并未因此改成失败；修改只使后续复验稳定，**没有修改产品或弱化业务断言**。

日志摘要：

- `run-01/output.log`：`5b71bb367597ba96597e3fa08fca8447bb755f7e55404e218088cf0bb9ecc6a3`
- `run-02/output.log`：`661966395033bc2c22102131afa39907fdf8cffb8c984c4dac7e58e7a96ab431`

## 已实际证明的用户结果

1. `run_tools`、`run_code`、`ast_grep`、`lsp`、`security_scan` 均由默认组合根登记且通过真实准备。原五目标完全缺失的准入反例，在本候选的该边界已消除；不能据此声称五目标全部原行为已通过。
2. Python两次默认模型调用共享真实变量，第二次得到 `INDEPENDENT_STATE_RESULT 42`；真实文件副作用写为 `1`。
3. 默认 `security_scan` 扫描一个真实合成文件，得到对应文件和 `secret_api_key_literal` 规则；模型投影不含命中正文。实际审计事件为1条，`critical=1 / warning=1 / info=0`、`filesScanned=1`、`engines=[baseline]`，只有计数等元数据，未含合成命中值。本轮不宣称外部 semgrep/gitleaks 已实际成功运行。
4. 默认 `run_tools` 真实读文件并写另一文件；真实库中有两条独立子调用日志及成功回执，外层返回 `INDEPENDENT_PTC_SUMMARY`。原始子标记 `PTC_PRIVATE_CHILD_DO_NOT_SEND` 没有进入实际外部模型 HTTP 请求或模型历史正文。
5. 实际服务正常停止，清理无超时；同一合成数据库由新服务实例重开后，原工具结果仍可读，文件副作用仍为 `1`，读历史没有重新执行代码。它关闭了本组“只在活Storage读两次”的证据不足，但不是 R08 迁移/回滚/前滚证明。

[run-02行为观察](../../../artifacts/rust-tauri/R08/default-composition-independent-01/run-02/behavior-observations.json) 是关库后的真实合成SQLite只读核查：12条成功调用日志，包含2条PTC子日志；14次实际外部模型请求已经固化到同目录 `model-requests.json`。只查询调用事实，没有读取或导出凭证表。[扫描审计观察](../../../artifacts/rust-tauri/R08/default-composition-independent-01/run-02/security-audit-observation.json) 保存对应计数事件及原文件摘要。

## 保留的独立验收缺口

本组测试没有验证 guest/phone/workflow 完整授权矩阵、批准后撤权、Started窗口撤权、迟到结果、配额1/协作收尾、崩溃重放、delegation全工具语义；也没有实际运行AST/LSP业务、取消/超时的全部原分支或真实UI。因此 [授权独审](ADMISSION_AUTHORITY_INDEPENDENT_REVIEW.md) 仍为 `FAIL_NOT_CLOSED`，只将本报告列出的默认装配和实际恢复边界记为本地独立通过。

后续仅在候选再变、出现新失败或补缺口时按影响复验；不机械重复本组。旧证据和原断言继续保留，不能以本组1项绿色替代阶段门禁或完整授权独审。
