//! 命令解析的错误类型。
//!
//! 错误要能被模型读懂并自行纠正，因此每个变体都携带位置与原文片段，诊断里给出可操作
//! 的建议。变体在第一批就冻结，后续阶段只填空，避免拿着旧枚举到处补 match。

use std::fmt;

/// 解析失败的原因。
///
/// 每个变体都对应一种模型可以自行修正的书写错误，或者是文法明确不支持的构造。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// 输入为空或只有空白。
    Empty,
    /// 出现了文法不支持的算符，例如 `;`、`&&`、`&`、`$(`、反引号。
    UnsupportedOperator {
        /// 出现位置（字节偏移）。
        offset: usize,
        /// 原文片段，用于回显。
        text: String,
        /// 该构造不支持的原因。
        reason: &'static str,
    },
    /// 未加引号时出现了通配符。
    GlobNotAllowed {
        /// 出现位置（字节偏移）。
        offset: usize,
        /// 触发的字符。
        ch: char,
    },
    /// 引号没有闭合。
    UnterminatedQuote {
        /// 引号开始的位置（字节偏移）。
        offset: usize,
        /// 引号字符。
        quote: char,
    },
    /// 缺少命令名（例如以重定向开头）。
    MissingProgram,
    /// 重定向或 heredoc 缺少目标。
    MissingTarget {
        /// 算符出现的位置（字节偏移）。
        offset: usize,
        /// 算符文本，例如 `>` 或 `<<`。
        op: String,
    },
    /// heredoc 的分隔符不符合规则。
    InvalidDelimiter {
        /// 位置（字节偏移）。
        offset: usize,
        /// 模型写的分隔符。
        delimiter: String,
        /// 不符合的规则说明。
        reason: &'static str,
    },
    /// heredoc 的正文没有等到声明过的终止行。
    UnterminatedHereDoc {
        /// 分隔符，便于模型确认自己写的是哪一个。
        delimiter: String,
    },
    /// 终止行之后还有无法解析的残留文本。
    TrailingText {
        /// 残留开始的位置（字节偏移）。
        offset: usize,
        /// 残留片段。
        text: String,
    },
    /// 正文超过上限。
    BodyTooLarge {
        /// 当前正文的字节数。
        actual: usize,
        /// 允许的上限。
        limit: usize,
    },
    /// 同一处出现了互相冲突的 heredoc 组合，例如两个 `<<`，或 `<` 与 `<<` 同时出现。
    ConflictingHereDoc {
        /// 位置（字节偏移）。
        offset: usize,
    },
    /// 记号数量超过上限，通常是参数被误写成了大量碎片。
    TooManyTokens {
        /// 实际数量。
        count: usize,
        /// 允许的上限。
        limit: usize,
    },
}

impl ParseError {
    /// 这条错误是否值得调用方重试。
    ///
    /// # 边界与前提
    /// 语法错误一律不可重试：同样的输入总是得到同样的结果，重试只会重复失败。模型需要
    /// 改写命令，而不是重发。
    ///
    /// # 副作用
    /// 无。
    pub fn is_retryable(&self) -> bool {
        false
    }

    /// 错误的字节偏移，用于在诊断里定位。
    pub fn offset(&self) -> Option<usize> {
        match self {
            Self::Empty | Self::MissingProgram | Self::UnterminatedHereDoc { .. } => None,
            Self::UnsupportedOperator { offset, .. }
            | Self::GlobNotAllowed { offset, .. }
            | Self::UnterminatedQuote { offset, .. }
            | Self::MissingTarget { offset, .. }
            | Self::InvalidDelimiter { offset, .. }
            | Self::TrailingText { offset, .. }
            | Self::ConflictingHereDoc { offset } => Some(*offset),
            Self::BodyTooLarge { .. } | Self::TooManyTokens { .. } => None,
        }
    }

    /// 给模型看的一句话建议。
    ///
    /// # 边界与前提
    /// 建议必须具体到"改成什么"，不能只说"不支持"。模型据此重写命令的代价应当是一次
    /// 往返。
    ///
    /// # 副作用
    /// 无。
    pub fn hint(&self) -> String {
        match self {
            Self::Empty => "命令不能为空，请给出要执行的工具名与参数。".to_owned(),
            Self::UnsupportedOperator { text, reason, .. } => {
                let advice = match text.as_str() {
                    "<<<" => {
                        "这里指 herestring，它把字符串本身喂给标准输入；要提供正文请用 `<<` 加分隔符。"
                    }
                    "<<-" => "`<<-` 会剥掉正文的前导制表符，行为难以预测；请改用 `<<`。",
                    _ => "请拆成多次调用。",
                };
                format!("不支持 `{text}`：{reason}。{advice}")
            }
            Self::GlobNotAllowed { ch, .. } => format!(
                "不支持通配符 `{ch}`：写入目标必须在执行前已知，请显式列出路径或使用工具自己的过滤参数。"
            ),
            Self::UnterminatedQuote { quote, .. } => {
                format!("引号 `{quote}` 没有闭合，请补上配对的引号。")
            }
            Self::MissingProgram => "缺少命令名：请在重定向之前写出要执行的工具。".to_owned(),
            Self::MissingTarget { op, .. } => format!("`{op}` 后面需要跟一个路径。"),
            Self::InvalidDelimiter { delimiter, reason, .. } => format!(
                "heredoc 分隔符 `{delimiter}` 不合规则（{reason}）。建议使用形如 `ITA_EOF` 的大写前缀加随机后缀。"
            ),
            Self::UnterminatedHereDoc { delimiter } => format!(
                "heredoc 正文没有等到独占一行的 `{delimiter}`。正文可能被截断，或分隔符在正文里提前出现。"
            ),
            Self::TrailingText { .. } => {
                "分隔符之后还有无法解析的文本。请把命令写完再结束，不要在正文之后追加说明。"
                    .to_owned()
            }
            Self::BodyTooLarge { actual, limit } => format!(
                "正文 {actual} 字节超过上限 {limit} 字节，请分段写入或用打补丁代替整文件覆盖。"
            ),
            Self::ConflictingHereDoc { .. } => {
                "一次调用只支持一个 heredoc，且不能与 `<` 同时使用。".to_owned()
            }
            Self::TooManyTokens { count, limit } => {
                format!("记号数量 {count} 超过上限 {limit}，请把命令拆成多次调用。")
            }
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.hint())
    }
}

impl std::error::Error for ParseError {}

#[cfg(test)]
mod tests {
    use super::*;

    /// 文法错误属于输入错误，重试同一条命令只会得到同样的拒绝，因此一律标记为不可重试。
    #[test]
    fn syntax_errors_are_never_retryable() {
        assert!(!ParseError::Empty.is_retryable());
        assert!(!ParseError::MissingProgram.is_retryable());
    }

    /// 整体性错误没有具体位置，偏移必须是 `None`；有位置的错误必须给出字节偏移。
    #[test]
    fn offset_is_reported_only_when_position_is_known() {
        assert_eq!(ParseError::Empty.offset(), None);
        assert_eq!(ParseError::GlobNotAllowed { offset: 7, ch: '*' }.offset(), Some(7));
    }

    /// 诊断里回显模型实际写过的片段，模型才能认出被拒的是哪一段。
    #[test]
    fn hint_quotes_the_text_the_model_wrote() {
        let err = ParseError::UnsupportedOperator {
            offset: 3,
            text: "$(".to_owned(),
            reason: "命令替换会绕过权限预检",
        };
        let hint = err.hint();
        assert!(hint.contains("$("));
        assert!(hint.contains("绕过权限预检"));
    }

    /// 超限诊断同时给出实际字节数与上限，模型才知道该缩到多少。
    #[test]
    fn body_too_large_reports_actual_and_limit() {
        let hint = ParseError::BodyTooLarge { actual: 2_000_000, limit: 1_048_576 }.hint();
        assert!(hint.contains("2000000"));
        assert!(hint.contains("1048576"));
    }
}
