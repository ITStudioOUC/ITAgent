//! 保留轮次里的工具正文瘦身。
//!
//! 保留"最近三轮原文"听起来可控，但一轮里可能有一次两千行的文件读取。按轮保留而不动正文，
//! 预算照样会爆。因此保留轮次里的工具结果只留三样东西：命令、一行结果、可按编号寻回的
//! 节点。
//!
//! 原文不删除，只是不放进消息数组——它仍在内容寻址存储里，模型需要时可以按节点编号取回。
//! 这把"压缩有损"降级成"多一次往返"，是这套设计能同时做到省预算与不丢信息的关键。

use super::super::ids::NodeId;
use super::super::node::{BlobRef, Node, NodeBody};

/// 瘦身后的工具结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlimToolResult {
    /// 结果所属的节点，模型可以按它取回原文。
    pub node: NodeId,
    /// 一行结果摘要。
    pub line: String,
    /// 退出码。
    pub exit_code: i32,
    /// 原文句柄；没有正文时为空。
    pub body: Option<BlobRef>,
}

/// 瘦身时遇到的异常。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlimError {
    /// 节点不是工具结果，调用方用错了函数。
    NotAToolResult(NodeId),
}

impl std::fmt::Display for SlimError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAToolResult(id) => write!(f, "节点 {id} 不是工具结果"),
        }
    }
}

impl std::error::Error for SlimError {}

/// 把一条工具结果瘦身。
///
/// # 步骤
/// 1. 取预览的第一行作为一行结果。
/// 2. 保留退出码与原文句柄。
///
/// # 边界与前提
/// 预览为空时用"（无输出）"占位，而不是留空：空字符串在消息里看起来像渲染错误，模型会
/// 试图重新执行命令来确认。多行预览只取第一行，其余靠节点编号寻回。
///
/// # 副作用
/// 无。
pub fn slim(node: &Node) -> Result<SlimToolResult, SlimError> {
    let NodeBody::ToolResult { body, preview, exit_code } = &node.body else {
        return Err(SlimError::NotAToolResult(node.id.clone()));
    };

    let line = preview.lines().next().unwrap_or("").trim();
    let line = if line.is_empty() { "（无输出）".to_owned() } else { line.to_owned() };

    Ok(SlimToolResult { node: node.id.clone(), line, exit_code: *exit_code, body: body.clone() })
}

/// 把瘦身结果渲染成给模型看的一行。
///
/// # 边界与前提
/// 带上节点编号是刻意的：没有它，模型无法表达"我要看这条结果的全文"，只能重跑命令。
///
/// # 副作用
/// 无。
pub fn render(slim: &SlimToolResult) -> String {
    format!("[退出码 {}] {}（完整输出见节点 {}）", slim.exit_code, slim.line, slim.node)
}

/// 批量瘦身一组节点，非工具结果的节点原样跳过。
///
/// # 边界与前提
/// 顺序保持不变；跳过的节点不出现在结果里，调用方按节点 id 对齐。
///
/// # 副作用
/// 无。
pub fn slim_all(nodes: &[Node]) -> Vec<SlimToolResult> {
    nodes.iter().filter_map(|n| slim(n).ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ids::SessionId;

    const BASE: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

    fn id(n: u8) -> NodeId {
        let mut text = BASE.to_owned();
        text.replace_range(24..26, &format!("{n:02}"));
        NodeId::parse(text).expect("应当合法")
    }

    fn session() -> SessionId {
        SessionId::parse(BASE).expect("应当合法")
    }

    fn result_node(n: u8, preview: &str, exit_code: i32) -> Node {
        Node {
            id: id(n),
            parent: None,
            session: session(),
            body: NodeBody::ToolResult {
                body: Some(BlobRef { hash: "ab".repeat(32), len: 4096 }),
                preview: preview.to_owned(),
                exit_code,
            },
        }
    }

    #[test]
    fn 多行预览只保留第一行() {
        let node = result_node(1, "第一行\n第二行\n第三行", 0);
        let slim = slim(&node).expect("应当成功");
        assert_eq!(slim.line, "第一行");
        assert_eq!(slim.exit_code, 0);
    }

    #[test]
    fn 空输出用占位而不是留空() {
        let node = result_node(1, "", 0);
        let slim = slim(&node).expect("应当成功");
        assert_eq!(slim.line, "（无输出）");
    }

    #[test]
    fn 只有空白的输出也用占位() {
        let node = result_node(1, "   \n\n", 0);
        assert_eq!(slim(&node).expect("应当成功").line, "（无输出）");
    }

    #[test]
    fn 原文句柄被保留以便寻回() {
        let node = result_node(1, "ok", 0);
        let slim = slim(&node).expect("应当成功");
        let body = slim.body.expect("应当有句柄");
        assert_eq!(body.len, 4096);
    }

    #[test]
    fn 渲染结果带节点编号() {
        let node = result_node(7, "ok", 2);
        let slim = slim(&node).expect("应当成功");
        let text = render(&slim);
        assert!(text.contains("退出码 2"));
        assert!(text.contains("完整输出见节点"));
        assert!(text.contains(id(7).as_str()));
    }

    #[test]
    fn 非工具结果被拒() {
        let node = Node {
            id: id(1),
            parent: None,
            session: session(),
            body: NodeBody::Turn {
                role: crate::context::node::TurnRole::User,
                text: "问".to_owned(),
            },
        };
        assert!(matches!(slim(&node), Err(SlimError::NotAToolResult(_))));
    }

    #[test]
    fn 批量瘦身跳过非工具节点并保持顺序() {
        let nodes = vec![
            result_node(1, "a", 0),
            Node {
                id: id(2),
                parent: None,
                session: session(),
                body: NodeBody::ToolCall { command: "ita edit".to_owned() },
            },
            result_node(3, "b", 1),
        ];
        let slimmed = slim_all(&nodes);
        assert_eq!(slimmed.len(), 2);
        assert_eq!(slimmed[0].node, id(1));
        assert_eq!(slimmed[1].node, id(3));
    }

    #[test]
    fn 无正文的结果也能瘦身() {
        let node = Node {
            id: id(1),
            parent: None,
            session: session(),
            body: NodeBody::ToolResult { body: None, preview: "done".to_owned(), exit_code: 0 },
        };
        let slim = slim(&node).expect("应当成功");
        assert!(slim.body.is_none());
    }
}
