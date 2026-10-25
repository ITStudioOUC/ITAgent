//! 保留窗口判定与重定基线动词。
//!
//! 保留窗口判定数该工作区的增量快照数，达到 `max_incrementals` 时触发 squash。窗口是参数
//! 不是散落常量：ADR 0018 要求配置入口，配置结构落地前由调用方从配置读出后传入。
//!
//! 重定基线是独立动词，自主模式一律拒绝，留痕三列成组写入。重定基线等于永久放弃之前的回滚
//! 点，不可逆。会话起点的基线是硬地板，任何模式下都不能删除（基线行只更新不删除）。
//!
//! 吃一个已开启的事务，不自行开事务。

use ita_kernel::context::CheckpointId;
use rusqlite::Transaction;

use crate::db::StoreError;
use crate::entries;
use crate::scope::WorkspaceScope;
use crate::squash;

/// 重定基线的发起模式。
///
/// 枚举让"自主模式"与"用户发起"各有名字，避免布尔的"真值是自主还是用户"歧义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RebaseMode {
    /// 自主模式：agent 无人值守连续推进。重定基线一律拒绝。
    Autonomous,
    /// 用户发起：用户显式发起的调用。
    User,
}

/// 重定基线请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebaseRequest {
    /// 谁发起的重定基线（留痕三列之一）。
    pub by: String,
    /// 为什么重定基线（留痕三列之一）。
    pub reason: String,
    /// 由谁人工确认（留痕三列之一）。
    pub confirmed_by: String,
    /// 发起模式。
    pub mode: RebaseMode,
}

/// 判定是否需要 squash：数该工作区的增量快照数，达到 `max_incrementals` 时返回 true。
///
/// # 边界与前提
/// 接收已开启的事务，不自行开事务。只读。
///
/// # 副作用
/// 只读。
pub(crate) fn needs_squash(
    tx: &Transaction,
    scope: &WorkspaceScope,
    max_incrementals: usize,
) -> Result<bool, StoreError> {
    let count: i64 = tx.query_row(
        "SELECT COUNT(*) FROM snapshots
         WHERE workspace_id = ?1 AND kind = 'incremental'",
        rusqlite::params![scope.workspace().as_str()],
        |row| row.get(0),
    )?;
    Ok(count as usize >= max_incrementals)
}

/// 在已开启的事务内重定基线：删所有增量、更新基线条目与留痕三列。
///
/// # 步骤
/// 1. 自主模式返回 [`StoreError::RebaseRejectedAutonomous`]。
/// 2. 留痕三列任一为空返回 [`StoreError::RebaseMissingConfirmation`]。
/// 3. 读基线 B，折叠整条链得到有效状态。
/// 4. 删所有增量（及其条目）。
/// 5. 更新基线条目为有效状态，写入留痕三列。基线行只更新不删除。
///
/// # 边界与前提
/// 接收已开启的事务，不自行开事务。留痕三列由 CHECK 约束成组同进同出且只对基线成立，存储
/// 层在写入前显式校验三列非空，不靠 CHECK 兜底。
///
/// # 副作用
/// 写 `snapshots` 与 `snapshot_entries` 表。
pub(crate) fn rebaseline_in(
    tx: &Transaction,
    scope: &WorkspaceScope,
    req: &RebaseRequest,
) -> Result<(), StoreError> {
    // 自主模式一律拒绝。
    if req.mode == RebaseMode::Autonomous {
        return Err(StoreError::RebaseRejectedAutonomous);
    }

    // 留痕三列任一为空被拒。CHECK 是表结构层的兜底，写入路径的校验是友好错误的第一道防线。
    if req.by.is_empty() || req.reason.is_empty() || req.confirmed_by.is_empty() {
        return Err(StoreError::RebaseMissingConfirmation);
    }

    let workspace = scope.workspace();

    // 读基线 B。
    let baseline_id: String = tx.query_row(
        "SELECT id FROM snapshots
         WHERE workspace_id = ?1 AND kind = 'baseline' AND parent_id IS NULL",
        rusqlite::params![workspace.as_str()],
        |row| row.get(0),
    )?;

    // 折叠整条链得到有效状态。
    let ids: Vec<String> = {
        let mut stmt = tx.prepare(
            "SELECT id FROM snapshots
             WHERE workspace_id = ?1
             ORDER BY seq ASC",
        )?;
        let rows = stmt.query_map(rusqlite::params![workspace.as_str()], |row| row.get(0))?;
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
    let effective = squash::fold(&manifests);

    // 删所有增量（及其条目）。基线行只更新不删除。
    tx.execute(
        "DELETE FROM snapshot_entries
         WHERE snapshot_id IN (SELECT id FROM snapshots
                               WHERE workspace_id = ?1 AND kind = 'incremental')",
        rusqlite::params![workspace.as_str()],
    )?;
    tx.execute(
        "DELETE FROM snapshots
         WHERE workspace_id = ?1 AND kind = 'incremental'",
        rusqlite::params![workspace.as_str()],
    )?;

    // 更新基线条目为有效状态，写入留痕三列。
    let baseline = CheckpointId::parse(baseline_id.clone()).expect("库内 id 应当合法");
    tx.execute(
        "DELETE FROM snapshot_entries WHERE snapshot_id = ?1",
        rusqlite::params![baseline_id],
    )?;
    for entry in &effective {
        entries::insert_in(tx, &baseline, entry)?;
    }
    tx.execute(
        "UPDATE snapshots
         SET rebased_by = ?1, rebase_reason = ?2, rebase_confirmed_by = ?3
         WHERE id = ?4",
        rusqlite::params![req.by, req.reason, req.confirmed_by, baseline_id],
    )?;

    Ok(())
}
