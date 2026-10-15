# EX12 两个切换遗漏的独立复核

结论：当前 Main 这 7 文件修复在限定服务范围 PASS。独立真实 HTTP 新正反控制与 target-only 原反例均通过；没有修改 Main、原断言、登记或门禁。该结论不代替正式 R06，也不扩称真实 UI/其他平台。

冻结源组 `b8084d75a5e645ad87b1e9d4906ebb9e1202dd453b0a8538426de18dbf9a7f87`，净 patch SHA `6a40e9ae137e6a56325b7630241d40f64cd08ef8986ceb43258767523501239a`。Main 7 文件逐个 SHA 与冻结候选相同。真正运行 Main service bin 前后 SHA 均 `6ff9b025e1138ca8e02bd250c5f9217eafc35cd3611d2c90ab1479861914db5f`，无源码/二进制漂移。

## 生产改动核对

- switch 复用同一 session_focus::create_locked，不另造会话内核。focus_mutation 覆盖来源读取、创建与广播，实际新会话有可信主体/Agent、cwd、权限与持久模型选择；旧 A/B SessionFacts 不改归属。
- 目标 models.chat 用原 AgentConfigDocument::model_reference 严格读完整 provider/id，显式未知目标模型在写入前拒绝。目标覆盖先于 thinking/default dispatch，只有无显式 ref 才继承原默认。
- effective_choice 默认入口只委托 effective_choice_with_model(...,None)，其原默认读取/推导函数体保留；显式 body.model 在原 thinking 前覆盖。最终新增模型预校验将显式无效 NewBody.model 保持原 HTTP400，不变成提前推导的500；无参数默认分支不加新要求。
- 当前助手删除先成功建立替代真实焦点，再进入原不可逆删除/持久收据流程。后段 rename 失败可保留已提交 B，响应明确给 replacementSwitchResult，不伪称回滚。原待清理、引用修复与重启处理保留。
- cwd 历史与焦点提交复用实际配置锁；焦点写失败尝试撤回本次目录历史，失败回退也明确报错，不能把两个文件误称操作系统级事务。

## 旧测试夹具语义

lifecycle 的假 `/tmp/first/second` 改为实际独占目录，增加原完整会话要求的已声明合成模型/工具工作区。cwd/历史顺序、重复去重、实际 YAML 值、拒绝码与所有原助手事件相对顺序均保留；不通过过滤事件放宽，而是严格加入真实 session-created/session-focus-changed。

r3_reference_failure 把只读 prefs 故障移到原已有 AfterMoved。hook 只捕获当时真实 B+新 B 会话偏好并加 uchg，不改写 Agent。故障后严格字节等于该快照、primary 仍 A，原 logicalDeleted/referencesPending/cleanupPending/replacementAgentId-null/referenceError 断言保留，重试后 primary/current B 且相同会话。rename 失败坏收据测试只把基准改到已实际提交 B 后，保留坏收据三入口/字节/资料/启动拒绝等强断言。原 279 叶文字未据此减免。

## 独立实跑

1. `independent-switch-controls-r2.py`（仅复用作者外部 HTTP 提供方/服务启动清理夹具；业务断言独立新增）实际 Main 服务启动、HTTP、真实网关/工具/SQLite/磁盘，exit0，9个实际外部替身请求、3个真实 Run。
   - B model ref 缺 provider，HTTP400；prefs、sessions、全部 usage 行、A/B YAML、模型选择 sidecar、事件完全不变，没有额外请求。
   - 切 B 后真正 provider 请求使用 target-model，而非 global-model。
   - 反向删除当前 B，创建全新 A 焦点；事件、响应、prefs 指向同一新 A 会话，下一真实请求使用 A/global-model。
   - 旧 A/B 各已有7条非空 canonical messages；两个旧会话归属、全部消息/branch heads/usage rows 在删除及下一 Run 后逐项原值不变。不是只检验行仍存在。
2. 冻结作者 target-only 原强检查用 Main bin 独立重跑，exit0、3个真实 provider HTTP 请求及对应实际 usage 行：没有全局 chat route，合法目标 B ref 仍成功，实际使用 target-model。

此前 r1 独立短检查也 PASS（6 请求），作为先验保留；r2 是为补非空双会话历史逐行保全断言而新建合成目录，不覆盖旧证据。全部用合成 home/tmp，HOME 未更改，不接真实业务资料。外部供应商为 loopback 替身，费用账本是真生产写入的 usage 记录，不声称真实供应商或非零货币扣款。

机器结论与逐文件/二进制 SHA：`switch-final-main-confirmation.json`。原始 HTTP/模型请求/DB事实：`actual-main-independent-r2/`、`actual-main-target-only-r1/`；日志同名前缀 `.log`。原失败与旧 R1 候选检查未删除。

本轮未独立重跑全部18父/真实界面/Windows/Linux/正式门禁。Root 执行的18与focus UI父属于另一路证据，不冒充本审阅者执行。后续因真正创建会话暴露的其他旧成功夹具，只接受必要真实模型/目录准备，独立配套未交付前不预签。

## 后续四份必要夹具配套只读核对

Node2补丁 e2c6a04f3d05466bcd83f20531aa5fd6a1722bab5ed164e408ea7589407ba106：仅启动前声明合成chat模型和真实独占workspace，CLI改用config；不配置memory/title辅助模型。Splash原31个assert调用全文相同，owner原UI/父断言源码未改。具备可解析模型不等于端点可生成内容，不以localhost:9声明供应商运行成功。见 node-fixture-static-review.json。

Rust2补丁41011b05902abb7c098c8ee79a862f8a3ec109aff63d9b83eaa7438fafac060a：readback仅声明合成可派发模型/凭证；原current-status无焦点case显式移除合成prefs的current_session_id，保留原null及读请求无副作用检查，避免依赖旧switch未建session的错误。settings两父保留原primary准备，先真实PUT order保留A引用；仅AfterMoved读回确认真实B及order[A,B]后加只读标记，不写回currentAgent A。这样后续真实修剪order失败产生原referencesPending，原UI11/12、零外来PUT及重试断言不变。UI两源SHA与冻结报告一致。见rust-fixture-static-review.json。

这四份仅签配套hunk未弱化的静态结论，未独立重跑它们的GUI父；作者或Root实际14+2/1+1结果仍分别归属其原证据，不代为冒签。
