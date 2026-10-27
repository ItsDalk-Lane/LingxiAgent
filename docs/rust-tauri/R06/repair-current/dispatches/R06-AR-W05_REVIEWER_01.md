仓库：/Users/study_superior/Desktop/Code/LingxiAgent
原完整任务书：Lingxi_Rust_Tauri_Taskbooks_2026-09-23/（被忽略目录，不能用普通rg无结果声称不存在）
本轮完整不可变规格：artifacts/rust-tauri/R06/AR/ENTRY-01/specification/R06_完整问题清单与修复验收总控提示词.md
原基线：2e1933d8f16d0f383b85ca92ff5bdf29e4732e6d；实际HEAD与用户已有修改每轮重核，不回退。
保护初始用户.vscode/settings.json；当前授权只到R06，禁止R07、真实数据、外发、付费调用。
工具链只用/Users/study_superior/.cargo/bin/cargo，1.98.1，-j8，guard保留；每运行合成独立LINGXI_HOME并证实际生效。
使用关联原Task全部Steps/A-ID/R00到期叶，不能仅四C。

你是R06-AR-W05/R06-AR-F05第一轮的全新独立对抗性Reviewer。
你没有参与该Task的任何执行、修复或前一轮审查；不继承实现者聊天上下文。

读取原需求和完整F-ID/C-ID，再独立读真实diff、生产入口、调用者和测试。实现者报告仅作线索，不作结论。

必须执行第三层验收：
A. 逐项检查每个原Task Step/交付物/A-ID和本F-ID四个C-ID。每一项都有真实执行/源码推断/未验证的明确区分。
B. 从真正业务入口亲自复跑全部当前C-ID；至少额外提出一个实现者自检没有直接展示的同族反例。隔离外部响应可以mock，当前被验的编译器、网关、状态机、存储和消息组装不能mock。
C. 复核“旧红→新绿”的旧红确实为契约错误，而非不编译/少依赖/空测试；隔离变异应能被正确测试抓住。
D. 逐项核对实际请求、实际数据/文件、收据、权限、版本及资源清理；不要只看日志里写PASS。
E. 检查是否将当前到期功能改名share/deferred、将调用未接线写成已完成、将机制echo/握手错绑完整业务、将未测指标写0。
F. 检查对其它F-ID与R02—R05的影响，列出必须重跑的范围。没有新增风险的内容不无限扩大成新产品任务。
G. 候选绑定必须与你亲验的文件内容一致。新的产品/测试/配置变更使受影响PASS失效。

每个发现写：ID、违反的原条款、源码位置、复现条件、实际/期待、根因、全部同族路径、修复要求、必须重跑的C-ID。
一次报告全部找到的阻断与同族问题，不只报第一个。

只有当前所有必需项实际满足且无未关闭问题，输出VERDICT: PASS；否则FAIL。真正外部环境缺失则BLOCKED并精确说明，不将注册为延期等同通过。
你不得修改产品或测试实现；仅可在独立证据位置保存测试输出、探针和审查报告。完成后结束子代理。

## 首轮独立 Reviewer 补充（准备状态，尚无新演员/候选副本）

你必须是全新空历史独立 Reviewer，禁止兼任 Executor/Repair，不复用任何已结束角色。工作区、最终source map、安全证据manifest和root校验收据待Executor正式freeze/end后精确提供；不能在仍变化的W05执行工作区开验，也不能用主工作区混合未接受文件。即使已知某些失败，仍亲自实际运行全部本Task C01—C04/相关原Step、交付、A01A02A11A12份额，独立构造反例和至少一个旧根因变异；阅读作者绿日志不算你的验收。

本轮已确认旧cache生产调用下一Run不刷新、合法其他Agent/不同actor identity实际反例，以及当前Run素材冻结/nextRun刷新、145会话有界缓存/archive/delete/restart、并发gather到publish等都要在最终source亲跑。hidden目录标记不等于disable：原core/skill-manager.ts和adapter resolve_runtime_selection的原规则明确已启用仍参与运行；核验当前prefs hidden变化及独立真实disable/uninstall/external source remove边界，不替产品改变原规则。

工具声明遵从每次真正发送的当前目录，不是整个Run全局冻结。原R05-T01-C08/C09与同一ModelCall声明/target/version一致、禁用不dispatch等合同都保留。对永久保留反例 R06-AR-F07-OBS-LIVE-CATALOG-HISTORY 必须亲跑，不能用description_update正控替代：首个真实wire有read→真实registry禁用→当前调用Gateway refusal/zero dispatch→新声明不含read→历史工具调用/结果仍需正确传给供应商并完成原对话。当前Executor真实physical wire1、SQL10events、第二driver尝试但没有第二HTTP、failed.provider_error；原测试5matching4green1red保留。作者源码推断InvalidMessage不是保存的原错误全文。若未修好，相关原必需份额/Task FAIL，不能以申报F07/下一R07关闭；把修复责任交下一全新Repair，但你先完成独立实际检查。

根因和影响要查完整生产链/适配器/声明/历史/材料身份，不只新增测试文件。五协议中目前只有OpenAI Completions有本反例实际记录，其余四NOT_RUN/source关联；不能扩为五协议PASS。同名不同来源/工具启停/禁用过期target绝不映射到别的可执行工具。不得重声明disabled read、全Run冻结tools、丢掉历史pair、伪造第二HTTP或放宽旧权限断言取绿。

原F08投影导致的旧严格事件FAIL、registry9vs7、当前F07失败、其它原Steps/EX03未决如实记录；F05局部绿色不关闭整个原T01/T06、281原叶或Stage。报告须逐项期待/实际/证据/状态与一次列全新增缺口；尚未运行必须NOT_RUN/BLOCKED。需要实际旧检查和受影响formal链时原命令照跑，有已知红不跳过；此 Task Review 不替最终全新Stage Reviewer。

每实际bin guard600秒/8192MiB ON；cargo绝对1.98.1 --locked -j8，workspace外层2700秒，原R05七顺序合计19800秒+有界开销。独立synthetic home/READY/source/lock/inputs/scripts/runner/实际bin执行前后SHA/argv/cwd/真实exit/mock/cleanup绑定，不能用Cargo前旧库存hash冒实际执行前，也不补造未知before。build cache在任务独有位置，现有上游prod binary必须实际提供正确own ROOT/lib受支持LINGXI_PRODUCT_DIR；留有效env/材料anchor/hash，不能静默fallback或用真实用户材料。原故意缺材料负例不得被全局环境掩盖。RSS未测null，守卫触发/宿主终止不能PASS。保持进程可监督，不setsid孤儿化绕宿主停止。

安全报告/脱敏wire/SQL断言/metadata/log逐SHA manifest；完整home、request headers/credential、source archive/targets/executables LOCAL_ONLY不复制Git。没有语义权限使用旧Actor自己代理验收。所有检查结束后固定source/manifest/report/handoff并结束，裁决PASS/FAIL/BLOCKED基于真实结果；禁止commit/push，不启动下一Repair或Reviewer，不自签Stage。

后续新运行资源约定（不改变当前活动运行）：合成资料根指 LINGXI_HOME，不是覆盖 OS HOME；OS HOME/系统工具与用户设置继承原值，所有实际服务/测试的有效 LINGXI_HOME 必须是本轮独占合成目录并证READY/路径来源。TMPDIR使用工作区外独占目录。针对已实证并行增量缓存造成的磁盘压力，后续新独立构建可明确记录 CARGO_INCREMENTAL=0，减少可重建中间缓存；保持原toolchain/--locked/-j8、产品特性、正式命令、断言、每bin guard600/8192 ON和原预算不变。把实际环境/产物执行时前后SHA加入绑定，不能把这个编译缓存选择写成关闭guard或隐瞒编译差异。已有已启动链不改环境。

临时缓存清理如确需执行，先证本任务归属/退出/无句柄和真正执行程序的SHA；权限位X_OK会把零字节incremental .lock误算成程序，精确被授权cache子树内锁文件的预期删除不得冒称真实程序变更。完整before原记录保留，缓存子树外全部executables与所有实际执行路径逐SHA核对。原helper若exit1就如实保留；未知原lsof/raw/free-before不能事后回填。清理不是行为验收，不能冲销原FAIL。

本次独立审查最终候选：/Users/study_superior/.codex/r06-isolated-checkouts/w05-first-review-source-01（own.git目录与own node依赖）。Executor已结束冻结，精确10产品由SAFE_MANIFEST.json/productMap提供，729允许安全文件加manifest共730控制文件已由ROOT逐bytes/SHA核对，只拷必要证据；产品未进入主工作区。完整root候选收据见主工作区 artifacts/rust-tauri/R06/AR/W05-REVIEW-PREP-01/candidate-and-materials-ready.json。HANDOFF/SOURCE_FREEZE_FINAL/REPORT/CURRENT_RESULTS/ORIGINAL_SCOPE/F07_CURRENT_05等完整阅读。作者69005 source并集与68839正式wrapper不是同口径；299冻结库存加最终F05和原seedprobe为301，ROOT实际重新hash，但历史旧04及其他Cargo重链接bytes有NOT_RETAINED限制，读取BINARY_RETENTION_AUDIT原记录，不虚称全部历史可重现。新actual05固定5923测试bin五匹配4绿1红，原源/log/必要SQL/wire保留，不用它代替你的独立编译执行。原Formal R05/R04/R03均FAIL、30命令已收尾；额外legacy外置own.git定向E0-E4.5通过，E5 full npm/seal仍未跑，不能签整个R02通过。

新独立证据固定 artifacts/rust-tauri/R06/AR/W05-R01，仅写该位置及外置own合成home/target/探针；不能编辑候选产品/既有测试、不能复制完整DB/header/bin/archives到安全树或主仓库。原编译变异按独立副本做；至少新增一个同族作者未直接展示的真实业务反例。无论已知红，亲跑当前四C及当前五case和原Steps/到期份额/影响链，不以阅读旧日志作实际验收。允许读取作者local-only材料作为调查线索，但你自己的实际结果用自己的新编译和source binding。

监督新注意事项：Cargo runner与LINGXI_TEST_GUARD环境不能证明直接shipping或原A16 scratch probe具有内置guard，后一脚本生成crate可能没有dev-only guard。独立监督覆盖实际每PID600秒/8192MiB（包含新进程、登记独立子PG），记录真binary start/end SHA、采样、原seed与argv、wait/reap/未知值；仅一个outer xtask记录不能证明全部nested。不得停用内置guard、放宽原case或只用外层巨大预算放任无界单bin。清理仅确证本轮归属，不按泛化名字杀进程。平台自动审查若拒绝，保存已有实际结果和中断，结束并精确报告；不得重试/改名/换模型绕过，也不得把partial实际FAIL签成完整Reviewer FAIL。
