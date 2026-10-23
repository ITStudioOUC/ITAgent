//! 命令的形状。
//!
//! 这个模块只描述"一条命令长什么样"，不做词法分析、不做权限判定、不执行任何东西。
//! 把它与解析器分开，是因为执行器要消费它来做权限预检与租约获取，而那份代码不该
//! 依赖词法分析的内部结构。

use std::fmt;

/// 重定向的方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedirectKind {
    /// `>`，截断写入。
    Write,
    /// `>>`，追加写入。
    Append,
    /// `<`，从文件读取标准输入。
    Read,
}

/// 一条重定向。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redirect {
    /// 方向。
    pub kind: RedirectKind,
    /// 目标路径。
    ///
    /// 解析器负责去掉多余的斜杠与末尾斜杠，但**不做**符号链接解析、大小写归一或与
    /// 工作区根的拼接——那需要文件系统，而内核不碰文件系统。同一文件的不同写法在这里
    /// 仍是不同的字符串，归一是资源键层（租约）的职责。
    pub target: String,
}

impl Redirect {
    /// 这条重定向是否写入目标文件。
    ///
    /// # 边界与前提
    /// `<` 不是写入，因此不参与写路径的提取。追加与截断都算写入。
    ///
    /// # 副作用
    /// 无。
    pub fn is_write(&self) -> bool {
        matches!(self.kind, RedirectKind::Write | RedirectKind::Append)
    }
}

/// 通过标准输入提供的正文。
///
/// 正文由解析器在此前收齐；这里只承载结果，不关心它是怎么被扫描出来的。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HereDoc {
    /// 模型声明的分隔符，保留原样以便诊断信息回显。
    pub delimiter: String,
    /// 正文内容，按字节原样保留，不转换换行。
    pub body: String,
}

/// 一条被解析出来的命令。
///
/// 版本一只支持单命令：没有管道、没有逻辑连接、没有命令替换。因此这个结构描述的是
/// 一次工具调用，而不是一段 shell 脚本。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// 命令名，即 argv 的第一项。
    pub program: String,
    /// 命令名之后的参数，保持模型书写的顺序。
    pub args: Vec<String>,
    /// 重定向，保持出现顺序。
    pub redirects: Vec<Redirect>,
    /// 标准输入正文，最多一个。
    pub heredoc: Option<HereDoc>,
}

impl Command {
    /// 提取这次调用会读取的文件路径，已排序去重。
    ///
    /// # 边界与前提
    /// 只取自 `<` 重定向。 argv 里出现的路径不推断——哪些参数是路径只有工具自己知道，
    /// 内核去猜会与工具声明的写入范围冲突。
    ///
    /// # 副作用
    /// 无。
    pub fn read_paths(&self) -> Vec<&str> {
        self.collect_paths(|r| r.kind == RedirectKind::Read)
    }

    /// 提取这次调用会写入的文件路径，已排序去重。
    ///
    /// # 边界与前提
    /// 只取自 `>` 与 `>>` 重定向，理由同 `read_paths`。排序是租约原子获取的要求：
    /// 固定顺序能避免两个调用以相反顺序互等。
    ///
    /// # 副作用
    /// 无。
    pub fn write_paths(&self) -> Vec<&str> {
        self.collect_paths(Redirect::is_write)
    }

    fn collect_paths(&self, keep: impl Fn(&Redirect) -> bool) -> Vec<&str> {
        let mut out: Vec<&str> =
            self.redirects.iter().filter(|r| keep(r)).map(|r| r.target.as_str()).collect();
        out.sort_unstable();
        out.dedup();
        out
    }
}

impl fmt::Display for Command {
    /// 还原成可以直接粘贴到终端里的一行命令。
    ///
    /// # 边界与前提
    /// 参数按需加单引号；含单引号的参数用双引号包裹。heredoc 连正文一起输出，因此结果
    /// 粘到真实终端里可以直接执行，不会停在等待输入的状态。
    /// 这个输出用于日志与复现，不保证与原输入逐字节相同（多余的引号会被省略）。
    ///
    /// # 副作用
    /// 无。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", shell_word(&self.program))?;
        for arg in &self.args {
            write!(f, " {}", shell_word(arg))?;
        }
        for redirect in &self.redirects {
            let op = match redirect.kind {
                RedirectKind::Write => ">",
                RedirectKind::Append => ">>",
                RedirectKind::Read => "<",
            };
            write!(f, " {op} {}", shell_word(&redirect.target))?;
        }
        if let Some(doc) = &self.heredoc {
            // 正文必须一起输出：只写 `<<'X'` 的命令粘到真实终端里会等待输入而挂住，
            // 而 0007 承诺命令是可以直接粘贴执行的。
            write!(f, " <<'{}'\n{}", doc.delimiter, doc.body)?;
            if !doc.body.ends_with('\n') {
                writeln!(f)?;
            }
            write!(f, "{}", doc.delimiter)?;
        }
        Ok(())
    }
}

/// 按需给一个词加引号，使输出可以被真实 shell 直接执行。
fn shell_word(word: &str) -> String {
    let plain = !word.is_empty()
        && word.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, '_' | '-' | '.' | '/' | ':' | '=' | '@' | '+' | ',')
        });
    if plain {
        return word.to_owned();
    }
    if word.contains('\'') {
        let escaped =
            word.replace('\\', "\\\\").replace('"', "\\\"").replace('$', "\\$").replace('`', "\\`");
        format!("\"{escaped}\"")
    } else {
        format!("'{word}'")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(args: &[&str], redirects: Vec<Redirect>) -> Command {
        Command {
            program: "ita".to_owned(),
            args: args.iter().map(|s| (*s).to_owned()).collect(),
            redirects,
            heredoc: None,
        }
    }

    fn redirect(kind: RedirectKind, target: &str) -> Redirect {
        Redirect { kind, target: target.to_owned() }
    }

    #[test]
    fn 读路径只来自输入重定向() {
        let c = cmd(
            &["edit", "a.txt"],
            vec![redirect(RedirectKind::Read, "in.txt"), redirect(RedirectKind::Write, "out.txt")],
        );
        assert_eq!(c.read_paths(), vec!["in.txt"]);
    }

    #[test]
    fn 写路径含截断与追加但不含读取() {
        let c = cmd(
            &[],
            vec![
                redirect(RedirectKind::Write, "b.txt"),
                redirect(RedirectKind::Append, "a.txt"),
                redirect(RedirectKind::Read, "c.txt"),
            ],
        );
        assert_eq!(c.write_paths(), vec!["a.txt", "b.txt"]);
    }

    #[test]
    fn 重复路径只保留一份且顺序稳定() {
        let c = cmd(
            &[],
            vec![
                redirect(RedirectKind::Write, "z.txt"),
                redirect(RedirectKind::Append, "a.txt"),
                redirect(RedirectKind::Write, "z.txt"),
            ],
        );
        assert_eq!(c.write_paths(), vec!["a.txt", "z.txt"]);
    }

    #[test]
    fn 无重定向时两组路径都为空() {
        let c = cmd(&["status"], vec![]);
        assert!(c.read_paths().is_empty());
        assert!(c.write_paths().is_empty());
    }

    #[test]
    fn 输出可直接粘贴执行() {
        let c = cmd(&["edit", "a b.txt"], vec![redirect(RedirectKind::Write, "out.txt")]);
        assert_eq!(c.to_string(), "ita edit 'a b.txt' > out.txt");
    }

    #[test]
    fn 含单引号的参数改用双引号() {
        let c = cmd(&["it's"], vec![]);
        assert_eq!(c.to_string(), "ita \"it's\"");
    }

    #[test]
    fn 带正文的命令粘贴后是完整命令() {
        let mut c = cmd(&["edit", "a.rs"], vec![]);
        c.heredoc =
            Some(HereDoc { delimiter: "ITA_EOF".to_owned(), body: "fn main() {}\n".to_owned() });
        assert_eq!(c.to_string(), "ita edit a.rs <<'ITA_EOF'\nfn main() {}\nITA_EOF");
    }

    #[test]
    fn 正文缺少结尾换行时补一个() {
        let mut c = cmd(&["edit"], vec![]);
        c.heredoc =
            Some(HereDoc { delimiter: "ITA_EOF".to_owned(), body: "no newline".to_owned() });
        assert_eq!(c.to_string(), "ita edit <<'ITA_EOF'\nno newline\nITA_EOF");
    }

    #[test]
    fn 追加写与读取可以同时存在() {
        let c = cmd(
            &[],
            vec![redirect(RedirectKind::Append, "log.txt"), redirect(RedirectKind::Read, "in.txt")],
        );
        assert_eq!(c.read_paths(), vec!["in.txt"]);
        assert_eq!(c.write_paths(), vec!["log.txt"]);
    }
}
