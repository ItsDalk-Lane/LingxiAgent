# C29 本机新候选产物验证

结论：C29 总项 **BLOCKED**；下列本机 macOS arm64 子项已实测通过，不替代正式安装、签名公证、跨平台或真实供应商验收。

## 候选与构建

- 最终生产代码冻结后于 2026-09-22 10:17:47Z 起重新复制源码、构建客户端/服务端、验 seed、electron-builder --dir。四步均退出 0。原始日志、时间、命令和退出码在 commands.jsonl；Node v24.16.0，产物自带 Node v24.15.0。
- 工作区基线 HEAD b0e9118427e16206dcd25ecfb566d766ed476727，未提交修复由 source-snapshot.json 的逐文件 SHA-256 绑定；10:22 再比对所列生产目录无差异。包内 sourceCommit 使用已有 LINGXI_SOURCE_COMMIT 参数传基线，**不能把基线 HEAD 当作包含未提交修复的提交**。
- build-context.json 包含源锁文件 hash、脏树和临时目录。主端 10:19 重新生成两份 build 元数据，晚于本次复制；post-build-metadata-diff.json 明列它们与构建快照不同，不宣称最终整树完全相同。生产目录 hash 相同。
- 独立临时构建树只在编译阶段链接现有锁定 node_modules；产物和提取 server seed 自带运行依赖，执行 cwd、Node 和页面 URL 均指向临时产物。未依赖源码目录启动。
- 仅本地临时 ed25519 keyset、临时签名和 ad-hoc codesign；SKIP_NOTARIZE=true、CSC_IDENTITY_AUTO_DISCOVERY=false。临时私钥/公钥文件均在 finally 删除。没有正式证书、发布或 /Applications 安装。
- 沿用此前构建的 mac arm64 computer-use/speech helper 和 Node 下载缓存，未重编译这些未修改 helper；这一项不作为新 helper 构建验证。
- intermediate-snapshot/ 保留先前中间快照，以及第一次无 .git 导致 metadata 失败的 exit 1 日志；中间结果不冒充最终候选结果。

## 本轮实际通过

| 子项 | 原始证据 | 说明 |
|---|---|---|
| 最新包构建/验 seed | build-client.log、build-server.log、verify-seed.log、electron-builder.log | 均 exit 0 |
| 独立 server 启动、鉴权、会话、WS read 工具真实执行、历史回读、技能、退出 | packaged-smoke.json、packaged-smoke.log、packaged-smoke.json.server.log | 7 PASS/0 FAIL；本地协议 witness，无真实模型费用 |
| 包内 Node/PTY 原生模块真实 shell | packaged-pty.log | 自带 Node 启动 node-pty，shell 输出 LINGXI_PACKAGED_PTY_OK，exit 0；不等于终端 GUI 面板验收 |
| 最新实际 .app 启动 + 新会话 | packaged-launch.json、packaged-electron.log、packaged-diagnostics/ | main.bundle.cjs、packaged:true，页面来自隔离数据目录 artifacts/renderer；会话完成 8 行本地输出 |
| PDF 实际界面预览 | packaged-pdf.png、packaged-pdf-ax.txt | 包内 Chromium PDF viewer 展示 fixture 的 Hello from PDF / Second line of text，1 页加载完毕 |
| 内置浏览器实际打开 | packaged-browser.png、packaged-browser-ax.txt、packaged-final-ax.txt | 点击会话中的 localhost 链接，包内 browser-viewer 展示 PACKAGED_BROWSER_OK；关闭按钮一次返回主界面，但后续 Browser 窗口仍可见，未将其算作完整销毁/无泄漏验证 |

## 隔离事实与限制

HOME 和业务 LINGXI_HOME 指向随机临时目录。macOS 的 Electron appData 不随 HOME 参数迁移：最初两个验证进程均用末级 data，触发同名 userData 的实例锁碰撞。该次 exit 0 不代表启动通过。原记录在 intermediate-snapshot/launch-initial/，且 f05-gui/GUI_REPORT.md 已更正早先“所有缓存隔离”说法。

最终候选使用随机唯一末级名；actual-user-data.log 的 lsof 确认缓存位于本轮独有的 Library/Application Support/Lingxi-f05-packaged-run-aZ3UYg。既有 Data 目录来源不完整，未删除。此事不涉及真实业务数据目录。

## 仍阻塞

- 正式签名、公证、安装器/拖入 Applications 后首次启动：本轮禁止发布且未执行正式安装，BLOCKED。
- Windows/Linux/macOS x64 及干净外部机器：只有本机 macOS arm64 环境，BLOCKED。
- 终端 GUI 卡片、真实供应商/模型、自动更新/回滚：当前验证仅包内 PTY 和本地 witness，不覆盖这些子项，BLOCKED。
- 不将 7 项局部协议检查与 PDF/browser 截图组合解释为九阶段全完成。
