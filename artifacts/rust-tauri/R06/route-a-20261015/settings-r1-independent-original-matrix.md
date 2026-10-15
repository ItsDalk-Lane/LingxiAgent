# 原19组逐项核对：206 当前不可 FULL / 候选 NEEDS_FIX

这是对上一份“本候选实测路径PASS”的补充并更正总体判定。原12实际窗口/2父21子通过保留，不能由44个check名称推导19组全部FULL。本次逐字读取现行 r06_original_leaf_bindings 的206/0B/3516原assertions；原文字未改。总体 NEEDS_FIX：新独立真实跨owner旧缓存误写已证实。

[P1] S5：RustAgentTab在新有效连接加载期间仍挂AgentTab，后者直接读全局旧settingsConfig/agents且通用编辑未绑数据owner。自己的小型独立Electron脚本使用现成当前CLI（与testbin同01:51:28编译，晚于最后Rust源），两个真实服务分别有同ID/不同名称助手；经真实HTTP正常创建，不绕过epoch门。生产settings.html在A完整加载后，将所选registry换B，真实网络只延迟B identity GET（不编造响应）。仍见A名称且input.disabled=false，输入后回车，真实B PUT config 200，B磁盘名变“A旧页误写到B”、A原名称保留。脚本最终以 EXPECTED_OWNER_ISOLATION_FAILED 退出1，证据 result.json、完整浏览器请求及转发状态、磁盘前后实际内容。冻结源与Main一字未改、无新target。

独立脚本 /tmp/r06-agent-settings-independent-final/cache-owner-ui/{orchestrator.mjs,main.cjs,preload.cjs}；结果 /tmp/r06-agent-settings-independent-final/evidence/cache-owner-result.json。前期 /tmp 与/private/tmp Vite路径让自己导入store形成第二模块的测试环境错误已校正为canonical repo，未计成产品失败。最终反例两次实际误写，最终receipt有PUT200及明确失败断言。日志 cache-owner-counterexample.log。

原边界单元补跑35项：34 PASS / 1 FAIL；失败 settings/__tests__/SettingsContent.test.tsx::verifies Rust identity and keeps unavailable settings visibly unavailable，预期清除旧Node缓存，实际保留。该旧测试还有旧“Rust全部不可用”的setup，不能只靠其失败认定产品问题，所以上述真窗口独立证明缓存归属风险。AgentToolsSection和pointercancel等成功结果只算纯UI补充，没用mock替代Rust/头像/工具配置业务核心。

同一自有生产窗口还补了真正无utility负控：globalModels没有memory，实际记忆开关disabled，click后0 PUT，见no-utility-proof.json；因此group4缺口已关闭。无需为此再造模拟测试。

以下 FULL 表示该原组稳定owner下的关键成功/拒绝和源码行为已有足够证据，不表示全页共享S5可以忽略；S5未修复前206和受影响的编辑入口都不能READY。不得因FULL组数允许登记整叶。

|原index|分组|判定|依据/具体缺口|
|---|---|---|---|
|0|打开设置容器自动初始化|PARTIAL|当前冷启动/读取拒绝/模型目录失败已真跑；同owner缓存旧unit通过，但Rust专用15秒强制ready和供应商慢/失败不阻塞缺实际证据。新增S5证明跨owner加载期间仍有旧可编辑缓存；原Rust隔离unit失败。|
|1|浏览助手卡片、手工创建或人物卡导入、删除前预览与确认|FULL|当前浏览六读/手建空名零POST/真实创建并切换/卡片plan-import与未选记忆不导入/头像素材/删除预览均实跑。卡片双服务反例修复与21删除子例补拒绝路径；普通失败由明确HTTP错误链及现役捕获，不以created toast认定switch。Rust预览读失败明确阻止，较旧Node静默退回加强，不冒称保留旧静默错误。|
|2|选择主助手、改名与主模型|PARTIAL|稳定连接改名/主助手/主模型及config写后读链已真跑，共享实际I/O与GET500证明部分成功语义；但新S5实证A旧名页向B同ID助手真实PUT200误写，隔离不成立。|
|3|元设定、身份和 AGENTS.md 保存|FULL|真实Yuan及identity/AGENTS独立PUT成功，另真正一项200另一项500，物理文件分别读回部分成功。无改动分支直接按差异构造零请求，源码保存；普通Yuan失败console路径不误写成统一toast承诺。|
|4|记忆开关及模型前置条件|FULL|真实窗口启停和config读回、真实PUT失败与写成功后GET500已证。自有额外真实无网关CLI窗口证明hasUtility=false、开关disabled=true、click后零PUT，no-utility-proof.json；loading props旧unit为补充，不替代真拒绝。|
|5|经验开关与经验列表|FULL|真实经验启停、编辑/删除/缓存刷新后列表保全，PUT磁盘/GET读回已证；源码仍保留乐观更新且失败不自动回滚、helperfalse不进行第二次读取，共享autoSave实际失败链已测，不称所有逐组合网络故障已跑。|
|6|人物卡预览和导出|FULL|真素材预览GET、预览只读、真实ZIP/记忆选择、文件和showInFinder调用已证。旧跨服务导出POST200原负控与当前归属保护/往返失效已真跑。这里只证明reveal调用文件存在，不证明系统Finder已打开。|
|7|任选工具启停|FULL|真实automation配置写与回读/页面投影已证；当前AgentToolsSection原11项纯UI单元通过，包括未给目录回退、无可选目录不渲染、快速点击fresh disabledRef。源码显式[]也零行，未载入disabled不写；shared helper失败链实际覆盖。|
|8|美化工具启停|FULL|生产工具注册后真实beautify设置开关/磁盘回读通过；与group7同组件目录/disabled逻辑及其原单元，不声称执行beautify业务。|
|9|Office 工具启停|FULL|生产工具注册后真实office设置开关/磁盘回读通过；与group7同组件目录/disabled逻辑及其原单元，不声称执行office业务。|
|10|点击已选助手头像选择图片、裁剪并保存|FULL|真实文件选择取消零上传，Image/Canvas实际裁剪编码、POST与磁盘PNG/GET投影通过。新增owner校验及onerror/catch是明确加强，不冒称原没有错误提示的缺陷必须重现。|
|11|打开编译记忆、编辑事实/今日/长期/周日内容或清除编译内容|FULL|真实四编译段/每日内容编辑、DELETE编译并原事实保留/局部清空已证；真实GET400错误不假空、物理材料不变。原多PUT非事务按源码及persona部分写入纪律保留。|
|12|查看全部记忆或确认清除全部记忆|FULL|真实查看、取消危险确认零DELETE、确认后真DELETE及facts读回为空已证。非2xx和DELETE data.error在现役HTTP捕获，不把GET失败当空成功；异常shape已加强显式错误。|
|13|打开记忆 embedding 选择器读取可用模型及当前配置，选择模型或明确清空后保存|FULL|真两GET加载、选embedding和显式null清除、磁盘回读通过；自己raw browser receipt索引147/148两PUT后即tenets POST，期间无config GET。真实uchg PUT失败后selector回滚原值/磁盘未变；自有无模型服务窗口可见noModels，初始异常源码显示内联错误。|
|14|查看、批准、拒绝、新增或删除 tenets|PARTIAL|真新增/批准/拒绝/删除和每次后续GET已证；源码refresh GET失败只设内联error保留旧列表且不抛出，符合原文。尚无写已200并持久、随后真实刷新GET失败、页面旧列表+错误不误判回滚的实际证据；这是专门部分成功边界，不用一般读取403代替。|
|15|查看梦状态、手动启动梦或保存自动梦开关|FULL|真实状态/运行/上次报告、POST202与完整五阶段模型计量、自动开关PUT+GET成功，以及真实auto PUT500内联错误/磁盘保全已证；源码running禁重复、各读取/保存错误捕获，共享GET500回读链已证。|
|16|打开梦修订列表、查看详情并确认恢复|FULL|真实list/diff后实际click restoreThis→confirmRestore→restored提示并刷新GET；原check名称只有list-and-diff，不是仅此检查范围，不能误报未恢复。现役错误捕获只在成功后置完成。|
|17|有助手、辅助模型且记忆已启用时自动检查记忆健康|PARTIAL|真实degraded条/失败步骤及当前候选明确daily缺项已证。前置不满足时effect清health/error且不请求的源码存在，但尚无带旧health→撤销前置→界面清除/零GET以及HTTP/JSON失败unavailable实际证据；单元仅覆盖degraded/Dream错误，不能替代。|
|18|拖拽助手卡片调整顺序|PARTIAL|真实Electron聚焦后sendInputEvent排序持久通过；旧原pointercancel/lost capture零PUT单元本次通过，源码无移动不提交。真实order PUT非2xx后乐观重排经loadAgents恢复的关键分支还未证明，不能由成功拖拽或静态.catch当已恢复。|

最小剩余补证：0缓存隔离先修S5，并用同owner重开/15秒等待/供应商慢失败不阻塞的真实初始化边界证据；14真实tenet写200后只对后续GET制造真实I/O错误，核磁盘/旧列表/错误；17带旧health撤销模型或memory前置及一次真实读取失败；18真实排序PUT拒绝后读取并恢复原顺序。不需要穷举每个按钮的所有网络排列，底层共用HTTP/存储错误链已有实测。不是要求恢复原文描述的静默错误缺陷。

0B：assertion0 当前审阅 PARTIAL（自己的native记忆开关证明配置，不等于ticker真实停/恢复）；源码 r06_memory_ticker_completion::toggling_memory_really_pauses_then_resumes_timer_and_preserves_paid_cancel 已具体检查disabledSince、paused、reenableAt和恢复，但本作者target没有该bin，本轮未运行。Root当前main-combined-tests.log亦未发现此suite；如Root已有绑定到本候选的真实ticker流水，可据此关闭，不把它当实现缺陷。assertion1 FULL：native failures与原始真实收据已证明前段locale=en持久、助手config写失败未改，未假称回滚；另ticker mixed_patch_real_late_io_failure_preserves_committed_global_without_fake_rollback是更精确enabled=false的可复用证明。

3516：原3组在稳定owner入口分别FULL（自己重跑原21子、完整/tombstone+bundle引用、not_orphan与removal_failed、最后助手400及DOM提示）。现有删除确认只绑定打开后的有效连接，S5可能让旧卡片在B加载期间被当B目标打开；不能由原3组FULL推导S5未修时整叶可READY。修复共享旧缓存可编辑问题后需让该额外反例零写；旧21不能删改。

Mac真实Electron+Rust服务已跑；Windows/Linux/Tauri/正式打包/真实供应商/全阶段 NOT_RUN。Main较新前置组合由Root另验；本记录源42manifest保持未改。后续冻结修复需要重新绑定实际新源码，不能沿旧c8b6坐标声称通过。
