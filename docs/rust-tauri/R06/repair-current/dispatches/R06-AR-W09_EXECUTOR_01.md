# R06-AR-W09 / R06-AR-F09：一次性新执行代理

状态：DISPATCH_READY。W10 第01轮 Reviewer 已完成 VERDICT: FAIL 并结束；完整 REPORT.md/findings.json/review-results.json/final-binding.json 与98项安全证据已按 SHA 复制到 artifacts/rust-tauri/R06/AR/W10-R01；safe-manifest SHA227aefc869bf8a1140f9385276775885181c5601dd50658f854a525a32425dd4。现在须创建全新空历史执行实例。

## 唯一角色与候选

你只执行 W09/F09，完成后结束，不转任修复者、Reviewer、下一 Task 或总控。不继承任何执行者聊天。禁止 commit/push/reset/clean/改写历史/发布；主代理负责交付。不能修改主工作区或其它代理目录。

工作区：`/Users/study_superior/.codex/worktrees/r06-w09-execution/LingxiAgent`。
当前基线应为 `bc2ce51fd005ea75b9949156974e15d2761eb734`，在 `2e1933d8f16d0f383b85ca92ff5bdf29e4732e6d` 之上仅 F08 已独立接受。先实际核对 HEAD、diff、锁及所有既有文件。

该工作区已机械复制五个冻结但尚未接受的 F10 文件，详见 `artifacts/rust-tauri/R06/AR/W09-PREP-01/current-source-resumption-contract-readback.json`；首次 Review 将提供实际 FAIL 报告。必须显式保留其身份保护与测试，不将它们冒称已接受。F09 与后续全新 W10 Repair/Reviewer 将对公共源码分别新鲜验收；任何源变更使旧结果按影响 STALE。不要把 W12 未接受的缓存修复、W01/W03 实现或锁变化擅自复制到这里。

独立证据根：`artifacts/rust-tauri/R06/AR/W09-E01`；构建输出使用自己的 `rust/target-w09-e01`。不可改写任何旧轮证据。不可沿用他人的 target 二进制作为本轮亲验。

## 必须实际阅读的权威资料

1. 完整 1219 行修复总控文件（不是只读 F09 标题），仓库 ENTRY-01 的不可变规格副本；原下载内容位于 `/Users/study_superior/Downloads/Lingxi_R06_Adversarial_Repair_2026-10-11/R06_完整问题清单与修复验收总控提示词.md`。
2. `Lingxi_Rust_Tauri_Taskbooks_2026-09-23/` 的共同约束、架构、所有权、阶段依赖、验收协议、风险、原 R06 及交付模板；目录被忽略时用 `rg --no-ignore` 查找，不冒称缺失。
3. 原 R06-T07 全五 Steps、三交付、A13/A14；SCOPE-01 的相关 R00 24 个到期叶、原断言及上游 share；必须逐条区分本 Task 的实际贡献与尚未完整验证，不能抹掉原义务。
4. R05-T01/T05/T07 的操作网关、配额、期限、usage 和前置交接；F08 有效交付，F10 冻结自检、全新第一 Reviewer 的完整 finding/范围与实际证据。报告只是线索，自己读当前真实入口及所有消费者。
5. 当前知识服务、操作链、嵌入协议、三个知识存储、job worker/启停/requeue/cancel/delete、索引完整性/配置/rebuild/查询/引用；原 TypeScript 现役 manager/engine/store 的真实契约。旧研究引擎不可恢复。

## 当前必须完成的行为

F09 四修复要求及 C01—C04 全部保持，不能只把 peek 换 next：

- 首批与后续批每个计划块在一次成功构建中只提交一次。按真实 HTTP 请求内容逐块核对，最终 upsert 行数不是调用证据。
- 块总量、物理请求、失败请求、重试、实际写入、检查点复用、ready 复用准确区分。不要删字段或调松 expected；失败时也保留可解释事实，不把取消后的已发送操作算零请求/零费用。
- 真实失败、进程中断并重启后的缺口续嵌：兼容且已确认有效的块不重复请求，只补缺口。配置/来源/内容/分块/模型参数不兼容必须独立准确的新 variant，旧已付费产物保留。安全拒绝 409 本身不是恢复完成。
- 当前 running job 在重启后无法再领取是实际原 Step4 缺口。正确处理 worker 所有权、并发服务/重复启动和实际仍活的 job，不能无条件把所有 running 重排并造成同时双发。启动/退出/失败窗口不得吞错或假完成。
- 已 ready、同一来源/内容/分块/兼容模型的重跑按明确规则复用。不能为标签维度探测重复嵌首批；确需额外真实请求则必须独立准确计数，并证明其必要性。配置 identity dimensions0 不是已确认 served 维度。
- 数量不足、重复/错 chunk-ID、错误维度、非有限数据、存储异常、半写和缺失块不能变 ready。完整性绑定真实计划集合，不仅看 COUNT。
- 保持 F10 同源 served identity、每批全身份指纹及查询隔离；恢复时同维度换模型也不得混入。未接受依赖不是豁免；需要改公共路径时先列精确 owned 扩展/影响，完成全族验证。
- 保持来源取消/删除优先，不复活已删来源。原文/引用不因索引重建而删除。缓存未来 schema、原 F12 问题继续标明，不能通过吞错/删未来库实现恢复。

F11 的期限、Run/Session/attempt/cause、已发送未知结算仍为当前 R06 必需义务。若本 Task 修改相关共享契约，列实证与重跑范围，不宣称后续/已申报关闭；不能用新的假 usage 使数量对账好看。

## 两层自检：必须先红再修并保存全部过程

先在当前未改候选用生产二进制 + 合成独立 home + 认证 HTTP + loopback embedding 建立精确首批重复、失败恢复与 running 重启反例，正常对照同样实际跑。被测网关、状态机、消息、存储、索引不能 mock；外部供应商响应可以确定性替身。

逐项自检：

1. C01：实际 1/64/65/128/129 个可区分计划块，原始每请求 payload、块 ID/文本、物理次数、每块恰一次、N 个新写向量与 usage 一致。
2. C02：第二及第三批故障，保存原批向量字节/ID/hash；真实进程停止/重启/正式恢复入口只补兼容缺口。另做首批落库前/后中断、提交未确认边界、同模型/同维度换模型、来源/分块变化、并发 worker。
3. C03：65 块全过程进度与真实请求/账本对账，done<=total；短响应、坏维度、非有限值、错集合及 DB 写入/完成异常均不能 ready。
4. C04：同一已完成结果重跑，再改变内容、模型、端点/参数；按明确兼容规则真实复用或新建/重建，计数不重复；旧 paid bytes 不误删。

对抗性自检：追加未直接覆盖的同族边界，并隔离恢复 peek 旧根因或真实旧版本红/当前绿对照。变异必须编译成功、实际匹配正确用例且因为行为错误失败；候选源码不受污染，之后重新绑定当前实际二进制。不能把准备错误/0 匹配/资源守卫触发当旧根因红。

保留原 T07 11 项、操作服务/存储/取消/期限/usage、A13/A14 与实际受影响 R05 回归，不删除或减少旧断言。需要适配真实父/child 或新归档节点时，先说明旧断言的原语义，再保留或增强精确全局数量/来源/顺序检查。完整 workspace 非零即记录 FAIL；baseline 同族红也不自动豁免。

## 运行与证据纪律

实际工具链应仍为当前锁定 Cargo1.98.1、Node24.16，Cargo 绝对入口 `/Users/study_superior/.cargo/bin/cargo`；`--locked -j8`。不增依赖/迁移除非真实必要并登记；不改 registry/阶段图来绕过红。

每个测试二进制守卫固定 600 秒/8192MiB 开启。完整 workspace 外层预算 2400 秒加有界准备开销，不能用 600 秒外层误杀整套；正式 R05 链按原映射命令预算总和 19800 秒加有界开销。始终受宿主监督，超时实际退出/清理，不能藏孤儿或移除父子归属。后台工具返回 session 时继续监督，按适当间隔检查。

每次记录 argv/cwd/实际工具链/锁、完整有效源码与非 Rust 输入 before/after、测试 exe 与 shipping exe before/after、开始结束/真实 exit/匹配数量、平台、mock 边界、有效 home/READY、原 wire/DB/文件结果、清理与守卫。Cargo 可能重建输出二进制：保留真实前后不同窗口，不补填 before；必要时最终直接执行已构建同一测试 exe 证明稳定窗口。未测 RSS=null，不填0。

实际运行 fmt、workspace all-target clippy、workspace no-fail-fast tests、contracts、boundaries、registry；适用原正式链须亲跑并保留每层真结果。没有运行的只能 NOT_RUN，真实环境缺失 BLOCKED，产品错误 FAIL，候选漂移 STALE。不能继承任何他人命令 PASS。

home 只新建合成隔离目录；所有真实用户目录不得复制/改名/清理。禁止真实收费/外发/真实数据迁移。凭据、home、私钥、整源码归档、target 一律 LOCAL_ONLY_NEVER_GIT，安全清单只包含必要脱敏请求/业务断言/报告/log/指纹，不复制整仓证据。

## 冻结交付

最终输出问题矩阵（根因→全部受影响路径→精确修复→四 C/原 Steps/A/叶份额/新增缺口→自检/对抗/重跑）、owned 文件 SHA/大小、候选完整 digest/锁/bin、每个历史失败和纠正窗口、剩余与 STALE 范围、原始证据安全 manifest。状态只能 READY_FOR_INDEPENDENT_REVIEW；有未完成必需项须明写，不能自签 ACCEPTED/PASS。

冻结后结束该实例。总控另建全新 Reviewer 亲验，不复用你或第一轮 W10 Reviewer。公共文件须等相关所有当前独立验收有效后精确交付，不能把未接受依赖误提交。
