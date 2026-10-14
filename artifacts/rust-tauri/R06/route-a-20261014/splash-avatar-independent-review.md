# R06 A02D8C572F5C Splash 本地头像独立复核

日期：2026-10-15。结论：**本轮限定范围 PASS**。没有发现冻结候选范围内的新阻断缺陷。当前Rust服务真实保存头像并真实停止后，生产Splash组件和preload通过真实Electron IPC显示正确当前助手头像；不存在/损坏/无权限/链接/后续加载失败均如实回到默认头像，未误当作设置或更新头像。不给R06_ACCEPTED/R07_READY。

## 原断言和候选坐标

原叶完整assertions从现行绑定直接读取，保持原文：

- 返回本地头像路径供 splash 渲染
- 路径不存在回退默认头像；不等于设置或更新头像

冻结净补丁 `/tmp/lingxi-splash-avatar.patch` SHA256 `7c7f1b79bec94d0b03dd8bed16affa11d7234decb2a2a32c3f10d54b5bec309e`，桌面基线 `bf8c533dc282fdf35901adaeaa5ddc1edcd557b5`。独立复用旧树 `/private/tmp/lingxi-r06-review-persona-native`，仅从该commit按所需desktop/shared/lib/构建配置重放输入，然后应用净补丁；没有完整checkout、复制历史artifacts或target/.git。旧树Rust此前补丁未用于本次编译或运行。

本轮5个冻结候选文件在开始与最后都匹配作者source-proof的精确SHA，三生产文件也与当前Main逐字SHA一致：

- main.cjs `bd71b90b2b04f1b02af2cf87eaffc534e90935f2f6710d69ee188dc62dcd5f85`
- SplashApp.tsx `83facbaf160d017dc3a88b1d12adcbd9654a9666c94cd0d8d6cbb296b2855fec`
- splash-avatar-path.cjs `1746dbd374eb62811b92733c37edc462078d264e317de549362d9d2846de054b`
- unit `2915459b1a5e48c9c73e9e357053dc33696f8f959d9583e37fcc88f8c1097a3b`
- 原强断言原生宿主 `ebcb30ad358621d3e74ea1974a687204d55f2a0cf4e22712adc53b8a296bdc8d`

未修改Main、作者树、登记、门禁、原断言，没有提交或推送。依赖为自己的node_modules目录逐包只读链接，共享.vite/.vite-temp/.cache未链接；本轮Vite缓存与输出均在独立树。只用合成HOME/TMPDIR/LINGXI_HOME，不读取用户实际资料，也未清理别人的缓存。

## 独立真实检查结果

1. main/preload/splash三个当前生产构建全部PASS，Node语法检查PASS。
2. 新8项helper单测+原preparing4/assets2，**14/14PASS**。
3. 在独立树执行冻结作者原16条强断言宿主，使用已冻结bf8c服务binary，**16场景PASS**。实际窗口截图由审阅者亲读，不作为最终新Main服务版本的替代证据。
4. 用当前Main新构建并独占复制的服务binary再执行独立加强宿主：**20场景PASS**，原16条强断言不变，加下面4项真正边界检查。最终结果 `/tmp/r06-splash-review-current-binary-final/receipt.json`，日志 `/tmp/r06-splash-review-current-binary-final.log`。

当前运行的服务不是旧bf8c binary：实际路径 `/private/tmp/r06-main-dream-20261015/bin/lingxi-service`，SHA256 `35ed7a5f32e65d70f5e426bf34443a2f9b3fce5c3e4f15fc233146f4450c6e76`，119765744字节，与Root binary-proof和本轮receipt记录一致。Root提供的编译来源清单digest为 `1fb667498a0b3568dfc8ee848c1cad68a103ae98bad532b466840a458f5dc781`；清单文件原字节SHA另记为 `8f065cdb800947ce16392d8c18a8a1af6a37eae92594e3de57cceeaa311541aa`，两种坐标不混称。本审阅没有重编无关Rust或把该来源清单当整阶段封印。

当前binary真实HTTP创建alpha/zeta、保存alpha JPEG/zeta WebP/user PNG、切换current=zeta且primary=alpha，实际磁盘字节核对，SIGTERM退出0并证明端口失联后才显示离线Splash。真实Electron42.8.1可见380×280窗口、生产preload.bundle和构建SplashApp，所有计成功图片都complete、naturalWidth>0、body opacity>=0.99；当前WebP实际Chromium解码48×48，sourcePath正确zeta而非alpha。

## 独立加的4项边界：全部真实PASS

- `review-node-existing-primary-behavior`：合成偏好同时保留Rust current=zeta和旧primaryAgent=alpha，切Node模式后真实窗口选择alpha JPEG及butter信息，不误用Rust下划线字段；真实读取不回写偏好，试后外部恢复原偏好。Rust数据保存仍使用当前Rustbinary，未冒充Node服务启动。
- `review-unlisted-local-page-refused`：同一个已知窗口导航到其它本地HTML，实际getAvatarPath/getSplashInfo均null，不能因file:协议就获取路径。
- `review-real-subframe-denied-mainframe-allowed`：额外真实测试窗口仅为子frame加nodeIntegrationInSubFrames以暴露同一生产preload，主frame的实际IPC可读zeta路径，真实iframe自身调用生产桥两方法均null；不是unit假的event.senderFrame。本实验没有改变产品窗口权限。
- `review-webp-link-swap-during-decode-default`：门控实际窗口的createImageBitmap完成窗口，观察生产解码已进入后，外部writer把原文件换成指向外部文件的symlink；释放后继续执行真实createImageBitmap，生产异步复检返回null，真实Splash显示Ming默认。外部文件与恢复后的原头像字节保全。没有替换文件层、IPC返回值或图片解码结果。

额外宿主源码 `/private/tmp/lingxi-r06-review-persona-native/artifacts/rust-tauri/R06/route-a-20261015/splash-avatar-review-boundaries.cjs`，源码SHA与全部精确坐标见JSON。初次额外探针因executeJavaScript最后表达式返回函数，Electron无法克隆函数而中止；这是探针返回值错误，不是产品失败，改为返回true后19项通过，新增Node对照后当前binary20项最终通过；初次失败日志保留，不虚列PASS。

## 截图与保存边界

审阅者对最终当前binary轮亲自读取了5张capturePage截图：zeta自定义WebP（蓝色圆头像）、Node旧alpha JPEG（绿色圆头像）、异步外部链接替换Ming默认（浅蓝鸟）、路径缺失Ming默认（浅蓝鸟）、空资料Lingxi默认（黑鸟）。截图均在 `/tmp/r06-splash-review-current-binary-final/`，不是作者截图或jsdom图像。

原16项还实际覆盖非法role、未知窗口/data origin、错格式、损坏WebP、后缀不匹配、目录、真实FIFO不阻塞、文件链接、祖先链接、chmod000实际EACCES及读取后Chromium加载前移走文件的onError默认。EACCES日志是预期的真实拒绝证据，界面仍按可读当前yuan退回Ming。最后核对原WebP与用户PNG未被头像读取设置或更新。

源码读取链检查使用同一lingxiHome；Rust current_agent_id优先，Node仅primaryAgent。只读普通文件、逐级拒绝home内链接、校验格式和实际解码、WebP等待后重查字节与当前受信页面；授权是已知Splash/Onboarding窗口当前主frame+精确本地页或显式loopback开发页，不向任意路径开放。开发loopback许可本轮unit覆盖，未另启动真实dev server。

## 新正式Rust父接线只读核对

Main新增 `r06_splash_avatar_live.rs::current_rust_saved_offline_splash_real_ipc_and_image_fallback` 已只读核对：每次生产preload/splash构建；使用env! CARGO_BIN_EXE当前候选服务；提供同一原16宿主；其最终读取路径为tests/rust-native/splash-avatar-native.cjs，已只读复核SHA ebcb30ad358621d3e74ea1974a687204d55f2a0cf4e22712adc53b8a296bdc8d，与本审阅执行的原16宿主逐字一致，并将该正式入口纳入源候选摘要；强制macOS不能skip计成功；timeout清理自有进程组；receipt数量16、当前zeta、默认/null、真实取消服务、图片完整性均保留。未在Main写文件或执行父cargo，Root父1/1PASS另有日志 `/private/tmp/r06-main-dream-20261015/splash-native-current.log`，本审阅不把Root结果冒充自身执行。其当前冻结binary已在本审阅20项独立重跑，解决旧binary不可代替当前服务证据的问题。

## 复跑与限制

原16/加强20用真实Electron可执行文件启动对应宿主，参数为独立候选树、全新合成输出目录、冻结服务binary路径；本轮使用Node24.16.0。命令与路径见 `/tmp/r06-splash-avatar-independent-confirmation.json`；日志还有 `/tmp/r06-splash-review-unit.log`、`/tmp/r06-splash-review-build-main.log`、`/tmp/r06-splash-review-build-preload.log`、`/tmp/r06-splash-review-build-splash.log`。

已验证macOS原生Electron集成宿主、实际生产IPC注册函数/preload/SplashApp及当前Rust真实保存→停服→离线读取显示，没有运行完整desktop/bootstrap/main引导、安装版、Tauri窗口、Windows/Linux、正式打包或全R06门禁，不扩大为全部应用启动成功。返回路径到后续Chromium读取不是原子文件系统操作，后续变坏由实际图片onError回退兜底，未声称可冻结用户文件。旧bf8c负控FAIL报告/预期与作者记录保持不变，本审阅没有弱化它以求通过。
