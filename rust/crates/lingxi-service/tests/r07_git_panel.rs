//! R07-T07 γ：git 面板（现役 server/routes/git-environment.ts 22 路由 +
//! server/git/git-command.ts 执行底座/解析器/操作族 + utility:call-text
//! summarize 槽）真实 loopback 集成测试：真实 git 二进制对临时仓执行，
//! 远程 = 本地 bare 仓（无网络），模型面 = 确定性 TCP 替身。
#![allow(clippy::too_many_lines)]

#[path = "r07_git_panel/mod.rs"]
mod r07_git_panel;
