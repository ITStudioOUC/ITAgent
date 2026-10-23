//! 上下文节点与帧。
//!
//! 节点是不可变的：一旦写入，内容永不改变。会话的推进只移动 head 指针，因此历史永远
//! 可以按节点 id 取回——这是回退、分叉与 replay 共同依赖的前提。
//!
//! 帧是节点的一种：它不存原文，只存一段摘要与它覆盖的节点区间。帧对检查点是弱引用，
//! 允许悬空，因为快照的保留窗口与上下文的保留策略是两件独立的事。

use super::ids::{CheckpointId, NodeId};

/// 节点承载的内容种类。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeBody {
    /// 系统提示与工具目录，会话开始时冻结。
    FixedLayer {
        /// 冻结内容的摘要，用于判断两次解析是否基于同一份固定层。
        fingerprint: String,
    },
    /// 一轮对话里的原文。
    Turn {
        /// 这一轮的完整消息文本。
        text: String,
    },
    /// 一次工具调用与它的结果。
    ToolCall {
        /// 模型给出的命令原文。
        command: String,
        /// 结果摘要（正文在内容寻址存储里，这里只留可读的一小段）。
        result_preview: String,
    },
    /// 折叠出来的帧。
    Frame(Frame),
    /// 一条系统通知：漂移提醒、文件已更新、工具目录追加等。
    Notice {
        /// 通知正文。
        text: String,
    },
}

/// 帧：被摘要替代的一段历史。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// 摘要正文。散文部分由模型写，事实部分由机器填，两者已经合并成一段文本。
    pub summary: String,
    /// 被覆盖的节点，按时间顺序。用于判断某个节点是否已经"在帧里"。
    pub covers: Vec<NodeId>,
    /// 这一段历史对应的检查点。允许悬空：快照可能已经被挤出保留窗口。
    pub checkpoint: Option<CheckpointId>,
    /// 生成这段摘要时固定层的指纹。
    ///
    /// 记录它是因为压缩会重建固定层；没有它，replay 无法判断旧帧当时看到的是哪一份
    /// 系统提示与工具目录，逐字节复现就不成立。
    pub fixed_layer_fingerprint: String,
}

impl Frame {
    /// 这个帧是否覆盖了某个节点。
    ///
    /// # 边界与前提
    /// 线性扫描：一个帧通常只覆盖几十个节点，而这段代码在解析路径上被调用一次而非每个
    /// 节点一次。
    ///
    /// # 副作用
    /// 无。
    pub fn covers_node(&self, node: &NodeId) -> bool {
        self.covers.iter().any(|n| n == node)
    }
}

/// 上下文树上的一个节点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// 节点 id。
    pub id: NodeId,
    /// 父节点。根节点没有父。
    pub parent: Option<NodeId>,
    /// 所属会话。分叉时子会话从某个祖先节点开始，节点本身不复制。
    pub session: super::ids::SessionId,
    /// 内容。
    pub body: NodeBody,
}

impl Node {
    /// 是否为帧节点。
    pub fn as_frame(&self) -> Option<&Frame> {
        match &self.body {
            NodeBody::Frame(frame) => Some(frame),
            _ => None,
        }
    }

    /// 是否为固定层节点。
    pub fn is_fixed_layer(&self) -> bool {
        matches!(self.body, NodeBody::FixedLayer { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ids::SessionId;

    fn id(n: u8) -> NodeId {
        let mut text = "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_owned();
        text.replace_range(25..26, &char::from(b'0' + n % 10).to_string());
        NodeId::parse(text).expect("应当合法")
    }

    fn session() -> SessionId {
        SessionId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAV").expect("应当合法")
    }

    fn frame(covers: Vec<NodeId>) -> Frame {
        Frame {
            summary: "做了 A，否决了 B".to_owned(),
            covers,
            checkpoint: None,
            fixed_layer_fingerprint: "fp-1".to_owned(),
        }
    }

    #[test]
    fn 帧记录自己覆盖的节点() {
        let f = frame(vec![id(1), id(2)]);
        assert!(f.covers_node(&id(1)));
        assert!(!f.covers_node(&id(3)));
    }

    #[test]
    fn 帧可以引用已不存在的检查点() {
        let mut f = frame(vec![]);
        f.checkpoint = Some(CheckpointId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAV").expect("合法"));
        // 弱引用不校验可达性：可达性由解析时注入的事实决定。
        assert!(f.checkpoint.is_some());
    }

    #[test]
    fn 节点可以取回自己的帧() {
        let node = Node {
            id: id(1),
            parent: None,
            session: session(),
            body: NodeBody::Frame(frame(vec![])),
        };
        assert!(node.as_frame().is_some());
        assert!(!node.is_fixed_layer());
    }

    #[test]
    fn 固定层节点可被识别() {
        let node = Node {
            id: id(1),
            parent: None,
            session: session(),
            body: NodeBody::FixedLayer { fingerprint: "fp".to_owned() },
        };
        assert!(node.is_fixed_layer());
        assert!(node.as_frame().is_none());
    }
}
