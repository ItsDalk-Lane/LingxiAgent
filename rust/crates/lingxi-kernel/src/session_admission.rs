//! 同一数据根的会话变更准入。只在登记/释放时持互斥锁，绝不跨网络等待。
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionError {
    Busy,
    Full,
}
#[derive(Debug, Default)]
pub struct SessionAdmission {
    owned: Mutex<BTreeSet<String>>,
}
#[derive(Debug)]
pub struct SessionPermit {
    gate: Arc<SessionAdmission>,
    ids: Vec<String>,
}
impl SessionAdmission {
    /// 多会话一次按排序取得全部权限，失败不留下任何部分占用。
    pub fn try_acquire(
        self: &Arc<Self>,
        ids: &[&str],
        cap: usize,
    ) -> Result<SessionPermit, AdmissionError> {
        let ids: Vec<String> = ids
            .iter()
            .map(|s| s.to_string())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let mut owned = self.owned.lock().unwrap_or_else(|p| p.into_inner());
        if ids.iter().any(|id| owned.contains(id)) {
            return Err(AdmissionError::Busy);
        }
        if owned.len().saturating_add(ids.len()) > cap {
            return Err(AdmissionError::Full);
        }
        owned.extend(ids.iter().cloned());
        Ok(SessionPermit {
            gate: Arc::clone(self),
            ids,
        })
    }
    pub fn is_busy(&self, id: &str) -> bool {
        self.owned
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .contains(id)
    }
}
impl SessionPermit {
    /// 绑定实际任务前核对它只继承本会话的操作权。
    /// 必须来自同一真实数据根，不能只凭相同会话字符串继承。
    pub fn belongs_to(&self, gate: &Arc<SessionAdmission>) -> bool {
        Arc::ptr_eq(&self.gate, gate)
    }
    pub fn covers(&self, session_id: &str) -> bool {
        self.ids.iter().any(|id| id == session_id)
    }
}
impl Drop for SessionPermit {
    fn drop(&mut self) {
        let mut owned = self.gate.owned.lock().unwrap_or_else(|p| p.into_inner());
        for id in &self.ids {
            owned.remove(id);
        }
    }
}
