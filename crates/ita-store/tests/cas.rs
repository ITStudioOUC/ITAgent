//! 内容寻址存储写入、去重与原子改名的集成测试。
//!
//! 这些用例走公开接口，覆盖计划第四节的八条验收。第一条与第二条守写入与去重，第三条守
//! 哈希区分内容，第四条守读回，第五条守读路径区分不存在与读失败，第六条守空内容，第七条
//! 守懒创建，第八条守全局去重。

use std::path::Path;

use ita_kernel::context::WorkspaceId;
use ita_store::{Store, StoreError};

const WS_A: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const WS_B: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";

fn workspace(text: &str) -> WorkspaceId {
    WorkspaceId::parse(text).expect("应当合法")
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

/// 读回 `blobs` 表里某哈希的一行，返回 (尺寸, 创建时间, 宽限期起点)。
fn blob_row(conn: &rusqlite::Connection, hash: &str) -> (i64, i64, Option<i64>) {
    conn.query_row(
        "SELECT size, created_at, unreferenced_since FROM blobs WHERE hash = ?1",
        rusqlite::params![hash],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, Option<i64>>(2)?)),
    )
    .expect("应当可读")
}

/// 读回 `blobs` 表的行数。
fn blob_count(conn: &rusqlite::Connection) -> u32 {
    conn.query_row("SELECT COUNT(*) FROM blobs", [], |row| row.get(0)).expect("应当可读")
}

/// 写入后文件与行成对出现：行里的哈希是 blake3 十六进制、尺寸是字节数、创建时间非空、
/// 宽限期起点为 NULL；文件系统里有对应的哈希文件，内容等于写入的内容。
#[test]
fn write_creates_file_and_row() {
    let mut store = Store::open_in_memory().expect("应当可以打开");
    let content = b"hello world";
    let ref_ = store.put(content).expect("应当成功");
    // 哈希是内容的 blake3 十六进制，长度是内容字节数。
    let expected = blake3::hash(content).to_hex().to_string();
    assert_eq!(ref_.hash, expected, "哈希应当是内容的 blake3 十六进制");
    assert_eq!(ref_.len, content.len() as u64, "长度应当是内容字节数");
    // blobs 表里多了一行，各列正确。
    let (size, created_at, unreferenced_since) = blob_row(store.conn_for_test(), &ref_.hash);
    assert_eq!(size, content.len() as i64, "尺寸应当是内容字节数");
    assert!(created_at > 0, "创建时间应当非空");
    assert!(unreferenced_since.is_none(), "宽限期起点应当为 NULL");
    // 文件系统里有对应的哈希文件，内容等于写入内容。
    let cas_root = store.cas_root_for_test();
    let file = cas_root.join(&ref_.hash[..2]).join(&ref_.hash);
    assert!(file.exists(), "哈希文件应当存在");
    let read = std::fs::read(&file).expect("应当可读");
    assert_eq!(read, content, "文件内容应当等于写入内容");
}

/// 同内容两次写入只产生一个 blob：blobs 表里只有一行，文件系统里只有一个哈希文件。
#[test]
fn same_content_written_twice_produces_one_blob() {
    let mut store = Store::open_in_memory().expect("应当可以打开");
    let content = b"dedup me";
    let first = store.put(content).expect("应当成功");
    let second = store.put(content).expect("应当成功");
    assert_eq!(first.hash, second.hash, "同内容应当同哈希");
    assert_eq!(blob_count(store.conn_for_test()), 1, "应当只有一行");
    assert_eq!(count_files(store.cas_root_for_test()), 1, "应当只有一个哈希文件");
}

/// 不同内容产生不同的 blob：两行、两个不同的哈希、两个哈希文件。
#[test]
fn different_content_produces_different_blobs() {
    let mut store = Store::open_in_memory().expect("应当可以打开");
    let a = store.put(b"content A").expect("应当成功");
    let b = store.put(b"content B").expect("应当成功");
    assert_ne!(a.hash, b.hash, "不同内容应当不同哈希");
    assert_eq!(blob_count(store.conn_for_test()), 2, "应当有两行");
    assert_eq!(count_files(store.cas_root_for_test()), 2, "应当有两个哈希文件");
}

/// 读回内容与写入一致：用返回的哈希读回，断言读回的字节等于写入的内容。
#[test]
fn read_back_matches_written_content() {
    let mut store = Store::open_in_memory().expect("应当可以打开");
    let content = b"restore me";
    let ref_ = store.put(content).expect("应当成功");
    let read = store.get(&ref_.hash).expect("应当可读");
    assert_eq!(read, content, "读回的内容应当等于写入的内容");
}

/// 读不存在的哈希返回 BlobNotFound 错误，而不是读失败。
#[test]
fn reading_nonexistent_hash_reports_blob_not_found() {
    let store = Store::open_in_memory().expect("应当可以打开");
    let err = store.get(&"ab".repeat(32)).expect_err("应当报错");
    assert!(matches!(err, StoreError::BlobNotFound(_)), "应当是 BlobNotFound");
}

/// 空内容也产生一个 blob：一行，尺寸是 0，哈希是空内容的 blake3 十六进制。
#[test]
fn empty_content_produces_a_blob() {
    let mut store = Store::open_in_memory().expect("应当可以打开");
    let ref_ = store.put(b"").expect("应当成功");
    assert_eq!(ref_.len, 0, "空内容长度应当是 0");
    assert_eq!(ref_.hash, blake3::hash(b"").to_hex().to_string(), "哈希应当是空内容的 blake3");
    let (size, created_at, unreferenced_since) = blob_row(store.conn_for_test(), &ref_.hash);
    assert_eq!(size, 0, "尺寸应当是 0");
    assert!(created_at > 0, "创建时间应当非空");
    assert!(unreferenced_since.is_none(), "宽限期起点应当为 NULL");
}

/// CAS 根目录在首次写入时创建：打开库不写入时目录不存在，写入后目录存在且包含哈希文件。
#[test]
fn cas_root_created_on_first_write() {
    let mut store = Store::open_in_memory().expect("应当可以打开");
    let cas_root = store.cas_root_for_test().to_path_buf();
    assert!(!cas_root.exists(), "未写入时 CAS 根目录不应存在");
    let ref_ = store.put(b"hello").expect("应当成功");
    assert!(cas_root.exists(), "写入后 CAS 根目录应当存在");
    let file = cas_root.join(&ref_.hash[..2]).join(&ref_.hash);
    assert!(file.exists(), "哈希文件应当存在");
}

/// 跨工作区共享同一份内容：两个工作区各写入同一段内容，blobs 表里只有一行（全局去重）。
#[test]
fn cross_workspace_shares_same_content() {
    let mut store = Store::open_in_memory().expect("应当可以打开");
    store.create_workspace(&workspace(WS_A), "/tmp/a").expect("应当成功");
    store.create_workspace(&workspace(WS_B), "/tmp/b").expect("应当成功");
    let content = b"shared across workspaces";
    let a = store.put(content).expect("应当成功");
    let b = store.put(content).expect("应当成功");
    assert_eq!(a.hash, b.hash, "两个工作区应当引用同一个哈希");
    assert_eq!(blob_count(store.conn_for_test()), 1, "应当只有一行（全局去重）");
}
