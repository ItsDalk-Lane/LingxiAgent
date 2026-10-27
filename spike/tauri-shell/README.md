# R01-T06 Tauri v2 桌面宿主隔离原型（lingxi-tauri-shell-spike）

隔离原型，**不接入生产**。目的：按任务书 R01-T06 验证 Tauri v2 作为 Electron 替代宿主的
系统能力、capability 授权边界（R01-A11）、sidecar 生命周期、测试能力隔离（R01-A12）。

## 目录

- `app/` — Tauri v2 工程（src-tauri 独立 manifest，**不属于** `rust/` headless workspace）
- `app/frontend/` — 静态前端（无打包器）：`index.html`(可信 main) / `untrusted.html`(打包但无授权)
- `sidecar/` — Rust sidecar 原型（固定二进制、受限参数、行式 JSON 握手协议）
- `scripts/` — 可重放 E2E 编排（loopback 证据服务器、WebDriver 探针、TCC 查询探针）

## 前置

- rustup 管理的 toolchain（仓库根 `rust-toolchain.toml` 固定 1.98.1）；`~/.cargo/bin` 在 PATH 前。
- Node >= 24（仅用于证据服务器与 tauri CLI；原型本体不依赖 Node）。
- 代理已死环境：所有联网命令前缀
  `env -u all_proxy -u ALL_PROXY -u http_proxy -u HTTP_PROXY -u https_proxy -u HTTPS_PROXY`。
- 一切测试流量限 loopback（证据服务器 127.0.0.1，远端页 = 本机 loopback HTTP 源）。

## 重放

```bash
cd spike/tauri-shell
# 0) 一次性安装 tauri CLI（spike 本地 devDependency，不动仓库根 package.json）
cd app && npm install && cd ..
# 1) 全流程（构建 sidecar + test/release 双产物 + E2E + 探测），证据写入指定目录
scripts/run_e2e.sh /path/to/artifacts/rust-tauri/R01/T06
```

单独步骤见 `scripts/run_e2e.sh` 注释。生产目录零改动；`git status --porcelain` 可证。

updater 测试密钥：首次重放自动生成到 `/tmp/lingxi-t06-keys/`（一次性 throwaway key +
口令文件 `.pass`，均只存 /tmp，不入库）；若密钥被重新生成，必须把新 pubkey
（`/tmp/lingxi-t06-keys/t06-updater-test.key.pub`）同步进
`app/src-tauri/tauri.conf.json` 的 `plugins.updater.pubkey` 再重跑。
打包冒烟会把 .app 复制到 realpath 目录运行（macOS /tmp→/private/tmp 符号链接会被
Tauri 的 current_exe() 检查拒绝）。
