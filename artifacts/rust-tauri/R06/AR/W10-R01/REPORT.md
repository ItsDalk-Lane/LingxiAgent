# R06-AR-F10 Reviewer01 — VERDICT: FAIL

本候选能在并发配置变化时阻止不同嵌入身份混入同一索引，但没有完成真实中断后的恢复。实际服务在首批64/89块已写、第二请求已发时被SIGKILL，改模型后同一合成home重启，正式恢复入口返回409，任务一直running，未形成新模型构建。旧paid A向量确实保留，这是安全性正例，不是恢复成功。F09是内部功能依赖，不是外部环境BLOCKED；F10仍NOT_COMPLETED。

角色为全新一次性独立Task Reviewer01，不是最终Stage Reviewer。没有修改候选五源或原测试，没有提交、推送、reset、clean、改用户配置、开始R07、真实账户付费或对外操作。之前749项安全证据只逐SHA核对完整性，没有继承执行者/旧Reviewer PASS。本轮所有实际argv/cwd/start/end/exit/输入前后摘要/执行bin/hash见commands/*/record.json；准备失败和0match记录都保留。

## 实际C验收

| C | 裁决 | 实际证据与边界 |
|---|---|---|
| C01 | PASS | 真实quota占满屏障在路由/凭据已解析但尚未发出的位置reload；实际wire model A、served身份/usage/variant A一致；后续B拒写旧A。c01.json两wire/两usage。 |
| C02 | PASS | A2维首批64向量落库，第二真实响应B2维；任务failed，A64保留，无B混入。隔离旧根因变异实际红，固定候选实际1匹配绿。 |
| C03 | PASS | 已ready A查询在途reload后仍使用实际served A相容variant；新B同维度和C异维度都明确FTS+VECTOR_NOT_READY，不混查。 |
| C04 | **FAIL** | failed分支改B重启正式reingest200、新B89向量ready且旧A64保留，通过该子分支；shipping SIGKILL running分支同home重启READY后正式reingest409、running64/89不恢复。 |

专属九测试退出0，包含一个明确断言running恢复409的测试。不能把九测试绿解释为完整C04绿。原始crash-running.json中的“BLOCKED_DEPENDENCY”是测试原输出，本Reviewer明确推翻该放行解释，当前功能契约为FAIL。细分期待/实际/原始请求/数据库读回见review-results.json和crash-independent-readback.json。

新增自选反例：provider alpha→beta保持same-model、同endpoint、同openai协议、同2维，在外部首响应屏障间reload。认证生产摄入→实际shipping Gateway/OperationService/KnowledgeService/SQLite走通；旧alpha64行保留，beta不混入alpha key，正式重摄入建立beta90行ready。五实际wire对应五usage，provider序列alpha,beta,beta,beta,beta；两个独立model_key准确。首轮独立探针请求体过大ConnectionReset、零供应商调用是PREPARATION_FAIL，随后只缩小独立合成资料、原产品不改。这里两provider配置使用相同合成凭据，不能冒充R05-A01全部不同凭据边界通过。

负对照准确物化固定候选Rust源码于LOCAL_ONLY_NEVER_GIT副本，仅移除跨批served_identity != identity检查，其余shape检查、Gateway、状态、SQLite保留。实际c02运行1匹配后变异任务done而非failed，断言exit101；不是编译失败或空测试。变异前后SHA及写错旧key读回见mutation-setup/mutation-readback。候选 never mutated，随后candidate exact同case 1匹配PASS，bin仍304140…；第一条绿对照误用测试名0匹配，保留NOT_RUN_ZERO_MATCH而不记PASS。

## 原任务份额

原R06任务书297—305行保留全部5 Steps及3交付，307—329行保留A13/A14。original-scope-review.json逐条含原文、期待、实际、状态与证据：124相关义务、24原到期独立叶、52原断言全部保留，没有将当前缺口改名share/deferred或移到R07。范围文字/ID采用冻结库存，结果全部独立生成，没有继承执行者结果。

| 原项 | 当前裁决 |
|---|---|
| Step1 导入至配置全链 | PARTIAL；原回归实跑，原所有导入/拒绝/副作用及EX07/08/09边界未全验。 |
| Step2 本地向量/来源/研究边界 | PARTIAL；显式scope/本地向量回归绿，未建research表不能证明旧research历史读取/展示兼容。 |
| Step3 身份、维度、参数与重建 | PARTIAL；F10身份隔离份额通过，构建重复请求/超量进度仍FAIL。 |
| Step4 fallback/持久化/取消/重启/schema | FAIL；真实running恢复失败；deadline/context、cache错误分类未关闭。 |
| Step5 不擅自恢复/删除，未决阻断 | PARTIAL；全部缺口登记，但登记不是验收成功。 |
| KnowledgeService/索引兼容矩阵/引用迁移测试 | 各PARTIAL，恢复与其它原边界未闭合。 |
| A13 | 原显式scope与原文引用限定案例PASS；不升级为真实聊天自动scope/F13 PASS。 |
| A14 | 不兼容模型/维度显式降级限定案例PASS；不升级为完整恢复或未测损坏BLOB PASS。 |

R05-T01四Steps/三交付/A01/A02的相关身份链份额也逐项保留：实际embedding路线、served、usage、variant已验；其它purpose/完整能力与不同凭据全矩阵NOT_RUN。生产五协议的build/parse/fingerprint是golden纯构建证据，不是五供应商HTTP或LIVE。真实模型采样/BE04本轮NOT_RUN，RSS实测null，8192MiB是守卫阈值而非峰值。

## 一次列全的发现

每项完整原条款、源码位置、复现条件、实际/期待、根因、全部影响路径、修复要求和必须重跑范围在findings.json。

- R01-F01 **FAIL**：claim_next_job只取queued，启动只重排pending_embedding，正式重试只接受failed。SIGKILL遗留running没有恢复入口。影响所有摄入、设置/源重建、重试、重启及未ready索引查询可用性。须以可信身份/单写者/稳定job检查点完成恢复，重绑F09/F11后全新Reviewer重跑F10四C及原Step4。
- R01-F02 **FAIL**：首批peek未消费，先写首批再全iterator重复首批。实际1块2请求、progress2/1；B89重建64+64+25共153/89；新增beta90重建154/90。ready检查前仍有成本probe。影响首次摄入、重试、重建、ready复用、summary和成本。须修复首批消费/复用及统一统计，重跑F09全部精确边界与F10四C；本轮没有冒充已经跑全F09 1/64/65/128/129矩阵。
- R01-F03 **FAIL**：Knowledge embed_served始终None deadline/context，usage等网络响应后才结算。真实SIGKILL第二请求已发，wire2仅usage1，首行Session/Run/Attempt/cause均null。影响后台job、hybrid query、知识工具、共享permit等待与迟到结果。须贯通可信上下文、稳定独立job预算、发前事实及重启未知结算，重跑F11四C/R05 usage/取消与F10四C。
- R01-F04 **FAIL**：实际原registry脚本exit1，MIGRATIONS9 vs registry7，缺v8/v9。须按原迁移事实维护登记、原脚本真实复验，不扩大白名单/退役门禁。影响R02登记及R03—R06正式放行。
- R01-F05 **FAIL**：原R03 run_lifecycle exact1匹配exit101。实际12事件含tool_result_model_projection，旧expected11未含；先前terminal/final-message断言未失败。本轮未跑纯bc完整基线，不能虚称F10新引入。须保留合法投影/一次terminal语义维护完整旧回归，重跑事件/投影与正式阶段链。
- R01-F06 **SOURCE_ONLY_OPEN / 两cache运行反例NOT_RUN**：两cache任意open失败仍remove db/wal/shm重建。原facts schema_newer检查绿不能关闭index/vector cache未来schema、权限、锁、I/O/并发保护。仍由F12按原四C独立完成，F10查询/恢复随后重绑。
- R01-F07 **NOT_FULLY_VERIFIED**：全部原24叶/52断言、旧research兼容、真实聊天自动scope、EX07/08/09完整边界和损坏BLOB直接负例没有被若干相关旧case全绿替代。全部仍R06必需，逐条见原份额矩阵。

## 真正执行与未执行

- 原受影响旧suite：service R05 operations7 + T07 knowledge11 + T07 tools6 = **24 PASS**；adapters knowledge store9 + operations27 = **36 PASS**。两区合计60个原case；专属九case、绿对照一case、变异红一case和旧事件FAIL一case另列，不混算。
- fmt/clippy workspace all-targets -Dwarnings、contracts、boundaries原入口各exit0。Cargo绝对1.98.1，所有构建/测试/clippy/xtask均--locked -j8。各testbin守卫600s/8192MiB ON；最长旧tools binary411.82s，没有关闭守卫或当峰值。
- 原directed legacy脚本亲跑适用E0—E4.5通过，包含真实原检查、类型/renderer/边界及正常clone独立镜像。E5全npm/审计seal明确BY_SCOPE SKIP，不能升级成全npm、seal或整层R05通过。
- 本Reviewer整层verify-stage R05/R04/R03、全Rust workspace tests、全npm/seal和未注册完整R06 gate均**NOT_RUN**；不继承执行者或总控历史正式结果。registry与旧事件原FAIL实际已亲跑，不以known failure省略。

仅外部loopback供应商响应替身；主C、新反例与变异被验内部Gateway/Operation/Knowledge/状态/SQLite均真实。原旧T07 suite有其原自造embedder/明确scope fixture，故只作该原回归证据，不能替代本轮真实Gateway身份链或正式聊天证明。旧R03 case原TurnProvider/ToolExecutor替身亦明确分开。

主C及新反例自己的effective LINGXI_HOME有实际READY与DB位置。原旧suite原样构造独立OS temp HomeSource::Cli，覆盖外层env，实际43个合成home/DB元数据及原配置来源见old-suite-effective-homes-final.json；不伪造env生效或READY stdout，不接触真实产品用户home。守卫资源与scope不为绿色改动。cleanup-final.json确认所有记录testPID及候选target运行进程均退出；新外部listener关闭、线程joined，legacy独占临时镜像被trap移除。合成home、凭据、private材料、完整mutant源、target只本地保留，不收Git。

## 最终固定

HEAD bc2ce51fd005ea75b9949156974e15d2761eb734；原lock d4f8c2977adbde14dd42e670205f1054fa7ef1ddceb9f0b13c190c2bfa9d6fd4。五owned全部与运行前相同，全部运行完整tracked有效输入摘要前后相同。

实际执行专属bin SHA 30414039c44816abd708082f87123756506c5c0e499e2c303cbc1e9bc72b1004；actual shipping SIGKILL/restart与新增别名反例均为86680bfa80908d169f7cd4e306ef50407bff9bbd6a00fe9e259f574ad5189924；mutant为6efbdbbd8c2cbe2d15dac98dd64bd1a37433dc1c464ee674b4c016593dc00140。全部实际执行bin及当前再读SHA一致，不能用执行者曾经构建的可变bin替换。本报告/原义务矩阵/findings与最终源/lock/bin完整SHA见final-binding.json。

safe-manifest.json只列本Reviewer必要报告、日志、脱敏合成wire/DB断言和元数据；排除所有home/config/target/完整archive及有合成credential字面的探针源码，完整分类见local-only-index.json。安全清单本身SHA在manifest-receipt.json，不加入自引用循环。原SHA历史/失败日志不覆写，本轮裁决为FAIL，Reviewer01结束，不承担下一轮修复或审查角色。
