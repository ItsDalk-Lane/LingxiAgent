# R01-T01｜确定模块和进程所有权 — 执行报告

- 执行者：ZCode:R01-T01-exec-r1（全新独立执行代理；不负责独立验收，不提交/推送）
- 状态：**READY_FOR_REVIEW**（PASS/FAIL 判定归总控另派的独立验收代理）
- 日期：2026-09-25（UTC 日志时间戳为 2026-09-24T21:xxZ）

## 1. 范围

任务书 §4 R01-T01 四步：建立六职责模块（初期最小 crate 承载）、核心/协议不依赖
桌面且宿主只经内核公开接口、画清信任边界（启动者/关闭者/数据写入者）、736 个
F-ID + 69 个 store 逐项分配唯一目标 owner。配套交付 ADR-001、DEPENDENCY_RULES.json、
OWNERSHIP_TARGET.json、生成器、契约校验器（含负向用例）与证据目录。

## 2. 源码基线与环境

- HEAD `328cc8bb5a807bdaad520b459907fb1fa4e10dca`（与冻结基线一致，开工实测
  `git rev-parse HEAD`），分支 `codex/rust-tauri-migration`。
- 开工 `git status --porcelain`：仅 ` M docs/rust-tauri/ORCHESTRATOR_PROGRESS.json`
  （总控账本；本任务未触碰、未提交、未回滚）。
- 环境：macOS 27.0 (26A428) arm64，Darwin 27.0.0；cargo/rustc 1.93.0 (Homebrew，
  254b59607 2026-01-19)；Python 3.14.3；Node v24.16.0。
- 网络：代理 127.0.0.1:7890 不可达；全部 cargo 命令以
  `env -u all_proxy -u ALL_PROXY -u http_proxy -u HTTP_PROXY -u https_proxy -u HTTPS_PROXY`
  + `--offline` 执行。本任务 crate 零外部依赖，未发生任何网络访问。
- 构建隔离：`CARGO_TARGET_DIR=/tmp/r01-t01-target`（仓库外）；仓库内无 target/ 污染。

## 3. 完成项

1. **rust/ 最小 workspace**（真实可编译，零外部依赖）：
   - `rust/Cargo.toml`（workspace，members = 2 crate，edition 2021）
   - `rust/crates/lingxi-protocol/`：不透明 ID 宏（SessionId/RunId/AttemptId/
     ModelCallId/ToolCallId/ResourceId）、`Seq`（wire 十进制字符串，超 JS 安全整数
     不丢精度，含 2^53+7 round-trip 测试）、`RunStatus` 八态词表（terminal 不可逆）、
     `ProtocolError`/`ErrorCode`、`ContractVersions`（R00 基线 1/1/1 锚点）。
   - `rust/crates/lingxi-kernel/`：仅依赖 lingxi-protocol。`RunContext`
     （principal/session/run/attempt/generation，无任何宿主句柄）、`Principal`
     （镜像 R00 认证三主体 + automation）、`RunStateMachine`（02 §4 状态机，
     终态拒绝一切迁移、cancelling 单向、queued 不可跳终态）、`ports.rs`
     （RunStore/ModelPort/ToolPort/CredentialPort，ToolOutcome 含 unknown）。
   - 其余四模块（lingxi-adapters/lingxi-service/lingxi-cli/xtask）按任务书允许
     在 ADR-001 声明归属与建立阶段（R02/R07），tauri-host 独立 manifest（R09）。
2. **DEPENDENCY_RULES.json**：DEP-01..07（协议/内核禁桌面栈传递依赖+源码扫描、
   kernel 禁 adapters、协议禁反向、宿主仅经 service/protocol、RunContext 无
   AppHandle、headless workspace 全禁桌面依赖）+ 六模块注册表 + TB-01..06
   信任边界（启动者/关闭者/写入者逐项）。
3. **OWNERSHIP_TARGET.json**：736 个 F-ID + 69 个 store 逐项唯一 owner（30 个
   受控 owner 词表）、11 条 critical_facts（每条恰好一个 owner，对照 02 §3）、
   worker_restrictions（六类禁止职责）。由 `r01_t01_build_ownership.py` 确定性
   生成（闭表映射，未知标签/store id 即中止，无静默默认），`--check` 支持漂移检测。
4. **契约校验器 `r01_t01_check_ownership.py`**：O1-O7（覆盖/唯一/注册表/关键事实
   单 owner/种类限制/计数一致/生成器漂移）+ D1-D5（cargo metadata 传递依赖、
   允许清单、源码 token 扫描、RunContext 字段扫描、模块注册一致性）+
   `--self-test` 负向电池 N1-N7 + `--emit-negative-fixtures`。
5. **ADR-001-ownership.md**：按 06 格式（问题/基线证据/不可违反契约/候选及实际
   验证/选择与理由/依赖版本/影响/退出条件/受影响任务）。
6. **现役生产代码零改动**：desktop/ server/ core/ lib/ shared/ cli/ hub/ plugins/
   skills2set/ package*.json 等均未触碰（见 §6 最终 git status）。

## 4. 验收场景：命令 / 预期 / 实际 / 退出码

证据文件均在 `artifacts/rust-tauri/R01/T01/`。注意：仓库 `.gitignore` 第 95 行
忽略全部 `*.log`（R00 证据日志同样未被 git 跟踪，保持一致惯例）；日志在本机
工作区可查，`.txt`/`.json` 证据可跟踪。

### R01-A01｜核心不依赖桌面

| 步骤 | 命令 | 预期 | 实际 | 退出码 | 证据 |
|---|---|---|---|---|---|
| 最小 crate 存在 | `ls rust/crates/{lingxi-protocol,lingxi-kernel}` | 两 crate 存在 | 存在 | 0 | rust/crates/ |
| 无桌面依赖构建 | `env -u …_proxy CARGO_TARGET_DIR=/tmp/r01-t01-target cargo build --manifest-path rust/Cargo.toml --workspace --offline` | 编译通过 | 编译通过（0.19s） | 0 | cargo-build-headless.log |
| 单元测试 | `cargo test --workspace --offline` | 全过 | 12/12 过 | 0 | cargo-test-headless.log |
| 依赖树无桌面栈 | `cargo metadata --offline` + 结构化检查 + `r01_t01_check_ownership.py` D1 | 无 tauri/electron | 解析图仅 2 包，forbidden hits NONE；D1 PASS | 0 | cargo-metadata.json、dependency-tree-evidence.log、check-ownership-positive-and-selftest.log |

注：metadata 原文 grep 到 'tauri'/'electron' 各 2 处，已核实为两个 crate
`description` 字段里陈述禁止条款的英文句子，非依赖项（证据日志内有 NOTE 说明）。

### R01-A02｜双负责人被检出

| 步骤 | 命令 | 预期 | 实际 | 退出码 | 证据 |
|---|---|---|---|---|---|
| 构造双 owner 目标表 | `r01_t01_check_ownership.py --emit-negative-fixtures artifacts/rust-tauri/R01/T01/negative` | 生成 N1/N2 负向夹具 | 已生成 | 0 | negative/OWNERSHIP_TARGET.n1-dual-owner.json 等 |
| 执行契约校验（双 owner） | `r01_t01_check_ownership.py --ownership …/n1-dual-owner.json --skip-deps --skip-drift` | 明确拒绝 | `FAIL [O4] critical fact run_terminal_state has 2 owners; exactly one owner is required` | **1** | check-ownership-negative-dual-owner.log |
| "二者同步"绕过变体 | 同上，`n2-sync-bypass.json` | 明确拒绝 | `FAIL [O4] … bypass fields ['reconciliation','secondary_owner'] … dual-write/sync is not an accepted escape hatch` | **1** | check-ownership-negative-sync-bypass.log |
| 全量负向电池 | `r01_t01_check_ownership.py --self-test` | N1-N7 全部按预期拒绝且正向全过 | N1-N7 REJECTED as required；正向 O1-O7/D1-D5 PASS | 0 | check-ownership-positive-and-selftest.log |

### 辅助检查

| 命令 | 结果 | 退出码 | 证据 |
|---|---|---|---|
| `r01_t01_build_ownership.py --check`（漂移） | UP_TO_DATE | 0 | generator-drift-and-fmt.log |
| `cargo fmt --all -- --check` | 无 diff | 0 | cargo-fmt-check.log |

## 5. 关键设计决定

1. 本阶段只建 lingxi-protocol + lingxi-kernel 两个真实 crate（任务书明示允许），
   其余四模块在 ADR 声明建立阶段；拒绝"空 crate 占位"。
2. R00 复合 target_owner 标签（"X＋Y＋Z"）解析为单一权威 owner + basis 注释；
   React/宿主/worker 记为消费方/执行体，永不做 owner（校验器 O6 机械执法）。
3. store 目标写进程：63 项 rust-service、5 项 tauri-host 壳自态、1 项
   build-release 工具；与 R00 现役单写者方向一致。
4. worker_restrictions 机器化：worker/ui 类 owner 不得拥有任何 F-ID、store 或
   critical fact；critical fact 条目只允许 {fact_id, description, owners,
   contract_ref} 四个键，任何 auxiliary owner 字段（secondary_owner/co_owners/
   reconciliation/…）即拒绝——这是"不得以二者同步绕过"的机械实现。
5. 信任边界 TB-01..06 落进 DEPENDENCY_RULES.json（机器可读）并在 ADR §5 展开。

## 6. 未执行项与限制

- 未跑全量 `npm test`（任务范围外；已知预存 4 个审计封印用例 FAIL 属治理门禁，
  本任务未触碰 .sync-audit/、未扩白名单；round2/round3 测试副作用文件未被触发，
  因未运行全量测试）。
- 未建 lingxi-adapters/lingxi-service/lingxi-cli/xtask crate（按计划归 R02/R07）；
  未创建 rust-toolchain.toml 与第三方依赖锁定（R01-T03 范围）。
- 协议 wire schema（serde/JSON Schema/TS 生成）未做（R01-T02 范围）；本阶段只
  固定身份词表、状态词表与错误信封。
- 跨平台（macOS x64/Windows/Linux）未验证：本任务全部证据在本机 macOS arm64；
  最小 crate 无平台相关代码，但其他平台编译证明归 R01-T03/R09/R10。
- 校验器对"计划态"模块（adapters/service/cli/xtask/tauri-host）的 DEP 规则在
  模块建立前不生效（status=planned 跳过），建立后自动纳入执法。

## 7. 问题与阻塞

- 无阻塞。过程性发现：N7 负向用例首次运行时 D3 先于 D4 触发（同一违规的两条
  规则），已将 N7 期望改为接受 (D3, D4) 之一并注释；另修复了 outside-repo 路径
  在违规消息中的 relative_to 崩溃（不影响判定逻辑，仅消息格式化）。
- 已知预存失败（按红线不追修）：审计封印家族 4 用例、r00_t02/r00_t03 校验器
  STALE 分类——本任务未运行、未改动相关文件。
- **R1 验收后续（2026-09-25）**：独立验收 R1（R01-T01_REVIEW_R1.md）判 FAIL，
  指出本文 §3/§4 中 "DEP-01..07（存在模块部分）全部 PASS" 的 DEP-03/D2 部分
  当时未被真实执法（transitive_deps() 连字符/下划线命名归一化缺陷，传递闭包在
  连字符内部包处断裂、D2 白名单交集恒空），另有 authoritative store 写者与关键
  事实词表两处加固缺口（F2/F3）。修复与复证记录见 R01-T01_REPAIR_R1.md：
  校验器改为以包 ID 建图、模式匹配双侧归一化、D5 对偶检查（planned 模块实体化
  即违例）、新增 O8 与 critical_facts 精确集合锁定，负向电池扩为 N1-N12。

## 8. 回退

本任务全部为新增隔离文件（rust/、docs/rust-tauri/R01/、artifacts/rust-tauri/R01/），
删除这三个新增路径即完全回退；无现役代码、无配置、无数据改动，无共享状态副作用。
/tmp/r01-t01-target 为临时构建目录，可随时删除。

## 9. 最终 git status --porcelain

```
 M docs/rust-tauri/ORCHESTRATOR_PROGRESS.json
?? artifacts/rust-tauri/R01/
?? docs/rust-tauri/R01/
?? rust/
```

（ORCHESTRATOR_PROGRESS.json 为总控账本预存修改，本任务未触碰；其余三项为本任务新增。）
