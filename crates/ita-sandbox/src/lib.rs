//! 沙箱策略与平台实现。
//!
//! 本 crate 属于内核之外的一层：它做 IO、时钟或子进程，因此不追求可复现的纯函数语义，
//! 但必须把副作用限制在自己的边界内，不向上层泄漏。
//!
//! 本 crate 允许在极少数位置使用 `unsafe`（系统调用与平台 FFI），一律逐处 `#[allow]`
//! 并写 `# Safety` 说明成立条件；其余位置由 `deny(unsafe_code)` 拦住。

#![deny(unsafe_code)]
#![deny(missing_docs)]
