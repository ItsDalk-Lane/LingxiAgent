# R06-AR-W06 / F06 P03 修复交付

状态：**READY_FOR_INDEPENDENT_REREVIEW**。本报告是修复者对固定候选的自检，不是独立验收；需要总控另建全新 Reviewer03。P03 未 stage、commit 或 push，未改用户 settings、W02/W12 或其它 Task 产品。

候选为 `W06-CANDIDATE-03`，固定副本 `/Users/study_superior/.codex/worktrees/r06-w06-review3/LingxiAgent`，HEAD `bc2ce51fd005ea75b9949156974e15d2761eb734`。原 audited base `2e1933d8f16d0f383b85ca92ff5bdf29e4732e6d` 仅用于历史追溯；已接受的 F08 保留在当前 base。root 精确复制后，所有最终执行均使用这份副本的源码、锁和自有非 symlink target。

15 个 owned 文件完整 SHA、差异及 frozen-source 在 [freeze-01.json](freeze-01.json)、[owned-tracked.diff](owned-tracked.diff)、[owned-new-files.diff](owned-new-files.diff)；最终源与二进制在 [binding-after-final.json](binding-after-final.json)。root 的 NUL canonical aggregate 为 `ff5e2a83231ae6f6f306a7558d9c801fffdb66667d47da7e9fff986d1aa51e35`；executor 的 sorted JSON aggregate 为 `c25cf6dc2678336df62c9c5a06ebc7113200377e1a45d9c339d2ffca17a530ad`，两种算法不混用。最终服务二进制 SHA `e2675cbe9b6102bfff1a2cf63166c18a7c96f263fe1e2cd030b3adcf0f402b17`，锁 SHA `3693acdef7ace5ff274f6650c9f2be0721f25ff2eb62503be0301f15537f9241`。主仓与候选最终逐 15 SHA 相等，settings 保持指定 `0f72eb…b3d3`。两个原测试文件在 P03 中与 P02 交接逐字节一致，[original-test-preservation.json](original-test-preservation.json) 不以相对 base 的历史改动误称本轮删松断言。

修复覆盖配置从解析、验证、读取、修改、保存到重载的同族入口。保留成熟解析器的 event 标签、标量样式、alias 和 mapping 身份，对所有敏感字段及 merge 被覆盖来源先验证；真正 merge 与 quoted/!!str/alias 字面 `<<` 分开处理。local `!bool/!str/!map/!int/!seq` 和未知 custom 标签原读取器拒绝，本实现同样拒绝。敏感字符串里的标准 binary/timestamp 等真实错误类型不会先被解成普通字符串。坏语法、类型、重复敏感键、坏 merge 与 I/O 错误均明确失败；只有正确的缺文件/空/缺字段才走原默认。

成熟接口边界明确如下：granit-parser 1.3.0 负责完整 YAML 语法与原始 event；serde_yaml_ng 负责成熟标量类型和 apply_merge；serde_saphyr 1.3.0 的类型反序列化接口与公开 Tagged serializer 负责类型读取及标签保存。SourceGraph/ConfigNode 只适配成熟节点和业务验证，不逐行解析、不对 YAML 源文做正则预处理、不自写完整语言。采用完整 core namespace whitelist 避免库的 local 别名与现役 schema 混淆。

现役 schema 已绑定本机实际 js-yaml **4.3.1**、package-lock 与生产 `lib/memory/config-loader.ts`。loadConfig/saveConfig 都直接 YAML.load、没有 custom schema 参数，实际为 installed DEFAULT_SCHEMA；[incumbent-schema-binding.json](incumbent-schema-binding.json) 保存入口、锁、原 resolver 来源 SHA。[typed-oracle.mjs](typed-oracle.mjs) 实际调用原 loadConfig → saveConfig → loadConfig，再对 Rust 保存结果用相同入口读回；没有仅用库默认 load 冒充生产读取。

单标量 schema 适配只发生在解析器已产出的节点：timestamp 使用现役 resolver 的原模式识别 date/timezone/仅日期，保留标签和原标量，不另造 Date 构造器；quoted、!!str 日期与非日期字面仍是字符串。binary 依据原 resolver 的 CR/LF、alphabet 与长度规则验证，但保留原标签/标量，避免更严格 codec 误拒现役允许的非规范尾位和空 binary。数值由成熟接口读取；原 schema 的 underscore、signed-dot、overflow string、显式错误数值以及 `-0`/`-0.0` 差异按实际原入口对照适配。实际 Uint8Array、Date、非有限数、负零和字符串类型逐项比较，不能用 JSON stringify 抹掉差异。标准未知 binary/timestamp/set/omap/pairs、%TAG/URI、数值、中文块文本在只改 memory/skills 后保存重载保真；6 组完整原输入、Rust 输出、原应用保存输出和归一化 typed 结果见 [final-incumbent-typed-results.json](final-incumbent-typed-results.json)。未据这些有限样本承诺任意库扩展均支持。

资源保护在前置分配阶段执行：入口 metadata/有界读取 256 MiB、成熟 parser depth 64；event、node、scalar、anchor、alias counters 在容器增长前检查；graph expansion/cycle/depth/scalar 有界。限制沿成熟库默认预算，没有按行扫描或小尺寸任意裁切。正常 2000 字段宽配置/alias 正对照通过，超深、指数 alias、250k 节点边界和稀疏超入口文件明确失败且保原件；真实 Run 零 wire、Gateway 零安装/提案。参数与反例源均绑定最终测试 SHA。

安装在执行副作用前验证配置；遇到真实后置配置写入失败，技能实际已经安装就返回“已安装，但启用失败”，原配置保持不变。删除先完整检查本助手/全部 live peer 配置、目录读取和 bundle 数据，已知错误先拒绝。发生真实后置 scrub 写入失败时保留已删除的实际结果，并返回 `SKILL_DELETED_REFERENCE_CLEANUP_FAILED`（现有 HTTP 错误契约 500）；没有假回滚或成功收据。7 个真实二进制 DELETE 隔离场景覆盖 normal、bad peer、peer directory、bad bundles、bundle directory、late write、unreadable agents，均核对物理文件、引用和陌生共享材料字节。正常 Gateway、坏配置零安装及 chmod 0555 后置真实写错也已执行；permissions 都在自己的合成目录恢复。[actual-side-effect-receipts.json](actual-side-effect-receipts.json) 保存原运行收据与文件状态。

历史处理遵循根因前置。RC01 REPORT SHA `c9533345853a2bc46c96ad0647db38dcf7c60c899390959e36ffdc0e36ee5753`，matrix SHA `6a3b71a6bcc8bd1c8bb5d3ead4e1a44848ff484952f4ec8f3ffd8bf58df2453d`。它的 8 根因/12 路径来源与 SOURCE_ONLY 身份完整保留，没有把 RC01 的 14 个 js-yaml 样本当业务验收。E01/P01/R01/R02/P02/RC01 全部 387 个 manifest 文件最终 SHA 复核通过，[historical-integrity-final.json](historical-integrity-final.json)。旧候选15源/二进制绑定见 [historical-source-binary-binding.json](historical-source-binary-binding.json)。

旧 line scanner 的正常对照 1 绿、flow/name 2 红重新执行；旧 R01 三份真实探针、旧 R02 四份真实探针及 semantic/tagcheck 全部重新执行，保留原始失败。旧 R02 semantic 18/105 不符合及 tagcheck 15/15 失败，生产 Gateway 旧配置异常仍安装并误称启用、后置写错仍报成功，均有本轮实际旧版本红证据。P03 初始真实 local 标签、标准类型擦除、删除先副作用、数值原入口 mismatch 和现役非规范 binary 误拒也保留，修后以固定候选复验。采集脚本 exit 0 只代表采集完成，历史记录中的语义红不因此变绿。全部失败、构建失败、测试期错误预期和中间绿证据未覆盖；[attempt-history.json](attempt-history.json) 区分 HISTORY_RED、INTERMEDIATE_STALE、FIXTURE_EXPECTATION_CORRECTED、FINAL_EXACT_PASS。

最终自检使用 `/Users/study_superior/.cargo/bin/cargo` 1.98.1、`--locked -j8`、8192 MB/600 s guard 和非阻塞父监督。生产 compiler/gather/Gateway/Run/SQLite/file/request 均未 mock；仅外部模型使用本机 loopback，合成独占 LINGXI_HOME 的实际 CLI ready、父子进程退出、输入源/文件 SHA 与前后15SHA可查。真实 home、旧数据迁移、付费和真实外发未执行，最终无本轮合成服务残留。

| 固定候选检查 | 实际结果 | 证据 |
|---|---|---|
| F06 全四C与同族新增边界 | 25 tests，0失败 | [final-four-c.json](final-four-c.json)、[four-c-self-check.json](four-c-self-check.json) |
| 原 T01/T06 适用回归 | 原77 tests，0失败 | [final-regression.json](final-regression.json) |
| R01 历史真实家族 | 16 个配置/wire/SQLite/preview精确判据通过 | [final-r01-contract-results.json](final-r01-contract-results.json) |
| R02 历史真实全家族 | 全部生产采集成功；105/105语义、15/15标签通过 | [final-r02-semantic.json](final-r02-semantic.json)、[final-r02-tagcheck.json](final-r02-tagcheck.json) |
| 现役类型保存往返 | 实际原应用入口6组相等 | [final-incumbent-typed.json](final-incumbent-typed.json) |
| fmt / workspace all-targets clippy -D warnings / contracts / boundaries | 全部exit0 | [results.json](results.json) |

全族“原问题 → 修复 → 正/反对照 → 最终证据 → 未验”逐8根因与12路径在 [family-repair-evidence-matrix.json](family-repair-evidence-matrix.json)。锁中原 registry 三元组没有删除/升级，新增10个已锁 parser/transitive packages；P03 的 regex依赖使用锁中既有版本，详见 [dependency-review.json](dependency-review.json)。所有17份最终执行前后15SHA一致。

原义务保留354记录/101叶，[original-obligation-review.json](original-obligation-review.json) 逐项保留原条款、归属、截止、P02历史与未完成范围。19个 Steps/交付/A 的配置及受影响回归份额有实际证据；48条叶断言仅登记配置份额；8条F06为本轮自检通过；8条F05继续OPEN/STALE；8条F08继承已接受base并回归；263条本轮没有完整原义务证据，明确NOT_RUN，不按邻近路径或77数量关闭整Task/叶。PI_REPLACEMENT_MATRIX、完整prompt差异/全部供应商开销/后台调度、UI/宿主/其它管理/包生命周期按原owner/deadline保持。此处NOT_RUN不构成豁免或改期。

同 session 下一 Run 仍可能使用旧 prompt 的 F05 真实失败保持 **OPEN/STALE**。原 session-freeze 回归绿仅表示保留旧契约测试；新session reload只证明 F06解析保存，不能替F05验收。F01/F07/F13/EX03/EX12/F14/F15及完整原Task、LIVE供应商、其它OS和正式打包均未被P03自免或关闭。W02/W12 Reviewer 的 Daybreak 平台限制未重试或绕过。本轮普通配置产品修复没有遇到相同平台阻挡。

交付已冻结可供全新独立复审。总控仍负责精确Git与后续独立裁决；若 Reviewer03 找到需改产品的新失败，应在主仓修复后新 freeze/root copy，当前最终证据保留并转STALE。
