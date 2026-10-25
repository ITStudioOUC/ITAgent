//! 清扫算法：两观协议、行先行文件随后、孤儿文件清扫。
//!
//! 回收是全局标记-清除加宽限期，不是引用计数。引用计数在崩溃、事务回滚、并发 squash 下容易
//! 失配，失配的后果是删掉仍在使用的对象，也就是数据损坏；标记-清除失配的后果只是"该删的没
//! 删"（占用空间），在两种失败模式之间选更安全的那种。标记集是三个来源的并集加一个参数，
//! 见 [`marks`]。
//!
//! 宽限期是两观协议：`unreferenced_since` 是"被观察为无引用"的时刻，不是创建时间。第一次
//! 观察只写 `unreferenced_since = now`，不删；第二次观察（且 `now - unreferenced_since >=
//! grace_period`）才删行删文件。拿创建时间当起点会把刚写入的对象提前判死——宽限期服务的是
//! "刚被引用、清单尚未提交"这个窗口，正在执行的工具可能刚引用它而清单尚未提交。
//!
//! 删除顺序是行先行、文件随后：行删除在事务内，文件删除在事务提交之后。中断只留下无引用
//! 文件（安全，多占空间），不留下指向空内容的行（危险，丢内容）。这是 06.5 写入路径（文件
//! 先行、行随后）的逆：写入时中断只留无引用文件，删除时中断也只留无引用文件。
//!
//! 孤儿文件清扫是回收的一部分：扫 CAS 目录，删没有 blob 行的文件（包括 `.tmp`）。没有行的
//! 文件是无引用对象，删掉只省空间，不丢内容。
//!
//! 吃一个已开启的事务或连接，不自行开事务。

use std::path::Path;

use rusqlite::{Connection, Transaction};

use crate::blobs;
use crate::cas;
use crate::db::{StoreError, now_millis};
use crate::marks;

/// 回收结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GcResult {
    /// 被删的行数（及其文件被删）。
    pub deleted: u64,
    /// 被第一次观察的行数（写了 `unreferenced_since` 但未删）。
    pub observed: u64,
    /// 被清扫的孤儿文件数（无 blob 行的文件，包括 `.tmp`）。
    pub orphan_files: u64,
}

/// 行清扫：标记、两观协议、删无引用且超宽限期的行。
///
/// # 步骤
/// 1. 收集标记集（三张表加 `extra_roots` 参数）。
/// 2. 对每个无引用的 blob：第一次观察写 `unreferenced_since = now`，不删；第二次观察且超宽
///    限期删行。
///
/// # 边界与前提
/// 接收已开启的事务，不自行开事务。行删除在事务内，文件删除由调用方在提交之后做。
///
/// # 副作用
/// 写 `blobs` 表。
///
/// 返回被删行的哈希列表（供文件清扫）与第一次观察的行数。
pub(crate) fn sweep_in(
    tx: &Transaction,
    grace_period: i64,
    extra_roots: &[String],
) -> Result<(Vec<String>, u64), StoreError> {
    let marks = marks::collect(tx, extra_roots)?;
    let now = now_millis()?;
    let mut deleted = Vec::new();
    let mut observed = 0;

    // 读全部 blob 行。
    let mut stmt = tx.prepare("SELECT hash, unreferenced_since FROM blobs")?;
    let rows =
        stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?)))?;
    for row in rows {
        let (hash, unreferenced_since) = row?;
        // 被引用，保留。
        if marks.contains(&hash) {
            continue;
        }
        match unreferenced_since {
            None => {
                // 第一次观察：写 `unreferenced_since = now`，不删。
                tx.execute(
                    "UPDATE blobs SET unreferenced_since = ?1 WHERE hash = ?2",
                    rusqlite::params![now, hash],
                )?;
                observed += 1;
            }
            Some(since) => {
                // 第二次观察：超宽限期删行。
                if now - since >= grace_period {
                    tx.execute("DELETE FROM blobs WHERE hash = ?1", rusqlite::params![hash])?;
                    deleted.push(hash);
                }
                // 未超宽限期保留，不碰 `unreferenced_since`。
            }
        }
    }

    Ok((deleted, observed))
}

/// 文件清扫：删被删行的文件、清扫孤儿文件。
///
/// # 步骤
/// 1. 删被删行的哈希文件。
/// 2. 扫 CAS 目录，删没有 blob 行的文件（包括 `.tmp`）。
///
/// # 边界与前提
/// 接收一个连接（提交后）。文件删除在事务提交之后，中断只留无引用文件。
///
/// # 副作用
/// 删 CAS 目录下的文件。
///
/// 返回被清扫的孤儿文件数。
pub(crate) fn sweep_files(
    conn: &Connection,
    cas_root: &Path,
    deleted: &[String],
) -> Result<u64, StoreError> {
    // 删被删行的哈希文件。
    for hash in deleted {
        let path = cas::blob_path(cas_root, hash);
        if path.exists() {
            std::fs::remove_file(&path).map_err(StoreError::Cas)?;
        }
    }
    // 清扫孤儿文件。
    sweep_orphans(conn, cas_root)
}

/// 清扫孤儿文件：没有 blob 行的文件（包括 `.tmp`）。
fn sweep_orphans(conn: &Connection, cas_root: &Path) -> Result<u64, StoreError> {
    let mut count = 0;
    // CAS 根目录不存在时没有文件可清。
    let top = match std::fs::read_dir(cas_root) {
        Ok(top) => top,
        Err(_) => return Ok(0),
    };
    for entry in top.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let sub = match std::fs::read_dir(&dir) {
            Ok(sub) => sub,
            Err(_) => continue,
        };
        for file in sub.flatten() {
            let path = file.path();
            if !path.is_file() {
                continue;
            }
            let name = file.file_name().to_string_lossy().to_string();
            // `.tmp` 文件是无引用对象；完整哈希文件在 blobs 没有行时是无引用对象。
            let is_orphan =
                if name.ends_with(".tmp") { true } else { !blobs::exists(conn, &name)? };
            if is_orphan {
                std::fs::remove_file(&path).map_err(StoreError::Cas)?;
                count += 1;
            }
        }
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Store;

    /// 写入一个 blob（行加文件），返回其哈希。
    fn put_blob(store: &mut Store, content: &[u8]) -> String {
        store.put(content).expect("应当成功").hash
    }

    /// 把某 blob 的 `created_at` 与 `unreferenced_since` 设成过去，模拟宽限期已过。
    fn set_past_grace(store: &Store, hash: &str, grace_period: i64) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("时钟应当可用")
            .as_millis() as i64;
        store
            .conn_for_test()
            .execute(
                "UPDATE blobs SET created_at = ?1, unreferenced_since = ?2 WHERE hash = ?3",
                rusqlite::params![now - grace_period - 1000, now - grace_period - 500, hash],
            )
            .expect("应当可改");
    }

    /// 读回 `blobs` 表某哈希的 `unreferenced_since`。
    fn unreferenced_since(conn: &rusqlite::Connection, hash: &str) -> Option<i64> {
        conn.query_row(
            "SELECT unreferenced_since FROM blobs WHERE hash = ?1",
            rusqlite::params![hash],
            |row| row.get::<_, Option<i64>>(0),
        )
        .expect("应当可读")
    }

    /// 读回 `blobs` 表某哈希的行是否存在。
    fn row_exists(conn: &rusqlite::Connection, hash: &str) -> bool {
        conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM blobs WHERE hash = ?1)",
            rusqlite::params![hash],
            |row| row.get::<_, bool>(0),
        )
        .expect("应当可读")
    }

    /// 第一次观察：无引用且 `unreferenced_since` 为空时，行清扫只写不删。
    #[test]
    fn first_observation_writes_grace_period_start_without_deleting() {
        let mut store = Store::open_in_memory().expect("应当可以打开");
        let hash = put_blob(&mut store, b"first observation");
        let tx = store.conn_mut().transaction().expect("应当可以开事务");
        let (deleted, observed) = sweep_in(&tx, 1000, &[]).expect("应当成功");
        assert!(deleted.is_empty(), "第一次观察不应当删行");
        assert_eq!(observed, 1, "应当观察到一行");
        tx.commit().expect("应当可以提交");
        assert!(row_exists(store.conn_for_test(), &hash), "行应当还在");
        assert!(
            unreferenced_since(store.conn_for_test(), &hash).is_some(),
            "宽限期起点应当被写成非空"
        );
    }

    /// 第二次观察：无引用且超宽限期时，行清扫删行。
    #[test]
    fn second_observation_past_grace_period_deletes_row() {
        let mut store = Store::open_in_memory().expect("应当可以打开");
        let hash = put_blob(&mut store, b"past grace");
        set_past_grace(&store, &hash, 1000);
        let tx = store.conn_mut().transaction().expect("应当可以开事务");
        let (deleted, observed) = sweep_in(&tx, 1000, &[]).expect("应当成功");
        assert_eq!(deleted, vec![hash.clone()], "应当删掉这一行");
        assert_eq!(observed, 0, "不是第一次观察");
        tx.commit().expect("应当可以提交");
        assert!(!row_exists(store.conn_for_test(), &hash), "行应当被删");
    }

    /// 被快照条目引用的 blob 不被删：快照引用是标记根。
    #[test]
    fn snapshot_referenced_blob_is_kept() {
        let mut store = Store::open_in_memory().expect("应当可以打开");
        let hash = put_blob(&mut store, b"referenced");
        // 插一条引用该哈希的快照条目。
        let id = ita_kernel::context::WorkspaceId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAV")
            .expect("应当合法");
        store.create_workspace(&id, "/tmp/ws").expect("应当成功");
        store
            .conn_for_test()
            .execute(
                "INSERT INTO snapshots (id, workspace_id, seq, parent_id, kind, created_at)
                 VALUES ('01ARZ3NDEKTSV4RRFFQ69G5FB0', ?1, 1, NULL, 'baseline', 0)",
                rusqlite::params![id.as_str()],
            )
            .expect("应当可插");
        store
            .conn_for_test()
            .execute(
                "INSERT INTO snapshot_entries
                     (snapshot_id, path, kind, hash, mode, size, mtime, content_stored)
                 VALUES ('01ARZ3NDEKTSV4RRFFQ69G5FB0', 'a.txt', 'file', ?1, 420, 10, NULL, 1)",
                rusqlite::params![hash],
            )
            .expect("应当可插");
        // 即使超宽限期，被引用的 blob 也不删。
        set_past_grace(&store, &hash, 1000);
        let tx = store.conn_mut().transaction().expect("应当可以开事务");
        let (deleted, _) = sweep_in(&tx, 1000, &[]).expect("应当成功");
        assert!(deleted.is_empty(), "被引用的 blob 不应当被删");
        tx.commit().expect("应当可以提交");
        assert!(row_exists(store.conn_for_test(), &hash), "被引用的行应当保留");
    }

    /// 行先行、文件随后：行清扫删行不删文件，文件清扫在提交后才删文件。
    #[test]
    fn row_deletion_precedes_file_deletion() {
        let mut store = Store::open_in_memory().expect("应当可以打开");
        let hash = put_blob(&mut store, b"row first");
        set_past_grace(&store, &hash, 1000);
        let file = store.cas_root_for_test().join(&hash[..2]).join(&hash);
        assert!(file.exists(), "写入后文件应当存在");
        // 行清扫：删行，不删文件。
        let tx = store.conn_mut().transaction().expect("应当可以开事务");
        let (deleted, _) = sweep_in(&tx, 1000, &[]).expect("应当成功");
        assert_eq!(deleted, vec![hash.clone()], "应当删掉这一行");
        assert!(!row_exists(&tx, &hash), "行应当被删");
        assert!(file.exists(), "行清扫不应当删文件");
        tx.commit().expect("应当可以提交");
        // 文件清扫：提交后才删文件。
        let orphans =
            sweep_files(store.conn(), store.cas_root_for_test(), &deleted).expect("应当成功");
        assert_eq!(orphans, 0, "没有孤儿文件");
        assert!(!file.exists(), "文件清扫应当删文件");
    }

    /// 孤儿文件被清：没有 blob 行的哈希文件被删。
    #[test]
    fn orphan_file_without_blob_row_is_swept() {
        let mut store = Store::open_in_memory().expect("应当可以打开");
        // 写入一个 blob，再删掉它的行，留下无行文件。
        let hash = put_blob(&mut store, b"orphan file");
        store
            .conn_for_test()
            .execute("DELETE FROM blobs WHERE hash = ?1", rusqlite::params![hash])
            .expect("应当可删");
        let file = store.cas_root_for_test().join(&hash[..2]).join(&hash);
        assert!(file.exists(), "删行后文件应当还在");
        let orphans = sweep_files(store.conn(), store.cas_root_for_test(), &[]).expect("应当成功");
        assert_eq!(orphans, 1, "应当清扫一个孤儿文件");
        assert!(!file.exists(), "孤儿文件应当被删");
    }

    /// `.tmp` 文件被清：无行的 `.tmp` 文件被删。
    #[test]
    fn tmp_file_is_swept() {
        let mut store = Store::open_in_memory().expect("应当可以打开");
        // 写入一个 blob，再写一个 `.tmp` 文件。
        let hash = put_blob(&mut store, b"tmp file");
        let tmp = store.cas_root_for_test().join(&hash[..2]).join(format!("{hash}.tmp"));
        std::fs::write(&tmp, b"partial").expect("应当可写");
        let orphans = sweep_files(store.conn(), store.cas_root_for_test(), &[]).expect("应当成功");
        assert_eq!(orphans, 1, "应当清扫一个 `.tmp` 文件");
        assert!(!tmp.exists(), "`.tmp` 文件应当被删");
        // 正常的 blob 文件不受影响。
        let file = store.cas_root_for_test().join(&hash[..2]).join(&hash);
        assert!(file.exists(), "有行的 blob 文件应当保留");
    }
}
