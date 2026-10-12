# R06-AR-W06 / F06 — 独立对抗性审查 R01

**VERDICT: FAIL。** 固定候选的常见配置样本已修好，但仍存在一个未关闭根因：配置允许非字符串 YAML 键，却只按字符串键读取与校验。现役解析器能正确读到的关闭记忆/关闭工作区技能配置，在候选里仍被当作缺字段，恢复默认开启。独立亲跑已经证明真正模型请求收到合成私密记忆和本应关闭的工作区技能。相同错误还把其它助手正在使用的技能误列为孤儿清理候选。没有真正删除该技能。

审查者 `/root/ar_w06_reviewer_01`，未参与实现或其它轮审查，本轮结束后不复用。产品、已有测试、Git均只读。额外探针与证据仅写本目录；没有复制源码树、.git或构建缓存。

## 固定候选与实际构建

- cwd：`/Users/study_superior/.codex/worktrees/r06-independent-review/LingxiAgent`。
- 基线/HEAD：`2e1933d8f16d0f383b85ca92ff5bdf29e4732e6d`，加 W06-CANDIDATE-01/binding.json 的15个准确SHA文件（产品8、Cargo2、测试5），不含W02/W12改动。
- 开始和结束重新计算15SHA，全部与候选一致；见 `binding-before.json` / `binding-after.json`。所有验证结果绑定这份内容而不是只绑定HEAD。
- 本人使用 `/Users/study_superior/.cargo/bin/cargo` 1.98.1，rustc 1.98.1，Cargo全部 `--locked -j8`，本人Cargo串行；保留8192MB/600s guard，有真实可监督进程。
- 独立target固定 `cwd/rust/target`，没有共享target。实际 service binary SHA256：`ac78d36d8f84034bc4b49c72ca293b3ce04cdbc901d6a5f9baa3ff83080a60f7`。
- 每次run及每个二进制样本都有新合成LINGXI_HOME；启动READY打印实际home及source=cli与期望一致。所有模型端点都是127.0.0.1替身。编译器、gather、网关、Run、存储和消息组装全部真实。没有访问真实数据、付费供应商或发送给真实联系人。
- 子进程均terminate并wait，loopback替身shutdown/join，逐样本TemporaryDirectory清理完成；统一runner创建的7个空home已定向移除，见 `cleanup.json`。没有清理未知目录。

## 阻断 R06-AR-W06-R01-F01（HIGH）

违反：F06修复要求1—4、C01等价语义、C03异常类型/重复敏感键、C04现役兼容；原T01 Step1/4、T06 Step2/3中配置和技能范围份额。原R00记忆开关、工作区技能策略、清理预览叶的配置份额同时受影响。

根因定位：`rust/crates/lingxi-adapters/src/storage/agent_config.rs` 22—54只验证Mapping及通过字符串键找到的值，57—62的field只用字符串get；序列键被保留为另一种Value，敏感键校验、默认值和后续set都看不见它。未规范化的等价键也绕过敏感重复键检查。

两个最小记忆样本：

```yaml
? [memory]
: {enabled: false}
```

```yaml
memory:
  ? [enabled]
  : false
```

现役 `lib/memory/config-loader.ts` 使用的js-yaml两者都读成 `{"memory":{"enabled":false}}`，见 `logs/incumbent-oracle.log`。候选的真实二进制经认证HTTP GET config-flags均返回200/`memoryEnabled:true`；真实POST execute→Run completed；捕获的首个provider HTTP请求包含 `REVIEWER-ONLY-NONCE`。执行前后原配置字节不变，证明不是测试重写或替身拼装造成。期待是保留现役禁用语义，或在不支持该键类型时明确拒绝并零模型请求；绝不能把不识别的键当缺字段默认开启。

同根因全部已找到路径与结果：

| 路径 | 独立实际结果 | 期待 |
|---|---|---|
| 根memory键/分区enabled键 | 两个样本flags开启、模型收到记忆 | 禁用或明确报错、零泄入 |
| workspace_context内 `[discover_project_skills]` | flags=true，真实wire出现 `PROJECT-ONLY-REVIEW-NONCE`；普通字符串键对照flags=false且wire没有该技能 | 明确false生效或拒绝 |
| 其它Agent的skills内 `[enabled]` | cleanup-preview列出双方仍用的shared-review；普通键对照skills=[] | 不误报独占，或拒绝无效配置 |
| 根 `[agent]`、agent内 `[name]` | 合法带引号的姓名被忽略，实际请求使用默认名字 | 人格值保真或明确拒绝 |
| 同一文档同时有字符串memory键及 `[memory]` | 200/Run完成；现役js-yaml以duplicated mapping key拒绝 | 按规范化结果拒绝敏感重复 |

以上前四类是真实二进制/HTTP/provider或预览入口复现；未实际执行删除。`probe-results.json`、`family-probe-results.json`、`effects-probe-results.json`保留输入、完整请求、flags、runId、真实数据库终态、配置未变、有效home、READY和binarySHA。其中family/effects脚本原来的matches_expectation字段仅断言记忆主轴，不是姓名/发现/清理业务通过；本报告对其它字段与wire独立比对，不能据该字段的true掩盖问题。

同族消费者完整源码检查：

- `context_compiler.rs:381,388,594,628`：主Agent材料与其它Agent roster；`gather`的memory默认true。
- `memory.rs:62,117,128,325,1024,1054,1150,1202,1257`：语言、flags、记忆/经验各HTTP入口和更新。
- `memorytools.rs:343,563`：执行时开关复核、技能启用。
- `persona.rs:70,86,103`：语言与yuan人格选择。
- `skills.rs:338,339,354,759,764,807,941`：名单/策略、启停、删除引用、孤儿预览、模型材料选择。
- `agent_memory.rs:1912,1916,1920,1945,1970`：统一读/写与原子补丁。
- `skill_library.rs:658—685`：其它Agent引用收集。

统一入口确实已接线，旧行扫描器已从产品读取链移除；这是新入口的键语义遗漏，不是只新增解析器未接线。普通无效值/语法读取错误已沿各处传播，不存在原先反馈unwrap_or_default吞错；其复合键旁路仍让各家族默认行为错误。experience、locale/yuan、workspace_disabled以及写入set路径属于相同字符串lookup根因，源码推断受影响，未逐项真实复现，不冒称运行结果。

修复要求：在统一入口、默认值和更新之前，对根键及所有拥有的敏感分区字段键执行与现役一致的归一化/类型边界检查；规范化后检测冲突重复；支持或明确拒绝，不允许静默忽略。覆盖anchor/merge相互作用并保留未知兼容数据。不靠各消费者分别加特例，不改松旧断言。为上述记忆、发现策略、其它Agent引用及重复键增加真正业务断言。

必须重跑F06四C及上述所有同族反例、原77项、记忆真实工具拒绝、共享技能预览/保存字段保真、格式/clippy/contracts/boundaries；后续公共修复影响F05缓存、F07目录和R02—R05配置/运行/网关消费者，按影响闭包重测。所有产品/测试/配置修改会使本候选审查失效，下一轮须全新Reviewer。

## 四C亲验

| C-ID | 期待 | 实际裁决 |
|---|---|---|
| C01 | 等价写法不改变记忆禁用 | block/flow必需样本亲跑PASS；扩大到现役允许等价键写法真实FAIL，整体等价性尚未满足 |
| C02 | 引号内#、转义、Unicode、BOM/CRLF保真 | 具名二进制样本及gather单双引号/多行/Unicode均亲跑PASS；额外quoted-key转义也PASS；复合姓名键缺口归上方根因 |
| C03 | 同名深层不串扰，异常类型/重复明确拒绝 | 现有普通错误值/语法/重复均拒绝；不支持的复合键静默忽略、规范化重复接受，FAIL |
| C04 | 缺文件/空/部分/老配置正确默认，未知字段不丢 | 缺文件/空/部分/example及未知字段保存样本PASS；现役复合键语义不兼容，FAIL |

本人四C：10/10测试通过（binary1测试内部10配置样本，service6、adapters3），不是据实现者日志签通过。独立额外16个二进制样本覆盖合并序列优先级、显式bool标签、quoted-key Unicode、异常merge、显式string假布尔、未知字段及同族反例；存在实际违约。读取全程不改写原配置。

## 原条款与范围份额

已全文读取固定AGENTS、Reviewer模板、完整F06/通用要求/三层验收、原Taskbooks T01/T06全部Steps/交付物/A01/A02/A11/A12及通用约束/独立模板，读取E01原全文义务与份额登记。`original-obligation-review.json`保留354条原文及deadline（10 Steps、6交付、4 A、263 R00叶断言、31关联修复Step、28 C、12旧review义务）；101叶只是分组索引，没有按数量判PASS。

T01 Step1组成顺序、Step2同源artifact、Step3静态前缀、Step4中英文/子代理、Step5模板冻结的普通配置具名回归已亲跑；配置等价性、姓名及工作区材料反例仍失败。ContextCompiler/ContextArtifact和原差异报告作为已有交付保留，本审查不接受完整原Task。

T06 Step1事实/compiled/经验/修订原回归亲跑，记忆开关失败；Step2 Agent隔离回归绿，但其它Agent技能引用误读；Step3名单/安装审核/启停/遮蔽普通配置回归绿而复合发现/引用失败；Step4四项hooks回归绿且本候选未改hook/旧loop；Step5审核+真实gateway越界/symlink拒绝亲跑绿。MemoryService、SkillService、Pi replacement原交付保留；梦境调度、记忆完整生命周期、技能同源、worker/实验等EX原到期份额未关闭，不能归R07或登记延期代替满足。

A01真实system/观测摘要及A02 golden/segment预算/超限响亮失败具名回归亲跑绿；没有把同源误当正确材料，也没有把static长度当完整tools wire预算。A11事实/tenets/compiled与真实工具绑定隔离亲跑绿；A12风险确认token、TTL、内容变化与gateway越界两层亲跑绿。这些是本项配置影响回归，完整父子范围、缓存、目录、记忆/Skills生命周期及其他到期叶仍分别归F01/F05/F07/EX和F14/F15逐案闭环。没有将pending/share当完整PASS。

## 旧红核验

E01 old-positive实际Finished test profile、1 passed；old-red实际2匹配测试、0 passed/2 failed，panic为flow私密记忆仍注入和quoted-name得到`"助手`；不是编译错误、少依赖或零匹配。历史warnings不影响运行事实。

本人另外从基线git show直接提取准确AgentConfigScan/scan_agent_config，编译独立guard链接的old-scanner-probe，1正对照绿/2契约红（exit101，3测试确实执行）；失败值为memory=None和name=`"助手`，正好对应源码根因。当前真正binary/gather/Run相同正常契约亲跑绿。此独立复现是准确基线解析器，不宣称完整旧二进制重跑；E01 equivalent-old-three-tests.rs为事后等效提取，没有当作历史测试字节快照。原历史测试字节不可复证的限制明确保留。

## 本人命令、实际结果与未验证

`results.json`内runs完整记录每条原始命令、start/end、exit、test_count、锁/fixture/candidate SHA、logSHA和binarySHA；`probe-executions.json`记录额外脚本命令和16样本证据SHA，`evidence-manifest.json`绑定证据文件。Cargo命令均来自固定cwd，绝对Cargo，--locked -j8。

- 四C：exit0，10 passed；`logs/four-c.log`。
- 原T01/T06影响回归：exit0，77 passed；`logs/regression.log`。
- workspace all-targets clippy -D warnings、fmt --check、xtask check-contracts/check-boundaries：全部exit0。非测试命令的test_count=0是无测试计数，不是“零匹配测试PASS”。
- 独立旧扫描器：exit101，1 passed/2 failed，为预期契约红；`logs/old-control.log`。
- 新增二进制样本脚本执行正常exit0，存在业务反例FAIL；绝不因脚本exit0或其只检查记忆的字段true把违约记为PASS。

未执行完整workspace/正式R06门禁、R05→R02全阶段闭包、其它OS/打包、真实供应商/LIVE、旧完整binary重新构建、真实联系人外发或数据迁移。它们不由本项绿灯替代。当前FAIL是已实测代码缺陷，不是外部条件BLOCKED。没有Git写、提交、推送、发布或R07工作。
