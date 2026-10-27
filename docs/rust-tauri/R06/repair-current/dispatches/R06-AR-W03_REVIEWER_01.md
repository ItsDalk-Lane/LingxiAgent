仓库：/Users/study_superior/Desktop/Code/LingxiAgent
原完整任务书：Lingxi_Rust_Tauri_Taskbooks_2026-09-23/（被忽略目录，不能用普通rg无结果声称不存在）
本轮完整不可变规格：artifacts/rust-tauri/R06/AR/ENTRY-01/specification/R06_完整问题清单与修复验收总控提示词.md
原基线：2e1933d8f16d0f383b85ca92ff5bdf29e4732e6d；实际HEAD与用户已有修改每轮重核，不回退。
保护初始用户.vscode/settings.json；当前授权只到R06，禁止R07、真实数据、外发、付费调用。
工具链只用/Users/study_superior/.cargo/bin/cargo，1.98.1，-j8，guard保留；每运行合成独立LINGXI_HOME并证实际生效。
使用关联原Task全部Steps/A-ID/R00到期叶，不能仅四C。

你是R06-AR-W03/R06-AR-F03第1轮的全新独立对抗性Reviewer。
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

## 首轮独立验收补充（仅准备，尚未创建 Reviewer）

必须使用全新空历史身份，既不是原 Executor01/02，也不是其他 Task 的执行者或 Reviewer。候选只在 Executor02 完整自检、对抗检查、正式受影响链实际结算、冻结 manifest 并结束后由总控提供。不得审阅仍变化的执行目录或主目录中混合未验收文件。亲自运行所有 F03-C01—C04 和原 R06-T03 全部四 Steps、三交付、A05/A06、74 唯一到期叶/169 断言；部分关联绿例不等于全部完成。37 个候选产品/测试文件待最终清单校验，不能沿用旧 36 文件口径。

实际统一准入须覆盖生产 SessionService、认证 HTTP execute/fork/retry/rewind/archive/delete、SessionFiles、Memory/ResourceIO、Core NativeFileExecutor、后台任务/RunSupervisor、子 Agent Permit 和不同 Storage 包装器。合法路径与权限先后次序都要检查：同 DB 不同 Arc 仍共用准入；不同会话/不同 home 并行；合法 parent/child 不同 Agent；LocalUser 与会话 owner 原规则；缺失或伪造 Permit、错误 DB/actor/parent/CancelScope 不得获得操作权或发送模型调用。只读历史与已删除源 journal 原契约也不能因补锁被破坏。

至少追加一个独立构造同族反例。对于取消后仍在物理执行的原生 write/edit 或可信 HostToolForward ResourceIO，亲验真实文件、请求、Busy/Conflict、独立会话放行、实际任务退出后才释放。不能只检查外层 future 被丢弃；不能把超时未知结果恢复为成功或重新执行。检查 Durable operation journal、状态/事件失败注入、重复 requestId/restart，以及禁止复活已删除源或补造新任务。至少验证旧根因变异会在真正匹配的行为断言失败，编译失败或空测试不是旧红。

作者十套 focused 实际134通过、最终 workspace1889通过/3失败的结果仅为线索，必须亲跑当前源；早期错误汇总141已纠正为九套121，保留原记录。原严格事件失败与新增 F08 projection、registry 10vs7、原 T03 未验证份额及其它 Task 交互如实列出，已知失败也不免独立实际验收。不能因分配 F14 或登记申报关闭本 Task 当前必需合同。

外置 build target 的真实 shipping binary 找不到源码祖先 lib：正式链显式 LINGXI_PRODUCT_DIR=你的独立工作区/lib，记录真实 env/material anchor/hash。原生产正例必须实际经过 READY、认证 HTTP、真实模型 wire、SQL、文件与 usage。此前外置 target 缺资源导致 C02 failed.provider_error/zero wire 的历史运行保留为失败及 CANCELLED_INCOMPLETE；不是门禁成功，也不先验认定产品坏。独立相同 bin 正确产品目录原 C02 两 wire/completed/file/usage 真通过；独立缺材料负控 zero wire/failed.provider_error。前台 execute 是 await 到真实终态，不能宣称早返回后台或把共享逻辑时间的零毫秒当物理持续时间。保留原故意缺材料负例，不用全局 env 掩盖它。

Guard 每实际 bin 600秒/8192MiB ON；Cargo 绝对路径1.98.1 --locked -j8；workspace 外层2700秒；原 R05 七顺序项目合计19800秒加有界开销。使用工作区外独占合成 HOME/TMPDIR 和独占 build cache，每条嵌套命令证据必须位于该条实际 registered stdout/stderr 的目录，避免源指纹漂移。不能扩大排除规则或改正式门禁取绿。执行前后真实 test bin SHA 必须直接取得，不拿 Cargo 前旧库存顶替、不补造未知 before；记录 shipping/source/lock/runner/input 前后和 wait/reap/guard/cleanup。RSS 未测 null，宿主中断 NOT_RUN/BLOCKED。

读取完整原规格、当前产品 diff、原任务资料、全量 source map 和报告；safe evidence 逐文件 SHA。home/凭据/request headers/full source archive/build targets/binaries 为 LOCAL_ONLY，不进 Git。不得修改产品/既有测试，不 commit/push、不启动 Repair/下一 Reviewer、不代签 Stage。所有自建探针/输出/报告分离保存；一次报告全部实际发现，冻结后结束。最终 PASS/FAIL/BLOCKED 取决于你真实验证的所有当前必需项。

后续新运行资源约定（不改变当前活动运行）：合成资料根指 LINGXI_HOME，不是覆盖 OS HOME；OS HOME/系统工具与用户设置继承原值，所有实际服务/测试的有效 LINGXI_HOME 必须是本轮独占合成目录并证READY/路径来源。TMPDIR使用工作区外独占目录。针对已实证并行增量缓存造成的磁盘压力，后续新独立构建可明确记录 CARGO_INCREMENTAL=0，减少可重建中间缓存；保持原toolchain/--locked/-j8、产品特性、正式命令、断言、每bin guard600/8192 ON和原预算不变。把实际环境/产物执行时前后SHA加入绑定，不能把这个编译缓存选择写成关闭guard或隐瞒编译差异。已有已启动链不改环境。

临时缓存清理如确需执行，先证本任务归属/退出/无句柄和真正执行程序的SHA；权限位X_OK会把零字节incremental .lock误算成程序，精确被授权cache子树内锁文件的预期删除不得冒称真实程序变更。完整before原记录保留，缓存子树外全部executables与所有实际执行路径逐SHA核对。原helper若exit1就如实保留；未知原lsof/raw/free-before不能事后回填。清理不是行为验收，不能冲销原FAIL。
