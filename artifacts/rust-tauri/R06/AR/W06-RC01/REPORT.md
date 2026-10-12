# W06-RC01 全路径根因分析

状态：**ROOT_CAUSE_MATRIX_READY_FOR_NEW_REPAIR**。本代理为全新一次性 ROOT-CAUSE，仅做源码分析、历史证据核验和现役解析语义探针；不是 Executor、Repair、业务自检或第三层 Reviewer。本状态只允许交给全新 P03 继续，不是 READY_FOR_REVIEW / PASS。R01、R02 正式 FAIL 不变；P02 保持部分交接，不得复用本代理执行修复或审查。

## 输入与只读边界

实际主仓 HEAD 为 `2e1933d8f16d0f383b85ca92ff5bdf29e4732e6d`，分支 `codex/rust-tauri-migration`。P02 的15个 owned SHA 全部实算匹配，7个是P02新增改变。开始绑定见 `binding-start.json`，结束绑定见 `binding-end.json`。用户设置 SHA256 为 `0f72eb0a2549c95973c2aa8abe54c9aa44491fb9ca2a1c6e437404655d36b3d3`。只写本目录，未修改产品、永久测试、Cargo、控制文档、其它W02/W08/W12成果，没有Git写操作、真实资料/付费/外发/发布/R07。settings仅计算保护摘要，不输出内容。

已读取完整1219行不可变修复规格，F06四要求/C01—C04、§5.3、原T01/T06全部Step/交付/A01/A02/A11/A12，共同契约与原R05有效离线交接；后者含FINAL-04与独立Git收据，属于历史有效前置，不给当前新锁/消费者自动背书。历史E01/P01/R01/P02/R02报告、全部finding、候选、结果和相关原始失败证据已核验；五轮manifest共380项均无哈希变化，详见 `historical-integrity.json`。没有重跑其业务。R05旧执行中记载的进程方式不构成当前规避监督的许可。

`original-obligation-share-register.json`逐条保留354原记录（10 Step、6交付、4 A、263叶断言、31关联修复要求、28 C、12旧审查义务）及101叶组的原文、原owner、截止与未完成份额。SCOPE登记是需求证据，不能作为验收结果；全原Task/叶/A均未签PASS。原10月8日人类实际提示词捕获缺失这一SCOPE已登记SPEC_GAP继续保留，不阻断可以明确分析的配置契约。

## 根因结论

共同根因是：**先把YAML压成不足以保存类型/键身份的表示，再以字符串查找和默认值消费；消费与保存、缓存与副作用收据之间未形成一个可证明的完整边界。** “换成熟库”解决语法，不会自动解决其默认schema、宿主Value、序列化和业务错误传播。下表的历史实际失败来自原始证据；新P02风险只标源码推断。本人仅亲跑 `incumbent-typed-oracle.json` 的14个合成js-yaml样本，没有亲跑Rust或签业务FAIL。

| 根因 | 原条款 / 已有证据 | P02已经处理 | 下一轮仍需处理或证明 |
|---|---|---|---|
| RC01 行扫描丢结构与引号 | F06要求1—4/C01—C04；E01旧block正常1绿，flow/quoted-name两契约红；R01/R02准确旧扫描器1绿2红 | 所有已查Rust配置消费者接统一文档；I/O/parse错误传播 | 保留旧红有效性、真gather→wire正反例。函数名scan仍在但已委托完整文档，不是旧行扫描 |
| RC02 非字符串键及merge来源被默认吞掉 | R01-F01；真实flags/wire/项目技能/peer预览与规范化重复失败 | 源事件图验证root、agent/memory/experience/skills/workspace_context，含所有被覆盖merge来源；非字符串owned键明确拒绝 | 全敏感键、alias key、重复/merge优先级、未知深层合法值再验；不能只查展开后Value |
| RC03 普通字符串`<<`丢失身份 | R02-F01：105断言18违约；记忆/经验/姓名/工作区/peer/future PATCH有真实失败 | granit style/tag区分，serde-saphyr真正merge，serializer给普通`<<`加引号 | plain true merge与quoted/!!str/别名键/分裂%TAG/mixed merge并存均需实际读→改→读→wire，不可禁止合法字面键 |
| RC04 tag身份在类型校验前被擦除 | R02-F02：3真实未知tag样本，15拒绝/保字节断言全违约 | reject_unsupported_tags拒绝!custom等；standard!!bool/!!str/!!map/!!seq初检 | **尚有源码推断旁路：库认识本地!bool/!str/!map/!int，现役js-yaml全部拒绝；SourceGraph忽略值tag。** `memory: {enabled: !bool true}`、`agent: {name: !str 姓名}`、`memory: !map {enabled: false}`必须生产实测，不能凭strict选项名闭环 |
| RC05 Value/serializer改变兼容未知数据或接受错误敏感类型 | F06要求1/2/4、C03/C04，原T06-S03保存份额；R02 future字面层级已实际丢失 | 普通mapping/list/string/bool/null与字面键保存测试已加；统一render | **源码推断：!!binary解码成String，公共serde_yaml_ng::Value只收到visit_string，原二进制类型消失。** future.binary PATCH可能从二进制变文字；agent.name/locale/skills成员同tag可能绕过字符串实际类型边界。implicit timestamp变普通字符串、explicit timestamp/!!set/omap/pairs可能明确拒绝，须与现役区分，不能统称保真。未知非字符串深层键/整数范围/非有限数/Unicode块标量往返也需有限语义电池 |
| RC06 解析失败在副作用之后才发现、收据失真 | F06错误边界、T06-S03/S05、共同工具收据契约；P02 red-install-gateway-02：正常对照绿，3坏tag真实installed=true却虚报enabled，1测试exit101 | install前read_flags；enable_for_agent错误明确“已安装但启用失败” | 安装前错误零安装+配置不变；安装后配置变化/写失败必须有实际installed/未enabled收据，不能假回滚。**源码推断：delete_skill先删技能再scrub解析peer；坏peer可能删除后Err，read_dir/entries/bundles吞错仍存在**，先真实合成验证再做最小诚实闭环，不能借EX03边界免掉配置消费者 |
| RC07 前置SourceGraph不受后置预算约束 | F06完整解析/错误边界与原通用完整错误/资源契约 | 成熟serde-saphyr默认预算/alias限制；granit默认collection nesting255 | 所有事件先收Vec、图先构建后才进入serde预算；第一遍没有总event/node/scalar/输入字节约束。普通深度不是无穷（granit255），但浅大流仍先无界分配；read_to_string先全读。须在同一输入入口与前置阶段执行有限资源界限，正常大配置对照和越界明确错误；不移除guard、不另造语言 |
| RC08 写后新会话读取不等于同会话下一Run生效 | F05-C01及T01-S03；P02 initial-four-c-01原日志真实FAIL：flags已false、旧prompt仍有memory | P02新session证明重新采集/保存，仅此部分 | ContextCompiler缓存(session,bool)早返回，Agent/材料代次/失效缺失仍在。F05 OPEN/STALE，交原owner；W06不通过删同session断言或新session绿声称动态生效。最终组合必须复跑F05/实际wire与观测 |

## 完整生产链与同族旁路

每条路径的源码锚、入口、默认、写与重载要求详见 `root-cause-matrix.json` 的 `paths`。已独立从全Rust `config.yaml` / `AgentConfigPatch` / `scan_agent_config` 搜索确认：没有另一套现役行扫描配置读取；仍有非配置的frontmatter/Markdown扫描，它们不在本根因自动扩成全语言改造。

1. 认证execute → SessionStore/RunSupervisor → runs.rs:1166 `compiled_for_run` → context_compiler.rs:628 gather → :388 parse → AgentConfigDocument → ContextArtifact render → provider；bootstrap lib.rs:1514绑定SkillMaterialPort。cache命中绕开重新采集属于F05，不以“统一parser已接线”免除。
2. context_compiler.rs:569 roster读取其它存活目录，逐agent解析姓名，再把默认与名单放入真实材料。peer坏配置必须诊断，不能因当前agent合法而漏校验。
3. memory.rs:62 locale；:117 flags；:128 patch；:325 health；experience GET/PUT :1024/:1054与feedback :1150，配置HTTP PATCH/GET :1202/:1257。记忆缺字段默认true、经验默认false、project默认true、compatible默认false；默认只能应用于**合法缺字段**。feedback原吞parse已去除，但写入/暂停的现役独立业务语义不由F06重设计。
4. persona.rs:70/:86/:103 locale/yuan与模板回落；真实字符串、wrong tag、未知值不能误选persona。配置先读失败响亮，locale偏好与人格模板业务仍按原份额保持。
5. skills.rs:326读取lists/policy、:370 list、:426 set全名单、:452单项启停和workspace_disabled、:941 runtime catalog；所有修改调用:348统一write后atomic_write。flags/lists每次都完整parse；配置缺文件的管理面404与runtime空名单形态分别有既有契约，不能机械把全部改为默认。
6. skills.rs:709 delete→:743 scrub跨agent引用；skill_library.rs:658 peer引用→:807 orphan preview。普通读取/preview无删除；scrub目录读取/flatten/file_type/缺路径分支与bundles.remove_skills的吞错必须按真实资料与缺目录分开。删除成功后再发现bad peer不等于零副作用。最小配置拒绝/部分结果由W06证明；完整包格式、GitHub开关、同步、组合等EX03仍OPEN。
7. agent_memory.rs:1911 flags/lists、write_flags/write_skill_lists→AgentConfigDocument::set/render；:1970 patch旧文档+候选验证后atomic_write。SkillService及MemoryTools writer直接atomic_write，必须证明它们render后可再次用同入口解析并保持未知语义，不能只用磁盘contains判断完整保真。并发配置版本协议仍按EX03/R07原义务处理，当前安装期间变坏的错误边界不能跳过。
8. 真ToolInvocationGateway → MemoryTools :343 tenet复核（paused成功说明但零提案、错误Failed）与 :447 install（先review/安装，再:572 enable）。外部guard/model可以loopback，Gateway/配置/文件/收据必须真实。Tools安装的前置配置与落盘之后配置变化分别验证。

TS现役对照是lib/memory/config-loader.ts实际js-yaml.load/dump；core/agent.ts:1337保存→清缓存→重载及记忆/技能刷新；core/skill-manager.ts:200名单/策略；lib/skills/skill-removal.ts peer/scrub。现役的历史吞错不是Rust继续静默降级的许可。原支持的standard!!标签、custom local标签、non-specific `!`、unknown深层数据分别处理，不新增另一套手写schema。

## 对P02依赖方案的证据判断

两个新直接依赖均锁定1.3.0，8个新registry包，既有registry版本不变；serde_yaml_ng=0.10.0继续承担公共Value和标量键类型判定。成熟parser实际接口来自已下载且Cargo锁定的官方registry源码，来源/文件摘要写入矩阵。web对docs.rs三页返回不可达，未据空页面推断；使用本机锁定源码作为第一来源，没有安装或换模型。

granit事件图是结构校验层，按parser Event/style/tag/anchor操作，不是手写YAML语法，本方向可保留。独立关注在于：它与第二遍serde-saphyr/第三方Value的**同一schema、实际tag和预算**必须证明一致；图仅记录key/merge，丢掉value tag，因此不能替代值类型校验。serde-saphyr `de/tags.rs`明确列!bool/!str等兼容别名；`de/live_events.rs:551` strict允许其SfTag；`de/deserializer.rs:763` generic visitor擦tag、:526把binary解为UTF-8 String。官方实现事实支持RC04/RC05的源码推断，但本人没有执行Rust输入，不能写实测FAIL。

serde-saphyr默认预算存在：events1,000,000、nodes250,000、depth64、scalar64MiB、aliases50,000及replay limits；reader byte256MiB**只作用于reader**，当前from_str不限制已经读入的字节。granit Options默认flow/block255；因此RC07不是“解析器完全没有保护”，而是前置全量阶段早于保护。错误诊断应包含配置位置/原因并避免输出秘密，不重新设计全局错误码。

保存格式/注释规范化与现役dump类似，是已登记行为；未知值类型和层级的静默变化不属于格式变化。不能仅比较两个同样擦tag的Value或再用serde_yaml_ng.apply_merge充当原语义oracle。新oracle实际Node24.16.0/js-yaml4.3.1拒绝!bool/!str/!map/!int，保留!!binary Uint8Array与timestamp Date；它仅是读取/保存语义证据，不是完整旧Agent运行或当前Rust证明。

## P03最小完整修复与验证矩阵

P03须先核实15SHA与停写，保留P02部分成果及全部失败，然后一次处理矩阵，不只补最新报错。以成熟库实际节点/标签语义建立读取、验证、消费、保存、重新读取的统一保证；不能恢复扫描、正则预处理、自造新语言、删除合法`<<`、全默认关闭、删旧断言或扩大原需求。新增源码推断必须先正常对照，再实际生产反例；不成立则有证据关闭推断，不强改产品。

必须覆盖：

- C01：block/flow/anchor/true merge/显式覆盖/merge序列优先级，主记忆与workspace/material真wire；C02：单双引号、转义、#/:/Unicode、BOM/CRLF、literal/folded/chomping和保存后重读。
- C03：所有root/owned字符串字段/布尔/list/list成员异常类型、quoted false、复合/非字符串键、重复/规范冲突、被覆盖merge源、recursive/undefined aliases、多文档；普通字符串`<<`不得提升同名深层字段。standard!!、verbatim URI、%TAG split、non-specific `!`、库local别名、custom tag必须分开。
- C04：缺文件/空/注释/部分/example脱敏样本，不支持/损坏/非映射不混成默认；未知普通值、nested非字符串键、binary/timestamp/标准集合与特殊数字进行现役→当前parse→PATCH flags→启停lists→render→同入口重读→js-yaml语义对照。支持必须保原意，不支持必须明确拒绝并保原字节，不能默默降级。
- 路径业务：experience GET/PUT/feedback、persona locale/yuan、roster peer、技能list/policy/enable/disable/组合调用set路径/runtime目录、peer cleanup preview/scrub；真Gateway tenet拒绝零落盘、install前bad config零安装、安装后enable错误真实部分收据。合成删除反例先守明确目录、实际检查文件/全部config/收据，绝不碰真实或共享资料。
- 资源：正常宽/深/alias对照，前置byte/event/node/depth/anchor/merge与alias replay边界；超限明确错误、原文件不改、零wire/安装/提案，受监督进程正常回收。选择产品现有允许规模内的合理上限，不任意砍合法普通配置。
- 历史旧红：原flow/name旧红、R01复合/peer旧红、R02字面/tag旧红、P02有效install旧红均保留并核候选；至少真实新的!bool/!str/binary对照当前生产路径，不把编译失败/fixture错误/零筛选当红。

最终精确P03候选在新隔离副本、自有target、实际合成LINGXI_HOME/READY下复跑全部四C、全部历史同族及新增边界、原77定向回归、fmt、workspace/all-targets clippy -D warnings、contracts、boundaries。Cargo使用绝对`/Users/study_superior/.cargo/bin/cargo`1.98.1，适用命令--locked -j8，guard8192MB/600s、父监督、所有初次失败/重试保留；不复制/污染真实home。测试fixture及binary与输入SHA前后不漂移才是有效最终窗口。P02初检03有20绿，但owned前后4项发生变化、binary也变，最终格式SHA未再跑；必须标STALE而非最终证据。

F05同会话动态配置、F07声明目录、EX03完整技能生命周期、F01历史/父子材料以及F08等原剩余份额各自OPEN，不由本矩阵关闭。F14/F15最终组合重新绑定所有受影响场景与原354记录/101叶份额；最终stage须完整workspace、registry一致性、正式R06门禁、受影响R05→R04→R03→R02闭包，并保存每层实际命令/exit/绑定。R02 v8/v9登记缺项历史保持，W06不擅改门禁或免除。目标OS/打包/LIVE未验如实记录，合法递延保原owner/截止。

## 交接

本目录输出的SOURCE_ONLY矩阵已经形成；无平台阻止本次分析，未重试受阻Reviewer、未请求Daybreak、未改变模型/权限或绕审批。只读节点探针与历史核验不算两层业务自检，更不算独立验收。交接后本代理立即结束，由**全新P03**实施/验证，再交**另一全新Reviewer**。
