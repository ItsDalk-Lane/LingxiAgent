# W07/F07 全新一次性执行派发

你是全新空历史执行实例02，只负责 R06-AR-W07/F07，不是Reviewer，不复用其它任务执行者。执行工作区、准确HEAD、own dependencies/新target由总控实际准备回执追加后派发；没有派发不启动。不得改主仓/其它候选。唯一目标是R06，不执行R07。

先完整读取 ENTRY-01/specification/R06_完整问题清单与修复验收总控提示词.md 的1219行，原9共同资料与原 R06-T01/T06/T07/T08 全部Steps/交付/A01A02A11A12A13A14A15A16、R00全部到期叶份额，前置R04/R05工具目录/网关/代次/批准/规范化ToolCall/usage与所有消费者。原Taskbooks被忽略，使用rg --no-ignore确认，不能冒称不存在。原10月08提示词缺失精确SPEC_GAP，不据此减需求。读取 AR_ORIGINAL_OBLIGATIONS.json/SCOPE-01/全部旧Task和Stage未决。

总控只读准备 `artifacts/rust-tauri/R06/AR/W07-PREP-01/source-contract-readback.json` 是源码线索，不是实际红/绿或预定方案。实际 HEAD 先核对、不强制回退。纯已接受bc仅包含F08；F01/F03/F05/F06/F09/F10/F12候选未接受，不任意导入和继承旧PASS。需要依赖必须登记准确源文件SHA，原范围仍必须验，相关份额STALE后组合新执行者/新Review。

## 当前源码线索与完整修复边界

kernel/model_exchange.rs ToolDeclarationSnapshot::from_registry 只筛Available，description=display_name；service/knowledgetools.rs和memorytools.rs无条件Available；service/lib.rs真实注册。ToolRegistry有discover/describe/generation，Gateway把Deferred标OnDemand，但方法/HTTP接口不等于模型能发现并执行。Rust context纪律字符串提及mcp_search_tools/mcp_describe_tool/mcp_call，旧TS core/tool-catalog-bridge.ts才有模型桥完整实现。必须自己追踪实际消费者，不凭总控文本认定全部不存在。

完整区分“注册/可调用/常驻/按需”，保原四基础常驻+原契约确需发现/说明/调度，不能机械压成4。修正full manifest description->真实五协议wire/name/schema/generation/target同源。扫描knowledge/memory/skills及其它新接入同根因。不能仅将Available改Deferred而关闭入口/削减用户能力。

实际模型 discover→describe→call 必须接真实当前目录/真实Gateway/原权限/批准/资源规范化/执行目标/收据/取消/usage；不能由替身先知道target私自直调HTTP或手工给模型装scope冒充生产。允许采用现有原目录桥或明确等价接线，不扩大成微服务/重写UI/完整真实MCP服务。模型自报owner/Run/Agent/generation/资源不能获得可信身份。不同source同名精确disambiguation，stale description不能执行新语义对象。禁用/撤权包括直接/按需/子代理/可信宿主路径，原LocalUser合同保留，不为拒绝测试杜撰新的全体owner限制。

原R05 ToolDeclarationSnapshot在每次send时读取真实目录generation，当前目录/权限需独立复核；F05冻结当前Run persona/memory/system材料，不授权无依据冻结所有call工具目录。旧call声明与材料观测必须保持当时真实内容；实际每call声明变化需要来源/映射/完整协议历史工具对，不用新列表猜旧wire_name。真正移除/重名/变代次时响亮处理，不能静默删旧历史或重放未知工具。

## 两层实际自检

C01：正式默认服务真实bin+认证聊天+loopback，未选知识来源普通首请求；严格actual resident set+必要infra，工具manifest可用但非resident阴性，wire system+tools成本使用原估算/真实记录明确边界，未测=null；不以system无说明代替tools不存在。
C02：真实模型依实际目录返回结果再请求describe/call；期间屏障更新代次，旧定义拒绝新对象、重新读取才合法。保真实原始请求顺序/内容/实际执行/收据，不能替身预加载全部工具schema。知识scope暂未接线不假装C02已完成，必要最小可合法来源路径明确，与F13关联原义务OPEN。
C03：五个协议族真实外部wire工具说明包含唯一参数约束原文；真实name/schema/generation/同名source正确，非display_name。description改回name隔离变异须准确红，0匹配/不编译不算红。
C04：激活后禁用/撤权/来源切换，通过direct和deferred/子代理等真实适用链，确切零blocked dispatch、合法路径保留；原固定行为评测schema/参数/结果/任务四维分别计算，A15/A16及BE-10负对照不退役。原T08 full评测不足不能推到R07。

正常正对照→真实未修行为红→同族根因与最小完整修改→所有C/原Steps/A/leaf逐项期待/实际/证据→第二层竞态/错误/取消/同源映射/原根因隔离变异。外部model loopback可替身，当前编译器/ToolRegistry/Gateway/Storage/真实driver/消息组装不得mock。检查真实SQL/event/usage、原完整wire，而非函数存在/日志PASS。

原R04工具目录/批准/基础工具/动态策略/桥/子代理/资源/R05 model_plane/binary_wiring五protocol/closed-loop/取消/配额/当前F08七tests及语义相关原回归全部适用。真实fmt/allworkspace-alltargets clippy -Dwarnings/contracts/boundaries/registry/wholeworkspace，并按影响正式R05→R04→R03→R02。原registry9vs7红保持F14OPEN，不改白名单/相等语义/旧SQL来抹红。运行新R06正式门禁未接线不能预写PASS。完整所需缺口NOT_RUN/FAIL/BLOCKED清楚区分，不凭已登记减少范围。

## 证据和监督

全新 W07-E02/自己的target-w07-e02；Cargo绝对 /Users/study_superior/.cargo/bin/cargo --locked -j8 当前1.98.1，Node24.16。每testbinary600秒/8192MiB ON，whole外层2400+有界准备；R05原7项顺序总预算19800+准备，不600误杀整套。不脱离宿主/孤儿化，真实终态/reap/owned cleanup。

每命令argv/cwd/start-end/真实exit/匹配数/guard/platform/mock边界，actual synthetic LINGXI_HOME+READY/SQL路径/来源、完整有效Rust+非Rust+runner+外部输入SHA/锁/实际执行bin before-after。before缺失不可回填，Cargo不同bin窗口诚实保留；未测RSS=null。原测试自造home要记实际home，不冒充外env。所有失败/准备错误/宿主终止/缺失final/0匹配都保留不算PASS。

synthetic homes/配置凭据/密钥/targets/bins/源码archives LOCAL_ONLY_NEVER_GIT，不读写真实用户数据或实际外发/paid，无Git操作。自己的必要清理只能确认进程退出且精确owned；不清理其它候选/旧证据。

最终冻结 REPORT/results/ownedSHA/锁/bin windows/SOURCEFAMILY全族生产消费者/原scope全Steps A叶原文/全部失败与重试/逐项与对抗自检/remaining影响STALE/safe evidence manifest/HANDOFF。只能READY_FOR_INDEPENDENT_REVIEW，结束不继续下一任务。总控将创建全新Reviewer实际验收，不自签。平台实际中止保存原文并只停该动作，禁止变措辞/模型/权限绕过。

新增实际同族OPEN（须最终W05原record绑定后独立重现）：R06-AR-F07-OBS-LIVE-CATALOG-HISTORY。W05执行者已实际观察call1宣告read后禁用，Gateway正确forbidden/零dispatch，但call2按实时目录移除read后原协议builder无法编码历史read工具对，local provider_error且没有第二HTTP。保持当前send-time宣告/权限与旧调用身份分离；不得重宣告禁用read、冻全Run工具目录、扔掉旧tool pair、猜wire target或改松断言来造绿。F07 Task须覆盖全协议/跨Run历史同族调用，既有F01候选受影响要标STALE并后续实际重验；平台阻塞的F01 Review不能替代，本Task正常独立职责不绕审查。合法目录description/generation改变的两次wire正控不代替禁用负控。具体W05 finding/安全manifest完成后交接，当前只是有来源的实际报告待root精确证据收取。

正式生产二进制资源目录补充：若实际 executable 在工作区外的 owned target，原 resolve_product_dir 无项目祖先会回落 synthetic HOME/product；先实际确认内置 yuan/identity 材料位置。使用受支持显式 LINGXI_PRODUCT_DIR=本候选 ROOT/lib、真实记录有效env及材料SHA，使原业务断言运行；保留故意测试缺材料/错误配置的负例，不静默fallback、不补写旧env。W03同actual bin/原C02 exact1matching仅此环境差异正控真实PASS，记录为 artifacts/rust-tauri/R06/AR/W03-PREP-01/external-target-product-dir-positive-readback.json；未来演员不继承该结果，自己实际证明。

后续新运行资源约定（不改变当前活动运行）：合成资料根指 LINGXI_HOME，不是覆盖 OS HOME；OS HOME/系统工具与用户设置继承原值，所有实际服务/测试的有效 LINGXI_HOME 必须是本轮独占合成目录并证READY/路径来源。TMPDIR使用工作区外独占目录。针对已实证并行增量缓存造成的磁盘压力，后续新独立构建可明确记录 CARGO_INCREMENTAL=0，减少可重建中间缓存；保持原toolchain/--locked/-j8、产品特性、正式命令、断言、每bin guard600/8192 ON和原预算不变。把实际环境/产物执行时前后SHA加入绑定，不能把这个编译缓存选择写成关闭guard或隐瞒编译差异。已有已启动链不改环境。

临时缓存清理如确需执行，先证本任务归属/退出/无句柄和真正执行程序的SHA；权限位X_OK会把零字节incremental .lock误算成程序，精确被授权cache子树内锁文件的预期删除不得冒称真实程序变更。完整before原记录保留，缓存子树外全部executables与所有实际执行路径逐SHA核对。原helper若exit1就如实保留；未知原lsof/raw/free-before不能事后回填。清理不是行为验收，不能冲销原FAIL。


## 第二实例准备：前一候选源码彻底删除，必须重新建立证据

本文件仅PREPARED_NOT_DISPATCHED，不代表新角色已创建。前一执行者传输断开，没有最终冻结交接；用户随后明确隔离工作副本已彻底删除。外部控制目录仅保留六项中间源码备份及部分运行资料，不能冒称完整候选或套用其旧绿。新执行者须从实际已提交bc基线的独立普通clone建立当前最小完整修复；可以只读核对旧六文件作设计线索，但不能将中间版本或源码假定为已验证最终版本。

需要重新亲跑所有四C、五协议真实目录/完整描述/工具调用历史、重启跨Run及失败/取消后的真实配对恢复，严格保F08有限投影/raw独立留存和权限动态更新。原R04矩阵/closed_loop/T08外部模型替身如沿用未宣告私调disabled/deferred目标，须把替身接真实describe→call但保原授权/拒绝/语义效果全部断言；不能修改生产网关来允许未声明调用，也不能把旧逻辑的结果数变成新常数。任何typed历史缺可信映射必须响亮失败，不猜当前新target或重执行旧工具。全F01（分支/子代理/非文字/中断等）若仍未完整实现和实际覆盖，原义务明确OPEN，不以最小User恢复自签F01。

前W05 Review部分材料丢失但原执行安全记录仍存，root此前实际独立观察的撤权失败保留；新执行者仍需自己的旧红/新正控/变异。禁止复用旧Actor，禁止重试任何Daybreak未确认的受阻Review。当前Task正常重建修复不是旧Review的替代。允许在自身clone只读git status/diff/rev-parse记录，不commit/push或改主仓。所有新源码和安全证据冻结时留下可精确重建的当前owned文件字节及manifest，敏感全archive/home/bin仍LOCAL_ONLY。

## 第二实例实际准备坐标（仍未派发）

2026-10-11T23:24:36.338322+00:00：自己的普通clone `/private/tmp/lingxi-r06-w07-executor-02`，真实HEAD `bc2ce51fd005ea75b9949156974e15d2761eb734`，实体.git目录；没有导入主仓未接受源码或用户设置。188份输入逐字节SHA核对、6份中间备份仅在 `artifacts/rust-tauri/R06/AR/W07-PREP-01/intermediate-design-only` 作设计线索；不能视为最终产品。W05当前F07撤权失败的原安全finding/SQL/wire已复制，原二进制是否当前保留没有从历史字段推定。node_modules为自己的APFS复制实体目录，准确锁差异在 preparation回执；Cargo绝对 `/Users/study_superior/.cargo/bin/cargo` 真实1.98.1。所有tests NOT_RUN，角色未创建。回执 `artifacts/rust-tauri/R06/AR/W07-PREP-01/executor02-workspace-preparation.json`。未来正式派发后用新外置target、自己的合成home、guard与完整证据；禁止继承任何旧绿。

## 补充原F01完整源码恢复线索 2026-10-11T23:29:45.798130+00:00

总控找回原W01 SHA绑定补丁，以bc精确Git对象机械还原25/25全部全文SHA匹配文件（非只中间版本），仅复制到本工作区 `artifacts/rust-tauri/R06/AR/W01-CANDIDATE-RECOVERY-01/frozen-source` 作为必要跨Run/完整工具历史接口的只读线索，未覆盖任何产品。其manifest记录所有原SHA/patch/base/真实check/apply。若本F07完整接线确需其未接受接口，执行者先读全部生产消费者并明确登记采用的准确字节/修改范围，自己完成本轮完整稳定运行；不得继承F01旧绿或假设其独立验收已通过。所有受影响F01/F03/F05/F08原份额STALE，原F01 Reviewer Daybreak阻断依然不重试、不替代。不要为了避开恢复好的完整角色历史而退化成只存User/猜tool映射的最小假修复。
