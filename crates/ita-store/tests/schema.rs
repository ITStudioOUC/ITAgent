//! 表结构测试：约束是否真的在拦。
//!
//! 这些用例不测业务逻辑，只测"库里写不进坏数据"。表结构是最后一道防线，它失效时错误会以
//! 数据不一致的形式在很久之后出现，所以每条约束都要有对应用例。

use ita_kernel::context::WorkspaceId;
use ita_store::Store;

const WS: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const WS_OTHER: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";
const SESSION: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAX";
const SNAPSHOT: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAA";
const SNAPSHOT_CHILD: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAB";

fn workspace(text: &str) -> WorkspaceId {
    WorkspaceId::parse(text).expect("应当合法")
}

fn store_with_workspace() -> Store {
    let mut store = Store::open_in_memory().expect("应当可以打开");
    store.create_workspace(&workspace(WS), "/tmp/ws").expect("应当成功");
    store
}

/// 把一条基线快照写进库，供依赖快照 id 的用例复用。
fn insert_baseline(conn: &rusqlite::Connection, id: &str) {
    conn.execute(
        "INSERT INTO snapshots (id, workspace_id, seq, kind, created_at) VALUES (?1, ?2, 1, 'baseline', 0)",
        rusqlite::params![id, WS],
    )
    .expect("应当可插");
}

/// 迁移跑完后四张内容表的表与索引俱全。
///
/// 表或索引缺失时，后续每条依赖它的用例都会以奇怪的错误失败，所以这一条先兜住形状。
#[test]
fn content_tables_and_indexes_exist() {
    let store = store_with_workspace();
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

    // 按父查子、按会话回收读集都靠这两条索引，缺了就是全表扫。
    for index in ["snapshots_by_parent", "read_set_by_session"] {
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='index' AND name=?1)",
                rusqlite::params![index],
                |row| row.get(0),
            )
            .expect("应当可查");
        assert!(exists, "索引 {index} 不存在");
    }
}

#[test]
fn 快照条目的主键是快照加路径() {
    let store = store_with_workspace();
    let conn = store.conn_for_test();
    insert_baseline(conn, SNAPSHOT);

    let insert = |path: &str| {
        conn.execute(
            "INSERT INTO snapshot_entries (snapshot_id, path, kind, hash, size, content_stored)
             VALUES (?1, ?2, 'file', 'h', 1, 1)",
            rusqlite::params![SNAPSHOT, path],
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
    insert(store.conn_for_test(), SNAPSHOT, WS).expect("首次应当成功");
    assert!(insert(store.conn_for_test(), SNAPSHOT_CHILD, WS).is_err(), "同工作区同 seq 应当被拒");

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
        rusqlite::params![SNAPSHOT, WS_OTHER],
    );
    assert!(result.is_err(), "外键应当拦住悬空的工作区引用");
}

/// 快照条目的快照 id 必须指向真实存在的快照。
#[test]
fn snapshot_entry_rejects_unknown_snapshot() {
    let store = store_with_workspace();
    let result = store.conn_for_test().execute(
        "INSERT INTO snapshot_entries (snapshot_id, path, kind, hash, size, content_stored)
         VALUES (?1, 'a.rs', 'file', 'h', 1, 1)",
        rusqlite::params![SNAPSHOT],
    );
    assert!(result.is_err(), "外键应当拦住悬空的快照引用");
}

/// 读集的工作区列必须指向已注册的工作区。
#[test]
fn read_set_rejects_unknown_workspace() {
    let store = store_with_workspace();
    let conn = store.conn_for_test();
    conn.execute(
        "INSERT INTO sessions (id, workspace_id, head_node, created_at) VALUES (?1, ?2, 'n', 0)",
        rusqlite::params![SESSION, WS],
    )
    .expect("应当可插");

    let result = conn.execute(
        "INSERT INTO read_set (workspace_id, path, session_id, last_hash, last_read_seq)
         VALUES (?1, 'a.rs', ?2, 'h', 1)",
        rusqlite::params![WS_OTHER, SESSION],
    );
    assert!(result.is_err(), "外键应当拦住未注册的工作区");
}

/// 读集的会话列必须指向真实存在的会话。
#[test]
fn read_set_rejects_unknown_session() {
    let store = store_with_workspace();
    let result = store.conn_for_test().execute(
        "INSERT INTO read_set (workspace_id, path, session_id, last_hash, last_read_seq)
         VALUES (?1, 'a.rs', ?2, 'h', 1)",
        rusqlite::params![WS, SESSION],
    );
    assert!(result.is_err(), "外键应当拦住宿空引用的会话");
}

/// 快照的 kind 只允许基线与增量两种。
#[test]
fn snapshot_kind_is_limited_to_baseline_and_incremental() {
    let store = store_with_workspace();
    let result = store.conn_for_test().execute(
        "INSERT INTO snapshots (id, workspace_id, seq, kind, created_at) VALUES (?1, ?2, 1, 'weekly', 0)",
        rusqlite::params![SNAPSHOT, WS],
    );
    assert!(result.is_err(), "未知的快照类型应当被拒");
}

/// 条目的 kind 只允许文件、目录与墓碑三种。
#[test]
fn entry_kind_is_limited_to_file_dir_and_tombstone() {
    let store = store_with_workspace();
    let conn = store.conn_for_test();
    insert_baseline(conn, SNAPSHOT);

    let result = conn.execute(
        "INSERT INTO snapshot_entries (snapshot_id, path, kind, hash, size, content_stored)
         VALUES (?1, 'a.rs', 'symlink', 'h', 1, 1)",
        rusqlite::params![SNAPSHOT],
    );
    assert!(result.is_err(), "未知的条目类型应当被拒");
}

/// 墓碑与目录不得携带内容标记或哈希，文件则必须把两者都给出。
///
/// 这组约束让"墓碑却记着已存内容"这种自相矛盾的行无法写进库，而不是靠调用方每处记得。
#[test]
fn tombstone_and_content_are_mutually_exclusive() {
    let store = store_with_workspace();
    let conn = store.conn_for_test();
    insert_baseline(conn, SNAPSHOT);

    let insert = |path: &str, kind: &str, hash: Option<&str>, size: Option<i64>, stored| {
        conn.execute(
            "INSERT INTO snapshot_entries (snapshot_id, path, kind, hash, size, content_stored)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![SNAPSHOT, path, kind, hash, size, stored],
        )
    };

    let err = insert("dead.rs", "tombstone", None, None, Some(1)).expect_err("墓碑不应带内容标记");
    assert!(err.to_string().contains("CHECK"), "应当由 CHECK 拦住：{err}");
    insert("dead.rs", "tombstone", None, None, None).expect("墓碑本身应当可插");

    insert("dir", "dir", Some("h"), None, Some(1)).expect_err("目录不应带哈希");
    insert("dir", "dir", None, None, None).expect("目录本身应当可插");

    insert("big.bin", "file", None, Some(64), Some(0)).expect_err("文件必须带哈希");
    insert("big.bin", "file", Some("h"), None, Some(0)).expect_err("文件必须带尺寸");
    insert("big.bin", "file", Some("h"), Some(64), None).expect_err("文件必须带内容标记");
    insert("big.bin", "file", Some("h"), Some(64), Some(2)).expect_err("内容标记只有 0 与 1");
    insert("big.bin", "file", Some("h"), Some(64), Some(0)).expect("超阈值文件应当可插");
    insert("small.rs", "file", Some("h"), Some(64), Some(1)).expect("已存内容的文件应当可插");
}

/// 重定基线只对基线快照成立，且发起人、原因、人工确认三者同进同出。
#[test]
fn rebase_trail_only_allows_baseline_with_full_trail() {
    let store = store_with_workspace();
    let conn = store.conn_for_test();

    let insert = |id: &str, kind: &str, rebased_by, reason, confirmed| {
        conn.execute(
            "INSERT INTO snapshots
                 (id, workspace_id, seq, kind, rebased_by, rebase_reason, rebase_confirmed_by, created_at)
             VALUES (?1, ?2, 1, ?3, ?4, ?5, ?6, 0)",
            rusqlite::params![id, WS, kind, rebased_by, reason, confirmed],
        )
    };

    insert(SNAPSHOT_CHILD, "incremental", Some("agent"), Some("压缩后收起旧回滚点"), Some("user"))
        .expect_err("增量快照不允许带重定基线的留痕");

    insert(SNAPSHOT_CHILD, "baseline", Some("agent"), Some("压缩后收起旧回滚点"), None)
        .expect_err("缺少人工确认的留痕应当被拒");
    insert(SNAPSHOT_CHILD, "baseline", Some("agent"), None, Some("user"))
        .expect_err("缺少原因的留痕应当被拒");

    insert(SNAPSHOT_CHILD, "baseline", Some("agent"), Some("压缩后收起旧回滚点"), Some("user"))
        .expect("完整的重定基线留痕应当可插");
}

/// 宽限期起点不能早于 blob 的创建时间。
///
/// 起点由回收扫描在观察到"变为无引用"时写下，早于创建时间只可能是算错或传错，会让对象被
/// 立刻判死。
#[test]
fn blob_unreferenced_since_cannot_predate_creation() {
    let store = store_with_workspace();
    let conn = store.conn_for_test();
    let insert = |hash: &str, since: i64| {
        conn.execute(
            "INSERT INTO blobs (hash, size, created_at, unreferenced_since) VALUES (?1, 1, 100, ?2)",
            rusqlite::params![hash, since],
        )
    };
    insert("early", 99).expect_err("早于创建时间的起点应当被拒");
    insert("ok", 100).expect("等于创建时间应当可插");
}

/// 存在子快照时删除父快照会被外键拦住，合并必须先重挂子边再删父。
///
/// 父边若改成删除时置空，谱系会静默断掉，且空父边与"顶层快照"不可区分，因此这里守的是
/// "忘记重挂就整笔失败"这个行为。
#[test]
fn deleting_parent_snapshot_with_child_is_rejected() {
    let store = store_with_workspace();
    let conn = store.conn_for_test();
    insert_baseline(conn, SNAPSHOT);
    conn.execute(
        "INSERT INTO snapshots (id, workspace_id, seq, parent_id, kind, created_at)
         VALUES (?1, ?2, 2, ?3, 'incremental', 0)",
        rusqlite::params![SNAPSHOT_CHILD, WS, SNAPSHOT],
    )
    .expect("子快照应当可插");

    let result = conn.execute("DELETE FROM snapshots WHERE id = ?1", rusqlite::params![SNAPSHOT]);
    assert!(result.is_err(), "子快照存在时删父应当被外键拦住");
}
