//! 解析：把 [`ResolveInput`] 变成模型实际看到的消息数组。
//!
//! 这是整个上下文模型的核心，也是唯一一个"输入相同则输出逐字节相同"的保证所在。它不读
//! 库、不读时钟、不读文件系统——凡是需要的状态都在输入里。回退与压缩因此只是改变输入，
//! 而不需要两套语义。
//!
//! 输出固定四段，顺序不可变：
//!
//! 1. 固定层（系统提示、工具目录、AGENTS.md 的冻结版本）
//! 2. 帧（按时间顺序的摘要）
//! 3. 最近轮次原文
//! 4. 当前待办与末尾通知

use super::ids::CheckpointId;
use super::input::{CheckpointFacts, ResolveInput};
use super::node::{Frame, NodeBody, TurnRole};

/// 消息在数组里的位置，用于诊断与测试。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Segment {
    /// 固定层。
    Fixed,
    /// 帧摘要。
    Frame,
    /// 最近轮次原文。
    Recent,
    /// 当前待办与末尾通知。
    Current,
}

/// 一条消息的角色。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    /// 系统提示。
    System,
    /// 用户输入。
    User,
    /// 模型输出。
    Assistant,
    /// 工具调用。
    ToolCall,
    /// 工具结果。
    ToolResult,
}

/// 一条消息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// 所属段。
    pub segment: Segment,
    /// 角色。
    pub role: MessageRole,
    /// 正文。
    pub text: String,
}

/// 漂移说明：帧描述的世界与当前磁盘的关系。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drift {
    /// 检查点仍可达且磁盘一致。
    Consistent,
    /// 检查点仍可达，但磁盘已被后续修改覆盖。
    DiskChanged,
    /// 检查点已不在保留窗口内，但磁盘内容与摘要不矛盾。
    Unreachable,
    /// 检查点已不可达且磁盘已变化。**只报无法核对，不报方向。**
    UnreachableAndChanged,
}

impl Drift {
    /// 由两个正交的布尔量得出。
    ///
    /// # 边界与前提
    /// 四种组合都有名字。把它们压成三态会让"不可达且已变化"没有名字，而那一格恰恰最
    /// 危险：模型会误以为知道方向。
    ///
    /// # 副作用
    /// 无。
    pub fn from_facts(facts: CheckpointFacts) -> Self {
        match (facts.reachable, facts.disk_matches) {
            (true, true) => Self::Consistent,
            (true, false) => Self::DiskChanged,
            (false, true) => Self::Unreachable,
            (false, false) => Self::UnreachableAndChanged,
        }
    }

    /// 注入给模型的一句话。
    ///
    /// # 边界与前提
    /// `UnreachableAndChanged` 只说"无法核对"，不说"更旧"或"更新"——因为确实不知道。
    ///
    /// # 副作用
    /// 无。
    pub fn notice(self) -> &'static str {
        match self {
            Self::Consistent => "文件当前状态与本摘要描述一致。",
            Self::DiskChanged => "摘要描述的文件状态已被后续修改覆盖，当前磁盘是更新后的状态。",
            Self::Unreachable => {
                "该摘要对应的快照已不在保留窗口内，无法核对；当前磁盘内容与本摘要不矛盾。"
            }
            Self::UnreachableAndChanged => {
                "该摘要对应的快照已不在保留窗口内，且磁盘内容已变化，无法判断方向。"
            }
        }
    }
}

/// 解析结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedContext {
    /// 按顺序排好的消息。
    pub messages: Vec<Message>,
    /// 摘要与磁盘的漂移说明，按帧的顺序。
    pub drift: Vec<(CheckpointId, Drift)>,
    /// 估算用量：正文总字节数。
    ///
    /// 这是抽象计数的一种实现（按字节），调用方可以换成自己的分词计数。
    pub estimated_usage: u64,
}

/// 解析。
///
/// # 步骤
/// 1. 校验输入自洽。
/// 2. 固定层作为第一条系统消息。
/// 3. 每个帧产出一条系统消息（摘要 + 漂移说明）。
/// 4. 最近轮次按方向展开成消息。
/// 5. 当前待办与通知放在末尾。
///
/// # 边界与前提
/// 输入必须先通过 [`ResolveInput::validate`]；本函数会再校验一次，因为它是唯一对外承诺
/// 可复现的入口，不该依赖调用方的自觉。
///
/// # 副作用
/// 无。
///
/// # 注意事项
/// 输出只由输入决定：不排序、不去重、不读环境。同样的输入两次调用必然得到相同结果，
/// 这是 replay 与缓存命中率计算的基础。
pub fn resolve(input: &ResolveInput) -> Result<ResolvedContext, super::input::InputError> {
    input.validate()?;

    let mut messages = Vec::new();
    let mut drift = Vec::new();

    // 第一段：固定层。
    if let NodeBody::FixedLayer { content, .. } = &input.fixed_layer.body {
        messages.push(Message {
            segment: Segment::Fixed,
            role: MessageRole::System,
            text: content.clone(),
        });
    }

    // 第二段：帧。
    for frame in &input.frames {
        messages.push(Message {
            segment: Segment::Frame,
            role: MessageRole::System,
            text: frame_text(frame),
        });
        if let Some(checkpoint) = &frame.checkpoint
            && let Some(facts) = input.facts(checkpoint)
        {
            let state = Drift::from_facts(facts);
            drift.push((checkpoint.clone(), state));
            messages.push(Message {
                segment: Segment::Frame,
                role: MessageRole::System,
                text: state.notice().to_owned(),
            });
        }
    }

    // 第三段：最近轮次。
    for node in &input.recent {
        if let Some(message) = node_to_message(node) {
            messages.push(message);
        }
    }

    // 第四段：当前待办与通知。
    for node in &input.current {
        if let Some(message) = node_to_message(node) {
            messages.push(message);
        }
    }

    let estimated_usage = messages.iter().map(|m| m.text.len() as u64).sum();
    Ok(ResolvedContext { messages, drift, estimated_usage })
}

/// 帧的正文：摘要加上它覆盖的范围说明。
fn frame_text(frame: &Frame) -> String {
    if frame.covers.is_empty() {
        frame.summary.clone()
    } else {
        format!("{}\n（本摘要覆盖 {} 个更早的节点）", frame.summary, frame.covers.len())
    }
}

/// 把一个节点转成消息。帧与固定层不在这里出现（它们有各自的段）。
fn node_to_message(node: &super::node::Node) -> Option<Message> {
    match &node.body {
        NodeBody::FixedLayer { .. } | NodeBody::Frame(_) => None,
        NodeBody::Turn { role, text } => Some(Message {
            segment: Segment::Recent,
            role: match role {
                TurnRole::User => MessageRole::User,
                TurnRole::Assistant => MessageRole::Assistant,
            },
            text: text.clone(),
        }),
        NodeBody::ToolCall { command } => Some(Message {
            segment: Segment::Recent,
            role: MessageRole::ToolCall,
            text: command.clone(),
        }),
        NodeBody::ToolResult { preview, exit_code, .. } => Some(Message {
            segment: Segment::Recent,
            role: MessageRole::ToolResult,
            text: format!("[退出码 {exit_code}] {preview}"),
        }),
        NodeBody::Notice { text } => Some(Message {
            segment: Segment::Current,
            role: MessageRole::System,
            text: text.clone(),
        }),
        NodeBody::Todo { items } => Some(Message {
            segment: Segment::Current,
            role: MessageRole::System,
            text: format!(
                "当前待办：\n{}",
                items.iter().map(|i| format!("- {i}")).collect::<Vec<_>>().join("\n")
            ),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::budget::Budget;
    use crate::context::ids::{NodeId, SessionId};
    use crate::context::input::CheckpointFacts;
    use crate::context::node::Node;

    const BASE: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

    fn id(n: u8) -> NodeId {
        let mut text = BASE.to_owned();
        text.replace_range(25..26, &char::from(b'0' + n % 10).to_string());
        NodeId::parse(text).expect("应当合法")
    }

    fn checkpoint(n: u8) -> CheckpointId {
        let mut text = BASE.to_owned();
        text.replace_range(24..26, &format!("{n:02}"));
        CheckpointId::parse(text).expect("应当合法")
    }

    fn session() -> SessionId {
        SessionId::parse(BASE).expect("应当合法")
    }

    fn node(n: u8, body: NodeBody) -> Node {
        Node { id: id(n), parent: None, session: session(), body }
    }

    fn fixed() -> Node {
        node(
            0,
            NodeBody::FixedLayer {
                fingerprint: "fp".to_owned(), content: "系统提示".to_owned()
            },
        )
    }

    fn base_input() -> ResolveInput {
        ResolveInput {
            fixed_layer: fixed(),
            frames: vec![],
            recent: vec![node(
                1,
                NodeBody::Turn { role: TurnRole::User, text: "做这个".to_owned() },
            )],
            current: vec![node(2, NodeBody::Todo { items: vec!["写测试".to_owned()] })],
            checkpoint_facts: vec![],
            budget: Budget::defaults(),
        }
    }

    #[test]
    fn 四段顺序固定() {
        let mut input = base_input();
        input.frames.push(Frame {
            summary: "做了 A".to_owned(),
            covers: vec![id(3)],
            checkpoint: None,
            fixed_layer_fingerprint: "fp".to_owned(),
        });
        input
            .recent
            .push(node(4, NodeBody::Turn { role: TurnRole::Assistant, text: "好了".to_owned() }));

        let resolved = resolve(&input).expect("应当成功");
        let segments: Vec<Segment> = resolved.messages.iter().map(|m| m.segment).collect();
        assert_eq!(
            segments,
            vec![
                Segment::Fixed,
                Segment::Frame,
                Segment::Recent,
                Segment::Recent,
                Segment::Current
            ]
        );
    }

    #[test]
    fn 同一输入两次解析逐字节相同() {
        let input = base_input();
        let a = resolve(&input).expect("应当成功");
        let b = resolve(&input).expect("应当成功");
        assert_eq!(a, b);
    }

    #[test]
    fn 固定层是第一条系统消息() {
        let resolved = resolve(&base_input()).expect("应当成功");
        assert_eq!(resolved.messages[0].role, MessageRole::System);
        assert_eq!(resolved.messages[0].text, "系统提示");
    }

    #[test]
    fn 用户与助手消息保留方向() {
        let mut input = base_input();
        input
            .recent
            .push(node(3, NodeBody::Turn { role: TurnRole::Assistant, text: "好了".to_owned() }));
        let resolved = resolve(&input).expect("应当成功");
        let roles: Vec<MessageRole> = resolved.messages.iter().map(|m| m.role).collect();
        assert_eq!(
            roles,
            vec![
                MessageRole::System,
                MessageRole::User,
                MessageRole::Assistant,
                MessageRole::System
            ]
        );
    }

    #[test]
    fn 工具调用与结果各成一条消息() {
        let mut input = base_input();
        input.recent = vec![
            node(1, NodeBody::ToolCall { command: "ita edit a.rs".to_owned() }),
            node(2, NodeBody::ToolResult { body: None, preview: "ok".to_owned(), exit_code: 0 }),
        ];
        let resolved = resolve(&input).expect("应当成功");
        assert_eq!(resolved.messages[1].role, MessageRole::ToolCall);
        assert_eq!(resolved.messages[2].role, MessageRole::ToolResult);
        assert!(resolved.messages[2].text.contains("退出码 0"));
    }

    #[test]
    fn 帧带漂移说明() {
        let mut input = base_input();
        let cp = checkpoint(1);
        input.frames.push(Frame {
            summary: "改了 X".to_owned(),
            covers: vec![id(3)],
            checkpoint: Some(cp.clone()),
            fixed_layer_fingerprint: "fp".to_owned(),
        });
        input.checkpoint_facts =
            vec![(cp.clone(), CheckpointFacts { reachable: true, disk_matches: false })];

        let resolved = resolve(&input).expect("应当成功");
        assert_eq!(resolved.drift, vec![(cp, Drift::DiskChanged)]);
        assert!(resolved.messages[1].text.contains("改了 X"));
        assert!(resolved.messages[2].text.contains("已被后续修改覆盖"));
    }

    #[test]
    fn 不可达且已变化时不报方向() {
        let notice =
            Drift::from_facts(CheckpointFacts { reachable: false, disk_matches: false }).notice();
        assert!(notice.contains("无法判断方向"));
        assert!(!notice.contains("更新后"));
        assert!(!notice.contains("更旧"));
    }

    #[test]
    fn 四种漂移各有不同的措辞() {
        let combos = [(true, true), (true, false), (false, true), (false, false)];
        let notices: Vec<&str> = combos
            .iter()
            .map(|(r, d)| {
                Drift::from_facts(CheckpointFacts { reachable: *r, disk_matches: *d }).notice()
            })
            .collect();
        let unique: std::collections::BTreeSet<&str> = notices.iter().copied().collect();
        assert_eq!(unique.len(), 4, "四种组合必须有四种不同措辞");
    }

    #[test]
    fn 待办放在最后一段() {
        let resolved = resolve(&base_input()).expect("应当成功");
        let last = resolved.messages.last().expect("应当有消息");
        assert_eq!(last.segment, Segment::Current);
        assert!(last.text.contains("写测试"));
    }

    #[test]
    fn 通知也放在当前段() {
        let mut input = base_input();
        input
            .current
            .insert(0, node(5, NodeBody::Notice { text: "a.rs 已被其他会话修改".to_owned() }));
        let resolved = resolve(&input).expect("应当成功");
        assert!(resolved.messages.iter().any(|m| m.text.contains("其他会话修改")));
    }

    #[test]
    fn 用量是正文总字节数() {
        let resolved = resolve(&base_input()).expect("应当成功");
        let manual: u64 = resolved.messages.iter().map(|m| m.text.len() as u64).sum();
        assert_eq!(resolved.estimated_usage, manual);
    }

    #[test]
    fn 帧覆盖数写进正文() {
        let mut input = base_input();
        input.frames.push(Frame {
            summary: "做了 A".to_owned(),
            covers: vec![id(3), id(4), id(5)],
            checkpoint: None,
            fixed_layer_fingerprint: "fp".to_owned(),
        });
        let resolved = resolve(&input).expect("应当成功");
        assert!(resolved.messages[1].text.contains("3 个更早的节点"));
    }

    #[test]
    fn 非法输入被拒() {
        let mut input = base_input();
        input.fixed_layer = node(9, NodeBody::Turn { role: TurnRole::User, text: "x".to_owned() });
        assert!(resolve(&input).is_err());
    }
}
