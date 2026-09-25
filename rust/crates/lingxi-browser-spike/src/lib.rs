//! lingxi-browser-spike — R01-T04 浏览器替代原型（受控 Chromium 宿主，CDP over pipe）。
//!
//! 非生产组件（kind=prototype，DEPENDENCY_RULES.json 注册；不拥有任何业务事实）。
//!
//! 设计要点：
//! - 以 `--remote-debugging-pipe`（fd3=命令入，fd4=响应出，NUL 分隔 JSON）驱动
//!   有头 Chromium；不开启任何 TCP 调试端口，调试面不经网络栈暴露。
//! - 每个会话 = 独立 CDP browser context（隔离 storage/cookie），映射现役
//!   Electron per-session partition 语义（desktop/main.cjs `_browserPartitionName`）。
//! - DOM snapshot 脚本在运行时从生产文件 desktop/main.cjs 只读提取
//!   （SNAPSHOT_SCRIPT 模板字符串），保证原型与现役实现使用同一感知算法；
//!   提取结果 sha256 记入证据。
//!
//! 红线遵守：不连真实外网（全部 127.0.0.1）、不用真实账号、不读真实用户数据。

pub mod cdp;
pub mod launcher;
pub mod ops;
pub mod snapshot_source;
