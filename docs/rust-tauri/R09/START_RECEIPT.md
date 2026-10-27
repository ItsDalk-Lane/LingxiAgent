# R09 开工回执（2026-10-26，最多 10 行）

1. 分支 `codex/rust-tauri-migration`，HEAD `aac8cb9fe6bb7bfa7a41d75528221136bfe55e50`（=调研基点，无后续提交）；工作区仅 4 个 R08 `SUITES/r08_ui_pages/{live,raw}` 未跟踪缓存，不覆盖。
2. 原账独立核对：8 任务 / 16 基础场景 / 74 去重补充叶 / 168 条原断言；R09-T03 与 R09-T07 共享 46 叶（按叶 ID 去重，不重复计数）——与任务书数字一致。
3. 工具链：rustup `1.98.1` 已装（`~/.cargo/bin`；默认 PATH 的 Homebrew 1.93.0 不读 `rust-toolchain.toml`，所有 Rust 命令一律 `rustup run 1.98.1`）；node v24.16.0；python3 3.14.3；cargo-tauri/@tauri-apps/cli 未装（R09 以最小依赖补装并锁版本）。
4. 平台/真机：仅 macOS 27.0.1 arm64 一台；macOS x64 / Windows x64 / Linux x64 无可用机器 → 平台阻塞 BLOCKED-PLATFORM-*（如实记录，不改写成平台不支持）。Xcode 仅 CLT 27.0（无 Xcode.app），安装包/签名链需实测。
5. 现状：`desktop/src-tauri` 仍是 R06 文件资源宿主（tauri 2.12.1、独立 `[workspace]`、`bundle.active=false`、`frontendDist=../dist-tauri-resources`）；完整 React `DesktopHost-Tauri` 尚不存在；xtask 仅注册 R02–R08；`npm run build:shell` 是 Electron 链，不作 Tauri 证据。
6. 本轮范围：先搭 R09 地图/证据合同（`R09.json`/`R09_INPUTS.json`、xtask `main.rs`/`runner_identity` 注册、源码清单、独立负例），再按 T01→T08 实现取证；本回执只记录开工事实，不代表任何场景通过。
7. 隔离：`LINGXI_HOME`、安装目录、运行目录均隔离（`/private/tmp/lingxi-r09-*`）；不替换日用安装、不迁移真实数据、不发布、不合并 main、不 push。
