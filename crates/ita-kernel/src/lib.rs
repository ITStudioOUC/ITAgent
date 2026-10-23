//! ITAgent 的纯逻辑内核。
//!
//! 本 crate 只做数据变换：上下文树的解析、命令文法的分析、租约与权限的策略判定。
//! 它不依赖任何带 IO、时钟、随机数或异步运行时的 crate，因为解析结果必须能按同一份
//! 输入逐字节复现，权限判定必须能被穷举测试。这条边界由依赖检查与 lint 共同强制，
//! 不是约定。
//!
//! 需要存储、网络、子进程或模型调用的逻辑一律放在本 crate 之外，通过显式传入的数据
//! 与本 crate 交互。

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(clippy::disallowed_methods)]
#![deny(clippy::disallowed_types)]

pub mod command;
pub mod context;
