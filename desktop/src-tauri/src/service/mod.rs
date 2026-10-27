//! R09-T02 服务 supervisor 模块（Tauri 宿主侧）。
//!
//! 布局（设计 §4.1 的落地拆分，语义/常量/命令/事件不变）：
//! - `env`        环境快照（启动路径只读一次 env）
//! - `resolve`    数据根、dev/packaged 二进制、启动前互斥门
//! - `manifest`   构建清单 + 协议门（lingxi-protocol 常量）
//! - `digest`     文件 sha / 签名后内容摘要（Mach-O/PE）
//! - `instance`   instance.json/local-token.json 私读与运行时协议门
//! - `client`     identity/health HTTP 校验
//! - `remote`     远端 env 契约（dev-only）
//! - `shutdown`   SIGTERM→17s→SIGKILL→5s 与残留报告
//! - `supervisor` 状态机：spawn→READY→identity→publish→监控/有限重启→退出
//! - `diagnostics` `{home}/diagnostics/tauri-launch/events.jsonl` + tauri-crash.log
//! - `error`     稳定错误码 + expected/actual（A03 断言面）

pub mod client;
pub mod diagnostics;
pub mod digest;
pub mod env;
pub mod error;
pub mod instance;
pub mod manifest;
pub mod remote;
pub mod resolve;
pub mod shutdown;
pub mod supervisor;

pub use diagnostics::Diagnostics;
pub use error::{ServiceResult, StartError};
pub use supervisor::{
    ConnectionInfoView, Lifecycle, ServerConnection, ServiceMode, ServiceRuntime, SupervisorConfig,
    Timings,
};
