//! 唯一数据库写者：unit 监督、路由、事件日志与模型代理。
//!
//! 本 crate 位于内核之外，可以持有 IO、时钟或网络句柄；但这些副作用必须收在本层，
//! 向上只暴露普通数据与显式错误。

#![deny(unsafe_code)]
#![deny(missing_docs)]

pub mod units;
