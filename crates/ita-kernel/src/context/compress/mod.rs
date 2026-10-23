//! 语义压缩。
//!
//! 压缩不是"删掉旧消息"，而是造一个帧并把 head 移到新节点——与回退共用同一个原语。因此
//! 这个模块只提供纯函数：何时压缩、压缩哪一段、摘要怎么校验、提交计划长什么样。真正的
//! 编排（发起模型调用、按错误分类重试、执行短事务）在 server 侧的折叠协调器里，不在内核。
//!
//! 边界之所以这样划，是因为内核不能做 IO：一旦这里能调用模型，压缩就再也无法在不联网的
//! 情况下单测，而压缩逻辑恰恰是最需要穷举测试的部分。

pub mod facts;
pub mod fold;
pub mod format;
pub mod recent;
pub mod trigger;

pub use facts::{Facts, FactsError, Failure, FileChange};
pub use fold::{FoldError, FoldPlan, plan as plan_fold};
pub use format::{
    CompressError, CompressOutput, END, PROMPT_VERSION, PROSE_CLOSE, PROSE_OPEN,
    format_instructions, parse_output,
};
pub use recent::{SlimError, SlimToolResult, render as render_slim, slim, slim_all};
pub use trigger::{TriggerDecision, decide};
