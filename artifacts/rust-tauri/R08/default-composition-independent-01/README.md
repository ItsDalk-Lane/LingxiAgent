# 默认组合根独立实际复验宿主（准备态）

当前状态：PASS_LOCAL_DEFAULT_COMPOSITION_ONLY。已在主集成冻结候选和释放窗口后完成run-01/run-02独立实际复验，精确结果见两轮receipt及review-result.json。整体授权准入仍FAIL_NOT_CLOSED，不能代领其他原断言。旧R07失败宿主与所有历史回执保持原样。

只替代 loopback OpenAI SSE 协议；不注入 TurnProvider、ToolRegistry、ToolGateway、ToolExecutor 或 ProcessSupervisor，不手工调用产品 registrar。真实默认装配的 registry/gateway、kernel、权限、解释器、扫描/程序外围、SQLite、HTTP执行入口及服务停止/关库重开均保持产品实现。

先检查五目标真实resolve和gateway prepare；此准备检查不宣称AST/LSP已实际执行。再从默认HTTP会话执行→真实外部模型协议请求→正式目录describe/mcp_call调用run_code（跨调用变量+文件副作用）、security_scan（真实合成文件规则/不返回命中正文）、run_tools（真实读写子动作/独立journal/外部模型只见所选汇总）。服务实际退出后重开同一合成数据库，读回工具结果且文件效果仍为一次。

准备检查采用固定可信测试上下文，不执行其handle；三组业务从真实HTTP入口创建Run。它不代领guest/phone/workflow撤权矩阵、全部五工具原断言、UI或R08迁移/回滚/前滚。所需环境缺失或能力拒绝必须实报，不能自动skip、注册假目标或改预期。

使用 `run_review.py --run` 才实际执行；默认不运行。脚本固定1.98.1，以全新独立临时home/tmp目录运行，不改全局HOME；每轮新建证据目录，保存精确命令、日志、退出码、源码前后摘要和环境边界。任何编译失败/断言失败原样保留。
