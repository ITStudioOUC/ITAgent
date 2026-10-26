---
id: 33
title: 批次 08.2 本地 socket 与连接级工作区绑定的执行计划
state: active
---

# 批次 08.2 本地 socket 与连接级工作区绑定的执行计划

拆分依据给 08.2 的验收是「握手后一次请求得到响应，断开后绑定释放」。核对基线 148b8ba 的已提交代码、08.1 的执行计划（detail 32）、07.4 的执行计划（detail 31）、ADR 0002 与 0013 与 0014 与 0018 之后，结论是：**验收口径成立且不需要改，但它依赖的六件事必须写进计划，否则实现者会在细节上做对类型、做错语义。** 六件事分别是：本地 socket 是 unix domain socket，server 是监听方、worker 是客户端（ADR 0002「每个工具调用前的租约获取、每次事件追加都要过一次本地 socket」）；I/O 模型是同步 `std::net`，每连接一个线程，不引入 tokio（工作区没有 tokio，分层门不挡 `std::net`，08.3 的队列与 08.4 的 group commit 会在连接线程之上叠加，socket 层不需要改）；握手是客户端带 `SessionId` 的消息，server 从 `sessions` 表查会话的工作区并把连接绑定到该工作区（ADR 0002「workspace 是会话属性，不是请求参数；server 从连接身份与握手时的会话信息推导工作区」）；请求信封是 08.1 建的 `Invocation`，响应是 08.1 建的 `Response`，handler 的入参类型只有 `Invocation`，workspace 被捕获在 handler 的 `self` 里而不是出现在参数列表里（编译器保证）；socket 路径是构造参数，配置结构是后续批次的事（ADR 0018 的参数表是内核参数，socket 路径是 server I/O 参数）；08.2 的 handler 是返回固定 `Response` 的桩，真实 handler（事件追加等）是 08.3 及后续批次的事。文件规划 `crates/ita-server/src/ipc/{listener,conn,handshake}.rs` 与现状吻合，08.2 在 `ita-store` 加一个 `session_workspace` 方法（会话到工作区的查询），在 `ita-server` 的 `Cargo.toml` 加 `ita-kernel` 一条依赖（分层门允许表 `ita-server: {ita-kernel, ita-store, ita-proto, ita-model, ita-workflow, ita-tools}` 已登记），零新增外部依赖。

## 结论

1. 08.2 的原始拆分成立。基线 148b8ba 的 `ita-server` 有 `lib.rs` 与 `units` 模块（八个文件），08.1 正在 `ita-proto` 的空壳上建 `envelope.rs` 与 `version.rs`，08.2 在其上加 `ipc` 模块建本地 socket 与连接级工作区绑定。需要改写的是「怎么做」，不是「做什么」。
2. **本地 socket 是 unix domain socket，server 是监听方、worker 是客户端。** ADR 0002 的硬边界是「全系统一个 SQLite 数据库文件，只由 server 进程打开，worker、执行器、工具都不持有数据库连接，所有读写经 server 的 IPC 完成。把写集中到 server 的代价是 server 进入关键路径：每个工具调用前的租约获取、每次事件追加都要过一次本地 socket」。因此 socket 是 unix domain socket（同机，可取 peer 凭据），server 是监听方（`ipc` unit），worker 是客户端。08.2 建 server 侧的监听方与连接级工作区绑定，worker 侧的客户端是后续批次的事。
3. **I/O 模型是同步 `std::net`，每连接一个线程，不引入 tokio。** 工作区没有 tokio（根 `Cargo.toml` 的 `workspace.dependencies` 没有 tokio），分层门不挡 `std::net`（门的注释明确「`std::net`、`tokio::net`、构建脚本内的网络访问、传递依赖与 `pub use` 转出口都不在检查范围内」）。08.2 的验收是「握手后一次请求得到响应，断开后绑定释放」，是单次请求/响应循环。同步的每连接线程模型足够且可重做：08.3 的权威与遥测分流与有界队列、08.4 的 group commit 会在连接线程之上叠加，socket 层不需要改。引入 tokio 是 ADR 没有要求的大依赖决策，与 08.1 守的「零新增外部依赖」纪律冲突。
4. **握手是客户端带 `SessionId` 的消息，server 从 `sessions` 表查会话的工作区并把连接绑定到该工作区。** ADR 0002 的硬边界是「workspace 是会话属性，不是请求参数。server 从连接身份（同机取 peer 凭据）和握手时的会话信息推导工作区，请求信封里不出现 workspace 字段」。握手带 `SessionId`，server 从 `sessions` 表查会话的工作区（`sessions` 表有 `workspace_id` 列）。peer 凭据（SO_PEERCRED）被读取并记录为连接身份，但工作区来自会话。这是「workspace 是会话属性，不是请求参数」的最强形式。
5. **请求信封是 08.1 建的 `Invocation`，响应是 08.1 建的 `Response`，handler 的入参类型只有 `Invocation`。** 08.1 的 `Handler` trait 签名是 `fn handle(&self, invocation: Invocation) -> Result<Response, HandlerError>`。08.1 已提交的 `Response` 类型是 `{ stdout: Vec<u8>, stderr: Vec<u8>, exit_code: i32 }`（工具调用的可观察结果）。workspace 被捕获在 handler 的 `self`（`WorkspaceScope`）里，而不是出现在参数列表里。`ita-proto` 的 `Handler` trait 是泛型的（不引用 `WorkspaceScope`），具体 handler（在 `ita-server`）把 `WorkspaceScope` 放在 `self` 里。workspace 绝不出现在 handler 的参数列表里——它在 handler 的 `self` 里。这是 ADR 0002 的保证。
6. **socket 路径是构造参数，配置结构是后续批次的事。** ADR 0018 的参数表是内核参数（快照保留、租约 TTL 等），socket 路径是 server I/O 参数，不在参数表里。「全部参数都要有配置入口」的总原则仍是好准则，但 08.2 的最小做法是：socket 路径是构造参数。配置结构（持有 socket 路径作为可调参数）是后续批次的事。
7. **08.2 的 handler 是返回固定 `Response` 的桩，真实 handler 是 08.3 及后续批次的事。** 桩 handler 把连接的 `WorkspaceScope` 捕获在 `self` 里，返回固定的 `Response`。真实 handler（事件追加等）是 08.3 及后续批次的事。桩的目的是证明请求/响应循环工作。

## 一、拆分是否仍然成立

核对覆盖了三处：拆分依据里的文件规划、07.4 的执行计划（detail 31）产出的类型与 watch 通道、以及基线 148b8ba 的 `ita-server` 与 `ita-proto` 现状。

| 拆分里的说法 | 现状 | 判断 |
|---|---|---|
| 验收「握手后一次请求得到响应」 | 基线无 socket 或握手代码，`ita-server` 只有 `units` 模块 | 成立，本批在 08.1 之上从零建 |
| 验收「断开后绑定释放」 | 基线无连接状态或连接表 | 成立，本批从零建 |
| 文件 `crates/ita-server/src/ipc/{listener,conn,handshake}.rs` | 三个文件都不存在，`ita-server` 没有 `ipc` 模块 | 成立，08.2 加 `ipc` 模块与三个文件，并在 `lib.rs` 声明 |
| 依赖「08.1」 | 08.1 产出 `Invocation`、`encode`/`decode`、`Handler`、`Response`、`ProtocolVersion`、`SupportedRange`、`negotiate` | 成立，08.2 消费这些 |
| socket 是 unix domain socket，server 是监听方 | ADR 0002「每个工具调用前的租约获取、每次事件追加都要过一次本地 socket」 | 成立，socket 是 unix domain socket，server 是监听方 |
| 握手带 `SessionId`，server 查会话的工作区 | ADR 0002「workspace 是会话属性，不是请求参数」 | 成立，握手带 `SessionId`，server 查会话的工作区 |
| I/O 模型是同步 `std::net`，不引入 tokio | 工作区没有 tokio，分层门不挡 `std::net` | 成立，I/O 模型是同步 `std::net` |

被证伪的是拆分之外的一件事：拆分没有提到握手带 `SessionId` 还是 `WorkspaceId`、I/O 模型是同步还是异步、socket 路径是构造参数还是配置结构。本批裁决：握手带 `SessionId`（server 查会话的工作区），I/O 模型是同步 `std::net`（不引入 tokio），socket 路径是构造参数（配置结构是后续批次的事），见第二节第 2.1、2.2、2.4 条与第八节。

## 二、信封与版本协商之外 08.2 的实际形状

### 2.1 本地 socket 是 unix domain socket，server 是监听方、worker 是客户端

`IpcListener` 是 server 侧的监听方，建在 `std::net::UnixListener` 上。它接受连接，每个连接派生一个线程（每连接一个线程）。监听方持有 socket 路径（构造参数）与 `Store` 句柄（供握手的会话到工作区查询）。worker 是客户端，worker 侧的客户端是后续批次的事。08.2 建 server 侧的监听方与连接级工作区绑定。

socket 路径是构造参数（`PathBuf`）。默认是相对路径（如 `ita.sock`），测试用临时目录。配置结构（持有 socket 路径作为可调参数）是后续批次的事。

### 2.2 握手是客户端带 `SessionId` 的消息，server 查会话的工作区并绑定连接

`Hello { version: ProtocolVersion, session: SessionId }` 是握手消息（客户端到 server）。server 先做版本协商（08.1 的 `negotiate`），不匹配则发 `HelloReject { reason: VersionRejected { supported } }`。匹配则查会话的工作区（新的 store 方法 `session_workspace`），会话不存在则发 `HelloReject { reason: UnknownSession { session } }`。存在则把连接绑定到该工作区并发 `HelloAck { workspace: WorkspaceId }`。

`HelloAck` 带工作区 ID（确认绑定）。这不违反 ADR 0002，因为 ADR 0002 的「请求信封里不出现 workspace 字段」是针对请求信封（`Invocation`），不是握手响应。工作区是从会话推导的（会话属性），握手响应带推导出的工作区 ID 确认绑定。

peer 凭据（SO_PEERCRED）被读取并记录为连接身份（`ConnState` 的 `peer_uid`），但工作区来自会话。这是 ADR 0002 的「server 从连接身份（同机取 peer 凭据）和握手时的会话信息推导工作区」。

### 2.3 连接状态持有绑定的工作区与 peer 凭据

`ConnState { peer_uid: u32, session: SessionId, workspace: WorkspaceId, scope: WorkspaceScope }` 是连接状态。`peer_uid` 从 SO_PEERCRED 读取（连接身份）。`session` 来自握手。`workspace` 从会话推导。`scope` 是 `WorkspaceScope`（由 `Store::scope` 创建）。连接状态存在连接表里：`HashMap<ConnectionId, ConnState>`。连接断开时，连接状态从连接表移除（绑定释放）。

`ConnectionId` 是 `u32` 计数器（连接的索引）。server 给每个新连接分配一个 `ConnectionId`。连接断开时，`ConnState` 从连接表移除。连接表是 `Mutex<HashMap<ConnectionId, ConnState>>`（多线程访问）。

`Store` 句柄不是 `Send`（它持有 `Connection`，而 `Connection` 不是 `Send`）。因此连接线程不能共享 `Store` 句柄。握手需要 `Store` 句柄做会话到工作区查询，所以握手在主线程（`IpcListener` 的线程）做，不在连接线程做。`IpcListener` 的 `accept` 循环是：接受连接，在主线程做握手（用 `Store` 句柄），握手成功则派生一个线程跑请求/响应循环（只传 `WorkspaceScope`，它是 `Clone` 且 `Send`），握手失败则关闭连接。这是同步的 `accept` 循环，对 08.2 的单次请求/响应循环足够。08.3 及后续批次的 `accept` 循环会更复杂（立即派生线程，在连接线程做握手），但 socket 层不需要改。

### 2.4 请求信封是 08.1 建的 `Invocation`，响应是 08.1 建的 `Response`

请求/响应循环是：从 socket 读一个 `Invocation` 信封，解码它，查连接的 `ConnState`（取 `WorkspaceScope`），建一个持有 `WorkspaceScope` 的 handler，调 `handler.handle(invocation)`，编码并发一个 `Response`。handler 在 08.2 是桩。桩 handler 把 `WorkspaceScope` 捕获在 `self` 里，返回固定的 `Response`。真实 handler（事件追加等）是 08.3 及后续批次的事。

08.1 已提交的 `Response` 类型是 `{ stdout: Vec<u8>, stderr: Vec<u8>, exit_code: i32 }`（工具调用的可观察结果：标准输出、标准错误与退出码）。桩 handler 返回固定的 `Response`（如 `Response { stdout: vec![], stderr: vec![], exit_code: 0 }`），不真正运行工具。08.1 的 `Handler` trait 签名是 `fn handle(&self, invocation: Invocation) -> Result<Response, HandlerError>`，`HandlerError` 是 `PolicyDenied`/`LeaseDenied`/`Failed` 三变体。桩 handler 返回 `Ok(Response)`，不返回 `Err`。

### 2.5 I/O 模型是同步 `std::net`，每连接一个线程，不引入 tokio

工作区没有 tokio（根 `Cargo.toml` 的 `workspace.dependencies` 没有 tokio），分层门不挡 `std::net`。08.2 的验收是「握手后一次请求得到响应，断开后绑定释放」，是单次请求/响应循环。同步的每连接线程模型足够且可重做：08.3 的权威与遥测分流与有界队列、08.4 的 group commit 会在连接线程之上叠加，socket 层不需要改。引入 tokio 是 ADR 没有要求的大依赖决策，与 08.1 守的「零新增外部依赖」纪律冲突。

### 2.6 与 07.4 watch 通道的关系

07.4 的 `CapabilityWatcher` 是进程内的 watch 通道，不是 IPC。08.2 的本地 socket 是真实 socket，能力推送（ADR 0013「能力集合的推送与工具目录的追加通知是同一套机制」）以后会在 08.3 及后续批次接到这条 socket 上。08.2 不碰 watch 通道，只建本地 socket 与连接级工作区绑定。07.4 的 watch 通道是进程内同步通知，08.x 把通道接到真实 socket 时，能力集变更与工具目录追加走同一套信封机制（ADR 0013）。

## 三、ADR 对 08.2 的硬要求

与 08.2 相关的决策分布在 ADR 0002、0013、0014 与 0018，逐条对照如下。

| 要求 | 出处 | 08.2 怎么落地 |
|---|---|---|
| workspace 是会话属性，不是请求参数 | 0002 决策段 | 握手带 `SessionId`，server 查会话的工作区；请求信封（`Invocation`）没有 workspace 字段 |
| 请求信封里不出现 workspace 字段，由接口形状保证 | 0002 决策段 | `Invocation` 结构体没有 workspace 字段，handler 签名没有 workspace 参数，workspace 被捕获在 handler 的 `self` 里 |
| server 从连接身份与握手时的会话信息推导工作区 | 0002 决策段 | 握手带 `SessionId`，server 从 `sessions` 表查会话的工作区；peer 凭据（SO_PEERCRED）被读取并记录为连接身份 |
| 消息分两类：权威写与遥测 | 0002 决策段 | 08.2 只定义 socket 与握手，两类消息的分流是 08.3 的事 |
| server 内部是单写线程加有界队列加批量提交 | 0002 决策段 | 08.2 是单次请求/响应循环，单写线程与有界队列是 08.3 与 08.4 的事 |
| 能力集合的推送与工具目录的追加通知是同一套机制 | 0013 决策段 | 08.2 不碰 watch 通道，能力推送在 08.3 及后续批次接到真实 socket |
| `ipc` unit 是最小拓扑的一部分（store 到 router 到 ipc，必需） | 0013 决策段 | 08.2 建 `ipc` unit（socket 监听方） |
| 执行器是独立进程，不是 server 内的 unit | 0014 决策段 | 08.2 的 unit 是 server 内部子系统，不含执行器 |
| 全部参数都要有配置入口，读取路径统一走配置结构 | 0018 第九节 | socket 路径在 08.2 是构造参数，配置结构是后续批次的事（ADR 0018 的参数表是内核参数，socket 路径是 server I/O 参数） |
| 握手后一次请求得到响应，断开后绑定释放 | ROADMAP 08 行验收 | 握手带 `SessionId`，server 查会话的工作区并绑定连接；一次请求得到响应；连接断开时连接状态从连接表移除 |

三处需要特别说清的核实结果。

**第一，`ita-server` 基线有 `units` 模块，08.2 在其上加 `ipc` 模块。** 基线 148b8ba 的 `crates/ita-server/src/` 有 `lib.rs`（只有 `pub mod units;`）与 `units/` 模块（八个文件：`mod.rs`（`UnitId`、`DepKind`）、`topology.rs`（`Topology`、`find_cycle`、`startup_order`、`CycleError`）、`state.rs`（`UnitState`、`UnitEvent`、`next`）、`probe.rs`（`ProbeResult`、`ReadinessProbe`）、`supervisor.rs`（`Supervisor`）、`plan.rs`（`StartupPlan`、`StartupPlanError`）、`activation.rs`（`ActivationResult`、`activate`）、`capability.rs`（`CapabilitySet`、`build_capability_set`）、`watch.rs`（`CapabilityUpdate`、`CapabilityWatcher`））。08.2 在其上加 `ipc` 模块（`mod.rs`、`listener.rs`、`handshake.rs`、`conn.rs`），并在 `lib.rs` 声明。08.2 不修改 `units` 模块，只消费 `UnitId`。

**第二，`ita-proto` 基线是空壳，08.1 在其上加 `envelope.rs` 与 `version.rs`。** 基线 148b8ba 的 `crates/ita-proto/` 只有 `Cargo.toml`（`[dependencies]` 为空）与 `src/lib.rs`（模块文档注释与 `#![forbid(unsafe_code)]`、`#![deny(missing_docs)]`）。08.1 在其上加 `envelope.rs` 与 `version.rs`。08.2 消费 08.1 的 `Invocation`、`encode`/`decode`、`Handler`、`Response`、`ProtocolVersion`、`SupportedRange`、`negotiate`。

**第三，I/O 模型是同步 `std::net`，不引入 tokio。** 工作区没有 tokio（根 `Cargo.toml` 的 `workspace.dependencies` 没有 tokio），分层门不挡 `std::net`。08.2 的验收是「握手后一次请求得到响应，断开后绑定释放」，是单次请求/响应循环。同步的每连接线程模型足够且可重做。引入 tokio 是 ADR 没有要求的大依赖决策，与 08.1 守的「零新增外部依赖」纪律冲突。

## 四、改写后的验收标准

每条都能写成一个测试，括号里写明它守的是什么。

1. **握手带 `SessionId`，server 查会话的工作区并绑定连接。** 注册一个工作区（`WorkspaceId`），建一个会话（属于该工作区的 `SessionId`），在临时 socket 路径上启动 server，客户端连接，发 `Hello { version, session }`，断言 server 发 `HelloAck { workspace }` 且 `workspace` 等于会话的工作区。守的是「握手带 `SessionId`，server 查会话的工作区并绑定连接」。
2. **握手后一次请求得到响应。** 握手成功后，发一个 `Invocation` 信封（08.1 的 `encode`），断言 server 发一个 `Response`（08.1 的类型）且解码后的 `Response` 合法。守的是「握手后一次请求得到响应」。
3. **断开后绑定释放。** 握手成功后，客户端断开，断言 server 的连接表里不再有该连接的 `ConnState`（绑定释放）。守的是「断开后绑定释放」。
4. **版本不匹配在握手被拒并回显支持范围。** 发一个 `Hello { version: 5, session }`，支持范围是 `min=1, max=4`，断言 server 发 `HelloReject { reason: VersionRejected { supported } }` 且 `supported` 是 `min=1, max=4`。守的是「版本不匹配在握手被拒并回显支持范围」。
5. **未知会话在握手被拒。** 发一个 `Hello { version, session }`，`session` 不存在，断言 server 发 `HelloReject { reason: UnknownSession { session } }`。守的是「未知会话在握手被拒」。
6. **handler 的入参类型只有 `Invocation`，没有 workspace 参数。** 定义一个 `Handler` 实现（桩 handler），签名是 `fn handle(&self, invocation: Invocation) -> Result<Response, HandlerError>`，断言 `Invocation` 结构体里没有 workspace 字段（编译期保证），handler 的 `self` 持有 `WorkspaceScope`（workspace 被捕获在 handler 的 `self` 里，不是参数列表里）。守的是「handler 的入参类型只有 `Invocation`，没有 workspace 参数」。
7. **连接状态持有绑定的工作区与 peer 凭据。** 握手成功后，断言连接的 `ConnState` 有 `workspace`（等于会话的工作区）与 `peer_uid`（从 SO_PEERCRED 读取）。守的是「连接状态持有绑定的工作区与 peer 凭据」。
8. **socket 路径是构造参数。** 用一个临时 socket 路径构造 `IpcListener`，断言监听方在那个路径上监听。守的是「socket 路径是构造参数」。
9. **08.2 不引入 tokio。** 断言 `ita-server` 的 `Cargo.toml` 只加 `ita-kernel` 一条依赖，零新增外部依赖，`listener.rs`、`handshake.rs`、`conn.rs` 只用 `std::net` 与 08.1 的类型。守的是「08.2 不引入 tokio」。
10. **08.2 不碰 watch 通道。** 断言 08.2 不修改 `units` 模块，只消费 `UnitId`。守的是「08.2 不碰 watch 通道」。

第 1 条守的是「握手带 `SessionId`，server 查会话的工作区并绑定连接」。第 2 条守的是「握手后一次请求得到响应」。第 3 条守的是「断开后绑定释放」。第 4 条守的是「版本不匹配在握手被拒并回显支持范围」。第 5 条守的是「未知会话在握手被拒」。第 6 条守的是「handler 的入参类型只有 `Invocation`，没有 workspace 参数」。第 7 条守的是「连接状态持有绑定的工作区与 peer 凭据」。第 8 条守的是「socket 路径是构造参数」。第 9 条守的是「08.2 不引入 tokio」。第 10 条守的是「08.2 不碰 watch 通道」。

## 五、文件清单

| 文件 | 动作 | 内容 |
|---|---|---|
| `crates/ita-server/src/ipc/mod.rs` | 新建 | 模块声明，再导出关键类型（`IpcListener`、`Hello`、`HelloAck`、`HelloReject`、`ConnState`、`ConnectionTable`） |
| `crates/ita-server/src/ipc/listener.rs` | 新建 | `IpcListener` 结构（socket 路径、`Store` 句柄、`ConnectionTable`）、`accept`（接受连接并派生线程）、内联 `#[cfg(test)]` 测试 |
| `crates/ita-server/src/ipc/handshake.rs` | 新建 | `Hello`/`HelloAck`/`HelloReject` 类型、`HelloRejectReason` 枚举（`VersionRejected`/`UnknownSession`）、握手逻辑（版本协商加会话到工作区查询）、内联 `#[cfg(test)]` 测试 |
| `crates/ita-server/src/ipc/conn.rs` | 新建 | `ConnState` 结构（`peer_uid`/`session`/`workspace`/`scope`）、`ConnectionTable` 结构（`Mutex<HashMap<ConnectionId, ConnState>>`）、请求/响应循环（读 `Invocation`、解码、调 handler、发 `Response`）、桩 handler、内联 `#[cfg(test)]` 测试 |
| `crates/ita-server/src/lib.rs` | 修改 | 声明 `ipc` 模块 |
| `crates/ita-server/Cargo.toml` | 修改 | 加 `ita-kernel` 一条依赖（分层门允许表 `ita-server: {ita-kernel, ita-store, ita-proto, ita-model, ita-workflow, ita-tools}` 已登记），零新增外部依赖 |
| `crates/ita-store/src/db.rs` | 修改 | 加 `session_workspace(session_id) -> Result<WorkspaceId, StoreError>` 方法（会话到工作区的查询）、内联 `#[cfg(test)]` 测试 |
| `crates/ita-server/src/units/**` | **不动** | 07.1 至 07.4 的产出，08.2 只消费 `UnitId` |
| `crates/ita-proto/**` | **不动** | 08.1 的产出，08.2 只消费 `Invocation`、`encode`/`decode`、`Handler`、`Response`、`ProtocolVersion`、`SupportedRange`、`negotiate` |

`listener.rs` 的 `IpcListener` 是公开结构，`accept` 是公开方法。`handshake.rs` 的 `Hello`/`HelloAck`/`HelloReject` 是公开类型，握手逻辑是公开函数。`conn.rs` 的 `ConnState` 是公开结构，`ConnectionTable` 是公开结构，请求/响应循环是公开方法。测试内联在各文件的 `#[cfg(test)] mod tests` 里，不另开 `tests/` 集成测试文件——纯逻辑模块的内联测试是 Rust 惯例，且 `ita-server` 基线没有 `tests/` 目录。`session_workspace` 方法加在 `ita-store` 的 `db.rs`，从 `sessions` 表查会话的工作区（`sessions` 表有 `workspace_id` 列），会话不存在返回 `StoreError::UnknownSession`。

## 六、实现顺序与每步验证

分四步，前三步各一个提交，第四步是全仓验证不单独提交。同一批次共用同一个版本号，版本只在最后一次提交前推进一次。顺序按「能独立验证的先做」排，`session_workspace` store 方法是最便宜的定形，`ipc` 模块的 socket 与握手是核心算法。

| 步骤 | 做什么 | 验证方式 |
|---|---|---|
| 一 | 在 `ita-store` 的 `db.rs` 加 `session_workspace` 方法，写内联测试 | `cargo nextest run -p ita-store -E 'test~session_workspace'`，覆盖会话到工作区查询、未知会话返回错误 |
| 二 | 写 `ipc/handshake.rs`（`Hello`、`HelloAck`、`HelloReject`、`HelloRejectReason`、握手逻辑）与内联测试，更新 `lib.rs` 声明 `ipc` 模块 | `cargo nextest run -p ita-server -E 'test~handshake'`，覆盖第四节第 4、5 条（版本不匹配被拒并回显支持范围、未知会话被拒） |
| 三 | 写 `ipc/listener.rs`（`IpcListener`、`accept`）与 `ipc/conn.rs`（`ConnState`、`ConnectionTable`、请求/响应循环、桩 handler）与内联测试，更新 `lib.rs` 声明 `ipc` 模块，在 `Cargo.toml` 加 `ita-kernel` 依赖 | `cargo nextest run -p ita-server`，此时测试覆盖第四节全部十条验收 |
| 四 | 补全仓验证与文档 | `bash .w1mer/scripts/verify.sh all`，全仓验证通过 |

测试函数名用英文 snake_case，上方一行 `///` 用中文写这个用例守的行为。这条规则在 06.2.2 里入册，断言消息仍然用中文。

## 七、风险：容易做成测试通过但语义错的地方

| 风险 | 为什么测试会照样通过 | 对策 |
|---|---|---|
| 握手带 `WorkspaceId` 而不是 `SessionId`（客户端声明工作区） | 编译通过，但「workspace 是会话属性，不是请求参数」的验收被偷换 | 第四节第 1 条：断言握手带 `SessionId`，server 查会话的工作区 |
| handler 签名有 workspace 参数（handler 能表达 workspace） | 编译通过，但「handler 的入参类型只有 `Invocation`，没有 workspace 参数」的验收被偷换 | 第四节第 6 条：断言 `Invocation` 结构体里没有 workspace 字段，handler 签名没有 workspace 参数，编译器保证 |
| I/O 模型是异步 tokio（引入 tokio） | 编译通过，但「08.2 不引入 tokio」的纪律被破坏 | 08.2 不引入 tokio，I/O 模型是同步 `std::net` |
| workspace 从请求里读（`Invocation` 有 workspace 字段） | 编译通过，但「workspace 是会话属性，不是请求参数」被破坏 | 第四节第 6 条：断言 `Invocation` 结构体里没有 workspace 字段 |
| 断开后绑定不释放（连接状态留在连接表里） | 正常路径通过，连接断开时才暴露 | 第四节第 3 条：断言 server 的连接表里不再有该连接的 `ConnState` |
| socket 路径是常量而不是构造参数 | 编译通过，但「全部参数都要有配置入口」被破坏 | socket 路径是构造参数，配置结构是后续批次的事 |
| 版本协商不在握手做（版本不校验） | 正常路径通过，版本不匹配时才暴露 | 第四节第 4 条：断言版本不匹配在握手被拒并回显支持范围 |
| 未知会话不拒绝（连接绑定到幻影工作区） | 正常路径通过，会话不存在时才暴露 | 第四节第 5 条：断言未知会话在握手被拒 |
| 连接状态不持有 peer 凭据（连接身份丢失） | 正常路径通过，但「server 从连接身份与握手时的会话信息推导工作区」被破坏 | 第四节第 7 条：断言连接状态持有 `peer_uid`（从 SO_PEERCRED 读取） |
| watch 通道混进 08.2（能力推送接到真实 socket） | 编译通过，但「这一批只有一个目的」失效 | 08.2 不碰 watch 通道，能力推送在 08.3 及后续批次接到真实 socket |

还有一条不属于「测试通过但语义错」，但会直接影响可复审性的：I/O 模型是同步 `std::net`，不引入 tokio。工作区没有 tokio，分层门不挡 `std::net`。08.2 的验收是「握手后一次请求得到响应，断开后绑定释放」，是单次请求/响应循环。同步的每连接线程模型足够且可重做。引入 tokio 是 ADR 没有要求的大依赖决策，与 08.1 守的「零新增外部依赖」纪律冲突。这条职责分离是「这一批只有一个目的」的前提，不靠调用方记得「I/O 模型是同步 `std::net`」。

## 八、已裁决的分歧点

五处分歧点由 Explorer 直接定形，理由充分。

| 分歧 | 两种做法 | 定形 |
|---|---|---|
| 握手带 `SessionId` 还是 `WorkspaceId` | 甲：带 `SessionId`（server 查会话的工作区）。乙：带 `WorkspaceId`（客户端声明工作区） | **直接定形选甲。** 理由见第二节第 2.2 条。ADR 0002 说「workspace 是会话属性，不是请求参数。server 从连接身份与握手时的会话信息推导工作区」。会话决定工作区，握手带会话，server 从会话推导工作区。带 `WorkspaceId` 会让客户端声明工作区，是「workspace 是请求参数」，不是「workspace 是会话属性」 |
| I/O 模型是同步 `std::net` 还是异步 tokio | 甲：同步 `std::net`，每连接一个线程（不引入 tokio）。乙：异步 tokio | **直接定形选甲。** 理由见第二节第 2.5 条。工作区没有 tokio，分层门不挡 `std::net`。08.2 的验收是单次请求/响应循环。同步的每连接线程模型足够且可重做。引入 tokio 是 ADR 没有要求的大依赖决策，与 08.1 守的「零新增外部依赖」纪律冲突 |
| socket 路径是构造参数还是配置结构 | 甲：构造参数（配置结构是后续批次的事）。乙：配置结构（ADR 0018「全部参数都要有配置入口」） | **直接定形选甲。** 理由见第二节第 2.1 条。ADR 0018 的参数表是内核参数（快照保留、租约 TTL 等），socket 路径是 server I/O 参数，不在参数表里。「全部参数都要有配置入口」的总原则仍是好准则，但 08.2 的最小做法是：socket 路径是构造参数。配置结构（持有 socket 路径作为可调参数）是后续批次的事 |
| `HelloAck` 带 `WorkspaceId` 还是不带 | 甲：带 `WorkspaceId`（确认绑定）。乙：不带 `WorkspaceId`（客户端已经知道工作区） | **直接定形选甲。** 理由见第二节第 2.2 条。ADR 0002 的「请求信封里不出现 workspace 字段」是针对请求信封（`Invocation`），不是握手响应。工作区是从会话推导的（会话属性），握手响应带推导出的工作区 ID 确认绑定。这不违反 ADR 0002 |
| 08.2 的 handler 是桩还是真实 handler | 甲：返回固定 `Response` 的桩（真实 handler 是 08.3 及后续批次的事）。乙：真实 handler（事件追加等） | **直接定形选甲。** 理由见第二节第 2.4 条。08.2 的验收是「握手后一次请求得到响应，断开后绑定释放」，是单次请求/响应循环。桩 handler 足够证明请求/响应循环工作。真实 handler（事件追加等）是 08.3 及后续批次的事 |

## 九、本批不做的事

为了让复审能用「这一批只有一个目的」来判断，以下几项明确不属于 08.2：权威与遥测分流、有界队列与背压（08.3）；group commit（08.4）；批量读 `load_context`（08.5）；租约的获取与释放（09.x）；执行器进程（0014，独立进程，不是 unit）；工作流节点类型（`ita-workflow`，与 unit 类型分开）；事件日志与模型代理（08.x）；异步执行（08.2 是同步 `std::net`，不引入 tokio）；版本号落库（进程内计数器，不新增迁移）；工具目录追加本身（工具系统批次，07.4 只保证 watch 通道可承载）；worker 侧客户端（worker 侧的协议客户端是后续批次的事）；配置结构（socket 路径是构造参数，配置结构是后续批次的事）。

## 十、Explorer 核对与补强（基线 148b8ba）

本节的核对对象是批次基线 148b8ba 的已提交代码，不是实现者的工作树。结论是：十条验收对基线全部成立，本地 socket 与连接级工作区绑定的口径都没有被证伪，需要改写的只是「怎么做」与五处待裁决项的定形。下面逐条记录核对结果与五处补强。

### 10.1 `ita-server` 现状核对：`units` 模块有八个文件，08.2 加 `ipc` 模块

基线 148b8ba 的 `crates/ita-server/src/` 有 `lib.rs`（只有 `pub mod units;`）与 `units/` 模块（八个文件：`mod.rs`（`UnitId`、`DepKind`）、`topology.rs`（`Topology`、`find_cycle`、`startup_order`、`CycleError`）、`state.rs`（`UnitState`、`UnitEvent`、`next`）、`probe.rs`（`ProbeResult`、`ReadinessProbe`）、`supervisor.rs`（`Supervisor`）、`plan.rs`（`StartupPlan`、`StartupPlanError`）、`activation.rs`（`ActivationResult`、`activate`）、`capability.rs`（`CapabilitySet`、`build_capability_set`）、`watch.rs`（`CapabilityUpdate`、`CapabilityWatcher`））。08.2 在其上加 `ipc` 模块（`mod.rs`、`listener.rs`、`handshake.rs`、`conn.rs`），并在 `lib.rs` 声明。08.2 不修改 07.1 的 `topology.rs`、07.2 的 `state.rs`/`probe.rs`/`supervisor.rs`、07.3 的 `activation.rs`/`plan.rs`、07.4 的 `capability.rs`/`watch.rs`，只消费 `UnitId`。

### 10.2 `ita-proto` 现状核对：空壳，08.1 加两个文件

基线 148b8ba 的 `crates/ita-proto/` 只有 `Cargo.toml`（`[dependencies]` 为空）与 `src/lib.rs`（模块文档注释与 `#![forbid(unsafe_code)]`、`#![deny(missing_docs)]`）。08.1 在其上加 `envelope.rs` 与 `version.rs`，并在 `lib.rs` 声明两个模块。08.1 在 `Cargo.toml` 加 `ita-kernel` 一条依赖（分层门允许表 `ita-proto: {ita-kernel, ita-model}` 已登记），零新增外部依赖。08.2 不修改内核的 `command/` 与 `context/` 模块，只消费 08.1 的 `Invocation`、`encode`/`decode`、`Handler`、`Response`、`ProtocolVersion`、`SupportedRange`、`negotiate`。

### 10.3 `ita-store` 现状核对：`sessions` 表有 `workspace_id` 列，08.2 加 `session_workspace`

基线 148b8ba 的迁移文件 `0001_core.sql` 有 `sessions` 表（`id TEXT PRIMARY KEY`、`workspace_id TEXT NOT NULL REFERENCES workspaces(id)`、`parent_session`、`fork_node`、`head_node`、`head_generation`、`created_at`）。`sessions` 表有 `workspace_id` 列，会话到工作区的查询是 `SELECT workspace_id FROM sessions WHERE id = ?1`。08.2 在 `ita-store` 的 `db.rs` 加 `session_workspace(session_id) -> Result<WorkspaceId, StoreError>` 方法，从 `sessions` 表查会话的工作区。会话不存在返回 `StoreError::UnknownSession`。

### 10.4 08.1 产出类型核对：`Invocation`、`encode`/`decode`、`Handler`、`Response`、`ProtocolVersion`、`SupportedRange`、`negotiate`

08.1 已提交（提交 `5943ecb` 与 `bcb2176`）产出 `Invocation`（`version`/`caller`/`session`/`cwd`/`argv`/`stdin`/`lease_id` 七字段，无 workspace 字段）、`Caller`（名义类型）、`encode`/`decode`（长度前缀文本编解码）、`Handler` trait（`fn handle(&self, invocation: Invocation) -> Result<Response, HandlerError>`）、`Response`（`stdout: Vec<u8>`/`stderr: Vec<u8>`/`exit_code: i32`，工具调用的可观察结果）与 `HandlerError`（`PolicyDenied`/`LeaseDenied`/`Failed` 三变体）类型、`ProtocolVersion`（名义类型，`new(major)` 与 `major()`）、`SupportedRange`（`min`/`max` 两字段，`new(min, max)` 构造期校验下限不大于上限）、`negotiate` 纯函数、`CURRENT_PROTOCOL_VERSION`（版本 1）与 `DEFAULT_SUPPORTED_RANGE`（只接受当前版本）。08.2 的 `Hello`/`HelloAck`/`HelloReject` 与 `Invocation`/`Response` 是同一 `ipc` 模块下的不同类型，互不混用。08.2 消费 08.1 的 `Invocation` 信封与 `Response` 类型，不重新实现信封与版本协商。

### 10.5 五处补强

**第一，本地 socket 是 unix domain socket，server 是监听方、worker 是客户端。** ADR 0002 的硬边界是「全系统一个 SQLite 数据库文件，只由 server 进程打开，worker、执行器、工具都不持有数据库连接，所有读写经 server 的 IPC 完成。把写集中到 server 的代价是 server 进入关键路径：每个工具调用前的租约获取、每次事件追加都要过一次本地 socket」。因此 socket 是 unix domain socket（同机，可取 peer 凭据），server 是监听方（`ipc` unit），worker 是客户端。08.2 建 server 侧的监听方与连接级工作区绑定，worker 侧的客户端是后续批次的事。

**第二，握手是客户端带 `SessionId` 的消息，server 从 `sessions` 表查会话的工作区并把连接绑定到该工作区。** ADR 0002 的硬边界是「workspace 是会话属性，不是请求参数。server 从连接身份（同机取 peer 凭据）和握手时的会话信息推导工作区，请求信封里不出现 workspace 字段」。握手带 `SessionId`，server 从 `sessions` 表查会话的工作区（`sessions` 表有 `workspace_id` 列）。peer 凭据（SO_PEERCRED）被读取并记录为连接身份，但工作区来自会话。这是「workspace 是会话属性，不是请求参数」的最强形式。

**第三，I/O 模型是同步 `std::net`，每连接一个线程，不引入 tokio。** 工作区没有 tokio（根 `Cargo.toml` 的 `workspace.dependencies` 没有 tokio），分层门不挡 `std::net`（门的注释明确「`std::net`、`tokio::net`、构建脚本内的网络访问、传递依赖与 `pub use` 转出口都不在检查范围内」）。08.2 的验收是「握手后一次请求得到响应，断开后绑定释放」，是单次请求/响应循环。同步的每连接线程模型足够且可重做：08.3 的权威与遥测分流与有界队列、08.4 的 group commit 会在连接线程之上叠加，socket 层不需要改。

**第四，请求信封是 08.1 建的 `Invocation`，响应是 08.1 建的 `Response`，handler 的入参类型只有 `Invocation`。** 08.1 的 `Handler` trait 签名是 `fn handle(&self, invocation: Invocation) -> Result<Response, HandlerError>`。workspace 被捕获在 handler 的 `self`（`WorkspaceScope`）里，而不是出现在参数列表里。`ita-proto` 的 `Handler` trait 是泛型的（不引用 `WorkspaceScope`），具体 handler（在 `ita-server`）把 `WorkspaceScope` 放在 `self` 里。workspace 绝不出现在 handler 的参数列表里——它在 handler 的 `self` 里。这是 ADR 0002 的保证。

**第五，socket 路径是构造参数，配置结构是后续批次的事。** ADR 0018 的参数表是内核参数（快照保留、租约 TTL 等），socket 路径是 server I/O 参数，不在参数表里。「全部参数都要有配置入口」的总原则仍是好准则，但 08.2 的最小做法是：socket 路径是构造参数。配置结构（持有 socket 路径作为可调参数）是后续批次的事。
