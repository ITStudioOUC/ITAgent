//! 清单折叠：把链上多个快照的清单折叠成一个有效状态。
//!
//! 折叠是左结合的覆盖加墓碑删除操作：从空映射开始，按链序（基线到目标）遍历每个清单，
//! 墓碑删掉该路径，文件与目录覆盖既有。折叠满足结合律，squash 与回滚对齐共用这一套算法，
//! 因此提取为共享模块，squash 与 align 共同调用，不各写一份。
//!
//! 纯计算，无文件系统访问，不吃事务。

use std::collections::BTreeMap;

use crate::entries::SnapshotEntry;

/// 折叠一组清单，得到有效状态。
///
/// # 步骤
/// 1. 从空映射开始，按链序（基线到目标）遍历每个清单。
/// 2. 对每条条目：墓碑删掉该路径，文件与目录覆盖既有。
///
/// # 边界与前提
/// 输入是链序的清单，输出是有效状态。折叠结果只含 file 与 dir，不含墓碑。超阈值条目
/// （`content_stored = 0`）保留原样：折叠只覆盖路径，不改条目的字段。
///
/// # 副作用
/// 无。纯计算，无文件系统访问。
pub(crate) fn fold(manifests: &[Vec<SnapshotEntry>]) -> Vec<SnapshotEntry> {
    let mut map: BTreeMap<String, SnapshotEntry> = BTreeMap::new();
    for manifest in manifests {
        for entry in manifest {
            match entry {
                SnapshotEntry::Tombstone { path } => {
                    map.remove(path);
                }
                other => {
                    let path = match other {
                        SnapshotEntry::File { path, .. } => path,
                        SnapshotEntry::Dir { path, .. } => path,
                        _ => unreachable!("墓碑已在上面处理"),
                    };
                    map.insert(path.clone(), other.clone());
                }
            }
        }
    }
    map.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 折叠是左结合的覆盖加墓碑删除：墓碑删掉路径，文件与目录覆盖既有。
    #[test]
    fn fold_is_left_associative_overwrite_with_tombstone_delete() {
        let manifests = vec![
            vec![
                SnapshotEntry::File {
                    path: "a.txt".into(),
                    hash: "aa".repeat(32),
                    size: 1,
                    mode: 0o644,
                    content_stored: false,
                    mtime: Some(1),
                },
                SnapshotEntry::Dir { path: "src".into(), mode: 0o755, mtime: 1 },
            ],
            vec![
                SnapshotEntry::File {
                    path: "a.txt".into(),
                    hash: "bb".repeat(32),
                    size: 2,
                    mode: 0o644,
                    content_stored: false,
                    mtime: Some(2),
                },
                SnapshotEntry::Tombstone { path: "src".into() },
            ],
            vec![SnapshotEntry::File {
                path: "b.txt".into(),
                hash: "cc".repeat(32),
                size: 3,
                mode: 0o644,
                content_stored: false,
                mtime: Some(3),
            }],
        ];
        let folded = fold(&manifests);
        // a.txt 被 I1 覆盖（size 2），src 被墓碑删除，b.txt 新增。
        assert_eq!(folded.len(), 2, "应当折叠出两条：a.txt 与 b.txt");
        assert!(
            folded.iter().any(|e| matches!(e, SnapshotEntry::File { path, size, .. }
                if path == "a.txt" && *size == 2)),
            "a.txt 应当被 I1 覆盖"
        );
        assert!(
            folded.iter().any(|e| matches!(e, SnapshotEntry::File { path, .. }
                if path == "b.txt")),
            "b.txt 应当新增"
        );
        assert!(
            !folded.iter().any(|e| matches!(e, SnapshotEntry::Dir { path, .. }
                if path == "src")),
            "src 应当被墓碑删除"
        );
    }

    /// 超阈值条目在折叠中保留原样：`content_stored = 0` 的条目折叠后仍是超阈值条目。
    #[test]
    fn fold_preserves_over_threshold_entries() {
        let manifests = vec![
            vec![SnapshotEntry::File {
                path: "big.bin".into(),
                hash: "dd".repeat(32),
                size: 2_000_000,
                mode: 0o644,
                content_stored: false,
                mtime: Some(1_700_000_000_000),
            }],
            vec![SnapshotEntry::File {
                path: "other.txt".into(),
                hash: "ee".repeat(32),
                size: 10,
                mode: 0o644,
                content_stored: false,
                mtime: Some(1_700_000_001_000),
            }],
        ];
        let folded = fold(&manifests);
        assert!(
            folded.iter().any(
                |e| matches!(e, SnapshotEntry::File { path, hash, size, content_stored, mtime, .. }
                if path == "big.bin"
                    && hash == &"dd".repeat(32)
                    && *size == 2_000_000
                    && !content_stored
                    && mtime == &Some(1_700_000_000_000))
            ),
            "超阈值条目应当原样保留"
        );
    }

    /// 折叠结果不含墓碑：被墓碑删除的路径不在结果里。
    #[test]
    fn fold_result_contains_no_tombstones() {
        let manifests = vec![
            vec![SnapshotEntry::File {
                path: "gone.txt".into(),
                hash: "ff".repeat(32),
                size: 5,
                mode: 0o644,
                content_stored: false,
                mtime: Some(1),
            }],
            vec![SnapshotEntry::Tombstone { path: "gone.txt".into() }],
        ];
        let folded = fold(&manifests);
        assert!(folded.is_empty(), "被墓碑删除的路径不在结果里");
    }
}
