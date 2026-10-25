//! 轮边界与折叠范围。
//!
//! 压缩必须知道"压到哪为止"和"留哪一段"。这两件事都依赖同一个定义：什么算一轮。
//!
//! 一轮的边界是**用户消息**：一条用户输入开启一轮，其后的助手回复、工具调用与结果都
//! 属于这一轮。按节点数数轮是错的——一轮里可能有几十个工具节点，那样"保留三轮"会退化成
//! "保留最近三个节点"。
//!
//! 边界选用户消息而不是助手消息，是因为保留段必须以"用户要求做什么"开头。若以助手消息
//! 为界，保留段会从助手回复开始，而它对应的用户输入被压进摘要——模型会看到自己在回答
//! 一个已经不在上下文里的问题。
//!
//! 折叠范围严格止于保留轮次的起点：摘要覆盖 `[旧头部, 保留起点)`，与保留部分不重叠。
//! 重叠会让同一件事以摘要和原文两种版本同时出现在上下文里，模型遇到矛盾时行为不可预测。

use super::super::node::{Node, TurnRole};

/// 折叠计划。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoldPlan {
    /// 要被摘要覆盖的节点，按时间顺序（旧到新）。
    pub covered: Vec<super::super::ids::NodeId>,
    /// 要保留为原文的节点，按时间顺序。
    pub kept: Vec<super::super::ids::NodeId>,
}

/// 无法计算折叠范围的原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FoldError {
    /// 轮次不足：历史里没有足够多的轮次可以保留。
    NotEnoughTurns {
        /// 实际找到的轮数。
        found: u32,
        /// 需要保留的轮数。
        keep: u32,
    },
    /// 轮数够，但保留段已经延伸到历史开头，没有可压缩的部分。
    NothingToFold {
        /// 实际找到的轮数。
        turns: u32,
        /// 保留的轮数。
        keep: u32,
    },
}

/// 该节点是否固定保留、不参与折叠。
///
/// 固定层在压缩时重建，待办必须原样保留；两者都不属于"可以被摘要掉的历史"。
fn is_pinned(node: &Node) -> bool {
    matches!(
        node.body,
        super::super::node::NodeBody::FixedLayer { .. } | super::super::node::NodeBody::Todo { .. }
    )
}

impl std::fmt::Display for FoldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotEnoughTurns { found, keep } => {
                write!(f, "只有 {found} 轮，不足以在保留 {keep} 轮之外再压缩")
            }
            Self::NothingToFold { turns, keep } => {
                write!(f, "共 {turns} 轮、保留 {keep} 轮时保留段已到历史开头，没有可压缩的部分")
            }
        }
    }
}

impl std::error::Error for FoldError {}

/// 计算折叠范围。
///
/// # 步骤
/// 1. 从新到旧找出每一轮的起点（用户消息）。
/// 2. 保留最近 `keep` 轮，找到保留部分的第一个节点。
/// 3. 该节点之前的全部节点归入覆盖范围。
///
/// # 边界与前提
/// `history` 必须按时间顺序（旧到新）。只数用户消息：助手回复、工具调用与结果不开启新轮。
/// 轮数恰好等于 `keep` 时没有可压缩的部分，返回 [`FoldError::NotEnoughTurns`]——此时
/// 硬压会把保留段也吞掉。
///
/// # 副作用
/// 无。
///
/// # 注意事项
/// 保留段从第 `keep` 轮（从新往旧数）的用户消息开始，因此它总是以"用户要求做什么"开头，
/// 与该轮的助手回复、工具调用连在一起，语义完整。
pub fn plan(history: &[Node], keep: u32) -> Result<FoldPlan, FoldError> {
    if keep == 0 {
        // 保留零轮等于把全部历史压掉，压缩后模型没有任何原文可对照，摘要的错误无法被
        // 自己发现。预算构造期已经拒绝这种配置，这里再挡一次。
        return Err(FoldError::NotEnoughTurns { found: 0, keep: 0 });
    }

    // 找出所有用户消息的位置，它们是轮次的起点。
    let turn_starts: Vec<usize> = history
        .iter()
        .enumerate()
        .filter(|(_, node)| {
            matches!(node.body, super::super::node::NodeBody::Turn { role: TurnRole::User, .. })
        })
        .map(|(index, _)| index)
        .collect();

    let found = u32::try_from(turn_starts.len()).unwrap_or(u32::MAX);
    if turn_starts.len() < keep as usize {
        return Err(FoldError::NotEnoughTurns { found, keep });
    }

    // 保留最近 keep 轮的起点：从末尾往前数第 keep 个。
    let keep_from = turn_starts[turn_starts.len() - keep as usize];

    // 轮数够但保留段一直延伸到开头，说明没有可压缩的部分。这与"轮数不足"是两件事：
    // 前者是"已经压无可压"，后者是"历史太短"，调用方的反应不同（前者可以继续用，后者
    // 说明配置有问题）。
    if keep_from == 0 {
        return Err(FoldError::NothingToFold { turns: found, keep });
    }

    // 固定层与待办不参与折叠：前者在压缩时重建（0018 第三节），后者必须原样保留
    // （0004：压缩后仍要有当前待办）。把它们留在 `kept` 里，位置由调用方按四段结构排。
    let covered =
        history[..keep_from].iter().filter(|node| !is_pinned(node)).map(|n| n.id.clone()).collect();
    let kept = history[..keep_from]
        .iter()
        .filter(|node| is_pinned(node))
        .map(|n| n.id.clone())
        .chain(history[keep_from..].iter().map(|n| n.id.clone()))
        .collect();
    Ok(FoldPlan { covered, kept })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ids::{NodeId, SessionId};
    use crate::context::node::NodeBody;

    const BASE: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

    fn id(n: u8) -> NodeId {
        // 用末两位编码，避免 n >= 10 时与低位 id 碰撞。
        let mut text = BASE.to_owned();
        text.replace_range(24..26, &format!("{n:02}"));
        NodeId::parse(text).expect("应当合法")
    }

    fn session() -> SessionId {
        SessionId::parse(BASE).expect("应当合法")
    }

    fn user(n: u8) -> Node {
        Node {
            id: id(n),
            parent: None,
            session: session(),
            body: NodeBody::Turn { role: TurnRole::User, text: "问".to_owned() },
        }
    }

    fn assistant(n: u8) -> Node {
        Node {
            id: id(n),
            parent: None,
            session: session(),
            body: NodeBody::Turn { role: TurnRole::Assistant, text: "答".to_owned() },
        }
    }

    fn tool(n: u8) -> Node {
        Node {
            id: id(n),
            parent: None,
            session: session(),
            body: NodeBody::ToolCall { command: "ita edit".to_owned() },
        }
    }

    /// 四轮历史，每轮是 用户 → 助手 → 工具。
    fn four_turns() -> Vec<Node> {
        let mut out = Vec::new();
        for turn in 0..4u8 {
            let base = turn * 3;
            out.push(user(base + 1));
            out.push(assistant(base + 2));
            out.push(tool(base + 3));
        }
        out
    }

    /// 保留三轮时前一轮被覆盖，折叠永远留下最新的轮次。
    #[test]
    fn keeping_three_turns_covers_previous_turn() {
        let history = four_turns();
        let plan = plan(&history, 3).expect("应当可以折叠");
        // 第一轮三个节点被覆盖，其余九个保留。
        assert_eq!(plan.covered.len(), 3);
        assert_eq!(plan.kept.len(), 9);
        assert_eq!(plan.covered[0], id(1));
        assert_eq!(plan.kept[0], id(4), "保留段从第二轮的用户消息开始");
    }

    /// 覆盖与保留不重叠且合起来是全部节点，顺序不变，节点不丢不重。
    #[test]
    fn covered_and_kept_disjoint_and_exhaustive() {
        let history = four_turns();
        let plan = plan(&history, 3).expect("应当可以折叠");
        let mut all: Vec<NodeId> = plan.covered.clone();
        all.extend(plan.kept.clone());
        let original: Vec<NodeId> = history.iter().map(|n| n.id.clone()).collect();
        assert_eq!(all, original, "两部分必须恰好覆盖全部历史且顺序不变");
    }

    /// 轮数恰好等于保留数时报"无可压缩"而不是"历史太短"，两种情况被分开。
    #[test]
    fn turns_equal_to_keep_reports_nothing_to_fold() {
        let history = four_turns();
        let err = plan(&history, 4).expect_err("应当被拒");
        // 与"轮数不足"分开：这里是"已经压无可压"，不是"历史太短"。
        assert_eq!(err, FoldError::NothingToFold { turns: 4, keep: 4 });
    }

    /// 轮数少于保留数时拒绝，太短的历史不会被折叠。
    #[test]
    fn turns_less_than_keep_rejected() {
        let history = four_turns();
        let err = plan(&history, 9).expect_err("应当被拒");
        assert!(matches!(err, FoldError::NotEnoughTurns { .. }));
    }

    /// 保留零轮被拒，保留什么都不做的折叠永远不会被计划。
    #[test]
    fn keep_zero_rejected() {
        assert!(plan(&four_turns(), 0).is_err());
    }

    /// 保留段总是以用户消息开头，保留的轮次不会从轮中间被切开。
    #[test]
    fn kept_segment_always_starts_with_user_message() {
        let history = four_turns();
        for keep in 1..=3 {
            let plan = plan(&history, keep).expect("应当可以折叠");
            let first = history.iter().find(|n| n.id == plan.kept[0]).expect("应当能找到");
            assert!(
                matches!(first.body, NodeBody::Turn { role: TurnRole::User, .. }),
                "保留 {keep} 轮时保留段却从非用户消息开始"
            );
        }
    }

    /// 固定层与待办不参与折叠，系统提示与待办项永远不会被摘要掉。
    #[test]
    fn fixed_layer_and_todo_not_participate_in_folding() {
        let mut history = four_turns();
        let fixed = Node {
            id: id(90),
            parent: None,
            session: session(),
            body: NodeBody::FixedLayer {
                fingerprint: "fp".to_owned(), content: "系统".to_owned()
            },
        };
        let todo = Node {
            id: id(91),
            parent: None,
            session: session(),
            body: NodeBody::Todo { items: vec!["写测试".to_owned()] },
        };
        history.insert(0, fixed.clone());
        history.push(todo.clone());

        let plan = plan(&history, 3).expect("应当可以折叠");
        assert!(!plan.covered.contains(&fixed.id), "固定层不应被摘要掉");
        assert!(plan.kept.contains(&fixed.id), "固定层应留在保留侧");
        assert!(plan.kept.contains(&todo.id), "待办必须原样保留");
        assert!(!plan.covered.contains(&todo.id));
    }

    /// 工具节点不开启新轮，一轮里塞十个工具节点仍然只算一轮。
    #[test]
    fn tool_nodes_do_not_open_new_turns() {
        // 一轮里塞十个工具节点，仍然只算一轮。
        let mut history = vec![user(1), assistant(2)];
        for n in 3..=9 {
            history.push(tool(n));
        }
        history.push(user(20));
        history.push(assistant(21));
        // 两轮、保留一轮：覆盖范围包含第一轮的全部十一个节点。
        let plan = plan(&history, 1).expect("应当可以折叠");
        assert_eq!(plan.covered.len(), 9, "第一轮的十一个节点里九个被覆盖");
        assert_eq!(plan.kept.len(), 2, "第二轮只有一个用户消息与一个助手回复");
    }

    /// 空历史被拒，空折叠永远不会被计划。
    #[test]
    fn empty_history_rejected() {
        assert!(plan(&[], 3).is_err());
    }

    /// 保留两轮时覆盖前两轮，折叠永远留下最新的轮次。
    #[test]
    fn keeping_two_turns_covers_first_two() {
        let history = four_turns();
        let plan = plan(&history, 2).expect("应当可以折叠");
        // 保留最近两轮：起点是第三轮的用户消息 id(7)。
        assert_eq!(plan.kept[0], id(7));
        assert_eq!(plan.covered.len(), 6);
    }
}
