//! 预算：解析与压缩共用的容量模型。
//!
//! 单位是"抽象计数"而不是 token：分词属于模型层，内核不能依赖具体分词器，否则同一份
//! 输入在不同模型下会得到不同的解析结果，逐字节复现就不成立。调用方按自己的计数方式
//! 填数，内核只做加减与比较。
//!
//! 压缩预留是这个结构里最容易漏的一项：压缩本身要一次模型调用，输入是整个上下文。如果
//! 预算里没有单独为它留位置，到阈值时压缩请求会因自身超窗而失败。

/// 预算参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Budget {
    /// 上下文窗口的硬上限。
    pub limit: u64,
    /// 压缩触发比率，以千分比表示（850 表示 85%）。
    ///
    /// 用整数而不是浮点：浮点在跨平台序列化与比较时会产生"看起来相等其实不等"的问题，
    /// 而预算判断必须可复现。
    pub compress_trigger_permille: u32,
    /// 压缩调用自身需要的预留量。
    pub compress_reserve: u64,
    /// 保留的最近轮数。
    pub keep_recent_turns: u32,
    /// 预览的最大字节数（头 + 尾）。
    pub preview_bytes: u64,
}

/// 构造预算时的错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BudgetError {
    /// 上限为零。
    ZeroLimit,
    /// 触发比率不在 1 到 1000 之间。
    TriggerOutOfRange {
        /// 实际值。
        actual: u32,
    },
    /// 预留量不小于上限，压缩永远无法进行。
    ReserveTooLarge {
        /// 预留量。
        reserve: u64,
        /// 上限。
        limit: u64,
    },
    /// 预留量比触发点之后剩下的空间还大。
    ReserveExceedsHeadroom {
        /// 预留量。
        reserve: u64,
        /// 触发点之后剩余的空间。
        headroom: u64,
    },
    /// 保留轮数为零：压缩后没有原文可对照，摘要的错误无法被模型自己发现。
    ZeroRecentTurns,
}

impl std::fmt::Display for BudgetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZeroLimit => write!(f, "上下文上限不能为零"),
            Self::TriggerOutOfRange { actual } => {
                write!(f, "压缩触发比率应在 1 到 1000 之间，实际为 {actual}")
            }
            Self::ReserveTooLarge { reserve, limit } => {
                write!(f, "压缩预留 {reserve} 不小于上下文上限 {limit}")
            }
            Self::ReserveExceedsHeadroom { reserve, headroom } => write!(
                f,
                "压缩预留 {reserve} 超过触发点之后剩余的空间 {headroom}，压缩请求会因自身超窗而失败"
            ),
            Self::ZeroRecentTurns => write!(f, "保留轮数不能为零"),
        }
    }
}

impl std::error::Error for BudgetError {}

impl Budget {
    /// 默认预算。
    ///
    /// # 边界与前提
    /// 这些值来自 docs/adr/0018 的参数表，是默认值而不是不变量；生产路径应当从配置构造。
    ///
    /// # 副作用
    /// 无。
    pub fn defaults() -> Self {
        Self {
            limit: 200_000,
            compress_trigger_permille: 850,
            compress_reserve: 20_000,
            keep_recent_turns: 3,
            preview_bytes: 5 * 1024,
        }
    }

    /// 校验并构造。
    ///
    /// # 步骤
    /// 1. 上限与保留轮数不能为零。
    /// 2. 触发比率落在 1 到 1000。
    /// 3. 预留量小于上限。
    /// 4. 预留量不超过触发点之后剩余的空间——否则压缩永远无法完成。
    ///
    /// # 边界与前提
    /// 第 4 条是 85% 阈值下最容易踩的坑：阈值越高，剩余空间越少，预留越容易装不下。
    ///
    /// # 副作用
    /// 无。
    pub fn new(
        limit: u64,
        compress_trigger_permille: u32,
        compress_reserve: u64,
        keep_recent_turns: u32,
        preview_bytes: u64,
    ) -> Result<Self, BudgetError> {
        if limit == 0 {
            return Err(BudgetError::ZeroLimit);
        }
        if !(1..=1000).contains(&compress_trigger_permille) {
            return Err(BudgetError::TriggerOutOfRange { actual: compress_trigger_permille });
        }
        if keep_recent_turns == 0 {
            return Err(BudgetError::ZeroRecentTurns);
        }
        if compress_reserve >= limit {
            return Err(BudgetError::ReserveTooLarge { reserve: compress_reserve, limit });
        }
        let headroom = limit - self::trigger_point(limit, compress_trigger_permille);
        if compress_reserve > headroom {
            return Err(BudgetError::ReserveExceedsHeadroom {
                reserve: compress_reserve,
                headroom,
            });
        }
        Ok(Self {
            limit,
            compress_trigger_permille,
            compress_reserve,
            keep_recent_turns,
            preview_bytes,
        })
    }

    /// 压缩触发点（绝对量）。
    ///
    /// # 边界与前提
    /// 向下取整：宁可早一点压缩，也不要晚到压缩请求自己发不出去。
    ///
    /// # 副作用
    /// 无。
    pub fn trigger_at(&self) -> u64 {
        trigger_point(self.limit, self.compress_trigger_permille)
    }

    /// 给定当前用量，是否该压缩。
    ///
    /// # 边界与前提
    /// 达到触发点即触发，不要求超过。
    ///
    /// # 副作用
    /// 无。
    pub fn should_compress(&self, used: u64) -> bool {
        used >= self.trigger_at()
    }

    /// 触发点之后还能装多少。
    ///
    /// # 边界与前提
    /// 用量已经超过上限时返回零，不返回负数（无符号减法会下溢）。
    ///
    /// # 副作用
    /// 无。
    pub fn remaining_after_trigger(&self, used: u64) -> u64 {
        self.limit.saturating_sub(used.max(self.trigger_at()))
    }
}

fn trigger_point(limit: u64, permille: u32) -> u64 {
    limit.saturating_mul(u64::from(permille)) / 1000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 默认预算自洽() {
        let b = Budget::defaults();
        assert!(
            Budget::new(
                b.limit,
                b.compress_trigger_permille,
                b.compress_reserve,
                b.keep_recent_turns,
                b.preview_bytes
            )
            .is_ok()
        );
        assert_eq!(b.trigger_at(), 170_000);
    }

    #[test]
    fn 达到触发点即触发() {
        let b = Budget::defaults();
        assert!(!b.should_compress(169_999));
        assert!(b.should_compress(170_000));
        assert!(b.should_compress(200_000));
    }

    #[test]
    fn 预留装不下时构造失败() {
        // 上限 100、触发 850 千分比 → 触发点 85、剩余 15；预留 20 装不下。
        let err = Budget::new(100, 850, 20, 3, 1024).expect_err("应当被拒");
        assert!(matches!(err, BudgetError::ReserveExceedsHeadroom { reserve: 20, headroom: 15 }));
    }

    #[test]
    fn 高阈值让预留更容易装不下() {
        // 同样上限与预留，阈值 95% 时剩余只有 5，必然失败。
        assert!(Budget::new(100, 950, 10, 3, 1024).is_err());
        // 阈值 50% 时剩余 50，同样预留可以通过。
        assert!(Budget::new(100, 500, 10, 3, 1024).is_ok());
    }

    #[test]
    fn 非法参数逐个被拒() {
        assert_eq!(Budget::new(0, 850, 1, 3, 1024), Err(BudgetError::ZeroLimit));
        assert!(matches!(
            Budget::new(100, 0, 1, 3, 1024),
            Err(BudgetError::TriggerOutOfRange { actual: 0 })
        ));
        assert!(matches!(
            Budget::new(100, 1001, 1, 3, 1024),
            Err(BudgetError::TriggerOutOfRange { actual: 1001 })
        ));
        assert_eq!(Budget::new(100, 850, 1, 0, 1024), Err(BudgetError::ZeroRecentTurns));
        assert!(matches!(
            Budget::new(100, 850, 100, 3, 1024),
            Err(BudgetError::ReserveTooLarge { .. })
        ));
    }

    #[test]
    fn 剩余空间不会下溢() {
        let b = Budget::defaults();
        assert_eq!(b.remaining_after_trigger(0), 30_000);
        assert_eq!(b.remaining_after_trigger(170_000), 30_000);
        assert_eq!(b.remaining_after_trigger(200_000), 0);
        assert_eq!(b.remaining_after_trigger(999_999), 0);
    }

    #[test]
    fn 触发点向下取整() {
        // 999 * 850 / 1000 = 849.15 → 849，宁可早压一点。
        assert_eq!(trigger_point(999, 850), 849);
    }

    #[test]
    fn 百分百触发点等于上限() {
        let b = Budget::new(100, 1000, 0, 3, 1024).expect("预留为零时应当通过");
        assert_eq!(b.trigger_at(), 100);
        assert!(b.should_compress(100));
    }
}
