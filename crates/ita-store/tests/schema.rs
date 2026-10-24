//! 表结构测试：约束是否真的在拦。
//!
//! 这些用例不测业务逻辑，只测"库里写不进坏数据"。表结构是最后一道防线，它失效时错误会以
//! 数据不一致的形式在很久之后出现，所以每条约束都要有对应用例。

use ita_kernel::context::WorkspaceId;
use ita_store::Store;

const WS: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const WS_OTHER: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";
const SESSION: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAX";

fn workspace(text: &str) -> WorkspaceId {
    WorkspaceId::parse(text).expect("应当合法")
}

fn store_with_workspace() -> Store {
    let mut store = Store::open_in_memory().expect("应当可以打开");
    store.create_workspace(&workspace(WS), "/tmp/ws").expect("应当成功");
    store
}

#[test]
fn 两张内容表都存在() {
    let store = store_with_workspace();
    // 迁移跑到版本 2 时这两张表必须存在；查询失败即说明迁移没建全。
    let conn = store.conn_for_test();
    for table in ["blobs", "snapshots", "snapshot_entries", "read_set"] {
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                rusqlite::params![table],
                |row| row.get(0),
            )
            .expect("应当可查");
        assert!(exists, "表 {table} 不存在");
    }
}

#[test]
fn 快照条目的主键是快照加路径() {
    let store = store_with_workspace();
    let conn = store.conn_for_test();
    conn.execute(
        "INSERT INTO snapshots (id, workspace_id, seq, kind, created_at) VALUES (?1, ?2, 1, 'baseline', 0)",
        rusqlite::params!["01ARZ3NDEKTSV4RRFFQ69G5FAA", WS],
    )
    .expect("应当可插");

    let insert = |path: &str| {
        conn.execute(
            "INSERT INTO snapshot_entries (snapshot_id, path, kind) VALUES (?1, ?2, 'file')",
            rusqlite::params!["01ARZ3NDEKTSV4RRFFQ69G5FAA", path],
        )
    };
    insert("a.rs").expect("首次应当成功");
    assert!(insert("a.rs").is_err(), "同一快照内同一路径不能有两条");
    insert("b.rs").expect("不同路径应当成功");
}

#[test]
fn 快照的_seq_在工作区内唯一() {
    let mut store = store_with_workspace();
    let insert = |conn: &rusqlite::Connection, id: &str, ws: &str| {
        conn.execute(
            "INSERT INTO snapshots (id, workspace_id, seq, kind, created_at) VALUES (?1, ?2, 1, 'incremental', 0)",
            rusqlite::params![id, ws],
        )
    };
    insert(store.conn_for_test(), "01ARZ3NDEKTSV4RRFFQ69G5FAA", WS).expect("首次应当成功");
    assert!(
        insert(store.conn_for_test(), "01ARZ3NDEKTSV4RRFFQ69G5FAB", WS).is_err(),
        "同工作区同 seq 应当被拒"
    );

    // 另一个工作区可以用同一个 seq。
    store.create_workspace(&workspace(WS_OTHER), "/tmp/other").expect("应当成功");
    insert(store.conn_for_test(), "01ARZ3NDEKTSV4RRFFQ69G5FAC", WS_OTHER)
        .expect("跨工作区应当允许");
}

#[test]
fn 读集主键包含工作区路径与会话() {
    let store = store_with_workspace();
    let conn = store.conn_for_test();
    conn.execute(
        "INSERT INTO sessions (id, workspace_id, head_node, created_at) VALUES (?1, ?2, 'n', 0)",
        rusqlite::params![SESSION, WS],
    )
    .expect("应当可插");

    let insert = |hash: &str| {
        conn.execute(
            "INSERT INTO read_set (workspace_id, path, session_id, last_hash, last_read_seq)
             VALUES (?1, 'a.rs', ?2, ?3, 1)",
            rusqlite::params![WS, SESSION, hash],
        )
    };
    insert("h1").expect("首次应当成功");
    assert!(insert("h2").is_err(), "同一三元组不能有两条记录");
}

#[test]
fn 快照引用不存在的工作区时被拒() {
    let store = store_with_workspace();
    let conn = store.conn_for_test();
    let result = conn.execute(
        "INSERT INTO snapshots (id, workspace_id, seq, kind, created_at) VALUES (?1, ?2, 1, 'baseline', 0)",
        rusqlite::params!["01ARZ3NDEKTSV4RRFFQ69G5FAA", WS_OTHER],
    );
    assert!(result.is_err(), "外键应当拦住悬空的工作区引用");
}
