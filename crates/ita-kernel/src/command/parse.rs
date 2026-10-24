//! 语法装配：把记号序列变成一条 [`Command`]。
//!
//! 词法层已经把不该出现的构造拒掉了，这一层只负责结构：谁是命令名、哪些是参数、重定向
//! 指向哪里、heredoc 的正文从哪开始。因此这里的错误种类很少，全部是结构性的。

use super::ast::{Command, Redirect, RedirectKind};
use super::error::ParseError;
use super::heredoc::{DEFAULT_MAX_BODY, scan_heredoc};
use super::lex::{TokenKind, scan};

/// 解析一条命令。
///
/// # 步骤
/// 1. 词法扫描，得到记号序列。
/// 2. 取第一个词作为命令名；第一个记号不是词即为缺少命令名。
/// 3. 逐项归约：普通词进参数，三种重定向读取目标路径，`<<` 交给 heredoc 扫描器。
/// 4. 校验结构约束：至多一个 heredoc、不能与 `<` 同现、重定向必须有目标。
///
/// # 边界与前提
/// 输入是完整的一条命令行，允许尾部带换行与正文（heredoc）。输入为空返回
/// [`ParseError::Empty`]。函数不读任何外部状态，`max_body` 由调用方从配置传入。
///
/// # 副作用
/// 无。
///
/// # 注意事项
/// heredoc 的正文从 `<<` 之后的第一个换行开始，因此同一行里 `<<` 之后只能出现分隔符。
/// 出现别的词一律按语法错误处理——否则"正文从哪里开始"会有歧义。
pub fn parse(input: &str, max_body: usize) -> Result<Command, ParseError> {
    parse_with_limits(input, max_body, MAX_TOKENS)
}

/// 与 [`parse`] 相同，但记号上限也由调用方给出。
///
/// # 边界与前提
/// 上限在词法扫描之后检查，因此它限制的是装配阶段的工作量，不是输入字节数；输入规模由
/// 调用方在更外层限制。
///
/// # 副作用
/// 无。
pub fn parse_with_limits(
    input: &str,
    max_body: usize,
    max_tokens: usize,
) -> Result<Command, ParseError> {
    if input.trim().is_empty() {
        return Err(ParseError::Empty);
    }

    let tokens = scan(input)?;
    if tokens.is_empty() {
        return Err(ParseError::Empty);
    }
    if tokens.len() > max_tokens {
        return Err(ParseError::TooManyTokens { count: tokens.len(), limit: max_tokens });
    }

    // 结构约束在装配之前检查：`<<` 一旦被处理就会吃掉余下输入，装配过程中看不到
    // 第二个 heredoc，只能提前数。
    let heredoc_count = tokens.iter().filter(|t| t.kind == TokenKind::HereDocStart).count();
    let has_read_redirect = tokens.iter().any(|t| t.kind == TokenKind::Read);
    if heredoc_count > 1 || (heredoc_count == 1 && has_read_redirect) {
        let offset = tokens
            .iter()
            .filter(|t| t.kind == TokenKind::HereDocStart)
            .nth(1)
            .or_else(|| tokens.iter().find(|t| t.kind == TokenKind::Read))
            .map_or(0, |t| t.offset);
        return Err(ParseError::ConflictingHereDoc { offset });
    }

    let mut program: Option<String> = None;
    let mut args = Vec::new();
    let mut redirects = Vec::new();
    let mut heredoc = None;

    let mut iter = tokens.into_iter().peekable();
    while let Some(token) = iter.next() {
        match token.kind {
            TokenKind::Word(word) => match program {
                None => program = Some(word),
                Some(_) => {
                    // `<<` 之后同一行不允许再出现词：正文的起点必须唯一。
                    if heredoc.is_some() {
                        return Err(ParseError::TrailingText { offset: token.offset, text: word });
                    }
                    args.push(word);
                }
            },
            TokenKind::Write | TokenKind::Append | TokenKind::Read => {
                let kind = match token.kind {
                    TokenKind::Write => RedirectKind::Write,
                    TokenKind::Append => RedirectKind::Append,
                    _ => RedirectKind::Read,
                };
                let Some(target) = iter.next() else {
                    return Err(ParseError::MissingTarget {
                        offset: token.offset,
                        op: op_text(kind).to_owned(),
                    });
                };
                let TokenKind::Word(path) = target.kind else {
                    return Err(ParseError::MissingTarget {
                        offset: token.offset,
                        op: op_text(kind).to_owned(),
                    });
                };
                if path.is_empty() {
                    return Err(ParseError::MissingTarget {
                        offset: target.offset,
                        op: op_text(kind).to_owned(),
                    });
                }
                redirects.push(Redirect { kind, target: path });
            }
            TokenKind::HereDocStart => {
                // 正文从 `<<` 之后的换行开始；把这一段的绝对起点交给扫描器。
                let rest = &input[token.offset + 2..];
                heredoc = Some(scan_heredoc(rest, max_body)?);
                // 正文已经吃掉余下全部输入，不再有后续记号。
                break;
            }
        }
    }

    let Some(program) = program else {
        return Err(ParseError::MissingProgram);
    };

    Ok(Command { program, args, redirects, heredoc })
}

fn op_text(kind: RedirectKind) -> &'static str {
    match kind {
        RedirectKind::Write => ">",
        RedirectKind::Append => ">>",
        RedirectKind::Read => "<",
    }
}

/// 用默认上限解析。
///
/// # 边界与前提
/// 供测试与不关心上限的调用方使用；生产路径应当从配置取值后调用 [`parse`]。
///
/// # 副作用
/// 无。
pub fn parse_with_defaults(input: &str) -> Result<Command, ParseError> {
    parse(input, DEFAULT_MAX_BODY)
}

/// 解析器一次能接受的记号数量上限。
///
/// 这不是安全边界（词法层已经限制了输入规模），而是给"参数特别多"这类误用一个明确的
/// 错误，而不是让下游拿到一个巨大的 AST。
pub const MAX_TOKENS: usize = 4096;

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(input: &str) -> Command {
        parse_with_defaults(input).expect("应当解析成功")
    }

    fn err(input: &str) -> ParseError {
        parse_with_defaults(input).expect_err("应当被拒")
    }

    /// 第一个词是命令名，其余按原顺序成为参数，且不产生重定向与正文。
    #[test]
    fn program_and_args_keep_their_order() {
        let c = ok("ita edit a.rs --check");
        assert_eq!(c.program, "ita");
        assert_eq!(c.args, vec!["edit", "a.rs", "--check"]);
        assert!(c.redirects.is_empty());
        assert!(c.heredoc.is_none());
    }

    /// 输入、截断、追加三类重定向都能装进命令，并分别落进读集与写集。
    #[test]
    fn all_three_redirect_kinds_are_assembled() {
        let c = ok("ita cat < in.txt > out.txt >> log.txt");
        assert_eq!(c.read_paths(), vec!["in.txt"]);
        assert_eq!(c.write_paths(), vec!["log.txt", "out.txt"]);
    }

    /// heredoc 的正文与分隔符被收进命令本身，不再作为参数出现。
    #[test]
    fn heredoc_body_is_captured_into_command() {
        let c = ok("ita edit a.rs <<'ITA_EOF'\nfn main() {}\nITA_EOF\n");
        let doc = c.heredoc.expect("应当有正文");
        assert_eq!(doc.delimiter, "ITA_EOF");
        assert_eq!(doc.body, "fn main() {}\n");
        assert_eq!(c.args, vec!["edit", "a.rs"]);
    }

    /// 空串与纯空白都判为空命令，不进入文法分析。
    #[test]
    fn empty_input_is_rejected() {
        assert!(matches!(err(""), ParseError::Empty));
        assert!(matches!(err("   \n"), ParseError::Empty));
    }

    /// 第一个记号是算符时报缺少命令名，而不是把它当成普通参数吞下。
    #[test]
    fn command_starting_with_redirect_reports_missing_program() {
        assert!(matches!(err("> out.txt"), ParseError::MissingProgram));
    }

    /// 算符后面没有目标、或紧跟另一个算符，都报缺少重定向目标。
    #[test]
    fn redirect_without_target_is_rejected() {
        assert!(matches!(err("ita cat >"), ParseError::MissingTarget { .. }));
        assert!(matches!(err("ita cat > > x"), ParseError::MissingTarget { .. }));
    }

    /// 装配层同样拒绝管道，提示沿用词法层的版本一说明。
    #[test]
    fn pipe_is_rejected() {
        assert!(err("ita a | ita b").hint().contains("版本一"));
    }

    /// 分号连接的第二条命令不在文法内，提示要点明分号不是可用的分隔符。
    #[test]
    fn two_commands_on_one_line_are_rejected() {
        assert!(err("ita a; ita b").hint().contains(";"));
    }

    /// 一次调用只允许一个 heredoc，出现第二个即整条拒绝，而不是取后者。
    #[test]
    fn second_heredoc_is_rejected() {
        let e = err("ita a <<'X'\nbody\nX\n <<'Y'\nbody\nY");
        assert!(matches!(
            e,
            ParseError::ConflictingHereDoc { .. } | ParseError::TrailingText { .. }
        ));
    }

    /// heredoc 本身就是标准输入，再给输入重定向会让输入来源不唯一，因此拒绝。
    #[test]
    fn heredoc_with_input_redirect_is_rejected() {
        let e = err("ita a < in.txt <<'X'\nbody\nX");
        assert!(matches!(e, ParseError::ConflictingHereDoc { .. }));
    }

    /// 分隔符所在行只允许空白，同行出现别的词会让正文从哪里开始产生歧义。
    #[test]
    fn extra_word_on_heredoc_line_is_rejected() {
        let e = err("ita a <<'X' extra\nbody\nX");
        assert!(matches!(e, ParseError::TrailingText { .. }));
    }

    /// 正文没等到分隔符即视为被截断，整条命令拒绝，不允许写出一部分。
    #[test]
    fn unterminated_heredoc_is_rejected() {
        let e = err("ita edit <<'X'\nbody without end");
        assert!(e.hint().contains("没有等到"));
    }

    /// 上限是参数而不是常量：收紧到 2 字节时拒绝 6 字节正文，放宽到 1024 字节后同一输入通过。
    #[test]
    fn body_over_limit_is_rejected_and_limit_is_configurable() {
        let input = "ita edit <<'X'\nhello\nX";
        assert!(parse(input, 2).is_err(), "上限 2 字节应当拒掉 6 字节正文");
        assert!(parse(input, 1024).is_ok());
    }

    /// 通配符在词法阶段就被拦下，装配层不会拿到未展开的路径。
    #[test]
    fn glob_is_rejected_before_assembly() {
        assert!(err("ita edit *.rs").hint().contains("通配符"));
    }

    /// 解析再渲染要还原成可执行命令，带正文时也要完整。
    #[test]
    fn parsed_command_renders_back_to_executable_text() {
        let c = ok("ita edit a.rs <<'ITA_EOF'\nx\nITA_EOF\n");
        assert_eq!(c.to_string(), "ita edit a.rs <<'ITA_EOF'\nx\nITA_EOF");
    }

    /// 参数里的等号属于内容，不能被当作键值分隔符切开。
    #[test]
    fn equals_sign_inside_argument_is_preserved() {
        let c = ok("ita run --max=10 --name=a=b");
        assert_eq!(c.args, vec!["run", "--max=10", "--name=a=b"]);
    }

    /// 命令名可以带引号，解析后取引号内的值作为命令名。
    #[test]
    fn quoted_program_name_is_accepted() {
        let c = ok("'ita' edit a.rs");
        assert_eq!(c.program, "ita");
    }

    /// 空串不是有效路径，与缺少目标同样报重定向目标缺失。
    #[test]
    fn empty_redirect_target_is_rejected() {
        let e = err("ita cat > \"\"");
        assert!(matches!(e, ParseError::MissingTarget { .. }));
    }

    /// 记号上限由参数给定，超出时报出实际数量与上限。
    #[test]
    fn token_limit_is_configurable() {
        let e = parse_with_limits("ita a b c d", 1024, 2).expect_err("应当被拒");
        assert!(matches!(e, ParseError::TooManyTokens { count: 5, limit: 2 }));
    }

    /// 守的是"重复的正文来源必须在词法层就暴露"：只扫第一行会让写在同一行的第二个起始符无处可藏，而不是被静默忽略。
    #[test]
    fn two_heredocs_on_one_line_conflict() {
        // 词法只扫第一行，因此第二个 `<<` 必须出现在同一行才能被看见；换行之后的内容
        // 已经属于正文。
        let e = err("ita a <<'X' <<'Y'\nbody\nX");
        assert!(matches!(e, ParseError::ConflictingHereDoc { .. }));
    }

    /// 空参数与空正文都是合法形态，不能因为长度为 0 被丢掉或判错。
    #[test]
    fn empty_argument_and_empty_body_are_both_accepted() {
        let c = ok("ita echo \"\" <<'X'\nX");
        assert_eq!(c.args, vec!["echo", ""]);
        assert_eq!(c.heredoc.expect("应当有正文").body, "");
    }
}
