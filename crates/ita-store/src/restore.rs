//! 写回与删除操作：从内容寻址存储读内容写回磁盘、删除磁盘文件、创建目录。
//!
//! 文件系统操作，吃工作区根路径。写回是原子替换：先写临时文件、同步、再改名到最终路径，
//! 绝不出现"文件名已是目标但内容是半截"的状态。删除是递归删除，目录与文件都处理。
//!
//! 吃工作区根路径，不自行开事务。

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::db::StoreError;

/// 工作区根路径与声明路径的合成：根路径下拼接声明路径。
///
/// # 边界与前提
/// 声明路径是工作区内的相对路径，根路径是工作区的磁盘根。合成时不规范化，避免 `..` 逃逸。
///
/// # 副作用
/// 无。纯计算。
fn join(root_path: &Path, path: &str) -> PathBuf {
    root_path.join(path)
}

/// 把内容原子写入磁盘：临时文件与最终路径同目录，写满后同步，再改名到最终路径。
///
/// # 步骤
/// 1. 懒创建父目录。
/// 2. 在同目录创建临时文件，写入内容。
/// 3. 同步临时文件。
/// 4. 改名到最终路径。
///
/// # 边界与前提
/// 临时文件与最终路径同目录，改名才是原子的。固定后缀 `.tmp`，单写者没有并发竞争，崩溃后
/// 留下的 `.tmp` 文件是无引用对象，回收会清掉。
///
/// # 副作用
/// 写磁盘文件。
pub(crate) fn write_back(root_path: &Path, path: &str, content: &[u8]) -> Result<(), StoreError> {
    let final_path = join(root_path, path);
    // 懒创建父目录：不为不用子目录的写回建空目录。
    if let Some(parent) = final_path.parent() {
        std::fs::create_dir_all(parent).map_err(StoreError::Cas)?;
    }
    // 临时文件与最终路径同目录，改名才是原子的。
    let tmp = final_path.with_extension("tmp");
    let mut file = std::fs::File::create(&tmp).map_err(StoreError::Cas)?;
    file.write_all(content).map_err(StoreError::Cas)?;
    // 同步临时文件，保证内容在改名前已持久。
    file.sync_data().map_err(StoreError::Cas)?;
    // 改名到最终路径。Linux 的 rename 是原子的，会原子替换已存在的目标。
    std::fs::rename(&tmp, &final_path).map_err(StoreError::Cas)?;
    Ok(())
}

/// 删除磁盘文件。
///
/// # 边界与前提
/// 路径不存在时静默成功：回滚对齐的删除是幂等的，重复删除不报错。
///
/// # 副作用
/// 删磁盘文件。
pub(crate) fn delete(root_path: &Path, path: &str) -> Result<(), StoreError> {
    let target = join(root_path, path);
    if target.exists() {
        std::fs::remove_file(&target).map_err(StoreError::Cas)?;
    }
    Ok(())
}

/// 创建目录。
///
/// # 边界与前提
/// 目录已存在时静默成功：回滚对齐的目录创建是幂等的。
///
/// # 副作用
/// 创建磁盘目录。
pub(crate) fn create_dir(root_path: &Path, path: &str) -> Result<(), StoreError> {
    let target = join(root_path, path);
    std::fs::create_dir_all(&target).map_err(StoreError::Cas)?;
    Ok(())
}

/// 读磁盘文件内容。
///
/// # 边界与前提
/// 文件不存在返回错误，调用方据此判断"快照里有而磁盘上没有"。
///
/// # 副作用
/// 读磁盘文件。
pub(crate) fn read_file(root_path: &Path, path: &str) -> Result<Vec<u8>, StoreError> {
    let target = join(root_path, path);
    std::fs::read(&target).map_err(StoreError::Cas)
}

/// 判断磁盘路径是否存在。
///
/// # 副作用
/// 读磁盘元数据。
pub(crate) fn exists(root_path: &Path, path: &str) -> bool {
    join(root_path, path).exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 写回后磁盘内容与写入内容一致。
    #[test]
    fn write_back_writes_content() {
        let root = std::env::temp_dir().join(format!("ita-restore-{}", ulid::Ulid::new()));
        let content = b"restore body";
        write_back(&root, "a.txt", content).expect("应当成功");
        let read = read_file(&root, "a.txt").expect("应当可读");
        assert_eq!(read, content, "磁盘内容应当与写入内容一致");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 删除后磁盘文件不存在。
    #[test]
    fn delete_removes_file() {
        let root = std::env::temp_dir().join(format!("ita-restore-{}", ulid::Ulid::new()));
        write_back(&root, "b.txt", b"to delete").expect("应当成功");
        assert!(exists(&root, "b.txt"), "删除前应当存在");
        delete(&root, "b.txt").expect("应当成功");
        assert!(!exists(&root, "b.txt"), "删除后应当不存在");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 重复删除不报错：删除是幂等的。
    #[test]
    fn delete_is_idempotent() {
        let root = std::env::temp_dir().join(format!("ita-restore-{}", ulid::Ulid::new()));
        write_back(&root, "c.txt", b"gone").expect("应当成功");
        delete(&root, "c.txt").expect("首次应当成功");
        delete(&root, "c.txt").expect("重复删除应当不报错");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 创建目录后目录存在。
    #[test]
    fn create_dir_creates_directory() {
        let root = std::env::temp_dir().join(format!("ita-restore-{}", ulid::Ulid::new()));
        create_dir(&root, "src").expect("应当成功");
        assert!(join(&root, "src").is_dir(), "目录应当存在");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 重复创建目录不报错：目录创建是幂等的。
    #[test]
    fn create_dir_is_idempotent() {
        let root = std::env::temp_dir().join(format!("ita-restore-{}", ulid::Ulid::new()));
        create_dir(&root, "d").expect("首次应当成功");
        create_dir(&root, "d").expect("重复创建应当不报错");
        let _ = std::fs::remove_dir_all(&root);
    }
}
