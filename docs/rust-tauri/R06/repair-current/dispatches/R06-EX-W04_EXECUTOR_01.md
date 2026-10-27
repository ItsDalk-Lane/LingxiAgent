仓库：/Users/study_superior/Desktop/Code/LingxiAgent
原完整任务书：Lingxi_Rust_Tauri_Taskbooks_2026-09-23/（被忽略目录，不能用普通rg无结果声称不存在）
本轮完整不可变规格：artifacts/rust-tauri/R06/AR/ENTRY-01/specification/R06_完整问题清单与修复验收总控提示词.md
原基线：2e1933d8f16d0f383b85ca92ff5bdf29e4732e6d；实际HEAD与用户已有修改每轮重核，不回退。
保护初始用户.vscode/settings.json；当前授权只到R06，禁止R07、真实数据、外发、付费调用。
工具链只用/Users/study_superior/.cargo/bin/cargo，1.98.1，-j8，guard保留；每运行合成独立LINGXI_HOME并证实际生效。
使用关联原Task全部Steps/A-ID/R00到期叶，本EX按原5叶/10条断言逐项自检；四C仅适用15F，不能虚构本EX的F-ID/C-ID。

你是LingxiAgent R06修复的一次性执行代理，只负责R06-EX-W04/R06-EX-04。
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

额外原义务工作单：R06-EX-W04；规格 docs/rust-tauri/R06/repair-current/dispatches/R06-EX-W04_SPEC.json。
全份原義务档案 artifacts/rust-tauri/R06/AR/SCOPE-01/obligations.json，原Task/Step/原断言与合法后续依据逐项保留。
本单当前为源码资料/待实验，不是自动确证缺陷；先独立核对最小当前服务份额、可达旧语义，保留原始条款。确实已实现必须交真实生产正反证据给独立Reviewer；不能自行删义务或扩大已撤回研究/后续UI/真实迁移范围。
四C属于15F基础规范；本EX单另逐条原Step/A/原叶断言验收，不能自造重复案例凑数。


## 具体准备：尚未创建角色或执行运行

本单完整原范围5叶/10断言、关联R06-T06全部5Steps/交付物/A11A12和当前份额保留。只修原手动记忆整理业务与状态、修订/恢复核心服务，不能因无R07定时ticker把合法手动run/state永久503；R07自动定时/心跳调度仅按原条款留待后续，本单不开始R07，不另写Agent循环，不恢复撤回的独立研究或完整本地模型管理。

总控2026-10-12只读主当前源：memory.rs dream_run_route/status_route在有效Agent根检查后恒dream_unavailable，reason=dream_ticker_absent_in_candidate；revision list/detail/restore已有生产入口。此为SOURCE_ONLY线索，主源包含未独立接受W06，不能把本读取当执行者隔离基线或旧行为红。角色创建前要先得到普通own.git已提交基线副本、精确所需冻结原输入、明确且不继承PASS的必要依赖来源；总控不能把主dirty当已验收产品整块覆盖。自己的运行先建立真实合法Agent/有记忆内容/正确显式模型配置/权限正控，再从真实HTTP手动入口亲验旧红。无配置/记忆关闭/ticker不可用的合法拒绝不是缺陷，配置合法不应恒拒绝。应先分析MemoryService与当前配置解析生产链、R05真实ModelGateway调用/usage/取消和原TS dream完整语义。

完整现役链必须读取server/routes/memory-dream.ts、core/agent.ts及真实MemoryTicker调用者、lib/memory/dream/{runner,model-runner,state-store,revision-store,memory-units}.ts和相关原测试/配置/错误映射，不仅移植response字段。Rust持有实际业务状态、模型调度和数据事务；用loopback固定provider响应只替外部模型传输，不mockMemoryService/真正ModelGateway/状态机/存储。沿原R03生命周期和R05调度/真实usage账本，不能以fake202、echo状态、直接手造revision或Node后端代跑来满足成功。禁止真实付费/真实记忆资料迁移。

逐叶亲跑列修订、对比实时current、status、restore与手动run的成功及原拒绝。指定Agent必须以请求目标而非焦点推断；A!=B及A=B、未知Agent404、权限settings.write拒绝、memory-disabled409、忙/已有运行409、真实不可用503/模型配置拒绝、失效revision404均保原合同。首个成功202只表示接受，应实际观察真实模型请求、已完成/失败/取消运行状态、使用收据和修订/当前记忆文件、编译快照/后续同会话真实wire；state的字段/触发manual/模型/时间/错误准确对应真实原业务。已有部分结果/忙/重复提交/明确模型失败/未知响应/取消/物理仍活/重启等按原R03/R05处理，不静默重付、不覆盖未知为成功。新fee未知保持null，不用NULL当0。

revision由真正dream成功流程产出，真实before与after/来源/原日期和hash可读；失败或输入变化/忙冲突不得伪部分成功/跨Agent污染。restore先正确snapshot当前再事务恢复、保持原冲突边界和恢复revision来源；真实改当前内容后对比接口必须每次实时读新current。编译刷新不能只新Session生效，同会话下一请求使用新真实记忆；如果受F05共享缓存依赖未提供，明确OPEN并交全新Review裁决，不能改名为R07已完成。测试可精确合成前置或故障注入，但不能把直接写state/revision当生产成果；所有输入与故障范围明确绑定。

同族取消/并发读写/同Agent重复/跨Agent独立/原修订导入兼容/路径与IO故障、原memory flag与命名优先级全部查生产调用者。修复最小必要完整业务，保持已有F06具体原权限/Agent作用域，但不能借本EX重试W06平台受阻独立Review。必须依赖的未接受共享源在自己副本由本新执行者明确整合并亲跑受影响原检查，不能继承旧绿或读取仍活动执行者改动。源码新注释中文，原产物/配套门禁原断言保留，不退休或放松。

本单第一层按5叶10断言、全部原T06 Steps/A/交付逐项期待/实际/证据/状态；第二层自己独立对抗，至少一个同族隔离旧根因变异能实际抓住（编译失败/零匹配不算）。当前必要门禁/原R06 memory回归与受影响R05→R04→R03→R02保持原命令、原预算与真实FAIL，不用全局登记比例凑PASS。完整Task未修依赖诚实OPEN/NOT_RUN，只有新Reviewer能独立验收。

每实际PID守卫600秒/8192MiB，cargo/rustc/build/xtask2700秒，原R05七项19800秒+有界准备；绝对cargo1.98.1 --locked -j8、OS HOME继承、LINGXI_HOME独占合成、ROOT/lib LINGXI_PRODUCT_DIR、外TMPDIR、CARGO_INCREMENTAL=0。PID+birth/lstart+PPID含不同PGID、真实程序启动前后SHA与实际env/READY/有效源/nonRust/contracts/locks/原输入/runner/argv/cwd/UTC/exit/RSS/wait/cleanup全记录；未知不能填0或用库存/first_seen冒启动前。全部失败/无效前置/来源漂移保留不改旧窗口。

完整源/配置/home/DB/bin/targets只LOCAL_ONLY，safe只必要自有候选单文件与无秘密证据。完整freeze/HANDOFF/安全逐文件清单/最终源和输入准确集合/真实END结束后由总控创建全新Reviewer，FAIL后全新Repair→全新Reviewer；不得commit/push、自签PASS、复用结束角色、真实对外/付费/用户资料迁移。R06未放行，R07不开始。
