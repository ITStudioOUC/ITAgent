//! 标识符类型。
//!
//! 五种 id 都是 26 字符的 ULID 文本形式。用专用类型而不是裸 `String`，是为了让"把节点
//! id 当会话 id 传"这类错误在编译期就被拦住——它们在三处以上同时出现，靠命名区分不够。

use std::fmt;

/// ULID 的文本长度。
pub const ULID_LEN: usize = 26;

/// 一个 id 无法被解析的原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdError {
    /// 长度不是 26。
    BadLength {
        /// 实际长度。
        actual: usize,
    },
    /// 含非法的 Crockford base32 字符。
    BadChar {
        /// 位置（字节偏移）。
        offset: usize,
        /// 那个字符。
        ch: char,
    },
}

impl fmt::Display for IdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadLength { actual } => {
                write!(f, "标识符长度应为 {ULID_LEN}，实际为 {actual}")
            }
            Self::BadChar { offset, ch } => {
                write!(f, "标识符第 {offset} 个字符 `{ch}` 不是合法的 Crockford base32")
            }
        }
    }
}

impl std::error::Error for IdError {}

/// 生成一个带具体类型的 id。
///
/// 五个类型共用同一套校验与格式化，差异只在类型本身。用宏而不是泛型，是为了让每个类型
/// 都是独立的名义类型：`fn f(NodeId)` 与 `fn f(SessionId)` 不可互换。
macro_rules! id_type {
    ($(#[$meta:meta])* $name:ident, $label:literal) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// 从文本构造，校验长度与字符集。
            ///
            /// # 边界与前提
            /// 只接受大写形式；小写会被拒绝而不是自动转成大写，避免"两个看起来相同的 id
            /// 其实不等"。
            ///
            /// # 副作用
            /// 无。
            pub fn parse(text: impl Into<String>) -> Result<Self, IdError> {
                let text = text.into();
                if text.len() != ULID_LEN {
                    return Err(IdError::BadLength { actual: text.len() });
                }
                for (offset, ch) in text.char_indices() {
                    if !is_crockford(ch) {
                        return Err(IdError::BadChar { offset, ch });
                    }
                    // ULID 是 48 位时间戳加 80 位随机，恰好 128 位；首位只用到 0 到 7。
                    // 允许更大的首位会接受任何生成器都不会产生的文本。
                    if offset == 0 && !matches!(ch, '0'..='7') {
                        return Err(IdError::BadChar { offset, ch });
                    }
                }
                Ok(Self(text))
            }

            /// 取文本形式。
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl $name {
            /// 类型名，用于诊断信息。
            pub fn kind() -> &'static str {
                $label
            }
        }
    };
}

/// Crockford base32：去掉了 I、L、O、U，避免与 1、0 混淆。
fn is_crockford(ch: char) -> bool {
    ch.is_ascii_digit() || matches!(ch, 'A'..='H' | 'J' | 'K' | 'M' | 'N' | 'P'..='T' | 'V'..='Z')
}

id_type!(
    /// 上下文树上的一个节点。
    NodeId,
    "节点"
);
id_type!(
    /// 一个会话。分叉会产生新的会话，共享祖先节点。
    SessionId,
    "会话"
);
id_type!(
    /// 一个工作区。
    WorkspaceId,
    "工作区"
);
id_type!(
    /// 一次检查点：上下文位置与文件快照的组合标识。
    CheckpointId,
    "检查点"
);
id_type!(
    /// 一条事件。append-only 事件日志里的一行，按工作区内的 seq 排序。
    EventId,
    "事件"
);

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

    /// 五种标识符都接受规范的 26 字符 Crockford base32 串。
    #[test]
    fn valid_identifier_is_accepted() {
        assert!(NodeId::parse(VALID).is_ok());
        assert!(SessionId::parse(VALID).is_ok());
        assert!(WorkspaceId::parse(VALID).is_ok());
        assert!(CheckpointId::parse(VALID).is_ok());
        assert!(EventId::parse(VALID).is_ok());
    }

    /// 长度不符的串被拒，且错误里带上实际长度，让调用方看见错在哪。
    #[test]
    fn bad_length_is_rejected_and_reports_actual_length() {
        let err = NodeId::parse("short").expect_err("应当被拒");
        assert_eq!(err, IdError::BadLength { actual: 5 });
        assert!(err.to_string().contains('5'));
    }

    /// 小写输入被拒而不是自动转大写，拼写错误不会被悄悄读成合法标识符。
    #[test]
    fn lowercase_is_rejected_not_autocased() {
        let err = NodeId::parse(VALID.to_lowercase()).expect_err("应当被拒");
        assert!(matches!(err, IdError::BadChar { .. }));
    }

    /// 首位只能是 0 到 7，这是 Crockford base32 对最高位的约束。
    #[test]
    fn first_character_must_be_zero_to_seven() {
        for ch in ['8', '9', 'A', 'Z'] {
            let mut text = VALID.to_owned();
            text.replace_range(0..1, &ch.to_string());
            assert!(NodeId::parse(text).is_err(), "首位 {ch} 应当被拒");
        }
        for ch in ['0', '7'] {
            let mut text = VALID.to_owned();
            text.replace_range(0..1, &ch.to_string());
            assert!(NodeId::parse(text).is_ok(), "首位 {ch} 应当被接受");
        }
    }

    /// 8 与 9 在非首位是合法的，约束只落在首位。
    #[test]
    fn eight_and_nine_are_legal_outside_first_position() {
        let mut text = VALID.to_owned();
        text.replace_range(25..26, "9");
        assert!(NodeId::parse(text.clone()).is_ok());
        text.replace_range(25..26, "8");
        assert!(NodeId::parse(text).is_ok());
    }

    /// 字母表逐字符验证，被拒的只有 Crockford 排除的四个字母。
    #[test]
    fn alphabet_is_validated_character_by_character() {
        for ch in '0'..='9' {
            let mut text = VALID.to_owned();
            text.replace_range(25..26, &ch.to_string());
            assert!(NodeId::parse(text).is_ok(), "{ch} 应当被接受");
        }
        for ch in 'A'..='Z' {
            let mut text = VALID.to_owned();
            text.replace_range(25..26, &ch.to_string());
            let expected = !matches!(ch, 'I' | 'L' | 'O' | 'U');
            assert_eq!(NodeId::parse(text).is_ok(), expected, "{ch} 的接受性不符");
        }
    }

    /// I、L、O、U 被拒，避免与 1、0 混淆；接受它们的解析器会把错标识符读成对的。
    #[test]
    fn confusable_characters_are_rejected() {
        // I、L、O、U 不在 Crockford 字母表里。
        for ch in ['I', 'L', 'O', 'U'] {
            let mut text = VALID.to_owned();
            text.replace_range(25..26, &ch.to_string());
            assert!(NodeId::parse(text).is_err(), "{ch} 应当被拒");
        }
    }

    /// 解析出的标识符的文本形式必须等于原串，日志与传输往返不丢信息。
    #[test]
    fn text_form_round_trips() {
        let id = NodeId::parse(VALID).expect("应当合法");
        assert_eq!(id.as_str(), VALID);
        assert_eq!(id.to_string(), VALID);
    }

    /// 每种标识符类型暴露自己的中文名用于诊断，错误消息能说清是哪类标识符。
    #[test]
    fn type_name_is_used_for_diagnostics() {
        assert_eq!(NodeId::kind(), "节点");
        assert_eq!(CheckpointId::kind(), "检查点");
    }
}
