# R5 深目录性能优化边界独立复核

PASS_R5_OPTIMIZATION_SCOPE。实际复跑原3条目录替换/祖先移出后链接/深层物理取消+并发busy+下一Run反例，全PASS，原输入不变，新输出 `/tmp/native-discovery-r5-independent-frames`。源码/命令 `/tmp/native-discovery-r5-frame-probes.py`，日志 `.log`，实际bin/SHA与源码SHA见 `/tmp/native-discovery-r5-independent-confirmation.json`。

只读核对entry两侧目录verify、target重新authorize、非link before/meta/末次entry身份均保留；仅当授权后的target.parent确实等于当前parent且before不是链接时复用原DirFd，链接/异parent仍独立打开。walk首次names前后verify后重用刚持有dir，仍核fd identity；后续帧恢复从路径重新授权并比expected。没有发现确定的重挂载/父重命名/链接绕过。

Root400层/下一Run/取消/再下一Run50检查21.07s记录由Root执行，本轮没有重跑全50；没有提高timeout/去掉原断言。Windows/Linux运行、整个正式窗口没有在本轮独立运行，不签跨平台或全R06完成。未改Main/源/门禁。
