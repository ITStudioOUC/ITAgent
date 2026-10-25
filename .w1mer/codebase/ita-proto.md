# ita-proto — 协议信封与本地帧编解码

> 当前状态：空壳。`crates/ita-proto/src/lib.rs` 只有模块文档注释与 `#![forbid(unsafe_code)]`、`#![deny(missing_docs)]`，没有实现代码。

## 预定职责

协议信封与本地帧编解码：字段怎么排、帧怎么切、版本怎么对齐。`Invocation` 信封与版本协商（`08.1`）。

## 依赖

`ita-kernel`、`ita-model`（见 `STRUCTURE.md` 依赖边表）。`ita-proto -> ita-model` 是单向的，`ita-model` 绝不依赖 `ita-proto`。

## 边界

- 不建立连接、不做路由、不持有会话状态。
- 信封里没有 `workspace` 字段，工作区由 server 从连接身份推导，不是请求参数。
