# ita-kernel — 纯逻辑内核

> 内核只做数据变换：命令文法解析、上下文树与帧、纯函数 `resolve()`、语义压缩的纯计划。
> 零依赖（含 dev 依赖），无 IO、无时钟、无随机、无异步。边界由 `check_kernel_purity.sh` 与
> `clippy.toml` 的禁入清单共同强制，不是约定。

## 职责

- **命令文法**（`crates/ita-kernel/src/command/`）：把一条命令行变成 `Command`。版本一只支持
  单命令：参数、`>` `>>` `<` 重定向、单个 heredoc。管道、命令替换、反引号、逻辑连接、通配符、
  后台执行一律在词法层拒绝，理由是它们会让写入目标集合在执行前不可知。
- **上下文树**（`crates/ita-kernel/src/context/tree.rs`）：节点池加会话 head 指针，head 更新
  走比较并交换。回退与分叉只移动 head，不复制节点。
- **纯解析**（`crates/ita-kernel/src/context/resolve.rs`）：把 `ResolveInput` 变成模型实际看到的
  消息数组。同样的输入必然得到逐字节相同的输出——这是 replay 与前缀缓存的基础。
- **语义压缩的纯计划**（`crates/ita-kernel/src/context/compress/`）：何时压缩、压哪一段、摘要
  格式契约、事实层校验、提交计划。真正的模型调用与事务提交在 server 侧，不在内核。
- **读集与陈旧写判定**（`crates/ita-kernel/src/context/read_set.rs`）：租约只解决"谁先写"，
  读集解决"我读到的是不是最新的"。键是（工作区, 路径, 会话）。

## 关键类型与抽象

### 命令文法（`crates/ita-kernel/src/command/`）

- `Command`（`ast.rs`）：`program` + `args` + `redirects` + 至多一个 `heredoc`。`read_paths()`
  与 `write_paths()` 只取自重定向，排序去重——排序是租约原子获取的要求。`Display` 还原成可
  直接粘贴执行的命令（heredoc 连正文一起输出）。
- `ParseError`（`error.rs`）：变体在第一批就冻结，后续只填空。每个变体带字节偏移与可操作
  建议；`is_retryable()` 恒为 `false`——语法错误重试只会重复失败。
- `scan`（`lex.rs`）：词法层。未加引号的 `$`、反引号、`$(`、`*`、`?`、`[`、`{`、`}`、`~` 全部
  拒绝；双引号内只解释 bash 也解释的四个转义（`\"` `\\` `\$` `` \` ``），保证同一条命令在
  ita 与真实 shell 里写出相同的字节。
- `parse` / `parse_with_limits`（`parse.rs`）：装配层。上限（正文字节数、记号数）由调用方
  从配置传入，不是常量。至多一个 heredoc，且不能与 `<` 同现。
- `scan_heredoc`（`heredoc.rs`）：终止行必须独占一行、与分隔符逐字节相等、大小写敏感、行尾
  不允许空格。正文按字节原样保留，不转换换行。正文里撞上分隔符时后面的内容变成残留文本，
  整条拒绝而不是静默截断。

### 上下文树（`crates/ita-kernel/src/context/`）

- `Node`（`node.rs`）：不可变。`NodeBody` 的变体：`FixedLayer`（冻结的系统提示与工具目录，
  带正文与指纹）、`Turn`（用户/助手原文）、`ToolCall`/`ToolResult`（分两个节点，因为供应商
  报文里就是两条消息）、`Frame`、`Notice`、`Todo`。`ToolResult` 的正文是 `BlobRef`（哈希 + 长度）
  加预览，不是内联文本。
- `Frame`（`node.rs`）：摘要 + 覆盖节点区间 + 弱引用检查点 + 固定层指纹。`covers` 按时间顺序
  且不重复，构造方保证。
- `Tree` / `HeadToken`（`tree.rs`）：head 令牌带（会话, 节点, 代次）。代次是单调的，解决
  "回退到祖先再前进"后旧令牌重新变成当前值的 ABA 问题；会话字段防止两个从同一节点分叉的
  子会话互相推进。`append` 只允许接在当前 head 之后；`rewind` 只能到祖先。
- `NodeId` 等五个 id 类型（`ids.rs`）：26 字符 ULID 文本，Crockford base32，首位只允许 0–7。
  用名义类型区分，`NodeId` 与 `SessionId` 不可互换。
- `ResolveInput` / `CheckpointFacts`（`input.rs`）：`resolve()` 需要的一切外部状态都由调用方
  装进来。`validate()` 校验结构自洽：帧不重叠、帧引用的检查点都有事实、recent/current 里不出
  现帧或固定层。
- `Budget`（`budget.rs`）：`limit`、`compress_trigger_permille`（千分比）、`compress_reserve`、
  `keep_recent_turns`、`preview_bytes`（字节）。用抽象计数不用 token——分词属于模型层。
- `ReadSet` / `PathKey` / `StaleVerdict`（`read_set.rs`）：`PathKey` 做纯字符串归一（去多余
  `/`、去前导 `./`、去尾斜杠），不做符号链接解析——那需要文件系统。判定只比哈希，不比序号；
  没有记录时返回 `Unknown` 而不是 `Stale`，否则首次写新文件永远失败。
- `ResolvedContext`（`resolve.rs`）：`messages` + `drift` + `body_bytes`。`body_bytes` 是字节
  数，与预算的抽象计数不同量纲，不要直接比较。

### 语义压缩（`crates/ita-kernel/src/context/compress/`）

- `decide`（`trigger.rs`）：`NotYet` / `Compress` / `CannotCompress`。"该压"与"能压"是两个问题：
  触发点之后剩余空间装不下压缩预留时，返回 `CannotCompress`，调用方必须缩小输入。
- `plan`（`fold.rs`）：轮边界是**用户消息**——一条用户输入开启一轮，其后的助手回复与工具节点
  都属于这一轮。`covered` 与 `kept` 不重叠且合起来是全部历史。固定层与待办不参与折叠。
- `parse_output` / `format_instructions`（`format.rs`）：散文用 `<prose>...</prose>` 包裹，整段
  以 `<end>` 收尾。缺标记即判截断，整次压缩重试，绝不拿半截摘要替换上下文。`PROMPT_VERSION`
  改动提示词时递增。
- `Facts` / `Failure` / `FileChange`（`facts.rs`）：机器填写的事实层。失败记录必须两半齐全
  （尝试 + 结果），已否决方案必须带理由。不要求"有改动就必须有失败"——没有失败时明确写
  "无失败尝试"。
- `slim` / `render` / `slim_all`（`recent.rs`）：保留轮次里的工具结果只留命令、一行结果（截断
  到 `LINE_LIMIT` 字节）、退出码、原文句柄。原文不删除，只是不放进消息数组。
- `CommitPlan` / `HeadTokenLike` / `plan`（`commit.rs`）：只产出计划，不执行提交。折叠范围末尾
  必须就是当前 head，否则计划作废。调用方用同一个 head 令牌做比较并交换。

## 内部数据流

- **命令**：`parse(input, max_body)` → `scan`（词法，拒绝不支持的构造）→ 装配（取命令名、
  归约重定向、heredoc 交给 `scan_heredoc`）→ `Command`。执行器消费 `Command` 做权限预检与
  租约获取，不依赖词法内部结构。
- **解析**：调用方把固定层、帧、最近轮次、当前待办、检查点事实、预算装进 `ResolveInput` →
  `resolve()` 先 `validate()` 再四段拼装（固定层 → 帧 → 最近轮次 → 当前待办）→ `ResolvedContext`。
  不排序、不去重、不读环境。
- **压缩**：`decide(budget, used)` 判断该不该压 → `plan(history, keep)` 算折叠范围 →
  `parse_output(raw, limit)` 校验模型输出 → `facts.validate()` 校验事实层 →
  `plan(head, fold, summary, facts, ...)` 产出 `CommitPlan` → server 侧用同一令牌做比较并交换
  提交。内核全程不碰数据库。

## 契约

### 提供（给其他模块）

- `command::parse` / `parse_with_limits` / `parse_with_defaults`：命令入口。
- `command::Command` 及 `read_paths()` / `write_paths()` / `Display`：执行器消费。
- `context::resolve` 与 `ResolvedContext`：server 侧的模型调用消费。
- `context::Tree` 与 `HeadToken`：server 侧的会话推进消费。
- `context::ReadSet` 与 `StaleVerdict`：server 侧的写入前判定消费。
- `context::budget::Budget`：预算参数。
- `context::compress` 的各计划函数：server 侧的折叠协调器消费。

### 消费（从其他模块）

- **无 crate 依赖**。所有外部状态由调用方作为参数传入：固定层内容、帧、最近轮次、检查点
  事实、预算、用量、正文上限、记号上限。内核不查库、不读时钟、不读文件系统。

## 不变量

- **纯函数**：`resolve()` 与压缩计划函数都是纯函数。同样的输入两次调用必然得到相同结果。
  这是 replay 与前缀缓存命中率计算的基础。
- **节点不可变**：节点一旦写入，内容永不改变。历史永远可按节点 id 取回。回退与分叉只移动
  head 指针，不复制节点。
- **head 比较并交换**：head 更新必须带（会话, 节点, 代次）三元组。代次单调递增，回退之后旧
  令牌依然失效。
- **轮边界是用户消息**：一条用户输入开启一轮。压缩按轮数而不是节点数数，保留段必须以"用户
  要求做什么"开头。
- **覆盖与保留不重叠**：折叠范围严格止于保留轮次的起点，摘要与原文不重叠。
- **摘要与事实分治**：散文由模型写，事实由机器填。事实层不接受模型的输入。缺失败记录就不
  允许出帧。
- **压缩触发点向下取整**：宁可早一点压缩，也不要晚到压缩请求自己发不出去。
- **读集按（工作区, 路径, 会话）分键**：同一会话对同一路径的重复读取只在序号更大时覆盖。
  判定只比哈希，不比序号。
- **路径归一是纯字符串操作**：`PathKey` 只做去多余 `/`、去前导 `./`、去尾斜杠。符号链接解析、
  大小写归一、工作区根拼接由调用方在构造键之前完成。
- **零依赖**：`Cargo.toml` 的 `[dependencies]` 为空。`check_kernel_purity.sh` 用 `cargo tree`
  断言依赖树只有一行（`ita-kernel` 自己），`clippy.toml` 的禁入清单断言不读时钟、不碰文件系统、
  不起子进程或线程、不读环境变量、不写标准输出。
- **构建脚本禁止**：`check_kernel_purity.sh` 用 `cargo metadata` 断言 `ita-kernel` 没有
  `custom-build` target。构建脚本在编译期能做任意 IO，也能把依赖藏进构建图。
- **错误变体冻结**：`ParseError` 的变体在第一批就冻结，后续阶段只填空。避免拿着旧枚举到处
  补 match。
- **错误可操作**：每个 `ParseError` 变体都带字节偏移与可操作建议，模型据此重写命令的代价
  应当是一次往返。`is_retryable()` 恒为 `false`。
- **heredoc 整条拒绝**：正文里撞上分隔符时，后面的内容变成残留文本，整条拒绝，不静默截断。
  宁可整体拒绝也不写出一部分。
- **正文按字节原样保留**：不转换换行，不解释任何转义。任何换行规范化都会让写出的文件与模型
  给出的内容不一致。
- **双引号转义与 bash 对齐**：双引号内只解释 bash 也会解释的四个转义（`\"` `\\` `\$` `` \` ``），
  其余按字面保留两个字符。这样同一条命令在 ita 里与在真实 shell 里写出的字节一致。
- **heredoc 终止行逐字节相等**：大小写敏感，行尾不允许空格，带回车不算匹配。
- **预算用抽象计数不用 token**：分词属于模型层，内核依赖具体分词器会让同一份输入在不同模型
  下得到不同的解析结果。
- **压缩预留必须装进触发点与上限之间的空间**：构造期就拒绝装不下的配置，否则压缩请求会因
  自身超窗而失败。
- **帧不重叠**：任意两个帧都不能共享节点。完全包含的帧被拒，嵌套摘要永远不会被产出。
- **固定层与待办不参与折叠**：固定层在压缩时重建，待办必须原样保留。
- **提交计划末尾必须就是当前 head**：否则新帧的父节点与覆盖内容不连续，树会断。计划作废时
  重算而不是重试，因为折叠范围也是基于旧 head 算的。
