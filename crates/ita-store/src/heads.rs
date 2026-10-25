//! head 的比较并交换原语。
//!
//! head 存在 `sessions` 表上，不是独立的 head 表。比较并交换是一条 `UPDATE` 同时匹配
//! `(head_node, head_generation)`，影响行数为零即冲突。
//!
//! 只比节点会在"回退到祖先再前进"的 ABA 情形下失效：节点 id 重新变成"当前值"，一个过期的
//! 写入会通过比较。代次是单调的，因此能区分"同一个节点，但不是同一次推进"。这条与内核
//! `Tree::append` 的语义一致，落库版本不能把代次丢掉。
//!
//! 这个原语是通用的：它吃 `(会话, 期望节点, 期望代次, 新节点)`，不关心新节点是帧还是回退目标。
//! 帧提交是它的一个消费者，回退是另一个。本批只接线帧提交。

use ita_kernel::context::{NodeId, SessionId};
use rusqlite::{Connection, Transaction};

use crate::db::StoreError;

/// 读一个会话当前的 head 令牌。
///
/// # 步骤
/// 1. 查询该会话的 `head_node` 与 `head_generation`。
///
/// # 边界与前提
/// 会话不存在时返回 [`StoreError::UnknownSession`]。
///
/// # 副作用
/// 只读。
///
/// # 注意事项
/// 本批只接线帧提交，读回退与事件读路径在后续批次才消费它，因此这里允许暂时未被调用。
#[allow(dead_code)]
pub(crate) fn read_head(
    conn: &Connection,
    session: &SessionId,
) -> Result<(NodeId, u64), StoreError> {
    conn.query_row(
        "SELECT head_node, head_generation FROM sessions WHERE id = ?1",
        rusqlite::params![session.as_str()],
        |row| {
            let node: String = row.get(0)?;
            let generation: u64 = row.get(1)?;
            Ok((NodeId::parse(node).expect("库内 id 应当合法"), generation))
        },
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => StoreError::UnknownSession(session.clone()),
        other => StoreError::Sqlite(other),
    })
}

/// 执行 head 的比较并交换。
///
/// # 步骤
/// 1. 执行一条 `UPDATE`，同时匹配 `(head_node, head_generation)`。
/// 2. 把 `head_node` 设为新节点，`head_generation` 加一。
/// 3. 返回影响行数。
///
/// # 边界与前提
/// 接收一个已开启的事务，不自行开事务。影响行数为零即冲突，由调用方解释。
///
/// # 副作用
/// 写 `sessions`。
pub(crate) fn compare_and_swap(
    tx: &Transaction,
    session: &SessionId,
    expected_node: &NodeId,
    expected_generation: u64,
    new_node: &NodeId,
) -> Result<usize, StoreError> {
    tx.execute(
        "UPDATE sessions
         SET head_node = ?2, head_generation = head_generation + 1
         WHERE id = ?1 AND head_node = ?3 AND head_generation = ?4",
        rusqlite::params![
            session.as_str(),
            new_node.as_str(),
            expected_node.as_str(),
            expected_generation
        ],
    )
    .map_err(StoreError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Store;
    use ita_kernel::context::WorkspaceId;

    const WS: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    const SESSION: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAX";
    const NODE_A: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    const NODE_B: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";

    /// 建一个带会话与节点的库，返回 (库, 会话, 节点 A, 节点 B)。
    fn setup() -> (Store, SessionId, NodeId, NodeId) {
        let mut store = Store::open_in_memory().expect("应当可以打开");
        let ws = WorkspaceId::parse(WS).expect("应当合法");
        store.create_workspace(&ws, "/tmp/ws").expect("应当成功");
        let session = SessionId::parse(SESSION).expect("应当合法");
        store
            .conn_for_test()
            .execute(
                "INSERT INTO sessions (id, workspace_id, head_node, head_generation, created_at)
                 VALUES (?1, ?2, ?3, 0, 0)",
                rusqlite::params![SESSION, WS, NODE_A],
            )
            .expect("应当可插");
        store
            .conn_for_test()
            .execute(
                "INSERT INTO nodes (id, workspace_id, session_id, body_kind, created_at)
                 VALUES (?1, ?2, ?3, 'turn', 0)",
                rusqlite::params![NODE_A, WS, SESSION],
            )
            .expect("应当可插");
        store
            .conn_for_test()
            .execute(
                "INSERT INTO nodes (id, workspace_id, session_id, body_kind, created_at)
                 VALUES (?1, ?2, ?3, 'turn', 0)",
                rusqlite::params![NODE_B, WS, SESSION],
            )
            .expect("应当可插");
        (
            store,
            session,
            NodeId::parse(NODE_A).expect("应当合法"),
            NodeId::parse(NODE_B).expect("应当合法"),
        )
    }

    /// 读回当前 head 令牌，节点与代次都正确。
    #[test]
    fn read_head_returns_current_token() {
        let (store, session, node_a, _) = setup();
        let (node, generation) = read_head(store.conn(), &session).expect("应当可读");
        assert_eq!(node, node_a, "head 应当指向节点 A");
        assert_eq!(generation, 0, "初始代次为 0");
    }

    /// 不存在的会话读 head 报未知会话，而不是返回空值。
    #[test]
    fn read_head_on_unknown_session_reports_unknown() {
        let (store, _, _, _) = setup();
        let ghost = SessionId::parse("01ARZ3NDEKTSV4RRFFQ69G5FA6").expect("应当合法");
        let err = read_head(store.conn(), &ghost).expect_err("应当报错");
        assert!(matches!(err, StoreError::UnknownSession(_)));
    }

    /// 节点与代次都匹配时 CAS 成功，head 移到新节点、代次加一。
    #[test]
    fn compare_and_swap_succeeds_when_token_matches() {
        let (mut store, session, node_a, node_b) = setup();
        let tx = store.conn_mut().transaction().expect("应当可以开事务");
        let changed = compare_and_swap(&tx, &session, &node_a, 0, &node_b).expect("应当成功");
        assert_eq!(changed, 1, "应当影响一行");
        tx.commit().expect("应当可以提交");
        let (node, generation) = read_head(store.conn(), &session).expect("应当可读");
        assert_eq!(node, node_b, "head 应当移到节点 B");
        assert_eq!(generation, 1, "代次应当加一");
    }

    /// 节点不符时 CAS 冲突，影响行数为零，head 不动。
    #[test]
    fn compare_and_swap_conflicts_on_node_mismatch() {
        let (mut store, session, _, node_b) = setup();
        let tx = store.conn_mut().transaction().expect("应当可以开事务");
        let changed = compare_and_swap(&tx, &session, &node_b, 0, &node_b).expect("应当成功");
        assert_eq!(changed, 0, "节点不符应当影响零行");
        tx.rollback().expect("应当可以回滚");
    }

    /// 代次不符时 CAS 冲突，影响行数为零——这是 ABA 防护。
    #[test]
    fn compare_and_swap_conflicts_on_generation_mismatch() {
        let (mut store, session, node_a, node_b) = setup();
        // 先把代次推进到 1（head 仍是节点 A）。
        store
            .conn_for_test()
            .execute(
                "UPDATE sessions SET head_generation = 1 WHERE id = ?1",
                rusqlite::params![SESSION],
            )
            .expect("应当可改");
        let tx = store.conn_mut().transaction().expect("应当可以开事务");
        // 期望代次 0，但实际是 1：应当冲突。
        let changed = compare_and_swap(&tx, &session, &node_a, 0, &node_b).expect("应当成功");
        assert_eq!(changed, 0, "代次不符应当影响零行");
        tx.rollback().expect("应当可以回滚");
    }
}
