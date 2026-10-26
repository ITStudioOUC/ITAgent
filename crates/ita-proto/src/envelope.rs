//! Invocation 信封与编解码。
//!
//! 信封是 server 与 worker 之间 IPC 消息的封装：字段怎么排、怎么编码、怎么解码。本模块
//! 不建立连接、不做路由、不持有会话状态。
//!
//! 信封里没有 workspace 字段，工作区由 server 从连接身份推导，不是请求参数。这条由接口
//! 形状保证：[`Invocation`] 没有这个字段，[`Handler`] 的入参类型里也没有这个参数，
//! handler 拿不到 workspace。
//!
//! 编解码是长度前缀文本：每个字段先写大端 4 字节长度前缀再写内容，解码按长度切回。与帧
//! 正文的长度前缀文本同型，不引入 serde。

use std::fmt;

use ita_kernel::command::{Command, HereDoc, Redirect, RedirectKind};
use ita_kernel::context::{LeaseId, SessionId};

use crate::version::ProtocolVersion;

/// 调用者标识名义类型。
///
/// 调用者是固定命名子系统，不是动态建行实体，所以标识是校验过的标签，不是 ULID。合法
/// 标签是小写字母数字加连字符，非空，首尾不连字符，不连续连字符。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Caller(String);

/// 调用者标签无法被解析的原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallerError {
    /// 标签为空。
    Empty,
    /// 标签首尾是连字符。
    LeadingOrTrailingHyphen {
        /// 被拒的标签。
        label: String,
    },
    /// 标签含连续连字符。
    ConsecutiveHyphens {
        /// 被拒的标签。
        label: String,
    },
    /// 标签含小写字母数字与连字符之外的字符。
    InvalidChar {
        /// 那个字符的字节偏移。
        offset: usize,
        /// 那个字符。
        ch: char,
    },
}

impl fmt::Display for CallerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "调用者标签不能为空"),
            Self::LeadingOrTrailingHyphen { label } => {
                write!(f, "调用者标签 `{label}` 首尾不能是连字符")
            }
            Self::ConsecutiveHyphens { label } => {
                write!(f, "调用者标签 `{label}` 含连续连字符")
            }
            Self::InvalidChar { offset, ch } => {
                write!(f, "调用者标签第 {offset} 个字符 `{ch}` 不是小写字母数字或连字符")
            }
        }
    }
}

impl std::error::Error for CallerError {}

impl Caller {
    /// 从标签构造，校验字符集与形状。
    ///
    /// # 边界与前提
    /// - 空串被拒。
    /// - 首尾连字符被拒。
    /// - 连续连字符被拒。
    /// - 大写、非 ASCII 与其它字符被拒，不自动转换。
    ///
    /// # 副作用
    /// 无。
    pub fn new(label: impl Into<String>) -> Result<Self, CallerError> {
        let label = label.into();
        if label.is_empty() {
            return Err(CallerError::Empty);
        }
        let chars: Vec<char> = label.chars().collect();
        if chars.first() == Some(&'-') || chars.last() == Some(&'-') {
            return Err(CallerError::LeadingOrTrailingHyphen { label });
        }
        for (offset, &ch) in chars.iter().enumerate() {
            let legal = ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-';
            if !legal {
                return Err(CallerError::InvalidChar { offset, ch });
            }
        }
        for window in chars.windows(2) {
            if window[0] == '-' && window[1] == '-' {
                return Err(CallerError::ConsecutiveHyphens { label });
            }
        }
        Ok(Self(label))
    }

    /// 取标签形式。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Caller {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// 一次工具调用的信封。
///
/// 七个字段里没有 workspace：工作区是会话属性，由 server 从连接身份推导，不是请求参数。
/// 信封的 `argv` 是内核的命令 AST，执行器消费它做权限预检与租约获取，不重新解析命令。
/// `stdin` 是 heredoc 正文之外的独立输入流，`Command` 的 `heredoc` 字段承载 heredoc 正文，
/// 两者不混用。`lease_id` 是嵌套调用继承外层租约的标识，本信封只承载它，租约的获取与
/// 释放是执行器的事。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    /// 协议版本。
    pub version: ProtocolVersion,
    /// 调用者标识。
    pub caller: Caller,
    /// 会话 id。
    pub session: SessionId,
    /// 工作目录路径。
    pub cwd: String,
    /// 命令 AST，执行器消费它做权限预检与租约获取。
    pub argv: Command,
    /// heredoc 正文之外的独立输入流，空则 `None`。
    pub stdin: Option<Vec<u8>>,
    /// 嵌套调用继承外层租约的可选标识。
    pub lease_id: Option<LeaseId>,
}

/// 信封解码失败的原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// 字节流在字段中间结束。
    Truncated,
    /// 最后一个字段之后还有多余字节。
    ExtraBytes,
    /// 协议版本字段的长度前缀不是 4。
    BadVersionLength {
        /// 读到的长度前缀。
        length: u32,
    },
    /// 重定向方向字节不是 0、1、2。
    BadRedirectKind {
        /// 读到的字节。
        byte: u8,
    },
    /// 存在标志既不是 0 也不是 1。
    BadFlag {
        /// 读到的字节。
        byte: u8,
    },
    /// 标识符文本不是合法的 26 字符 ULID。
    BadId {
        /// 出错的标识符类型名，用于诊断。
        kind: &'static str,
    },
    /// 调用者标签不合法。
    BadCaller,
    /// 文本字段不是合法 UTF-8。
    InvalidUtf8,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => write!(f, "信封字节流在字段中间结束"),
            Self::ExtraBytes => write!(f, "信封最后一个字段之后还有多余字节"),
            Self::BadVersionLength { length } => {
                write!(f, "信封协议版本字段的长度前缀应为 4，实际为 {length}")
            }
            Self::BadRedirectKind { byte } => {
                write!(f, "信封重定向方向字节 {byte} 不是 0、1、2")
            }
            Self::BadFlag { byte } => write!(f, "信封存在标志 {byte} 既不是 0 也不是 1"),
            Self::BadId { kind } => write!(f, "信封的{kind}标识符不是合法的 26 字符 ULID"),
            Self::BadCaller => write!(f, "信封的调用者标签不合法"),
            Self::InvalidUtf8 => write!(f, "信封的文本字段不是合法 UTF-8"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// 一次调用的响应。
///
/// 工具调用的可观察结果就是标准输出、标准错误与退出码三样；stdout 与 stderr 是字节流，
/// 不解释换行，不转码。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    /// 标准输出。
    pub stdout: Vec<u8>,
    /// 标准错误。
    pub stderr: Vec<u8>,
    /// 退出码。
    pub exit_code: i32,
}

/// 处理失败的原因。
///
/// 策略拒绝与租约拒绝是两种不同的失败，分成两个变体：策略拒绝是"生效策略不收这次
/// 调用"，租约拒绝是"目标路径被占"，调用方对两者的处置不同。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandlerError {
    /// 策略拒绝：生效策略不收这次调用。
    PolicyDenied {
        /// 拒绝原因。
        reason: String,
    },
    /// 租约拒绝：目标路径被占，或写入前校验不通过。
    LeaseDenied {
        /// 拒绝原因。
        reason: String,
    },
    /// 处理失败：策略与租约都通过，但调用本身失败。
    Failed {
        /// 失败原因。
        reason: String,
    },
}

impl fmt::Display for HandlerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PolicyDenied { reason } => write!(f, "策略拒绝：{reason}"),
            Self::LeaseDenied { reason } => write!(f, "租约拒绝：{reason}"),
            Self::Failed { reason } => write!(f, "处理失败：{reason}"),
        }
    }
}

impl std::error::Error for HandlerError {}

/// server 侧处理函数。
///
/// 入参类型只有 [`Invocation`]，没有 workspace 参数：工作区是会话属性，由 server 从连接
/// 身份推导，不进 handler 签名。这条由编译器保证，不靠调用方记得。
pub trait Handler {
    /// 处理一次调用。
    ///
    /// # 边界与前提
    /// 实现方不接触工作区、数据库或网络连接；一切外部状态由调用方显式传入。
    ///
    /// # 副作用
    /// 由实现方决定，本契约不约束。
    fn handle(&self, invocation: Invocation) -> Result<Response, HandlerError>;
}

/// 把一个信封编码成长度前缀字节流。
///
/// # 步骤
/// 1. 写协议版本：长度前缀 4 加大端 4 字节主版本号。
/// 2. 写调用者标签、会话 id、工作目录，各是长度前缀文本。
/// 3. 写命令 AST：程序名、参数计数与每个参数、重定向计数与每条方向加目标、heredoc 存在
///    标志加分隔符与正文。
/// 4. 写独立输入流：1 字节存在标志加大端 4 字节长度加原始字节。
/// 5. 写外层租约标识：1 字节存在标志加长度前缀文本。
///
/// # 边界与前提
/// 纯函数，无 IO。每个字段先写长度再写内容，解码按长度切回，正文含换行不构成歧义。
///
/// # 副作用
/// 无。
pub fn encode(invocation: &Invocation) -> Vec<u8> {
    let mut out = Vec::new();

    // 协议版本：长度前缀 4 加大端 4 字节主版本号。
    write_u32(&mut out, invocation.version.major());

    // 调用者标签、会话 id、工作目录。
    write_text(&mut out, invocation.caller.as_str());
    write_text(&mut out, invocation.session.as_str());
    write_text(&mut out, &invocation.cwd);

    // 命令 AST。
    write_command(&mut out, &invocation.argv);

    // 独立输入流：存在标志加长度前缀原始字节。
    match &invocation.stdin {
        Some(bytes) => {
            out.push(1);
            write_len(&mut out, bytes.len());
            out.extend_from_slice(bytes);
        }
        None => out.push(0),
    }

    // 外层租约标识：存在标志加长度前缀文本。
    match &invocation.lease_id {
        Some(id) => {
            out.push(1);
            write_text(&mut out, id.as_str());
        }
        None => out.push(0),
    }

    out
}

/// 把长度前缀字节流解码回一个信封。
///
/// # 步骤
/// 1. 读协议版本、调用者标签、会话 id、工作目录。
/// 2. 读命令 AST。
/// 3. 读独立输入流与外层租约标识。
/// 4. 校验最后一个字段之后没有多余字节。
///
/// # 边界与前提
/// 纯函数，无 IO。字节流在字段中间结束报 `Truncated`，字段之后还有多余字节报
/// `ExtraBytes`，未知方向字节与存在标志报对应变体，标识符文本不合法报 `BadId`。
///
/// # 副作用
/// 无。
pub fn decode(bytes: &[u8]) -> Result<Invocation, DecodeError> {
    let mut cursor = Cursor { bytes, pos: 0 };

    // 协议版本：长度前缀必须为 4，内容是大端 4 字节主版本号。
    let version = ProtocolVersion::new(cursor.read_u32()?);

    // 调用者标签。
    let caller = Caller::new(cursor.read_text()?).map_err(|_| DecodeError::BadCaller)?;

    // 会话 id。
    let session = SessionId::parse(cursor.read_text()?)
        .map_err(|_| DecodeError::BadId { kind: SessionId::kind() })?;

    // 工作目录。
    let cwd = cursor.read_text()?;

    // 命令 AST。
    let argv = read_command(&mut cursor)?;

    // 独立输入流。
    let stdin = if cursor.read_flag()? {
        let len = cursor.read_len()?;
        Some(cursor.read_fixed(len)?.to_vec())
    } else {
        None
    };

    // 外层租约标识。
    let lease_id = if cursor.read_flag()? {
        Some(
            LeaseId::parse(cursor.read_text()?)
                .map_err(|_| DecodeError::BadId { kind: LeaseId::kind() })?,
        )
    } else {
        None
    };

    // 最后一个字段之后不应还有字节。
    if !cursor.is_done() {
        return Err(DecodeError::ExtraBytes);
    }

    Ok(Invocation { version, caller, session, cwd, argv, stdin, lease_id })
}

/// 写一个长度前缀：大端 4 字节。
fn write_len(out: &mut Vec<u8>, len: usize) {
    out.extend_from_slice(&(len as u32).to_be_bytes());
}

/// 写一个长度前缀文本字段。
fn write_text(out: &mut Vec<u8>, text: &str) {
    write_len(out, text.len());
    out.extend_from_slice(text.as_bytes());
}

/// 写一个长度前缀 4 的大端 4 字节无符号整数。
fn write_u32(out: &mut Vec<u8>, value: u32) {
    write_len(out, 4);
    out.extend_from_slice(&value.to_be_bytes());
}

/// 写一条命令 AST。
fn write_command(out: &mut Vec<u8>, command: &Command) {
    write_text(out, &command.program);

    write_len(out, command.args.len());
    for arg in &command.args {
        write_text(out, arg);
    }

    write_len(out, command.redirects.len());
    for redirect in &command.redirects {
        out.push(match redirect.kind {
            RedirectKind::Write => 0,
            RedirectKind::Append => 1,
            RedirectKind::Read => 2,
        });
        write_text(out, &redirect.target);
    }

    match &command.heredoc {
        Some(doc) => {
            out.push(1);
            write_text(out, &doc.delimiter);
            write_text(out, &doc.body);
        }
        None => out.push(0),
    }
}

/// 读回一条命令 AST。
fn read_command(cursor: &mut Cursor<'_>) -> Result<Command, DecodeError> {
    let program = cursor.read_text()?;

    let arg_count = cursor.read_len()?;
    let mut args = Vec::with_capacity(arg_count);
    for _ in 0..arg_count {
        args.push(cursor.read_text()?);
    }

    let redirect_count = cursor.read_len()?;
    let mut redirects = Vec::with_capacity(redirect_count);
    for _ in 0..redirect_count {
        let kind = match cursor.read_fixed(1)?[0] {
            0 => RedirectKind::Write,
            1 => RedirectKind::Append,
            2 => RedirectKind::Read,
            byte => return Err(DecodeError::BadRedirectKind { byte }),
        };
        redirects.push(Redirect { kind, target: cursor.read_text()? });
    }

    let heredoc = if cursor.read_flag()? {
        Some(HereDoc { delimiter: cursor.read_text()?, body: cursor.read_text()? })
    } else {
        None
    };

    Ok(Command { program, args, redirects, heredoc })
}

/// 按长度切字节的游标。
struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    /// 读 4 字节大端长度前缀。
    fn read_len(&mut self) -> Result<usize, DecodeError> {
        let raw = self.read_fixed(4)?;
        Ok(u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]) as usize)
    }

    /// 读一个长度前缀 4 的大端 4 字节无符号整数。
    fn read_u32(&mut self) -> Result<u32, DecodeError> {
        let len = self.read_len()?;
        if len != 4 {
            return Err(DecodeError::BadVersionLength { length: len as u32 });
        }
        let raw = self.read_fixed(4)?;
        Ok(u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]))
    }

    /// 读恰好 `n` 个字节。
    fn read_fixed(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        if self.pos + n > self.bytes.len() {
            return Err(DecodeError::Truncated);
        }
        let slice = &self.bytes[self.pos..self.pos + n];
        self.pos += n;
        Ok(slice)
    }

    /// 读一个长度前缀文本字段。
    fn read_text(&mut self) -> Result<String, DecodeError> {
        let len = self.read_len()?;
        let raw = self.read_fixed(len)?;
        String::from_utf8(raw.to_vec()).map_err(|_| DecodeError::InvalidUtf8)
    }

    /// 读一个 1 字节存在标志。
    fn read_flag(&mut self) -> Result<bool, DecodeError> {
        match self.read_fixed(1)?[0] {
            0 => Ok(false),
            1 => Ok(true),
            byte => Err(DecodeError::BadFlag { byte }),
        }
    }

    /// 是否已读到末尾。
    fn is_done(&self) -> bool {
        self.pos == self.bytes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_ULID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

    fn invocation() -> Invocation {
        Invocation {
            version: ProtocolVersion::new(1),
            caller: Caller::new("worker-a").expect("合法标签"),
            session: SessionId::parse(VALID_ULID).expect("合法"),
            cwd: "/tmp/ws".to_owned(),
            argv: Command {
                program: "echo".to_owned(),
                args: vec!["hello".to_owned()],
                redirects: vec![Redirect {
                    kind: RedirectKind::Write,
                    target: "out.txt".to_owned(),
                }],
                heredoc: None,
            },
            stdin: None,
            lease_id: None,
        }
    }

    /// 全部字段编码再解码等于原值：同版本往返一致，不丢字段、不丢字节。
    #[test]
    fn round_trip_is_identical() {
        let mut original = invocation();
        original.stdin = Some(vec![1, 2, 3]);
        original.lease_id = Some(LeaseId::parse(VALID_ULID).expect("合法"));
        let decoded = decode(&encode(&original)).expect("应当能解码");
        assert_eq!(decoded, original);
    }

    /// handler 的入参类型只有 Invocation，没有 workspace 参数：工作区是会话属性，由
    /// server 从连接身份推导，不进 handler 签名。这条由编译器保证。
    #[test]
    fn handler_cannot_express_workspace() {
        struct TestHandler;
        impl Handler for TestHandler {
            fn handle(&self, invocation: Invocation) -> Result<Response, HandlerError> {
                Ok(Response {
                    stdout: invocation.argv.program.as_bytes().to_vec(),
                    stderr: Vec::new(),
                    exit_code: 0,
                })
            }
        }
        let response = TestHandler.handle(invocation()).expect("应当成功");
        assert_eq!(response.exit_code, 0);
        assert_eq!(response.stdout, b"echo");
    }

    /// 信封的 argv 是内核的命令 AST：程序名、参数、重定向、heredoc 四字段逐一往返相等。
    #[test]
    fn argv_round_trips_command_ast() {
        let mut original = invocation();
        original.argv.heredoc =
            Some(HereDoc { delimiter: "ITA_EOF".to_owned(), body: "fn main() {}\n".to_owned() });
        original
            .argv
            .redirects
            .push(Redirect { kind: RedirectKind::Read, target: "in.txt".to_owned() });
        let decoded = decode(&encode(&original)).expect("应当能解码");
        assert_eq!(decoded.argv, original.argv);
        assert_eq!(decoded.argv.program, "echo");
        assert_eq!(decoded.argv.args, vec!["hello".to_owned()]);
        assert_eq!(decoded.argv.redirects.len(), 2);
        assert_eq!(decoded.argv.heredoc.as_ref().expect("应当有正文").body, "fn main() {}\n");
    }

    /// stdin 是 heredoc 正文之外的独立输入流：Some 与 None 都往返不丢。
    #[test]
    fn stdin_round_trips_independent_input_stream() {
        let mut with_stdin = invocation();
        with_stdin.stdin = Some(vec![1, 2, 3]);
        let decoded = decode(&encode(&with_stdin)).expect("应当能解码");
        assert_eq!(decoded.stdin, Some(vec![1, 2, 3]));

        let without_stdin = invocation();
        let decoded = decode(&encode(&without_stdin)).expect("应当能解码");
        assert_eq!(decoded.stdin, None);
    }

    /// lease_id 是嵌套调用继承外层租约的标识：Some 与 None 都往返不丢。
    #[test]
    fn lease_id_round_trips_outer_lease_marker() {
        let mut with_lease = invocation();
        with_lease.lease_id = Some(LeaseId::parse(VALID_ULID).expect("合法"));
        let decoded = decode(&encode(&with_lease)).expect("应当能解码");
        assert_eq!(decoded.lease_id, with_lease.lease_id);

        let without_lease = invocation();
        let decoded = decode(&encode(&without_lease)).expect("应当能解码");
        assert_eq!(decoded.lease_id, None);
    }

    /// 字节流在字段中间结束时报截断，不会把半截字段读成合法值。
    #[test]
    fn truncated_input_is_rejected() {
        let bytes = encode(&invocation());
        let truncated = &bytes[..bytes.len() - 1];
        assert!(matches!(decode(truncated), Err(DecodeError::Truncated)));
    }

    /// 最后一个字段之后还有多余字节时报 ExtraBytes，不会静默丢弃尾部数据。
    #[test]
    fn extra_bytes_are_rejected() {
        let mut bytes = encode(&invocation());
        bytes.push(0xFF);
        assert!(matches!(decode(&bytes), Err(DecodeError::ExtraBytes)));
    }

    /// 重定向方向字节不是 0、1、2 时被拒，解码不会把未知方向读成某个已知方向。
    #[test]
    fn bad_redirect_kind_is_rejected() {
        let mut bytes = Vec::new();
        write_u32(&mut bytes, 1);
        write_text(&mut bytes, "worker-a");
        write_text(&mut bytes, VALID_ULID);
        write_text(&mut bytes, "/tmp/ws");
        write_text(&mut bytes, "echo");
        write_len(&mut bytes, 0);
        write_len(&mut bytes, 1);
        bytes.push(3);
        write_text(&mut bytes, "out.txt");
        bytes.push(0);
        bytes.push(0);
        assert!(matches!(decode(&bytes), Err(DecodeError::BadRedirectKind { byte: 3 })));
    }

    /// 存在标志既不是 0 也不是 1 时被拒，heredoc 与租约标识不会把未知标志读成存在。
    #[test]
    fn bad_presence_flag_is_rejected() {
        let mut bytes = Vec::new();
        write_u32(&mut bytes, 1);
        write_text(&mut bytes, "worker-a");
        write_text(&mut bytes, VALID_ULID);
        write_text(&mut bytes, "/tmp/ws");
        write_text(&mut bytes, "echo");
        write_len(&mut bytes, 0);
        write_len(&mut bytes, 0);
        bytes.push(2);
        bytes.push(0);
        assert!(matches!(decode(&bytes), Err(DecodeError::BadFlag { byte: 2 })));
    }

    /// 会话 id 不是合法的 26 字符 ULID 时被拒，错误里带回类型名。
    #[test]
    fn bad_session_id_is_rejected() {
        let mut bytes = Vec::new();
        write_u32(&mut bytes, 1);
        write_text(&mut bytes, "worker-a");
        write_text(&mut bytes, "not-a-ulid");
        assert!(matches!(decode(&bytes), Err(DecodeError::BadId { kind: "会话" })));
    }

    /// 调用者标签含大写时被拒，解码不会把未校验的标签读进信封。
    #[test]
    fn bad_caller_label_is_rejected() {
        let mut bytes = Vec::new();
        write_u32(&mut bytes, 1);
        write_text(&mut bytes, "Worker");
        assert!(matches!(decode(&bytes), Err(DecodeError::BadCaller)));
    }

    /// 调用者标签的校验规则：空串、首尾连字符、连续连字符、非法字符都被拒。
    #[test]
    fn caller_label_validation_rules() {
        assert_eq!(Caller::new(""), Err(CallerError::Empty));
        assert!(matches!(Caller::new("-store"), Err(CallerError::LeadingOrTrailingHyphen { .. })));
        assert!(matches!(Caller::new("a--b"), Err(CallerError::ConsecutiveHyphens { .. })));
        assert!(matches!(Caller::new("Store"), Err(CallerError::InvalidChar { .. })));
        assert!(Caller::new("worker-1").is_ok());
    }
}
