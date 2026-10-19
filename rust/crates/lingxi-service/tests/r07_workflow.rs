//! R07-T07-δ workflow 引擎的真实服务集成入口；用例只替模型面（确定性替身），
//! 组合根、工具网关、Run、子 run 派发、journal 落盘与事件总线均为真实实现。
#[path = "r07_workflow/mod.rs"]
mod workflow;
