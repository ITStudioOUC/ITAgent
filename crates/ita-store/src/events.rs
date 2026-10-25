//! 事件追加与按 seq 读取。
//!
//! 事件是 append-only 的真相：这一模块只提供追加与读取，不提供更新与删除。追加是权威写，
//! 必须在事务提交后才返回，返回的 seq 与事件 id 代表已持久化的那一行。
//!
//! 事件引用的会话与节点（如有）必须属于同一工作区，由写入路径在事务内用 `SELECT EXISTS`
//! 断言，而不是靠表结构的外键——给既有列补外键做不到，这是迁移只加不改的硬约束。

use ita_kernel::context::{EventId, NodeId, SessionId};
use rusqlite::{Connection, Transaction};

use crate::db::{StoreError, now_millis};
use crate::preview::{self, PreviewLimit};
use crate::scope::WorkspaceScope;
use crate::seq::allocate;

/// 一条待追加的事件。
///
/// 正文的哈希由调用方给出，长度与预览由正文本身推出。哈希与正文必须指向同一份内容，
/// 真正的校验要等内容寻址存储落地才算得出。
#[derive(Debug)]
pub struct NewEvent {
    session_id: Option<SessionId>,
    node_id: Option<NodeId>,
    kind: String,
    actor: String,
    payload: Option<Payload>,
}

/// 事件正文：内容哈希与全文。
#[derive(Debug)]
pub struct Payload {
    /// 内容哈希，必须与 `body` 指向同一份内容。
    hash: String,
    /// 全文。长度与预览由存储层从这里推出。
    body: String,
}

impl NewEvent {
    /// 构造一条不带会话、节点与正文的事件。
    pub fn new(kind: impl Into<String>, actor: impl Into<String>) -> Self {
        Self {
            session_id: None,
            node_id: None,
            kind: kind.into(),
            actor: actor.into(),
            payload: None,
        }
    }

    /// 关联一个会话。
    pub fn with_session(mut self, session: SessionId) -> Self {
        self.session_id = Some(session);
        self
    }

    /// 关联一个节点。
    pub fn with_node(mut self, node: NodeId) -> Self {
        self.node_id = Some(node);
        self
    }

    /// 附带正文。哈希与正文必须指向同一份内容。
    pub fn with_payload(mut self, hash: impl Into<String>, body: impl Into<String>) -> Self {
        self.payload = Some(Payload { hash: hash.into(), body: body.into() });
        self
    }
}

/// 追加成功的确认内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppendedEvent {
    /// 该工作区内分配到的 seq。
    pub seq: i64,
    /// 事件 id。
    pub id: EventId,
}

/// 一条已持久化的事件。
#[derive(Debug)]
pub struct Event {
    pub seq: i64,
    pub id: EventId,
    pub session_id: Option<SessionId>,
    pub node_id: Option<NodeId>,
    pub kind: String,
    pub actor: String,
    pub payload_hash: Option<String>,
    pub payload_len: Option<i64>,
    pub preview_head: Option<String>,
    pub preview_tail: Option<String>,
    pub created_at: i64,
}

/// 在已开启的事务内追加一条事件。
///
/// # 步骤
/// 1. 校验会话与节点归属（如有）。
/// 2. 从计数器分配 seq。
/// 3. 插入事件行。
///
/// # 边界与前提
/// 接收已开启的事务，不自行开事务。校验失败或插入失败时返回错误，事务回滚，seq 不被
/// 消耗。06.4 应当先做 head 的比较并交换，成功之后再调用它，这样冲突时不消耗 seq。
///
/// # 副作用
/// 写 `events` 与 `workspace_seq`。
pub(crate) fn append_in(
    tx: &Transaction,
    scope: &WorkspaceScope,
    event: &NewEvent,
    limit: PreviewLimit,
) -> Result<AppendedEvent, StoreError> {
    let workspace = scope.workspace();

    // 校验那一步：会话与节点必须属于本工作区。不通过则回滚，计数器不消耗。
    if let Some(session) = &event.session_id {
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM sessions WHERE id = ?1 AND workspace_id = ?2)",
            rusqlite::params![session.as_str(), workspace.as_str()],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(StoreError::UnknownSession(session.clone()));
        }
    }
    if let Some(node) = &event.node_id {
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM nodes WHERE id = ?1 AND workspace_id = ?2)",
            rusqlite::params![node.as_str(), workspace.as_str()],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(StoreError::UnknownNode(node.clone()));
        }
    }

    // 插入那一步：分配 seq 并插入事件，同一事务。
    let seq = allocate(tx, workspace)?;
    let (payload_hash, payload_len, preview_head, preview_tail) = match &event.payload {
        Some(payload) => {
            let (head, tail) = preview::truncate(&payload.body, &limit);
            (
                Some(payload.hash.clone()),
                Some(i64::try_from(payload.body.len()).expect("正文长度不超 i64")),
                Some(head),
                tail,
            )
        }
        None => (None, None, None, None),
    };
    let id = EventId::parse(ulid::Ulid::new().to_string()).expect("ulid 产出合法 id");
    tx.execute(
        "INSERT INTO events
             (workspace_id, seq, id, session_id, node_id, kind, actor,
              payload_hash, payload_len, preview_head, preview_tail, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        rusqlite::params![
            workspace.as_str(),
            seq,
            id.as_str(),
            event.session_id.as_ref().map(|s| s.as_str()),
            event.node_id.as_ref().map(|n| n.as_str()),
            event.kind.as_str(),
            event.actor.as_str(),
            payload_hash,
            payload_len,
            preview_head,
            preview_tail,
            now_millis()?,
        ],
    )?;

    Ok(AppendedEvent { seq, id })
}

/// 读取某工作区在给定 seq 之后的至多 N 条事件，按 seq 升序。
///
/// # 边界与前提
/// 只按 seq 排序，不用 `created_at`：事件顺序由 seq 决定，时间戳单次测试里恰好单调不可靠。
/// 主键 `(workspace_id, seq)` 天然支持按工作区加 seq 范围查询，不需要新索引。
///
/// # 副作用
/// 只读。
pub(crate) fn read_after(
    conn: &Connection,
    scope: &WorkspaceScope,
    after_seq: i64,
    limit: usize,
) -> Result<Vec<Event>, StoreError> {
    let workspace = scope.workspace();
    let mut stmt = conn.prepare(
        "SELECT seq, id, session_id, node_id, kind, actor, payload_hash, payload_len,
                preview_head, preview_tail, created_at
         FROM events
         WHERE workspace_id = ?1 AND seq > ?2
         ORDER BY seq ASC
         LIMIT ?3",
    )?;
    let rows =
        stmt.query_map(rusqlite::params![workspace.as_str(), after_seq, limit as i64], |row| {
            Ok(Event {
                seq: row.get(0)?,
                id: EventId::parse(row.get::<_, String>(1)?).map_err(|e| {
                    rusqlite::Error::InvalidParameterName(format!("事件 id 解析失败：{e}"))
                })?,
                session_id: row
                    .get::<_, Option<String>>(2)?
                    .map(|s| SessionId::parse(s).expect("库内 id 应当合法")),
                node_id: row
                    .get::<_, Option<String>>(3)?
                    .map(|s| NodeId::parse(s).expect("库内 id 应当合法")),
                kind: row.get(4)?,
                actor: row.get(5)?,
                payload_hash: row.get(6)?,
                payload_len: row.get(7)?,
                preview_head: row.get(8)?,
                preview_tail: row.get(9)?,
                created_at: row.get(10)?,
            })
        })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(StoreError::from)
}
