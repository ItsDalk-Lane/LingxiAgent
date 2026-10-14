# A02D8C572F5C：Rust模式离线启动头像

## 完成范围

原叶R00-T02-LA-A02D8C572F5C两条断言保持原文：返回本地头像路径供splash渲染；路径不存在回退默认头像、不等于设置或更新头像。本项仅桌面启动/引导IPC、本地只读头像解析和当前SplashApp失败回退。没有新增设置头像接口，没有改Rust业务、主树源码/登记/门禁/白名单，没有提交推送。

基线从当前LingxiAgent工作目录实际HEAD bf8c533dc282fdf35901adaeaa5ddc1edcd557b5亲读并记录，不能把literal main ref当本轮主工作目录。新全量worktree检出历史artifacts到约49%时ENOSPC，git撤回失败目录；按主控指令复用原归档独立树，只同步1574个桌面/共用输入、保存逐文件精确基线，不复制历史artifacts或整个target。本补丁只含本项净增量，与旧归档/EX/memory补丁无关。

## 实际生产链与修复

Electron main.cjs的lingxiHome由shared/hana-runtime-paths.cjs resolveLingxiHome(LINGXI_HOME)确定；startRustServer同一变量作为--home和环境LINGXI_HOME交给Rust。启动画面在服务尚未启动时显示，仍应走离线本地桥，而不是反查HTTP。既有preload getAvatarPath→实际get-avatar-path IPC→platform.getFileUrl→当前SplashApp img一直是合法消费点。

原壳选择助手只读primaryAgent；现役Rust agents服务实际写current_agent_id/primary_agent_id，current优先。原代码遇到两助手会扫描第一目录，显示错头像。本项启动数据注册函数在Rust模式读取实际下划线记录，Node模式保留primaryAgent。头像与启动基本信息读取同一个选中助手；未触全局getCurrentAgentId及其他窗口消费者。用户头像仍读user/avatars，助手头像仍读agents/{id}/avatars；不注册任意传入路径为权威。Rust当前新写目录与此一致，因此无后台改动。

读取仅允许合法role、合法Rust agentId、真实普通config和不带删除标记的助手。检查home内每级目录及文件，不接收文件/祖先链接、目录/FIFO等非普通文件，最多15MiB。文件头须对应PNG/JPEG/WebP，PNG/JPEG再由真实nativeImage解码。实际实验确认Electron42 nativeImage不支持有效WebP，故WebP用已授权当前frame的createImageBitmap(Blob字节)真实解码，不加载URL、不改CSP、不新增依赖。异步解码后复核文件字节及当前本地页面，离开本地入口不返回路径。

IPC仅当前已知启动/引导窗口主frame、精确本地HTML或显式loopback开发页可用；未知窗口、子frame、远端页面拒绝null，不新增远端本地路径披露。真正EACCES等意外读取错误保持严格IPC拒绝，当前组件独立处理头像/信息读取结果并显示可用的当前角色默认头像。即使返回路径后文件被移走，实际img onError也回到默认头像。

现役Rust config.yaml保存字符串带引号；旧正则把"ming"连引号交给视觉选择器，导致默认头像也可能选错。本项读取真实YAML已有依赖，保留未知内容只读，不回写。坏YAML/字段类型明确拒绝，错误不输出原配置内容或密钥。

## 真实闭环验证

主控已实际编译bf8c的单个binary先复制为/tmp/splash-avatar-evidence/bin/lingxi-service（118145120字节，SHA与源码依赖见source-proof），不再依赖会被主控后续重编覆盖的target。

集成脚本设置runtime=rust，使用现役resolveRustBinary/readOwnedRustConnection和同一resolveLingxiHome；真实HTTP创建alpha、zeta两助手，PUT默认alpha、POST当前zeta，保存alpha JPEG/zeta WebP/user PNG，核实际偏好和已落盘原字节。随后真实SIGTERM退出0并证明原服务端口请求失败，才建立窗口。窗口使用生产注册函数、生产打包preload.bundle.cjs、生产构建splash.html/当前SplashApp，无头像返回字符串替身。

实际Electron42.8.1、可见380×280窗口的16项记录全部PASS：

1. 真实Rust三格式保存与current/primary归属；2. 服务已真实停止；3. 离线正确zeta WebP路径与实际48×48解码图片；4. user PNG真实IPC路径+Chromium48×48加载及非法role拒绝；5. 未知窗口/远端origin拒绝；6. 缺失头像默认Ming；7. 错格式默认；8. 损坏WebP默认；9. 后缀不匹配默认；10. 目录默认；11. 真实FIFO默认且不阻塞；12. 文件链接默认；13. 祖先链接默认；14. chmod000真实EACCES，IPC拒绝而组件仍Ming默认；15. IPC返回后、实际Chromium文件加载前移走原文件，真实onError默认；16. 离线空宅null路径/null名字与Lingxi默认。

所有图片记录必须complete、naturalWidth>0、窗口visible、body opacity>=0.99，才计实际显示。自定义/Ming默认/空宅Lingxi默认三张capturePage截图已经亲读。所有读取场景结束仍校验原WebP与用户PNG字节保全，没有设置/更新副作用。

稳定最终receipt来自/tmp/splash-avatar-evidence/final-isolated/receipt.json，本目录保留同字节splash-avatar-native-receipt.json与截图。原始运行目录、二进制、profile和stderr不提交。

## 修前负控与检查

负控把bf8c精确getCurrentAgentId及两IPC注册片段装入真实ipc-wrapper/真实Electron窗口，其他组件、Rust数据保存/停服和强断言不变。首次实际失败exit1：当前SplashApp显示alpha JPEG（src agents/alpha/avatars/agent.jpg、48×48、opacity1），期望zeta WebP始终不满足。日志/tmp/splash-avatar-old-host-negative.log。不把未捕获、fixture路径或纯函数结果当GUI成功。

新增8个辅助单测与旧preparing4/assets2共14/14 PASS；实际Electron16记录PASS。root tsc --noEmit、tsconfig.test tsc、main/splash/preload生产构建及Node语法检查PASS。原有断言和门禁未改。

最终运行HOME=/tmp/splash-avatar-evidence/synthetic-home、TMPDIR=/tmp/splash-avatar-evidence/tmp，单个自有Rustbinary；未运行cargo（本项无Rust改动）。最初整目录node_modules链接使Vite空.vite-temp目录元数据在共享依赖目录更新；发现后改为独立node_modules目录、逐包只读引用并排除.vite/.vite-temp/.cache，最终缓存均独立，不擅自清理共享缓存。主树源码/登记/门禁未写入，但不把先前共享缓存元数据说成完全没有写入。

## 局限与复跑

真实macOS Electron IPC/生产preload/当前组件原生集成宿主已测；没有运行完整desktop/bootstrap.cjs/main.cjs引导、安装版或Tauri窗口，不宣称其通过。Windows/Linux、正式打包、全R06门禁未执行。无R06_ACCEPTED/R07_READY或治理变更。文件读取与之后窗口加载并非文件系统原子操作，后续外部变化由实际图片错误回退兜底，不声称能冻结用户文件。

复跑先构建main/preload/splash（既有命令），再用实际Electron运行本目录splash-avatar-native.cjs，参数依次为候选仓库绝对路径、独立合成输出目录、固定自有实际Rustbinary绝对路径。原始强断言在脚本里；负控仅通过测试进程环境LINGXI_SPLASH_BASELINE_SOURCE指定bf8c精确main.cjs源码，产品没有该模式。需要实际GUI与现役bf8c产品素材，不能用headless或字符串mock签原生PASS。
