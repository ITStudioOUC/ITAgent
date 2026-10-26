# ita-proto — 协议信封与版本协商

协议信封与本地帧编解码（`crates/ita-proto`）。只定义信封、帧格式与版本协商：字段怎么排、
帧怎么切、版本怎么对齐。不建立连接、不做路由、不持有会话状态。依赖仅 `ita-kernel`
（见 `crates/ita-proto/Cargo.toml`），零新增外部依赖。

## Responsibilities

- Invocation 信封与编解码：七个字段、长度前缀文本编解码、解码失败原因（`crates/ita-proto/src/envelope.rs`）。
- 版本协商：协议版本名义类型、支持范围构造期校验、协商纯函数（`crates/ita-proto/src/version.rs`）。

## Key types & abstractions

- `Invocation`（`crates/ita-proto/src/envelope.rs`）：一次工具调用的信封。七个字段：
  `version`（协议版本）、`caller`（调用者标识）、`session`（会话 id）、`cwd`（工作目录）、
  `argv`（内核的命令 AST）、`stdin`（heredoc 正文之外的独立输入流）、`lease_id`
  （嵌套调用继承外层租约的可选标识）。**没有 workspace 字段**——工作区是会话属性，由
  server 从连接身份推导，不是请求参数。
- `Caller`（同文件）：调用者标识名义类型。校验过的标签，小写字母数字加连字符，非空，
  首尾不连字符，不连续连字符。
- `Handler` trait（同文件）：server 侧处理函数，签名 `fn handle(&self, invocation: Invocation)
  -> Result<Response, HandlerError>`。入参类型只有 `Invocation`，没有 workspace 参数，
  这条由编译器保证。
- `Response`（同文件）：一次调用的响应，标准输出、标准错误与退出码三样，stdout 与
  stderr 是字节流。
- `HandlerError`（同文件）：处理失败的原因。策略拒绝与租约拒绝分成两个变体，调用方对
  两者的处置不同。
- `ProtocolVersion`（`crates/ita-proto/src/version.rs`）：协议版本名义类型，`u32` 主版本号。
  协议是单一 IPC 契约，不是库，不用 semver 三元组。
- `SupportedRange`（同文件）：支持范围，闭区间 `min..=max`，下限大于上限在构造期被拒。
  是可调参数，默认值由 `DEFAULT_SUPPORTED_RANGE` 给出，server 启动时从配置结构读取。
- `negotiate`（同文件）：版本协商纯函数。请求版本落在支持范围内返回 `Accepted`，否则
  返回 `Rejected { supported }`，回显支持范围。

## Internal data flow

- 编码：`encode` 把信封写成字节流，每个字段先写大端 4 字节长度前缀再写内容。协议版本
  是长度前缀 4 加大端 4 字节主版本号；调用者标签、会话 id、工作目录是长度前缀文本；
  命令 AST 的程序名、参数、重定向、heredoc 逐个编码；独立输入流与租约标识是 1 字节存在
  标志加长度前缀内容。
- 解码：`decode` 按长度切回，字节流在字段中间结束报 `Truncated`，字段之后还有多余字节
  报 `ExtraBytes`，未知方向字节与存在标志报对应变体，标识符文本不合法报 `BadId`。
- 协商：`negotiate(requested, supported)` 是纯函数，同样的输入必然得到同样的结果。

## Contracts

### 提供（给其他模块）

- `Invocation` / `encode` / `decode`：server 与 worker 之间 IPC 消息的封装与编解码。
- `Handler` / `Response` / `HandlerError`：server 侧处理函数的契约。
- `ProtocolVersion` / `SupportedRange` / `negotiate`：版本协商。

### 消费（从其他模块）

- `ita-kernel` 的 `Command` AST（`program`/`args`/`redirects`/`heredoc`）与 `SessionId`、
  `LeaseId` 名义类型。

## Invariants

- **信封里没有 workspace 字段**：工作区是会话属性，由 server 从连接身份推导，不是请求
  参数。这条由接口形状保证：`Invocation` 没有这个字段，`Handler` 的入参类型里也没有
  这个参数。
- **同版本往返一致**：编码再解码等于原值，逐字段相等，不是等于某个规范化形式。
- **长度前缀文本编解码**：每个字段先写长度再写内容，解码按长度切回。与帧正文的长度前缀
  文本同型，不引入 serde。
- **不建 socket、不引入 tokio、不碰网络**：信封与版本协商是纯数据变换。真实 socket 与
  连接级工作区绑定是后续批次的事。
- **支持范围是可调参数**：默认值由常量给出，读取走配置结构，不散落在调用点。
