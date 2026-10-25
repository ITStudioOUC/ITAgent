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
    /// 正文总字节数。
    ///
    /// **不要直接与 [`Budget`](super::budget::Budget) 比较**：预算用的是调用方定义的
    /// 抽象计数（通常是分词后的 token 数），字节数与它不同量纲。把字节数当用量送入阈值
    /// 判断会让 85% 在真实用量约三分之一时就触发。这个字段的用途是诊断与日志。
    pub body_bytes: u64,
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
        match &frame.checkpoint {
            Some(checkpoint) => {
                let Some(facts) = input.facts(checkpoint) else {
                    // 校验阶段已经保证事实存在，这里只是不让缺失悄悄变成"没有漂移"。
                    continue;
                };
                let state = Drift::from_facts(facts);
                drift.push((checkpoint.clone(), state));
                messages.push(Message {
                    segment: Segment::Frame,
                    role: MessageRole::System,
                    text: state.notice().to_owned(),
                });
            }
            None => messages.push(Message {
                segment: Segment::Frame,
                role: MessageRole::System,
                text: "该摘要没有对应的文件快照，无法核对文件状态。".to_owned(),
            }),
        }
    }

    // 第三段：最近轮次。
    for node in &input.recent {
        if let Some(message) = node_to_message(node, Segment::Recent) {
            messages.push(message);
        }
    }

    // 第四段：当前待办与通知。
    for node in &input.current {
        if let Some(message) = node_to_message(node, Segment::Current) {
            messages.push(message);
        }
    }

    let body_bytes = messages.iter().map(|m| m.text.len() as u64).sum();
    Ok(ResolvedContext { messages, drift, body_bytes })
}

/// 帧的正文：摘要加上它覆盖的范围说明。
fn frame_text(frame: &Frame) -> String {
    if frame.covers.is_empty() {
        frame.summary.clone()
    } else {
        format!("{}\n（本摘要覆盖 {} 个更早的节点）", frame.summary, frame.covers.len())
    }
}

/// 把一个节点转成消息。
///
/// # 边界与前提
/// `segment` 由调用方给出（节点位于哪一段），不由节点类型推断：同一种 `Notice` 可能
/// 出现在最近轮次里（几轮前的"文件已更新"），也可能出现在当前段（刚刚发生的）。按类型
/// 推断会让段标签与实际位置不一致，而按段数"最近三轮"的逻辑会因此数错。
///
/// # 副作用
/// 无。
fn node_to_message(node: &super::node::Node, segment: Segment) -> Option<Message> {
    let (role, text) = match &node.body {
        // 帧与固定层有各自的段；输入校验已排除它们出现在 recent/current。
        NodeBody::FixedLayer { .. } | NodeBody::Frame(_) => return None,
        NodeBody::Turn { role, text } => (
            match role {
                TurnRole::User => MessageRole::User,
                TurnRole::Assistant => MessageRole::Assistant,
            },
            text.clone(),
        ),
        NodeBody::ToolCall { command } => (MessageRole::ToolCall, command.clone()),
        NodeBody::ToolResult { preview, exit_code, .. } => {
            // 工具结果只取预览的第一条非空行：整段预览可能是 4 KiB，而它已经由
            // 内容寻址存储保存，需要全文时按节点编号取回。
            let line =
                preview.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("（无输出）");
            (MessageRole::ToolResult, format!("[退出码 {exit_code}] {line}"))
        }
        NodeBody::Notice { text } => (MessageRole::System, text.clone()),
        NodeBody::Todo { items } => (
            MessageRole::System,
            format!(
                "当前待办：\n{}",
                items.iter().map(|i| format!("- {i}")).collect::<Vec<_>>().join("\n")
            ),
        ),
    };
    Some(Message { segment, role, text })
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

    /// 四段按固定顺序出现，模型永远看到同一种布局。
    #[test]
    fn four_segment_order_is_fixed() {
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
                // 帧没有检查点时额外跟一条"无快照可核对"的说明，因此 Frame 段占两条。
                Segment::Fixed,
                Segment::Frame,
                Segment::Frame,
                Segment::Recent,
                Segment::Recent,
                Segment::Current
            ]
        );
    }

    /// 同一输入两次解析逐字节相同，解析结果是输入的纯函数。
    #[test]
    fn same_input_resolves_byte_identical_twice() {
        let input = base_input();
        let a = resolve(&input).expect("应当成功");
        let b = resolve(&input).expect("应当成功");
        assert_eq!(a, b);
    }

    /// 固定层成为第一条系统消息，模型指令永远先于历史出现。
    #[test]
    fn fixed_layer_is_first_system_message() {
        let resolved = resolve(&base_input()).expect("应当成功");
        assert_eq!(resolved.messages[0].role, MessageRole::System);
        assert_eq!(resolved.messages[0].text, "系统提示");
    }

    /// 用户与助手消息保留方向，模型能分清谁说了什么。
    #[test]
    fn user_and_assistant_messages_keep_direction() {
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

    /// 工具调用与结果各成一条消息，模型能把命令与输出配对。
    #[test]
    fn tool_call_and_result_each_become_one_message() {
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

    /// 检查点可达但磁盘已变的帧带漂移说明，模型知道摘要可能过期。
    #[test]
    fn frame_carries_drift_notice() {
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

    /// 检查点不可达且磁盘已变化时不报方向：此时无法判断哪边更新，任何方向词都会误导模型。
    #[test]
    fn unreachable_and_changed_reports_unknown_direction() {
        let notice =
            Drift::from_facts(CheckpointFacts { reachable: false, disk_matches: false }).notice();
        assert!(notice.contains("无法判断方向"));
        assert!(!notice.contains("更新后"));
        assert!(!notice.contains("更旧"));
    }

    /// 四种漂移组合各有不同的措辞，模型能分辨它们。
    #[test]
    fn four_drifts_each_have_distinct_wording() {
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

    /// 待办放在最后一段，是模型看到的最近内容。
    #[test]
    fn todo_placed_in_last_segment() {
        let resolved = resolve(&base_input()).expect("应当成功");
        let last = resolved.messages.last().expect("应当有消息");
        assert_eq!(last.segment, Segment::Current);
        assert!(last.text.contains("写测试"));
    }

    /// 通知也放在当前段，不会被埋进历史。
    #[test]
    fn notice_also_placed_in_current_segment() {
        let mut input = base_input();
        input
            .current
            .insert(0, node(5, NodeBody::Notice { text: "a.rs 已被其他会话修改".to_owned() }));
        let resolved = resolve(&input).expect("应当成功");
        assert!(resolved.messages.iter().any(|m| m.text.contains("其他会话修改")));
    }

    /// 字节数是正文总长度，与预算不是同一量纲，不能混用。
    #[test]
    fn byte_count_is_body_length_not_budget() {
        let resolved = resolve(&base_input()).expect("应当成功");
        let manual: u64 = resolved.messages.iter().map(|m| m.text.len() as u64).sum();
        assert_eq!(resolved.body_bytes, manual);
        // 字节数与预算不是同一量纲：默认预算的触发点是 170000 个抽象计数，而这里的字节数
        // 只有几十，直接比较会得出"远未触发"的结论，反之亦然。此处只断言字段存在且可读。
        assert!(resolved.body_bytes < 1024);
    }

    /// 帧没有检查点时明确说明无法核对，模型知道摘要不可验证。
    #[test]
    fn frame_without_checkpoint_explains_cannot_verify() {
        let mut input = base_input();
        input.frames.push(Frame {
            summary: "做了 A".to_owned(),
            covers: vec![id(3)],
            checkpoint: None,
            fixed_layer_fingerprint: "fp".to_owned(),
        });
        let resolved = resolve(&input).expect("应当成功");
        assert!(resolved.messages[2].text.contains("没有对应的文件快照"));
        assert!(resolved.drift.is_empty(), "无检查点的帧不产生漂移记录");
    }

    /// 空覆盖的帧被拒，空摘要永远不会被产出。
    #[test]
    fn frame_with_empty_coverage_is_rejected() {
        let mut input = base_input();
        input.frames.push(Frame {
            summary: "空".to_owned(),
            covers: vec![],
            checkpoint: None,
            fixed_layer_fingerprint: "fp".to_owned(),
        });
        assert!(resolve(&input).is_err());
    }

    /// 最近段里的通知保持最近段标签，段标签由所在位置决定而不是由正文种类决定。
    #[test]
    fn notice_in_recent_segment_keeps_recent_label() {
        let mut input = base_input();
        input.recent.push(node(6, NodeBody::Notice { text: "几轮前文件被改过".to_owned() }));
        let resolved = resolve(&input).expect("应当成功");
        let notice =
            resolved.messages.iter().find(|m| m.text.contains("几轮前")).expect("应当有这条通知");
        assert_eq!(notice.segment, Segment::Recent, "段标签应由所在位置决定");
    }

    /// 帧的覆盖数写进正文，模型知道有多少个节点被摘要。
    #[test]
    fn frame_coverage_count_written_into_body() {
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

    /// 固定层不是固定层的输入被拒，畸形输入不会到达模型。
    #[test]
    fn illegal_input_is_rejected() {
        let mut input = base_input();
        input.fixed_layer = node(9, NodeBody::Turn { role: TurnRole::User, text: "x".to_owned() });
        assert!(resolve(&input).is_err());
    }
}
