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
        /// 压缩调用可用的输入预算。
        input_budget: u64,
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
    if headroom < budget.compress_reserve {
        return TriggerDecision::CannotCompress { headroom, required: budget.compress_reserve };
    }

    // 压缩调用的输入不能超过剩余空间，否则请求自身超窗。
    TriggerDecision::Compress { input_budget: headroom }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::budget::Budget;

    fn budget() -> Budget {
        // 上限 200000、触发点 170000、预留 20000。
        Budget::defaults()
    }

    #[test]
    fn 未达触发点时不压缩() {
        let decision = decide(&budget(), 100_000);
        assert_eq!(decision, TriggerDecision::NotYet { used: 100_000, trigger_at: 170_000 });
    }

    #[test]
    fn 刚好达到触发点时压缩() {
        let decision = decide(&budget(), 170_000);
        assert_eq!(decision, TriggerDecision::Compress { input_budget: 30_000 });
    }

    #[test]
    fn 剩余空间不足时明确拒绝而不是硬压() {
        // 用量 190000，剩余 10000，小于预留 20000。
        let decision = decide(&budget(), 190_000);
        assert_eq!(
            decision,
            TriggerDecision::CannotCompress { headroom: 10_000, required: 20_000 }
        );
    }

    #[test]
    fn 用量超过上限时剩余为零而不是下溢() {
        let decision = decide(&budget(), 999_999);
        assert_eq!(decision, TriggerDecision::CannotCompress { headroom: 0, required: 20_000 });
    }

    #[test]
    fn 高阈值让可压缩窗口更窄() {
        // 同样预留 20000、上限 200000：95% 阈值下触发点 190000，剩余 10000，压不了。
        let tight = Budget::new(200_000, 950, 20_000, 3, 4096);
        assert!(tight.is_err(), "构造期就应当拒绝这种组合");
    }

    #[test]
    fn 输入预算等于剩余空间() {
        let decision = decide(&budget(), 175_000);
        assert_eq!(decision, TriggerDecision::Compress { input_budget: 25_000 });
    }
}
