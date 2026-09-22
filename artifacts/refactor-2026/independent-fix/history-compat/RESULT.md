# 旧协议兼容 8 项补验

2026-09-22 本轮实际结果：两套 8 项全部通过，0 跳过，退出码 0。

初始只读检查确认 `/tmp/lingxi-baseline-1d42b740` 不存在，且旧提交 `1d42b7405c76292f617291e3a01cd2f3ef5efd04` 在本地 git 中可用。创建 detached worktree 后用 git rev-parse 再确认旧 HEAD，原始输出 old-sha.log。独立 HOME/LINGXI_HOME/TMPDIR/npm cache 下运行旧树 npm ci，按旧 package-lock 安装 1282 包，退出 0。

未修改两套测试的 skip 条件。其 cloneReady 检查 .git、旧 sessions.ts 和现行 runner 模板存在；beforeAll 将 tracked runner 模板复制到旧树 tests 目录后，用 Node24 原生 type stripping 启动旧 sessions 路由。原测试并不自行验证旧 SHA，本轮以 old-sha.log 独立补证。

实际命令、起止时间、退出码、Node 及临时 HOME 见 commands.jsonl；完整用例结果见 compat-eight.log。服务端四组合 4 项 + 新客户端旧服务端回退 4 项均通过。本验证层级为真实旧/新路由 HTTP 及现行客户端消费逻辑；旧客户端仅原请求构造，不等于完整旧 renderer/Electron 验证。

结束后 git worktree remove --force 成功，仅删除本轮新建副本及任务独有 HOME/缓存。未覆盖已有 /tmp 内容，未改生产源码、原测试跳过条件或真实用户业务数据。这 8 项是全量中跳过项目的独立补验，不改写此前全量运行的原始通过/跳过数字。
