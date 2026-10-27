# T08 → T09 交接

T08候选h的本地实现及适用20门已独审接受，范围以T08_REPORT及ACCEPTANCE_t08h为准；原343条不全领通过，最终清账仍归T12。

## 继续实施T09

沿当前分支与工作区，保留全部其他软件已有修改，不提交或推送。复用现有Rust执行、会话、事件、文件、凭证和私有运行实例读面。

CLI当前仍默认Node，Rust status缺实际助手/模型，chat/continue明确不可用。应接真实创建/历史/WebSocket/审批/取消/恢复及EOF退出，不在Rust失败时回退Node。

手机已支持Rust登录但bootstrap明确阻止继续；四个原手机scope不含settings.read或sessions.read。新增chat可读白名单上下文，真实读取选定助手/会话及实际模型，不透传完整设置、provider metadata或秘密，不扩大手机权限。既有GET /sessions是实际归属列表，可最小复用；studio-list另有sessions.read限制，不混为同一能力。

默认模型不能取错误全局焦点；明确Agent/session按现有effective_choice优先序。模型未配置、已无效和资料损坏分别显示，不伪造可运行状态。工作台和文件复用T07链，资源下载继续要求resources.read。

原39叶及A17/A18详细起点见[T09实施准备](T09_IMPLEMENTATION_READINESS.md)；补充CLI和mobile只读方案在artifacts/rust-tauri/R07/T09。以当前代码及实际行为为准，不把方案中的拟议DTO当已经存在。

## 必须延续的证据规则

全部Cargo由主控统一编排。Cargo test可能重建并覆盖共享服务程序；真实页面检查必须等同候选正式build成功并绑定SHA，必要时用独占只读副本。h副本记录见t08h-preserved-build1.json，后继不能借h检查声称新代码通过。

原失败不覆盖；新候选必须新编号。每项分别记录实际命令、退出、版本和日志。真实账号/付费/外发/真实数据/提交推送发布未获授权。

T12未清条款及13个本h未执行adapter项继续保留，不将未执行算通过。R06已获用户接受，不重新要求整轮R06；按实际修改做局部回归，最终R07共验另行固定候选。
