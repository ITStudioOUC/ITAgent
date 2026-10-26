//! 上下文模型：节点树、帧、解析输入与解析结果。
//!
//! 这个模块只做数据变换。给定一份输入，[`resolve`](resolve::resolve) 产出模型实际会看到的
//! 消息数组，同样的输入必然得到逐字节相同的输出。所有需要外部状态的东西——冻结的固定层、
//! 检查点的可达性、预算——都由调用方装进 [`ResolveInput`]，模块本身不查询任何东西。
//!
//! 这条约束不是风格偏好：回退与压缩都只是改变输入，如果解析能自己读库或读时钟，两者就
//! 无法共用同一套语义，replay 也会失真。

pub mod budget;
pub mod compress;
pub mod ids;
pub mod input;
pub mod node;
pub mod read_set;
pub mod resolve;
pub mod tree;

pub use budget::{Budget, BudgetError};
pub use ids::{CheckpointId, EventId, IdError, LeaseId, NodeId, SessionId, WorkspaceId};
pub use input::{CheckpointFacts, InputError, ResolveInput};
pub use node::{BlobRef, Frame, Node, NodeBody, TurnRole};
pub use read_set::{PathKey, ReadRecord, ReadSet, StaleVerdict};
pub use resolve::{Drift, Message, MessageRole, ResolvedContext, Segment, resolve};
pub use tree::{HeadToken, Tree, TreeError};
