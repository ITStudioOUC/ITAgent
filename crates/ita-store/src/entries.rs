//! 快照条目行操作：形状校验、blobs 断言、插入与读清单。
//!
//! 条目是三种 kind 的枚举，非法形状在类型层面不可表示：墓碑变体没有 `content_stored`、
//! `mode` 与 `mtime` 字段，目录变体没有哈希与尺寸，只有文件变体带内容标记。唯一无法在
//! 类型层面做不可表示的形状——文件条目 `content_stored = 0` 却缺 `mtime`——由写入路径以
//! 校验错误拒绝，而不是落进表结构的 CHECK 兜底。
//!
//! blobs 断言在写入路径做，不加外键：超阈值条目按内容算出哈希但正文不进内容存储，加外键
//! 会把正常条目判成悬空。
//!
//! 吃一个已开启的事务或连接，不自行开事务。

use ita_kernel::context::CheckpointId;
use rusqlite::{Connection, Transaction};

use crate::blobs;
use crate::db::StoreError;
use crate::scope::WorkspaceScope;

/// 一条快照条目。三种 kind 对应表里 `kind` 列的三个取值，非法形状在类型层面不可表示。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotEntry {
    /// 当时存在的文件。正文可能已进内容存储（`content_stored = true`），也可能超阈值未存
    /// （`false`）。
    File {
        /// 工作区内声明的路径。
        path: String,
        /// 内容哈希。超阈值条目按内容算出哈希，但正文不进内容存储。
        hash: String,
        /// 正文字节数。
        size: u64,
        /// 权限位。
        mode: u32,
        /// 正文是否已进内容存储。
        content_stored: bool,
        /// 修改时间（epoch 毫秒）。正文未存时必填：回滚只报告不写回，残留清单靠它判断文件
        /// 在快照之后动过没有。
        mtime: Option<i64>,
    },
    /// 当时存在的目录。有权限位与修改时间，没有哈希与尺寸。
    Dir {
        /// 工作区内声明的路径。
        path: String,
        /// 权限位。
        mode: u32,
        /// 修改时间（epoch 毫秒）。
        mtime: i64,
    },
    /// 墓碑：当时被删除，那里什么都没有。
    Tombstone {
        /// 工作区内声明的路径。
        path: String,
    },
}

/// 在已开启的事务内插入一条条目。
///
/// # 步骤
/// 1. 校验形状：文件条目 `content_stored = 0` 时必须带 `mtime`。
/// 2. 断言 blobs：`content_stored = 1` 的条目其哈希必须在 `blobs` 里。
/// 3. 插入条目行。
///
/// # 边界与前提
/// 接收已开启的事务，不自行开事务。形状校验失败或 blobs 断言失败时返回错误，事务回滚，
/// 不落任何行。
///
/// # 副作用
/// 写 `snapshot_entries` 表。
pub(crate) fn insert_in(
    tx: &Transaction,
    snapshot_id: &CheckpointId,
    entry: &SnapshotEntry,
) -> Result<(), StoreError> {
    match entry {
        SnapshotEntry::File { path, hash, size, mode, content_stored, mtime } => {
            // 形状校验：超阈值条目必须带修改时间。回滚只报告不写回，残留清单靠它判断文件在
            // 快照之后动过没有。
            if !content_stored && mtime.is_none() {
                return Err(StoreError::InvalidEntryShape(format!(
                    "路径 {path} 的条目 content_stored = 0 却缺 mtime：内容未存时修改时间是残留清单的唯一依据"
                )));
            }

            // blobs 断言：正文已存的条目其哈希必须在 blobs 里。这是写入路径的断言而不是
            // 外键——超阈值条目带哈希却没有 blob，外键会把它们判成悬空。
            if *content_stored && !blobs::exists(tx, hash)? {
                return Err(StoreError::MissingBlob(hash.clone()));
            }

            tx.execute(
                "INSERT INTO snapshot_entries
                     (snapshot_id, path, kind, hash, mode, size, mtime, content_stored)
                 VALUES (?1, ?2, 'file', ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    snapshot_id.as_str(),
                    path,
                    hash,
                    mode,
                    size,
                    mtime,
                    if *content_stored { 1 } else { 0 },
                ],
            )?;
        }
        SnapshotEntry::Dir { path, mode, mtime } => {
            tx.execute(
                "INSERT INTO snapshot_entries
                     (snapshot_id, path, kind, hash, mode, size, mtime, content_stored)
                 VALUES (?1, ?2, 'dir', NULL, ?3, NULL, ?4, NULL)",
                rusqlite::params![snapshot_id.as_str(), path, mode, mtime],
            )?;
        }
        SnapshotEntry::Tombstone { path } => {
            tx.execute(
                "INSERT INTO snapshot_entries
                     (snapshot_id, path, kind, hash, mode, size, mtime, content_stored)
                 VALUES (?1, ?2, 'tombstone', NULL, NULL, NULL, NULL, NULL)",
                rusqlite::params![snapshot_id.as_str(), path],
            )?;
        }
    }
    Ok(())
}

/// 在连接内读回一个快照的全部条目，按路径排序。
///
/// # 边界与前提
/// 只读，不自行开事务。快照不存在或属于另一工作区返回 [`StoreError::UnknownSnapshot`]。
/// `snapshot_entries` 表不带工作区列，跨工作区校验必须 join 回 `snapshots`。
///
/// # 副作用
/// 只读。
pub(crate) fn read_in(
    conn: &Connection,
    scope: &WorkspaceScope,
    snapshot_id: &CheckpointId,
) -> Result<Vec<SnapshotEntry>, StoreError> {
    let belongs: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM snapshots WHERE id = ?1 AND workspace_id = ?2)",
        rusqlite::params![snapshot_id.as_str(), scope.workspace().as_str()],
        |row| row.get(0),
    )?;
    if !belongs {
        return Err(StoreError::UnknownSnapshot(snapshot_id.clone()));
    }

    let mut stmt = conn.prepare(
        "SELECT path, kind, hash, mode, size, mtime, content_stored
         FROM snapshot_entries
         WHERE snapshot_id = ?1
         ORDER BY path ASC",
    )?;
    let rows = stmt.query_map(rusqlite::params![snapshot_id.as_str()], row_to_entry)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(StoreError::from)
}

/// 把一行映射回条目。形状由写入路径与表的 CHECK 保证，必填列缺失属于数据缺陷，panic 是
/// 正确响应。
fn row_to_entry(row: &rusqlite::Row) -> rusqlite::Result<SnapshotEntry> {
    let path: String = row.get(0)?;
    let kind: String = row.get(1)?;
    match kind.as_str() {
        "file" => Ok(SnapshotEntry::File {
            path,
            hash: row.get(2)?,
            mode: row.get::<_, Option<u32>>(3)?.expect("file 条目必须带 mode"),
            size: row.get(4)?,
            mtime: row.get(5)?,
            content_stored: row
                .get::<_, Option<i64>>(6)?
                .expect("file 条目必须带 content_stored")
                .ne(&0),
        }),
        "dir" => Ok(SnapshotEntry::Dir {
            path,
            mode: row.get::<_, Option<u32>>(3)?.expect("dir 条目必须带 mode"),
            mtime: row.get::<_, Option<i64>>(5)?.expect("dir 条目必须带 mtime"),
        }),
        "tombstone" => Ok(SnapshotEntry::Tombstone { path }),
        other => Err(rusqlite::Error::InvalidParameterName(format!(
            "kind {other} 不是 file、dir 或 tombstone"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Store;
    use ita_kernel::context::WorkspaceId;

    const WS: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    const SNAP: &str = "01ARZ3NDEKTSV4RRFFQ69G5FB0";

    fn checkpoint(text: &str) -> CheckpointId {
        CheckpointId::parse(text).expect("应当合法")
    }

    /// 打开内存库，注册工作区与一条快照行。
    fn store_with_snapshot() -> Store {
        let mut store = Store::open_in_memory().expect("应当可以打开");
        let id = WorkspaceId::parse(WS).expect("应当合法");
        store.create_workspace(&id, "/tmp/ws").expect("应当成功");
        store
            .conn_for_test()
            .execute(
                "INSERT INTO snapshots (id, workspace_id, seq, parent_id, kind, created_at)
                 VALUES (?1, ?2, 1, NULL, 'baseline', 0)",
                rusqlite::params![SNAP, WS],
            )
            .expect("应当可插");
        store
    }

    /// 读回 `snapshot_entries` 表的行数。
    fn entry_rows(store: &Store) -> u32 {
        store
            .conn_for_test()
            .query_row("SELECT COUNT(*) FROM snapshot_entries", [], |row| row.get(0))
            .expect("应当可读")
    }

    /// 超阈值文件条目缺 mtime 被写入路径的校验错误拒绝，而不是 CHECK 兜底。
    #[test]
    fn entry_over_threshold_without_mtime_is_rejected() {
        let mut store = store_with_snapshot();
        let tx = store.conn_mut().transaction().expect("应当可以开事务");
        let err = insert_in(
            &tx,
            &checkpoint(SNAP),
            &SnapshotEntry::File {
                path: "big.bin".into(),
                hash: "ee".repeat(32),
                size: 1,
                mode: 0o644,
                content_stored: false,
                mtime: None,
            },
        )
        .expect_err("应当被拒");
        assert!(matches!(err, StoreError::InvalidEntryShape(_)), "应当是写入路径的校验错误");
        drop(tx);
        assert_eq!(entry_rows(&store), 0, "被拒后条目行没有落");
    }

    /// 正文已存但 blobs 里没有该哈希的条目被拒，返回 MissingBlob。
    #[test]
    fn entry_stored_without_blob_is_rejected() {
        let mut store = store_with_snapshot();
        let tx = store.conn_mut().transaction().expect("应当可以开事务");
        let err = insert_in(
            &tx,
            &checkpoint(SNAP),
            &SnapshotEntry::File {
                path: "orphan.txt".into(),
                hash: "ab".repeat(32),
                size: 10,
                mode: 0o644,
                content_stored: true,
                mtime: None,
            },
        )
        .expect_err("应当被拒");
        assert!(matches!(err, StoreError::MissingBlob(_)), "应当是 MissingBlob");
        drop(tx);
        assert_eq!(entry_rows(&store), 0, "被拒后条目行没有落");
    }

    /// 正文已存且 blobs 里有该哈希的条目插入成功，content_stored 落 1。
    #[test]
    fn entry_stored_with_blob_is_inserted() {
        let mut store = store_with_snapshot();
        let hash = "aa".repeat(32);
        crate::blobs::insert(store.conn(), &hash, 10, 1).expect("应当可插");
        let tx = store.conn_mut().transaction().expect("应当可以开事务");
        insert_in(
            &tx,
            &checkpoint(SNAP),
            &SnapshotEntry::File {
                path: "a.txt".into(),
                hash,
                size: 10,
                mode: 0o644,
                content_stored: true,
                mtime: None,
            },
        )
        .expect("应当可插");
        tx.commit().expect("应当可提交");
        let content_stored: Option<i64> = store
            .conn_for_test()
            .query_row(
                "SELECT content_stored FROM snapshot_entries WHERE path = 'a.txt'",
                [],
                |row| row.get(0),
            )
            .expect("应当可读");
        assert_eq!(content_stored, Some(1), "content_stored 应当落 1");
    }

    /// 墓碑条目五列全空。
    #[test]
    fn entry_tombstone_inserts_all_null_columns() {
        let mut store = store_with_snapshot();
        let tx = store.conn_mut().transaction().expect("应当可以开事务");
        insert_in(&tx, &checkpoint(SNAP), &SnapshotEntry::Tombstone { path: "gone.txt".into() })
            .expect("应当可插");
        tx.commit().expect("应当可提交");
        let non_null: i64 = store
            .conn_for_test()
            .query_row(
                "SELECT (hash IS NOT NULL) + (mode IS NOT NULL) + (size IS NOT NULL)
                       + (mtime IS NOT NULL) + (content_stored IS NOT NULL)
                 FROM snapshot_entries WHERE path = 'gone.txt'",
                [],
                |row| row.get(0),
            )
            .expect("应当可读");
        assert_eq!(non_null, 0, "五列应当全空");
    }

    /// 同一快照内同一路径的两条条目，第二条被主键拒绝。
    #[test]
    fn entry_duplicate_path_is_rejected() {
        let mut store = store_with_snapshot();
        let tx = store.conn_mut().transaction().expect("应当可以开事务");
        let entry = SnapshotEntry::Tombstone { path: "dup.txt".into() };
        insert_in(&tx, &checkpoint(SNAP), &entry).expect("首次应当可插");
        let err = insert_in(&tx, &checkpoint(SNAP), &entry).expect_err("重复应当被拒");
        assert!(matches!(err, StoreError::Sqlite(_)), "应当是主键冲突");
        drop(tx);
        assert_eq!(entry_rows(&store), 0, "回滚后条目行没有落");
    }
}
