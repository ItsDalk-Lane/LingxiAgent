# R10 Chromium 分发决策（记录，不实现）

状态：**DEFERRED / 沿用系统 Chrome**

## 决策

R10 不把 Chromium 打包进 Tauri 发行物；继续使用运行时路径发现：

1. `LINGXI_CHROMIUM_PATH` 显式提供（必须是存在的绝对可执行文件）；
2. 否则按服务内建候选路径查找（安装资源目录 / `/Applications/Google Chrome.app` / 常见 Linux 路径）；
3. 都没有时显式 `UnavailableBrowser`，不静默降级、不伪造 PDF 能力。

## 理由

- OWNER-02/03：当前只做 macOS Apple 平台、不做正式签名；随包 Chromium 带来体积、嵌套签名、公证与更新链复杂度。
- R09 T05 已证明系统 Chrome 路径在受控环境可用；PDF 运行时失败是 CDP `Not attached to an active page` 已知抖动，不是路径缺失。
- `bundle.resources` 随包方案（ADR-002/003 推荐形态）保留到发布通道/许可/签名裁决后再做。

## 后续触发条件

- 要求“安装即工作、无系统 Chrome”；
- 正式签名/公证凭据就位；
- 发布产物需要纳入 Chromium 版本锁定、许可与更新一致性。

届时再实现 `tauri.release.conf.json` 资源目录 + 服务注入 `LINGXI_CHROMIUM_PATH`，并重跑 T05 运行期证据（本轮不做）。
