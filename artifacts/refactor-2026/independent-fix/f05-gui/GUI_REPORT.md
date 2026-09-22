# C25/C26 本轮真实 GUI 证据

本轮实际用源码 Electron 主进程、Vite 当前 renderer、真实全量 server 和本地协议供应商启动，HOME/LINGXI_HOME 均为新建临时目录。未操作用户正在运行的 Lingxi，也未运行会覆盖开发数据目录的 scripts/launch.js/dev-web.js。技能：webapp-testing。工具：cua_repl 原生 Electron UI。

首次启动为 onboarding，保存 initial-onboarding 日志；第二次预种合成 preferences.setupComplete 后进入主窗口。实际动作顺序（这是操作摘要，不伪装原始命令日志）：输入中文任务 → 点击发送 → 第17行输出时粘贴中文草稿 → 第53行输出时选中“中文草稿”并向上滚动 → 第68行输出时 Cmd+C/右移/Cmd+V，草稿出现“中文草稿中文草稿” → 流完成到90行 → 打开运行信息卡。原始动态 AX/截图返回在本次 CUA 工具记录中；落盘 AX/截图为后续快照，不能拿快照时间声称仍在流式输出。截图实际视觉检查可见草稿和滚动后较早正文行。

typeText 中文尝试未造成可见输入，paste 成功；因此只证明中文粘贴编辑，不宣称真实输入法 composition。文本替身未生成长代码/Markdown或工具卡；运行信息卡展开不能替代工具卡展开。C25 总项仍 BLOCKED，已验证子项保留。

C26 未执行真实浏览器开合循环，也未采集多稳态JS堆/原生进程/监听器/定时器/各缓存边界。主端同时跑全量测试与构建，当前观察不能作配对性能结论。不存在“缺GUI授权”阻塞；真正缺的是原任务书完整负载与测量证据。需在固定负载空闲环境补齐，不能把一次任务或浏览器控制桥连接当成 A10 PASS。

launch.json绑定当前SHA/Node/隔离路径/PID/端口；electron.log与vite.log为原始stdout+stderr；provider-count.log只记本地请求时间，不包含凭证；result.json记录会话真实写入和第90行、缺口、退出。仅终止本任务 supervisor，其自建Electron退出0、Vite收到停止退出143，两者如实记录exits.jsonl。临时数据不作为安装产物证据。


## 隔离修正（本轮后续实测）
macOS Electron 的 appData 不随 HOME 环境变量迁移。后续新包与另一轮测试同时使用 LINGXI_HOME 末级 data 时发生实例锁碰撞；lsof 确认缓存实际位于用户 Library/Application Support/Data。早先两轮只能证明业务数据隔离，不能声明所有 Electron 缓存隔离。该 Data 目录来源无法完整确认，因此保留不删除。后续新包与长运行验证均改用唯一随机末级目录名，并以 lsof 记录实际 userData。

## 最终验收引用

以上原始短轮摘要保留。其未执行浏览器循环/Markdown/工具详情的结论已被本轮后续补验更新，当前C26请读 ../f05-soak-extended/C26_RESULT.json 与 docs/refactor-2026/independent-fix/C26-local-soak.md。扩展实测42次浏览器任务、42次取消、99样本，并补持续代码输出期间的交互；真实IME仍未证，资源整体仍BLOCKED。截图中的审批栏问题已按F03真实消费者回归先红后绿修复；修前截图不作为修后GUI整体通过证明。最终产物另外在 ../f05-package/PACKAGE_REPORT.md 记录。
