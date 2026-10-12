# R06-AR-W06 / F06 — 第二候选独立对抗性验收 R02

**VERDICT: FAIL。** R01 的复合键旁路已能明确拒绝，原四C测试13项和受影响原回归77项均由本人亲跑通过；但独立新增反例发现两类未关闭的配置语义问题：普通字符串键 `<<` 被错误当作合并指令；未知自定义 tag 的敏感值被自动解包并接受。实际结果包括把默认关闭的经验读取开启、改变模型中的记忆/姓名/工作区技能材料、错误计算其它助手技能引用，以及保存时改变未知数据结构。这些是已实测代码缺陷，不是外部条件阻塞。

一次性全新 Reviewer `/root/ar_w06_reviewer_02`，没有参与 E01、P01 或 R01。只读候选产品/测试/配置，证据只写本目录。没有修复、Git写入、用户原settings读写、真实资料、付费请求、真实外发、共享技能删除或R07工作。

## 候选与实际执行绑定

- 实际 cwd：`/Users/study_superior/.codex/worktrees/r06-w06-review2/LingxiAgent`；基线HEAD `2e1933d8f16d0f383b85ca92ff5bdf29e4732e6d`。15产品/配置/测试SHA严格符合 W06-CANDIDATE-02/binding.json；开始与结束复核一致，见 binding-before/after.json。本人没有从有其它修复的主仓构建。
- Cargo/rustc均1.98.1，绝对工具链；适用Cargo命令均 `--locked -j8`。fmt本身没有lock/并发参数。target仅本候选 `rust/target`，无共享target。实际service binary SHA256 `406279b9ecabfc8d1ce3678c322ae6900064f00dd2f156d893be77d5ba3d47c1`。
- 每条运行新合成LINGXI_HOME。原二进制测试28次READY，以及独立扩展22样本最终v2记录，均亲验真实canonical home与source=cli。扩展记录有每个子进程实际command、PID、起止、退出码、binary SHA、READY、原始YAML、真实wire/HTTP/数据库终态与保存后文件。各子进程guard日志确认8192MB/600s；没有关闭guard。
- 外部模型仅127.0.0.1 HTTP/SSE应答替身；parser/gather/compiler/HTTP/Run/Gateway/存储/消息组装真实。nonce在合成文件内，替身未预拼prompt。读取前后字节未变；只有明确测试PATCH改变自己的合成配置。
- 22样本服务SIGTERM后wait成功，替身shutdown/join，临时home实际消失。只清理本轮自建夹具和wrapper空home；共享SKILL.md在预览后仍存在。没有复制源码树、.git或target到证据目录。

## 阻断 R06-AR-W06-R02-F01：字符串 `<<` 被误解释为merge（HIGH）

违反F06修复要求1/3/4、C03嵌套同名键不提升、C04缺字段默认及未知字段保留；原T01 Step1/3/4、T06 Step1/2/3中的配置份额。关联原叶为记忆开关、经验读取、工作区技能策略、助手配置保存和共享技能清理预览；逐原记录见 original-obligation-review.json。

根因：`rust/crates/lingxi-adapters/src/storage/agent_config.rs:41` 仅按键字面等于 `<<` 判断merge，无法区分普通带引号字符串或显式 `!!str` 与真正merge键；`:67-70` 先反序列化成失去该区分的Value再全局apply_merge。apply_merge还递归改变未知分区。未知兼容值保存声明因此不成立。

最小样本：

```yaml
memory: {"<<": {enabled: false}}
```

现役 `lib/memory/config-loader.ts` 实际使用的 js-yaml.load 解析为 `memory["<<"].enabled=false`，没有 `memory.enabled`，所以保留原默认开启。候选真实GET flags返回false，真实POST execute/Run completed、provider wire没有合成记忆nonce。`memory: {!!str <<: {enabled: false}}`同样失败。真正 `memory: {<<: *d}`（d.enabled=false）正对照正确关闭；不能通过关闭merge功能绕过该问题。

独立同族结果：

| 实际输入/入口 | 期待 | 实际 |
|---|---|---|
| memory字面 `<<`，引号与!!str两种 | 不提升嵌套enabled；缺字段默认true | flags=false；真实wire无记忆；PATCH把字面结构删除 |
| `experience: {"<<": {enabled: true}}` | enabled缺失默认false；GET experience=403 | flags=true；真实GET=200，返回 `EXPERIENCE-REVIEW-NONCE` |
| 真正experience merge enabled=true正对照 | GET=200 | 200；普通enabled=false对照403 |
| workspace_context字面 `<<` 内discover=false | 字面未知键不关闭默认项目发现；wire有项目nonce | flags=false；真实wire缺项目技能 |
| 其它Agent skills字面 `<<` 内enabled=[shared-review] | 不作为真实启用引用；当前Agent独占预览应列shared-review | cleanup-preview=200/skills=[]，漏报独占；仅预览未删除 |
| agent字面 `<<` 内name | 不提升未知嵌套name | 真实wire使用 `LITERAL-NAME-UNEXPECTED` |
| 根部合法未知 `"<<": 7` + memory.enabled=false | 接受并保留值，正常运行/保存 | flags/PATCH=400 root must be a mapping；Run failed，零wire |
| future内部 `{"<<": {marker: keep}, other: value}`，PATCH只改memory.enabled | future原值保留 | HTTP200，但future变为 `{other: value, marker: keep}`，字面键和原层级丢失 |

见 probe-results-v2.json、effects-probe-results-v2.json、additional-family-results-v2.json；incumbent-oracle/additional-oracle日志为现役独立语义对照。semantic-check-v2对实际记录进行105个明确断言，87成立/18违约，exit1；脚本正常跑完，不是编译或空测试失败。

必须修复：在统一入口保留并正确识别YAML键的类型/merge语义，只有真正merge才展开；普通字符串 `<<` 及未知分区中的普通字符串值必须保留，不允许将合法字符串键统一拒绝来减少现役支持范围。保存只改所属字段，不展开普通未知字段或将其变成敏感字段。真merge、alias/优先级与复合键拒绝继续有效。

## 阻断 R06-AR-W06-R02-F02：未知tag值绕过敏感类型校验（MEDIUM）

违反F06修复要求1/2/4、C03异常类型明确报错、C04不混同损坏/不支持；原T01 Step1/4、T06 Step1/3配置份额。

三个独立实际样本：

```yaml
memory: {enabled: !custom true}
```

```yaml
memory: {enabled: false}
agent: {name: !custom LITERAL-TAG-NAME}
```

```yaml
memory: !custom {enabled: true}
```

现役js-yaml对全部样本报unknown tag，不能据此继续正常运行。候选全部flags=200、Run completed、各1个真实provider请求；两个记忆样本wire包含合成记忆nonce，姓名样本wire使用未知类型下的姓名。PATCH也全部200并保存。期待在不支持该类型时明确400/failed/零provider，并保留原文件字节。

根因：`agent_config.rs:32-34` 的as_mapping、`:88-108` 的as_str/as_bool及`:110-124`的as_sequence/as_str会自动untag；锁定serde_yaml_ng实现明确调用untag_ref/untag_mut。校验只检查解包后的内容，未检查敏感分区/字段是否携带未支持tag。非字符串键已被拒绝，并不保护这条值类型旁路。

见 tagged-value-results-v2.json与tagged-value-oracle日志。tagged-check-v2从真实数据执行15个拒绝/字节保护断言，全部违约，exit1。必须在默认值、消费和set之前明确验证敏感mapping/字符串/布尔/list及list成员的实际类型与tag语义；现役不支持的tag明确拒绝，支持的 `!!bool`/`!!str`及未知兼容值边界分别验证，不能一概删除未知字段。

## 全族调用路径及必须重跑

统一入口已真实接线，没有旧行扫描生产旁路；问题位于新入口的语义/类型边界。两条根因均影响所有以下消费者：

- context_compiler.rs:381/388/594/628：主材料、其它Agent roster、gather，随后真实compiler→Run→provider wire。
- memory.rs:62/117/128/325/1024/1054/1150/1202/1257：语言、flags、记忆、经验各HTTP面与配置PATCH。
- memorytools.rs:343/563：真实Gateway执行时记忆复核、技能启用写配置。
- persona.rs:70/86/103：locale/yuan解析与人格回落。
- skills.rs:338/339/354/759/764/807/941：名单/策略、启停、删除引用、共享预览、runtime目录。
- agent_memory.rs:1912/1916/1920/1945/1970：统一读写/原子patch；skill_library.rs:658-685：其它Agent引用采集。

表内记忆、姓名、经验HTTP、项目wire、peer预览与flag PATCH均是真实执行；其它具体分支受影响是源码推断，未假称全部入口运行。真实Gateway的普通paused/复合键/merge拒绝及tenet零落盘已由本人four-c具名测试亲跑；新增tag/字面键尚需补该入口的业务断言。没有执行删除路径。

两根因关闭后须重跑F06全部四C、R01所有反例、上述新增字符串/tag家族及原77回归、真实Gateway、共享预览、配置PATCH未知值保护、fmt/clippy/contracts/boundaries。公共配置变更影响F05缓存代次、F07目录、相关R02—R05配置/运行/网关消费者；其结果按实际改动标STALE，F14/F15在集成最终候选复验。不能以本轮单项绿替代阶段闭包。

## 四C与原需求份额

| C | 本轮必需原样本实际 | 完整配置契约裁决 |
|---|---|---|
| C01 | block/flow关闭真实wire亲跑PASS | 原两样本成立；不代表所有YAML语义已兼容 |
| C02 | 引号内#、转义/中文/冒号、注释、BOM/CRLF具名样本亲跑PASS | 原字符串值样本成立；字面键语义另属F01 |
| C03 | 普通嵌套/错误值/语法/重复敏感键，以及R01复合键、被覆盖merge来源拒绝亲跑PASS | 字面键提升深层同名字段、未知tag值接受，FAIL |
| C04 | 缺文件/空/部分/example、普通未知值保留亲跑PASS | 字面键默认语义错误、未知结构PATCH改写，FAIL |

已读取完整AGENTS、指定Reviewer模板、总控边界/F06四C/§6-13、原T01/T06全部Step/交付/A01/A02/A11/A12、共同约束与SCOPE-01原义务。original-obligation-review.json保留354原记录（10 Steps、6交付、4A、263R00断言、31修复Step、28C、12旧review），逐项标本轮实际部分/源码推断/未验。scope-leaf-reconciliation.json保留101原叶分组，原owner与deadline不变；不是按101或354数量签PASS。

T01五Step分别记录组成/同源artifact/静态前缀/语言与子代理/冻结模板：相关77具名回归都实际跑过，组成、目录和姓名配置份额被新反例推翻；ContextCompiler、ContextArtifact和差异报告只确认源码/交付保留。T06五Step分别记录事实/compiled/经验/修订、隔离、发现审核名单/人格、四hooks、R04执行边界：相关回归实际跑过，经验开关/名单/人格份额仍FAIL。MemoryService、SkillService、Pi账本只确认保留。

A01真实wire/观测、A02 golden/segment/超限、A11跨Agent物理/业务隔离与工具绑定、A12 token/TTL/内容变化和越界/symlink两层，仅接受已亲跑的配置影响回归份额。完整F01/F05/F07/F08/EX、梦境调度/记忆及Skills生命周期、父子完整授权与其它原到期义务继续OPEN，未因owner分配延期，未宣称完整原Task或A通过。

## 旧红有效性、保留与命令结果

本人从git-show基线核实旧AgentConfigScan/scan_agent_config准确字节，独立证据探针链接本候选guard编译成功（compile-old exit0）；实际运行3测试，block正对照1通过，flow/quoted-name2契约失败（old-contract exit101，memory=None/名称=`"助手`）。不是编译失败、缺依赖或0匹配；当前真实binary相同基础契约绿。不是重建完整旧binary，历史专属测试的准确旧字节快照不可复证限制继续保留。

P01 red-family-03原日志实际3测试、1passed/2failed，普通配置真实正对照成立；原复合键被错误接受、peer清理违约造成红。red-family-01编译错误和02测试层级误读不算业务旧红。R01/P01全部manifest逐文件核验一致；本轮没有改旧历史。本人初次旧探针边界提取assert失败发生在编译前，记setup-attempts.json/NOT_RUN，随后准确提取才获得有效旧红。

旧两份跟踪测试只加Result.unwrap适配，函数名/样本/原断言全保留；专属原四C样本与Gateway断言保留并亲跑，新增内容不替代旧保护。见test-preservation.json及原diff。

本人运行结果：four-c exit0/13passed；regression exit0/77passed；fmt、workspace all-targets clippy -Dwarnings、xtask contracts/boundaries均exit0。以上不掩盖semantic-check-v2和tagged-check-v2真实业务FAIL。非Rust测试命令test_count=0仅表示没有Rust测试计数，探针实际22样本与105/15断言单独记录。

另亲跑registry检查exit1：迁移源码9项/登记7项，v8/v9缺登记，与既有F14问题一致，留给原owner；不是此次配置修复新造成，也没有为绿灯改登记。没有执行完整workspace tests、R06正式门禁、R05→R02全阶段闭包、其它OS、打包或真实供应商/LIVE；均NOT_RUN。本项无法放行的原因是上述两条配置缺陷。

每条命令实际command/start/end/exit/lock/fixture/candidate/log与可执行文件SHA见results.json及各run JSON；最终v2追加每个实际服务进程起止和清理。所有初次输出/失败/复测保留。下一步必须由全新Repair处理，两项关闭后由另一个全新Reviewer重审；本人结束，不自行Repair或接下项。
