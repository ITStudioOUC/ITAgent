//! 词法扫描。
//!
//! 输入是一整条命令行，输出是一串记号。这一层只认字面与算符，不理解命令结构：管道与
//! 重定向的合法性留给语法装配，heredoc 的正文由后续阶段接管。
//!
//! 拒绝在这里而不是在装配阶段发生，是为了让错误位置尽可能靠近原文，且保证"任何被拒的
//! 构造都不会被装配阶段看到一半"。

use super::error::ParseError;

/// 一个词法记号的种类。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    /// 普通词，已去掉引号并处理转义。
    Word(String),
    /// `>`，截断写入。
    Write,
    /// `>>`，追加写入。
    Append,
    /// `<`，输入重定向。
    Read,
    /// `<<`，heredoc 的开始，其后必须紧跟分隔符。
    HereDocStart,
}

/// 带位置信息的记号。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// 种类。
    pub kind: TokenKind,
    /// 在原文中的起始字节偏移。
    pub offset: usize,
}

/// 扫描一条命令，产出记号序列。
///
/// # 步骤
/// 1. 跳过空白，遇到行尾结束。
/// 2. 识别算符：`>>` 优先于 `>`，`<<` 优先于 `<`。
/// 3. 识别词：双引号内保留字面、单引号内完全保留、反斜杠转义下一个字符。
/// 4. 在未加引号的位置拒绝文法不支持的构造。
///
/// # 边界与前提
/// 输入必须是单行（不含换行）。含换行的输入由调用方切成命令行与正文之后再分别传入；
/// 这里遇到换行即视为命令结束。空输入返回空序列，由装配阶段判为空命令错误。
///
/// # 副作用
/// 无。
///
/// # 注意事项
/// 反引号与 `$(` 在未加引号与双引号内都拒绝，因为真实 shell 在这两处会执行替换；单引号内
/// 不执行替换，因此按字面保留，不做特殊处理。
///
/// 双引号内只解释 bash 也会解释的四个转义（`\"` `\\` `\$` `` \` ``），其余反斜杠组合
/// 按字面保留两个字符。这样同一条命令在 ita 里与在真实 shell 里写出的字节一致。
pub fn scan(input: &str) -> Result<Vec<Token>, ParseError> {
    let bytes = input.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b' ' | b'\t' => i += 1,
            b'\n' | b'\r' => break,
            b'>' => {
                if bytes.get(i + 1) == Some(&b'>') {
                    tokens.push(Token { kind: TokenKind::Append, offset: i });
                    i += 2;
                } else {
                    tokens.push(Token { kind: TokenKind::Write, offset: i });
                    i += 1;
                }
            }
            b'<' => {
                if bytes.get(i + 1) == Some(&b'<') {
                    if bytes.get(i + 2) == Some(&b'<') {
                        return Err(ParseError::UnsupportedOperator {
                            offset: i,
                            text: "<<<".to_owned(),
                            reason: "herestring 在真实 shell 里把字符串本身喂给标准输入，与 heredoc 语义不同",
                        });
                    }
                    if bytes.get(i + 2) == Some(&b'-') {
                        return Err(ParseError::UnsupportedOperator {
                            offset: i,
                            text: "<<-".to_owned(),
                            reason: "剥前导制表符会让正文与真实写入的字节不一致",
                        });
                    }
                    tokens.push(Token { kind: TokenKind::HereDocStart, offset: i });
                    i += 2;
                } else {
                    tokens.push(Token { kind: TokenKind::Read, offset: i });
                    i += 1;
                }
            }
            b';' => {
                return Err(ParseError::UnsupportedOperator {
                    offset: i,
                    text: ";".to_owned(),
                    reason: "多条命令会让写入目标集合在执行前不可知",
                });
            }
            b'&' => {
                let text = if bytes.get(i + 1) == Some(&b'&') { "&&" } else { "&" };
                return Err(ParseError::UnsupportedOperator {
                    offset: i,
                    text: text.to_owned(),
                    reason: "逻辑连接与后台执行会让一次调用变成多条命令",
                });
            }
            b'`' => {
                return Err(ParseError::UnsupportedOperator {
                    offset: i,
                    text: "`".to_owned(),
                    reason: "命令替换会绕过权限预检",
                });
            }
            b'$' if bytes.get(i + 1) == Some(&b'(') => {
                return Err(ParseError::UnsupportedOperator {
                    offset: i,
                    text: "$(".to_owned(),
                    reason: "命令替换会绕过权限预检",
                });
            }
            b'|' => {
                return Err(ParseError::UnsupportedOperator {
                    offset: i,
                    text: "|".to_owned(),
                    reason: "版本一只支持单命令，管道会让一次调用变成多个进程",
                });
            }
            b'*' | b'?' => {
                return Err(ParseError::GlobNotAllowed { offset: i, ch: b as char });
            }
            _ => {
                let (word, next) = scan_word(input, i)?;
                tokens.push(Token { kind: TokenKind::Word(word), offset: i });
                i = next;
            }
        }
    }

    Ok(tokens)
}

/// 从 `start` 起扫描一个词，返回解码后的文本与下一个位置。
///
/// # 边界与前提
/// 调用方保证 `start` 指向非空白、非算符的字节。词在空白、行尾或算符处结束。
///
/// # 副作用
/// 无。
fn scan_word(input: &str, start: usize) -> Result<(String, usize), ParseError> {
    let bytes = input.as_bytes();
    let mut out = String::new();
    let mut i = start;

    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b' ' | b'\t' | b'\n' => break,
            // 回车只在 CRLF 处结束；词中间的回车按普通字符保留，与真实 shell 一致。
            b'\r' if bytes.get(i + 1) == Some(&b'\n') => break,
            // 算符在词中间同样终止这个词：`a>b` 是两个记号加一个词。
            b'>' | b'<' | b'|' | b';' | b'&' | b'`' => break,
            b'*' | b'?' | b'[' => {
                return Err(ParseError::GlobNotAllowed { offset: i, ch: b as char });
            }
            b'{' | b'}' => {
                return Err(ParseError::UnsupportedOperator {
                    offset: i,
                    text: (b as char).to_string(),
                    reason: "大括号展开会让目标路径在执行前不可知",
                });
            }
            b'~' if i == start => {
                return Err(ParseError::UnsupportedOperator {
                    offset: i,
                    text: "~".to_owned(),
                    reason: "波浪线展开会让目标路径在执行前不可知",
                });
            }
            b'$' => {
                return Err(ParseError::UnsupportedOperator {
                    offset: i,
                    text: if bytes.get(i + 1) == Some(&b'(') {
                        "$(".to_owned()
                    } else {
                        "$".to_owned()
                    },
                    reason: "变量与命令替换都会让目标路径在执行前不可知",
                });
            }
            b'\'' => {
                let quote_start = i;
                i += 1;
                let body_start = i;
                while i < bytes.len() && bytes[i] != b'\'' {
                    i += 1;
                }
                if i >= bytes.len() {
                    return Err(ParseError::UnterminatedQuote { offset: quote_start, quote: '\'' });
                }
                out.push_str(&input[body_start..i]);
                i += 1;
            }
            b'"' => {
                let quote_start = i;
                i += 1;
                loop {
                    if i >= bytes.len() {
                        return Err(ParseError::UnterminatedQuote {
                            offset: quote_start,
                            quote: '"',
                        });
                    }
                    match bytes[i] {
                        b'"' => {
                            i += 1;
                            break;
                        }
                        b'`' => {
                            return Err(ParseError::UnsupportedOperator {
                                offset: i,
                                text: "`".to_owned(),
                                reason: "命令替换会绕过权限预检",
                            });
                        }
                        b'$' if bytes.get(i + 1) == Some(&b'(') => {
                            return Err(ParseError::UnsupportedOperator {
                                offset: i,
                                text: "$(".to_owned(),
                                reason: "命令替换会绕过权限预检",
                            });
                        }
                        b'$' => {
                            return Err(ParseError::UnsupportedOperator {
                                offset: i,
                                text: "$".to_owned(),
                                reason: "变量展开会让目标路径在执行前不可知",
                            });
                        }
                        b'\\' if i + 1 < bytes.len() => {
                            // 只解释 bash 在双引号里也会解释的四个字符；其余保持字面两个字符，
                            // 否则同一条命令在 ita 与真实 shell 里会写出不同的字节。
                            let escaped = input[i + 1..].chars().next().expect("范围已检查");
                            if matches!(escaped, '"' | '\\' | '$' | '`') {
                                out.push(escaped);
                                i += 1 + escaped.len_utf8();
                            } else {
                                out.push('\\');
                                i += 1;
                            }
                        }
                        _ => {
                            let ch = input[i..].chars().next().expect("范围已检查");
                            out.push(ch);
                            i += ch.len_utf8();
                        }
                    }
                }
            }
            b'\\' if i + 1 < bytes.len() => {
                let escaped = input[i + 1..].chars().next().expect("范围已检查");
                out.push(escaped);
                i += 1 + escaped.len_utf8();
            }
            _ => {
                let ch = input[i..].chars().next().expect("范围已检查");
                out.push(ch);
                i += ch.len_utf8();
            }
        }
    }

    Ok((out, i))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(input: &str) -> Vec<TokenKind> {
        scan(input).expect("应当解析成功").into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn 普通命令切成命令名与参数() {
        assert_eq!(
            kinds("ita edit a.rs"),
            vec![
                TokenKind::Word("ita".to_owned()),
                TokenKind::Word("edit".to_owned()),
                TokenKind::Word("a.rs".to_owned()),
            ]
        );
    }

    #[test]
    fn 双引号内保留空格() {
        assert_eq!(
            kinds("ita edit \"a b.rs\""),
            vec![
                TokenKind::Word("ita".to_owned()),
                TokenKind::Word("edit".to_owned()),
                TokenKind::Word("a b.rs".to_owned()),
            ]
        );
    }

    #[test]
    fn 单引号内不解释转义() {
        assert_eq!(
            kinds("ita echo 'a\\nb'"),
            vec![
                TokenKind::Word("ita".to_owned()),
                TokenKind::Word("echo".to_owned()),
                TokenKind::Word("a\\nb".to_owned()),
            ]
        );
    }

    #[test]
    fn 双引号内只解释_bash_也解释的四个转义() {
        // `\n` 在 bash 的双引号里是字面两个字符，这里必须保持一致，否则同一条命令在两边
        // 会写出不同的字节，"可粘贴复现"就不成立。
        assert_eq!(
            kinds("ita echo \"a\\nb\""),
            vec![
                TokenKind::Word("ita".to_owned()),
                TokenKind::Word("echo".to_owned()),
                TokenKind::Word("a\\nb".to_owned()),
            ]
        );
        // 引号与反斜杠本身仍然被转义。
        assert_eq!(
            kinds("ita echo \"a\\\"b\\\"c\""),
            vec![
                TokenKind::Word("ita".to_owned()),
                TokenKind::Word("echo".to_owned()),
                TokenKind::Word("a\"b\"c".to_owned()),
            ]
        );
    }

    #[test]
    fn 变量与括号展开被拒() {
        for input in ["ita edit $HOME/a", "ita edit {a,b}.rs", "ita edit ~/a"] {
            assert!(scan(input).is_err(), "{input} 应当被拒");
        }
    }

    #[test]
    fn 方括号按通配符拒绝() {
        let err = scan("ita edit [ab].rs").expect_err("应当被拒");
        assert!(matches!(err, ParseError::GlobNotAllowed { .. }));
    }

    #[test]
    fn 空参数保留为零长词() {
        assert_eq!(
            kinds("ita echo \"\""),
            vec![
                TokenKind::Word("ita".to_owned()),
                TokenKind::Word("echo".to_owned()),
                TokenKind::Word(String::new()),
            ]
        );
    }

    #[test]
    fn utf8_参数按字符边界切分() {
        assert_eq!(
            kinds("ita echo 中文"),
            vec![
                TokenKind::Word("ita".to_owned()),
                TokenKind::Word("echo".to_owned()),
                TokenKind::Word("中文".to_owned()),
            ]
        );
    }

    #[test]
    fn 词中间的回车按普通字符保留() {
        assert_eq!(
            kinds("ita echo a\rb"),
            vec![
                TokenKind::Word("ita".to_owned()),
                TokenKind::Word("echo".to_owned()),
                TokenKind::Word("a\rb".to_owned()),
            ]
        );
    }

    #[test]
    fn 重定向算符与词分开() {
        assert_eq!(
            kinds("ita edit a.rs >> out.txt"),
            vec![
                TokenKind::Word("ita".to_owned()),
                TokenKind::Word("edit".to_owned()),
                TokenKind::Word("a.rs".to_owned()),
                TokenKind::Append,
                TokenKind::Word("out.txt".to_owned()),
            ]
        );
    }

    #[test]
    fn 相邻的算符无需空格() {
        assert_eq!(
            kinds("ita edit a.rs>out.txt"),
            vec![
                TokenKind::Word("ita".to_owned()),
                TokenKind::Word("edit".to_owned()),
                TokenKind::Word("a.rs".to_owned()),
                TokenKind::Write,
                TokenKind::Word("out.txt".to_owned()),
            ]
        );
    }

    #[test]
    fn 双小于号单独成为记号() {
        assert_eq!(
            kinds("ita edit <<'X'"),
            vec![
                TokenKind::Word("ita".to_owned()),
                TokenKind::Word("edit".to_owned()),
                TokenKind::HereDocStart,
                TokenKind::Word("X".to_owned()),
            ]
        );
    }

    #[test]
    fn 命令替换被拒() {
        for input in ["ita $(rm -rf /)", "ita `rm`", "ita echo \"$(rm)\""] {
            let err = scan(input).expect_err("应当被拒");
            assert!(!err.is_retryable());
        }
    }

    #[test]
    fn 逻辑连接与后台被拒() {
        for input in ["ita a && ita b", "ita a; ita b", "ita a &"] {
            assert!(scan(input).is_err(), "{input} 应当被拒");
        }
    }

    #[test]
    fn 管道被拒并说明是版本一限制() {
        let err = scan("ita a | ita b").expect_err("应当被拒");
        assert!(err.hint().contains("版本一"), "提示应当说明这是版本限制");
    }

    #[test]
    fn 未加引号的通配符被拒() {
        for input in ["ita edit *.rs", "ita edit a?.rs"] {
            let err = scan(input).expect_err("应当被拒");
            assert!(matches!(err, ParseError::GlobNotAllowed { .. }));
        }
    }

    #[test]
    fn 引号内的通配符是字面值() {
        assert_eq!(
            kinds("ita edit \"*.rs\""),
            vec![
                TokenKind::Word("ita".to_owned()),
                TokenKind::Word("edit".to_owned()),
                TokenKind::Word("*.rs".to_owned()),
            ]
        );
    }

    #[test]
    fn herestring_与剥制表符的_heredoc_被拒() {
        let err = scan("ita edit <<<EOF").expect_err("应当被拒");
        assert!(err.hint().contains("<<"));
        let err = scan("ita edit <<-EOF").expect_err("应当被拒");
        assert!(err.hint().contains("<<-") || err.hint().contains("制表符"));
    }

    #[test]
    fn 未闭合的引号给出位置() {
        let err = scan("ita edit 'unclosed").expect_err("应当被拒");
        assert_eq!(err.offset(), Some(9));
    }

    #[test]
    fn 换行结束扫描() {
        assert_eq!(
            kinds("ita edit\nextra"),
            vec![TokenKind::Word("ita".to_owned()), TokenKind::Word("edit".to_owned()),]
        );
    }

    #[test]
    fn 空输入得到空序列() {
        assert!(kinds("   ").is_empty());
    }
}
