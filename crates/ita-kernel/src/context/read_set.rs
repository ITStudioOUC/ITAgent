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

use super::ids::{SessionId, WorkspaceId};

/// 一个归一化后的路径键。
///
/// 内核不做文件系统解析（符号链接、大小写、工作区根拼接都需要 IO），但它必须让"同一
/// 文件的不同写法"在比较前就被统一，否则 `./a.rs` 与 `a.rs` 会各占一条记录，陈旧写判定
/// 静默退化为"未知"。这里只做纯字符串层面的归一：去掉多余的 `/`、去掉开头的 `./`、
/// 去掉末尾的 `/`。更进一步的归一（真实路径、大小写）由调用方在构造键之前完成。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PathKey(String);

impl PathKey {
    /// 从模型给出的路径构造。
    ///
    /// # 边界与前提
    /// 空路径被拒绝；归一化只处理分隔符，不解析 `..`（那需要知道当前目录）。
    ///
    /// # 副作用
    /// 无。
    pub fn new(raw: &str) -> Option<Self> {
        let mut parts: Vec<&str> = Vec::new();
        for part in raw.split('/') {
            match part {
                "" | "." => continue,
                other => parts.push(other),
            }
        }
        if parts.is_empty() {
            return None;
        }
        Some(Self(parts.join("/")))
    }

    /// 取归一化后的文本。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

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
    // 外层按工作区，再按路径，内层按会话。工作区维度是必需的：同一个相对路径在两个
    // 工作区里是两个文件，缺了它会让一个工作区的读取影响另一个的判定。
    // 全程用 BTreeMap：遍历顺序必须确定，否则诊断输出会在两次运行间变化。
    entries: BTreeMap<WorkspaceId, BTreeMap<PathKey, BTreeMap<SessionId, ReadRecord>>>,
}

impl ReadSet {
    /// 建一个空读集。
    pub fn new() -> Self {
        Self::default()
    }

    /// 记录一次读取。
    ///
    /// # 边界与前提
    /// 同一会话对同一路径的重复读取只在序号更大时覆盖：乱序到达的旧记录不能把新记录
    /// 顶掉，否则判定会拿一个过期的哈希去比较，产生假的"陈旧"。
    ///
    /// # 副作用
    /// 修改内部表。
    pub fn record(
        &mut self,
        workspace: &WorkspaceId,
        path: &PathKey,
        session: SessionId,
        hash: String,
        seq: u64,
    ) {
        let slot = self
            .entries
            .entry(workspace.clone())
            .or_default()
            .entry(path.clone())
            .or_default()
            .entry(session);
        match slot {
            std::collections::btree_map::Entry::Occupied(mut occupied) => {
                if seq >= occupied.get().seq {
                    occupied.insert(ReadRecord { hash, seq });
                }
            }
            std::collections::btree_map::Entry::Vacant(vacant) => {
                vacant.insert(ReadRecord { hash, seq });
            }
        }
    }

    /// 取某个会话对某路径的读记录。
    ///
    /// # 副作用
    /// 无。
    pub fn get(
        &self,
        workspace: &WorkspaceId,
        path: &PathKey,
        session: &SessionId,
    ) -> Option<&ReadRecord> {
        self.entries.get(workspace).and_then(|m| m.get(path)).and_then(|m| m.get(session))
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
    pub fn judge(
        &self,
        workspace: &WorkspaceId,
        path: &PathKey,
        session: &SessionId,
        current_hash: &str,
    ) -> StaleVerdict {
        match self.get(workspace, path, session) {
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
    pub fn readers(&self, workspace: &WorkspaceId, path: &PathKey) -> Vec<&SessionId> {
        match self.entries.get(workspace).and_then(|m| m.get(path)) {
            None => Vec::new(),
            Some(map) => map.keys().collect(),
        }
    }

    /// 列出读过某路径、且视图与给定哈希不一致的会话。
    ///
    /// # 边界与前提
    /// 这是"文件已更新"通知应当发给的对象：只发给全部读者会让已经重新读过最新版本的
    /// 会话收到无意义的提醒，而它恰恰最不该被打断。
    ///
    /// # 副作用
    /// 无。
    pub fn stale_readers(
        &self,
        workspace: &WorkspaceId,
        path: &PathKey,
        current_hash: &str,
    ) -> Vec<&SessionId> {
        match self.entries.get(workspace).and_then(|m| m.get(path)) {
            None => Vec::new(),
            Some(map) => {
                map.iter().filter(|(_, r)| r.hash != current_hash).map(|(s, _)| s).collect()
            }
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
        self.entries.retain(|_, by_path| {
            by_path.retain(|_, by_session| {
                by_session.remove(session);
                !by_session.is_empty()
            });
            !by_path.is_empty()
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

    fn workspace(n: u8) -> WorkspaceId {
        let mut text = BASE.to_owned();
        text.replace_range(23..26, &format!("{n:03}"));
        WorkspaceId::parse(text).expect("应当合法")
    }

    fn path(s: &str) -> PathKey {
        PathKey::new(s).expect("应当可以归一")
    }

    #[test]
    fn 路径归一化去掉冗余分隔符() {
        assert_eq!(path("./a.rs").as_str(), "a.rs");
        assert_eq!(path("src//main.rs").as_str(), "src/main.rs");
        assert_eq!(path("src/").as_str(), "src");
        assert!(PathKey::new("").is_none());
        assert!(PathKey::new("./").is_none());
    }

    #[test]
    fn 同一文件的不同写法归到同一键() {
        assert_eq!(path("./src/main.rs"), path("src/main.rs"));
        assert_eq!(path("src//main.rs"), path("src/main.rs"));
    }

    #[test]
    fn 读到同一版本时可以写入() {
        let mut set = ReadSet::new();
        let (w, a, p) = (workspace(1), session(1), path("a.rs"));
        set.record(&w, &p, a.clone(), "h1".to_owned(), 1);
        assert_eq!(set.judge(&w, &p, &a, "h1"), StaleVerdict::Fresh);
    }

    #[test]
    fn 别人改过之后写入被拒并给出两边的哈希() {
        let mut set = ReadSet::new();
        let (w, a, p) = (workspace(1), session(1), path("a.rs"));
        set.record(&w, &p, a.clone(), "h1".to_owned(), 1);
        assert_eq!(
            set.judge(&w, &p, &a, "h2"),
            StaleVerdict::Stale { expected: "h1".to_owned(), actual: "h2".to_owned() }
        );
    }

    #[test]
    fn 没有记录时判为未知而不是过期() {
        let set = ReadSet::new();
        assert_eq!(
            set.judge(&workspace(1), &path("new.rs"), &session(1), "h1"),
            StaleVerdict::Unknown
        );
    }

    #[test]
    fn 不同会话各自持有自己的读记录() {
        // 这是"只记最后一次读取"会出错的地方：A 读 h1、B 读 h2，A 的视图仍然是 h1。
        let mut set = ReadSet::new();
        let (w, p) = (workspace(1), path("a.rs"));
        let a = session(1);
        let b = session(2);
        set.record(&w, &p, a.clone(), "h1".to_owned(), 1);
        set.record(&w, &p, b.clone(), "h2".to_owned(), 2);

        assert_eq!(set.judge(&w, &p, &a, "h1"), StaleVerdict::Fresh);
        assert_eq!(
            set.judge(&w, &p, &b, "h1"),
            StaleVerdict::Stale { expected: "h2".to_owned(), actual: "h1".to_owned() }
        );
    }

    #[test]
    fn 不同工作区的同一相对路径互不影响() {
        let mut set = ReadSet::new();
        let (w1, w2, p) = (workspace(1), workspace(2), path("a.rs"));
        let a = session(1);
        set.record(&w1, &p, a.clone(), "h1".to_owned(), 1);
        assert_eq!(set.judge(&w1, &p, &a, "h1"), StaleVerdict::Fresh);
        // 另一个工作区里同名文件从未被读过。
        assert_eq!(set.judge(&w2, &p, &a, "h1"), StaleVerdict::Unknown);
    }

    #[test]
    fn 乱序到达的旧记录不覆盖新记录() {
        let mut set = ReadSet::new();
        let (w, a, p) = (workspace(1), session(1), path("a.rs"));
        set.record(&w, &p, a.clone(), "h2".to_owned(), 5);
        set.record(&w, &p, a.clone(), "h1".to_owned(), 2);
        assert_eq!(set.get(&w, &p, &a).expect("应当有记录").hash, "h2");
    }

    #[test]
    fn 完整序列_读旧_被改_重读_再判新鲜() {
        let mut set = ReadSet::new();
        let (w, a, p) = (workspace(1), session(1), path("a.rs"));
        set.record(&w, &p, a.clone(), "h1".to_owned(), 1);
        assert!(matches!(set.judge(&w, &p, &a, "h2"), StaleVerdict::Stale { .. }));
        set.record(&w, &p, a.clone(), "h2".to_owned(), 2);
        assert_eq!(set.judge(&w, &p, &a, "h2"), StaleVerdict::Fresh);
    }

    #[test]
    fn 读者列表包含所有读过该文件的会话且有序() {
        let mut set = ReadSet::new();
        let (w, p) = (workspace(1), path("a.rs"));
        let a = session(3);
        let b = session(1);
        set.record(&w, &p, a.clone(), "h1".to_owned(), 1);
        set.record(&w, &p, b.clone(), "h1".to_owned(), 2);
        let readers = set.readers(&w, &p);
        assert_eq!(readers.len(), 2);
        assert!(readers[0] < readers[1], "BTreeMap 保证按会话 id 升序");
    }

    #[test]
    fn 只把通知发给视图过期的读者() {
        let mut set = ReadSet::new();
        let (w, p) = (workspace(1), path("a.rs"));
        let stale = session(1);
        let fresh = session(2);
        set.record(&w, &p, stale.clone(), "h1".to_owned(), 1);
        set.record(&w, &p, fresh.clone(), "h2".to_owned(), 2);

        let targets = set.stale_readers(&w, &p, "h2");
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0], &stale, "已重读最新版本的会话不该被打断");
    }

    #[test]
    fn 没人读过时读者列表为空() {
        let set = ReadSet::new();
        assert!(set.readers(&workspace(1), &path("nobody.rs")).is_empty());
        assert!(set.stale_readers(&workspace(1), &path("nobody.rs"), "h1").is_empty());
    }

    #[test]
    fn 会话结束时记录被清掉() {
        let mut set = ReadSet::new();
        let (w, p) = (workspace(1), path("a.rs"));
        let a = session(1);
        let b = session(2);
        set.record(&w, &p, a.clone(), "h1".to_owned(), 1);
        set.record(&w, &p, b.clone(), "h1".to_owned(), 2);
        set.forget_session(&a);

        assert!(set.get(&w, &p, &a).is_none());
        assert!(set.get(&w, &p, &b).is_some());
        assert_eq!(set.readers(&w, &p).len(), 1);
        // 清掉之后判定退化为未知，而不是错误。
        assert_eq!(set.judge(&w, &p, &a, "h1"), StaleVerdict::Unknown);
    }

    #[test]
    fn 清空最后一个读者时路径项被移除() {
        let mut set = ReadSet::new();
        let (w, p) = (workspace(1), path("a.rs"));
        let a = session(1);
        set.record(&w, &p, a.clone(), "h1".to_owned(), 1);
        set.forget_session(&a);
        assert!(set.readers(&w, &p).is_empty());
    }

    #[test]
    fn 不同路径互不影响() {
        let mut set = ReadSet::new();
        let (w, a) = (workspace(1), session(1));
        set.record(&w, &path("a.rs"), a.clone(), "h1".to_owned(), 1);
        set.record(&w, &path("b.rs"), a.clone(), "h2".to_owned(), 2);
        assert_eq!(set.judge(&w, &path("a.rs"), &a, "h1"), StaleVerdict::Fresh);
        assert_eq!(set.judge(&w, &path("b.rs"), &a, "h2"), StaleVerdict::Fresh);
        assert_eq!(set.judge(&w, &path("c.rs"), &a, "h3"), StaleVerdict::Unknown);
    }
}
