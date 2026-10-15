# EX12 两个旧 Node 夹具配套

仅补两 Node 夹具的服务启动前置：声明 synthetic chat 模型和独立授权工作区，通过 --config 交真实 CLI。没有配置 memory/title 等辅助模型，没有改变头像、UI、双连接隔离、失败或无 utility 禁用/零 PUT 断言；没有改任何生产源码、原 Rust 父、门禁、治理、Main 源码或另一作者 Rust 夹具。

原始 Main 两 Rust 父使用当前 Main 服务均真实 FAIL（其原样 Vite 前置会正常重建 Main 的 preload/splash 生成产物，不修改生产源码；修后重建全部在私有窄根）：splash-original-parent-fail.log / owner-original-parent-fail.log。失败点都是原 POST agents/switch 的 200 成功断言得到 500 model plane unavailable。新正确生产链需要真实创建/聚焦会话，旧夹具没有声明 chat，不能放宽生产或把期望改为 500。

本包只补有效模型声明及工作区。模型端点是 localhost:9，仅为创建时的实际可选模型声明，本次头像/设置检查不执行聊天供应商调用，不声称该端点可以生成回复或证明付费调用。原无辅助模型语义仍成立。

修后用原 Rust 父逐字复制源码（SHA 见 manifest）实际 PASS1+PASS1。父没有脚本 override，所以在私有窄源根复用当前 Main 源/依赖，原父只由编译时 CARGO_MANIFEST_DIR 指向该根、CARGO_BIN_EXE_lingxi-service 指向当前 Main 服务；未编辑父代码/断言。用 1.98.1 rustc --test 及已有 Main rlib 依赖重编，没有新 target、没有复制 artifacts/target/node_modules。源根只有必要配置、12MiB 当前 React 源及顶层入口；其他只读引用 Main。真实 preload/splash 构建产物写私有 desktop 输出。

- r06_splash_avatar_live-fixed-parent-r2.log：原 16 条真实 Electron/头像/离线/加载失败/权限/链接/默认图记录及截图全部通过。
- r06_settings_owner_current_source-fixed-parent-r2.log：原 current UI 双真实服务零外来 PUT，A/B 原名称保全；memory 未配置，按钮 disabled=true、零 PUT；当前二进制 SHA 前后不变。
- manifest.json：两源 base/candidate SHA、原父逐字 SHA、当前服务 SHA、真实原始收据路径和环境。
- node-fixtures.patch：只有两 Node 文件，Main 当前逐文件基线 apply --check PASS。两文件 node --check PASS。

早期私有窄根仅用 symlink html 导致 Vite 相对入口拒绝，以及首次 config 临时放空 home 内触发真实 data-epoch 拒绝，均保留于 *-fixed-parent.log；它们是本次私有夹具准备问题，不算生产反例。不修改 epoch 或放宽启动闸，最终 config 移到 home 外，必要源入口逐字复制后按原父重新构建执行。

原始 PASS 收据：
- /private/tmp/r06-ex12-node-fixtures/tmp/r06-splash-native-44840-1792047912566527000/receipt.json
- /private/tmp/r06-ex12-node-fixtures/tmp/r06-settings-owner-44866-1792047917683622000/parent-receipt.json

复跑（按 manifest 中 PATHPrefix 加入现有 PATH，R06_NODE_BIN/TMPDIR/LINGXI_HOME 使用 manifest 合成目录，移除 ELECTRON_RUN_AS_NODE）：
/private/tmp/r06-ex12-node-fixtures/r06_splash_avatar_live --show-output
/private/tmp/r06-ex12-node-fixtures/r06_settings_owner_current_source --show-output
两腿顺序执行，不并发开窗口。Main 集成后应正常以原 cargo --locked 父再跑，它们的 Rust 源完全未变。
