//! 工作区 seq 计数器的读改写。
//!
//! seq 从 `workspace_seq` 计数器分配，而不是从事件表里取最大值。计数器按工作区分行，在写
//! 事务内读改写：先读出 `next_seq` 作为本条事件的 seq，再写回加一。分配与插入事件必须在
//! 同一个事务里，否则崩溃会留下"计数器已推进、事件没写"的洞。
//!
//! 不用 `UPDATE ... RETURNING` 一步写法：`CONVENTIONS` 禁用 `RETURNING`，那是留住换
//! Postgres/MySQL 的门的一部分。安全性由 `BEGIN IMMEDIATE` 提供——事务一开始就拿写锁，
//! 读到的值在写回之前不会被别人改。

use ita_kernel::context::WorkspaceId;
use rusqlite::Transaction;

use crate::db::StoreError;

/// 从计数器分配下一个 seq。
///
/// # 步骤
/// 1. 读出该工作区的 `next_seq`。
/// 2. 写回 `next_seq + 1`。
/// 3. 返回读出的值作为本条事件的 seq。
///
/// # 边界与前提
/// 接收一个已开启的事务，不自行开事务。读与写都在该事务内，因此分配与插入原子。工作区
/// 不存在时读出空行，转为 [`StoreError::Sqlite`]。
///
/// # 副作用
/// 写 `workspace_seq`。
pub(crate) fn allocate(tx: &Transaction, workspace: &WorkspaceId) -> Result<i64, StoreError> {
    let next_seq: i64 = tx.query_row(
        "SELECT next_seq FROM workspace_seq WHERE workspace_id = ?1",
        rusqlite::params![workspace.as_str()],
        |row| row.get(0),
    )?;
    tx.execute(
        "UPDATE workspace_seq SET next_seq = ?1 WHERE workspace_id = ?2",
        rusqlite::params![next_seq + 1, workspace.as_str()],
    )?;
    Ok(next_seq)
}
