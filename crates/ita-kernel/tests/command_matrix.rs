//! 失败矩阵：所有拒绝路径的集中验证。
//!
//! 这个测试文件刻意不复用模块内部的单元测试，而是在公共入口 [`ita_kernel::command::parse`]
//! 上跑表驱动用例。原因是单元测试可以各自绕过某一段，而契约是"从一条命令到拒绝"的整条
//! 路径；只有从入口进，才能保证某条规则没有被上游的另一个错误抢先吃掉。
//!
//! 每行用例都对应 docs/adr/0007 里的一条规则，改规则时这张表必须先改。

use ita_kernel::command::{DEFAULT_MAX_BODY, ParseError, parse};

/// 一条失败用例：输入、期望的错误类别、以及诊断里必须出现的关键词。
struct Case {
    /// 人类可读的用例名。
    name: &'static str,
    /// 输入的命令行。
    input: &'static str,
    /// 期望的错误类别（用于分支断言）。
    matches: fn(&ParseError) -> bool,
    /// 诊断里必须出现的关键词，保证提示对模型可操作。
    hint: &'static str,
}

fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "空命令",
            input: "   ",
            matches: |e| matches!(e, ParseError::Empty),
            hint: "不能为空",
        },
        Case {
            name: "缺少命令名",
            input: "> out.txt",
            matches: |e| matches!(e, ParseError::MissingProgram),
            hint: "缺少命令名",
        },
        Case {
            name: "命令替换",
            input: "ita $(rm -rf /)",
            matches: |e| matches!(e, ParseError::UnsupportedOperator { .. }),
            hint: "$(",
        },
        Case {
            name: "反引号替换",
            input: "ita `rm`",
            matches: |e| matches!(e, ParseError::UnsupportedOperator { .. }),
            hint: "`",
        },
        Case {
            name: "逻辑与",
            input: "ita a && ita b",
            matches: |e| matches!(e, ParseError::UnsupportedOperator { .. }),
            hint: "&&",
        },
        Case {
            name: "分号",
            input: "ita a; ita b",
            matches: |e| matches!(e, ParseError::UnsupportedOperator { .. }),
            hint: ";",
        },
        Case {
            name: "后台执行",
            input: "ita a &",
            matches: |e| matches!(e, ParseError::UnsupportedOperator { .. }),
            hint: "&",
        },
        Case {
            name: "管道",
            input: "ita a | ita b",
            matches: |e| matches!(e, ParseError::UnsupportedOperator { .. }),
            hint: "版本一",
        },
        Case {
            name: "通配符星号",
            input: "ita edit *.rs",
            matches: |e| matches!(e, ParseError::GlobNotAllowed { .. }),
            hint: "通配符",
        },
        Case {
            name: "通配符问号",
            input: "ita edit a?.rs",
            matches: |e| matches!(e, ParseError::GlobNotAllowed { .. }),
            hint: "通配符",
        },
        Case {
            name: "方括号",
            input: "ita edit [ab].rs",
            matches: |e| matches!(e, ParseError::GlobNotAllowed { .. }),
            hint: "通配符",
        },
        Case {
            name: "大括号展开",
            input: "ita edit {a,b}.rs",
            matches: |e| matches!(e, ParseError::UnsupportedOperator { .. }),
            hint: "大括号",
        },
        Case {
            name: "波浪线展开",
            input: "ita edit ~/a.rs",
            matches: |e| matches!(e, ParseError::UnsupportedOperator { .. }),
            hint: "波浪线",
        },
        Case {
            name: "变量展开",
            input: "ita edit $HOME/a.rs",
            matches: |e| matches!(e, ParseError::UnsupportedOperator { .. }),
            hint: "变量",
        },
        Case {
            name: "herestring",
            input: "ita edit <<<EOF",
            matches: |e| matches!(e, ParseError::UnsupportedOperator { .. }),
            hint: "<<",
        },
        Case {
            name: "剥制表符的 heredoc",
            input: "ita edit <<-EOF",
            matches: |e| matches!(e, ParseError::UnsupportedOperator { .. }),
            hint: "<<-",
        },
        Case {
            name: "未闭合的单引号",
            input: "ita edit 'unclosed",
            matches: |e| matches!(e, ParseError::UnterminatedQuote { .. }),
            hint: "没有闭合",
        },
        Case {
            name: "未闭合的双引号",
            input: "ita edit \"unclosed",
            matches: |e| matches!(e, ParseError::UnterminatedQuote { .. }),
            hint: "没有闭合",
        },
        Case {
            name: "重定向缺目标",
            input: "ita cat >",
            matches: |e| matches!(e, ParseError::MissingTarget { .. }),
            hint: "需要跟一个路径",
        },
        Case {
            name: "重定向目标为空串",
            input: "ita cat > \"\"",
            matches: |e| matches!(e, ParseError::MissingTarget { .. }),
            hint: "需要跟一个路径",
        },
        Case {
            name: "heredoc 未终止",
            input: "ita edit <<'X'\nbody without terminator",
            matches: |e| matches!(e, ParseError::UnterminatedHereDoc { .. }),
            hint: "没有等到",
        },
        Case {
            name: "heredoc 终止行带尾随空格",
            input: "ita edit <<'X'\nbody\nX \n",
            matches: |e| matches!(e, ParseError::UnterminatedHereDoc { .. }),
            hint: "没有等到",
        },
        Case {
            name: "heredoc 分隔符大小写不符",
            input: "ita edit <<'X'\nbody\nx\n",
            matches: |e| matches!(e, ParseError::UnterminatedHereDoc { .. }),
            hint: "没有等到",
        },
        Case {
            name: "heredoc 正文撞上分隔符",
            input: "ita edit <<'X'\na\nX\nb\n",
            matches: |e| matches!(e, ParseError::TrailingText { .. }),
            hint: "无法解析的文本",
        },
        Case {
            name: "heredoc 分隔符之后有残留",
            input: "ita edit <<'X'\nbody\nX\ntrailing",
            matches: |e| matches!(e, ParseError::TrailingText { .. }),
            hint: "无法解析的文本",
        },
        Case {
            name: "heredoc 同行多余词",
            input: "ita edit <<'X' extra\nbody\nX",
            matches: |e| matches!(e, ParseError::TrailingText { .. }),
            hint: "无法解析的文本",
        },
        Case {
            name: "非法分隔符字符",
            input: "ita edit <<'a.b'\nbody\na.b",
            matches: |e| matches!(e, ParseError::InvalidDelimiter { .. }),
            hint: "分隔符",
        },
        Case {
            name: "分隔符以连字符开头",
            input: "ita edit <<'-X'\nbody\n-X",
            matches: |e| matches!(e, ParseError::InvalidDelimiter { .. }),
            hint: "连字符",
        },
        Case {
            name: "未引号正文含美元符号",
            input: "ita edit <<X\ncost $5\nX",
            matches: |e| matches!(e, ParseError::InvalidDelimiter { .. }),
            hint: "<<'",
        },
        Case {
            name: "未引号正文含反斜杠",
            input: "ita edit <<X\na\\nb\nX",
            matches: |e| matches!(e, ParseError::InvalidDelimiter { .. }),
            hint: "<<'",
        },
        Case {
            name: "两个 heredoc",
            input: "ita edit <<'X' <<'Y'\nbody\nX",
            matches: |e| matches!(e, ParseError::ConflictingHereDoc { .. }),
            hint: "只支持一个",
        },
        Case {
            name: "heredoc 与输入重定向同现",
            input: "ita edit < in.txt <<'X'\nbody\nX",
            matches: |e| matches!(e, ParseError::ConflictingHereDoc { .. }),
            hint: "只支持一个",
        },
    ]
}

#[test]
fn 失败矩阵逐条命中() {
    let mut failures = Vec::new();

    for case in cases() {
        match parse(case.input, DEFAULT_MAX_BODY) {
            Ok(cmd) => failures.push(format!("{}：应当被拒，却解析出 `{cmd}`", case.name)),
            Err(err) => {
                if !(case.matches)(&err) {
                    failures.push(format!("{}：错误类别不符，实际 {err:?}", case.name));
                }
                if !err.hint().contains(case.hint) {
                    failures.push(format!(
                        "{}：诊断里缺少关键词 `{}`，实际为 `{}`",
                        case.name,
                        case.hint,
                        err.hint()
                    ));
                }
                if err.is_retryable() {
                    failures.push(format!("{}：语法错误不应标记为可重试", case.name));
                }
            }
        }
    }

    assert!(failures.is_empty(), "失败矩阵有 {} 条不符：\n{}", failures.len(), failures.join("\n"));
}

#[test]
fn 每条拒绝都带可定位的偏移或明确说明() {
    for case in cases() {
        let Err(err) = parse(case.input, DEFAULT_MAX_BODY) else {
            continue;
        };
        // 无位置信息的错误只允许是"整体性"的几种，其余必须给偏移。
        let whole_input = matches!(
            err,
            ParseError::Empty
                | ParseError::MissingProgram
                | ParseError::UnterminatedHereDoc { .. }
                | ParseError::BodyTooLarge { .. }
                | ParseError::TooManyTokens { .. }
        );
        assert!(
            whole_input || err.offset().is_some(),
            "{}：错误 `{err:?}` 既无偏移也不属于整体性错误",
            case.name
        );
    }
}

#[test]
fn 正文超上限在公共入口上被拒() {
    let big = "x".repeat(DEFAULT_MAX_BODY + 1);
    let input = format!("ita edit <<'X'\n{big}\nX");
    let err = parse(&input, DEFAULT_MAX_BODY).expect_err("应当被拒");
    assert!(matches!(err, ParseError::BodyTooLarge { .. }));
    assert!(err.hint().contains("上限"));
}

#[test]
fn 合法命令不被误拒() {
    // 与失败矩阵互为对照：这些必须全部通过，否则规则收得太紧。
    let legal = [
        "ita edit a.rs",
        "ita edit a.rs > out.txt",
        "ita edit a.rs >> out.txt",
        "ita cat < in.txt",
        "ita edit \"a b.rs\"",
        "ita edit 'a b.rs'",
        "ita echo \"\"",
        "ita echo a=b",
        "ita edit <<'ITA_EOF'\nfn main() {}\nITA_EOF\n",
        "ita edit <<'ITA_EOF'\ncost $5 and `cmd`\nITA_EOF\n",
        "ita edit \"*.rs\"",
        "ita edit <<'X'\nX",
    ];
    for input in legal {
        assert!(parse(input, DEFAULT_MAX_BODY).is_ok(), "合法命令被拒：{input}");
    }
}
