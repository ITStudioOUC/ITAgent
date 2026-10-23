//! 压缩输出的格式契约。
//!
//! 压缩要一次模型调用，而模型输出必须被严格校验：截断、格式错、把说明写进正文，三者都
//! 会让摘要不可用。契约因此带一个显式的结束标记——缺标记即判截断，整次压缩重试，绝不
//! 拿半截摘要去替换上下文。
//!
//! 散文与事实分开：散文由模型写（做了什么、为什么），事实由机器填（改动文件、失败的测试、
//! 报错摘要）。模型只负责散文部分，事实部分不接受它的输入——这是抑制"自我总结时系统性
//! 美化"的主要手段。

/// 散文部分的开始标记。
pub const PROSE_OPEN: &str = "<prose>";
/// 散文部分的结束标记。
pub const PROSE_CLOSE: &str = "</prose>";
/// 整段输出的结束标记。
pub const END: &str = "<end>";
/// 提示版本：改动提示词时递增，压缩记录里带上它以便 replay。
pub const PROMPT_VERSION: u32 = 1;

/// 压缩输出被拒的原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompressError {
    /// 缺少结束标记：输出被截断。
    Truncated,
    /// 缺少散文段，或标记不成对。
    MissingProse,
    /// 散文段之外出现了非空白内容。
    StrayText {
        /// 那段内容的前 40 个字符。
        text: String,
    },
    /// 散文为空。
    EmptyProse,
    /// 散文超过长度上限。
    ProseTooLong {
        /// 实际长度。
        actual: usize,
        /// 上限。
        limit: usize,
    },
}

impl CompressError {
    /// 是否值得重试。
    ///
    /// # 边界与前提
    /// 全部可重试：这些错误都来自模型输出的随机性，重发一次可能就好。与命令文法的语法
    /// 错误不同——那种重试必然得到同样结果。
    ///
    /// # 副作用
    /// 无。
    pub fn is_retryable(&self) -> bool {
        true
    }

    /// 给模型或日志看的一句话。
    pub fn hint(&self) -> String {
        match self {
            Self::Truncated => {
                format!("压缩输出缺少结束标记 `{END}`，可能被截断；请重新生成完整输出。")
            }
            Self::MissingProse => {
                format!("压缩输出缺少 `{PROSE_OPEN}` 与 `{PROSE_CLOSE}` 包裹的散文段。")
            }
            Self::StrayText { text } => {
                format!("散文段之外出现了内容 `{text}`；除散文段与结束标记外不要写任何东西。")
            }
            Self::EmptyProse => "散文段为空，请写出这一段历史做了什么、为什么。".to_owned(),
            Self::ProseTooLong { actual, limit } => {
                format!("散文 {actual} 字节超过上限 {limit} 字节，请压缩到更短。")
            }
        }
    }
}

impl std::fmt::Display for CompressError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.hint())
    }
}

impl std::error::Error for CompressError {}

/// 解析后的压缩输出。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompressOutput {
    /// 模型写的散文。
    pub prose: String,
    /// 生成时使用的提示版本。
    pub prompt_version: u32,
}

/// 解析模型的压缩输出。
///
/// # 步骤
/// 1. 必须以结束标记收尾（允许尾部空白）；否则判截断。
/// 2. 去掉结束标记后，整段必须恰好是一个散文段。
/// 3. 散文段不能为空，且不超过上限。
///
/// # 边界与前提
/// 标记区分大小写，且不允许嵌套。散文里出现 `<prose>` 这类字符串不特殊处理——它是普通
/// 文本，只有最外层的成对标记有意义。
///
/// # 副作用
/// 无。
///
/// # 注意事项
/// 这里只校验形状，不校验内容质量。内容质量靠事实层与"压缩后重复已否决操作"这个离线
/// 指标来兜，不做自动评分。
pub fn parse_output(raw: &str, prose_limit: usize) -> Result<CompressOutput, CompressError> {
    let trimmed = raw.trim_end();
    let Some(body) = trimmed.strip_suffix(END) else {
        return Err(CompressError::Truncated);
    };
    let body = body.trim();

    let Some(open_at) = body.find(PROSE_OPEN) else {
        return Err(CompressError::MissingProse);
    };
    let after_open = &body[open_at + PROSE_OPEN.len()..];
    let Some(close_at) = after_open.find(PROSE_CLOSE) else {
        return Err(CompressError::MissingProse);
    };

    // 散文段之前与之后都不允许有内容：模型倾向于在标记之外补一句解释，而那会被误当成
    // 摘要的一部分或直接丢失。
    if !body[..open_at].trim().is_empty() {
        return Err(CompressError::StrayText {
            text: body[..open_at].trim().chars().take(40).collect(),
        });
    }
    let rest = &after_open[close_at + PROSE_CLOSE.len()..];
    if !rest.trim().is_empty() {
        return Err(CompressError::StrayText { text: rest.trim().chars().take(40).collect() });
    }

    let prose = after_open[..close_at].trim();
    if prose.is_empty() {
        return Err(CompressError::EmptyProse);
    }
    if prose.len() > prose_limit {
        return Err(CompressError::ProseTooLong { actual: prose.len(), limit: prose_limit });
    }

    Ok(CompressOutput { prose: prose.to_owned(), prompt_version: PROMPT_VERSION })
}

/// 渲染给模型的格式说明。
///
/// # 边界与前提
/// 与 [`parse_output`] 是同一份契约的两面：改动任何一面都必须同时改另一面，并递增
/// [`PROMPT_VERSION`]，否则 replay 会拿旧版本的解释去读新格式的输出。
///
/// # 副作用
/// 无。
pub fn format_instructions() -> String {
    format!(
        "把这段历史压缩成摘要，严格按下面的格式输出，不要写任何其它内容：\n\
         {PROSE_OPEN}\n<摘要正文：做了什么、为什么这样选、否决了哪些方案>\n{PROSE_CLOSE}\n\
         {END}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMIT: usize = 4096;

    fn good(prose: &str) -> String {
        format!("{PROSE_OPEN}\n{prose}\n{PROSE_CLOSE}\n{END}")
    }

    #[test]
    fn 合法输出被解析() {
        let out = parse_output(&good("改了 A，否决了 B"), LIMIT).expect("应当成功");
        assert_eq!(out.prose, "改了 A，否决了 B");
        assert_eq!(out.prompt_version, PROMPT_VERSION);
    }

    #[test]
    fn 尾部空白被容忍() {
        let raw = format!("{}\n\n  ", good("x"));
        assert!(parse_output(&raw, LIMIT).is_ok());
    }

    #[test]
    fn 缺少结束标记判截断() {
        let raw = format!("{PROSE_OPEN}\nx\n{PROSE_CLOSE}");
        let err = parse_output(&raw, LIMIT).expect_err("应当被拒");
        assert_eq!(err, CompressError::Truncated);
        assert!(err.is_retryable());
    }

    #[test]
    fn 缺少散文标记被拒() {
        let raw = format!("只有一段话\n{END}");
        assert_eq!(parse_output(&raw, LIMIT), Err(CompressError::MissingProse));
    }

    #[test]
    fn 散文标记不成对被拒() {
        let raw = format!("{PROSE_OPEN}\nx\n{END}");
        assert_eq!(parse_output(&raw, LIMIT), Err(CompressError::MissingProse));
    }

    #[test]
    fn 标记之外的说明被拒() {
        // 开场白写在散文段之前。
        let raw = format!("好的，这是摘要：\n{PROSE_OPEN}\nx\n{PROSE_CLOSE}\n{END}");
        let err = parse_output(&raw, LIMIT).expect_err("应当被拒");
        assert!(matches!(err, CompressError::StrayText { .. }), "实际为 {err:?}");

        // 收尾说明写在结束标记之前。
        let raw = format!("{PROSE_OPEN}\nx\n{PROSE_CLOSE}\n以上。\n{END}");
        assert!(matches!(parse_output(&raw, LIMIT), Err(CompressError::StrayText { .. })));

        // 结束标记之后还有内容时，先判截断——标记不在末尾就不是结束标记。
        let raw = format!("{}\n以上。", good("x"));
        assert_eq!(parse_output(&raw, LIMIT), Err(CompressError::Truncated));
    }

    #[test]
    fn 空散文被拒() {
        assert_eq!(parse_output(&good("   "), LIMIT), Err(CompressError::EmptyProse));
    }

    #[test]
    fn 散文超长被拒并给出两个数() {
        let long = "x".repeat(LIMIT + 1);
        let err = parse_output(&good(&long), LIMIT).expect_err("应当被拒");
        match err {
            CompressError::ProseTooLong { actual, limit } => {
                assert_eq!(actual, LIMIT + 1);
                assert_eq!(limit, LIMIT);
            }
            other => panic!("错误类别不符：{other:?}"),
        }
    }

    #[test]
    fn 散文里出现标记字符串按普通文本处理() {
        let out = parse_output(&good("提到了 <prose> 这个字符串"), LIMIT).expect("应当成功");
        assert!(out.prose.contains("<prose>"));
    }

    #[test]
    fn 所有错误都可重试() {
        for err in [
            CompressError::Truncated,
            CompressError::MissingProse,
            CompressError::EmptyProse,
            CompressError::StrayText { text: "x".to_owned() },
            CompressError::ProseTooLong { actual: 1, limit: 0 },
        ] {
            assert!(err.is_retryable(), "{err:?} 应当可重试");
        }
    }

    #[test]
    fn 格式说明与解析器一致() {
        // 按说明生成的输出必须能被解析：两者是同一份契约的两面。
        let instructions = format_instructions();
        assert!(instructions.contains(PROSE_OPEN));
        assert!(instructions.contains(PROSE_CLOSE));
        assert!(instructions.contains(END));
        assert!(parse_output(&good("按说明写的摘要"), LIMIT).is_ok());
    }
}
