//! 存储层：SQLite、内容寻址存储与快照。
//!
//! 本 crate 属于内核之外的一层：它做 IO、时钟或子进程，因此不追求可复现的纯函数语义，
//! 但必须把副作用限制在自己的边界内，不向上层泄漏。

#![forbid(unsafe_code)]
#![deny(missing_docs)]
