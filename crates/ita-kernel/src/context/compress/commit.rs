//! 压缩提交计划。
//!
//! 压缩的最后一步是"写一个新帧并把 head 移过去"。这一步必须是原子的：摘要没拿到就绝不能
//! 移动 head，否则上下文被清空而摘要不存在，整段任务白跑。
//!
//! 内核只产出**计划**，不执行提交：写库、比较并交换 head、记录调用都在 server 侧。这样
//! "什么情况下允许提交"可以在不碰数据库的情况下穷举测试，而"怎么提交"由存储层保证。

use super::super::ids::NodeId;
use super::facts::Facts;
use super::fold::FoldPlan;
use super::format::CompressOutput;

/// 一个 head 令牌的最小形状。
///
/// 必须包含代次：只用节点 id 做比较并交换时，"回退到祖先再前进"会让旧令牌重新变成
/// "当前值"，从而通过比较——这正是 04.2 用代次解决的 ABA 问题。压缩路径不能把它丢掉。
pub trait HeadTokenLike {
    /// 期望的 head 节点。
    fn node(&self) -> &NodeId;
    /// 期望的代次。
    fn generation(&self) -> u64;
}

/// 提交计划：调用方按它执行一次短事务。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitPlan {
    /// 新帧的父节点，即提交前的 head。
    pub parent: NodeId,
    /// 提交前的 head 代次。
    ///
    /// 与 `parent` 一起构成提交时必须匹配的完整条件：只比节点会在 ABA 情形下失效。
    pub parent_generation: u64,
    /// 新帧覆盖的节点。
    pub covers: Vec<NodeId>,
    /// 新帧的完整正文：散文 + 事实段。
    pub summary: String,
    /// 生成摘要时固定层的指纹。
    pub fixed_layer_fingerprint: String,
    /// 检查点（如果这段历史有快照）。
    pub checkpoint: Option<super::super::ids::CheckpointId>,
    /// 提示版本，用于 replay。
    pub prompt_version: u32,
}

/// 无法生成提交计划的原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitError {
    /// 摘要校验失败。
    BadSummary(super::format::CompressError),
    /// 事实层不完整。
    BadFacts(super::facts::FactsError),
    /// 折叠范围为空：没有可压缩的内容。
    EmptyFold,
    /// 折叠范围的末尾不是当前 head：计划是基于另一段历史算出来的。
    FoldEndsBeforeHead {
        /// 覆盖段的最后一个节点。
        last_covered: NodeId,
        /// 当前 head。
        head: NodeId,
    },
}

impl std::fmt::Display for CommitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadSummary(e) => write!(f, "摘要不可用：{e}"),
            Self::BadFacts(e) => write!(f, "事实层不完整：{e}"),
            Self::EmptyFold => write!(f, "折叠范围为空，没有可压缩的内容"),
            Self::FoldEndsBeforeHead { last_covered, head } => write!(
                f,
                "折叠范围止于 {last_covered}，但当前 head 是 {head}；计划是基于另一段历史算的"
            ),
        }
    }
}

impl std::error::Error for CommitError {}

/// 生成提交计划。
///
/// # 步骤
/// 1. 校验折叠范围非空。
/// 2. 校验事实层完整。
/// 3. 校验 head 令牌与折叠计划的父节点一致。
/// 4. 把散文与事实段拼成帧正文。
///
/// # 边界与前提
/// 顺序是刻意的：先校验便宜的结构，再拼正文。第 3 步把"基于过期 head 算出的计划"挡在
/// 提交之前——否则会写出一个父节点不存在的帧，树就断了。
///
/// # 副作用
/// 无。
///
/// # 注意事项
/// 返回的计划**还没有提交**。调用方必须用同一个 head 令牌做比较并交换；令牌失效时整个
/// 计划作废，重算而不是重试——因为折叠范围也是基于旧 head 算的。
pub fn plan(
    head: &impl HeadTokenLike,
    fold: &FoldPlan,
    summary: &CompressOutput,
    facts: &Facts,
    fixed_layer_fingerprint: &str,
    checkpoint: Option<super::super::ids::CheckpointId>,
) -> Result<CommitPlan, CommitError> {
    if fold.covered.is_empty() {
        return Err(CommitError::EmptyFold);
    }
    facts.validate().map_err(CommitError::BadFacts)?;

    // 折叠范围的末尾必须就是当前 head：否则新帧的父节点与覆盖内容不连续，树会断。
    // 这条校验不需要整棵树，只需要折叠结果本身——覆盖段是按时间顺序排的，最后一个就是
    // 覆盖到的位置。
    let last_covered = fold.covered.last().expect("非空已在上面校验");
    if last_covered != head.node() {
        return Err(CommitError::FoldEndsBeforeHead {
            last_covered: last_covered.clone(),
            head: head.node().clone(),
        });
    }

    let parent = head.node().clone();

    // 散文在前、事实在后：模型读自己的叙述，再用事实校对。
    let summary_text = format!("{}\n\n{}", summary.prose, facts.render());

    Ok(CommitPlan {
        parent,
        parent_generation: head.generation(),
        covers: fold.covered.clone(),
        summary: summary_text,
        fixed_layer_fingerprint: fixed_layer_fingerprint.to_owned(),
        checkpoint,
        prompt_version: summary.prompt_version,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::compress::facts::{Failure, FileChange};
    use crate::context::ids::NodeId;

    const BASE: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

    fn id(n: u8) -> NodeId {
        let mut text = BASE.to_owned();
        text.replace_range(24..26, &format!("{n:02}"));
        NodeId::parse(text).expect("应当合法")
    }

    /// 测试用的最小令牌。
    struct Token(NodeId, u64);

    impl HeadTokenLike for Token {
        fn node(&self) -> &NodeId {
            &self.0
        }
        fn generation(&self) -> u64 {
            self.1
        }
    }

    fn fold(covered: Vec<NodeId>) -> FoldPlan {
        FoldPlan { covered, kept: vec![id(90)] }
    }

    fn output(prose: &str) -> CompressOutput {
        CompressOutput { prose: prose.to_owned(), prompt_version: 1 }
    }

    fn facts() -> Facts {
        Facts {
            changed_files: vec![FileChange { path: "a.rs".to_owned(), added: 1, removed: 0 }],
            failures: vec![Failure {
                attempt: "cargo build".to_owned(),
                outcome: "E0308".to_owned(),
            }],
            rejected: vec![],
            checkpoint: None,
            turns: 2,
            elapsed_secs: 60,
        }
    }

    #[test]
    fn 计划包含散文与事实两段() {
        let plan =
            plan(&Token(id(1), 3), &fold(vec![id(1)]), &output("改了 A"), &facts(), "fp", None)
                .expect("应当成功");
        assert!(plan.summary.starts_with("改了 A"));
        assert!(plan.summary.contains("事实记录："));
        assert!(plan.summary.contains("cargo build → E0308"));
        assert_eq!(plan.parent, id(1));
        assert_eq!(plan.parent_generation, 3);
        assert_eq!(plan.covers, vec![id(1)]);
        assert_eq!(plan.prompt_version, 1);
    }

    #[test]
    fn 空折叠范围被拒() {
        let err = plan(&Token(id(1), 3), &fold(vec![]), &output("x"), &facts(), "fp", None)
            .expect_err("应当被拒");
        assert_eq!(err, CommitError::EmptyFold);
    }

    #[test]
    fn 事实层不完整时不出计划() {
        let mut bad = facts();
        bad.failures = vec![Failure { attempt: " ".to_owned(), outcome: "x".to_owned() }];
        let err = plan(&Token(id(1), 3), &fold(vec![id(1)]), &output("x"), &bad, "fp", None)
            .expect_err("应当被拒");
        assert!(matches!(err, CommitError::BadFacts(_)));
    }

    #[test]
    fn 计划记录固定层指纹与检查点() {
        let cp = crate::context::ids::CheckpointId::parse(BASE).expect("合法");
        let plan = plan(
            &Token(id(1), 3),
            &fold(vec![id(1)]),
            &output("x"),
            &facts(),
            "fp-7",
            Some(cp.clone()),
        )
        .expect("应当成功");
        assert_eq!(plan.fixed_layer_fingerprint, "fp-7");
        assert_eq!(plan.checkpoint, Some(cp));
    }

    #[test]
    fn 事实段在散文之后() {
        let plan =
            plan(&Token(id(1), 3), &fold(vec![id(1)]), &output("叙述"), &facts(), "fp", None)
                .expect("应当成功");
        let prose_at = plan.summary.find("叙述").expect("应当有散文");
        let facts_at = plan.summary.find("事实记录：").expect("应当有事实");
        assert!(prose_at < facts_at, "散文必须在事实之前");
    }

    #[test]
    fn 折叠范围末尾与_head_不一致时拒绝出计划() {
        // 覆盖段止于 id(1)，而 head 是 id(2)：两者不连续，写出来的帧会接错父节点。
        let err = plan(&Token(id(2), 1), &fold(vec![id(1)]), &output("x"), &facts(), "fp", None)
            .expect_err("应当被拒");
        assert!(matches!(err, CommitError::FoldEndsBeforeHead { .. }));
    }

    #[test]
    fn 事实层不完整时也拒绝出计划() {
        // 意图是"计划不生成，因此 head 一定不变"：这里用错误类型表达该不变量。
        let mut bad = facts();
        bad.rejected = vec![("方案".to_owned(), " ".to_owned())];
        let err = plan(&Token(id(1), 3), &fold(vec![id(1)]), &output("x"), &bad, "fp", None)
            .expect_err("应当被拒");
        assert!(matches!(err, CommitError::BadFacts(_)));
    }

    #[test]
    fn 计划可复用于多次比较() {
        // 计划本身不持有可变状态：同一个计划可以用同一个令牌尝试提交，失败就整体作废。
        let token = Token(id(1), 3);
        let a =
            plan(&token, &fold(vec![id(1)]), &output("x"), &facts(), "fp", None).expect("应当成功");
        let b =
            plan(&token, &fold(vec![id(1)]), &output("x"), &facts(), "fp", None).expect("应当成功");
        assert_eq!(a, b, "同样的输入必须产出同样的计划");
    }
}
