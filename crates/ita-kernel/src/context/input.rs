//! 解析输入：纯函数 [`resolve`](super::resolve::resolve) 需要的全部外部状态。
//!
//! 解析不读库、不读时钟、不读文件系统，因此凡是它要用的东西都必须由调用方装进来。这个
//! 结构就是那份"装好的状态"：固定层内容、帧、最近轮次、检查点事实、预算。
//!
//! 把输入显式化带来一个额外的好处：replay 只需要保存这个结构，就能逐字节复现某一轮模型
//! 看到的全部内容。

use super::budget::Budget;
use super::ids::{CheckpointId, NodeId};
use super::node::{Frame, Node, NodeBody};

/// 一个检查点在当前时刻的事实。
///
/// 两个布尔量是正交的：检查点可能还在保留窗口内但磁盘已经变了，也可能已经不可达而磁盘
/// 恰好一致。把它们压成一个三态枚举会让"不可达且已变化"这一格没有名字，而它恰恰是最
/// 危险的一格——模型会误以为知道方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckpointFacts {
    /// 该检查点是否仍在快照保留窗口内。
    pub reachable: bool,
    /// 当前磁盘内容是否等于该检查点记录的哈希。
    pub disk_matches: bool,
}

/// 解析输入。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveInput {
    /// 固定层：系统提示、工具目录快照、AGENTS.md 的冻结内容。
    ///
    /// 会话开始时冻结；只有压缩可以重建它。变化通过追加 `Notice` 节点传播，而不是改写
    /// 这里的内容——改写会作废整个前缀缓存。
    pub fixed_layer: Node,
    /// 帧，按时间顺序。
    pub frames: Vec<Frame>,
    /// 保留的最近轮次原文，按时间顺序。
    ///
    /// 直接放节点而不是只放 id：解析不能查库，调用方在装输入时就把它们取出来。0018 写
    /// `Vec<NodeId>` 是形状示意，真正的契约是"解析所需的一切都在输入里"，此处取更严格
    /// 的读法并在此注明。
    pub recent: Vec<Node>,
    /// 当前待办与其它需要放在末尾的节点。
    pub current: Vec<Node>,
    /// 检查点事实，键是检查点 id。
    pub checkpoint_facts: Vec<(CheckpointId, CheckpointFacts)>,
    /// 预算。
    pub budget: Budget,
}

/// 装输入时的错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputError {
    /// 帧的顺序不对：后一个帧的开始早于前一个帧的结束。
    FramesOutOfOrder {
        /// 出问题的位置。
        index: usize,
    },
    /// 帧引用了不在检查点事实表里的检查点。
    ///
    /// 调用方必须补齐事实；解析不会为缺失项发明一种措辞，因为"未知"与"不可达"在给模型
    /// 的提示里必须是不同的话。
    MissingCheckpointFacts(CheckpointId),
    /// 固定层节点的内容不是固定层。
    NotFixedLayer,
    /// 最近轮次里出现了帧或固定层节点——那说明调用方把两类东西混在了一起。
    WrongRecentBody {
        /// 出问题的节点。
        node: NodeId,
    },
}

impl std::fmt::Display for InputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FramesOutOfOrder { index } => write!(f, "第 {index} 个帧的顺序与前一帧冲突"),
            Self::MissingCheckpointFacts(id) => {
                write!(f, "帧引用的检查点 {id} 缺少事实，调用方必须先补齐")
            }
            Self::NotFixedLayer => write!(f, "固定层节点的内容不是固定层"),
            Self::WrongRecentBody { node } => {
                write!(f, "最近轮次里出现了不该有的节点 {node}（帧或固定层）")
            }
        }
    }
}

impl std::error::Error for InputError {}

impl ResolveInput {
    /// 校验输入自洽。
    ///
    /// # 步骤
    /// 1. 固定层节点的内容确实是固定层。
    /// 2. 每个帧引用的检查点都有事实。
    /// 3. 最近轮次里没有帧与固定层节点。
    /// 4. 帧之间不重叠。
    ///
    /// # 边界与前提
    /// 这里只校验结构，不校验"帧是否覆盖了最近轮次"——那属于压缩提交时的检查，因为
    /// 解析器可以合法地看到一段"既在帧里又在最近轮次里"的输入（调用方装错了，但解析
    /// 仍然应当给出确定的结果）。
    ///
    /// # 副作用
    /// 无。
    pub fn validate(&self) -> Result<(), InputError> {
        if !self.fixed_layer.is_fixed_layer() {
            return Err(InputError::NotFixedLayer);
        }
        for frame in &self.frames {
            if let Some(checkpoint) = &frame.checkpoint
                && !self.checkpoint_facts.iter().any(|(id, _)| id == checkpoint)
            {
                return Err(InputError::MissingCheckpointFacts(checkpoint.clone()));
            }
        }
        for node in &self.recent {
            if matches!(node.body, NodeBody::Frame(_) | NodeBody::FixedLayer { .. }) {
                return Err(InputError::WrongRecentBody { node: node.id.clone() });
            }
        }
        // 任意两个帧都不能共享节点：只看相邻帧的首尾会漏掉"后一帧完全包含前一帧"这类
        // 情况，而那正是压缩重复执行最容易产生的错误输入。
        for (index, later) in self.frames.iter().enumerate().skip(1) {
            let earlier = &self.frames[..index];
            if earlier.iter().any(|frame| frame.covers.iter().any(|n| later.covers.contains(n))) {
                return Err(InputError::FramesOutOfOrder { index });
            }
        }
        Ok(())
    }

    /// 取某个检查点的事实。
    ///
    /// # 边界与前提
    /// 调用前应当先 [`validate`](Self::validate)；缺失时返回 `None`，由解析决定如何措辞。
    ///
    /// # 副作用
    /// 无。
    pub fn facts(&self, checkpoint: &CheckpointId) -> Option<CheckpointFacts> {
        self.checkpoint_facts.iter().find(|(id, _)| id == checkpoint).map(|(_, facts)| *facts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ids::SessionId;
    use crate::context::node::TurnRole;

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

    fn fixed() -> Node {
        Node {
            id: id(0),
            parent: None,
            session: session(),
            body: NodeBody::FixedLayer {
                fingerprint: "fp".to_owned(),
                content: "system".to_owned(),
            },
        }
    }

    fn turn_node(n: u8) -> Node {
        Node {
            id: id(n),
            parent: None,
            session: session(),
            body: NodeBody::Turn { role: TurnRole::User, text: "hi".to_owned() },
        }
    }

    fn frame(covers: Vec<NodeId>, cp: Option<CheckpointId>) -> Frame {
        Frame {
            summary: "s".to_owned(),
            covers,
            checkpoint: cp,
            fixed_layer_fingerprint: "fp".to_owned(),
        }
    }

    fn input() -> ResolveInput {
        ResolveInput {
            fixed_layer: fixed(),
            frames: vec![],
            recent: vec![turn_node(1)],
            current: vec![],
            checkpoint_facts: vec![],
            budget: Budget::defaults(),
        }
    }

    #[test]
    fn 最小输入通过校验() {
        assert!(input().validate().is_ok());
    }

    #[test]
    fn 固定层内容不对时被拒() {
        let mut i = input();
        i.fixed_layer = turn_node(1);
        assert_eq!(i.validate(), Err(InputError::NotFixedLayer));
    }

    #[test]
    fn 帧引用的检查点必须带事实() {
        let mut i = input();
        i.frames = vec![frame(vec![id(2)], Some(checkpoint(1)))];
        assert!(matches!(i.validate(), Err(InputError::MissingCheckpointFacts(_))));

        i.checkpoint_facts =
            vec![(checkpoint(1), CheckpointFacts { reachable: true, disk_matches: true })];
        assert!(i.validate().is_ok());
    }

    #[test]
    fn 最近轮次里不能有帧或固定层() {
        let mut i = input();
        i.recent = vec![Node {
            id: id(3),
            parent: None,
            session: session(),
            body: NodeBody::Frame(frame(vec![], None)),
        }];
        assert!(matches!(i.validate(), Err(InputError::WrongRecentBody { .. })));
    }

    #[test]
    fn 帧之间重叠被拒() {
        let mut i = input();
        i.frames = vec![frame(vec![id(1), id(2)], None), frame(vec![id(2), id(3)], None)];
        assert_eq!(i.validate(), Err(InputError::FramesOutOfOrder { index: 1 }));
    }

    #[test]
    fn 完全包含的帧被拒() {
        let mut i = input();
        i.frames = vec![frame(vec![id(1), id(2)], None), frame(vec![id(1), id(2), id(3)], None)];
        assert_eq!(i.validate(), Err(InputError::FramesOutOfOrder { index: 1 }));
    }

    #[test]
    fn 不相邻但重叠的帧被拒() {
        let mut i = input();
        i.frames = vec![
            frame(vec![id(1), id(2)], None),
            frame(vec![id(3), id(4)], None),
            frame(vec![id(2), id(5)], None),
        ];
        assert_eq!(i.validate(), Err(InputError::FramesOutOfOrder { index: 2 }));
    }

    #[test]
    fn 相邻但不重叠的帧被接受() {
        let mut i = input();
        i.frames = vec![frame(vec![id(1), id(2)], None), frame(vec![id(3), id(4)], None)];
        assert!(i.validate().is_ok());
    }

    #[test]
    fn 事实按检查点取回() {
        let mut i = input();
        i.checkpoint_facts =
            vec![(checkpoint(1), CheckpointFacts { reachable: false, disk_matches: true })];
        let facts = i.facts(&checkpoint(1)).expect("应当有事实");
        assert!(!facts.reachable);
        assert!(facts.disk_matches);
        assert!(i.facts(&checkpoint(2)).is_none());
    }
}
