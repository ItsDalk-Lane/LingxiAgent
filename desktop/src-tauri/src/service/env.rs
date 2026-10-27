//! R09-T02 环境快照：启动路径只读一次环境变量，测试注入同一形状。
//!
//! 生产由 `EnvSnapshot::from_process()` 生成（一次性快照，避免解析途中环境被
//! 外部改动）；测试直接构造键值表。所有 env 契约（LINGXI_HOME、服务二进制、
//! 远端 URL/token 文件、降级门）都从这一个快照读取。

use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvSnapshot {
    vars: BTreeMap<String, String>,
}

impl EnvSnapshot {
    pub fn from_process() -> Self {
        let mut vars = BTreeMap::new();
        for (key, value) in std::env::vars() {
            vars.insert(key, value);
        }
        Self { vars }
    }

    pub fn from_pairs<I, K, V>(pairs: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let mut vars = BTreeMap::new();
        for (key, value) in pairs {
            vars.insert(key.into(), value.into());
        }
        Self { vars }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.vars.get(key).map(String::as_str)
    }

    pub fn get_non_empty(&self, key: &str) -> Option<&str> {
        self.get(key).filter(|value| !value.is_empty())
    }

    pub fn to_map(&self) -> &BTreeMap<String, String> {
        &self.vars
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_values_are_treated_as_unset() {
        let env = EnvSnapshot::from_pairs([("LINGXI_HOME", "")]);
        assert_eq!(env.get("LINGXI_HOME"), Some(""));
        assert_eq!(env.get_non_empty("LINGXI_HOME"), None);
    }
}
