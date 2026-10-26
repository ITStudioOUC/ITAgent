---
id: 32
title: 批次 08.1 Invocation 信封与版本协商的执行计划
state: active
---

# 批次 08.1 Invocation 信封与版本协商的执行计划

拆分依据给 08.1 的验收是「同版本往返一致，不兼容版本明确报错，handler 无法表达 workspace」，主要文件 `crates/ita-proto/src/{envelope,version}.rs`，依赖 02 与 04。核对基线 34730b2 的已提交代码、07.4 的执行计划（detail 31）、ADR 0002 与 0014 与 0018 之后，结论是：**验收口径成立且不需要改，但它依赖的六件事必须写进计划，否则实现者会在细节上做对类型、做错语义。** 六件事分别是：信封是 server 与 worker 之间 IPC 消息的封装，落在 `ita-proto`，带协议版本字段（ADR 0002 明确「请求信封里不出现 workspace 字段，这条由接口形状保证：业务处理函数的入参类型里根本没有这个字段」）；版本协商是 server 收到请求时校验协议版本，不匹配则拒绝并回显支持范围（支持范围是可调参数，读取统一走配置结构，ADR 0018）；信封往返是长度前缀文本编解码，同版本编码再解码必须逐字节等价（06.4 已把帧正文落成长度前缀文本，08.1 的信封编码与它同型，不引入 serde）；handler 是 server 侧处理函数，入参类型只有 `Invocation`，没有 workspace 参数，这条由编译器保证而不是靠调用方记得；信封的 `argv` 是内核的 `Command` AST（`program`/`args`/`redirects`/`heredoc`），`stdin` 是 heredoc 正文之外的独立输入流，`lease id` 是嵌套调用继承外层租约的可选标识（ADR 0014）；08.1 不建 socket、不引入 tokio、不碰网络，真实 socket 与连接级工作区绑定是 08.2 的事，07.4 的进程内 watch 通道在 08.x 接到真实 socket 时复用本批的信封。文件规划 `crates/ita-proto/src/{envelope,version}.rs` 与现状吻合，08.1 在 `ita-proto` 的 `Cargo.toml` 加 `ita-kernel` 一条依赖（分层门允许表 `ita-proto: {ita-kernel, ita-model}` 已登记），零新增外部依赖。

## 结论

1. 08.1 的原始拆分成立。基线 34730b2 的 `ita-proto` 是空壳（`lib.rs` 只有模块文档注释与 `#![forbid(unsafe_code)]`、`#![deny(missing_docs)]`，`Cargo.toml` 的 `[dependencies]` 为空），08.1 在其上从零建 `envelope.rs` 与 `version.rs`。需要改写的是「怎么做」，不是「做什么」。
2. **信封是 server 与 worker 之间 IPC 消息的封装，落在 `ita-proto`，不进内核。** ADR 0002 的硬边界是「workspace 是会话属性，不是请求参数；server 从连接身份与握手时的会话信息推导工作区，请求信封里不出现 workspace 字段。这条由接口形状保证：业务处理函数的入参类型里根本没有这个字段」。因此信封类型必须独立于内核的上下文树类型，而内核的职责域是「上下文树的解析、命令文法的分析、租约与权限的策略判定」（`lib.rs` 文档注释），信封不在其中。07.1 已把 `UnitId` 落在 `ita-server`，07.4 已把 `CapabilitySet`/`CapabilityWatcher` 落在同一模块，08.1 的信封落在 `ita-proto`，维持不改。
3. **版本协商是 server 收到请求时校验协议版本，不匹配则拒绝并回显支持范围。** 信封带协议版本字段，server 端持有一个支持范围（可调参数，读取走配置结构，ADR 0018 第九节「全部参数都要有配置入口」）。协商是纯函数：输入是请求版本与支持范围，输出是接受或拒绝（带支持范围）。拒绝时回显支持范围，让调用方看见错在哪。
4. **信封往返是长度前缀文本编解码，同版本编码再解码必须逐字节等价。** 06.4 已把帧正文落成长度前缀文本（写进 `nodes.inline_body`，不引入 `serde_json`），08.1 的信封编码与它同型：每个字段先写长度再写内容，解码按长度切回。往返一致是「编码再解码等于原值」，不是「编码再解码等于某个规范化形式」。08.1 不引入 serde，不新增外部依赖。
5. **handler 是 server 侧处理函数，入参类型只有 `Invocation`，没有 workspace 参数。** 这条由编译器保证：handler 的签名是 `fn handle(&self, invocation: Invocation) -> Result<Response, HandlerError>`，`Invocation` 结构体里没有 workspace 字段，因此 handler 拿不到 workspace。workspace 由 server 从连接身份推导（08.2 的握手），不进 handler 签名。这是 ADR 0002「业务处理函数的入参类型里根本没有这个字段」的落地。
6. **08.1 不建 socket、不引入 tokio、不碰网络。** 08.1 在 08.2 之前，真实 socket 与连接级工作区绑定是 08.2 的事。07.4 的进程内 watch 通道是同步的进程内通知，08.x 会把这条通道接到真实 socket，届时复用本批的信封。因此 08.1 不引入 tokio、不建 socket、不碰网络，信封与版本协商是纯数据变换。

## 一、拆分是否仍然成立

核对覆盖了三处：拆分依据里的文件规划、07.4 的执行计划（detail 31）产出的类型与 watch 通道、以及基线 34730b2 的 `ita-proto` 现状。

| 拆分里的说法 | 现状 | 判断 |
|---|---|---|
| 验收「同版本往返一致」 | 基线无信封或编解码代码，`ita-proto` 是空壳 | 成立，本批在空壳上从零建 |
| 验收「不兼容版本明确报错」 | 基线无版本协商代码 | 成立，本批从零建 |
| 验收「handler 无法表达 workspace」 | 基线无 handler 类型，`Invocation` 不存在 | 成立，本批建 `Invocation` 与 handler 签名 |
| 文件 `crates/ita-proto/src/{envelope,version}.rs` | 两个文件都不存在，`ita-proto` 只有 `lib.rs` | 成立，08.1 加两个文件并在 `lib.rs` 声明 |
| 依赖「02,04」 | 02 是仓库骨架（`ita-proto` 空壳已建），04 是上下文模型（`SessionId` 等 id 类型在内核） | 成立，08.1 消费内核的 id 类型与 `Command` AST |
| 信封是 IPC 消息封装，带协议版本 | ADR 0002「请求信封里不出现 workspace 字段」 | 成立，信封落 `ita-proto`，带版本字段 |
| 版本协商是校验协议版本，不匹配拒绝并回显支持范围 | ADR 0018「全部参数都要有配置入口」 | 成立，支持范围是可调参数，读取走配置结构 |
| 08.1 不建 socket、不引入 tokio | 08.1 在 08.2 之前，无 socket | 成立，信封与版本协商是纯数据变换 |

被证伪的是拆分之外的一件事：拆分没有提到信封的 `argv` 是内核的 `Command` AST 还是裸命令串、`stdin` 是 heredoc 正文还是独立输入流、`lease id` 是内核的 `LeaseId` 名义类型还是裸 ULID 串。本批裁决：`argv` 是内核的 `Command` AST（`program`/`args`/`redirects`/`heredoc`），`stdin` 是 heredoc 正文之外的独立输入流（`Option<Vec<u8>>`），`lease id` 是内核的 `LeaseId` 名义类型（08.1 在内核 `ids.rs` 用既有 `id_type!` 宏加 `LeaseId`，与 `NodeId`/`SessionId` 同型），见第二节第 2.1 条与第八节。

## 二、信封与版本协商的实际形状

### 2.1 信封是 server 与 worker 之间 IPC 消息的封装

`Invocation` 是信封类型，字段：`version: ProtocolVersion`（协议版本）、`caller: Caller`（调用者标识，worker 标签名义类型）、`session: SessionId`（内核的会话 id）、`cwd: String`（工作目录路径）、`argv: Command`（内核的命令 AST，`program`/`args`/`redirects`/`heredoc`）、`stdin: Option<Vec<u8>>`（heredoc 正文之外的独立输入流，空则 `None`）、`lease_id: Option<LeaseId>`（嵌套调用继承外层租约的可选标识，ADR 0014）。信封里没有 `workspace` 字段，工作区由 server 从连接身份推导，不是请求参数。

`argv` 取内核的 `Command` AST 而不是裸命令串，是因为执行器要消费它来做权限预检与租约获取（`read_paths`/`write_paths` 提取读写路径），而那份代码不该依赖词法分析的内部结构。`Command` 的 `heredoc` 字段承载 heredoc 正文，`stdin` 字段承载 heredoc 之外的独立输入流，两者不混用。`lease_id` 是嵌套调用继承外层租约的标识，08.1 只承载它，租约的获取与释放是 09.x 的事。

### 2.2 版本协商是校验协议版本，不匹配拒绝并回显支持范围

`ProtocolVersion` 是协议版本名义类型，`u32` 主版本号（协议是单一 IPC 契约，不是库，不用 semver 三元组）。`SupportedRange` 是支持范围，`min: ProtocolVersion` 与 `max: ProtocolVersion` 两个字段，`min <= max` 在构造期校验。协商是纯函数 `negotiate(requested: ProtocolVersion, supported: SupportedRange) -> Result<Accepted, Rejected>`：`requested` 在 `min..=max` 区间内返回 `Accepted`，否则返回 `Rejected { supported }`，回显支持范围。拒绝时回显支持范围，让调用方看见错在哪。

支持范围是可调参数，读取统一走配置结构（ADR 0018 第九节「全部参数都要有配置入口。默认值写死在内核常量里，但读取路径统一走一个配置结构，允许按项目、按工作区覆盖」）。08.1 把支持范围落成 `SupportedRange` 类型与一个默认值常量，读取路径走配置结构，不散落在调用点。

### 2.3 信封往返是长度前缀文本编解码

`encode(invocation: &Invocation) -> Vec<u8>` 与 `decode(bytes: &[u8]) -> Result<Invocation, DecodeError>` 是信封的编解码。每个字段先写长度（大端 4 字节）再写内容，解码按长度切回。`Command` AST 的 `program`/`args`/`redirects`/`heredoc` 逐个编码，`stdin` 的 `Option<Vec<u8>>` 用 1 字节标记（0 为 `None`，1 为 `Some`）加长度前缀内容。往返一致是「编码再解码等于原值」，不是「编码再解码等于某个规范化形式」。08.1 不引入 serde，不新增外部依赖，与 06.4 的帧正文长度前缀文本同型。

### 2.4 handler 是 server 侧处理函数，入参类型只有 `Invocation`

`Handler` 是 server 侧处理函数 trait，签名是 `fn handle(&self, invocation: Invocation) -> Result<Response, HandlerError>`。`Response` 是新的响应类型，`HandlerError` 是错误类型。`Invocation` 结构体里没有 workspace 字段，因此 handler 拿不到 workspace。workspace 由 server 从连接身份推导（08.2 的握手），不进 handler 签名。这是 ADR 0002「业务处理函数的入参类型里根本没有这个字段」的落地，由编译器保证而不是靠调用方记得。

### 2.5 08.1 不建 socket、不引入 tokio、不碰网络

08.1 在 08.2 之前，真实 socket 与连接级工作区绑定是 08.2 的事。07.4 的进程内 watch 通道是同步的进程内通知，08.x 会把这条通道接到真实 socket，届时复用本批的信封。因此 08.1 不引入 tokio、不建 socket、不碰网络，信封与版本协商是纯数据变换。08.1 在 `ita-proto` 的 `Cargo.toml` 加 `ita-kernel` 一条依赖（分层门允许表 `ita-proto: {ita-kernel, ita-model}` 已登记），零新增外部依赖。

### 2.6 与 07.4 watch 通道的关系

07.4 的 `CapabilityWatcher` 是进程内的 watch 通道，持有 `Store` 句柄，变更时读 `units` 表重建能力集，版本号递增一次，向全部订阅者推送。它是进程内的，不是 IPC。08.1 的信封是 server 与 worker 之间 IPC 消息的封装，能力推送（`CapabilityUpdate`）以后会在 08.2 接到真实 socket 时复用本批的信封。08.1 不碰 watch 通道，只定义信封与版本协商。07.4 的 watch 通道是进程内同步通知，08.x 把通道接到真实 socket 时，能力集变更与工具目录追加走同一套信封机制（ADR 0013「能力集合的推送与工具目录的追加通知是同一套机制」）。

## 三、ADR 对信封与版本协商的硬要求

与 08.1 相关的决策分布在 ADR 0002、0014 与 0018，逐条对照如下。

| 要求 | 出处 | 08.1 怎么落地 |
|---|---|---|
| workspace 是会话属性，不是请求参数 | 0002 决策段 | 信封里没有 workspace 字段，handler 入参类型只有 `Invocation`，workspace 由 server 从连接身份推导 |
| 请求信封里不出现 workspace 字段，由接口形状保证 | 0002 决策段 | `Invocation` 结构体没有 workspace 字段，handler 签名没有 workspace 参数，编译器保证 |
| 消息分两类：权威写与遥测 | 0002 决策段 | 08.1 只定义信封与版本协商，两类消息的分流是 08.3 的事 |
| 嵌套调用继承外层租约，不授予新路径、不新建第二张租约 | 0014 决策段 | 信封的 `lease_id` 是 `Option<LeaseId>`，嵌套调用继承外层租约的标识，08.1 只承载它 |
| 一次工具调用的边界由执行器划定 | 0014 决策段 | 信封的 `argv` 是内核的 `Command` AST，执行器消费它做权限预检与租约获取 |
| 全部参数都要有配置入口，读取路径统一走配置结构 | 0018 第九节 | 支持范围是可调参数，落成 `SupportedRange` 类型与默认值常量，读取走配置结构 |
| 信封带协议版本，版本不匹配拒绝并回显支持范围 | ROADMAP 08 行验收 | `negotiate` 纯函数，`requested` 在 `min..=max` 区间内返回 `Accepted`，否则返回 `Rejected { supported }` |
| 同版本往返一致 | ROADMAP 08 行验收 | `encode` 再 `decode` 等于原值，长度前缀文本编解码，不引入 serde |
| handler 无法表达 workspace | ROADMAP 08 行验收 | handler 签名 `fn handle(&self, invocation: Invocation) -> Result<Response, HandlerError>`，`Invocation` 没有 workspace 字段 |

三处需要特别说清的核实结果。

**第一，`ita-proto` 基线是空壳，08.1 在其上加 `envelope.rs` 与 `version.rs`。** 基线 34730b2 的 `crates/ita-proto/` 只有 `Cargo.toml`（`[dependencies]` 为空）与 `src/lib.rs`（模块文档注释与 `#![forbid(unsafe_code)]`、`#![deny(missing_docs)]`），08.1 在其上加 `envelope.rs` 与 `version.rs`，并在 `lib.rs` 声明两个模块。08.1 在 `Cargo.toml` 加 `ita-kernel` 一条依赖（分层门允许表 `ita-proto: {ita-kernel, ita-model}` 已登记），零新增外部依赖。

**第二，信封的 `argv` 是内核的 `Command` AST，不是裸命令串。** 内核的 `Command` 有 `program`/`args`/`redirects`/`heredoc` 四字段，`read_paths`/`write_paths` 提取读写路径，执行器要消费它来做权限预检与租约获取。信封的 `argv` 取 `Command` AST，`stdin` 取 heredoc 正文之外的独立输入流，两者不混用。`lease_id` 是嵌套调用继承外层租约的标识，08.1 只承载它，租约的获取与释放是 09.x 的事。

**第三，08.1 不建 socket、不引入 tokio、不碰网络。** 08.1 在 08.2 之前，真实 socket 与连接级工作区绑定是 08.2 的事。07.4 的进程内 watch 通道是同步的进程内通知，08.x 会把这条通道接到真实 socket，届时复用本批的信封。因此 08.1 不引入 tokio、不建 socket、不碰网络，信封与版本协商是纯数据变换。

## 四、改写后的验收标准

每条都能写成一个测试，括号里写明它守的是什么。

1. **同版本往返一致。** 建一个 `Invocation`（带 `version`/`caller`/`session`/`cwd`/`argv`/`stdin`/`lease_id` 全部字段），调 `encode` 编码成字节，调 `decode` 解码回 `Invocation`，断言解码结果等于原值（逐字段相等，`argv` 的 `Command` AST 逐字段相等，`stdin` 的字节序列相等）。守的是「同版本往返一致」。
2. **不兼容版本明确报错。** 建一个 `Invocation`，协议版本是 5，支持范围是 `min=1, max=4`，调 `negotiate`，断言返回 `Rejected { supported }`，`supported` 是 `min=1, max=4`。守的是「不兼容版本明确报错，回显支持范围」。
3. **兼容版本被接受。** 建一个 `Invocation`，协议版本是 3，支持范围是 `min=1, max=4`，调 `negotiate`，断言返回 `Accepted`。守的是「兼容版本被接受」。
4. **支持范围边界值被接受。** 协议版本等于 `min` 与等于 `max` 都被接受，`min-1` 与 `max+1` 都被拒绝。守的是「支持范围是闭区间 `min..=max`」。
5. **handler 无法表达 workspace。** 定义一个 `Handler` 实现，签名是 `fn handle(&self, invocation: Invocation) -> Result<Response, HandlerError>`，断言 `Invocation` 结构体里没有 workspace 字段（编译期保证，测试里用一个 `Invocation` 实例调用 handler，handler 拿不到 workspace）。守的是「handler 无法表达 workspace」。
6. **信封的 `argv` 是内核的 `Command` AST。** 建一个 `Invocation`，`argv` 是 `Command { program, args, redirects, heredoc }`，调 `encode` 再 `decode`，断言解码结果的 `argv` 是同一个 `Command`（`program`/`args`/`redirects`/`heredoc` 逐字段相等）。守的是「信封的 `argv` 是内核的 `Command` AST」。
7. **信封的 `stdin` 是 heredoc 正文之外的独立输入流。** 建一个 `Invocation`，`stdin` 是 `Some(vec![1, 2, 3])`，调 `encode` 再 `decode`，断言解码结果的 `stdin` 是 `Some(vec![1, 2, 3])`。再建一个 `stdin` 是 `None` 的 `Invocation`，断言解码结果的 `stdin` 是 `None`。守的是「`stdin` 是 heredoc 正文之外的独立输入流」。
8. **信封的 `lease_id` 是嵌套调用继承外层租约的标识。** 建一个 `Invocation`，`lease_id` 是 `Some(LeaseId)`，调 `encode` 再 `decode`，断言解码结果的 `lease_id` 是同一个 `LeaseId`。再建一个 `lease_id` 是 `None` 的 `Invocation`，断言解码结果的 `lease_id` 是 `None`。守的是「`lease_id` 是嵌套调用继承外层租约的标识」。
9. **支持范围是可调参数，读取走配置结构。** 建一个 `SupportedRange`，`min=1, max=4`，断言 `min <= max` 在构造期校验（`min > max` 被拒）。守的是「支持范围是可调参数，读取走配置结构」。
10. **08.1 不建 socket、不引入 tokio、不碰网络。** 断言 `ita-proto` 的 `Cargo.toml` 只加 `ita-kernel` 一条依赖，零新增外部依赖，`envelope.rs` 与 `version.rs` 只用 `std` 与内核的 id 类型与 `Command` AST。守的是「08.1 不建 socket、不引入 tokio、不碰网络」。

第 1 条守的是「同版本往返一致」。第 2、3、4 条守的是「不兼容版本明确报错，回显支持范围」与「兼容版本被接受」与「支持范围是闭区间」。第 5 条守的是「handler 无法表达 workspace」。第 6 条守的是「信封的 `argv` 是内核的 `Command` AST」。第 7 条守的是「`stdin` 是 heredoc 正文之外的独立输入流」。第 8 条守的是「`lease_id` 是嵌套调用继承外层租约的标识」。第 9 条守的是「支持范围是可调参数，读取走配置结构」。第 10 条守的是「08.1 不建 socket、不引入 tokio、不碰网络」。

## 五、文件清单

| 文件 | 动作 | 内容 |
|---|---|---|
| `crates/ita-proto/src/envelope.rs` | 新建 | `Invocation` 结构（`version`/`caller`/`session`/`cwd`/`argv`/`stdin`/`lease_id` 七字段，无 workspace 字段）、`Caller` 名义类型（worker 标签，校验过的标签，与 `UnitId` 同型）、`encode` 与 `decode`（长度前缀文本编解码）、`Handler` trait（`fn handle(&self, invocation: Invocation) -> Result<Response, HandlerError>`）、`Response` 与 `HandlerError` 类型、内联 `#[cfg(test)]` 测试 |
| `crates/ita-proto/src/version.rs` | 新建 | `ProtocolVersion` 名义类型（`u32` 主版本号）、`SupportedRange` 类型（`min`/`max` 两字段，`min <= max` 构造期校验）、`negotiate` 纯函数（`requested` 在 `min..=max` 区间内返回 `Accepted`，否则返回 `Rejected { supported }`）、`Accepted` 与 `Rejected` 类型、内联 `#[cfg(test)]` 测试 |
| `crates/ita-proto/src/lib.rs` | 修改 | 声明 `envelope`、`version` 两个模块，再导出关键类型 |
| `crates/ita-proto/Cargo.toml` | 修改 | 加 `ita-kernel` 一条依赖（分层门允许表 `ita-proto: {ita-kernel, ita-model}` 已登记），零新增外部依赖 |
| `crates/ita-kernel/src/context/ids.rs` | 修改 | 用既有 `id_type!` 宏加 `LeaseId` 名义类型（26 字符 Crockford base32 ULID，与 `NodeId`/`SessionId` 同型），并在 `context/mod.rs` 重新导出 |
| `crates/ita-kernel/src/context/mod.rs` | 修改 | 重新导出 `LeaseId` |
| `crates/ita-proto/src/` 其余文件 | **不动** | `ita-proto` 基线只有 `lib.rs`，08.1 加 `envelope.rs` 与 `version.rs` |
| `crates/ita-server/**` | **不动** | 08.1 不碰 server，handler 是 `ita-proto` 的 trait，server 在 08.2 实现它 |
| `crates/ita-store/**` | **不动** | 08.1 不新增迁移、不改存储层 |

`envelope.rs` 的 `Invocation` 是公开结构，`encode`/`decode` 是公开函数，`Handler` 是公开 trait。`version.rs` 的 `ProtocolVersion`/`SupportedRange` 是公开类型，`negotiate` 是公开纯函数。测试内联在各文件的 `#[cfg(test)] mod tests` 里，不另开 `tests/` 集成测试文件——纯逻辑模块的内联测试是 Rust 惯例，且 `ita-proto` 基线没有 `tests/` 目录。`LeaseId` 加在内核 `ids.rs`，用既有 `id_type!` 宏，与 `NodeId`/`SessionId`/`WorkspaceId`/`CheckpointId`/`EventId` 同型，标识符名义类型从五个变六个。

## 六、实现顺序与每步验证

分四步，前三步各一个提交，第四步是全仓验证不单独提交。同一批次共用同一个版本号，版本只在最后一次提交前推进一次。顺序按「能独立验证的先做」排，`version.rs` 的版本协商纯函数是最便宜的定形，`envelope.rs` 的信封与编解码是核心算法。

| 步骤 | 做什么 | 验证方式 |
|---|---|---|
| 一 | 在内核 `ids.rs` 用 `id_type!` 宏加 `LeaseId`，在 `context/mod.rs` 重新导出，写内联测试 | `cargo nextest run -p ita-kernel -E 'test~lease_id'`，覆盖 `LeaseId` 的解析与文本形式往返 |
| 二 | 写 `version.rs`（`ProtocolVersion`、`SupportedRange`、`negotiate`、`Accepted`、`Rejected`）与内联测试，更新 `lib.rs` 声明 `version` 模块 | `cargo nextest run -p ita-proto -E 'test~version'`，覆盖第四节第 2、3、4、9 条（不兼容版本报错、兼容版本接受、边界值、支持范围构造期校验） |
| 三 | 写 `envelope.rs`（`Invocation`、`Caller`、`encode`、`decode`、`Handler`、`Response`、`HandlerError`）与内联测试，更新 `lib.rs` 声明 `envelope` 模块，在 `Cargo.toml` 加 `ita-kernel` 依赖 | `cargo nextest run -p ita-proto`，此时测试覆盖第四节全部十条验收 |
| 四 | 补全仓验证与文档 | `bash .w1mer/scripts/verify.sh all`，全仓验证通过 |

测试函数名用英文 snake_case，上方一行 `///` 用中文写这个用例守的行为。这条规则在 06.2.2 里入册，断言消息仍然用中文。

## 七、风险：容易做成测试通过但语义错的地方

| 风险 | 为什么测试会照样通过 | 对策 |
|---|---|---|
| 信封里有 workspace 字段（handler 能表达 workspace） | 编译通过，但「handler 无法表达 workspace」的验收被偷换 | 第四节第 5 条：断言 `Invocation` 结构体里没有 workspace 字段，handler 签名没有 workspace 参数，编译器保证 |
| 版本协商接受区间外的版本（off-by-one） | 正常路径通过，边界值才暴露 | 第四节第 4 条：断言 `min-1` 与 `max+1` 被拒，`min` 与 `max` 被接受 |
| 往返丢失信息（heredoc 正文被截断） | 正常路径通过，大正文才暴露 | 第四节第 1 条：断言编码再解码等于原值，逐字段相等 |
| `caller` 是未校验的裸 `String` | 编译通过，但作用域泄漏风险 | `Caller` 是名义类型，校验过的标签，与 `UnitId` 同型 |
| `lease_id` 是裸 ULID 串 | 编译通过，但「把节点 id 当租约 id 传」这类错误在编译期拦不住 | `LeaseId` 是内核的名义类型，`id_type!` 宏，与 `NodeId`/`SessionId` 同型 |
| 版本是裸 `u32` 没有名义类型 | 编译通过，但「把 crate 版本当协议版本传」这类错误在编译期拦不住 | `ProtocolVersion` 是名义类型，`u32` 主版本号 |
| 编码不是长度前缀（解码不知道边界） | 编译通过，但解码在多字段时切错 | 长度前缀文本编解码，与 06.4 的帧正文同型，每个字段先写长度再写内容 |
| handler 返回裸 `Result` 没有 `Response` 类型 | 编译通过，但响应形状没有定义 | `Response` 是新的响应类型，`HandlerError` 是错误类型 |
| 支持范围是常量而不是配置结构 | 编译通过，但「全部参数都要有配置入口」被破坏 | 支持范围落成 `SupportedRange` 类型与默认值常量，读取走配置结构 |
| 08.1 引入 tokio 或建 socket | 编译通过，但「08.1 在 08.2 之前，无 socket」的纪律被破坏 | 08.1 不引入 tokio、不建 socket、不碰网络，信封与版本协商是纯数据变换 |
| 信封落内核而不是 `ita-proto` | 编译通过，但「信封是 IPC 消息封装」的职责划错了一层 | 信封落 `ita-proto`，不进内核，内核的职责域是「上下文树的解析、命令文法的分析、租约与权限的策略判定」 |

还有一条不属于「测试通过但语义错」，但会直接影响可复审性的：信封的 `argv` 是内核的 `Command` AST，不是裸命令串。执行器要消费它来做权限预检与租约获取，而那份代码不该依赖词法分析的内部结构。若把 `argv` 做成裸命令串，会让执行器重新解析一遍命令，与「内核只分析一次、执行器消费结果」的分工冲突。这条职责分离是「这一批只有一个目的」的前提，不靠调用方记得「`argv` 是 `Command` AST」。

## 八、已裁决的分歧点

五处分歧点由 Explorer 直接定形，理由充分。

| 分歧 | 两种做法 | 定形 |
|---|---|---|
| 信封的 `argv` 是内核的 `Command` AST 还是裸命令串 | 甲：`Command` AST（`program`/`args`/`redirects`/`heredoc`）。乙：裸命令串 | **直接定形选甲。** 理由见第二节第 2.1 条。执行器要消费 `Command` 做权限预检与租约获取（`read_paths`/`write_paths` 提取读写路径），而那份代码不该依赖词法分析的内部结构。裸命令串会让执行器重新解析一遍命令，与「内核分析一次、执行器消费结果」的分工冲突 |
| 信封的 `stdin` 是 heredoc 正文还是独立输入流 | 甲：heredoc 正文之外的独立输入流（`Option<Vec<u8>>`）。乙：heredoc 正文本身 | **直接定形选甲。** 理由见第二节第 2.1 条。`Command` 的 `heredoc` 字段已经承载 heredoc 正文，`stdin` 字段承载 heredoc 之外的独立输入流，两者不混用。若 `stdin` 是 heredoc 正文，会与 `Command.heredoc` 重复 |
| 信封的 `lease_id` 是内核的 `LeaseId` 名义类型还是裸 ULID 串 | 甲：内核的 `LeaseId` 名义类型（`id_type!` 宏）。乙：裸 ULID 串 | **直接定形选甲。** 理由见第二节第 2.1 条。内核的 `id_type!` 宏是既有模式，`LeaseId` 与 `NodeId`/`SessionId` 同型，「把节点 id 当租约 id 传」这类错误在编译期被拦住。裸 ULID 串会让这类错误在运行时才暴露 |
| 版本是 `u32` 主版本号还是 semver 三元组 | 甲：`u32` 主版本号。乙：semver 三元组（major/minor/patch） | **直接定形选甲。** 理由见第二节第 2.2 条。协议是单一 IPC 契约，不是库，不用 semver 三元组。`u32` 主版本号足够表达「同版本往返一致」与「不兼容版本明确报错」两条验收 |
| 支持范围是常量还是配置结构 | 甲：`SupportedRange` 类型与默认值常量，读取走配置结构。乙：散落在调用点的常量 | **直接定形选甲。** 理由见第二节第 2.2 条。ADR 0018 第九节「全部参数都要有配置入口。默认值写死在内核常量里，但读取路径统一走一个配置结构，允许按项目、按工作区覆盖」。支持范围是可调参数，落成 `SupportedRange` 类型与默认值常量，读取走配置结构 |

## 九、本批不做的事

为了让复审能用「这一批只有一个目的」来判断，以下几项明确不属于 08.1：真实 socket 与连接级工作区绑定（08.2）；权威与遥测分流、有界队列与背压（08.3）；group commit（08.4）；批量读 `load_context`（08.5）；租约的获取与释放（09.x）；执行器进程（0014，独立进程，不是 unit）；工作流节点类型（`ita-workflow`，与 unit 类型分开）；事件日志与模型代理（08.x）；异步执行（08.1 是纯数据变换，不引入 tokio）；版本号落库（进程内计数器，不新增迁移）；工具目录追加本身（工具系统批次，07.4 只保证 watch 通道可承载）。

## 十、Explorer 核对与补强（基线 34730b2）

本节的核对对象是批次基线 34730b2 的已提交代码，不是实现者的工作树。结论是：十条验收对基线全部成立，信封与版本协商的口径都没有被证伪，需要改写的只是「怎么做」与五处待裁决项的定形。下面逐条记录核对结果与五处补强。

### 10.1 `ita-proto` 现状核对：空壳，08.1 加两个文件

基线 34730b2 的 `crates/ita-proto/` 只有 `Cargo.toml`（`[dependencies]` 为空）与 `src/lib.rs`（模块文档注释与 `#![forbid(unsafe_code)]`、`#![deny(missing_docs)]`），08.1 在其上加 `envelope.rs` 与 `version.rs`，并在 `lib.rs` 声明两个模块。08.1 在 `Cargo.toml` 加 `ita-kernel` 一条依赖（分层门允许表 `ita-proto: {ita-kernel, ita-model}` 已登记），零新增外部依赖。08.1 不修改内核的 `command/` 与 `context/` 模块，只在 `ids.rs` 用既有 `id_type!` 宏加 `LeaseId` 并在 `context/mod.rs` 重新导出。

### 10.2 内核 id 类型核对：`LeaseId` 用既有 `id_type!` 宏

基线 34730b2 的 `crates/ita-kernel/src/context/ids.rs` 有 `NodeId`/`SessionId`/`WorkspaceId`/`CheckpointId`/`EventId` 五个名义类型，用 `id_type!` 宏生成（26 字符 Crockford base32 ULID，拒 `I/L/O/U`，首位 0 到 7）。08.1 用同一宏加 `LeaseId`，与其余五个同型，标识符同型，标识符名义类型从五个变六个。`LeaseId` 的 `parse`/`as_str`/`kind` 与其余五个一致，`kind()` 返回「租约」。

### 10.3 内核 `Command` AST 核对：信封的 `argv` 取 `Command`

基线 34730b2 的 `crates/ita-kernel/src/command/ast.rs` 有 `Command { program, args, redirects, heredoc }`，`read_paths`/`write_paths` 提取读写路径，执行器要消费它来做权限预检与租约获取。信封的 `argv` 取 `Command` AST，`stdin` 取 heredoc 正文之外的独立输入流，两者不混用。`Command` 的 `heredoc` 字段承载 heredoc 正文，`stdin` 字段承载 heredoc 之外的独立输入流。

### 10.4 07.4 watch 通道核对：进程内同步通知，08.x 接 socket

07.4 的执行计划（detail 31）产出 `CapabilitySet`/`CapabilityWatcher`，watch 通道是进程内的，不是 IPC。`CapabilityWatcher` 持有 `Store` 句柄、当前能力集与订阅者列表，`subscribe` 注册订阅者并立即推送当前完整能力集（版本 0），`notify_change` 读 `units` 表重建能力集、递增版本号、向全部订阅者推送。08.1 的信封是 server 与 worker 之间 IPC 消息的封装，能力推送（`CapabilityUpdate`）以后会在 08.2 接到真实 socket 时复用本批的信封。08.1 不碰 watch 通道，只定义信封与版本协商。

### 10.5 五处补强

**第一，信封是 server 与 worker 之间 IPC 消息的封装，落在 `ita-proto`，不进内核。** ADR 0002 的硬边界是「workspace 是会话属性，不是请求参数；server 从连接身份与握手时的会话信息推导工作区，请求信封里不出现 workspace 字段。这条由接口形状保证：业务处理函数的入参类型里根本没有这个字段」。因此信封类型必须独立于内核的上下文树类型，而内核的职责域是「上下文树的解析、命令文法的分析、租约与权限的策略判定」（`lib.rs` 文档注释），信封不在其中。08.1 的信封落在 `ita-proto`，维持不改。

**第二，版本协商是校验协议版本，不匹配拒绝并回显支持范围。** 信封带协议版本字段，server 端持有一个支持范围（可调参数，读取走配置结构，ADR 0018 第九节「全部参数都要有配置入口」）。协商是纯函数：输入是请求版本与支持范围，输出是接受或拒绝（带支持范围）。拒绝时回显支持范围，让调用方看见错在哪。

**第三，信封往返是长度前缀文本编解码，同版本编码再解码必须逐字节等价。** 06.4 已把帧正文落成长度前缀文本（写进 `nodes.inline_body`，不引入 `serde_json`），08.1 的信封编码与它同型：每个字段先写长度再写内容，解码按长度切回。往返一致是「编码再解码等于原值」，不是「编码再解码等于某个规范化形式」。08.1 不引入 serde，不新增外部依赖。

**第四，handler 是 server 侧处理函数，入参类型只有 `Invocation`，没有 workspace 参数。** 这条由编译器保证：handler 的签名是 `fn handle(&self, invocation: Invocation) -> Result<Response, HandlerError>`，`Invocation` 结构体里没有 workspace 字段，因此 handler 拿不到 workspace。workspace 由 server 从连接身份推导（08.2 的握手），不进 handler 签名。这是 ADR 0002「业务处理函数的入参类型里根本没有这个字段」的落地。

**第五，08.1 不建 socket、不引入 tokio、不碰网络。** 08.1 在 08.2 之前，真实 socket 与连接级工作区绑定是 08.2 的事。07.4 的进程内 watch 通道是同步的进程内通知，08.x 会把这条通道接到真实 socket，届时复用本批的信封。因此 08.1 不引入 tokio、不建 socket、不碰网络，信封与版本协商是纯数据变换。08.1 在 `ita-proto` 的 `Cargo.toml` 加 `ita-kernel` 一条依赖（分层门允许表 `ita-proto: {ita-kernel, ita-model}` 已登记），零新增外部依赖。
