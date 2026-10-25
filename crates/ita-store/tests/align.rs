//! 回滚对齐的集成测试。
//!
//! 这些用例走公开接口，覆盖计划第四节的十条验收。第 1 条守范围内新建删除，第 2 条守超阈值只
//! 报告，第 3 条守范围外不碰，第 4、5 条守写回前核对 blobs 与"内容缺失"标注，第 6、7 条守外部
//! 改动检测与 force 覆盖，第 8 条守沿链折叠，第 9 条守跨工作区校验，第 10 条守空清单合法性。
//!
//! 对齐要扫工作区磁盘，因此测试用临时目录作工作区根路径，在库里注册工作区时把根路径指到临时
//! 目录，再在临时目录里建文件、调对齐、断言磁盘状态。

use std::path::{Path, PathBuf};

use ita_kernel::context::WorkspaceId;
use ita_store::{ResidualReason, SnapshotEntry, Store, StoreError};

const WS_A: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const WS_B: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";

fn workspace(text: &str) -> WorkspaceId {
    WorkspaceId::parse(text).expect("应当合法")
}

/// 建一个临时目录，返回路径。测试结束后清理。
fn temp_root() -> PathBuf {
    let path = std::env::temp_dir().join(format!("ita-align-{}", ulid::Ulid::new()));
    std::fs::create_dir_all(&path).expect("应当可以建目录");
    path
}

/// 打开一个库，注册工作区 A，根路径指向临时目录。返回 (store, scope, root)。
fn store_with_a() -> (Store, ita_store::WorkspaceScope, PathBuf) {
    let root = temp_root();
    let mut store = Store::open_in_memory().expect("应当可以打开");
    store
        .create_workspace(&workspace(WS_A), root.to_str().expect("应当是合法路径"))
        .expect("应当成功");
    let scope = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    (store, scope, root)
}

/// 在磁盘上写一个文件。
fn write_disk(root: &Path, path: &str, content: &[u8]) {
    let target = root.join(path);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).expect("应当可以建父目录");
    }
    std::fs::write(&target, content).expect("应当可以写");
}

/// 读磁盘文件内容。
fn read_disk(root: &Path, path: &str) -> Vec<u8> {
    std::fs::read(root.join(path)).expect("应当可读")
}

/// 判断磁盘路径是否存在。
fn disk_exists(root: &Path, path: &str) -> bool {
    root.join(path).exists()
}

/// 正文已存的文件条目。
fn stored_file(store: &mut Store, path: &str) -> SnapshotEntry {
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

/// 范围内新建文件被删：基线声明一个文件，增量对它写墓碑，磁盘上该路径存在（快照之后新建），
/// 调用 align，断言该路径被删除、残留清单里没有它。
#[test]
fn align_deletes_newly_created_in_range() {
    let (mut store, scope, root) = store_with_a();
    // 基线声明 a.txt。
    let entry = stored_file(&mut store, "a.txt");
    store.create_snapshot(&scope, &[entry]).expect("应当成功");
    // 增量对 a.txt 写墓碑。
    let second = store
        .create_snapshot(&scope, &[SnapshotEntry::Tombstone { path: "a.txt".into() }])
        .expect("应当成功");
    // 磁盘上 a.txt 存在（快照之后新建）。
    write_disk(&root, "a.txt", b"newly created");
    // 对齐到增量，范围是 a.txt。
    let result = store.align(&scope, &second.id, &["a.txt".to_owned()], false).expect("应当成功");
    // a.txt 被删，残留清单里没有它。
    assert!(!disk_exists(&root, "a.txt"), "a.txt 应当被删");
    assert!(!result.residual.iter().any(|e| e.path == "a.txt"), "残留清单里不应有 a.txt");
    let _ = std::fs::remove_dir_all(&root);
}

/// 超阈值条目只报告、不删除、不覆盖：有效状态里对某路径写 content_stored = 0 的超阈值条目，
/// 磁盘上该路径存在，调用 align，断言该路径不被删除、不被覆盖、进残留清单（带尺寸）。
#[test]
fn align_reports_oversized_without_deleting_or_overwriting() {
    let (mut store, scope, root) = store_with_a();
    // 基线声明一个超阈值文件。
    let created = store
        .create_snapshot(
            &scope,
            &[SnapshotEntry::File {
                path: "big.bin".into(),
                hash: "ee".repeat(32),
                size: 2_000_000,
                mode: 0o644,
                content_stored: false,
                mtime: Some(1_700_000_000_000),
            }],
        )
        .expect("应当成功");
    // 磁盘上 big.bin 存在。
    write_disk(&root, "big.bin", b"large content on disk");
    // 对齐，范围是 big.bin。
    let result =
        store.align(&scope, &created.id, &["big.bin".to_owned()], false).expect("应当成功");
    // big.bin 不被删、不被覆盖，进残留清单（带尺寸）。
    assert!(disk_exists(&root, "big.bin"), "big.bin 应当不被删");
    assert_eq!(read_disk(&root, "big.bin"), b"large content on disk", "big.bin 应当不被覆盖");
    let big =
        result.residual.iter().find(|e| e.path == "big.bin").expect("big.bin 应当在残留清单里");
    assert_eq!(big.size, 2_000_000, "残留清单应当带尺寸");
    assert_eq!(big.reason, ResidualReason::Oversized, "原因应当是超阈值");
    let _ = std::fs::remove_dir_all(&root);
}

/// 范围外不碰并在残留清单写明路径、尺寸与原因：有效状态里对某路径写条目，但该路径不在
/// range 里，调用 align，断言该路径不被删除、不被覆盖、进残留清单（带路径、尺寸、原因）。
#[test]
fn align_does_not_touch_out_of_range_and_reports() {
    let (mut store, scope, root) = store_with_a();
    // 基线声明 a.txt（正文已存）。
    let entry = stored_file(&mut store, "a.txt");
    let created = store.create_snapshot(&scope, &[entry]).expect("应当成功");
    // 磁盘上 a.txt 存在。
    write_disk(&root, "a.txt", b"out of range content");
    // 对齐，范围是空的（a.txt 不在范围里）。
    let result = store.align(&scope, &created.id, &[], false).expect("应当成功");
    // a.txt 不被删、不被覆盖，进残留清单（带路径、尺寸、原因）。
    assert!(disk_exists(&root, "a.txt"), "a.txt 应当不被删");
    assert_eq!(read_disk(&root, "a.txt"), b"out of range content", "a.txt 应当不被覆盖");
    let a = result.residual.iter().find(|e| e.path == "a.txt").expect("a.txt 应当在残留清单里");
    assert_eq!(a.reason, ResidualReason::OutOfRange, "原因应当是范围外写入");
    let _ = std::fs::remove_dir_all(&root);
}

/// 写回前核对有内容条目的 blobs：有效状态里对某路径写 content_stored = 1 的条目，但哈希不在
/// blobs 里，调用 align，断言该路径不被写回、进残留清单标注"内容缺失"。
#[test]
fn align_checks_blobs_before_write_back() {
    let (mut store, scope, root) = store_with_a();
    // 先 put 一个 blob，让写入路径的断言通过。
    let blob = store.put(b"orphan body").expect("应当成功");
    // 基线声明 orphan.txt（正文已存）。
    let created = store
        .create_snapshot(
            &scope,
            &[SnapshotEntry::File {
                path: "orphan.txt".into(),
                hash: blob.hash.clone(),
                size: 11,
                mode: 0o644,
                content_stored: true,
                mtime: None,
            }],
        )
        .expect("应当成功");
    // 删掉 blobs 行，模拟"哈希不在 blobs 里"。
    store
        .conn_for_test()
        .execute("DELETE FROM blobs WHERE hash = ?1", rusqlite::params![blob.hash])
        .expect("应当可删");
    // 对齐，范围是 orphan.txt。
    let result =
        store.align(&scope, &created.id, &["orphan.txt".to_owned()], false).expect("应当成功");
    // orphan.txt 不被写回，进残留清单标注"内容缺失"。
    assert!(!disk_exists(&root, "orphan.txt"), "orphan.txt 应当不被写回");
    let orphan = result
        .residual
        .iter()
        .find(|e| e.path == "orphan.txt")
        .expect("orphan.txt 应当在残留清单里");
    assert_eq!(orphan.reason, ResidualReason::MissingContent, "原因应当是内容缺失");
    let _ = std::fs::remove_dir_all(&root);
}

/// 缺失内容的路径不写回并在残留清单标注"内容缺失"：同上一条，断言残留清单里的原因是
/// "内容缺失"。
#[test]
fn align_marks_missing_content_in_residual_list() {
    let (mut store, scope, root) = store_with_a();
    let blob = store.put(b"missing body").expect("应当成功");
    let created = store
        .create_snapshot(
            &scope,
            &[SnapshotEntry::File {
                path: "missing.txt".into(),
                hash: blob.hash.clone(),
                size: 12,
                mode: 0o644,
                content_stored: true,
                mtime: None,
            }],
        )
        .expect("应当成功");
    store
        .conn_for_test()
        .execute("DELETE FROM blobs WHERE hash = ?1", rusqlite::params![blob.hash])
        .expect("应当可删");
    let result =
        store.align(&scope, &created.id, &["missing.txt".to_owned()], false).expect("应当成功");
    assert!(!disk_exists(&root, "missing.txt"), "missing.txt 应当不被写回");
    let missing = result
        .residual
        .iter()
        .find(|e| e.path == "missing.txt")
        .expect("missing.txt 应当在残留清单里");
    assert_eq!(missing.reason, ResidualReason::MissingContent, "残留清单里的原因应当是内容缺失");
    let _ = std::fs::remove_dir_all(&root);
}

/// 存在其它外部改动时默认拒绝：有效状态里对某路径写 content_stored = 1 的条目，磁盘上该路径
/// 存在且哈希与目标不一致，调用 align（force = false），断言返回 AlignRefusedExternal 错误、
/// 磁盘不变。
#[test]
fn align_refuses_external_change_by_default() {
    let (mut store, scope, root) = store_with_a();
    // 基线声明 ext.txt（正文已存）。
    let entry = stored_file(&mut store, "ext.txt");
    let created = store.create_snapshot(&scope, &[entry]).expect("应当成功");
    // 磁盘上 ext.txt 存在，但内容不同（哈希与目标不一致）。
    write_disk(&root, "ext.txt", b"external change");
    // 对齐，force = false，应当被拒。
    let err =
        store.align(&scope, &created.id, &["ext.txt".to_owned()], false).expect_err("应当被拒");
    assert!(matches!(err, StoreError::AlignRefusedExternal(_)), "应当是 AlignRefusedExternal");
    // 磁盘不变。
    assert_eq!(read_disk(&root, "ext.txt"), b"external change", "磁盘应当不变");
    let _ = std::fs::remove_dir_all(&root);
}

/// 显式强制时覆盖外部改动：同上一条，调用 align（force = true），断言该路径被覆盖为目标内容、
/// 磁盘哈希等于目标哈希。
#[test]
fn align_overwrites_external_change_when_forced() {
    let (mut store, scope, root) = store_with_a();
    // 基线声明 ext.txt（正文已存）。
    let entry = stored_file(&mut store, "ext.txt");
    let created = store.create_snapshot(&scope, &[entry]).expect("应当成功");
    // 磁盘上 ext.txt 存在，但内容不同。
    write_disk(&root, "ext.txt", b"external change");
    // 对齐，force = true，应当覆盖。
    store.align(&scope, &created.id, &["ext.txt".to_owned()], true).expect("应当成功");
    // ext.txt 被覆盖为目标内容。
    let expected = "body of ext.txt".to_string();
    assert_eq!(read_disk(&root, "ext.txt"), expected.as_bytes(), "ext.txt 应当被覆盖为目标内容");
    let _ = std::fs::remove_dir_all(&root);
}

/// 回滚目标是链上某个快照的有效状态，需要先沿链折叠：创建基线加若干增量，目标是最老增量，
/// 调用 align，断言对齐的是基线到目标的折叠结果（不是目标快照自己的清单）。
#[test]
fn align_folds_chain_from_baseline_to_target() {
    let (mut store, scope, root) = store_with_a();
    // 基线声明 base.txt。
    let base_entry = stored_file(&mut store, "base.txt");
    store.create_snapshot(&scope, &[base_entry]).expect("应当成功");
    // 增量声明 inc1.txt。
    let inc_entry = stored_file(&mut store, "inc1.txt");
    let inc = store.create_snapshot(&scope, &[inc_entry]).expect("应当成功");
    // 磁盘上 base.txt 与 inc1.txt 都不存在。
    // 对齐到增量，范围是 base.txt 与 inc1.txt。
    store
        .align(&scope, &inc.id, &["base.txt".to_owned(), "inc1.txt".to_owned()], false)
        .expect("应当成功");
    // base.txt 与 inc1.txt 都被写回，证明对齐的是折叠结果（含 base.txt），不是增量自己的清单。
    assert!(disk_exists(&root, "base.txt"), "base.txt 应当被写回（折叠结果）");
    assert!(disk_exists(&root, "inc1.txt"), "inc1.txt 应当被写回");
    let base_content = "body of base.txt".to_string();
    assert_eq!(read_disk(&root, "base.txt"), base_content.as_bytes(), "base.txt 应当是基线的内容");
    let _ = std::fs::remove_dir_all(&root);
}

/// 跨工作区对齐被拒：在工作区 A 创建快照，用工作区 B 的作用域对齐，断言返回 UnknownSnapshot。
#[test]
fn align_rejects_cross_workspace() {
    let (mut store, scope_a, root) = store_with_a();
    store.create_workspace(&workspace(WS_B), "/tmp/b").expect("应当成功");
    let scope_b = store.scope(&workspace(WS_B)).expect("应当可以取作用域");
    // 工作区 A 创建一个快照。
    let created = store.create_snapshot(&scope_a, &[]).expect("应当成功");
    // 用 B 的作用域对齐被拒。
    let err = store.align(&scope_b, &created.id, &[], false).expect_err("应当被拒");
    assert!(matches!(err, StoreError::UnknownSnapshot(_)), "应当是 UnknownSnapshot");
    let _ = std::fs::remove_dir_all(&root);
}

/// 空有效状态允许：对齐一个空有效状态的快照，断言成功、残留清单为空、磁盘不变。
#[test]
fn align_allows_empty_effective_state() {
    let (mut store, scope, root) = store_with_a();
    // 磁盘上预先放一个文件，对齐后应当不变。
    write_disk(&root, "keep.txt", b"keep me");
    // 创建一个空清单的快照。
    let created = store.create_snapshot(&scope, &[]).expect("应当成功");
    // 对齐，范围是空的。
    let result = store.align(&scope, &created.id, &[], false).expect("应当成功");
    // 成功、残留清单为空、磁盘不变。
    assert!(result.residual.is_empty(), "残留清单应当为空");
    assert_eq!(read_disk(&root, "keep.txt"), b"keep me", "磁盘应当不变");
    let _ = std::fs::remove_dir_all(&root);
}
