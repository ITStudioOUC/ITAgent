//! 基线加增量 squash 的集成测试。
//!
//! 这些用例走公开接口，覆盖计划第四节的第 1 至 8 条验收。第 1 与 2 条守触发条件，第 3、4、7、8
//! 条守折叠算法与"有效条目不变"不变量，第 5 与 6 条守重挂顺序与跨工作区校验。
//!
//! 有效条目不变量的测试需要折叠整条链，而折叠是 crate 内部入口，因此测试自己实现折叠作为
//! 参照：左结合的覆盖加墓碑删除。

use std::collections::BTreeMap;

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

/// 读回快照行：(kind, seq, parent_id)。
fn snapshot_row(conn: &rusqlite::Connection, id: &str) -> (String, i64, Option<String>) {
    conn.query_row(
        "SELECT kind, seq, parent_id FROM snapshots WHERE id = ?1",
        rusqlite::params![id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
    .expect("应当可读")
}

/// 读回 `snapshots` 表的行数。
fn snapshot_count(conn: &rusqlite::Connection) -> u32 {
    conn.query_row("SELECT COUNT(*) FROM snapshots", [], |row| row.get(0)).expect("应当可读")
}

/// 读回某工作区全部快照的 id，按 seq 升序。
fn snapshot_ids(conn: &rusqlite::Connection, ws: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT id FROM snapshots WHERE workspace_id = ?1 ORDER BY seq ASC")
        .expect("应当可准备");
    stmt.query_map(rusqlite::params![ws], |row| row.get(0))
        .expect("应当可查")
        .collect::<Result<Vec<_>, _>>()
        .expect("应当可读")
}

/// 读回某快照的全部条目，按路径排序。
fn manifest(conn: &rusqlite::Connection, id: &str) -> Vec<SnapshotEntry> {
    let mut stmt = conn
        .prepare(
            "SELECT path, kind, hash, mode, size, mtime, content_stored
             FROM snapshot_entries WHERE snapshot_id = ?1 ORDER BY path ASC",
        )
        .expect("应当可准备");
    let rows = stmt.query_map(rusqlite::params![id], row_to_entry).expect("应当可查");
    rows.collect::<Result<Vec<_>, _>>().expect("应当可读")
}

/// 把一行映射回条目。
fn row_to_entry(row: &rusqlite::Row) -> rusqlite::Result<SnapshotEntry> {
    let path: String = row.get(0)?;
    let kind: String = row.get(1)?;
    match kind.as_str() {
        "file" => Ok(SnapshotEntry::File {
            path,
            hash: row.get(2)?,
            mode: row.get(3)?,
            size: row.get(4)?,
            mtime: row.get(5)?,
            content_stored: row.get::<_, Option<i64>>(6)?.map(|v| v != 0).unwrap_or(false),
        }),
        "dir" => Ok(SnapshotEntry::Dir { path, mode: row.get(3)?, mtime: row.get(5)? }),
        "tombstone" => Ok(SnapshotEntry::Tombstone { path }),
        other => Err(rusqlite::Error::InvalidParameterName(format!(
            "kind {other} 不是 file、dir 或 tombstone"
        ))),
    }
}

/// 参照折叠：左结合的覆盖加墓碑删除。测试用它计算有效条目不变量的两侧。
fn fold(manifests: &[Vec<SnapshotEntry>]) -> Vec<SnapshotEntry> {
    let mut map: BTreeMap<String, SnapshotEntry> = BTreeMap::new();
    for manifest in manifests {
        for entry in manifest {
            match entry {
                SnapshotEntry::Tombstone { path } => {
                    map.remove(path);
                }
                other => {
                    let path = match other {
                        SnapshotEntry::File { path, .. } => path,
                        SnapshotEntry::Dir { path, .. } => path,
                        _ => unreachable!(),
                    };
                    map.insert(path.clone(), other.clone());
                }
            }
        }
    }
    map.into_values().collect()
}

/// 折叠整条链得到有效状态。
fn effective_state(conn: &rusqlite::Connection, ws: &str) -> Vec<SnapshotEntry> {
    let ids = snapshot_ids(conn, ws);
    let manifests: Vec<Vec<SnapshotEntry>> = ids.iter().map(|id| manifest(conn, id)).collect();
    fold(&manifests)
}

/// 创建一个文件条目，正文已存。
fn file_entry(store: &mut Store, path: &str) -> SnapshotEntry {
    let content = format!("body of {path}");
    let blob = store.put(content.as_bytes()).expect("应当成功");
    SnapshotEntry::File {
        path: path.into(),
        hash: blob.hash,
        size: content.len() as u64,
        mode: 0o644,
        content_stored: true,
        mtime: None,
    }
}

/// 创建一个基线加 `n` 个增量，每个增量声明一个唯一路径的文件。返回基线 id 与全部增量 id。
fn create_chain(
    store: &mut Store,
    scope: &ita_store::WorkspaceScope,
    n: usize,
) -> (String, Vec<String>) {
    let base = file_entry(store, "base.txt");
    let baseline = store.create_snapshot(scope, &[base]).expect("应当成功");
    let mut ids = Vec::new();
    for i in 0..n {
        let entry = file_entry(store, &format!("file_{i}.txt"));
        let created = store.create_snapshot(scope, &[entry]).expect("应当成功");
        ids.push(created.id.as_str().to_owned());
    }
    (baseline.id.as_str().to_owned(), ids)
}

/// 增量数未达保留窗口时 squash 是空操作：快照行数不变、基线条目不变、无错误。
#[test]
fn squash_is_noop_below_window() {
    let mut store = store_with_a();
    let scope = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    let (baseline_id, _) = create_chain(&mut store, &scope, 63);
    let before = snapshot_count(store.conn_for_test());
    let baseline_entries_before = manifest(store.conn_for_test(), &baseline_id);
    store.squash_if_needed(&scope, 64).expect("应当成功");
    assert_eq!(snapshot_count(store.conn_for_test()), before, "快照行数应当不变");
    assert_eq!(
        manifest(store.conn_for_test(), &baseline_id),
        baseline_entries_before,
        "基线条目应当不变"
    );
}

/// 增量数达到保留窗口时最老两条并入基线：快照行数减二、最老两个增量的行被删、基线行仍在。
#[test]
fn squash_merges_two_oldest_at_window() {
    let mut store = store_with_a();
    let scope = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    let (baseline_id, ids) = create_chain(&mut store, &scope, 64);
    let before = snapshot_count(store.conn_for_test());
    store.squash_if_needed(&scope, 64).expect("应当成功");
    assert_eq!(snapshot_count(store.conn_for_test()), before - 2, "快照行数应当减二");
    // 最老两个增量的行被删。
    let i1 = &ids[0];
    let i2 = &ids[1];
    let i1_exists: bool = store
        .conn_for_test()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM snapshots WHERE id = ?1)",
            rusqlite::params![i1],
            |row| row.get(0),
        )
        .expect("应当可读");
    let i2_exists: bool = store
        .conn_for_test()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM snapshots WHERE id = ?1)",
            rusqlite::params![i2],
            |row| row.get(0),
        )
        .expect("应当可读");
    assert!(!i1_exists, "最老增量 I1 的行应当被删");
    assert!(!i2_exists, "第二老增量 I2 的行应当被删");
    // 基线行仍在。
    let (kind, _, _) = snapshot_row(store.conn_for_test(), &baseline_id);
    assert_eq!(kind, "baseline", "基线行应当仍在");
}

/// 合并后基线条目是折叠 B、I1、I2 的结果：按路径覆盖、墓碑删除。
#[test]
fn squash_baseline_entries_are_fold_of_b_i1_i2() {
    let mut store = store_with_a();
    let scope = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    let (baseline_id, ids) = create_chain(&mut store, &scope, 64);
    // 折叠 B、I1、I2 的清单。
    let b = manifest(store.conn_for_test(), &baseline_id);
    let i1 = manifest(store.conn_for_test(), &ids[0]);
    let i2 = manifest(store.conn_for_test(), &ids[1]);
    let expected = fold(&[b, i1, i2]);
    store.squash_if_needed(&scope, 64).expect("应当成功");
    let actual = manifest(store.conn_for_test(), &baseline_id);
    assert_eq!(actual, expected, "基线条目应当等于折叠 B、I1、I2 的结果");
}

/// 合并后最新快照的有效条目不变：折叠整条链前后逐条相等。
#[test]
fn squash_preserves_effective_entries_of_latest_snapshot() {
    let mut store = store_with_a();
    let scope = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    create_chain(&mut store, &scope, 64);
    let before = effective_state(store.conn_for_test(), WS_A);
    store.squash_if_needed(&scope, 64).expect("应当成功");
    let after = effective_state(store.conn_for_test(), WS_A);
    assert_eq!(before, after, "有效条目应当不变");
}

/// 合并事务先重挂子快照再删父：I3 的 parent_id 是基线的 id，I1 与 I2 的行被删，I3 的行仍在。
#[test]
fn squash_reattaches_child_before_deleting_parent() {
    let mut store = store_with_a();
    let scope = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    let (baseline_id, ids) = create_chain(&mut store, &scope, 64);
    let i3 = &ids[2];
    store.squash_if_needed(&scope, 64).expect("应当成功");
    let (_, _, parent) = snapshot_row(store.conn_for_test(), i3);
    assert_eq!(
        parent.as_deref(),
        Some(baseline_id.as_str()),
        "I3 的 parent_id 应当是基线的 id，而不是被删的 I2 的 id"
    );
    let i3_exists: bool = store
        .conn_for_test()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM snapshots WHERE id = ?1)",
            rusqlite::params![i3],
            |row| row.get(0),
        )
        .expect("应当可读");
    assert!(i3_exists, "I3 的行应当仍在");
}

/// 重挂时父子快照属于同一工作区：构造 I3 与基线属于不同工作区的坏链，squash 返回跨工作区
/// 错误、整笔回滚、I3 的 parent_id 不变。
#[test]
fn squash_rejects_cross_workspace_child() {
    let mut store = store_with_a();
    store.create_workspace(&workspace(WS_B), "/tmp/b").expect("应当成功");
    let scope_a = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    // 建 65 个增量：I3 被移到工作区 B 后，工作区 A 仍剩 64 个，触发窗口照常到达。
    let (baseline_id, ids) = create_chain(&mut store, &scope_a, 65);
    let i3 = &ids[2];
    // 用测试专用连接把 I3 的 workspace_id 改成工作区 B，构造坏链。
    store
        .conn_for_test()
        .execute(
            "UPDATE snapshots SET workspace_id = ?1 WHERE id = ?2",
            rusqlite::params![WS_B, i3],
        )
        .expect("应当可改");
    let err = store.squash_if_needed(&scope_a, 64).expect_err("应当被拒");
    assert!(matches!(err, StoreError::CrossWorkspace(_)), "应当是跨工作区错误");
    // 整笔回滚：I3 的 parent_id 不变（仍是 I2 的 id）。
    let (_, _, parent) = snapshot_row(store.conn_for_test(), i3);
    assert_eq!(parent.as_deref(), Some(ids[1].as_str()), "回滚后 I3 的 parent_id 应当不变");
    // 最老两个增量的行仍在。
    let i1_exists: bool = store
        .conn_for_test()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM snapshots WHERE id = ?1)",
            rusqlite::params![ids[0]],
            |row| row.get(0),
        )
        .expect("应当可读");
    assert!(i1_exists, "回滚后 I1 的行应当仍在");
    let _ = baseline_id;
}

/// 折叠结果不含墓碑：I1 对某路径写墓碑，squash 后新基线条目里没有该路径。
#[test]
fn squash_fold_result_contains_no_tombstones() {
    let mut store = store_with_a();
    let scope = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    // 基线声明一个文件，增量 I1 对它写墓碑。
    let content = b"declared then deleted";
    let blob = store.put(content).expect("应当成功");
    store
        .create_snapshot(
            &scope,
            &[SnapshotEntry::File {
                path: "gone.txt".into(),
                hash: blob.hash,
                size: content.len() as u64,
                mode: 0o644,
                content_stored: true,
                mtime: None,
            }],
        )
        .expect("应当成功");
    store
        .create_snapshot(&scope, &[SnapshotEntry::Tombstone { path: "gone.txt".into() }])
        .expect("应当成功");
    // 再加 63 个增量凑满 64（墓碑是第 1 个增量）。
    for i in 0..63 {
        let entry = file_entry(&mut store, &format!("file_{i}.txt"));
        store.create_snapshot(&scope, &[entry]).expect("应当成功");
    }
    store.squash_if_needed(&scope, 64).expect("应当成功");
    // 读基线条目，断言 gone.txt 不在结果里。
    let baseline_ids = snapshot_ids(store.conn_for_test(), WS_A);
    let baseline_id = &baseline_ids[0];
    let entries = manifest(store.conn_for_test(), baseline_id);
    assert!(
        !entries
            .iter()
            .any(|e| matches!(e, SnapshotEntry::Tombstone { path } if path == "gone.txt")),
        "新基线条目里不应有 gone.txt 的墓碑"
    );
    assert!(
        !entries
            .iter()
            .any(|e| matches!(e, SnapshotEntry::File { path, .. } if path == "gone.txt")),
        "新基线条目里不应有 gone.txt 的文件条目"
    );
}

/// 超阈值条目在折叠中保留原样：`content_stored = 0` 的条目折叠后仍是超阈值条目。
#[test]
fn squash_preserves_over_threshold_entries() {
    let mut store = store_with_a();
    let scope = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    // 基线声明一个超阈值文件。
    store
        .create_snapshot(
            &scope,
            &[SnapshotEntry::File {
                path: "big.bin".into(),
                hash: "ee".repeat(32),
                size: 2_000_000,
                mode: 0o644,
                content_stored: false,
                mtime: Some(1_700_000_000_000),
            }],
        )
        .expect("应当成功");
    // 加 64 个增量凑满窗口。
    for i in 0..64 {
        let entry = file_entry(&mut store, &format!("file_{i}.txt"));
        store.create_snapshot(&scope, &[entry]).expect("应当成功");
    }
    store.squash_if_needed(&scope, 64).expect("应当成功");
    // 读基线条目，断言 big.bin 仍是 content_stored = 0 且 hash、size、mtime 不变。
    let baseline_ids = snapshot_ids(store.conn_for_test(), WS_A);
    let baseline_id = &baseline_ids[0];
    let entries = manifest(store.conn_for_test(), baseline_id);
    let big = entries
        .iter()
        .find(|e| matches!(e, SnapshotEntry::File { path, .. } if path == "big.bin"))
        .expect("big.bin 应当在基线条目里");
    match big {
        SnapshotEntry::File { hash, size, content_stored, mtime, .. } => {
            assert_eq!(hash, &"ee".repeat(32), "hash 应当不变");
            assert_eq!(*size, 2_000_000, "size 应当不变");
            assert!(!content_stored, "content_stored 应当仍是 0");
            assert_eq!(*mtime, Some(1_700_000_000_000), "mtime 应当不变");
        }
        _ => panic!("big.bin 应当是文件条目"),
    }
}
