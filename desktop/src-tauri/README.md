# 当前资源桌面入口

这个入口实现 R06 已授权的 11 条桌面文件能力，不包含其他桌面迁移工作。宿主独立构建，服务仍是独立进程。需要先启动当前 Rust 服务，显式配置 workspace，再将服务 home 传给宿主。服务目录中的本机凭证不交给渲染页；监听始终调用 `/lingxi/v1/resource-io` 原入口。

```sh
node node_modules/vite/bin/vite.js build --config desktop/vite.tauri-resources.config.ts
cargo build --manifest-path desktop/src-tauri/Cargo.toml
LINGXI_RESOURCE_SERVICE_HOME=/absolute/service-home desktop/src-tauri/target/debug/lingxi-resource-desktop
```

界面可通过原生文件选择器加入文件，也可由用户明确输入绝对路径查看。原生选取的结果只是路径，不触发读取和写入。查看文件后自动订阅 Rust ResourceIO 的真实事件并重读。文件选择器取消返回空数组。独立窗口的写操作在宿主能力表与执行口都拒绝，读取也只允许其自己的文件。

文本读取上限 5 MiB，base64/Office 字节读取上限 20 MiB；二进制及较大文件显示信息卡。Markdown 与 Office 预览放在无脚本权限的隔离 iframe。DOCX 使用浏览器版 mammoth，XLSX 使用现有 ExcelJS 的实际转换。保存核对实际字节版本；封面变化遇冲突会重新读取，只再试一次。这里保留先读后写的原约定，不提供跨进程原子性的承诺。

## 必需检查

```sh
cargo test --manifest-path desktop/src-tauri/Cargo.toml --test native_resources real_file_read_write_copy_and_version_boundaries -- --exact
cargo test --manifest-path desktop/src-tauri/Cargo.toml --test native_resources edit_allowlist_refuses_system_commands -- --exact
node node_modules/vitest/vitest.mjs run --config desktop/vite.tauri-resources.config.ts desktop/src/tauri-resources/__tests__/resource-desktop.test.tsx
```

真实桌面检查使用可选 `e2e-test`，普通构建不启动 WebDriver。测试的服务 workspace 必须包含新的 `LINGXI_DESKTOP_PROOF_DIR` 合成目录；该目录不能有旧观察回执。设置新的隔离 `TAURI_WEBDRIVER_PORT`。

```sh
LINGXI_RESOURCE_SERVICE_HOME=/absolute/synthetic-home LINGXI_DESKTOP_PROOF_DIR=/absolute/synthetic-workspace/new-proof TAURI_WEBDRIVER_PORT=19465 cargo test --manifest-path desktop/src-tauri/Cargo.toml --features e2e-test --test desktop_host native_desktop_resource_roundtrip_real_gui -- --exact --nocapture
```

原生 UI 检查在 `e2e-phase.json` 和 stdout 写出以下阶段；每阶段最多等待 300 秒，缺观察就失败，不跳过。

- `PICK_FILE`：在真实系统选择器里选择记录的 selected.txt；界面收到真实路径后继续。
- `CANCEL_PICKER`：取消第二次真实选择器；macOS 可用取消按钮或 Cmd+.。
- `OS_OPEN_WINDOW`：确认系统默认程序真正显示 selected.txt，不把打开请求退出 0 当窗口证据。将本轮 CUA/实际截图写入绝对截图文件，并保存 `os-open-observation.json`：`{"path":"记录中的绝对路径","observedWindow":true,"program":"真实程序名","windowTitle":"真实标题","screenshot":"本轮截图绝对路径"}`。自动化只消费外部实际观察，不生成通过替身。
- `VERIFY_SYSTEM_TRASH`：核对原文件不存在且系统废纸篓里有实际载荷，保存 `system-trash-observation.json`：`{"source":"阶段记录中的原绝对路径","trashPath":"真实系统废纸篓载荷的绝对路径"}`。检查会读取 Trash 文件字节并比对，不允许永久删除替代。

该 GUI 检查还真正验证 ResourceIO `resource.changed` 刷新、旧监听 `file-changed`、主窗口保存失败可见、独立只读窗口的宿主拒写、复制/base64 的磁盘字节和 macOS 剪切/复制/粘贴/全选。外部观察必须连同原始截图和候选源码一起登记，测试回执单独不能代替人的实际窗口观察。
