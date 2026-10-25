# ita-worker — 工作区 worker

> 当前状态：空壳。`crates/ita-worker/src/lib.rs` 有模块文档注释、`#![deny(unsafe_code)]`、`#![deny(missing_docs)]`，以及一个空实现 `pub fn run() {}`，没有会话循环逻辑。

## 预定职责

工作区 worker。worker 是无状态的：不持有数据库连接、不持有模型凭据、默认无网络，崩溃后由 server 从事件日志重建。因此本 crate 只放会话循环与协议客户端，不放任何持久化逻辑。

## 依赖

`ita-kernel`、`ita-proto`、`ita-model`（见 `STRUCTURE.md` 依赖边表）。`ita-worker` 绝不依赖 `ita-store`——数据库只能通过 `ita-server` 到达。

## 边界

- 不得在 `run()` 里读取环境变量或打开数据库，二者都会破坏 worker 的无状态前提。
- 读路径：批量 `load_context` RPC。写路径：权威同步 RPC（追加事件、CAS head、租约）；遥测可丢。
- `workspace` 是连接/会话属性，由 server 从连接身份推导，不是请求参数。
