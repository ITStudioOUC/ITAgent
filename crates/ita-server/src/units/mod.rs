//! unit 监督：server 子系统生命周期监督。
//!
//! unit 是 server 内固定命名的子系统（store、router、ipc、lease、tools、model、telemetry），
//! 拓扑在启动时静态声明，不动态建行。本模块含拓扑类型与无环校验（`topology`）、状态机
//! （`state`）、就绪探针（`probe`）与监督者（`supervisor`），不含启动门与能力集快照。
//!
//! unit 与工作流节点是两套监督语义，靠类型与文档分开：本模块的类型独立于 `ita-workflow`
//! 的节点类型，不复用、不混用。
//!
//! 最小拓扑里的 `lease ↔ tools` 是双向记号。按字面建成互相 `Requires` 会自己成环，被自己的
//! 无环校验拒绝。因此 `↔` 落成单方向边：`lease` 依赖 `tools`（租约仲裁需要工具目录注册完成
//! 才能仲裁跨工作区资源，工具目录注册不依赖租约仲裁），`tools` 先启动。测试按这个方向实例化。

pub mod probe;
pub mod state;
pub mod supervisor;
pub mod topology;

pub use probe::{ProbeResult, ReadinessProbe};
pub use state::{UnitEvent, UnitState, next};
pub use supervisor::{ShutdownResult, StepOutcome, Supervisor};
pub use topology::{CycleError, Topology};

use std::fmt;

/// unit 的标签名义类型。
///
/// unit 是固定命名子系统，不是动态建行实体，所以标识是校验过的标签，不是 ULID。合法标签是
/// 小写字母数字加连字符，非空，首尾不连字符，不连续连字符，与 systemd unit 命名同型。
///
/// `Ord` 是标签的词典序，供产生稳定启动顺序的最小堆排序；`Hash` 供边的集合去重。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UnitId(String);

/// unit 标签无法被解析的原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnitIdError {
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

impl UnitId {
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
    pub fn new(label: impl Into<String>) -> Result<Self, UnitIdError> {
        let label = label.into();
        if label.is_empty() {
            return Err(UnitIdError::Empty);
        }
        let chars: Vec<char> = label.chars().collect();
        if chars.first() == Some(&'-') || chars.last() == Some(&'-') {
            return Err(UnitIdError::LeadingOrTrailingHyphen { label });
        }
        for (offset, &ch) in chars.iter().enumerate() {
            let legal = ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-';
            if !legal {
                return Err(UnitIdError::InvalidChar { offset, ch });
            }
        }
        for window in chars.windows(2) {
            if window[0] == '-' && window[1] == '-' {
                return Err(UnitIdError::ConsecutiveHyphens { label });
            }
        }
        Ok(Self(label))
    }

    /// 取标签形式。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for UnitId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Display for UnitIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "unit 标签不能为空"),
            Self::LeadingOrTrailingHyphen { label } => {
                write!(f, "unit 标签 `{label}` 首尾不能是连字符")
            }
            Self::ConsecutiveHyphens { label } => {
                write!(f, "unit 标签 `{label}` 含连续连字符")
            }
            Self::InvalidChar { offset, ch } => {
                write!(f, "unit 标签第 {offset} 个字符 `{ch}` 不是小写字母数字或连字符")
            }
        }
    }
}

impl std::error::Error for UnitIdError {}

/// 两个 unit 之间的依赖种类。
///
/// 三种种类在图结构里方向相同：`X` 依赖 `Y`（无论哪种）都产生 `Y → X` 的边，即被依赖的
/// `Y` 先启动。种类只记录在边上，供失败传播与启动门消费，无环校验与启动排序不消费种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DepKind {
    /// 硬依赖：被依赖方失败则本方连带失败。
    Requires,
    /// 软依赖：被依赖方失败则本方只是降级。
    Wants,
    /// 仅排序：只规定被依赖方先启动，不隐含依赖。
    After,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 正常的小写标签被接受。
    #[test]
    fn unit_id_accepts_lowercase_label() {
        assert!(UnitId::new("store").is_ok());
        assert!(UnitId::new("lease").is_ok());
        assert!(UnitId::new("model-proxy").is_ok());
        assert!(UnitId::new("a1").is_ok());
    }

    /// 空标签被拒。
    #[test]
    fn unit_id_rejects_empty_label() {
        assert_eq!(UnitId::new(""), Err(UnitIdError::Empty));
    }

    /// 首尾连字符被拒。
    #[test]
    fn unit_id_rejects_leading_or_trailing_hyphen() {
        assert!(matches!(UnitId::new("-store"), Err(UnitIdError::LeadingOrTrailingHyphen { .. })));
        assert!(matches!(UnitId::new("store-"), Err(UnitIdError::LeadingOrTrailingHyphen { .. })));
    }

    /// 连续连字符被拒。
    #[test]
    fn unit_id_rejects_consecutive_hyphens() {
        assert!(matches!(UnitId::new("a--b"), Err(UnitIdError::ConsecutiveHyphens { .. })));
    }

    /// 大写、非 ASCII 与其它字符被拒，不自动转换。
    #[test]
    fn unit_id_rejects_invalid_characters() {
        assert!(matches!(UnitId::new("Store"), Err(UnitIdError::InvalidChar { .. })));
        assert!(matches!(UnitId::new("store_"), Err(UnitIdError::InvalidChar { .. })));
        assert!(matches!(UnitId::new("存"), Err(UnitIdError::InvalidChar { .. })));
    }

    /// 解析出的标签文本形式必须等于原串，Display 与 as_str 一致。
    #[test]
    fn unit_id_text_form_round_trips() {
        let id = UnitId::new("store").expect("应当合法");
        assert_eq!(id.as_str(), "store");
        assert_eq!(id.to_string(), "store");
    }

    /// Ord 是标签词典序，最小堆可以按它排序。
    #[test]
    fn unit_id_orders_by_label() {
        let a = UnitId::new("a").expect("合法");
        let b = UnitId::new("b").expect("合法");
        let aa = UnitId::new("aa").expect("合法");
        assert!(a < b);
        assert!(a < aa);
        // 逐字符比较，'a' < 'b'，所以 "aa" < "b"。
        assert!(aa < b);
    }

    /// Hash 与 Eq 一致，相同标签落同一桶。
    #[test]
    fn unit_id_hash_is_consistent_with_eq() {
        use std::collections::HashSet;
        let a = UnitId::new("store").expect("合法");
        let b = UnitId::new("store").expect("合法");
        assert_eq!(a, b);
        let mut set = HashSet::new();
        set.insert(a);
        assert!(set.contains(&b));
        assert_eq!(set.len(), 1);
    }
}
