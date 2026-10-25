//! 基线加增量 squash 的合并事务。
//!
//! squash 是独立动词，不折进 `create_snapshot`：06.6 的 `create_snapshot` 只按链追加，
//! 链改写（删最老两个增量、重挂子边、更新基线条目）是 06.7 的职责。存储层把保留窗口作为
//! 参数接收，不硬编码 64（ADR 0018 要求配置入口，配置结构落地前由调用方从配置读出后传入）。
//!
//! 合并事务的硬约束是 `parent_id` 的无动作自引用外键：删最老两个增量时，它们的子增量还在，
//! 若先删父后重挂，外键会拦住整笔。因此事务内顺序是先把子快照重挂到基线，再删父快照。
//!
//! 折叠是左结合的覆盖加墓碑删除操作：按链序（基线到目标）遍历每个快照的清单，墓碑删掉该
//! 路径，文件与目录覆盖既有。折叠满足结合律，squash 前后最新快照的有效条目相等。
//!
//! 吃一个已开启的事务，不自行开事务。

use std::collections::BTreeMap;

use ita_kernel::context::CheckpointId;
use rusqlite::{OptionalExtension, Transaction};

use crate::db::StoreError;
use crate::entries::{self, SnapshotEntry};
use crate::scope::WorkspaceScope;

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

/// 在已开启的事务内合并最老两个增量进基线。
///
/// # 步骤
/// 1. 读基线 B、最老两个增量 I1、I2，以及 I2 的子快照 I3（按 `parent_id` 查）。
/// 2. 折叠 B、I1、I2 的清单，得到新基线条目。
/// 3. 重挂子快照：`UPDATE snapshots SET parent_id = B.id WHERE id = I3.id`。
/// 4. 删 I1、I2 的条目。
/// 5. 删 I1、I2 的行。
/// 6. 替换基线条目：删 B 的旧条目，插折叠后的新条目。
///
/// # 边界与前提
/// 接收已开启的事务，不自行开事务。顺序的硬约束在第 3 步与第 5 步之间：`parent_id` 是无
/// 动作自引用外键，若先删 I2 再重挂 I3，外键会拦住整笔（I3 仍指向被删的 I2）。因此必须先
/// 重挂 I3 到 B，再删 I1、I2。重挂前校验 I3 与 B 同工作区，不等则返回错误。
///
/// # 副作用
/// 写 `snapshots` 与 `snapshot_entries` 表。
pub(crate) fn merge_in(tx: &Transaction, scope: &WorkspaceScope) -> Result<(), StoreError> {
    let workspace = scope.workspace();

    // 读基线 B。
    let baseline_id: String = tx.query_row(
        "SELECT id FROM snapshots
         WHERE workspace_id = ?1 AND kind = 'baseline' AND parent_id IS NULL",
        rusqlite::params![workspace.as_str()],
        |row| row.get(0),
    )?;

    // 读最老两个增量 I1、I2。
    let (i1_id, i2_id): (String, String) = tx.query_row(
        "SELECT first.id, second.id FROM
         (SELECT id FROM snapshots
          WHERE workspace_id = ?1 AND kind = 'incremental'
          ORDER BY seq ASC LIMIT 1) first,
         (SELECT id FROM snapshots
          WHERE workspace_id = ?1 AND kind = 'incremental'
          ORDER BY seq ASC LIMIT 1 OFFSET 1) second",
        rusqlite::params![workspace.as_str()],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;

    // 读 I2 的子快照 I3。
    let i3_id: Option<String> = tx
        .query_row(
            "SELECT id FROM snapshots WHERE parent_id = ?1",
            rusqlite::params![i2_id],
            |row| row.get(0),
        )
        .optional()?;

    // 重挂前的跨工作区校验：I3 与 B 的 workspace_id 必须相等（都等于作用域的工作区）。
    if let Some(ref i3_id) = i3_id {
        let i3_workspace: String = tx.query_row(
            "SELECT workspace_id FROM snapshots WHERE id = ?1",
            rusqlite::params![i3_id],
            |row| row.get(0),
        )?;
        if i3_workspace != workspace.as_str() {
            return Err(StoreError::CrossWorkspace(i3_id.clone()));
        }
    }

    // 折叠 B、I1、I2 的清单，得到新基线条目。
    let baseline = CheckpointId::parse(baseline_id.clone()).expect("库内 id 应当合法");
    let i1 = CheckpointId::parse(i1_id.clone()).expect("库内 id 应当合法");
    let i2 = CheckpointId::parse(i2_id.clone()).expect("库内 id 应当合法");
    let b_entries = entries::read_in(tx, scope, &baseline)?;
    let i1_entries = entries::read_in(tx, scope, &i1)?;
    let i2_entries = entries::read_in(tx, scope, &i2)?;
    let folded = fold(&[b_entries, i1_entries, i2_entries]);

    // 重挂子快照：先把 I3 挂到 B，再删 I1、I2。无动作外键的硬约束。
    if let Some(ref i3_id) = i3_id {
        tx.execute(
            "UPDATE snapshots SET parent_id = ?1 WHERE id = ?2",
            rusqlite::params![baseline_id, i3_id],
        )?;
    }

    // 删 I1、I2 的条目。
    tx.execute(
        "DELETE FROM snapshot_entries WHERE snapshot_id IN (?1, ?2)",
        rusqlite::params![i1_id, i2_id],
    )?;

    // 删 I1、I2 的行。
    tx.execute("DELETE FROM snapshots WHERE id IN (?1, ?2)", rusqlite::params![i1_id, i2_id])?;

    // 替换基线条目：删 B 的旧条目，插折叠后的新条目。
    tx.execute(
        "DELETE FROM snapshot_entries WHERE snapshot_id = ?1",
        rusqlite::params![baseline_id],
    )?;
    for entry in &folded {
        entries::insert_in(tx, &baseline, entry)?;
    }

    Ok(())
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
