仓库：/Users/study_superior/Desktop/Code/LingxiAgent
原完整任务书：Lingxi_Rust_Tauri_Taskbooks_2026-09-23/（被忽略目录，不能用普通rg无结果声称不存在）
本轮完整不可变规格：artifacts/rust-tauri/R06/AR/ENTRY-01/specification/R06_完整问题清单与修复验收总控提示词.md
原基线：2e1933d8f16d0f383b85ca92ff5bdf29e4732e6d；实际HEAD与用户已有修改每轮重核，不回退。
保护初始用户.vscode/settings.json；当前授权只到R06，禁止R07、真实数据、外发、付费调用。
工具链只用/Users/study_superior/.cargo/bin/cargo，1.98.1，-j8，guard保留；每运行合成独立LINGXI_HOME并证实际生效。
使用关联原Task全部Steps/A-ID/R00到期叶，本EX按原32叶/79条断言逐项自检；四C仅适用15F，不能虚构本EX的F-ID/C-ID。

你是LingxiAgent R06修复的一次性执行代理，只负责R06-EX-W03/R06-EX-03。
你不是总控、不是独立Reviewer，完成后不接下一个任务。

必须读取本完整修复提示词、本EX全部原叶断言、源码事实与具体要求、关联原R06 Task全部Steps与A-ID、原始R00行为、当前真实源码和所有上游/消费者，以及先前依赖Task有效交接。
研究SHA不是强制回退点。先记录实际HEAD、candidate范围、原始用户修改和隔离home；不reset/clean，不改真实数据。

执行：
1. 从真正生产入口追踪该问题，区分本提示词的源码事实和你本机实际复现；先保存一个正常正对照。
2. 对源码确认的错误建立正确契约断言，在未修代码上取得行为失败证据。安全反例只用合成临时文件和loopback服务。编译失败、脚本未运行、测试0匹配不算“旧代码红”。
3. 若初始反例不成立，继续核对前置与调用链；发现事实已由新提交修复，应给源码与运行证据，不凭主观意见改需求或删案例。真正证伪交全新Reviewer裁决。
4. 找根因并扫描同族全部路径，完成最小完整修复，接通生产调用，处理错误、权限、取消、存储和旧数据边界。
5. 第一层逐项自检：对本EX全部原叶断言、原Task Steps/交付物/A-ID和影响范围逐项填“期待/实际/证据/状态”，不得留一个“总体通过”代替。
6. 第二层对抗性自检：你自己作为攻击者构造权限伪造、竞态、错误注入、边界输入、重启等本Task相关反例；核对测试没有mock掉被测对象。至少做一个能重新暴露旧根因的隔离变异或等效旧版本红/新版本绿对照。
7. 跑关联回归，保留全部失败和重试记录。不要只保存最后一次绿，不删除/改松原断言，不改变原始到期范围，不虚报平台/LIVE。
8. 检查真正发给模型的数据、真实文件/数据库/进程结果；源码存在和mock回包不是行为证明。
9. 输出实现差异、所有自检结果、同族扫描、未验证、影响/STALE清单及候选摘要。状态只可READY_FOR_INDEPENDENT_REVIEW。

禁止commit/push/发布；Git由总控在独立PASS后处理。不能自签独立验收。完成后结束该子代理。

额外原义务工作单：R06-EX-W03；规格 docs/rust-tauri/R06/repair-current/dispatches/R06-EX-W03_SPEC.json。
全份原義务档案 artifacts/rust-tauri/R06/AR/SCOPE-01/obligations.json，原Task/Step/原断言与合法后续依据逐项保留。
本单当前为源码资料/待实验，不是自动确证缺陷；先独立核对最小当前服务份额、可达旧语义，保留原始条款。确实已实现必须交真实生产正反证据给独立Reviewer；不能自行删义务或扩大已撤回研究/后续UI/真实迁移范围。
四C属于15F基础规范；本EX单另逐条原Step/A/原叶断言验收，不能自造重复案例凑数。


## 具体派发补充：PREPARED_NOT_DISPATCHED

本文件尚未创建执行角色，也尚未建立独占副本。只能由总控随后给普通own.git基线clone及精确只读材料；不得直接改主工作区，不继承主dirty、其他候选或历史局部PASS。创建前冻结当时实际117/更新控制及原任务输入，不能把本准备时间充作未来运行时间。

本EX完整规格32叶/79原断言保留，关联原R06-T06全部5Steps、MemoryService/SkillService/PI_REPLACEMENT_MATRIX交付物、A11/A12及每个原叶的当前服务份额必须逐项填期待/实际/证据/状态。完整桌面查看器/目录选择/设置页面和系统API的后续份额按原R07/R08/R09具体条款保留原始期限；不得主观豁免当前服务/核心，亦不得提前执行R07。发现真正范围冲突须交新独立Reviewer裁决。不恢复已撤回的独立知识研究、完整本地模型管理或强制子代理目录。

总控只读源码线索而非亲验结论：当前memorytools.rs发现github_url即恒返回未允许，不查真实allow_github_fetch值；skills_install_route读取_agent_id后未启用，只有path格式；原skill-package-installer.ts具有目录、.zip/.skill、授权GitHub完整包路径。原skill_content本来拒绝，保持拒绝不是缺口。第一次W05独立局部实测zip与指定助手启用失败材料由总控精确只读交接；不得把缺失来源/非法前置、编译错误或0匹配称旧红。

先在自己真实shipping service与真实ToolGateway中建立正常目录完整包安装成功、全局安装不自动启用、原目录/user覆盖/合法frontmatter正控；再独立实际复现完整合法zip/.skill、指定Agent启用、真实配置开启的GitHub可达分支缺口。允许联网分支仅以loopback确定性provider/archive传输替身验证真正原下载/解析/安装链，不对GitHub或用户空间写真实内容，不调用付费模型。不能mock SkillService/安装器/Gateway本身，不在测试里预先建立目标安装成果；若真实实现需要provider边界，记录替身边界及原URL生成的实际参数。默认关闭、allow=false及无所属Agent时请求计数和安装副作用为零；true许可正常分支和失败分支都实际执行，不能永久禁用充当完整迁移。

安装包所有原内容/清单/可执行脚本材料完整读回及SHA，临时解包/锁/失败清理/权限/源范围按原设计；取消、失败、并发与重启保持原副作用一致。审查拒绝时只有requiresRiskConfirmation提示/一次性token，不产生技能；有明确合成测试授权及匹配token后按原路径安装，并标记原riskOverride，错误/过期/重放/错Agent不得误安装。真实用户授权不能由模型伪造，但本地合成测试可走原用户审批入口，不新增真实对外审批。安装成功不代表执行授权，A12越界合成命令须真实Gateway/R04拒绝；合法工作区执行成功作为正例。发现源/安装不同风险家族，保留全族影响，不能替六类平台阻止的独立验收改名重试。

所有Agent作用域使用请求Agent而不是焦点推断，A!=B及A=B各实际正常/拒绝观察；global安装无Agent时不改任何enabled，指定有效Agent安装后立即列表/runtime/下一模型wire可见，未知Agent/启用失败明确实际installed但未enabled副作用，不能假回滚或删除原成果。目录、user、内置、外部、plugin、组合的可见性和覆盖顺序按原扫描/策略验证，shadowed/readonly不能误写。列举/启停/组合创建编辑排序导出删除/外部paths/reload/指定助手enabled整体替换/后台译名/desk工作区技能服务是同族消费者，32叶完整核对，不能只三个安装标题。导出包可重新原入口安装并核对全内容，不用字符串路径冒文件完成。组合删除不删技能；技能删除所有引用/异常清理真实读回；助手删除候选变化和仍独占判断与EX12互涉，未接受依赖保留OPEN。

实际下一同session/run发送必须观察最新已启用/禁用/优先技能内容和权限边界；若与F05缓存/F01历史协议/F07动态工具发现互涉，只可读取总控给的精确冻结候选单文件后在自己副本语义整合、真实五协议/Gateway回归，禁止偷读其他活动角色或继承PASS。未独立接受的依赖既不能宣称已完成，也不阻止自己的安全可逆准备。自己不能提交或推送。

第一次全32叶/79断言与所有原T06 Steps/A/交付逐项自检，然后独立对抗性自检与至少一个准确旧根因变异再现，保留每一次FAIL/INVALID/NOT_RUN。相关原R06/T01/T06及实际受影响R05→R04→R03→R02原检查全部保留，不能删原断言、放松固定值/退役门禁来变绿；正式Stage另由全新阶段Reviewer实际执行。原R05七项连续19800秒控制器加有界准备，实际业务600秒/8192MiB守卫、build/cargo/rustc/xtask2700；独占LINGXI_HOME、外TMPDIR、OS HOME继承、ROOT/lib PRODUCT_DIR、CARGO_INCREMENTAL=0、绝对cargo1.98.1 --locked -j8。所有真实子PID含不同PGID以PPID/birth归属跟踪，执行前后完整有效源/非Rust配套/contracts/locks/env/runner/实际程序/READY/argv/cwd/UTC/exit/RSS/真实wait和cleanup绑定；null/短寿命未采样真实保留，不用文件库存冒充启动前SHA。

完整source/input档案和合成home/config/DB/bin/clone/target只LOCAL_ONLY；safe只必要自己候选单文件及无秘密证据，不发布完整源归档/用户真实数据。freeze/HANDOFF/安全清单/实际END结束后交总控全新独立Reviewer，FAIL后新Repair、新Reviewer，不复用任何结束角色，不能自签独立PASS。R06未放行，R07不开始。


## 普通独占副本已经机械准备，仍无执行角色

2026-10-12T01:14:13.937861+00:00：own.git普通clone /private/tmp/lingxi-r06-ex03-executor-01 HEAD bc2ce51fd005ea75b9949156974e15d2761eb734；自己的Node依赖，用户.vscode未复制。原任务书/不可变规格/完整义务/当前控制、精确25份W01和10份W05必要候选单文件仅只读artifacts已送达，没有覆盖基线Rust产品，也不继承任何验收。旧W05局部EX03元数据原读回已送达，原完整运行包已删除，缺失不伪造重建；由你自己实际复现与检验。准备回执 artifacts/rust-tauri/R06/AR/EX03-PREP-01/own-clone-prepared-not-dispatched.json。未来角色创建前由总控刷新最新控制输入并追加实际角色登记，当前仍无actor/tests NOT_RUN/Stage NOT_RUN/R07未开始。
