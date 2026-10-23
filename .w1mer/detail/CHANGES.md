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
- 批次 04.2 新增 `crates/ita-kernel/src/context/tree.rs`，内核对外暴露 `Tree`、`HeadToken` 与
  `TreeError`，版本推进到 0.1.14。契约层面新增 head 的比较并交换：`append` 只允许接在当前 head
  之后，`rewind` 只能到祖先，`fork` 在任意已有节点上建新会话且不动父会话的 head，分叉不复制节点。
  四条待闭合的约定：`HeadToken` 只含节点与代次、不含会话，不同会话从同一节点分叉会得到完全相同的
  令牌，一个子 agent 的令牌可以推进另一个子 agent 的 head；代次只活在内存里，重启后按零起算会与旧
  令牌重合，比较并交换的依据没有持久化载体；`rewind` 到当前 head 会照样把代次加一，一次没有状态
  变化的调用即可作废其他持有者的令牌；`ancestors` 遇到未知父节点静默截断且没有截断标记，`rewind`
  的祖先判定在部分加载时会把合法回退误报为父节点不连续。另有三处错误语义错位：`init` 用
  `UnknownSession` 表示会话已存在，`init` 用 `NotDescendantOfHead` 表示根节点带父，`append` 对无父
  节点把自身 id 填进 `parent` 字段。
- 批次 04.3 新增 `crates/ita-kernel/src/context/{budget,input}.rs`，内核对外暴露 `Budget`、
  `BudgetError`、`ResolveInput`、`CheckpointFacts` 与 `InputError`，版本推进到 0.1.15。契约层面新增
  压缩预算的构造约定：上限与保留轮数非零、触发比率取 1 到 1000 的千分比、预留小于上限且不超过触发
  点之后的余量，默认值 200000 与 850 千分比推出触发点 170000 与余量 30000。五条待闭合的约定：
  `ResolveInput.fixed_layer` 是 `Node` 而 `NodeBody::FixedLayer` 只存指纹，解析拿不到固定层正文；
  `recent` 取 `Vec<Node>` 而 0018 写 `Vec<NodeId>`，注释还留着元组说法；`checkpoint_facts` 取 `Vec`
  而非映射，重复键无校验；帧重叠只比较相邻帧首尾，完全包含与非相邻重叠漏过；`preview_bytes` 是
  字节而预算余额是抽象计数，两种单位同处一个结构且无校验。
- 批次 04.4 新增 `crates/ita-kernel/src/context/resolve.rs`，内核对外暴露 `Segment`、`MessageRole`、
  `Message`、`Drift`、`ResolvedContext` 与 `resolve`，版本推进到 0.1.16。契约层面新增四段输出的
  形状约定：固定层一条系统消息，每个帧一条摘要消息并在其检查点有事实时紧跟一条漂移说明，最近轮次
  与当前段按输入顺序展开，`estimated_usage` 为正文总字节数。四条待闭合的约定：`estimated_usage`
  是字节而 `Budget` 的上限与触发点是抽象计数，直接送入 `should_compress` 会让压缩提前到真实用量
  的三分之一；`node_to_message` 把 `Notice` 一律标为当前段、`Turn` 与工具消息一律标为最近段，
  段标签由节点类型决定而非所在段，按 `Segment` 数最近三轮会数错；`current` 里的帧与固定层节点
  被静默丢弃而不报错，`recent` 有对称校验、`current` 没有；`covers` 为空的帧通过校验并退化成
  纯摘要，`checkpoint_facts` 重复键的语义由首条命中决定而顺序未固定。
- 批次 04.6 新增 `crates/ita-kernel/src/context/read_set.rs`，内核对外暴露 `ReadSet`、
  `ReadRecord` 与 `StaleVerdict`，版本推进到 0.1.17。契约层面新增陈旧写判定的三态语义：
  `Fresh` 允许写入，`Stale` 带上调用者最后读到的哈希与当前哈希，`Unknown` 允许写入并由调用方
  在事件里标注；表的键是（路径, 会话），同一会话重复读取覆盖旧记录。四条待闭合的约定：
  `ReadSet` 没有工作区维度而 0003 与 0018 都按工作区维护，多个工作区共用一个实例会让读者列表
  互串；路径与哈希都是裸字符串，归一化既没交给资源键层也没有 `PathKey` 承接，同一文件的两种
  写法会退化成 `Unknown` 并静默允许写入；`readers` 只返回读过该路径的全部会话，交不出"记录已
  过期"的子集，且不导出遍历原语，存储层取不到全表；`ReadRecord.seq` 没有任何消费点，`record`
  也不比较序号，乱序或重放的旧记录会覆盖新记录并造成误判过期。
- 批次 05.1 新增 `crates/ita-kernel/src/context/compress/format.rs` 与模块入口，内核对外暴露
  `parse_output`、`format_instructions`、`CompressOutput`、`CompressError`、`PROMPT_VERSION`
  与三个标记常量，版本推进到 0.1.18。契约层面新增压缩输出的形状约定：整段以 `<end>` 收尾
  （允许尾部空白），恰好一个 `<prose>` 段且段外无内容，散文非空且不超过调用方给出的字节上限；
  失败即拒，不拿半截摘要替换上下文。六条待闭合的约定：输出结构在类型上确实不含事实，但
  `format_instructions` 的示例正文仍邀请模型写"否决了哪些方案"，与 0004 的事实层分工冲突；
  `prompt_version` 在解析时填编译期常量，记的是解析器版本而非生成文本所用提示版本；版本与
  提示文本无联动，一致性用例喂给解析器的是测试自拼的样例而非模板正文；标记不在末尾、大小写
  不符、标记后多一句都归入 `Truncated`，提示语因此会误报"被截断"；`is_retryable` 恒真，
  把长度类失败与随机性类失败并成一类；散文上限是调用方入参的裸字节数，0018 参数表没有这一项，
  也不受 `compress_reserve` 约束。
