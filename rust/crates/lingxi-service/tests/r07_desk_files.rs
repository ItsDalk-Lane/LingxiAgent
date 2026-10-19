//! R07-T07 β3：desk 工作台文件面（现役 server/routes/desk.ts 的
//! path/files/search-files/jian 六路由 + mount-aware-file-service.ts 的
//! readText/safeDeleteIfExists/copyLocalPathIntoDirectory + 批准目录门禁）
//! 真实 loopback 集成测试。
#![allow(clippy::too_many_lines)]

#[path = "r07_desk_files/mod.rs"]
mod r07_desk_files;
