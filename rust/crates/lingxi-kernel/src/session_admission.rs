//! 同一数据根的会话变更准入。只在登记/释放时持互斥锁，绝不跨网络等待。
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, Weak};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionError {
    Busy,
    Full,
}
type ReleaseCallback = Arc<dyn Fn(&[String]) + Send + Sync>;
struct ReleaseListener {
    token: Arc<()>,
    callback: ReleaseCallback,
}

#[derive(Default)]
pub struct SessionAdmission {
    owned: Mutex<BTreeSet<String>>,
    listener: Mutex<Option<ReleaseListener>>,
}
impl std::fmt::Debug for SessionAdmission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionAdmission")
            .field("owned", &self.owned)
            .finish_non_exhaustive()
    }
}

/// 注册者持有此句柄；释放后允许同一数据根的新服务重新绑定监听。
pub struct ReleaseListenerRegistration {
    gate: Weak<SessionAdmission>,
    token: Arc<()>,
}
impl Drop for ReleaseListenerRegistration {
    fn drop(&mut self) {
        if let Some(gate) = self.gate.upgrade() {
            let mut slot = gate.listener.lock().unwrap_or_else(|p| p.into_inner());
            if slot
                .as_ref()
                .is_some_and(|listener| Arc::ptr_eq(&listener.token, &self.token))
            {
                *slot = None;
            }
        }
    }
}
#[derive(Debug)]
pub struct SessionPermit {
    gate: Arc<SessionAdmission>,
    ids: Vec<String>,
}
impl SessionAdmission {
    /// 每个数据根仅一个监听；回调始终在占用和监听锁外调用，不依赖异步运行时。
    pub fn register_release_listener(
        self: &Arc<Self>,
        callback: ReleaseCallback,
    ) -> Result<ReleaseListenerRegistration, &'static str> {
        let mut slot = self.listener.lock().unwrap_or_else(|p| p.into_inner());
        if slot.is_some() {
            return Err("session_release_listener_already_registered");
        }
        let token = Arc::new(());
        *slot = Some(ReleaseListener {
            token: Arc::clone(&token),
            callback,
        });
        Ok(ReleaseListenerRegistration {
            gate: Arc::downgrade(self),
            token,
        })
    }
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
        {
            let mut owned = self.gate.owned.lock().unwrap_or_else(|p| p.into_inner());
            for id in &self.ids {
                owned.remove(id);
            }
        }
        let callback = self
            .gate
            .listener
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .map(|listener| Arc::clone(&listener.callback));
        if let Some(callback) = callback {
            callback(&self.ids);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn final_shared_permit_release_notifies_after_unlock_and_listener_is_unique() {
        let gate = Arc::new(SessionAdmission::default());
        let callbacks = Arc::new(AtomicUsize::new(0));
        let weak = Arc::downgrade(&gate);
        let observed = Arc::clone(&callbacks);
        let registration = gate
            .register_release_listener(Arc::new(move |ids| {
                let gate = weak.upgrade().expect("gate");
                assert_eq!(ids, &["a", "b"]);
                assert!(!gate.is_busy("a"));
                assert!(!gate.is_busy("b"));
                // 同时验证占用锁、监听锁均已释放。
                assert!(gate.listener.try_lock().is_ok());
                observed.fetch_add(1, Ordering::SeqCst);
            }))
            .expect("register");
        assert!(gate.register_release_listener(Arc::new(|_| {})).is_err());
        let permit = Arc::new(gate.try_acquire(&["b", "a", "a"], 2).expect("permit"));
        let child = Arc::clone(&permit);
        drop(permit);
        assert!(gate.is_busy("a"));
        assert_eq!(callbacks.load(Ordering::SeqCst), 0);
        std::thread::spawn(move || drop(child))
            .join()
            .expect("drop");
        assert_eq!(callbacks.load(Ordering::SeqCst), 1);
        drop(registration);
        let _next = gate
            .register_release_listener(Arc::new(|_| {}))
            .expect("rebind");
    }

    #[test]
    fn default_gate_and_unregistered_listener_keep_release_behavior() {
        let gate = Arc::new(SessionAdmission::default());
        drop(gate.try_acquire(&["a"], 1).expect("permit"));
        assert!(!gate.is_busy("a"));
        let registration = gate
            .register_release_listener(Arc::new(|_| panic!("已注销监听不得调用")))
            .expect("register");
        drop(registration);
        drop(gate.try_acquire(&["a"], 1).expect("permit"));
        assert!(!gate.is_busy("a"));
    }

    #[test]
    fn concurrent_final_releases_each_notify_once_without_holding_admission_lock() {
        let gate = Arc::new(SessionAdmission::default());
        let observed = Arc::new(Mutex::new(Vec::new()));
        let weak = Arc::downgrade(&gate);
        let recorded = Arc::clone(&observed);
        let _registration = gate
            .register_release_listener(Arc::new(move |ids| {
                let gate = weak.upgrade().expect("gate");
                for id in ids {
                    assert!(!gate.is_busy(id));
                }
                recorded.lock().expect("observed").extend_from_slice(ids);
            }))
            .expect("listener");
        let first = gate.try_acquire(&["a"], 2).expect("a");
        let second = gate.try_acquire(&["b"], 2).expect("b");
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let a = Arc::clone(&barrier);
        let first = std::thread::spawn(move || {
            a.wait();
            drop(first);
        });
        let b = Arc::clone(&barrier);
        let second = std::thread::spawn(move || {
            b.wait();
            drop(second);
        });
        barrier.wait();
        first.join().expect("a released");
        second.join().expect("b released");
        let mut actual = observed.lock().expect("observed").clone();
        actual.sort();
        assert_eq!(actual, ["a", "b"]);
        assert!(!gate.is_busy("a"));
        assert!(!gate.is_busy("b"));
    }
}
