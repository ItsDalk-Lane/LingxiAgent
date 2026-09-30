# R04-T03 派单｜EXECUTOR-R04-T03-E01

派单时间：2026-09-30。派单人：R04 总控编排器。
TASK_BASE_SHA：39168bfb0（R04-T02 已独立 PASS 并推送确认；T02 由 1a1602c33+39168bfb0 两提交交付，最终树=已审候选）。

---

你是一次性执行代理，只负责 R04-T03：批准、撤销与只读模式。
基线：39168bfb0；分支：codex/rust-tauri-migration；工作区：/Users/study_superior/Desktop/Code/LingxiAgent。

完整读取 R04 总控提示词、原阶段书、本 Task 全部 Steps/Deliverables、
R04-A05 与 R04-A06、补充义务 R04-SUP-01（R03 递交的 ask 档审批差距，本 Task 到期）、
R04-T01/T02 报告（前置）、R03 现行交接、真实源码与测试。不得依赖前一个代理的聊天记忆。

必读文件：
- /Users/study_superior/Downloads/Lingxi_R04_自动执行总控提示词_2026-09-30.md（重点 §4.3 ask 子代理审批差距、§7.T03）
- 任务书 R04-T03 节 + 共同规范 01/02/05
- docs/rust-tauri/R04/R04_SCOPE_MATRIX.json（SUP-01 全文）
- docs/rust-tauri/R03/R03_HANDOFF.json interfaces[0].r04_obligation_t06_o1 原文
- docs/rust-tauri/R04/R04-T01_REPORT.md、R04-T02_REPORT.md、reviews/R04-T02_REVIEW_R2.md（O05：policy/gate 优先级收口归本 Task）
- 源码：rust/crates/lingxi-service/src/{approval.rs,toolgateway.rs,runs.rs,lib.rs,auth.rs}、lingxi-kernel/src/subagent.rs（RunGrant/authorize_child_tool_with_registry_id）、T01/T02 测试
- 现役语义参照：core/session-permission-mode.ts、lib/tools/session-permission-wrapper.ts、lib/tools/subagent-tool-policy.ts（operate/ask/read_only、预授权、approvalPolicy/allowHumanApproval）

任务要点（原 Task 全文仍为准）：
1. 迁移现役 operate/ask/read_only、预授权和子代理审批能力规则；权限裁决在 Rust 完成；模型审批建议不能覆盖硬性拒绝；R05 真实审批模型未接入时不伪造模型批准。
2. 【R04-SUP-01 到期】关闭 R03-T06-O1：父会话 ask 档下子代理省略 access 或请求写权限、且子代理不能人工审批（allowHumanApproval=false）时，不允许折叠成 Operate 后放行写操作；按现役 approvalPolicy（deny_on_prompt）语义返回结构化拒绝 TOOL_APPROVAL_UNAVAILABLE，不自动同意、不无限等待、不降级裸执行。同时保护：父 read_only 下拒绝提权、显式 read 的衰减（T02 F01 修复成果不回退）、正常 Operate 的合法写操作；是否存在有效预授权按真实旧契约判断，不一刀切关闭所有合法写操作。测试审批替身只扮演外部答复者，允许/拒绝规则由真实 Rust 授权链裁决。
3. 批准记录绑定有效参数摘要、真实目标、资源集合、调用主体、执行节点、Run、代次、期限、允许次数；提交和消费并发保障（两个并发请求不能各花掉同一次批准）。
4. 拒绝、超时、取消、重复点击、迟到批准、权限修订、工具卸载必须有确定结果；等待期间不预执行副作用；取消后迟到批准不能复活调用；重启后旧批准按明确持久化/失效规则处理（不能默认延续）。
5. 真正派发前重新裁决可用性、身份和授权；明确"撤销先赢"与"操作已开始"边界，不声称撤销可撤回已完成外部操作。受认证的最小批准交互接口与真实链测试（不新做整个设置 UI）。
6. 收口 T02 遗留 O05（policy Allowed 与已接 approval gate 的优先级语义）：给出明确裁定并测试钉住。

验收（两场景都必须真实执行）：
- R04-A05 批准后改参不能执行：用户批准文件 A，提交时改成文件 B/改变正文/扩大资源范围；拒绝或重新申请，B 与未批准内容不得被写入。
- R04-A06 等待审批期间禁用工具：请求等待批准时禁用/卸载目标，再批准旧请求；仍不执行，错误明确表明目标或授权已失效。
- 追加对抗检查：父 read_only 的写权限请求、父 ask 且子代理不可人工审批、正常 operate 对照；批准超时与执行竞争；重启后票据重放；不同别名不得重复使用一次授权。

验证命令（/Users/study_superior/.cargo/bin/cargo）：fmt/clippy/test workspace --locked（r00 防火墙环境失败隔离核证）、check-contracts/check-boundaries、R03 十套件钉数+A15/A16、T01/T02 套件回归。

禁止修改需求让实现通过、删断言、吞错误、静默降级或伪造证据。禁止 commit/push。
报告 docs/rust-tauri/R04/R04-T03_REPORT.md；证据 artifacts/rust-tauri/R04/T03-E01/；更新 R04_TEST_MAP.json T03 条目。
自检全过只返回 READY_FOR_REVIEW。结束本次代理。
