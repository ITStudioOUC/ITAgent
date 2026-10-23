---
id: CHANGES
title: Recent changes (architecture-impact deltas)
state: active
---

# Recent Changes

> Architecture-impact deltas recorded by reviewers during main batches.
> A periodic compact merges these into the stable `codebase/` docs and clears
> this log. One cache invalidation per compact — keep stable periods long.

## Deltas

> One bullet per delta, tagged with the target module doc:
> `- [gc] contract X moved`. Tags = module names from
> `codebase/INDEX.md`. Untagged → unassigned.
> `w1mer sync` shows a grouped preview; `w1mer sync --apply` writes + clears.

- 批次 01 落地了十三份架构决策记录与重写后的项目说明，系统骨架由此确定：内核纯逻辑与三进程隔离、
  单数据库单写者、纯函数解析与上下文树、语义压缩即折叠、两层回滚正交、内容寻址快照、受限命令
  文法、只用原生工具调用、单次调用租约、沙箱策略与保真度、工作流事件溯源、工具清单与模板、
  server 内 unit 监督。契约层面新增了 `resolve(head, policy)`、沙箱的 `probe` 与 `apply` 两个
  操作、工具模板的回调协议，以及快照与租约的元数据形状。
- 批次 02.1 建起 cargo 工作区：根清单新增 `[workspace.package]` 作为版本唯一来源，成员目前只有
  `ita-kernel`，其依赖表为空。契约层面新增两条 crate 级约束，内核禁止 unsafe，内核公开项必须有
  文档注释；版本口径定为 0.1.0，后续 crate 一律用 `.workspace = true` 继承，不再各写一份。
- 批次 02.2 落地内核纯净门：`crates/ita-kernel/clippy.toml` 逐条枚举内核禁止的 std 类型与
  方法（文件、网络、进程、线程、时钟、环境变量、标准输出），`lib.rs` 新增
  `deny(clippy::disallowed_methods)` 与 `deny(clippy::disallowed_types)` 使命中直接变成编译
  失败，新增 `.w1mer/scripts/check_kernel_purity.sh` 断言内核零普通/构建依赖且无 build.rs；
  版本推进到 0.1.1。契约层面新增两条可执行约束：内核不得出现在产物的依赖图里，内核源码不得
  引用被禁的 std 路径。
- 批次 02.3 新增 `.github/workflows/ci.yml`，按格式、clippy 加内核纯净门、构建加测试切成三个
  并行 job，clippy 与构建两步带 `--locked`，测试用 nextest 并暂时允许空测试集通过。契约层面 CI
  成为唯一强制 `-D warnings` 与锁文件校验的地方，本地的 `cargo clippy` 与 pre-commit 仍比 CI 松，
  这个差距只靠详情文档的风险表提醒，没有本地门承接。
- 批次 02.4 把工作区从单成员扩到五个成员，新增 ita-store、ita-proto、ita-sandbox、ita-exec
  四个零依赖空壳，职责按目标布局切分为存储、协议、沙箱与执行器，成员之间没有依赖边。契约层面
  新增一条待改约束：沙箱与执行器需要系统调用级实现，crate 级 unsafe 禁令应为 `deny` 而非
  `forbid`，否则 landlock 与 seccomp 无法就地实现；另外版本推进脚本必须同步全部成员的锁文件
  条目，工作区成员不再只有一个。审查时该批次尚无提交，上述内容以工作树为准。
- 批次 02.5 在工作树里把成员从五个扩到九个，新增 ita-tools、ita-model、ita-workflow、ita-server
  四个零依赖空壳，版本推出 0.1.5；本条记录时该批次尚无提交，以工作树为准。契约层面这次没有
  新增调用契约，只新增四条 crate 级约束：四者均 `deny(unsafe_code)` 并 `deny(missing_docs)`。
  依赖方向尚缺一条关键边：验收要求 server 依赖 store、proto、model、workflow，当前四者依赖表
  仍为空，目标布局里的上层到基础层的单向依赖还没建立。
- 批次 02.6 落地 `ita` 单二进制的角色分发与 ita-worker 空壳，成员增至十一个，版本推进到 0.1.6。
  `crates/ita-cli/src/role.rs` 新增 `Role` 枚举与 `parse`、`as_str`，角色只从参数解析、缺省为 master，
  不读环境变量；`crates/ita-cli/src/main.rs` 承担 `--version` 与分发，未知角色退出 2，三个已知角色
  打印"尚未实现"后退出 0。ita-worker 仅有 `pub fn run()` 占位，注释写明不得读环境变量或开数据库。
  契约层面新增两条尚未被门守住的约定：缺省角色是 master，未实现的角色退出 0；另外 CI 删除了
  `--no-tests=pass`，空测试集重新视为失败，`STACK.md` 的命令行同步去掉该参数。
- 批次 02.7 只同步文档，没有新增调用契约：`STRUCTURE.md` 的"目标布局"改为"crate 布局"，删掉
  "尚未创建"并补上十一个成员已存在、内核零依赖、ita-cli 是唯一二进制三条事实；`STACK.md` 修正
  仓库卫生一行，把 `AGENTS.md` 从未忽略改为已提交；`README.md` 补开发入口两条 cargo 命令。文档
  与仓库的偏差仍在：两处版本号写 0.1.6 而本批次推到 0.1.7，`STACK.md` 的构建命令仍是 `cargo test`。
- 批次 03.1 新建 `crates/ita-kernel/src/command/`，公开 `Command`、`Redirect`、`RedirectKind`、
  `HereDoc`、`ParseError` 五组纯类型，读写路径由重定向提取后排序去重，供执行器做租约预检；
  内核 `lib.rs` 首次对外暴露 `command` 模块，版本推进到 0.1.8。契约层面新增三条待执行器承接的
  约定：写路径的排序去重键是重定向目标字符串，尚未覆盖同一文件的不同书写形式；`read_paths`
  只反映 `<` 重定向，与 0018 第四节按会话记录读写集的来源不同；`ParseError::is_retryable`
  恒为假，语法错误因此是独立于策略拒绝与租约拒绝的第三类拒绝。`Display` 对 heredoc 只输出
  分隔符、不输出正文，可粘贴复现的承诺尚未成立。
- 批次 03.2 在 `crates/ita-kernel/src/command/lex.rs` 新增纯函数 `scan`，对外暴露 `Token`、`TokenKind`
  与 `scan`，词法层成为 `parse` 的唯一入口，版本推进到 0.1.9。契约层面新增三条尚未闭合的偏差：
  双引号内自行解释 `\n` `\t` `\r`，与 bash 的双引号规则不同，粘贴执行的字节会变；通配符只拒了
  `*` 与 `?`，字符类、花括号展开、变量引用与波浪号按字面通过，与 0007 的"通配符没有例外"冲突；
  行中单个回车被当作命令结束，其后文本静默丢弃。未闭合引号报的是引号起点而非词起点。
- 批次 03.3 在 `crates/ita-kernel/src/command/heredoc.rs` 新增纯函数 `scan_heredoc`，对外再暴露
  `DEFAULT_MAX_BODY` 与 `MAX_DELIMITER_LEN` 两个常量，版本推进到 0.1.10。契约层面新增四条尚未
  闭合的偏差：`scan_heredoc` 只返回 `HereDoc`、不返回分隔符之后被消费的长度，同一行上紧跟分隔符的
  其余文本（例如 `<<'EOF' > out`）被静默丢弃，03.4 的装配层拿不到它、也无法续接；`TrailingText`
  的偏移漏加了分隔符本身的长度（三个分量都相对 `after_delimiter`），`InvalidDelimiter` 的偏移相对
  `rest`，两套基准并存且函数文档未写基准；正文含 `$` 或反引号时借用 `InvalidDelimiter` 并以偏移 0
  报错，诊断首句会把它说成分隔符不合规则；未加引号的分隔符只拦了 `$` 与反引号，漏了 `\` 接 `\`、
  `$`、反引号与换行四种引用组合，正文含反斜杠时与真实 bash 交付的字节不同。另新增一个 0018 第九节
  参数表里没有的阈值：分隔符长度上限 64 字节，以常量形式硬编码。
- 审查 R_01_1 记录了三类未闭合点，尚未有对应模块文档可以挂靠，等执行器与存储模块建成后分别
  并入：执行器的进程归属在 0001 与 0013 之间不一致；工作流节点的独占资源与单次调用租约的生命
  周期不一致；沙箱策略的来源在工具清单与节点权限范围之间没有合成规则。超阈值文件在对齐算法里
  的删除与保留也有两份相反的表述。
- 批次 03.4 在 `crates/ita-kernel/src/command/parse.rs` 新增纯函数 `parse` 与 `parse_with_defaults`，
  对外再暴露 `MAX_TOKENS`，装配层成为词法的唯一上层，版本推进到 0.1.11。契约层面新增三条待闭合
  的偏差：`ConflictingHereDoc` 在装配层已不可达，两个 heredoc 与 `< `后置的 heredoc 实际由
  `scan_heredoc` 的残留检测以 `TrailingText` 报出，同一条结构规则有两种错误类型；重定向目标允许
  空字符串，`write_paths()` 会产出空串，资源键层拼接后指向工作区根，且 `Redirect::target` 文档
  声称的"去掉多余与末尾斜杠"在装配层没有实现；`MAX_TOKENS` 在词法整条扫完、每个词各持一份
  `String` 之后才检查，不构成内存边界，同时它以常量而非配置项出现，与 0018 第九节"全部参数走
  配置结构"冲突。`parse` 的错误偏移存在两套基准：装配层自身用输入绝对偏移，交给 `scan_heredoc`
  的片段用 `<<` 之后的相对偏移，入口函数文档未声明基准。
- 批次 03.5 新增集成测试 `crates/ita-kernel/tests/command_matrix.rs`，在公共 `parse()` 上把拒绝路径
  收敛为三十二条表驱动用例，另加十二条合法命令做对照，版本推进到 0.1.12。本次没有新增调用契约，
  只把既有契约固定成可执行判据：错误类别、提示关键词与 `is_retryable` 恒假三条同时断言。契约层面
  留下两处缺口与一处弱断言：ADR 0007 要求的 CRLF 终止行在入口层没有用例，`<<"X"` 与未加引号的
  干净正文这两种明示接受的写法没有进入合法对照表；`<<`、`&`、`分隔符`、`无法解析的文本` 四个提示
  关键词对同类型错误恒真，因而只能证明"有提示"，不能证明"提示说对了"。
- 批次 04.1 新增 `crates/ita-kernel/src/context/{ids,node,mod}.rs`，内核首次对外暴露 `context` 模块，
  版本推进到 0.1.13。契约层面新增 `NodeId`、`SessionId`、`WorkspaceId`、`CheckpointId` 四个名义
  标识符（26 字符 Crockford base32，拒 `I/L/O/U`），以及 `Node { id, parent, session, body }` 与
  `Frame { summary, covers, checkpoint, fixed_layer_fingerprint }` 两个不可变结构。四条待闭合的约定：
  `NodeBody::Turn` 只有文本、没有消息方向与轮次边界，而 0003 的四段结构要求区分最近三轮；`ToolCall`
  把调用与结果压进一个节点，与原生工具调用两条消息的形状不一致；工具结果只有预览、没有内容寻址
  句柄，与"大对象外置"不变量冲突；`Node.session` 与跨会话父边的组合语义未写明，分叉共享祖先的
  实现方式留给 04.2 自行决定。`Frame.fixed_layer_fingerprint` 记录生成摘要时刻的固定层，但 0018
  第一节的 `ResolveInput` 不接收它，漂移表也只有两个布尔量，至今没有消费点。
