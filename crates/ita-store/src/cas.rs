//! 内容寻址存储的写入、去重与原子改名。
//!
//! 正文不进数据库，按 blake3 哈希存到独立目录，文件名就是哈希。目录是哈希的前两个十六进制
//! 字符，文件是完整哈希。写入必须原子：先写临时文件、同步、再改名到最终路径，绝不出现
//! "文件名已是哈希但内容是半截"的状态。
//!
//! 去重是"先查 blobs 行，存在则跳过写入"。单写者没有并发竞争，插行用 `INSERT OR IGNORE`
//! 保幂等。
//!
//! 写入顺序是刻意的：文件先行、行随后。中断只留下无引用文件（安全，多占空间），不留下
//! 指向空内容的行（危险，丢内容）。

use std::io::Write;
use std::path::{Path, PathBuf};

use ita_kernel::context::BlobRef;
use rusqlite::{Connection, Transaction};

use crate::blobs;
use crate::db::{StoreError, now_millis};

/// 计算内容的 blake3 哈希，十六进制文本。
///
/// blake3 的哈希是 256 位（32 字节），十六进制文本是 64 字符。
fn hash_hex(content: &[u8]) -> String {
    blake3::hash(content).to_hex().to_string()
}

/// 哈希对应的文件路径：CAS 根目录下，前两个十六进制字符是子目录，完整哈希是文件名。
///
/// 回收的文件清扫也按这个布局删文件，因此路径推导是 crate 内部共享的。
pub(crate) fn blob_path(cas_root: &Path, hash: &str) -> PathBuf {
    cas_root.join(&hash[..2]).join(hash)
}

/// 把内容原子写入 CAS 目录：临时文件与最终路径同目录，写满后同步，再改名到最终路径。
///
/// # 步骤
/// 1. 懒创建子目录。
/// 2. 在同目录创建临时文件，写入内容。
/// 3. 同步临时文件。
/// 4. 改名到最终路径。
///
/// # 边界与前提
/// 临时文件与最终路径同目录，改名才是原子的。固定后缀 `.tmp`，单写者没有并发竞争，崩溃后
/// 留下的 `.tmp` 文件是无引用对象，回收会清掉。
///
/// # 副作用
/// 写 CAS 目录下的文件。
fn write_atomically(cas_root: &Path, hash: &str, content: &[u8]) -> Result<(), StoreError> {
    let path = blob_path(cas_root, hash);
    // 懒创建子目录：不为不用 CAS 的库建空目录。
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(StoreError::Cas)?;
    }
    // 临时文件与最终路径同目录，改名才是原子的。
    let tmp = path.with_extension("tmp");
    let mut file = std::fs::File::create(&tmp).map_err(StoreError::Cas)?;
    file.write_all(content).map_err(StoreError::Cas)?;
    // 同步临时文件，保证内容在改名前已持久。
    file.sync_data().map_err(StoreError::Cas)?;
    // 改名到最终路径。Linux 的 rename 是原子的，会原子替换已存在的目标。
    std::fs::rename(&tmp, &path).map_err(StoreError::Cas)?;
    Ok(())
}

/// 在已开启的事务内写入一段内容，返回内容寻址引用。
///
/// # 步骤
/// 1. 计算内容的 blake3 哈希。
/// 2. 查 blobs 行：存在则跳过写入，直接返回哈希与长度。
/// 3. 不存在则先原子写入文件，再插入行。
///
/// # 边界与前提
/// 接收已开启的事务，不自行开事务。文件写入在行插入之前，顺序由代码结构保证。
///
/// # 副作用
/// 写 CAS 目录下的文件，写 `blobs` 表。
pub(crate) fn put_in(
    tx: &Transaction,
    cas_root: &Path,
    content: &[u8],
) -> Result<BlobRef, StoreError> {
    let hash = hash_hex(content);
    let len = u64::try_from(content.len()).expect("内容长度不超 u64");

    // 去重那一步：先查行，存在则跳过写入。
    if blobs::exists(tx, &hash)? {
        return Ok(BlobRef { hash, len });
    }

    // 文件先行：原子写入。
    write_atomically(cas_root, &hash, content)?;
    // 行随后：INSERT OR IGNORE 保幂等。
    blobs::insert(tx, &hash, len, now_millis()?)?;

    Ok(BlobRef { hash, len })
}

/// 按哈希读回内容。
///
/// # 边界与前提
/// 行不存在返回 [`StoreError::BlobNotFound`]；行存在但文件读失败返回 [`StoreError::Cas`]。
/// 只读，不自行开事务。
///
/// # 副作用
/// 读文件系统。
pub(crate) fn get_in(
    conn: &Connection,
    cas_root: &Path,
    hash: &str,
) -> Result<Vec<u8>, StoreError> {
    let exists = blobs::exists(conn, hash)?;
    if !exists {
        return Err(StoreError::BlobNotFound(hash.to_owned()));
    }
    std::fs::read(blob_path(cas_root, hash)).map_err(StoreError::Cas)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 哈希是 64 字符的十六进制：blake3 是 256 位。
    #[test]
    fn hash_is_sixty_four_hex_chars() {
        let hash = hash_hex(b"hello");
        assert_eq!(hash.len(), 64, "blake3 是 256 位，十六进制是 64 字符");
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()), "应当全是十六进制字符");
    }

    /// 同内容同哈希、不同内容不同哈希：哈希是内容的函数。
    #[test]
    fn hash_is_a_function_of_content() {
        assert_eq!(hash_hex(b"abc"), hash_hex(b"abc"), "同内容应当同哈希");
        assert_ne!(hash_hex(b"abc"), hash_hex(b"abd"), "不同内容应当不同哈希");
    }

    /// 文件路径是 CAS 根目录下的前两个十六进制字符子目录加完整哈希文件名。
    #[test]
    fn blob_path_layout_is_two_char_dir_then_full_hash() {
        let hash = hash_hex(b"hello");
        let path = blob_path(Path::new("/tmp/blobs"), &hash);
        assert_eq!(
            path,
            Path::new("/tmp/blobs").join(&hash[..2]).join(&hash),
            "布局应当是 blobs/<2字符>/<完整哈希>"
        );
    }
}
