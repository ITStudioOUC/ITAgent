# ITAgent — Roadmap

> Section order in the file = priority order; `list` and `re-rank` follow file order.
> Within a section, open rows are ranked by `Defer` (desc), then ID.
> `Defer` counts how many times the task was skipped (`w1mer defer <id>`).
> Each completed task: mark `[x]` + date + commit, add a one-line measured
> effect summary and `done`; full data goes to the results archive.
> Insert new items with derived sub-ids (`05.1`, `05.1.2`); never batch-renumber.

## ID rules

- Unbounded rolling hierarchical IDs: `01`, `05`, `05.1`, `05.1.1`, ...
- Children branch deeper on demand (sub-batch fixes, review follow-ups).
- Ordering: pre-order traversal (parent first, children right behind).
- Cross-references stay stable; derived ids link review/results/bug docs.

## Task granularity

Rows at this level are **epics** (weeks of work, one clear deliverable each).
Each epic is decomposed into `NN.M` sub-tasks before dispatch; the breakdown,
acceptance criteria and file boundaries live in the epic's `Doc` column.
A sub-task is one batch: one acceptance behaviour, one verification pass, and no
unrelated changes mixed in. File and line counts are not a criterion — what makes
a change reviewable is that it has a single purpose.

Sub-task breakdowns for epics past the current frontier are **provisional**.
The Explorer re-checks the epic's plan against the code as it exists when that
epic is reached; disproved sub-tasks are rewritten or deleted rather than
forced. Do not add top-level rows for work that fits inside an existing epic.

## Status states

`todo` `done` `reviewed` `issue`

## 一、Foundation

Foundation tasks are hard prerequisites: everything after them codes against the
contracts they freeze. Nothing else opens before 01 and 02 are done.

- **01** freezes the design in `docs/adr/` ADRs (including rejected
  alternatives) so later batches never re-litigate settled questions.
- **02** creates the workspace shape (`ita-kernel` pure layer first) so the
  separation is enforced by the compiler from day one.

| # | Task | Doc | Status | Defer | Effect |
|---|------|-----|--------|-------|--------|
<!-- w1mer:task:foundation -->
| 01 | 设计决策固化：docs/adr 架构决策记录全套 | — | done | 0 | 十三份架构决策记录落在 docs/adr，覆盖内核边界、单库单写者、上下文树与解析、压缩、两层回滚、内容寻址快照、受限文法、原生工具调用、租约分层、沙箱保真度、工作流事件溯源、工具信任链、unit 监督；README 已按决策重写 |
| 01.1 | 审查架构决策记录的内部一致性与可执行性 | — | done | 0 | 六条决策矛盾全部由新 ADR 0014 至 0018 关闭，探针字段与参数表补齐，README 与不变量表同步修订 |
| 02 | 仓库骨架：cargo workspace + ita 单二进制 + CI | — | done | 0 | 七个批次完成：工作区、纯净门、CI 三门、两层空壳、单二进制分发、文档对齐；版本由 0.1.0 推进至 0.1.7 |
| 02.1 | 根清单与内核空壳 | .w1mer/detail/01_plan-02.md | done | 0 | 工作区单成员、内核零依赖、版本 0.1.0 由 workspace.package 唯一来源；review 判 ok，纯净门的可执行方案已实测并归档 |
| 02.2 | 内核纯净门与依赖断言 | .w1mer/detail/01_plan-02.md | done | 0 | 纯净门落地：crate 内 clippy 配置 83 行、内核两条 deny、脚本断言零普通依赖且无 build.rs；注入 Instant::now 实测 clippy 退出 101、脚本退出 1，还原后通过；版本 0.1.1 |
| 02.2.1 | 补审纯净门并核对 CI 方案 | — | done | 0 | 四处缺口关闭：dev 依赖与构建脚本不再有豁免通道，补八个 std 入口，注释改为与实测一致；三项验证与纯净门全绿 |
| 02.3 | CI 流水线五项门 | .w1mer/detail/01_plan-02.md | done | 0 | 三个并行 job 落地（格式、clippy 与纯净门、构建与测试），五道门与本地三件套一致；审查关闭两处文档问题并补上一处流程缺陷（版本推进须同步锁文件），版本 0.1.3 |
| 02.4 | 基础层空壳与依赖方向 | .w1mer/detail/01_plan-02.md | done | 0 | 四个基础层 crate 空壳落地且互不依赖，内核依赖仍为空；审查抓出三处将导致返工的缺陷（forbid 挡 FFI、版本推进只刷一条锁记录、纯净门检查越界）并已修复，版本 0.1.4 |
| 02.5 | 上层空壳与依赖方向 | .w1mer/detail/01_plan-02.md | done | 0 | 九个成员齐备，server 依赖 store/proto/model/workflow，内核仍是零依赖；版本 0.1.5 |
| 02.6 | 单二进制角色分发与 worker 空壳 | .w1mer/detail/01_plan-02.md | done | 0 | ita 二进制可按角色分发，版本来自清单，三个角色占位退出 0、未知角色退出 2；九项测试全绿，CI 的空测试集豁免已按标注删除；版本 0.1.6 |
| 02.7 | 收口：文档同步与骨架验收 | .w1mer/detail/01_plan-02.md | done | 0 | 十一个 crate 与目录一一对应，STACK 的五条命令与 CI 逐条一致且都带锁，STRUCTURE 增加状态段区分目标与现状；版本 0.1.7，骨架史诗完成 |











## 二、Kernel

Pure logic only (`ita-kernel`): no IO, no async, no DB. This is where the
"restrained core" claim is either true or false.

- **03** lexer: argv + pipe + redirect + heredoc `<<` (single command, quoted
  delimiter preferred), full-body-then-write, failure matrix as tests.
- **04** context: node tree (DAG, explicit head), `resolve()` as a pure function,
  frame weak refs, three-state drift notice, read-set tracking.
- **05** semantic compression: transactional head switch, keep last 3 turns,
  machine-filled facts + model-written prose, compression call logged.

| # | Task | Doc | Status | Defer | Effect |
|---|------|-----|--------|-------|--------|
<!-- w1mer:task:kernel -->
| 03 | 受限 bash 文法：Command AST + lexer + heredoc + 失败矩阵 | — | done | 0 | 五个批次完成：语法树、词法、heredoc、装配、失败矩阵；75 项内核测试与 5 项入口矩阵全绿，命令文法可直接被执行器消费 |
| 03.1 | Command AST 类型与目标提取 | .w1mer/detail/02_plan-03.md | done | 0 | 命令语法树与错误类型冻结：读路径只取自输入重定向、写路径取自两种输出重定向并排序去重，Display 连 heredoc 正文一起输出因此可粘贴复现；13 项测试全绿，版本 0.1.8 |
| 03.2 | 词法扫描：词、引号与算符 | .w1mer/detail/02_plan-03.md | done | 0 | 词法层落地：词、单双引号、转义（与真实 shell 一致）、四种算符，拒绝命令替换、变量与括号展开、通配符、逻辑连接、管道、herestring、剥制表符；34 项测试全绿，版本 0.1.9 |
| 03.3 | heredoc 状态机与十项失败 | .w1mer/detail/02_plan-03.md | done | 0 | heredoc 状态机落地：分隔符校验（字符集、长度、首字符）、逐行精确匹配、残留检测、正文上限、未引号正文的展开字符检查；十类失败全部有检测，54 项测试全绿，版本 0.1.10 |
| 03.4 | 语法装配：单命令与重定向 | .w1mer/detail/02_plan-03.md | done | 0 | 语法装配落地：命令名、参数、三种重定向、单个 heredoc；结构约束（两个 heredoc、与输入重定向同现、空目标、记号上限）全部前移到装配前检查；75 项测试全绿，版本 0.1.11 |
| 03.5 | 失败矩阵总表与诊断 | .w1mer/detail/02_plan-03.md | done | 0 | 失败矩阵在公共入口上跑 33 条拒绝用例与 15 条合法对照，覆盖十类失败与全部越界构造；每条拒绝都断言错误类别、可操作关键词、不可重试、带偏移；版本 0.1.12 |
| 04 | 上下文模型：Node/Frame/resolve/三态 notice/读集 | — | done | 0 | 六个子任务完成（漂移通知并入解析）：标识符与节点、上下文树、解析输入与预算、四段解析、读集与陈旧写；160 项测试全绿 |
| 04.1 | Node、Frame 与 id 类型 | .w1mer/detail/03_plan-04.md | done | 0 | 四类名义标识符与节点、帧、内容句柄定型；补上消息方向、轮次边界、工具调用与结果分离、待办变体、跨会话父边语义；96 项测试全绿，版本 0.1.13 |
| 04.2 | 上下文树：head、分叉与 CAS | .w1mer/detail/03_plan-04.md | done | 0 | 上下文树落地：节点不可变、head 带会话与代次做比较并交换、分叉共享祖先不复制、回退仅限祖先、祖先链不完整即报错；112 项测试全绿，版本 0.1.14 |
| 04.3 | ResolveInput 与预算结构 | .w1mer/detail/03_plan-04.md | done | 0 | 解析输入与预算定型：固定层带正文、帧不重叠、检查点事实必须补齐、压缩预留与 85% 触发点的相容性在构造期校验；129 项测试全绿，版本 0.1.15 |
| 04.4 | 四段解析与逐字节复现 | .w1mer/detail/03_plan-04.md | done | 0 | 四段解析落地：固定层、帧、最近、当前按序展开，四种漂移各有措辞且不可达格不报方向，无检查点的帧显式说明；145 项测试全绿，版本 0.1.16 |
| 04.5 | 漂移四组合通知 | .w1mer/detail/03_plan-04.md | done | 0 | 并入 04.4：Drift 的四组合与四种措辞已随解析落地，含不可达格不报方向与四种措辞互异的用例 |
| 04.6 | 读集记录与陈旧写判定 | .w1mer/detail/03_plan-04.md | done | 0 | 读集落地：按工作区、路径键、会话三层建模，纯字符串归一化，乱序记录不覆盖，能只把通知发给视图过期的会话；160 项测试全绿，版本 0.1.17 |
| 05 | 语义压缩：事务性切换 + 保留 3 轮 + 事实层 | — | done | 0 | 六个子任务完成：格式契约、触发判定、折叠范围、正文瘦身、事实层、提交计划；222 项测试全绿 |
| 05.1 | 压缩输出格式与解析 | .w1mer/detail/04_plan-05.md | done | 0 | 压缩输出契约落地：散文段与结束标记严格校验，事实层不接收模型输入，截断/超长与随机性错误分成两类重试判据；174 项测试全绿，版本 0.1.18 |
| 05.2 | 触发阈值与预留预算 | .w1mer/detail/04_plan-05.md | done | 0 | 压缩触发落地：该压与能压分开判定，剩余空间不足时明确拒绝而不是硬压，零剩余空间一律不可压；181 项测试全绿，版本 0.1.19 |
| 05.3 | 轮边界与折叠范围 | .w1mer/detail/04_plan-05.md | done | 0 | 轮边界统一为用户消息并复用 opens_turn，覆盖范围止于保留起点，固定层与待办不参与折叠，区分历史太短与压无可压；191 项测试全绿，版本 0.1.20 |
| 05.4 | 保留轮次工具正文瘦身 | .w1mer/detail/04_plan-05.md | done | 0 | 工具结果瘦身为命令加一行结果加节点编号，单行上限 512 字节且按字符边界截断，首条非空行，解析侧同步收窄；203 项测试全绿，版本 0.1.21 |
| 05.5 | 事实层组装与必填校验 | .w1mer/detail/04_plan-05.md | done | 0 | 事实层落地：改动文件、失败尝试、已否决方案、检查点、轮数、耗时六类事实，失败小节永远输出，尝试与结果两半必填；214 项测试全绿，版本 0.1.22 |
| 05.6 | 提交计划与 head 比较并交换 | .w1mer/detail/04_plan-05.md | done | 0 | 提交计划落地：折叠末尾必须等于当前 head，计划带代次以复用 04.2 的 ABA 防护，散文在前事实在后，事实不完整即不出计划；222 项测试全绿，版本 0.1.23 |



















## 三、Store & Server

Single DB, server is the only writer, workers are stateless and DB-blind.

- **06** schema + CAS: `blobs` global content-addressed store, `snapshots` /
  `snapshot_entries` (with tombstones), baseline + squash, mark-sweep GC with grace.
- **07** units: dependency topology (`Requires`/`Wants`/`After`), `ready` vs
  `running`, required units gate the CLI, the rest load asynchronously.
- **08** server I/O: `Invocation` envelope (caller, session, cwd, argv, stdin,
  lease ids), two message classes (authoritative-sync vs telemetry), group commit.

| # | Task | Doc | Status | Defer | Effect |
|---|------|-----|--------|-------|--------|
<!-- w1mer:task:server -->
| 06 | 存储层：单 DB schema + CAS + 快照 squash + GC | — | active | 0 |
| 06.1 | 存储打开、作用域与迁移运行器 | .w1mer/detail/05_plan-06.md | done | 0 | 存储层可打开：立即事务与忙等超时、WAL 实测校验、迁移带脚本哈希与旧库检测、工作区注册原子化、作用域只能由已存在工作区构造；245 项测试全绿，版本 0.1.24 |
| 06.2 | 初始 schema：核心表与快照表 | .w1mer/detail/05_plan-06.md | done | 0 | 初始表结构落地：四张内容表与工作区外键、快照序号唯一；约束在补审批次定形，259 项测试全绿，版本 0.1.26 |
| 06.2.1 | 处置初始表结构审查的八条问题 | .w1mer/detail/05_plan-06.md | done | 0 | 八条处置全部落地：两张表的 kind 取值域、条目形态互斥 CHECK、重定基线留痕三列、两条索引、宽限期起点列、注释修订、squash 重挂子边的约定；净增九条拒绝用例，259 项测试全绿，版本 0.1.26 |
| 06.2.1.1 | 补齐条目形态约束并修正文档矛盾 | .w1mer/detail/05_plan-06.md | done | 0 | 条目形态约束补齐：目录与墓碑各自单支，目录保留权限位与修改时间而尺寸判空，墓碑五列全空，文件尺寸非负；重定基线留痕拒绝空串；kind 用例说明改为由形态约束兜底；detail/20 与外键裁决的相反结论就地标注 |
| 06.2.1.1.1 | 文件条目在未存内容时必须带修改时间 | .w1mer/detail/05_plan-06.md | done | 0 | 未存内容的文件条目必须带修改时间已写进表结构：条目形态约束的 file 支新增 content_stored = 1 或 mtime 非空的条件，超阈值条目缺修改时间被拒而正文已存的条目不要求；新增一条用例守这两档并含防过度收紧的回归断言，261 项测试全绿，版本 0.1.28 |
| 06.2.2 | 测试用例命名英文化与命名规范入册 | .w1mer/detail/20_测试函数名英文化与初始表结构约束定形的执行计划.md | done | 0 | 命名规范入册并完成第一批改名：注释规范新增第六节，规定测试名用英文 snake_case 且上方加一行中文行为说明，含四条判据的正反例对照表；命令文法六个文件八十个用例改名并补说明，测试总数保持二百六十一项不变。其余三组模块的一百六十九个用例转到 06.2.3。 |
| 06.2.3 | 其余三组模块的测试用例改名 | .w1mer/detail/20_测试函数名英文化与初始表结构约束定形的执行计划.md | todo | 0 | |
| 06.3 | 事件追加、seq 分配与预览截断 | .w1mer/detail/05_plan-06.md | todo | 0 | |
| 06.4 | 帧写入与 head 比较并交换 | .w1mer/detail/05_plan-06.md | todo | 0 | |
| 06.5 | CAS 写入、去重与原子改名 | .w1mer/detail/05_plan-06.md | todo | 0 | |
| 06.6 | 快照清单：文件、墓碑与超阈值 | .w1mer/detail/05_plan-06.md | todo | 0 | |
| 06.7 | 基线加增量 squash 与重定基线 | .w1mer/detail/05_plan-06.md | todo | 0 | |
| 06.8 | 回滚对齐：写回、删除与残留清单 | .w1mer/detail/05_plan-06.md | todo | 0 | |
| 06.9 | 标记清除 GC：快照根、租约根与宽限期 | .w1mer/detail/05_plan-06.md | todo | 0 | |
| 07 | unit 监督者：依赖拓扑 + ready 定义 + 渐进加载 | — | active | 0 |
| 07.1 | 拓扑类型与依赖无环校验 | .w1mer/detail/06_plan-07.md | todo | 0 | |
| 07.2 | 状态机、就绪探针与停止顺序 | .w1mer/detail/06_plan-07.md | todo | 0 | |
| 07.3 | 必需与非必需启动门与降级 | .w1mer/detail/06_plan-07.md | todo | 0 | |
| 07.4 | 能力集快照与变更推送 | .w1mer/detail/06_plan-07.md | todo | 0 | |
| 08 | server 写路径与 IPC：Invocation 信封 + 两类消息 + group commit | — | active | 0 |
| 08.1 | Invocation 信封与版本协商 | .w1mer/detail/07_plan-08.md | todo | 0 | |
| 08.2 | 本地 socket 与连接级工作区绑定 | .w1mer/detail/07_plan-08.md | todo | 0 | |
| 08.3 | 权威与遥测分流、有界队列与背压 | .w1mer/detail/07_plan-08.md | todo | 0 | |
| 08.4 | group commit：5 毫秒或 256 条 | .w1mer/detail/07_plan-08.md | todo | 0 | |
| 08.5 | 批量读 load_context 单次往返 | .w1mer/detail/07_plan-08.md | todo | 0 | |

























## 四、Execution & Sandbox

The worker talks only to the executor; the executor consults the server for
leases and delegates to the sandbox. Nested calls (bash → tool) inherit the
outer lease and never acquire new paths.

- **09** lease: one tool call, atomic multi-path acquisition (all-or-nothing,
  sorted), TTL + heartbeat, stale-write hash check, cancellation to pgid,
  snapshot only after reap.
- **10** sandbox: `Policy` (declarative, cross-platform expressible) vs
  `Fidelity` (honest capability matrix, deny-by-default on missing dimensions).
- **11** tools: AUR-style manifest, `ita-tool-shim` ABI, install-time approval
  bound to (name, version, content hash, platform), permission diff on upgrade.

| # | Task | Doc | Status | Defer | Effect |
|---|------|-----|--------|-------|--------|
<!-- w1mer:task:execution -->
| 09 | 执行器：单次租约 + 多路径原子获取 + 取消传播 | — | active | 0 |
| 09.1 | 租约类型与资源键归一 | .w1mer/detail/08_plan-09.md | todo | 0 | |
| 09.2 | 多路径原子获取与 TTL 落库 | .w1mer/detail/08_plan-09.md | todo | 0 | |
| 09.3 | 租约 RPC 与执行器客户端 | .w1mer/detail/08_plan-09.md | todo | 0 | |
| 09.4 | 陈旧写哈希校验与读集 | .w1mer/detail/08_plan-09.md | todo | 0 | |
| 09.5 | 调用边界与执行器骨架 | .w1mer/detail/08_plan-09.md | todo | 0 | |
| 09.6 | 进程组与 subreaper 回收范围 | .w1mer/detail/08_plan-09.md | todo | 0 | |
| 09.7 | 取消传播到整组 | .w1mer/detail/08_plan-09.md | todo | 0 | |
| 09.8 | 逐层回收与超时强杀 | .w1mer/detail/08_plan-09.md | todo | 0 | |
| 09.9 | 回收后快照与实际写入差集 | .w1mer/detail/08_plan-09.md | todo | 0 | |
| 09.10 | 嵌套调用继承外层租约 | .w1mer/detail/08_plan-09.md | todo | 0 | |
| 09.11 | 声明越界审计与严格模式 | .w1mer/detail/08_plan-09.md | todo | 0 | |
| 10 | Linux 沙箱：landlock/seccomp + Policy/Fidelity 能力矩阵 | — | active | 0 |
| 10.1 | Policy 与 Fidelity 内核模型 | .w1mer/detail/09_plan-10.md | todo | 0 | |
| 10.2 | 策略交集与来源标注 | .w1mer/detail/09_plan-10.md | todo | 0 | |
| 10.3 | 策略拒绝与租约拒绝分型 | .w1mer/detail/09_plan-10.md | todo | 0 | |
| 10.4 | Sandbox trait 与能力探针 | .w1mer/detail/09_plan-10.md | todo | 0 | |
| 10.5 | 路径真实解析与穿越防护 | .w1mer/detail/09_plan-10.md | todo | 0 | |
| 10.6 | Linux 能力探针 | .w1mer/detail/09_plan-10.md | todo | 0 | |
| 10.7 | Landlock 文件规则 | .w1mer/detail/09_plan-10.md | todo | 0 | |
| 10.8 | Seccomp 过滤器 | .w1mer/detail/09_plan-10.md | todo | 0 | |
| 10.9 | 端口级网络与域名意图 | .w1mer/detail/09_plan-10.md | todo | 0 | |
| 10.10 | 环境清洗与资源上限 | .w1mer/detail/09_plan-10.md | todo | 0 | |
| 10.11 | 沙箱监督进程与失败语义 | .w1mer/detail/09_plan-10.md | todo | 0 | |
| 10.12 | Linux 逃逸负向测试 | .w1mer/detail/09_plan-10.md | todo | 0 | |
| 11 | 工具系统：manifest + shim ABI + 包管理器 + 首次批准 | — | active | 0 |
| 11.1 | 清单 schema 与解析 | .w1mer/detail/10_plan-11.md | todo | 0 | |
| 11.2 | 清单规范哈希与平台绑定 | .w1mer/detail/10_plan-11.md | todo | 0 | |
| 11.3 | Shim ABI 版本化编解码 | .w1mer/detail/10_plan-11.md | todo | 0 | |
| 11.4 | 最小 shim 模板二进制 | .w1mer/detail/10_plan-11.md | todo | 0 | |
| 11.5 | 执行器侧 shim 回调宿主 | .w1mer/detail/10_plan-11.md | todo | 0 | |
| 11.6 | 工具输出契约与截断 | .w1mer/detail/10_plan-11.md | todo | 0 | |
| 11.7 | 工具注册表与 catalog 事件 | .w1mer/detail/10_plan-11.md | todo | 0 | |
| 11.8 | 首次批准四元组绑定 | .w1mer/detail/10_plan-11.md | todo | 0 | |
| 11.9 | 构建沙箱与源白名单 | .w1mer/detail/10_plan-11.md | todo | 0 | |
| 11.10 | 安装流水线与原子落位 | .w1mer/detail/10_plan-11.md | todo | 0 | |
| 11.11 | 升级权限差异批准 | .w1mer/detail/10_plan-11.md | todo | 0 | |
| 11.12 | 受限试运行与访问观测 | .w1mer/detail/10_plan-11.md | todo | 0 | |
| 11.13 | 双栏权限差距报告 | .w1mer/detail/10_plan-11.md | todo | 0 | |
| 11.14 | Fidelity 可用性门 | .w1mer/detail/10_plan-11.md | todo | 0 | |







































## 五、Intelligence

- **12** models: OpenAI + Anthropic, single static `{command: string}` tool schema,
  native tool calling only, thinking blocks passed through opaque, usage logged,
  all requests proxied by the server (workers and sandboxes never reach the network).
- **13** tool retrieval (optional, default off): BM25 v1 → rerank → selector as a
  hint only, output frozen into the event log, feedback signal recorded.

| # | Task | Doc | Status | Defer | Effect |
|---|------|-----|--------|-------|--------|
<!-- w1mer:task:intelligence -->
| 12 | 模型层：双 provider 适配 + native codec + usage 账本 + server 代理 | — | active | 0 |
| 12.1 | 归一化消息与工具定义类型 | .w1mer/detail/11_plan-12.md | todo | 0 | |
| 12.2 | Anthropic 非流式编解码 | .w1mer/detail/11_plan-12.md | todo | 0 | |
| 12.3 | OpenAI 非流式编解码 | .w1mer/detail/11_plan-12.md | todo | 0 | |
| 12.4 | SSE 流式分帧与增量聚合 | .w1mer/detail/11_plan-12.md | todo | 0 | |
| 12.5 | thinking 块不透明回传 | .w1mer/detail/11_plan-12.md | todo | 0 | |
| 12.6 | usage 落事件表与账本查询 | .w1mer/detail/11_plan-12.md | todo | 0 | |
| 12.7 | server 模型代理与非流式转发 | .w1mer/detail/11_plan-12.md | todo | 0 | |
| 12.8 | 流式经 IPC 转发与取消传播 | .w1mer/detail/11_plan-12.md | todo | 0 | |
| 12.9 | 单工具调用回合状态机 | .w1mer/detail/11_plan-12.md | todo | 0 | |
| 13 | 工具检索（可选）：BM25→rerank→selector + 反馈落库 | — | active | 0 |
| 13.1 | 检索开关与工具目录索引 | .w1mer/detail/12_plan-13.md | todo | 0 | |
| 13.2 | BM25 打分与 top-k 提示消息 | .w1mer/detail/12_plan-13.md | todo | 0 | |
| 13.3 | 检索结果冻结进事件日志 | .w1mer/detail/12_plan-13.md | todo | 0 | |
| 13.4 | rerank 接口与恒等实现 | .w1mer/detail/12_plan-13.md | todo | 0 | |
| 13.5 | selector 与候选收缩 | .w1mer/detail/12_plan-13.md | todo | 0 | |
| 13.6 | 反馈信号落库与命中率统计 | .w1mer/detail/12_plan-13.md | todo | 0 | |

















## 六、Orchestration

DSL is optional for v1, but subagent semantics must be reserved in the kernel now
(sessions fork, results merge by explicit append, never auto-merge).

- **14** workflow: JSON schema, node completion recorded as events (state derived,
  signals are only latency optimization), static write-set conflict lint at load
  and pull time.
- **15** subagent: two inheritance modes (compressed brief / inherit-context +
  tail instruction), fork-not-append, depth and budget caps.
- **16** node scheduling: declared write-set resource classes (files/trees/ports/db),
  concurrency allowed until first write, declared-vs-actual audit.

| # | Task | Doc | Status | Defer | Effect |
|---|------|-----|--------|-------|--------|
<!-- w1mer:task:orchestration -->
| 14 | 工作流 DSL：JSON schema + 事件溯源状态 + 静态冲突 lint | — | active | 0 |
| 14.1 | 工作流 JSON 类型与拒未知字段 | .w1mer/detail/13_plan-14.md | todo | 0 | |
| 14.2 | 资源键归一与粒度冲突 | .w1mer/detail/13_plan-14.md | todo | 0 | |
| 14.3 | 依赖环与读写悬挂的载入期拒绝 | .w1mer/detail/13_plan-14.md | todo | 0 | |
| 14.4 | 静态检查判定表与三档动作 | .w1mer/detail/13_plan-14.md | todo | 0 | |
| 14.5 | 节点完成事件与状态推导重放 | .w1mer/detail/13_plan-14.md | todo | 0 | |
| 14.6 | 完成记录的事务提交与幂等去重 | .w1mer/detail/13_plan-14.md | todo | 0 | |
| 14.7 | 信号仅作唤醒且可丢失 | .w1mer/detail/13_plan-14.md | todo | 0 | |
| 14.8 | 内置函数白名单与声明式参数 | .w1mer/detail/13_plan-14.md | todo | 0 | |
| 15 | subagent：两种继承模式 + 分叉不合并 + 深度预算 | — | active | 0 |
| 15.1 | 内核分叉原语：子会话共享祖先 | .w1mer/detail/14_plan-15.md | todo | 0 | |
| 15.2 | 子会话派生元数据与预算列 | .w1mer/detail/14_plan-15.md | todo | 0 | |
| 15.3 | 深度与预算上限的纯函数准入 | .w1mer/detail/14_plan-15.md | todo | 0 | |
| 15.4 | 简报模式的上下文组装 | .w1mer/detail/14_plan-15.md | todo | 0 | |
| 15.5 | 继承模式：父前缀加一条尾指令 | .w1mer/detail/14_plan-15.md | todo | 0 | |
| 15.6 | 子结果摘要由父显式合回 | .w1mer/detail/14_plan-15.md | todo | 0 | |
| 15.7 | 写集与独占声明按交集继承 | .w1mer/detail/14_plan-15.md | todo | 0 | |
| 15.8 | 子会话用量记账与预算截停 | .w1mer/detail/14_plan-15.md | todo | 0 | |
| 16 | 节点调度：独占声明 + 读写分离 + 声明/实际审计 | — | active | 0 |
| 16.1 | 资源类模型与跨工作区资源键 | .w1mer/detail/15_plan-16.md | todo | 0 | |
| 16.2 | 独占门纯状态机与先进先出排队 | .w1mer/detail/15_plan-16.md | todo | 0 | |
| 16.3 | 门表持久化与心跳续期回收 | .w1mer/detail/15_plan-16.md | todo | 0 | |
| 16.4 | 启动门接入节点调度 | .w1mer/detail/15_plan-16.md | todo | 0 | |
| 16.5 | 首次写租约排队而非立即拒绝 | .w1mer/detail/15_plan-16.md | todo | 0 | |
| 16.6 | 声明与实际写入偏差审计 | .w1mer/detail/15_plan-16.md | todo | 0 | |
| 16.7 | 偏差汇总查询与按节点视图 | .w1mer/detail/15_plan-16.md | todo | 0 | |

























## 七、Quality & Ops

Only local, zero-cost measurement. No paid eval runs: real sessions are the dataset.

- **17** instrumentation: per-turn latency breakdown, cold-start-to-ready, tool path
  (executor→sandbox→shim→reap→snapshot), IPC RTT; usage fields are free from the
  provider and must be logged.
- **18** export/import: workspace bundle (metadata rows + referenced CAS blobs +
  snapshot manifests) — replaces "copy the directory" now that the DB is shared.

| # | Task | Doc | Status | Defer | Effect |
|---|------|-----|--------|-------|--------|
<!-- w1mer:task:quality -->
| 17 | 埋点与基准：turn 延迟分解 + 冷启动 ready + 工具路径 | — | active | 0 |
| 17.1 | 注入式时钟与 span 原语 | .w1mer/detail/16_plan-17.md | todo | 0 | |
| 17.2 | 一次轮次的五阶段延迟分解 | .w1mer/detail/16_plan-17.md | todo | 0 | |
| 17.3 | usage 四类 token 入事件表 | .w1mer/detail/16_plan-17.md | todo | 0 | |
| 17.4 | IPC 往返基准与百分位 | .w1mer/detail/16_plan-17.md | todo | 0 | |
| 17.5 | 冷启动到就绪的时间分解 | .w1mer/detail/16_plan-17.md | todo | 0 | |
| 17.6 | 工具调用路径五段分解 | .w1mer/detail/16_plan-17.md | todo | 0 | |
| 17.7 | 快照耗时随规模变化曲线 | .w1mer/detail/16_plan-17.md | todo | 0 | |
| 18 | 工作区 export/import：搬移、备份与 replay | — | active | 0 |
| 18.1 | bundle 目录布局与 manifest | .w1mer/detail/17_plan-18.md | todo | 0 | |
| 18.2 | 按作用域导出元数据行 | .w1mer/detail/17_plan-18.md | todo | 0 | |
| 18.3 | 被引用 CAS 内容打包 | .w1mer/detail/17_plan-18.md | todo | 0 | |
| 18.4 | 导入预检与坏包拒绝 | .w1mer/detail/17_plan-18.md | todo | 0 | |
| 18.5 | 导入落库与存储层回读 | .w1mer/detail/17_plan-18.md | todo | 0 | |
| 18.6 | 导出与导入子命令往返 | .w1mer/detail/17_plan-18.md | todo | 0 | |















## 八、Backlog

(Items awaiting re-prioritization)

- 19 remote transport (WS/HTTP, cursor resume) — waits for the local IPC envelope
  to prove stable; topology (a) already fixed.
- 20 Postgres adapter — waits for real load data; operation-level `Store` trait only.
- 21 TUI — owner designs the frontend personally; CLI only until then.
- 22 event archival — only when a workspace exceeds 200 MB events or `load_context`
  exceeds 20 ms. `EventRange` trait is the seam left open; nothing else now.

<!-- w1mer:task:backlog -->
| 19 | 远程传输：WS/HTTP + 事件游标续传（拓扑 a） | — | todo | 0 |
| 20 | Postgres 适配层：操作级 Store trait | — | todo | 0 |
| 21 | TUI：owner 亲自设计前端 | — | todo | 0 |
| 22 | 事件归档：200MB / 20ms 判据触发后再做 | — | todo | 0 |
