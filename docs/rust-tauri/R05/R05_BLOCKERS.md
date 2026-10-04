# R05_BLOCKERS — 缺口、负责人、最迟解除阶段

- 生成：EXECUTOR-R05-T08，2026-10-04。范围：R05 阶段终态仍未解除的缺口（阻塞/延期/环境项）。
- 规则：只有真实阻塞才登记；已关闭的修复轮不在此重复（见 `R05_ACCEPTANCE_LEDGER.json` 的 fix_rounds 与各 REVIEW-T0x）。

## 1. RR-BLK-CREDENTIALS — LIVE 真实供应商验证未授权（延期，非代码缺口）

- 状态：`BLOCKED_NOT_AUTHORIZED`（登记于 `R05_LIVE_VERIFICATION.json`）。
- 内容：真实 provider 登录/请求/工具往返/取消/刷新与跨 provider 媒体实测未获授权（无测试凭证、无目标账号、无预算）。按任务书 §9 允许登记至最迟 **R10**。
- 负责人：用户（提供凭证与预算授权）；实现侧无剩余工作（离线全链已交付并过独立审查）。
- 影响：不影响 offline_gate；`stage_readiness` 只能取 `ACCEPTED_OFFLINE_SCOPE_WITH_REGISTERED_LIVE_DEFERRALS` 形态（在独立审查通过后）。

## 2. R05-ENV-ALF-UNSIGNED-TEST-BINARY — macOS 应用防火墙对未签名测试二进制的 LAN 段拦截（环境项）

- 状态：环境项（T01 已定案，台账 `PROGRESS_LEDGER.json` → environment_items）。R05-T08 窗口内未复现（工作区测试与门禁运行中 r00_management_leaves 全绿）；如最终门禁窗口复现且为唯一失败，按环境项登记，不放宽门禁、不改该测试、不当代码回归修。
- 负责人：用户（ALF Allow / socketfilterfw / 开发者签名三种解法任一）；执行者无法自解（无免密 sudo）。
- 最迟解除：不设阶段期限——它是运行环境项，不是阶段义务；每次复现按台账登记。

## 3. R05-T05-C12 代理/私有 CA 面 — 登记的能力差距（延期到网络加固阶段）

- 状态：`NOT_RUN`（台账登记：现役无 Rust 侧代理配置处理代码，代理面为 NOT_APPLICABLE 附审查者来源证据；私有 CA 通道（NODE_EXTRA_CA_CERTS 等价物）为真实能力差距）。
- 负责人：后续网络加固阶段（任务书允许的显式延期，REV-T05 已复核）。
- 最迟解除：随 R05 任务书允许的延期规则（该缺口在 SCOPE_MATRIX 有后续归属行）；TLS 验证保持默认开启，无降级点（负向 R05-GATE-N12 类保护在 T05 套件）。

## 4. 平台验证缺口（继承）

- Windows：进程/沙盒执行面尚非已实现（R04 起登记）；R05 未在 Windows 实测。
- Linux：未真机验证项继承 R02–R04。
- 负责人：具备对应平台的执行窗口；最迟解除：按 ORCHESTRATOR_PROGRESS 的平台义务行（不在 R05 内清零，也不因 R05 宣称支持）。

## 5. 已知非阻塞携带项（来自审查轮，登记在案）

- REV-T07 R01 的 F-04/F-05/F-06（mustFix=false）：`UsageFolder::fold` 注释矛盾（当前调用方不可达）、`decode_operation_usage` 非对象分支的 invalid_detail 未截断/scrub、操作面台账行零测试覆盖。已在 T07 fix_rounds.not_fixed_registered 登记，随阶段携带。
- R05-T01-C01/C12 的范围矩阵再生成脚本（python/node）不是 cargo 测试，未注册为门禁命令——其产物（SCOPE_MATRIX/CALLSITE/PROVIDER 矩阵）在 T01 终树生成并经独立审查核对；R05 门禁经 rust_test_workspace + 生产者覆盖代码面。若后续要把矩阵再生成纳入机器门禁，需注册新命令（登记为可选增强，非缺口）。

## 无其他已知阻塞

R05-T08 终树五门禁与 verify-stage R05 的真实结果见 `R05_REPORT.md`；如门禁失败，失败项按实登记，不以本文件预告。
