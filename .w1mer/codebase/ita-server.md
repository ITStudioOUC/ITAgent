# ita-server — 唯一数据库写者

> 当前状态：空壳。`crates/ita-server/src/lib.rs` 只有模块文档注释与 `#![deny(unsafe_code)]`、`#![deny(missing_docs)]`，没有实现代码。

## 预定职责

唯一数据库写者：unit 监督、路由、事件日志与模型代理。unit 拓扑类型与依赖无环校验（`07.1`）、状态机与就绪探针（`07.2`）、必需与非必需启动门（`07.3`）、能力集快照（`07.4`）、本地 socket 与连接级工作区绑定（`08.2`）、权威与遥测分流（`08.3`）、group commit（`08.4`）、批量读（`08.5`）。

## 依赖

`ita-kernel`、`ita-store`、`ita-proto`、`ita-model`、`ita-workflow`、`ita-tools`（见 `STRUCTURE.md` 依赖边表）。

## 能力归属

HTTP/TLS 栈只允许在 `ita-server` 使用。`nix`/`rustix` 也在 `ita-server` 用于 unit 监督。SQLite 驱动只允许在 `ita-store`。

## 边界

- 位于内核之外，可以持有 IO、时钟或网络句柄；但这些副作用必须收在本层，向上只暴露普通数据与显式错误。
- 数据库只有 server 能打开；worker 无连接、无 DDL、无 `workspace` 参数。
- 全部模型流量经 server 代理，凭据不进沙箱。
