//! R08-T05：迁移协调器（cutover）模块。
//!
//! 设计锚：docs/rust-tauri/R08/T05_DESIGN.md（Lead 已批）。
//! - `journal`：10 相切换状态机，落盘 `{work-dir}/journal.json`，phase 是
//!   恢复的唯一权威（mtime 永不参与裁决）；atomic_write_private 链落盘。
//! - `probe`：双锁探针（旧栈 server-info 四态 + 新栈 OS 锁非变异探针）。
//! - `preflight`：六项 fail-closed 预检，逐项取证落报告。
//! - `stillpoint`：69 类全集枚举 + 双遍清单静止窗口。
//! - `backup`：切割备份（copy-set→scratch→Online Backup），T06 契约备齐。
//! - `pointer`：原子指针切换（常规 JSON 文件，rename 原子语义，拒 symlink）。
//! - `selfcheck`：自检协调器驱动（§9.1/§9.2）——拉起 `--self-check` 进程、
//!   解析 SELF_CHECK_LISTEN 行、只读计数探针对拍采用回执期望、业务面响亮
//!   关闭抽样、SIGTERM 退出与锁释放探针。
//!
//! 编排入口在 Phase E 接入 main.rs `migrate` 动词；本层全部可单测。

pub mod backup;
pub mod journal;
pub mod pointer;
pub mod preflight;
pub mod probe;
pub mod selfcheck;
pub mod stillpoint;
