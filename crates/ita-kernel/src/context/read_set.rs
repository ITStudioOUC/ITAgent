//! 读集与陈旧写判定。
//!
//! 租约只解决"谁先写"，解决不了"我读到的是不是最新的"。A 读了文件、租约已释放、B 改了
//! 文件、A 再写入——这个序列里没有任何并发，租约完全拦不住，而它是并行探索阶段最常见的
//! 失败模式。
//!
//! 这里维护"谁在什么时候读到了什么"，并在写入前判定调用者的视图是否已经过期。表的键是
//! （路径, 会话）：同一路径上不同会话各有自己的最后读到的哈希，只记"最后一次读取"会让
//! 判定与读者无关，那是错的。

use std::collections::BTreeMap;

use super::ids::SessionId;

/// 一条读记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadRecord {
    /// 读到时的内容哈希。
    pub hash: String,
    /// 读到时的序号，用于判断先后。
    pub seq: u64,
}

/// 陈旧写判定的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaleVerdict {
    /// 调用者读到的版本就是当前版本，可以写入。
    Fresh,
    /// 调用者的视图已过期，拒绝写入。
    Stale {
        /// 调用者最后读到的哈希。
        expected: String,
        /// 磁盘上的当前哈希。
        actual: String,
    },
    /// 没有该会话的读记录。
    ///
    /// 允许写入，但调用方应当在事件里标注"未知"——这不是错误，而是"没有依据可以比较"。
    Unknown,
}

/// 读集。
///
/// 只做内存里的记录与判定；持久化与回收属于存储层。这样"陈旧写该怎么判"可以在不碰
/// 数据库的情况下穷举测试。
#[derive(Debug, Default)]
pub struct ReadSet {
    // 外层键是路径，内层键是会话。用 BTreeMap 而不是 HashMap：遍历顺序必须确定，
    // 否则"列出读过某文件的会话"这种诊断输出会在两次运行间变化。
    entries: BTreeMap<String, BTreeMap<SessionId, ReadRecord>>,
}

impl ReadSet {
    /// 建一个空读集。
    pub fn new() -> Self {
        Self::default()
    }

    /// 记录一次读取。
    ///
    /// # 边界与前提
    /// 同一会话对同一路径的重复读取会覆盖旧记录：判定的依据是"最后一次读到的版本"。
    ///
    /// # 副作用
    /// 修改内部表。
    pub fn record(&mut self, path: &str, session: SessionId, hash: String, seq: u64) {
        self.entries.entry(path.to_owned()).or_default().insert(session, ReadRecord { hash, seq });
    }

    /// 取某个会话对某路径的读记录。
    ///
    /// # 副作用
    /// 无。
    pub fn get(&self, path: &str, session: &SessionId) -> Option<&ReadRecord> {
        self.entries.get(path).and_then(|m| m.get(session))
    }

    /// 判定一次写入是否基于过期视图。
    ///
    /// # 边界与前提
    /// 只比较哈希，不比较序号：序号用于诊断"谁先读的"，而正确性只取决于内容是否相同。
    /// 没有记录时返回 [`StaleVerdict::Unknown`]，由调用方决定是否允许——默认允许并标注，
    /// 因为拒绝会让"第一次写一个新文件"永远失败。
    ///
    /// # 副作用
    /// 无。
    pub fn judge(&self, path: &str, session: &SessionId, current_hash: &str) -> StaleVerdict {
        match self.get(path, session) {
            None => StaleVerdict::Unknown,
            Some(record) if record.hash == current_hash => StaleVerdict::Fresh,
            Some(record) => StaleVerdict::Stale {
                expected: record.hash.clone(),
                actual: current_hash.to_owned(),
            },
        }
    }

    /// 列出读过某路径的所有会话，按会话 id 排序。
    ///
    /// # 边界与前提
    /// 用于写入后推送"文件已更新"通知：通知对象是"读过这个文件的所有会话"，而不只是
    /// 正在等锁的那个。
    ///
    /// # 副作用
    /// 无。
    pub fn readers(&self, path: &str) -> Vec<&SessionId> {
        match self.entries.get(path) {
            None => Vec::new(),
            Some(map) => map.keys().collect(),
        }
    }

    /// 清掉某个会话的全部记录。
    ///
    /// # 边界与前提
    /// 会话结束时调用。读集不是权威数据：丢了只少一条通知，不会导致判定错误（会退化成
    /// `Unknown`）。
    ///
    /// # 副作用
    /// 修改内部表，可能删掉空的路径项。
    pub fn forget_session(&mut self, session: &SessionId) {
        self.entries.retain(|_, map| {
            map.remove(session);
            !map.is_empty()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

    fn session(n: u8) -> SessionId {
        let mut text = BASE.to_owned();
        text.replace_range(24..26, &format!("{n:02}"));
        SessionId::parse(text).expect("应当合法")
    }

    #[test]
    fn 读到同一版本时可以写入() {
        let mut set = ReadSet::new();
        let a = session(1);
        set.record("a.rs", a.clone(), "h1".to_owned(), 1);
        assert_eq!(set.judge("a.rs", &a, "h1"), StaleVerdict::Fresh);
    }

    #[test]
    fn 别人改过之后写入被拒并给出两边的哈希() {
        let mut set = ReadSet::new();
        let a = session(1);
        set.record("a.rs", a.clone(), "h1".to_owned(), 1);
        assert_eq!(
            set.judge("a.rs", &a, "h2"),
            StaleVerdict::Stale { expected: "h1".to_owned(), actual: "h2".to_owned() }
        );
    }

    #[test]
    fn 没有记录时判为未知而不是过期() {
        let set = ReadSet::new();
        assert_eq!(set.judge("new.rs", &session(1), "h1"), StaleVerdict::Unknown);
    }

    #[test]
    fn 不同会话各自持有自己的读记录() {
        // 这是"只记最后一次读取"会出错的地方：A 读 h1、B 读 h2，A 的视图仍然是 h1。
        let mut set = ReadSet::new();
        let a = session(1);
        let b = session(2);
        set.record("a.rs", a.clone(), "h1".to_owned(), 1);
        set.record("a.rs", b.clone(), "h2".to_owned(), 2);

        assert_eq!(set.judge("a.rs", &a, "h1"), StaleVerdict::Fresh);
        assert_eq!(
            set.judge("a.rs", &b, "h1"),
            StaleVerdict::Stale { expected: "h2".to_owned(), actual: "h1".to_owned() }
        );
    }

    #[test]
    fn 同一会话重复读取覆盖旧记录() {
        let mut set = ReadSet::new();
        let a = session(1);
        set.record("a.rs", a.clone(), "h1".to_owned(), 1);
        set.record("a.rs", a.clone(), "h2".to_owned(), 5);
        assert_eq!(set.judge("a.rs", &a, "h2"), StaleVerdict::Fresh);
        assert_eq!(set.get("a.rs", &a).expect("应当有记录").seq, 5);
    }

    #[test]
    fn 读者列表包含所有读过该文件的会话且有序() {
        let mut set = ReadSet::new();
        let a = session(3);
        let b = session(1);
        set.record("a.rs", a.clone(), "h1".to_owned(), 1);
        set.record("a.rs", b.clone(), "h1".to_owned(), 2);
        let readers = set.readers("a.rs");
        assert_eq!(readers.len(), 2);
        // BTreeMap 保证按会话 id 升序。
        assert!(readers[0] < readers[1]);
        assert!(readers.contains(&&a));
        assert!(readers.contains(&&b));
    }

    #[test]
    fn 没人读过时读者列表为空() {
        let set = ReadSet::new();
        assert!(set.readers("nobody.rs").is_empty());
    }

    #[test]
    fn 会话结束时记录被清掉() {
        let mut set = ReadSet::new();
        let a = session(1);
        let b = session(2);
        set.record("a.rs", a.clone(), "h1".to_owned(), 1);
        set.record("a.rs", b.clone(), "h1".to_owned(), 2);
        set.forget_session(&a);

        assert!(set.get("a.rs", &a).is_none());
        assert!(set.get("a.rs", &b).is_some());
        assert_eq!(set.readers("a.rs").len(), 1);
        // 清掉之后判定退化为未知，而不是错误。
        assert_eq!(set.judge("a.rs", &a, "h1"), StaleVerdict::Unknown);
    }

    #[test]
    fn 清空最后一个读者时路径项被移除() {
        let mut set = ReadSet::new();
        let a = session(1);
        set.record("a.rs", a.clone(), "h1".to_owned(), 1);
        set.forget_session(&a);
        assert!(set.readers("a.rs").is_empty());
    }

    #[test]
    fn 不同路径互不影响() {
        let mut set = ReadSet::new();
        let a = session(1);
        set.record("a.rs", a.clone(), "h1".to_owned(), 1);
        set.record("b.rs", a.clone(), "h2".to_owned(), 2);
        assert_eq!(set.judge("a.rs", &a, "h1"), StaleVerdict::Fresh);
        assert_eq!(set.judge("b.rs", &a, "h2"), StaleVerdict::Fresh);
        assert_eq!(set.judge("c.rs", &a, "h3"), StaleVerdict::Unknown);
    }
}
