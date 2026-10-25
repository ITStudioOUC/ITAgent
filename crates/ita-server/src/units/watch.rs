//! 能力变更推送：进程内 watch 通道。
//!
//! watch 通道是进程内的，不是 IPC：持有 `Store` 句柄，变更时读 `units` 表重建能力集，
//! 版本号递增一次，向全部订阅者推送。订阅时立即推送完整能力集（版本 0），每次
//! `notify_change` 版本号递增一次，推送承载完整能力集而不是真 delta。
//!
//! 通道是通用的"能力变更推送"：能力集变更是其中一种消息，工具目录追加以后会在同一通道
//! 上加为另一种消息，机制相同。版本号是进程内单调递增计数器，不落库：server 重启后版本
//! 归零，但首次推送仍是完整能力集，语义不变。

use ita_store::{Store, StoreError};

use super::capability::{CapabilitySet, build_capability_set};

/// 订阅者回调：接收一条能力变更消息。
type Subscriber = Box<dyn FnMut(&CapabilityUpdate)>;

/// 能力变更消息：watch 通道推送的统一形状。
///
/// 通道是通用的"能力变更推送"，能力集变更是其中一种消息。工具目录追加以后会在同一通道
/// 上加为另一种消息，机制相同。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityUpdate {
    /// 能力集变更：承载完整能力集与版本号。
    CapabilitySet(CapabilitySet),
}

/// 能力 watch：持有 `Store` 句柄、当前能力集与订阅者列表。
///
/// 能力集读 `units` 表建，不读内存状态。首次推送（订阅时）是完整能力集（版本 0），每次
/// `notify_change` 版本号递增一次。版本号是进程内单调递增计数器，不落库。
pub struct CapabilityWatcher {
    store: Store,
    set: CapabilitySet,
    subscribers: Vec<Subscriber>,
}

impl CapabilityWatcher {
    /// 建 watcher：读 `units` 表建初始能力集（版本 0）。
    ///
    /// # 步骤
    /// 1. 读 `units` 表的全部 unit 状态。
    /// 2. 投影成能力集，版本号 0。
    ///
    /// # 边界与前提
    /// - 空表得到空能力集（两个列表都空），不报错。
    ///
    /// # 副作用
    /// 只读。
    pub fn new(store: Store) -> Result<Self, StoreError> {
        let records = store.read_all_unit_states()?;
        let set = build_capability_set(&records, 0);
        Ok(Self { store, set, subscribers: Vec::new() })
    }

    /// 注册订阅者并立即推送当前完整能力集（版本 0）。
    ///
    /// # 步骤
    /// 1. 向新订阅者立即推送当前能力集。
    /// 2. 把回调登记进订阅者列表。
    ///
    /// # 边界与前提
    /// - 首次推送是完整能力集，不是增量变更。
    ///
    /// # 副作用
    /// 订阅者列表加一项，回调被调用一次。
    pub fn subscribe<F>(&mut self, mut callback: F)
    where
        F: FnMut(&CapabilityUpdate) + 'static,
    {
        let update = CapabilityUpdate::CapabilitySet(self.set.clone());
        callback(&update);
        self.subscribers.push(Box::new(callback));
    }

    /// 读 `units` 表重建能力集，版本号递增，向全部订阅者推送。
    ///
    /// # 步骤
    /// 1. 读 `units` 表的全部 unit 状态。
    /// 2. 版本号递增一次，投影成新的能力集。
    /// 3. 向全部订阅者推送新的能力集。
    ///
    /// # 边界与前提
    /// - 版本号严格递增：不回退、不重复。
    /// - 推送承载完整能力集，不是真 delta。
    ///
    /// # 副作用
    /// 当前能力集被替换，全部订阅者回调被调用。
    pub fn notify_change(&mut self) -> Result<CapabilitySet, StoreError> {
        let records = self.store.read_all_unit_states()?;
        let set = build_capability_set(&records, self.set.version + 1);
        let update = CapabilityUpdate::CapabilitySet(set.clone());
        for subscriber in &mut self.subscribers {
            subscriber(&update);
        }
        self.set = set.clone();
        Ok(set)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    use crate::units::UnitId;
    use ita_store::UnitState;

    fn id(label: &str) -> UnitId {
        UnitId::new(label).expect("测试标签应当合法")
    }

    fn seed(store: &mut Store, label: &str, state: UnitState) {
        store.set_unit_state(label, state).expect("写入应当成功");
    }

    /// 订阅者先收完整能力集：第一条消息是完整能力集，版本 0。
    #[test]
    fn subscribe_pushes_full_capability_set_at_version_zero() {
        let mut store = Store::open_in_memory().expect("应当可以打开");
        seed(&mut store, "store", UnitState::Ready);
        seed(&mut store, "model", UnitState::Degraded);
        let mut watcher = CapabilityWatcher::new(store).expect("应当可以构造");
        let log = Rc::new(RefCell::new(Vec::new()));
        let value = log.clone();
        watcher.subscribe(move |update| value.borrow_mut().push(update.clone()));
        assert_eq!(log.borrow().len(), 1, "订阅应当推送一次");
        match &log.borrow()[0] {
            CapabilityUpdate::CapabilitySet(set) => {
                assert_eq!(set.version, 0, "首次推送应当是版本 0");
                assert_eq!(set.ready, vec![id("store")], "就绪列表应当是 [store]");
                assert_eq!(set.degraded, vec![id("model")], "降级列表应当是 [model]");
            }
        }
    }

    /// 降级后收带版本号的增量：改 unit 状态后 notify_change 推送版本号递增的完整能力集。
    #[test]
    fn notify_change_pushes_version_incremented_set() {
        let mut store = Store::open_in_memory().expect("应当可以打开");
        seed(&mut store, "store", UnitState::Ready);
        seed(&mut store, "model", UnitState::Degraded);
        let mut watcher = CapabilityWatcher::new(store).expect("应当可以构造");
        let log = Rc::new(RefCell::new(Vec::new()));
        let value = log.clone();
        watcher.subscribe(move |update| value.borrow_mut().push(update.clone()));
        // model 从 degraded 改成 ready，能力集应当反映表的新状态。
        watcher.store.set_unit_state("model", UnitState::Ready).expect("写入应当成功");
        let set = watcher.notify_change().expect("应当成功");
        assert_eq!(log.borrow().len(), 2, "应当收到两条推送");
        match &log.borrow()[1] {
            CapabilityUpdate::CapabilitySet(second) => {
                assert_eq!(second.version, 1, "第二条推送应当是版本 1");
                assert_eq!(
                    second.ready,
                    vec![id("model"), id("store")],
                    "就绪列表应当是 [model, store]"
                );
                assert!(second.degraded.is_empty(), "降级列表应当为空");
            }
        }
        assert_eq!(set.version, 1, "返回的能力集应当是版本 1");
    }

    /// 版本号单调递增：连续 notify_change 版本号从 0 增到 1 再增到 2。
    #[test]
    fn version_is_monotonically_increasing() {
        let store = Store::open_in_memory().expect("应当可以打开");
        let mut watcher = CapabilityWatcher::new(store).expect("应当可以构造");
        let log = Rc::new(RefCell::new(Vec::new()));
        let value = log.clone();
        watcher.subscribe(move |update| value.borrow_mut().push(update.clone()));
        watcher.notify_change().expect("应当成功");
        watcher.notify_change().expect("应当成功");
        let versions: Vec<u64> = log
            .borrow()
            .iter()
            .map(|update| match update {
                CapabilityUpdate::CapabilitySet(set) => set.version,
            })
            .collect();
        assert_eq!(versions, vec![0, 1, 2], "版本号应当 0、1、2 严格递增");
    }

    /// 能力集读 units 表建：改表后 notify_change 反映表的新状态，不是读内存状态。
    #[test]
    fn capability_set_is_rebuilt_from_units_table() {
        let mut store = Store::open_in_memory().expect("应当可以打开");
        seed(&mut store, "store", UnitState::Ready);
        seed(&mut store, "model", UnitState::Ready);
        let mut watcher = CapabilityWatcher::new(store).expect("应当可以构造");
        let log = Rc::new(RefCell::new(Vec::new()));
        let value = log.clone();
        watcher.subscribe(move |update| value.borrow_mut().push(update.clone()));
        // 直接改表：store 从 ready 改成 degraded，能力集应当跟表走。
        watcher.store.set_unit_state("store", UnitState::Degraded).expect("写入应当成功");
        watcher.notify_change().expect("应当成功");
        match &log.borrow()[1] {
            CapabilityUpdate::CapabilitySet(set) => {
                assert_eq!(set.ready, vec![id("model")], "就绪列表应当是 [model]");
                assert_eq!(set.degraded, vec![id("store")], "降级列表应当是 [store]");
            }
        }
    }

    /// 能力集变更推送给已连接客户端：两个订阅者都收到推送。
    #[test]
    fn all_subscribers_receive_the_push() {
        let store = Store::open_in_memory().expect("应当可以打开");
        let mut watcher = CapabilityWatcher::new(store).expect("应当可以构造");
        let first = Rc::new(RefCell::new(Vec::new()));
        let second = Rc::new(RefCell::new(Vec::new()));
        let first_value = first.clone();
        let second_value = second.clone();
        watcher.subscribe(move |update| first_value.borrow_mut().push(update.clone()));
        watcher.subscribe(move |update| second_value.borrow_mut().push(update.clone()));
        watcher.notify_change().expect("应当成功");
        assert_eq!(first.borrow().len(), 2, "第一个订阅者应当收到两条消息");
        assert_eq!(second.borrow().len(), 2, "第二个订阅者应当收到两条消息");
    }

    /// 推送与工具目录追加是同一套机制：通道消息是通用的能力变更消息，能力集变更是其中一种。
    #[test]
    fn channel_carries_generic_capability_update() {
        let store = Store::open_in_memory().expect("应当可以打开");
        let mut watcher = CapabilityWatcher::new(store).expect("应当可以构造");
        let log = Rc::new(RefCell::new(Vec::new()));
        let value = log.clone();
        watcher.subscribe(move |update| value.borrow_mut().push(update.clone()));
        watcher.notify_change().expect("应当成功");
        // 通道消息是通用的能力变更消息：能力集变更是其中一种，工具目录追加以后会在同一
        // 通道上加为另一种，机制相同。
        for update in log.borrow().iter() {
            assert!(matches!(update, CapabilityUpdate::CapabilitySet(_)), "消息应当是能力集变更");
        }
    }
}
