//! 标记清除 GC 的集成测试。
//!
//! 这些用例走公开接口 [`Store::gc`]，覆盖计划第四节的十条验收。第一条与第二条守"无引用且超宽
//! 限期被删"与"宽限期起点写在 `unreferenced_since`"，第三条守快照引用是标记根，第四条守租约
//! 根是附加标记根，第五条守未过期对象保留，第六条守节点正文是永久标记根，第七条守事件正文是
//! 永久标记根，第八条守孤儿文件清扫，第九条守删除顺序是行先行文件随后，第十条守回收是全局操作。
//!
//! 宽限期是两观协议：第一次观察只写不删，第二次观察才删。要测"超宽限期"，必须把
//! `created_at` 与 `unreferenced_since` 都设成过去，否则 `now - unreferenced_since` 永远小于
//! 宽限期，第二次观察永远不触发。

use std::path::Path;

use ita_kernel::context::WorkspaceId;
use ita_store::Store;

const WS_A: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const WS_B: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";
const SNAP: &str = "01ARZ3NDEKTSV4RRFFQ69G5FB0";

fn workspace(text: &str) -> WorkspaceId {
    WorkspaceId::parse(text).expect("应当合法")
}

/// 打开一个库并注册工作区 A。
fn store_with_a() -> Store {
    let mut store = Store::open_in_memory().expect("应当可以打开");
    store.create_workspace(&workspace(WS_A), "/tmp/a").expect("应当成功");
    store
}

/// 写入一段内容，返回其哈希。
fn put(store: &mut Store, content: &[u8]) -> String {
    store.put(content).expect("应当成功").hash
}

/// 把某 blob 的 `created_at` 与 `unreferenced_since` 设成过去，模拟宽限期已过。
///
/// `created_at` 必须早于 `unreferenced_since`，否则违反 `unreferenced_since_not_before_creation`
/// 的 CHECK。两者都设成 `now - grace_period - 偏移`，保证 `now - unreferenced_since >= grace_period`。
fn set_past_grace(conn: &rusqlite::Connection, hash: &str, grace_period: i64) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("时钟应当可用")
        .as_millis() as i64;
    conn.execute(
        "UPDATE blobs SET created_at = ?1, unreferenced_since = ?2 WHERE hash = ?3",
        rusqlite::params![now - grace_period - 1000, now - grace_period - 500, hash],
    )
    .expect("应当可改");
}

/// 读回 `blobs` 表某哈希的一行，返回 (尺寸, 创建时间, 宽限期起点)。
fn blob_row(conn: &rusqlite::Connection, hash: &str) -> (i64, i64, Option<i64>) {
    conn.query_row(
        "SELECT size, created_at, unreferenced_since FROM blobs WHERE hash = ?1",
        rusqlite::params![hash],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, Option<i64>>(2)?)),
    )
    .expect("应当可读")
}

/// 某哈希的行是否存在。
fn row_exists(conn: &rusqlite::Connection, hash: &str) -> bool {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM blobs WHERE hash = ?1)",
        rusqlite::params![hash],
        |row| row.get::<_, bool>(0),
    )
    .expect("应当可读")
}

/// 某哈希的 CAS 文件路径。
fn file_path(store: &Store, hash: &str) -> std::path::PathBuf {
    store.cas_root_for_test().join(&hash[..2]).join(hash)
}

/// 递归统计一个目录下的文件数。
fn count_files(dir: &Path) -> u32 {
    let mut count = 0;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                count += count_files(&path);
            } else {
                count += 1;
            }
        }
    }
    count
}

/// 无引用且超宽限期的 blob 被删：行与文件都被删。
#[test]
fn unreferenced_past_grace_period_is_deleted() {
    let mut store = store_with_a();
    let hash = put(&mut store, b"delete me");
    set_past_grace(store.conn_for_test(), &hash, 1000);
    let result = store.gc(1000, &[]).expect("应当成功");
    assert_eq!(result.deleted, 1, "应当删掉一行");
    assert!(!row_exists(store.conn_for_test(), &hash), "行应当被删");
    assert!(!file_path(&store, &hash).exists(), "文件应当被删");
}

/// 宽限期起点写在 blobs.unreferenced_since：第一次观察只写不删，行还在、宽限期起点被写成非空。
#[test]
fn grace_period_start_is_written_on_first_observation() {
    let mut store = store_with_a();
    let hash = put(&mut store, b"first observation");
    let result = store.gc(1000, &[]).expect("应当成功");
    assert_eq!(result.deleted, 0, "第一次观察不应当删行");
    assert_eq!(result.observed, 1, "应当观察到一行");
    assert!(row_exists(store.conn_for_test(), &hash), "行应当还在");
    let (_, _, unreferenced_since) = blob_row(store.conn_for_test(), &hash);
    assert!(unreferenced_since.is_some(), "宽限期起点应当被写成非空");
}

/// 快照引用保留：被快照条目引用的 blob 不删，行与文件都在。
#[test]
fn snapshot_referenced_blob_is_kept() {
    let mut store = store_with_a();
    let hash = put(&mut store, b"referenced by snapshot");
    // 插一条快照行与一条引用该哈希的条目。
    store
        .conn_for_test()
        .execute(
            "INSERT INTO snapshots (id, workspace_id, seq, parent_id, kind, created_at)
             VALUES (?1, ?2, 1, NULL, 'baseline', 0)",
            rusqlite::params![SNAP, WS_A],
        )
        .expect("应当可插");
    store
        .conn_for_test()
        .execute(
            "INSERT INTO snapshot_entries
                 (snapshot_id, path, kind, hash, mode, size, mtime, content_stored)
             VALUES (?1, 'a.txt', 'file', ?2, 420, 10, NULL, 1)",
            rusqlite::params![SNAP, hash],
        )
        .expect("应当可插");
    // 即使超宽限期，被引用的 blob 也不删。
    set_past_grace(store.conn_for_test(), &hash, 1000);
    let result = store.gc(1000, &[]).expect("应当成功");
    assert_eq!(result.deleted, 0, "被引用的 blob 不应当被删");
    assert!(row_exists(store.conn_for_test(), &hash), "被引用的行应当保留");
    assert!(file_path(&store, &hash).exists(), "被引用的文件应当保留");
}

/// 租约根保留：传进 extra_roots 的 blob 不删，行与文件都在。
#[test]
fn lease_root_is_kept() {
    let mut store = store_with_a();
    let hash = put(&mut store, b"held by lease");
    set_past_grace(store.conn_for_test(), &hash, 1000);
    let result = store.gc(1000, std::slice::from_ref(&hash)).expect("应当成功");
    assert_eq!(result.deleted, 0, "租约根不应当被删");
    assert!(row_exists(store.conn_for_test(), &hash), "租约根的行应当保留");
    assert!(file_path(&store, &hash).exists(), "租约根的文件应当保留");
}

/// 未过期对象保留：第一次观察后行还在、宽限期起点被写成非空、文件还在。
#[test]
fn unexpired_object_is_kept() {
    let mut store = store_with_a();
    let hash = put(&mut store, b"not yet expired");
    let result = store.gc(1000, &[]).expect("应当成功");
    assert_eq!(result.deleted, 0, "未过期不应当删行");
    assert!(row_exists(store.conn_for_test(), &hash), "行应当还在");
    let (_, _, unreferenced_since) = blob_row(store.conn_for_test(), &hash);
    assert!(unreferenced_since.is_some(), "宽限期起点应当被写成非空");
    assert!(file_path(&store, &hash).exists(), "文件应当还在");
}

/// 孤儿文件被清：没有 blob 行的文件被删。
#[test]
fn orphan_file_is_swept() {
    let mut store = store_with_a();
    // 写入一个 blob，再删掉它的行，留下无行文件。
    let hash = put(&mut store, b"orphan file");
    store
        .conn_for_test()
        .execute("DELETE FROM blobs WHERE hash = ?1", rusqlite::params![hash])
        .expect("应当可删");
    assert!(file_path(&store, &hash).exists(), "删行后文件应当还在");
    let result = store.gc(1000, &[]).expect("应当成功");
    assert_eq!(result.orphan_files, 1, "应当清扫一个孤儿文件");
    assert!(!file_path(&store, &hash).exists(), "孤儿文件应当被删");
}

/// 回收是全局操作，不按工作区过滤：两个工作区各写一个无引用 blob，都被删。
#[test]
fn gc_is_global_not_per_workspace() {
    let mut store = store_with_a();
    store.create_workspace(&workspace(WS_B), "/tmp/b").expect("应当成功");
    let a = put(&mut store, b"workspace A blob");
    let b = put(&mut store, b"workspace B blob");
    set_past_grace(store.conn_for_test(), &a, 1000);
    set_past_grace(store.conn_for_test(), &b, 1000);
    let result = store.gc(1000, &[]).expect("应当成功");
    assert_eq!(result.deleted, 2, "两个工作区的 blob 都应当被删");
    assert!(!row_exists(store.conn_for_test(), &a), "工作区 A 的行应当被删");
    assert!(!row_exists(store.conn_for_test(), &b), "工作区 B 的行应当被删");
    assert!(!file_path(&store, &a).exists(), "工作区 A 的文件应当被删");
    assert!(!file_path(&store, &b).exists(), "工作区 B 的文件应当被删");
}

/// 节点正文保留：被节点正文引用的 blob 不删，行与文件都在。
#[test]
fn node_body_is_kept() {
    let mut store = store_with_a();
    let hash = put(&mut store, b"node body");
    // 插一个会话与一个引用该哈希的节点。
    let session = "01ARZ3NDEKTSV4RRFFQ69G5FAX";
    let node = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    store
        .conn_for_test()
        .execute(
            "INSERT INTO sessions (id, workspace_id, head_node, head_generation, created_at)
             VALUES (?1, ?2, ?3, 0, 0)",
            rusqlite::params![session, WS_A, node],
        )
        .expect("应当可插");
    store
        .conn_for_test()
        .execute(
            "INSERT INTO nodes (id, workspace_id, session_id, body_kind, body_hash, body_len, created_at)
             VALUES (?1, ?2, ?3, 'turn', ?4, 10, 0)",
            rusqlite::params![node, WS_A, session, hash],
        )
        .expect("应当可插");
    set_past_grace(store.conn_for_test(), &hash, 1000);
    let result = store.gc(1000, &[]).expect("应当成功");
    assert_eq!(result.deleted, 0, "节点正文不应当被删");
    assert!(row_exists(store.conn_for_test(), &hash), "节点正文的行应当保留");
    assert!(file_path(&store, &hash).exists(), "节点正文的文件应当保留");
}

/// 事件正文保留：被事件正文引用的 blob 不删，行与文件都在。
#[test]
fn event_payload_is_kept() {
    let mut store = store_with_a();
    let hash = put(&mut store, b"event payload");
    // 追加一条引用该哈希的事件正文。
    store
        .conn_for_test()
        .execute(
            "INSERT INTO events
                 (workspace_id, seq, id, kind, actor, payload_hash, payload_len, created_at)
             VALUES (?1, 1, '01ARZ3NDEKTSV4RRFFQ69G5FA6', 'note', 'test', ?2, 10, 0)",
            rusqlite::params![WS_A, hash],
        )
        .expect("应当可插");
    set_past_grace(store.conn_for_test(), &hash, 1000);
    let result = store.gc(1000, &[]).expect("应当成功");
    assert_eq!(result.deleted, 0, "事件正文不应当被删");
    assert!(row_exists(store.conn_for_test(), &hash), "事件正文的行应当保留");
    assert!(file_path(&store, &hash).exists(), "事件正文的文件应当保留");
}

/// 行先行、文件随后：中断只留无引用文件。
///
/// 模拟"行删了、文件删失败"的中断：删掉行后留下无行文件，再跑一次回收，孤儿文件清扫把文件清掉。
/// 这守的是"删除顺序是行先行、文件随后"——中断只留无引用文件（安全），不留下指向空内容的行。
#[test]
fn interruption_leaves_only_orphan_file() {
    let mut store = store_with_a();
    let hash = put(&mut store, b"row first file after");
    set_past_grace(store.conn_for_test(), &hash, 1000);
    // 模拟行删了、文件删失败：删行，留下无行文件。
    store
        .conn_for_test()
        .execute("DELETE FROM blobs WHERE hash = ?1", rusqlite::params![hash])
        .expect("应当可删");
    assert!(!row_exists(store.conn_for_test(), &hash), "行应当被删");
    assert!(file_path(&store, &hash).exists(), "中断后文件应当还在");
    // 再跑一次回收，孤儿文件清扫把文件清掉。
    let result = store.gc(1000, &[]).expect("应当成功");
    assert_eq!(result.orphan_files, 1, "应当清扫一个无行文件");
    assert!(!file_path(&store, &hash).exists(), "无行文件应当被清");
}

/// 读回一个 CAS 根目录下的文件数，验证孤儿清扫不伤正常文件。
#[test]
fn orphan_sweep_does_not_touch_referenced_files() {
    let mut store = store_with_a();
    // 写入一个被引用的 blob 与一个无行文件。
    let hash = put(&mut store, b"referenced file");
    let orphan = put(&mut store, b"orphan file");
    store
        .conn_for_test()
        .execute("DELETE FROM blobs WHERE hash = ?1", rusqlite::params![orphan])
        .expect("应当可删");
    assert_eq!(count_files(store.cas_root_for_test()), 2, "应当有两个文件");
    let result = store.gc(1000, &[]).expect("应当成功");
    assert_eq!(result.orphan_files, 1, "应当只清扫一个无行文件");
    assert_eq!(count_files(store.cas_root_for_test()), 1, "应当只剩一个文件");
    assert!(file_path(&store, &hash).exists(), "被引用的文件应当保留");
    assert!(!file_path(&store, &orphan).exists(), "无行文件应当被删");
}
