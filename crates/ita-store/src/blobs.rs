//! `blobs` 表的行操作：插入行与查行存在性。
//!
//! 吃一个已开启的连接或事务，不自行开事务。纯 SQL 操作，无文件系统访问。
//!
//! `hash` 是主键，插行用 `INSERT OR IGNORE`：即使去重检查被绕过（并发情形，虽然单写者
//! 不会发生），重复插入也不会报错。这是写入路径的幂等性，与"分配与插入同一事务"同型。

use rusqlite::Connection;

/// 插入一行。
///
/// # 边界与前提
/// `unreferenced_since` 恒为 NULL，表示"还没被观察过"——宽限期起点由回收扫描观察
/// "变为无引用"的时刻时写入，创建时间不能顶替它。
///
/// # 副作用
/// 写 `blobs` 表。
pub(crate) fn insert(
    conn: &Connection,
    hash: &str,
    size: u64,
    created_at: i64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT OR IGNORE INTO blobs (hash, size, created_at, unreferenced_since)
         VALUES (?1, ?2, ?3, NULL)",
        rusqlite::params![hash, size, created_at],
    )?;
    Ok(())
}

/// 查一行是否存在。
///
/// # 副作用
/// 只读。
pub(crate) fn exists(conn: &Connection, hash: &str) -> Result<bool, rusqlite::Error> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM blobs WHERE hash = ?1)",
        rusqlite::params![hash],
        |row| row.get(0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Store;

    /// 插入行后行存在，尺寸与创建时间正确，宽限期起点为 NULL。
    #[test]
    fn insert_creates_row_with_null_grace_period() {
        let store = Store::open_in_memory().expect("应当可以打开");
        insert(store.conn(), "aa".repeat(32).as_str(), 12, 42).expect("应当可插");
        let row = store
            .conn_for_test()
            .query_row(
                "SELECT size, created_at, unreferenced_since FROM blobs WHERE hash = ?1",
                rusqlite::params!["aa".repeat(32)],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                    ))
                },
            )
            .expect("应当可读");
        assert_eq!(row, (12, 42, None), "尺寸、创建时间应当正确，宽限期起点应当为 NULL");
    }

    /// 重复插入同一哈希不报错，行也不重复。
    #[test]
    fn repeated_insert_is_idempotent() {
        let store = Store::open_in_memory().expect("应当可以打开");
        let hash = "bb".repeat(32);
        insert(store.conn(), &hash, 5, 1).expect("首次应当可插");
        insert(store.conn(), &hash, 5, 1).expect("重复应当不报错");
        let count: u32 = store
            .conn_for_test()
            .query_row(
                "SELECT COUNT(*) FROM blobs WHERE hash = ?1",
                rusqlite::params![hash],
                |row| row.get(0),
            )
            .expect("应当可读");
        assert_eq!(count, 1, "行不应重复");
    }

    /// 不存在的哈希查出来是假，存在时是真。
    #[test]
    fn exists_distinguishes_present_and_absent() {
        let store = Store::open_in_memory().expect("应当可以打开");
        assert!(
            !exists(store.conn(), "cc".repeat(32).as_str()).expect("应当可读"),
            "未插入时应当不存在"
        );
        insert(store.conn(), "dd".repeat(32).as_str(), 0, 1).expect("应当可插");
        assert!(
            exists(store.conn(), "dd".repeat(32).as_str()).expect("应当可读"),
            "插入后应当存在"
        );
    }
}
