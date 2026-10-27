# R07 → R08 独立准入复核

审查日期：2026-10-21（Asia/Shanghai）。审查者：独立子代理 `r07_admission_review`，未参与 R07 实现。本次只审查前置准入，不承担 R08 实现或其最终独审。

## 结论

**FAIL：R07 最终候选不能仅凭 gate-11 的 408 叶 PASS 放行 R08 全业务依赖。** 回执真实存在且所测实现与当前提交相符，但存在“用旧工具的局部单测证明 Rust 真实接线”和“测试通过与原断言不是同一行为”两类关键缺口。下列 F01/F02 必须关闭，或者获得明确且不弱化原断言的责任裁决后，才能消费受影响接口。

R08 仍可继续无依赖准备：启动隔离、显式数据根/实例空间修复、原断言地图、存储类别与迁移设计、固定夹具及验收器拒绝空证据的准备。不得据此宣称全产品 Rust API 已交接、功能 A/B 已通过或 R08 已完成。**本审查不重开已接受 R06，不恢复已退役 Electron WebContentsView。**

## 候选、原书与受测版本绑定

- 分支 `codex/rust-tauri-migration`，当前提交 `86840d1700397d3dd620a41f2b884df4cddd5cc2`。
- 已读本地 AGENTS、Rust/Tauri 原书 00—05、R07/R08、91；遵从用户本轮隔离、安全、27 项后续份额与浏览器裁决。未混入旧 P00/Pi 保留路线。
- gate-11：`artifacts/rust-tauri/R07/T12/verify-r07-closeout-11/verify-stage-result.json`，SHA-256 `684da5b626951f171fc5a1ec4f380386fd4dbad25a1d98dbc00d52f6356ec607`。
- gate-11 的 `testedSha=bfe32d107fb60852712b897a25403d7f092671b3`、`worktreeDirty=true` 是真实受测坐标。不能把其基点误当全部受测实现。前后工作树清单逐字相同，95,344 项，工作树摘要 `811d75e92f7b94885b0c8e42888ac7db5bf616e2d868880cee70ad3f5fd28a9b`；清单自身摘要 `c81efa7726221351f6626fd8df137f705383033d46d6bd154d6150d33943d095` 与回执相符。
- 在主代理 R08 修改前，逐文件重算了清单中仍被当前 Git 跟踪的 92,930 个文件。仅五件 R07 收尾文档改变：`R07_HANDOFF.json`、`R07_LEAF_REMAINING.json/.md`、`R07_PROGRESS.json`、`R07_REPORT.md`。R07 被测实现未发现新增源码差异。所有 runnerSourceBinding 列出的编译源摘要与磁盘一致。
- 另有大量既有、未跟踪 R07 历史证据，本审查未清理、覆盖或改写。主代理随后进行的 R08 启动隔离修改属于新候选，需独立绑定其验证，不能沿用上述全树摘要。

## 回执核验与实际复跑

机器回执核验：57/57 命令、24/24 基础场景、408/408 补充叶、2/2 扩展项 PASS；35/35 移交视图及 43/43 前阶段递延视图通过。57 命令登记的证据均存在且非空。54 个 SUITES 回执退出码为 0，凡回执带 `logSha256` 均重新核算一致。`LEAF_CASES/leaf-cases.json` 自身摘要正确，包含 54 套件的 1,055 个案例，全部标记通过。**这些机械核验通过，不等于逐条原断言语义已被证明。**

实际独立抽样重跑：

```text
cargo test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r07_t12_closure
```

在独立合成目录 `/private/tmp/r08-r07-admission-ej64xgem` 中设置 LINGXI_HOME/TMPDIR/TMP/TEMP，未改全局 HOME。退出 0，3/3 通过：

- `r07_t12_a23_runtime_route_matrix`
- `r07_t12_a24_closed_loop_without_legacy_kernel`
- `r07_t12_kernel_detector_catches_a_real_node_process`

原始日志已固化为 `artifacts/rust-tauri/R08/r07-admission-review-01/closure-1.93.log`，SHA-256 `9af2098ea1129f1ad9d6dcd8e5f5fc5407fa8cad4d814f16c839d7d00297c2b0`，历时 35.66 秒。此复跑证明已被这些测试覆盖的 Rust 链确实运行，未推翻其结果；该套件不执行下面五个缺口工具。

环境限定：此次附加复跑使用 PATH 中的 Homebrew cargo/rustc 1.93.0。仓库要求 1.98.1，`/Users/study_superior/.cargo/bin/cargo` 与 `rustc` 已实际核对为 1.98.1；因此本次 1.93.0 结果只作附加复现，不冒充锁定工具链的正式 R08 验收。

## F01｜关键：旧工具单测不能证明 Rust 执行面已接入

涉及五个已被 R07 按 `full_original_behavior` 记 PASS 的原叶：

| 工具 | 叶 ID | 原责任 |
|---|---|---|
| run_tools | R00-T02-LA-0818574ABD43 | R04/R07 |
| ast_grep | R00-T02-LA-196D50D8DD6E | R04/R07 |
| lsp | R00-T02-LA-2D896560381E | R04/R07 |
| run_code | R00-T02-LA-C88F29B5114A | R04/R07 |
| security_scan | R00-T02-LA-CD5D7FC02D8E | R04/R07 |

`R07_LEAF_BINDINGS.json` 对这组统一声称“工具实现在 lib/ 共享层，Rust 服务与旧内核共用同一工具面”。实际证据生产者 `scripts/rust-tauri/r07_ui_cases.py` 直接执行旧 `lib/tools`、`lib/sandbox` 与 `tests/lsp-tool` 的 Vitest 测试；这些用例没有进入 Rust 服务的正式工具登记/网关。

已沿真实链核对：`ServiceState::bootstrap_with_deps` 从空的正式 `ToolRegistry` 装配工具；`toolcatalog_bridge.rs:168` 的 `mcp_search_tools` 只搜索该登记表，`:176` 的 `mcp_describe_tool` 同样从该表 resolve；`CatalogDefinitions::normalize` 中的 `mcp_call` 仍 resolve 同一个登记表并重走授权。动态目录不会自动加载旧 TS 工具。`mcp_settings` 的内置延迟设置只调整现有登记工具是否常驻；外装 MCP 只登记用户明确配置并已连接的目标。`workers` 入口是明确配置的单项工具注册，不是所有旧 lib 工具的默认桥接。

这五工具在 Rust 中能找到的同名项目是可选配置名称、提示词或历史/测试声明，不能代替实际执行注册。独立运行已证明：**与 A24 相同的干净合成配置下，这五个工具全部从正式登记表 resolve 失败，返回 TargetNotFound。** 不是测试依赖缺失或编译失败。外部临时验收宿主复用 A24 的真实模型面、凭证、上下文和工作区，不注入假工具网关/假执行器，不更改仓库实现。它只把 A24 的后半段替换为逐工具读取正式登记表并断言存在；动态目录正是读取这张表，因此已核到动态/外围入口边界，不能用“可能按需加载”抵消缺口。

运行证据保留于 `/private/tmp/r08-r07-registry-review-midvkf04`：完整 `Cargo.toml`、独立锁文件、`tests/admission.rs`、`registry.log`、`receipt.json`。执行环境同为上述 Homebrew 1.93.0，macOS arm64；数据根为该临时包的 home，TMPDIR 为其 tmp；未改变全局 HOME。外部临时包的独立锁不改仓库锁；只使用 offline 解析，复用当前真实服务库。

```text
cargo test --manifest-path /private/tmp/r08-r07-registry-review-midvkf04/Cargo.toml --offline --test admission r07_t12_a24_closed_loop_without_legacy_kernel -- --exact --nocapture
```

退出 **101**，实际运行 1 项测试后断言失败，历时 204.99 秒（含独立包编译）。日志 SHA-256 `495d0fda1903f787fcd2d9fb503d02153a368c845d05d102d6ecb58181fef4f6`。五条实际输出为：

```text
ADMISSION_TOOL_MISSING run_tools TargetNotFound
ADMISSION_TOOL_MISSING run_code TargetNotFound
ADMISSION_TOOL_MISSING ast_grep TargetNotFound
ADMISSION_TOOL_MISSING lsp TargetNotFound
ADMISSION_TOOL_MISSING security_scan TargetNotFound
```

此初轮红证据是独立宿主对真实默认装配的反例，不冒充原 gate-11 失败，也不以工具链不同掩盖其限制。初轮只写本报告，并把临时包位置交给主代理；随后获准固化与锁定工具链复验，结果见下文。正式修复复验仍需从模型实际入口触发完整动作/授权/存储链。

补查开关语义：`shared/tool-categories.ts` 的默认关闭列表只有 `workflow`，这五工具不是默认禁用；Rust `settings_available_tools` 会把这些可选名称加入设置页，因此“设置里列出该名称”同样不能证明它能实际执行。

### 锁定 1.98.1 的同断言复验与证据固化

收到主代理明确固化授权后，新建 `artifacts/rust-tauri/R08/r07-admission-review-01/`，只保存复验包的 Cargo.toml/Cargo.lock、测试源码、回执和日志；**未复制任何 home、凭证或数据库**。原 1.93 失败原样保存在 `homebrew-1.93/`，原 closure 日志独立保留。新 1.98.1 证据在 `rustup-1.98.1/`。

新建独立合成根 `/private/tmp/r08-r07-registry-review-198-4xn5v034`，原样复制同一验收宿主；两轮 `tests/admission.rs` 摘要均为 `4a03dea3738dfd06eb01add94bef27a2ab4ea4df03c196589ee154af31f63aa4`，未改断言、未放宽测试。实际命令：

```text
/Users/study_superior/.cargo/bin/rustup run 1.98.1 cargo test --manifest-path /private/tmp/r08-r07-registry-review-198-4xn5v034/Cargo.toml --offline --test admission r07_t12_a24_closed_loop_without_legacy_kernel -- --exact --nocapture
```

实际环境为 macOS 27.0.1 arm64，cargo `1.98.1 (797e8a9bc 2026-08-05)`，rustc `1.98.1 (48a229cea 2026-09-01)`；合成 home/tmp 均在新根下，未改全局 HOME，完整显式环境在新回执中。结果依然是 **退出 101，1 个测试运行后断言失败，五工具全部 TargetNotFound**；历时 110.661 秒（含编译）。这不是编译失败或环境 BLOCKED。

- 新日志：`rustup-1.98.1/registry.log`，SHA-256 `6281edb3ac90d25fc0e51e02fca5cafe2aca1cb6e7048d980bdd997f3fca17c4`。
- 新回执：`rustup-1.98.1/receipt.json`，SHA-256 `c8a498b8eec10e7c45e7d69b5212dc8e6b55d45c5be710822e15da7183aea3af`。
- 受测候选仍为 `86840d1700397d3dd620a41f2b884df4cddd5cc2`。受测四个真实库（service/kernel/adapters/protocol）的 src、各 Cargo.toml、workspace Cargo.toml/Cargo.lock 与 rust-toolchain.toml 共 321 文件逐件摘要记于回执；运行前后同为 `dda9c29e2523dfc95ba396b9c098fc7f586be48bb61ddd760c31bddd862dc39e`，`candidateSourceStable=true`。这是明确范围的真实服务源码绑定，不把同时进行的 R08 桌面改动纳入这次服务反例的覆盖声明。

固化后的可重复方式：把 `rustup-1.98.1/` 的 Cargo.toml/Cargo.lock/tests 复制到新的外部临时目录，给 LINGXI_HOME/TMPDIR 指向其全新 home/tmp，再用上面的锁定命令替换 manifest 路径运行。不要把证据目录当数据根；Cargo.toml 的真实源码依赖路径由原候选回执绑定，换 checkout 时需明确记录路径与源码摘要。

R08 影响：T01/A01 的多工具兼容以及 T07/A13 的全功能 A/B 都需要真正的工具执行面；若缺失，旧界面入口仍可见但无法使用，不能由前端适配修成“隐藏/禁用”。解除条件：提供/修复真实默认注册或既定外围执行链，以合成模型从实际入口逐工具调用，经过统一授权并检查真实效果/拒绝/取消/持久记录，再绑定相应原断言。若作为 R07 前置修复处理，不应挪作 R09 或重开 R06。此锁定工具链复验仍为 FAIL，未因固化证据变成通过；F01/F02 后续关闭责任继续保留。

## F02｜关键：逐断言数量匹配，但实际绑定了不同的行为

这是同一证据映射根因，应按组核对全部类似绑定，不能只补本报告点名的个例。`r07_apply_leaf_bindings.py` 的组数检查、叶内不重复检查都成立；它们不检查每个测试究竟断言了什么。具体可复核反例如下：

| 原叶与断言 | 绑定的案例 | 实际测试证明什么 | 未证明的要求 |
|---|---|---|---|
| run_code：重启后旧变量消失 | `缺语言：RUN_CODE_LANGUAGE_MISSING + 安装指引` | 工具在无 python3 时返回缺语言错误（run-code-tool.test.ts:90） | 未重启，未检验变量消失 |
| run_code：取消清理内核 | `内核死亡：如实报告 exit 与部分输出，指路 restart` | 测试自己调用假 manager.kill，再检查下一次报 dead（:99） | 未发取消、未检验真实进程清理 |
| lsp：不可写时只预览不落盘 | `rename：prepareRename 验证 → 编辑计算 → 写回` | 明确期待 applied=true、写入一份文件（tests/lsp-tool.test.ts:96） | 与“只预览不落盘”要求相反 |
| run_tools：每个子调用独立权限判定 | `权限契约：execute 动作 + routine 档，能力名对齐工具名` | 检查外层工具的静态 sessionPermission（ptc-tool.test.ts:49） | 未执行子调用、未独立拒绝越权子调用 |
| ast_grep：命中结构而非同文字符串 | `content 模式分页 + 诚实 footer` | `execFile` 被 mock 成固定匹配 JSON，检验分页文字（ast-tools.test.ts:69） | 未实际解析结构，错误字符串匹配同样能过 |
| security_scan：无越权读取 | `changes 模式：非 git 目录如实说明并建议 path 模式` | 非 Git 提示 | 未证明授权外读取被拒 |

补充同因界面例：`R00-T02-LA-000E6E1301C0` 的原断言“撤销设备/凭证后条目移除、重新加载失败仍报错、失败保留条目”只绑定 `revokes individual credentials without requiring whole-device revocation`。该测试（AccessTab.test.tsx:490）返回不变的 pairedSummary，并只检查 POST 被调用，未断言条目移除或失败路径。生成凭证/连接远端/保存资料等原条款也含空态、错误与刷新失败要求，绑定的单一成功路径没有逐项覆盖。

R08 影响：无法把 R07 的 408 个 PASS 当作全业务/权限/取消语义可信输入，尤其不能由绿色总数替代 T01/T07 的实际接线验收。解除条件：保留 R00 原断言与历史 PASS 回执；把当前不足明确登记为 FAIL/BLOCKED，修复逐原断言到具名行为证据的映射。允许一个断言绑定多条必要测试，不用“叶内案例不重复”强行选择不相关测试。判据变化需独审；不改原要求来适应现有案例。

## 27 项后续份额与浏览器裁决

从 gate-11/R07 地图按 `laterShare 非空 && r00ExecutionStageIds 含 R08` 得到正好 27 项；全部已包含于 R00 `FEATURE_STAGE_ACCEPTANCE.json` 的 126 个 R08 补充叶，不另加计数。这 27 项的 R08 客户端显示/交互/实际恢复责任继续有效，R07 服务测试不能代领。

旧浏览器宿主退役依据：`docs/rust-tauri/R07/T10_LEAF_EVIDENCE_DRAFT.json` 的 retiredByDesign 记录用户 2026-10-19 决定，测试 `r06_session_map_browser_completion::current_browser_host_restores_target_preserves_old_view_and_reports_disconnect`，对应 R08 叶 `R00-T02-LA-FBDEEDAC0B11`，明确要求在 R08 重建替代宿主覆盖。本审查沿用此决定，不把旧测试当需恢复功能。

其他仅属于 R09 的 viewer/窗口份额保留其原阶段集合，不拉回 R08；它们不能抵消上面的 R08 替代宿主覆盖责任。

## 未执行与下一步边界

- 未重跑 R07 全量门禁或所有 408 叶；机械核验全部回执、源码抽查与具名复跑已如实列出。
- 未读取真实用户数据库/附件，未启动生产默认，未提交/推送/发布，未更改 R07 原证据或原任务书。
- 未验证真实账户、付费供应商、非 macOS 或正式安装包。这些按现有合法后续责任保留，不据此新增本次准入阻断。
- 在 F01/F02 关闭前，R08 的全业务依赖验收受阻；隔离与地图/迁移准备可继续。有效接口应逐项核对实际链路，而不是把整份旧 PASS 当豁免。
