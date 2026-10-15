# 工具暴露、派生资料与旧合成目录修复：独立交叉只读复核

结论：PASS（仅本次补丁与环境修复的只读复核范围），没有发现需修缺陷。没有修改 Main、HEAD，没有启动 Cargo/npm、生成器或重跑任何检查。作者已有运行日志作为既有证据阅读，未冒充本 Reviewer 独立动态复跑，也不将原正式失败改为通过。所有本次输出只在本目录。

## 1. 工具声明不是执行授权

审阅的是 /tmp/r06-repair-tool-exposure/net.patch 的三文件净增量，当前 Main 的三文件字节均等作者 candidate SHA，当前 git diff HEAD 的三文件内容与 net.patch 完全相同。不是只看作者结论或常量未变。

`ToolRegistry::add_model_resident`（kernel/toolcatalog.rs:1747）在同一锁内确认实际 target 已登记，仅向已有 Some resident 集合添加该 target，真实首次添加才增加 catalog_generation。不存在的 target 返回 TargetNotFound；重复调用不加代次。None 分支保持此前“所有 Available 均声明”的语义，不把它变成仅一个声明。没有更改 manifest、availability、permission、schema、target_generation、资源绑定或 executor；也没有根据 local_name 模糊查找另一个来源。

`model_declaration_snapshot_with_activated_targets` 仍先要求 Availability::Available，再要求 resident 或当前可信 activated 集合，故把 Deferred/Disabled target 加入 resident 也不会使它出现在当前声明。`prepare_invocation` 仍核目录代次、真实目标、当前可调用性、当前 schema 并从真实 manifest 导出权限；gateway 继续资源规范化、policy/approval 与执行前复检。ToolDeclarationSnapshot 继续从同一完整描述生成 schema/说明/target，冲突按真实目标命名空间处理；未知 wire name 的反查仍 None，没有新增猜测/别名回退路径。已有旧快照保持冻结，不能借新增 resident 改写历史 target。

当前生产 add_model_resident 只有两个启动期调用：
1. service/lib.rs:1518，process tools 已真实注册、原目录桥配置完成后，只取 register_process_tools 返回的 write_stdin_target。不是从模型名字或参数激活，也不把全部 Available 放入 resident。
2. service/lib.rs:2006，worker 必须来自原封闭配置入口，且真实模型链/工作区、生产 callback 端口、argv 可执行文件与 cwd、register_worker_tool、原显式 Available 设置均成功后，只取实际注册返回的 registered.target。其他 register_worker_tool 调用者仍原 Deferred；无 workers 配置时不进入。既有 plugin:builtin 身份、schema、Execute 权限和 path_args 授权路径原样保留，未提升为 first-party、LocalUser 或另一个执行对象。

## 2. 与原 R06/R05 合同的相容性

F07 原文（AR_ISSUE_MATRIX.json:1915–1919）要求区分“注册/可调用/常驻/按需”，保留四基础与必要目录入口，其他工具依会话来源/授权激活；C01:1931–1941 要求实际 wire 严格等于冻结集合加明确必要基础设施，不能无条件出现全部知识/记忆工具。不能仅以 RESIDENT_NAMES 还是七项，就称所有真实请求仍只有七项。

本修复的实际声明是：普通真实生产链八项（七项基线 + write_stdin），显式配置单操作 worker 后九项（再加该配置实际 target）。这是有意识恢复原 R05 合同，而非把八/九说成七：未改的 r05_t08_production_tools.rs:20、:847–853 明确要求原四基础与 write_stdin 终端配对都在生产 wire；:1010 以后要求真实 main→配置 worker→aux→main。原 service/lib.rs:1985 起既有注释与 Available 设置已经明确配置 worker 必须 resident，此前只改 availability 不改 resident 筛选才使该既有要求失效。知识、记忆、技能和无关新注册 Available 目标仍排除在默认共享 resident 集合之外，原按需 describe→同 Run 记忆→当前 generation→gateway 调度链保留；本补丁没有改按需目录逻辑或可信激活来源。因此这两个精确宿主追加与原合同相容，不构成“因为 Available 就全部常驻”的复发。

原 F24 整个文件与 HEAD 逐字相同，原 6 个场景及 guard 均未改。F07 去掉新增两个函数后全文与 HEAD 逐字相同；新两测试真实构造 registry，覆盖未注册/重复/代次/None、默认七项、无关 target、真实目标/schema、Deferred/Disabled 和旧快照。新测试的 worker 用 ReadOnly manifest，仅证明目录 API 边界，不能单靠它证明生产 worker 的 Execute 权限；后者由生产未改 manifest/gateway 审阅与既有 F24/R04 实跑日志共同支持。已阅读作者原始 36 项日志：F24 6、F07 10、R04 catalog 6/gateway 13、unknown-wire 1，均日志实际 PASS；本轮未重新运行，不宣称新的独立动态验收。

## 3. 两份生成 JSON

当前两份 JSON 的完整字节 SHA 分别等于前次只读隔离、未修改正规生成器输出（20e90c...504c / 1f265e...3ba4）。实际结构差异也仅预期字段：
- API_COMPAT_MATRIX：contentSha、preload/types 两输入 SHA、inventory.preloadIpcChannels 增加 invoke:quick-chat-open-session-ack；surfaces 和 summary 全部不变。所有 sourceDigests 独立读取当前输入字节复算均匹配。生成器源码仍按完整 inventory/surfaces/summary/sourceDigests 计算 contentSha 并全文比较，没有字段豁免。
- OWNERSHIP_TARGET：仅 feature_inventory 来源 SHA，独立读取当前 R00 输入也匹配；没有改变 feature/store 所有权、owner 数量、公共 API 或接受规则。生成器投影字段与 SHA 输入逻辑已阅读，未执行或修改生成器。

ack 不是悬空登记：preload.cjs:86–92 在原 onQuickChatOpenSession 闭包成功/失败分支实际 invoke；main.cjs:5647 有实际 wrapIpcHandler；quick-chat-session-handoff.cjs 的 acknowledge 校验主窗口 sender/mainFrame、requestId、会话和 ownerKey，再聚焦并收起快捷窗。因此新增内部 IPC 没有凭空新增公开 PlatformApi 方法的需要。这个产品实现早已在 HEAD，本次只有派生清单追上真实输入。

.sync-audit 与两审计测试、R02 脚本相对 HEAD 无已跟踪差异，未更改 verified coordinate、白名单或检查标准。本次不讨论或追认早期 R00 条款修改的授权；当前生成修复没有修改 R00。

## 4. 旧合成目录权限修复

独立读取原修复收据，路径限定本任务旧未跟踪 archive-execution-r2 合成运行 home 内 agents/lingxi/sessions；当前实际 uid=501、权限0700，完整文件遍历仅两文件，字节 SHA 与收据 before/after 都完全一致。修复收据记录0500→0700；新副本 cpExit=0、rmExit=0、removed=true，副本两文件 SHA 同原始。此举恢复拥有者对自己合成父目录的删除权限，没有删用户业务文件、改变 R02 清理标准或去掉不成功就失败的要求。

边界：0500 前态和新副本实际 cp/rm 退出码来自原始修复收据；本 Reviewer 只读核当前 mode/uid/两文件字节，没有重新 chmod/copy/rm，不能将收据历史冒充本轮重新执行。原正式失败保留，环境修复只使后续原标准可以正常执行。

## 交付与限制

RESULT.json 包含独立当前 SHA/来源逐字对照、三原合同文件保留比较、两 JSON 精确字段 diff、全部 API 来源 SHA、权限当前事实与原始收据 SHA。reviewed-tool-current.diff / reviewed-generated-current.diff 为本轮实际读取的 Main diff。此审阅不覆盖其他作者改动、全量正式门禁、Windows/Linux、真实外部供应商或当前 GUI 动态验证。
