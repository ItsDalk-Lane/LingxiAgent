//! 第三方插件生态已退役：数据目录里的旧用户插件不再加载。只记录告知日志，
//! 不删除用户数据——目录内容的处置权留给用户（对齐旧引擎 core/engine.ts 的
//! 兼容提示行为）。

use std::path::Path;

/// 旧用户插件目录相对数据根的位置（旧引擎：`path.join(lingxiHome, "plugins")`）。
const LEGACY_PLUGINS_DIRNAME: &str = "plugins";

/// 探测旧用户插件目录下残留的插件名（子目录、忽略点开头的隐藏项）。
/// 目录不存在或探测失败一律返回空——提示性探测绝不能影响启动。
pub fn legacy_user_plugin_names(home: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(home.join(LEGACY_PLUGINS_DIRNAME)) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| !name.starts_with('.'))
        .collect();
    names.sort();
    names
}

/// 启动时调用一次：发现残留即写一条结构化告知日志；返回探测到的名字供
/// 调用方（或测试）核对。不删除、不加载、不阻塞。
pub fn warn_if_legacy_plugins(home: &Path) -> Vec<String> {
    let names = legacy_user_plugin_names(home);
    if !names.is_empty() {
        tracing::warn!(
            count = names.len(),
            plugins = ?names,
            "[plugins] legacy user plugins under the data home are no longer loaded \
             (third-party plugin ecosystem retired)"
        );
    }
    names
}
