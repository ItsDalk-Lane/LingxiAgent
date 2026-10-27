仓库：/Users/study_superior/Desktop/Code/LingxiAgent
原完整任务书：Lingxi_Rust_Tauri_Taskbooks_2026-09-23/（被忽略目录，不能用普通rg无结果声称不存在）
本轮完整不可变规格：artifacts/rust-tauri/R06/AR/ENTRY-01/specification/R06_完整问题清单与修复验收总控提示词.md
原基线：2e1933d8f16d0f383b85ca92ff5bdf29e4732e6d；实际HEAD与用户已有修改每轮重核，不回退。
保护初始用户.vscode/settings.json；当前授权只到R06，禁止R07、真实数据、外发、付费调用。
工具链只用/Users/study_superior/.cargo/bin/cargo，1.98.1，-j8，guard保留；每运行合成独立LINGXI_HOME并证实际生效。
使用关联原Task全部Steps/A-ID/R00到期叶，本EX按原19叶/53条断言逐项自检；四C仅适用15F，不能虚构本EX的F-ID/C-ID。

你是LingxiAgent R06修复的一次性执行代理，只负责R06-EX-W10/R06-EX-10。
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

额外原义务工作单：R06-EX-W10；规格 docs/rust-tauri/R06/repair-current/dispatches/R06-EX-W10_SPEC.json。
全份原義务档案 artifacts/rust-tauri/R06/AR/SCOPE-01/obligations.json，原Task/Step/原断言与合法后续依据逐项保留。
本单当前为源码资料/待实验，不是自动确证缺陷；先独立核对最小当前服务份额、可达旧语义，保留原始条款。确实已实现必须交真实生产正反证据给独立Reviewer；不能自行删义务或扩大已撤回研究/后续UI/真实迁移范围。
四C属于15F基础规范；本EX单另逐条原Step/A/原叶断言验收，不能自造重复案例凑数。


## 具体执行准备：没有角色，不能继承验收

本单 PREPARED_NOT_DISPATCHED。尚无执行角色、隔离副本、产品覆盖或行为检查。派发前总控提供W04最后19候选的freeze/safe/HANDOFF/END及当前整合来源；活动W04源码不能提前当最终输入。已有记录只作定位线索，所有本单原19叶/53断言和T05完整当前份额必须由你实际核对、修复、自检、对抗、回归，再由全新独立Reviewer亲验；不自造F/C，不仅修URL或mount标题。

准确范围是原R06-EX-10 / R06-EX-W10、R06-T05-S01/S03及R04→R06明确交接ResourceIO核心API份额。原F-D09资源族不同于R06-AR-F09知识embedding。工作台UI/外层长期监听/宿主picker按各自原R07/R09条款后期；现阶段授权local_fs/mount实际API不能再次无依据推R07。原stat/read/list/search/write/write_expected_version/move/rename/trash以及watch/subscribe/events/unsubscribe/unwatch各自服务份额、resource get/HEAD/text等19叶按原台账的逐操作权限与支持矩阵，不能统一400、统一成功或全算source-only。

先完整读原TS ResourceService/ResourceIO/provider/mount配置、当前Rust resourceio/resources/resourceaccess/resourceio调用者和真实上游身份/取消/物理退出链，建立逐provider逐操作的原支持、权限、真实成功、拒绝、错误、原状态副作用矩阵。先查合法mount注册/解析/凭据/能力及有效配置的真正入口；不能拿不存在的mount或虚构字段请求400当完整mount验收。所有修改由本单独占副本做，保持原本权限和未知兼容配置，不同时让其他代理修这一provider根因。

资源URL成功前提已被总控校准，见现有AR_PROGRESS.control_observations/R06-URL-LEGAL-SUCCESS-ANTECEDENT-CORRECTION-01。原lib/resource-io/providers/url-provider.ts实际stat/read/materialize true，但明确拒绝localhost/.localhost/私有解析地址。W04 currentV4 a4db...只证明direct loopback200与formal unsupported400，不能冒原合法URL成功旧红；FirstReviewer和旧结果原件保留，具体前提由新Reviewer裁决。url/session_file对不支持的修改/监听操作仍应明确拒绝；不能因为资源只读支持而要求所有操作200。受支持URL读取/状态/物化按具体原契约核对合法目标、大小/超时/重定向/授权与产物，测试只用允许的合成loopback传输、可信生产可配置测试边界或原允许的底层传输替身；不得mock掉被测ResourceIO、安全检查或实体结果。若无法构造合法受控成功前置，诚实NOT_RUN/BLOCKED对应窗口而继续mount/local_fs；不访问真实公网、不放宽私有地址安全闸、不加全局绕过开关、不伪称旧引擎LIVE。

与W04同族消费者要保留真实stage/message/tool/final引用及别名、原Run活动/终态history、托管归属、同物理路径/符号链接/case/目录包含引用、最后真实引用释放后正常GC、临时/孤儿/冷过期文件恢复和补偿。独立hardlink若仍有合法实际路径8B可读，不强加永久保留原无引用名字。原ResourceIO读流/大内容、目录撤权、符号链接变化、物理blocking exit、inheritedprincipal/跨Agent/跨session/跨provider、watch订阅与释放实际句柄作用域全部按原断言，而不从路由注册推效果。F02/F03等平台受阻独立审查保持BLOCKED，不改名/换代理/模型/权限重试，不以本EX普通产品修复解除它们。

先跑必要针对性和直接受影响检查；已知缺口在本单边界内集中修全族，再冻结稳定候选。必须有逐项第一层期待/实际/证据/状态→第二层对抗旧红/新绿或等价原根因变异→关联回归的真实顺序。原必跑完整检查不抽样，同条件未变且有效完成的本角色窗口不为整理交接重复。所有旧失败/无效正控/先前source-only/未测物理退出都保留，不回填旧通过；没有原依据的future或登记不能关闭当前必需项。

继承OS HOME，合成LINGXI_HOME，真实LINGXI_PRODUCT_DIR=本副本/lib与外部TMPDIR绑定；cargo绝对路径锁定-j8，CARGO_INCREMENTAL=0。业务PID guard600秒/8192MiB，cargo/rustc/xtask聚合2700；原完整链预算按原合同。记录真实PID+birth/PPID/RSS/生命周期/wait、argv/env/READY/source/input/bin前后SHA和请求/SQL/文件/cleanup；null未知诚实、不以first_seen冒prestart或PSabsence冒独立wait。大构建先检查空间，限制并发；不删除副本/用户文件/源/证据/实际验收程序，确切本轮可重建且已退出缓存依原授权/审批。

完整源码/home/配置/DB/bin只LOCAL_ONLY。安全证据只必要原始材料与自有逐文件候选。最后freeze/source/input/runner、全部有效和失败窗口/STALE、精简完整HANDOFF/safe manifest/真实END后立即结束；总控再创建全新Reviewer，你不可自签PASS、commit/push、重写历史、真实迁移/付费调用/外发或开始R07。
