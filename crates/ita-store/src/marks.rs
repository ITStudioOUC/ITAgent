//! 标记集：从三张表加 `extra_roots` 参数收集根哈希。
//!
//! 标记集是回收的公共依赖：`gc.rs` 的清扫用它判"被引用"。它是三个来源的并集，不是"只标快照
//! 条目"——节点正文（`nodes.body_hash`）与事件正文（`events.payload_hash`）同样引用 CAS，
//! 节点不可变、事件 append-only，两者都是永久标记根，漏标即数据损坏。
//!
//! 第四个来源是 `extra_roots` 参数：活跃租约引用的哈希。租约表还没建（09.x 执行器的职责），
//! 由调用方从租约表读出后传入，存储层不解析租约表，与 06.8 的"范围是参数"同型。
//!
//! 吃一个已开启的事务或连接，不自行开事务。

use std::collections::HashSet;

use rusqlite::Connection;

/// 从三张表加 `extra_roots` 参数收集标记集。
///
/// # 步骤
/// 1. 收集 `snapshot_entries.hash`（非空的文件条目哈希）。
/// 2. 收集 `nodes.body_hash`（非空的节点正文哈希）。
/// 3. 收集 `events.payload_hash`（非空的事件正文哈希）。
/// 4. 并入 `extra_roots` 参数。
///
/// # 边界与前提
/// 接收已开启的连接或事务，不自行开事务。只读。`content_stored = 0` 的哈希不在 blobs 里，
/// 标记它无害——标记一个不存在的哈希不报错。
///
/// # 副作用
/// 只读。
pub(crate) fn collect(
    conn: &Connection,
    extra_roots: &[String],
) -> Result<HashSet<String>, rusqlite::Error> {
    let mut set = HashSet::new();
    // 快照条目：所有文件条目的哈希。
    collect_column(conn, "SELECT hash FROM snapshot_entries WHERE hash IS NOT NULL", &mut set)?;
    // 节点正文：节点不可变、永不删除，是永久标记根。
    collect_column(conn, "SELECT body_hash FROM nodes WHERE body_hash IS NOT NULL", &mut set)?;
    // 事件正文：事件 append-only、永不删除，是永久标记根。
    collect_column(
        conn,
        "SELECT payload_hash FROM events WHERE payload_hash IS NOT NULL",
        &mut set,
    )?;
    // 租约根：由调用方传入。
    for hash in extra_roots {
        set.insert(hash.clone());
    }
    Ok(set)
}

/// 执行一条只读查询，把单列的每行值并入标记集。
fn collect_column(
    conn: &Connection,
    sql: &str,
    set: &mut HashSet<String>,
) -> Result<(), rusqlite::Error> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    for row in rows {
        set.insert(row?);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Store;
    use ita_kernel::context::WorkspaceId;

    const WS: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    const SESSION: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAX";
    const SNAP: &str = "01ARZ3NDEKTSV4RRFFQ69G5FB0";
    const NODE: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

    /// 打开内存库，注册工作区、会话与一条快照行，返回库。
    fn store_with_snapshot() -> Store {
        let mut store = Store::open_in_memory().expect("应当可以打开");
        let id = WorkspaceId::parse(WS).expect("应当合法");
        store.create_workspace(&id, "/tmp/ws").expect("应当成功");
        store
            .conn_for_test()
            .execute(
                "INSERT INTO sessions (id, workspace_id, head_node, head_generation, created_at)
                 VALUES (?1, ?2, ?3, 0, 0)",
                rusqlite::params![SESSION, WS, NODE],
            )
            .expect("应当可插");
        store
            .conn_for_test()
            .execute(
                "INSERT INTO nodes (id, workspace_id, session_id, body_kind, created_at)
                 VALUES (?1, ?2, ?3, 'turn', 0)",
                rusqlite::params![NODE, WS, SESSION],
            )
            .expect("应当可插");
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

    /// 空库空参数：标记集为空。
    #[test]
    fn empty_store_yields_empty_mark_set() {
        let store = store_with_snapshot();
        let set = collect(store.conn(), &[]).expect("应当可读");
        assert!(set.is_empty(), "没有引用时标记集应当为空");
    }

    /// 快照条目的哈希进标记集。
    #[test]
    fn snapshot_entry_hash_is_marked() {
        let store = store_with_snapshot();
        let hash = "aa".repeat(32);
        store
            .conn_for_test()
            .execute(
                "INSERT INTO snapshot_entries
                     (snapshot_id, path, kind, hash, mode, size, mtime, content_stored)
                 VALUES (?1, 'a.txt', 'file', ?2, 420, 10, NULL, 1)",
                rusqlite::params![SNAP, hash],
            )
            .expect("应当可插");
        let set = collect(store.conn(), &[]).expect("应当可读");
        assert!(set.contains(&hash), "快照条目哈希应当被标记");
    }

    /// 节点正文的哈希进标记集：节点不可变、永不删除，是永久标记根。
    #[test]
    fn node_body_hash_is_marked() {
        let store = store_with_snapshot();
        let hash = "bb".repeat(32);
        store
            .conn_for_test()
            .execute("UPDATE nodes SET body_hash = ?1 WHERE id = ?2", rusqlite::params![hash, NODE])
            .expect("应当可改");
        let set = collect(store.conn(), &[]).expect("应当可读");
        assert!(set.contains(&hash), "节点正文哈希应当被标记");
    }

    /// 事件正文的哈希进标记集：事件 append-only、永不删除，是永久标记根。
    #[test]
    fn event_payload_hash_is_marked() {
        let store = store_with_snapshot();
        let hash = "cc".repeat(32);
        store
            .conn_for_test()
            .execute(
                "INSERT INTO events
                     (workspace_id, seq, id, kind, actor, payload_hash, payload_len, created_at)
                 VALUES (?1, 1, '01ARZ3NDEKTSV4RRFFQ69G5FA6', 'note', 'test', ?2, 10, 0)",
                rusqlite::params![WS, hash],
            )
            .expect("应当可插");
        let set = collect(store.conn(), &[]).expect("应当可读");
        assert!(set.contains(&hash), "事件正文哈希应当被标记");
    }

    /// `extra_roots` 参数进标记集：租约根由调用方传入，存储层不解析租约表。
    #[test]
    fn extra_roots_are_marked() {
        let store = store_with_snapshot();
        let lease_hash = "dd".repeat(32);
        let set = collect(store.conn(), std::slice::from_ref(&lease_hash)).expect("应当可读");
        assert!(set.contains(&lease_hash), "租约根应当被标记");
    }

    /// 三个来源加参数取并集：四种哈希都在标记集里。
    #[test]
    fn mark_set_is_union_of_all_sources() {
        let store = store_with_snapshot();
        let entry = "aa".repeat(32);
        let body = "bb".repeat(32);
        let payload = "cc".repeat(32);
        let lease = "dd".repeat(32);
        store
            .conn_for_test()
            .execute(
                "INSERT INTO snapshot_entries
                     (snapshot_id, path, kind, hash, mode, size, mtime, content_stored)
                 VALUES (?1, 'a.txt', 'file', ?2, 420, 10, NULL, 1)",
                rusqlite::params![SNAP, entry],
            )
            .expect("应当可插");
        store
            .conn_for_test()
            .execute("UPDATE nodes SET body_hash = ?1 WHERE id = ?2", rusqlite::params![body, NODE])
            .expect("应当可改");
        store
            .conn_for_test()
            .execute(
                "INSERT INTO events
                     (workspace_id, seq, id, kind, actor, payload_hash, payload_len, created_at)
                 VALUES (?1, 1, '01ARZ3NDEKTSV4RRFFQ69G5FA6', 'note', 'test', ?2, 10, 0)",
                rusqlite::params![WS, payload],
            )
            .expect("应当可插");
        let set = collect(store.conn(), std::slice::from_ref(&lease)).expect("应当可读");
        assert!(set.contains(&entry), "快照条目哈希应当被标记");
        assert!(set.contains(&body), "节点正文哈希应当被标记");
        assert!(set.contains(&payload), "事件正文哈希应当被标记");
        assert!(set.contains(&lease), "租约根应当被标记");
        assert_eq!(set.len(), 4, "标记集应当恰好是四个来源的并集");
    }
}
