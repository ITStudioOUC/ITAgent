//! 快照清单创建与读取的集成测试。
//!
//! 这些用例走公开接口，覆盖计划第四节的十三条验收。第 1 至 3 条守快照链的 seq 与父边，
//! 第 4 至 8 条守条目形状的三种 kind，第 9 条守 blobs 断言，第 10 条守条目主键，第 11 与
//! 12 条守读路径，第 13 条守空清单的合法性。

use ita_kernel::context::WorkspaceId;
use ita_store::{SnapshotEntry, Store, StoreError};

const WS_A: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const WS_B: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";

fn workspace(text: &str) -> WorkspaceId {
    WorkspaceId::parse(text).expect("应当合法")
}

/// 打开一个库并注册工作区 A。
fn store_with_a() -> Store {
    let mut store = Store::open_in_memory().expect("应当可以打开");
    store.create_workspace(&workspace(WS_A), "/tmp/a").expect("应当成功");
    store
}

/// 读回快照行：(kind, seq, parent_id, created_at)。
fn snapshot_row(conn: &rusqlite::Connection, id: &str) -> (String, i64, Option<String>, i64) {
    conn.query_row(
        "SELECT kind, seq, parent_id, created_at FROM snapshots WHERE id = ?1",
        rusqlite::params![id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )
    .expect("应当可读")
}

/// 读回的一条条目行。
struct EntryRow {
    kind: String,
    hash: Option<String>,
    mode: Option<i32>,
    size: Option<i64>,
    mtime: Option<i64>,
    content_stored: Option<i64>,
}

/// 读回条目行。
fn entry_row(conn: &rusqlite::Connection, snapshot_id: &str, path: &str) -> EntryRow {
    conn.query_row(
        "SELECT kind, hash, mode, size, mtime, content_stored
         FROM snapshot_entries WHERE snapshot_id = ?1 AND path = ?2",
        rusqlite::params![snapshot_id, path],
        |row| {
            Ok(EntryRow {
                kind: row.get(0)?,
                hash: row.get(1)?,
                mode: row.get(2)?,
                size: row.get(3)?,
                mtime: row.get(4)?,
                content_stored: row.get(5)?,
            })
        },
    )
    .expect("应当可读")
}

/// 读回 `snapshots` 表的行数。
fn snapshot_count(conn: &rusqlite::Connection) -> u32 {
    conn.query_row("SELECT COUNT(*) FROM snapshots", [], |row| row.get(0)).expect("应当可读")
}

/// 读回 `snapshot_entries` 表的行数。
fn entry_count(conn: &rusqlite::Connection) -> u32 {
    conn.query_row("SELECT COUNT(*) FROM snapshot_entries", [], |row| row.get(0)).expect("应当可读")
}

/// 工作区的首个快照是基线：seq 为 1，父边为空。
#[test]
fn first_snapshot_is_baseline_with_seq_one_and_null_parent() {
    let mut store = store_with_a();
    let scope = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    let created = store.create_snapshot(&scope, &[]).expect("应当成功");
    assert_eq!(created.seq, 1, "首个快照的 seq 应当是 1");
    let (kind, seq, parent, created_at) = snapshot_row(store.conn_for_test(), created.id.as_str());
    assert_eq!(kind, "baseline", "首个快照应当是基线");
    assert_eq!(seq, 1, "seq 应当是 1");
    assert!(parent.is_none(), "父边应当为空");
    assert!(created_at > 0, "创建时间应当非空");
}

/// 第二个快照是增量：seq 为 2，父边是第一个快照的 id。
#[test]
fn second_snapshot_is_incremental_linked_to_first() {
    let mut store = store_with_a();
    let scope = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    let first = store.create_snapshot(&scope, &[]).expect("应当成功");
    let second = store.create_snapshot(&scope, &[]).expect("应当成功");
    assert_eq!(second.seq, 2, "第二个快照的 seq 应当是 2");
    let (kind, seq, parent, _) = snapshot_row(store.conn_for_test(), second.id.as_str());
    assert_eq!(kind, "incremental", "第二个快照应当是增量");
    assert_eq!(seq, 2, "seq 应当是 2");
    assert_eq!(parent.as_deref(), Some(first.id.as_str()), "父边应当是基线的 id，而不是 NULL");
}

/// 快照 seq 从表内最大值加一分配，不是从计数器分配：预置一条 seq = 100 的行，走公开入口
/// 创建得到 101，而不是 2。
#[test]
fn seq_comes_from_table_max_plus_one_not_counter() {
    let mut store = store_with_a();
    let scope = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    store
        .conn_for_test()
        .execute(
            "INSERT INTO snapshots (id, workspace_id, seq, parent_id, kind, created_at)
             VALUES (?1, ?2, 100, NULL, 'baseline', 0)",
            rusqlite::params!["01ARZ3NDEKTSV4RRFFQ69G5FB0", WS_A],
        )
        .expect("应当可插");
    let created = store.create_snapshot(&scope, &[]).expect("应当成功");
    assert_eq!(created.seq, 101, "应当是 MAX(seq)+1 = 101，而不是 2");
}

/// 文件条目带 content_stored = 1 时，hash、size、mode 写入，mtime 可空。
#[test]
fn file_entry_stored_writes_hash_size_mode_and_null_mtime() {
    let mut store = store_with_a();
    let scope = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    let content = b"snapshot body";
    let blob = store.put(content).expect("应当成功");
    let created = store
        .create_snapshot(
            &scope,
            &[SnapshotEntry::File {
                path: "docs/readme.md".into(),
                hash: blob.hash.clone(),
                size: content.len() as u64,
                mode: 0o644,
                content_stored: true,
                mtime: None,
            }],
        )
        .expect("应当成功");
    let row = entry_row(store.conn_for_test(), created.id.as_str(), "docs/readme.md");
    assert_eq!(row.kind, "file", "kind 应当是 file");
    assert_eq!(row.hash.as_deref(), Some(blob.hash.as_str()), "hash 应当是 blob 的哈希");
    assert_eq!(row.mode, Some(0o644), "mode 应当非空");
    assert_eq!(row.size, Some(content.len() as i64), "size 应当是内容字节数");
    assert!(row.mtime.is_none(), "正文已存时 mtime 应当为 NULL");
    assert_eq!(row.content_stored, Some(1), "content_stored 应当是 1");
}

/// 文件条目带 content_stored = 0 时，hash、size、mtime 必填；缺 mtime 被写入路径的校验
/// 错误拒绝。
#[test]
fn file_entry_over_threshold_requires_mtime() {
    let mut store = store_with_a();
    let scope = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    // 带 mtime 的超阈值条目成功。
    let created = store
        .create_snapshot(
            &scope,
            &[SnapshotEntry::File {
                path: "large.bin".into(),
                hash: "ee".repeat(32),
                size: 2_000_000,
                mode: 0o644,
                content_stored: false,
                mtime: Some(1_700_000_000_000),
            }],
        )
        .expect("应当成功");
    let row = entry_row(store.conn_for_test(), created.id.as_str(), "large.bin");
    assert_eq!(row.kind, "file", "kind 应当是 file");
    assert_eq!(row.hash.as_deref(), Some("ee".repeat(32).as_str()), "hash 应当非空");
    assert_eq!(row.mode, Some(0o644), "mode 应当非空");
    assert_eq!(row.size, Some(2_000_000), "size 应当非空");
    assert_eq!(row.mtime, Some(1_700_000_000_000), "mtime 应当非空");
    assert_eq!(row.content_stored, Some(0), "content_stored 应当是 0");

    // 缺 mtime 的超阈值条目被拒，且错误是写入路径的校验错误而不是 CHECK。
    let err = store
        .create_snapshot(
            &scope,
            &[SnapshotEntry::File {
                path: "large2.bin".into(),
                hash: "ff".repeat(32),
                size: 3_000_000,
                mode: 0o644,
                content_stored: false,
                mtime: None,
            }],
        )
        .expect_err("应当被拒");
    assert!(matches!(err, StoreError::InvalidEntryShape(_)), "应当是写入路径的校验错误");
}

/// 目录条目写入 mode 与 mtime，hash、size、content_stored 三列全 NULL。
#[test]
fn dir_entry_writes_mode_and_mtime_with_null_content_columns() {
    let mut store = store_with_a();
    let scope = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    let created = store
        .create_snapshot(
            &scope,
            &[SnapshotEntry::Dir { path: "src".into(), mode: 0o755, mtime: 1_700_000_000_000 }],
        )
        .expect("应当成功");
    let row = entry_row(store.conn_for_test(), created.id.as_str(), "src");
    assert_eq!(row.kind, "dir", "kind 应当是 dir");
    assert_eq!(row.mode, Some(0o755), "mode 应当非空");
    assert_eq!(row.mtime, Some(1_700_000_000_000), "mtime 应当非空");
    assert!(row.hash.is_none(), "hash 应当为 NULL");
    assert!(row.size.is_none(), "size 应当为 NULL");
    assert!(row.content_stored.is_none(), "content_stored 应当为 NULL");
}

/// 墓碑条目五列全空。
#[test]
fn tombstone_entry_has_all_five_columns_null() {
    let mut store = store_with_a();
    let scope = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    let created = store
        .create_snapshot(&scope, &[SnapshotEntry::Tombstone { path: "gone.txt".into() }])
        .expect("应当成功");
    let row = entry_row(store.conn_for_test(), created.id.as_str(), "gone.txt");
    assert_eq!(row.kind, "tombstone", "kind 应当是 tombstone");
    assert!(row.hash.is_none(), "hash 应当为 NULL");
    assert!(row.mode.is_none(), "mode 应当为 NULL");
    assert!(row.size.is_none(), "size 应当为 NULL");
    assert!(row.mtime.is_none(), "mtime 应当为 NULL");
    assert!(row.content_stored.is_none(), "content_stored 应当为 NULL");
}

/// 删除的声明路径留下墓碑：基线声明一个文件，增量对同一路径写墓碑。
#[test]
fn deleted_declared_path_leaves_tombstone() {
    let mut store = store_with_a();
    let scope = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    let content = b"declared then deleted";
    let blob = store.put(content).expect("应当成功");
    // 基线声明文件。
    store
        .create_snapshot(
            &scope,
            &[SnapshotEntry::File {
                path: "docs/old.md".into(),
                hash: blob.hash.clone(),
                size: content.len() as u64,
                mode: 0o644,
                content_stored: true,
                mtime: None,
            }],
        )
        .expect("应当成功");
    // 增量删除它。
    let second = store
        .create_snapshot(&scope, &[SnapshotEntry::Tombstone { path: "docs/old.md".into() }])
        .expect("应当成功");
    let row = entry_row(store.conn_for_test(), second.id.as_str(), "docs/old.md");
    assert_eq!(row.kind, "tombstone", "被删除路径的条目应当是墓碑");
}

/// content_stored = 1 的条目其哈希不在 blobs 里时被拒，整笔回滚。
#[test]
fn content_stored_entry_missing_blob_is_rejected_and_rolled_back() {
    let mut store = store_with_a();
    let scope = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    // 哈希没有先 put 进 blobs。
    let err = store
        .create_snapshot(
            &scope,
            &[SnapshotEntry::File {
                path: "orphan.txt".into(),
                hash: "ab".repeat(32),
                size: 10,
                mode: 0o644,
                content_stored: true,
                mtime: None,
            }],
        )
        .expect_err("应当被拒");
    assert!(matches!(err, StoreError::MissingBlob(_)), "应当是 MissingBlob");
    assert_eq!(snapshot_count(store.conn_for_test()), 0, "snapshots 表没有新增行");
    assert_eq!(entry_count(store.conn_for_test()), 0, "snapshot_entries 表没有新增行");
}

/// 同一快照内同一路径不能有两条：第二条被拒，整笔回滚。
#[test]
fn duplicate_path_in_one_snapshot_is_rejected() {
    let mut store = store_with_a();
    let scope = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    let content = b"same path twice";
    let blob = store.put(content).expect("应当成功");
    let entry = SnapshotEntry::File {
        path: "twice.txt".into(),
        hash: blob.hash.clone(),
        size: content.len() as u64,
        mode: 0o644,
        content_stored: true,
        mtime: None,
    };
    let err = store.create_snapshot(&scope, &[entry.clone(), entry]).expect_err("第二条应当被拒");
    assert!(matches!(err, StoreError::Sqlite(_)), "应当是主键冲突");
    assert_eq!(snapshot_count(store.conn_for_test()), 0, "snapshots 表没有新增行");
    assert_eq!(entry_count(store.conn_for_test()), 0, "snapshot_entries 表没有新增行");
}

/// 读回清单与写入一致：每条的 kind、hash、size、mode、mtime、content_stored 逐字段相等。
#[test]
fn read_back_entries_match_written_entries() {
    let mut store = store_with_a();
    let scope = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    let content = b"body to read back";
    let blob = store.put(content).expect("应当成功");
    let entries = vec![
        SnapshotEntry::File {
            path: "b.txt".into(),
            hash: blob.hash.clone(),
            size: content.len() as u64,
            mode: 0o644,
            content_stored: true,
            mtime: None,
        },
        SnapshotEntry::File {
            path: "a-large.bin".into(),
            hash: "cd".repeat(32),
            size: 4_000_000,
            mode: 0o600,
            content_stored: false,
            mtime: Some(1_700_000_001_000),
        },
        SnapshotEntry::Dir { path: "src".into(), mode: 0o755, mtime: 1_700_000_002_000 },
        SnapshotEntry::Tombstone { path: "z-gone.txt".into() },
    ];
    let created = store.create_snapshot(&scope, &entries).expect("应当成功");
    let read = store.snapshot_entries(&scope, &created.id).expect("应当可读");
    // 读路径按路径排序，顺序是 a-large.bin、b.txt、src、z-gone.txt。
    assert_eq!(read.len(), 4, "应当读出四条");
    assert_eq!(read[0], entries[1], "超阈值条目应当逐字段相等");
    assert_eq!(read[1], entries[0], "正文已存条目应当逐字段相等");
    assert_eq!(read[2], entries[2], "目录条目应当逐字段相等");
    assert_eq!(read[3], entries[3], "墓碑条目应当逐字段相等");
}

/// 跨工作区读快照被拒：工作区 A 创建的快照用工作区 B 的作用域读，返回 UnknownSnapshot。
#[test]
fn cross_workspace_snapshot_read_is_rejected() {
    let mut store = store_with_a();
    store.create_workspace(&workspace(WS_B), "/tmp/b").expect("应当成功");
    let scope_a = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    let scope_b = store.scope(&workspace(WS_B)).expect("应当可以取作用域");
    let created = store.create_snapshot(&scope_a, &[]).expect("应当成功");
    // 用 B 的作用域读元信息被拒。
    let err = store.snapshot(&scope_b, &created.id).expect_err("应当被拒");
    assert!(matches!(err, StoreError::UnknownSnapshot(_)), "应当是 UnknownSnapshot");
    // 用 B 的作用域读清单同样被拒。
    let err = store.snapshot_entries(&scope_b, &created.id).expect_err("应当被拒");
    assert!(matches!(err, StoreError::UnknownSnapshot(_)), "应当是 UnknownSnapshot");
    // 用 A 的作用域读通过。
    assert_eq!(
        store.snapshot(&scope_a, &created.id).expect("应当可读").seq,
        1,
        "同工作区读应当通过"
    );
    assert_eq!(
        store.snapshot_entries(&scope_a, &created.id).expect("应当可读").len(),
        0,
        "同工作区读清单应当通过"
    );
}

/// 空清单允许：快照成功，snapshot_entries 表里没有新增行。
#[test]
fn empty_manifest_is_allowed() {
    let mut store = store_with_a();
    let scope = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    let created = store.create_snapshot(&scope, &[]).expect("应当成功");
    assert_eq!(snapshot_count(store.conn_for_test()), 1, "快照行应当插入");
    assert_eq!(entry_count(store.conn_for_test()), 0, "snapshot_entries 表没有新增行");
    assert_eq!(
        store.snapshot_entries(&scope, &created.id).expect("应当可读").len(),
        0,
        "读路径应当读出空清单"
    );
}
