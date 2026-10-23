//! heredoc 的正文扫描。
//!
//! heredoc 的语义是"把一段正文作为标准输入交给命令"。它属于词法层而不是某个编辑工具的
//! 附属能力，因为需要它的工具不止一个，而"怎么判断正文结束"只应该有一份实现。
//!
//! 三条硬规则决定了这里的全部结构：
//!
//! - 终止行必须独占一行、与分隔符逐字节相等、大小写敏感、行尾不允许空格。
//! - 正文按字节原样保留，不转换换行，也不解释任何转义。
//! - 分隔符缺失即视为被截断，调用方必须整条拒绝，不能写出一部分。

use super::ast::HereDoc;
use super::error::ParseError;

/// heredoc 解析的输入上限。
///
/// 这是默认值，不是不变量；调用方可以从配置里传别的值进来。代码文件远低于此，超过
/// 这个量级应当改成分段打补丁。
pub const DEFAULT_MAX_BODY: usize = 1024 * 1024;

/// 分隔符的长度上限。
///
/// 太长的分隔符没有实际收益，却会让诊断信息变长；太短则容易在正文里意外撞上。
pub const MAX_DELIMITER_LEN: usize = 64;

/// 从命令行尾部切出 heredoc。
///
/// # 步骤
/// 1. 校验分隔符：非空、不超过长度上限、只含字母数字与下划线连字符、首字符不是连字符。
/// 2. 定位正文的起始：命令行之后必须紧跟换行。
/// 3. 逐行扫描，找到与分隔符逐字节相等的一行。
/// 4. 校验正文长度，并检查终止行之后没有残留文本。
///
/// # 边界与前提
/// `rest` 是命令行在 `<<` 之后的全部内容，第一行是分隔符所在的行。返回的 `HereDoc`
/// 只含分隔符与正文；终止行之后的残留文本会让整条命令被拒，因为那通常意味着模型在
/// 正文之后又写了说明，或者内层 heredoc 撞上了外层分隔符。
///
/// # 副作用
/// 无。
///
/// # 注意事项
/// 正文里出现同名的行就会提前终止——这是 heredoc 的固有语义，不是缺陷。因此工具描述里
/// 要要求模型选一个罕见的分隔符。
pub fn scan_heredoc(rest: &str, max_body: usize) -> Result<HereDoc, ParseError> {
    // after_offset 是"分隔符之后那段"在 rest 里的起点，用于把正文里的位置换算成
    // rest 内的绝对偏移，保证所有错误的 offset 都相对同一个基准。
    let (delimiter, quoted, after_delimiter, after_offset) = split_delimiter(rest)?;

    let body_start = match after_delimiter.find('\n') {
        Some(idx) => idx + 1,
        // 分隔符之后没有换行：说明模型只写了 `<<'X'` 就结束了，正文不存在。
        None => {
            return Err(ParseError::UnterminatedHereDoc { delimiter });
        }
    };

    let body_and_rest = &after_delimiter[body_start..];
    let mut cursor = 0;
    let mut body_end = None;

    while cursor <= body_and_rest.len() {
        let line_end =
            body_and_rest[cursor..].find('\n').map_or(body_and_rest.len(), |idx| cursor + idx);
        // 终止行比较时不带行尾换行，但允许行尾是 CRLF 里的 CR——真实 shell 也会把回车
        // 当作正文的一部分，因此含 CR 的行不算匹配。
        let line = &body_and_rest[cursor..line_end];

        if line == delimiter {
            body_end = Some((cursor, line_end));
            break;
        }
        if line_end == body_and_rest.len() {
            break;
        }
        cursor = line_end + 1;
    }

    let Some((end, terminator_end)) = body_end else {
        return Err(ParseError::UnterminatedHereDoc { delimiter });
    };

    let body = &body_and_rest[..end];
    if body.len() > max_body {
        return Err(ParseError::BodyTooLarge { actual: body.len(), limit: max_body });
    }

    // 终止行之后只允许换行与空白。出现别的字符通常意味着分隔符在正文里提前撞上，
    // 或者模型在命令之后追加了说明——两种都应当整条拒绝。
    let trailing_start = terminator_end;
    let trailing = &body_and_rest[trailing_start..];
    if let Some(offset) = trailing.find(|c: char| !c.is_whitespace()) {
        return Err(ParseError::TrailingText {
            offset: after_offset + body_start + trailing_start + offset,
            text: trailing[offset..].chars().take(40).collect(),
        });
    }

    // 未加引号的分隔符在真实 shell 里会展开正文；这里不支持展开，因此只在正文不含
    // 展开字符时接受。含 `$` 或反引号的正文必须改用引号形式。
    if !quoted && body.contains(['$', '`', '\\']) {
        return Err(ParseError::InvalidDelimiter {
            offset: 0,
            delimiter,
            reason: "未加引号的分隔符在真实 shell 里会展开正文（变量、命令替换、反斜杠转义），而正文含这些字符；请改用 <<'分隔符' 的写法",
        });
    }

    Ok(HereDoc { delimiter, body: body.to_owned() })
}

/// 切出分隔符，返回 (分隔符, 是否被引号包裹, 分隔符之后的文本, 该文本在 rest 内的起点)。
///
/// # 边界与前提
/// `rest` 以分隔符开头，前后允许有空白。分隔符可以被单引号或双引号包裹，两种在真实
/// shell 里都表示"不展开"。
///
/// # 副作用
/// 无。
fn split_delimiter(rest: &str) -> Result<(String, bool, &str, usize), ParseError> {
    let trimmed = rest.trim_start_matches([' ', '\t']);
    let base_offset = rest.len() - trimmed.len();

    let (raw, quoted, consumed) = match trimmed.chars().next() {
        Some(q @ ('\'' | '"')) => {
            let after_open = &trimmed[q.len_utf8()..];
            let Some(close) = after_open.find(q) else {
                return Err(ParseError::UnterminatedQuote { offset: base_offset, quote: q });
            };
            (&after_open[..close], true, q.len_utf8() + close + q.len_utf8())
        }
        _ => {
            let end = trimmed.find(|c: char| c.is_whitespace()).unwrap_or(trimmed.len());
            (&trimmed[..end], false, end)
        }
    };

    validate_delimiter(raw, base_offset)?;
    Ok((raw.to_owned(), quoted, &trimmed[consumed..], base_offset + consumed))
}

/// 校验分隔符的形状。
///
/// # 边界与前提
/// 只允许字母、数字、下划线与连字符；首字符不能是连字符，以免与 `<<-` 混淆；长度上限
/// 由 [`MAX_DELIMITER_LEN`] 决定。
///
/// # 副作用
/// 无。
fn validate_delimiter(raw: &str, offset: usize) -> Result<(), ParseError> {
    if raw.is_empty() {
        return Err(ParseError::InvalidDelimiter {
            offset,
            delimiter: String::new(),
            reason: "分隔符不能为空",
        });
    }
    if raw.len() > MAX_DELIMITER_LEN {
        return Err(ParseError::InvalidDelimiter {
            offset,
            delimiter: raw.to_owned(),
            reason: "分隔符超过长度上限",
        });
    }
    if raw.starts_with('-') {
        return Err(ParseError::InvalidDelimiter {
            offset,
            delimiter: raw.to_owned(),
            reason: "分隔符不能以连字符开头，以免与 <<- 混淆",
        });
    }
    if !raw.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err(ParseError::InvalidDelimiter {
            offset,
            delimiter: raw.to_owned(),
            reason: "分隔符只能含字母、数字、下划线与连字符",
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(input: &str) -> HereDoc {
        scan_heredoc(input, DEFAULT_MAX_BODY).expect("应当解析成功")
    }

    #[test]
    fn 基本正文与终止行() {
        let doc = ok("'ITA_EOF'\nfn main() {}\nITA_EOF\n");
        assert_eq!(doc.delimiter, "ITA_EOF");
        assert_eq!(doc.body, "fn main() {}\n");
    }

    #[test]
    fn 双引号包裹与单引号等价() {
        let doc = ok("\"ITA_EOF\"\nbody\nITA_EOF");
        assert_eq!(doc.body, "body\n");
    }

    #[test]
    fn 未加引号且正文无展开字符时接受() {
        let doc = ok("ITA_EOF\nplain body\nITA_EOF");
        assert_eq!(doc.body, "plain body\n");
    }

    #[test]
    fn 未加引号且正文含美元符号时拒绝() {
        let err =
            scan_heredoc("ITA_EOF\ncost $5\nITA_EOF", DEFAULT_MAX_BODY).expect_err("应当被拒");
        assert!(err.hint().contains("<<'"));
    }

    #[test]
    fn 未终止时报出分隔符() {
        let err = scan_heredoc("'ITA_EOF'\nbody without terminator\n", DEFAULT_MAX_BODY)
            .expect_err("应当被拒");
        assert!(
            matches!(err, ParseError::UnterminatedHereDoc { ref delimiter } if delimiter == "ITA_EOF")
        );
    }

    #[test]
    fn 分隔符之后有残留文本时拒绝() {
        let err = scan_heredoc("'ITA_EOF'\nbody\nITA_EOF\ntrailing", DEFAULT_MAX_BODY)
            .expect_err("应当被拒");
        assert!(matches!(err, ParseError::TrailingText { .. }));
    }

    #[test]
    fn 分隔符之后只有空白时接受() {
        let doc = ok("'ITA_EOF'\nbody\nITA_EOF\n\n  \n");
        assert_eq!(doc.body, "body\n");
    }

    #[test]
    fn 正文超上限时拒绝并给出两个数() {
        let big = "x".repeat(DEFAULT_MAX_BODY + 1);
        let input = format!("'X'\n{big}\nX");
        let err = scan_heredoc(&input, DEFAULT_MAX_BODY).expect_err("应当被拒");
        assert!(matches!(err, ParseError::BodyTooLarge { .. }));
    }

    #[test]
    fn 空正文可以接受() {
        let doc = ok("'X'\nX");
        assert_eq!(doc.body, "");
    }

    #[test]
    fn 非法分隔符被拒() {
        for input in ["'a b'\nbody\na b", "''\nbody\n", "'a.b'\nbody\na.b", "'-x'\nbody\n-x"] {
            assert!(scan_heredoc(input, DEFAULT_MAX_BODY).is_err(), "{input} 应当被拒");
        }
    }

    #[test]
    fn 超长分隔符被拒() {
        let long = "A".repeat(MAX_DELIMITER_LEN + 1);
        let input = format!("'{long}'\nbody\n{long}");
        assert!(scan_heredoc(&input, DEFAULT_MAX_BODY).is_err());
    }

    #[test]
    fn 终止行带尾随空格时不匹配() {
        let err = scan_heredoc("'X'\nbody\nX \n", DEFAULT_MAX_BODY).expect_err("应当被拒");
        assert!(matches!(err, ParseError::UnterminatedHereDoc { .. }));
    }

    #[test]
    fn 终止行带回车时不匹配() {
        let err = scan_heredoc("'X'\nbody\nX\r\n", DEFAULT_MAX_BODY).expect_err("应当被拒");
        assert!(matches!(err, ParseError::UnterminatedHereDoc { .. }));
    }

    #[test]
    fn 正文保留回车不转换换行() {
        // 正文里的回车原样保留；终止行本身必须是纯 `X`，带回车不算匹配（与真实 shell 一致）。
        let doc = ok("'X'\nbody\r\nX\n");
        assert_eq!(doc.body, "body\r\n");
    }

    #[test]
    fn 大小写敏感() {
        let err = scan_heredoc("'X'\nbody\nx\n", DEFAULT_MAX_BODY).expect_err("应当被拒");
        assert!(matches!(err, ParseError::UnterminatedHereDoc { .. }));
    }

    #[test]
    fn 正文里撞上分隔符会被残留检测拦下() {
        // heredoc 的固有语义是"遇到同名行就结束"，因此正文里撞上分隔符时，后面的内容会
        // 变成残留文本。这条正是 0007 要求的检测手段：整体拒绝，而不是静默截断。
        let err = scan_heredoc("'X'\na\nX\nb\n", DEFAULT_MAX_BODY).expect_err("应当被拒");
        assert!(matches!(err, ParseError::TrailingText { .. }));
    }

    #[test]
    fn 分隔符后无换行时报未终止() {
        let err = scan_heredoc("'X'", DEFAULT_MAX_BODY).expect_err("应当被拒");
        assert!(matches!(err, ParseError::UnterminatedHereDoc { .. }));
    }

    #[test]
    fn 残留文本的位置相对整段输入() {
        // `'X'` 3 字节、`\n` 1、`body\n` 5、`X` 1、`\n` 1，残留的 `t` 在第 11 字节。
        // 所有错误的 offset 都以 rest 为基准，调用方加上 `<<` 的位置即得整条命令里的偏移。
        let err = scan_heredoc("'X'\nbody\nX\ntrailing", DEFAULT_MAX_BODY).expect_err("应当被拒");
        assert_eq!(err.offset(), Some(11));

        // 残留前多一个空行时偏移随之加一。
        let err = scan_heredoc("'X'\nbody\nX\n\ntrailing", DEFAULT_MAX_BODY).expect_err("应当被拒");
        assert_eq!(err.offset(), Some(12));
    }

    #[test]
    fn 未加引号且正文含反斜杠时拒绝() {
        let err = scan_heredoc("X\na\\nb\nX", DEFAULT_MAX_BODY).expect_err("应当被拒");
        assert!(matches!(err, ParseError::InvalidDelimiter { .. }));
    }

    #[test]
    fn 未闭合的引号报引号位置() {
        let err = scan_heredoc("'X\nbody\nX", DEFAULT_MAX_BODY).expect_err("应当被拒");
        assert!(matches!(err, ParseError::UnterminatedQuote { .. }));
    }
}
