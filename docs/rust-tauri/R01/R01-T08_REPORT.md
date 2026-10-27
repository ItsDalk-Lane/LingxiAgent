# R01-T08 执行报告｜冻结架构关卡与风险决策

- 执行者：ZCode:R01-T08-exec-r1（全新独立执行代理；不负责独立验收、不 commit/push）
- 日期：2026-09-25｜分支 codex/rust-tauri-migration
- 基线复核（开工留证）：`git rev-parse HEAD` = `2bbec6d072f1a06305392828bfe3a7fd9739e667`（与任务书基线一致）；
  `git status --porcelain` 仅 6 个 T06 未入库构建二进制（本任务处置对象），无其他残留。
- 环境：macOS 27.0 arm64；Node v24.16.0；Python 3.14.3；rustup+cargo/rustc 1.98.1（`~/.cargo/bin` 先于
  Homebrew 1.93.0 入 PATH，与 DEPENDENCY_DECISIONS D-01 操作要求一致）。
- 结论：**READY_FOR_REVIEW**（A15/A16 执行证据齐备；独立验收待总控另派）。

## 1. 任务书 §4 四步执行情况

### 步骤 1：汇总与风险登记 → RISK_REGISTER.json

43 条目（分布详见 §5 汇总）。逐类核对原始报告后登记：
- T02 验收 F1–F5（R01-T02_REVIEW_R1 §5 原文逐条核对）+ headSha 修复复验 F-info-1
  （R01-T02_FIX_HEADSHA_REVIEW_R1 §F-info-1）。
- T05 R2 的 N1/N2/N3（R01-T05_REVIEW_R2 §N1–N3）+ T05 repair 登记的 T04 交叉影响
  （R01-T05_REPAIR_R1「交叉影响（T04）」段：spike_browser 无 Fetch 拦截层、loopback WS 不受候选代码约束）。
- T06 spike 风险 R1–R8（TAURI_SPIKE_REPORT 原文）+ 录音/听写授权正链路、屏幕真实采集、代理矩阵
  UNVERIFIED（SHELL_CAPABILITY_MATRIX capabilities 逐项核对）+ 跨平台 UNVERIFIED（platform_scope_note）。
- T07：PROD-DEFECT-1（R01-T07_REVIEW_R2 变体表 + REPAIR_R1 §5 + ADR-004 §5）、F3（6 个 shell-local
  存储不过闸，DATA_COMPATIBILITY_MATRIX note 核对）、协作式闸定性（F2，已 DOCUMENTED）。
- 阶段级：AUDIT-SEAL-PREEXISTING 两基点分类原样承继 R00 HANDOFF；BLK-CREDENTIALS/PLATFORM/
  LONGRUN-G1/RELEASE-AUTH 承继；T03-R3-F01/F02（归 R07-T12）承继；另承继 R00 其余未决
  （T05-R1-F03/F04、T06-F08/F09/F10、T06-O1、ROUND2-TEST-SIDEEFFECT）保持单一事实源。
- **新发现 RR-T08-F1**（本任务门禁重跑时发现，MEDIUM，门禁健壮性）：见 §3。

### 步骤 2：双向覆盖检查 → r01_t08_coverage_check.py（A16）

机器校验（非人工抽查），9 组检查全 PASS：C1 计数（736/736/69）、C2 F-ID 双向一一对应（无缺失/孤儿/重复）、
C3 target_owner 全登记且无 worker 承载、C4 全部 F-ID 有合法实施阶段、C5 11 条关键事实冻结集精确一致且
core/service 单负、C6 69 存储归属闭合、C7 反向直方图（19 owner 承载 F-ID；11 个零 F-ID owner 承担存储/
职责，如实列出非缺陷）、C8 14 条 Pi 能力全部 KERNEL_MIGRATION（rust_owner+target_stage 非空，无仅靠第二
Agent loop 保留项）、C9 Pi 四类未映射差集全空 + 19 条 worker 反例闭合。
运行：`python3 -B docs/rust-tauri/R01/r01_t08_coverage_check.py --out artifacts/rust-tauri/R01/T08/coverage/coverage-report.json`
exit=0 → COVERAGE-CLOSED；`--self-test` 负向 6/6（删映射/指 worker/未登记 owner/第二 loop 保留/worker
持关键事实均按预期拒绝 + 正面对照）。日志：artifacts/rust-tauri/R01/T08/coverage/。
所有必需能力均有实现路径（owner + 阶段 + Pi 归属），可称架构覆盖闭合（限静态清单层，真实实现归后续阶段）。

### 步骤 3：隔离核查、处置与验收账本

- **隔离机器核查**：r01_t08_isolation_check.py —— 2209 个生产文件（package.json/desktop/server/core/lib/
  shared/scripts/build/vite/tsconfig/electron-builder 配置）逐字节扫描 9 类原型引用模式，**0 违规**；
  209 个原型跟踪文件 0 个落在生产目录；spike 构建产物 gitignore 覆盖确认；rust/target 未被根 .gitignore
  覆盖（如实报告不判负，目录约定建议 R02 增补）。结果 ISOLATED，exit=0
  （artifacts/rust-tauri/R01/T08/isolation/）。
- **可复用代码标注**（供 R02+ 直接消费）：
  - `rust/crates/lingxi-protocol/`（协议权威源 + gen/verify/server 三 bin）→ R02/R04/R05/R08 复用；
  - `rust/crates/lingxi-kernel/`（ports 骨架）→ R02 组合根起点；
  - `rust/crates/lingxi-spike/`（health/tls/check_deps 探针）→ R02 服务雏形参考；
  - `rust/crates/lingxi-browser-spike/`（CDP 管道+Fetch 审计+CSP 注入）→ worker.browser-engine /
    worker.doc-parse（R07）候选基座；
  - `spike/tauri-shell/`（84 跟踪文件：窗口/托盘/快捷键/通知/剪贴板/对话框/ACL/sidecar/updater e2e）
    → R09 宿主起点；
  - `contracts/generated/`（56 文件）+ `tests/migration/r01-t02/` TS 消费端 → 兼容层参考；
  - `scripts/rust-tauri/r01-t02-*` 三门禁 → CI 门禁（须先按 RR-T08-F1 硬化）。
- **T06 二进制处置**：6 个未入库二进制（约 124MB）删除前逐文件复算 SHA-256，与已入库
  SHASUMS-binaries.txt 逐字符一致（三组同哈希，位级可复现）；删除并留证
  artifacts/rust-tauri/R01/T08/disposition/t06-binaries-disposition.json（含复现路径）。
- **/tmp 清理**：R01 各轮（T01–T08 执行/评审/修复）74 项约 36GB 已删除，范围清单与 du 快照见
  artifacts/rust-tauri/R01/T08/tmp-cleanup/（含明确未触碰的 R00/历史项清单）；T08 本轮 handshake 证据
  已先复制入 gates/；`/tmp/lingxi-r01-t08-rust-target`（本轮 cargo 缓存）保留至评审后。
- **T05 R2-N1 收口**：更正 tests/migration/r01-t05/ 两处负向资产注释（setBlockedURLs→实际 CSP
  connect-src 机制；注释级改动，未被任何 SHA 清单钉住，零语义变化）。N3 收口说明：T05 证据两清单
  作用域不同（根 632 项=主证据树；repair-r1 653 项=修复子树），均全量 OK，非证据缺失。
- **验收账本**：R01_ACCEPTANCE_LEDGER.json（16/16 REQUIRED 场景：tested_sha/命令/退出码/证据指针/
  mock 边界/复核者；格式遵循 task-result.template.json，惯例参照 R00-T07_RESULTS.json；不修改 R00 账本）。

### 步骤 4：依赖顺序与目录约定

写入 R01_HANDOFF.json `allowed_next_scope` 与 R01_REPORT §下一阶段；要点：R02 直接消费协议 crate/
所有权/依赖锁/ADR-004 数据规则；跨平台放行挂 R09/R10；本阶段结论仅限 macOS arm64。

## 2. 关卡检查器（A15：高风险功能不被演示遮蔽）

r01_t08_gate_check.py + 真实输入 r01_t08_gate_inputs.json（五能力域：browser_host/pdf_renderer/
shell_capabilities/storage_cutover/protocol_chain；每项必需能力钉住证据文件 SHA-256，检查器复算核验）。
规则：必需能力非 VERIFIED → 域 NOT_COMPLETE；证据缺失/哈希不符 → BLOCKED；递延项必须在
RISK_REGISTER 挂账（截止阶段+失败处理）否则 BLOCKED；任一域不 COMPLETE → 总体 NO-GO 并明确阻塞替壳。

- **负向（A15 核心）**：`--self-test` 5/5。N1 构造「截图 VERIFIED + 用户接管 FAILED」输入 →
  browser_host=NOT_COMPLETE、VERDICT=NO-GO、SHELL-REPLACEMENT=BLOCKED 且原因点名 user_takeover
  （截图证据不能遮蔽接管失败）；N2 证据哈希篡改 → BLOCKED；N3 递延项未挂账 → BLOCKED；
  N4 必需能力 UNVERIFIED → NO-GO。日志 artifacts/rust-tauri/R01/T08/gate-checker/gate-selftest.log。
- **真实 R01 数据**：五域全部 COMPLETE（证据哈希复算全一致）；VERDICT=**PASS_WITH_CONDITIONS**——
  15 个递延项全部挂账（录音/听写/屏幕/代理/跨平台→R09/R10；T02 F1–F5→R04/R08；T07 缺陷/壳存储→R02/R09；
  WebRTC/Fetch 层→R09）。判定与 SHELL_CAPABILITY_MATRIX/BROWSER_SPIKE_REPORT 等源证据逐项一致
  （gate-report.json 含逐能力 result）。替壳路径结论：ALLOWED_FOR_NEXT_STAGE_ONLY——允许进入下一阶段
  实施，不等于允许替壳上线；跨平台/授权态放行仍挂 R09/R10 强制关卡。

## 3. 全量重跑门禁留证（HEAD=2bbec6d0，日志 artifacts/rust-tauri/R01/T08/gates/）

| 门禁 | 命令 | 退出码 | 结果 |
|---|---|---|---|
| T01 正向 | `python3 -B docs/rust-tauri/R01/r01_t01_check_ownership.py` | 0 | RESULT: OK |
| T01 N1–N15 | `… --self-test` | 0 | 15 弹药全按规则 ID 拒绝 |
| T01 生成器 | `r01_t01_build_ownership.py --check` | 0 | UP_TO_DATE features=736 stores=69 |
| T02 roundtrip | `bash scripts/rust-tauri/r01-t02-roundtrip.sh`（全新 CARGO_TARGET_DIR） | 0 | 12 golden 字节一致 + tsc + 负向 |
| T02 handshake | `r01-t02-handshake.sh /tmp/lingxi-r01t08-handshake`（同上） | 0 | 5 场景全过（证据已复制入 gates/） |
| T02 check-generated | `r01-t02-check-generated.sh`（同上） | 0 | 56 生成文件 + 624 条目零漂移，正确指向本仓库树 |
| cargo test | `CARGO_TARGET_DIR=/tmp/lingxi-r01-t08-rust-target cargo test --workspace --offline --manifest-path rust/Cargo.toml`（rustup 1.98.1） | 0 | 45 passed / 0 failed / 0 ignored |

未跑全量 npm test（任务规格既定）：审计封印 4 用例预存 FAIL 分类承继 R00（RR-AUDIT-SEAL-PREEXISTING）。

**新发现 RR-T08-F1（如实报告总控，不自行修复前序交付）**：T02 三门禁脚本默认共享
`CARGO_TARGET_DIR=/tmp/lingxi-r01t02-target`，lingxi-protocol-gen 用编译期 `env!(CARGO_MANIFEST_DIR)`
解析仓库根。本任务首轮重跑时，共享缓存内残留评审期在 `/tmp/r01t02-review-clone` 克隆编译的二进制未被
重编译，`--check` 输出自认比对的是 `/private/tmp/r01t02-review-clone/contracts/generated`（见
gates/t02-check-generated.log）——即用错靶树也绿灯。以全新 target 复跑（gates/*-fresh-target.log）
证明本仓库生成树真实零漂移，缺陷限于门禁脚本健壮性；残留克隆与缓存已随 /tmp 清理删除，修复建议
（target 目录按检出派生 / gen 运行时解析根）记入 RISK_REGISTER RR-T08-F1，截止 R02（CI 门禁固化前）。

## 4. 交付物清单

docs/rust-tauri/R01/：R01_REPORT.md、R01_HANDOFF.json、RISK_REGISTER.json、R01_ACCEPTANCE_LEDGER.json、
R01-T08_REPORT.md、r01_t08_coverage_check.py、r01_t08_gate_check.py、r01_t08_gate_inputs.json、
r01_t08_isolation_check.py。
artifacts/rust-tauri/R01/T08/：gates/（门禁日志+handshake 证据）、coverage/（报告+日志）、
gate-checker/（报告+日志）、isolation/（报告+日志）、disposition/（T06 二进制处置记录）、
tmp-cleanup/（清单+快照+记录）。
注释更正：tests/migration/r01-t05/negative/ws-loopback.html、tests/migration/r01-t05/run_repair_r1_negative.sh。
逐文件 SHA-256 汇总于本报告 §6 与 R01_HANDOFF.json artifact_hashes。

## 5. RISK_REGISTER 汇总

43 条。按状态：OPEN 24、REGISTERED 1（PROD-DEFECT-1）、DOCUMENTED 2、CARRIED 14、CLOSED_IN_R01 2（N1/N3）。
按截止阶段（机器复算值）：R02×3（PROD-DEFECT-1、RR-T08-F1、RR-T02-FINFO1）、R04×4（T02 F1–F4）、
R08×2（F5、协作式闸覆盖 R08/R09 单列）、R09×14、R09/R10×2、R09 最迟 R10×1（跨平台）、R10×3
（BLK-CREDENTIALS/PLATFORM/LONGRUN-G1 承继）、R11×1（RELEASE-AUTH）、R03×1（T05-R1-F03）、R07×2
（T03-R3-F01/F02）、R01-T08×2（N1/N3 已收口）、总控 seal 工作流×1（AUDIT-SEAL）、演进类/其他×7
（夹具/bench/汇总器/测试卫生/已披露事件等）。
机器汇总复算：`python3 -c "import json,collections;rs=json.load(open('docs/rust-tauri/R01/RISK_REGISTER.json'))['risks'];print(len(rs),collections.Counter(r['status'] for r in rs))"`。

## 6. 问题与阻塞

- RR-T08-F1（新发现，门禁健壮性，MEDIUM）——唯一新发现问题，已登记，建议 R02 硬化。
- 无阻塞执行的事项；A15/A16 待独立验收。
- git status --porcelain（执行末态，见返回总控节）：仅本任务交付与两处注释更正。

## 附：复算命令

```bash
python3 -B docs/rust-tauri/R01/r01_t08_coverage_check.py            # COVERAGE-CLOSED exit 0
python3 -B docs/rust-tauri/R01/r01_t08_coverage_check.py --self-test # 6/6
python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py                # PASS_WITH_CONDITIONS exit 0
python3 -B docs/rust-tauri/R01/r01_t08_gate_check.py --self-test    # 5/5（N1=截图过+接管失败→NO-GO）
python3 -B docs/rust-tauri/R01/r01_t08_isolation_check.py           # ISOLATED exit 0
```
