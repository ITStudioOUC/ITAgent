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
    /// 产生这条结果的命令。
    ///
    /// 必须带上：没有命令，模型看到一行 `error[E0308]` 时无法判断是哪次调用产生的，
    /// 只能重跑一遍来确认。
    pub command: Option<String>,
    /// 一行结果摘要。
    pub line: String,
    /// 退出码。
    pub exit_code: i32,
    /// 原文句柄；没有正文时为空。
    pub body: Option<BlobRef>,
}

/// 一行结果的长度上限（字节）。
///
/// 预览可能是一整段没有换行的日志，取"第一行"并不能保证它短。超过上限时截断并加省略号，
/// 否则"瘦身"对这类输出完全无效。
pub const LINE_LIMIT: usize = 512;

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
/// 1. 取预览里第一条非空行作为一行结果，超过 [`LINE_LIMIT`] 时截断。
/// 2. 保留命令、退出码与原文句柄。
///
/// # 边界与前提
/// 预览全为空时用"（无输出）"占位，而不是留空：空字符串在消息里看起来像渲染错误，模型会
/// 试图重新执行命令来确认。跳过空行是因为工具输出常以空行开头，取第一行会得到空串。
///
/// `command` 由调用方传入（来自同一轮的 `ToolCall` 节点）：内核不维护"哪个调用产生了哪条
/// 结果"的映射，那属于会话结构，由组装解析输入的一方负责。
///
/// # 副作用
/// 无。
pub fn slim(node: &Node, command: Option<&str>) -> Result<SlimToolResult, SlimError> {
    let NodeBody::ToolResult { body, preview, exit_code } = &node.body else {
        return Err(SlimError::NotAToolResult(node.id.clone()));
    };

    let first = preview.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("");
    let line = if first.is_empty() {
        "（无输出）".to_owned()
    } else if first.len() > LINE_LIMIT {
        format!("{}…", &first[..floor_boundary(first, LINE_LIMIT)])
    } else {
        first.to_owned()
    };

    Ok(SlimToolResult {
        node: node.id.clone(),
        command: command.map(str::to_owned),
        line,
        exit_code: *exit_code,
        body: body.clone(),
    })
}

/// 找不超过 `limit` 的最大字符边界，避免按字节截断把多字节字符切开。
fn floor_boundary(text: &str, limit: usize) -> usize {
    let mut end = limit.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    end
}

/// 把瘦身结果渲染成给模型看的一行。
///
/// # 边界与前提
/// 带上节点编号是刻意的：没有它，模型无法表达"我要看这条结果的全文"，只能重跑命令。
///
/// # 副作用
/// 无。
pub fn render(slim: &SlimToolResult) -> String {
    let command = slim.command.as_deref().unwrap_or("（未知命令）");
    format!(
        "$ {command}\n[退出码 {}] {}（完整输出见节点 {}）",
        slim.exit_code, slim.line, slim.node
    )
}

/// 批量瘦身一组节点，非工具结果的节点跳过。
///
/// # 边界与前提
/// 顺序保持不变；跳过的节点不出现在结果里，调用方按节点 id 对齐。这里只吞掉
/// [`SlimError::NotAToolResult`]——它目前是唯一的错误变体，将来若增加别的变体，必须
/// 改成显式匹配，不能让新错误被静默跳过。
///
/// 每条结果的命令由调用方按会话结构配对；本函数不推断。
///
/// # 副作用
/// 无。
pub fn slim_all(nodes: &[Node], commands: &[Option<String>]) -> Vec<SlimToolResult> {
    nodes
        .iter()
        .zip(commands.iter().chain(std::iter::repeat(&None)))
        .filter_map(|(node, command)| slim(node, command.as_deref()).ok())
        .collect()
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

    /// 多行预览只保留第一条非空行，瘦身结果是一行有代表性的文本。
    #[test]
    fn multiline_preview_keeps_first_nonempty_line() {
        let node = result_node(1, "\n第一行\n第二行", 0);
        let slim = slim(&node, Some("ita edit a.rs")).expect("应当成功");
        assert_eq!(slim.line, "第一行");
        assert_eq!(slim.exit_code, 0);
        assert_eq!(slim.command.as_deref(), Some("ita edit a.rs"));
    }

    /// 超长单行被截断并带省略号，瘦身结果不会超出行限。
    #[test]
    fn overlong_single_line_is_truncated() {
        let long = "x".repeat(LINE_LIMIT + 100);
        let node = result_node(1, &long, 0);
        let slim = slim(&node, None).expect("应当成功");
        assert!(slim.line.len() <= LINE_LIMIT + "…".len());
        assert!(slim.line.ends_with('…'));
    }

    /// 截断不会切开多字节字符，字符边界处切断不会产生坏串。
    #[test]
    fn truncation_does_not_split_multibyte_characters() {
        let long = "中".repeat(LINE_LIMIT);
        let node = result_node(1, &long, 0);
        let slim = slim(&node, None).expect("应当成功");
        // 只要能构造出字符串就没有切断字符边界（否则这里会 panic 在切片上）。
        assert!(slim.line.ends_with('…'));
    }

    /// 非零退出码时错误文本仍然保留，失败不会被悄悄丢掉。
    #[test]
    fn error_text_preserved_on_nonzero_exit() {
        let node = result_node(1, "error[E0308]: mismatched types", 101);
        let slim = slim(&node, Some("cargo build")).expect("应当成功");
        assert_eq!(slim.exit_code, 101);
        assert!(slim.line.contains("E0308"));
    }

    /// 空输出用占位而不是留空，模型能分清"无输出"与"缺输出"。
    #[test]
    fn empty_output_uses_placeholder_not_blank() {
        let node = result_node(1, "", 0);
        let slim = slim(&node, None).expect("应当成功");
        assert_eq!(slim.line, "（无输出）");
    }

    /// 只有空白的输出也用占位，空白行不会被当成真实输出。
    #[test]
    fn whitespace_only_output_uses_placeholder_too() {
        let node = result_node(1, "   \n\n", 0);
        assert_eq!(slim(&node, None).expect("应当成功").line, "（无输出）");
    }

    /// 原文句柄被保留以便寻回，瘦身结果不是完整输出的唯一副本。
    #[test]
    fn original_blob_handle_kept_for_retrieval() {
        let node = result_node(1, "ok", 0);
        let slim = slim(&node, None).expect("应当成功");
        let body = slim.body.expect("应当有句柄");
        assert_eq!(body.len, 4096);
    }

    /// 渲染结果带命令、退出码与节点编号，模型能分清哪个节点产出了这份输出。
    #[test]
    fn rendering_includes_command_and_node_id() {
        let node = result_node(7, "ok", 2);
        let slim = slim(&node, Some("ita edit a.rs")).expect("应当成功");
        let text = render(&slim);
        assert!(text.contains("$ ita edit a.rs"));
        assert!(text.contains("退出码 2"));
        assert!(text.contains("完整输出见节点"));
        assert!(text.contains(id(7).as_str()));
    }

    /// 缺命令时明确标注未知，模型不会去猜命令。
    #[test]
    fn missing_command_marked_explicitly_unknown() {
        let node = result_node(7, "ok", 0);
        let slim = slim(&node, None).expect("应当成功");
        assert!(render(&slim).contains("未知命令"));
    }

    /// 非工具结果被拒，对话轮次不会被瘦成工具结果。
    #[test]
    fn non_tool_result_is_rejected() {
        let node = Node {
            id: id(1),
            parent: None,
            session: session(),
            body: NodeBody::Turn {
                role: crate::context::node::TurnRole::User,
                text: "问".to_owned(),
            },
        };
        assert!(matches!(slim(&node, None), Err(SlimError::NotAToolResult(_))));
    }

    /// 批量瘦身跳过非工具节点并保持工具结果的顺序，瘦身列表是忠实子集。
    #[test]
    fn batch_slimming_skips_non_tool_nodes_keeps_order() {
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
        let commands = vec![Some("ita edit".to_owned()), None, Some("cargo test".to_owned())];
        let slimmed = slim_all(&nodes, &commands);
        assert_eq!(slimmed.len(), 2);
        assert_eq!(slimmed[0].node, id(1));
        assert_eq!(slimmed[1].node, id(3));
        assert_eq!(slimmed[1].command.as_deref(), Some("cargo test"));
    }

    /// 无正文的结果也能瘦身，缺正文的工具结果不是错误。
    #[test]
    fn result_without_body_can_still_be_slimmed() {
        let node = Node {
            id: id(1),
            parent: None,
            session: session(),
            body: NodeBody::ToolResult { body: None, preview: "done".to_owned(), exit_code: 0 },
        };
        let slim = slim(&node, None).expect("应当成功");
        assert!(slim.body.is_none());
    }
}
