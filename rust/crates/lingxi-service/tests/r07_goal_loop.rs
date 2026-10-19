//! R07-T07-δ 目标与循环的真实服务集成入口；用例只替模型面（确定性替身），
//! 服务组合根、Run、工具网关、slash、落盘与事件总线均为真实实现。
#[path = "r07_goal_loop/mod.rs"]
mod goal_loop;
