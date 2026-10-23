//! 上下文节点与帧。
//!
//! 节点是不可变的：一旦写入，内容永不改变。会话的推进只移动 head 指针，因此历史永远
//! 可以按节点 id 取回——这是回退、分叉与 replay 共同依赖的前提。
//!
//! 帧是节点的一种：它不存原文，只存一段摘要与它覆盖的节点区间。帧对检查点是弱引用，
//! 允许悬空，因为快照的保留窗口与上下文的保留策略是两件独立的事。

use super::ids::{CheckpointId, NodeId, SessionId};

/// 一段大文本在内容寻址存储里的位置。
///
/// 正文不进节点：工具输出可能有几十万字节，全部塞进上下文节点会让解析与复制都变得昂贵。
/// 节点里只留句柄与预览，需要原文时按句柄取回——这同时是"压缩是有损的、但可以再取回"
/// 这个承诺的实现基础。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobRef {
    /// 内容的哈希，十六进制文本形式。
    pub hash: String,
    /// 字节数。
    pub len: u64,
}

/// 消息的方向。
///
/// 解析时要按方向拼装消息数组，压缩时要数"最近三轮"，两者都依赖这个字段；不写进节点
/// 就只能靠位置推断，而分叉与回退会让位置不再单调。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnRole {
    /// 用户（或上层编排者）发出的输入。
    User,
    /// 模型产出的内容。
    Assistant,
}

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
        /// 这一轮由谁产生。
        role: TurnRole,
        /// 这一轮的完整消息文本。
        text: String,
    },
    /// 一次工具调用。
    ///
    /// 调用与结果分成两个节点，因为原生工具调用在供应商的报文里就是两条消息：助手的
    /// `tool_use` 与用户的 `tool_result`。合成一个节点会让解析时无法还原消息数组。
    ToolCall {
        /// 模型给出的命令原文。
        command: String,
    },
    /// 一次工具调用的结果。
    ToolResult {
        /// 结果正文在内容寻址存储里的位置。
        body: Option<BlobRef>,
        /// 给模型看的一小段预览；正文超长时只保留头尾。
        preview: String,
        /// 进程退出码。非零不代表失败，由调用方解释。
        exit_code: i32,
    },
    /// 折叠出来的帧。
    Frame(Frame),
    /// 一条系统通知：漂移提醒、文件已更新、工具目录追加等。
    Notice {
        /// 通知正文。
        text: String,
    },
    /// 当前待办列表。
    ///
    /// 单独一个变体而不是塞进 `Notice`，因为它有固定结构与固定位置（解析结果的最后
    /// 一段），而且压缩时要原样保留而不是被摘要掉。
    Todo {
        /// 待办条目，按顺序。
        items: Vec<String>,
    },
}

/// 帧：被摘要替代的一段历史。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// 摘要正文。散文部分由模型写，事实部分由机器填，两者已经合并成一段文本。
    pub summary: String,
    /// 被覆盖的节点，按时间顺序且不重复。
    ///
    /// 顺序由构造方保证（压缩时按遍历顺序收集）。解析时按顺序判断"某个节点是否已在帧
    /// 里"，不做排序，因为排序会掩盖构造方的错误。
    pub covers: Vec<NodeId>,
    /// 这一段历史对应的检查点。允许悬空：快照可能已经被挤出保留窗口。
    pub checkpoint: Option<CheckpointId>,
    /// 生成这段摘要时固定层的指纹。
    ///
    /// 记录它是因为压缩会重建固定层；没有它，replay 无法判断旧帧当时看到的是哪一份
    /// 系统提示与工具目录，逐字节复现就不成立。注意这是"当时"的指纹，不是当前的。
    pub fixed_layer_fingerprint: String,
}

impl Frame {
    /// 这个帧是否覆盖了某个节点。
    ///
    /// # 边界与前提
    /// 线性扫描。解析路径上应当对每个待判断节点调用一次，而不是对每个节点遍历全部帧；
    /// 帧的数量远小于节点数量，因此这个方向的代价是可接受的。
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
    ///
    /// **父边可以跨会话**：分叉产生的新会话从某个祖先节点继续，那个祖先属于父会话。
    /// 因此遍历祖先链时不能按 `session` 过滤，否则会静默丢掉共享的那一段历史。
    pub parent: Option<NodeId>,
    /// 创建这个节点的会话。祖先节点保留它原本的会话，不因分叉而改写。
    pub session: SessionId,
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

    /// 这个节点是否属于"最近三轮"意义上的对话轮次。
    ///
    /// # 边界与前提
    /// 只有助手消息开启新的一轮；用户输入、工具调用与结果都属于当前这一轮。压缩时按
    /// 这个边界数三轮，不能按节点数数——一轮里可能有很多节点。
    ///
    /// # 副作用
    /// 无。
    pub fn opens_turn(&self) -> bool {
        matches!(self.body, NodeBody::Turn { role: TurnRole::Assistant, .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn 帧的检查点是弱引用() {
        // 弱引用的含义是"不保证可达"，因此构造时不校验，可达性由解析时注入的事实决定。
        let mut f = frame(vec![]);
        f.checkpoint = Some(CheckpointId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAV").expect("合法"));
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

    #[test]
    fn 只有助手消息开启新的一轮() {
        let assistant = Node {
            id: id(1),
            parent: None,
            session: session(),
            body: NodeBody::Turn { role: TurnRole::Assistant, text: "好".to_owned() },
        };
        let user = Node {
            id: id(2),
            parent: None,
            session: session(),
            body: NodeBody::Turn { role: TurnRole::User, text: "做这个".to_owned() },
        };
        let tool = Node {
            id: id(3),
            parent: None,
            session: session(),
            body: NodeBody::ToolCall { command: "ita edit a.rs".to_owned() },
        };
        assert!(assistant.opens_turn());
        assert!(!user.opens_turn());
        assert!(!tool.opens_turn());
    }

    #[test]
    fn 工具结果的正文以句柄承载() {
        let node = Node {
            id: id(1),
            parent: None,
            session: session(),
            body: NodeBody::ToolResult {
                body: Some(BlobRef { hash: "ab".repeat(32), len: 1024 }),
                preview: "ok".to_owned(),
                exit_code: 0,
            },
        };
        let NodeBody::ToolResult { body: Some(blob), .. } = &node.body else {
            panic!("应当是带句柄的工具结果");
        };
        assert_eq!(blob.len, 1024);
    }

    #[test]
    fn 待办是独立变体() {
        let node = Node {
            id: id(1),
            parent: None,
            session: session(),
            body: NodeBody::Todo { items: vec!["写测试".to_owned()] },
        };
        assert!(!node.opens_turn());
        assert!(node.as_frame().is_none());
    }
}
