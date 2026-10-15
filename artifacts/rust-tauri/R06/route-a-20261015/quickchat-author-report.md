# QuickChat 单叶冻结交付

- 原叶：A2C385A33493。完整两条原断言见 quickchat-original-leaf.json，未改断言/白名单/门禁/登记。
- 基线提交：018060254e91f06ed7fe282150f4c26475b00cf3；仅23文件净包，逐文件基线/候选 SHA 见 quickchat-one-manifest.json。
- 候选聚合 SHA：ae1b42e5cd18472720f6d642edee11e740e6554fddd1562f927443b71e260799。
- 补丁 SHA：1d876bab8439a33949bafbf2c20d830b8adc13626f9350d4220970e8f77c0faa。
- Main 全程未改、未提交/推送，未添加依赖/缓存目录，未改权限、未访问真实资料。

实际行为：主进程只接收当前快捷窗主frame请求。每次生成独立requestId并冻结来源摘要、SID/path及期限。主renderer必须先走真实owned switch（保留既有同路径快速返回），再核实当前稳定SID/path/连接代际，最后ACK。只有ACK有效且主窗口真正获得焦点，才隐藏来源窗口。过期、错来源、错目标、窗口销毁/替换、主导航、重复ACK、负ACK都明确失败并保留快捷窗。

现役Rust快捷窗原bootstrap明确报未迁移，因此补最小真实启动与发送：复用正式typedWS、composer coordinator与当前真实API；session-permission-default新增只读真实偏好路由（默认auto，权限settings.read），不放大权限。5语言补失败提示/已有缺失标题。其余旧聊天路径保留。

最终原叶真实父：
`CARGO_TARGET_DIR=/private/tmp/lingxi-ex01-target CARGO_INCREMENTAL=0 /Users/study_superior/.cargo/bin/cargo +1.98.1 test --manifest-path rust/Cargo.toml --locked -p lingxi-service --test r06_quickchat_completion -- --nocapture`

quickchat-parent-frozen-final.log：1/1 PASS，10.03秒。实际产物：
`/private/var/folders/5r/wq54gtms21gfjgr5v94ynfyh0000gn/T/lingxi-quickchat-completion-a869a2cdf0ebe88b`

9组硬断言：真实生产show/尺寸改变；实际QuickChatApp发消息、CLI创建真实自有会话和HTTP模型回复；真实switch+ACK后聚焦主窗并隐藏；主窗DOM包含同一回复；同路径真实服务focus事件；实际renderer迟到ACK不能隐藏；真实主连接凭证变化拒绝；真实会话先归档后永久删除再打开失败；真实main.destroy后明确失败且快捷窗仍可见。删除前按实际SID从真实model_call_usage读1条账，和1次原始HTTP请求相符；ledger-receipt/raw-provider-requests/electron.log/result.json保留。截图 floating.png、floating-replied.png、main-opened.png 均实际生成，已亲读后两张。

父测试自行从当前生产preload源构建到本轮临时证据目录，随后启动双真实Electron BrowserWindow。悬浮窗口show/resize函数从当前生产main.cjs原样提取执行，外部页面入口/诊断依赖注入合成宿主；会话/发送/组件/IPC/来源守卫均用生产模块。超时注入只延迟真实renderer已完成的ACK，主进程测试期限3秒（生产15秒）。没有伪造SID、直接注入聊天内容或借用旧收据。

旧缺陷真实复现：node quickchat-baseline-probe.cjs（非净包）执行冻结基线原handler，mainWindow=null且目标不存在时实际hidden=1，断言exit1，见 quickchat-baseline-failure.log。它是实际旧处理器负控，不是新实现的源码字符串匹配。

其他验证：quickchat-unit-frozen.log 6文件24项PASS（原窗口/布局/生命周期/来源绑定回归及新交接行为）；quickchat-typecheck-frozen.log PASS；quickchat-clippy-final.log严格-D warnings PASS（之后仅locale/真窗口夹具变更，无Rust变化）；三生产CJS node --check PASS；quickchat-replay-result.json 23文件净补丁重放逐SHA一致。

早期失败均保留：临时偏好目录未建、测试宿主未使用产品CJS互操作配置、删除会话后才读账导致账被真实清除，均修正验证安排且未放松断言。首轮Clippy误读前一私树V14 check缓存；仅cargo clean -p lingxi-adapters后重生成14.38秒PASS，未增加target或下载依赖。

合并边界：main仅quick-chat-open-session handler块；preload仅两交接方法；app-init仅import与callback；types仅QuickChat合同；lib/auth仅一只读路由注册；userprefs仅新只读handler。不要整文件覆盖Root资源/实验或会话作者browser hunks。无需改session-actions、ChatPage、输入框/ReviewPanel/typedWS路由。

限制：本轮真实宿主为macOS Electron/当前Rust CLI；Windows/Linux运行NOT_RUN。模型为真实HTTP OpenAI协议替身，不是付费供应商。全应用打包与全库门禁由Root集成候选另跑，未冒称完成。
