# ADR-001｜模块与进程所有权（R01-T01）

- 状态：READY_FOR_REVIEW（执行者结论；独立验收归总控另派代理）
- 日期：2026-09-25
- 执行基线：分支 `codex/rust-tauri-migration`，HEAD `328cc8bb5a807bdaad520b459907fb1fa4e10dca`（R00 正式封印 C4）
- 作者：ZCode:R01-T01-exec-r1

## 1. 问题与用户目标

迁移到「Rust 内核 + Tauri 宿主」后，每项核心事实必须有且只有一个负责人：
运行终态、批准决定、上下文组装、模型路由、会话身份、存储写入。如果这一层不冻结，
后续每个阶段都会靠"两边都写、事后同步"绕开单写者契约（任务书 02 §3/§8 明确禁止）。
本 ADR 冻结：六个职责模块的归属与建立顺序、依赖方向、信任边界、736 个 F-ID 与
69 个 store 的逐项目标 owner，以及配套机器校验。

## 2. 基线证据（观察事实）

- R00 交接：`docs/rust-tauri/R00/R00_HANDOFF.json`（status READY_FOR_REVIEW，
  `allowed_next_scope` 授权 R01 以其清单为输入）。
- R00-T02：`FEATURE_INVENTORY.json` 736 个生产叶子（24 域，sha256
  `48141268…` 见交接哈希表），每叶含 `target_owner` 标签（23 种取值，来自
  03 矩阵域级标签，多表面复合表述如 "Rust Run/Session＋React"）。
- R00-T04：`STORES.json` 69 个注册 store（42 authoritative / 13 rebuildable_cache /
  14 adjacent_compatible），现写者分布：server 写 62 项、desktop 仅写自身壳自态
  5 项 + OTA 列车等；`OWNERSHIP_CURRENT.md` §1-3 锚定认证三主体、路由授权唯一入口、
  单写者现状与 R-T04-01..07 跨进程风险。
- R00-T03：`PI_REPLACEMENT_MATRIX.json` PI-01..14 能力盘点（Agent loop、上下文、
  工具调度等须迁入 Rust 内核；worker 反例清单 19 条）。
- 现役源码锚点抽查（本 HEAD 逐一 `test -f` 通过）：`server/index.ts`、
  `core/tool-invocation-gateway.ts`、`shared/tool-categories.ts`、
  `core/model-operation-resolver.ts`、`server/assistant-event-normalizer.ts`、
  `core/engine.ts`、`core/server-auth.ts`、`lib/pi-sdk/index.ts`、
  `shared/persistence/store-registry.ts`、`shared/contract-versions.json`。
- 环境：macOS 27.0 arm64；cargo/rustc 1.93.0（Homebrew）；Python 3.14.3。
  网络代理（127.0.0.1:7890）不可达，全部 cargo 命令以 `env -u …_proxy` 直连
  并 `--offline` 执行；最小 crate 零外部依赖，无需联网。

## 3. 不可违反契约

1. 核心/协议（lingxi-protocol、lingxi-kernel）不依赖 tauri/electron 及任何桌面/WebView
   栈；kernel 不反向依赖 adapters（ports 在 kernel，实现由 service 注入）。
2. 宿主与传输只依赖内核公开接口；`RunContext` 永远不含 AppHandle/窗口句柄。
3. 每项核心事实恰好一个 owner；不接受"双写后同步"作为合法形态（负向用例 N1/N2
   证明该绕过被明确拒绝）。
4. 外围 worker 不拥有运行终态、审批决定、上下文组装、模型循环，不拥有任何
   store 写权与任何 F-ID；需要模型时经受控 Rust 回调（kernel.model-gateway）。
5. 业务权威 store 的目标写进程唯一为 rust-service；tauri-host 只写自身壳自态；
   旧 Electron 与新 Rust 不得双写同一权威数据（迁移期由 epoch 闸/分离数据根保证，
   策略归 R01-T07）。
6. R01 原型不是生产入口：本任务不改任何现役生产代码，生产默认链保持原样。

## 4. 候选及实际验证

| 候选 | 验证 | 结论 |
|---|---|---|
| A. 六模块一次全建空 crate 骨架 | 空 crate 无真实代码即"占位实现"，违反禁止替代条款 | 否决 |
| B. 本阶段只建真实可编译的 lingxi-protocol + lingxi-kernel，其余四模块在 ADR 声明归属与建立阶段 | cargo build/test/metadata 离线通过；契约校验器以 cargo metadata 机械执法 | **采纳**（任务书明示允许"初期少量 crate 承载"） |
| C. 一功能一服务拆分 | 02 §2 禁止扩成微服务 | 否决 |
| D. owner 用 R00 复合标签原样保留（如 "X＋Y＋Z"） | 复合标签无法回答"唯一负责人"，负向场景无法机械判定 | 否决：解析为单一权威 + 消费/执行方注释 |

实际验证记录（证据目录 `artifacts/rust-tauri/R01/T01/`）：

- `cargo build --workspace --offline`（CARGO_TARGET_DIR=/tmp/r01-t01-target）退出码 0。
- `cargo test --workspace --offline`：12 个单元测试全部通过
  （协议 5：不透明 ID/Seq 十进制字符串精度/状态终态/版本锚点；内核 7：状态机
  合法与非法迁移、终态不可逆、取消单向、RunContext 形状）。
- `cargo metadata --offline`：workspace 两个成员，解析图仅含
  lingxi-kernel → lingxi-protocol 一条内部依赖，无 tauri/electron/tao/wry/webkit2gtk。
- 契约校验器正向：O1-O7、D1-D5、DEP-01..07（存在模块部分）全部 PASS。
- 负向电池 N1-N7 全部按预期拒绝（双 owner、"二者同步"绕过、覆盖缺失、
  worker 拥有功能/关键事实、kernel→tauri 传递依赖、RunContext 携带 AppHandle）。

## 5. 选择与理由

**模块归属（建立阶段）**：lingxi-protocol（R01-T01 已建）／lingxi-kernel（R01-T01
已建）／lingxi-adapters（R02）／lingxi-service（R02）／lingxi-cli（R07）／
xtask（R02）。tauri-host 为独立 manifest（desktop/src-tauri，R09），不进 headless
workspace，避免 `cargo test --workspace` 被迫安装 WebView 依赖。

**owner 控制词表**（30 个，`OWNERSHIP_TARGET.json owner_registry`）：内核域组件
11 个（run-supervisor/session/context/tool-gateway/policy/scheduler/memory/
knowledge/resource/model-gateway/skill），adapters 5，service 4，cli/xtask 各 1，
host 2，ui 1（永不作 owner），worker 4（永不作 owner），build 1。

**F-ID 分配**：以 R00 逐叶 `target_owner` 标签为输入，闭表映射到单一 owner
（生成器 `r01_t01_build_ownership.py`，未知标签即中止，无静默默认）。多表面标签
解析规则：业务权威归 Rust 侧组件；React/宿主/worker 记为消费方或执行体并写入 basis。
示例：D15 系统操作 owner=tauri-host.os-helper（执行），批准决定仍在 kernel.policy
（critical_facts 单行锁定）；D14 浏览器 owner=adapters.browser（BrowserPort 实现），
独立宿主进程为 worker.browser-engine 执行体。

**store 分配**：69 个显式逐条映射（闭表，缺/冗余均中止）。目标写进程：63 项
rust-service，5 项 tauri-host 壳自态，1 项（signed-artifacts）build-release
工具按契约固定序列应用；与现状（server 单写者 + desktop 仅壳态）方向一致。
authoritative 分类 store 的 owner 必须是 core/service/adapters 类且
target_writer_process 必须为 rust-service，由校验器 O8 机械执法。

**关键事实表**（11 条，对照 02 §3 逐行）：authenticated_principal→service.auth；
session_identity_branch_messages→kernel.session；run_terminal_state→
kernel.run-supervisor；attempt_generation_fence→kernel.run-supervisor；
tool_availability→kernel.tool-gateway；params_approval_resource_scope→
kernel.policy；model_credential_selection→kernel.model-gateway；
real_files_and_authorization→kernel.resource；history_realtime_projection_semantics→
kernel.session；usage_causal_trace→kernel.model-gateway；
scheduler_trigger_dedup→kernel.scheduler。

**信任边界**（`DEPENDENCY_RULES.json trust_boundaries`，TB-01..06）：rust-service
（启动者：tauri-host 托管或用户直接 headless 启动；关闭者：启动者；写者：全部
authoritative store）／tauri-host（写：仅壳自态）／旧 Electron 壳（现役冻结，
迁移期不双写）／外围 worker（service 按租约拉起与回收；写：仅 scratch）／
外部服务（完全不可信，经 adapters 校验）／react-ui（不持久化任何东西）。

## 6. 依赖版本

- 本阶段 rust/ 两个 crate 零外部依赖（离线可编译）；Cargo.lock 由构建生成，
  第三方依赖锁定与 rust-toolchain.toml 归 R01-T03。
- 工具链实测：cargo 1.93.0 / rustc 1.93.0 (254b59607 2026-01-19)，edition 2021。
- 现役 Node 侧契约版本（R00 基线，未升级）：PRELOAD_API_VERSION 1 /
  SERVER_PROTOCOL_VERSION 1 / DATA_EPOCH 1（shared/contract-versions.json）。

## 7. 性能/安全/兼容影响

- 性能：本任务无可观测运行时性能影响（不接线生产）。单写者与明确边界是 R02+
  取消/恢复性能门槛（05 §6）的前提。
- 安全：信任边界把凭证材料限定在 service 内；worker 无凭证直读权；负向用例
  证明双负责人与"同步绕过"被机械拒绝。Bridge 平台账号体系仍是外部信任假设
  （R00 R-T04-05），不因本 ADR 宣称已证实安全。
- 兼容：旧 ID 保留（协议层不透明 ID 不重新编号）；旧状态名由传输层兼容映射；
  旧 JSONL 只读导入与 epoch 拒写策略归 R01-T07，本 ADR 不预决。

## 8. 退出条件

- 本 ADR 被替代或修订的条件：新建立 crate 的真实职责与本表冲突（须先改
  DEPENDENCY_RULES/OWNERSHIP_TARGET 并通过校验器）、R01-T08 关卡审查要求调整、
  或某必需能力被证明无实现路径（此时按 06 停止相关替换而非静默缩范围）。
- 失效信号：校验器任何 O/D 检查失败、OWNERSHIP_TARGET 与 R00 清单漂移
  （--check 退出 1）、或 R00 清单本身被授权更新（重跑生成器即可重算）。
- 关键事实词表治理：`critical_facts` 的 11 条是锁定精确集合（校验器对词表外
  条目直接拒绝，O4）；新增或改名关键事实属治理变更，须先修订本 ADR 并经
  关卡审查，不得以新 fact_id 夹带第二语义负责人。
- 依赖图归一约定：cargo metadata 的 `deps[].name` 会把连字符包名改写成
  下划线 crate 标识符（`lingxi-adapters`→`lingxi_adapters`）；校验器以包 ID
  建传递闭包、模式匹配双侧归一化，并对 status=planned 模块做对偶检查
  （已落盘/已进 workspace 即违例），防止"计划态"模块被静默实体化后绕过
  尚未生效的规则。

## 9. 受影响任务/测试

- 直接消费方：R01-T02（协议类型源扩展 lingxi-protocol）、R01-T03（锁定依赖与
  rust-toolchain.toml）、R01-T07（存储切换 ADR 以本表 store owner 为输入）、
  R01-T08（A16 双向覆盖检查复用 OWNERSHIP_TARGET）。
- 下游阶段：R02 建 lingxi-adapters/lingxi-service/xtask 时须保持 DEP-01..07；
  R07 建 worker 账本时受 worker_restrictions 约束（含 R00 交接的
  T03-R3-F01/F02 登记完整性后续项）。
- 测试：校验器 `r01_t01_check_ownership.py`（含 --self-test 负向电池 N1-N12）为
  本 ADR 的常驻门禁；R02 起应挂入 xtask check-boundaries（R02 范围，本任务不建）。
