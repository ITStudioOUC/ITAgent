//! 压缩触发：何时该压，以及压之前必须确认什么。
//!
//! 触发判断本身只有一次比较，但"该压"不等于"能压"：压缩调用要把整个上下文送进模型，如果
//! 触发点之后剩余的空间装不下这次调用，压缩请求会因自身超窗而失败。85% 的阈值让这个风险
//! 比低阈值时更大，因此这里把"该压"与"能压"分成两个问题。
//!
//! 两个判断都是纯函数：不读时钟、不读用量表，用量由调用方传入。

use super::super::budget::Budget;

/// 是否应当开始压缩。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerDecision {
    /// 还不到时候。
    NotYet {
        /// 当前用量。
        used: u64,
        /// 触发点。
        trigger_at: u64,
    },
    /// 可以压缩。
    Compress {
        /// 触发点之后剩余的空间。
        ///
        /// **这不是压缩调用的输入预算**：压缩的输入是整个上下文，其规模约等于 `used`。
        /// 这个数只用来判断"压缩请求自身会不会超窗"，即 `used + 本次调用开销 <= limit`。
        /// 把它当成输入预算会让调用方误裁掉十几万上下文。
        headroom: u64,
    },
    /// 该压但压不了：剩余空间装不下压缩调用。
    ///
    /// 这不是"再等等就好"——用量只会继续增长。调用方必须缩小输入（减少保留轮次、更早
    /// 触发下一次压缩，或先丢弃工具输出正文）才能继续。
    CannotCompress {
        /// 触发点之后剩余的空间。
        headroom: u64,
        /// 压缩调用需要的预留量。
        required: u64,
    },
}

/// 判断是否应当压缩。
///
/// # 步骤
/// 1. 用量未达触发点：返回 [`TriggerDecision::NotYet`]。
/// 2. 已达触发点：检查触发点之后的剩余空间是否装得下压缩预留。
/// 3. 装得下则返回可用的输入预算，装不下则明确拒绝。
///
/// # 边界与前提
/// `used` 是调用方按自己的分词方式算出的抽象计数，不能传字节数——两者不同量纲，混用会让
/// 阈值在真实用量的三分之一处就触发。
///
/// # 副作用
/// 无。
pub fn decide(budget: &Budget, used: u64) -> TriggerDecision {
    let trigger_at = budget.trigger_at();
    if used < trigger_at {
        return TriggerDecision::NotYet { used, trigger_at };
    }

    let headroom = budget.limit.saturating_sub(used);
    // 剩余空间为零时，即便预留也是零，也没有空间放压缩请求的任何开销。
    if headroom == 0 || headroom < budget.compress_reserve {
        return TriggerDecision::CannotCompress { headroom, required: budget.compress_reserve };
    }

    // 压缩调用的输入约等于当前用量；剩余空间只需要装下这次调用的额外开销。
    TriggerDecision::Compress { headroom }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::budget::Budget;

    fn budget() -> Budget {
        // 上限 200000、触发点 170000、预留 20000。
        Budget::defaults()
    }

    /// 未达触发点时不压缩，短历史永远不会被压缩。
    #[test]
    fn no_compression_below_trigger_point() {
        let decision = decide(&budget(), 100_000);
        assert_eq!(decision, TriggerDecision::NotYet { used: 100_000, trigger_at: 170_000 });
    }

    /// 刚好达到触发点时压缩，边界是包含的。
    #[test]
    fn compression_at_exact_trigger_point() {
        let decision = decide(&budget(), 170_000);
        assert_eq!(decision, TriggerDecision::Compress { headroom: 30_000 });
    }

    /// 剩余空间小于预留时明确拒绝而不是硬压，装不下的请求不会被发出。
    #[test]
    fn insufficient_headroom_refused_not_forced() {
        // 用量 190000，剩余 10000，小于预留 20000。
        let decision = decide(&budget(), 190_000);
        assert_eq!(
            decision,
            TriggerDecision::CannotCompress { headroom: 10_000, required: 20_000 }
        );
    }

    /// 用量超过上限时剩余空间为零而不是下溢，超限用量不会被报成巨大剩余。
    #[test]
    fn headroom_zero_not_underflow_over_limit() {
        let decision = decide(&budget(), 999_999);
        assert_eq!(decision, TriggerDecision::CannotCompress { headroom: 0, required: 20_000 });
    }

    /// 高阈值是否可用取决于上限与预留的比例，同样的预留在一个上限下失败、在另一个上限下通过。
    #[test]
    fn high_threshold_availability_depends_on_limit_reserve_ratio() {
        // 上限 200000、阈值 95% 时剩余只有 10000，装不下 20000 的预留，构造期就拒绝。
        assert!(Budget::new(200_000, 950, 20_000, 3, 4096).is_err());
        // 同样的阈值与预留，上限翻倍后剩余 20000，恰好可以构造。
        assert!(Budget::new(400_000, 950, 20_000, 3, 4096).is_ok());
    }

    /// 剩余空间随用量下降，用量越接近上限，决策越紧。
    #[test]
    fn headroom_decreases_with_usage() {
        assert_eq!(decide(&budget(), 175_000), TriggerDecision::Compress { headroom: 25_000 });
        assert_eq!(decide(&budget(), 180_000), TriggerDecision::Compress { headroom: 20_000 });
    }

    /// 即使预留为零，用量越过上限后也拒绝压缩：没有空间放压缩请求的任何开销。
    #[test]
    fn compression_refused_when_over_limit_even_with_zero_reserve() {
        // 预留为零时构造期不会拦，但用量越过上限后剩余空间为零，压缩请求自身必然超窗。
        let b = Budget::new(100, 500, 0, 3, 4096).expect("零预留应当可以构造");
        assert_eq!(decide(&b, 120), TriggerDecision::CannotCompress { headroom: 0, required: 0 });
        // 剩余空间为零时即便"预留为零"也不该判为可压：没有空间放压缩请求的任何开销。
        assert!(!matches!(decide(&b, 120), TriggerDecision::Compress { .. }));
    }
}
