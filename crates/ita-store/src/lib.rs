//! 存储层。
//!
//! 这一层是唯一接触数据库的地方，且只由 server 进程使用。它对外暴露**操作级**接口而不是
//! SQL：业务代码拿到的是"追加一条事件"这样的方法，而不是一个可以随手拼 `WHERE` 的连接。
//! 这样做的理由有两条：
//!
//! 1. 作用域泄漏（漏写 `WHERE workspace_id`）是跨工作区污染的唯一入口，而它在操作级接口
//!    里不可能发生——工作区是调用时传入的上下文，不是 SQL 片段。
//! 2. 未来换 Postgres 时只需要换实现，不需要改调用方，也不需要让 SQL 方言渗进业务代码。
//!
//! 迁移只允许加表加列，不改不删：事件表是不可变的真相，删列会让历史无法 replay。

mod align;
mod blobs;
mod cas;
pub mod commit;
pub mod db;
mod entries;
pub mod events;
mod fold;
mod gc;
mod heads;
mod marks;
pub mod migrate;
pub mod preview;
pub mod residual;
pub mod restore;
pub mod retention;
pub mod scope;
mod seq;
mod snapshots;
mod squash;
mod units;

pub use align::AlignResult;
pub use commit::{CommittedFrame, FrameBodyError, decode_frame, encode_frame};
pub use db::{Store, StoreError};
pub use entries::SnapshotEntry;
pub use events::{AppendedEvent, Event, NewEvent};
pub use gc::GcResult;
pub use preview::PreviewLimit;
pub use residual::{ResidualEntry, ResidualReason};
pub use retention::{RebaseMode, RebaseRequest};
pub use scope::WorkspaceScope;
pub use snapshots::{CreatedSnapshot, SnapshotInfo};
pub use units::UnitState;
