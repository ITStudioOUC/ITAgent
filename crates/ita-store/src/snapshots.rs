//! 快照行操作：seq 分配、父边自动链接、kind 推导、插入与读元信息。
//!
//! 快照链是线性的，每个工作区一条链。存储层自动链接：取该工作区 seq 最大的快照作父，
//! 没有则父为空、kind 取 `baseline`，否则取 `incremental`。调用方只给清单，不给链结构。
//!
//! seq 从表内最大值加一分配，不从计数器分配：快照 seq 是工作区内的序号，不是断线续传的
//! 游标，不会被归档搬走，因此表内推导在单写者加 `BEGIN IMMEDIATE` 下是原子的。
//!
//! 吃一个已开启的事务或连接，不自行开事务。

use ita_kernel::context::CheckpointId;
use rusqlite::{Connection, OptionalExtension, Transaction};

use crate::db::{StoreError, now_millis};
use crate::entries::{self, SnapshotEntry};
use crate::scope::WorkspaceScope;

/// 创建快照的确认内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedSnapshot {
    /// 快照 id，即检查点编号。
    pub id: CheckpointId,
    /// 该工作区内分配到的 seq。
    pub seq: i64,
}

/// 快照元信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotInfo {
    /// 快照 id。
    pub id: CheckpointId,
    /// 该工作区内的 seq。
    pub seq: i64,
    /// 父边。首个快照没有父。
    pub parent: Option<CheckpointId>,
    /// `baseline` 或 `incremental`。
    pub kind: String,
    /// 创建时间（epoch 毫秒）。
    pub created_at: i64,
}

/// 在已开启的事务内创建一个快照：分配 seq、链接父边、插入快照行与清单条目。
///
/// # 步骤
/// 1. 分配 seq：表内最大值加一。
/// 2. 链接父边：seq 最大的快照作父，没有则基线。
/// 3. 插入快照行。
/// 4. 逐条插入清单条目。
///
/// # 边界与前提
/// 接收已开启的事务，不自行开事务。任一步失败时返回错误，事务回滚，快照行与条目行都不
/// 落。空清单合法：工具调用没改任何文件时快照仍然打，只是清单为空。
///
/// # 副作用
/// 写 `snapshots` 与 `snapshot_entries` 表。
pub(crate) fn create_in(
    tx: &Transaction,
    scope: &WorkspaceScope,
    entries: &[SnapshotEntry],
) -> Result<CreatedSnapshot, StoreError> {
    let workspace = scope.workspace();

    // seq 分配：表内最大值加一。快照 seq 是序号不是游标，不会被归档搬走，表内推导在单写
    // 者加 `BEGIN IMMEDIATE` 下是原子的。
    let seq: i64 = tx.query_row(
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM snapshots WHERE workspace_id = ?1",
        rusqlite::params![workspace.as_str()],
        |row| row.get(0),
    )?;

    // 父边链接：该工作区 seq 最大的快照作父，没有则父为空、kind 取 baseline。
    let parent: Option<String> = tx
        .query_row(
            "SELECT id FROM snapshots WHERE workspace_id = ?1 ORDER BY seq DESC LIMIT 1",
            rusqlite::params![workspace.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    let kind = if parent.is_some() { "incremental" } else { "baseline" };

    // 快照 id 复用内核的 CheckpointId：ULID 生成器产出 26 字符 Crockford base32，首位落在
    // 0 到 7，与内核的校验完全相容。
    let id = CheckpointId::parse(ulid::Ulid::new().to_string()).expect("ulid 产出合法 id");

    tx.execute(
        "INSERT INTO snapshots (id, workspace_id, seq, parent_id, kind, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![id.as_str(), workspace.as_str(), seq, parent, kind, now_millis()?,],
    )?;

    // 插入清单条目。任何一条失败，整笔事务回滚。
    for entry in entries {
        entries::insert_in(tx, &id, entry)?;
    }

    Ok(CreatedSnapshot { id, seq })
}

/// 在连接内读回快照元信息。
///
/// # 边界与前提
/// 只读，不自行开事务。快照不存在或属于另一工作区返回 [`StoreError::UnknownSnapshot`]。
///
/// # 副作用
/// 只读。
pub(crate) fn read_meta(
    conn: &Connection,
    scope: &WorkspaceScope,
    id: &CheckpointId,
) -> Result<SnapshotInfo, StoreError> {
    let row: Option<(i64, Option<String>, String, i64)> = conn
        .query_row(
            "SELECT seq, parent_id, kind, created_at
             FROM snapshots WHERE id = ?1 AND workspace_id = ?2",
            rusqlite::params![id.as_str(), scope.workspace().as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let (seq, parent_text, kind, created_at) =
        row.ok_or_else(|| StoreError::UnknownSnapshot(id.clone()))?;
    Ok(SnapshotInfo {
        id: id.clone(),
        seq,
        parent: parent_text.map(|p| CheckpointId::parse(p).expect("库内 id 应当合法")),
        kind,
        created_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Store;
    use ita_kernel::context::WorkspaceId;

    const WS: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

    /// 打开内存库并注册工作区，返回作用域。
    fn store_with_scope() -> (Store, WorkspaceScope) {
        let mut store = Store::open_in_memory().expect("应当可以打开");
        let id = WorkspaceId::parse(WS).expect("应当合法");
        store.create_workspace(&id, "/tmp/ws").expect("应当成功");
        let scope = store.scope(&id).expect("应当可以取作用域");
        (store, scope)
    }

    /// 读回快照行：(kind, seq, parent_id)。
    fn snapshot_row(store: &Store, id: &CheckpointId) -> (String, i64, Option<String>) {
        store
            .conn_for_test()
            .query_row(
                "SELECT kind, seq, parent_id FROM snapshots WHERE id = ?1",
                rusqlite::params![id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("应当可读")
    }

    /// 空工作区的首个快照是基线：seq 为 1，父边为空。
    #[test]
    fn snapshot_first_is_baseline_with_seq_one() {
        let (mut store, scope) = store_with_scope();
        let tx = store.conn_mut().transaction().expect("应当可以开事务");
        let created = create_in(&tx, &scope, &[]).expect("应当成功");
        tx.commit().expect("应当可提交");
        assert_eq!(created.seq, 1, "首个快照的 seq 应当是 1");
        let (kind, seq, parent) = snapshot_row(&store, &created.id);
        assert_eq!(kind, "baseline", "首个快照应当是基线");
        assert_eq!(seq, 1, "seq 应当是 1");
        assert!(parent.is_none(), "父边应当为空");
    }

    /// 第二个快照是增量：seq 为 2，父边是第一个快照的 id。
    #[test]
    fn snapshot_second_is_incremental_linked_to_first() {
        let (mut store, scope) = store_with_scope();
        let tx = store.conn_mut().transaction().expect("应当可以开事务");
        let first = create_in(&tx, &scope, &[]).expect("应当成功");
        let second = create_in(&tx, &scope, &[]).expect("应当成功");
        tx.commit().expect("应当可提交");
        assert_eq!(second.seq, 2, "第二个快照的 seq 应当是 2");
        let (kind, seq, parent) = snapshot_row(&store, &second.id);
        assert_eq!(kind, "incremental", "第二个快照应当是增量");
        assert_eq!(seq, 2, "seq 应当是 2");
        assert_eq!(parent.as_deref(), Some(first.id.as_str()), "父边应当是基线的 id");
    }

    /// 快照 seq 从表内最大值加一分配：预置一条 seq = 100 的行，再创建得到 101。
    #[test]
    fn snapshot_seq_comes_from_table_max_plus_one() {
        let (mut store, scope) = store_with_scope();
        store
            .conn_for_test()
            .execute(
                "INSERT INTO snapshots (id, workspace_id, seq, parent_id, kind, created_at)
                 VALUES (?1, ?2, 100, NULL, 'baseline', 0)",
                rusqlite::params!["01ARZ3NDEKTSV4RRFFQ69G5FB0", WS],
            )
            .expect("应当可插");
        let tx = store.conn_mut().transaction().expect("应当可以开事务");
        let created = create_in(&tx, &scope, &[]).expect("应当成功");
        tx.commit().expect("应当可提交");
        assert_eq!(created.seq, 101, "应当是 MAX(seq)+1 = 101，而不是 2");
    }

    /// 读回元信息与插入一致，跨工作区读被拒。
    #[test]
    fn snapshot_read_meta_matches_inserted_and_cross_workspace_rejected() {
        let (mut store, scope) = store_with_scope();
        store
            .create_workspace(
                &WorkspaceId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAW").expect("应当合法"),
                "/tmp/b",
            )
            .expect("应当成功");
        let created = store.create_snapshot(&scope, &[]).expect("应当成功");
        let info = store.snapshot(&scope, &created.id).expect("应当可读");
        assert_eq!(info.id, created.id, "id 应当与返回值一致");
        assert_eq!(info.seq, created.seq, "seq 应当与返回值一致");
        assert!(info.parent.is_none(), "首个快照没有父");
        assert_eq!(info.kind, "baseline", "首个快照是基线");
        assert!(info.created_at > 0, "创建时间应当非空");
        // 跨工作区读被拒。
        let scope_b = store
            .scope(&WorkspaceId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAW").expect("应当合法"))
            .expect("应当可以取作用域");
        let err = store.snapshot(&scope_b, &created.id).expect_err("应当被拒");
        assert!(matches!(err, StoreError::UnknownSnapshot(_)), "应当是 UnknownSnapshot");
    }
}
