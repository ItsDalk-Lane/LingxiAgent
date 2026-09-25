# R01 阶段报告｜目标契约与高风险替代验证

## 阶段与结论

**READY_FOR_REVIEW**（执行汇总结论；阶段放行归总控另派的独立验收，本报告不含执行者自评 PASS）。

## 范围

仅 R01（T01–T08）。基线差异：R00 正式封印 C4（`328cc8bb5a807bdaad520b459907fb1fa4e10dca`）→ 当前 HEAD
`2bbec6d072f1a06305392828bfe3a7fd9739e667`，全部为 R01 任务/账本提交。获准 ADR：ADR-001（所有权）、
ADR-002（浏览器宿主选型）、ADR-003（文档渲染器选型）、ADR-004（存储切换与旧版本拒写）。
**生产目录（desktop/ server/ core/ lib/ shared/）零改动**；唯一非 docs/artifacts 改动为 T08 按 T05 R2-N1
移交收口的两个负向测试资产注释更正（tests/migration/r01-t05/，注释级、零语义变化、未被任何哈希清单钉住）。

## 源码

- 起止 SHA：`328cc8bb…`（R01 stage base）→ `2bbec6d0…`（当前 HEAD）；T08 交付未提交（执行代理无 commit 授权），
  工作树摘要见 R01_HANDOFF.json `working_tree_digest`。
- 分支：codex/rust-tauri-migration。
- 依赖锁摘要：package-lock.json `e54a16fe…`（R00 起未变）；rust/Cargo.lock `d1d8b4a9…`（280 条目，
  T03 锁 279 + T04 新增 lingxi-browser-spike 自身，无新增第三方版本）；rust-toolchain.toml channel=1.98.1
  `eec34104…`。

## 环境

macOS 27.0 arm64（Mac15,14）；Node v24.16.0；Python 3.14.3；rustup 1.29.1 + rustc/cargo 1.98.1
（锁定）。构建模式：cargo --offline（crates.io 依赖 T03 已 fetch 入锁）；浏览器/PDF 原型用本机 Chrome
+ 限 loopback 吊具；Tauri spike 双产物（app-test/app-release）。外部替身：lingxi-proto-server（loopback
确定性替身）、testsite.mjs（127.0.0.1:18281）、canary（127.0.0.1）；无真实账号、无真实用户数据。
**平台结论仅覆盖 macOS arm64；Windows/Linux/macOS x64 全部 UNVERIFIED，挂 R09/R10 强制关卡。**

## 完成项

- **T01 所有权**：ADR-001 + DEPENDENCY_RULES.json + OWNERSHIP_TARGET.json（736 F-ID/69 store/30 owner/
  11 关键事实逐项归属）+ 机器校验器（正向 O1–O8/D 系列 + N1–N15 负向 + 生成器 --check 零漂移）。
- **T02 协议**：PROTOCOL_SPEC.md（lingxi.wire v1 独立版本轴）+ rust/crates/lingxi-protocol（serde 权威源）+
  生成链（JSON Schema 56 文件 / TS 绑定 / 12 golden / MANIFEST）+ API_COMPAT_MATRIX.json（624 条目：
  427 业务兼容 retain + 197 native_host 映射）+ 握手原型。headSha 漂移缺陷已修复为内容派生戳（b9442d86f）。
- **T03 依赖锁定**：DEPENDENCY_DECISIONS.md（D-01..D-08：rustup 1.98.1 / axum 0.8.9 / reqwest 0.13.5+rustls
  平台根证书 / rusqlite 0.40.2 bundled / tracing / jsonschema 0.57.0 / rmcp 3.4.1）+ PLATFORM_BUILD_MATRIX.json
  + spike_health/spike_tls_probe 真实运行。
- **T04 浏览器原型**：ADR-002 选定受控 Chromium（CDP over pipe）；32 项等价断言 + 隔离 B1–B6 + 接管 D1–D4
  全 VERIFIED；WKWebView/wry 缺口如实记录为落选理由。
- **T05 PDF/办公原型**：ADR-003 选定受控 Chromium printToPDF；19 场景 28 VERIFIED/0 FAILED；A09 中文长文档
  35 项全过；A10 危险资源 7×DENY + 无限脚本 exit=2；repair-r1 CSP connect-src 封堵 WS（负向 7/7）。
- **T06 系统能力原型**：TAURI_SPIKE_REPORT + SHELL_CAPABILITY_MATRIX（14 能力逐项）；15 步 e2e 双产物
  realpath/symlink 两轮全绿 + sabotage 负向；ACL untrusted/remote 11/11 全拒；updater 篡改验签失败。
- **T07 存储切换**：ADR-004（D2 分离根+原子指针）+ DATA_COMPATIBILITY_MATRIX.json（69 存储）+
  ROLLBACK_DESIGN.md；旧二进制拒写变体矩阵实测（含 corrupt 类 fail-open 如实暴露并立项 PROD-DEFECT-1）；
  A14 回滚演练 ROLLBACK-DRILL-PASSED。
- **T08 冻结关卡**：RISK_REGISTER.json（43 条目，逐条截止阶段+失败处理）+ 关卡检查器（A15）+ 双向覆盖
  检查（A16）+ 隔离核查（机器验证 ISOLATED）+ T06 二进制处置（删除留证）+ /tmp R01 克隆清理（74 项 36GB）+
  本验收账本 + R01_HANDOFF.json。详见 R01-T08_REPORT.md。

## 行为变化

无用户可见变化。全部原型为隔离目录（rust/、spike/、contracts/、tests/migration/r01-*），机器核查证明
生产入口零引用（r01_t08_isolation_check.py：2209 生产文件扫描 0 违规）。本阶段不是新增功能：任务书 §1
明确「原型不得成为默认生产入口」。

## 验收

逐场景命令/预期/实际/退出码/证据指针的完整账本见 **R01_ACCEPTANCE_LEDGER.json**（16/16 REQUIRED）。

| 场景 | 结果 | 关键命令 | 退出码 | 复核 |
|---|---|---|---|---|
| R01-A01 核心不依赖桌面 | PASS | cargo build/test/metadata --offline | 0 | R3 |
| R01-A02 双负责人被检出 | PASS | r01_t01_check_ownership.py --self-test（N1–N15） | 0 | R3 |
| R01-A03 跨语言 round-trip | PASS | r01-t02-roundtrip.sh（12 golden 逐字节） | 0 | R1+fix |
| R01-A04 版本不兼容可诊断 | PASS | r01-t02-handshake.sh（5 场景 400/4409） | 0 | R1+fix |
| R01-A05 锁文件可复现 | PASS | fetch --locked + build --locked --offline | 0 | R1 |
| R01-A06 依赖缺失不掩盖 | PASS | 缺依赖对照实验 exit 101 | 101（预期） | R1 |
| R01-A07 浏览器能力等价 | PASS | T04 四阶段 32 断言 + 旧侧对照 | 0 | R1+docfix |
| R01-A08 跨会话/不可信页隔离 | PASS | B1–B6 + ADV-1..8 对抗 | 0 | R1 |
| R01-A09 中文长文档完整 | PASS | r01-t05-replay.sh a09（35 项全过） | 0 | R2 |
| R01-A10 危险资源/失败可控 | PASS | S5 7×DENY + S6 exit=2 + WS 负向 7/7 | 0/2（预期） | R2 |
| R01-A11 自定义命令受限 | PASS | run_e2e.sh（ACL 11/11 拒） | 0 | R2 |
| R01-A12 测试能力不进生产 | PASS | 双产物探测 + 篡改验签失败 | 0 | R2 |
| R01-A13 旧程序拒写可验证 | PASS | a13 探针组 + 10 变体矩阵 | 0/1（变体预期） | R2 |
| R01-A14 回滚不丢新数据 | PASS | rollback-drill.py step1–7 | 0 | R2 |
| R01-A15 高风险不被演示遮蔽 | READY_FOR_REVIEW | r01_t08_gate_check.py（负向 5/5 + 真实数据 PASS_WITH_CONDITIONS） | 0 | 待独立验收 |
| R01-A16 目标职责闭合 | READY_FOR_REVIEW | r01_t08_coverage_check.py（COVERAGE-CLOSED + 负向 6/6） | 0 | 待独立验收 |

T08 全量重跑门禁（本阶段 HEAD 上真实复跑，日志 artifacts/rust-tauri/R01/T08/gates/）：
T01 校验器正向 exit=0、N1–N15 exit=0、生成器 --check exit=0（736/69 up-to-date）；
T02 roundtrip exit=0、handshake exit=0、check-generated exit=0（56 生成文件+624 条目零漂移）；
cargo test --workspace --offline（rustup 1.98.1，CARGO_TARGET_DIR=/tmp 全新目录）exit=0，45 passed 0 failed。
**重跑新发现 RR-T08-F1**：T02 门禁脚本共享默认 CARGO_TARGET_DIR + 生成器用编译期 CARGO_MANIFEST_DIR，
共享缓存内评审遗留克隆编译的二进制使 --check 比对了遗留克隆树而非本仓库；已用全新 target 复跑证明本仓库
生成树真实零漂移（缺陷限门禁健壮性，归属 R02 硬化；未自行修复前序交付，如实登记并报告总控）。

## 安全与数据

- 权限负向：T04 ADV-4/5/6（file:// 越权/CDP 面）、T06 ACL 11/11、T05 ATK1–ATK8 全部真实拒绝。
- 真实进程：旧发布二进制拒写探针（A13）用现役真实二进制；T06 sidecar 崩溃恢复 sabotage 负向 exit=1。
- 单写者：T01 O8 锁定 rust-service 唯一业务写者（精确串匹配，shadow writer 负向 N15）。
- 迁移/回滚：ADR-004 epoch 闸 + D2 分离根；A14 演练幂等合并 sha256 稳定；协作式闸定性入档（RR-T07-F2）。
- 未伪造授权：TCC 全部只读查询；录音/听写授权正链路、屏幕真实采集如实 UNVERIFIED。

## 完整映射

F-ID→T-ID→A-ID→测试→结果：736 F-ID 的机器双向闭合由 r01_t08_coverage_check.py 输出
（artifacts/rust-tauri/R01/T08/coverage/coverage-report.json）：736/736 有目标 owner（非 worker）与
合法实施阶段；11 关键事实 core/service 单负；14 Pi 能力全部 KERNEL_MIGRATION 无第二 loop 保留项。
叶子功能到阶段/任务的分配在 OWNERSHIP_TARGET.feature_ownership（stage_ids/task 级归后续阶段任务书）。
**未覆盖集合**：无未归属 F-ID；未验证集合 = 跨平台（RR-T06-PLATFORM）、授权态媒体能力
（RR-T06-MIC/SPEECH/SCREEN）、代理矩阵（RR-T06-PROXY）、WebRTC 边界（RR-T05-N2）、T04 原型 Fetch 层
（RR-T05-X1）、shell-local 存储过闸（RR-T07-F3）——全部挂账 R09/R10（代理矩阵最迟 R10）。

## 已知缺陷

全部 43 条见 RISK_REGISTER.json（含根因/后果/截止阶段/失败处理/证据指针）。最关键：
- **RR-T07-PROD-DEFECT-1（REGISTERED，归属候选 R02）**：data-epoch-coordinator corrupt-failure fail-open
  + 警告文本失实；R01 不改生产代码；R02 新实现须 fail-closed 并重跑 4 变体。
- **RR-T08-F1（MEDIUM，归属 R02 门禁硬化）**：T02 门禁脚本陈旧二进制错绑（本节验收段已述）。
- **RR-T02-F2/F3（MEDIUM，截止先于 R04/R08）**：TS canonical-json 浮点/>2^53 硬失败+spec 矛盾；
  EventPayload 回退吞畸形已知事件+两信封不变量未编码。
- **RR-T06-R6（HIGH，R09）**：updater 必须 https，dangerousInsecureTransportProtocol 禁进生产。
- **RR-T06-MIC/SPEECH/SCREEN、RR-T06-PLATFORM（HIGH，R09/R10）**：授权态媒体能力与跨平台未验证。

## 未执行/BLOCKED

- 全量 npm test 本轮未重跑（任务规格既定）：审计封印家族 4 用例预存 FAIL 的两基点分类原样承继 R00
  （RR-AUDIT-SEAL-PREEXISTING；R01 提交后继续预期红；封印推进属总控 seal 工作流，非本阶段实现缺陷）。
  不阻止本阶段放行评审（本阶段无生产代码改动）。
- 跨平台四组真机（BLK-PLATFORM）、真实凭证 LIVE（BLK-CREDENTIALS）、长时稳定性（BLK-LONGRUN-G1）：
  承继 R00，最晚解除 R10；发布授权（BLK-RELEASE-AUTH）R11。均不阻止 R01 放行（阶段放行条件只要求
  实施平台真实原型证据 + 未验项挂 R09/R10 强制关卡——已满足并挂账）。
- check_deps 动态库缺失分支 macOS 实跑（RR-T03-F3）：归 R09/R10，不阻止。

## 回退

本阶段无生产改动，回退 = 删除/停用隔离原型（rust/、spike/、contracts/、tests/migration/r01-*、
scripts/rust-tauri/r01-*、docs/rust-tauri/R01/、artifacts/rust-tauri/R01/）即还原；不涉及用户数据。
原型内回滚设计（A14）已演练：ROLLBACK-DRILL-PASSED，回滚流程不丢切换后新增数据（先导出归档再动指针、
幂等合并），无「直接删新目录」步骤。T06 的 6 个未入库二进制已按「删除无用实验而保留证据」处置
（SHA-256 与已入库清单核验一致后删除，artifacts/rust-tauri/R01/T08/disposition/）。

## 独立审查

T01–T07 各任务独立验收全部 PASS（复核者/轮次/报告 SHA 见 ORCHESTRATOR_PROGRESS.json 与
R01_ACCEPTANCE_LEDGER.json review 字段）；T02 headSha 修复复验 PASS。T08（A15/A16）的独立验收
**待总控另派**——本报告所有 T08 结论为执行证据，未验范围：跨平台实机、授权态 TCC 正链路、
真实凭证 LIVE 项。

## 下一阶段

- **允许范围**：R02（Rust 独立服务、存储与事件基础）。必须消费输入：lingxi.wire v1 协议 crate 与生成链、
  OWNERSHIP_TARGET/DEPENDENCY_RULES、DEPENDENCY_DECISIONS 锁表、ADR-004 数据规则、RISK_REGISTER
  （尤其 RR-T07-PROD-DEFECT-1 修复归属、RR-T08-F1 门禁硬化、RR-T02-F1..F5 协议修复截止）。
- **依赖顺序与目录约定**：见 R01_HANDOFF.json `allowed_next_scope` 与 §依赖顺序段（本报告不复制事实源）。
- **不允许开始**：正式替壳（R09 之前不得以原型替代生产入口）；跨平台放行结论；生产 CSP/代理/updater
  配置定稿；任何把 UNVERIFIED 项写成已验证的行为。

## 远程/发布

未获准、未执行：本阶段无 commit/push/PR/tag/release 动作由本执行代理发起（T01–T07 提交推送由总控
在独立验收 PASS 后执行，记录于 ORCHESTRATOR_PROGRESS.json）；发布授权（BLK-RELEASE-AUTH）未授予。
