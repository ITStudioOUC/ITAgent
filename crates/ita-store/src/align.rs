//! 回滚对齐：沿链折叠读有效状态、四种情况处理、外部改动检测、残留清单组装。
//!
//! 对齐是独立动词，不折进 `create_snapshot`：06.6 的 `create_snapshot` 只按链追加，06.7 的
//! squash 改写链，06.8 的对齐是第三种独立职责——读有效状态、对齐磁盘、输出残留清单。
//!
//! 回滚目标是链上某个快照的有效状态，需要先沿链折叠：从基线到目标（含目标）的折叠结果，
//! 不是目标快照自己的清单。折叠用共享模块 `fold` 的算法，与 06.7 的 squash 共用同一套左结合
//! 的覆盖加墓碑删除。
//!
//! 范围（工具清单声明的写集）是参数，不是存储层自己算的：存储层不解析工具清单，范围由调用方
//! 从清单读出后传入。范围外的路径不碰、只进残留清单。
//!
//! 超阈值条目（`content_stored = 0`）只报告、从不删除、从不覆盖（ADR 0017）。写回前核对 blobs，
//! 缺失内容标注"内容缺失"。存在外部改动时默认拒绝，除非显式强制（ADR 0018 第七节）。
//!
//! 吃一个已开启的事务或连接，不自行开事务。

use std::collections::BTreeSet;
use std::path::Path;

use ita_kernel::context::CheckpointId;
use rusqlite::{OptionalExtension, Transaction};

use crate::blobs;
use crate::cas;
use crate::db::StoreError;
use crate::entries::{self, SnapshotEntry};
use crate::fold;
use crate::residual::{ResidualEntry, ResidualReason};
use crate::restore;
use crate::scope::WorkspaceScope;

/// 对齐结果：残留清单。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlignResult {
    /// 残留清单：路径、尺寸、原因。
    pub residual: Vec<ResidualEntry>,
}

/// 在已开启的事务内对齐磁盘到目标快照的有效状态。
///
/// # 步骤
/// 1. 读目标快照的 seq，校验它属于本工作区。
/// 2. 读所有 seq 小于等于目标 seq 的快照，按 seq 升序，折叠它们的清单得到有效状态。
/// 3. 读工作区根路径。
/// 4. 对有效状态里的每条条目，按四种情况处理：写回、覆盖、墓碑删除、范围内新建删除。
/// 5. 对范围内磁盘上存在但不在有效状态里的路径，删除。
/// 6. 组装残留清单。
///
/// # 边界与前提
/// 接收已开启的事务，不自行开事务。范围是参数，存储层不解析工具清单。超阈值条目只报告。
/// 写回前核对 blobs，缺失内容标注"内容缺失"。存在外部改动时默认拒绝，除非显式强制。
///
/// # 副作用
/// 读文件系统、写文件系统（写回、删除、创建目录）。
pub(crate) fn align_in(
    tx: &Transaction,
    scope: &WorkspaceScope,
    target: &CheckpointId,
    range: &[String],
    force: bool,
    cas_root: &Path,
) -> Result<AlignResult, StoreError> {
    let workspace = scope.workspace();
    let mut residual = Vec::new();

    // 读目标快照的 seq，校验它属于本工作区。
    let target_seq: Option<i64> = tx
        .query_row(
            "SELECT seq FROM snapshots WHERE id = ?1 AND workspace_id = ?2",
            rusqlite::params![target.as_str(), workspace.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    let target_seq = target_seq.ok_or_else(|| StoreError::UnknownSnapshot(target.clone()))?;

    // 读所有 seq 小于等于目标 seq 的快照，按 seq 升序，折叠它们的清单得到有效状态。
    let ids: Vec<String> = {
        let mut stmt = tx.prepare(
            "SELECT id FROM snapshots
             WHERE workspace_id = ?1 AND seq <= ?2
             ORDER BY seq ASC",
        )?;
        let rows =
            stmt.query_map(rusqlite::params![workspace.as_str(), target_seq], |row| row.get(0))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    let mut manifests = Vec::new();
    for id in &ids {
        let entries = entries::read_in(
            tx,
            scope,
            &CheckpointId::parse(id.clone()).expect("库内 id 应当合法"),
        )?;
        manifests.push(entries);
    }
    let effective = fold::fold(&manifests);

    // 读工作区根路径。
    let root_path: String = tx.query_row(
        "SELECT root_path FROM workspaces WHERE id = ?1",
        rusqlite::params![workspace.as_str()],
        |row| row.get(0),
    )?;
    let root_path = Path::new(&root_path);

    // 有效状态里的路径集合，用于判断"不在有效状态里"。
    let effective_paths: BTreeSet<String> = effective
        .iter()
        .map(|e| match e {
            SnapshotEntry::File { path, .. } => path.clone(),
            SnapshotEntry::Dir { path, .. } => path.clone(),
            SnapshotEntry::Tombstone { path } => path.clone(),
        })
        .collect();

    // 对有效状态里的每条条目，按四种情况处理。
    for entry in &effective {
        match entry {
            SnapshotEntry::Dir { path, .. } => {
                // 范围外不碰，进残留清单。
                if !range.contains(path) {
                    residual.push(ResidualEntry {
                        path: path.clone(),
                        size: 0,
                        reason: ResidualReason::OutOfRange,
                    });
                    continue;
                }
                // 范围内：目录不存在则创建，已存在则不碰。
                if !restore::exists(root_path, path) {
                    restore::create_dir(root_path, path)?;
                }
            }
            SnapshotEntry::File { path, hash, size, content_stored, .. } => {
                // 范围外不碰，进残留清单。
                if !range.contains(path) {
                    residual.push(ResidualEntry {
                        path: path.clone(),
                        size: *size,
                        reason: ResidualReason::OutOfRange,
                    });
                    continue;
                }
                // 超阈值条目只报告，从不删除、从不覆盖。
                if !content_stored {
                    residual.push(ResidualEntry {
                        path: path.clone(),
                        size: *size,
                        reason: ResidualReason::Oversized,
                    });
                    continue;
                }
                // 写回前核对 blobs：缺失内容标注"内容缺失"。
                if !blobs::exists(tx, hash)? {
                    residual.push(ResidualEntry {
                        path: path.clone(),
                        size: *size,
                        reason: ResidualReason::MissingContent,
                    });
                    continue;
                }
                // 磁盘上没有则写回，有则读盘算哈希。
                if !restore::exists(root_path, path) {
                    let content = cas::get_in(tx, cas_root, hash)?;
                    restore::write_back(root_path, path, &content)?;
                } else {
                    let disk_content = restore::read_file(root_path, path)?;
                    let disk_hash = blake3::hash(&disk_content).to_hex().to_string();
                    if disk_hash != *hash {
                        // 外部改动：默认拒绝，除非显式强制。
                        if !force {
                            return Err(StoreError::AlignRefusedExternal(path.clone()));
                        }
                        let content = cas::get_in(tx, cas_root, hash)?;
                        restore::write_back(root_path, path, &content)?;
                    }
                }
            }
            SnapshotEntry::Tombstone { .. } => {
                // 折叠后不含墓碑，但防御性处理。
                unreachable!("折叠后不含墓碑");
            }
        }
    }

    // 范围内磁盘上存在但不在有效状态里的路径，删除（范围内新建删除）。
    for path in range {
        if !effective_paths.contains(path) && restore::exists(root_path, path) {
            restore::delete(root_path, path)?;
        }
    }

    Ok(AlignResult { residual })
}
