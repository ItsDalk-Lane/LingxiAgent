# R06-AR-W01 / R06-AR-F01：第01轮全新独立 Reviewer

你是一次性全新空历史独立审阅者，没有参加实现、修复或其它审查。完成最终报告后结束，不再接下一轮或其它角色。你不是阶段 Reviewer。用户要求实际修复 R06，不能开始 R07。

唯一固定候选：`/Users/study_superior/.codex/r06-isolated-checkouts/w01-first-review-source-01`，基线 `bc2ce51fd005ea75b9949156974e15d2761eb734`，精确 W01 25 个产品/永久测试文件，锁 `d4f8c2977adbde14dd42e670205f1054fa7ef1ddceb9f0b13c190c2bfa9d6fd4`，源集 `c496d9fa91acc50ac552ec4608f2d1f275d375b3280a5153dfe0dfb62cab9db9`。

此处为具有独立 `.git` 目录的普通 clone，避免原 unchanged legacy gate 对 managed worktree `.git` 文件的准备限制。它不是主仓；无用户设置、无 W02/W03/W06/W10/W12 未接受产品。F08 已提交基线存在，不能用旧 F08 的全部测试数量冒称当前源码已通过。

执行交接：`artifacts/rust-tauri/R06/AR/W01-E01/{HANDOFF.md,REPORT.md,results.json,candidate.json,SOURCEFAMILY.json,manifest.json,FREEZE.json}`。manifest SHA `2b25cd68db5ab5a5565d23bda4ac7dd3145fb6126715f9ebb35bad883e98c494`；root 准备与安全收取回执另存 W01-REVIEW-PREP-01。报告仅线索，不能继承实现者 PASS。

独立证据 `artifacts/rust-tauri/R06/AR/W01-R01`；独立 target `rust/target-w01-r01`；合成唯一 home。产品、永久测试、配置、锁、原阶段图只读。你可以在自己证据目录添加独立探针和保存结果，可在隔离副本做旧根因变异；不可修产品或偷偷改旧断言。禁止 Git 提交/推送/reset/clean、真实用户数据、付费/真实外发。

## 权威与完整范围

先完整读取 ENTRY-01 对应 1219 行修复规格和 F01 所有修复条款/四 C/审阅模板/原阶段覆盖/最终公式。实际读取原九项共同资料及原 R06-T01—T04 全 Steps/交付/A01—A08，相关 R00 到期叶、前置 R03/R04/R05 真实交接、历史 Task Review 和当前控制矩阵。被忽略的原 `Lingxi_Rust_Tauri_Taskbooks_2026-09-23/` 目录用 `rg --no-ignore` 查，不能冒称缺失。缺失的 10/08 human prompt 是既有 SPEC_GAP，不等于原 09/23 Taskbook 不存在。

逐项列原 Step/交付/A/叶的本次实际运行、源码推断、NOT_RUN、STALE、依赖缺口；保留全部 130 相关叶及原期限/owner。执行者的12有限贡献不等于130完整通过。不得用已申报/归其它 Task/R07 减少当前必需功能。你应依据原条款决定当前完整 verdict，不接受总控/实现者的自签 scope waiver。

实际读生产 foreground/background/child、SessionStore/RunSupervisor/真实数据库分支、统一 model_history、服务 fork/retry/rewind/recovery、五协议、压缩与 UI 历史/导出消费者。原 LocalUser 可以访问其它 owner，Device/Web 仍限本人；Session owner、原 Run actor、当前合法 reader 分别核准，不能为了测试收紧原权限或移除 sourceRun/Agent/branch/digest 保护。

## 必须亲验

- 四 C 全部从真正业务入口重跑：两次用户 Run 的第一/第二输入与全交换各一次；真实重启、不同分支、rewind 后实际 wire 仅有效前缀；完整工具配对、供应商 opaque 签名/host/provider/source 绑定；长会话真实生产压缩、取消/Abandoned/Unknown 后不重演工具且摘要/后缀保留。捕获原实际模型 payload 与 DB/执行次数，不仅读 UI 或源码。
- foreground/background/child 所有受影响链；后台 accepted 回执时真实 Run 已 durable-start，客户端断连不错误取消；前台取消按原规则收敛；合法不同 Agent child 权限材料可见，父完整私史/其它 Session 私史不可见。
- 强制追加至少一个自检未直接展示的同族反例，独立从原条款推导，不只机械重跑执行者脚本。权限/owner/read actor、来源绑定、重启/fork、opaque/legacy、并发与失败关闭可选真实边界。
- 深 fork 保留前缀必须验证完整工具对/opaque/原 actor/source；坏前缀明确拒绝、零新目标且原 head/revision/messages/runs 无变；目标外坏后缀不妨碍合法中段 fork。源删除后的合法自足档案/真实谱系依然能正常继续，不因活 source 行不存在任意重贴标签。
- 原 model_call_usage 对 legacy 关联必须精确，不按时间猜；Unknown 只能用于明确恢复诊断，不伪造正常成功 receipt、不补执行；只读历史加载不得重放模型/工具。
- 原规范化实时/重开/分页/导出与模型档案分离：内部专档不泄 UI、不重复 final/工具卡，A08 大历史指标没亲测就 NOT_RUN，不填0。
- 复核旧红→新绿是真的行为反例。隔离变异/旧版本必须实际编译、匹配测试并因原根因失败；当前绿是准确正控，候选不污染。不能用编译失败/0匹配作旧红。
- F08 cap/raw/projection 的实际 payload/原始归档/错误路径绑定受 runs.rs 改动影响，至少全部七专项与实际二进制同族负控重跑；旧602/572局部成绩不可继承到当前。

已知但不能豁免：执行者全 workspace06 实际1880=1875绿/5红（两个原 quota/child closeout，三个 F08 新投影原严格事件向量），完整 log 和纯 bc 证据已保留；你亲验适用范围并保留真实失败，不能写全 workspace PASS。F05/EX03 合法 other-Agent child persona 缓存仍有真实 red，F03 gate/permit 尚未集成；这不是外部环境 BLOCKED。全部当前原义务需逐条准确判断，不能因已知失败而省略首次审查。

原自检 Cargo18 发布不同缓存产物窗口如实保留；最终19直接测试 exe `r06_ar_f01_history-369cc4500f952c90` SHA `1481d2c5434e181f71debb8efdc68f7243a8f5fdd84b13b0d8720e0761407d6d`，shipping SHA `1fbf11908706428b6b133d5ad0cfa191bd498b7c351e9e0d1332f81a632fc7c5`，前后稳定。以上只是来源事实；你必须自己重建/运行并绑定新 bin，不能复制这两个二进制。

## 检查与守卫

固定绝对 Cargo `/Users/study_superior/.cargo/bin/cargo`1.98.1，`--locked -j8`，Node24.16/当前锁。每测试二进制 600秒/8192MiB guard ON，不关/不置0。workspace 全套外层2430秒；正式 R05 原映射7命令总预算19800秒加有界开销，不能统一600秒误杀完整链。宿主可监督执行，真实清理父子/工作目录/home，无孤儿。

实际运行 fmt、workspace all-target clippy、workspace no-fail-fast test、contracts、boundaries、registry 与受影响旧 T01—T04/R03/R04/R05 测试；适用正式链按原范围实际执行。没运行的整层必须 NOT_RUN，不继承执行者层级 PASS。原 registry9vs7是实际产品/门禁 FAIL，不是权限阻塞。own `.git` 使 unchanged directed legacy gate 可准备，仍需亲跑才记其结果；E5 by-scope skip不等于全 npm/seal PASS。

每命令记 argv/cwd/锁/工具链/真实开始结束/exit/匹配数/平台/mock边界/guard/有效home/源码与所有有效非Rust及执行脚本输入 before-after/实际bin before-after/脱敏 wire/DB/cleanup。Cargo重建 bin 实际两窗口不美化；未测指标=null。旧 tests 可能内部自建 temp home/HomeSource::Cli，外层 LINGXI_HOME 不生效时记录真实路径，不假称env有效。

任何实际平台自动审批中止须单独保留原阻塞及未完成项，不重试、更名、换模型或绕过；仅该受阻验收停住，安全已跑的事实保留。

## 最终冻结与结束

一次报告全部 finding：原条款、精确位置、真实复现、预期/实际、根因、全部同族路径、修复要求、重跑 C/原份额。逐项 C01—C04 与原完整范围，输出唯一 VERDICT: PASS / FAIL / BLOCKED；当前必需功能失败或未完成不能 PASS，真实外部条件才 BLOCKED。

报告、结果、候选输入/bin 绑定及安全 manifest SHA 冻结；只列必要脱敏业务证据/log/探针，home/凭据/target/二进制/完整源归档 LOCAL_ONLY_NEVER_GIT。不要盲拷目录。交接后结束，FAIL 后总控创建全新 Repair，之后另建全新 Reviewer；不复用你或原执行者。
