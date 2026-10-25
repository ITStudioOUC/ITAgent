//! 帧提交与帧正文编码。
//!
//! 一次帧提交在一个立即事务里做三件事：校验会话与祖先节点同属一个工作区、插入帧节点、
//! 比较并交换 head、追加事件。顺序是刻意的：事件追加排在 head 比较并交换成功之后，冲突才
//! 不烧 seq。
//!
//! 帧正文落库方式是**长度前缀文本**，写进 `nodes.inline_body`，不引入 `serde_json`、不新增
//! 迁移。理由是仓里目前没有任何 JSON 依赖，为一个非安全用途的序列化加依赖违背"不引入不
//! 必要依赖"；而 `nodes` 表没有为帧单列字段，`ALTER TABLE` 加列会把帧专属的列塞进通用节点
//! 表，污染 `turn`、`tool_call` 等其它正文种类。帧的四个字段全部可逆。
//!
//! `prompt_version` 不进帧节点：内核的 `Frame` 结构没有这个字段，它属于压缩调用事件，落库
//! 推迟到内容寻址引用落地之后。帧节点只存 `Frame` 的四个字段。

use ita_kernel::context::compress::CommitPlan;
use ita_kernel::context::{CheckpointId, Frame, NodeId, SessionId};
use rusqlite::Transaction;

use crate::db::{Store, StoreError, now_millis};
use crate::events::{self, NewEvent};
use crate::heads;
use crate::preview::PreviewLimit;
use crate::scope::WorkspaceScope;

/// 帧正文解码失败的原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameBodyError {
    /// 长度前缀缺失或不是数字。
    BadLengthPrefix,
    /// 正文被截断。
    Truncated,
    /// 检查点标志既不是 0 也不是 1。
    BadCheckpointFlag,
    /// 覆盖或检查点 id 不是合法的 26 字节标识符。
    BadId,
    /// 指纹之后还有多余的字节。
    ExtraBytes,
}

impl std::fmt::Display for FrameBodyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadLengthPrefix => write!(f, "帧正文的长度前缀缺失或不是数字"),
            Self::Truncated => write!(f, "帧正文被截断"),
            Self::BadCheckpointFlag => write!(f, "帧正文的检查点标志既不是 0 也不是 1"),
            Self::BadId => write!(f, "帧正文的覆盖或检查点 id 不是合法的 26 字节标识符"),
            Self::ExtraBytes => write!(f, "帧正文的指纹之后还有多余的字节"),
        }
    }
}

impl std::error::Error for FrameBodyError {}

/// 一次帧提交成功的确认内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommittedFrame {
    /// 新帧节点 id。
    pub node: NodeId,
    /// 提交后的 head 代次。
    pub generation: u64,
}

/// 把一个帧编码成长度前缀文本。
///
/// # 步骤
/// 1. 写摘要的长度前缀与正文。
/// 2. 写覆盖计数与每个 id。
/// 3. 写检查点存在标志与可选 id。
/// 4. 写固定层指纹的长度前缀与正文。
///
/// # 边界与前提
/// 纯函数，无数据库访问。摘要与指纹是自由文本，可含换行，但读方按长度精确取字节，换行不
/// 构成歧义。整个串是合法 UTF-8。
///
/// # 副作用
/// 无。
pub fn encode_frame(frame: &Frame) -> String {
    let mut out = String::new();
    out.push_str(&frame.summary.len().to_string());
    out.push(':');
    out.push_str(&frame.summary);
    out.push('\n');
    out.push_str(&frame.covers.len().to_string());
    out.push('\n');
    for id in &frame.covers {
        out.push_str(id.as_str());
        out.push('\n');
    }
    match &frame.checkpoint {
        Some(cp) => {
            out.push('1');
            out.push('\n');
            out.push_str(cp.as_str());
            out.push('\n');
        }
        None => {
            out.push('0');
            out.push('\n');
        }
    }
    out.push_str(&frame.fixed_layer_fingerprint.len().to_string());
    out.push(':');
    out.push_str(&frame.fixed_layer_fingerprint);
    out
}

/// 把长度前缀文本解码回一个帧。
///
/// # 步骤
/// 1. 读摘要的长度前缀与正文。
/// 2. 读覆盖计数与每个 id。
/// 3. 读检查点存在标志与可选 id。
/// 4. 读固定层指纹的长度前缀与正文。
///
/// # 边界与前提
/// 纯函数，无数据库访问。读方按长度精确取字节，换行不构成歧义。
///
/// # 副作用
/// 无。
pub fn decode_frame(body: &str) -> Result<Frame, FrameBodyError> {
    let mut rest = body;

    // 摘要。
    let (summary_len, after) = read_len_prefix(rest, ':')?;
    rest = after;
    if rest.len() < summary_len {
        return Err(FrameBodyError::Truncated);
    }
    let summary = rest[..summary_len].to_owned();
    rest = &rest[summary_len..];
    if !rest.starts_with('\n') {
        return Err(FrameBodyError::Truncated);
    }
    rest = &rest[1..];

    // 覆盖列表。
    let (count, after) = read_len_prefix(rest, '\n')?;
    rest = after;
    let mut covers = Vec::with_capacity(count);
    for _ in 0..count {
        if rest.len() < 27 {
            return Err(FrameBodyError::Truncated);
        }
        let id = NodeId::parse(&rest[..26]).map_err(|_| FrameBodyError::BadId)?;
        covers.push(id);
        rest = &rest[26..];
        if !rest.starts_with('\n') {
            return Err(FrameBodyError::Truncated);
        }
        rest = &rest[1..];
    }

    // 检查点。
    if rest.is_empty() || !matches!(rest.as_bytes()[0], b'0' | b'1') {
        return Err(FrameBodyError::BadCheckpointFlag);
    }
    let has_checkpoint = rest.as_bytes()[0] == b'1';
    rest = &rest[1..];
    if !rest.starts_with('\n') {
        return Err(FrameBodyError::Truncated);
    }
    rest = &rest[1..];
    let checkpoint = if has_checkpoint {
        if rest.len() < 27 {
            return Err(FrameBodyError::Truncated);
        }
        let cp = CheckpointId::parse(&rest[..26]).map_err(|_| FrameBodyError::BadId)?;
        rest = &rest[26..];
        if !rest.starts_with('\n') {
            return Err(FrameBodyError::Truncated);
        }
        rest = &rest[1..];
        Some(cp)
    } else {
        None
    };

    // 固定层指纹。
    let (fingerprint_len, after) = read_len_prefix(rest, ':')?;
    rest = after;
    if rest.len() < fingerprint_len {
        return Err(FrameBodyError::Truncated);
    }
    let fingerprint = rest[..fingerprint_len].to_owned();
    rest = &rest[fingerprint_len..];
    if !rest.is_empty() {
        return Err(FrameBodyError::ExtraBytes);
    }

    Ok(Frame { summary, covers, checkpoint, fixed_layer_fingerprint: fingerprint })
}

/// 读一个长度前缀：十进制数字后跟一个分隔符。
///
/// # 边界与前提
/// 数字与分隔符之间没有其它字符。分隔符是 `:` 或 `\n`。
fn read_len_prefix(rest: &str, delimiter: char) -> Result<(usize, &str), FrameBodyError> {
    let mut len = 0;
    while len < rest.len() && rest.as_bytes()[len].is_ascii_digit() {
        len += 1;
    }
    if len == 0 || rest.as_bytes()[len] != delimiter as u8 {
        return Err(FrameBodyError::BadLengthPrefix);
    }
    let num: usize = rest[..len].parse().map_err(|_| FrameBodyError::BadLengthPrefix)?;
    Ok((num, &rest[len + 1..]))
}

/// 在已开启的事务内提交一个帧。
///
/// # 步骤
/// 1. 校验会话、父节点、覆盖列表都同属一个工作区。
/// 2. 插入帧节点。
/// 3. 比较并交换 head。
/// 4. 追加压缩事件。
///
/// # 边界与前提
/// 接收一个已开启的事务，不自行开事务。事件追加排在 head 比较并交换成功之后，冲突时不写
/// 帧行、不动 head、不烧 seq。
///
/// # 副作用
/// 写 `nodes`、`sessions`、`events` 与 `workspace_seq`。
pub(crate) fn commit_in(
    tx: &Transaction,
    scope: &WorkspaceScope,
    session: &SessionId,
    new_node: &NodeId,
    plan: &CommitPlan,
    actor: &str,
) -> Result<CommittedFrame, StoreError> {
    let workspace = scope.workspace();

    // 校验会话属于本工作区。
    let session_in_ws: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM sessions WHERE id = ?1 AND workspace_id = ?2)",
        rusqlite::params![session.as_str(), workspace.as_str()],
        |row| row.get(0),
    )?;
    if !session_in_ws {
        return Err(StoreError::CrossWorkspace(session.as_str().to_owned()));
    }

    // 校验父节点属于本工作区。
    let parent_in_ws: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM nodes WHERE id = ?1 AND workspace_id = ?2)",
        rusqlite::params![plan.parent.as_str(), workspace.as_str()],
        |row| row.get(0),
    )?;
    if !parent_in_ws {
        return Err(StoreError::CrossWorkspace(plan.parent.as_str().to_owned()));
    }

    // 校验覆盖列表里每个节点属于本工作区。
    for cover in &plan.covers {
        let cover_in_ws: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM nodes WHERE id = ?1 AND workspace_id = ?2)",
            rusqlite::params![cover.as_str(), workspace.as_str()],
            |row| row.get(0),
        )?;
        if !cover_in_ws {
            return Err(StoreError::CrossWorkspace(cover.as_str().to_owned()));
        }
    }

    // 插入帧节点。
    let frame = Frame {
        summary: plan.summary.clone(),
        covers: plan.covers.clone(),
        checkpoint: plan.checkpoint.clone(),
        fixed_layer_fingerprint: plan.fixed_layer_fingerprint.clone(),
    };
    let body = encode_frame(&frame);
    tx.execute(
        "INSERT INTO nodes
             (id, workspace_id, session_id, parent_id, body_kind, inline_body, created_at)
         VALUES (?1, ?2, ?3, ?4, 'frame', ?5, ?6)",
        rusqlite::params![
            new_node.as_str(),
            workspace.as_str(),
            session.as_str(),
            plan.parent.as_str(),
            body,
            now_millis()?,
        ],
    )?;

    // 比较并交换 head。
    let changed =
        heads::compare_and_swap(tx, session, &plan.parent, plan.parent_generation, new_node)?;
    if changed == 0 {
        return Err(StoreError::HeadConflict);
    }

    // 追加事件。
    let event = NewEvent::new("compression", actor)
        .with_session(session.clone())
        .with_node(new_node.clone());
    events::append_in(tx, scope, &event, PreviewLimit::defaults())?;

    Ok(CommittedFrame { node: new_node.clone(), generation: plan.parent_generation + 1 })
}

impl Store {
    /// 提交一个帧。
    ///
    /// # 步骤
    /// 1. 开一个立即事务。
    /// 2. 校验会话与祖先节点同属一个工作区、插入帧节点、比较并交换 head、追加事件。
    /// 3. 提交。
    ///
    /// # 边界与前提
    /// 提交是权威写，提交后才返回。冲突时帧节点行不落库、head 不动、seq 不消耗。
    ///
    /// # 副作用
    /// 写 `nodes`、`sessions`、`events` 与 `workspace_seq`。
    pub fn commit_frame(
        &mut self,
        scope: &WorkspaceScope,
        session: &SessionId,
        new_node: &NodeId,
        plan: &CommitPlan,
        actor: &str,
    ) -> Result<CommittedFrame, StoreError> {
        let tx = self.conn_mut().transaction()?;
        let committed = commit_in(&tx, scope, session, new_node, plan, actor)?;
        tx.commit()?;
        Ok(committed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ita_kernel::context::CheckpointId;

    const CP: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    const COVER_1: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";
    const COVER_2: &str = "01ARZ3NDEKTSV4RRFFQ69G5FA6";

    /// 非空摘要、多个覆盖、有检查点、有指纹的帧，编码后解码逐字段相等。
    #[test]
    fn encode_then_decode_round_trips_full_frame() {
        let frame = Frame {
            summary: "做了 A，否决了 B".to_owned(),
            covers: vec![
                NodeId::parse(COVER_1).expect("应当合法"),
                NodeId::parse(COVER_2).expect("应当合法"),
            ],
            checkpoint: Some(CheckpointId::parse(CP).expect("应当合法")),
            fixed_layer_fingerprint: "fp-7".to_owned(),
        };
        let body = encode_frame(&frame);
        let decoded = decode_frame(&body).expect("应当可解码");
        assert_eq!(decoded, frame, "解码应当逐字段相等");
    }

    /// 摘要含换行与冒号时，按长度取字节不引入歧义。
    #[test]
    fn summary_with_newlines_and_colons_round_trips() {
        let frame = Frame {
            summary: "第一行\n第二行: 带冒号".to_owned(),
            covers: vec![],
            checkpoint: None,
            fixed_layer_fingerprint: "fp".to_owned(),
        };
        let body = encode_frame(&frame);
        let decoded = decode_frame(&body).expect("应当可解码");
        assert_eq!(decoded, frame, "换行与冒号不引入歧义");
    }

    /// 空摘要与空指纹也能往返。
    #[test]
    fn empty_summary_and_fingerprint_round_trip() {
        let frame = Frame {
            summary: String::new(),
            covers: vec![NodeId::parse(COVER_1).expect("应当合法")],
            checkpoint: None,
            fixed_layer_fingerprint: String::new(),
        };
        let body = encode_frame(&frame);
        let decoded = decode_frame(&body).expect("应当可解码");
        assert_eq!(decoded, frame, "空串也能往返");
    }

    /// 被截断的正文报截断错误，而不是静默产出残缺帧。
    #[test]
    fn truncated_body_reports_truncated() {
        let frame = Frame {
            summary: "摘要".to_owned(),
            covers: vec![NodeId::parse(COVER_1).expect("应当合法")],
            checkpoint: Some(CheckpointId::parse(CP).expect("应当合法")),
            fixed_layer_fingerprint: "fp".to_owned(),
        };
        let body = encode_frame(&frame);
        let truncated = &body[..body.len() / 2];
        assert!(matches!(decode_frame(truncated), Err(FrameBodyError::Truncated)));
    }

    /// 检查点标志既不是 0 也不是 1 时报标志错误。
    #[test]
    fn bad_checkpoint_flag_reports_bad_flag() {
        // 摘要 "x"(1)、覆盖 0 个、检查点标志 2(非法)、指纹 "fp"(2)。
        let bad = "1:x\n0\n2\n2:fp";
        assert!(matches!(decode_frame(bad), Err(FrameBodyError::BadCheckpointFlag)));
    }

    /// 指纹之后还有多余字节时报多余字节错误。
    #[test]
    fn extra_bytes_after_fingerprint_report_extra() {
        let frame = Frame {
            summary: "x".to_owned(),
            covers: vec![],
            checkpoint: None,
            fixed_layer_fingerprint: "fp".to_owned(),
        };
        let body = encode_frame(&frame) + "尾巴";
        assert!(matches!(decode_frame(&body), Err(FrameBodyError::ExtraBytes)));
    }
}
