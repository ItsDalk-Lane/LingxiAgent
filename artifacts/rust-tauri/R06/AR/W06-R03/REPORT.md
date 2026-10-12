# R06-AR-W06 / F06 — 第三轮独立实际验收

VERDICT: **FAIL**。本轮配置解析与保存仍不完全符合现役应用的实际规则；无效配置还可通过真实技能安装入口产生文件变化并被报为安装、启用成功。已一次汇总找到的两类同族阻断。本 Reviewer 未修产品、原测试、门禁或锁，未 stage/commit/push。

本报告不关闭完整 R06-T01/T06、A01/A02/A11/A12、原叶或整个阶段。F05 同一会话下一 Run 的缓存刷新仍 OPEN；新会话重新读取不能替代它。F01/F07/F13/EX03/EX12/F14/F15 及剩余原 owner/截止义务保持原归属。

## 固定对象与独立边界

- 角色：全新、一次性 Reviewer03；未参与该 Task 的实现、Repair 或前两轮 Reviewer，不复用实现者聊天结论。
- 实际验收区：`/Users/study_superior/.codex/worktrees/r06-w06-review3/LingxiAgent`。HEAD `bc2ce51fd005ea75b9949156974e15d2761eb734` 是已接受 F08 基线；F06 为冻结的 15 文件工作树候选，不能仅用 HEAD 声称全部候选已提交。
- 15 文件 NUL canonical aggregate：`ff5e2a83231ae6f6f306a7558d9c801fffdb66667d47da7e9fff986d1aa51e35`；锁：`3693acdef7ace5ff274f6650c9f2be0721f25ff2eb62503be0301f15537f9241`。
- 本轮实际服务：工作区自己的非 symlink `rust/target/debug/lingxi-service`，初始 SHA `e2675cbe9b6102bfff1a2cf63166c18a7c96f263fe1e2cd030b3adcf0f402b17`。全部二进制反例记录实际路径、SHA、命令、READY 和退出码；最终 build 及终态见 `service-build.json`、`binding-after.json`。
- 主仓用户 `.vscode/settings.json` 仅计算保护摘要，未修改：`0f72eb0a2549c95973c2aa8abe54c9aa44491fb9ca2a1c6e437404655d36b3d3`。
- 前后复核全部 292 个 Rust 源码/manifest/lock，以及 15 文件、服务二进制和用户设置。所有证据探针属于本目录。前后完整绑定见 `binding-before.json` / `binding-after.json`；探针和日志由最终 manifest 绑定。
- 工具链为 `/Users/study_superior/.cargo/bin/cargo` / `rustc` 1.98.1，Cargo 使用 `--locked -j8`。运行保留 8192MiB/600s guard。每次使用独占合成 home；真正服务 READY 显示 `source=cli` 和合成目录，并非只声明环境变量。未测峰值 RSS，全部为 `null`，不能写为 0。
- 仅 loopback 外部模型应答替身；实际生产配置、gather、Gateway、Run、认证 HTTP、权威存储和消息组装不 mock。未迁移真实数据、付费、真实外发，未进入 R07；没有借本验收重试 W02/W12。

已阅读不可变完整规格（1219 行）、忽略目录中的原任务书/相关 T01/T06 全 Steps/交付/A-ID、到期叶与前置交接、Reviewer A—G 模板、SCOPE-01、两轮正式 FAIL、RC01 八根因/十二路径、P03 最终报告/矩阵/四 C/原 scope 和失败记录，以及真实配置调用者和现役配置加载/保存源码。结构化历史记录的 hash/过程成功不继承为本轮业务成功。

独立复核 P03 的 274 条 manifest 全部摘要吻合，manifest SHA 为 `2bb912436e5621864ac8f418e5d8c29b7f7debbc75a29afbe266d7637aace1d5`；17 个最终命令日志摘要、exit、前后候选摘要吻合。此完整性复核是证据来源验证，不能免除独立亲跑，详见 `P03-integrity-review.json`。

## 阻断一：现役文字被转为数字；非法显式整数仍可安装

**ID：R06-AR-W06-R03-F01，HIGH，OPEN。** 违反 F06 修复 1/2/4、C03/C04，原 T01 Steps 1/4、T06 Steps 3/5 的配置份额及未知兼容字段保护要求。

源码位置：`rust/crates/lingxi-adapters/src/storage/agent_config.rs:533` 和 `:561`。`scalar_value` 将 plain 标量交给 `serde_saphyr::Value` 的数字解析，仅加少量特殊兼容分支。现役 `js-yaml 4.3.1 DEFAULT_SCHEMA` 的整数 resolver 只接受小写 `0b/0o/0x` 前缀；候选所用 resolver 接受大写前缀，导致库解析结果成为错误的规范类型。显式 `!!int` 同样缺失现役完整 lexical 验证。

最小复现：合成 Agent 配置 `memory: {enabled: false}\nfuture: 0B10\n`。真实认证服务读取并执行 Run 后，只 PATCH `/lingxi/v1/agents/lingxi/config` 的 `memory.enabled=true`。现役原应用 loadConfig → saveConfig（仅修改 memory）→ loadConfig/getRawConfig 保留 `future` 为字符串 `0B10`；实际候选保存 `future: 2`。该请求返回 200，真实 Run completed、外部 provider 捕获 1 个实际请求。未知字段的类型和值在一次无关设置修改中被改变。

完整同族已亲跑：`0B10`、`0O17`、`0Xff`、`+0B10`、`-0B10`、`+0O17`、`-0O17`、`+0Xff`、`-0Xff` 共 9 个应为文字的值被改成数字。小写前缀、无效普通前缀文本、普通 bool/null 正对照仍符合现役结果。显式 `!!int 0B10/0O17/0Xff` 被现役加载明确拒绝，候选 flags/PATCH 返回 200 并正常执行 Run。

真实 Gateway 额外输入 `future: !!int 0B10` 后，`p02-install-10` 的技能文件确实安装，配置确实追加启用技能，收据为 `Success` 且写“enabled for this agent”，其配置被改为 `future: 2`。期待为副作用前拒绝，不安装、不启用、不修改原配置。

根因全路径：RC01 P01—P12 共享统一配置入口，因此统一 resolver 错误同时影响读取、材料采集、人格/语言、记忆/经验、技能选择、预览/清理/删除、知识工具、技能安装及配置写回。新标量族已直接通过 P01/P03/P11/P12 实际运行；其余路径是实际原回归加共同入口源码推断，未谎称新反例逐个路由都已注入。读取和写回后再解析使用同一宽松 schema，内部自比较会漏掉现役类型差异。

修复要求：继续使用成熟语法解析器得到的节点，在中央标量 resolver 中落实现役 DEFAULT_SCHEMA 的完整整数词法规则；合法 plain 文本保留类型和值，非法显式整数在任何写入/安装前拒绝。不得靠大小写整体转换、原文正则改写、默认关闭、收窄合法配置或绕过原未知字段保护过关。

证据：`new-family-business-summary.json`、`extra-v3-results.json`、`extra-v3-typed-results.json`、`extra-v3-oracle.mjs`、`gateway-extra-test.diff`、`gateway-extra-results.json` 和 `logs/gateway-extra-behavior.log`。必须重跑全部四 C、历史 R01/R02 族、该数值族的所有写入份额与真实安装、原 77、适用门禁及最终组合阶段回归。

## 阻断二：无效显式空值标签被接受，安装收据错误

**ID：R06-AR-W06-R03-F02，HIGH，OPEN。** 违反 F06 修复 1/2/4、C03/C04，原 T06 Steps 3/5 配置份额及异常输入前置拒绝、真实收据要求。与上一项属于同一 schema 兼容族，分别保留最小修复责任。

源码位置：`agent_config.rs:533` 和 `:572`。`validate_tag` 接受标准 `null` 标签，但 `Some(core)` 在转换前不验证空值文字是否能由现役 null resolver 解析；库转换为 `Value::Null` 后丢失非法文字。owned 字段类型检查不能保护未知兼容节点。

已亲跑 `future: !!null wrong`、`future: !!null false`、`future: !!null 7`。现役原应用均报“cannot resolve a node ... null explicit tag”；候选均 flags 200、Run completed、真实 provider wire 1、memory-only PATCH 200，将非法源保存为 `future: null`。期待 flags/PATCH 400、Run failed、provider wire 0、原配置字节不变。合法 `!!null "~"` 正对照通过；无效 float/int 对照保持拒绝。

真实 Gateway 中 `future: !!null wrong` 导致 `p02-install-9` 的文件实际安装，技能实际启用，返回安装且启用成功；内层完整测试退出 101，明确列出 9/10 两个错误。由此证明这是副作用前配置校验漏洞，不仅是加载后展示差异。正常受支持标签和资源深度拒绝控制仍通过。真实安装后 Permission denied 对照正确报告“installed ... but enabling failed”，文件已安装而配置未改，未伪称回滚或启用成功；该既有正确 partial 收据不能掩盖新无效输入的错误成功。

根因全路径同 P01—P12；直接亲验范围与源码推断边界同上一项。修复要求：在标准 null 标签转换前按现役 lexical resolver 验证；保留现役合法空值，不把非法输入降为 null/default。完整文档必须在安装、删除/清理及任何写入副作用前验证，后置 I/O 故障仍按实际影响准确报告 partial。

早期 `extra-typed-results.json` 的 `unknown-null-bad-extra` 曾显示 PASS：helper 因原加载错误、没有可比较结果而将空比较判等。这是采集盲区，**不是业务 PASS**；原始 completed/wire 已证明接受非法配置。修正后的 v3 oracle 把 flags、terminal、wire、PATCH 和字节不变纳入明确拒绝契约，三个 null 样本全部 FAIL。所有早期失败/中间记录保留，未替换历史红。

证据与重跑：`new-family-business-summary.json`、`extra-v3-typed-results.json`、真实 Gateway 日志；重跑全部四 C、八根因/十二路径、旧 R01/R02、null/int 全词法负例和合法正例、真实安装/删除前置拒绝与后置 partial 收据、原 77 和门禁。

## 实际验收与旧红验证

| 本轮亲跑 | 结果 | 边界 |
| --- | --- | --- |
| 四 C 原有用例 25 项 | 25 绿，exit 0 | C01/C02 命名契约绿；新增同族反例使 C03/C04 总结论 FAIL |
| 受影响原回归 | 77/77，exit 0 | 原 T01/T06 对应测试；不是完整原 Task/叶验收 |
| 旧 R01 全族 | 16 个实际二进制案例契约绿 | 类型、重复、compound key、merge/source、文件效果重新运行 |
| 旧 R02 全族 | 105/105 语义断言、15/15 标签断言绿 | quoted/!!str `<<` 与真 merge/alias/%TAG、被覆盖源、标准/local/custom 标签、实际保存/预览 |
| 现役 typed 原应用 oracle | 原 6 组绿 | Date/Uint8Array/set/omap/pairs/非有限数/-0 等实际类型保持；新增 12 组中 7 组红、5 组控制绿 |
| 生产旧 flow 扫描变异 | 编译 0、测试编译 0；block 正控绿、必需 C01 flow 红，业务 exit 101 | 临时生产源码副本，不改固定候选；真实 gather/Run/wire 不 mock |
| fmt、workspace all-target clippy `-Dwarnings`、contracts、boundaries | exit 0 | 实际命令/日志在 command-review |
| F08 已接受份额回归 | 7/7，exit 0 | 不能继承 F08 历史602/572为全阶段绿 |
| R05/R04/R03 受影响路径 | 43/43，exit 0 | wiring/closed loop/Gateway/acceptance matrix 局部；不是全部 R05→R02 正式验收 |
| registry consistency | exit 1 | inherited：migrations 9，registry 7，缺 v8/v9；F14 原 owner |
| 原 cancellation approval round trip | 1 失败，exit 101 | 已接受 F08 多了 `tool_result_model_projection`，旧完整事件向量少该项；不归因 F06 |

旧红不是构建失败或空测试。`mutation-v3.py` 从原审计基线取旧扫描函数，隔离替换生产 `parse_agent_material`；临时复制仅 service 源，读取固定 target 的已绑定依赖，实际 rustc 编译并链接生产 library。实际控制各捕获一条 provider 请求，block 正控绿，flow 关闭记忆断言红。固定候选四 C 中同一 C01 绿。构建/探针早期错误 `mutation.json` / `mutation-v2.json` 不算旧红，完整保留。临时二进制、source、依赖 SHA 见 `transient-probe-binding.json` 和 inner results，临时树已清理。

安装额外探针也只修改隔离的测试输入两条，生产源码保持 fixed SHA；不是实现修复。编译两个阶段均 0，完整实际 Gateway 内层 101。外层收集脚本 `exit 0` 仅表示采集完成，在最终 `command-review.json` 被标为 `FAIL_OBSERVED`。所有采集/运行状态与业务判断明确分开。

`extra.py` 初次探针错用 `/config-flags` PATCH，405 后 JSON 读取报错，异常 finally 又留下采集线程。服务子进程已 terminate/wait，随后仅终止本次精确采集 PID 71639，记录真实 exit -15；这是探针错误 NOT_RUN，不是产品反例。后续正确 `/config` 版本重新实际执行。无本 Task 活进程、所有独占 wrapper homes 和临时副本均已清理，见 `cleanup-review.json`。未测内存仍 null。

## 原义务与未完成范围

`original-obligation-review.json` 完整保留 354 条相关记录、101 个叶组的原文字、来源、owner 和 deadline，并另加本轮的实际执行/源码关联/未执行状态。相关 Task Step/交付/A-ID 仅有已执行的配置份额，未知兼容字段及异常配置份额 FAIL，完整任务始终 OPEN。48 条配置关联叶断言不凭文件级归组签全义务 PASS。

`full-original-scope-preserved.json` 完整保留 SCOPE-01 的 1234 条登记、281 个独立叶及 687 个断言。全叶/全断言运行本轮 NOT_RUN；“已登记”没有改成“已亲跑”。原共同约束、交接、上游 R06 份额及 Pi 能力/扩展义务保留原 owner 和到期点，不用 share/deferred 改名免除当前到期要求。

逐 A—G、四 C、RC01 八根因、十二路径及每个真实命令见 `check-matrix.json`。P01/P03/P11/P12 的新增反例为实际执行；其它新同族路径仅从共同入口推断，原路径回归已执行仍不能冒充同反例逐路由执行。资源深度边界实际运行；byte/event/node/scalar/alias 最大预算压力本轮未逐个跑到边界，前置计数/分配次序为源码推断。

必须由原 owner 继续：F05 同 session 下一 Run 缓存/提示刷新；F01/F07/F13、EX03/EX12 的剩余义务；F14 原义务/门禁重新绑定；F15 最终组合阶段独立收口。registry 和 accepted F08 取消电池兼容必须处理并重跑；不能空 filter、删 ordered/approval 断言或降低检查力。Windows/Linux、正式打包、真实供应商、完整 workspace tests、全部 R02—R05 正式验收和最终组合 R06 阶段本轮 NOT_RUN/ST​ALE，必须最终重新运行，不继承旧局部 PASS。

本轮没有实际平台审批阻塞；结论为产品契约 FAIL，而非 BLOCKED。证据完整性通过不能改变这个裁决。
