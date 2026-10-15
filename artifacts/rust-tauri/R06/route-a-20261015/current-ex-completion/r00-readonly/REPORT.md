# R00清单扫描器只读核验

结论：当前扫描实际FAIL，四个“只有注册/只有调用”差项是静态发现器漏识别；同时两条新增channel在旧IPC_DOMAINS确实未归属，不能把整个问题说成“换个引号即可全绿”。没有改Main、扫描器、登记或白名单，也没重生成任何原清单。

## 实际复现

`python3 -B docs/rust-tauri/R00/r00_t02_inventory.py` 当前退出1，current-original-check.log留完整回溯：handler-only get-server-connection-info / quick-chat-open-session-ack，bridge-only get-avatar-path / get-splash-info。这只是第一处失败；尚未跑到源码摘要/生成件漂移等后续校验，不能因消除四项就宣称全部检查通过。

独立TypeScript语法树只读发现器 ast-probe.cjs 解析当前4真实源码，不执行Electron。ast-result.json逐条给出：

- get-server-connection-info：Main5565已注册，preload36真实调用用单引号；旧桥接正则仅接受双引号，故漏。
- quick-chat-open-session-ack：Main5647已注册，preload90/91两分支真实invoke在Promise回调内；旧正则要求同一行先出现对象属性“word:”，故漏。
- get-avatar-path / get-splash-info：preload142/143真实调用，实际handler位于共享模块126/147；Main5876真实调用registerSplashDataIpc。旧扫描只读Main/updater两文件，且handler正则只接受双引号，因此漏共享模块的真实注册。

同一独立AST读取的两侧频道集合差集均为空。这只证明所读4文件中的静态注册/调用配对，不替代当前Electron窗口/权限/图片解码运行证明；那些由各原叶现行原生测试负责，此次没有冒用作者截图或仅凭AST声称动态PASS。

## 两个仍需正式归属的新通道

旧IPC_DOMAINS没有 get-server-connection-info / quick-chat-open-session-ack，前者是统一连接描述、后者是既有QuickChat打开会话的确认步骤；头像/启动信息两项已有映射。即使修正发现器，旧脚本下一检查747-748仍会拒绝这两条未归属channel。应依据已存在功能真实契约明确支持关系/必要新入口，而非忽略该频道、扩大白名单或靠改引号绕过检测。此报告不擅自给新FID或改变原验收。

## 是否本轮必要检查

当前package.json scripts、scripts/、tests/、.github/及R06文档中无 r00_t02_inventory 调用引用；R06正式xtask stage_maps/R06.json的commands没有该命令，r06_stage_inputs.py/evaluation_audit.py也不调用它。故这不是当前 npm test/typecheck 或R06已登记命令的直接执行项；其失败不能偷换成R06某业务测试失败，也不能被说成已通过。

但它仍是R00-A03/A04的现役登记校验：R00 ACCEPTANCE_MAP.json tests[T-002]和results RES-R00-A03/A04明确绑定此脚本，r00_t07_build_map.py240也保留该对应。历史PASS仅绑定当时16aeb380d等冻结版本，不能当作当前树扫描结果。若目标包括重新证明当前R00清单完整/全仓门禁，此处是确实未解决的清单维护缺口，不能退役门禁或重绑旧戳冒充新扫描通过。

## 建议边界

后续若授权修扫描器，应使实际语法树发现覆盖字符串引号、嵌套回调和从Main确实注册的共享模块，同时保留新增IPC无归属、重复注册、桥接缺失等原负例。新增通道需按实际功能归属维护，不能简单排除。扫描器修复与生成件更新要单列审阅，并保留本次原FAIL。

候选逐文件SHA见source-sha.json。没有运行完整Electron/Tauri、跨平台或完整R00再生成/负例套件。
