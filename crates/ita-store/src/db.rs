//! 存储句柄。
//!
//! 一个 [`Store`] 对应一个数据库文件，且只由 server 进程持有。worker 与执行器不持有它，
//! 因此"作用域泄漏"只可能发生在 server 内部，而 server 内部的接口形状（见
//! [`WorkspaceScope`]）已经把它堵住。
//!
//! 写路径的纪律在这里落地：单连接、`BEGIN IMMEDIATE`、`busy_timeout`。不使用延迟事务，
//! 因为它会在提交时才升级锁，产生无法重试的 `SQLITE_BUSY`。

use std::path::Path;

use rusqlite::Connection;

use crate::migrate::{self, MigrateError};
use crate::scope::WorkspaceScope;

/// 存储层错误。
#[derive(Debug)]
pub enum StoreError {
    /// 数据库操作失败。
    Sqlite(rusqlite::Error),
    /// 迁移失败。
    Migrate(MigrateError),
    /// 工作区不存在。
    UnknownWorkspace(String),
    /// 该文件系统不支持 WAL，返回实际使用的日志模式。
    WalUnavailable(String),
    /// 系统时钟早于 1970。
    ClockBeforeEpoch,
    /// 时间戳超出 i64 范围。
    ClockOverflow,
    /// 工作区已存在。
    WorkspaceExists(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite(e) => write!(f, "数据库错误：{e}"),
            Self::Migrate(e) => write!(f, "迁移失败：{e}"),
            Self::UnknownWorkspace(id) => write!(f, "未知工作区 {id}"),
            Self::ClockBeforeEpoch => write!(f, "系统时钟早于 1970，拒绝写入不可区分的时间戳"),
            Self::ClockOverflow => write!(f, "时间戳超出可存储范围"),
            Self::WalUnavailable(mode) => write!(
                f,
                "该文件系统不支持 WAL（实际模式 {mode}）；读将被写阻塞，批量提交的吞吐假设不成立"
            ),
            Self::WorkspaceExists(id) => write!(f, "工作区 {id} 已存在"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sqlite(e)
    }
}

impl From<MigrateError> for StoreError {
    fn from(e: MigrateError) -> Self {
        Self::Migrate(e)
    }
}

/// 数据库句柄。
pub struct Store {
    conn: Connection,
}

impl Store {
    /// 打开（或创建）一个数据库文件并跑完迁移。
    ///
    /// # 步骤
    /// 1. 打开连接，开启外键约束与 WAL。
    /// 2. 设置忙等超时：写事务短暂冲突时自动重试，而不是立刻失败。
    /// 3. 跑迁移。
    ///
    /// # 边界与前提
    /// 同一时刻只允许一个 `Store` 指向同一个文件（单写者）。这不是靠锁保证的，而是靠
    /// "只有 server 打开数据库"这条进程模型保证的。
    ///
    /// # 副作用
    /// 创建文件、写库结构、可能创建 WAL 与 SHM 文件。
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let conn = Connection::open(path)?;
        Self::configure(conn)
    }

    /// 打开一个内存库，仅供测试。
    ///
    /// # 副作用
    /// 无持久化副作用。
    pub fn open_in_memory() -> Result<Self, StoreError> {
        let conn = Connection::open_in_memory()?;
        Self::configure(conn)
    }

    fn configure(mut conn: Connection) -> Result<Self, StoreError> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        // 5 秒忙等：写事务冲突时自动重试，而不是立刻抛 SQLITE_BUSY。
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        // 立即事务：后续所有 `transaction()` 一开始就拿写锁。延迟事务在 WAL 下会在提交时
        // 才升级锁，可能拿到 SQLITE_BUSY_SNAPSHOT——它不能靠重试语句解决。
        conn.set_transaction_behavior(rusqlite::TransactionBehavior::Immediate);

        migrate::run(&mut conn)?;
        Ok(Self { conn })
    }

    /// 打开一个文件库并确认 WAL 真的生效。
    ///
    /// # 边界与前提
    /// WAL 让读不被写阻塞，而单写者加批量提交的吞吐假设正建立在这上面。某些文件系统
    /// （网络盘、部分容器卷）不支持共享内存，会静默退回 rollback journal，因此这里校验
    /// 实际模式而不是假设 `pragma_update` 成功就拿到了 WAL。
    ///
    /// # 副作用
    /// 创建文件、WAL 与 SHM 文件。
    pub fn open_with_wal(path: &Path) -> Result<Self, StoreError> {
        let conn = Connection::open(path)?;
        let mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err(StoreError::WalUnavailable(mode));
        }
        Self::configure(conn)
    }

    /// 注册一个工作区。
    ///
    /// # 边界与前提
    /// 工作区 id 由调用方生成（ULID），重复注册返回 [`StoreError::WorkspaceExists`]。
    ///
    /// # 副作用
    /// 写库。
    pub fn create_workspace(
        &mut self,
        id: &ita_kernel::context::WorkspaceId,
        root_path: &str,
    ) -> Result<(), StoreError> {
        let now = now_millis()?;
        // 两条插入必须在同一事务里：中间崩溃会留下"有工作区、没计数器"的库，而重跑时
        // 第一条 INSERT 影响零行、直接报已存在，计数器行再也补不上。
        let tx = self.conn.transaction()?;
        let changed = tx.execute(
            "INSERT INTO workspaces (id, root_path, created_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO NOTHING",
            rusqlite::params![id.as_str(), root_path, now],
        )?;
        if changed == 0 {
            return Err(StoreError::WorkspaceExists(id.as_str().to_owned()));
        }
        tx.execute(
            "INSERT INTO workspace_seq (workspace_id, next_seq) VALUES (?1, 1)",
            rusqlite::params![id.as_str()],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// 为一个已存在的工作区创建作用域。
    ///
    /// # 边界与前提
    /// 工作区必须已经注册；否则返回 [`StoreError::UnknownWorkspace`]。这个检查是作用域
    /// 唯一的构造入口，因此"作用域一定对应真实工作区"成为不变量。
    ///
    /// # 副作用
    /// 只读。
    pub fn scope(
        &self,
        id: &ita_kernel::context::WorkspaceId,
    ) -> Result<WorkspaceScope, StoreError> {
        let exists: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM workspaces WHERE id = ?1)",
            rusqlite::params![id.as_str()],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(StoreError::UnknownWorkspace(id.as_str().to_owned()));
        }
        Ok(WorkspaceScope::new(id.clone()))
    }

    /// 已注册的工作区数量。
    ///
    /// # 副作用
    /// 只读。
    pub fn workspace_count(&self) -> Result<u32, StoreError> {
        Ok(self.conn.query_row("SELECT COUNT(*) FROM workspaces", [], |row| row.get(0))?)
    }

    /// 当前迁移版本。
    ///
    /// # 副作用
    /// 只读。
    pub fn schema_version(&self) -> Result<u32, StoreError> {
        Ok(migrate::current_version(&self.conn)?)
    }

    /// 供 crate 内部模块使用的连接。
    ///
    /// # 边界与前提
    /// `pub(crate)`：业务代码（其他 crate）拿不到连接，只能走操作级方法。
    ///
    /// # 副作用
    /// 取决于调用方。
    // 目前只有本文件的测试用到它；后续事件与快照模块会正式使用。
    // 用 allow 而不是 expect：测试目标里它确实被用到，expect 会在那里不成立。
    #[allow(dead_code)]
    pub(crate) fn conn(&self) -> &Connection {
        &self.conn
    }

    /// 供集成测试检查表结构的连接。
    ///
    /// # 边界与前提
    /// 只在测试里可用（`#[doc(hidden)]` + 测试专用名字），业务代码不应调用。
    ///
    /// # 副作用
    /// 取决于调用方。
    #[doc(hidden)]
    pub fn conn_for_test(&self) -> &Connection {
        &self.conn
    }
}

/// 当前时间（epoch 毫秒）。
///
/// # 边界与前提
/// 时钟异常（早于 1970）时返回错误而不是零：写出一行 `created_at = 0` 会与"1970 年"不可
/// 区分，而这种行会在排序与过期判断里造成难以追踪的偏差。
///
/// # 副作用
/// 读时钟。存储层允许，内核不允许。
pub(crate) fn now_millis() -> Result<i64, StoreError> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| StoreError::ClockBeforeEpoch)?
        .as_millis();
    i64::try_from(millis).map_err(|_| StoreError::ClockOverflow)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ita_kernel::context::WorkspaceId;

    const ID_A: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    const ID_B: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";

    fn workspace(text: &str) -> WorkspaceId {
        WorkspaceId::parse(text).expect("应当合法")
    }

    #[test]
    fn 打开时跑完迁移() {
        let store = Store::open_in_memory().expect("应当可以打开");
        assert_eq!(store.schema_version().expect("应当可读"), 2);
    }

    #[test]
    fn 注册工作区后可以取作用域() {
        let mut store = Store::open_in_memory().expect("应当可以打开");
        let id = workspace(ID_A);
        store.create_workspace(&id, "/tmp/ws").expect("应当成功");
        let scope = store.scope(&id).expect("应当可以取作用域");
        assert_eq!(scope.workspace(), &id);
        assert_eq!(store.workspace_count().expect("应当可读"), 1);
    }

    #[test]
    fn 未注册的工作区取作用域被拒() {
        let store = Store::open_in_memory().expect("应当可以打开");
        let err = store.scope(&workspace(ID_B)).expect_err("应当被拒");
        assert!(matches!(err, StoreError::UnknownWorkspace(_)));
    }

    #[test]
    fn 重复注册同一工作区被拒() {
        let mut store = Store::open_in_memory().expect("应当可以打开");
        let id = workspace(ID_A);
        store.create_workspace(&id, "/tmp/ws").expect("首次应当成功");
        let err = store.create_workspace(&id, "/tmp/ws").expect_err("应当被拒");
        assert!(matches!(err, StoreError::WorkspaceExists(_)));
    }

    #[test]
    fn 多个工作区互不影响() {
        let mut store = Store::open_in_memory().expect("应当可以打开");
        store.create_workspace(&workspace(ID_A), "/tmp/a").expect("应当成功");
        store.create_workspace(&workspace(ID_B), "/tmp/b").expect("应当成功");
        assert_eq!(store.workspace_count().expect("应当可读"), 2);
    }

    #[test]
    fn 外键约束已开启() {
        let store = Store::open_in_memory().expect("应当可以打开");
        // 直接插一条引用不存在工作区的会话，应当被外键拦住。
        let result = store.conn().execute(
            "INSERT INTO sessions (id, workspace_id, head_node, created_at)
             VALUES ('01ARZ3NDEKTSV4RRFFQ69G5FAV', '01ARZ3NDEKTSV4RRFFQ69G5FAW', 'x', 0)",
            [],
        );
        assert!(result.is_err(), "外键应当拦住悬空引用");
    }
}
