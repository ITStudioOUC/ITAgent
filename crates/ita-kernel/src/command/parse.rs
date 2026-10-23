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

    #[test]
    fn 命令名与参数按顺序保留() {
        let c = ok("ita edit a.rs --check");
        assert_eq!(c.program, "ita");
        assert_eq!(c.args, vec!["edit", "a.rs", "--check"]);
        assert!(c.redirects.is_empty());
        assert!(c.heredoc.is_none());
    }

    #[test]
    fn 三种重定向都能装配() {
        let c = ok("ita cat < in.txt > out.txt >> log.txt");
        assert_eq!(c.read_paths(), vec!["in.txt"]);
        assert_eq!(c.write_paths(), vec!["log.txt", "out.txt"]);
    }

    #[test]
    fn heredoc_正文被收进命令() {
        let c = ok("ita edit a.rs <<'ITA_EOF'\nfn main() {}\nITA_EOF\n");
        let doc = c.heredoc.expect("应当有正文");
        assert_eq!(doc.delimiter, "ITA_EOF");
        assert_eq!(doc.body, "fn main() {}\n");
        assert_eq!(c.args, vec!["edit", "a.rs"]);
    }

    #[test]
    fn 空输入被拒() {
        assert!(matches!(err(""), ParseError::Empty));
        assert!(matches!(err("   \n"), ParseError::Empty));
    }

    #[test]
    fn 以重定向开头时报缺少命令名() {
        assert!(matches!(err("> out.txt"), ParseError::MissingProgram));
    }

    #[test]
    fn 重定向缺目标被拒() {
        assert!(matches!(err("ita cat >"), ParseError::MissingTarget { .. }));
        assert!(matches!(err("ita cat > > x"), ParseError::MissingTarget { .. }));
    }

    #[test]
    fn 管道被拒() {
        assert!(err("ita a | ita b").hint().contains("版本一"));
    }

    #[test]
    fn 两条命令被拒() {
        assert!(err("ita a; ita b").hint().contains(";"));
    }

    #[test]
    fn 两个_heredoc_被拒() {
        let e = err("ita a <<'X'\nbody\nX\n <<'Y'\nbody\nY");
        assert!(matches!(
            e,
            ParseError::ConflictingHereDoc { .. } | ParseError::TrailingText { .. }
        ));
    }

    #[test]
    fn heredoc_与输入重定向同现被拒() {
        let e = err("ita a < in.txt <<'X'\nbody\nX");
        assert!(matches!(e, ParseError::ConflictingHereDoc { .. }));
    }

    #[test]
    fn heredoc_同行出现多余词被拒() {
        let e = err("ita a <<'X' extra\nbody\nX");
        assert!(matches!(e, ParseError::TrailingText { .. }));
    }

    #[test]
    fn 未终止的_heredoc_被拒() {
        let e = err("ita edit <<'X'\nbody without end");
        assert!(e.hint().contains("没有等到"));
    }

    #[test]
    fn 正文超上限被拒且上限可配置() {
        let input = "ita edit <<'X'\nhello\nX";
        assert!(parse(input, 2).is_err(), "上限 2 字节应当拒掉 6 字节正文");
        assert!(parse(input, 1024).is_ok());
    }

    #[test]
    fn 通配符在装配前就被拒() {
        assert!(err("ita edit *.rs").hint().contains("通配符"));
    }

    #[test]
    fn 解析结果可以还原成可执行命令() {
        let c = ok("ita edit a.rs <<'ITA_EOF'\nx\nITA_EOF\n");
        assert_eq!(c.to_string(), "ita edit a.rs <<'ITA_EOF'\nx\nITA_EOF");
    }

    #[test]
    fn 参数里的等号原样保留() {
        let c = ok("ita run --max=10 --name=a=b");
        assert_eq!(c.args, vec!["run", "--max=10", "--name=a=b"]);
    }

    #[test]
    fn 命令名带引号也可以() {
        let c = ok("'ita' edit a.rs");
        assert_eq!(c.program, "ita");
    }

    #[test]
    fn 重定向目标是空串时被拒() {
        let e = err("ita cat > \"\"");
        assert!(matches!(e, ParseError::MissingTarget { .. }));
    }

    #[test]
    fn 记号上限可配置() {
        let e = parse_with_limits("ita a b c d", 1024, 2).expect_err("应当被拒");
        assert!(matches!(e, ParseError::TooManyTokens { count: 5, limit: 2 }));
    }

    #[test]
    fn 同一行两个_heredoc_报冲突() {
        // 词法只扫第一行，因此第二个 `<<` 必须出现在同一行才能被看见；换行之后的内容
        // 已经属于正文。
        let e = err("ita a <<'X' <<'Y'\nbody\nX");
        assert!(matches!(e, ParseError::ConflictingHereDoc { .. }));
    }

    #[test]
    fn 空参数与空正文都能处理() {
        let c = ok("ita echo \"\" <<'X'\nX");
        assert_eq!(c.args, vec!["echo", ""]);
        assert_eq!(c.heredoc.expect("应当有正文").body, "");
    }
}
