//! 事件追加、seq 分配与预览截断的集成测试。
//!
//! 这些用例走公开接口，覆盖计划第四节的十四条验收。前四条与第十至十三条测 seq 与 id，
//! 第五至九条测预览截断，第十四条测会话与节点归属。

use ita_kernel::context::{NodeId, SessionId, WorkspaceId};
use ita_store::{NewEvent, PreviewLimit, Store, StoreError};

const WS_A: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const WS_B: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";

fn workspace(text: &str) -> WorkspaceId {
    WorkspaceId::parse(text).expect("应当合法")
}

/// 打开一个库并注册工作区 A。
fn store_with_a() -> Store {
    let mut store = Store::open_in_memory().expect("应当可以打开");
    store.create_workspace(&workspace(WS_A), "/tmp/a").expect("应当成功");
    store
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

/// 同工作区连续追加三条事件，seq 依次为 1、2、3，且追加后计数器为 4。
#[test]
fn three_consecutive_appends_get_seq_one_two_three() {
    let mut store = store_with_a();
    let scope = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    for expected in [1, 2, 3] {
        let appended = store
            .append_event(&scope, &NewEvent::new("k", "a"), &PreviewLimit::defaults())
            .expect("应当成功");
        assert_eq!(appended.seq, expected, "第 {expected} 条事件的 seq 不符");
    }
    assert_eq!(next_seq(store.conn_for_test(), WS_A), 4, "追加后计数器应为 4");
}

/// 两个工作区各自从 1 开始：A 追加两条后，B 的第一条仍是 1，A 的第三条是 3。
#[test]
fn seq_starts_at_one_per_workspace() {
    let mut store = store_with_a();
    store.create_workspace(&workspace(WS_B), "/tmp/b").expect("应当成功");
    let scope_a = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    let scope_b = store.scope(&workspace(WS_B)).expect("应当可以取作用域");

    store
        .append_event(&scope_a, &NewEvent::new("k", "a"), &PreviewLimit::defaults())
        .expect("应当成功");
    store
        .append_event(&scope_a, &NewEvent::new("k", "a"), &PreviewLimit::defaults())
        .expect("应当成功");
    let b_first = store
        .append_event(&scope_b, &NewEvent::new("k", "a"), &PreviewLimit::defaults())
        .expect("应当成功");
    assert_eq!(b_first.seq, 1, "B 的第一条应当是 1");
    let a_third = store
        .append_event(&scope_a, &NewEvent::new("k", "a"), &PreviewLimit::defaults())
        .expect("应当成功");
    assert_eq!(a_third.seq, 3, "A 的第三条应当是 3");
}

/// seq 取自计数器而不是表里的最大值：把计数器预置成 100，追加一条得到 100。
#[test]
fn seq_comes_from_counter_not_max() {
    let mut store = store_with_a();
    let scope = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    // 先追加三条，表里最大 seq 是 3。
    for _ in 0..3 {
        store
            .append_event(&scope, &NewEvent::new("k", "a"), &PreviewLimit::defaults())
            .expect("应当成功");
    }
    // 把计数器直接改成 100。
    store
        .conn_for_test()
        .execute(
            "UPDATE workspace_seq SET next_seq = 100 WHERE workspace_id = ?1",
            rusqlite::params![WS_A],
        )
        .expect("应当可改");
    let appended = store
        .append_event(&scope, &NewEvent::new("k", "a"), &PreviewLimit::defaults())
        .expect("应当成功");
    assert_eq!(appended.seq, 100, "应当取自计数器 100，而不是 MAX(seq)+1=4");
}

/// 追加返回的 seq 与库里那一行、以及读路径读到的 seq 三者一致。
#[test]
fn appended_seq_matches_persisted_row_and_read_path() {
    let mut store = store_with_a();
    let scope = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    let first = store
        .append_event(&scope, &NewEvent::new("k", "a"), &PreviewLimit::defaults())
        .expect("应当成功");
    let second = store
        .append_event(&scope, &NewEvent::new("k", "a"), &PreviewLimit::defaults())
        .expect("应当成功");

    let read = store.events_after(&scope, 0, 10).expect("应当可读");
    assert_eq!(read.len(), 2, "应当读出两条");
    assert_eq!(read[0].seq, first.seq, "第一条的 seq 应当等于返回值");
    assert_eq!(read[1].seq, second.seq, "第二条的 seq 应当等于返回值");
    assert_eq!(read[0].id, first.id, "第一条的 id 应当等于返回值");
    assert_eq!(read[1].id, second.id, "第二条的 id 应当等于返回值");
}

/// 正文长度不超过头加尾（默认 5120 字节）时不截断。
#[test]
fn short_body_not_truncated() {
    let mut store = store_with_a();
    let scope = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    let body = "a".repeat(5120);
    store
        .append_event(
            &scope,
            &NewEvent::new("k", "a").with_payload("h", &body),
            &PreviewLimit::defaults(),
        )
        .expect("应当成功");
    let read = store.events_after(&scope, 0, 10).expect("应当可读");
    assert_eq!(read[0].preview_head.as_deref(), Some(body.as_str()), "不超过预算时头等于完整正文");
    assert!(read[0].preview_tail.is_none(), "不超过预算时尾为空");
    assert_eq!(read[0].payload_len, Some(5120), "长度等于正文长度");
}

/// 正文超过 5120 字节时，头是前 4096 字节、尾是最后 1024 字节。
#[test]
fn long_body_truncated_to_head_and_tail() {
    let mut store = store_with_a();
    let scope = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    let body = "a".repeat(6000);
    store
        .append_event(
            &scope,
            &NewEvent::new("k", "a").with_payload("h", &body),
            &PreviewLimit::defaults(),
        )
        .expect("应当成功");
    let read = store.events_after(&scope, 0, 10).expect("应当可读");
    assert_eq!(read[0].preview_head.as_deref(), Some(&body[..4096]), "头应当是前 4096 字节");
    assert_eq!(
        read[0].preview_tail.as_deref(),
        Some(&body[6000 - 1024..]),
        "尾应当是最后 1024 字节"
    );
    assert_eq!(read[0].payload_len, Some(6000), "长度仍是全量");
}

/// 截断不切开多字节字符：全中文正文，两段各自是合法 UTF-8 且与原文的头尾一致。
#[test]
fn truncation_does_not_split_multibyte_chars() {
    let mut store = store_with_a();
    let scope = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    let body = "中".repeat(3000); // 9000 字节
    store
        .append_event(
            &scope,
            &NewEvent::new("k", "a").with_payload("h", &body),
            &PreviewLimit::defaults(),
        )
        .expect("应当成功");
    let read = store.events_after(&scope, 0, 10).expect("应当可读");
    let head = read[0].preview_head.as_ref().expect("应当有头");
    let tail = read[0].preview_tail.as_ref().expect("应当有尾");
    assert!(head.len() <= 4096, "头不超过预算");
    assert!(tail.len() <= 1024, "尾不超过预算");
    assert_eq!(head, &body[..head.len()], "头应当是正文的头部");
    assert_eq!(tail, &body[body.len() - tail.len()..], "尾应当是正文的尾部");
    // 4096 不是 3 的倍数，首段只能退到 4095 字节。
    assert_eq!(head.len(), 4095, "首段应当退到 4095 字节");
    // 尾段从 7977 字节开始（第一个字符边界），共 1023 字节。
    assert_eq!(tail.len(), 1023, "尾段应当是 1023 字节");
}

/// 边界两侧各一条：恰好 5120 字节不截断，5121 字节截断。
#[test]
fn boundary_at_5120_and_5121() {
    let mut store = store_with_a();
    let scope = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    let at = "a".repeat(5120);
    store
        .append_event(
            &scope,
            &NewEvent::new("k", "a").with_payload("h", &at),
            &PreviewLimit::defaults(),
        )
        .expect("应当成功");
    let over = "a".repeat(5121);
    store
        .append_event(
            &scope,
            &NewEvent::new("k", "a").with_payload("h", &over),
            &PreviewLimit::defaults(),
        )
        .expect("应当成功");
    let read = store.events_after(&scope, 0, 10).expect("应当可读");
    // 第一条：恰好 5120，不截断。
    assert_eq!(read[0].preview_head.as_deref(), Some(at.as_str()), "5120 字节不截断");
    assert!(read[0].preview_tail.is_none(), "5120 字节尾为空");
    // 第二条：5121，截断。
    assert_eq!(read[1].preview_head.as_deref().map(|s| s.len()), Some(4096), "5121 字节头为 4096");
    assert_eq!(read[1].preview_tail.as_deref().map(|s| s.len()), Some(1024), "5121 字节尾为 1024");
}

/// 没有正文的事件四列全空。
#[test]
fn event_without_payload_has_null_columns() {
    let mut store = store_with_a();
    let scope = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    store
        .append_event(&scope, &NewEvent::new("k", "a"), &PreviewLimit::defaults())
        .expect("应当成功");
    let read = store.events_after(&scope, 0, 10).expect("应当可读");
    assert!(read[0].payload_hash.is_none(), "无正文时哈希为空");
    assert!(read[0].payload_len.is_none(), "无正文时长度为空");
    assert!(read[0].preview_head.is_none(), "无正文时头为空");
    assert!(read[0].preview_tail.is_none(), "无正文时尾为空");
}

/// 事件 id 是 26 字符的 ULID 文本，两条事件的 id 不同。
#[test]
fn event_id_is_26_char_ulid_and_unique() {
    let mut store = store_with_a();
    let scope = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    let first = store
        .append_event(&scope, &NewEvent::new("k", "a"), &PreviewLimit::defaults())
        .expect("应当成功");
    let second = store
        .append_event(&scope, &NewEvent::new("k", "a"), &PreviewLimit::defaults())
        .expect("应当成功");
    assert_eq!(first.id.as_str().len(), 26, "id 长度应为 26");
    assert_eq!(second.id.as_str().len(), 26, "id 长度应为 26");
    assert_ne!(first.id, second.id, "两条事件的 id 应当不同");
    // 能被 EventId::parse 接受（26 字符与首位 0 到 7 都通过）。
    assert!(ita_kernel::context::EventId::parse(first.id.as_str()).is_ok());
}

/// 同一工作区内 seq 唯一，跨工作区可重复。
#[test]
fn seq_unique_within_workspace_repeatable_across() {
    let mut store = store_with_a();
    store.create_workspace(&workspace(WS_B), "/tmp/b").expect("应当成功");
    let scope_a = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    let scope_b = store.scope(&workspace(WS_B)).expect("应当可以取作用域");
    store
        .append_event(&scope_a, &NewEvent::new("k", "a"), &PreviewLimit::defaults())
        .expect("应当成功");
    store
        .append_event(&scope_b, &NewEvent::new("k", "a"), &PreviewLimit::defaults())
        .expect("应当成功");
    // 两个工作区的第一条都是 seq 1，这是合法的。
    let read_a = store.events_after(&scope_a, 0, 10).expect("应当可读");
    let read_b = store.events_after(&scope_b, 0, 10).expect("应当可读");
    assert_eq!(read_a[0].seq, 1, "A 的第一条是 1");
    assert_eq!(read_b[0].seq, 1, "B 的第一条也是 1");
    // 同一工作区内重复 seq 会被主键拒绝。
    let result = store.conn_for_test().execute(
        "INSERT INTO events (workspace_id, seq, id, kind, actor, created_at)
         VALUES (?1, 1, 'x', 'k', 'a', 0)",
        rusqlite::params![WS_A],
    );
    assert!(result.is_err(), "同一工作区重复 seq 应当被主键拒绝");
}

/// 写事务失败时 seq 不被消耗：先占住下一行，再追加，应当失败且计数器不变。
#[test]
fn failed_write_does_not_consume_seq() {
    let mut store = store_with_a();
    let scope = store.scope(&workspace(WS_A)).expect("应当可以取作用域");
    // 先占住 seq 1 那一行。
    store
        .conn_for_test()
        .execute(
            "INSERT INTO events (workspace_id, seq, id, kind, actor, created_at)
             VALUES (?1, 1, 'x', 'k', 'a', 0)",
            rusqlite::params![WS_A],
        )
        .expect("应当可插");
    let before = next_seq(store.conn_for_test(), WS_A);
    assert_eq!(before, 1, "占位前计数器应为 1");
    // 追加会尝试分配 seq 1 并插入，但那一行已被占住，主键冲突，事务回滚。
    let result = store.append_event(&scope, &NewEvent::new("k", "a"), &PreviewLimit::defaults());
    assert!(result.is_err(), "重复 seq 应当失败");
    let after = next_seq(store.conn_for_test(), WS_A);
    assert_eq!(after, before, "失败后计数器不变");
}

/// 取不到作用域时计数器不变。
#[test]
fn scope_failure_leaves_counter_unchanged() {
    let store = store_with_a();
    // 未注册的工作区取作用域被拒。
    let err = store.scope(&workspace("01ARZ3NDEKTSV4RRFFQ69G5FA6")).expect_err("应当被拒");
    assert!(matches!(err, StoreError::UnknownWorkspace(_)));
    // 已注册工作区的计数器保持原值。
    assert_eq!(next_seq(store.conn_for_test(), WS_A), 1, "未追加时计数器保持 1");
}

/// 事件引用的会话与节点必须属于同一工作区。
#[test]
fn session_and_node_must_belong_to_same_workspace() {
    let mut store = store_with_a();
    store.create_workspace(&workspace(WS_B), "/tmp/b").expect("应当成功");
    let scope_a = store.scope(&workspace(WS_A)).expect("应当可以取作用域");

    // 在 A 注册一个会话，在 B 注册一个会话与一个节点。
    store
        .conn_for_test()
        .execute(
            "INSERT INTO sessions (id, workspace_id, head_node, created_at) VALUES (?1, ?2, 'n', 0)",
            rusqlite::params!["01ARZ3NDEKTSV4RRFFQ69G5FAX", WS_A],
        )
        .expect("应当可插");
    store
        .conn_for_test()
        .execute(
            "INSERT INTO sessions (id, workspace_id, head_node, created_at) VALUES (?1, ?2, 'n', 0)",
            rusqlite::params!["01ARZ3NDEKTSV4RRFFQ69G5FAY", WS_B],
        )
        .expect("应当可插");
    store
        .conn_for_test()
        .execute(
            "INSERT INTO nodes (id, workspace_id, session_id, body_kind, created_at)
             VALUES (?1, ?2, ?3, 'text', 0)",
            rusqlite::params!["01ARZ3NDEKTSV4RRFFQ69G5FAZ", WS_B, "01ARZ3NDEKTSV4RRFFQ69G5FAY"],
        )
        .expect("应当可插");

    let session_a = SessionId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAX").expect("应当合法");
    let session_b = SessionId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAY").expect("应当合法");
    let node_b = NodeId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAZ").expect("应当合法");

    // 引用 A 的会话追加通过。
    store
        .append_event(
            &scope_a,
            &NewEvent::new("k", "a").with_session(session_a.clone()),
            &PreviewLimit::defaults(),
        )
        .expect("应当成功");
    // 引用 B 的会话追加被拒，计数器不变。
    let before = next_seq(store.conn_for_test(), WS_A);
    let result = store.append_event(
        &scope_a,
        &NewEvent::new("k", "a").with_session(session_b.clone()),
        &PreviewLimit::defaults(),
    );
    assert!(result.is_err(), "引用 B 的会话应当被拒");
    assert_eq!(next_seq(store.conn_for_test(), WS_A), before, "被拒后计数器不变");
    // 引用 B 的节点追加同样被拒。
    let result = store.append_event(
        &scope_a,
        &NewEvent::new("k", "a").with_node(node_b.clone()),
        &PreviewLimit::defaults(),
    );
    assert!(result.is_err(), "引用 B 的节点应当被拒");
    assert_eq!(next_seq(store.conn_for_test(), WS_A), before, "被拒后计数器不变");
    // 不引用任何会话与节点的追加通过。
    store
        .append_event(&scope_a, &NewEvent::new("k", "a"), &PreviewLimit::defaults())
        .expect("应当成功");
}
