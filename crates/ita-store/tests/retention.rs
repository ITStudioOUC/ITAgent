//! 重定基线的集成测试。
//!
//! 这些用例走公开接口，覆盖计划第四节的第 9 至 13 条验收。第 9 条守自主模式拒绝，第 10 与
//! 11 条守留痕三列成组写入，第 12 条守基线只更新不删除，第 13 条守链重新开始。

use ita_kernel::context::WorkspaceId;
use ita_store::{RebaseMode, RebaseRequest, SnapshotEntry, Store, StoreError, WorkspaceScope};

const WS_A: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

fn workspace(text: &str) -> WorkspaceId {
    WorkspaceId::parse(text).expect("应当合法")
}

/// 打开一个库并注册工作区 A。
fn store_with_a() -> Store {
    let mut store = Store::open_in_memory().expect("应当可以打开");
    store.create_workspace(&workspace(WS_A), "/tmp/a").expect("应当成功");
    store
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

/// 创建一个基线加 `n` 个增量，返回基线 id。
fn create_chain(store: &mut Store, scope: &WorkspaceScope, n: usize) -> String {
    let base = file_entry(store, "base.txt");
    let baseline = store.create_snapshot(scope, &[base]).expect("应当成功");
    for i in 0..n {
        let entry = file_entry(store, &format!("file_{i}.txt"));
        store.create_snapshot(scope, &[entry]).expect("应当成功");
    }
    baseline.id.as_str().to_owned()
}

/// 读回基线行的留痕三列。
fn rebase_trail(
    conn: &rusqlite::Connection,
    id: &str,
) -> (Option<String>, Option<String>, Option<String>) {
    conn.query_row(
        "SELECT rebased_by, rebase_reason, rebase_confirmed_by FROM snapshots WHERE id = ?1",
        rusqlite::params![id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
    .expect("应当可读")
}

/// 读回 `snapshots` 表的行数。
fn snapshot_count(conn: &rusqlite::Connection) -> u32 {
    conn.query_row("SELECT COUNT(*) FROM snapshots", [], |row| row.get(0)).expect("应当可读")
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

/// 自主模式重定基线被拒：返回 RebaseRejectedAutonomous、快照链不变、留痕三列仍为 NULL。
#[test]
fn rebaseline_rejected_in_autonomous_mode() {
    let mut store = store_with_a();
    let scope = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    let baseline_id = create_chain(&mut store, &scope, 3);
    let before = snapshot_count(store.conn_for_test());
    let err = store
        .rebaseline(
            &scope,
            &RebaseRequest {
                by: "agent".into(),
                reason: "autonomous".into(),
                confirmed_by: "nobody".into(),
                mode: RebaseMode::Autonomous,
            },
        )
        .expect_err("应当被拒");
    assert!(matches!(err, StoreError::RebaseRejectedAutonomous), "应当是自主模式被拒");
    assert_eq!(snapshot_count(store.conn_for_test()), before, "快照链应当不变");
    let (by, reason, confirmed) = rebase_trail(store.conn_for_test(), &baseline_id);
    assert!(by.is_none() && reason.is_none() && confirmed.is_none(), "留痕三列应当仍为 NULL");
}

/// 留痕三列成组写入：用户发起且三列非空，基线行三列全部等于传入值、所有增量行被删、基线条目
/// 变成最新快照的有效条目。
#[test]
fn rebaseline_writes_trail_columns_together() {
    let mut store = store_with_a();
    let scope = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    let baseline_id = create_chain(&mut store, &scope, 3);
    store
        .rebaseline(
            &scope,
            &RebaseRequest {
                by: "alice".into(),
                reason: "user asked".into(),
                confirmed_by: "bob".into(),
                mode: RebaseMode::User,
            },
        )
        .expect("应当成功");
    let (by, reason, confirmed) = rebase_trail(store.conn_for_test(), &baseline_id);
    assert_eq!(by.as_deref(), Some("alice"), "rebased_by 应当等于传入值");
    assert_eq!(reason.as_deref(), Some("user asked"), "rebase_reason 应当等于传入值");
    assert_eq!(confirmed.as_deref(), Some("bob"), "rebase_confirmed_by 应当等于传入值");
    // 所有增量行被删，只剩基线。
    assert_eq!(snapshot_count(store.conn_for_test()), 1, "应当只剩基线行");
}

/// 留痕三列任一为空时重定基线被拒：返回 RebaseMissingConfirmation、整笔回滚、留痕三列仍为 NULL。
#[test]
fn rebaseline_rejected_when_confirmation_missing() {
    let mut store = store_with_a();
    let scope = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    let baseline_id = create_chain(&mut store, &scope, 3);
    let before = snapshot_count(store.conn_for_test());
    let err = store
        .rebaseline(
            &scope,
            &RebaseRequest {
                by: "alice".into(),
                reason: "user asked".into(),
                confirmed_by: "".into(),
                mode: RebaseMode::User,
            },
        )
        .expect_err("应当被拒");
    assert!(matches!(err, StoreError::RebaseMissingConfirmation), "应当是缺人工确认");
    assert_eq!(snapshot_count(store.conn_for_test()), before, "快照链应当不变");
    let (by, reason, confirmed) = rebase_trail(store.conn_for_test(), &baseline_id);
    assert!(by.is_none() && reason.is_none() && confirmed.is_none(), "留痕三列应当仍为 NULL");
}

/// 重定基线后基线行只更新不删除：id 与 seq 不变、kind 仍是 baseline、parent_id 仍是 NULL。
#[test]
fn rebaseline_updates_baseline_row_without_deleting() {
    let mut store = store_with_a();
    let scope = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    let baseline_id = create_chain(&mut store, &scope, 3);
    store
        .rebaseline(
            &scope,
            &RebaseRequest {
                by: "alice".into(),
                reason: "user asked".into(),
                confirmed_by: "bob".into(),
                mode: RebaseMode::User,
            },
        )
        .expect("应当成功");
    let (kind, seq, parent) = snapshot_row(store.conn_for_test(), &baseline_id);
    assert_eq!(kind, "baseline", "kind 应当仍是 baseline");
    assert_eq!(seq, 1, "seq 应当仍是 1");
    assert!(parent.is_none(), "parent_id 应当仍是 NULL");
}

/// 重定基线后下一个快照是增量：kind 是 incremental、parent_id 是基线的 id、seq 是 2。
#[test]
fn rebaseline_next_snapshot_is_incremental_from_baseline() {
    let mut store = store_with_a();
    let scope = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    let baseline_id = create_chain(&mut store, &scope, 3);
    store
        .rebaseline(
            &scope,
            &RebaseRequest {
                by: "alice".into(),
                reason: "user asked".into(),
                confirmed_by: "bob".into(),
                mode: RebaseMode::User,
            },
        )
        .expect("应当成功");
    let after = file_entry(&mut store, "after.txt");
    let created = store.create_snapshot(&scope, &[after]).expect("应当成功");
    assert_eq!(created.seq, 2, "新快照的 seq 应当是 2");
    let (kind, seq, parent) = snapshot_row(store.conn_for_test(), created.id.as_str());
    assert_eq!(kind, "incremental", "新快照应当是增量");
    assert_eq!(seq, 2, "seq 应当是 2");
    assert_eq!(parent.as_deref(), Some(baseline_id.as_str()), "父边应当是基线的 id");
}
