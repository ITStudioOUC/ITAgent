//! 存储句柄。
//!
//! 一个 [`Store`] 对应一个数据库文件，且只由 server 进程持有。worker 与执行器不持有它，
//! 因此"作用域泄漏"只可能发生在 server 内部，而 server 内部的接口形状（见
//! [`WorkspaceScope`]）已经把它堵住。
//!
//! 写路径的纪律在这里落地：单连接、`BEGIN IMMEDIATE`、`busy_timeout`。不使用延迟事务，
//! 因为它会在提交时才升级锁，产生无法重试的 `SQLITE_BUSY`。

use std::path::{Path, PathBuf};

use ita_kernel::context::{BlobRef, CheckpointId, NodeId, SessionId};
use rusqlite::Connection;

use crate::align::{self, AlignResult};
use crate::cas;
use crate::entries::{self, SnapshotEntry};
use crate::events::{self, AppendedEvent, Event, NewEvent};
use crate::migrate::{self, MigrateError};
use crate::preview::PreviewLimit;
use crate::retention::{self, RebaseRequest};
use crate::scope::WorkspaceScope;
use crate::snapshots::{self, CreatedSnapshot, SnapshotInfo};
use crate::squash;

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
    /// 事件引用的会话不属于本工作区。
    UnknownSession(SessionId),
    /// 事件引用的节点不属于本工作区。
    UnknownNode(NodeId),
    /// head 比较并交换冲突：期望的节点与代次不再是当前值。
    HeadConflict,
    /// 帧引用的会话或节点属于另一工作区。
    CrossWorkspace(String),
    /// 内容寻址存储的文件系统操作失败。
    Cas(std::io::Error),
    /// 按哈希读回内容时，行不存在。
    BlobNotFound(String),
    /// 条目声明正文已存，但内容寻址存储里没有该哈希对应的行。
    MissingBlob(String),
    /// 快照不存在，或属于另一工作区。
    UnknownSnapshot(CheckpointId),
    /// 条目形状不合法：正文未存的文件条目缺修改时间。
    InvalidEntryShape(String),
    /// 自主模式重定基线被拒：agent 无人值守连续推进时重定基线一律拒绝。
    RebaseRejectedAutonomous,
    /// 重定基线缺人工确认：留痕三列任一为空。
    RebaseMissingConfirmation,
    /// 回滚对齐被拒：范围内存在外部改动，且未显式强制。
    AlignRefusedExternal(String),
    /// 回滚对齐时工作区根路径不存在。
    AlignRootMissing(String),
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
            Self::UnknownSession(id) => write!(f, "事件引用的会话 {id} 不属于本工作区"),
            Self::UnknownNode(id) => write!(f, "事件引用的节点 {id} 不属于本工作区"),
            Self::HeadConflict => write!(f, "head 比较并交换冲突：期望的节点与代次不再是当前值"),
            Self::CrossWorkspace(id) => write!(f, "帧引用的会话或节点 {id} 属于另一工作区"),
            Self::Cas(e) => write!(f, "内容寻址存储写入失败：{e}"),
            Self::BlobNotFound(hash) => write!(f, "内容寻址存储里没有哈希 {hash} 对应的行"),
            Self::MissingBlob(hash) => {
                write!(f, "条目声明正文已存，但内容寻址存储里没有哈希 {hash} 对应的行")
            }
            Self::UnknownSnapshot(id) => write!(f, "快照 {id} 不存在或属于另一工作区"),
            Self::InvalidEntryShape(msg) => write!(f, "条目形状不合法：{msg}"),
            Self::RebaseRejectedAutonomous => {
                write!(f, "自主模式重定基线被拒：没有人工确认时默认禁止")
            }
            Self::RebaseMissingConfirmation => write!(f, "重定基线缺人工确认：留痕三列任一为空"),
            Self::AlignRefusedExternal(path) => {
                write!(f, "回滚对齐被拒：路径 {path} 存在外部改动，未显式强制")
            }
            Self::AlignRootMissing(path) => write!(f, "回滚对齐时工作区根路径不存在：{path}"),
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
    /// 内容寻址存储的根目录，数据库文件同级的 `blobs` 目录。懒创建：首次写入时才建。
    cas_root: PathBuf,
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
    /// 创建文件、写库结构、可能创建 WAL 与 SHM 文件。CAS 根目录不在这里建，首次写入时才建。
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let conn = Connection::open(path)?;
        Self::configure(conn, cas_root_for(path))
    }

    /// 打开一个内存库，仅供测试。
    ///
    /// # 副作用
    /// 无持久化副作用。CAS 根目录用临时路径，每个库一个。
    pub fn open_in_memory() -> Result<Self, StoreError> {
        let conn = Connection::open_in_memory()?;
        // 内存库没有数据库文件，CAS 根目录落在临时目录，每个库一个，互不干扰。
        let cas_root = std::env::temp_dir().join(format!("ita-cas-{}", ulid::Ulid::new()));
        Self::configure(conn, cas_root)
    }

    fn configure(mut conn: Connection, cas_root: PathBuf) -> Result<Self, StoreError> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        // 5 秒忙等：写事务冲突时自动重试，而不是立刻抛 SQLITE_BUSY。
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        // 立即事务：后续所有 `transaction()` 一开始就拿写锁。延迟事务在 WAL 下会在提交时
        // 才升级锁，可能拿到 SQLITE_BUSY_SNAPSHOT——它不能靠重试语句解决。
        conn.set_transaction_behavior(rusqlite::TransactionBehavior::Immediate);

        migrate::run(&mut conn)?;
        Ok(Self { conn, cas_root })
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
        Self::configure(conn, cas_root_for(path))
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

    /// 追加一条事件。
    ///
    /// # 步骤
    /// 1. 开一个立即事务。
    /// 2. 在事务内追加事件（校验归属、分配 seq、插入）。
    /// 3. 提交。
    ///
    /// # 边界与前提
    /// 追加是权威写，提交后才返回。返回的 seq 与事件 id 代表已持久化的那一行。
    ///
    /// # 副作用
    /// 写 `events` 与 `workspace_seq`。
    pub fn append_event(
        &mut self,
        scope: &WorkspaceScope,
        event: &NewEvent,
        limit: &PreviewLimit,
    ) -> Result<AppendedEvent, StoreError> {
        let tx = self.conn_mut().transaction()?;
        let appended = events::append_in(&tx, scope, event, *limit)?;
        tx.commit()?;
        Ok(appended)
    }

    /// 读取某工作区在给定 seq 之后的至多 N 条事件，按 seq 升序。
    ///
    /// # 边界与前提
    /// 只按 seq 排序，不用 `created_at`：事件顺序由 seq 决定。
    ///
    /// # 副作用
    /// 只读。
    pub fn events_after(
        &self,
        scope: &WorkspaceScope,
        after_seq: i64,
        limit: usize,
    ) -> Result<Vec<Event>, StoreError> {
        events::read_after(self.conn(), scope, after_seq, limit)
    }

    /// 写入一段内容到内容寻址存储，返回引用。
    ///
    /// # 步骤
    /// 1. 开一个立即事务。
    /// 2. 在事务内写入内容：去重、原子写文件、插行。
    /// 3. 提交。
    ///
    /// # 边界与前提
    /// 写入是权威写，提交后才返回。返回的哈希与长度代表已持久化的内容。CAS 是全局的，
    /// 不带作用域参数。
    ///
    /// # 副作用
    /// 写 CAS 目录下的文件，写 `blobs` 表。
    pub fn put(&mut self, content: &[u8]) -> Result<BlobRef, StoreError> {
        // 先取出 CAS 根路径，避免与可变连接借用冲突。
        let cas_root = self.cas_root.clone();
        let tx = self.conn_mut().transaction()?;
        let result = cas::put_in(&tx, &cas_root, content)?;
        tx.commit()?;
        Ok(result)
    }

    /// 按哈希读回内容。
    ///
    /// # 边界与前提
    /// 行不存在返回 [`StoreError::BlobNotFound`]；行存在但文件读失败返回
    /// [`StoreError::Cas`]。只读，不自行开事务。
    ///
    /// # 副作用
    /// 读文件系统。
    pub fn get(&self, hash: &str) -> Result<Vec<u8>, StoreError> {
        cas::get_in(self.conn(), &self.cas_root, hash)
    }

    /// 创建一个快照。
    ///
    /// # 步骤
    /// 1. 开一个立即事务。
    /// 2. 在事务内分配 seq、链接父边、插入快照行与清单条目。
    /// 3. 提交。
    ///
    /// # 边界与前提
    /// 创建是权威写，提交后才返回。调用方只给清单，不给链结构：父边与 kind 由存储层自动
    /// 链接，首个快照是基线，其余是增量。
    ///
    /// # 副作用
    /// 写 `snapshots` 与 `snapshot_entries` 表。
    pub fn create_snapshot(
        &mut self,
        scope: &WorkspaceScope,
        entries: &[SnapshotEntry],
    ) -> Result<CreatedSnapshot, StoreError> {
        let tx = self.conn_mut().transaction()?;
        let created = snapshots::create_in(&tx, scope, entries)?;
        tx.commit()?;
        Ok(created)
    }

    /// 读取快照元信息。
    ///
    /// # 边界与前提
    /// 快照不存在或属于另一工作区返回 [`StoreError::UnknownSnapshot`]。
    ///
    /// # 副作用
    /// 只读。
    pub fn snapshot(
        &self,
        scope: &WorkspaceScope,
        id: &CheckpointId,
    ) -> Result<SnapshotInfo, StoreError> {
        snapshots::read_meta(self.conn(), scope, id)
    }

    /// 读取一个快照的全部条目，按路径排序。
    ///
    /// # 边界与前提
    /// 跨工作区校验与 [`Store::snapshot`] 相同。
    ///
    /// # 副作用
    /// 只读。
    pub fn snapshot_entries(
        &self,
        scope: &WorkspaceScope,
        id: &CheckpointId,
    ) -> Result<Vec<SnapshotEntry>, StoreError> {
        entries::read_in(self.conn(), scope, id)
    }

    /// 增量数达到保留窗口时把最老两个增量并入基线。
    ///
    /// # 步骤
    /// 1. 开一个立即事务。
    /// 2. 数该工作区的增量快照数，达到 `max_incrementals` 时合并最老两个增量进基线。
    /// 3. 提交。
    ///
    /// # 边界与前提
    /// `max_incrementals` 是参数不是散落常量：ADR 0018 要求配置入口，配置结构落地前由调用方
    /// 从配置读出后传入。未达到窗口时是空操作。合并事务先重挂子快照再删父，是无动作外键的
    /// 硬约束。
    ///
    /// # 副作用
    /// 写 `snapshots` 与 `snapshot_entries` 表。
    pub fn squash_if_needed(
        &mut self,
        scope: &WorkspaceScope,
        max_incrementals: usize,
    ) -> Result<(), StoreError> {
        let tx = self.conn_mut().transaction()?;
        if retention::needs_squash(&tx, scope, max_incrementals)? {
            squash::merge_in(&tx, scope)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// 重定基线：删所有增量、更新基线条目与留痕三列。
    ///
    /// # 步骤
    /// 1. 开一个立即事务。
    /// 2. 在事务内重定基线：自主模式拒绝、留痕三列校验、删增量、更新基线。
    /// 3. 提交。
    ///
    /// # 边界与前提
    /// 重定基线是独立动词，自主模式一律拒绝，留痕三列成组写入。基线行只更新不删除。
    ///
    /// # 副作用
    /// 写 `snapshots` 与 `snapshot_entries` 表。
    pub fn rebaseline(
        &mut self,
        scope: &WorkspaceScope,
        req: &RebaseRequest,
    ) -> Result<(), StoreError> {
        let tx = self.conn_mut().transaction()?;
        retention::rebaseline_in(&tx, scope, req)?;
        tx.commit()?;
        Ok(())
    }

    /// 回滚对齐：把磁盘对齐到目标快照的有效状态，输出残留清单。
    ///
    /// # 步骤
    /// 1. 开一个立即事务。
    /// 2. 在事务内沿链折叠读有效状态、对齐磁盘、组装残留清单。
    /// 3. 提交。
    ///
    /// # 边界与前提
    /// 对齐是独立动词，不折进 `create_snapshot`。范围是参数，存储层不解析工具清单。超阈值条目
    /// 只报告。写回前核对 blobs，缺失内容标注"内容缺失"。存在外部改动时默认拒绝，除非显式强制。
    ///
    /// # 副作用
    /// 读文件系统、写文件系统（写回、删除、创建目录）。
    pub fn align(
        &mut self,
        scope: &WorkspaceScope,
        target: &CheckpointId,
        range: &[String],
        force: bool,
    ) -> Result<AlignResult, StoreError> {
        let cas_root = self.cas_root.clone();
        let tx = self.conn_mut().transaction()?;
        let result = align::align_in(&tx, scope, target, range, force, &cas_root)?;
        tx.commit()?;
        Ok(result)
    }

    /// 供 crate 内部模块使用的连接。
    ///
    /// # 边界与前提
    /// `pub(crate)`：业务代码（其他 crate）拿不到连接，只能走操作级方法。
    ///
    /// # 副作用
    /// 取决于调用方。
    pub(crate) fn conn(&self) -> &Connection {
        &self.conn
    }

    /// 供 crate 内部模块获取可变连接，用于开写事务。
    ///
    /// # 边界与前提
    /// `pub(crate)`：业务代码拿不到连接。写方法取 `&mut self`，与 `create_workspace` 的形状
    /// 一致，不在同一句柄上留下两种事务获取方式。
    ///
    /// # 副作用
    /// 取决于调用方。
    pub(crate) fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
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

    /// 供集成测试检查 CAS 根目录的路径。
    ///
    /// # 边界与前提
    /// 只在测试里可用（`#[doc(hidden)]` + 测试专用名字），业务代码不应调用。
    ///
    /// # 副作用
    /// 无。
    #[doc(hidden)]
    pub fn cas_root_for_test(&self) -> &Path {
        &self.cas_root
    }
}

/// 从数据库文件路径派生 CAS 根目录：数据库文件同级的 `blobs` 目录。
fn cas_root_for(path: &Path) -> PathBuf {
    path.parent().map(|p| p.join("blobs")).unwrap_or_else(|| PathBuf::from("blobs"))
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

    /// 打开时跑完迁移，返回的库一定是最新版表结构。
    #[test]
    fn migrations_completed_on_open() {
        let store = Store::open_in_memory().expect("应当可以打开");
        assert_eq!(store.schema_version().expect("应当可读"), 2);
    }

    /// 注册工作区后可以取作用域，业务查询能绑定到该工作区。
    #[test]
    fn scope_obtainable_after_workspace_registration() {
        let mut store = Store::open_in_memory().expect("应当可以打开");
        let id = workspace(ID_A);
        store.create_workspace(&id, "/tmp/ws").expect("应当成功");
        let scope = store.scope(&id).expect("应当可以取作用域");
        assert_eq!(scope.workspace(), &id);
        assert_eq!(store.workspace_count().expect("应当可读"), 1);
    }

    /// 未注册的工作区取作用域被拒，查询不会绑定到幻影工作区。
    #[test]
    fn scope_for_unregistered_workspace_rejected() {
        let store = Store::open_in_memory().expect("应当可以打开");
        let err = store.scope(&workspace(ID_B)).expect_err("应当被拒");
        assert!(matches!(err, StoreError::UnknownWorkspace(_)));
    }

    /// 重复注册同一工作区被拒，工作区路径不会被静默覆盖。
    #[test]
    fn duplicate_workspace_registration_rejected() {
        let mut store = Store::open_in_memory().expect("应当可以打开");
        let id = workspace(ID_A);
        store.create_workspace(&id, "/tmp/ws").expect("首次应当成功");
        let err = store.create_workspace(&id, "/tmp/ws").expect_err("应当被拒");
        assert!(matches!(err, StoreError::WorkspaceExists(_)));
    }

    /// 多个工作区互不影响，注册一个不波及其它工作区。
    #[test]
    fn multiple_workspaces_do_not_interfere() {
        let mut store = Store::open_in_memory().expect("应当可以打开");
        store.create_workspace(&workspace(ID_A), "/tmp/a").expect("应当成功");
        store.create_workspace(&workspace(ID_B), "/tmp/b").expect("应当成功");
        assert_eq!(store.workspace_count().expect("应当可读"), 2);
    }

    /// 外键约束已开启，引用不存在工作区的行会被拦住。
    #[test]
    fn foreign_key_constraints_are_on() {
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
