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
- 批次 05.2 新增 `crates/ita-kernel/src/context/compress/trigger.rs`，内核对外暴露
  `TriggerDecision` 与 `decide`，版本推进到 0.1.19。契约层面新增压缩触发的三分支判定：
  `NotYet` 带当前用量与触发点，`Compress` 带一个额度，`CannotCompress` 带剩余空间与所需预留；
  预留不足时明确拒绝而不是硬压。三条待闭合的约定：`Compress` 的额度取自 `limit - used`，
  而 0004 要求压缩调用的输入是整个上下文，二者不是同一量；`headroom` 与预算模块的
  `remaining_after_trigger` 是同一概念的两个定义；零预留时用量越界仍返回可压缩，
  得到的额度为零而散文上限至少 4 KiB。另有一条留给后续批次：没有任何一处保证压缩后用量
  回落到触发点以下。
- 批次 05.3 新增 `crates/ita-kernel/src/context/compress/fold.rs`，内核对外暴露 `FoldPlan`
  与 `FoldError`，以及 `plan` 的别名 `plan_fold`，版本推进到 0.1.20。契约层面新增折叠范围的
  形状约定：按用户消息切轮、保留最近若干轮的起点及其后全部节点，覆盖段与保留段互补且不重叠。
  五条待闭合的约定：「轮」在 `Node::opens_turn`（以助手消息为界）与本模块（以用户消息为界）有
  两份互斥定义，前者无生产调用点；`found == keep` 靠"首个用户消息在下标零"间接返回错误，调用方
  无法区分"压完了"与"历史太短"；覆盖段无条件吞掉起点之前的固定层与待办节点，与 0018 第三节要求
  压缩时重建固定层、与待办须原样保留两条直接冲突；`FoldPlan.kept` 是单一列表，四段结构要求把它
  拆成最近轮次与当前待办两段，且 `history` 是否含旧帧未写明；`keep == 0` 分支的 `found` 硬写零，
  诊断会把非空历史报成空历史。
- 批次 05.4 新增 `crates/ita-kernel/src/context/compress/recent.rs`，并从 `compress` 模块导出
  `SlimToolResult`、`SlimError` 与 `slim`、`slim_all`、`render`，版本推进到 0.1.21。契约层面新增
  保留轮次的工具结果形状 `SlimToolResult { node, line, exit_code, body }`，渲染文案为"退出码、
  摘要、完整输出见节点"。三处待闭合：结构里没有命令字段，0004 第 43 行要求的"命令 + 一行结果 +
  编号"在单条消息里不成立，命令只能靠相邻的调用消息推断；`line` 直接取 4 KiB 预览的首行且不截断，
  无换行的大段输出省不下预算，首行为空而后续有内容时还会误报"（无输出）"；内核只产出编号，
  接收编号取回原文的工具与命令尚未在任何 crate 中定义。
- 批次 05.5 新增 `crates/ita-kernel/src/context/compress/facts.rs`，并从 `compress` 模块导出
  `Facts`、`FactsError` 与 `Failure`，版本推进到 0.1.22。契约层面新增压缩事实层的形状：
  `Facts { changed_files, failures, rejected, checkpoint, turns }` 配 `validate` 与 `render`，
  三条拒绝条件是轮数为零、有改动但无失败记录、已否决方案缺理由。四处待闭合：拒绝条件把
  "有改动"与"有失败"绑成互为前提，纯注释或一次编译即过的批次会被拒绝出帧；0004 第 20 行
  要求的实际耗时没有字段，时钟仍须由调用方传入；改动文件用三元组按位置表达增删行数；
  失败记录的 `attempt` 与 `outcome` 未做空白校验，可与"不可修改"的渲染文案一起让失败项变空。
- 批次 05.6 新增 `crates/ita-kernel/src/context/compress/commit.rs`，并从 `compress` 模块导出
  `HeadTokenLike`、`CommitPlan`、`CommitError` 与 `plan` 的别名 `plan_commit`，版本推进到 0.1.23。
  契约层面新增压缩提交计划的形状：父节点取自 head 令牌、覆盖段取自折叠计划、正文为散文与事实段
  以双换行拼接，另带固定层指纹、检查点与提示版本；顺序为先查折叠范围非空、再查事实层完整、最后
  拼正文。五处待闭合：`HeadTokenLike` 只要节点、丢了 04.2 引入的代次与会话，计划本身也不携带令牌，
  提交时的比较并交换无法确定比的是计划算出的那一次，回退后重用重新成为可能；`plan` 文档声称的
  第 3 步"校验 head 与折叠计划一致"没有实现，`CommitError::HeadMismatch` 因此不可达且没有用例；
  入参已是解析通过的 `CompressOutput`，`BadSummary` 同样不可达；`FoldPlan.kept` 未被使用，"覆盖段
  止于 head、覆盖段与保留段不重叠"这条 0004 与输入校验都留给提交阶段的检查无人承接；事实段的
  "覆盖轮数"与帧渲染的"覆盖 N 个节点"是两种口径共用一个词，帧正文还因 `Facts::render` 的末尾
  换行以空行收尾。
- 批次 06.1 新增 `crates/ita-store/{src/db.rs,src/migrate.rs,src/scope.rs}` 与
  `migrations/0001_core.sql`，存储层首次对外暴露 `Store`、`StoreError`、`WorkspaceScope`
  与 `migrate::run`，版本推进到 0.1.24；根清单新增 `rusqlite`（bundled 特性）与 `ulid`
  两个工作区依赖。契约层面新增三条形状：`Store::open` 打开即跑完迁移、`Store::scope`
  以"工作区已注册"为构造前提、迁移版本记在 `schema_version` 表而非 SQLite 的
  `user_version`。另有六条待闭合：工作区行与 `workspace_seq` 行分两条语句写，中断后
  计数器无法补写且重跑只会返回 `WorkspaceExists`；迁移脚本内容漂移没有任何检查，
  已迁移的库里改过的 SQL 不会执行而新库按新内容建表；实际没有一处使用
  `BEGIN IMMEDIATE`，`busy_timeout` 对 `SQLITE_BUSY_SNAPSHOT` 无效；`journal_mode=WAL`
  失败被静默忽略且不校验实际模式；`WorkspaceScope` 只证明工作区存在、不证明来自连接，
  `create_workspace` 与 `workspace_count` 是不带作用域的公开入口；`nodes` 的
  `workspace_id` 与 `session_id` 没有同工作区约束，`events` 的会话与节点列没有外键。
- 批次 06.2 新增 `crates/ita-store/migrations/0002_content.sql` 与集成测试 `tests/schema.rs`，
  存储层首次有了内容寻址与快照的表形状，`Store` 新增仅供测试使用的 `conn_for_test`，迁移版本
  推进到 2。契约层面新增四条形状：blob 只存哈希、尺寸与创建时间，正文在文件系统里；快照按
  （工作区, 序号）唯一，`kind` 区分基线与增量；快照条目以（快照, 路径）为主键，`kind` 取
  file、dir、tombstone，超阈值文件以 `content_stored = 0` 表达；读集以（工作区, 路径, 会话）
  为主键。审查判为 issues 的六条：整份迁移没有一个 CHECK，`content_stored` 的默认值让
  "墓碑且已存内容"成为默认路径；重定基线只有两个可空列且无约束；`snapshots.parent_id` 是无
  动作的自引用外键，squash 删除父快照会被拦住，该列也没有索引；blobs 缺宽限期起点字段；
  文件头注释仍写着引用计数；用例未覆盖条目与读集的外键以及 kind 取值域。
- 批次 06.2.1 把 0002 的"只在某种取值下才成立"的规则从注释搬进表结构：`snapshots.kind`
  与条目 `kind` 各有取值域，重定基线留痕补齐为发起人、原因、人工确认三列并要求成组出现且
  只对基线成立，条目新增跨列 CHECK 固定"file 必须有哈希与尺寸并显式给出内容标记，dir 与
  墓碑三者皆空"。`content_stored` 取消默认值改为可空，超阈值文件用 0、已存用 1、不适用用
  空，三种语义不再混用；`blobs` 补 `unreferenced_since` 作为宽限期起点。补 `snapshots(parent_id)`
  与 `read_set(session_id)` 两条索引。`parent_id` 保留无动作外键，合并事务先重挂子边再删父
  成为约定，空父边因此不会与顶层快照混淆。注释层面改正引用计数的陈述，`0001_core.sql` 的
  "一切业务表都带 workspace_id"改为默认说法并点明 `blobs` 与快照条目两处例外。代价：两份迁移
  的脚本内容都变了，按旧版本建过的库启动时会报脚本漂移（`ScriptChanged`），只能删库重建；
  这是"跨列约束只能在建表这一次定形"的直接后果，在仓库尚未对外发布时付出最小。待闭合：
  `unreferenced_since` 目前只有列与下界约束，写入时机由回收扫描决定；重定基线的"会话起点
  基线是硬地板"落实在合并事务的删除判定里，表上没有对应标记列。另一条被否掉的方案是给
  `snapshot_entries.hash` 加指向 `blobs(hash)` 的外键：超阈值条目的哈希是按内容算出的，
  但正文没有进内容存储，也就不在 `blobs` 里，外键会把这类条目误判成悬空引用。
