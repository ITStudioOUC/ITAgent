//! 帧写入与 head 比较并交换的集成测试。
//!
//! 覆盖计划第四节的九条验收：正确期望成功、节点不符冲突、代次不符（ABA）冲突、跨工作区
//! 会话/父节点/覆盖列表被拒、帧正文可逆、重复节点 id 被拒、冲突时事件不追加。

use ita_kernel::context::compress::CommitPlan;
use ita_kernel::context::{CheckpointId, NodeId, SessionId, WorkspaceId};
use ita_store::{Store, StoreError, WorkspaceScope, decode_frame};

const WS_A: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const WS_B: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";
const SESSION_A: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAX";
const SESSION_B: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAY";
const NODE_X: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const NODE_Y: &str = "01ARZ3NDEKTSV4RRFFQ69G5FA6";
const NODE_F: &str = "01ARZ3NDEKTSV4RRFFQ69G5FA2";
const NODE_F2: &str = "01ARZ3NDEKTSV4RRFFQ69G5FA7";
const COVER_1: &str = "01ARZ3NDEKTSV4RRFFQ69G5FA3";
const COVER_2: &str = "01ARZ3NDEKTSV4RRFFQ69G5FA4";
const NODE_B1: &str = "01ARZ3NDEKTSV4RRFFQ69G5FA5";
const CP: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

fn workspace(text: &str) -> WorkspaceId {
    WorkspaceId::parse(text).expect("应当合法")
}

fn node(text: &str) -> NodeId {
    NodeId::parse(text).expect("应当合法")
}

/// 读回某工作区的 `workspace_seq.next_seq`。
fn next_seq(conn: &rusqlite::Connection, ws: &str) -> i64 {
    conn.query_row(
        "SELECT next_seq FROM workspace_seq WHERE workspace_id = ?1",
        rusqlite::params![ws],
        |row| row.get(0),
    )
    .expect("应当可读")
}

/// 建工作区 A，会话 head 指向 NODE_X 第 1 代，并插入根节点、NODE_Y 与两个覆盖节点。
fn setup_a() -> (Store, WorkspaceScope, SessionId, NodeId) {
    let mut store = Store::open_in_memory().expect("应当可以打开");
    store.create_workspace(&workspace(WS_A), "/tmp/a").expect("应当成功");
    let scope = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    let session = SessionId::parse(SESSION_A).expect("应当合法");
    store
        .conn_for_test()
        .execute(
            "INSERT INTO sessions (id, workspace_id, head_node, head_generation, created_at)
             VALUES (?1, ?2, ?3, 1, 0)",
            rusqlite::params![SESSION_A, WS_A, NODE_X],
        )
        .expect("应当可插");
    for (id, parent) in
        [(NODE_X, None), (NODE_Y, None), (COVER_1, Some(NODE_X)), (COVER_2, Some(NODE_X))]
    {
        store
            .conn_for_test()
            .execute(
                "INSERT INTO nodes (id, workspace_id, session_id, parent_id, body_kind, created_at)
                 VALUES (?1, ?2, ?3, ?4, 'turn', 0)",
                rusqlite::params![id, WS_A, SESSION_A, parent],
            )
            .expect("应当可插");
    }
    (store, scope, session, node(NODE_X))
}

/// 在工作区 B 注册一个会话与一个节点。
fn register_b(store: &mut Store) {
    store.create_workspace(&workspace(WS_B), "/tmp/b").expect("应当成功");
    store
        .conn_for_test()
        .execute(
            "INSERT INTO sessions (id, workspace_id, head_node, head_generation, created_at)
             VALUES (?1, ?2, ?3, 0, 0)",
            rusqlite::params![SESSION_B, WS_B, NODE_X],
        )
        .expect("应当可插");
    store
        .conn_for_test()
        .execute(
            "INSERT INTO nodes (id, workspace_id, session_id, body_kind, created_at)
             VALUES (?1, ?2, ?3, 'turn', 0)",
            rusqlite::params![NODE_B1, WS_B, SESSION_B],
        )
        .expect("应当可插");
}

/// 构造一个提交计划。
fn plan(parent: &NodeId, generation: u64, covers: Vec<NodeId>) -> CommitPlan {
    CommitPlan {
        parent: parent.clone(),
        parent_generation: generation,
        covers,
        summary: "摘要".to_owned(),
        fixed_layer_fingerprint: "fp".to_owned(),
        checkpoint: None,
        prompt_version: 1,
    }
}

/// 正确期望 head 提交帧成功，head 移到新帧节点、代次加一、seq 消耗一个。
#[test]
fn correct_expected_head_commits_frame() {
    let (mut store, scope, session, root) = setup_a();
    let before = next_seq(store.conn_for_test(), WS_A);
    let new_node = node(NODE_F);
    let plan = plan(&root, 1, vec![node(COVER_1)]);
    let committed =
        store.commit_frame(&scope, &session, &new_node, &plan, "actor").expect("应当成功");
    assert_eq!(committed.node, new_node, "应当返回新帧节点");
    assert_eq!(committed.generation, 2, "新代次应当是 2");
    let (kind, parent): (String, String) = store
        .conn_for_test()
        .query_row(
            "SELECT body_kind, parent_id FROM nodes WHERE id = ?1",
            rusqlite::params![NODE_F],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("应当可读");
    assert_eq!(kind, "frame", "body_kind 应当是 frame");
    assert_eq!(parent, NODE_X, "parent_id 应当是 X");
    let (head, generation): (String, i64) = store
        .conn_for_test()
        .query_row(
            "SELECT head_node, head_generation FROM sessions WHERE id = ?1",
            rusqlite::params![SESSION_A],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("应当可读");
    assert_eq!(head, NODE_F, "head 应当是帧节点");
    assert_eq!(generation, 2, "代次应当是 2");
    assert_eq!(next_seq(store.conn_for_test(), WS_A), before + 1, "seq 应当加一");
}

/// 过期期望 head（节点不符）返回冲突，帧行不落库、head 不动、seq 不变。
#[test]
fn stale_expected_head_node_mismatch_conflicts() {
    let (mut store, scope, session, _) = setup_a();
    let before = next_seq(store.conn_for_test(), WS_A);
    let new_node = node(NODE_F);
    let plan = plan(&node(NODE_Y), 1, vec![node(COVER_1)]);
    let err =
        store.commit_frame(&scope, &session, &new_node, &plan, "actor").expect_err("应当冲突");
    assert!(matches!(err, StoreError::HeadConflict), "应当是冲突错误");
    let exists: bool = store
        .conn_for_test()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM nodes WHERE id = ?1)",
            rusqlite::params![NODE_F],
            |row| row.get(0),
        )
        .expect("应当可读");
    assert!(!exists, "帧节点行不应落库");
    let (head, generation): (String, i64) = store
        .conn_for_test()
        .query_row(
            "SELECT head_node, head_generation FROM sessions WHERE id = ?1",
            rusqlite::params![SESSION_A],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("应当可读");
    assert_eq!(head, NODE_X, "head 应当仍是 X");
    assert_eq!(generation, 1, "代次应当仍是 1");
    assert_eq!(next_seq(store.conn_for_test(), WS_A), before, "seq 应当不变");
}

/// 过期期望 head（代次不符，ABA 情形）返回冲突，帧行不落库、代次保持 3。
#[test]
fn stale_expected_head_generation_mismatch_conflicts() {
    let (mut store, scope, session, root) = setup_a();
    let first = node(NODE_F);
    let plan1 = plan(&root, 1, vec![node(COVER_1)]);
    store.commit_frame(&scope, &session, &first, &plan1, "actor").expect("首次应当成功");
    // 用测试连接把 head 改回 X、代次改成 3（模拟回退再前进）。
    store
        .conn_for_test()
        .execute(
            "UPDATE sessions SET head_node = ?1, head_generation = 3 WHERE id = ?2",
            rusqlite::params![NODE_X, SESSION_A],
        )
        .expect("应当可改");
    let second = node(NODE_F2);
    let plan2 = plan(&root, 1, vec![node(COVER_2)]);
    let err = store.commit_frame(&scope, &session, &second, &plan2, "actor").expect_err("应当冲突");
    assert!(matches!(err, StoreError::HeadConflict), "应当是冲突错误");
    let exists: bool = store
        .conn_for_test()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM nodes WHERE id = ?1)",
            rusqlite::params![NODE_F2],
            |row| row.get(0),
        )
        .expect("应当可读");
    assert!(!exists, "帧节点行不应落库");
    let generation: i64 = store
        .conn_for_test()
        .query_row(
            "SELECT head_generation FROM sessions WHERE id = ?1",
            rusqlite::params![SESSION_A],
            |row| row.get(0),
        )
        .expect("应当可读");
    assert_eq!(generation, 3, "代次应当仍是 3");
}

/// 帧引用的会话属于另一工作区被拒，帧行不落库、A 的 seq 不变。
#[test]
fn frame_referencing_foreign_session_rejected() {
    let (mut store, scope, _, root) = setup_a();
    register_b(&mut store);
    let before = next_seq(store.conn_for_test(), WS_A);
    let session_b = SessionId::parse(SESSION_B).expect("应当合法");
    let new_node = node(NODE_F);
    let plan = plan(&root, 1, vec![node(COVER_1)]);
    let err =
        store.commit_frame(&scope, &session_b, &new_node, &plan, "actor").expect_err("应当被拒");
    assert!(matches!(err, StoreError::CrossWorkspace(_)), "应当是跨工作区错误");
    let exists: bool = store
        .conn_for_test()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM nodes WHERE id = ?1)",
            rusqlite::params![NODE_F],
            |row| row.get(0),
        )
        .expect("应当可读");
    assert!(!exists, "帧节点行不应落库");
    assert_eq!(next_seq(store.conn_for_test(), WS_A), before, "A 的 seq 应当不变");
}

/// 帧的父节点属于另一工作区被拒，帧行不落库、seq 不变。
#[test]
fn frame_with_foreign_parent_rejected() {
    let (mut store, scope, session, _) = setup_a();
    register_b(&mut store);
    let before = next_seq(store.conn_for_test(), WS_A);
    let new_node = node(NODE_F);
    let plan = plan(&node(NODE_B1), 1, vec![node(COVER_1)]);
    let err =
        store.commit_frame(&scope, &session, &new_node, &plan, "actor").expect_err("应当被拒");
    assert!(matches!(err, StoreError::CrossWorkspace(_)), "应当是跨工作区错误");
    let exists: bool = store
        .conn_for_test()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM nodes WHERE id = ?1)",
            rusqlite::params![NODE_F],
            |row| row.get(0),
        )
        .expect("应当可读");
    assert!(!exists, "帧节点行不应落库");
    assert_eq!(next_seq(store.conn_for_test(), WS_A), before, "seq 应当不变");
}

/// 帧的覆盖列表含另一工作区的节点被拒，帧行不落库、seq 不变。
#[test]
fn frame_with_foreign_cover_rejected() {
    let (mut store, scope, session, root) = setup_a();
    register_b(&mut store);
    let before = next_seq(store.conn_for_test(), WS_A);
    let new_node = node(NODE_F);
    let plan = plan(&root, 1, vec![node(NODE_B1)]);
    let err =
        store.commit_frame(&scope, &session, &new_node, &plan, "actor").expect_err("应当被拒");
    assert!(matches!(err, StoreError::CrossWorkspace(_)), "应当是跨工作区错误");
    let exists: bool = store
        .conn_for_test()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM nodes WHERE id = ?1)",
            rusqlite::params![NODE_F],
            |row| row.get(0),
        )
        .expect("应当可读");
    assert!(!exists, "帧节点行不应落库");
    assert_eq!(next_seq(store.conn_for_test(), WS_A), before, "seq 应当不变");
}

/// 帧正文可逆：带非空摘要、多个覆盖、检查点与指纹的帧，读出后解码逐字段相等。
#[test]
fn frame_body_is_reversible() {
    let (mut store, scope, session, root) = setup_a();
    let new_node = node(NODE_F);
    let covers = vec![node(COVER_1), node(COVER_2)];
    let plan = CommitPlan {
        parent: root.clone(),
        parent_generation: 1,
        covers: covers.clone(),
        summary: "摘要正文".to_owned(),
        fixed_layer_fingerprint: "fp-9".to_owned(),
        checkpoint: Some(CheckpointId::parse(CP).expect("应当合法")),
        prompt_version: 1,
    };
    store.commit_frame(&scope, &session, &new_node, &plan, "actor").expect("应当成功");
    let body: String = store
        .conn_for_test()
        .query_row(
            "SELECT inline_body FROM nodes WHERE id = ?1",
            rusqlite::params![NODE_F],
            |row| row.get(0),
        )
        .expect("应当可读");
    let decoded = decode_frame(&body).expect("应当可解码");
    assert_eq!(decoded.summary, "摘要正文", "摘要应当还原");
    assert_eq!(decoded.covers, covers, "覆盖应当还原");
    assert_eq!(decoded.checkpoint, plan.checkpoint, "检查点应当还原");
    assert_eq!(decoded.fixed_layer_fingerprint, "fp-9", "指纹应当还原");
    let kind: String = store
        .conn_for_test()
        .query_row("SELECT body_kind FROM nodes WHERE id = ?1", rusqlite::params![NODE_F], |row| {
            row.get(0)
        })
        .expect("应当可读");
    assert_eq!(kind, "frame", "body_kind 应当是 frame");
}

/// 重复的帧节点 id 被拒，head 不动、seq 只消耗一次。
#[test]
fn duplicate_frame_node_id_rejected() {
    let (mut store, scope, session, root) = setup_a();
    let before = next_seq(store.conn_for_test(), WS_A);
    let new_node = node(NODE_F);
    let plan1 = plan(&root, 1, vec![node(COVER_1)]);
    store.commit_frame(&scope, &session, &new_node, &plan1, "actor").expect("首次应当成功");
    // 用同一个节点 id 提交第二次。
    let plan2 = plan(&new_node, 2, vec![node(COVER_2)]);
    let err =
        store.commit_frame(&scope, &session, &new_node, &plan2, "actor").expect_err("应当被拒");
    assert!(matches!(err, StoreError::Sqlite(_)), "应当是重复节点错误");
    let (head, generation): (String, i64) = store
        .conn_for_test()
        .query_row(
            "SELECT head_node, head_generation FROM sessions WHERE id = ?1",
            rusqlite::params![SESSION_A],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("应当可读");
    assert_eq!(head, NODE_F, "head 应当仍是第一次的帧节点");
    assert_eq!(generation, 2, "代次应当仍是 2");
    assert_eq!(next_seq(store.conn_for_test(), WS_A), before + 1, "seq 只消耗一次");
}

/// 冲突时事件不追加，事件表无新增行、seq 不变。
#[test]
fn conflict_does_not_append_event() {
    let (mut store, scope, session, _) = setup_a();
    let before_seq = next_seq(store.conn_for_test(), WS_A);
    let events_before: i64 = store
        .conn_for_test()
        .query_row(
            "SELECT COUNT(*) FROM events WHERE workspace_id = ?1",
            rusqlite::params![WS_A],
            |row| row.get(0),
        )
        .expect("应当可读");
    let new_node = node(NODE_F);
    let plan = plan(&node(NODE_Y), 1, vec![node(COVER_1)]);
    let err =
        store.commit_frame(&scope, &session, &new_node, &plan, "actor").expect_err("应当冲突");
    assert!(matches!(err, StoreError::HeadConflict));
    let events_after: i64 = store
        .conn_for_test()
        .query_row(
            "SELECT COUNT(*) FROM events WHERE workspace_id = ?1",
            rusqlite::params![WS_A],
            |row| row.get(0),
        )
        .expect("应当可读");
    assert_eq!(events_after, events_before, "事件表不应新增行");
    assert_eq!(next_seq(store.conn_for_test(), WS_A), before_seq, "seq 应当不变");
}
