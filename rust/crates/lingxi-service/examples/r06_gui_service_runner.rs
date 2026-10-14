//! R06 GUI 编排支撑（非产品入口）：为原生桌面真实窗口证明启动一个
//! 真实 lingxi-service 实例，把 `instance.json` / `local-token.json`
//! 发布进指定 home，供 lingxi-resource-desktop 通过
//! LINGXI_RESOURCE_SERVICE_HOME 发现并连接。
//!
//! 仅测试编排使用：不承载业务事实，不进入任何生产入口；进程退出即
//! 释放单写锁，home 由编排方按轮次新建，不复用旧记录。

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lingxi_service::instance;
use lingxi_service::{
    prepare_layout, run, HomeSource, NetworkMode, ServiceConfig, ServiceDeps, ServiceState,
    DEFAULT_SHUTDOWN_TIMEOUT_MS,
};

fn main() {
    let mut args = std::env::args().skip(1);
    let home = PathBuf::from(
        args.next()
            .expect("用法: r06_gui_service_runner <绝对home目录> [--workspace <绝对目录>] [--idle-seconds <秒>]"),
    );
    assert!(home.is_absolute(), "home 必须为绝对路径");
    let mut workspace: Option<PathBuf> = None;
    let mut idle_secs = 3600u64;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--workspace" => {
                let dir = PathBuf::from(args.next().expect("--workspace 需要目录"));
                assert!(dir.is_absolute(), "--workspace 必须为绝对路径");
                workspace = Some(dir.canonicalize().unwrap_or(dir));
            }
            "--idle-seconds" => {
                idle_secs = args
                    .next()
                    .expect("--idle-seconds 需要数值")
                    .parse()
                    .expect("合法秒数");
            }
            other => panic!("未知参数：{other}"),
        }
    }

    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    runtime.block_on(async move {
        let layout = prepare_layout(&home).expect("准备 home 布局");
        let (guard, existing) = instance::acquire(&layout).expect("获取单写锁");
        if existing.is_some() {
            panic!("home 已存在实例记录，拒绝静默顶替；请为本轮换新 home");
        }
        let config = ServiceConfig {
            bind_addr: "127.0.0.1:0".parse().expect("loopback 地址"),
            data_home: home.clone(),
            home_source: HomeSource::Cli,
            network_mode: NetworkMode::Loopback,
            shutdown_timeout_ms: DEFAULT_SHUTDOWN_TIMEOUT_MS,
        };
        let deps = ServiceDeps {
            workspace_root: workspace,
            ..ServiceDeps::default()
        };
        // 引导必须沿用锁内身份（bootstrap_with_locked_instance），
        // 令 instance.json 与 local-token.json 指向同一次启动。
        let state = ServiceState::bootstrap_with_locked_instance(config, &layout, deps, &guard)
            .await
            .expect("bootstrap");
        // 单写锁必须存活到进程退出：publish 后放回共享槽，不得提前释放。
        let slot: Arc<Mutex<Option<instance::InstanceGuard>>> = Arc::new(Mutex::new(Some(guard)));
        let publisher = slot.clone();

        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let serve = tokio::spawn(async move {
            run(
                state,
                async {
                    let _ = stop_rx.await;
                },
                move |addr: SocketAddr| {
                    let mut slot = publisher.lock().expect("guard 槽可用");
                    let mut guard = slot.take().expect("guard 仍在槽内");
                    guard
                        .publish_with_transport(addr, "http")
                        .expect("发布 instance 记录");
                    *slot = Some(guard);
                    println!("READY {addr}");
                },
                None,
            )
            .await
        });

        // Ctrl-C 或空闲上限先到者触发优雅停机；编排方也可直接结束进程。
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = tokio::time::sleep(Duration::from_secs(idle_secs)) => {}
        }
        let _ = stop_tx.send(());
        let outcome = serve.await.expect("join serve").expect("serve 结果");
        eprintln!("EXIT outcome={outcome:?} home={}", home.display());
    });
}
