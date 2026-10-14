# R06 收口独立 Review（RV3）— R06-AR-F06 / R06-AR-F07 修复真实性

- 冻结候选：`844a10b2b83a98bb909a9f83bb89f140021d5adc`（fix(rust-tauri): R06 closeout candidate）
- 审查区：`git worktree /tmp/lingxi-rv3`（HEAD 验证 = 844a10b2b；tracked diff 全程为空，仅本审查新增 2 个未跟踪探针，见附录）
- 工具链：`/Users/study_superior/.cargo/bin/cargo test --manifest-path /tmp/lingxi-rv3/rust/Cargo.toml --locked`，`CARGO_TARGET_DIR=/tmp/rv3-target`（repo 外，影响见环境注记），合成环境 `/tmp/rv3-env`
- Reviewer：全新独立（未参与实现/Repair/前轮 Review）；本报告只裁 F06 与 F07 的修复真实性，不关闭 R06 阶段。

## VERDICT

| 项 | 判决 |
| --- | --- |
| **R06-AR-F06（YAML 解析）** | **PASS**（五条 remaining_scope 义务全部满足；R1/R2/R3 全部历史 blocker 亲验为已修） |
| **R06-AR-F07（工具目录）** | **PASS**（五条 required_steps 义务全部满足；BE-01/BE-02 与目录四态经真实二进制/wire 亲验） |

无 BLOCKED 项：t08 初跑 7 红经定位为本审查外置 target 目录造成的测试环境耦合（产品产品目录祖先回溯失败），设置 `LINGXI_PRODUCT_DIR` 后 13/13 全绿；根因与修复路径见「环境注记」，非产品缺陷、非外部不可抗条件。

## 亲跑证据（全部本 Reviewer 本机实际执行）

| 套件 | 结果 |
| --- | --- |
| `-p lingxi-adapters --lib` | 123 passed / 0 failed |
| `-p lingxi-adapters --test r06_ar_f06_d2_replay_probe` | 1/1（R03 大写基数 + 非法 !!int/!!null 全族，含 9 个大写前缀 plain 保持字符串、memory-only patch 不变型） |
| `-p lingxi-adapters --test r06_ar_f06_config_preservation` | 12/12（复合键、字面 merge、未知 tag、真 merge 优先级、别名键身份、资源预算、未知字段保真） |
| `-p lingxi-service --test r06_ar_f06_config` | 9/9（四 C gather 等价、真实记忆工具复查、真实 Gateway 安装前置拒绝——含 R03-F01/F02 在 install 入口的重放） |
| `-p lingxi-service --test r06_ar_f06_binary` | 4/4（**真实二进制** + 认证 HTTP + 真实 wire + runs.db：四 C 10 例、P01 复合键 16 例 fail-closed、P02 字面键/标签 33 例、P03 删除收据） |
| `-p lingxi-service --test r06_ar_f07_catalog` | 8/8（C01 目录四态、C02 发现→说明→调度、C03 五协议全描述、C04 撤权与历史对、按需调用需本轮说明+当前代次） |
| `-p lingxi-service --test r06_t08_behavior_eval` | 13/13（含 BE-01/BE-02；需 `LINGXI_PRODUCT_DIR`，见环境注记） |
| 本审查未跟踪探针 `zz_rv3_reviewer_probe`（真实二进制 HTTP 级，12 例） | 1/1 |
| 本审查未跟踪探针 `zz_rv3_edge_probe`（!!float 整数 round-trip 微差分） | 1/1 |
| 现役 oracle：js-yaml 4.3.1 DEFAULT_SCHEMA（主仓 node_modules，本人独立运行 node 脚本） | 全部争议字面量表与实现一致 |

## F06 逐义务核验

1. **单一真实解析入口**：`rust/crates/lingxi-adapters/src/storage/agent_config.rs` 的 `AgentConfigDocument::parse` —— granit-parser(=1.3.0) 事件流 + serde_yaml_ng 0.10（merge/Value）+ serde-saphyr(=1.3.0，strict_booleans/reject_non_finite_typeless_float 官方 options)；现役 js-yaml 4.3.1 词法规则集中在中央 `scalar_value`/`is_incumbent_int_literal`/`is_incumbent_null_literal`/`is_incumbent_float_literal`/`is_timestamp`。这正是 R03 报告修复要求指定的路径（「继续使用成熟语法解析器得到的节点，在中央标量 resolver 中落实现役 DEFAULT_SCHEMA 的完整整数词法规则；不得靠大小写整体转换、原文正则改写」）。依赖按 `=` 精确锁定。同族消费者（context_compiler/memory/persona/skills/memorytools/agent_memory/skill_library）grep 核实全部经统一入口或其 read_flags/read_skill_lists 包装，无遗留行扫描器、无绕开解析的 serde_yaml 直读。
2. **缺文件/缺字段默认 vs 损坏报错**：缺文件/空/仅注释 → 默认；字段缺失 → None 消费者默认；语法损坏/类型不符/重复键 → flags 400 + run failed + **零 provider wire** + 原配置字节不变（binary 套件 c01 例 + 本人 HTTP 探针亲验）。解析失败不再变默认开启（旧行扫描 unwrap_or(true) 根因消除）。
3. **等价性测试**：引号内 #/转义/中文冒号、flow/block mapping、嵌套同名键不串扰、BOM/CRLF、显式布尔（含 `True/TRUE` 本人探针亲验=js-yaml true）在 gather 与真实 wire 两级覆盖；本人独立 oracle 表（0B10/0O17/0Xff±→字符串；0b10→2、0o17→15、017→17、0378→378；-.5→字符串；1_000→字符串；1e999→字符串；True/TRUE/False/FALSE→bool；Null/NULL/~/空→null；.inf/.nan→非有限数）与 d2 探针断言逐项一致。
4. **同族排查与未知字段保真**：memory-only PATCH/安装写回保留未知嵌套结构与转义列表（c04 + preservation 12 例 + 本人探针 `future: 0B10` patch 后仍为字符串）；`enable_for_agent`（memorytools.rs:577）错误经 `?` 显式传播，不再 `let _ =` 吞错。
5. **W06-R01-F01 复合键/归一化重复**：`? [memory]`/`? [agent]`/字段级复合键/归一化重复/anchor/merge 复合 16 例（binary P01）全部 400 fail-closed、零 wire、字节不变、peer 复合键预览 400 不误列共享技能；本人探针复核 root 复合键。

### 历史 blocker 逐条亲验

| Blocker | 亲验结果 |
| --- | --- |
| W06-R01-B01 复合键漏读→默认开启 | **已修**：非字符串键响亮 400（`unsupported YAML key type; keys must be strings`），run failed、0 wire、字节不变（binary P01 + 本人 HTTP 探针）。注：js-yaml 因 JS 数组键 toString 巧合接受 `? [memory]`；产品以 fail-closed 拒绝替代，该解决方案经 R02 正式轮确认并由 R03 复跑绿，不构成新缺口。 |
| W06-R02-MERGE-LITERAL 字面 `<<` 误 merge | **已修**：`"<<": {...}` 与 `!!str <<` 为普通字符串键（memory 保持默认，本人探针 wire 含记忆=默认开）；真 merge `<<: *d` 正对照关闭记忆；materialize 显式把字面键重标为 `tag:yaml.org,2002:str` 防库误展开。 |
| W06-R02-UNKNOWN-YAML-TAG `!custom` 自动解包 | **已修**：`validate_tag` 白名单（str/bool/int/float/null/binary/timestamp/merge + map/set/seq/omap/pairs），`!custom` 在 15 个位置全部 400（binary P02 + 本人探针）；本地 `!bool/!str/!map` 非核心 tag 同样拒绝。 |
| W06-R03-F01 大写基数/显式 !!int | **已修**：plain `0B10/0O17/0Xff`(±) 保持字符串（9 例 d2 + 本人 HTTP patch 亲验）；`!!int 0B10/0O17/0Xff/wrong/false/空/1_000` 拒绝（d2 + 本人探针 flags 400/failed/0 wire/patch 拒绝/字节不变 + install 入口副作用前拒绝）；`is_incumbent_int_literal` 仅小写前缀+允许前导 0 十进制，`017→17`、`0378→378` 与本人 js-yaml oracle 一致。 |
| W06-R03-F02 非法 !!null | **已修**：`!!null wrong/false/7` → 400/failed/0 wire/patch 拒绝/字节不变/安装前拒绝（本人探针 + config 套件 install 重放）；合法 `!!null ~`/bare/`null/Null/NULL` 正对照保持 null（本人探针 patch 后仍 null）。 |

## F07 逐义务核验

1. **注册/可调用/常驻/按需四概念**：`model_declarations = Available ∧ (常驻集 ∨ 本轮 activated)`（toolcatalog.rs:1748 附近）；C01 测试注册 Available 的 knowledge_search/search_memory/skills_list 不进声明但仍可 prepare_invocation——可用≠常驻。
2. **不机械压到 4**：`RESIDENT_NAMES` = read/write/edit/exec_command + mcp_search_tools/mcp_describe_tool/mcp_call（toolcatalog_bridge.rs:14）＝7；BE-02 真实 wire 声明集合逐一等于该 7 项。
3. **真实完整描述**：`description_of` 用 `manifest.description`（非 display_name）；BE-02 在真实请求 wire 上断言 write 的完整正式说明原文与未裁剪 schema；C03 在五协议族上保持 name/schema/description/generation 一致。
4. **memory/Skills/knowledge 同族复查**：knowledge/memory 工具同一注册路径、manifest 携带实质完整使用说明（如 knowledge_search 分层检索/分页/citationMarkdown 全文），经 mcp_describe_tool 返回 `full.listing.description`；BE-02 证明它们不常驻（wire 无它们）；`on_demand_calls_require_this_run_description_and_current_generation` 证明 mcp_call 需同 run 说明 + 当前代次、跨 run/换代/撤权全部拒绝。
5. **真实请求开销统计**：BE-01/BE-02 断言基于替身捕获的**实际 provider 请求** tools 数组与 system messages（BE-01 另断言两轮 tools 逐字节一致=LIVE 每轮重建），非静态 prompt 字符串。

toolcatalog.rs 在本候选的 diff（+44/-20）为格式化 + `refusal_reason` pub 化（kernel 拒绝文案与 service 网关 stale-pin 共用一套词汇），实质四态/常驻/描述逻辑在此前提交已落地并由上述测试在本候选上整体复验。

## 环境注记（非产品缺陷）

- **t08 产品目录耦合**：`context_compiler::resolve_product_dir` 依 exe 祖先回溯找 `lib/yuan/lingxi.md`。本审查 `CARGO_TARGET_DIR=/tmp/rv3-target` 在 repo 外，未自带 product 的二进制测试（be01/03/04/05/09/10/usage）首次运行 7 红（run failed.provider_error、0 wire）。手工最小复现定位根因：无 product 目录 → 人格材料解析失败 → run failed（provider_error 分类，无 stub 命中）；设 `LINGXI_PRODUCT_DIR=/tmp/lingxi-rv3/lib` 后**全套 13/13 绿**。冻结候选提交信息亦如实登记了同类 r00_management 环境受限。审查期间未改任何产品源码。
- **矩阵台账滞后**：冻结点上 `AR_ISSUE_MATRIX.json` 仍载 F06 旧状态（R06_NOT_ACCEPTED、blockers pending final report）；D2 重放结论体现在提交信息与测试套件中，矩阵状态更新属 F14/登记面收尾，不影响本判决的事实基础。

## 次要观察（非阻断）

- `!!float 12`：现役 js-yaml 存回 `future: 12`，Rust render 存 `12.0`——未知字段纯文本排版差异，数值语义两侧 reload 相等（本人 zz_rv3_edge_probe 亲验），无敏感字段/类型翻转影响。
- run 终态对配置类失败统一归类 `failed.provider_error`（具体诊断经 flags/配置接口 400 文本给出）——为现行固定分类，binary 套件已按此断言，非 F06/F07 缺口。

## 附录：本审查新增未跟踪探针（证据）

- `rust/crates/lingxi-service/tests/zz_rv3_reviewer_probe.rs` sha256 `541cb8aea47937ec9e8d129357c408a30a396eb00681b91945a0a5c8e03db211`（12 例真实二进制 HTTP：R03-F01/F02 精确族、!!null/~ 正对照、True/TRUE、字面/真 merge 双向、复合键、!custom、memory-only PATCH 类型保真与拒绝后字节不变）
- `rust/crates/lingxi-adapters/tests/zz_rv3_edge_probe.rs` sha256 `3696e81dcc6f87f9ebf4162c1ccb964c18e87e41da1d9b4d9a17aed0348b225e`
- worktree 其余 tracked 文件与 844a10b2b 完全一致（`git status` 仅上述 2 个未跟踪文件）。
