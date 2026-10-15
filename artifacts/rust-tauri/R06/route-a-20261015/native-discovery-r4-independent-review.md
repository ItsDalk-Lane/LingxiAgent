# R4 Windows glob 修复独立复核

结论：R3 确认的 Windows 普通绝对 glob 前缀缺陷在本次源码范围已修复，PASS_FIX_SCOPE。原R3 NEEDS_FIX报告与表达式负控完整保留。

实际核对 Main discovery_tools.rs：windows_glob_path 同时规范普通/扩展盘符与UNC；path_text、glob、glob_matches、find有效模式判断均接同一表示。grep正文regex未更改，只有glob过滤走此路径。Unix path_text仍原样保留真实反斜线文件名。正式父/Python fixture删除HOME覆盖，无原40断言弱化。

独立把当前生产 windows_glob_path 与整个glob函数提取后，使用绝对rustc1.98.1及既有regex库执行，不建立Cargo target。原R3两条失败输入保持原值：盘符/UNC现在均匹配；加扩展盘符/扩展UNC/相对反斜线/纯文件名4组，合计6/6 PASS；每组均有其他驱动器目录不匹配的负控。

探针 `/tmp/native-discovery-r4-independent-glob-probe.rs`，日志同名 `.log`，源码SHA证明 `/tmp/native-discovery-r4-independent-source-proof.json`。这是宿主实际生产表达式执行，Windows句柄实机NOT_RUN。

Root本轮当前Main正式父50+原R04文件11 exit0日志已只读核对：`/private/tmp/r06-main-native-20261015/current-source-and-file-regression.log`。本审阅没有把Root运行冒充自己运行；前一轮我独立跑过R3正式父50+自有3边界例。

Windows已经实现Available；Windows/Linux/macOS x64实机仍NOT_RUN。没有全service Windows交叉编译/链接或实机证明；本轮仅关闭确定前缀缺陷，不签跨平台三叶完成或R06_ACCEPTED/R07_READY。未改Main/作者源码/登记/门禁、未提交。

当前discovery_tools.rs SHA256：6b0f2e787522a1bd24dcd859a3d20ad9ba04f8711ef82d166df557f5a809a2d1
