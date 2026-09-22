# 最终候选打包验证（C29）

结论：本机新包的可执行验证通过，完整 C29 仍为 BLOCKED。不能据此宣布正式安装验收完成。

## 当前源码与产物

本轮最终候选于 2026-09-22 10:42–10:43 UTC 构建。当前 2,000 个生产源码、配置与构建元数据文件逐字节比较无差异（source-comparison.json）。HEAD 是审查提交加当前未提交修复，不能只用 HEAD 表示候选。产物 SHA256 见 candidate-hashes.json。

client、server、seed 校验与 electron-builder --dir 原始退出码均为 0；commands.jsonl 保留实际命令和时间。只生成 macOS arm64 本地 .app，使用临时 Ed25519 验签材料，未发布、未安装到 Applications、未公证；临时密钥已删。已有本地 speech/computer-use helper 与缓存 Node 被复用，不声称本轮重新编译这些原生 helper。

## 新产物实际执行

- 从新 .app 提取的 server seed 使用自带 Node v24.15.0，7/7 检查通过：启动、鉴权、建会话、真实 WS/read 工具往返、历史回读、技能列表、优雅退出。协议供应商为本机确定性桩，无真实付费模型。
- 新 seed 的 node-pty 真实执行 shell，输出 LINGXI_PACKAGED_PTY_OK，退出 0。
- 新 .app 真实启动，实际渲染新 seed 页面，建立隔离会话并完成流式回复。
- 新 .app 的内嵌浏览器实际打开本地页面并显示 PACKAGED_BROWSER_OK；证据 packaged-browser-ax.txt/png。
- 新 .app 的对话文件预览实际显示 1 页 PDF 与 Hello from PDF / Second line of text；证据 packaged-pdf-ax.txt/png。
- 终端 GUI 已实际请求 exec_command，但产品自动审核缺少辅助模型，日志明确 reviewer_not_configured，动作未执行。该项 BLOCKED；未改权限或绕过审核。原生 PTY 通过不代替终端 GUI 通过。

旧 before-final-helper-fix GUI 证据没有当作最终候选通过证据，因为最终有两处 renderer 源码变化。本节均为最终 .app 重跑。

## 失败、隔离与限制

最初 seed 冒烟在删除 HOME 覆盖时误删 LINGXI_HOME，既有内核所有权检查拒绝启动；失败日志 *.isolation-launch-failed 与退出 1 完整保留。修正隔离参数后重新执行才得到 7/7，通过没有覆盖失败。

GUI 使用随机 LINGXI_HOME 和实际 lsof 证实的独立 Electron userData。HOME 没有修改，因此首次会话显示已有默认 OH-WorkSpace；该会话执行工具被审核阻止，没有工作区写入工具运行。PDF 明确来自本次临时目录，副本进入隔离 session-files。为安全重放，启动脚本随后增加 desk.home_folder 指向临时工作区，此脚本设置未作为已经运行的证明。

新包进程和本地供应商已停止。只删除本次随机临时数据目录及独立 userData，删除前 lsof 无打开句柄；清理见 cleanup.json。保留用户原有应用进程、Application Support/Data 与已有业务文件。构建候选留在临时目录便于复核。

正式签名/公证安装包、外部干净系统安装、Windows/Linux/macOS x64、真实供应商和安装级更新回退没有当前环境或授权，保持 BLOCKED。此前源码级回退证据不替代安装级验证。
